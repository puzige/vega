//! Core shared types (tech-spec §3): the T11 data-model subset only.
//!
//! This card deliberately ships the *Thread* structure plus the
//! [`ThreadMode`]/[`ThreadStatus`] enums, aligned field-by-field with the
//! `threads` DDL (`migrations/0001_init.sql`). The streaming/event payload
//! types (runtime events, chat messages, tool calls) belong to S3/S4 and
//! must not appear here yet.

use std::sync::Arc;

/// Error surfaced by the vega_conversation orchestration layer.
///
/// Thread-management storage failures remain display strings, while the live
/// agent pipeline preserves the shared [`vega_runtime::VegaError`] kind and
/// fields for UI decisions. Send + Sync by construction (owned data only).
#[derive(Debug, thiserror::Error)]
pub enum ConversationError {
    /// A store/IO failure, reported with the underlying error message.
    #[error("store error: {0}")]
    Store(String),
    /// The referenced thread does not exist.
    #[error("thread not found: {0}")]
    NotFound(String),
    /// No project row exists yet, so a thread cannot be created.
    #[error("no project exists; register a project first")]
    NoProject,
    /// A row carries a value outside the DDL vocabulary (e.g. `mode`).
    #[error("corrupt thread row: {0}")]
    CorruptRow(String),
    /// Headless runtime/provider/persistence failure with its structured kind
    /// and fields preserved for callers.
    #[error("runtime error: {0}")]
    Runtime(Arc<vega_runtime::VegaError>),
}

/// Message identifier used by conversation events.
pub type MessageId = String;

/// Provider tool-call identifier used by conversation events and storage.
pub type CallId = String;

/// Permission mode selected by a thread (tech-spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// Reject write-class tools.
    ReadOnly,
    /// Ask before mutations.
    Confirm,
    /// Auto-approve except hard-blocked dangerous commands.
    Auto,
}

/// `RunMode` name used by the tech spec; the persisted implementation was
/// introduced as [`ThreadMode`] in S2.
pub type RunMode = ThreadMode;

/// Integer millionths of one US dollar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Microcents(pub i64);

/// Provider token counts attached to one API call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenUsage {
    /// Prompt tokens.
    pub input: u64,
    /// Completion tokens.
    pub output: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
    /// Cache-write tokens.
    pub cache_write: u64,
}

/// Complete tool proposal emitted to UI/store consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// Provider call id.
    pub id: CallId,
    /// Tool name.
    pub tool: String,
    /// Complete raw JSON input.
    pub input_json: String,
}

/// Permission decision recorded for a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// Approved for this call only.
    Once,
    /// Persisted project-level rule (S5).
    Always,
    /// Denied.
    Deny,
}

/// Persisted tool-call lifecycle (tech-spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatus {
    /// Waiting for a permission decision.
    PendingApproval,
    /// Approved.
    Approved,
    /// Rejected.
    Rejected,
    /// Running.
    Running,
    /// Completed successfully.
    Success,
    /// Completed with a tool error.
    Failed,
    /// Cancelled while running.
    Cancelled,
}

/// A display chunk from a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutputChunk(pub String);

/// Terminal tool result delivered to the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// Terminal lifecycle status.
    pub status: ToolCallStatus,
    /// Truncated display output.
    pub output: String,
    /// True when a persisted result was reused by call id.
    pub reused: bool,
}

/// Why a conversation message finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationStopReason {
    /// Provider natural end.
    End,
    /// Provider generation limit.
    Length,
    /// Runtime tool-call safety limit.
    ToolLimit,
}

