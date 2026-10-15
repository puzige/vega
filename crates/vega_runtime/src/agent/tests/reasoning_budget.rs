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
                usages: Vec::new(),
                usage_complete: false,
            })
        })
    }
}

#[tokio::test]
async fn issue372_repeated_compaction_releases_only_discarded_reasoning_and_replay() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("source.txt"), "source result").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let text = "progress ".repeat(4_000);
    let mut rounds = (0..10)
        .map(|index| vec![ScriptStep::events(tool_round(index, &text, true))])
        .collect::<Vec<_>>();
    rounds.push(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("final answer".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let provider = MockProvider::new_rounds(rounds);
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut req = request(vec![ChatMessage::new(
        ChatRole::User,
        "Task goal: finish the implementation. Latest correction: preserve tool pairing.",
    )]);
    req.reasoning = Some(FrozenReasoning {
        provider: "deepseek".into(),
        model: "mock".into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        choice: ReasoningChoice::ProviderDefault,
        supports_disabled: false,
        preserve_reasoning_content: true,
        disabled_wire: None,
        declared_efforts: Vec::new(),
    });
    let mut four_rounds = vec![ChatMessage::new(
        ChatRole::System,
        req.system_prompt.clone(),
    )];
    four_rounds.extend(req.history.clone());
    for index in 0..4 {
        four_rounds.push(ChatMessage::assistant_with_tools_and_reasoning(
            &text,
            Some("r".repeat(REASONING_TURN_MAX_BYTES)),
            vec![ChatToolCall {
                id: format!("call-{index}"),
                name: "read".into(),
                input_json: r#"{"path":"source.txt"}"#.into(),
            }],
        ));
        four_rounds.push(ChatMessage::tool_result(
            format!("call-{index}"),
            "source result",
        ));
    }
    let estimate =
        crate::estimate_wire_context(&four_rounds, &tool_definitions(RuntimeRunMode::Ask))
            .unwrap()
            .input_tokens;
    let input_budget = estimate * 5 / 4;
    req.context_budget = Some(ContextBudget::new(input_budget + 1_000, 1_000, true).unwrap());
    req.context_compaction_hook = Some(Arc::new(RetainLastRound {
        calls: calls.clone(),
        seen: seen.clone(),
    }));
    let mut streamed_bytes = 0usize;
    let outcome = run_agent_with_sink(&provider, &tools, req, CancellationToken::new(), |event| {
        if let RuntimeEvent::ThinkingDelta(delta) | RuntimeEvent::SummaryDelta(delta) = event {
            streamed_bytes += delta.len();
        }
        async { Ok(()) }
    })
    .await
    .unwrap();
    assert!(!outcome.failed);
    assert_eq!(
        outcome.final_text,
        format!("{}final answer", text.repeat(10))
    );
    assert_eq!(outcome.executed_tool_call_count, 10);
    assert!(streamed_bytes > REASONING_RUN_MAX_BYTES * 2);
    assert!(calls.load(Ordering::SeqCst) >= 2);
    assert_eq!(provider.requests().len(), 11);
    let requests = provider.requests();
    for request in &requests {
        let bytes = request
            .messages
            .iter()
            .flat_map(|message| &message.response_reasoning)
            .map(|item| item.to_string().len())
            .sum::<usize>();
        assert!(bytes <= REASONING_RUN_MAX_BYTES);
        let retained = request
            .messages
            .iter()
            .filter_map(|message| message.reasoning_content.as_ref())
            .map(String::len)
            .sum::<usize>();
        assert!(retained <= REASONING_RUN_MAX_BYTES);
    }
    for compacted in seen.lock().unwrap().iter() {
        assert!(
            compacted
                .messages
                .iter()
                .any(|message| message.content.contains("Task goal"))
        );
        assert!(
            compacted
                .messages
                .iter()
                .any(|message| message.content.contains("Latest correction"))
        );
    }
    assert!(
        requests
            .last()
            .unwrap()
            .messages
            .iter()
            .any(|message| message.content.contains("Progress"))
    );
    assert!(requests.last().unwrap().messages.iter().any(|message| {
        message
            .reasoning_content
            .as_ref()
            .is_some_and(|reasoning| reasoning.len() == REASONING_TURN_MAX_BYTES)
    }));
    let reasoning_events = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ThinkingDelta(delta) | RuntimeEvent::SummaryDelta(delta) => {
                Some(delta.len())
            }
            _ => None,
        })
        .sum::<usize>();
    assert!(reasoning_events <= REASONING_RUN_MAX_BYTES);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ToolCallFinished(_)))
            .count(),
        10
    );
}

