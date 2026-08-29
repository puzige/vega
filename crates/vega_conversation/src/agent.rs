//! Conversation-layer orchestration for the headless runtime (S4-T20).

use std::time::{SystemTime, UNIX_EPOCH};

use tokio_util::sync::CancellationToken;
use vega_runtime::{AgentRequest, Provider, RuntimeEvent, RuntimeToolStatus, run_agent};
use vega_store::{Store, messages, token_usage, tool_calls};

use crate::types::{ConversationError, ConversationEvent, from_runtime_event};

const HISTORY_WINDOW: usize = 50;

/// Persisted task result and the sole event stream exposed to UI/store
/// consumers.
#[derive(Debug)]
pub struct ConversationRun {
    /// User message id.
    pub user_message_id: String,
    /// Assistant message id.
    pub assistant_message_id: String,
    /// Ordered conversation events.
    pub events: Vec<ConversationEvent>,
    /// Final visible assistant Markdown.
    pub content: String,
    /// Whether cancellation interrupted the message.
    pub interrupted: bool,
}

/// Persists a user turn, drives the runtime, converts every runtime event,
/// and audits messages/tool calls/usage in the existing six-table schema.
pub async fn run_thread_task(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    user_content: &str,
    system_prompt: &str,
    cancel: CancellationToken,
) -> Result<ConversationRun, ConversationError> {
    let thread = vega_store::threads::find(store.conn(), thread_id)
        .map_err(store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.to_string()))?;
    let user_seq = messages::next_seq(store.conn(), thread_id).map_err(store_error)?;
    let user_message_id = ulid::Ulid::generate().to_string();
    let assistant_message_id = ulid::Ulid::generate().to_string();
    let now = now_ms();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: user_message_id.clone(),
            thread_id: thread_id.to_string(),
            seq: user_seq,
            role: "user".to_string(),
            kind: "text".to_string(),
            content: user_content.to_string(),
            status: "done".to_string(),
            created_at: now,
        },
    )
    .map_err(store_error)?;
    let assistant_seq = user_seq + 1;
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: assistant_message_id.clone(),
            thread_id: thread_id.to_string(),
            seq: assistant_seq,
            role: "assistant".to_string(),
            kind: "text".to_string(),
            content: String::new(),
            status: "streaming".to_string(),
            created_at: now,
        },
    )
    .map_err(store_error)?;

    let history = messages::recent(store.conn(), thread_id, HISTORY_WINDOW)
        .map_err(store_error)?
        .into_iter()
        .filter_map(|message| {
            let role = match message.role.as_str() {
                "user" => Some(vega_runtime::ChatRole::User),
                "assistant" => Some(vega_runtime::ChatRole::Assistant),
                _ => None,
            }?;
            Some(vega_runtime::ChatMessage::new(role, message.content))
        })
        .collect();
    let completed_tool_results =
        tool_calls::successful_results(store.conn(), thread_id).map_err(store_error)?;
    let request = AgentRequest {
        model: thread.model.clone(),
        system_prompt: system_prompt.to_string(),
        history,
        max_tokens: None,
        completed_tool_results,
    };
    let outcome = match run_agent(provider, tools, request, cancel).await {
        Ok(outcome) => outcome,
        Err(error) => {
            messages::finish(
                store.conn(),
                &assistant_message_id,
                &error.to_string(),
                "failed",
            )
            .map_err(store_error)?;
            return Err(ConversationError::Runtime(error.to_string()));
        }
    };

    let mut events = vec![ConversationEvent::MessageStarted {
        message_id: assistant_message_id.clone(),
        seq: assistant_seq as u64,
    }];
    let mut next_tool_seq = tool_calls::next_seq(store.conn(), thread_id).map_err(store_error)?;
    for event in &outcome.events {
        persist_runtime_event(
            store,
            thread_id,
            &assistant_message_id,
            &thread.model,
            &mut next_tool_seq,
            event,
        )?;
        if let Some(converted) = from_runtime_event(&assistant_message_id, event) {
            events.push(converted);
        }
    }
    let status = if outcome.interrupted {
        "interrupted"
    } else {
        "done"
    };
    messages::finish(
        store.conn(),
        &assistant_message_id,
        &outcome.final_text,
        status,
    )
    .map_err(store_error)?;

    Ok(ConversationRun {
        user_message_id,
        assistant_message_id,
        events,
        content: outcome.final_text,
        interrupted: outcome.interrupted,
    })
}

