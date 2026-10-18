use super::super::pipeline::{
    history_from_context_source_with_checkpoint, source_after_checkpoint,
};
use super::*;
use vega_runtime::{ChatMessage, ChatRole, ContextCompactionHook, ContextRuntimeError};
use vega_store::context_compaction::{ContextCheckpoint, latest_checkpoint, load_source};

fn seed_terminal_history(store: &Store, status: &str, empty_text: bool) {
    let content = if empty_text {
        ""
    } else {
        "Checking earlier work. Preserving the observations."
    };
    store.conn().execute(
        "INSERT INTO messages (id, thread_id, seq, role, content, status, created_at) VALUES ('goal', 'thread-1', 1, 'user', 'Preserve the earlier work', 'done', 1), ('previous', 'thread-1', 2, 'assistant', ?1, ?2, 2)",
        (content, status),
    ).unwrap();
    let statuses = match status {
        "interrupted" => ["success", "success", "cancelled"],
        "failed" => ["success", "failed", "rejected"],
        _ => unreachable!(),
    };
    for (index, status) in statuses.into_iter().enumerate() {
        let id = format!("previous-read-{index}");
        let output = match status {
            "success" => format!("Earlier observation {index}"),
            "failed" => "Tool error: file read failed".into(),
            "rejected" => "Tool error: permission denied".into(),
            "cancelled" => vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT.into(),
            _ => unreachable!(),
        };
        let approval = ApprovalAudit {
            decision: if status == "rejected" {
                Approval::Deny
            } else {
                Approval::Once
            },
            note: None,
            source: if status == "rejected" {
                ApprovalSource::User
            } else {
                ApprovalSource::ReadonlyTool
            },
            danger: None,
        }
        .to_json()
        .unwrap();
        let offset = if empty_text {
            0
        } else if index < 2 {
            "Checking earlier work. ".len()
        } else {
            content.len()
        };
        store.conn().execute(
            "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, output_text, status, approval, created_at, finished_at, text_offset_bytes) VALUES (?1, 'thread-1', 'previous', ?2, 'read', '{\"path\":\"lib.rs\"}', ?3, ?4, ?5, 2, 2, ?6)",
            (&id, index as i64 + 1, output, status, approval, offset as i64),
        ).unwrap();
        store.conn().execute(
            "INSERT INTO tool_call_rounds (message_id, call_id, response_id) VALUES ('previous', ?1, ?2)",
            (&id, format!("previous-round-{}", usize::from(index == 2))),
        ).unwrap();
    }
}

fn seed_followup(store: &Store) {
    store.conn().execute(
        "INSERT INTO messages (id, thread_id, seq, role, content, status, created_at) VALUES ('next-user', 'thread-1', 3, 'user', 'Continue with the retained request', 'done', 3), ('current', 'thread-1', 4, 'assistant', '', 'done', 4)",
        [],
    ).unwrap();
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    }
    .to_json()
    .unwrap();
    store.conn().execute(
        "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, output_text, status, approval, created_at, finished_at, text_offset_bytes) VALUES ('current-read', 'thread-1', 'current', (SELECT COALESCE(MAX(seq), 0) + 1 FROM tool_calls), 'read', '{\"path\":\"lib.rs\"}', 'Current observation', 'success', ?1, 4, 4, 0)",
        [approval],
    ).unwrap();
    store.conn().execute(
        "INSERT INTO tool_call_rounds (message_id, call_id, response_id) VALUES ('current', 'current-read', 'current-round')",
        [],
    ).unwrap();
}

fn final_response(text: &str) -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta(text.into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]
}

fn assert_summary_preserves_results(
    provider: &MockProvider,
    source: &vega_store::context_compaction::ContextSource,
) {
    let requests = provider.requests();
    let summary = &requests[0];
    assert!(summary.tools.is_empty());
    assert!(summary.messages[0].content.contains("context-compaction"));
    for call in &source.tool_calls {
        assert!(
            summary.messages.iter().any(|message| {
                message.role == ChatRole::User
                    && serde_json::from_str::<serde_json::Value>(&message.content).is_ok_and(
                        |value| {
                            let observation = &value["vega_history_observation"];
                            observation["kind"] == "tool_result"
                                && observation["call_id"] == call.id
                                && observation["status"] == call.status
                                && observation["output"].as_str() == call.output_text.as_deref()
                        },
                    )
            }),
            "missing result for {}",
            call.id
        );
    }
}

