//! Headless agentic loop (tech-spec §4.2, A3-03 / S4-T20).

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use futures::{FutureExt, StreamExt};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use vega_token::{PricingCatalog, PricingProfile};

use crate::error::VegaError;
use crate::provider::{
    ChatMessage, ChatRequest, ChatRole, ChatToolCall, Provider, ProviderEvent, StopReason,
    ToolDefinition,
};
use crate::{
    RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimeCapabilityOutcome,
    RuntimeDangerFacts, RuntimeExecutePermission, RuntimeMutatingTool, RuntimePermissionMode,
    RuntimePermissionOutcome, RuntimePermissionPrompt, RuntimePermissionTarget, RuntimeRunMode,
    RuntimeToolClass, RuntimeUserDecision, decide_capability, decide_execute_permission,
};

/// Maximum number of tool calls executed by one task.
pub const TOOL_CALL_LIMIT: usize = 100;
/// Stable content-free result for a provider call id that conflicts with
/// persisted owner, tool, or safe input identity.
pub const CALL_ID_CONFLICT_OUTPUT: &str = "Tool error: persisted call identity conflict";
/// Stable content-free result when cancellation is observed after durable
/// approval/running but before any tool worker starts.
pub const CANCELLED_BEFORE_EXECUTION_OUTPUT: &str = "Tool cancelled before execution.";

const OUTPUT_HALF_LINES: usize = 2_000;
const OUTPUT_TRUNCATION_MARKER: &str = "…[tool output truncated: middle lines omitted]";
/// Authoritative production permission wait required by tech-spec §4.3.
pub const PERMISSION_TIMEOUT: Duration = Duration::from_secs(600);

/// One project-scoped exact permission rule preloaded by conversation.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RuntimeExactRule {
    /// Mutating tool name.
    pub tool: RuntimeMutatingTool,
    /// Byte-exact command or normalized project-relative path.
    pub pattern: String,
}

impl fmt::Debug for RuntimeExactRule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeExactRule")
            .field("tool", &self.tool)
            .field("pattern_bytes", &self.pattern.len())
            .finish()
    }
}

/// Headless tool and permission facts for one task.
#[derive(Clone)]
pub struct RuntimeToolConfig {
    /// Ask, Plan, or Execute capability boundary.
    pub run_mode: RuntimeRunMode,
    /// Execute-mode mutation policy.
    pub permission_mode: RuntimePermissionMode,
    /// Project id used only for checkpoint scope binding.
    pub project_id: String,
    /// Thread id used only for checkpoint scope binding.
    pub thread_id: String,
    /// External checkpoint root. Never enters events, provider wire, or errors.
    pub checkpoint_root: PathBuf,
    /// Exact project rules loaded at task start.
    pub exact_rules: Vec<RuntimeExactRule>,
    foreign_call_ids: HashSet<String>,
    permission_timeout: Duration,
}

impl RuntimeToolConfig {
    /// Builds a production task config with the fixed ten-minute prompt timeout.
    pub fn new(
        run_mode: RuntimeRunMode,
        permission_mode: RuntimePermissionMode,
        project_id: String,
        thread_id: String,
        checkpoint_root: PathBuf,
        exact_rules: Vec<RuntimeExactRule>,
    ) -> Self {
        Self {
            run_mode,
            permission_mode,
            project_id,
            thread_id,
            checkpoint_root,
            exact_rules,
            foreign_call_ids: HashSet::new(),
            permission_timeout: PERMISSION_TIMEOUT,
        }
    }

    /// Registers globally occupied call ids owned by other threads without
    /// exposing their tool inputs or results to this runtime.
    pub fn with_foreign_call_ids(mut self, ids: Vec<String>) -> Self {
        self.foreign_call_ids = ids.into_iter().collect();
        self
    }

    fn readonly() -> Self {
        Self::new(
            RuntimeRunMode::Ask,
            RuntimePermissionMode::ReadOnly,
            String::new(),
            String::new(),
            PathBuf::new(),
            Vec::new(),
        )
    }

