use super::*;
use vega_store::run_diagnostics::{DiagnosticPhase, DiagnosticState};

fn deepseek_reasoning() -> FrozenReasoning {
    FrozenReasoning {
        provider: "deepseek".into(),
        model: "mock-model".into(),
        protocol: vega_runtime::ReasoningProtocol::DeepSeekChatCompletions,
        replay_policy: vega_runtime::ReasoningReplayPolicy::DeepSeekTools,
        choice: vega_runtime::ReasoningChoice::ProviderDefault,
        supports_disabled: false,
        disabled_wire: None,
        declared_efforts: Vec::new(),
    }
}

fn responses_reasoning() -> FrozenReasoning {
    FrozenReasoning {
        provider: "custom".into(),
        model: "mock-model".into(),
        protocol: vega_runtime::ReasoningProtocol::ResponsesApi,
        replay_policy: vega_runtime::ReasoningReplayPolicy::ResponsesItems,
        choice: vega_runtime::ReasoningChoice::ProviderDefault,
        supports_disabled: false,
        disabled_wire: None,
        declared_efforts: Vec::new(),
    }
}

fn terminal_answer(text: &str) -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta(text.into()),
        ProviderEvent::ChatReasoningContent(vega_runtime::ChatReasoningPresence::NonEmpty(
            "terminal response state".into(),
        )),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]
}

fn terminal_answer_without_reasoning(text: &str) -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta(text.into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]
}

fn responses_terminal_answer(text: &str, response_id: &str) -> Vec<ScriptStep> {
    let reasoning = serde_json::json!({
        "type":"reasoning",
        "id":format!("{response_id}-reasoning"),
        "encrypted_content":"PRIVATE_TERMINAL_RESPONSE_STATE"
    });
    vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta(text.into()),
        ProviderEvent::ReasoningReplay(vec![reasoning.clone()]),
        ProviderEvent::ResponseReplay(vega_runtime::ResponseReplayLayout {
            response_id: Some(response_id.into()),
            slots: vec![
                vega_runtime::ResponseReplaySlot::Reasoning(reasoning),
                vega_runtime::ResponseReplaySlot::Message(serde_json::json!({
                    "type":"message",
                    "id":format!("{response_id}-message"),
                    "role":"assistant",
                    "status":"completed",
                    "phase":"final",
                    "content":[{"type":"output_text","text":text}]
                })),
            ],
        }),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]
}

struct MutatingCompactionProvider {
    inner: MockProvider,
    database_path: std::path::PathBuf,
    mutated: std::sync::atomic::AtomicBool,
}

impl vega_runtime::Provider for MutatingCompactionProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<
        'static,
        Result<vega_runtime::EventStream, vega_runtime::VegaError>,
    > {
        let inner = self.inner.clone();
        let database_path = self.database_path.clone();
        let mutate = !self.mutated.swap(true, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            if mutate {
                let store = Store::open(database_path).map_err(vega_runtime::VegaError::Store)?;
                store
                    .conn()
                    .execute(
                        "UPDATE messages SET content = 'Externally changed during compaction' \
                         WHERE id = (SELECT id FROM messages WHERE thread_id = 'thread-1' \
                         AND role = 'user' ORDER BY seq LIMIT 1)",
                        [],
                    )
                    .map_err(vega_runtime::VegaError::Store)?;
            }
            inner.chat_stream(request, cancel).await
        })
    }
}

async fn run_replay_turn(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    text: &str,
    reasoning: Option<FrozenReasoning>,
    registry: &ReplaySessionRegistry,
    route: [u8; 32],
) -> ConversationRun {
    run_thread_task_with_images_reasoning_and_mcp(
        store,
        provider,
        tools,
        "thread-1",
        text,
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_replay_session_registry(registry.clone(), route),
        None,
        None,
        reasoning,
        Vec::new(),
        Vec::new(),
    )
    .await
    .unwrap()
}