async fn continue_terminal_history(status: &str) {
    let (store, dir, _) = setup();
    seed_terminal_history(&store, status, false);
    store
        .conn()
        .execute(
            "UPDATE messages SET content = content || ?1 WHERE id = 'goal'",
            [" saved context".repeat(3_200)],
        )
        .unwrap();
    let before = load_source(store.conn(), "thread-1").unwrap();
    vega_store::context_compaction::save_model_policy(
        store.conn(),
        &vega_store::context_compaction::ModelContextPolicy {
            provider: "mock-provider".into(),
            model: "mock-model".into(),
            input_limit: Some(20_000),
            output_reserve: Some(1_000),
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let provider = MockProvider::new_rounds(vec![
        final_response("Earlier work and terminal tool outcomes retained."),
        final_response("Continued successfully."),
    ]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = run_thread_task_with_permission_config_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Continue after the earlier attempt",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        Some(FrozenReasoning::unknown("mock-provider", "mock-model")),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert!(!run.interrupted);
    assert_eq!(run.content, "Continued successfully.");
    assert!(run.events.iter().all(|event| !matches!(
        event,
        ConversationEvent::ToolCallProposed { .. } | ConversationEvent::ToolCallRunning { .. }
    )));
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_summary_preserves_results(&provider, &before);
    assert!(!requests[1].tools.is_empty());
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| { message.content.contains("Historical context summary") })
    );
    assert!(requests[1].messages.iter().any(|message| {
        message.role == ChatRole::User && message.content == "Continue after the earlier attempt"
    }));
    assert!(
        requests[1]
            .messages
            .iter()
            .all(|message| { message.tool_calls.is_empty() && message.tool_call_id.is_none() })
    );
    let checkpoint = latest_checkpoint(store.conn(), "thread-1", "mock-model")
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.covered_through_seq, 2);
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == crate::types::ContextCompactionStatus::Succeeded
    )));
    let after = load_source(store.conn(), "thread-1").unwrap();
    assert_eq!(
        &after.messages[..before.messages.len()],
        before.messages.as_slice()
    );
    assert_eq!(after.tool_calls, before.tool_calls);
}

#[tokio::test]
async fn stopped_tool_history_continues_after_automatic_compaction() {
    continue_terminal_history("interrupted").await;
}

#[tokio::test]
async fn failed_tool_history_retries_after_automatic_compaction() {
    continue_terminal_history("failed").await;
}

async fn compact_history(
    store: &Store,
    provider: &MockProvider,
    messages: Vec<ChatMessage>,
    estimate: vega_runtime::ContextEstimate,
    owner: &str,
) -> Result<vega_runtime::ContextCompactionResult, vega_runtime::ContextCompactionFailure> {
    let source = load_source(store.conn(), "thread-1").unwrap();
    let budget = vega_runtime::ContextBudget::new(20_000, 1_000, true).unwrap();
    let hook = ConversationCompactionHook::new(
        provider,
        store.database_path().unwrap().to_path_buf(),
        "thread-1",
        "mock-model",
        None,
        None,
    );
    hook.compact(
        vega_runtime::ContextCompactionRequest {
            system_prompt: "system".into(),
            messages,
            tools: vec![],
            budget,
            estimate,
            target_tokens: budget.target_tokens().unwrap(),
            source_version: source.source_version,
            source_fingerprint: Some(source.fingerprint),
            require_source_fence: false,
            source_owner_id: Some(owner.into()),
        },
        CancellationToken::new(),
    )
    .await
}