    #[cfg(test)]
    fn with_permission_timeout(mut self, timeout: Duration) -> Self {
        self.permission_timeout = timeout;
        self
    }
}

impl Default for RuntimeToolConfig {
    fn default() -> Self {
        Self::readonly()
    }
}

impl fmt::Debug for RuntimeToolConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeToolConfig")
            .field("run_mode", &self.run_mode)
            .field("permission_mode", &self.permission_mode)
            .field("project_id_bytes", &self.project_id.len())
            .field("thread_id_bytes", &self.thread_id.len())
            .field("checkpoint_root", &"[REDACTED]")
            .field("exact_rule_count", &self.exact_rules.len())
            .field("foreign_call_id_count", &self.foreign_call_ids.len())
            .finish()
    }
}

/// Cancellable permission boundary implemented by conversation/UI.
pub trait RuntimePermissionHook: Send + Sync {
    /// Requests one content-free decision.
    fn request(
        &self,
        prompt: RuntimePermissionPrompt,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>>;
}

struct RejectPermissionHook;

impl RuntimePermissionHook for RejectPermissionHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        async { Ok(RuntimeUserDecision::Timeout) }.boxed()
    }
}

/// Inputs for one agent task.
#[derive(Clone)]
pub struct AgentRequest {
    /// Provider model id.
    pub model: String,
    /// System instruction placed before all history.
    pub system_prompt: String,
    /// Existing history window, oldest to newest.
    pub history: Vec<ChatMessage>,
    /// Optional generation cap forwarded to the provider.
    pub max_tokens: Option<u32>,
    /// Successfully persisted tool results keyed by provider call id.
    ///
    /// These results are observed again without re-executing the tool, which
    /// makes restart/retry idempotent at the call-id boundary.
    pub completed_tool_results: HashMap<String, CompletedToolCall>,
    /// Tool capability, permission, checkpoint, and exact-rule facts.
    pub tool_config: RuntimeToolConfig,
    /// Immutable pricing capability frozen at run preflight (S7-T38/C3).
    ///
    /// `None` keeps the S4 legacy/unpriced semantics: usage rows persist with
    /// `cost_microcents = 0` and NULL pricing columns.
    pub pricing_catalog: Option<PricingCatalog>,
}

impl fmt::Debug for AgentRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentRequest")
            .field("model_bytes", &self.model.len())
            .field("system_prompt_bytes", &self.system_prompt.len())
            .field("history_count", &self.history.len())
            .field("max_tokens", &self.max_tokens)
            .field(
                "completed_tool_result_count",
                &self.completed_tool_results.len(),
            )
            .field("tool_config", &self.tool_config)
            .finish()
    }
}

/// One terminal tool call recovered from persistence for idempotent retry.
#[derive(Clone, PartialEq, Eq)]
pub struct CompletedToolCall {
    /// Original tool name.
    pub tool: String,
    /// Original complete JSON input.
    pub input_json: String,
    /// Terminal result to observe again.
    pub result: RuntimeToolResult,
}

impl fmt::Debug for CompletedToolCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompletedToolCall")
            .field("tool_bytes", &self.tool.len())
            .field("input_json_bytes", &self.input_json.len())
            .field("result", &self.result)
            .finish()
    }
}

/// Why the whole runtime loop converged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFinishReason {
    /// The provider ended naturally.
    End,
    /// The provider reached its generation cap.
    Length,
    /// The task reached [`TOOL_CALL_LIMIT`].
    ToolLimit,
}

/// Exact pricing provenance stamped by the runtime for one provider call
/// (S7-T38/C3). Conversation persists these onto the `token_usage` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeUsagePricing {
    /// Exact engine version that produced the quote (e.g. `pricing_v1`).
    pub version: String,
    /// Rate profile selected by the frozen UTC timestamp.
    pub profile: String,
    /// Unix UTC seconds captured at the logical provider call start.
    pub call_started_at: i64,
}

fn unix_utc_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

/// Runtime-local token accounting. Conversation converts this into its
/// shared [`ConversationEvent`](vega_conversation) representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeTokenUsage {
    /// Prompt tokens.
    pub input: u64,
    /// Completion tokens.
    pub output: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
    /// Cache-write tokens.
    pub cache_write: u64,
}

