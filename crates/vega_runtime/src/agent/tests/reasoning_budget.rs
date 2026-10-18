use super::*;

fn bounded_thinking() -> Vec<ProviderEvent> {
    (0..4)
        .map(|_| ProviderEvent::ThinkingDelta("r".repeat(REASONING_DELTA_MAX_BYTES)))
        .collect()
}

fn tool_round(index: usize, text: &str, replay: bool) -> Vec<ProviderEvent> {
    let mut events = bounded_thinking();
    if replay {
        events.push(ProviderEvent::ReasoningReplay(vec![serde_json::json!({
            "id": format!("reasoning-{index}"),
            "type": "reasoning",
            "encrypted_content": "q".repeat(240 * 1024)
        })]));
    }
    events.extend([
        ProviderEvent::TextDelta(text.into()),
        ProviderEvent::ToolUse {
            id: format!("call-{index}"),
            name: "read".into(),
            input_json: r#"{"path":"source.txt"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ]);
    events
}

struct RetainLastRound {
    calls: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<ContextCompactionRequest>>>,
}

impl ContextCompactionHook for RetainLastRound {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(request.clone());
        let start = request
            .messages
            .iter()
            .rposition(|message| !message.tool_calls.is_empty())
            .unwrap();
        let mut messages = vec![ChatMessage::new(
            ChatRole::User,
            "Task goal: finish the implementation. Progress: prior read rounds completed. Latest correction: preserve tool pairing.",
        )];
        messages.extend_from_slice(&request.messages[start..]);
        Box::pin(async move {
            Ok(ContextCompactionResult {
                messages,
                source_version: request.source_version + 1,
                source_fingerprint: request.source_fingerprint,
                replay_cold_prefix_messages: Some(1),
                replay_interleaved_user_boundary: None,
                usages: Vec::new(),
                usage_complete: false,
            })
        })
    }
}

#[tokio::test]
async fn large_thinking_and_summary_complete_with_bounded_retention() {
    for summary in [false, true] {
        let project = tempdir().unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let text = "思考🧠".repeat(100_000);
        let delta = if summary {
            ProviderEvent::SummaryDelta(text.clone())
        } else {
            ProviderEvent::ThinkingDelta(text.clone())
        };
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            delta,
            ProviderEvent::TextDelta("finished".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]);
        let mut received = String::new();
        let cancel = CancellationToken::new();
        let outcome = run_agent_with_sink(
            &provider,
            &tools,
            request(Vec::new()),
            cancel.clone(),
            |event| {
                if let RuntimeEvent::ThinkingDelta(delta) | RuntimeEvent::SummaryDelta(delta) =
                    event
                {
                    received.push_str(&delta);
                }
                async { Ok(()) }
            },
        )
        .await
        .unwrap();
        assert!(!outcome.failed);
        assert!(!cancel.is_cancelled());
        assert_eq!(outcome.final_text, "finished");
        assert_eq!(received, text);
        let retained = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ThinkingDelta(delta) | RuntimeEvent::SummaryDelta(delta) => {
                    Some(delta.as_str())
                }
                _ => None,
            })
            .collect::<String>();
        assert!(!retained.is_empty());
        assert!(retained.len() <= REASONING_TURN_MAX_BYTES);
        assert!(text.starts_with(&retained));
        assert!(
            outcome
                .messages
                .iter()
                .all(|message| message.reasoning_content.is_none())
        );
        assert!(
            !outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::Error(_)))
        );
    }
}