fn persist_runtime_event(
    store: &Store,
    thread_id: &str,
    message_id: &str,
    model: &str,
    next_tool_seq: &mut i64,
    event: &RuntimeEvent,
) -> Result<(), ConversationError> {
    match event {
        RuntimeEvent::ToolCallProposed(call) => {
            if !tool_calls::exists(store.conn(), &call.id).map_err(store_error)? {
                tool_calls::insert(
                    store.conn(),
                    tool_calls::NewToolCall {
                        id: &call.id,
                        thread_id,
                        message_id,
                        seq: *next_tool_seq,
                        tool: &call.name,
                        input_json: &call.input_json,
                        status: "pending_approval",
                        created_at: now_ms(),
                    },
                )
                .map_err(store_error)?;
                *next_tool_seq += 1;
            }
        }
        RuntimeEvent::ToolCallApproved { call_id } => {
            tool_calls::update(store.conn(), call_id, "approved", Some("once"), None, None)
                .map_err(store_error)?;
        }
        RuntimeEvent::ToolCallRunning { call_id } => {
            tool_calls::update(store.conn(), call_id, "running", None, None, None)
                .map_err(store_error)?;
        }
        RuntimeEvent::ToolCallFinished(result) if !result.reused => {
            let (status, approval) = match result.status {
                RuntimeToolStatus::Rejected => ("rejected", Some("deny")),
                RuntimeToolStatus::Success => ("success", Some("once")),
                RuntimeToolStatus::Failed => ("failed", Some("once")),
            };
            tool_calls::update(
                store.conn(),
                &result.call_id,
                status,
                approval,
                Some(&result.output),
                Some(now_ms()),
            )
            .map_err(store_error)?;
        }
        RuntimeEvent::UsageUpdated {
            usage,
            cost_microcents,
        } => {
            token_usage::insert(
                store.conn(),
                token_usage::NewTokenUsage {
                    thread_id,
                    message_id: Some(message_id),
                    model,
                    input_tokens: usage.input,
                    output_tokens: usage.output,
                    cache_read_tokens: usage.cache_read,
                    cache_write_tokens: usage.cache_write,
                    cost_microcents: *cost_microcents,
                    created_at: now_ms(),
                },
            )
            .map_err(store_error)?;
        }
        RuntimeEvent::TextDelta(_)
        | RuntimeEvent::ThinkingDelta(_)
        | RuntimeEvent::ToolCallOutput { .. }
        | RuntimeEvent::Finished(_)
        | RuntimeEvent::Interrupted
        | RuntimeEvent::Error(_)
        | RuntimeEvent::ToolCallFinished(_) => {}
    }
    Ok(())
}

fn store_error(error: impl ToString) -> ConversationError {
    ConversationError::Store(error.to_string())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;
    use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason};

    use super::*;
    use crate::types::{ConversationEvent, ToolCallStatus};

    fn setup() -> (Store, tempfile::TempDir, String) {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "// TODO: persist me\n").unwrap();
        let store = Store::open(dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            dir.path().to_str().unwrap(),
            "fixture",
            Some("master"),
        )
        .unwrap();
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                id: "thread-1",
                project_id: &project.id,
                title: "",
                mode: "execute",
                permission_mode: "confirm",
                model: "mock-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        (store, dir, project.id)
    }

    fn scripted_provider(call_id: &str, input_json: &str) -> MockProvider {
        MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("Checking. ".into()),
                ProviderEvent::ThinkingDelta("Need grep".into()),
                ProviderEvent::ToolUse {
                    id: call_id.into(),
                    name: "grep".into(),
                    input_json: input_json.into(),
                },
                ProviderEvent::Usage {
                    input: 12,
                    output: 3,
                    cache_read: 2,
                    cache_write: 1,
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("Found the TODO.".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
        ])
    }

    #[tokio::test]
    async fn persists_messages_tool_lifecycle_and_zero_cost_usage() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = scripted_provider("call-1", r#"{"pattern":"TODO","path":"lib.rs"}"#);
        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Find TODO",
            "You inspect repositories.",
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(run.content, "Checking. Found the TODO.");
        assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ThinkingDelta { delta, .. } if delta == "Need grep")));
        assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallApproved { call_id, .. } if call_id == "call-1")));
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Success && result.output.contains("lib.rs:1:// TODO")
        )));

        let assistant: (String, String, i64) = store
            .conn()
            .query_row(
                "SELECT content, status, seq FROM messages WHERE id = ?1",
                [&run.assistant_message_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            assistant,
            ("Checking. Found the TODO.".into(), "done".into(), 2)
        );
        let tool: (String, String, String, String) = store
            .conn()
            .query_row(
                "SELECT status, approval, output_text, input_json FROM tool_calls WHERE id = 'call-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(tool.0, "success");
        assert_eq!(tool.1, "once");
        assert!(tool.2.contains("lib.rs:1:// TODO"));
        assert_eq!(tool.3, r#"{"pattern":"TODO","path":"lib.rs"}"#);
        let usage: (i64, i64, i64, i64, i64, String) = store
            .conn()
            .query_row(
                "SELECT input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
                 cost_microcents, model FROM token_usage",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(usage, (12, 3, 2, 1, 0, "mock-model".into()));

        let tables: Vec<String> = {
            let mut stmt = store
                .conn()
                .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(
            tables,
            vec![
                "messages",
                "permissions",
                "projects",
                "threads",
                "token_usage",
                "tool_calls"
            ]
        );
    }

    #[tokio::test]
    async fn retry_reuses_persisted_call_id_without_running_the_tool() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let first = scripted_provider("stable-call", r#"{"pattern":"TODO","path":"lib.rs"}"#);
        run_thread_task(
            &store,
            &first,
            &tools,
            "thread-1",
            "First",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        fs::remove_file(dir.path().join("lib.rs")).unwrap();

        let retry = scripted_provider("stable-call", r#"{"pattern":"TODO","path":"missing.rs"}"#);
        let run = run_thread_task(
            &store,
            &retry,
            &tools,
            "thread-1",
            "Retry",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.reused
                    && result.status == ToolCallStatus::Success
                    && result.output.contains("persist me")
        )));
        let count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE id = 'stable-call'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn rejected_unknown_tool_is_audited_without_execution() {
        let (store, dir, _project_id) = setup();
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
        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Write",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Rejected && result.output.contains("denied")
        )));
        let status: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, approval FROM tool_calls WHERE id = 'write-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, ("rejected".into(), "deny".into()));
        assert!(!run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Failed
                    || matches!(result.status, ToolCallStatus::Approved | ToolCallStatus::Running)
        )));
    }
}