async fn automatic_replay_two_tools_and_final_raw_state(
    reasoning: FrozenReasoning,
    responses: bool,
    final_text: &str,
) -> Vec<vega_runtime::ChatRequest> {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let calls = [
        ("replay-read-one", r#"{"path":"lib.rs"}"#),
        ("replay-read-two", r#" { "path": "lib.rs", "offset": 1 } "#),
    ];
    let raw_tool_reasoning = vec![
        serde_json::json!({
            "type":"reasoning",
            "id":"tool-reasoning-one",
            "phase":"analysis",
            "encrypted_content":"PRIVATE_TOOL_STATE_ONE"
        }),
        serde_json::json!({
            "type":"reasoning",
            "id":"tool-reasoning-two",
            "phase":"analysis",
            "encrypted_content":"PRIVATE_TOOL_STATE_TWO"
        }),
    ];
    let mut first_tool_events = vec![ProviderEvent::TextDelta("Reading both files.".into())];
    if responses {
        first_tool_events.push(ProviderEvent::ReasoningReplay(raw_tool_reasoning.clone()));
        first_tool_events.push(ProviderEvent::ResponseReplay(
            vega_runtime::ResponseReplayLayout {
                response_id: Some("tool-response".into()),
                slots: vec![
                    vega_runtime::ResponseReplaySlot::Reasoning(raw_tool_reasoning[0].clone()),
                    vega_runtime::ResponseReplaySlot::Message(serde_json::json!({
                        "type":"message",
                        "id":"tool-message",
                        "role":"assistant",
                        "status":"completed",
                        "phase":"commentary",
                        "content":[{"type":"output_text","text":"Reading both files."}]
                    })),
                    vega_runtime::ResponseReplaySlot::FunctionCall {
                        item: serde_json::json!({
                            "type":"function_call",
                            "call_id":calls[0].0,
                            "name":"read",
                            "arguments":calls[0].1,
                            "phase":"commentary"
                        }),
                        call_id: calls[0].0.into(),
                        name: "read".into(),
                        arguments: calls[0].1.into(),
                    },
                    vega_runtime::ResponseReplaySlot::Reasoning(raw_tool_reasoning[1].clone()),
                    vega_runtime::ResponseReplaySlot::FunctionCall {
                        item: serde_json::json!({
                            "type":"function_call",
                            "call_id":calls[1].0,
                            "name":"read",
                            "arguments":calls[1].1,
                            "phase":"commentary"
                        }),
                        call_id: calls[1].0.into(),
                        name: "read".into(),
                        arguments: calls[1].1.into(),
                    },
                ],
            },
        ));
    } else {
        first_tool_events.push(ProviderEvent::ChatReasoningContent(
            vega_runtime::ChatReasoningPresence::NonEmpty("PRIVATE_CHAT_TOOL_STATE".into()),
        ));
    }
    for (id, arguments) in calls {
        first_tool_events.push(ProviderEvent::ToolUse {
            id: id.into(),
            name: "read".into(),
            input_json: arguments.into(),
        });
    }
    first_tool_events.push(ProviderEvent::Done {
        stop_reason: StopReason::ToolUse,
    });
    let mut first_final_events = vec![ProviderEvent::TextDelta(final_text.into())];
    let raw_final_reasoning = serde_json::json!({
        "type":"reasoning",
        "id":"final-reasoning",
        "phase":"analysis",
        "encrypted_content":"PRIVATE_FINAL_RESPONSE_STATE"
    });
    if responses {
        first_final_events.push(ProviderEvent::ReasoningReplay(vec![
            raw_final_reasoning.clone(),
        ]));
        let mut slots = vec![vega_runtime::ResponseReplaySlot::Reasoning(
            raw_final_reasoning,
        )];
        if !final_text.is_empty() {
            slots.push(vega_runtime::ResponseReplaySlot::Message(
                serde_json::json!({
                    "type":"message", "id":"final-message", "role":"assistant",
                    "status":"completed", "phase":"final",
                    "content":[{"type":"output_text","text":final_text}]
                }),
            ));
        }
        first_final_events.push(ProviderEvent::ResponseReplay(
            vega_runtime::ResponseReplayLayout {
                response_id: Some("final-response".into()),
                slots,
            },
        ));
    } else {
        first_final_events.push(ProviderEvent::ChatReasoningContent(
            vega_runtime::ChatReasoningPresence::NonEmpty("PRIVATE_FINAL_CHAT_STATE".into()),
        ));
    }
    first_final_events.push(ProviderEvent::Done {
        stop_reason: StopReason::End,
    });
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(first_tool_events)],
        vec![ScriptStep::events(first_final_events)],
        if responses {
            responses_terminal_answer("Second real user answer.", "second-response")
        } else {
            terminal_answer("Second real user answer.")
        },
    ]);
    let registry = ReplaySessionRegistry::new();
    let first = run_replay_turn(
        &store,
        &provider,
        &tools,
        "First request asks for two reads.",
        Some(reasoning.clone()),
        &registry,
        [38; 32],
    )
    .await;
    assert!(!first.failed);
    let second = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Second request follows both reads.",
        Some(reasoning),
        &registry,
        [38; 32],
    )
    .await;
    assert!(!second.failed);
    provider.requests()
}

