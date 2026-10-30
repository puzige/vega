use std::sync::Arc;

use crate::types::{
    AttemptId, AttemptKind, CleanupFailure, CommittedReceipt, ConversationError,
    OrdinarySubmission, OrdinaryWorker, PersistedPayload, SubmissionError, SubmissionFailure,
    ThreadAdmission,
};
use vega_store::{Connection, Store, messages};

#[derive(Clone)]
pub struct OrdinaryRun {
    pub submission: OrdinarySubmission,
    pub worker: Arc<OrdinaryWorker>,
    lifetime: Arc<OrdinaryLifetime>,
    pub(crate) thread: Option<crate::types::Thread>,
}

struct OrdinaryLifetime {
    submission: OrdinarySubmission,
    attempt: AttemptId,
}

impl Drop for OrdinaryLifetime {
    fn drop(&mut self) {
        let _ = self.submission.seal(self.attempt);
    }
}

pub(crate) struct OrdinaryTask {
    worker: Option<OrdinaryWorker>,
    submission: OrdinarySubmission,
    attempt: AttemptId,
}

impl OrdinaryTask {
    pub(crate) fn register(run: Option<&OrdinaryRun>) -> Result<Option<Self>, ConversationError> {
        run.map(|run| {
            Ok(Self {
                worker: Some(run.worker.register_worker().map_err(fact_error)?),
                submission: run.submission.clone(),
                attempt: run.worker.attempt_id(),
            })
        })
        .transpose()
    }

    pub(crate) fn complete(mut self, result: Result<(), SubmissionFailure>) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.complete(result);
        }
    }
}

impl Drop for OrdinaryTask {
    fn drop(&mut self) {
        drop(self.worker.take());
        if self
            .submission
            .snapshot()
            .attempts
            .last()
            .is_some_and(|attempt| attempt.workers == 0)
        {
            let _ = self.submission.seal(self.attempt);
        }
    }
}

impl OrdinaryRun {
    pub fn new(submission: OrdinarySubmission, worker: Arc<OrdinaryWorker>) -> Self {
        let lifetime = Arc::new(OrdinaryLifetime {
            submission: submission.clone(),
            attempt: worker.attempt_id(),
        });
        Self {
            submission,
            worker,
            lifetime,
            thread: None,
        }
    }

    pub fn with_thread_snapshot(mut self, thread: crate::types::Thread) -> Self {
        self.thread = Some(thread);
        self
    }

    pub(crate) fn verify(&self, store: &Store, thread: &str) -> Result<(), ConversationError> {
        crate::thread_access::verify(store, thread, self.worker.admission())?;
        let _ = &self.lifetime;
        let snapshot = self.submission.snapshot();
        if snapshot.unavailable
            || snapshot.thread_id != thread
            || snapshot.attempts.last().map(|attempt| attempt.id) != Some(self.worker.attempt_id())
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        Ok(())
    }

    pub(crate) fn recovery(&self) -> Result<Option<Arc<CommittedReceipt>>, ConversationError> {
        let snapshot = self.submission.snapshot();
        let attempt = snapshot
            .attempts
            .last()
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        if snapshot.unavailable || attempt.id != self.worker.attempt_id() {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        Ok(match &attempt.kind {
            AttemptKind::CommittedRecovery(receipt) => Some(receipt.clone()),
            _ => None,
        })
    }

    pub(crate) fn assistant_id(&self) -> Result<String, ConversationError> {
        self.submission
            .snapshot()
            .attempts
            .last()
            .filter(|attempt| attempt.id == self.worker.attempt_id())
            .map(|attempt| attempt.assistant_message_id.clone())
            .ok_or(ConversationError::InvalidTaskIdentity)
    }
}

pub(crate) struct OrdinaryPreparation {
    pub worker: OrdinaryWorker,
    pub run: OrdinaryRun,
}

impl OrdinaryPreparation {
    pub(crate) fn register(run: &OrdinaryRun) -> Result<Self, ConversationError> {
        Ok(Self {
            run: run.clone(),
            worker: run.worker.register_worker().map_err(fact_error)?,
        })
    }

    pub(crate) fn bind_persisted(
        &self,
        conn: &Connection,
    ) -> Result<CommittedReceipt, ConversationError> {
        let user = messages::find(conn, self.run.submission.user_message_id())
            .map_err(super::runtime_store_error)?
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        let assistant = messages::find(conn, &self.run.assistant_id()?)
            .map_err(super::runtime_store_error)?
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        let images = persisted_images(conn, &user.id)?;
        let receipt = CommittedReceipt {
            user_message_id: user.id,
            user_seq: user.seq,
            assistant_message_id: assistant.id,
            assistant_seq: assistant.seq,
            next_seq: messages::next_seq(conn, &user.thread_id)
                .map_err(super::runtime_store_error)?,
            persisted_text: user.content,
            persisted_images: images,
        };
        self.worker
            .bind_persisted_payload(PersistedPayload::new(
                receipt.persisted_text.clone(),
                receipt.persisted_images.clone(),
            ))
            .map_err(fact_error)?;
        Ok(receipt)
    }

