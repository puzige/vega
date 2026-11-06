use super::*;
use crate::write_coordination::{
    self, ObservationScope, PersistenceCategory, WriteActivity, WriteCategory,
};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const THREAD: &str = "01M6PDYTPK83CNB2WENXDAYYE4";
const RUN: &str = "01M6PDYTPK83CNB2WENXDAYYE5";

struct Fixture {
    root: PathBuf,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("persistence-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).expect("root");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).expect("mode");
        let root = root.canonicalize().expect("canonical root");
        let path = root.join("vega.db");
        let store = crate::Store::open(&path).expect("store");
        store.migrate().expect("migrate");
        Self { root, path }
    }
    fn connect(&self) -> Connection {
        let conn = Connection::open(&self.path).expect("connect");
        conn.busy_timeout(Duration::from_millis(25))
            .expect("timeout");
        conn
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn failure() -> PersistenceFailure {
    PersistenceFailure {
        thread_id: THREAD.into(),
        run_id: RUN.into(),
        message_id: RUN.into(),
        occurred_at: 10,
        category: PersistenceCategory::ResponseStarted,
        elapsed_ms: 30,
        sqlite_extended_code: Some(5),
        observations: Vec::new(),
    }
}

#[test]
fn engine_contention_is_saved_without_database_write_and_survives_reopen() {
    let fixture = Fixture::new();
    let blocker = fixture.connect();
    blocker
        .execute_batch("BEGIN IMMEDIATE")
        .expect("external writer");
    let conn = fixture.connect();
    let scope = ObservationScope::begin(&conn);
    let started = Instant::now();
    let error = match write_coordination::WriteTransaction::new(&conn, WriteCategory::RuntimeEvent)
    {
        Ok(_) => panic!("writer must fail"),
        Err(error) => error,
    };
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    write_coordination::record_persistence(
        &conn,
        PersistenceCategory::ResponseStarted,
        started,
        false,
        Some(5),
    );
    let mut record = failure();
    record.observations = scope.observations();
    super::record(&conn, record.clone()).expect("sidecar while writer held");
    drop(scope);
    drop(conn);
    let conn = fixture.connect();
    let records = read_by_run(&conn, THREAD, RUN).expect("reopen");
    assert_eq!(records, vec![record]);
    let json = serde_json::to_value(&records[0]).expect("json");
    let observations = json["observations"].as_array().expect("observations");
    assert!(
        observations
            .iter()
            .any(|item| item["operation"] == "begin" && item["succeeded"] == false)
    );
    assert!(
        !observations
            .iter()
            .any(|item| item["operation"] == "queue_timeout")
    );
    let exported: serde_json::Value = serde_json::from_str(
        &crate::run_diagnostics::export_run(&conn, THREAD, RUN)
            .expect("export with no db diagnostics"),
    )
    .expect("export json");
    assert_eq!(exported["schema_version"], 3);
    assert_eq!(
        exported["persistence_failures"]
            .as_array()
            .expect("failures")
            .len(),
        1
    );
    assert_eq!(exported["persistence_failures_unavailable"], false);
    blocker.execute_batch("ROLLBACK").expect("release");
}

#[test]
fn malformed_or_untrusted_files_are_rejected_without_target_changes() {
    for case in 0..6 {
        let fixture = Fixture::new();
        let conn = fixture.connect();
        record(&conn, failure()).expect("record");
        let directory = fixture.root.join("persistence-failures");
        let path = directory.join("00.json");
        let target = fixture.root.join("target");
        std::fs::write(&target, b"unchanged").expect("target");
        match case {
            0 => {
                std::fs::remove_file(&path).expect("remove");
                symlink(&target, &path).expect("symlink");
            }
            1 => {
                std::fs::remove_file(&path).expect("remove");
                std::fs::hard_link(&target, &path).expect("hard link");
            }
            2 => std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                .expect("broad mode"),
            3 => std::fs::write(&path, b"{\"secret\":\"not accepted\"}").expect("invalid"),
            4 => {
                OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .expect("open")
                    .set_len(MAX_BYTES + 1)
                    .expect("large");
            }
            _ => std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
                .expect("directory mode"),
        }
        assert!(read_by_run(&conn, THREAD, RUN).is_err(), "case {case}");
        assert_eq!(std::fs::read(&target).expect("target read"), b"unchanged");
    }
}

#[test]
fn symlink_directory_is_not_followed_and_invalid_identifiers_are_not_persisted() {
    let fixture = Fixture::new();
    let conn = fixture.connect();
    let target = fixture.root.join("target");
    std::fs::create_dir(&target).expect("target");
    symlink(&target, fixture.root.join("persistence-failures")).expect("link");
    assert!(record(&conn, failure()).is_err());
    assert_eq!(std::fs::read_dir(&target).expect("target dir").count(), 0);
    let fixture = Fixture::new();
    let conn = fixture.connect();
    let mut record = failure();
    record.run_id = "credential-or-content".into();
    assert!(super::record(&conn, record).is_err());
    assert!(!fixture.root.join("persistence-failures").exists());
}

#[test]
fn concurrent_failures_stay_bounded_and_scopes_do_not_mix_commands() {
    let fixture = Fixture::new();
    std::thread::scope(|scope| {
        for worker in 0..4 {
            let path = fixture.path.clone();
            scope.spawn(move || {
                let conn = Connection::open(path).expect("worker");
                for sequence in 0..20 {
                    let mut event = failure();
                    event.occurred_at = worker * 20 + sequence;
                    record(&conn, event).expect("record concurrent");
                }
            });
        }
    });
    assert_eq!(
        std::fs::read_dir(fixture.root.join("persistence-failures"))
            .expect("dir")
            .count(),
        CAPACITY + 1
    );
    let conn = fixture.connect();
    assert_eq!(
        read_by_run(&conn, THREAD, RUN).expect("read").len(),
        CAPACITY
    );
    let first = ObservationScope::begin(&conn);
    let permit = write_coordination::acquire(&conn, WriteCategory::Permissions).expect("permit");
    drop(permit);
    assert!(!first.observations().is_empty());
    drop(first);
    let second = ObservationScope::begin(&conn);
    assert!(second.observations().is_empty());
    let _identity =
        write_coordination::ExecutionScope::begin(THREAD, RUN, WriteActivity::Persistence);
    drop(second);
}

fn worker(path: &Path, action: &str, sequence: u64) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        .arg("--exact")
        .arg("persistence_failures::development_tests::subprocess_writer_entry")
        .arg("--nocapture")
        .env("VEGA_DIAGNOSTIC_TEST_DATABASE", path)
        .env("VEGA_DIAGNOSTIC_TEST_ACTION", action)
        .env("VEGA_DIAGNOSTIC_TEST_SEQUENCE", sequence.to_string());
    command
}

#[test]
fn subprocess_writer_entry() {
    let Some(path) = std::env::var_os("VEGA_DIAGNOSTIC_TEST_DATABASE") else {
        return;
    };
    let conn = Connection::open(path).expect("child connection");
    let sequence: u64 = std::env::var("VEGA_DIAGNOSTIC_TEST_SEQUENCE")
        .expect("sequence")
        .parse()
        .expect("number");
    if std::env::var("VEGA_DIAGNOSTIC_TEST_ACTION").expect("action") == "crash" {
        let (directory, _) = directory(&conn, true).expect("directory");
        let _lock = lock_directory(&directory).expect("lock");
        write_temporary(&directory, b"interrupted failure snapshot").expect("staging");
        std::process::exit(73);
    }
    let mut event = failure();
    event.occurred_at = sequence;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match record(&conn, event.clone()) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "diagnostic lock not released");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("record failed: {error}"),
        }
    }
    std::process::exit(74);
}