#[tokio::test]
async fn automatic_replay_chat_and_responses_keep_two_tool_results_and_final_raw_state() {
    for final_text in ["First final answer after both reads.", ""] {
        let chat =
            automatic_replay_two_tools_and_final_raw_state(deepseek_reasoning(), false, final_text)
                .await;
        let responses =
            automatic_replay_two_tools_and_final_raw_state(responses_reasoning(), true, final_text)
                .await;
        for (requests, final_raw) in [
            (&chat, "PRIVATE_FINAL_CHAT_STATE"),
            (&responses, "PRIVATE_FINAL_RESPONSE_STATE"),
        ] {
            assert_eq!(requests.len(), 3);
            let replay = &requests[2].messages;
            let assistant = replay
                .iter()
                .find(|message| {
                    message.role == vega_runtime::ChatRole::Assistant
                        && message.content == "Reading both files."
                })
                .expect("two-call assistant response should replay");
            assert_eq!(assistant.tool_calls.len(), 2);
            assert_eq!(
                assistant
                    .tool_calls
                    .iter()
                    .map(|call| call.id.as_str())
                    .collect::<Vec<_>>(),
                ["replay-read-one", "replay-read-two"]
            );
            assert_eq!(assistant.tool_calls[0].input_json, r#"{"path":"lib.rs"}"#);
            assert_eq!(
                assistant.tool_calls[1].input_json,
                r#" { "path": "lib.rs", "offset": 1 } "#
            );
            assert_eq!(
                replay
                    .iter()
                    .filter(|message| message.role == vega_runtime::ChatRole::Tool)
                    .count(),
                2
            );
            let final_answer = replay
                .iter()
                .find(|message| {
                    message.role == vega_runtime::ChatRole::Assistant
                        && message.content == final_text
                })
                .expect("final assistant response should replay");
            if final_raw == "PRIVATE_FINAL_CHAT_STATE" {
                assert_eq!(
                    final_answer.reasoning_content.as_deref(),
                    Some("PRIVATE_FINAL_CHAT_STATE")
                );
            } else {
                assert!(
                    final_answer
                        .response_reasoning
                        .iter()
                        .any(|item| item["encrypted_content"] == final_raw)
                );
                let layout = final_answer
                    .response_replay_layout
                    .as_ref()
                    .expect("Responses final order should replay");
                assert_eq!(
                    layout.slots.len(),
                    if final_text.is_empty() { 1 } else { 2 }
                );
            }
        }
    }
}

#[tokio::test]
async fn automatic_replay_missing_final_state_does_not_seal_and_next_turn_is_cold() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ChatReasoningContent(vega_runtime::ChatReasoningPresence::NonEmpty(
                "complete tool response state".into(),
            )),
            ProviderEvent::ToolUse {
                id: "missing-final-state-read".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        terminal_answer_without_reasoning("Final reply omitted required state."),
        terminal_answer("Second user follows the failed task."),
    ]);
    let registry = ReplaySessionRegistry::new();
    let first = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Read then finish with complete state",
        Some(deepseek_reasoning()),
        &registry,
        [45; 32],
    )
    .await;
    assert!(first.failed);
    let second = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Continue after incomplete final response",
        Some(deepseek_reasoning()),
        &registry,
        [45; 32],
    )
    .await;
    assert!(!second.failed);
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    let cold = &requests[2].messages;
    assert!(cold.iter().all(|message| {
        message.role != vega_runtime::ChatRole::Assistant
            || message
                .tool_calls
                .iter()
                .all(|call| call.id != "missing-final-state-read")
    }));
    let tool_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE id = 'missing-final-state-read'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tool_count, 1);
}

#[tokio::test]
async fn automatic_replay_final_response_survives_next_user_and_route_change_cools_history() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Checking the file.".into()),
            ProviderEvent::ChatReasoningContent(vega_runtime::ChatReasoningPresence::NonEmpty(
                "private tool reasoning".into(),
            )),
            ProviderEvent::ToolUse {
                id: "read-once".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        terminal_answer("First final answer."),
        terminal_answer("Warm answer."),
        terminal_answer("Cold answer."),
    ]);
    let registry = ReplaySessionRegistry::new();
    let first = run_replay_turn(
        &store,
        &provider,
        &tools,
        "First user request",
        Some(deepseek_reasoning()),
        &registry,
        [7; 32],
    )
    .await;
    assert!(!first.failed);
    let second = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Second user request",
        Some(deepseek_reasoning()),
        &registry,
        [7; 32],
    )
    .await;
    assert!(!second.failed);
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    let warm = &requests[2].messages;
    assert!(warm.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Assistant
            && message.content == "Checking the file."
            && message.reasoning_content.as_deref() == Some("private tool reasoning")
            && message
                .tool_calls
                .iter()
                .any(|call| call.id == "read-once" && call.input_json == r#"{"path":"lib.rs"}"#)
    }));
    assert_eq!(
        warm.iter()
            .filter(|message| message.tool_calls.iter().any(|call| call.id == "read-once"))
            .count(),
        1
    );
    assert!(warm.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Assistant
            && message.content == "First final answer."
    }));
    let third = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Third user request",
        Some(deepseek_reasoning()),
        &registry,
        [8; 32],
    )
    .await;
    assert!(!third.failed);
    let cold = &provider.requests()[3].messages;
    assert!(cold.iter().all(|message| {
        message.role != vega_runtime::ChatRole::Assistant
            || !message.tool_calls.iter().any(|call| call.id == "read-once")
    }));
    assert!(cold.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message.content.contains("vega_history_observation")
            && message.content.contains("First final answer.")
    }));
    let durable_calls: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE thread_id = 'thread-1' AND id = 'read-once'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(durable_calls, 1);
}

#[tokio::test]
async fn automatic_replay_source_mutation_invalidates_the_sealed_candidate() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        terminal_answer("Before source change."),
        terminal_answer("After source change."),
    ]);
    let registry = ReplaySessionRegistry::new();
    let first = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Original source",
        Some(deepseek_reasoning()),
        &registry,
        [9; 32],
    )
    .await;
    assert!(!first.failed);
    store
        .conn()
        .execute(
            "UPDATE messages SET content = 'Externally changed source' WHERE id = ?1",
            [&first.user_message_id],
        )
        .unwrap();
    let second = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Next request",
        Some(deepseek_reasoning()),
        &registry,
        [9; 32],
    )
    .await;
    assert!(!second.failed);
    let request = &provider.requests()[1];
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message.content == "Externally changed source"
    }));
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message.content.contains("vega_history_observation")
            && message.content.contains("Before source change.")
            && message.reasoning_content.is_none()
    }));
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User && message.content == "Next request"
    }));
}

