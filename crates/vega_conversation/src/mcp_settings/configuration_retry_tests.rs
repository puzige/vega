use super::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use vega_store::run_diagnostics::*;

static TEST_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    root: PathBuf,
    service: McpServerSettingsService,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("configuration-retry-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let database = root.join("synthetic.db");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        std::fs::write(root.join("config.yaml"), "schema_version: 1\n").unwrap();
        let service = McpServerSettingsService::new(database, root.clone())
            .with_configuration_cancel(CancellationToken::new());
        service.reconcile_declarations().unwrap();
        Self { root, service }
    }

    fn holder(&self) -> LockHolder {
        let mut child = Command::new("/usr/bin/python3")
            .arg("-c")
            .arg("import fcntl,sys\nf=open(sys.argv[1], 'r+')\nfcntl.flock(f, fcntl.LOCK_EX)\nprint('locked', flush=True)\nsys.stdin.readline()\n")
            .arg(self.root.join(".config.lock"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim(), "locked");
        LockHolder(child)
    }

    fn store(&self) -> Store {
        Store::open(&self.service.database_path).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

struct LockHolder(Child);
impl Drop for LockHolder {
    fn drop(&mut self) {
        if let Some(mut input) = self.0.stdin.take() {
            input.write_all(b"release\n").unwrap();
        }
        assert!(self.0.wait().unwrap().success());
    }
}

fn wait_until_retry(operation: &McpServerSettingsService) {
    let start = Instant::now();
    loop {
        if operation
            .configuration_budget
            .as_ref()
            .unwrap()
            .try_lock()
            .is_ok_and(|budget| budget.attempts > 0)
        {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn uncontended_credentials_read_does_not_spend_retry_budget() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let operation = fixture.service.configuration_operation();
    assert!(
        operation
            .current_owner_credential_values()
            .unwrap()
            .is_empty()
    );
    let budget = operation
        .configuration_budget
        .as_ref()
        .unwrap()
        .lock()
        .unwrap();
    assert_eq!(budget.attempts, 0);
    assert_eq!(budget.wait_ms, 0);
    assert!(budget.started.is_none());
}

#[test]
fn external_lock_release_reloads_latest_configuration_without_invalidating_epoch() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    fixture
        .service
        .lock()
        .unwrap()
        .epochs
        .insert("existing".into(), 7);
    let operation = fixture.service.configuration_operation();
    let holder = fixture.holder();
    let worker = operation.clone();
    let join = std::thread::spawn(move || worker.current_owner_credential_values());
    wait_until_retry(&operation);
    assert!(fixture.service.registry.try_lock().is_ok());
    let store = fixture.store();
    let transaction = store.conn().unchecked_transaction().unwrap();
    transaction
        .execute("UPDATE mcp_config_state SET generation=generation+1", [])
        .unwrap();
    transaction.commit().unwrap();
    std::fs::write(fixture.root.join("config.yaml"), "schema_version: 1\nmcp_servers:\n- id: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n  display_name: Synthetic\n  enabled: false\n  transport:\n    type: remote\n    endpoint: https://example.com/mcp\n    allow_loopback_http: false\n    authorization:\n      type: none\n").unwrap();
    drop(holder);
    assert!(join.join().unwrap().unwrap().is_empty());
    let rows = mcp_servers::list(store.conn()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    assert!(!rows[0].enabled);
    assert_eq!(fixture.service.lock().unwrap().epochs["existing"], 7);
}

#[test]
fn persistent_lock_exhausts_shared_budget_and_preserves_diagnostics() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    fixture
        .service
        .lock()
        .unwrap()
        .epochs
        .insert("existing".into(), 3);
    let operation = fixture.service.configuration_operation();
    let holder = fixture.holder();
    let start = Instant::now();
    let error = operation.current_owner_credential_values().unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(error.configuration_lock, Some("file"));
    let attempts = error.configuration_lock_attempts.unwrap();
    let waited = error.configuration_lock_wait_ms.unwrap();
    assert!((1..=6).contains(&attempts));
    assert!(waited <= 310);
    assert!(
        operation
            .configuration_budget
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .started
            .unwrap()
            .elapsed()
            >= Duration::from_millis(500)
            || attempts == 6
            || operation
                .configuration_budget
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .started
                .unwrap()
                .elapsed()
                + Duration::from_millis(10 << (attempts - 1))
                > Duration::from_millis(500)
    );
    assert_eq!(fixture.service.lock().unwrap().epochs["existing"], 4);
    let second = operation.current_owner_credential_values().unwrap_err();
    assert_eq!(second.configuration_lock_wait_ms, Some(waited));
    assert_eq!(
        second.configuration_lock_attempts,
        Some((attempts + 1).min(6))
    );
    let store = fixture.store();
    store.conn().execute_batch("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES ('synthetic-project','/synthetic','Synthetic',1,1); INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES ('synthetic-thread','synthetic-project','Synthetic','synthetic-model',1,1)").unwrap();
    let runtime_error: vega_runtime::VegaError = error.into();
    let event = NewDiagnosticEvent {
        thread_id: "synthetic-thread".into(),
        run_id: "synthetic-run".into(),
        attempt_id: "synthetic-attempt".into(),
        parent_attempt_id: None,
        tool_call_id: None,
        phase: DiagnosticPhase::Run,
        state: DiagnosticState::Failed,
        failure_code: Some(DiagnosticFailureCode::UnknownSafeFailure),
        occurred_at: 1,
        duration_ms: Some(310),
        metrics: DiagnosticMetrics {
            failure_source: Some("runtime".into()),
            failure_kind: Some("credential_check".into()),
            credential_stage: Some("mcp".into()),
            credential_reason: Some("configuration".into()),
            configuration_lock: runtime_error
                .diagnostic_metrics()
                .configuration_lock
                .map(str::to_owned),
            configuration_lock_attempts: runtime_error
                .diagnostic_metrics()
                .configuration_lock_attempts,
            configuration_lock_wait_ms: runtime_error
                .diagnostic_metrics()
                .configuration_lock_wait_ms,
            ..DiagnosticMetrics::default()
        },
    };
    let mut started = event.clone();
    started.state = DiagnosticState::Started;
    started.failure_code = None;
    started.duration_ms = None;
    started.metrics = DiagnosticMetrics::default();
    insert(store.conn(), &started).unwrap();
    insert(store.conn(), &event).unwrap();
    let run = read_by_run(store.conn(), "synthetic-thread", "synthetic-run").unwrap();
    assert_eq!(run.len(), 2);
    assert_eq!(run[0].event, started);
    assert_eq!(run[1].event, event);
    let thread = read_by_thread(store.conn(), "synthetic-thread").unwrap();
    assert_eq!(thread.len(), 2);
    assert_eq!(thread[0].event, started);
    assert_eq!(thread[1].event, event);
    let preparation = crate::submission_diagnostics::runtime_failure(
        crate::submission_diagnostics::PreparationStage::Launch,
        &runtime_error,
    );
    let submission = vega_store::submission_diagnostics::PreparationEvent {
        submission_id: ulid::Ulid::generate().to_string(),
        attempt: 1,
        state: vega_store::submission_diagnostics::PreparationState::Failed,
        occurred_at: 1,
        failure: Some(preparation),
    };
    vega_store::submission_diagnostics::append(store.conn(), &submission).unwrap();
    let exported =
        vega_store::submission_diagnostics::read(store.conn(), &submission.submission_id).unwrap();
    let value: serde_json::Value = serde_json::from_str(&exported[0]).unwrap();
    assert_eq!(value["failure"]["configuration_lock"], "file");
    assert_eq!(value["failure"]["configuration_lock_attempts"], attempts);
    assert_eq!(value["failure"]["configuration_lock_wait_ms"], waited);
    assert!(matches!(
        crate::types::RunFailureKind::from_runtime(&runtime_error),
        crate::types::RunFailureKind::ConfigurationBusy
    ));
    drop(holder);
}

#[test]
fn retry_budget_enforces_full_sequence_and_independent_deadline_without_reset() {
    use vega_store::yaml_config::ConfigurationLockBusy;
    let start = Instant::now();
    let mut now = start;
    let mut budget = ConfigurationRetryBudget::default();
    for millis in [10, 20, 40, 80, 160] {
        let delay = budget.next_delay(ConfigurationLockBusy::File, now).unwrap();
        assert_eq!(delay, Duration::from_millis(millis));
        now += delay;
    }
    let exhausted = budget
        .next_delay(ConfigurationLockBusy::Edit, now)
        .unwrap_err();
    assert_eq!(exhausted.configuration_lock_attempts, Some(6));
    assert_eq!(exhausted.configuration_lock_wait_ms, Some(310));
    assert_eq!(
        budget
            .next_delay(ConfigurationLockBusy::Registry, now)
            .unwrap_err()
            .configuration_lock_wait_ms,
        Some(310)
    );
    let mut deadline = ConfigurationRetryBudget::default();
    assert_eq!(
        deadline
            .next_delay(ConfigurationLockBusy::File, start)
            .unwrap(),
        Duration::from_millis(10)
    );
    let elapsed = deadline
        .next_delay(
            ConfigurationLockBusy::File,
            start + Duration::from_millis(500),
        )
        .unwrap_err();
    assert_eq!(elapsed.configuration_lock_attempts, Some(2));
    assert_eq!(elapsed.configuration_lock_wait_ms, Some(10));
    assert_eq!(
        deadline
            .next_delay(
                ConfigurationLockBusy::File,
                start + Duration::from_millis(501)
            )
            .unwrap_err()
            .configuration_lock_wait_ms,
        Some(10)
    );
    let mut insufficient = ConfigurationRetryBudget::default();
    insufficient
        .next_delay(ConfigurationLockBusy::File, start)
        .unwrap();
    let error = insufficient
        .next_delay(
            ConfigurationLockBusy::File,
            start + Duration::from_millis(490),
        )
        .unwrap_err();
    assert_eq!(error.configuration_lock_attempts, Some(2));
    assert_eq!(error.configuration_lock_wait_ms, Some(10));
}

#[test]
fn cancellation_stops_wait_without_revoking_unrelated_authority_or_late_work() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    fixture
        .service
        .lock()
        .unwrap()
        .epochs
        .insert("existing".into(), 9);
    let cancel = CancellationToken::new();
    let operation = fixture
        .service
        .clone()
        .with_configuration_cancel(cancel.clone())
        .configuration_operation();
    let holder = fixture.holder();
    let worker = operation.clone();
    let join = std::thread::spawn(move || worker.current_owner_credential_values());
    wait_until_retry(&operation);
    cancel.cancel();
    let start = Instant::now();
    let error = join.join().unwrap().unwrap_err();
    assert!(start.elapsed() < Duration::from_millis(150));
    assert_eq!(error.reason, CredentialReadReason::State);
    assert_eq!(fixture.service.lock().unwrap().epochs["existing"], 9);
    drop(holder);
    assert!(operation.current_owner_credential_values().is_err());
    assert!(
        !fixture
            .service
            .configuration_invalidation
            .load(Ordering::SeqCst)
    );
}

#[test]
fn malformed_configuration_and_unsafe_lock_file_do_not_retry() {
    let _serial = TEST_LOCK.lock().unwrap();
    for invalid_lock in [false, true] {
        let fixture = Fixture::new();
        if invalid_lock {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                fixture.root.join(".config.lock"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
        } else {
            std::fs::write(fixture.root.join("config.yaml"), "schema_version: [\n").unwrap();
        }
        let operation = fixture.service.configuration_operation();
        let error = operation.current_owner_credential_values().unwrap_err();
        assert_eq!(
            error.storage.unwrap().io_kind,
            Some(vega_runtime::RuntimeIoKind::InvalidData)
        );
        assert_eq!(
            error.reason,
            if invalid_lock {
                CredentialReadReason::ConfigurationLockValidation
            } else {
                CredentialReadReason::ConfigurationParse
            }
        );
        assert!(error.configuration_lock.is_none());
        assert_eq!(
            operation
                .configuration_budget
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .attempts,
            0
        );
    }
}

#[test]
fn configuration_compare_and_swap_conflict_is_not_lock_contention() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let edit = vega_store::yaml_config::begin(&fixture.root.join("config.yaml")).unwrap();
    std::fs::write(
        fixture.root.join("config.yaml"),
        "schema_version: 1\nui: {}\n",
    )
    .unwrap();
    let error = edit.check_current().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    assert!(vega_store::yaml_config::lock_busy(&error).is_none());
    assert!(matches!(
        vega_store::mcp_config::io_error(error),
        McpStoreError::ConfigurationIo(std::io::ErrorKind::WouldBlock)
    ));
}

#[test]
fn registry_contention_finishes_with_pending_invalidation_before_next_authority_use() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut registry = fixture.service.registry.lock().unwrap();
    registry.epochs.insert("existing".into(), 2);
    let operation = fixture.service.configuration_operation();
    let worker = operation.clone();
    let join = std::thread::spawn(move || worker.current_owner_credential_values());
    let error = join.join().unwrap().unwrap_err();
    assert_eq!(error.configuration_lock, Some("registry"));
    assert_eq!(error.configuration_lock_attempts, Some(6));
    assert!(
        fixture
            .service
            .configuration_invalidation
            .load(Ordering::SeqCst)
    );
    drop(registry);
    assert_eq!(fixture.service.lock().unwrap().epochs["existing"], 3);
    assert!(
        !fixture
            .service
            .configuration_invalidation
            .load(Ordering::SeqCst)
    );
}

#[test]
fn readiness_retries_only_lock_acquisition_before_empty_tool_projection() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let holder = fixture.holder();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        drop(holder);
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let ready = runtime.block_on(fixture.service.ready_for_run()).unwrap();
    release.join().unwrap();
    assert!(ready.ready_servers.is_empty());
    assert!(ready.unavailable.is_empty());
}