#[tokio::test]
async fn reopened_terminal_history_compacts_without_losing_current_user_or_tool_suffix() {
    for status in ["interrupted", "failed"] {
        let (store, dir, _) = setup();
        seed_terminal_history(&store, status, true);
        seed_followup(&store);
        let before = load_source(store.conn(), "thread-1").unwrap();
        drop(store);
        let reopened = Store::open(dir.path().join("vega.db")).unwrap();
        let source = load_source(reopened.conn(), "thread-1").unwrap();
        assert_eq!(source, before);
        let history = history_from_context_source_with_checkpoint(&source, None, "").unwrap();
        let old_rounds = history
            .iter()
            .filter(|message| {
                message
                    .tool_calls
                    .iter()
                    .any(|call| call.id.starts_with("previous-"))
            })
            .collect::<Vec<_>>();
        assert_eq!(old_rounds.len(), 2);
        assert!(old_rounds.iter().all(|message| message.content.is_empty()));
        assert_eq!(old_rounds[0].tool_calls.len(), 2);
        assert_eq!(old_rounds[1].tool_calls[0].id, "previous-read-2");
        let suffix = history[history
            .iter()
            .position(|message| message.content == "Continue with the retained request")
            .unwrap()..]
            .to_vec();
        let estimate = vega_runtime::estimate_chat_context("system", &history, &[]).unwrap();
        let provider = MockProvider::new(final_response("Earlier observations summarized."));
        let result = compact_history(&reopened, &provider, history, estimate, "current")
            .await
            .unwrap();
        assert_eq!(provider.requests().len(), 1);
        let mut summarized = source.clone();
        summarized
            .tool_calls
            .retain(|call| call.message_id == "previous");
        assert_summary_preserves_results(&provider, &summarized);
        assert_eq!(&result.messages[1..], suffix.as_slice());
        assert_eq!(load_source(reopened.conn(), "thread-1").unwrap(), before);
        let checkpoint = latest_checkpoint(reopened.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap();
        assert_eq!(checkpoint.covered_through_seq, 2);
        drop(reopened);
        let reloaded = Store::open(dir.path().join("vega.db")).unwrap();
        let stored_checkpoint = latest_checkpoint(reloaded.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap();
        assert_eq!(stored_checkpoint.id, checkpoint.id);
        let reloaded_source = load_source(reloaded.conn(), "thread-1").unwrap();
        assert_eq!(reloaded_source, before);
        let projected = history_from_context_source_with_checkpoint(
            &reloaded_source,
            Some(&stored_checkpoint),
            "",
        )
        .unwrap();
        assert_eq!(projected, result.messages);
        assert_eq!(
            projected
                .iter()
                .flat_map(|message| &message.tool_calls)
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            ["current-read"]
        );
        assert_eq!(
            projected
                .iter()
                .filter_map(|message| message.tool_call_id.as_deref())
                .collect::<Vec<_>>(),
            ["current-read"]
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum HistoryMutation {
    AssistantText,
    ToolArguments,
    ToolOutput,
    ToolOrder,
    MissingResult,
    DuplicateResult,
    MissingStoredOutput,
    RunningTool,
    StreamingAssistant,
    UnknownAssistantStatus,
}

#[tokio::test]
async fn terminal_history_compaction_rejects_changed_or_incomplete_sources() {
    for status in ["interrupted", "failed"] {
        for mutation in [
            HistoryMutation::AssistantText,
            HistoryMutation::ToolArguments,
            HistoryMutation::ToolOutput,
            HistoryMutation::ToolOrder,
            HistoryMutation::MissingResult,
            HistoryMutation::DuplicateResult,
            HistoryMutation::MissingStoredOutput,
            HistoryMutation::RunningTool,
            HistoryMutation::StreamingAssistant,
            HistoryMutation::UnknownAssistantStatus,
        ] {
            let (store, _dir, _) = setup();
            seed_terminal_history(&store, status, false);
            seed_followup(&store);
            let source = load_source(store.conn(), "thread-1").unwrap();
            let mut history =
                history_from_context_source_with_checkpoint(&source, None, "").unwrap();
            let estimate = vega_runtime::estimate_chat_context("system", &history, &[]).unwrap();
            let assistant = history
                .iter()
                .position(|message| {
                    message
                        .tool_calls
                        .iter()
                        .any(|call| call.id == "previous-read-0")
                })
                .unwrap();
            match mutation {
                HistoryMutation::AssistantText => history[assistant].content.push_str(" changed"),
                HistoryMutation::ToolArguments => {
                    history[assistant].tool_calls[0].input_json = r#"{"path":"changed.rs"}"#.into()
                }
                HistoryMutation::ToolOutput => history[assistant + 1].content.push_str(" changed"),
                HistoryMutation::ToolOrder => {
                    history[assistant].tool_calls.reverse();
                    history.swap(assistant + 1, assistant + 2);
                }
                HistoryMutation::MissingResult => {
                    history.remove(assistant + 1);
                }
                HistoryMutation::DuplicateResult => {
                    history.insert(assistant + 2, history[assistant + 1].clone());
                }
                HistoryMutation::MissingStoredOutput => {
                    store
                        .conn()
                        .execute(
                            "UPDATE tool_calls SET output_text = NULL WHERE id = 'previous-read-0'",
                            [],
                        )
                        .unwrap();
                }
                HistoryMutation::RunningTool => {
                    store
                        .conn()
                        .execute(
                            "UPDATE tool_calls SET status = 'running' WHERE id = 'previous-read-0'",
                            [],
                        )
                        .unwrap();
                }
                HistoryMutation::StreamingAssistant => {
                    store
                        .conn()
                        .execute(
                            "UPDATE messages SET status = 'streaming' WHERE id = 'previous'",
                            [],
                        )
                        .unwrap();
                }
                HistoryMutation::UnknownAssistantStatus => {
                    store
                        .conn()
                        .execute(
                            "UPDATE messages SET status = 'unknown' WHERE id = 'previous'",
                            [],
                        )
                        .unwrap();
                }
            }
            let before = load_source(store.conn(), "thread-1").unwrap();
            let provider = MockProvider::new(Vec::new());
            let result = compact_history(&store, &provider, history, estimate, "current").await;
            let expected = if matches!(
                mutation,
                HistoryMutation::MissingResult
                    | HistoryMutation::DuplicateResult
                    | HistoryMutation::MissingStoredOutput
                    | HistoryMutation::RunningTool
            ) {
                ContextRuntimeError::InvalidProjection
            } else {
                ContextRuntimeError::SourceChanged
            };
            assert!(
                matches!(result, Err(failure) if matches!(failure.error.as_ref(), VegaError::Context(error) if *error == expected)),
                "{status}: {mutation:?}"
            );
            assert!(provider.requests().is_empty(), "{status}: {mutation:?}");
            assert_eq!(load_source(store.conn(), "thread-1").unwrap(), before);
            assert!(
                latest_checkpoint(store.conn(), "thread-1", "mock-model")
                    .unwrap()
                    .is_none()
            );
        }
    }
}

const PARTIAL_PREFIX: &str = "已核对🌙前缀。";

fn append_partial_round(store: &Store, round: i64, text: &str) {
    store
        .conn()
        .execute(
            "UPDATE messages SET content = content || ?1 WHERE id = 'previous'",
            [text],
        )
        .unwrap();
    let offset: i64 = store
        .conn()
        .query_row(
            "SELECT length(CAST(content AS BLOB)) FROM messages WHERE id = 'previous'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    }
    .to_json()
    .unwrap();
    for parallel in 0..2 {
        let seq = round * 2 + parallel;
        let id = format!("partial-read-{seq}");
        store.conn().execute(
            "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, output_text, status, approval, created_at, finished_at, text_offset_bytes) VALUES (?1, 'thread-1', 'previous', ?2, 'read', '{\"path\":\"lib.rs\"}', ?3, 'success', ?4, 2, 2, ?5)",
            (&id, seq, format!("Retained observation {seq}"), &approval, offset),
        ).unwrap();
        store.conn().execute(
            "INSERT INTO tool_call_rounds (message_id, call_id, response_id) VALUES ('previous', ?1, ?2)",
            (&id, format!("partial-round-{round}")),
        ).unwrap();
    }
}

fn partial_history(store: &Store) -> Vec<ChatMessage> {
    let source = load_source(store.conn(), "thread-1").unwrap();
    let checkpoint = latest_checkpoint(store.conn(), "thread-1", "mock-model").unwrap();
    history_from_context_source_with_checkpoint(&source, checkpoint.as_ref(), "").unwrap()
}

fn summary_result_ids(provider: &MockProvider) -> Vec<String> {
    provider.requests()[0]
        .messages
        .iter()
        .filter_map(|message| {
            let value = serde_json::from_str::<serde_json::Value>(&message.content).ok()?;
            let observation = &value["vega_history_observation"];
            (observation["kind"] == "tool_result")
                .then(|| observation["call_id"].as_str().unwrap().to_string())
        })
        .collect()
}

async fn seed_partial_checkpoint(store: &Store) -> ContextCheckpoint {
    store.conn().execute(
        "INSERT INTO messages (id, thread_id, seq, role, content, status, created_at) VALUES ('goal', 'thread-1', 1, 'user', 'Preserve the earlier work', 'done', 1), ('previous', 'thread-1', 2, 'assistant', ?1, 'done', 2)",
        [PARTIAL_PREFIX],
    ).unwrap();
    append_partial_round(store, 1, "");
    append_partial_round(store, 2, "");
    let history = partial_history(store);
    store
        .conn()
        .execute(
            "UPDATE messages SET status = 'streaming' WHERE id = 'previous'",
            [],
        )
        .unwrap();
    let before = load_source(store.conn(), "thread-1").unwrap();
    let provider = MockProvider::new(final_response("First partial summary."));
    let estimate = vega_runtime::estimate_chat_context("system", &history, &[]).unwrap();
    let result = compact_history(store, &provider, history, estimate, "previous")
        .await
        .unwrap();
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(
        summary_result_ids(&provider),
        ["partial-read-2", "partial-read-3"]
    );
    assert_eq!(load_source(store.conn(), "thread-1").unwrap(), before);
    let checkpoint = latest_checkpoint(store.conn(), "thread-1", "mock-model")
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.covered_through_seq, 1);
    assert_eq!(checkpoint.covered_assistant_id.as_deref(), Some("previous"));
    assert_eq!(
        checkpoint.covered_assistant_text_bytes,
        PARTIAL_PREFIX.len() as u64
    );
    assert_eq!(checkpoint.covered_assistant_tool_seq, 3);
    assert_eq!(checkpoint.expected_previous_id, None);
    assert_eq!(checkpoint.source_fingerprint, before.fingerprint);
    let retained = before
        .tool_calls
        .iter()
        .filter(|call| call.seq as u64 > checkpoint.covered_assistant_tool_seq)
        .collect::<Vec<_>>();
    assert_eq!(retained.len(), 2);
    assert!(retained.iter().all(|call| {
        call.text_offset_bytes == Some(PARTIAL_PREFIX.len() as i64)
            && call.text_offset_bytes.unwrap() > 0
    }));
    let projected = source_after_checkpoint(&before, Some(&checkpoint)).unwrap();
    assert_eq!(projected.tool_calls.len(), 2);
    assert!(
        projected
            .tool_calls
            .iter()
            .all(|call| call.text_offset_bytes == Some(0))
    );
    assert_eq!(result.messages, partial_history(store));
    assert!(result.messages[1].content.is_empty());
    assert_eq!(result.messages[1].tool_calls[0].id, "partial-read-4");
    checkpoint
}

async fn resume_partial_history(status: &str) {
    let (store, dir, _) = setup();
    let first = seed_partial_checkpoint(&store).await;
    let tail = format!("未总结尾部🌙{}", " retained evidence".repeat(2_600));
    store
        .conn()
        .execute(
            "UPDATE messages SET status = ?1, content = content || ?2 WHERE id = 'previous'",
            (status, &tail),
        )
        .unwrap();
    vega_store::context_compaction::save_model_policy(
        store.conn(),
        &vega_store::context_compaction::ModelContextPolicy {
            provider: "mock-provider".into(),
            model: "mock-model".into(),
            input_limit: Some(20_000),
            output_reserve: Some(1_000),
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let before = load_source(store.conn(), "thread-1").unwrap();
    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    assert_eq!(load_source(reopened.conn(), "thread-1").unwrap(), before);
    let history = partial_history(&reopened);
    assert_eq!(history.last().unwrap().content, tail);
    assert!(history[1].content.is_empty());
    let provider = MockProvider::new_rounds(vec![
        final_response("Retained tail summarized."),
        final_response("Continued after partial compaction."),
    ]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = run_thread_task_with_permission_config_and_reasoning(
        &reopened,
        &provider,
        &tools,
        "thread-1",
        "Continue with the remaining work",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        Some(FrozenReasoning::unknown("mock-provider", "mock-model")),
    )
    .await
    .unwrap();
    assert!(!run.failed && !run.interrupted);
    assert_eq!(run.content, "Continued after partial compaction.");
    assert!(run.events.iter().all(|event| !matches!(
        event,
        ConversationEvent::ToolCallProposed { .. } | ConversationEvent::ToolCallRunning { .. }
    )));
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == crate::types::ContextCompactionStatus::Succeeded
    )));
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].tools.is_empty());
    assert_eq!(
        summary_result_ids(&provider),
        ["partial-read-4", "partial-read-5"]
    );
    assert!(
        requests[0]
            .messages
            .iter()
            .any(|message| message.content.contains(&first.summary))
    );
    assert!(
        requests[0]
            .messages
            .iter()
            .any(|message| message.content == tail)
    );
    assert!(
        requests[0]
            .messages
            .iter()
            .all(|message| !message.content.contains(PARTIAL_PREFIX))
    );
    assert!(requests[1].messages.iter().any(|message| {
        message.role == ChatRole::User && message.content == "Continue with the remaining work"
    }));
    assert!(
        requests[1]
            .messages
            .iter()
            .all(|message| { message.tool_calls.is_empty() && message.tool_call_id.is_none() })
    );
    let checkpoint = latest_checkpoint(reopened.conn(), "thread-1", "mock-model")
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.expected_previous_id, Some(first.id));
    assert_eq!(checkpoint.covered_through_seq, 2);
    assert_eq!(checkpoint.covered_assistant_id, None);
    let after = load_source(reopened.conn(), "thread-1").unwrap();
    assert_eq!(
        &after.messages[..before.messages.len()],
        before.messages.as_slice()
    );
    assert_eq!(after.tool_calls, before.tool_calls);
    let projection = partial_history(&reopened);
    drop(reopened);
    let reloaded = Store::open(dir.path().join("vega.db")).unwrap();
    assert_eq!(load_source(reloaded.conn(), "thread-1").unwrap(), after);
    assert_eq!(partial_history(&reloaded), projection);
    assert_eq!(
        latest_checkpoint(reloaded.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap(),
        checkpoint
    );
    assert_eq!(projection.len(), 3);
    assert!(projection[0].content.contains("Retained tail summarized."));
    assert_eq!(projection[1].content, "Continue with the remaining work");
    assert_eq!(projection[2].content, "Continued after partial compaction.");
}

#[tokio::test]
async fn stopped_partial_history_reopens_and_automatically_compacts_again() {
    resume_partial_history("interrupted").await;
}

#[tokio::test]
async fn failed_partial_history_reopens_and_automatically_compacts_again() {
    resume_partial_history("failed").await;
}

#[tokio::test]
async fn completed_partial_history_uses_latest_cursor_and_retains_live_owner() {
    let (store, _dir, _) = setup();
    let first = seed_partial_checkpoint(&store).await;
    let step = "第二阶段✅";
    append_partial_round(&store, 3, step);
    append_partial_round(&store, 4, "");
    let before = load_source(store.conn(), "thread-1").unwrap();
    let history = partial_history(&store);
    let estimate = vega_runtime::estimate_chat_context("system", &history, &[]).unwrap();
    let provider = MockProvider::new(final_response("Second partial summary."));
    let result = compact_history(&store, &provider, history, estimate, "previous")
        .await
        .unwrap();
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(
        summary_result_ids(&provider),
        [
            "partial-read-4",
            "partial-read-5",
            "partial-read-6",
            "partial-read-7"
        ]
    );
    assert!(
        provider.requests()[0]
            .messages
            .iter()
            .any(|message| message.content.contains(&first.summary))
    );
    assert_eq!(load_source(store.conn(), "thread-1").unwrap(), before);
    let second = latest_checkpoint(store.conn(), "thread-1", "mock-model")
        .unwrap()
        .unwrap();
    assert_eq!(second.expected_previous_id, Some(first.id));
    assert_eq!(second.covered_through_seq, 1);
    assert_eq!(second.covered_assistant_id.as_deref(), Some("previous"));
    assert_eq!(
        second.covered_assistant_text_bytes,
        (PARTIAL_PREFIX.len() + step.len()) as u64
    );
    assert_eq!(second.covered_assistant_tool_seq, 7);
    assert_eq!(second.source_fingerprint, before.fingerprint);
    assert_eq!(result.messages, partial_history(&store));
    assert!(result.messages[1].content.is_empty());
    let tail = "最后的未总结尾部。";
    store
        .conn()
        .execute(
            "UPDATE messages SET status = 'done', content = content || ?1 WHERE id = 'previous'",
            [tail],
        )
        .unwrap();
    seed_followup(&store);
    let history = partial_history(&store);
    let suffix_index = history
        .iter()
        .position(|message| message.content == "Continue with the retained request")
        .unwrap();
    let suffix = history[suffix_index..].to_vec();
    store
        .conn()
        .execute(
            "UPDATE messages SET status = 'streaming' WHERE id = 'current'",
            [],
        )
        .unwrap();
    let before = load_source(store.conn(), "thread-1").unwrap();
    let estimate = vega_runtime::estimate_chat_context("system", &history, &[]).unwrap();
    let provider = MockProvider::new(final_response("Completed history summarized."));
    let result = compact_history(&store, &provider, history, estimate, "current")
        .await
        .unwrap();
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(
        summary_result_ids(&provider),
        ["partial-read-8", "partial-read-9"]
    );
    let requests = provider.requests();
    assert!(
        requests[0]
            .messages
            .iter()
            .any(|message| message.content.contains(&second.summary))
    );
    assert!(
        requests[0]
            .messages
            .iter()
            .any(|message| message.content == tail)
    );
    assert!(requests[0].messages.iter().all(|message| {
        !message.content.contains(PARTIAL_PREFIX)
            && !message.content.contains(step)
            && !message.content.contains(&first.summary)
    }));
    assert_eq!(&result.messages[1..], suffix.as_slice());
    assert_eq!(
        result.messages.last().unwrap().tool_call_id.as_deref(),
        Some("current-read")
    );
    assert_eq!(load_source(store.conn(), "thread-1").unwrap(), before);
    let third = latest_checkpoint(store.conn(), "thread-1", "mock-model")
        .unwrap()
        .unwrap();
    assert_eq!(third.expected_previous_id, Some(second.id));
    assert_eq!(third.covered_through_seq, 2);
    assert_eq!(third.covered_assistant_id, None);
}

#[derive(Clone, Copy, Debug)]
enum PartialHistoryMutation {
    Text,
    Arguments,
    Output,
    CallOrder,
    Offset,
    MissingResult,
    DuplicateResult,
    MissingRound,
}

#[tokio::test]
async fn partial_history_rejects_retained_tail_mutations_without_replacing_checkpoint() {
    for mutation in [
        PartialHistoryMutation::Text,
        PartialHistoryMutation::Arguments,
        PartialHistoryMutation::Output,
        PartialHistoryMutation::CallOrder,
        PartialHistoryMutation::Offset,
        PartialHistoryMutation::MissingResult,
        PartialHistoryMutation::DuplicateResult,
        PartialHistoryMutation::MissingRound,
    ] {
        let (store, _dir, _) = setup();
        let checkpoint = seed_partial_checkpoint(&store).await;
        let tail = "保留🌙尾部";
        store.conn().execute(
            "UPDATE messages SET status = 'failed', content = content || ?1 WHERE id = 'previous'",
            [tail],
        ).unwrap();
        seed_followup(&store);
        let mut history = partial_history(&store);
        let estimate = vega_runtime::estimate_chat_context("system", &history, &[]).unwrap();
        let assistant = history
            .iter()
            .position(|message| {
                message
                    .tool_calls
                    .iter()
                    .any(|call| call.id == "partial-read-4")
            })
            .unwrap();
        let tail_index = history
            .iter()
            .position(|message| message.content == tail)
            .unwrap();
        assert!(history[assistant].content.is_empty());
        match mutation {
            PartialHistoryMutation::Text => history[tail_index].content.push_str(" changed"),
            PartialHistoryMutation::Arguments => {
                history[assistant].tool_calls[0].input_json = r#"{"path":"changed.rs"}"#.into()
            }
            PartialHistoryMutation::Output => history[assistant + 1].content.push_str(" changed"),
            PartialHistoryMutation::CallOrder => {
                history[assistant].tool_calls.reverse();
                history.swap(assistant + 1, assistant + 2);
            }
            PartialHistoryMutation::Offset => {
                let character = history[tail_index].content.remove(0);
                history[assistant].content.push(character);
            }
            PartialHistoryMutation::MissingResult => {
                history.remove(assistant + 1);
            }
            PartialHistoryMutation::DuplicateResult => {
                history.insert(assistant + 2, history[assistant + 1].clone());
            }
            PartialHistoryMutation::MissingRound => {
                drop(history.drain(assistant..assistant + 3));
            }
        }
        let before = load_source(store.conn(), "thread-1").unwrap();
        let provider = MockProvider::new(Vec::new());
        let result = compact_history(&store, &provider, history, estimate, "current").await;
        let expected = if matches!(
            mutation,
            PartialHistoryMutation::MissingResult | PartialHistoryMutation::DuplicateResult
        ) {
            ContextRuntimeError::InvalidProjection
        } else {
            ContextRuntimeError::SourceChanged
        };
        assert!(
            matches!(result, Err(failure) if matches!(failure.error.as_ref(), VegaError::Context(error) if *error == expected)),
            "{mutation:?}"
        );
        assert!(provider.requests().is_empty(), "{mutation:?}");
        assert_eq!(load_source(store.conn(), "thread-1").unwrap(), before);
        assert_eq!(
            latest_checkpoint(store.conn(), "thread-1", "mock-model")
                .unwrap()
                .unwrap(),
            checkpoint
        );
    }
}

async fn runtime_progress_result(dir: &std::path::Path) -> ChatMessage {
    let tools = vega_tools::Tools::new(dir).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "previous-read-1".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        final_response("Verified the file."),
    ]);
    let outcome = vega_runtime::run_agent(
        &provider,
        &tools,
        vega_runtime::AgentRequest {
            model: "mock-model".into(),
            system_prompt: "system".into(),
            history: vec![ChatMessage::new(ChatRole::User, "Read the file")],
            max_tokens: None,
            completed_tool_results: Default::default(),
            tool_config: RuntimeToolConfig::default(),
            pricing_catalog: None,
            reasoning: None,
            replay_frame: None,
            context_budget: None,
            context_source_version: None,
            context_source_fingerprint: None,
            context_operation_id: None,
            context_compaction_hook: None,
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed);
    let result = outcome
        .messages
        .into_iter()
        .find(|message| message.role == ChatRole::Tool)
        .unwrap();
    assert_ne!(result, result.without_runtime_guidance());
    assert!(!result.content.contains("<vega_runtime_notice>"));
    assert!(
        provider.requests()[1]
            .messages
            .last()
            .unwrap()
            .content
            .contains("<vega_runtime_notice>")
    );
    result
}

#[tokio::test]
async fn runtime_progress_metadata_is_ignored_by_compaction_but_tool_tampering_is_not() {
    for status in ["interrupted", "failed"] {
        for tamper in ["none", "content", "reasoning", "call_id"] {
            let (store, dir, _) = setup();
            let reminder = runtime_progress_result(dir.path()).await;
            seed_terminal_history(&store, status, true);
            seed_followup(&store);
            store
                .conn()
                .execute(
                    "UPDATE tool_calls SET output_text = ?1 WHERE id = 'previous-read-1'",
                    [&reminder.content],
                )
                .unwrap();
            let source = load_source(store.conn(), "thread-1").unwrap();
            let mut history =
                history_from_context_source_with_checkpoint(&source, None, "").unwrap();
            let index = history
                .iter()
                .position(|message| message.tool_call_id.as_deref() == Some("previous-read-1"))
                .unwrap();
            assert_eq!(history[index], reminder.without_runtime_guidance());
            history[index] = reminder;
            match tamper {
                "none" => {}
                "content" => history[index].content.push_str(" changed"),
                "reasoning" => {
                    history[index].reasoning_content = Some("unexpected tool state".into())
                }
                "call_id" => history[index].tool_call_id = Some("other-call".into()),
                _ => unreachable!(),
            }
            let estimate = vega_runtime::estimate_chat_context("system", &history, &[]).unwrap();
            let provider = MockProvider::new(final_response("Earlier observations summarized."));
            let result = compact_history(&store, &provider, history, estimate, "current").await;
            if tamper == "none" {
                let result = result.unwrap();
                assert_eq!(provider.requests().len(), 1);
                assert!(
                    provider.requests()[0]
                        .messages
                        .iter()
                        .all(|message| !message.content.contains("<vega_runtime_notice>"))
                );
                assert_eq!(load_source(store.conn(), "thread-1").unwrap(), source);
                let checkpoint = latest_checkpoint(store.conn(), "thread-1", "mock-model")
                    .unwrap()
                    .unwrap();
                drop(store);
                let reopened = Store::open(dir.path().join("vega.db")).unwrap();
                let reloaded = load_source(reopened.conn(), "thread-1").unwrap();
                assert_eq!(reloaded, source);
                assert_eq!(
                    history_from_context_source_with_checkpoint(&reloaded, Some(&checkpoint), "")
                        .unwrap(),
                    result.messages
                );
            } else {
                let expected = if tamper == "call_id" {
                    ContextRuntimeError::InvalidProjection
                } else {
                    ContextRuntimeError::SourceChanged
                };
                assert!(
                    matches!(result, Err(failure) if matches!(failure.error.as_ref(), VegaError::Context(error) if *error == expected)),
                    "{status}: {tamper}"
                );
                assert!(provider.requests().is_empty(), "{status}: {tamper}");
                assert_eq!(load_source(store.conn(), "thread-1").unwrap(), source);
                assert!(
                    latest_checkpoint(store.conn(), "thread-1", "mock-model")
                        .unwrap()
                        .is_none()
                );
            }
        }
    }
}

#[tokio::test]
async fn stopped_or_failed_silent_run_reopens_and_compacts_without_persisting_progress_notice() {
    for failed in [false, true] {
        let (store, dir, _) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "silent-read".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"lib.rs"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![if failed {
                ScriptStep::Error {
                    status: Some(400),
                    message: "provider rejected".into(),
                    retryable: false,
                }
            } else {
                ScriptStep::Cancelled
            }],
        ]);
        let first = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            &"Preserve the task. ".repeat(3_200),
            "system",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(first.failed, failed);
        assert_eq!(first.interrupted, !failed);
        assert_eq!(first.content, "");
        assert!(
            provider.requests()[1]
                .messages
                .iter()
                .any(|message| message.role == ChatRole::Tool
                    && message.content.contains("<vega_runtime_notice>"))
        );
        let source = load_source(store.conn(), "thread-1").unwrap();
        assert!(
            source
                .messages
                .iter()
                .all(|message| !message.content.contains("<vega_runtime_notice>"))
        );
        assert!(source.tool_calls.iter().all(|call| {
            !call
                .output_text
                .as_deref()
                .unwrap_or_default()
                .contains("<vega_runtime_notice>")
        }));
        vega_store::context_compaction::save_model_policy(
            store.conn(),
            &vega_store::context_compaction::ModelContextPolicy {
                provider: "mock-provider".into(),
                model: "mock-model".into(),
                input_limit: Some(20_000),
                output_reserve: Some(1_000),
                automatic_compaction: true,
                updated_at: 1,
            },
        )
        .unwrap();
        drop(store);
        let reopened = Store::open(dir.path().join("vega.db")).unwrap();
        let continued_provider = MockProvider::new_rounds(vec![
            final_response("Preserve earlier work and continue the task."),
            final_response("Continued successfully."),
        ]);
        let continued = run_thread_task_with_permission_config_and_reasoning(
            &reopened,
            &continued_provider,
            &tools,
            "thread-1",
            "Continue",
            "system",
            CancellationToken::new(),
            &RejectPermissionHook,
            |_| Ok(()),
            PersistenceActorConfig::default(),
            None,
            None,
            Some(FrozenReasoning::unknown("mock-provider", "mock-model")),
        )
        .await
        .unwrap();
        assert!(!continued.failed && !continued.interrupted);
        assert_eq!(continued_provider.requests().len(), 2);
        assert_eq!(continued.content, "Continued successfully.");
        assert!(continued_provider.requests()[0].tools.is_empty());
        assert!(
            continued_provider
                .requests()
                .iter()
                .flat_map(|request| &request.messages)
                .all(|message| !message.content.contains("<vega_runtime_notice>"))
        );
        assert_eq!(
            load_source(reopened.conn(), "thread-1").unwrap().tool_calls,
            source.tool_calls
        );
    }
}