async fn seed_manual_replay_prefix(
    route: [u8; 32],
) -> (
    Store,
    tempfile::TempDir,
    vega_tools::Tools,
    MockProvider,
    ReplaySessionRegistry,
) {
    let (store, directory, _) = setup();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    crate::agent::save_context_settings(
        &store,
        &crate::types::ContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(50_000),
            output_reserve: 1_000,
            automatic_compaction: false,
            updated_at: 1,
        },
    )
    .unwrap();
    let provider = MockProvider::new_rounds(vec![
        terminal_answer("First durable answer before manual compaction."),
        terminal_answer("Second durable answer before manual compaction."),
        terminal_answer("Next answer after manual compaction."),
    ]);
    let registry = ReplaySessionRegistry::new();
    for text in [
        "First durable user request before manual compaction.",
        "Second durable user request before manual compaction.",
    ] {
        let run = run_replay_turn(
            &store,
            &provider,
            &tools,
            text,
            Some(deepseek_reasoning()),
            &registry,
            route,
        )
        .await;
        assert!(!run.failed);
    }
    (store, directory, tools, provider, registry)
}

async fn seed_automatic_replay_prefix(
    provider: &MockProvider,
    registry: &ReplaySessionRegistry,
    route: [u8; 32],
) -> (Store, tempfile::TempDir, vega_tools::Tools) {
    let (store, directory, _) = setup();
    vega_store::context_compaction::save_model_policy(
        store.conn(),
        &vega_store::context_compaction::ModelContextPolicy {
            provider: "deepseek".into(),
            model: "mock-model".into(),
            input_limit: Some(100_000),
            output_reserve: Some(1_000),
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    for text in [
        "First user before provider overflow compaction.",
        "Second user before provider overflow compaction.",
    ] {
        let run = run_replay_turn(
            &store,
            provider,
            &tools,
            text,
            Some(deepseek_reasoning()),
            registry,
            route,
        )
        .await;
        assert!(!run.failed);
        assert!(!run.interrupted);
    }
    (store, directory, tools)
}

#[tokio::test]
async fn automatic_replay_provider_overflow_rebases_the_cold_frame_and_usage_anchor() {
    let tool_id = "read-after-auto-replay-compaction";
    let tool_response = vec![
        ProviderEvent::TextDelta("Reading from the compacted context.".into()),
        ProviderEvent::ChatReasoningContent(vega_runtime::ChatReasoningPresence::NonEmpty(
            "AUTO_REPLAY_TOOL_RAW_STATE".into(),
        )),
        ProviderEvent::ToolUse {
            id: tool_id.into(),
            name: "read".into(),
            input_json: r#"{"path":"lib.rs"}"#.into(),
        },
        ProviderEvent::Usage {
            input: 730,
            output: 18,
            cache_read: Some(0),
            cache_write: Some(0),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ];
    let mut final_response = vec![ProviderEvent::TextDelta(
        "Completed after automatic provider-overflow compaction.".into(),
    )];
    final_response.extend([
        ProviderEvent::ChatReasoningContent(vega_runtime::ChatReasoningPresence::NonEmpty(
            "AUTO_REPLAY_FINAL_RAW_STATE".into(),
        )),
        ProviderEvent::Usage {
            input: 780,
            output: 22,
            cache_read: Some(0),
            cache_write: Some(0),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ]);
    let provider = MockProvider::new_rounds(vec![
        terminal_answer("First durable answer before provider overflow."),
        terminal_answer("Second durable answer before provider overflow."),
        vec![ScriptStep::Error {
            status: Some(400),
            message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
            retryable: false,
        }],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta(
                "AUTO_REPLAY_CHECKPOINT_SUMMARY replaces the pre-overflow prefix".into(),
            ),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(tool_response)],
        vec![ScriptStep::events(final_response)],
        terminal_answer("Next user after automatic provider-overflow compaction."),
    ]);
    let registry = ReplaySessionRegistry::new();
    let (store, _directory, tools) =
        seed_automatic_replay_prefix(&provider, &registry, [25; 32]).await;
    let first = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Current user triggering provider overflow.",
        Some(deepseek_reasoning()),
        &registry,
        [25; 32],
    )
    .await;
    assert!(!first.failed);
    assert!(!first.interrupted);
    let requests = provider.requests();
    assert_eq!(requests.len(), 6);
    assert!(!requests[2].tools.is_empty());
    assert!(requests[3].tools.is_empty());
    assert!(requests[3].messages.iter().any(|message| {
        message
            .content
            .contains("Current user triggering provider overflow.")
    }));
    let rebased = &requests[4].messages;
    let rebased_frame = rebased[0]
        .replay_frame()
        .expect("accepted provider overflow compaction retains its replay frame");
    assert_eq!(rebased_frame.cold_prefix_len(), 2);
    assert!(rebased.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message
                .content
                .contains("AUTO_REPLAY_CHECKPOINT_SUMMARY replaces the pre-overflow prefix")
    }));
    assert!(rebased.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message.content == "Current user triggering provider overflow."
    }));
    let after_tool = &requests[5].messages;
    assert_eq!(
        after_tool[0]
            .replay_frame()
            .expect("tool round keeps the rebased cold boundary")
            .cold_prefix_len(),
        2
    );
    assert!(after_tool.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Assistant
            && message.reasoning_content.as_deref() == Some("AUTO_REPLAY_TOOL_RAW_STATE")
            && message
                .tool_calls
                .iter()
                .any(|call| call.id == tool_id && call.input_json == r#"{"path":"lib.rs"}"#)
    }));
    assert!(after_tool.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Tool
            && message.tool_call_id.as_deref() == Some(tool_id)
    }));
    let compacted =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .expect("runtime overflow recovery installs the durable summary");
    assert!(compacted.summary.contains("AUTO_REPLAY_CHECKPOINT_SUMMARY"));
    assert!(first.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == crate::types::ContextCompactionStatus::Succeeded
    )));
    let accounting = first
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ContextAccounting { record, .. } => Some(*record),
            _ => None,
        })
        .collect::<Vec<_>>();
    let post_summary = accounting
        .iter()
        .find(|record| record.stage == crate::types::ContextAccountingStage::PostSummaryTarget)
        .expect("accepted summary publishes a fresh estimated budget source");
    assert_eq!(
        post_summary.source,
        crate::types::ContextAccountingSource::Estimated
    );
    assert_eq!(post_summary.covered_messages, 0);
    let anchored = accounting
        .iter()
        .find(|record| {
            record.stage == crate::types::ContextAccountingStage::PrimaryPreflight
                && record.source == crate::types::ContextAccountingSource::UsageAnchored
        })
        .expect("the accepted retry usage anchors only the rebased tool suffix");
    assert_eq!(anchored.provider_input_baseline, Some(730));
    assert_eq!(anchored.covered_messages, requests[4].messages.len());
    let second = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Next user after automatic provider-overflow compaction.",
        Some(deepseek_reasoning()),
        &registry,
        [25; 32],
    )
    .await;
    assert!(!second.failed);
    let requests_after_second = provider.requests();
    let warm = &requests_after_second[6].messages;
    assert_eq!(
        warm[0]
            .replay_frame()
            .expect("sealed post-compaction session keeps its accepted boundary")
            .cold_prefix_len(),
        2
    );
    assert!(warm.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Assistant
            && message.reasoning_content.as_deref() == Some("AUTO_REPLAY_TOOL_RAW_STATE")
    }));
    assert!(warm.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Assistant
            && message.reasoning_content.as_deref() == Some("AUTO_REPLAY_FINAL_RAW_STATE")
    }));
    assert!(warm.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message
                .content
                .contains("AUTO_REPLAY_CHECKPOINT_SUMMARY replaces the pre-overflow prefix")
    }));
    assert!(warm.iter().all(|message| {
        message.reasoning_content.as_deref() != Some("terminal response state")
    }));
    let next_accounting = second.events.iter().find_map(|event| match event {
        ConversationEvent::ContextAccounting { record, .. }
            if record.stage == crate::types::ContextAccountingStage::PrimaryPreflight =>
        {
            Some(*record)
        }
        _ => None,
    });
    let next_accounting = next_accounting.expect("the next worker estimates its warm request");
    assert_eq!(
        next_accounting.source,
        crate::types::ContextAccountingSource::Estimated
    );
    assert_eq!(next_accounting.covered_messages, 0);
    let executions: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE id = ?1",
            [tool_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(executions, 1);
}