#[test]
fn invalid_mcp_declaration_is_not_retried_and_explicit_repair_reads_fresh_configuration() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.join("config.yaml"),
        "schema_version: 1\nmcp_servers: invalid\n",
    )
    .unwrap();
    let operation = fixture.service.configuration_operation();
    let error = operation.current_owner_credential_values().unwrap_err();
    assert_eq!(error.reason, CredentialReadReason::ConfigurationParse);
    assert!(error.configuration_lock.is_none());
    assert_eq!(
        operation
            .configuration_budget
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .attempts,
        0
    );
    assert_eq!(
        crate::types::RunFailureKind::from_runtime(&error.into()),
        crate::types::RunFailureKind::ConfigurationRepairRequired
    );
    std::fs::write(
        fixture.root.join("config.yaml"),
        "schema_version: 1\nmcp_servers: []\n",
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let ready = runtime.block_on(fixture.service.ready_for_run()).unwrap();
    assert!(ready.ready_servers.is_empty());
    assert!(ready.unavailable.is_empty());
}

#[test]
fn released_lock_does_not_allow_a_changed_invalid_configuration_or_reuse_stale_authority() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    fixture
        .service
        .lock()
        .unwrap()
        .epochs
        .insert("existing".into(), 4);
    let operation = fixture.service.configuration_operation();
    let holder = fixture.holder();
    let worker = operation.clone();
    let join = std::thread::spawn(move || worker.current_owner_credential_values());
    wait_until_retry(&operation);
    std::fs::write(
        fixture.root.join("config.yaml"),
        "schema_version: 1\nmcp_servers: invalid\n",
    )
    .unwrap();
    drop(holder);
    let error = join.join().unwrap().unwrap_err();
    assert_eq!(error.reason, CredentialReadReason::ConfigurationParse);
    assert!(error.configuration_lock.is_none());
    assert_eq!(fixture.service.lock().unwrap().epochs["existing"], 5);
    assert!(
        operation
            .configuration_budget
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .attempts
            < 6
    );
}