/// Runtime-local tool call, deliberately independent from UI/conversation.
#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeToolCall {
    /// Provider-side call id.
    pub id: String,
    /// Requested tool name.
    pub name: String,
    /// Complete raw JSON input.
    pub input_json: String,
}

impl fmt::Debug for RuntimeToolCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeToolCall")
            .field("id_bytes", &self.id.len())
            .field("name_bytes", &self.name.len())
            .field("input_json_bytes", &self.input_json.len())
            .finish()
    }
}

/// Terminal status of a runtime tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeToolStatus {
    /// The S4 read-only placeholder gate denied an unknown/write tool.
    Rejected,
    /// The tool completed successfully.
    Success,
    /// The tool was approved but returned an error.
    Failed,
    /// A running tool completed after cancellation was requested.
    Cancelled,
}

/// Terminal tool result appended to provider context.
#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeToolResult {
    /// Call id.
    pub call_id: String,
    /// Text shown to the provider and persisted for audit.
    pub output: String,
    /// Terminal status.
    pub status: RuntimeToolStatus,
    /// True when a persisted result was reused instead of executing again.
    pub reused: bool,
    /// Exact process exit code for live/recovered bash calls.
    pub exit_code: Option<i32>,
    /// Exact duration for live/recovered bash calls.
    pub duration_ms: Option<u64>,
    /// Exact truncation fact for live calls; absent after persisted recovery.
    pub truncated: Option<bool>,
    /// Strict terminal approval audit for rejection/validation paths.
    pub approval: Option<RuntimeApprovalAudit>,
    /// Exact rule selected by Always even when a later ReadOnly step rejects.
    pub remember_rule: Option<RuntimePermissionTarget>,
}

impl fmt::Debug for RuntimeToolResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeToolResult")
            .field("call_id_bytes", &self.call_id.len())
            .field("output_bytes", &self.output.len())
            .field("status", &self.status)
            .field("reused", &self.reused)
            .field("exit_code", &self.exit_code)
            .field("duration_ms", &self.duration_ms)
            .field("truncated", &self.truncated)
            .field("has_approval", &self.approval.is_some())
            .field("has_remember_rule", &self.remember_rule.is_some())
            .finish()
    }
}

/// Runtime-only event stream. `vega_conversation` is responsible for
/// converting it into the sole UI/store event type.
#[derive(Clone)]
pub enum RuntimeEvent {
    /// Visible assistant delta.
    TextDelta(String),
    /// Reasoning delta.
    ThinkingDelta(String),
    /// Complete tool call before the S4 placeholder permission decision.
    ToolCallProposed(RuntimeToolCall),
    /// Invalid write/edit projection atomically reaches a terminal rejection.
    ToolCallValidationRejected {
        /// Content-free strict invalid projection.
        call: RuntimeToolCall,
        /// Content-free terminal result and validation audit.
        result: RuntimeToolResult,
    },
    /// A reused provider call id disagreed with its persisted immutable identity.
    ToolCallConflict {
        /// Current content-safe call projection.
        call: RuntimeToolCall,
        /// Stable failed result; persistence must remain unchanged.
        result: RuntimeToolResult,
    },
    /// Read-only tool auto-approved by the S4 placeholder gate.
    ToolCallApproved {
        /// Provider call id.
        call_id: String,
        /// Strict permission audit persisted before execution.
        audit: RuntimeApprovalAudit,
        /// Exact rule to persist atomically for an Always decision.
        remember_rule: Option<RuntimePermissionTarget>,
    },
    /// Approved tool began running.
    ToolCallRunning {
        /// Provider call id.
        call_id: String,
    },
    /// Complete display chunk from a tool.
    ToolCallOutput {
        /// Provider call id.
        call_id: String,
        /// Truncated display text.
        chunk: String,
    },
    /// Terminal tool result.
    ToolCallFinished(RuntimeToolResult),
    /// Provider usage priced by the frozen run-start catalog (S7-T38).
    UsageUpdated {
        /// Token counts from the provider.
        usage: RuntimeTokenUsage,
        /// Checked integer cost from the frozen catalog quote (0 for priced
        /// zero; 0 with no pricing provenance keeps the S4 legacy placeholder).
        cost_microcents: i64,
        /// Exact pricing provenance; `None` keeps S4 legacy/unpriced rows.
        pricing: Option<RuntimeUsagePricing>,
    },
    /// Natural/length/limit convergence.
    Finished(RuntimeFinishReason),
    /// Cancellation was observed.
    Interrupted,
    /// Runtime/provider error surfaced to the conversation.
    Error(Arc<VegaError>),
}