#[tokio::test]
async fn automatic_replay_provider_overflow_cancellation_does_not_publish_a_rebased_frame() {
    let provider = MockProvider::new_rounds(vec![
        terminal_answer("First durable answer before cancelled provider overflow."),
        terminal_answer("Second durable answer before cancelled provider overflow."),
        vec![ScriptStep::Error {
            status: Some(400),
            message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
            retryable: false,
        }],
        vec![ScriptStep::Cancelled],
        terminal_answer("Next user after cancelled provider overflow compaction."),
    ]);
    let registry = ReplaySessionRegistry::new();
    let (store, _directory, tools) =
        seed_automatic_replay_prefix(&provider, &registry, [26; 32]).await;
    let interrupted = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Current user whose automatic compaction is cancelled.",
        Some(deepseek_reasoning()),
        &registry,
        [26; 32],
    )
    .await;
    assert!(interrupted.interrupted);
    assert!(!interrupted.failed);
    assert!(!provider.requests()[2].tools.is_empty());
    assert!(provider.requests()[3].tools.is_empty());
    assert!(interrupted.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == crate::types::ContextCompactionStatus::Cancelled
    )));
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
    let resumed = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Resume after cancelled provider overflow compaction.",
        Some(deepseek_reasoning()),
        &registry,
        [26; 32],
    )
    .await;
    assert!(!resumed.failed);
    let resumed_requests = provider.requests();
    let next = &resumed_requests[4].messages;
    let frame = next[0]
        .replay_frame()
        .expect("cancelled compaction cannot publish a replacement frame");
    assert_eq!(frame.cold_prefix_len(), next.len() - 1);
    assert!(next.iter().all(|message| {
        message.reasoning_content.as_deref() != Some("terminal response state")
            && !message
                .content
                .contains("AUTO_REPLAY_CHECKPOINT_SUMMARY replaces the pre-overflow prefix")
    }));
    assert!(next.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message.content == "Resume after cancelled provider overflow compaction."
    }));
}

