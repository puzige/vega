use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

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

struct WriteRequest {
    path: PathBuf,
    database: DatabaseIdentity,
    event: PreparationEvent,
    status: Arc<WriteStatus>,
}

fn writer() -> Option<&'static mpsc::SyncSender<WriteRequest>> {
    static WRITER: OnceLock<Option<mpsc::SyncSender<WriteRequest>>> = OnceLock::new();
    WRITER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel::<WriteRequest>(256);
            std::thread::Builder::new()
                .name("submission-diagnostics".into())
                .spawn(move || {
                    for request in receiver {
                        let succeeded = Store::open_existing(&request.path).is_ok_and(|store| {
                            if store.database_identity() != request.database {
                                return false;
                            }
                            vega_store::submission_diagnostics::append(store.conn(), &request.event)
                                .is_ok()
                        });
                        if !succeeded {
                            request.status.failed.store(true, Ordering::Release);
                            tracing::warn!("submission diagnostic persistence failed");
                        }
                        request.status.pending.fetch_sub(1, Ordering::AcqRel);
                    }
                })
                .ok()
                .map(|_| sender)
        })
        .as_ref()
}

#[derive(Default)]
struct JournalState {
    events: Vec<PreparationEvent>,
    target: Option<(PathBuf, DatabaseIdentity)>,
}

#[derive(Default)]
pub(crate) struct Journal {
    state: Mutex<JournalState>,
    status: Arc<WriteStatus>,
}

impl Journal {
    pub(crate) fn attach(&self, path: &Path, database: DatabaseIdentity) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.target.is_some() {
            return;
        }
        state.target = Some((path.to_owned(), database));
        for event in &state.events {
            self.enqueue(path, database, event.clone());
        }
    }

    fn enqueue(&self, path: &Path, database: DatabaseIdentity, event: PreparationEvent) {
        self.status.pending.fetch_add(1, Ordering::AcqRel);
        let sent = writer().is_some_and(|sender| {
            sender
                .try_send(WriteRequest {
                    path: path.to_owned(),
                    database,
                    event,
                    status: self.status.clone(),
                })
                .is_ok()
        });
        if !sent {
            self.status.failed.store(true, Ordering::Release);
            self.status.pending.fetch_sub(1, Ordering::AcqRel);
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
        if let Some((path, database)) = &journal.target {
            self.enqueue(path, *database, event.clone());
        }
        journal.events.push(event);
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