/// Runtime-to-UI/store unique event stream (tech-spec §3).
#[derive(Debug, Clone)]
pub enum ConversationEvent {
    /// A streaming assistant row was created.
    MessageStarted {
        /// Assistant message id.
        message_id: MessageId,
        /// Monotonic thread-local sequence.
        seq: u64,
    },
    /// Visible assistant delta.
    TextDelta {
        /// Assistant message id.
        message_id: MessageId,
        /// Incremental visible text.
        delta: String,
    },
    /// Reasoning delta.
    ThinkingDelta {
        /// Assistant message id.
        message_id: MessageId,
        /// Incremental reasoning text.
        delta: String,
    },
    /// Tool proposal awaiting the placeholder permission hook.
    ToolCallProposed {
        /// Complete proposal.
        call: ToolCall,
    },
    /// Tool approval.
    ToolCallApproved {
        /// Provider call id.
        call_id: CallId,
        /// Permission decision.
        approval: Approval,
    },
    /// Tool output chunk.
    ToolCallOutput {
        /// Provider call id.
        call_id: CallId,
        /// Truncated display output.
        chunk: ToolOutputChunk,
    },
    /// Terminal tool result.
    ToolCallFinished {
        /// Provider call id.
        call_id: CallId,
        /// Terminal result.
        result: ToolResult,
    },
    /// Provider usage and integer cost.
    UsageUpdated {
        /// Assistant message id.
        message_id: MessageId,
        /// Provider token counts.
        usage: TokenUsage,
        /// Integer cost (zero in S4).
        cost: Microcents,
    },
    /// Assistant message converged.
    MessageFinished {
        /// Assistant message id.
        message_id: MessageId,
        /// Convergence reason.
        stop_reason: ConversationStopReason,
    },
    /// Runtime/provider error.
    Error {
        /// Assistant message id, when a message had started.
        message_id: Option<MessageId>,
        /// Safe display error.
        error: Arc<vega_runtime::VegaError>,
    },
    /// Cancellation was observed.
    Interrupted {
        /// Interrupted assistant message id.
        message_id: MessageId,
    },
}

/// Converts one headless runtime event into the shared conversation event.
/// Runtime-only `ToolCallRunning` is persisted but has no UI event in §3.
pub fn from_runtime_event(
    message_id: &str,
    event: &vega_runtime::RuntimeEvent,
) -> Option<ConversationEvent> {
    use vega_runtime::{RuntimeEvent, RuntimeFinishReason, RuntimeToolStatus};

    match event {
        RuntimeEvent::TextDelta(delta) => Some(ConversationEvent::TextDelta {
            message_id: message_id.to_string(),
            delta: delta.clone(),
        }),
        RuntimeEvent::ThinkingDelta(delta) => Some(ConversationEvent::ThinkingDelta {
            message_id: message_id.to_string(),
            delta: delta.clone(),
        }),
        RuntimeEvent::ToolCallProposed(call) => Some(ConversationEvent::ToolCallProposed {
            call: ToolCall {
                id: call.id.clone(),
                tool: call.name.clone(),
                input_json: call.input_json.clone(),
            },
        }),
        RuntimeEvent::ToolCallApproved { call_id } => Some(ConversationEvent::ToolCallApproved {
            call_id: call_id.clone(),
            approval: Approval::Once,
        }),
        RuntimeEvent::ToolCallRunning { .. } => None,
        RuntimeEvent::ToolCallOutput { call_id, chunk } => {
            Some(ConversationEvent::ToolCallOutput {
                call_id: call_id.clone(),
                chunk: ToolOutputChunk(chunk.clone()),
            })
        }
        RuntimeEvent::ToolCallFinished(result) => Some(ConversationEvent::ToolCallFinished {
            call_id: result.call_id.clone(),
            result: ToolResult {
                status: match result.status {
                    RuntimeToolStatus::Rejected => ToolCallStatus::Rejected,
                    RuntimeToolStatus::Success => ToolCallStatus::Success,
                    RuntimeToolStatus::Failed => ToolCallStatus::Failed,
                    RuntimeToolStatus::Cancelled => ToolCallStatus::Cancelled,
                },
                output: result.output.clone(),
                reused: result.reused,
            },
        }),
        RuntimeEvent::UsageUpdated {
            usage,
            cost_microcents,
        } => Some(ConversationEvent::UsageUpdated {
            message_id: message_id.to_string(),
            usage: TokenUsage {
                input: usage.input,
                output: usage.output,
                cache_read: usage.cache_read,
                cache_write: usage.cache_write,
            },
            cost: Microcents(*cost_microcents),
        }),
        RuntimeEvent::Finished(reason) => Some(ConversationEvent::MessageFinished {
            message_id: message_id.to_string(),
            stop_reason: match reason {
                RuntimeFinishReason::End => ConversationStopReason::End,
                RuntimeFinishReason::Length => ConversationStopReason::Length,
                RuntimeFinishReason::ToolLimit => ConversationStopReason::ToolLimit,
            },
        }),
        RuntimeEvent::Interrupted => Some(ConversationEvent::Interrupted {
            message_id: message_id.to_string(),
        }),
        RuntimeEvent::Error(error) => Some(ConversationEvent::Error {
            message_id: Some(message_id.to_string()),
            error: error.clone(),
        }),
    }
}

