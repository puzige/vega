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
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(600);

/// One project-scoped exact permission rule preloaded by conversation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeExactRule {
    /// Mutating tool name.
    pub tool: RuntimeMutatingTool,
    /// Byte-exact command or normalized project-relative path.
    pub pattern: String,
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
            .field("project_id", &self.project_id)
            .field("thread_id", &self.thread_id)
            .field("checkpoint_root", &"[REDACTED]")
            .field("exact_rules", &self.exact_rules)
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
#[derive(Debug, Clone)]
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
}

/// One terminal tool call recovered from persistence for idempotent retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedToolCall {
    /// Original tool name.
    pub tool: String,
    /// Original complete JSON input.
    pub input_json: String,
    /// Terminal result to observe again.
    pub result: RuntimeToolResult,
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
            .field("id", &self.id)
            .field("name", &self.name)
            .field("input_json", &"[REDACTED]")
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Runtime-only event stream. `vega_conversation` is responsible for
/// converting it into the sole UI/store event type.
#[derive(Debug, Clone)]
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
    /// Provider usage with the S4 cost hook fixed at zero.
    UsageUpdated {
        /// Token counts from the provider.
        usage: RuntimeTokenUsage,
        /// Cost hook placeholder; S7 replaces this with pricing.
        cost_microcents: i64,
    },
    /// Natural/length/limit convergence.
    Finished(RuntimeFinishReason),
    /// Cancellation was observed.
    Interrupted,
    /// Runtime/provider error surfaced to the conversation.
    Error(Arc<VegaError>),
}

/// Complete headless task outcome.
#[derive(Debug, Clone)]
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

/// Runs the S4 headless agent loop with real fenced read/glob/grep tools.
pub async fn run_agent(
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    request: AgentRequest,
    cancel: CancellationToken,
) -> Result<AgentOutcome, VegaError> {
    run_agent_with_permission_sink(
        provider,
        tools,
        request,
        cancel,
        &RejectPermissionHook,
        |_| async { Ok(()) },
    )
    .await
}

/// Runs the agent and delivers each owned runtime event to an async sink at
/// its real lifecycle boundary before the loop may continue when required.
///
/// Returning an error from `sink` stops the task immediately. Conversation
/// uses awaited acknowledgements to persist critical state before side
/// effects while allowing text deltas to enter a bounded batching pipeline.
pub async fn run_agent_with_sink<F, Fut>(
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    request: AgentRequest,
    cancel: CancellationToken,
    sink: F,
) -> Result<AgentOutcome, VegaError>
where
    F: FnMut(RuntimeEvent) -> Fut,
    Fut: Future<Output = Result<(), VegaError>>,
{
    run_agent_with_permission_sink(
        provider,
        tools,
        request,
        cancel,
        &RejectPermissionHook,
        sink,
    )
    .await
}

/// Runs the full six-tool loop with an object-safe permission hook.
pub async fn run_agent_with_permission_sink<F, Fut>(
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    request: AgentRequest,
    cancel: CancellationToken,
    permission_hook: &dyn RuntimePermissionHook,
    mut sink: F,
) -> Result<AgentOutcome, VegaError>
where
    F: FnMut(RuntimeEvent) -> Fut,
    Fut: Future<Output = Result<(), VegaError>>,
{
    macro_rules! emit {
        ($events:ident, $sink:ident, $event:expr) => {{
            let event = $event;
            $sink(event.clone()).await?;
            $events.push(event);
        }};
    }

    let mut messages = Vec::with_capacity(request.history.len() + 1);
    messages.push(ChatMessage::new(ChatRole::System, request.system_prompt));
    messages.extend(request.history);
    let mut completed = request.completed_tool_results;
    let tool_config = request.tool_config;
    let mut exact_rules: HashSet<RuntimeExactRule> =
        tool_config.exact_rules.iter().cloned().collect();
    let mut events = Vec::new();
    let mut final_text = String::new();
    let mut tool_call_count = 0usize;
    let mut executed_tool_call_count = 0usize;

    loop {
        if cancel.is_cancelled() {
            emit!(events, sink, RuntimeEvent::Interrupted);
            return Ok(outcome(
                events,
                messages,
                final_text,
                tool_call_count,
                executed_tool_call_count,
                true,
                false,
            ));
        }

        let chat_request = ChatRequest {
            model: request.model.clone(),
            messages: messages.clone(),
            tools: tool_definitions(tool_config.run_mode),
            max_tokens: request.max_tokens,
        };
        let mut stream = match provider.chat_stream(chat_request, cancel.clone()).await {
            Ok(stream) => stream,
            Err(VegaError::Cancelled) => {
                emit!(events, sink, RuntimeEvent::Interrupted);
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    executed_tool_call_count,
                    true,
                    false,
                ));
            }
            Err(error) => {
                emit!(events, sink, RuntimeEvent::Error(Arc::new(error)));
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    executed_tool_call_count,
                    false,
                    true,
                ));
            }
        };

        let mut assistant_text = String::new();
        let mut calls = Vec::new();
        let mut stop_reason = None;
        loop {
            let next = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    emit!(events, sink, RuntimeEvent::Interrupted);
                    return Ok(outcome(
                        events,
                        messages,
                        final_text,
                        tool_call_count,
                        executed_tool_call_count,
                        true,
                        false,
                    ));
                }
                next = stream.next() => next,
            };
            let Some(item) = next else { break };
            match item {
                Ok(ProviderEvent::TextDelta(delta)) => {
                    assistant_text.push_str(&delta);
                    final_text.push_str(&delta);
                    emit!(events, sink, RuntimeEvent::TextDelta(delta));
                }
                Ok(ProviderEvent::ThinkingDelta(delta)) => {
                    emit!(events, sink, RuntimeEvent::ThinkingDelta(delta));
                }
                Ok(ProviderEvent::ToolUse {
                    id,
                    name,
                    input_json,
                }) => {
                    calls.push(RuntimeToolCall {
                        id,
                        name,
                        input_json,
                    });
                }
                Ok(ProviderEvent::Usage {
                    input,
                    output,
                    cache_read,
                    cache_write,
                }) => {
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::UsageUpdated {
                            usage: RuntimeTokenUsage {
                                input,
                                output,
                                cache_read,
                                cache_write,
                            },
                            cost_microcents: 0,
                        }
                    );
                }
                Ok(ProviderEvent::Done {
                    stop_reason: reason,
                }) => stop_reason = Some(reason),
                Err(VegaError::Cancelled) => {
                    emit!(events, sink, RuntimeEvent::Interrupted);
                    return Ok(outcome(
                        events,
                        messages,
                        final_text,
                        tool_call_count,
                        executed_tool_call_count,
                        true,
                        false,
                    ));
                }
                Err(error) => {
                    emit!(events, sink, RuntimeEvent::Error(Arc::new(error)));
                    return Ok(outcome(
                        events,
                        messages,
                        final_text,
                        tool_call_count,
                        executed_tool_call_count,
                        false,
                        true,
                    ));
                }
            }
        }

        if calls.is_empty() {
            messages.push(ChatMessage::new(ChatRole::Assistant, assistant_text));
            let finish = match stop_reason.unwrap_or(StopReason::End) {
                StopReason::Length => RuntimeFinishReason::Length,
                StopReason::End | StopReason::ToolUse => RuntimeFinishReason::End,
            };
            emit!(events, sink, RuntimeEvent::Finished(finish));
            return Ok(outcome(
                events,
                messages,
                final_text,
                tool_call_count,
                executed_tool_call_count,
                false,
                false,
            ));
        }

        let mut prepared_calls = Vec::with_capacity(calls.len());
        for call in calls {
            match prepare_runtime_call(tools, &tool_config, call) {
                Ok(prepared) => prepared_calls.push(prepared),
                Err(error) => {
                    emit!(events, sink, RuntimeEvent::Error(Arc::new(error)));
                    return Ok(outcome(
                        events,
                        messages,
                        final_text,
                        tool_call_count,
                        executed_tool_call_count,
                        false,
                        true,
                    ));
                }
            }
        }
        let wire_calls = prepared_calls
            .iter()
            .map(|prepared| {
                let call = prepared.call();
                ChatToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input_json: call.input_json.clone(),
                }
            })
            .collect();
        messages.push(ChatMessage::assistant_with_tools(
            assistant_text,
            wire_calls,
        ));

        for prepared in prepared_calls {
            let call = prepared.call().clone();
            if tool_call_count >= TOOL_CALL_LIMIT {
                let notice = format!(
                    "Tool call limit ({TOOL_CALL_LIMIT}) reached; stopping without executing additional tools."
                );
                final_text.push_str(&notice);
                emit!(events, sink, RuntimeEvent::TextDelta(notice.clone()));
                messages.push(ChatMessage::new(ChatRole::Assistant, notice));
                emit!(
                    events,
                    sink,
                    RuntimeEvent::Finished(RuntimeFinishReason::ToolLimit)
                );
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    executed_tool_call_count,
                    false,
                    false,
                ));
            }
            tool_call_count += 1;
            if tool_config.foreign_call_ids.contains(&call.id) {
                let conflict = conflict_result(&call);
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ToolCallConflict {
                        call: call.clone(),
                        result: conflict.clone(),
                    }
                );
                messages.push(ChatMessage::tool_result(call.id, conflict.output));
                continue;
            }
            if let PreparedRuntimeCall::InvalidWriteEdit { result, .. } = &prepared {
                let mut terminal = terminal_result(
                    &call,
                    result.clone(),
                    RuntimeToolStatus::Rejected,
                    Some(validation_audit()),
                );
                if let Some(prior) = completed.get(&call.id).cloned() {
                    if prior.tool != call.name
                        || !runtime_inputs_semantically_equal(
                            &call.name,
                            &prior.input_json,
                            &call.input_json,
                        )
                    {
                        let conflict = conflict_result(&call);
                        emit!(
                            events,
                            sink,
                            RuntimeEvent::ToolCallConflict {
                                call: call.clone(),
                                result: conflict.clone(),
                            }
                        );
                        messages.push(ChatMessage::tool_result(&call.id, &conflict.output));
                        continue;
                    }
                    terminal = prior.result;
                    terminal.reused = true;
                    terminal.truncated = None;
                }
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ToolCallValidationRejected {
                        call: call.clone(),
                        result: terminal.clone(),
                    }
                );
                messages.push(ChatMessage::tool_result(&call.id, &terminal.output));
                completed.insert(
                    call.id.clone(),
                    CompletedToolCall {
                        tool: call.name.clone(),
                        input_json: call.input_json.clone(),
                        result: terminal,
                    },
                );
                continue;
            }
            if let Some(prior) = completed.get(&call.id).cloned() {
                if prior.tool != call.name
                    || !runtime_inputs_semantically_equal(
                        &call.name,
                        &prior.input_json,
                        &call.input_json,
                    )
                {
                    let conflict = conflict_result(&call);
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ToolCallConflict {
                            call: call.clone(),
                            result: conflict.clone(),
                        }
                    );
                    messages.push(ChatMessage::tool_result(call.id, conflict.output));
                    continue;
                }
                emit!(events, sink, RuntimeEvent::ToolCallProposed(call.clone()));
                let mut result = prior.result;
                result.reused = true;
                result.truncated = None;
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ToolCallOutput {
                        call_id: call.id.clone(),
                        chunk: result.output.clone(),
                    }
                );
                emit!(events, sink, RuntimeEvent::ToolCallFinished(result.clone()));
                messages.push(ChatMessage::tool_result(call.id, result.output));
                continue;
            }
            if cancel.is_cancelled() {
                emit!(events, sink, RuntimeEvent::Interrupted);
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    executed_tool_call_count,
                    true,
                    false,
                ));
            }
            emit!(events, sink, RuntimeEvent::ToolCallProposed(call.clone()));

            let authorization = authorize_call(
                &prepared,
                &tool_config,
                &exact_rules,
                permission_hook,
                &cancel,
            )
            .await?;
            let (mut result, cancelled_while_running) = match authorization {
                Authorization::Terminal(result) => (result, false),
                Authorization::Approved {
                    audit,
                    remember_rule,
                } => {
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ToolCallApproved {
                            call_id: call.id.clone(),
                            audit: audit.clone(),
                            remember_rule: remember_rule.clone(),
                        }
                    );
                    if let Some(target) = remember_rule {
                        exact_rules.insert(RuntimeExactRule {
                            tool: target.tool,
                            pattern: target.exact_pattern,
                        });
                    }
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ToolCallRunning {
                            call_id: call.id.clone(),
                        }
                    );
                    if cancel.is_cancelled() {
                        (
                            terminal_result(
                                &call,
                                CANCELLED_BEFORE_EXECUTION_OUTPUT.to_string(),
                                RuntimeToolStatus::Cancelled,
                                Some(audit),
                            ),
                            true,
                        )
                    } else {
                        executed_tool_call_count += 1;
                        let (mut result, cancelled) =
                            execute_prepared_waiting(prepared, tools, &cancel).await;
                        result.approval = Some(audit);
                        (result, cancelled)
                    }
                }
            };
            if cancelled_while_running {
                result.status = RuntimeToolStatus::Cancelled;
            }
            emit!(
                events,
                sink,
                RuntimeEvent::ToolCallOutput {
                    call_id: call.id.clone(),
                    chunk: result.output.clone(),
                }
            );
            emit!(events, sink, RuntimeEvent::ToolCallFinished(result.clone()));
            if let Some(target) = &result.remember_rule {
                exact_rules.insert(RuntimeExactRule {
                    tool: target.tool,
                    pattern: target.exact_pattern.clone(),
                });
            }
            messages.push(ChatMessage::tool_result(&call.id, &result.output));
            completed.insert(
                call.id,
                CompletedToolCall {
                    tool: call.name,
                    input_json: call.input_json,
                    result,
                },
            );

            if cancelled_while_running || cancel.is_cancelled() {
                emit!(events, sink, RuntimeEvent::Interrupted);
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    executed_tool_call_count,
                    true,
                    false,
                ));
            }
        }
    }
}

