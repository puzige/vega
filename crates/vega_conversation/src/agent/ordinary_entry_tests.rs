use super::ordinary::OrdinaryRun;
use super::persistence::OrdinaryEntryFault;
use super::{
    ConversationRun, PermissionHook, PersistenceActorConfig, finish_ordinary_worker,
    run_thread_task_with_images_reasoning_and_mcp,
};
use crate::types::{
    CleanupFact, CommitFact, CommittedReceipt, ConversationError, ConversationEvent,
    ImageAttachment, OrdinarySubmission, PermissionDecision, PermissionRequest, PrimaryFact,
    RawSubmission, RetryBlocker, RetryEligibility, SubmissionFailure,
};
use futures::future::BoxFuture;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use vega_runtime::model_compat::{
    ModelApi, ModelRoute, ResolvedModel, decode_model_catalog, freeze_choice, resolve_model,
};
use vega_runtime::{
    ChatRequest, EventStream, Provider, ProviderEvent, ReasoningChoice, StopReason, VegaError,
};
use vega_store::{Store, messages};

const ACCEPTED_TEXT: &str = "frozen accepted reference contents";

struct EntryFixture {
    root: PathBuf,
    store: Store,
    tools: vega_tools::Tools,
    thread: String,
    model: ResolvedModel,
}