#[tokio::test]
async fn large_reasoning_and_replay_reach_tool_continuation_unchanged() {
    for policy in [
        crate::ReasoningReplayPolicy::DeepSeekTools,
        crate::ReasoningReplayPolicy::ResponsesItems,
    ] {
        let project = tempdir().unwrap();
        fs::write(project.path().join("source.txt"), "source result").unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let reasoning = "推理".repeat(100_000);
        let replay = (0..300).map(|index| serde_json::json!({"id":format!("reasoning-{index}"), "type":"reasoning", "encrypted_content":"x".repeat(2048)})).collect::<Vec<_>>();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(replay_fixture_events(
                vec![
                    ProviderEvent::ThinkingDelta(reasoning.clone()),
                    ProviderEvent::ReasoningReplay(replay.clone()),
                    ProviderEvent::ToolUse {
                        id: "call".into(),
                        name: "read".into(),
                        input_json: r#"{"path":"source.txt"}"#.into(),
                    },
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ],
                policy,
            ))],
            vec![ScriptStep::events(replay_fixture_events(
                vec![
                    ProviderEvent::TextDelta("finished".into()),
                    ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    },
                ],
                policy,
            ))],
        ]);
        let mut req = replay_fixture_request(Vec::new(), policy);
        let mut prior = ChatMessage::new(ChatRole::Assistant, "prior reasoning");
        if policy == crate::ReasoningReplayPolicy::DeepSeekTools {
            prior.reasoning_content = Some(reasoning.clone());
        } else {
            prior.response_reasoning = replay.clone();
            let mut slots = replay
                .iter()
                .cloned()
                .map(crate::ResponseReplaySlot::Reasoning)
                .collect::<Vec<_>>();
            slots.push(crate::ResponseReplaySlot::Message(serde_json::json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"prior reasoning"}]})));
            prior.response_replay_layout = Some(crate::ResponseReplayLayout {
                response_id: None,
                slots,
            });
        }
        req.history.push(prior.clone());
        req.context_budget = Some(ContextBudget::new(10_000_000, 1_000, true).unwrap());
        let result = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert!(!result.failed);
        assert_eq!(result.final_text, "finished");
        assert_eq!(result.executed_tool_call_count, 1);
        let requests = provider.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].messages[1].content, prior.content);
        assert_eq!(
            requests[0].messages[1].reasoning_content,
            prior.reasoning_content
        );
        assert_eq!(
            requests[0].messages[1].response_reasoning,
            prior.response_reasoning
        );
        let message = requests[1]
            .messages
            .iter()
            .find(|message| !message.tool_calls.is_empty())
            .unwrap();
        if policy == crate::ReasoningReplayPolicy::DeepSeekTools {
            assert_eq!(
                message.reasoning_content.as_deref(),
                Some(reasoning.as_str())
            );
            assert!(message.response_reasoning.is_empty());
        } else {
            assert_eq!(message.response_reasoning, replay);
            assert!(message.reasoning_content.is_none());
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Boundary {
    Timeout,
    SourceChanged,
    InvalidSystem,
    OverTarget,
    Cancelled,
}

struct BoundaryHook(Boundary);

impl ContextCompactionHook for BoundaryHook {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        let behavior = self.0;
        Box::pin(async move {
            let messages = match behavior {
                Boundary::Timeout => {
                    return Err(ContextCompactionFailure::new(
                        VegaError::Context(ContextRuntimeError::SummaryTimedOut),
                        None,
                    ));
                }
                Boundary::SourceChanged => {
                    return Err(ContextCompactionFailure::new(
                        VegaError::Context(ContextRuntimeError::SourceChanged),
                        None,
                    ));
                }
                Boundary::InvalidSystem => vec![ChatMessage::new(ChatRole::System, "invalid")],
                Boundary::OverTarget => vec![ChatMessage::new(
                    ChatRole::User,
                    "x".repeat(usize::try_from(request.target_tokens).unwrap() * 4),
                )],
                Boundary::Cancelled => {
                    cancel.cancel();
                    vec![ChatMessage::new(ChatRole::User, "summary")]
                }
            };
            Ok(ContextCompactionResult {
                messages,
                source_version: request.source_version + 1,
                source_fingerprint: request.source_fingerprint,
                replay_cold_prefix_messages: None,
                replay_interleaved_user_boundary: None,
                usages: Vec::new(),
                usage_complete: false,
            })
        })
    }
}

#[tokio::test]
async fn cancellation_after_large_thinking_prevents_tool_execution() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("source.txt"), "source result").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ThinkingDelta("x".repeat(2 * REASONING_TURN_MAX_BYTES)),
        ProviderEvent::ToolUse {
            id: "not-executed".into(),
            name: "read".into(),
            input_json: r#"{"path":"source.txt"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let cancel = CancellationToken::new();
    let result = run_agent_with_sink(
        &provider,
        &tools,
        request(Vec::new()),
        cancel.clone(),
        |event| {
            if matches!(event, RuntimeEvent::ThinkingDelta(_)) {
                cancel.cancel();
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(result.interrupted);
    assert!(!result.failed);
    assert_eq!(result.executed_tool_call_count, 0);
    assert!(
        !result
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Error(_)))
    );
}
