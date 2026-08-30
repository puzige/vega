//! Core shared types (tech-spec §3): the T11 data-model subset only.
//!
//! This card deliberately ships the *Thread* structure plus the
//! [`ThreadMode`]/[`ThreadStatus`] enums, aligned field-by-field with the
//! `threads` DDL (`migrations/0001_init.sql`). The streaming/event payload
//! types (runtime events, chat messages, tool calls) belong to S3/S4 and
//! must not appear here yet.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize};

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

impl PermissionMode {
    /// Exact DDL/config value for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "readonly",
            Self::Confirm => "confirm",
            Self::Auto => "auto",
        }
    }

    /// Parses the exact `readonly|confirm|auto` vocabulary.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "readonly" => Some(Self::ReadOnly),
            "confirm" => Some(Self::Confirm),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// Content-free permission prompt shared with UI/store consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    /// Provider call id.
    pub call_id: CallId,
    /// Exact mutating tool name.
    pub tool: String,
    /// Full bash command or normalized project-relative path.
    pub display_target: String,
    /// Stable danger rule id for a danger prompt.
    pub danger_rule_id: Option<String>,
    /// Stable danger reason for a danger prompt.
    pub danger_reason: Option<String>,
}

/// UI decision returned to the runtime permission hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Allow this call once.
    Once,
    /// Allow and remember the exact signature.
    Always,
    /// Reject, optionally with a note.
    Deny { note: Option<String> },
    /// Permission wait expired or disappeared.
    Timeout,
}

/// Source of a persisted approval audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalSource {
    /// Explicit danger-card response.
    Danger,
    /// Read-only permission mode.
    ReadOnly,
    /// Ask/Plan capability gate.
    RunMode,
    /// Exact persisted rule.
    Rule,
    /// Auto permission mode.
    Auto,
    /// Ordinary permission-card response.
    User,
    /// Permission timeout.
    Timeout,
    /// Invalid write/edit input.
    Validation,
    /// Read-only built-in tool.
    ReadonlyTool,
    /// Startup recovery.
    Recovery,
    /// Exact S4 bare value read through the legacy branch.
    Legacy,
}

impl ApprovalSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Danger => "danger",
            Self::ReadOnly => "readonly",
            Self::RunMode => "run_mode",
            Self::Rule => "rule",
            Self::Auto => "auto",
            Self::User => "user",
            Self::Timeout => "timeout",
            Self::Validation => "validation",
            Self::ReadonlyTool => "readonly_tool",
            Self::Recovery => "recovery",
            Self::Legacy => "legacy",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "danger" => Some(Self::Danger),
            "readonly" => Some(Self::ReadOnly),
            "run_mode" => Some(Self::RunMode),
            "rule" => Some(Self::Rule),
            "auto" => Some(Self::Auto),
            "user" => Some(Self::User),
            "timeout" => Some(Self::Timeout),
            "validation" => Some(Self::Validation),
            "readonly_tool" => Some(Self::ReadonlyTool),
            "recovery" => Some(Self::Recovery),
            "legacy" => Some(Self::Legacy),
            _ => None,
        }
    }
}

/// Nested danger decision retained when later policy also rejects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DangerAudit {
    /// Stable danger rule id.
    pub rule_id: String,
    /// Danger-card decision.
    pub decision: Approval,
    /// Optional denial note.
    pub note: Option<String>,
}

/// Strict four-field approval audit persisted in `tool_calls.approval`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalAudit {
    /// Final decision.
    pub decision: Approval,
    /// Optional denial note.
    pub note: Option<String>,
    /// Decision source.
    pub source: ApprovalSource,
    /// Nested danger decision, if a danger card was shown.
    pub danger: Option<DangerAudit>,
}

/// Strict approval codec failure. Callers must fail closed.
#[derive(Debug, thiserror::Error)]
pub enum ApprovalCodecError {
    /// JSON shape or scalar type is invalid.
    #[error("invalid approval audit shape")]
    InvalidShape,
    /// Decision/source vocabulary is unknown.
    #[error("invalid approval audit vocabulary")]
    InvalidVocabulary,
    /// Fields form an impossible permission audit.
    #[error("invalid approval audit semantics")]
    InvalidSemantics,
    /// Legacy audits are read-only and cannot be emitted by S5.
    #[error("legacy approval audit cannot be encoded")]
    LegacyWrite,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalWireRead {
    decision: String,
    note: RequiredNullableString,
    source: String,
    danger: RequiredNullableDanger,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DangerWireRead {
    rule_id: String,
    decision: String,
    note: RequiredNullableString,
}

struct RequiredNullableString(Option<String>);

impl<'de> Deserialize<'de> for RequiredNullableString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer).map(Self)
    }
}

