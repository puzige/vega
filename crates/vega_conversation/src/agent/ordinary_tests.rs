use super::ordinary::{OrdinaryPreparation, OrdinaryRun};
use super::*;
use crate::types::{
    CommitFact, ImageAttachment, OrdinarySubmission, PrimaryFact, RawSubmission, RetryEligibility,
    SubmissionFailure,
};

struct Fixture {
    root: PathBuf,
    store: Store,
    thread: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-ordinary-{}", ulid::Ulid::generate()));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("store.sqlite")).unwrap();
        store.migrate().unwrap();
        let thread = crate::threads::create_standalone_thread(&store, "model", "confirm")
            .unwrap()
            .id;
        Self {
            root,
            store,
            thread,
        }
    }

    fn begin(&self, images: Vec<ImageAttachment>) -> OrdinaryRun {
        let admission = crate::thread_access::acquire(&self.store, &self.thread).unwrap();
        let (submission, worker) = OrdinarySubmission::begin(
            self.store.database_identity(),
            self.thread.clone(),
            RawSubmission::new("raw".into(), images, 1, "model".into(), None, None),
            admission,
        )
        .unwrap();
        worker
            .bind_accepted_payload("accepted reference snapshot".into())
            .unwrap();
        OrdinaryRun::new(submission, Arc::new(worker))
    }

    fn prepare(
        &self,
        run: &OrdinaryRun,
        cancel: CancellationToken,
    ) -> Result<PreparedRun, ConversationError> {
        let preparation = OrdinaryPreparation::register(run).unwrap();
        let raw = run.submission.raw();
        let content = run
            .submission
            .snapshot()
            .accepted
            .unwrap()
            .text()
            .to_owned();
        let config = PersistenceActorConfig::default().with_ordinary_submission(Some(run.clone()));
        let mut config = config;
        config.steering_cancel = Some(cancel);
        let result = prepare_run_with_images_and_reasoning(
            self.store.database_path().unwrap().to_path_buf(),
            self.thread.clone(),
            content,
            "system".into(),
            run.submission.user_message_id().to_owned(),
            run.assistant_id().unwrap(),
            config,
            false,
            None,
            None,
            raw.images().to_vec(),
            Some(&preparation),
        );
        preparation.complete(result.is_ok());
        result
    }

    fn finish_unstarted(&self, run: OrdinaryRun, prepared: PreparedRun) -> OrdinarySubmission {
        messages::finish(
            self.store.conn(),
            &prepared.assistant_message_id,
            "",
            "failed",
        )
        .unwrap();
        run.worker.record_cleanup(Ok(())).unwrap();
        let submission = run.submission.clone();
        drop(prepared);
        let attempt = run.worker.attempt_id();
        ordinary::finish_ordinary_worker(
            &submission,
            attempt,
            run.worker,
            Err(SubmissionFailure::LaunchFailed),
        );
        submission
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn image(value: u8) -> ImageAttachment {
    let pixels = image::RgbaImage::from_pixel(1, 1, image::Rgba([value, 0, 0, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    ImageAttachment::from_bytes(bytes.into_inner()).unwrap()
}

#[test]
fn transaction_receipt_precedes_handoff_and_recovery_inserts_only_new_assistant() {
    let fixture = Fixture::new();
    let images = vec![image(1), image(2)];
    let run = fixture.begin(images.clone());
    let prepared = fixture.prepare(&run, CancellationToken::new()).unwrap();
    let snapshot = run.submission.snapshot();
    let CommitFact::Committed(receipt) = &snapshot.attempts.last().unwrap().commit else {
        panic!("receipt missing");
    };
    assert_eq!(receipt.user_message_id, run.submission.user_message_id());
    assert_eq!(receipt.persisted_text, "accepted reference snapshot");
    assert_eq!(receipt.persisted_images, images);
    assert_eq!(
        snapshot.attempts.last().unwrap().primary,
        PrimaryFact::NotYetDriven
    );
    assert_eq!(
        ordinary::persisted_images(fixture.store.conn(), &receipt.user_message_id).unwrap(),
        images
    );
    let old_assistant = receipt.assistant_message_id.clone();
    let old_user = receipt.user_message_id.clone();
    drop(snapshot);
    let submission = fixture.finish_unstarted(run, prepared);
    assert_eq!(
        submission.snapshot().retry,
        RetryEligibility::CommittedNotStarted
    );
    let worker = submission
        .try_retry(submission.snapshot().attempts.last().unwrap().id)
        .unwrap();
    let retry = OrdinaryRun::new(submission.clone(), Arc::new(worker));
    let recovered = fixture.prepare(&retry, CancellationToken::new()).unwrap();
    assert_ne!(recovered.assistant_message_id, old_assistant);
    assert_eq!(recovered.user_message_id, old_user);
    let rows = vega_store::context_compaction::load_source(fixture.store.conn(), &fixture.thread)
        .unwrap()
        .messages;
    assert_eq!(rows.iter().filter(|row| row.role == "user").count(), 1);
    assert_eq!(rows.iter().filter(|row| row.role == "assistant").count(), 2);
    assert_eq!(
        messages::find(fixture.store.conn(), &old_assistant)
            .unwrap()
            .unwrap()
            .status,
        "failed"
    );
    assert_eq!(
        ordinary::persisted_images(fixture.store.conn(), &old_user).unwrap(),
        images
    );
    assert!(
        recovered
            .request
            .history
            .iter()
            .any(|message| message.role == vega_runtime::ChatRole::Assistant
                && message.content.is_empty())
    );
    fixture.finish_unstarted(retry, recovered);
}

#[test]
fn known_rollback_leaves_no_user_and_retries_the_same_accepted_snapshot() {
    let fixture = Fixture::new();
    let run = fixture.begin(vec![image(3)]);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(fixture.prepare(&run, cancel).is_err());
    assert!(
        messages::find(fixture.store.conn(), run.submission.user_message_id())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        run.submission.snapshot().attempts.last().unwrap().commit,
        CommitFact::NotCommitted
    );
    let submission = run.submission.clone();
    let attempt = run.worker.attempt_id();
    ordinary::finish_ordinary_worker(
        &submission,
        attempt,
        run.worker,
        Err(SubmissionFailure::Cancelled),
    );
    assert_eq!(submission.snapshot().retry, RetryEligibility::Uncommitted);
    let worker = submission.try_retry(attempt).unwrap();
    let retry = OrdinaryRun::new(submission.clone(), Arc::new(worker));
    let prepared = fixture.prepare(&retry, CancellationToken::new()).unwrap();
    assert_eq!(prepared.user_message_id, submission.user_message_id());
    assert_eq!(
        submission.snapshot().accepted.unwrap().text(),
        "accepted reference snapshot"
    );
    fixture.finish_unstarted(retry, prepared);
}

#[test]
fn recovery_rejects_changed_image_bytes_and_newer_history_without_inserting() {
    for change_images in [false, true] {
        let fixture = Fixture::new();
        let run = fixture.begin(vec![image(4)]);
        let prepared = fixture.prepare(&run, CancellationToken::new()).unwrap();
        let submission = fixture.finish_unstarted(run, prepared);
        if change_images {
            fixture
                .store
                .conn()
                .execute(
                    "UPDATE image_attachments SET encoded=?1 WHERE message_id=?2",
                    (image(5).bytes(), submission.user_message_id()),
                )
                .unwrap();
        } else {
            let next = messages::next_seq(fixture.store.conn(), &fixture.thread).unwrap();
            messages::insert(
                fixture.store.conn(),
                &messages::MessageRow {
                    id: ulid::Ulid::generate().to_string(),
                    thread_id: fixture.thread.clone(),
                    seq: next,
                    role: "user".into(),
                    kind: "text".into(),
                    content: "newer".into(),
                    status: "done".into(),
                    created_at: 1,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
        }
        let count =
            vega_store::context_compaction::load_source(fixture.store.conn(), &fixture.thread)
                .unwrap()
                .messages
                .len();
        let worker = submission
            .try_retry(submission.snapshot().attempts.last().unwrap().id)
            .unwrap();
        let retry = OrdinaryRun::new(submission.clone(), Arc::new(worker));
        assert!(fixture.prepare(&retry, CancellationToken::new()).is_err());
        assert_eq!(
            vega_store::context_compaction::load_source(fixture.store.conn(), &fixture.thread)
                .unwrap()
                .messages
                .len(),
            count
        );
        assert!(
            submission
                .snapshot()
                .attempts
                .last()
                .unwrap()
                .history_changed
        );
        let attempt = retry.worker.attempt_id();
        ordinary::finish_ordinary_worker(
            &submission,
            attempt,
            retry.worker,
            Err(SubmissionFailure::PreparationFailed),
        );
        assert!(matches!(
            submission.snapshot().retry,
            RetryEligibility::Blocked(crate::types::RetryBlocker::HistoryChanged)
        ));
    }
}

#[test]
fn occupied_restart_is_read_only_and_service_mutations_cannot_change_input() {
    let fixture = Fixture::new();
    let run = fixture.begin(Vec::new());
    let prepared = fixture.prepare(&run, CancellationToken::new()).unwrap();
    assert!(crate::threads::set_thread_model(&fixture.store, &fixture.thread, "other").is_err());
    assert!(
        crate::threads::set_thread_mode(
            &fixture.store,
            &fixture.thread,
            crate::types::ThreadMode::Ask
        )
        .is_err()
    );
    assert!(crate::threads::delete_thread(&fixture.store, &fixture.thread).is_err());
    assert!(
        crate::threads::set_thread_permission_mode(
            &fixture.store,
            &fixture.thread,
            crate::types::PermissionMode::FullAccess
        )
        .is_err()
    );
    assert!(
        crate::threads::set_thread_status(
            &fixture.store,
            &fixture.thread,
            crate::types::ThreadStatus::Archived
        )
        .is_err()
    );
    assert!(
        crate::plans::review_plan(
            &fixture.store,
            &fixture.thread,
            "absent",
            crate::types::PlanReviewAction::Approve
        )
        .is_err()
    );
    let page = crate::history::restart_history_page(&fixture.store, &fixture.thread, 20).unwrap();
    assert!(page.entries.iter().any(|entry| matches!(entry, crate::history::HistoryEntry::UserText { message_id, .. } if message_id == run.submission.user_message_id())));
    assert_eq!(
        messages::find(fixture.store.conn(), &prepared.assistant_message_id)
            .unwrap()
            .unwrap()
            .status,
        "streaming"
    );
    fixture.finish_unstarted(run, prepared);
    assert!(crate::threads::set_thread_model(&fixture.store, &fixture.thread, "other").is_ok());
}

#[test]
fn persisted_transaction_image_is_distinct_from_accepted_model_input() {
    let fixture = Fixture::new();
    fixture.store.conn().execute_batch("CREATE TRIGGER persisted_user_projection AFTER INSERT ON messages WHEN NEW.role='user' BEGIN UPDATE messages SET content='persisted projection' WHERE id=NEW.id; END;").unwrap();
    let run = fixture.begin(Vec::new());
    let prepared = fixture.prepare(&run, CancellationToken::new()).unwrap();
    let snapshot = run.submission.snapshot();
    let CommitFact::Committed(receipt) = &snapshot.attempts.last().unwrap().commit else {
        panic!("receipt missing");
    };
    assert_eq!(receipt.persisted_text, "persisted projection");
    assert_eq!(
        snapshot.accepted.unwrap().text(),
        "accepted reference snapshot"
    );
    assert!(
        prepared
            .request
            .history
            .iter()
            .any(|message| message.role == vega_runtime::ChatRole::User
                && message.content == "accepted reference snapshot")
    );
    fixture.finish_unstarted(run, prepared);
}

#[test]
fn primary_start_with_empty_output_cannot_recover_and_status_cannot_create_proof() {
    let fixture = Fixture::new();
    let run = fixture.begin(Vec::new());
    let prepared = fixture.prepare(&run, CancellationToken::new()).unwrap();
    run.worker.mark_primary_started().unwrap();
    let submission = fixture.finish_unstarted(run, prepared);
    let snapshot = submission.snapshot();
    assert_eq!(
        snapshot.retry,
        RetryEligibility::Blocked(crate::types::RetryBlocker::PrimaryStarted)
    );
    let attempt = snapshot.attempts.last().unwrap().id;
    assert!(matches!(
        ordinary::observe_ordinary_history(&fixture.store, &submission, attempt).unwrap(),
        ordinary::OrdinaryHistoryObservation::MatchingUser
    ));
    assert!(submission.try_retry(attempt).is_err());
    fixture
        .store
        .conn()
        .execute(
            "UPDATE messages SET content='changed' WHERE id=?1",
            [submission.user_message_id()],
        )
        .unwrap();
    assert!(matches!(
        ordinary::observe_ordinary_history(&fixture.store, &submission, attempt).unwrap(),
        ordinary::OrdinaryHistoryObservation::Conflict
    ));
    assert!(submission.try_retry(attempt).is_err());
}

#[test]
fn lost_registered_worker_cannot_unlock_or_create_retry_proof() {
    let fixture = Fixture::new();
    let run = fixture.begin(Vec::new());
    let task = ordinary::OrdinaryTask::register(Some(&run))
        .unwrap()
        .unwrap();
    let submission = run.submission.clone();
    drop(run);
    assert!(
        vega_store::thread_admission::try_acquire(
            fixture.store.database_identity(),
            &fixture.thread
        )
        .unwrap()
        .is_none()
    );
    drop(task);
    let snapshot = submission.snapshot();
    assert!(snapshot.attempts.last().unwrap().sealed);
    assert!(snapshot.attempts.last().unwrap().abnormal);
    assert_eq!(
        snapshot.retry,
        RetryEligibility::Blocked(crate::types::RetryBlocker::Unknown)
    );
    assert!(
        vega_store::thread_admission::try_acquire(
            fixture.store.database_identity(),
            &fixture.thread
        )
        .unwrap()
        .is_some()
    );
}
