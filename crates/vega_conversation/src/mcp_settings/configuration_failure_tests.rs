use super::*;
use crate::types::RunFailureKind;
use std::os::unix::fs::PermissionsExt;
use vega_store::run_diagnostics::*;
use vega_store::yaml_config::{ConfigurationFailure, ConfigurationFailureStage};

struct Fixture {
    root: PathBuf,
    service: McpServerSettingsService,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("configuration-service-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let database = root.join("synthetic.db");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        std::fs::write(root.join("config.yaml"), "schema_version: 1\n").unwrap();
        let service = McpServerSettingsService::new(database, root.clone());
        service.reconcile_declarations().unwrap();
        Self { root, service }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn readiness_recovers_only_after_current_configuration_is_repaired() {
    let fixture = Fixture::new();
    assert!(
        fixture
            .service
            .ready_for_run()
            .await
            .unwrap()
            .ready_servers
            .is_empty()
    );
    let path = fixture.root.join("config.yaml");
    std::fs::write(&path, "schema_version: [secret-sentinel\n").unwrap();
    let error = match fixture.service.ready_for_run().await {
        Err(McpSettingsError::CredentialRead(error)) => error,
        _ => panic!("malformed configuration accepted or misclassified"),
    };
    assert_eq!(error.reason, CredentialReadReason::ConfigurationParse);
    assert_eq!(
        RunFailureKind::from_runtime(&error.into()),
        RunFailureKind::ConfigurationRepairRequired
    );
    for invalid in [
        "schema_version: 1\nmcp_servers: invalid\n",
        "schema_version: 1\nmcp_servers:\n  - id: invalid\n    display_name: synthetic\n    enabled: false\n    transport:\n      type: local\n      executable: /synthetic\n      args: []\n      environment: []\n",
    ] {
        std::fs::write(&path, invalid).unwrap();
        let error = match fixture.service.ready_for_run().await {
            Err(McpSettingsError::CredentialRead(error)) => error,
            _ => panic!("invalid declaration accepted or misclassified"),
        };
        assert_eq!(error.reason, CredentialReadReason::ConfigurationParse);
        assert_eq!(
            RunFailureKind::from_runtime(&error.into()),
            RunFailureKind::ConfigurationRepairRequired
        );
    }
    std::fs::write(&path, "schema_version: 1\nui:\n  theme: light\n").unwrap();
    assert!(
        fixture
            .service
            .ready_for_run()
            .await
            .unwrap()
            .ready_servers
            .is_empty()
    );
    let lock = fixture.root.join(".config.lock");
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        matches!(fixture.service.ready_for_run().await, Err(McpSettingsError::CredentialRead(error)) if error.reason == CredentialReadReason::ConfigurationLockValidation)
    );
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        fixture
            .service
            .ready_for_run()
            .await
            .unwrap()
            .ready_servers
            .is_empty()
    );
    assert_eq!(
        vega_store::config::read_from(&path).unwrap().ui.theme,
        "light"
    );
}