struct RequiredNullableDanger(Option<DangerWireRead>);

impl<'de> Deserialize<'de> for RequiredNullableDanger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<DangerWireRead>::deserialize(deserializer).map(Self)
    }
}

#[derive(Serialize)]
struct ApprovalWireWrite<'a> {
    decision: &'a str,
    note: &'a Option<String>,
    source: &'a str,
    danger: Option<DangerWireWrite<'a>>,
}

#[derive(Serialize)]
struct DangerWireWrite<'a> {
    rule_id: &'a str,
    decision: &'a str,
    note: &'a Option<String>,
}

impl ApprovalAudit {
    /// Decodes exact S5 JSON or the exact S4 bare `once|always|deny` values.
    pub fn from_json(raw: &str) -> Result<Self, ApprovalCodecError> {
        if let Some(decision) = Approval::parse(raw) {
            return Ok(Self {
                decision,
                note: None,
                source: ApprovalSource::Legacy,
                danger: None,
            });
        }
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|_| ApprovalCodecError::InvalidShape)?;
        require_exact_keys(&value, &["decision", "note", "source", "danger"])?;
        if let Some(danger) = value.get("danger")
            && !danger.is_null()
        {
            require_exact_keys(danger, &["rule_id", "decision", "note"])?;
        }
        let wire: ApprovalWireRead =
            serde_json::from_str(raw).map_err(|_| ApprovalCodecError::InvalidShape)?;
        let note = wire.note.0;
        let decision =
            Approval::parse(&wire.decision).ok_or(ApprovalCodecError::InvalidVocabulary)?;
        let source =
            ApprovalSource::parse(&wire.source).ok_or(ApprovalCodecError::InvalidVocabulary)?;
        if source == ApprovalSource::Legacy {
            return Err(ApprovalCodecError::InvalidSemantics);
        }
        let danger = wire
            .danger
            .0
            .map(|danger| {
                Ok(DangerAudit {
                    rule_id: danger.rule_id,
                    decision: Approval::parse(&danger.decision)
                        .ok_or(ApprovalCodecError::InvalidVocabulary)?,
                    note: danger.note.0,
                })
            })
            .transpose()?;
        let audit = Self {
            decision,
            note,
            source,
            danger,
        };
        audit.validate(false)?;
        Ok(audit)
    }

    /// Encodes the canonical strict four-field S5 JSON shape.
    pub fn to_json(&self) -> Result<String, ApprovalCodecError> {
        if self.source == ApprovalSource::Legacy {
            return Err(ApprovalCodecError::LegacyWrite);
        }
        self.validate(true)?;
        serde_json::to_string(&ApprovalWireWrite {
            decision: self.decision.as_str(),
            note: &self.note,
            source: self.source.as_str(),
            danger: self.danger.as_ref().map(|danger| DangerWireWrite {
                rule_id: &danger.rule_id,
                decision: danger.decision.as_str(),
                note: &danger.note,
            }),
        })
        .map_err(|_| ApprovalCodecError::InvalidShape)
    }

    fn validate(&self, encoding: bool) -> Result<(), ApprovalCodecError> {
        if self.source == ApprovalSource::Legacy {
            return if encoding {
                Err(ApprovalCodecError::LegacyWrite)
            } else if self.note.is_none() && self.danger.is_none() {
                Ok(())
            } else {
                Err(ApprovalCodecError::InvalidSemantics)
            };
        }
        if self.note.is_some() && self.decision != Approval::Deny {
            return Err(ApprovalCodecError::InvalidSemantics);
        }
        if let Some(danger) = &self.danger
            && (danger.rule_id.is_empty()
                || (danger.note.is_some() && danger.decision != Approval::Deny))
        {
            return Err(ApprovalCodecError::InvalidSemantics);
        }
        let valid = match self.source {
            ApprovalSource::Danger => self
                .danger
                .as_ref()
                .is_some_and(|danger| self.decision == danger.decision && self.note == danger.note),
            ApprovalSource::ReadOnly => {
                self.decision == Approval::Deny
                    && self.note.is_none()
                    && self.danger.as_ref().is_none_or(|danger| {
                        matches!(danger.decision, Approval::Once | Approval::Always)
                            && danger.note.is_none()
                    })
            }
            ApprovalSource::RunMode | ApprovalSource::Validation | ApprovalSource::Recovery => {
                self.decision == Approval::Deny && self.note.is_none() && self.danger.is_none()
            }
            ApprovalSource::Rule => {
                self.decision == Approval::Always && self.note.is_none() && self.danger.is_none()
            }
            ApprovalSource::Auto => {
                self.decision == Approval::Once && self.note.is_none() && self.danger.is_none()
            }
            ApprovalSource::User => self.danger.is_none(),
            ApprovalSource::Timeout => {
                self.decision == Approval::Deny
                    && self.note.is_none()
                    && self.danger.as_ref().is_none_or(|danger| {
                        danger.decision == Approval::Deny && danger.note.is_none()
                    })
            }
            ApprovalSource::ReadonlyTool => {
                self.decision == Approval::Once && self.note.is_none() && self.danger.is_none()
            }
            ApprovalSource::Legacy => false,
        };
        valid
            .then_some(())
            .ok_or(ApprovalCodecError::InvalidSemantics)
    }
}

