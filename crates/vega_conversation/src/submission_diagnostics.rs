pub(crate) mod background;
pub use background::shutdown as shutdown_background_writes;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub use vega_store::submission_diagnostics::{
    PreparationEvent, PreparationFailure, PreparationReason, PreparationStage, PreparationState,
};
use vega_store::{DatabaseIdentity, Store};

use crate::types::ConversationError;

pub fn runtime_failure(
    stage: PreparationStage,
    error: &vega_runtime::VegaError,
) -> PreparationFailure {
    use PreparationReason as Reason;
    let metrics = error.diagnostic_metrics();
    let reason = match error {
        vega_runtime::VegaError::Cancelled => Reason::State,
        vega_runtime::VegaError::ModelImagesUnsupported => Reason::Capability,
        vega_runtime::VegaError::ProjectInstructions(error) => match error.failure() {
            vega_tools::ProjectInstructionsFailure::InvalidRootPath => Reason::MissingWorkspace,
            vega_tools::ProjectInstructionsFailure::Unavailable => Reason::InstructionsUnavailable,
            vega_tools::ProjectInstructionsFailure::TooLarge => Reason::InstructionsTooLarge,
            vega_tools::ProjectInstructionsFailure::InvalidText => Reason::InstructionsInvalid,
        },
        vega_runtime::VegaError::ReasoningSelectionInvalid { .. }
        | vega_runtime::VegaError::ReasoningReplayUnsupported
        | vega_runtime::VegaError::ReasoningReplayIncomplete => Reason::Reasoning,
        vega_runtime::VegaError::CredentialExposureBlocked
        | vega_runtime::VegaError::CredentialCheckUnavailable
        | vega_runtime::VegaError::CredentialRead(_)
        | vega_runtime::VegaError::CredentialContinuationUnavailable
        | vega_runtime::VegaError::CredentialProjectionInvalid { .. } => Reason::CredentialSafety,
        vega_runtime::VegaError::Context(_) => Reason::Context,
        _ => match metrics.runtime_reason {
            Some("database") => Reason::Database,
            Some("io") => Reason::Io,
            Some("admission") => Reason::Admission,
            Some("state") => Reason::State,
            Some("references") => Reason::References,
            Some("worker") => Reason::Worker,
            Some("pipeline") => Reason::Pipeline,
            Some("approval_audit") => Reason::ApprovalAudit,
            Some("capability") => Reason::Capability,
            Some("context_budget" | "context_arithmetic") => Reason::Context,
            _ => Reason::Unknown,
        },
    };
    PreparationFailure {
        stage,
        reason,
        sqlite_code: metrics.sqlite_code,
        sqlite_extended_code: metrics.sqlite_extended_code,
        io_kind: metrics.io_kind,
        credential_stage: metrics.credential_stage,
        credential_reason: metrics.credential_reason,
        configuration_lock: metrics.configuration_lock,
        configuration_lock_attempts: metrics.configuration_lock_attempts,
        configuration_lock_wait_ms: metrics.configuration_lock_wait_ms,
    }
}

pub fn conversation_failure(
    stage: PreparationStage,
    error: &ConversationError,
) -> PreparationFailure {
    let reason = match error {
        ConversationError::Runtime(error) => return runtime_failure(stage, error),
        ConversationError::InvalidTaskIdentity => PreparationReason::InvalidIdentity,
        ConversationError::NotFound(_) => PreparationReason::MissingThread,
        ConversationError::NoProject => PreparationReason::MissingWorkspace,
        ConversationError::CorruptRow(_) => PreparationReason::CorruptState,
        ConversationError::ContextCleanupPending => PreparationReason::CleanupPending,
        ConversationError::PendingPlan => PreparationReason::PendingPlan,
        ConversationError::UnsupportedHistoricalConsumer
        | ConversationError::InvalidCodexSessionTransition => PreparationReason::State,
        ConversationError::Store(_) => PreparationReason::Unknown,
    };
    PreparationFailure::new(stage, reason)
}