/// Run mode of a thread (tech-spec §3 `RunMode`, A2-09): ask | plan | execute.
///
/// Stored as the lowercase DDL string in `threads.mode`
/// (`TEXT NOT NULL DEFAULT 'execute'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadMode {
    /// Ask mode: read-only question answering.
    Ask,
    /// Plan mode: propose a plan without executing it.
    Plan,
    /// Execute mode: run tools subject to the permission gate (DDL default).
    Execute,
}

impl ThreadMode {
    /// The DDL string for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadMode::Ask => "ask",
            ThreadMode::Plan => "plan",
            ThreadMode::Execute => "execute",
        }
    }

    /// Parses the DDL string; `None` for values outside `ask|plan|execute`.
    ///
    /// Named `parse` (not `from_str`) so the inherent method does not shadow
    /// `std::str::FromStr` (clippy `should_implement_trait`); the enum keeps
    /// `Option` semantics until an error type is warranted.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ask" => Some(ThreadMode::Ask),
            "plan" => Some(ThreadMode::Plan),
            "execute" => Some(ThreadMode::Execute),
            _ => None,
        }
    }
}

/// Lifecycle status of a thread: active | archived.
///
/// Stored as the lowercase DDL string in `threads.status`
/// (`TEXT NOT NULL DEFAULT 'active'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadStatus {
    /// Live in the sidebar list (DDL default).
    Active,
    /// Hidden from the main list (T13 manages the archive).
    Archived,
}

impl ThreadStatus {
    /// The DDL string for this status.
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadStatus::Active => "active",
            ThreadStatus::Archived => "archived",
        }
    }

    /// Parses the DDL string; `None` for values outside `active|archived`.
    ///
    /// Named `parse` for the same reason as [`ThreadMode::parse`].
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(ThreadStatus::Active),
            "archived" => Some(ThreadStatus::Archived),
            _ => None,
        }
    }
}

/// A conversation thread, aligned field-by-field with the `threads` DDL.
///
/// `permission_mode` and `model` stay plain `String`s on purpose: the
/// shared `PermissionMode` enum is part of the S3/S4 type surface and this
/// card must not define it ahead of spec (the string vocabulary
/// `readonly|confirm|auto` matches `vega_store::config::Defaults`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// Primary key (ulid, generated by this crate on creation).
    pub id: String,
    /// Owning project id (`projects.id`, foreign key).
    pub project_id: String,
    /// Display title; empty until the user renames (T13).
    pub title: String,
    /// Run mode (`ask|plan|execute`).
    pub mode: ThreadMode,
    /// Permission mode (`readonly|confirm|auto`).
    pub permission_mode: String,
    /// Model id; empty string until a provider is configured (S4).
    pub model: String,
    /// Lifecycle status (`active|archived`).
    pub status: ThreadStatus,
    /// Pinned to the top of its group (ordering is T12/T13).
    pub pinned: bool,
    /// Unread marker; stays 0 until streaming lands (S3).
    pub unread: bool,
    /// Creation timestamp, unix milliseconds.
    pub created_at: i64,
    /// Last-activity timestamp, unix milliseconds (bumped on open/touch).
    pub updated_at: i64,
}

/// Minimal projection of the project a thread attaches to.
///
/// T11 resolves the "current project" for thread creation; the full
/// project model belongs to T10/S3 and is not pre-defined here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentProject {
    /// Owning project id (`projects.id`).
    pub id: String,
    /// Display name.
    pub name: String,
}

/// Field set accepted by the thread update path
/// (`vega_conversation::threads::update_thread`, backed by
/// `vega_store::threads::update`).
///
/// `None` leaves a column untouched; `Some` overwrites it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadUpdate {
    /// New title.
    pub title: Option<String>,
    /// New lifecycle status.
    pub status: Option<ThreadStatus>,
    /// New pinned flag.
    pub pinned: Option<bool>,
    /// New unread flag.
    pub unread: Option<bool>,
}

