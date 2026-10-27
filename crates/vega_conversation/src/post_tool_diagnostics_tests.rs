use crate::pending_messages::QueueSteeringSource;
use crate::types::RunFailureKind;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    RuntimeFailureDetail, RuntimeFailureReason as Reason, RuntimeFailureStage as Stage,
    RuntimeSteeringSource, VegaError,
};
use vega_store::{Store, pending_messages as rows, run_diagnostics as diagnostics};

struct Fixture {
    root: PathBuf,
    store: Store,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-diagnostics-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("test.db")).unwrap();
        store.migrate().unwrap();
        store.conn().execute("INSERT INTO threads(id,title,model,created_at,updated_at) VALUES ('thread','test','model',0,0)", []).unwrap();
        Self { root, store }
    }
    fn source(&self, cancel: CancellationToken) -> QueueSteeringSource {
        QueueSteeringSource::new(
            self.root.join("test.db"),
            self.root.clone(),
            "thread".into(),
            "generation".into(),
            Arc::new(|| Ok(Vec::new())),
            cancel,
        )
    }
    fn pending(&self, payload: &str) {
        rows::insert(
            self.store.conn(),
            &rows::PendingMessageRow {
                id: "pending".into(),
                thread_id: "thread".into(),
                ordinal: 0,
                behavior: "steer".into(),
                generation: "generation".into(),
                payload: payload.into(),
                created_at: 0,
            },
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn empty_and_cancelled_queues_do_not_fail_or_consume_messages() {
    let fixture = Fixture::new();
    let source = fixture.source(CancellationToken::new());
    assert!(source.ready().await.unwrap().is_none());
    assert!(source.take().await.unwrap().is_none());
    fixture.pending(r#"{"content":"hello","images":[]}"#);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(fixture.source(cancel).take().await.unwrap().is_none());
    assert_eq!(rows::count(fixture.store.conn(), "thread").unwrap(), 1);
}

#[tokio::test]
async fn malformed_pending_message_reports_decode_without_claiming() {
    let fixture = Fixture::new();
    fixture.pending("PRIVATE_PAYLOAD_CANARY");
    let error = fixture
        .source(CancellationToken::new())
        .take()
        .await
        .err()
        .unwrap();
    let metrics = error.diagnostic_metrics();
    assert_eq!(metrics.runtime_stage, Some("pending_decode"));
    assert_eq!(metrics.runtime_reason, Some("pending_decode"));
    assert_eq!(
        RunFailureKind::from_runtime(&error),
        RunFailureKind::RuntimeState
    );
    assert!(!format!("{error:?} {error}").contains("PRIVATE_PAYLOAD_CANARY"));
    assert!(
        rows::next_steer(fixture.store.conn(), "thread", "generation")
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn admission_conflict_is_not_reported_as_io_or_retried() {
    let fixture = Fixture::new();
    let _owner =
        vega_store::thread_admission::try_acquire(fixture.store.database_identity(), "thread")
            .unwrap()
            .unwrap();
    let error = fixture
        .source(CancellationToken::new())
        .take()
        .await
        .err()
        .unwrap();
    assert_eq!(
        error.diagnostic_metrics().runtime_stage,
        Some("pending_admission")
    );
    assert_eq!(error.diagnostic_metrics().runtime_reason, Some("admission"));
}

#[tokio::test]
async fn read_and_claim_database_failures_keep_sqlite_codes() {
    let fixture = Fixture::new();
    fixture
        .store
        .conn()
        .execute("ALTER TABLE pending_messages RENAME TO unavailable", [])
        .unwrap();
    let error = fixture
        .source(CancellationToken::new())
        .ready()
        .await
        .unwrap_err();
    assert_eq!(
        error.diagnostic_metrics().runtime_stage,
        Some("pending_read")
    );
    assert_eq!(error.diagnostic_metrics().runtime_reason, Some("database"));
    assert_eq!(error.diagnostic_metrics().sqlite_code, Some(1));
    fixture
        .store
        .conn()
        .execute("ALTER TABLE unavailable RENAME TO pending_messages", [])
        .unwrap();
    fixture.pending(r#"{"content":"hello","images":[]}"#);
    fixture.store.conn().execute_batch("CREATE TRIGGER reject_claim BEFORE UPDATE ON pending_messages BEGIN SELECT RAISE(ABORT, 'PRIVATE_SQL_CANARY'); END;").unwrap();
    let error = fixture
        .source(CancellationToken::new())
        .take()
        .await
        .err()
        .unwrap();
    let metrics = error.diagnostic_metrics();
    assert_eq!(metrics.runtime_stage, Some("pending_claim"));
    assert_eq!(metrics.runtime_reason, Some("database"));
    assert_eq!(metrics.sqlite_code, Some(19));
    assert_eq!(metrics.sqlite_extended_code, Some(1811));
    assert!(!format!("{error:?} {error}").contains("PRIVATE_SQL_CANARY"));
    assert!(
        rows::next_steer(fixture.store.conn(), "thread", "generation")
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn normal_pending_message_is_claimed_once() {
    let fixture = Fixture::new();
    fixture.pending(r#"{"content":"hello","images":[]}"#);
    let source = fixture.source(CancellationToken::new());
    let input = source.take().await.unwrap().unwrap();
    assert_eq!(input.message.content, "hello");
    assert_eq!(input.original_content, "hello");
    assert!(source.take().await.unwrap().is_none());
}

#[test]
fn runtime_classification_survives_store_export_and_historical_read() {
    let fixture = Fixture::new();
    for (index, reason, expected) in [
        (0, Reason::Admission, RunFailureKind::RuntimeState),
        (1, Reason::References, RunFailureKind::RuntimeReferences),
        (2, Reason::Capability, RunFailureKind::RuntimeCapability),
        (
            3,
            Reason::ContextBudget,
            RunFailureKind::RuntimeContextBudget,
        ),
        (
            4,
            Reason::ContextArithmetic,
            RunFailureKind::RuntimeContextArithmetic,
        ),
        (5, Reason::Worker, RunFailureKind::RuntimePipeline),
    ] {
        let error = VegaError::RuntimeFailure(RuntimeFailureDetail::new(reason))
            .at_runtime_stage(Stage::RequestPreparation);
        assert_eq!(RunFailureKind::from_runtime(&error), expected);
        let metrics = crate::agent::diagnostics::failure_metrics(&error);
        assert_eq!(RunFailureKind::from_diagnostic(&metrics), expected);
        assert_eq!(
            crate::agent::diagnostics::run_failure(&error),
            diagnostics::DiagnosticFailureCode::RuntimeFailure
        );
        let run = format!("run-{index}");
        diagnostics::insert(
            fixture.store.conn(),
            &diagnostics::NewDiagnosticEvent {
                thread_id: "thread".into(),
                run_id: run.clone(),
                attempt_id: run.clone(),
                parent_attempt_id: None,
                tool_call_id: None,
                phase: diagnostics::DiagnosticPhase::Run,
                state: diagnostics::DiagnosticState::Started,
                failure_code: None,
                occurred_at: 0,
                duration_ms: None,
                metrics: diagnostics::DiagnosticMetrics::default(),
            },
        )
        .unwrap();
        diagnostics::insert(
            fixture.store.conn(),
            &diagnostics::NewDiagnosticEvent {
                thread_id: "thread".into(),
                run_id: run.clone(),
                attempt_id: run.clone(),
                parent_attempt_id: None,
                tool_call_id: None,
                phase: diagnostics::DiagnosticPhase::Run,
                state: diagnostics::DiagnosticState::Failed,
                failure_code: Some(diagnostics::DiagnosticFailureCode::RuntimeFailure),
                occurred_at: 1,
                duration_ms: Some(1),
                metrics,
            },
        )
        .unwrap();
        let export: serde_json::Value = serde_json::from_str(
            &diagnostics::export_run(fixture.store.conn(), "thread", &run).unwrap(),
        )
        .unwrap();
        assert_eq!(export["events"][1]["failure_code"], "runtime_failure");
        assert_eq!(
            export["events"][1]["metrics"]["runtime_reason"],
            reason.as_str()
        );
        let read = diagnostics::read_by_run(fixture.store.conn(), "thread", &run).unwrap();
        assert_eq!(
            RunFailureKind::from_diagnostic(&read[1].event.metrics),
            expected
        );
    }
}

#[tokio::test]
async fn invalid_references_fail_before_pending_claim() {
    let fixture = Fixture::new();
    fixture.pending(r#"{"content":"@missing-file.txt","images":[]}"#);
    let error = fixture
        .source(CancellationToken::new())
        .take()
        .await
        .err()
        .unwrap();
    assert_eq!(
        error.diagnostic_metrics().runtime_stage,
        Some("pending_references")
    );
    assert_eq!(
        error.diagnostic_metrics().runtime_reason,
        Some("references")
    );
    assert!(
        rows::next_steer(fixture.store.conn(), "thread", "generation")
            .unwrap()
            .is_some()
    );
}