#[tokio::test]
async fn issue372_retry_discards_partial_thinking_replay_and_tools_before_recharging() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("source.txt"), "source result").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let mut stream = Vec::new();
    for attempt in 1..=5 {
        stream.extend(bounded_thinking());
        stream.push(ProviderEvent::ReasoningReplay(vec![
            serde_json::json!({"type":"reasoning", "encrypted_content":"q".repeat(240 * 1024)}),
        ]));
        stream.push(ProviderEvent::ToolUse {
            id: format!("discarded-{attempt}"),
            name: "read".into(),
            input_json: r#"{"path":"source.txt"}"#.into(),
        });
        stream.push(ProviderEvent::RetryScheduled {
            attempt,
            max_retries: 5,
        });
    }
    stream.extend(tool_round(99, "accepted", true));
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(stream)],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let mut accepted_thinking = 0;
    let mut retry_count = 0;
    let outcome = run_agent_with_sink(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
        |event| {
            match event {
                RuntimeEvent::ThinkingDelta(_) => accepted_thinking += 1,
                RuntimeEvent::ResponseRetrying { .. } => {
                    accepted_thinking = 0;
                    retry_count += 1;
                }
                _ => {}
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(!outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_eq!(retry_count, 5);
    assert_eq!(accepted_thinking, 4);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ThinkingDelta(_)))
            .count(),
        0
    );
    assert!(
        provider.requests()[1]
            .messages
            .iter()
            .flat_map(|message| &message.tool_calls)
            .all(|call| call.id == "call-99")
    );
    assert_eq!(
        provider.requests()[1]
            .messages
            .iter()
            .flat_map(|message| &message.response_reasoning)
            .count(),
        1
    );
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
    let project = tempdir().unwrap();
    fs::write(project.path().join("source.txt"), "source result").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let reasoning = "推理".repeat(100_000);
    let replay = (0..300).map(|index| serde_json::json!({"id":format!("reasoning-{index}"), "type":"reasoning", "encrypted_content":"x".repeat(2048)})).collect::<Vec<_>>();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
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
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("finished".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let mut req = request(Vec::new());
    req.reasoning = Some(FrozenReasoning {
        provider: "deepseek".into(),
        model: "mock".into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        choice: ReasoningChoice::ProviderDefault,
        supports_disabled: false,
        preserve_reasoning_content: true,
        disabled_wire: None,
        declared_efforts: Vec::new(),
    });
    let mut prior = ChatMessage::new(ChatRole::Assistant, "prior reasoning");
    prior.reasoning_content = Some(reasoning.clone());
    prior.response_reasoning = replay.clone();
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
    assert_eq!(requests[0].messages[1], prior);
    let message = requests[1]
        .messages
        .iter()
        .find(|message| !message.tool_calls.is_empty())
        .unwrap();
    assert_eq!(
        message.reasoning_content.as_deref(),
        Some(reasoning.as_str())
    );
    assert_eq!(message.response_reasoning, replay);
}

#[derive(Clone, Copy)]
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
                Boundary::OverTarget => request.messages,
                Boundary::Cancelled => {
                    cancel.cancel();
                    vec![ChatMessage::new(ChatRole::User, "summary")]
                }
            };
            Ok(ContextCompactionResult {
                messages,
                source_version: request.source_version + 1,
                source_fingerprint: request.source_fingerprint,
                usages: Vec::new(),
                usage_complete: false,
            })
        })
    }
}

