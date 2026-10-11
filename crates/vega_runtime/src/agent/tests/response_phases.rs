use super::*;

fn assembled_chat_response(finish_reason: &str, tool: bool) -> Vec<ProviderEvent> {
    let mut assembler = crate::openai::SseAssembler::default();
    let mut delta = serde_json::json!({"content":"useful partial"});
    if tool {
        delta["tool_calls"] = serde_json::json!([{
            "index":0,"id":"read","type":"function",
            "function":{"name":"read","arguments":"{\"path\":\"test.txt\"}"}
        }]);
    }
    let mut events = assembler
        .absorb(
            &serde_json::json!({
                "choices":[{"delta":delta,"finish_reason":finish_reason}],
                "usage":{"prompt_tokens":500,"completion_tokens":10}
            })
            .to_string(),
        )
        .unwrap();
    events.extend(assembler.finalize().unwrap());
    events
}

#[tokio::test]
async fn issue296_real_sse_finish_reason_controls_final_without_losing_text() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    for reason in ["stop", "content_filter", "future_provider_reason", ""] {
        let provider = MockProvider::new(vec![ScriptStep::events(assembled_chat_response(
            reason, false,
        ))]);
        let result = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.final_text, "useful partial");
        assert!(!result.failed && !result.interrupted);
        assert!(matches!(
            result.events.last(),
            Some(RuntimeEvent::Finished(RuntimeFinishReason::End))
        ));
        let is_final = result.events.iter().any(|event| {
            matches!(
                event,
                RuntimeEvent::AssistantResponseCompleted {
                    kind: RuntimeAssistantResponseKind::Final,
                    ..
                }
            )
        });
        assert_eq!(is_final, reason == "stop", "finish_reason={reason}");
    }
}

#[tokio::test]
async fn issue296_real_sse_unknown_finish_cannot_anchor_next_tool_round() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("test.txt"), "hello").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    for reason in ["tool_calls", "content_filter", "future_provider_reason"] {
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(assembled_chat_response(reason, true))],
            vec![ScriptStep::events(assembled_chat_response("stop", false))],
        ]);
        let mut req = request(vec![ChatMessage::new(ChatRole::User, "read")]);
        req.context_budget = Some(ContextBudget::new(100_000, 1_000, true).unwrap());
        req.context_source_version = Some(1);
        let result = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.final_text, "useful partialuseful partial");
        let decisions = result
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ContextAccountingUpdated(decision)
                    if decision.stage == ContextAccountingStage::PrimaryPreflight =>
                {
                    Some(decision)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(decisions.len(), 2);
        assert_eq!(
            decisions[1].source,
            if reason == "tool_calls" {
                ContextAccountingSource::UsageAnchored
            } else {
                ContextAccountingSource::Estimated
            },
            "finish_reason={reason}"
        );
        assert_eq!(
            decisions[1].provider_input_baseline,
            (reason == "tool_calls").then_some(500)
        );
    }
}

#[tokio::test]
async fn issue296_response_spans_follow_protocol_rounds_and_utf8_bytes() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("test.txt"), "hello").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("检查。".into()),
            ProviderEvent::ToolUse {
                id: "read".into(),
                name: "read".into(),
                input_json: r#"{"path":"test.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("答案。".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let result = run_agent(
        &provider,
        &tools,
        request(vec![ChatMessage::new(ChatRole::User, "read")]),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let responses: Vec<_> = result
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::AssistantResponseCompleted {
                response_id,
                text_start_bytes,
                text_end_bytes,
                kind,
            } => Some((response_id, *text_start_bytes, *text_end_bytes, *kind)),
            _ => None,
        })
        .collect();
    assert_eq!(result.final_text, "检查。答案。");
    assert_eq!(responses.len(), 2);
    assert_ne!(responses[0].0, responses[1].0);
    assert_eq!(
        (responses[0].1, responses[0].2, responses[0].3),
        (0, "检查。".len(), RuntimeAssistantResponseKind::Commentary)
    );
    assert_eq!(
        (responses[1].1, responses[1].2, responses[1].3),
        (
            "检查。".len(),
            result.final_text.len(),
            RuntimeAssistantResponseKind::Final
        )
    );
    let final_index = result
        .events
        .iter()
        .position(|event| {
            matches!(
                event,
                RuntimeEvent::AssistantResponseCompleted {
                    kind: RuntimeAssistantResponseKind::Final,
                    ..
                }
            )
        })
        .unwrap();
    assert!(matches!(
        result.events[final_index + 1],
        RuntimeEvent::Finished(RuntimeFinishReason::End)
    ));
}

#[tokio::test]
async fn issue296_response_final_requires_explicit_valid_end_and_nonempty_text() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    for events in [
        vec![ProviderEvent::TextDelta("partial".into())],
        vec![
            ProviderEvent::TextDelta("partial".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::Length,
            },
        ],
        vec![
            ProviderEvent::TextDelta("partial".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ],
        vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }],
        vec![
            ProviderEvent::TextDelta(" \n\t".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ],
        vec![
            ProviderEvent::TextDelta("partial".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
            ProviderEvent::TextDelta("late".into()),
        ],
    ] {
        let provider = MockProvider::new(vec![ScriptStep::events(events)]);
        let result = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!result.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::AssistantResponseCompleted {
                kind: RuntimeAssistantResponseKind::Final,
                ..
            }
        )));
    }
}

#[tokio::test]
async fn issue296_response_failed_or_cancelled_output_is_never_final() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    for terminal in [
        ScriptStep::Error {
            status: Some(503),
            message: "fixture failure".into(),
            retryable: false,
        },
        ScriptStep::Cancelled,
    ] {
        let provider = MockProvider::new(vec![ScriptStep::text("useful partial"), terminal]);
        let result = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.final_text, "useful partial");
        assert!(result.failed || result.interrupted);
        assert!(
            !result
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::AssistantResponseCompleted { .. }))
        );
    }
}