#[test]
fn classified_failures_roundtrip_without_content_and_project_consistently() {
    let fixture = Fixture::new();
    let store = Store::open(&fixture.service.database_path).unwrap();
    let thread = ulid::Ulid::generate().to_string();
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            reasoning_preferences: None,
            id: &thread,
            project_id: "",
            title: "synthetic",
            mode: "execute",
            permission_mode: "confirm",
            permission_revision: 0,
            full_access_grant: None,
            model: "synthetic",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    let cases = [
        (
            ConfigurationFailureStage::Poisoned,
            std::io::ErrorKind::Other,
            RunFailureKind::ConfigurationRestartRequired,
        ),
        (
            ConfigurationFailureStage::Directory,
            std::io::ErrorKind::PermissionDenied,
            RunFailureKind::ConfigurationAccessUnavailable,
        ),
        (
            ConfigurationFailureStage::LockOpen,
            std::io::ErrorKind::PermissionDenied,
            RunFailureKind::ConfigurationAccessUnavailable,
        ),
        (
            ConfigurationFailureStage::LockValidation,
            std::io::ErrorKind::InvalidData,
            RunFailureKind::ConfigurationRepairRequired,
        ),
        (
            ConfigurationFailureStage::Read,
            std::io::ErrorKind::Other,
            RunFailureKind::ConfigurationAccessUnavailable,
        ),
        (
            ConfigurationFailureStage::Parse,
            std::io::ErrorKind::InvalidData,
            RunFailureKind::ConfigurationRepairRequired,
        ),
    ];
    for (stage, kind, expected) in cases {
        let error = configuration_failure(ConfigurationFailure { stage, kind });
        let runtime: vega_runtime::VegaError = error.into();
        assert_eq!(RunFailureKind::from_runtime(&runtime), expected);
        let run = ulid::Ulid::generate().to_string();
        let mut event = NewDiagnosticEvent {
            thread_id: thread.clone(),
            run_id: run.clone(),
            attempt_id: run.clone(),
            parent_attempt_id: None,
            tool_call_id: None,
            phase: DiagnosticPhase::Run,
            state: DiagnosticState::Started,
            failure_code: None,
            occurred_at: 1,
            duration_ms: None,
            metrics: DiagnosticMetrics::default(),
        };
        insert(store.conn(), &event).unwrap();
        event.state = DiagnosticState::Failed;
        event.failure_code = Some(DiagnosticFailureCode::UnknownSafeFailure);
        event.duration_ms = Some(1);
        event.metrics = DiagnosticMetrics {
            credential_stage: Some("mcp".into()),
            credential_reason: Some(error.reason.as_str().into()),
            runtime_stage: Some("request_preparation".into()),
            runtime_reason: Some("io".into()),
            io_kind: Some(vega_runtime::RuntimeIoKind::from(kind).as_str().into()),
            failure_source: Some("runtime".into()),
            failure_kind: Some("credential_check".into()),
            ..Default::default()
        };
        insert(store.conn(), &event).unwrap();
        let rows = read_by_run(store.conn(), &thread, &run).unwrap();
        assert_eq!(rows[1].event.metrics, event.metrics);
        let restored = if stage == ConfigurationFailureStage::Poisoned {
            assert!(!vega_store::yaml_config::edit_lock_poisoned());
            RunFailureKind::CredentialCheckUnavailable
        } else {
            expected
        };
        assert_eq!(
            RunFailureKind::from_diagnostic(&rows[1].event.metrics),
            restored
        );
        if stage == ConfigurationFailureStage::Poisoned {
            use vega_store::messages::{self, MessageRow};
            let user = ulid::Ulid::generate().to_string();
            for (id, seq, role, status) in
                [(&user, 1, "user", "done"), (&run, 2, "assistant", "failed")]
            {
                messages::insert(
                    store.conn(),
                    &MessageRow {
                        id: id.clone(),
                        thread_id: thread.clone(),
                        seq,
                        role: role.into(),
                        kind: "text".into(),
                        content: "synthetic".into(),
                        status: status.into(),
                        created_at: 1,
                        plan_status: None,
                        plan_review_note: None,
                        plan_reviewed_at: None,
                    },
                )
                .unwrap();
            }
            store.conn().execute("INSERT INTO credential_continuations (id, thread_id, user_message_id, source_version, source_fingerprint, raw_user_fingerprint, state, reason, created_at) VALUES (?1, ?2, ?3, 1, 'synthetic', 'synthetic', 'ready', 'unavailable', 1)", [&run, &thread, &user]).unwrap();
            let page = crate::history::latest_history_page(&store, &thread, 20).unwrap();
            assert!(page.entries.iter().any(|entry| matches!(entry, crate::history::HistoryEntry::CredentialContinuation { message_id, failure: RunFailureKind::CredentialCheckUnavailable, .. } if message_id == &run)));
        }
        let payload = export_run(store.conn(), &thread, &run).unwrap();
        assert!(payload.contains(error.reason.as_str()));
        assert!(!payload.contains(fixture.root.to_str().unwrap()));
        use vega_store::submission_diagnostics as submission;
        let failure = crate::submission_diagnostics::runtime_failure(
            submission::PreparationStage::Launch,
            &runtime,
        );
        assert_eq!(failure.credential_reason, Some(error.reason.as_str()));
        submission::append(
            store.conn(),
            &submission::PreparationEvent {
                submission_id: run.clone(),
                attempt: 1,
                state: submission::PreparationState::Failed,
                occurred_at: 1,
                failure: Some(failure),
            },
        )
        .unwrap();
        assert!(submission::read(store.conn(), &run).unwrap()[0].contains(error.reason.as_str()));
    }
    let legacy = CredentialReadError::configuration_io(
        CredentialReadStage::Mcp,
        std::io::ErrorKind::WouldBlock,
    );
    assert_eq!(
        RunFailureKind::from_runtime(&legacy.into()),
        RunFailureKind::CredentialInternalUnavailable
    );
}