    pub(crate) fn complete(self, result: Result<(), &ConversationError>) {
        let _ = self.worker.complete(result.map_err(|error| {
            if matches!(error, ConversationError::Runtime(error) if matches!(error.as_ref(), vega_runtime::VegaError::Cancelled)) {
                SubmissionFailure::Cancelled
            } else {
                SubmissionFailure::Preparation(crate::submission_diagnostics::conversation_failure(
                    crate::submission_diagnostics::PreparationStage::Conversation, error,
                ))
            }
        }));
    }
}

pub(crate) fn fact_error(_: SubmissionError) -> ConversationError {
    ConversationError::Store(vega_i18n::t("domain.ordinary_send.facts_unavailable").into())
}

pub(crate) fn record_cleanup(run: Option<&OrdinaryRun>, succeeded: bool) {
    if let Some(run) = run {
        let _ = run.worker.record_cleanup(if succeeded {
            Ok(())
        } else {
            Err(CleanupFailure::Failed)
        });
    }
}

pub(crate) fn persisted_images(
    conn: &Connection,
    message: &str,
) -> Result<Vec<crate::types::ImageAttachment>, ConversationError> {
    let user = messages::find(conn, message)
        .map_err(super::runtime_store_error)?
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    vega_store::image_attachments::for_messages(conn, &user.thread_id, std::slice::from_ref(&user))
        .map_err(super::runtime_store_error)?
        .into_iter()
        .enumerate()
        .map(|(ordinal, row)| {
            if row.ordinal != ordinal as i64 || row.artifact_call_id.is_some() {
                return Err(ConversationError::InvalidTaskIdentity);
            }
            crate::types::ImageAttachment::from_bytes(row.encoded)
                .map_err(|_| ConversationError::InvalidTaskIdentity)
        })
        .collect()
}

pub(crate) fn verify_recovery(
    conn: &Connection,
    run: &OrdinaryRun,
    thread: &str,
    receipt: &CommittedReceipt,
) -> Result<(), ConversationError> {
    let mismatch =
        || ConversationError::Store(vega_i18n::t("domain.ordinary_send.history_changed").into());
    let user = messages::find(conn, &receipt.user_message_id)
        .map_err(super::runtime_store_error)?
        .ok_or_else(mismatch)?;
    let assistant = messages::find(conn, &receipt.assistant_message_id)
        .map_err(super::runtime_store_error)?
        .ok_or_else(mismatch)?;
    if user.id != run.submission.user_message_id()
        || user.thread_id != thread
        || user.role != "user"
        || user.kind != "text"
        || user.status != "done"
        || user.seq != receipt.user_seq
        || user.content != receipt.persisted_text
        || persisted_images(conn, &user.id)? != receipt.persisted_images
        || assistant.thread_id != thread
        || assistant.role != "assistant"
        || assistant.kind != "text"
        || assistant.status != "failed"
        || assistant.seq != receipt.assistant_seq
        || !assistant.content.is_empty()
        || messages::next_seq(conn, thread).map_err(super::runtime_store_error)? != receipt.next_seq
    {
        let _ = run.submission.invalidate_recovery(run.worker.attempt_id());
        return Err(mismatch());
    }
    let effects: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tool_calls WHERE message_id = ?1) OR EXISTS(SELECT 1 FROM assistant_responses WHERE message_id = ?1)",
        [&receipt.assistant_message_id], |row| row.get(0),
    ).map_err(super::runtime_store_error)?;
    if effects {
        let _ = run.submission.invalidate_recovery(run.worker.attempt_id());
        return Err(mismatch());
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub enum OrdinaryHistoryObservation {
    Absent,
    MatchingUser,
    Conflict,
}

pub fn observe_ordinary_history(
    store: &Store,
    submission: &OrdinarySubmission,
    attempt: AttemptId,
) -> Result<OrdinaryHistoryObservation, ConversationError> {
    let snapshot = submission.snapshot();
    let facts = snapshot
        .attempts
        .last()
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    if snapshot.database != store.database_identity() || facts.id != attempt || facts.workers != 0 {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let transaction = store
        .conn()
        .unchecked_transaction()
        .map_err(super::runtime_store_error)?;
    let Some(user) = messages::find(&transaction, submission.user_message_id())
        .map_err(super::runtime_store_error)?
    else {
        return Ok(OrdinaryHistoryObservation::Absent);
    };
    let payload = facts.persisted.as_deref();
    let matches = user.thread_id == snapshot.thread_id
        && user.role == "user"
        && user.kind == "text"
        && user.status == "done"
        && payload.is_some_and(|payload| user.content == payload.text())
        && payload.is_some_and(|payload| {
            persisted_images(&transaction, &user.id).is_ok_and(|images| images == payload.images())
        });
    transaction.commit().map_err(super::runtime_store_error)?;
    Ok(if matches {
        OrdinaryHistoryObservation::MatchingUser
    } else {
        OrdinaryHistoryObservation::Conflict
    })
}

pub fn finish_ordinary_worker(
    submission: &OrdinarySubmission,
    attempt: AttemptId,
    worker: Arc<OrdinaryWorker>,
    result: Result<(), SubmissionFailure>,
) {
    if let Ok(worker) = Arc::try_unwrap(worker) {
        let _ = worker.complete(result);
        let _ = submission.seal(attempt);
    }
}

pub(crate) fn verified_admission(
    store: &Store,
    thread: &str,
    admission: Option<&ThreadAdmission>,
) -> Result<ThreadAdmission, ConversationError> {
    match admission {
        Some(admission) => {
            crate::thread_access::verify(store, thread, admission)?;
            Ok(admission.clone())
        }
        None => crate::thread_access::acquire(store, thread),
    }
}