#[tokio::test]
async fn automatic_replay_manual_checkpoint_success_invalidates_the_idle_prefix() {
    let (store, _directory, tools, provider, registry) = seed_manual_replay_prefix([21; 32]).await;
    let source_before =
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let summary_provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("MANUAL_CHECKPOINT_SUMMARY replaces the old replay prefix".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let events = crate::agent::compact_thread_manually_accounted(
        &store,
        &summary_provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        Some(deepseek_reasoning()),
        None,
        435,
    )
    .await
    .unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == crate::types::ContextCompactionStatus::Succeeded
    )));
    assert_eq!(summary_provider.requests().len(), 1);
    let source_after =
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    assert_eq!(source_before.fingerprint, source_after.fingerprint);
    let checkpoint =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .expect("production manual compaction installs the checkpoint");
    assert!(checkpoint.summary.contains("MANUAL_CHECKPOINT_SUMMARY"));
    let third = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Next user request after accepted compaction.",
        Some(deepseek_reasoning()),
        &registry,
        [21; 32],
    )
    .await;
    assert!(!third.failed);
    let request = &provider.requests()[2];
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message.content.contains("MANUAL_CHECKPOINT_SUMMARY")
    }));
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message.content == "Next user request after accepted compaction."
    }));
    assert!(request.messages.iter().all(|message| {
        !message
            .content
            .contains("First durable user request before manual compaction.")
            && !message
                .content
                .contains("First durable answer before manual compaction.")
    }));
    assert!(request.messages.iter().all(|message| {
        message.reasoning_content.is_none()
            && message.response_reasoning.is_empty()
            && message.response_replay_layout.is_none()
    }));
    let frame = request.messages[0]
        .replay_frame()
        .expect("next request uses the cold checkpoint projection");
    assert_eq!(frame.cold_prefix_len(), request.messages.len() - 1);
    let accounting = third.events.iter().find_map(|event| match event {
        ConversationEvent::ContextAccounting { record, .. }
            if record.stage == crate::types::ContextAccountingStage::PrimaryPreflight =>
        {
            Some(*record)
        }
        _ => None,
    });
    let accounting = accounting.expect("next request records its input anchor");
    assert_eq!(accounting.covered_messages, 0);
    assert_eq!(accounting.provider_input_baseline, None);
    assert_eq!(
        accounting.source,
        crate::types::ContextAccountingSource::Estimated
    );
}

#[tokio::test]
async fn automatic_replay_cancelled_manual_checkpoint_keeps_the_idle_prefix_fence() {
    let (store, _directory, tools, provider, registry) = seed_manual_replay_prefix([22; 32]).await;
    let source_before =
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let summary_provider = MockProvider::new(vec![]);
    let events = crate::agent::compact_thread_manually_accounted(
        &store,
        &summary_provider,
        "thread-1",
        "mock-model",
        "system",
        cancel,
        Some(deepseek_reasoning()),
        None,
        436,
    )
    .await
    .unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == crate::types::ContextCompactionStatus::Cancelled
    )));
    assert!(summary_provider.requests().is_empty());
    let source_after =
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    assert_eq!(source_before.fingerprint, source_after.fingerprint);
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
    let third = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Next user request after cancelled compaction.",
        Some(deepseek_reasoning()),
        &registry,
        [22; 32],
    )
    .await;
    assert!(!third.failed);
    let request = &provider.requests()[2];
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Assistant
            && message.content == "Second durable answer before manual compaction."
    }));
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message.content == "Next user request after cancelled compaction."
    }));
    assert!(
        request
            .messages
            .iter()
            .all(|message| { !message.content.contains("CANCELLED_CHECKPOINT_SUMMARY") })
    );
    assert_eq!(
        request.messages[0]
            .replay_frame()
            .expect("cancelled compaction keeps warm frame")
            .cold_prefix_len(),
        1
    );
}

