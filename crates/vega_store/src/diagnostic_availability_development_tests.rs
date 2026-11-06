use super::*;
use crate::write_coordination::{self, ObservationScope, WriteCategory, WriteTransaction};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::time::Duration;

struct Fixture {
    root: PathBuf,
    store: crate::Store,
    thread: String,
    run: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("availability-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = root.canonicalize().unwrap();
        let store = crate::Store::open(root.join("vega.db")).unwrap();
        store.migrate().unwrap();
        Self {
            root,
            store,
            thread: ulid::Ulid::generate().to_string(),
            run: ulid::Ulid::generate().to_string(),
        }
    }

    fn failure(&self) -> PersistenceFailure {
        PersistenceFailure {
            thread_id: self.thread.clone(),
            run_id: self.run.clone(),
            message_id: self.run.clone(),
            occurred_at: 1,
            category: PersistenceCategory::ResponseStarted,
            elapsed_ms: 25,
            sqlite_extended_code: Some(5),
            observations: Vec::new(),
        }
    }

    fn sidecar(&self) -> PathBuf {
        self.root.join("persistence-failures")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn missing_readable_and_failed_are_distinct_without_export_creating_files() {
    let fixture = Fixture::new();
    let report = read_report(fixture.store.conn(), &fixture.thread, &fixture.run);
    assert_eq!(report.state, ReadState::Missing);
    assert_eq!(report.fault.unwrap().stage, FailureStage::DirectoryOpen);
    assert!(!fixture.sidecar().exists());
    record(&fixture.store, fixture.failure()).unwrap();
    let report = read_report(fixture.store.conn(), &fixture.thread, "unmatched");
    assert_eq!(report.state, ReadState::Readable);
    assert!(report.failures.is_empty());
    assert!(report.fault.is_none());
    std::fs::remove_file(fixture.sidecar().join(".lock")).unwrap();
    let report = read_report(fixture.store.conn(), &fixture.thread, &fixture.run);
    assert_eq!(report.state, ReadState::Failed);
    assert_eq!(report.fault.unwrap().stage, FailureStage::LockOpen);
    assert!(!fixture.sidecar().join(".lock").exists());
}

#[test]
fn unsafe_destination_failure_is_exportable_without_database_events_and_isolated_by_identity() {
    let fixture = Fixture::new();
    let target = fixture.root.join("private-marker-destination");
    std::fs::create_dir(&target).unwrap();
    symlink(&target, fixture.sidecar()).unwrap();
    let fault = record(&fixture.store, fixture.failure()).unwrap_err();
    assert_eq!(fault.stage, FailureStage::DirectoryOpen);
    let encoded =
        crate::run_diagnostics::export_run(&fixture.store, &fixture.thread, &fixture.run).unwrap();
    let export: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(export["schema_version"], 4);
    assert_eq!(export["persistence_failures_state"], "failed");
    assert_eq!(export["persistence_failures_unavailable"], true);
    assert_eq!(
        export["current_process_save_failures"][0]["fault"]["stage"],
        "directory_open"
    );
    assert_eq!(
        export["current_process_save_failures"][0]["failure"]["sqlite_extended_code"],
        5
    );
    assert!(export["events"].as_array().unwrap().is_empty());
    assert!(!encoded.contains("private-marker"));
    assert!(!encoded.contains(fixture.root.to_str().unwrap()));
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    let reopened = crate::Store::open(fixture.root.join("vega.db")).unwrap();
    assert_eq!(
        process_save_failures(&reopened, &fixture.thread, &fixture.run).len(),
        1
    );
    assert!(process_save_failures(&reopened, &fixture.thread, "other").is_empty());
    let other = Fixture::new();
    assert!(process_save_failures(&other.store, &fixture.thread, &fixture.run).is_empty());
}

#[test]
fn validation_and_open_faults_are_precise_and_do_not_modify_unsafe_files() {
    let fixture = Fixture::new();
    record(&fixture.store, fixture.failure()).unwrap();
    let pending = fixture.sidecar().join(".pending");
    let marker = fixture.root.join("secret-marker");
    std::fs::write(&marker, b"do-not-change").unwrap();
    symlink(&marker, &pending).unwrap();
    let fault = record(&fixture.store, fixture.failure()).unwrap_err();
    assert_eq!(fault.stage, FailureStage::TemporaryOpen);
    assert_eq!(std::fs::read(&marker).unwrap(), b"do-not-change");
    std::fs::remove_file(&pending).unwrap();
    std::fs::write(&pending, b"unsafe-mode").unwrap();
    std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o644)).unwrap();
    let fault = record(&fixture.store, fixture.failure()).unwrap_err();
    assert_eq!(fault.stage, FailureStage::TemporaryValidation);
    assert_eq!(fault.io_kind, "invalid_data");
    assert_eq!(fault.os_code, None);
    std::fs::remove_file(&pending).unwrap();
    let lock = fixture.sidecar().join(".lock");
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        record(&fixture.store, fixture.failure()).unwrap_err().stage,
        FailureStage::LockValidation
    );
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600)).unwrap();
    let slot = fixture.sidecar().join("00.json");
    std::fs::set_permissions(&slot, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        record(&fixture.store, fixture.failure()).unwrap_err().stage,
        FailureStage::SlotSelection
    );
    assert_eq!(
        read_report(fixture.store.conn(), &fixture.thread, &fixture.run)
            .fault
            .unwrap()
            .stage,
        FailureStage::SlotValidation
    );
    std::fs::set_permissions(&slot, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&slot, b"not-json-private-marker").unwrap();
    let report = read_report(fixture.store.conn(), &fixture.thread, &fixture.run);
    assert_eq!(report.fault.unwrap().stage, FailureStage::SlotDecode);
    assert!(
        !serde_json::to_string(&report.fault)
            .unwrap()
            .contains("private-marker")
    );
    std::fs::set_permissions(fixture.sidecar(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        record(&fixture.store, fixture.failure()).unwrap_err().stage,
        FailureStage::DirectoryValidation
    );
}