#[tokio::test]
async fn issue372_rejected_projection_and_cancellation_never_release_retained_capacity() {
    for behavior in [
        Boundary::Timeout,
        Boundary::SourceChanged,
        Boundary::InvalidSystem,
        Boundary::OverTarget,
        Boundary::Cancelled,
    ] {
        let project = tempdir().unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let mut history = vec![ChatMessage::new(ChatRole::User, "x".repeat(2_000_000))];
        for _ in 0..4 {
            let mut message = ChatMessage::new(ChatRole::Assistant, "prior completed round");
            message.reasoning_content = Some("r".repeat(REASONING_TURN_MAX_BYTES));
            history.push(message);
        }
        history.push(ChatMessage::new(ChatRole::User, "continue"));
        let mut req = request(history.clone());
        let estimate = crate::estimate_chat_context(
            &req.system_prompt,
            &history,
            &tool_definitions(RuntimeRunMode::Ask),
        )
        .unwrap()
        .input_tokens;
        req.context_budget =
            Some(ContextBudget::new(estimate * 11 / 10 + 1_000, 1_000, true).unwrap());
        req.context_operation_id = Some("owned-run".into());
        req.context_compaction_hook = Some(Arc::new(BoundaryHook(behavior)));
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("x".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]);
        let mut observed = Vec::new();
        let result =
            run_agent_with_sink(&provider, &tools, req, CancellationToken::new(), |event| {
                observed.push(event);
                async { Ok(()) }
            })
            .await;
        assert!(
            !observed
                .iter()
                .any(|event| matches!(event, RuntimeEvent::ReasoningRetentionReleased))
        );
        match behavior {
            Boundary::Timeout => {
                let outcome = result.unwrap();
                assert!(!outcome.failed);
                assert_eq!(&outcome.messages[1..1 + history.len()], history);
                assert_eq!(provider.requests().len(), 1);
                assert!(
                    !outcome
                        .events
                        .iter()
                        .any(|event| matches!(event, RuntimeEvent::Error(_)))
                );
            }
            Boundary::Cancelled => {
                let outcome = result.unwrap();
                assert!(outcome.interrupted);
                assert_eq!(&outcome.messages[1..], history);
                assert!(provider.requests().is_empty());
            }
            _ => {
                assert!(result.is_err());
                assert!(provider.requests().is_empty());
            }
        }
    }
}

#[tokio::test]
async fn issue372_retry_does_not_release_reasoning_retained_before_the_current_call() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("source.txt"), "source result").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let mut prior = ChatMessage::new(ChatRole::Assistant, "prior round");
    prior.reasoning_content = Some("r".repeat(REASONING_TURN_MAX_BYTES));
    let mut first = bounded_thinking();
    first.push(ProviderEvent::RetryScheduled {
        attempt: 1,
        max_retries: 5,
    });
    first.extend(tool_round(0, "", false));
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(first)],
        vec![ScriptStep::events(tool_round(1, "", false))],
        vec![ScriptStep::events(tool_round(2, "", false))],
        vec![ScriptStep::events(vec![
            ProviderEvent::SummaryDelta("x".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(vec![prior]),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed);
    assert_eq!(provider.requests().len(), 4);
    assert_eq!(outcome.executed_tool_call_count, 3);
    assert_eq!(
        provider.requests()[3].messages[1]
            .reasoning_content
            .as_ref()
            .unwrap()
            .len(),
        REASONING_TURN_MAX_BYTES
    );
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Error(_)))
    );
}

