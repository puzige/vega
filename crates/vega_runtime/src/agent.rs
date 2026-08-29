//! Headless agentic loop (tech-spec §4.2, A3-03 / S4-T20).

use std::collections::HashMap;

use futures::StreamExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::error::VegaError;
use crate::provider::{
    ChatMessage, ChatRequest, ChatRole, ChatToolCall, Provider, ProviderEvent, StopReason,
    ToolDefinition,
};

/// Maximum number of tool calls executed by one task.
pub const TOOL_CALL_LIMIT: usize = 100;

const OUTPUT_HALF_LINES: usize = 2_000;
const OUTPUT_TRUNCATION_MARKER: &str = "…[tool output truncated: middle lines omitted]";

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
    pub completed_tool_results: HashMap<String, String>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeToolCall {
    /// Provider-side call id.
    pub id: String,
    /// Requested tool name.
    pub name: String,
    /// Complete raw JSON input.
    pub input_json: String,
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
}

/// Runtime-only event stream. `vega_conversation` is responsible for
/// converting it into the sole UI/store event type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    /// Visible assistant delta.
    TextDelta(String),
    /// Reasoning delta.
    ThinkingDelta(String),
    /// Complete tool call before the S4 placeholder permission decision.
    ToolCallProposed(RuntimeToolCall),
    /// Read-only tool auto-approved by the S4 placeholder gate.
    ToolCallApproved { call_id: String },
    /// Approved tool began running.
    ToolCallRunning { call_id: String },
    /// Complete display chunk from a tool.
    ToolCallOutput { call_id: String, chunk: String },
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
    Error(String),
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
    /// Number of unique tool calls executed or rejected in this run.
    pub tool_call_count: usize,
    /// Whether cancellation ended the task.
    pub interrupted: bool,
}

/// Runs the S4 headless agent loop with real fenced read/glob/grep tools.
pub async fn run_agent(
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    request: AgentRequest,
    cancel: CancellationToken,
) -> Result<AgentOutcome, VegaError> {
    let mut messages = Vec::with_capacity(request.history.len() + 1);
    messages.push(ChatMessage::new(ChatRole::System, request.system_prompt));
    messages.extend(request.history);
    let mut completed = request.completed_tool_results;
    let mut events = Vec::new();
    let mut final_text = String::new();
    let mut tool_call_count = 0usize;

    loop {
        if cancel.is_cancelled() {
            events.push(RuntimeEvent::Interrupted);
            return Ok(outcome(events, messages, final_text, tool_call_count, true));
        }

        let chat_request = ChatRequest {
            model: request.model.clone(),
            messages: messages.clone(),
            tools: readonly_tool_definitions(),
            max_tokens: request.max_tokens,
        };
        let mut stream = match provider.chat_stream(chat_request, cancel.clone()).await {
            Ok(stream) => stream,
            Err(VegaError::Cancelled) => {
                events.push(RuntimeEvent::Interrupted);
                return Ok(outcome(events, messages, final_text, tool_call_count, true));
            }
            Err(error) => {
                events.push(RuntimeEvent::Error(error.to_string()));
                return Err(error);
            }
        };

        let mut assistant_text = String::new();
        let mut calls = Vec::new();
        let mut stop_reason = None;
        loop {
            let next = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    events.push(RuntimeEvent::Interrupted);
                    return Ok(outcome(events, messages, final_text, tool_call_count, true));
                }
                next = stream.next() => next,
            };
            let Some(item) = next else { break };
            match item {
                Ok(ProviderEvent::TextDelta(delta)) => {
                    assistant_text.push_str(&delta);
                    final_text.push_str(&delta);
                    events.push(RuntimeEvent::TextDelta(delta));
                }
                Ok(ProviderEvent::ThinkingDelta(delta)) => {
                    events.push(RuntimeEvent::ThinkingDelta(delta));
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
                    events.push(RuntimeEvent::UsageUpdated {
                        usage: RuntimeTokenUsage {
                            input,
                            output,
                            cache_read,
                            cache_write,
                        },
                        cost_microcents: 0,
                    });
                }
                Ok(ProviderEvent::Done {
                    stop_reason: reason,
                }) => stop_reason = Some(reason),
                Err(VegaError::Cancelled) => {
                    events.push(RuntimeEvent::Interrupted);
                    return Ok(outcome(events, messages, final_text, tool_call_count, true));
                }
                Err(error) => {
                    events.push(RuntimeEvent::Error(error.to_string()));
                    return Err(error);
                }
            }
        }

        if calls.is_empty() {
            messages.push(ChatMessage::new(ChatRole::Assistant, assistant_text));
            let finish = match stop_reason.unwrap_or(StopReason::End) {
                StopReason::Length => RuntimeFinishReason::Length,
                StopReason::End | StopReason::ToolUse => RuntimeFinishReason::End,
            };
            events.push(RuntimeEvent::Finished(finish));
            return Ok(outcome(
                events,
                messages,
                final_text,
                tool_call_count,
                false,
            ));
        }

        let wire_calls = calls
            .iter()
            .map(|call| ChatToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                input_json: call.input_json.clone(),
            })
            .collect();
        messages.push(ChatMessage::assistant_with_tools(
            assistant_text,
            wire_calls,
        ));

        for call in calls {
            events.push(RuntimeEvent::ToolCallProposed(call.clone()));
            if let Some(prior) = completed.get(&call.id).cloned() {
                let result = RuntimeToolResult {
                    call_id: call.id.clone(),
                    output: prior.clone(),
                    status: RuntimeToolStatus::Success,
                    reused: true,
                };
                events.push(RuntimeEvent::ToolCallOutput {
                    call_id: call.id.clone(),
                    chunk: prior.clone(),
                });
                events.push(RuntimeEvent::ToolCallFinished(result));
                messages.push(ChatMessage::tool_result(call.id, prior));
                continue;
            }
            if tool_call_count >= TOOL_CALL_LIMIT {
                let notice = format!(
                    "Tool call limit ({TOOL_CALL_LIMIT}) reached; stopping without executing additional tools."
                );
                final_text.push_str(&notice);
                events.push(RuntimeEvent::TextDelta(notice.clone()));
                messages.push(ChatMessage::new(ChatRole::Assistant, notice));
                events.push(RuntimeEvent::Finished(RuntimeFinishReason::ToolLimit));
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    false,
                ));
            }
            tool_call_count += 1;

            let result = if is_readonly_tool(&call.name) {
                events.push(RuntimeEvent::ToolCallApproved {
                    call_id: call.id.clone(),
                });
                events.push(RuntimeEvent::ToolCallRunning {
                    call_id: call.id.clone(),
                });
                execute_readonly(tools, &call)
            } else {
                RuntimeToolResult {
                    call_id: call.id.clone(),
                    output: format!(
                        "denied: tool '{}' is unavailable until the S5 permission gate",
                        call.name
                    ),
                    status: RuntimeToolStatus::Rejected,
                    reused: false,
                }
            };
            events.push(RuntimeEvent::ToolCallOutput {
                call_id: call.id.clone(),
                chunk: result.output.clone(),
            });
            events.push(RuntimeEvent::ToolCallFinished(result.clone()));
            messages.push(ChatMessage::tool_result(&call.id, &result.output));
            completed.insert(call.id, result.output);

            if cancel.is_cancelled() {
                events.push(RuntimeEvent::Interrupted);
                return Ok(outcome(events, messages, final_text, tool_call_count, true));
            }
        }
    }
}