impl EntryFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-entry-{}", ulid::Ulid::generate()));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("store.sqlite")).unwrap();
        store.migrate().unwrap();
        let thread = crate::threads::create_standalone_thread(&store, "model", "confirm")
            .unwrap()
            .id;
        let tools = vega_tools::Tools::new(&root).unwrap();
        let catalog = decode_model_catalog(br#"{"version":1,"models":[{"provider":"provider","id":"model","api":"openai-completions","baseUrl":"https://example.invalid/v1","input":["text","image"]}]}"#).unwrap();
        let definition = catalog
            .lookup(
                &ModelRoute::new(ModelApi::ChatCompletions, "https://example.invalid/v1").unwrap(),
                &crate::types::ModelIdentity {
                    provider: "provider".into(),
                    model: "model".into(),
                },
            )
            .unwrap();
        let model = resolve_model(&definition, None).unwrap();
        Self {
            root,
            store,
            tools,
            thread,
            model,
        }
    }

    fn begin(&self) -> OrdinaryRun {
        let reasoning = freeze_choice(&self.model, ReasoningChoice::ProviderDefault).unwrap();
        let admission = crate::thread_access::acquire(&self.store, &self.thread).unwrap();
        let (submission, worker) = OrdinarySubmission::begin(
            self.store.database_identity(),
            self.thread.clone(),
            RawSubmission::new(
                "raw reference".into(),
                vec![original_image()],
                7,
                "model".into(),
                Some(Box::new(self.model.clone())),
                None,
            ),
            admission,
        )
        .unwrap();
        worker
            .bind_effective_authority(self.model.clone(), reasoning)
            .unwrap();
        worker.bind_accepted_payload(ACCEPTED_TEXT.into()).unwrap();
        OrdinaryRun::new(submission, Arc::new(worker))
    }

    async fn execute<F>(
        &self,
        run: OrdinaryRun,
        provider: &EntryProvider,
        cancel: CancellationToken,
        fault: Option<OrdinaryEntryFault>,
        sink: F,
    ) -> (
        OrdinarySubmission,
        Result<ConversationRun, ConversationError>,
    )
    where
        F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
    {
        let accepted = run.submission.snapshot().accepted.unwrap();
        let reasoning = run
            .submission
            .snapshot()
            .authority
            .unwrap()
            .reasoning()
            .clone();
        let config = PersistenceActorConfig {
            ordinary_entry_fault: fault,
            ..PersistenceActorConfig::default().with_ordinary_submission(Some(run.clone()))
        };
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            run_thread_task_with_images_reasoning_and_mcp(
                &self.store,
                provider,
                &self.tools,
                &self.thread,
                accepted.text(),
                "system",
                cancel.clone(),
                &NoPermission,
                sink,
                config,
                None,
                None,
                Some(reasoning),
                accepted.images().to_vec(),
                Vec::new(),
            ),
        )
        .await
        .expect("public ordinary entry must terminate");
        let completion = match &result {
            Ok(outcome) if !outcome.failed && !outcome.interrupted => Ok(()),
            _ if cancel.is_cancelled() => Err(SubmissionFailure::Cancelled),
            _ => Err(SubmissionFailure::LaunchFailed),
        };
        let submission = run.submission.clone();
        let attempt = run.worker.attempt_id();
        assert_eq!(Arc::strong_count(&run.worker), 1);
        finish_ordinary_worker(&submission, attempt, run.worker, completion);
        (submission, result)
    }

    fn assert_quiescent(&self, submission: &OrdinarySubmission) {
        let snapshot = submission.snapshot();
        assert!(!snapshot.unavailable);
        let facts = snapshot.attempts.last().unwrap();
        assert!(facts.sealed);
        assert_eq!(facts.workers, 0);
        assert!(
            vega_store::thread_admission::try_acquire(self.store.database_identity(), &self.thread)
                .unwrap()
                .is_some()
        );
    }

    fn assert_user(&self, submission: &OrdinarySubmission, assistant_count: usize) {
        let rows = vega_store::context_compaction::load_source(self.store.conn(), &self.thread)
            .unwrap()
            .messages;
        let users = rows
            .iter()
            .filter(|row| row.role == "user")
            .collect::<Vec<_>>();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].id, submission.user_message_id());
        assert_eq!(users[0].content, ACCEPTED_TEXT);
        assert_eq!(users[0].status, "done");
        assert_eq!(
            rows.iter().filter(|row| row.role == "assistant").count(),
            assistant_count
        );
        let images =
            vega_store::image_attachments::for_messages(self.store.conn(), &self.thread, &rows)
                .unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].message_id, submission.user_message_id());
        assert_eq!(images[0].encoded.as_slice(), original_image().bytes());
    }

    fn assert_committed(&self, submission: &OrdinarySubmission) -> Arc<CommittedReceipt> {
        let snapshot = submission.snapshot();
        let CommitFact::Committed(receipt) = &snapshot.attempts.last().unwrap().commit else {
            panic!("real transaction must publish its committed receipt");
        };
        assert_eq!(receipt.user_message_id, submission.user_message_id());
        assert_eq!(receipt.persisted_text, ACCEPTED_TEXT);
        assert!(receipt.persisted_images == vec![original_image()]);
        assert_eq!(
            messages::find(self.store.conn(), &receipt.user_message_id)
                .unwrap()
                .unwrap()
                .seq,
            receipt.user_seq
        );
        assert_eq!(
            messages::find(self.store.conn(), &receipt.assistant_message_id)
                .unwrap()
                .unwrap()
                .seq,
            receipt.assistant_seq
        );
        receipt.clone()
    }

    fn assert_failed_empty(&self, assistant: &str) {
        let row = messages::find(self.store.conn(), assistant)
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "failed");
        assert!(row.content.is_empty());
    }

    async fn recover(&self, submission: OrdinarySubmission, old_assistant: Option<&str>) {
        let snapshot = submission.snapshot();
        let accepted = snapshot.accepted.unwrap();
        let authority = snapshot.authority.unwrap();
        let worker = submission
            .try_retry(snapshot.attempts.last().unwrap().id)
            .unwrap();
        assert!(Arc::ptr_eq(
            &accepted,
            &submission.snapshot().accepted.unwrap()
        ));
        assert!(Arc::ptr_eq(
            &authority,
            &submission.snapshot().authority.unwrap()
        ));
        let run = OrdinaryRun::new(submission.clone(), Arc::new(worker));
        let provider = EntryProvider::new(self, &submission, Drive::Complete);
        let (submission, result) = self
            .execute(run, &provider, CancellationToken::new(), None, |_| Ok(()))
            .await;
        let outcome = result.unwrap();
        assert!(!outcome.failed && !outcome.interrupted);
        assert_eq!(outcome.user_message_id, submission.user_message_id());
        assert_eq!(outcome.content, "answer");
        assert_eq!(provider.calls.lock().unwrap().len(), 1);
        self.assert_quiescent(&submission);
        let receipt = self.assert_committed(&submission);
        assert_eq!(receipt.assistant_message_id, outcome.assistant_message_id);
        assert_eq!(
            submission.snapshot().attempts.last().unwrap().primary,
            PrimaryFact::Started
        );
        if let Some(old_assistant) = old_assistant {
            assert_ne!(receipt.assistant_message_id, old_assistant);
            self.assert_failed_empty(old_assistant);
        }
        self.assert_user(&submission, if old_assistant.is_some() { 2 } else { 1 });
        let calls = provider.calls.lock().unwrap();
        let users = calls[0]
            .messages
            .iter()
            .filter(|message| message.role == vega_runtime::ChatRole::User)
            .collect::<Vec<_>>();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].content, ACCEPTED_TEXT);
        assert!(users[0].images == vec![original_image()]);
    }
}