#[derive(Default)]
struct WriteStatus {
    pending: AtomicUsize,
    failed: AtomicBool,
}

struct AppendPreparationEvent {
    event: PreparationEvent,
}

impl crate::database_writer::Sealed for AppendPreparationEvent {}

impl crate::database_writer::DatabaseCommand for AppendPreparationEvent {
    type Output = Result<(), String>;

    fn apply(self, store: &Store) -> Self::Output {
        vega_store::submission_diagnostics::append(store.conn(), &self.event)
            .map_err(|_| "submission diagnostic persistence failed".to_string())
    }
}

#[derive(Default)]
struct JournalState {
    events: Vec<PreparationEvent>,
    target: Option<(PathBuf, DatabaseIdentity)>,
    next_write: usize,
    scheduled: bool,
}

#[derive(Default)]
pub(crate) struct Journal {
    state: Arc<Mutex<JournalState>>,
    status: Arc<WriteStatus>,
}

impl Journal {
    pub(crate) fn attach(&self, path: &Path, database: DatabaseIdentity) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.target.is_some() {
            return;
        }
        state.target = Some((path.to_owned(), database));
        self.status
            .pending
            .fetch_add(state.events.len(), Ordering::AcqRel);
        drop(state);
        self.schedule();
    }

    fn schedule(&self) {
        let mut journal = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if journal.scheduled
            || journal.target.is_none()
            || journal.next_write == journal.events.len()
        {
            return;
        }
        journal.scheduled = true;
        drop(journal);
        let state = self.state.clone();
        let status = self.status.clone();
        let worker = background::submit(move || {
                let mut failed = false;
                loop {
                    let request = {
                        let mut journal = state.lock().unwrap_or_else(|error| error.into_inner());
                        if journal.next_write == journal.events.len() {
                            journal.scheduled = false;
                            return if failed { Err("submission diagnostic persistence failed".into()) } else { Ok(()) };
                        }
                        let Some((path, database)) = journal.target.clone() else {
                            journal.scheduled = false;
                            return Err("submission diagnostic target unavailable".into());
                        };
                        let event = journal.events[journal.next_write].clone();
                        journal.next_write += 1;
                        (path, database, event)
                    };
                    let succeeded = crate::database_writer::commit(
                        &request.0,
                        request.1,
                        AppendPreparationEvent { event: request.2 },
                    )
                    .is_ok_and(|result| result.is_ok());
                    if !succeeded {
                        failed = true;
                        status.failed.store(true, Ordering::Release);
                        tracing::warn!("submission diagnostic persistence failed");
                    }
                    status.pending.fetch_sub(1, Ordering::AcqRel);
                }
            });
        if worker.is_err() {
            self.state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .scheduled = false;
            self.status.failed.store(true, Ordering::Release);
        }
    }

    pub(crate) fn record(
        &self,
        submission: String,
        attempt: u64,
        state: PreparationState,
        failure: Option<PreparationFailure>,
    ) {
        let mut journal = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if journal.events.iter().any(|event| {
            event.attempt == attempt
                && (event.state != PreparationState::Started)
                    == (state != PreparationState::Started)
        }) {
            return;
        }
        let event = PreparationEvent {
            submission_id: submission,
            attempt,
            state,
            failure,
            occurred_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| {
                    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
                }),
        };
        if journal.target.is_some() {
            self.status.pending.fetch_add(1, Ordering::AcqRel);
        }
        journal.events.push(event);
        drop(journal);
        self.schedule();
    }

    pub(crate) fn export(&self, attempt: u64) -> String {
        let journal = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let storage = if self.status.failed.load(Ordering::Acquire) {
            "failed_memory_retained"
        } else if journal.target.is_none() {
            "memory_only"
        } else if self.status.pending.load(Ordering::Acquire) != 0 {
            "pending"
        } else {
            "persisted"
        };
        serde_json::json!({"storage": storage, "events": journal.events.iter().filter(|event| event.attempt == attempt).collect::<Vec<_>>()}).to_string()
    }
}
