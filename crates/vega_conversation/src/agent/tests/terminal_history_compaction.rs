use super::super::pipeline::history_from_context_source_with_checkpoint;
use super::*;
use vega_runtime::{ChatMessage, ChatRole, ContextCompactionHook, ContextRuntimeError};
use vega_store::context_compaction::{latest_checkpoint, load_source};

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
        "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, output_text, status, approval, created_at, finished_at, text_offset_bytes) VALUES ('current-read', 'thread-1', 'current', 4, 'read', '{\"path\":\"lib.rs\"}', 'Current observation', 'success', ?1, 4, 4, 0)",
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
            source_owner_id: Some("current".into()),
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
        let result = compact_history(&reopened, &provider, history, estimate)
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
            let result = compact_history(&store, &provider, history, estimate).await;
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