#[test]
fn fresh_processes_preserve_history_and_independent_writers_do_not_collide() {
    let fixture = Fixture::new();
    for sequence in 0..3 {
        assert!(
            worker(&fixture.path, "record", sequence)
                .status()
                .expect("child")
                .code()
                == Some(74)
        );
    }
    let mut children: Vec<_> = (3..9)
        .map(|sequence| {
            worker(&fixture.path, "record", sequence)
                .spawn()
                .expect("spawn")
        })
        .collect();
    for child in &mut children {
        assert_eq!(child.wait().expect("join").code(), Some(74));
    }
    let conn = fixture.connect();
    let records = read_by_run(&conn, THREAD, RUN).expect("read");
    assert_eq!(
        records
            .iter()
            .map(|item| item.occurred_at)
            .collect::<Vec<_>>(),
        (0..9).collect::<Vec<_>>()
    );
    for sequence in 9..66 {
        let mut event = failure();
        event.occurred_at = sequence;
        record(&conn, event).expect("fill capacity");
    }
    let records = read_by_run(&conn, THREAD, RUN).expect("bounded records");
    assert_eq!(records.len(), CAPACITY);
    assert!(records.iter().any(|item| item.occurred_at == 65));
    assert_eq!(
        records
            .iter()
            .map(|item| item.occurred_at)
            .collect::<Vec<_>>(),
        (2..66).collect::<Vec<_>>()
    );
}

#[test]
fn interrupted_writes_reuse_one_staging_file_and_recover_after_restart() {
    let fixture = Fixture::new();
    for sequence in 0..(CAPACITY + 2) {
        assert_eq!(
            worker(&fixture.path, "crash", sequence as u64)
                .status()
                .expect("crash child")
                .code(),
            Some(73)
        );
        let entries: Vec<_> = std::fs::read_dir(fixture.root.join("persistence-failures"))
            .expect("directory")
            .map(|entry| entry.expect("entry"))
            .collect();
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| entry.metadata().expect("metadata").len() <= MAX_BYTES)
        );
    }
    assert!(
        worker(&fixture.path, "record", 80)
            .status()
            .expect("recover")
            .code()
            == Some(74)
    );
    let conn = fixture.connect();
    assert_eq!(read_by_run(&conn, THREAD, RUN).expect("read").len(), 1);
    assert!(!fixture.root.join("persistence-failures/.pending").exists());
}

#[test]
fn unsafe_abandoned_staging_is_rejected_without_modifying_targets() {
    for hard_link in [false, true] {
        let fixture = Fixture::new();
        let conn = fixture.connect();
        let (directory, _) = directory(&conn, true).expect("directory");
        drop(directory);
        let target = fixture.root.join("target");
        std::fs::write(&target, b"unchanged").expect("target");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).expect("mode");
        let pending = fixture.root.join("persistence-failures/.pending");
        if hard_link {
            std::fs::hard_link(&target, &pending).expect("hard link");
        } else {
            symlink(&target, &pending).expect("symlink");
        }
        assert!(record(&conn, failure()).is_err());
        assert_eq!(std::fs::read(&target).expect("target"), b"unchanged");
    }
}