enum PreparedRuntimeCall {
    Readonly(RuntimeToolCall),
    Write {
        call: RuntimeToolCall,
        tools: vega_tools::Tools,
        prepared: vega_tools::PreparedWrite,
    },
    Edit {
        call: RuntimeToolCall,
        tools: vega_tools::Tools,
        prepared: vega_tools::PreparedEdit,
    },
    Bash {
        call: RuntimeToolCall,
        tools: vega_tools::Tools,
        prepared: vega_tools::PreparedBash,
    },
    InvalidWriteEdit {
        call: RuntimeToolCall,
        result: String,
    },
    RunModeMutation(RuntimeToolCall),
    InvalidBash {
        call: RuntimeToolCall,
        code: vega_tools::BashErrorCode,
    },
    Unknown(RuntimeToolCall),
}

impl fmt::Debug for PreparedRuntimeCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRuntimeCall")
            .field("call", self.call())
            .field("private_input", &"[REDACTED]")
            .finish()
    }
}

impl PreparedRuntimeCall {
    fn call(&self) -> &RuntimeToolCall {
        match self {
            Self::Readonly(call)
            | Self::Unknown(call)
            | Self::RunModeMutation(call)
            | Self::InvalidWriteEdit { call, .. }
            | Self::InvalidBash { call, .. }
            | Self::Write { call, .. }
            | Self::Edit { call, .. }
            | Self::Bash { call, .. } => call,
        }
    }

    fn permission_target(&self) -> Option<RuntimePermissionTarget> {
        match self {
            Self::Write { call, prepared, .. } => Some(RuntimePermissionTarget {
                call_id: call.id.clone(),
                tool: RuntimeMutatingTool::Write,
                exact_pattern: prepared.normalized_path().to_string(),
                display_target: prepared.normalized_path().to_string(),
            }),
            Self::Edit { call, prepared, .. } => Some(RuntimePermissionTarget {
                call_id: call.id.clone(),
                tool: RuntimeMutatingTool::Edit,
                exact_pattern: prepared.normalized_path().to_string(),
                display_target: prepared.normalized_path().to_string(),
            }),
            Self::Bash { call, prepared, .. } => Some(RuntimePermissionTarget {
                call_id: call.id.clone(),
                tool: RuntimeMutatingTool::Bash,
                exact_pattern: prepared.command().to_string(),
                display_target: prepared.command().to_string(),
            }),
            _ => None,
        }
    }
}

enum Authorization {
    Approved {
        audit: RuntimeApprovalAudit,
        remember_rule: Option<RuntimePermissionTarget>,
    },
    Terminal(RuntimeToolResult),
}

fn prepare_runtime_call(
    base_tools: &vega_tools::Tools,
    config: &RuntimeToolConfig,
    raw_call: RuntimeToolCall,
) -> Result<PreparedRuntimeCall, VegaError> {
    match raw_call.name.as_str() {
        "read" | "glob" | "grep" => Ok(PreparedRuntimeCall::Readonly(raw_call)),
        "write" | "edit" => {
            let tool = if raw_call.name == "write" {
                vega_tools::MutationTool::Write
            } else {
                vega_tools::MutationTool::Edit
            };
            let audit = if tool == vega_tools::MutationTool::Write {
                base_tools.audit_write_json(&raw_call.input_json)
            } else {
                base_tools.audit_edit_json(&raw_call.input_json)
            };
            let audit = match audit {
                Ok(audit) => audit,
                Err(vega_tools::PrepareMutationError::Invalid(invalid)) => {
                    return invalid_runtime_call(raw_call, invalid);
                }
                Err(vega_tools::PrepareMutationError::Internal(_)) => {
                    return Err(safe_prepare_error(&raw_call.name));
                }
            };
            if vega_tools::CheckpointIds::new(&config.project_id, &config.thread_id, &raw_call.id)
                .is_err()
            {
                let invalid = vega_tools::InvalidMutation::from_raw(
                    tool,
                    &raw_call.input_json,
                    vega_tools::MutationErrorCode::CheckpointIdInvalid,
                )
                .map_err(|_| safe_prepare_error(&raw_call.name))?;
                return invalid_runtime_call(raw_call, invalid);
            }
            if config.run_mode != RuntimeRunMode::Execute {
                let safe_json = audit
                    .to_json()
                    .map_err(|_| safe_prepare_error(&raw_call.name))?;
                return Ok(PreparedRuntimeCall::RunModeMutation(RuntimeToolCall {
                    input_json: safe_json,
                    ..raw_call
                }));
            }
            let scoped = match base_tools.clone().with_mutation_context(
                &config.checkpoint_root,
                &config.project_id,
                &config.thread_id,
                &raw_call.id,
            ) {
                Ok(scoped) => scoped,
                Err(vega_tools::ToolError::Mutation(error))
                    if error.code() == vega_tools::MutationErrorCode::CheckpointIdInvalid =>
                {
                    let invalid = vega_tools::InvalidMutation::from_raw(
                        tool,
                        &raw_call.input_json,
                        vega_tools::MutationErrorCode::CheckpointIdInvalid,
                    )
                    .map_err(|_| safe_prepare_error(&raw_call.name))?;
                    return invalid_runtime_call(raw_call, invalid);
                }
                Err(_) => return Err(safe_prepare_error(&raw_call.name)),
            };
            if tool == vega_tools::MutationTool::Write {
                match scoped.prepare_write_json(&raw_call.input_json) {
                    Ok(prepared) => {
                        let safe_json = prepared
                            .audit()
                            .to_json()
                            .map_err(|_| safe_prepare_error("write"))?;
                        Ok(PreparedRuntimeCall::Write {
                            call: RuntimeToolCall {
                                input_json: safe_json,
                                ..raw_call
                            },
                            tools: scoped,
                            prepared,
                        })
                    }
                    Err(vega_tools::PrepareMutationError::Invalid(invalid)) => {
                        invalid_runtime_call(raw_call, invalid)
                    }
                    Err(vega_tools::PrepareMutationError::Internal(_)) => {
                        Err(safe_prepare_error("write"))
                    }
                }
            } else {
                match scoped.prepare_edit_json(&raw_call.input_json) {
                    Ok(prepared) => {
                        let safe_json = prepared
                            .audit()
                            .to_json()
                            .map_err(|_| safe_prepare_error("edit"))?;
                        Ok(PreparedRuntimeCall::Edit {
                            call: RuntimeToolCall {
                                input_json: safe_json,
                                ..raw_call
                            },
                            tools: scoped,
                            prepared,
                        })
                    }
                    Err(vega_tools::PrepareMutationError::Invalid(invalid)) => {
                        invalid_runtime_call(raw_call, invalid)
                    }
                    Err(vega_tools::PrepareMutationError::Internal(_)) => {
                        Err(safe_prepare_error("edit"))
                    }
                }
            }
        }
        "bash" if config.run_mode != RuntimeRunMode::Execute => {
            Ok(PreparedRuntimeCall::RunModeMutation(raw_call))
        }
        "bash" => match base_tools.prepare_bash_json(&raw_call.input_json) {
            Ok(prepared) => Ok(PreparedRuntimeCall::Bash {
                call: raw_call,
                tools: base_tools.clone(),
                prepared,
            }),
            Err(error) => Ok(PreparedRuntimeCall::InvalidBash {
                call: raw_call,
                code: error.code(),
            }),
        },
        _ => Ok(PreparedRuntimeCall::Unknown(RuntimeToolCall {
            input_json: "{}".to_string(),
            ..raw_call
        })),
    }
}