#[tokio::test]
async fn automatic_replay_overflowed_manual_compaction_keeps_the_idle_prefix_fence() {
    let (store, _directory, tools, provider, registry) = seed_manual_replay_prefix([23; 32]).await;
    let source_before =
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let summary_provider = MockProvider::new(vec![ScriptStep::Error {
        status: Some(400),
        message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
        retryable: false,
    }]);
    let events = crate::agent::compact_thread_manually_accounted(
        &store,
        &summary_provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        Some(deepseek_reasoning()),
        None,
        437,
    )
    .await
    .unwrap();
    assert!(!summary_provider.requests().is_empty());
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == crate::types::ContextCompactionStatus::Failed
                && record.failure.is_some()
    )));
    let source_after =
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    assert_eq!(source_before.fingerprint, source_after.fingerprint);
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
    let third = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Next user request after rejected overflow compaction.",
        Some(deepseek_reasoning()),
        &registry,
        [23; 32],
    )
    .await;
    assert!(!third.failed);
    let request = &provider.requests()[2];
    assert_eq!(
        request.messages[0]
            .replay_frame()
            .expect("overflow rejection keeps warm frame")
            .cold_prefix_len(),
        1
    );
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Assistant
            && message.content == "Second durable answer before manual compaction."
    }));
}

#[tokio::test]
async fn automatic_replay_source_cas_rejection_colds_the_next_request() {
    let (store, _directory, tools, provider, registry) = seed_manual_replay_prefix([24; 32]).await;
    let source_before =
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let summary_provider = MutatingCompactionProvider {
        inner: MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("stale summary must not install".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]),
        database_path: store.database_path().unwrap().to_path_buf(),
        mutated: std::sync::atomic::AtomicBool::new(false),
    };
    let events = crate::agent::compact_thread_manually_accounted(
        &store,
        &summary_provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        Some(deepseek_reasoning()),
        None,
        438,
    )
    .await
    .unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == crate::types::ContextCompactionStatus::Failed
                && record.failure == Some(crate::types::ContextCompactionFailureCode::SourceChanged)
    )));
    let source_after =
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    assert_ne!(source_before.fingerprint, source_after.fingerprint);
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
    let third = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Next user request after source conflict.",
        Some(deepseek_reasoning()),
        &registry,
        [24; 32],
    )
    .await;
    assert!(!third.failed);
    let request = &provider.requests()[2];
    let frame = request.messages[0]
        .replay_frame()
        .expect("source mismatch falls back to cold history");
    assert_eq!(frame.cold_prefix_len(), request.messages.len() - 1);
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message
                .content
                .contains("Externally changed during compaction")
    }));
    assert!(request.messages.iter().all(|message| {
        message.reasoning_content.is_none()
            && message.response_reasoning.is_empty()
            && message.response_replay_layout.is_none()
    }));
}

#[tokio::test]
async fn automatic_replay_seal_rejects_postrun_source_drift_and_next_turn_is_cold() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    for row in [
        vega_store::messages::MessageRow {
            id: "prior-user".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "user".into(),
            kind: "text".into(),
            content: "Prior user source".into(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
        vega_store::messages::MessageRow {
            id: "prior-assistant".into(),
            thread_id: "thread-1".into(),
            seq: 2,
            role: "assistant".into(),
            kind: "text".into(),
            content: "Prior assistant source".into(),
            status: "done".into(),
            created_at: 2,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    ] {
        vega_store::messages::insert(store.conn(), &row).unwrap();
    }
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Reading the file.".into()),
            ProviderEvent::ChatReasoningContent(vega_runtime::ChatReasoningPresence::NonEmpty(
                "private read reasoning".into(),
            )),
            ProviderEvent::ToolUse {
                id: "drift-read".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        terminal_answer("Completed after reading."),
        terminal_answer("Continue after durable drift."),
    ]);
    let registry = ReplaySessionRegistry::new();
    let mut tampered = false;
    let first = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Run with a durable seal check",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |event| {
            if !tampered
                && matches!(
                    event,
                    ConversationEvent::MessageFinished {
                        stop_reason: crate::types::ConversationStopReason::End,
                        ..
                    }
                )
            {
                store.conn().execute_batch(
                    "UPDATE messages SET seq = 100 WHERE id = 'prior-user';
                     UPDATE messages SET seq = 1 WHERE id = 'prior-assistant';
                     UPDATE messages SET seq = 2, content = 'Externally changed prior text' WHERE id = 'prior-user';
                     UPDATE tool_calls SET output_text = 'Tampered durable tool result' WHERE id = 'drift-read';",
                )?;
                tampered = true;
            }
            Ok(())
        },
        PersistenceActorConfig::default()
            .with_replay_session_registry(registry.clone(), [12; 32]),
        None,
        None,
        Some(deepseek_reasoning()),
        Vec::new(),
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!first.failed);
    assert!(tampered);
    let second = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Next after tampering",
        Some(deepseek_reasoning()),
        &registry,
        [12; 32],
    )
    .await;
    assert!(!second.failed);
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    let cold = &requests[2].messages;
    assert!(cold.iter().all(|message| {
        message.role != vega_runtime::ChatRole::Assistant
            || !message
                .tool_calls
                .iter()
                .any(|call| call.id == "drift-read")
    }));
    let projected = cold
        .iter()
        .filter(|message| message.role == vega_runtime::ChatRole::User)
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(projected.contains("Externally changed prior text"));
    assert!(projected.contains("Tampered durable tool result"));
    assert!(projected.contains("Prior assistant source"));
    assert!(
        projected.find("Prior assistant source") < projected.find("Externally changed prior text")
    );
}