fn outcome(
    events: Vec<RuntimeEvent>,
    messages: Vec<ChatMessage>,
    final_text: String,
    tool_call_count: usize,
    interrupted: bool,
) -> AgentOutcome {
    AgentOutcome {
        events,
        messages,
        final_text,
        tool_call_count,
        interrupted,
    }
}

fn is_readonly_tool(name: &str) -> bool {
    matches!(name, "read" | "glob" | "grep")
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
        },
        Err(message) => RuntimeToolResult {
            call_id: call.id.clone(),
            output: format!("Tool error: {message}"),
            status: RuntimeToolStatus::Failed,
            reused: false,
        },
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

fn readonly_tool_definitions() -> Vec<ToolDefinition> {
    vec![
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
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "glob".to_string(),
            description: "List project files matching a gitignore-style glob.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "pattern": { "type": "string" } },
                "required": ["pattern"]
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
                "required": ["pattern"]
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    use super::*;
    use crate::{MockProvider, ScriptStep};

    fn request(history: Vec<ChatMessage>) -> AgentRequest {
        AgentRequest {
            model: "mock".to_string(),
            system_prompt: "Be precise.".to_string(),
            history,
            max_tokens: None,
            completed_tool_results: HashMap::new(),
        }
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
    async fn unknown_tool_is_denied_and_observed() {
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
            RuntimeEvent::ToolCallFinished(RuntimeToolResult { status: RuntimeToolStatus::Rejected, output, .. })
                if output.contains("denied")
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
        req.completed_tool_results
            .insert("done-1".into(), "persisted output".into());
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(outcome.tool_call_count, 0);
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallFinished(RuntimeToolResult { reused: true, output, .. })
                if output == "persisted output"
        )));
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
        assert!(outcome.final_text.contains("Tool call limit (100) reached"));
        assert!(matches!(
            outcome.events.last(),
            Some(RuntimeEvent::Finished(RuntimeFinishReason::ToolLimit))
        ));
    }
}
