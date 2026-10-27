use super::*;
use crate::types::{
    PendingMessage, PendingMessageBehavior, PlanReviewAction, PlanReviewOutcome,
    RunCredentialReader,
};
use vega_runtime::RuntimeSteeringSource;

fn queued(fixture: &Fixture, behavior: PendingMessageBehavior) -> PendingMessage {
    let source = crate::threads::get_thread(&fixture.store, &fixture.thread).expect("queue source");
    let message = PendingMessage {
        id: ulid::Ulid::generate().to_string(),
        thread_id: fixture.thread.clone(),
        generation: "fixture-generation".into(),
        behavior,
        content: "retained queued input".into(),
        images: Vec::new(),
        references: Vec::new(),
        reference_mentions: Vec::new(),
        permission_fence: Some(
            crate::pending_messages::capture_authority(&fixture.store, &source)
                .expect("enqueue source"),
        ),
    };
    crate::pending_messages::enqueue(&fixture.store, &message).expect("persist accepted queue");
    message
}

fn counted(fixture: &Fixture, reads: &Arc<AtomicUsize>) -> PersistenceActorConfig {
    let reads = reads.clone();
    let reader = RunCredentialReader::new(
        fixture.database.clone(),
        fixture.thread.clone(),
        Some(fixture.root.clone()),
        Vec::new(),
        Arc::new(move || {
            reads.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }),
    );
    PersistenceActorConfig::default().with_run_credential_reader(reader)
}

fn prepare_pending(
    fixture: &Fixture,
    message: &PendingMessage,
    config: PersistenceActorConfig,
) -> Result<PreparedRun, ConversationError> {
    prepare_run_with_images_and_reasoning(
        fixture.database.clone(),
        fixture.thread.clone(),
        message.content.clone(),
        "synthetic system".into(),
        message.id.clone(),
        ulid::Ulid::generate().to_string(),
        config.with_pending_input(Some(message.id.clone()), None),
        false,
        None,
        None,
        message.images.clone(),
        None,
    )
}

#[test]
fn queued_input_cannot_acquire_future_full_access_or_normalize_history_before_rejection() {
    let fixture = Fixture::new();
    let message = queued(&fixture, PendingMessageBehavior::Queue);
    let accepted = crate::pending_messages::prepare_dispatch(
        &fixture.store,
        &fixture.thread,
        &message.id,
        &message.content,
        &message.images,
        &message.references,
    )
    .expect("original dispatch proof");
    fixture.store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at) VALUES('unfinished',?1,1,'assistant','text','retained partial','streaming',1)", [&fixture.thread]).expect("unsettled history");
    let owner = vega_store::thread_admission::try_acquire(
        fixture.store.database_identity(),
        &fixture.thread,
    )
    .expect("admission registry")
    .expect("original active owner");
    let source =
        crate::threads::get_thread(&fixture.store, &fixture.thread).expect("old future setting");
    let service = crate::thread_permissions::ThreadPermissionService::new(fixture.database.clone());
    let change = service
        .prepare(&source, PermissionMode::FullAccess)
        .expect("future full-access confirmation");
    service
        .save(&change, true)
        .expect("future permission saved while old run owns admission");
    drop(owner);
    let before = fixture.rows();
    let reads = Arc::new(AtomicUsize::new(0));
    let result = prepare_pending(
        &fixture,
        &message,
        counted(&fixture, &reads).with_accepted_pending_input(Some(accepted)),
    );
    assert!(matches!(
        result,
        Err(ConversationError::InvalidTaskIdentity)
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture.rows(),
        before,
        "reject before history recovery or assistant claim"
    );
    assert!(
        crate::pending_messages::prepare_dispatch(
            &fixture.store,
            &fixture.thread,
            &message.id,
            &message.content,
            &message.images,
            &message.references,
        )
        .is_err()
    );
    let retained = crate::pending_messages::remove(&fixture.store, &fixture.thread, &message.id)
        .expect("explicit edit can recover stale input");
    assert_eq!(retained.content, message.content);
    assert_eq!(retained.permission_fence, message.permission_fence);
}

#[test]
fn valid_queue_uses_its_saved_authority_and_legacy_payload_is_recoverable_but_not_dispatchable() {
    for legacy in [false, true] {
        let fixture = Fixture::new();
        let message = queued(&fixture, PendingMessageBehavior::Queue);
        assert!(
            crate::pending_messages::prepare_dispatch(
                &fixture.store,
                &fixture.thread,
                &message.id,
                &message.content,
                &message.images,
                &["unaccepted-reference.txt".into()]
            )
            .is_err()
        );
        if legacy {
            fixture.store.conn().execute("UPDATE pending_messages SET payload=json_remove(payload,'$.permission_fence') WHERE id=?1", [&message.id]).expect("pre-contract payload");
            assert!(
                crate::pending_messages::prepare_dispatch(
                    &fixture.store,
                    &fixture.thread,
                    &message.id,
                    &message.content,
                    &message.images,
                    &message.references,
                )
                .is_err()
            );
            assert!(matches!(
                prepare_pending(&fixture, &message, PersistenceActorConfig::default()),
                Err(ConversationError::InvalidTaskIdentity)
            ));
            let recovered =
                crate::pending_messages::remove(&fixture.store, &fixture.thread, &message.id)
                    .expect("legacy input remains editable");
            assert_eq!(recovered.content, message.content);
            assert!(recovered.permission_fence.is_none());
        } else {
            let accepted = crate::pending_messages::prepare_dispatch(
                &fixture.store,
                &fixture.thread,
                &message.id,
                &message.content,
                &message.images,
                &message.references,
            )
            .expect("valid dispatch");
            let run = prepare_pending(
                &fixture,
                &message,
                PersistenceActorConfig::default().with_accepted_pending_input(Some(accepted)),
            )
            .expect("exact queued source starts");
            assert_eq!(
                Some(&run.permission_fence),
                message.permission_fence.as_ref()
            );
            assert_eq!(run.user_message_id, message.id);
        }
    }
}