fn invalid_runtime_call(
    raw_call: RuntimeToolCall,
    invalid: vega_tools::InvalidMutation,
) -> Result<PreparedRuntimeCall, VegaError> {
    let safe_json = invalid
        .audit()
        .to_json()
        .map_err(|_| safe_prepare_error(&raw_call.name))?;
    Ok(PreparedRuntimeCall::InvalidWriteEdit {
        call: RuntimeToolCall {
            input_json: safe_json,
            ..raw_call
        },
        result: invalid.tool_result().to_string(),
    })
}

fn safe_prepare_error(tool: &str) -> VegaError {
    VegaError::Tool {
        tool: tool.to_string(),
        message: "safe input preparation failed".to_string(),
    }
}

async fn authorize_call(
    prepared: &PreparedRuntimeCall,
    config: &RuntimeToolConfig,
    exact_rules: &HashSet<RuntimeExactRule>,
    hook: &dyn RuntimePermissionHook,
    cancel: &CancellationToken,
) -> Result<Authorization, VegaError> {
    match prepared {
        PreparedRuntimeCall::InvalidWriteEdit { call, result } => {
            Ok(Authorization::Terminal(terminal_result(
                call,
                result.clone(),
                RuntimeToolStatus::Rejected,
                Some(validation_audit()),
            )))
        }
        PreparedRuntimeCall::RunModeMutation(call) => Ok(Authorization::Terminal(terminal_result(
            call,
            "Tool error: denied by run mode".to_string(),
            RuntimeToolStatus::Rejected,
            Some(run_mode_denial()),
        ))),
        PreparedRuntimeCall::InvalidBash { call, code } => {
            Ok(Authorization::Terminal(terminal_result(
                call,
                format!("Tool error: invalid bash input ({})", code.as_str()),
                RuntimeToolStatus::Rejected,
                Some(validation_audit()),
            )))
        }
        PreparedRuntimeCall::Unknown(call) => Ok(Authorization::Terminal(terminal_result(
            call,
            "Tool error: denied: unavailable tool".to_string(),
            RuntimeToolStatus::Rejected,
            Some(run_mode_denial()),
        ))),
        PreparedRuntimeCall::Readonly(_) => {
            if cancel.is_cancelled() {
                return Ok(cancelled_permission(prepared.call()));
            }
            match decide_capability(config.run_mode, RuntimeToolClass::Readonly) {
                RuntimeCapabilityOutcome::Approved(audit) => Ok(Authorization::Approved {
                    audit,
                    remember_rule: None,
                }),
                RuntimeCapabilityOutcome::Rejected(audit) => {
                    Ok(Authorization::Terminal(terminal_result(
                        prepared.call(),
                        "Tool error: denied".to_string(),
                        RuntimeToolStatus::Rejected,
                        Some(audit),
                    )))
                }
                RuntimeCapabilityOutcome::ExecuteEligible(_) => Err(safe_permission_error()),
            }
        }
        PreparedRuntimeCall::Write { .. }
        | PreparedRuntimeCall::Edit { .. }
        | PreparedRuntimeCall::Bash { .. } => {
            let target = prepared
                .permission_target()
                .ok_or_else(safe_permission_error)?;
            let capability =
                decide_capability(config.run_mode, RuntimeToolClass::Mutating(target.clone()));
            let RuntimeCapabilityOutcome::ExecuteEligible(eligibility) = capability else {
                return match capability {
                    RuntimeCapabilityOutcome::Rejected(audit) => {
                        Ok(Authorization::Terminal(terminal_result(
                            prepared.call(),
                            "Tool error: denied by run mode".to_string(),
                            RuntimeToolStatus::Rejected,
                            Some(audit),
                        )))
                    }
                    _ => Err(safe_permission_error()),
                };
            };
            let danger = if let PreparedRuntimeCall::Bash { prepared, .. } = prepared {
                vega_tools::danger::detect_danger(prepared.command())
                    .map_err(|_| safe_permission_error())?
                    .map(|danger| RuntimeDangerFacts {
                        rule_id: danger.rule_id.to_string(),
                        reason: danger.reason.to_string(),
                    })
            } else {
                None
            };
            if cancel.is_cancelled() && danger.is_none() {
                return Ok(cancelled_permission(prepared.call()));
            }
            let exact_rule_matches = exact_rules.contains(&RuntimeExactRule {
                tool: target.tool,
                pattern: target.exact_pattern.clone(),
            });
            decide_mutating_permission(
                eligibility,
                target,
                config.permission_mode,
                danger,
                exact_rule_matches,
                config.permission_timeout,
                hook,
                cancel,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn decide_mutating_permission(
    eligibility: crate::RuntimeExecuteEligibility,
    target: RuntimePermissionTarget,
    permission_mode: RuntimePermissionMode,
    danger: Option<RuntimeDangerFacts>,
    exact_rule_matches: bool,
    timeout: Duration,
    hook: &dyn RuntimePermissionHook,
    cancel: &CancellationToken,
) -> Result<Authorization, VegaError> {
    let facts = RuntimeExecutePermission {
        permission_mode,
        target: target.clone(),
        danger: danger.clone(),
        exact_rule_matches,
        danger_response: None,
        ordinary_response: None,
    };
    let initial =
        decide_execute_permission(eligibility, facts).map_err(|_| safe_permission_error())?;
    let RuntimePermissionOutcome::Prompt(prompt) = initial else {
        return permission_outcome(target, initial);
    };
    let (decision, _) = wait_for_permission(hook, prompt.clone(), timeout, cancel).await;
    let next_eligibility = match decide_capability(
        RuntimeRunMode::Execute,
        RuntimeToolClass::Mutating(target.clone()),
    ) {
        RuntimeCapabilityOutcome::ExecuteEligible(eligibility) => eligibility,
        _ => return Err(safe_permission_error()),
    };
    let response_facts = RuntimeExecutePermission {
        permission_mode,
        target: target.clone(),
        danger,
        exact_rule_matches,
        danger_response: prompt.danger.as_ref().map(|_| decision.clone()),
        ordinary_response: prompt.danger.is_none().then_some(decision),
    };
    let outcome = decide_execute_permission(next_eligibility, response_facts)
        .map_err(|_| safe_permission_error())?;
    permission_outcome(target, outcome)
}

fn permission_outcome(
    target: RuntimePermissionTarget,
    outcome: RuntimePermissionOutcome,
) -> Result<Authorization, VegaError> {
    match outcome {
        RuntimePermissionOutcome::Approved {
            audit,
            remember_rule,
        } => Ok(Authorization::Approved {
            audit,
            remember_rule: remember_rule.then_some(target),
        }),
        RuntimePermissionOutcome::Rejected {
            audit,
            remember_rule,
        } => {
            let mut result = terminal_result(
                &RuntimeToolCall {
                    id: target.call_id.clone(),
                    name: target.tool.as_str().to_string(),
                    input_json: String::new(),
                },
                "Tool error: permission denied".to_string(),
                RuntimeToolStatus::Rejected,
                Some(audit),
            );
            result.remember_rule = remember_rule.then_some(target);
            Ok(Authorization::Terminal(result))
        }
        RuntimePermissionOutcome::Prompt(_) => Err(safe_permission_error()),
    }
}

async fn wait_for_permission(
    hook: &dyn RuntimePermissionHook,
    prompt: RuntimePermissionPrompt,
    timeout: Duration,
    cancel: &CancellationToken,
) -> (RuntimeUserDecision, bool) {
    let prompt_cancel = cancel.child_token();
    let future = hook.request(prompt, prompt_cancel.clone());
    let waited = tokio::select! {
        biased;
        _ = cancel.cancelled() => (RuntimeUserDecision::Timeout, true),
        _ = tokio::time::sleep(timeout) => (RuntimeUserDecision::Timeout, false),
        decision = future => (decision.unwrap_or(RuntimeUserDecision::Timeout), false),
    };
    prompt_cancel.cancel();
    waited
}

fn validation_audit() -> RuntimeApprovalAudit {
    RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Deny,
        note: None,
        source: RuntimeApprovalSource::Validation,
        danger: None,
    }
}

fn run_mode_denial() -> RuntimeApprovalAudit {
    RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Deny,
        note: None,
        source: RuntimeApprovalSource::RunMode,
        danger: None,
    }
}

fn cancelled_permission(call: &RuntimeToolCall) -> Authorization {
    Authorization::Terminal(terminal_result(
        call,
        "Tool error: permission denied".to_string(),
        RuntimeToolStatus::Rejected,
        Some(RuntimeApprovalAudit {
            decision: RuntimeApprovalDecision::Deny,
            note: None,
            source: RuntimeApprovalSource::Timeout,
            danger: None,
        }),
    ))
}

fn safe_permission_error() -> VegaError {
    VegaError::Tool {
        tool: "permission".to_string(),
        message: "permission decision failed closed".to_string(),
    }
}

fn terminal_result(
    call: &RuntimeToolCall,
    output: String,
    status: RuntimeToolStatus,
    approval: Option<RuntimeApprovalAudit>,
) -> RuntimeToolResult {
    RuntimeToolResult {
        call_id: call.id.clone(),
        output,
        status,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval,
        remember_rule: None,
    }
}

fn conflict_result(call: &RuntimeToolCall) -> RuntimeToolResult {
    terminal_result(
        call,
        CALL_ID_CONFLICT_OUTPUT.to_string(),
        RuntimeToolStatus::Failed,
        None,
    )
}

fn runtime_inputs_semantically_equal(tool: &str, left: &str, right: &str) -> bool {
    if !matches!(tool, "write" | "edit") {
        return left == right;
    }
    if let (Ok(left), Ok(right)) = (
        vega_tools::WriteEditAudit::from_json(left),
        vega_tools::WriteEditAudit::from_json(right),
    ) {
        return left.tool().as_str() == tool && right.tool().as_str() == tool && left == right;
    }
    if let (Ok(left), Ok(right)) = (
        vega_tools::InvalidWriteEditAudit::from_json(left),
        vega_tools::InvalidWriteEditAudit::from_json(right),
    ) {
        return left.tool().as_str() == tool && right.tool().as_str() == tool && left == right;
    }
    false
}

async fn execute_prepared_waiting(
    prepared: PreparedRuntimeCall,
    base_tools: &vega_tools::Tools,
    cancel: &CancellationToken,
) -> (RuntimeToolResult, bool) {
    match prepared {
        PreparedRuntimeCall::Readonly(call) => {
            execute_readonly_waiting(base_tools, &call, cancel).await
        }
        PreparedRuntimeCall::Write {
            call,
            tools,
            prepared,
        } => {
            let call_for_worker = call.clone();
            let worker_cancel = cancel.child_token();
            let mut task = tokio::task::spawn_blocking(move || {
                if worker_cancel.is_cancelled() {
                    return terminal_result(
                        &call_for_worker,
                        CANCELLED_BEFORE_EXECUTION_OUTPUT.to_string(),
                        RuntimeToolStatus::Cancelled,
                        None,
                    );
                }
                let result = tools.execute_write(prepared);
                mutation_result(&call_for_worker, result, true)
            });
            wait_blocking_result(&call, &mut task, cancel).await
        }
        PreparedRuntimeCall::Edit {
            call,
            tools,
            prepared,
        } => {
            let call_for_worker = call.clone();
            let worker_cancel = cancel.child_token();
            let mut task = tokio::task::spawn_blocking(move || {
                if worker_cancel.is_cancelled() {
                    return terminal_result(
                        &call_for_worker,
                        CANCELLED_BEFORE_EXECUTION_OUTPUT.to_string(),
                        RuntimeToolStatus::Cancelled,
                        None,
                    );
                }
                let result = tools.execute_edit(prepared);
                mutation_result(&call_for_worker, result, false)
            });
            wait_blocking_result(&call, &mut task, cancel).await
        }
        PreparedRuntimeCall::Bash {
            call,
            tools,
            prepared,
        } => {
            let result = tools.execute_bash(prepared, cancel.child_token()).await;
            match result {
                Ok(output) => (
                    RuntimeToolResult {
                        call_id: call.id,
                        output: output.text,
                        status: RuntimeToolStatus::Success,
                        reused: false,
                        exit_code: Some(output.exit_code),
                        duration_ms: Some(output.duration_ms),
                        truncated: Some(output.truncated),
                        approval: None,
                        remember_rule: None,
                    },
                    false,
                ),
                Err(error) => {
                    let cancelled = error.code() == vega_tools::BashErrorCode::Cancelled;
                    (
                        terminal_result(
                            &call,
                            format!("Tool error: bash failed ({})", error.code().as_str()),
                            if cancelled {
                                RuntimeToolStatus::Cancelled
                            } else {
                                RuntimeToolStatus::Failed
                            },
                            None,
                        ),
                        cancelled,
                    )
                }
            }
        }
        PreparedRuntimeCall::InvalidWriteEdit { call, result } => (
            terminal_result(
                &call,
                result,
                RuntimeToolStatus::Rejected,
                Some(validation_audit()),
            ),
            false,
        ),
        PreparedRuntimeCall::RunModeMutation(call) => (
            terminal_result(
                &call,
                "Tool error: denied by run mode".to_string(),
                RuntimeToolStatus::Rejected,
                Some(run_mode_denial()),
            ),
            false,
        ),
        PreparedRuntimeCall::InvalidBash { call, code } => (
            terminal_result(
                &call,
                format!("Tool error: invalid bash input ({})", code.as_str()),
                RuntimeToolStatus::Rejected,
                Some(validation_audit()),
            ),
            false,
        ),
        PreparedRuntimeCall::Unknown(call) => (
            terminal_result(
                &call,
                "Tool error: denied: unavailable tool".to_string(),
                RuntimeToolStatus::Rejected,
                Some(run_mode_denial()),
            ),
            false,
        ),
    }
}

async fn wait_blocking_result(
    call: &RuntimeToolCall,
    task: &mut tokio::task::JoinHandle<RuntimeToolResult>,
    cancel: &CancellationToken,
) -> (RuntimeToolResult, bool) {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let result = match (&mut *task).await {
                Ok(result) => result,
                Err(_) => terminal_result(call, "Tool error: tool worker failed".to_string(), RuntimeToolStatus::Failed, None),
            };
            (result, true)
        }
        joined = &mut *task => {
            let result = joined.unwrap_or_else(|_| terminal_result(call, "Tool error: tool worker failed".to_string(), RuntimeToolStatus::Failed, None));
            (result, false)
        }
    }
}