fn require_exact_keys(
    value: &serde_json::Value,
    expected: &[&str],
) -> Result<(), ApprovalCodecError> {
    let object = value.as_object().ok_or(ApprovalCodecError::InvalidShape)?;
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(ApprovalCodecError::InvalidShape);
    }
    Ok(())
}

/// Maps a runtime prompt into the shared content-free UI projection.
pub fn permission_request_from_runtime(
    prompt: &vega_runtime::RuntimePermissionPrompt,
) -> PermissionRequest {
    PermissionRequest {
        call_id: prompt.target.call_id.clone(),
        tool: prompt.target.tool.as_str().to_string(),
        display_target: prompt.target.display_target.clone(),
        danger_rule_id: prompt.danger.as_ref().map(|danger| danger.rule_id.clone()),
        danger_reason: prompt.danger.as_ref().map(|danger| danger.reason.clone()),
    }
}

/// Maps runtime-local audit facts into the shared strict audit type.
pub fn approval_audit_from_runtime(audit: &vega_runtime::RuntimeApprovalAudit) -> ApprovalAudit {
    ApprovalAudit {
        decision: approval_from_runtime(audit.decision),
        note: audit.note.clone(),
        source: match audit.source {
            vega_runtime::RuntimeApprovalSource::Danger => ApprovalSource::Danger,
            vega_runtime::RuntimeApprovalSource::ReadOnly => ApprovalSource::ReadOnly,
            vega_runtime::RuntimeApprovalSource::RunMode => ApprovalSource::RunMode,
            vega_runtime::RuntimeApprovalSource::Rule => ApprovalSource::Rule,
            vega_runtime::RuntimeApprovalSource::Auto => ApprovalSource::Auto,
            vega_runtime::RuntimeApprovalSource::User => ApprovalSource::User,
            vega_runtime::RuntimeApprovalSource::Timeout => ApprovalSource::Timeout,
            vega_runtime::RuntimeApprovalSource::ReadonlyTool => ApprovalSource::ReadonlyTool,
        },
        danger: audit.danger.as_ref().map(|danger| DangerAudit {
            rule_id: danger.rule_id.clone(),
            decision: approval_from_runtime(danger.decision),
            note: danger.note.clone(),
        }),
    }
}

fn approval_from_runtime(decision: vega_runtime::RuntimeApprovalDecision) -> Approval {
    match decision {
        vega_runtime::RuntimeApprovalDecision::Once => Approval::Once,
        vega_runtime::RuntimeApprovalDecision::Always => Approval::Always,
        vega_runtime::RuntimeApprovalDecision::Deny => Approval::Deny,
    }
}

/// Maps a shared UI decision into the runtime-local decision vocabulary.
pub fn permission_decision_to_runtime(
    decision: PermissionDecision,
) -> vega_runtime::RuntimeUserDecision {
    match decision {
        PermissionDecision::Once => vega_runtime::RuntimeUserDecision::Once,
        PermissionDecision::Always => vega_runtime::RuntimeUserDecision::Always,
        PermissionDecision::Deny { note } => vega_runtime::RuntimeUserDecision::Deny { note },
        PermissionDecision::Timeout => vega_runtime::RuntimeUserDecision::Timeout,
    }
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

impl Approval {
    /// Exact approval vocabulary used by the strict codec.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Deny => "deny",
        }
    }

    /// Parses only exact bare approval values.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "once" => Some(Self::Once),
            "always" => Some(Self::Always),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
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
/// `permission_mode` is typed and fail-closed on create/load. `model` stays a
/// raw provider id string.
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
    pub permission_mode: PermissionMode,
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