impl Drop for EntryFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn original_image() -> ImageAttachment {
    let pixels = image::RgbaImage::from_pixel(1, 1, image::Rgba([17, 0, 0, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    ImageAttachment::from_bytes(bytes.into_inner()).unwrap()
}

struct NoPermission;

impl PermissionHook for NoPermission {
    fn request(
        &self,
        _: PermissionRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        Box::pin(async { panic!("ordinary entry regression must not request tool permission") })
    }
}

#[derive(Clone, Copy)]
enum Drive {
    Complete,
    Cancel,
    Fail,
}

struct EntryProvider {
    model: ResolvedModel,
    submission: OrdinarySubmission,
    calls: Mutex<Vec<ChatRequest>>,
    drive: Drive,
}

impl EntryProvider {
    fn new(fixture: &EntryFixture, submission: &OrdinarySubmission, drive: Drive) -> Self {
        Self {
            model: fixture.model.clone(),
            submission: submission.clone(),
            calls: Mutex::new(Vec::new()),
            drive,
        }
    }
}

impl Provider for EntryProvider {
    fn model_snapshot(&self) -> Option<ResolvedModel> {
        Some(self.model.clone())
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let snapshot = self.submission.snapshot();
        let facts = snapshot.attempts.last().unwrap();
        assert_eq!(facts.primary, PrimaryFact::Started);
        assert!(matches!(facts.commit, CommitFact::Committed(_)));
        self.calls.lock().unwrap().push(request);
        let drive = self.drive;
        Box::pin(async move {
            match drive {
                Drive::Complete => Ok(Box::pin(futures::stream::iter([
                    Ok(ProviderEvent::TextDelta("answer".into())),
                    Ok(ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    }),
                ])) as EventStream),
                Drive::Cancel => {
                    cancel.cancel();
                    Err(VegaError::Cancelled)
                }
                Drive::Fail => Err(VegaError::Io(std::io::Error::other("first drive failed"))),
            }
        })
    }
}

#[derive(Clone, Copy)]
enum Outlet {
    Message,
    Run,
}

fn fail_outlet(outlet: Outlet, event: &ConversationEvent) -> bool {
    matches!(
        (outlet, event),
        (Outlet::Message, ConversationEvent::MessageStarted { .. })
            | (Outlet::Run, ConversationEvent::RunStarted { .. })
    )
}

#[tokio::test]
async fn public_entry_outlet_failure_recovers_one_user_and_original_images() {
    for outlet in [Outlet::Message, Outlet::Run] {
        let fixture = EntryFixture::new();
        let run = fixture.begin();
        let old_assistant = run.submission.snapshot().attempts[0]
            .assistant_message_id
            .clone();
        let provider = EntryProvider::new(&fixture, &run.submission, Drive::Complete);
        let observed = run.submission.clone();
        let mut failed_outlets = 0;
        let (submission, result) = fixture
            .execute(run, &provider, CancellationToken::new(), None, |event| {
                if fail_outlet(outlet, event) {
                    fixture.assert_committed(&observed);
                    assert_eq!(
                        observed.snapshot().attempts[0].primary,
                        PrimaryFact::NotYetDriven
                    );
                    failed_outlets += 1;
                    return Err(VegaError::Io(std::io::Error::other("outlet unavailable")));
                }
                Ok(())
            })
            .await;
        assert!(result.is_err());
        assert_eq!(failed_outlets, 1);
        assert!(provider.calls.lock().unwrap().is_empty());
        fixture.assert_quiescent(&submission);
        fixture.assert_committed(&submission);
        fixture.assert_user(&submission, 1);
        fixture.assert_failed_empty(&old_assistant);
        let facts = submission.snapshot();
        assert_eq!(facts.attempts[0].primary, PrimaryFact::NotStarted);
        assert_eq!(facts.attempts[0].cleanup, CleanupFact::Succeeded);
        assert!(!facts.attempts[0].abnormal);
        assert_eq!(facts.retry, RetryEligibility::CommittedNotStarted);
        fixture.recover(submission, Some(&old_assistant)).await;
    }
}

#[tokio::test]
async fn public_entry_assistant_insert_rollback_retries_frozen_accepted_payload() {
    let fixture = EntryFixture::new();
    fixture.store.conn().execute_batch(
        "CREATE TRIGGER reject_assistant BEFORE INSERT ON messages WHEN NEW.role = 'assistant' BEGIN SELECT RAISE(ABORT, 'assistant insert rejected'); END;",
    ).unwrap();
    let run = fixture.begin();
    let provider = EntryProvider::new(&fixture, &run.submission, Drive::Complete);
    let mut started = 0;
    let (submission, result) = fixture
        .execute(run, &provider, CancellationToken::new(), None, |event| {
            if matches!(
                event,
                ConversationEvent::MessageStarted { .. } | ConversationEvent::RunStarted { .. }
            ) {
                started += 1;
            }
            Ok(())
        })
        .await;
    assert!(result.is_err());
    assert_eq!(started, 0);
    assert!(provider.calls.lock().unwrap().is_empty());
    fixture.assert_quiescent(&submission);
    assert!(
        vega_store::context_compaction::load_source(fixture.store.conn(), &fixture.thread)
            .unwrap()
            .messages
            .is_empty()
    );
    let image_count: i64 = fixture
        .store
        .conn()
        .query_row("SELECT COUNT(*) FROM image_attachments", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(image_count, 0);
    let snapshot = submission.snapshot();
    assert!(matches!(
        snapshot.attempts[0].commit,
        CommitFact::NotCommitted
    ));
    assert_eq!(snapshot.attempts[0].primary, PrimaryFact::NotStarted);
    assert_eq!(snapshot.attempts[0].cleanup, CleanupFact::NotRequired);
    assert_eq!(snapshot.retry, RetryEligibility::Uncommitted);
    fixture
        .store
        .conn()
        .execute_batch("DROP TRIGGER reject_assistant;")
        .unwrap();
    fixture.recover(submission, None).await;
}

#[tokio::test]
async fn public_entry_run_started_cancellation_retains_commit_and_allows_safe_recovery() {
    let fixture = EntryFixture::new();
    let run = fixture.begin();
    let old_assistant = run.submission.snapshot().attempts[0]
        .assistant_message_id
        .clone();
    let provider = EntryProvider::new(&fixture, &run.submission, Drive::Complete);
    let cancel = CancellationToken::new();
    let callback_cancel = cancel.clone();
    let mut clocks = 0;
    let (submission, result) = fixture
        .execute(run, &provider, cancel, None, |event| {
            if matches!(event, ConversationEvent::RunStarted { .. }) {
                clocks += 1;
                callback_cancel.cancel();
            }
            Ok(())
        })
        .await;
    assert!(result.is_err());
    assert_eq!(clocks, 1);
    assert!(provider.calls.lock().unwrap().is_empty());
    fixture.assert_quiescent(&submission);
    fixture.assert_committed(&submission);
    fixture.assert_user(&submission, 1);
    fixture.assert_failed_empty(&old_assistant);
    let snapshot = submission.snapshot();
    assert_eq!(snapshot.attempts[0].primary, PrimaryFact::NotStarted);
    assert_eq!(snapshot.attempts[0].cleanup, CleanupFact::Succeeded);
    assert_eq!(snapshot.retry, RetryEligibility::CommittedNotStarted);
    fixture.recover(submission, Some(&old_assistant)).await;
}

#[tokio::test]
async fn public_entry_empty_first_drive_cancellation_or_error_remains_started() {
    for drive in [Drive::Cancel, Drive::Fail] {
        let fixture = EntryFixture::new();
        let run = fixture.begin();
        let provider = EntryProvider::new(&fixture, &run.submission, drive);
        let (submission, result) = fixture
            .execute(run, &provider, CancellationToken::new(), None, |_| Ok(()))
            .await;
        let outcome = result.unwrap();
        assert!(outcome.content.is_empty());
        match drive {
            Drive::Cancel => assert!(outcome.interrupted && !outcome.failed),
            Drive::Fail => assert!(outcome.failed && !outcome.interrupted),
            Drive::Complete => unreachable!(),
        }
        assert_eq!(provider.calls.lock().unwrap().len(), 1);
        fixture.assert_quiescent(&submission);
        fixture.assert_committed(&submission);
        fixture.assert_user(&submission, 1);
        assert!(
            messages::find(fixture.store.conn(), &outcome.assistant_message_id)
                .unwrap()
                .unwrap()
                .content
                .is_empty()
        );
        let snapshot = submission.snapshot();
        assert_eq!(snapshot.attempts[0].primary, PrimaryFact::Started);
        assert_eq!(
            snapshot.retry,
            RetryEligibility::Blocked(RetryBlocker::PrimaryStarted)
        );
        assert!(submission.try_retry(snapshot.attempts[0].id).is_err());
    }
}

#[tokio::test]
async fn public_entry_actor_open_failure_proves_cleanup_before_recovery() {
    let fixture = EntryFixture::new();
    let run = fixture.begin();
    let old_assistant = run.submission.snapshot().attempts[0]
        .assistant_message_id
        .clone();
    let provider = EntryProvider::new(&fixture, &run.submission, Drive::Complete);
    let mut started = 0;
    let (submission, result) = fixture
        .execute(
            run,
            &provider,
            CancellationToken::new(),
            Some(OrdinaryEntryFault::ActorOpen),
            |event| {
                if matches!(
                    event,
                    ConversationEvent::MessageStarted { .. } | ConversationEvent::RunStarted { .. }
                ) {
                    started += 1;
                }
                Ok(())
            },
        )
        .await;
    assert!(result.is_err());
    assert_eq!(started, 0);
    assert!(provider.calls.lock().unwrap().is_empty());
    fixture.assert_quiescent(&submission);
    fixture.assert_committed(&submission);
    fixture.assert_user(&submission, 1);
    fixture.assert_failed_empty(&old_assistant);
    let snapshot = submission.snapshot();
    assert_eq!(snapshot.attempts[0].primary, PrimaryFact::NotStarted);
    assert_eq!(snapshot.attempts[0].cleanup, CleanupFact::Succeeded);
    assert_eq!(snapshot.retry, RetryEligibility::CommittedNotStarted);
    fixture.recover(submission, Some(&old_assistant)).await;
}

#[tokio::test]
async fn public_entry_persisted_error_without_ack_blocks_recovery() {
    let fixture = EntryFixture::new();
    let run = fixture.begin();
    let assistant = run.submission.snapshot().attempts[0]
        .assistant_message_id
        .clone();
    let provider = EntryProvider::new(&fixture, &run.submission, Drive::Complete);
    let mut outlets = 0;
    let (submission, result) = fixture
        .execute(
            run,
            &provider,
            CancellationToken::new(),
            Some(OrdinaryEntryFault::EventAckLost),
            |event| {
                if matches!(event, ConversationEvent::MessageStarted { .. }) {
                    outlets += 1;
                    return Err(VegaError::Io(std::io::Error::other("outlet unavailable")));
                }
                Ok(())
            },
        )
        .await;
    assert!(result.is_err());
    assert_eq!(outlets, 1);
    assert!(provider.calls.lock().unwrap().is_empty());
    fixture.assert_quiescent(&submission);
    fixture.assert_committed(&submission);
    fixture.assert_user(&submission, 1);
    fixture.assert_failed_empty(&assistant);
    let snapshot = submission.snapshot();
    assert_eq!(snapshot.attempts[0].primary, PrimaryFact::NotStarted);
    assert_eq!(snapshot.attempts[0].cleanup, CleanupFact::Failed);
    assert_eq!(
        snapshot.retry,
        RetryEligibility::Blocked(RetryBlocker::CleanupIncomplete)
    );
    assert!(submission.try_retry(snapshot.attempts[0].id).is_err());
}

#[tokio::test]
async fn public_entry_committed_receipt_loss_retains_durable_user_and_blocks_retry() {
    let fixture = EntryFixture::new();
    let run = fixture.begin();
    let provider = EntryProvider::new(&fixture, &run.submission, Drive::Complete);
    let mut started = 0;
    let (submission, result) = fixture
        .execute(
            run,
            &provider,
            CancellationToken::new(),
            Some(OrdinaryEntryFault::CommitReceiptLost),
            |event| {
                if matches!(
                    event,
                    ConversationEvent::MessageStarted { .. } | ConversationEvent::RunStarted { .. }
                ) {
                    started += 1;
                }
                Ok(())
            },
        )
        .await;
    assert!(result.is_err());
    assert_eq!(started, 0);
    assert!(provider.calls.lock().unwrap().is_empty());
    fixture.assert_quiescent(&submission);
    fixture.assert_user(&submission, 1);
    let snapshot = submission.snapshot();
    let facts = &snapshot.attempts[0];
    assert!(matches!(facts.commit, CommitFact::Unknown));
    assert_eq!(facts.primary, PrimaryFact::Unknown);
    assert_eq!(facts.cleanup, CleanupFact::Unknown);
    assert_eq!(
        snapshot.retry,
        RetryEligibility::Blocked(RetryBlocker::Unknown)
    );
    assert!(submission.try_retry(facts.id).is_err());
    assert!(matches!(
        super::observe_ordinary_history(&fixture.store, &submission, facts.id).unwrap(),
        super::OrdinaryHistoryObservation::MatchingUser
    ));
    assert_eq!(
        submission.snapshot().retry,
        RetryEligibility::Blocked(RetryBlocker::Unknown)
    );
    fixture.assert_user(&submission, 1);
}
