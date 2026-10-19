use super::*;

#[tokio::test]
async fn a_later_nonempty_source_supersedes_an_earlier_empty_alias_without_fabricating_content() {
    for together in [false, true] {
        let mut script = Script::read("one", json!({"reasoning_content":""}));
        if together {
            script.chunks[0]["choices"][0]["delta"]["reasoning_text"] =
                json!("actual nonempty source");
        } else {
            script.chunks.insert(1,json!({"choices":[{"delta":{"reasoning_text":"actual nonempty source","content":"visible "}}]}));
        }
        let (outcome, provider) = run_scripts(
            "https://proxy.example/v1",
            "custom",
            vec![script, Script::final_reply(json!({}))],
        )
        .await;
        assert!(!outcome.failed);
        let bodies = provider.bodies();
        let assistant = tool_messages(&bodies[1])[0];
        assert_eq!(assistant["reasoning_text"], "actual nonempty source");
        assert!(assistant.get("reasoning_content").is_none());
        assert_eq!(
            outcome.final_text,
            if together {
                "complete"
            } else {
                "visible complete"
            }
        );
    }
}

#[tokio::test]
async fn chat_reasoning_aliases_preserve_source_presence_and_original_text_through_tools() {
    for field in ["reasoning_content", "reasoning", "reasoning_text"] {
        for raw in [Value::Null, json!(""), json!("exact raw\n思考")] {
            let state = json!({field:raw.clone()});
            let (outcome, provider) = run_scripts(
                "https://proxy.example/v1",
                "custom-reasoner",
                vec![
                    Script::read("first", state.clone()),
                    Script::read("second", state.clone()),
                    Script::final_reply(state),
                ],
            )
            .await;
            assert!(!outcome.failed, "{field}: {raw}");
            assert_eq!(outcome.final_text, "complete");
            assert_eq!(outcome.executed_tool_call_count, 2);
            let bodies = provider.bodies();
            let calls = tool_messages(&bodies[2]);
            assert_eq!(calls.len(), 2);
            for (index, message) in calls.into_iter().enumerate() {
                assert_eq!(message["tool_calls"][0]["id"], ["first", "second"][index]);
                if raw.is_null() {
                    assert!(message.get(field).is_none());
                } else {
                    assert_eq!(message[field], raw);
                }
                for other in ["reasoning_content", "reasoning", "reasoning_text"] {
                    if other != field {
                        assert!(message.get(other).is_none());
                    }
                }
            }
            let finished = outcome
                .events
                .iter()
                .filter_map(|event| match event {
                    RuntimeEvent::ToolCallFinished(result) => Some(result),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(finished.len(), 2);
            assert!(
                finished
                    .iter()
                    .all(|result| result.status == RuntimeToolStatus::Success
                        && result.output.contains("verified tool result"))
            );
        }
    }
}

#[tokio::test]
async fn duplicate_equal_aliases_emit_one_thinking_delta_and_conflicts_fail_before_tools() {
    let (outcome, provider) = run_scripts("https://proxy.example/v1", "custom-reasoner", vec![
        Script::read("same", json!({"reasoning_content":"one raw","reasoning":"one raw","reasoning_text":"one raw"})),
        Script::final_reply(json!({})),
    ]).await;
    assert!(!outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 1);
    let thinking = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ThinkingDelta(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(thinking, "one raw");
    let bodies = provider.bodies();
    assert_eq!(tool_messages(&bodies[1])[0]["reasoning_content"], "one raw");
    for (first, second) in [
        ("reasoning_content", "reasoning"),
        ("reasoning", "reasoning_text"),
    ] {
        let mut script = Script::read(
            "conflict",
            json!({first:"first raw",second:"different raw"}),
        );
        let (outcome, _) = run_scripts(
            "https://proxy.example/v1",
            "custom-reasoner",
            vec![script, Script::final_reply(json!({}))],
        )
        .await;
        assert_failed_without_tools(&outcome);
        script = Script::read("switch", json!({first:"first raw"}));
        script.chunks.insert(
            1,
            json!({"choices":[{"delta":{second:"other source increment"}}]}),
        );
        let (outcome, _) = run_scripts(
            "https://proxy.example/v1",
            "custom-reasoner",
            vec![script, Script::final_reply(json!({}))],
        )
        .await;
        assert_failed_without_tools(&outcome);
    }
}

#[tokio::test]
async fn malformed_text_and_chat_detail_types_fail_the_real_runtime_without_tool_side_effects() {
    for state in [
        json!({"reasoning_content":{"raw":"wrong"}}),
        json!({"reasoning":["wrong"]}),
        json!({"reasoning_text":42}),
        json!({"reasoning_details":"wrong"}),
        json!({"reasoning_details":[{"type":"reasoning.encrypted","data":42}]}),
        json!({"reasoning_details":[{"type":"reasoning.text","text":"a","index":"wrong"}]}),
        json!({"reasoning_details":[{"type":"reasoning.unknown","data":"wrong"}]}),
    ] {
        let (outcome, _) = run_scripts(
            "https://proxy.example/v1",
            "custom-reasoner",
            vec![
                Script::read("unsafe", state),
                Script::final_reply(json!({})),
            ],
        )
        .await;
        assert_failed_without_tools(&outcome);
    }
}

#[tokio::test]
async fn chat_details_merge_only_equal_metadata_and_preserve_order_opaque_values_and_empty_presence()
 {
    let mut first = Script::read(
        "first",
        json!({"reasoning_content":"duplicate text","reasoning_details":[
            {"type":"reasoning.text","id":"t","index":0,"format":"vendor-v1","text":"part ","signature":"signed","vendor":{"slot":7}},
            {"type":"reasoning.text","id":"t","index":0,"format":"vendor-v1","text":"two","signature":"signed","vendor":{"slot":7}},
            {"type":"reasoning.text","id":"t","index":0,"format":"vendor-v1","text":"separate","signature":"different","vendor":{"slot":7}},
            {"type":"reasoning.encrypted","id":"e1","index":1,"data":"opaque-one","format":"vendor-v1"},
            {"type":"reasoning.encrypted","id":"e2","index":2,"data":"opaque-two","format":"vendor-v1"},
            {"type":"reasoning.summary","id":null,"index":3,"summary":"summary ","format":"vendor-v1"}
        ]}),
    );
    first.chunks.insert(
        1,
        json!({"choices":[{"delta":{"reasoning_details":[
            {"type":"reasoning.summary","id":null,"index":3,"summary":"tail","format":"vendor-v1"}
        ]}}]}),
    );
    let (outcome, provider) = run_scripts("https://openrouter.ai/api/v1", "vendor/model", vec![
        first, Script::read("second", json!({"reasoning_details":[]})),
        Script::final_reply(json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"final","data":"final-opaque"}]})),
    ]).await;
    assert!(!outcome.failed);
    assert_eq!(outcome.final_text, "complete");
    assert_eq!(outcome.executed_tool_call_count, 2);
    let bodies = provider.bodies();
    let calls = tool_messages(&bodies[2]);
    assert_eq!(
        calls[0]["reasoning_details"],
        json!([
            {"type":"reasoning.text","id":"t","index":0,"format":"vendor-v1","text":"part two","signature":"signed","vendor":{"slot":7}},
            {"type":"reasoning.text","id":"t","index":0,"format":"vendor-v1","text":"separate","signature":"different","vendor":{"slot":7}},
            {"type":"reasoning.encrypted","id":"e1","index":1,"data":"opaque-one","format":"vendor-v1"},
            {"type":"reasoning.encrypted","id":"e2","index":2,"data":"opaque-two","format":"vendor-v1"},
            {"type":"reasoning.summary","id":null,"index":3,"summary":"summary tail","format":"vendor-v1"}
        ])
    );
    assert!(calls[0].get("reasoning_content").is_none());
    assert_eq!(calls[1]["reasoning_details"], json!([]));
    assert!(
        !outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::TextDelta(text) => Some(text),
                _ => None,
            })
            .any(|text| text.contains("opaque") || text.contains("duplicate text"))
    );
}