#[cfg(test)]
mod permission_tests {
    use super::{
        Approval, ApprovalAudit, ApprovalCodecError, ApprovalSource, DangerAudit,
        PermissionDecision, PermissionMode, approval_audit_from_runtime,
        permission_decision_to_runtime, permission_request_from_runtime,
    };
    use vega_runtime::{
        RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimeDangerAudit,
        RuntimeDangerFacts, RuntimeMutatingTool, RuntimePermissionPrompt, RuntimePermissionTarget,
        RuntimeUserDecision,
    };

    fn audit(
        decision: Approval,
        source: ApprovalSource,
        danger: Option<DangerAudit>,
    ) -> ApprovalAudit {
        ApprovalAudit {
            decision,
            note: None,
            source,
            danger,
        }
    }

    #[test]
    fn permission_mode_round_trips_and_rejects_unknown_values() {
        for (raw, mode) in [
            ("readonly", PermissionMode::ReadOnly),
            ("confirm", PermissionMode::Confirm),
            ("auto", PermissionMode::Auto),
        ] {
            assert_eq!(PermissionMode::parse(raw), Some(mode));
            assert_eq!(mode.as_str(), raw);
        }
        for raw in ["", "Auto", "yolo", " confirm"] {
            assert_eq!(PermissionMode::parse(raw), None);
        }
    }

    #[test]
    fn every_legal_structured_audit_round_trips() {
        let danger_once = DangerAudit {
            rule_id: "danger-rule".into(),
            decision: Approval::Once,
            note: None,
        };
        let danger_always = DangerAudit {
            rule_id: "danger-rule".into(),
            decision: Approval::Always,
            note: None,
        };
        let danger_deny = DangerAudit {
            rule_id: "danger-rule".into(),
            decision: Approval::Deny,
            note: None,
        };
        let cases = [
            audit(
                Approval::Once,
                ApprovalSource::Danger,
                Some(danger_once.clone()),
            ),
            audit(
                Approval::Always,
                ApprovalSource::Danger,
                Some(danger_always.clone()),
            ),
            audit(
                Approval::Deny,
                ApprovalSource::Danger,
                Some(danger_deny.clone()),
            ),
            audit(Approval::Deny, ApprovalSource::ReadOnly, None),
            audit(
                Approval::Deny,
                ApprovalSource::ReadOnly,
                Some(danger_always),
            ),
            audit(Approval::Deny, ApprovalSource::RunMode, None),
            audit(Approval::Always, ApprovalSource::Rule, None),
            audit(Approval::Once, ApprovalSource::Auto, None),
            audit(Approval::Once, ApprovalSource::User, None),
            audit(Approval::Always, ApprovalSource::User, None),
            ApprovalAudit {
                decision: Approval::Deny,
                note: Some("not now".into()),
                source: ApprovalSource::User,
                danger: None,
            },
            audit(Approval::Deny, ApprovalSource::Timeout, None),
            audit(Approval::Deny, ApprovalSource::Timeout, Some(danger_deny)),
            audit(Approval::Deny, ApprovalSource::Validation, None),
            audit(Approval::Once, ApprovalSource::ReadonlyTool, None),
            audit(Approval::Deny, ApprovalSource::Recovery, None),
        ];
        for expected in cases {
            let json = expected.to_json().unwrap();
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(value.as_object().map(|object| object.len()), Some(4));
            if let Some(danger) = value.get("danger").and_then(|value| value.as_object()) {
                assert_eq!(danger.len(), 3);
            }
            assert_eq!(ApprovalAudit::from_json(&json).unwrap(), expected);
        }
    }

    #[test]
    fn recovery_encoding_is_the_canonical_exact_value() {
        let json = audit(Approval::Deny, ApprovalSource::Recovery, None)
            .to_json()
            .unwrap();
        assert_eq!(json, vega_store::recovery::RECOVERY_DENIAL_APPROVAL_JSON);
    }