#[tokio::test]
async fn unknown_final_reasoning_is_visible_but_never_sealed_for_warm_replay() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ChatReasoningContent(vega_runtime::ChatReasoningPresence::NonEmpty(
                "unknown raw final state".into(),
            )),
            ProviderEvent::TextDelta("Unknown visible answer.".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        terminal_answer("Continue cold."),
    ]);
    let registry = ReplaySessionRegistry::new();
    let first = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Unknown protocol request",
        Some(FrozenReasoning::unknown("custom", "mock-model")),
        &registry,
        [10; 32],
    )
    .await;
    assert!(!first.failed);
    assert_eq!(first.content, "Unknown visible answer.");
    let second = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Next unknown request",
        Some(FrozenReasoning::unknown("custom", "mock-model")),
        &registry,
        [10; 32],
    )
    .await;
    assert!(!second.failed);
    let request = &provider.requests()[1];
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::User
            && message.content.contains("vega_history_observation")
            && message.content.contains("Unknown visible answer.")
    }));
    assert!(request.messages.iter().all(|message| {
        message.reasoning_content.as_deref() != Some("unknown raw final state")
    }));
}

#[tokio::test]
async fn automatic_replay_failure_uses_closed_code_and_reopens_typed_cause() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Unsupported state before a tool call.".into()),
        ProviderEvent::ChatReasoningContent(vega_runtime::ChatReasoningPresence::NonEmpty(
            "private unsupported state".into(),
        )),
        ProviderEvent::ToolUse {
            id: "must-not-execute".into(),
            name: "read".into(),
            input_json: r#"{"path":"lib.rs"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let registry = ReplaySessionRegistry::new();
    let run = run_replay_turn(
        &store,
        &provider,
        &tools,
        "Reject unknown reasoning replay",
        Some(FrozenReasoning::unknown("custom", "mock-model")),
        &registry,
        [44; 32],
    )
    .await;

    assert!(run.failed);
    assert_eq!(provider.requests().len(), 1);
    let tool_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))
        .unwrap();
    assert_eq!(tool_count, 0);
    let rows = vega_store::run_diagnostics::read_by_thread(store.conn(), "thread-1").unwrap();
    for (phase, expected_state) in [
        (DiagnosticPhase::PrimaryModel, DiagnosticState::Failed),
        (DiagnosticPhase::Run, DiagnosticState::Failed),
    ] {
        let row = rows
            .iter()
            .find(|row| {
                row.event.run_id == run.assistant_message_id
                    && row.event.phase == phase
                    && row.event.state == expected_state
            })
            .expect("reasoning failure diagnostic");
        assert_eq!(
            row.event.failure_code,
            Some(vega_store::run_diagnostics::DiagnosticFailureCode::ProviderProtocol)
        );
        assert_eq!(row.event.metrics.failure_kind.as_deref(), Some("protocol"));
    }
    let invalid_code_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM run_diagnostic_events WHERE failure_code = 'reasoning_replay'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(invalid_code_count, 0);

    let reopened = Store::open_read_only(store.database_path().unwrap()).unwrap();
    let projection = crate::agent::read_context_projection_with_provider(
        &reopened,
        "thread-1",
        "mock-model",
        None,
        "system",
    )
    .unwrap();
    assert!(projection.run_failures.contains(&(
        run.assistant_message_id,
        crate::types::RunFailureKind::ReasoningReplay
    )));
}

#[tokio::test]
async fn automatic_replay_steering_seals_the_real_final_assistant_owner() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    crate::pending_messages::enqueue(
        &store,
        &crate::types::PendingMessage {
            id: "steer-replay".into(),
            thread_id: "thread-1".into(),
            generation: "replay-run".into(),
            behavior: crate::types::PendingMessageBehavior::Steer,
            content: "Use the corrected instruction".into(),
            images: Vec::new(),
        },
    )
    .unwrap();
    let steering = crate::pending_messages::QueueSteeringSource::new(
        store.database_path().unwrap().into(),
        dir.path().into(),
        "thread-1".into(),
        "replay-run".into(),
        Arc::new(|| Ok(Vec::new())),
        CancellationToken::new(),
    );
    let provider = MockProvider::new_rounds(vec![
        terminal_answer("Superseded answer."),
        terminal_answer("Steered final answer."),
        terminal_answer("Next task answer."),
    ]);
    let registry = ReplaySessionRegistry::new();
    let first = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Initial instruction",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default()
            .with_pending_input(None, Some(Arc::new(steering)))
            .with_replay_session_registry(registry.clone(), [11; 32]),
        None,
        None,
        Some(deepseek_reasoning()),
        Vec::new(),
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!first.failed);
    assert_eq!(first.content, "Steered final answer.");
    let owner = messages::find(store.conn(), &first.assistant_message_id)
        .unwrap()
        .unwrap();
    assert_eq!(owner.role, "assistant");
    assert_eq!(owner.status, "done");
    let second = run_replay_turn(
        &store,
        &provider,
        &tools,
        "A later real user request",
        Some(deepseek_reasoning()),
        &registry,
        [11; 32],
    )
    .await;
    assert!(!second.failed);
    let request = &provider.requests()[2];
    assert!(request.messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Assistant
            && message.content == "Steered final answer."
    }));
}
