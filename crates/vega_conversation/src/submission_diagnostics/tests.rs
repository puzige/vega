use super::*;
use crate::ordinary_submissions::{
    OrdinarySubmission, OrdinaryWorker, RawSubmission, SubmissionFailure,
};
use serde_json::Value;

fn submission() -> (Store, OrdinarySubmission, OrdinaryWorker) {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let thread = ulid::Ulid::generate().to_string();
    let admission = vega_store::thread_admission::try_acquire(store.database_identity(), &thread)
        .unwrap()
        .unwrap();
    let raw = RawSubmission::new(
        "secret body /private/work sk-secret".into(),
        vec![],
        vec!["private-file".into()],
        1,
        "secret-model".into(),
        None,
        None,
    );
    let (submission, worker) =
        OrdinarySubmission::begin(store.database_identity(), thread, raw, admission).unwrap();
    (store, submission, worker)
}

fn diagnostic(
    submission: &OrdinarySubmission,
    attempt: crate::ordinary_submissions::AttemptId,
) -> Value {
    serde_json::from_str(&submission.preparation_diagnostics(attempt).unwrap()).unwrap()
}

#[test]
fn unmaterialized_failure_retains_specific_cause_and_retry_has_distinct_events() {
    let (store, submission, worker) = submission();
    let first = worker.attempt_id();
    let later = worker.register_worker().unwrap();
    let detail = PreparationFailure::new(
        PreparationStage::Permission,
        PreparationReason::InvalidIdentity,
    );
    worker
        .complete(Err(SubmissionFailure::Preparation(detail)))
        .unwrap();
    later
        .complete(Err(SubmissionFailure::PreparationFailed))
        .unwrap();
    submission.seal(first).unwrap();
    let before = diagnostic(&submission, first);
    assert_eq!(before["events"][1]["failure"]["stage"], "permission");
    assert_eq!(before["events"][1]["failure"]["reason"], "invalid_identity");
    assert_eq!(before["events"][1]["state"], "failed");
    assert_eq!(
        store
            .conn()
            .query_row("SELECT count(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .conn()
            .query_row("SELECT count(*) FROM threads", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let retry = submission.try_retry(first).unwrap();
    let second = retry.attempt_id();
    assert_eq!(second.number(), first.number() + 1);
    retry.complete(Err(SubmissionFailure::Cancelled)).unwrap();
    submission.seal(second).unwrap();
    assert_eq!(diagnostic(&submission, first), before);
    let after = diagnostic(&submission, second);
    assert_eq!(after["events"].as_array().unwrap().len(), 2);
    assert_eq!(after["events"][1]["state"], "cancelled");
    for secret in [
        "secret body",
        "/private/work",
        "sk-secret",
        "private-file",
        "secret-model",
    ] {
        assert!(!format!("{before}{after}").contains(secret));
    }
}

#[test]
fn lost_worker_is_interrupted_and_does_not_make_retry_safe() {
    let (_, submission, worker) = submission();
    let attempt = worker.attempt_id();
    drop(worker);
    assert_eq!(
        diagnostic(&submission, attempt)["events"][1]["state"],
        "interrupted"
    );
    submission.seal(attempt).unwrap();
    assert!(submission.try_retry(attempt).is_err());
    assert_eq!(
        diagnostic(&submission, attempt)["events"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn sqlite_and_io_classification_never_serializes_error_text() {
    let store = Store::open(":memory:").unwrap();
    store.conn().execute_batch("CREATE TABLE unique_rows (id INTEGER PRIMARY KEY); INSERT INTO unique_rows VALUES (1);").unwrap();
    let error = store
        .conn()
        .execute("INSERT INTO unique_rows VALUES (1)", [])
        .unwrap_err();
    let detail = runtime_failure(PreparationStage::Database, &error.into());
    assert_eq!(detail.reason, PreparationReason::Database);
    assert_eq!(detail.sqlite_code, Some(19));
    assert!(detail.sqlite_extended_code.is_some());
    let error = vega_runtime::VegaError::Io(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "/private/secret sk-secret body",
    ));
    let detail = runtime_failure(PreparationStage::Workspace, &error);
    assert_eq!(detail.reason, PreparationReason::Io);
    assert_eq!(detail.io_kind, Some("permission_denied"));
    let json = serde_json::to_string(&detail).unwrap();
    assert!(!json.contains("/private"));
    assert!(!json.contains("sk-secret"));
    assert!(!json.contains("body"));
}

#[test]
fn storage_accepts_unmaterialized_attempts_and_rejects_duplicate_terminal() {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let id = ulid::Ulid::generate().to_string();
    let mut event = PreparationEvent {
        submission_id: id.clone(),
        attempt: 1,
        state: PreparationState::Started,
        occurred_at: 1,
        failure: None,
    };
    vega_store::submission_diagnostics::append(store.conn(), &event).unwrap();
    event.state = PreparationState::Failed;
    event.failure = Some(PreparationFailure::new(
        PreparationStage::Workspace,
        PreparationReason::MissingWorkspace,
    ));
    vega_store::submission_diagnostics::append(store.conn(), &event).unwrap();
    assert!(vega_store::submission_diagnostics::append(store.conn(), &event).is_err());
    event.attempt = 2;
    event.state = PreparationState::Started;
    event.failure = None;
    vega_store::submission_diagnostics::append(store.conn(), &event).unwrap();
    event.state = PreparationState::Prepared;
    vega_store::submission_diagnostics::append(store.conn(), &event).unwrap();
    assert_eq!(
        vega_store::submission_diagnostics::read(store.conn(), &id)
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn prepared_journal_is_not_task_success_and_storage_failure_keeps_memory() {
    let store = Store::open(":memory:").unwrap();
    let journal = Journal::default();
    let id = ulid::Ulid::generate().to_string();
    journal.record(id.clone(), 1, PreparationState::Started, None);
    journal.record(id.clone(), 1, PreparationState::Prepared, None);
    journal.record(
        id,
        1,
        PreparationState::Failed,
        Some(PreparationFailure::new(
            PreparationStage::Worker,
            PreparationReason::Unknown,
        )),
    );
    journal.attach(
        Path::new("/nonexistent-vega-diagnostic-root/database"),
        store.database_identity(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while journal.status.pending.load(Ordering::Acquire) != 0
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let exported: Value = serde_json::from_str(&journal.export(1)).unwrap();
    assert_eq!(exported["storage"], "failed_memory_retained");
    assert_eq!(exported["events"].as_array().unwrap().len(), 2);
    assert_eq!(exported["events"][1]["state"], "prepared");
    assert!(!exported.to_string().contains("succeeded"));
}

#[test]
fn committed_preparation_emits_prepared_once_without_claiming_run_success() {
    use crate::ordinary_submissions::{CommittedReceipt, PersistedPayload};
    let (_, submission, worker) = submission();
    let attempt = worker.attempt_id();
    worker.begin_preparing().unwrap();
    worker.bind_accepted_payload("accepted".into()).unwrap();
    worker
        .bind_persisted_payload(PersistedPayload::new("accepted".into(), vec![]))
        .unwrap();
    worker.begin_commit().unwrap();
    let assistant = submission
        .snapshot()
        .attempts
        .last()
        .unwrap()
        .assistant_message_id
        .clone();
    worker
        .record_committed(CommittedReceipt {
            user_message_id: submission.user_message_id().into(),
            user_seq: 1,
            assistant_message_id: assistant,
            assistant_seq: 2,
            next_seq: 3,
            persisted_text: "accepted".into(),
            persisted_images: vec![],
        })
        .unwrap();
    worker.mark_primary_started().unwrap();
    worker.mark_primary_started().unwrap();
    worker.record_cleanup(Ok(())).unwrap();
    worker
        .complete(Err(SubmissionFailure::PreparationFailed))
        .unwrap();
    submission.seal(attempt).unwrap();
    let exported = diagnostic(&submission, attempt);
    assert_eq!(exported["events"].as_array().unwrap().len(), 2);
    assert_eq!(exported["events"][1]["state"], "prepared");
    assert!(exported["events"][1]["failure"].is_null());
}