#[tokio::test]
async fn issue372_replay_item_edge_and_duplicate_protocol_failure_stay_distinct() {
    for (count, duplicate, expected) in [
        (256, false, None),
        (257, false, None),
        (1, true, Some(RuntimeDiagnosticFailure::ProviderProtocol)),
    ] {
        let project = tempdir().unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let mut events = vec![ProviderEvent::ReasoningReplay((0..count).map(|index| serde_json::json!({"id": format!("reasoning-{index}"), "type":"reasoning"})).collect())];
        if duplicate {
            events.push(ProviderEvent::ReasoningReplay(vec![
                serde_json::json!({"id":"duplicate", "type":"reasoning"}),
            ]));
        }
        events.push(ProviderEvent::Done {
            stop_reason: StopReason::End,
        });
        let provider = MockProvider::new(vec![ScriptStep::events(events)]);
        let outcome = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.failed, expected.is_some());
        assert_eq!(provider.requests().len(), 1);
        let failed = outcome.events.iter().find_map(|event| match event {
            RuntimeEvent::DiagnosticAttempt(attempt)
                if attempt.state == RuntimeDiagnosticState::Failed =>
            {
                attempt.failure
            }
            _ => None,
        });
        assert_eq!(failed, expected);
    }
}

#[tokio::test]
async fn issue375_non_replayed_thinking_exceeds_old_run_limit_without_context_pressure() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("source.txt"), "source result").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let mut rounds = (0..10)
        .map(|index| vec![ScriptStep::events(tool_round(index, "progress", false))])
        .collect::<Vec<_>>();
    rounds.push(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("finished".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let provider = MockProvider::new_rounds(rounds);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut req = request(vec![ChatMessage::new(
        ChatRole::User,
        "Task goal: finish implementation",
    )]);
    req.context_budget = Some(ContextBudget::new(10_000_000, 1_000, true).unwrap());
    req.context_compaction_hook = Some(Arc::new(RetainLastRound {
        calls: calls.clone(),
        seen: Arc::new(Mutex::new(Vec::new())),
    }));
    let mut observed_bytes = 0;
    let result = run_agent_with_sink(&provider, &tools, req, CancellationToken::new(), |event| {
        if let RuntimeEvent::ThinkingDelta(text) = event {
            observed_bytes += text.len();
        }
        async { Ok(()) }
    })
    .await
    .unwrap();
    assert!(!result.failed);
    assert!(result.final_text.ends_with("finished"));
    assert_eq!(result.executed_tool_call_count, 10);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider.requests().len(), 11);
    assert!(observed_bytes > REASONING_RUN_MAX_BYTES * 2);
    assert!(
        provider
            .requests()
            .iter()
            .flat_map(|request| &request.messages)
            .all(|message| message.reasoning_content.is_none())
    );
    let retained = result
        .events
        .iter()
        .map(|event| match event {
            RuntimeEvent::ThinkingDelta(text) | RuntimeEvent::SummaryDelta(text) => text.len(),
            _ => 0,
        })
        .sum::<usize>();
    assert!(retained <= REASONING_TURN_MAX_BYTES);
    assert!(
        !result
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ReasoningRetentionReleased))
    );
}

#[tokio::test]
async fn issue375_replay_items_across_rounds_follow_context_budget_not_lifetime_count() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("source.txt"), "source result").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let mut rounds = (0..6).map(|index| {
        let mut events = tool_round(index, "progress", false);
        events.retain(|event| !matches!(event, ProviderEvent::ThinkingDelta(_)));
        events.insert(0, ProviderEvent::ReasoningReplay((0..64).map(|item| serde_json::json!({"type":"reasoning", "id":format!("replay-{index}-{item}")})).collect()));
        vec![ScriptStep::events(events)]
    }).collect::<Vec<_>>();
    rounds.push(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let provider = MockProvider::new_rounds(rounds);
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "task goal")]);
    req.context_budget = Some(ContextBudget::new(1_000_000, 1_000, true).unwrap());
    let result = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!result.failed);
    assert_eq!(result.executed_tool_call_count, 6);
    assert_eq!(provider.requests().len(), 7);
    assert_eq!(
        provider.requests()[6]
            .messages
            .iter()
            .map(|message| message.response_reasoning.len())
            .sum::<usize>(),
        384
    );
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
