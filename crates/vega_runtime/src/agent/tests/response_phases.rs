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
async fn continuous_compaction_empty_responses_have_distinct_parallel_tool_rounds() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("test.txt"), "hello").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let rounds = [vec!["a", "b"], vec!["c", "d"]]
        .into_iter()
        .map(|ids| {
            let mut events = ids
                .into_iter()
                .map(|id| ProviderEvent::ToolUse {
                    id: id.into(),
                    name: "read".into(),
                    input_json: r#"{"path":"test.txt"}"#.into(),
                })
                .collect::<Vec<_>>();
            events.push(ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            });
            vec![ScriptStep::events(events)]
        })
        .chain(std::iter::once(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("done".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]))
        .collect();
    let provider = MockProvider::new_rounds(rounds);
    let result = run_agent(
        &provider,
        &tools,
        request(vec![ChatMessage::new(ChatRole::User, "read")]),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let rounds = result
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolRoundCompleted {
                response_id,
                call_ids,
            } => Some((response_id, call_ids)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(rounds.len(), 2);
    assert_ne!(rounds[0].0, rounds[1].0);
    assert_eq!(rounds[0].1, &["a", "b"]);
    assert_eq!(rounds[1].1, &["c", "d"]);
}