fn mutation_result(
    call: &RuntimeToolCall,
    result: Result<vega_tools::ToolOutput, vega_tools::ToolError>,
    write: bool,
) -> RuntimeToolResult {
    match result {
        Ok(output) => {
            let strict = if write {
                vega_tools::WriteSuccessOutput::from_json(&output.text).is_ok()
            } else {
                vega_tools::EditSuccessOutput::from_json(&output.text).is_ok()
            };
            if strict {
                RuntimeToolResult {
                    call_id: call.id.clone(),
                    output: output.text,
                    status: RuntimeToolStatus::Success,
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: Some(output.truncated),
                    approval: None,
                    remember_rule: None,
                }
            } else {
                terminal_result(
                    call,
                    "Tool error: invalid mutation result".to_string(),
                    RuntimeToolStatus::Failed,
                    None,
                )
            }
        }
        Err(_) => terminal_result(
            call,
            format!("Tool error: {} failed", call.name),
            RuntimeToolStatus::Failed,
            None,
        ),
    }
}

fn outcome(
    events: Vec<RuntimeEvent>,
    messages: Vec<ChatMessage>,
    final_text: String,
    tool_call_count: usize,
    executed_tool_call_count: usize,
    interrupted: bool,
    failed: bool,
) -> AgentOutcome {
    AgentOutcome {
        events,
        messages,
        final_text,
        tool_call_count,
        executed_tool_call_count,
        interrupted,
        failed,
    }
}

async fn execute_readonly_waiting(
    tools: &vega_tools::Tools,
    call: &RuntimeToolCall,
    cancel: &CancellationToken,
) -> (RuntimeToolResult, bool) {
    let owned_tools = tools.clone();
    let owned_call = call.clone();
    let worker_cancel = cancel.child_token();
    let mut task = tokio::task::spawn_blocking(move || {
        if worker_cancel.is_cancelled() {
            terminal_result(
                &owned_call,
                CANCELLED_BEFORE_EXECUTION_OUTPUT.to_string(),
                RuntimeToolStatus::Cancelled,
                None,
            )
        } else {
            execute_readonly(&owned_tools, &owned_call)
        }
    });
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let result = match task.await {
                Ok(result) => result,
                Err(error) => failed_tool_result(call, format!("tool worker failed: {error}")),
            };
            (result, true)
        }
        result = &mut task => {
            let result = match result {
                Ok(result) => result,
                Err(error) => failed_tool_result(call, format!("tool worker failed: {error}")),
            };
            (result, false)
        }
    }
}

fn execute_readonly(tools: &vega_tools::Tools, call: &RuntimeToolCall) -> RuntimeToolResult {
    let result =
        parse_input(&call.name, &call.input_json).and_then(|input| match call.name.as_str() {
            "read" => {
                let path = required_str(&input, "path")?;
                let offset = optional_usize(&input, "offset")?;
                let limit = optional_usize(&input, "limit")?;
                tools
                    .read(path, offset, limit)
                    .map_err(|error| error.to_string())
            }
            "glob" => {
                let pattern = required_str(&input, "pattern")?;
                tools.glob(pattern).map_err(|error| error.to_string())
            }
            "grep" => {
                let pattern = required_str(&input, "pattern")?;
                let path = optional_str(&input, "path")?;
                tools.grep(pattern, path).map_err(|error| error.to_string())
            }
            _ => Err("permission gate rejected a non-readonly tool".to_string()),
        });
    match result {
        Ok(output) => RuntimeToolResult {
            call_id: call.id.clone(),
            output: truncate_output_lines(&output.text),
            status: RuntimeToolStatus::Success,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: Some(output.truncated),
            approval: None,
            remember_rule: None,
        },
        Err(message) => RuntimeToolResult {
            call_id: call.id.clone(),
            output: format!("Tool error: {message}"),
            status: RuntimeToolStatus::Failed,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            approval: None,
            remember_rule: None,
        },
    }
}