#[test]
fn lock_syscall_failure_remains_distinct_from_busy_through_runtime_and_submission_diagnostics() {
    use vega_store::yaml_config::{ConfigurationFailure, ConfigurationFailureStage};
    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::Interrupted,
        std::io::ErrorKind::Other,
        std::io::ErrorKind::WouldBlock,
    ] {
        let error = mcp_read_error(McpStoreError::ConfigurationFailure(ConfigurationFailure {
            stage: ConfigurationFailureStage::LockAcquire,
            kind,
        }));
        assert_eq!(error.reason, CredentialReadReason::ConfigurationLockAcquire);
        assert!(error.configuration_lock.is_none());
        let runtime: vega_runtime::VegaError = error.into();
        assert_eq!(
            crate::types::RunFailureKind::from_runtime(&runtime),
            crate::types::RunFailureKind::ConfigurationAccessUnavailable
        );
        let detail = crate::submission_diagnostics::runtime_failure(
            crate::submission_diagnostics::PreparationStage::Launch,
            &runtime,
        );
        assert_eq!(detail.credential_reason, Some("configuration_lock_acquire"));
        assert!(detail.configuration_lock.is_none());
        assert!(detail.configuration_lock_attempts.is_none());
        assert!(detail.configuration_lock_wait_ms.is_none());
        let _serial = TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let store = fixture.store();
        let submission = vega_store::submission_diagnostics::PreparationEvent {
            submission_id: ulid::Ulid::generate().to_string(),
            attempt: 1,
            state: vega_store::submission_diagnostics::PreparationState::Failed,
            occurred_at: 1,
            failure: Some(detail),
        };
        vega_store::submission_diagnostics::append(store.conn(), &submission).unwrap();
        let exported =
            vega_store::submission_diagnostics::read(store.conn(), &submission.submission_id)
                .unwrap();
        let value: serde_json::Value = serde_json::from_str(&exported[0]).unwrap();
        assert_eq!(
            value["failure"]["credential_reason"],
            "configuration_lock_acquire"
        );
        assert_eq!(
            value["failure"]["io_kind"],
            vega_runtime::RuntimeIoKind::from(kind).as_str()
        );
        assert!(!exported[0].contains(fixture.root.to_str().unwrap()));
    }
}
