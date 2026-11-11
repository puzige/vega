use super::*;
use crate::types::RunFailureKind;

struct Fixture(McpServerSettingsService);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(ulid::Ulid::generate().to_string());
        std::fs::create_dir(&root).unwrap();
        let path = root.join("synthetic.db");
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        std::fs::write(root.join("config.yaml"), "schema_version: 1\n").unwrap();
        let service = McpServerSettingsService::new(path, root.clone());
        service.reconcile_declarations().unwrap();
        vega_store::keystore::set_key(&root, "fixture", "synthetic-value").unwrap();
        Self(service)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0.config_root).unwrap();
    }
}

fn contending(operation: &McpServerSettingsService) {
    let until = Instant::now() + Duration::from_secs(3);
    while operation
        .configuration_budget
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .attempts
        == 0
    {
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
}

#[test]
fn concurrent_readers_recheck_current_secrets_after_registry_release() {
    let fixture = Fixture::new();
    let first = fixture.0.configuration_operation();
    let second = fixture.0.configuration_operation();
    let guard = fixture.0.registry.lock().unwrap();
    std::thread::scope(|scope| {
        let a = scope.spawn(|| first.current_owner_credential_values());
        let b = scope.spawn(|| second.current_owner_credential_values());
        contending(&first);
        contending(&second);
        vega_store::keystore::set_key(&fixture.0.config_root, "fixture", "rotated-value").unwrap();
        drop(guard);
        for reader in [a, b] {
            assert_eq!(reader.join().unwrap().unwrap(), ["rotated-value"]);
        }
    });
}

#[test]
fn sustained_contention_latches_only_this_operation_and_remains_configuration_busy() {
    let fixture = Fixture::new();
    let operation = fixture.0.configuration_operation();
    let guard = fixture.0.registry.lock().unwrap();
    let error = operation.current_owner_credential_values().unwrap_err();
    assert_eq!(error.configuration_lock, Some("registry"));
    assert!((1..=6).contains(&error.configuration_lock_attempts.unwrap()));
    assert!(error.configuration_lock_wait_ms.unwrap() <= 310);
    assert_eq!(
        RunFailureKind::from_runtime(&vega_runtime::VegaError::CredentialRead(error)),
        RunFailureKind::ConfigurationBusy
    );
    drop(guard);
    assert_eq!(
        operation.current_owner_credential_values().unwrap_err(),
        error
    );
    assert_eq!(
        fixture.0.current_owner_credential_values().unwrap(),
        ["synthetic-value"]
    );
}

#[test]
fn cancelled_reader_exits_while_registry_remains_owned() {
    let fixture = Fixture::new();
    let operation = fixture.0.configuration_operation();
    let _guard = fixture.0.registry.lock().unwrap();
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| operation.current_owner_credential_values());
        contending(&operation);
        operation.configuration_cancel.cancel();
        let error = reader.join().unwrap().unwrap_err();
        assert!(error.configuration_lock.is_none());
    });
}

#[test]
fn accepted_digest_does_not_admit_changed_or_malformed_configuration() {
    let fixture = Fixture::new();
    let path = fixture.0.config_root.join("config.yaml");
    std::fs::write(&path, "schema_version: 1\nmcp_servers: [wrong]\n").unwrap();
    assert_eq!(
        fixture
            .0
            .current_owner_credential_values()
            .unwrap_err()
            .reason,
        CredentialReadReason::ConfigurationParse
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while fixture
        .0
        .reconciliation_running
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    std::fs::write(&path, "schema_version: 1\n").unwrap();
    assert_eq!(
        fixture.0.current_owner_credential_values().unwrap(),
        ["synthetic-value"]
    );
}

#[test]
fn contention_time_and_attempts_remain_cumulative_between_acquisitions() {
    let mut budget = ConfigurationRetryBudget::default();
    let busy = vega_store::yaml_config::ConfigurationLockBusy::Registry;
    budget.next_delay(busy, Duration::ZERO).unwrap();
    budget.contention_elapsed = Duration::from_millis(250);
    budget.check_acquisition(Duration::ZERO).unwrap();
    assert_eq!(
        budget.next_delay(busy, Duration::ZERO).unwrap(),
        Duration::from_millis(20)
    );
    let exhausted = budget
        .check_acquisition(Duration::from_millis(250))
        .unwrap_err();
    assert_eq!(
        budget.check_acquisition(Duration::ZERO).unwrap_err(),
        exhausted
    );
}

#[test]
fn changed_source_during_registry_wait_rejects_the_prevalidated_identity() {
    let fixture = Fixture::new();
    let path = fixture.0.config_root.join("config.yaml");
    let operation = fixture.0.configuration_operation();
    let guard = fixture.0.registry.lock().unwrap();
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| operation.current_owner_credential_values());
        contending(&operation);
        std::fs::write(path, "schema_version: 1\nmcp_servers: [invalid]\n").unwrap();
        drop(guard);
        assert_eq!(
            reader.join().unwrap().unwrap_err().reason,
            CredentialReadReason::ConfigurationParse
        );
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while fixture
        .0
        .reconciliation_running
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