    #[test]
    fn exact_field_sets_and_scalar_types_fail_closed() {
        for raw in [
            r#"{"decision":"once","note":null,"source":"user"}"#,
            r#"{"decision":"once","note":null,"source":"user","danger":null,"extra":1}"#,
            r#"{"decision":"once","decision":"deny","note":null,"source":"user","danger":null}"#,
            r#"{"decision":"once","note":[],"source":"user","danger":null}"#,
            r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"d","decision":"once"}}"#,
            r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"d","decision":"once","note":null,"extra":1}}"#,
            r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"d","decision":"once","decision":"deny","note":null}}"#,
            r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"d","decision":"once","note":[]}}"#,
            r#""once""#,
            "[]",
            "1",
            "true",
            "{",
        ] {
            assert!(ApprovalAudit::from_json(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn unknown_and_semantically_impossible_values_fail_closed() {
        for raw in [
            r#"{"decision":"later","note":null,"source":"user","danger":null}"#,
            r#"{"decision":"once","note":null,"source":"unknown","danger":null}"#,
            r#"{"decision":"once","note":null,"source":"legacy","danger":null}"#,
            r#"{"decision":"once","note":null,"source":"danger","danger":null}"#,
            r#"{"decision":"once","note":null,"source":"rule","danger":null}"#,
            r#"{"decision":"always","note":null,"source":"auto","danger":null}"#,
            r#"{"decision":"once","note":"not valid","source":"user","danger":null}"#,
            r#"{"decision":"deny","note":null,"source":"readonly_tool","danger":null}"#,
            r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"","decision":"once","note":null}}"#,
            r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"d","decision":"deny","note":null}}"#,
            r#"{"decision":"deny","note":null,"source":"readonly","danger":{"rule_id":"d","decision":"deny","note":null}}"#,
        ] {
            assert!(ApprovalAudit::from_json(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn legacy_bare_values_are_read_only_and_exact() {
        for (raw, decision) in [
            ("once", Approval::Once),
            ("always", Approval::Always),
            ("deny", Approval::Deny),
        ] {
            let decoded = ApprovalAudit::from_json(raw).unwrap();
            assert_eq!(decoded.decision, decision);
            assert_eq!(decoded.source, ApprovalSource::Legacy);
            assert!(matches!(
                decoded.to_json(),
                Err(ApprovalCodecError::LegacyWrite)
            ));
        }
        for raw in [" once", "once ", "Once", "DENY", "\nonce"] {
            assert!(ApprovalAudit::from_json(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn runtime_prompt_and_audit_mapping_are_field_exact_and_content_free() {
        let prompt = RuntimePermissionPrompt {
            target: RuntimePermissionTarget {
                call_id: "call-1".into(),
                tool: RuntimeMutatingTool::Write,
                exact_pattern: "src/lib.rs".into(),
                display_target: "src/lib.rs".into(),
            },
            danger: Some(RuntimeDangerFacts {
                rule_id: "rule-1".into(),
                reason: "reason".into(),
            }),
        };
        let request = permission_request_from_runtime(&prompt);
        assert_eq!(request.call_id, "call-1");
        assert_eq!(request.tool, "write");
        assert_eq!(request.display_target, "src/lib.rs");
        assert_eq!(request.danger_rule_id.as_deref(), Some("rule-1"));
        assert_eq!(request.danger_reason.as_deref(), Some("reason"));
        assert!(!request.display_target.contains("content"));

        let runtime = RuntimeApprovalAudit {
            decision: RuntimeApprovalDecision::Deny,
            note: None,
            source: RuntimeApprovalSource::ReadOnly,
            danger: Some(RuntimeDangerAudit {
                rule_id: "rule-1".into(),
                decision: RuntimeApprovalDecision::Always,
                note: None,
            }),
        };
        let shared = approval_audit_from_runtime(&runtime);
        assert_eq!(shared.decision, Approval::Deny);
        assert_eq!(shared.source, ApprovalSource::ReadOnly);
        assert_eq!(
            shared.danger.as_ref().map(|danger| danger.decision),
            Some(Approval::Always)
        );
        assert!(shared.to_json().is_ok());
    }

    #[test]
    fn shared_ui_decisions_map_one_way_to_runtime() {
        assert_eq!(
            permission_decision_to_runtime(PermissionDecision::Once),
            RuntimeUserDecision::Once
        );
        assert_eq!(
            permission_decision_to_runtime(PermissionDecision::Always),
            RuntimeUserDecision::Always
        );
        assert_eq!(
            permission_decision_to_runtime(PermissionDecision::Deny {
                note: Some("no".into())
            }),
            RuntimeUserDecision::Deny {
                note: Some("no".into())
            }
        );
        assert_eq!(
            permission_decision_to_runtime(PermissionDecision::Timeout),
            RuntimeUserDecision::Timeout
        );
    }
}