#[tokio::test]
async fn reasoning_detail_identity_conflicts_fail_before_any_tool_approval() {
    let state = json!({"reasoning_details":[
        {"type":"reasoning.encrypted","id":"same","index":0,"data":"one"},
        {"type":"reasoning.encrypted","id":"same","index":1,"data":"two"}
    ]});
    let (outcome, _) = run_scripts(
        "https://openrouter.ai/api/v1",
        "vendor/model",
        vec![
            Script::read("unsafe", state),
            Script::final_reply(json!({})),
        ],
    )
    .await;
    assert_failed_without_tools(&outcome);
}

#[tokio::test]
async fn opencode_go_reasoning_source_override_is_limited_to_its_real_service() {
    for (endpoint, expected) in [
        ("https://opencode.ai/zen/go/v1", "reasoning_content"),
        ("https://proxy.example/v1", "reasoning"),
    ] {
        let (outcome, provider) = run_scripts(
            endpoint,
            "kimi-k2.6",
            vec![
                Script::read("read", json!({"reasoning":"exact raw"})),
                Script::final_reply(json!({})),
            ],
        )
        .await;
        assert!(!outcome.failed);
        let bodies = provider.bodies();
        assert_eq!(tool_messages(&bodies[1])[0][expected], "exact raw");
    }
}
