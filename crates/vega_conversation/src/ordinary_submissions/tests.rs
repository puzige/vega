use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use ulid::Ulid;
use vega_store::Store;

use crate::types::PermissionFence;
use vega_store::thread_admission::{self, DatabaseIdentity, try_pause_database};

use super::{
    AttemptId, CleanupFact, CommitFact, CommittedReceipt, OrdinarySubmission, OrdinaryWorker,
    PersistedPayload, RawSubmission, RetryBlocker, RetryEligibility, SubmissionError,
    SubmissionFailure, invalidate_database_recovery,
};

fn begin(database: DatabaseIdentity, thread: &str) -> (OrdinarySubmission, OrdinaryWorker) {
    let admission = thread_admission::try_acquire(database, thread)
        .expect("registry available")
        .expect("isolated owner");
    OrdinarySubmission::begin(
        database,
        thread.into(),
        RawSubmission::new(
            "original input".into(),
            Vec::new(),
            1,
            "synthetic".into(),
            None,
            None,
        ),
        admission,
    )
    .expect("begin submission")
}

fn failed(
    database: DatabaseIdentity,
    thread: &str,
    committed: bool,
) -> (OrdinarySubmission, AttemptId) {
    let (submission, worker) = begin(database, thread);
    let attempt = worker.attempt_id();
    worker
        .bind_accepted_payload("accepted input".into())
        .expect("accepted payload");
    if committed {
        worker
            .stage_permission_fence(PermissionFence {
                schema_version: 1,
                execution_policy_version: 1,
                authority_epoch: Ulid::generate().to_string(),
                database_identity: database.permission_fence_key(),
                thread_id: thread.into(),
                project_id: "synthetic-project".into(),
                workspace: "/synthetic-workspace".into(),
                run_mode: "execute".into(),
                permission_mode: "confirm".into(),
                permission_revision: 0,
                exact_rules: Vec::new(),
                full_access_grant: None,
                delegation: None,
            })
            .expect("accepted permission fence")
            .publish();
        worker
            .bind_persisted_payload(PersistedPayload::new("persisted input".into(), Vec::new()))
            .expect("persisted payload");
        worker.begin_commit().expect("commit transition");
        worker
            .record_committed(CommittedReceipt {
                user_message_id: submission.user_message_id().into(),
                user_seq: 1,
                assistant_message_id: submission.snapshot().attempts[0]
                    .assistant_message_id
                    .clone(),
                assistant_seq: 2,
                next_seq: 3,
                persisted_text: "persisted input".into(),
                persisted_images: Vec::new(),
            })
            .expect("retained commit receipt");
        worker
            .record_cleanup(Ok(()))
            .expect("pipeline cleanup fact");
    }
    worker
        .complete(Err(SubmissionFailure::PreparationFailed))
        .expect("worker ends");
    submission.seal(attempt).expect("seal attempt");
    assert_eq!(
        submission.snapshot().retry,
        if committed {
            RetryEligibility::CommittedNotStarted
        } else {
            RetryEligibility::Uncommitted
        }
    );
    (submission, attempt)
}

#[test]
fn database_invalidation_blocks_old_retries_without_losing_submission_evidence() {
    let store = Store::open(":memory:").expect("isolated store");
    let other_store = Store::open(":memory:").expect("other isolated store");
    let database = store.database_identity();
    let other_database = other_store.database_identity();
    let thread = Ulid::generate().to_string();
    let (original, attempt) = failed(database, &thread, true);
    let fence = original
        .permission_fence()
        .expect("original permission fence");
    let (other, other_attempt) = failed(other_database, "thread", false);
    let before = original.snapshot();
    let pause = try_pause_database(database)
        .expect("pause registry")
        .expect("database pause");
    assert!(pause.verifies(database).expect("matching drained pause"));
    invalidate_database_recovery(database, &pause).expect("acknowledged invalidation");
    invalidate_database_recovery(database, &pause).expect("idempotent invalidation");
    let after = original.snapshot();
    assert_eq!(
        after.retry,
        RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated)
    );
    assert!(Arc::ptr_eq(&after.raw, &before.raw));
    assert_eq!(after.raw.text(), "original input");
    assert_eq!(
        after.accepted.as_ref().expect("accepted retained").text(),
        "accepted input"
    );
    assert_eq!(after.attempts.len(), before.attempts.len());
    assert_eq!(after.attempts[0].id, before.attempts[0].id);
    assert_eq!(after.attempts[0].commit, before.attempts[0].commit);
    assert_eq!(after.attempts[0].persisted, before.attempts[0].persisted);
    assert_eq!(
        original
            .permission_fence()
            .expect("retained permission fence"),
        fence
    );
    assert_eq!(after.attempts[0].cleanup, CleanupFact::Succeeded);
    assert_eq!(after.attempts[0].failure, before.attempts[0].failure);
    assert!(matches!(
        original.try_retry(attempt),
        Err(SubmissionError::RetryBlocked(
            RetryBlocker::PermissionInvalidated
        ))
    ));
    assert_eq!(other.snapshot().retry, RetryEligibility::Uncommitted);
    let other_retry = other
        .try_retry(other_attempt)
        .expect("other database usable");
    let other_retry_id = other_retry.attempt_id();
    other_retry
        .complete(Err(SubmissionFailure::Cancelled))
        .expect("end other retry");
    other.seal(other_retry_id).expect("seal other retry");
    drop(pause);
    assert!(matches!(
        original.try_retry(attempt),
        Err(SubmissionError::RetryBlocked(
            RetryBlocker::PermissionInvalidated
        ))
    ));
    let (fresh, fresh_attempt) = failed(database, &thread, false);
    let fresh_retry = fresh
        .try_retry(fresh_attempt)
        .expect("fresh submission stays usable");
    let fresh_retry_id = fresh_retry.attempt_id();
    fresh_retry
        .complete(Err(SubmissionFailure::Cancelled))
        .expect("end fresh retry");
    fresh.seal(fresh_retry_id).expect("seal fresh retry");
}