fn failed_tool_result(call: &RuntimeToolCall, message: String) -> RuntimeToolResult {
    RuntimeToolResult {
        call_id: call.id.clone(),
        output: format!("Tool error: {message}"),
        status: RuntimeToolStatus::Failed,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval: None,
        remember_rule: None,
    }
}

fn parse_input(tool: &str, input_json: &str) -> Result<Value, String> {
    let input: Value = serde_json::from_str(input_json)
        .map_err(|error| format!("invalid {tool} input JSON: {error}"))?;
    if input.is_object() {
        Ok(input)
    } else {
        Err(format!("{tool} input must be a JSON object"))
    }
}

fn required_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing or non-string '{key}'"))
}

fn optional_str<'a>(input: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| format!("'{key}' must be a string")),
    }
}

fn optional_usize(input: &Value, key: &str) -> Result<Option<usize>, String> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let raw = value
                .as_u64()
                .ok_or_else(|| format!("'{key}' must be a non-negative integer"))?;
            usize::try_from(raw)
                .map(Some)
                .map_err(|_| format!("'{key}' exceeds this platform's integer range"))
        }
    }
}

fn truncate_output_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= OUTPUT_HALF_LINES * 2 {
        return text.to_string();
    }
    let mut kept = Vec::with_capacity(OUTPUT_HALF_LINES * 2 + 1);
    kept.extend_from_slice(&lines[..OUTPUT_HALF_LINES]);
    kept.push(OUTPUT_TRUNCATION_MARKER);
    kept.extend_from_slice(&lines[lines.len() - OUTPUT_HALF_LINES..]);
    kept.join("\n")
}