#[tokio::test]
async fn steering_rejects_an_old_queue_before_references_credentials_or_claim() {
    let fixture = Fixture::new();
    let message = queued(&fixture, PendingMessageBehavior::Steer);
    crate::permission_contracts::change_thread_permission(
        &fixture.store,
        &fixture.thread,
        0,
        PermissionMode::FullAccess,
        true,
        2,
    )
    .expect("future authority changed");
    let source = crate::threads::get_thread(&fixture.store, &fixture.thread)
        .expect("new full-access thread");
    let fence = crate::pending_messages::capture_authority(&fixture.store, &source)
        .expect("accepted new run");
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = reads.clone();
    let source = crate::pending_messages::QueueSteeringSource::new(
        fixture.database.clone(),
        fixture.root.clone(),
        fixture.thread.clone(),
        message.generation.clone(),
        Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }),
        tokio_util::sync::CancellationToken::new(),
    );
    assert!(source.authority_slot().set(fence).is_ok());
    let before = fixture.rows();
    assert!(source.take().await.is_err());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.rows(), before);
    assert!(
        vega_store::pending_messages::find_dispatchable(
            fixture.store.conn(),
            &fixture.thread,
            &message.id
        )
        .expect("original pending row")
        .is_some()
    );
}

#[test]
fn plan_approval_is_atomic_and_recovery_never_borrows_later_permissions() {
    for changed in [false, true] {
        let fixture = Fixture::new();
        fixture
            .store
            .conn()
            .execute(
                "UPDATE threads SET mode='plan' WHERE id=?1",
                [&fixture.thread],
            )
            .expect("plan mode");
        fixture.store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at,plan_status) VALUES('plan',?1,1,'assistant','plan','retained approved plan','done',1,'pending')", [&fixture.thread]).expect("pending plan");
        fixture.store.conn().execute_batch("CREATE TRIGGER refuse_approval_fence BEFORE INSERT ON plan_instruction_authority BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").expect("atomicity fault");
        let before = fixture.rows();
        assert!(
            crate::plans::review_plan(
                &fixture.store,
                &fixture.thread,
                "plan",
                PlanReviewAction::Approve
            )
            .is_err()
        );
        assert_eq!(fixture.rows(), before);
        assert_eq!(
            crate::threads::get_thread(&fixture.store, &fixture.thread)
                .expect("rolled back mode")
                .mode,
            crate::types::ThreadMode::Plan
        );
        fixture
            .store
            .conn()
            .execute_batch("DROP TRIGGER refuse_approval_fence")
            .expect("remove fault");
        let PlanReviewOutcome::Applied {
            instruction_message_id: Some(instruction),
        } = crate::plans::review_plan(
            &fixture.store,
            &fixture.thread,
            "plan",
            PlanReviewAction::Approve,
        )
        .expect("approve with authority")
        else {
            panic!("approval winner");
        };
        assert_eq!(
            crate::plans::recoverable_approved_instruction(&fixture.store, &fixture.thread)
                .expect("original recovery"),
            Some(instruction.clone())
        );
        if changed {
            crate::permission_contracts::change_thread_permission(
                &fixture.store,
                &fixture.thread,
                0,
                PermissionMode::FullAccess,
                true,
                2,
            )
            .expect("later full access");
        }
        let before = fixture.rows();
        let reads = Arc::new(AtomicUsize::new(0));
        let result = prepare_run_with_images_and_reasoning(
            fixture.database.clone(),
            fixture.thread.clone(),
            crate::plans::APPROVAL_INSTRUCTION.into(),
            "synthetic system".into(),
            instruction.clone(),
            ulid::Ulid::generate().to_string(),
            counted(&fixture, &reads),
            true,
            None,
            None,
            Vec::new(),
            None,
        );
        if changed {
            assert!(matches!(
                result,
                Err(ConversationError::InvalidTaskIdentity)
            ));
            assert_eq!(reads.load(Ordering::SeqCst), 0);
            assert_eq!(fixture.rows(), before);
            assert_eq!(
                crate::plans::recoverable_approved_instruction(&fixture.store, &fixture.thread)
                    .expect("stale approval preserved"),
                None
            );
        } else {
            let run = result.expect("exact approval authority");
            assert_eq!(run.permission_fence.permission_mode, "confirm");
            assert_eq!(run.user_message_id, instruction);
        }
    }
}