#[test]
fn invalidation_refuses_wrong_database_and_active_owners() {
    let store = Store::open(":memory:").expect("isolated store");
    let other = Store::open(":memory:").expect("other store");
    let database = store.database_identity();
    let (submission, attempt) = failed(database, "thread", false);
    let wrong_pause = try_pause_database(other.database_identity())
        .expect("pause")
        .expect("other pause");
    assert!(matches!(
        invalidate_database_recovery(database, &wrong_pause),
        Err(SubmissionError::Busy)
    ));
    assert_eq!(submission.snapshot().retry, RetryEligibility::Uncommitted);
    let retry = submission.try_retry(attempt).expect("active retry");
    let retry_id = retry.attempt_id();
    let pause = try_pause_database(database)
        .expect("pause")
        .expect("own pause");
    assert!(matches!(
        invalidate_database_recovery(database, &pause),
        Err(SubmissionError::Busy)
    ));
    assert!(
        !submission
            .0
            .state
            .lock()
            .expect("state")
            .permission_invalidated
    );
    retry
        .complete(Err(SubmissionFailure::Cancelled))
        .expect("owner completion");
    submission.seal(retry_id).expect("owner sealed");
    invalidate_database_recovery(database, &pause).expect("drained invalidation");
    assert_eq!(
        submission.snapshot().retry,
        RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated)
    );
}

#[test]
fn poisoned_submission_refuses_acknowledgment_without_partial_invalidation() {
    let store = Store::open(":memory:").expect("isolated store");
    let database = store.database_identity();
    let (healthy, _) = failed(database, "thread", false);
    let (poisoned, _) = failed(database, "thread", false);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _guard = poisoned.0.state.lock().expect("fixture lock");
            panic!("isolated submission poison");
        }))
        .is_err()
    );
    let pause = try_pause_database(database)
        .expect("pause")
        .expect("database pause");
    assert!(matches!(
        invalidate_database_recovery(database, &pause),
        Err(SubmissionError::Unavailable)
    ));
    assert_eq!(healthy.snapshot().retry, RetryEligibility::Uncommitted);
    assert!(
        !healthy
            .0
            .state
            .lock()
            .expect("healthy state")
            .permission_invalidated
    );
    assert!(poisoned.snapshot().unavailable);
}

#[test]
fn invalidation_preserves_unknown_effects_instead_of_marking_them_complete() {
    let store = Store::open(":memory:").expect("isolated store");
    let database = store.database_identity();
    let (submission, worker) = begin(database, "thread");
    let attempt = worker.attempt_id();
    drop(worker);
    submission.seal(attempt).expect("seal lost worker");
    let before = submission.snapshot();
    assert!(before.attempts[0].abnormal);
    assert_eq!(before.attempts[0].commit, CommitFact::Unknown);
    let pause = try_pause_database(database)
        .expect("pause")
        .expect("database pause");
    invalidate_database_recovery(database, &pause).expect("invalidate retry only");
    let after = submission.snapshot();
    assert_eq!(after.attempts[0].commit, before.attempts[0].commit);
    assert_eq!(after.attempts[0].cleanup, before.attempts[0].cleanup);
    assert_eq!(after.attempts[0].primary, before.attempts[0].primary);
    assert_eq!(after.attempts[0].failure, before.attempts[0].failure);
    assert!(after.attempts[0].abnormal);
    assert_eq!(
        after.retry,
        RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated)
    );
}