fn tool_definitions(run_mode: RuntimeRunMode) -> Vec<ToolDefinition> {
    let mut definitions = vec![
        ToolDefinition {
            name: "read".to_string(),
            description: "Read a project-relative text file with line numbers.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 1 },
                    "limit": { "type": "integer", "minimum": 0 }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "glob".to_string(),
            description: "List project files matching a gitignore-style glob.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "pattern": { "type": "string" } },
                "required": ["pattern"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "grep".to_string(),
            description: "Search project files with a regular expression.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
        },
    ];
    if run_mode == RuntimeRunMode::Execute {
        definitions.extend([
            ToolDefinition {
                name: "write".to_string(),
                description: "Write a project-relative file after permission approval.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"],
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "edit".to_string(),
                description: "Replace one unique string in a project-relative file after approval."
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "old_string": { "type": "string" },
                        "new_string": { "type": "string" }
                    },
                    "required": ["path", "old_string", "new_string"],
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "bash".to_string(),
                description: "Run a sandboxed command at the project root after approval."
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "cmd": { "type": "string" },
                        "timeout_ms": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["cmd"],
                    "additionalProperties": false
                }),
            },
        ]);
    }
    definitions
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    use super::*;
    use crate::{MockProvider, ScriptStep};

    struct FixedHook {
        calls: Arc<AtomicUsize>,
        decision: Option<RuntimeUserDecision>,
    }

    impl RuntimePermissionHook for FixedHook {
        fn request(
            &self,
            _prompt: RuntimePermissionPrompt,
            _cancel: CancellationToken,
        ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let decision = self.decision.clone();
            async move {
                match decision {
                    Some(decision) => Ok(decision),
                    None => futures::future::pending().await,
                }
            }
            .boxed()
        }
    }

    struct ProbeHook {
        fail: bool,
        token: Arc<Mutex<Option<CancellationToken>>>,
    }

    impl RuntimePermissionHook for ProbeHook {
        fn request(
            &self,
            _prompt: RuntimePermissionPrompt,
            cancel: CancellationToken,
        ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
            if let Ok(mut stored) = self.token.lock() {
                *stored = Some(cancel);
            }
            let fail = self.fail;
            async move {
                if fail {
                    Err(VegaError::Tool {
                        tool: "permission".to_string(),
                        message: "closed".to_string(),
                    })
                } else {
                    Ok(RuntimeUserDecision::Once)
                }
            }
            .boxed()
        }
    }

    fn tool_config(
        run_mode: RuntimeRunMode,
        permission_mode: RuntimePermissionMode,
        checkpoint_root: PathBuf,
    ) -> RuntimeToolConfig {
        RuntimeToolConfig::new(
            run_mode,
            permission_mode,
            "project-1".to_string(),
            "thread-1".to_string(),
            checkpoint_root,
            Vec::new(),
        )
    }

    fn request(history: Vec<ChatMessage>) -> AgentRequest {
        AgentRequest {
            model: "mock".to_string(),
            system_prompt: "Be precise.".to_string(),
            history,
            max_tokens: None,
            completed_tool_results: HashMap::new(),
            tool_config: RuntimeToolConfig::default(),
        }
    }

    async fn run_bash_permission_case(
        permission_mode: RuntimePermissionMode,
        exact_rule: bool,
        decision: RuntimeUserDecision,
        command: &str,
    ) -> (AgentOutcome, usize) {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        let checkpoint = data.path().join("checkpoints");
        fs::create_dir(&checkpoint).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "bash-case".into(),
                    name: "bash".into(),
                    input_json: serde_json::json!({ "cmd": command }).to_string(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let mut req = request(Vec::new());
        let mut config = tool_config(RuntimeRunMode::Execute, permission_mode, checkpoint);
        if exact_rule {
            config.exact_rules.push(RuntimeExactRule {
                tool: RuntimeMutatingTool::Bash,
                pattern: command.to_string(),
            });
        }
        req.tool_config = config;
        let calls = Arc::new(AtomicUsize::new(0));
        let hook = FixedHook {
            calls: calls.clone(),
            decision: Some(decision),
        };
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        (outcome, calls.load(Ordering::SeqCst))
    }

    #[test]
    fn run_modes_advertise_exact_three_or_six_strict_tools() {
        for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
            let tools = tool_definitions(mode);
            assert_eq!(
                tools
                    .iter()
                    .map(|tool| tool.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["read", "glob", "grep"]
            );
            assert!(
                tools
                    .iter()
                    .all(|tool| tool.input_schema["additionalProperties"] == false)
            );
        }
        let tools = tool_definitions(RuntimeRunMode::Execute);
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["read", "glob", "grep", "write", "edit", "bash"]
        );
        assert!(
            tools
                .iter()
                .all(|tool| tool.input_schema["additionalProperties"] == false)
        );
    }

    #[tokio::test]
    async fn permission_wait_is_first_wins_fail_closed_and_cancels_child_token() {
        let target = RuntimePermissionTarget {
            call_id: "call".to_string(),
            tool: RuntimeMutatingTool::Write,
            exact_pattern: "file.txt".to_string(),
            display_target: "file.txt".to_string(),
        };
        let prompt = RuntimePermissionPrompt {
            target,
            danger: None,
        };

        let captured = Arc::new(Mutex::new(None));
        let (decision, cancelled) = wait_for_permission(
            &ProbeHook {
                fail: false,
                token: captured.clone(),
            },
            prompt.clone(),
            Duration::from_secs(60),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(decision, RuntimeUserDecision::Once);
        assert!(!cancelled);
        assert!(
            captured
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
        );

        let captured = Arc::new(Mutex::new(None));
        let (decision, cancelled) = wait_for_permission(
            &ProbeHook {
                fail: true,
                token: captured.clone(),
            },
            prompt.clone(),
            Duration::from_secs(60),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(decision, RuntimeUserDecision::Timeout);
        assert!(!cancelled);
        assert!(
            captured
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
        );

        let cancel = CancellationToken::new();
        cancel.cancel();
        let (decision, cancelled) = wait_for_permission(
            &ProbeHook {
                fail: false,
                token: Arc::new(Mutex::new(None)),
            },
            prompt,
            Duration::from_secs(60),
            &cancel,
        )
        .await;
        assert_eq!(decision, RuntimeUserDecision::Timeout);
        assert!(cancelled);
    }

    #[tokio::test]
    async fn ask_valid_mutations_are_safe_run_mode_rejections_without_hook_or_execution() {
        for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
            let project = tempdir().unwrap();
            let data = tempdir().unwrap();
            fs::write(project.path().join("note.txt"), "old").unwrap();
            let checkpoint = data.path().join("must-not-be-created");
            let tools = vega_tools::Tools::new(project.path()).unwrap();
            let provider = MockProvider::new_rounds(vec![
                vec![ScriptStep::events(vec![
                    ProviderEvent::ToolUse {
                        id: "write-1".into(),
                        name: "write".into(),
                        input_json: r#"{"path":"new.txt","content":"SECRET_BODY"}"#.into(),
                    },
                    ProviderEvent::ToolUse {
                        id: "edit-1".into(),
                        name: "edit".into(),
                        input_json:
                            r#"{"path":"note.txt","old_string":"old","new_string":"SECRET_NEW"}"#
                                .into(),
                    },
                    ProviderEvent::ToolUse {
                        id: "bash-1".into(),
                        name: "bash".into(),
                        input_json: r#"{"cmd":"echo allowed-audit"}"#.into(),
                    },
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ])],
                vec![ScriptStep::events(vec![ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }])],
            ]);
            let mut req = request(Vec::new());
            req.tool_config = tool_config(mode, RuntimePermissionMode::Confirm, checkpoint.clone());
            let calls = Arc::new(AtomicUsize::new(0));
            let hook = FixedHook {
                calls: calls.clone(),
                decision: Some(RuntimeUserDecision::Once),
            };
            let outcome = run_agent_with_permission_sink(
                &provider,
                &tools,
                req,
                CancellationToken::new(),
                &hook,
                |_| async { Ok(()) },
            )
            .await
            .unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(outcome.executed_tool_call_count, 0);
            assert_eq!(
                fs::read_to_string(project.path().join("note.txt")).unwrap(),
                "old"
            );
            assert!(!project.path().join("new.txt").exists());
            assert!(!checkpoint.exists());
            let rendered = format!("{:?}", outcome.events);
            assert!(!rendered.contains("SECRET_BODY"));
            assert!(!rendered.contains("SECRET_NEW"));
            assert_eq!(
                outcome
                    .events
                    .iter()
                    .filter(|event| matches!(
                        event,
                        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                            status: RuntimeToolStatus::Rejected,
                            approval: Some(RuntimeApprovalAudit {
                                source: RuntimeApprovalSource::RunMode,
                                ..
                            }),
                            ..
                        })
                    ))
                    .count(),
                3
            );
        }
    }

    #[tokio::test]
    async fn invalid_write_is_atomic_validation_rejection_before_any_proposal() {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        let checkpoint = data.path().join("checkpoints");
        fs::create_dir(&checkpoint).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "bad".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"x","content":"SECRET","extra":true}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Ask,
            RuntimePermissionMode::Confirm,
            checkpoint,
        );
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert!(
            matches!(outcome.events.first(), Some(RuntimeEvent::ToolCallValidationRejected { call, result }) if !call.input_json.contains("SECRET") && matches!(result.approval, Some(RuntimeApprovalAudit { source: RuntimeApprovalSource::Validation, .. })))
        );
        assert!(
            !outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::ToolCallProposed(_)))
        );
    }

    #[tokio::test]
    async fn ask_valid_body_with_invalid_call_id_is_validation_not_run_mode() {
        for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
            let project = tempdir().unwrap();
            let data = tempdir().unwrap();
            let checkpoint = data.path().join("must-not-be-created");
            let tools = vega_tools::Tools::new(project.path()).unwrap();
            let provider = MockProvider::new_rounds(vec![
                vec![ScriptStep::events(vec![
                    ProviderEvent::ToolUse {
                        id: String::new(),
                        name: "write".into(),
                        input_json: r#"{"path":"new.txt","content":"SECRET_BODY"}"#.into(),
                    },
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ])],
                vec![ScriptStep::events(vec![ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }])],
            ]);
            let mut req = request(Vec::new());
            req.tool_config = tool_config(mode, RuntimePermissionMode::Confirm, checkpoint.clone());
            let calls = Arc::new(AtomicUsize::new(0));
            let hook = FixedHook {
                calls: calls.clone(),
                decision: Some(RuntimeUserDecision::Once),
            };
            let outcome = run_agent_with_permission_sink(
                &provider,
                &tools,
                req,
                CancellationToken::new(),
                &hook,
                |_| async { Ok(()) },
            )
            .await
            .unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert!(!checkpoint.exists());
            assert!(outcome.events.iter().any(|event| matches!(
                event,
                RuntimeEvent::ToolCallValidationRejected { call, result }
                    if !call.input_json.contains("SECRET_BODY")
                        && result.output.contains("checkpoint_id_invalid")
                        && matches!(result.approval, Some(RuntimeApprovalAudit {
                            source: RuntimeApprovalSource::Validation,
                            ..
                        }))
            )));
        }
    }

    #[tokio::test]
    async fn execute_write_waits_for_once_and_provider_observes_only_safe_projection() {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        let checkpoint = data.path().join("checkpoints");
        let checkpoint_display = checkpoint.display().to_string();
        fs::create_dir(&checkpoint).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "write-1".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"new.txt","content":"SECRET_BODY"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            checkpoint,
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let hook = FixedHook {
            calls: calls.clone(),
            decision: Some(RuntimeUserDecision::Once),
        };
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            fs::read_to_string(project.path().join("new.txt")).unwrap(),
            "SECRET_BODY"
        );
        assert!(matches!(
            outcome
                .events
                .iter()
                .find(|event| matches!(event, RuntimeEvent::ToolCallFinished(_))),
            Some(RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                status: RuntimeToolStatus::Success,
                ..
            }))
        ));
        let requests = provider.requests();
        let wire = serde_json::to_string(&crate::openai::build_request_body(&requests[1])).unwrap();
        assert!(!wire.contains("SECRET_BODY"));
        assert!(!wire.contains(&checkpoint_display));
        assert!(!wire.contains("project-1"));
        assert!(!wire.contains("thread-1"));
        assert!(wire.contains("fingerprint_v1"));
        assert!(wire.contains("checkpoint_ref"));
    }

    #[tokio::test]
    async fn exact_same_turn_read_write_and_bash_calls_reuse_durable_results_once() {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        fs::write(project.path().join("source.txt"), "source").unwrap();
        let checkpoint = data.path().join("checkpoints");
        fs::create_dir(&checkpoint).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let read = || ProviderEvent::ToolUse {
            id: "read-1".into(),
            name: "read".into(),
            input_json: r#"{"path":"source.txt"}"#.into(),
        };
        let write = || ProviderEvent::ToolUse {
            id: "write-1".into(),
            name: "write".into(),
            input_json: r#"{"path":"new.txt","content":"body"}"#.into(),
        };
        let bash = || ProviderEvent::ToolUse {
            id: "bash-1".into(),
            name: "bash".into(),
            input_json: r#"{"cmd":"printf bash-ok"}"#.into(),
        };
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                read(),
                read(),
                write(),
                write(),
                bash(),
                bash(),
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            checkpoint,
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let hook = FixedHook {
            calls: calls.clone(),
            decision: Some(RuntimeUserDecision::Once),
        };
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(outcome.executed_tool_call_count, 3);
        assert_eq!(
            fs::read_to_string(project.path().join("new.txt")).unwrap(),
            "body"
        );
        let reused = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ToolCallFinished(result) if result.reused => Some(result),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(reused.len(), 3);
        assert!(
            reused
                .iter()
                .all(|result| { result.approval.is_some() && result.truncated.is_none() })
        );
    }

    #[tokio::test]
    async fn cancellation_at_proposal_or_running_ack_starts_no_mutation() {
        for cancel_on_running in [false, true] {
            let project = tempdir().unwrap();
            let data = tempdir().unwrap();
            let checkpoint = data.path().join("checkpoints");
            fs::create_dir(&checkpoint).unwrap();
            let tools = vega_tools::Tools::new(project.path()).unwrap();
            let provider = MockProvider::new(vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "write-1".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"new.txt","content":"must-not-write"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])]);
            let mut req = request(Vec::new());
            req.tool_config = tool_config(
                RuntimeRunMode::Execute,
                RuntimePermissionMode::Auto,
                checkpoint.clone(),
            );
            let cancel = CancellationToken::new();
            let sink_cancel = cancel.clone();
            let outcome = run_agent_with_permission_sink(
                &provider,
                &tools,
                req,
                cancel,
                &FixedHook {
                    calls: Arc::new(AtomicUsize::new(0)),
                    decision: Some(RuntimeUserDecision::Once),
                },
                move |event| {
                    let should_cancel = if cancel_on_running {
                        matches!(event, RuntimeEvent::ToolCallRunning { .. })
                    } else {
                        matches!(event, RuntimeEvent::ToolCallProposed(_))
                    };
                    if should_cancel {
                        sink_cancel.cancel();
                    }
                    async { Ok(()) }
                },
            )
            .await
            .unwrap();
            assert!(outcome.interrupted);
            assert_eq!(outcome.executed_tool_call_count, 0);
            assert!(!project.path().join("new.txt").exists());
            assert_eq!(fs::read_dir(&checkpoint).unwrap().count(), 0);
            let terminal = outcome.events.iter().find_map(|event| match event {
                RuntimeEvent::ToolCallFinished(result) => Some(result),
                _ => None,
            });
            if cancel_on_running {
                assert!(matches!(
                    terminal,
                    Some(RuntimeToolResult {
                        status: RuntimeToolStatus::Cancelled,
                        output,
                        ..
                    }) if output == CANCELLED_BEFORE_EXECUTION_OUTPUT
                ));
            } else {
                assert!(matches!(
                    terminal,
                    Some(RuntimeToolResult {
                        status: RuntimeToolStatus::Rejected,
                        approval: Some(RuntimeApprovalAudit {
                            source: RuntimeApprovalSource::Timeout,
                            ..
                        }),
                        ..
                    })
                ));
            }
        }
    }

    #[tokio::test]
    async fn permission_modes_rules_and_danger_ordering_reach_the_dispatcher() {
        let cases = [
            (
                RuntimePermissionMode::Confirm,
                false,
                RuntimeUserDecision::Deny { note: None },
                "printf denied",
                1,
                RuntimeToolStatus::Rejected,
                RuntimeApprovalSource::User,
            ),
            (
                RuntimePermissionMode::Auto,
                false,
                RuntimeUserDecision::Deny { note: None },
                "printf auto",
                0,
                RuntimeToolStatus::Success,
                RuntimeApprovalSource::Auto,
            ),
            (
                RuntimePermissionMode::ReadOnly,
                false,
                RuntimeUserDecision::Once,
                "printf readonly",
                0,
                RuntimeToolStatus::Rejected,
                RuntimeApprovalSource::ReadOnly,
            ),
            (
                RuntimePermissionMode::Confirm,
                true,
                RuntimeUserDecision::Deny { note: None },
                "printf ruled",
                0,
                RuntimeToolStatus::Success,
                RuntimeApprovalSource::Rule,
            ),
            (
                RuntimePermissionMode::Auto,
                false,
                RuntimeUserDecision::Deny { note: None },
                "rm -rf /",
                1,
                RuntimeToolStatus::Rejected,
                RuntimeApprovalSource::Danger,
            ),
            (
                RuntimePermissionMode::Auto,
                true,
                RuntimeUserDecision::Deny { note: None },
                "rm -rf /",
                1,
                RuntimeToolStatus::Rejected,
                RuntimeApprovalSource::Danger,
            ),
        ];
        for (mode, rule, decision, command, hook_calls, status, source) in cases {
            let (outcome, actual_calls) =
                run_bash_permission_case(mode, rule, decision, command).await;
            assert_eq!(actual_calls, hook_calls, "{command}");
            assert!(
                outcome.events.iter().any(|event| matches!(
                    event,
                    RuntimeEvent::ToolCallFinished(result)
                        if result.status == status
                            && result.approval.as_ref().is_some_and(|audit| audit.source == source)
                )),
                "{command}"
            );
        }

        let (outcome, calls) = run_bash_permission_case(
            RuntimePermissionMode::ReadOnly,
            false,
            RuntimeUserDecision::Always,
            "rm -rf /",
        )
        .await;
        assert_eq!(calls, 1);
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                status: RuntimeToolStatus::Rejected,
                approval: Some(RuntimeApprovalAudit {
                    source: RuntimeApprovalSource::ReadOnly,
                    danger: Some(crate::RuntimeDangerAudit {
                        decision: RuntimeApprovalDecision::Always,
                        ..
                    }),
                    ..
                }),
                remember_rule: Some(_),
                ..
            })
        )));

        let (outcome, calls) = run_bash_permission_case(
            RuntimePermissionMode::Auto,
            false,
            RuntimeUserDecision::Once,
            "git push --force",
        )
        .await;
        assert_eq!(calls, 1);
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                status: RuntimeToolStatus::Success,
                exit_code: Some(code),
                duration_ms: Some(_),
                approval: Some(RuntimeApprovalAudit {
                    source: RuntimeApprovalSource::Danger,
                    ..
                }),
                ..
            }) if *code != 0
        )));
    }

    #[tokio::test]
    async fn dangerous_cancel_after_proposal_keeps_nested_timeout_audit() {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        let checkpoint = data.path().join("checkpoints");
        fs::create_dir(&checkpoint).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "danger-cancel".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"rm -rf /"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Auto,
            checkpoint,
        );
        let cancel = CancellationToken::new();
        let sink_cancel = cancel.clone();
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            cancel,
            &FixedHook {
                calls: Arc::new(AtomicUsize::new(0)),
                decision: Some(RuntimeUserDecision::Once),
            },
            move |event| {
                if matches!(event, RuntimeEvent::ToolCallProposed(_)) {
                    sink_cancel.cancel();
                }
                async { Ok(()) }
            },
        )
        .await
        .unwrap();
        assert!(outcome.interrupted);
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                status: RuntimeToolStatus::Rejected,
                approval: Some(RuntimeApprovalAudit {
                    source: RuntimeApprovalSource::Timeout,
                    danger: Some(crate::RuntimeDangerAudit {
                        decision: RuntimeApprovalDecision::Deny,
                        ..
                    }),
                    ..
                }),
                ..
            })
        )));
    }

    #[tokio::test]
    async fn running_bash_cancellation_waits_for_process_reap() {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        let checkpoint = data.path().join("checkpoints");
        fs::create_dir(&checkpoint).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "bash-cancel".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"sleep 30 & wait"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Auto,
            checkpoint,
        );
        let cancel = CancellationToken::new();
        let sink_cancel = cancel.clone();
        let started = Instant::now();
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            cancel,
            &FixedHook {
                calls: Arc::new(AtomicUsize::new(0)),
                decision: Some(RuntimeUserDecision::Once),
            },
            move |event| {
                if matches!(event, RuntimeEvent::ToolCallRunning { .. }) {
                    let delayed = sink_cancel.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        delayed.cancel();
                    });
                }
                async { Ok(()) }
            },
        )
        .await
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(outcome.interrupted);
        assert_eq!(outcome.executed_tool_call_count, 1);
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                status: RuntimeToolStatus::Cancelled,
                output,
                ..
            }) if output == "Tool error: bash failed (cancelled)"
        )));
    }

    #[tokio::test]
    async fn always_rule_bypasses_the_second_write_in_the_same_turn() {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        let checkpoint = data.path().join("checkpoints");
        fs::create_dir(&checkpoint).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "write-first".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"same.txt","content":"first"}"#.into(),
                },
                ProviderEvent::ToolUse {
                    id: "write-second".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"same.txt","content":"second"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            checkpoint,
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let hook = FixedHook {
            calls: calls.clone(),
            decision: Some(RuntimeUserDecision::Always),
        };
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(outcome.executed_tool_call_count, 2);
        assert_eq!(
            fs::read_to_string(project.path().join("same.txt")).unwrap(),
            "second"
        );
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallApproved {
                call_id,
                audit: RuntimeApprovalAudit {
                    source: RuntimeApprovalSource::Rule,
                    ..
                },
                remember_rule: None,
            } if call_id == "write-second"
        )));
    }

    #[tokio::test]
    async fn permission_timeout_rejects_without_mutation() {
        let project = tempdir().unwrap();
        let data = tempdir().unwrap();
        let checkpoint = data.path().join("checkpoints");
        fs::create_dir(&checkpoint).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "write-1".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"new.txt","content":"body"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            checkpoint,
        )
        .with_permission_timeout(Duration::from_millis(5));
        let hook = FixedHook {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: None,
        };
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert!(!project.path().join("new.txt").exists());
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                status: RuntimeToolStatus::Rejected,
                approval: Some(RuntimeApprovalAudit {
                    source: RuntimeApprovalSource::Timeout,
                    ..
                }),
                ..
            })
        )));
    }

    #[tokio::test]
    async fn text_only_preserves_events_usage_and_visible_content() {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("a".into()),
            ProviderEvent::ThinkingDelta("reason".into()),
            ProviderEvent::TextDelta("b".into()),
            ProviderEvent::Usage {
                input: 5,
                output: 2,
                cache_read: 1,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]);
        let outcome = run_agent(
            &provider,
            &tools,
            request(vec![ChatMessage::new(ChatRole::User, "hello")]),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.final_text, "ab");
        assert!(!outcome.final_text.contains("reason"));
        assert_eq!(provider.requests().len(), 1);
        assert!(matches!(&outcome.events[..], [
            RuntimeEvent::TextDelta(first),
            RuntimeEvent::ThinkingDelta(thinking),
            RuntimeEvent::TextDelta(second),
            RuntimeEvent::UsageUpdated { usage: RuntimeTokenUsage { input: 5, output: 2, cache_read: 1, cache_write: 0 }, cost_microcents: 0 },
            RuntimeEvent::Finished(RuntimeFinishReason::End),
        ] if first == "a" && thinking == "reason" && second == "b"));
    }

    #[tokio::test]
    async fn tool_observe_round_uses_real_grep_and_converges() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "// TODO: wire loop\n").unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ThinkingDelta("searching".into()),
                ProviderEvent::ToolUse {
                    id: "call-grep".into(),
                    name: "grep".into(),
                    input_json: r#"{"pattern":"TODO"}"#.into(),
                },
                ProviderEvent::Usage {
                    input: 10,
                    output: 2,
                    cache_read: 1,
                    cache_write: 0,
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("Found lib.rs TODO".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
        ]);
        let outcome = run_agent(
            &provider,
            &tools,
            request(vec![ChatMessage::new(ChatRole::User, "Find TODOs")]),
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.final_text, "Found lib.rs TODO");
        assert_eq!(outcome.tool_call_count, 1);
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallOutput { chunk, .. } if chunk.contains("lib.rs:1:// TODO")
        )));
        let approved = outcome
            .events
            .iter()
            .position(|event| matches!(event, RuntimeEvent::ToolCallApproved { call_id, .. } if call_id == "call-grep"))
            .unwrap();
        let running = outcome
            .events
            .iter()
            .position(|event| matches!(event, RuntimeEvent::ToolCallRunning { call_id } if call_id == "call-grep"))
            .unwrap();
        let succeeded = outcome
            .events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                        call_id,
                        status: RuntimeToolStatus::Success,
                        ..
                    }) if call_id == "call-grep"
                )
            })
            .unwrap();
        assert!(approved < running && running < succeeded);
        assert!(matches!(
            outcome.events.last(),
            Some(RuntimeEvent::Finished(RuntimeFinishReason::End))
        ));
        let requests = provider.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].messages[0].role, ChatRole::System);
        assert!(requests[1].messages.iter().any(|message| {
            message.role == ChatRole::Tool
                && message.tool_call_id.as_deref() == Some("call-grep")
                && message.content.contains("lib.rs:1:// TODO")
        }));
    }

    #[tokio::test]
    async fn one_turn_executes_read_glob_and_grep_serially() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "// TODO: all tools\n").unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "read-1".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"lib.rs"}"#.into(),
                },
                ProviderEvent::ToolUse {
                    id: "glob-1".into(),
                    name: "glob".into(),
                    input_json: r#"{"pattern":"*.rs"}"#.into(),
                },
                ProviderEvent::ToolUse {
                    id: "grep-1".into(),
                    name: "grep".into(),
                    input_json: r#"{"pattern":"TODO"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let outcome = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let outputs: Vec<(&str, &str)> = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ToolCallOutput { call_id, chunk } => {
                    Some((call_id.as_str(), chunk.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(outputs.len(), 3);
        assert_eq!(outputs[0], ("read-1", "1 | // TODO: all tools"));
        assert_eq!(outputs[1], ("glob-1", "lib.rs"));
        assert_eq!(outputs[2], ("grep-1", "lib.rs:1:// TODO: all tools"));
        let requests = provider.requests();
        assert_eq!(requests.len(), 2);
        let follow_up = &requests[1].messages;
        let assistant = follow_up
            .iter()
            .find(|message| !message.tool_calls.is_empty())
            .unwrap();
        assert_eq!(
            assistant
                .tool_calls
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            vec!["read-1", "glob-1", "grep-1"]
        );
        assert_eq!(
            follow_up
                .iter()
                .filter(|message| message.role == ChatRole::Tool)
                .filter_map(|message| message.tool_call_id.as_deref())
                .collect::<Vec<_>>(),
            vec!["read-1", "glob-1", "grep-1"]
        );
    }

    #[tokio::test]
    async fn bad_json_and_path_escape_become_failed_results_then_model_continues() {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "bad-json".into(),
                    name: "read".into(),
                    input_json: "{".into(),
                },
                ProviderEvent::ToolUse {
                    id: "escape".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"../outside"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("Handled both errors.".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
        ]);
        let outcome = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.final_text, "Handled both errors.");
        let failures: Vec<&RuntimeToolResult> = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ToolCallFinished(result)
                    if result.status == RuntimeToolStatus::Failed =>
                {
                    Some(result)
                }
                _ => None,
            })
            .collect();
        assert_eq!(failures.len(), 2);
        assert!(failures[0].output.contains("invalid read input JSON"));
        assert!(failures[1].output.contains("path escapes the project root"));
    }

    #[tokio::test]
    async fn provider_error_emits_error_without_message_finished() {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::Error {
            status: Some(503),
            message: "unavailable".into(),
            retryable: false,
        }]);
        let outcome = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(outcome.failed);
        assert!(matches!(
            outcome.events.as_slice(),
            [RuntimeEvent::Error(error)]
                if matches!(
                    error.as_ref(),
                    VegaError::Provider {
                        status: Some(503),
                        message,
                        retryable: false,
                    } if message == "unavailable"
                )
        ));
    }

    #[tokio::test]
    async fn invalid_write_is_rejected_and_observed() {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "write-1".into(),
                    name: "write".into(),
                    input_json: "{}".into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let outcome = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallValidationRejected { result: RuntimeToolResult { status: RuntimeToolStatus::Rejected, output, .. }, .. }
                if output.contains("invalid write input")
        )));
    }

    #[tokio::test]
    async fn persisted_call_id_is_observed_without_execution() {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "done-1".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"missing"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let mut req = request(Vec::new());
        req.completed_tool_results.insert(
            "done-1".into(),
            CompletedToolCall {
                tool: "read".into(),
                input_json: r#"{"path":"missing"}"#.into(),
                result: RuntimeToolResult {
                    call_id: "done-1".into(),
                    output: "persisted output".into(),
                    status: RuntimeToolStatus::Success,
                    reused: true,
                    exit_code: None,
                    duration_ms: None,
                    truncated: None,
                    approval: None,
                    remember_rule: None,
                },
            },
        );
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(outcome.tool_call_count, 1);
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallFinished(RuntimeToolResult { reused: true, output, .. })
                if output == "persisted output"
        )));
    }

    #[tokio::test]
    async fn conflicting_persisted_call_id_is_not_silently_reused_or_executed() {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "done-1".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"different"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let mut req = request(Vec::new());
        req.completed_tool_results.insert(
            "done-1".into(),
            CompletedToolCall {
                tool: "read".into(),
                input_json: r#"{"path":"original"}"#.into(),
                result: RuntimeToolResult {
                    call_id: "done-1".into(),
                    output: "persisted output".into(),
                    status: RuntimeToolStatus::Success,
                    reused: true,
                    exit_code: None,
                    duration_ms: None,
                    truncated: None,
                    approval: None,
                    remember_rule: None,
                },
            },
        );
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(outcome.tool_call_count, 1);
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallConflict { result: RuntimeToolResult {
                status: RuntimeToolStatus::Failed,
                reused: false,
                output,
                ..
            }, .. } if output == CALL_ID_CONFLICT_OUTPUT
        )));
        assert!(provider.requests()[1].messages.iter().any(|message| {
            message.role == ChatRole::Tool && message.content == CALL_ID_CONFLICT_OUTPUT
        }));
    }

    #[tokio::test]
    async fn cancellation_stops_a_delayed_provider_under_one_second() {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![
            ScriptStep::delay(Duration::from_secs(30)),
            ScriptStep::text("late"),
        ]);
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            trigger.cancel();
        });
        let started = Instant::now();
        let outcome = run_agent(&provider, &tools, request(Vec::new()), cancel)
            .await
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(outcome.interrupted);
        assert!(matches!(
            outcome.events.last(),
            Some(RuntimeEvent::Interrupted)
        ));
    }

    #[tokio::test]
    async fn cancellation_before_start_makes_no_provider_request() {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::text("never")]);
        let cancel = CancellationToken::new();
        cancel.cancel();
        let outcome = run_agent(&provider, &tools, request(Vec::new()), cancel)
            .await
            .unwrap();
        assert!(outcome.interrupted);
        assert!(provider.requests().is_empty());
        assert!(matches!(
            outcome.events.as_slice(),
            [RuntimeEvent::Interrupted]
        ));
    }

    #[tokio::test]
    async fn stops_after_one_hundred_tool_calls_with_visible_notice() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "ok\n").unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let calls: Vec<ProviderEvent> = (0..=TOOL_CALL_LIMIT)
            .map(|index| ProviderEvent::ToolUse {
                id: format!("call-{index}"),
                name: "read".into(),
                input_json: r#"{"path":"a.txt"}"#.into(),
            })
            .chain(std::iter::once(ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            }))
            .collect();
        let provider = MockProvider::new(vec![ScriptStep::events(calls)]);
        let outcome = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.tool_call_count, TOOL_CALL_LIMIT);
        assert_eq!(outcome.executed_tool_call_count, TOOL_CALL_LIMIT);
        let finished_calls = outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ToolCallFinished(_)))
            .count();
        assert_eq!(finished_calls, TOOL_CALL_LIMIT);
        assert!(!outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallOutput { call_id, .. } if call_id == "call-100"
        )));
        assert!(outcome.final_text.contains("Tool call limit (100) reached"));
        assert!(matches!(
            outcome.events.last(),
            Some(RuntimeEvent::Finished(RuntimeFinishReason::ToolLimit))
        ));
    }

    #[test]
    fn tool_output_keeps_two_thousand_head_and_tail_lines() {
        let text = (0..4_005)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let truncated = truncate_output_lines(&text);
        let lines: Vec<&str> = truncated.lines().collect();
        assert_eq!(lines.len(), 4_001);
        assert_eq!(lines[0], "line-0");
        assert_eq!(lines[1_999], "line-1999");
        assert_eq!(lines[2_000], OUTPUT_TRUNCATION_MARKER);
        assert_eq!(lines[2_001], "line-2005");
        assert_eq!(lines[4_000], "line-4004");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_during_a_read_waits_for_it_then_skips_the_next_call() {
        let dir = tempdir().unwrap();
        let slow_content = "line\n".repeat(400_000);
        fs::write(dir.path().join("slow.txt"), slow_content).unwrap();
        fs::write(dir.path().join("second.txt"), "must not run\n").unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "slow".into(),
                name: "read".into(),
                input_json: r#"{"path":"slow.txt"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "second".into(),
                name: "read".into(),
                input_json: r#"{"path":"second.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(2)).await;
            trigger.cancel();
        });
        let outcome = run_agent(&provider, &tools, request(Vec::new()), cancel)
            .await
            .unwrap();
        assert!(outcome.interrupted);
        let approved = outcome
            .events
            .iter()
            .position(|event| matches!(event, RuntimeEvent::ToolCallApproved { call_id, .. } if call_id == "slow"))
            .unwrap();
        let running = outcome
            .events
            .iter()
            .position(|event| matches!(event, RuntimeEvent::ToolCallRunning { call_id } if call_id == "slow"))
            .unwrap();
        let cancelled = outcome
            .events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    RuntimeEvent::ToolCallFinished(result)
                        if result.call_id == "slow"
                            && result.status == RuntimeToolStatus::Cancelled
                            && !result.output.is_empty()
                )
            })
            .unwrap();
        let interrupted = outcome
            .events
            .iter()
            .position(|event| matches!(event, RuntimeEvent::Interrupted))
            .unwrap();
        assert!(approved < running && running < cancelled && cancelled < interrupted);
        assert!(!outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallRunning { call_id } if call_id == "second"
        )));
    }

    #[tokio::test]
    async fn repeated_call_id_counts_every_observation_and_rejects_the_101st() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "ok\n").unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let calls: Vec<ProviderEvent> = (0..=TOOL_CALL_LIMIT)
            .map(|_| ProviderEvent::ToolUse {
                id: "same-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"a.txt"}"#.into(),
            })
            .chain(std::iter::once(ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            }))
            .collect();
        let provider = MockProvider::new(vec![ScriptStep::events(calls)]);

        let outcome = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.tool_call_count, TOOL_CALL_LIMIT);
        assert_eq!(outcome.executed_tool_call_count, 1);
        assert_eq!(
            outcome
                .events
                .iter()
                .filter(|event| matches!(event, RuntimeEvent::ToolCallFinished(_)))
                .count(),
            TOOL_CALL_LIMIT
        );
        assert!(matches!(
            outcome.events.last(),
            Some(RuntimeEvent::Finished(RuntimeFinishReason::ToolLimit))
        ));
    }

    #[tokio::test]
    async fn repeated_call_id_across_rounds_cannot_loop_forever() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "ok\n").unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "same-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"a.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);

        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            run_agent(
                &provider,
                &tools,
                request(Vec::new()),
                CancellationToken::new(),
            ),
        )
        .await
        .expect("tool-use safety limit must converge")
        .unwrap();

        assert_eq!(outcome.tool_call_count, TOOL_CALL_LIMIT);
        assert_eq!(outcome.executed_tool_call_count, 1);
        assert_eq!(provider.requests().len(), TOOL_CALL_LIMIT + 1);
        assert!(matches!(
            outcome.events.last(),
            Some(RuntimeEvent::Finished(RuntimeFinishReason::ToolLimit))
        ));
    }
}
