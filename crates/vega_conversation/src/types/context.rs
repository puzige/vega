//! Shared, content-free context-compaction state for conversation consumers.
//!
//! These values are deliberately separate from assistant prose and from the
//! runtime's provider-facing request types.  They are safe to retain in a UI
//! status timeline without exposing transcript, summary, image, or provider
//! payloads.

use super::ThreadMode;

/// Lifecycle state exposed by the context control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionStatus {
    /// No budget/settings or no operation has been observed yet.
    Unknown,
    /// A budget is configured and no operation is currently running.
    Ready,
    /// A manual or automatic summary is in flight.
    Compacting,
    /// The latest operation installed a durable checkpoint.
    Succeeded,
    /// The latest operation failed and can be retried.
    Failed,
    /// The latest operation was cancelled without replacing the projection.
    Cancelled,
}

/// Content-free failure vocabulary for a status record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionFailureCode {
    /// The operation was cancelled by the user or run boundary.
    Cancelled,
    /// The source changed before the checkpoint could be installed.
    SourceChanged,
    /// No complete prefix could be selected while retaining the newest user.
    NoCompactablePrefix,
    /// The source or result exceeded a supported bound.
    TooLarge,
    /// The summary was empty, truncated, malformed, or otherwise unusable.
    InvalidSummary,
    /// A historical image cannot be represented safely by the summary call.
    ImagesUnsupported,
    /// The configured input/output budget could not be satisfied.
    OverLimit,
    /// Another runtime or persistence failure occurred.
    Unavailable,
}

/// Summary-call accounting state carried by a status record.  This is kept
/// separate from the assistant usage event because a summary has no message
/// id; `Unknown` also prevents a later priced primary call from making the
/// whole run appear fully accounted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionUsageState {
    /// The bounded summary request is still in flight.
    Pending,
    /// The provider sent Usage, with or without pricing provenance.
    Known { priced: bool },
    /// The provider ended without Usage; cost must remain unknown.
    Unknown,
}

/// Conversation-facing persisted settings for one exact `(thread, model)`.
/// `context_limit: None` intentionally represents an unknown/unconfigured
/// provider capability and keeps the legacy send path available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSettings {
    /// Conversation identity.
    pub thread_id: String,
    /// Exact provider/model identity.
    pub model: String,
    /// Configured total context capacity, or `None` when unknown.
    pub context_limit: Option<u64>,
    /// Positive output reserve for the next primary request.
    pub output_reserve: u64,
    /// Whether automatic compaction is enabled.
    pub automatic_compaction: bool,
    /// Unix-millisecond update timestamp.
    pub updated_at: i64,
}

/// UI/application intent to load or save the exact conversation/model
/// settings.  The request id is echoed by the controller when acknowledging
/// the asynchronous operation so a switched thread cannot consume a late
/// result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSettingsRequested {
    /// Owner identity for the asynchronous request.
    pub request_id: u64,
    /// Exact thread/model settings projection.
    pub settings: ContextSettings,
}

/// UI/application intent to start one manual compaction operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionRequested {
    /// Exact thread identity.
    pub thread_id: String,
    /// Frozen model identity selected by the caller.
    pub model: String,
    /// Owner identity for the asynchronous operation.
    pub request_id: u64,
}

/// UI/application intent to cancel one manual or automatic compaction owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionCancelRequested {
    /// Exact thread identity.
    pub thread_id: String,
    /// Frozen model identity selected by the caller.
    pub model: String,
    /// Owner identity; stale cancellation must not affect a newer request.
    pub request_id: u64,
}

/// Headless read projection consumed by the controller/UI.  It is assembled
/// by the conversation service from one durable source snapshot; callers do
/// not need SQLite access or to recreate provider tool schemas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextProjection {
    /// Exact persisted settings for the requested model, when configured.
    pub settings: Option<ContextSettings>,
    /// Deterministic wire estimate including the system message and real tool
    /// definitions for the thread's run mode.
    pub estimated_tokens: Option<u64>,
    /// Whether a complete older prefix exists and a configured budget could
    /// drive a safe manual compaction.
    pub compactable: bool,
    /// Last content-free status, with interrupted in-flight rows recovered as
    /// Cancelled so a restart cannot leave the UI permanently busy.
    pub last_status: Option<ContextCompactionStatusRecord>,
    /// Sticky durable accounting uncertainty from a summary with no Usage or
    /// an interrupted summary operation.
    pub unknown_usage: bool,
    /// Source revision used for the estimate.
    pub source_version: u64,
    /// Stable SHA-256 source identity used to fence a manual operation.
    pub source_fingerprint: String,
    /// Persisted thread mode used to select the real tool schemas.
    pub run_mode: ThreadMode,
}

/// One chronological status entry for a manual/automatic operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionStatusRecord {
    /// Monotonic owner generation; stale UI updates must not cross it.
    pub generation: u64,
    /// Lifecycle state at this point in the operation.
    pub status: ContextCompactionStatus,
    /// Unix-millisecond timestamp assigned by the conversation boundary.
    pub updated_at: i64,
    /// Approximate input estimate, when a request was assembled.
    pub estimated_tokens: Option<u64>,
    /// Configured input budget after output reserve.
    pub input_budget: Option<u64>,
    /// Target estimate selected for compaction.
    pub target_tokens: Option<u64>,
    /// Durable source version associated with this record.
    pub source_version: Option<u64>,
    /// Safe failure code; raw provider/store text never crosses this type.
    pub failure: Option<ContextCompactionFailureCode>,
    /// Summary-call accounting state.
    pub usage: ContextCompactionUsageState,
}