impl ThreadUpdate {
    /// Whether no field is set; the update is then a no-op.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.status.is_none()
            && self.pinned.is_none()
            && self.unread.is_none()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        ConversationEvent, Microcents, ThreadMode, ThreadStatus, TokenUsage, from_runtime_event,
    };

    #[test]
    fn thread_mode_round_trips_the_ddl_vocabulary() {
        for (value, mode) in [
            ("ask", ThreadMode::Ask),
            ("plan", ThreadMode::Plan),
            ("execute", ThreadMode::Execute),
        ] {
            assert_eq!(ThreadMode::parse(value), Some(mode));
            assert_eq!(mode.as_str(), value);
        }
    }

    #[test]
    fn thread_mode_rejects_unknown_strings() {
        assert_eq!(ThreadMode::parse("Ask"), None);
        assert_eq!(ThreadMode::parse(""), None);
        assert_eq!(ThreadMode::parse("yolo"), None);
    }

    #[test]
    fn thread_status_round_trips_the_ddl_vocabulary() {
        for (value, status) in [
            ("active", ThreadStatus::Active),
            ("archived", ThreadStatus::Archived),
        ] {
            assert_eq!(ThreadStatus::parse(value), Some(status));
            assert_eq!(status.as_str(), value);
        }
        assert_eq!(ThreadStatus::parse("done"), None);
    }

    #[test]
    fn converts_text_thinking_and_usage_runtime_events() {
        let message_id = "message-1";
        assert!(matches!(
            from_runtime_event(message_id, &vega_runtime::RuntimeEvent::TextDelta("hello".into())),
            Some(ConversationEvent::TextDelta { message_id, delta })
                if message_id == "message-1" && delta == "hello"
        ));
        assert!(matches!(
            from_runtime_event(message_id, &vega_runtime::RuntimeEvent::ThinkingDelta("why".into())),
            Some(ConversationEvent::ThinkingDelta { message_id, delta })
                if message_id == "message-1" && delta == "why"
        ));
        let usage = vega_runtime::RuntimeTokenUsage {
            input: 10,
            output: 4,
            cache_read: 3,
            cache_write: 2,
        };
        assert!(matches!(
            from_runtime_event(
                message_id,
                &vega_runtime::RuntimeEvent::UsageUpdated {
                    usage,
                    cost_microcents: 0
                }
            ),
            Some(ConversationEvent::UsageUpdated {
                usage: TokenUsage {
                    input: 10,
                    output: 4,
                    cache_read: 3,
                    cache_write: 2
                },
                cost: Microcents(0),
                ..
            })
        ));
    }

    #[test]
    fn converts_errors_without_losing_structured_fields() {
        let provider =
            vega_runtime::RuntimeEvent::Error(Arc::new(vega_runtime::VegaError::Provider {
                status: Some(429),
                message: "rate limited".into(),
                retryable: true,
            }));
        assert!(matches!(
            from_runtime_event("message-1", &provider),
            Some(ConversationEvent::Error { error, .. })
                if matches!(
                    error.as_ref(),
                    vega_runtime::VegaError::Provider {
                        status: Some(429),
                        message,
                        retryable: true,
                    } if message == "rate limited"
                )
        ));

        let tool = vega_runtime::RuntimeEvent::Error(Arc::new(vega_runtime::VegaError::Tool {
            tool: "read".into(),
            message: "collision".into(),
        }));
        assert!(matches!(
            from_runtime_event("message-1", &tool),
            Some(ConversationEvent::Error { error, .. })
                if matches!(
                    error.as_ref(),
                    vega_runtime::VegaError::Tool { tool, message }
                        if tool == "read" && message == "collision"
                )
        ));

        let cancelled =
            vega_runtime::RuntimeEvent::Error(Arc::new(vega_runtime::VegaError::Cancelled));
        assert!(matches!(
            from_runtime_event("message-1", &cancelled),
            Some(ConversationEvent::Error { error, .. })
                if matches!(error.as_ref(), vega_runtime::VegaError::Cancelled)
        ));
    }
}