impl fmt::Debug for RuntimeEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TextDelta(value) => formatter
                .debug_tuple("TextDelta")
                .field(&format_args!("{} bytes", value.len()))
                .finish(),
            Self::ThinkingDelta(value) => formatter
                .debug_tuple("ThinkingDelta")
                .field(&format_args!("{} bytes", value.len()))
                .finish(),
            Self::ToolCallProposed(call) => formatter
                .debug_tuple("ToolCallProposed")
                .field(call)
                .finish(),
            Self::ToolCallValidationRejected { call, result } => formatter
                .debug_struct("ToolCallValidationRejected")
                .field("call", call)
                .field("result", result)
                .finish(),
            Self::ToolCallConflict { call, result } => formatter
                .debug_struct("ToolCallConflict")
                .field("call", call)
                .field("result", result)
                .finish(),
            Self::ToolCallApproved {
                call_id,
                audit: _,
                remember_rule,
            } => formatter
                .debug_struct("ToolCallApproved")
                .field("call_id_bytes", &call_id.len())
                .field("has_remember_rule", &remember_rule.is_some())
                .finish(),
            Self::ToolCallRunning { call_id } => formatter
                .debug_struct("ToolCallRunning")
                .field("call_id_bytes", &call_id.len())
                .finish(),
            Self::ToolCallOutput { call_id, chunk } => formatter
                .debug_struct("ToolCallOutput")
                .field("call_id_bytes", &call_id.len())
                .field("chunk_bytes", &chunk.len())
                .finish(),
            Self::ToolCallFinished(result) => formatter
                .debug_tuple("ToolCallFinished")
                .field(result)
                .finish(),
            Self::UsageUpdated {
                usage,
                cost_microcents,
                pricing,
            } => formatter
                .debug_struct("UsageUpdated")
                .field("usage", usage)
                .field("cost_microcents", cost_microcents)
                .field("priced", &pricing.is_some())
                .finish(),
            Self::Finished(reason) => formatter.debug_tuple("Finished").field(reason).finish(),
            Self::Interrupted => formatter.write_str("Interrupted"),
            Self::Error(_) => formatter.write_str("Error([redacted])"),
        }
    }
}

/// Complete headless task outcome.
#[derive(Clone)]
pub struct AgentOutcome {
    /// Ordered runtime events.
    pub events: Vec<RuntimeEvent>,
    /// Full provider context after convergence.
    pub messages: Vec<ChatMessage>,
    /// Concatenated visible assistant text across rounds.
    pub final_text: String,
    /// Number of provider tool-use proposals observed in this run.
    pub tool_call_count: usize,
    /// Number of read-only tools actually started in this process.
    pub executed_tool_call_count: usize,
    /// Whether cancellation ended the task.
    pub interrupted: bool,
    /// Whether a provider/runtime error ended the task.
    pub failed: bool,
}

impl fmt::Debug for AgentOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentOutcome")
            .field("event_count", &self.events.len())
            .field("message_count", &self.messages.len())
            .field("final_text_bytes", &self.final_text.len())
            .field("tool_call_count", &self.tool_call_count)
            .field("executed_tool_call_count", &self.executed_tool_call_count)
            .field("interrupted", &self.interrupted)
            .field("failed", &self.failed)
            .finish()
    }
}

mod loop_;
mod tools_exec;

#[cfg(test)]
mod tests;

pub use loop_::{run_agent, run_agent_with_permission_sink, run_agent_with_sink};
pub(crate) use tools_exec::*;