#[test]
fn nonblocking_sidecar_lock_contention_is_classified_without_wait_or_fallback() {
    let fixture = Fixture::new();
    record(&fixture.store, fixture.failure()).unwrap();
    let (directory, _) = directory(fixture.store.conn(), false).unwrap();
    let held = lock_directory(&directory, false).unwrap();
    let started = std::time::Instant::now();
    let fault = record(&fixture.store, fixture.failure()).unwrap_err();
    assert_eq!(fault.stage, FailureStage::LockAcquire);
    assert_eq!(fault.io_kind, "would_block");
    assert!(fault.os_code.is_some());
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(std::fs::read_dir(fixture.sidecar()).unwrap().count(), 2);
    drop(held);
    assert_eq!(
        read_report(fixture.store.conn(), &fixture.thread, &fixture.run).state,
        ReadState::Readable
    );
}

#[test]
fn failure_receipts_and_disk_slots_remain_bounded_and_success_does_not_add_receipts() {
    let fixture = Fixture::new();
    for index in 0..70 {
        let mut failure = fixture.failure();
        failure.occurred_at = index;
        record(&fixture.store, failure).unwrap();
    }
    let saved = read_by_run(fixture.store.conn(), &fixture.thread, &fixture.run).unwrap();
    assert_eq!(saved.len(), CAPACITY);
    assert_eq!(saved.last().unwrap().occurred_at, 69);
    assert!(process_save_failures(&fixture.store, &fixture.thread, &fixture.run).is_empty());
    assert_eq!(
        std::fs::read_dir(fixture.sidecar()).unwrap().count(),
        CAPACITY + 1
    );
    std::fs::set_permissions(fixture.sidecar(), std::fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..70 {
        assert!(record(&fixture.store, fixture.failure()).is_err());
    }
    assert!(process_save_failures(&fixture.store, &fixture.thread, &fixture.run).len() <= CAPACITY);
    assert_eq!(SAVE_FAILURES.lock().unwrap().len(), CAPACITY);
}

#[test]
fn actual_engine_and_queue_timeouts_retain_distinct_observations_when_sidecar_cannot_save() {
    for queue in [false, true] {
        let fixture = Fixture::new();
        let path = fixture.root.join("vega.db");
        let waiter =
            crate::Store::open_with_busy_timeout(&path, Duration::from_millis(30)).unwrap();
        let target = fixture.root.join("untouched");
        std::fs::create_dir(&target).unwrap();
        symlink(&target, fixture.sidecar()).unwrap();
        let (ready, waiting) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let thread = fixture.thread.clone();
        let run = fixture.run.clone();
        let blocker = std::thread::spawn(move || {
            let store = crate::Store::open(&path).unwrap();
            let _activity = write_coordination::ExecutionScope::begin(
                &thread,
                &run,
                write_coordination::WriteActivity::Preparation,
            );
            let permit = if queue {
                Some(
                    write_coordination::acquire(store.conn(), WriteCategory::RuntimeEvent).unwrap(),
                )
            } else {
                None
            };
            store.conn().execute_batch("BEGIN IMMEDIATE").unwrap();
            ready.send(()).unwrap();
            released.recv().unwrap();
            store.conn().execute_batch("ROLLBACK").unwrap();
            drop(permit);
        });
        waiting.recv().unwrap();
        let scope = ObservationScope::begin(waiter.conn());
        let error = match WriteTransaction::new(waiter.conn(), WriteCategory::RuntimeEvent) {
            Ok(_) => panic!("writer unexpectedly acquired"),
            Err(error) => error,
        };
        assert_eq!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy)
        );
        let mut failure = fixture.failure();
        failure.observations = scope.observations();
        assert!(record(&waiter, failure).is_err());
        release.send(()).unwrap();
        blocker.join().unwrap();
        let receipts = process_save_failures(&fixture.store, &fixture.thread, &fixture.run);
        let observation = serde_json::to_value(&receipts[0].failure.observations).unwrap();
        let events = observation.as_array().unwrap();
        if queue {
            let event = events
                .iter()
                .find(|event| event["operation"] == "queue_timeout")
                .unwrap();
            assert_eq!(event["blocking_owner"]["execution"]["run_id"], fixture.run);
        } else {
            let event = events
                .iter()
                .find(|event| event["operation"] == "begin" && event["succeeded"] == false)
                .unwrap();
            assert!(event["blocking_owner"].is_null());
        }
        assert_eq!(std::fs::read_dir(target).unwrap().count(), 0);
    }
}

#[test]
fn fault_conversion_keeps_only_stage_kind_and_os_code() {
    for stage in [
        FailureStage::DirectoryCreate,
        FailureStage::TemporaryWrite,
        FailureStage::Publish,
        FailureStage::DirectorySync,
        FailureStage::SlotRead,
    ] {
        let fault = at::<()>(
            stage,
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "private-path-and-secret",
            )),
        )
        .unwrap_err();
        assert_eq!(fault.stage, stage);
        assert_eq!(fault.io_kind, "permission_denied");
        assert_eq!(fault.os_code, None);
        assert!(!serde_json::to_string(&fault).unwrap().contains("secret"));
        assert!(!fault.to_string().contains("private"));
    }
}
