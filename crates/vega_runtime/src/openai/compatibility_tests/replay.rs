use super::*;
use crate::{
    FrozenReasoning, ReasoningChoice, ReasoningProtocol, ReasoningReplayPolicy, ReplayFrame,
};

#[test]
fn non_live_wire_ids_are_stable_unique_and_results_match_without_mutating_source_identity() {
    let ids = [
        format!("call|{}A", "same".repeat(18)),
        format!("call|{}B", "same".repeat(18)),
        format!("call/{}A", "same".repeat(18)),
    ];
    let assistant = ChatMessage::assistant_with_tools(
        "",
        ids.iter()
            .map(|id| crate::ChatToolCall {
                id: id.clone(),
                name: "read".into(),
                input_json: "{}".into(),
            })
            .collect(),
    );
    let mut messages = vec![assistant.clone()];
    messages.extend(ids.iter().map(|id| ChatMessage::tool_result(id, "result")));
    for endpoint in ["https://api.openai.com/v1", "https://proxy.example/v1"] {
        let projected =
            crate::project_model_messages(&messages).expect("normal history projection");
        let request = ChatRequest {
            model: "unknown".into(),
            messages: projected,
            tools: vec![],
            max_tokens: None,
            reasoning: None,
        };
        let provider = OpenAiProvider::new(endpoint, "key").expect("transport");
        let body = provider.request_body(&request).expect("wire body");
        assert_eq!(body, provider.request_body(&request).expect("repeat body"));
        let calls = body["messages"][0]["tool_calls"].as_array().expect("calls");
        let wire_ids = calls
            .iter()
            .map(|call| call["id"].as_str().expect("wire id"))
            .collect::<Vec<_>>();
        assert_ne!(wire_ids[0], wire_ids[1]);
        for (index, wire_id) in wire_ids.iter().take(2).enumerate() {
            assert!(wire_id.len() <= 40);
            assert!(
                wire_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            );
            assert_eq!(body["messages"][index + 1]["tool_call_id"], *wire_id);
        }
        if endpoint == "https://api.openai.com/v1" {
            assert!(wire_ids[2].len() <= 40);
        } else {
            assert_eq!(wire_ids[2], ids[2]);
        }
        assert_eq!(request.messages[0].tool_calls, assistant.tool_calls);
    }
    let ids = ["call|replacement", "call_replacement"];
    let assistant = ChatMessage::assistant_with_tools(
        "",
        ids.iter()
            .map(|id| crate::ChatToolCall {
                id: (*id).into(),
                name: "read".into(),
                input_json: "{}".into(),
            })
            .collect(),
    );
    let mut messages = vec![assistant];
    messages.extend(ids.iter().map(|id| ChatMessage::tool_result(*id, "result")));
    let request = ChatRequest {
        model: "unknown".into(),
        messages: crate::project_model_messages(&messages)
            .expect("replacement collision projection"),
        tools: vec![],
        max_tokens: None,
        reasoning: None,
    };
    let provider = OpenAiProvider::new("https://proxy.example/v1", "key").expect("transport");
    let body = provider
        .request_body(&request)
        .expect("collision-safe wire");
    assert_ne!(
        body["messages"][0]["tool_calls"][0]["id"],
        body["messages"][0]["tool_calls"][1]["id"]
    );
    for index in 0..2 {
        assert_eq!(
            body["messages"][0]["tool_calls"][index]["id"],
            body["messages"][index + 1]["tool_call_id"]
        );
    }
}

#[tokio::test]
async fn same_route_live_ids_are_not_rewritten_and_cold_projection_never_revives_calls() {
    let id = format!("live|{}", "identity".repeat(12));
    let (_root, tools) = fixture();
    let provider = ScriptedProvider::new(
        "https://api.openai.com/v1",
        vec![
            Script::read(&id, json!({"reasoning_text":"live state"})),
            Script::final_reply(json!({})),
        ],
    );
    let mut initial = request("unknown");
    let mut frame = ReplayFrame::new(2, ReasoningReplayPolicy::Unknown);
    frame.attach_route_fingerprint([7; 32]);
    initial.replay_frame = Some(frame);
    let outcome = crate::run_agent(&provider, &tools, initial, CancellationToken::new())
        .await
        .expect("live outcome");
    assert!(!outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 1);
    let bodies = provider.bodies();
    assert_eq!(tool_messages(&bodies[1])[0]["tool_calls"][0]["id"], id);
    assert!(outcome.events.iter().any(
        |event| matches!(event, RuntimeEvent::ToolCallFinished(result) if result.call_id == id)
    ));
    let cold_provider = ScriptedProvider::new(
        "https://api.openai.com/v1",
        vec![Script::final_reply(json!({}))],
    );
    let mut cold = request("unknown");
    cold.history = outcome.messages[1..]
        .iter()
        .map(ChatMessage::without_runtime_metadata)
        .collect();
    cold.history
        .push(ChatMessage::new(ChatRole::User, "continue"));
    cold.replay_frame = Some(ReplayFrame::new(
        cold.history.len() + 1,
        ReasoningReplayPolicy::Unknown,
    ));
    let continued = crate::run_agent(&cold_provider, &tools, cold, CancellationToken::new())
        .await
        .expect("cold outcome");
    assert!(!continued.failed);
    assert_eq!(continued.executed_tool_call_count, 0);
    let body = &cold_provider.bodies()[0];
    assert!(tool_messages(body).is_empty());
    assert!(
        body["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .all(|message| message.get("reasoning_text").is_none()
                && message.get("reasoning_details").is_none())
    );
    assert!(body.to_string().contains("vega_history_observation"));
}

#[tokio::test]
async fn new_text_and_detail_state_survives_warm_and_is_rejected_when_environment_changes() {
    for state in [
        json!({"reasoning_text":"warm raw unique"}),
        json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"e","data":"warm opaque unique"}]}),
    ] {
        let (_root, tools) = fixture();
        let provider = ScriptedProvider::new(
            "https://proxy.example/v1",
            vec![
                Script::read("warm", state.clone()),
                Script::final_reply(state),
            ],
        );
        let mut initial = request("model-one");
        let mut frame = ReplayFrame::new(2, ReasoningReplayPolicy::Unknown);
        frame.attach_route_fingerprint([9; 32]);
        initial.replay_frame = Some(frame);
        let outcome = crate::run_agent(&provider, &tools, initial, CancellationToken::new())
            .await
            .expect("initial warm");
        assert!(!outcome.failed);
        let frame = outcome.messages[0].replay_frame().expect("frame");
        assert!(frame.seal_eligible());
        let environment = frame.environment_fingerprint().expect("binding");
        for change in ["none", "route", "model", "api", "tool-policy", "choice"] {
            let mut next = request(if change == "model" {
                "model-two"
            } else {
                "model-one"
            });
            next.history = outcome.messages[1..]
                .iter()
                .map(ChatMessage::without_runtime_metadata)
                .collect();
            next.history
                .push(ChatMessage::new(ChatRole::User, "continue"));
            if change == "tool-policy" {
                next.tool_config = RuntimeToolConfig::default().with_turn_limit(7);
            }
            if change == "api" {
                let mut frozen = FrozenReasoning::unknown("proxy", "model-one");
                frozen.protocol = ReasoningProtocol::ResponsesApi;
                frozen.replay_policy = ReasoningReplayPolicy::ResponsesItems;
                next.reasoning = Some(frozen);
            }
            if change == "choice" {
                let mut frozen = FrozenReasoning::unknown("proxy", "model-one");
                frozen.protocol = ReasoningProtocol::OpenAiChatCompletions;
                frozen.replay_policy = ReasoningReplayPolicy::StandardChat;
                frozen.declared_efforts = vec!["high".into()];
                frozen.choice = ReasoningChoice::Effort("high".into());
                next.reasoning = Some(frozen);
            }
            let mut frame =
                ReplayFrame::new(next.history.len() + 1, ReasoningReplayPolicy::Unknown);
            frame.attach_route_fingerprint(if change == "route" { [8; 32] } else { [9; 32] });
            frame.attach_candidate(outcome.messages.clone(), environment);
            next.replay_frame = Some(frame);
            let provider = ScriptedProvider::new(
                "https://proxy.example/v1",
                vec![Script::final_reply(json!({}))],
            );
            let continued = crate::run_agent(&provider, &tools, next, CancellationToken::new())
                .await
                .expect("continued");
            assert!(!continued.failed, "{change}");
            assert_eq!(continued.executed_tool_call_count, 0);
            let body = provider.bodies()[0].to_string();
            assert_eq!(
                body.contains("warm raw unique") || body.contains("warm opaque unique"),
                change == "none",
                "{change}"
            );
        }
    }
}

#[tokio::test]
async fn opaque_bound_call_identity_cannot_be_silently_normalized_after_losing_live_binding() {
    let (_root, tools) = fixture();
    let provider = ScriptedProvider::new(
        "https://proxy.example/v1",
        vec![
            Script::read(
                "opaque|long-call",
                json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"opaque","data":"bound opaque"}]}),
            ),
            Script::final_reply(json!({})),
        ],
    );
    let outcome = crate::run_agent(
        &provider,
        &tools,
        request("unknown"),
        CancellationToken::new(),
    )
    .await
    .expect("opaque run");
    assert!(!outcome.failed);
    let mut request = provider.requests()[1].clone();
    for message in &mut request.messages {
        *message = message.without_live_tool_arguments();
    }
    request.messages = crate::project_model_messages(&request.messages)
        .expect("ordinary non-live wire projection");
    let body = OpenAiProvider::new("https://api.openai.com/v1", "key")
        .expect("transport")
        .request_body(&request);
    assert!(body.is_err());
}

fn fragmented_read(id: &str, field: &str, size: usize) -> Script {
    let mut script = Script::read(id, json!({}));
    let chunks = std::iter::repeat_n("x".repeat(32 * 1024), size / (32 * 1024))
        .map(|piece| json!({"choices":[{"delta":{field:piece}}]}))
        .collect::<Vec<_>>();
    script.chunks.splice(1..1, chunks);
    script
}

#[tokio::test]
async fn new_raw_channels_obey_turn_run_and_cancellation_limits_before_new_tool_execution() {
    for field in ["reasoning", "reasoning_text"] {
        let (outcome, _) = run_scripts(
            "https://proxy.example/v1",
            "unknown",
            vec![
                fragmented_read("unsafe", field, 288 * 1024),
                Script::final_reply(json!({})),
            ],
        )
        .await;
        assert_failed_without_tools(&outcome);
        let scripts = (0..5)
            .map(|index| fragmented_read(&format!("read-{index}"), field, 224 * 1024))
            .chain(std::iter::once(Script::final_reply(json!({}))))
            .collect();
        let (outcome, _) = run_scripts("https://proxy.example/v1", "unknown", scripts).await;
        assert!(outcome.failed);
        assert_eq!(outcome.executed_tool_call_count, 4);
    }
    let (outcome, _) = run_scripts("https://proxy.example/v1", "unknown", vec![Script::read("unsafe", json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"e","data":"x".repeat(crate::REASONING_TURN_MAX_BYTES+1)}]})), Script::final_reply(json!({}))]).await;
    assert_failed_without_tools(&outcome);
    let scripts=(0..5).map(|turn| {
        let mut script=Script::read(&format!("details-{turn}"),json!({}));
        script.chunks.splice(1..1,(0..7).map(|index|json!({"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.encrypted","id":format!("e-{index}"),"index":index,"data":"x".repeat(32*1024)}]}}]})));
        script
    }).chain(std::iter::once(Script::final_reply(json!({})))).collect();
    let (outcome, _) = run_scripts("https://proxy.example/v1", "unknown", scripts).await;
    assert!(outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 4);
    let mut script = Script::read(
        "cancelled",
        json!({"reasoning_text":"partial cancelled raw"}),
    );
    script.cancel = true;
    let (outcome, _) = run_scripts("https://proxy.example/v1", "unknown", vec![script]).await;
    assert!(outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 0);
}

#[tokio::test]
async fn new_raw_state_is_counted_by_real_context_and_resident_estimates_and_debug_is_redacted() {
    for state in [
        json!({"reasoning_text":"raw sentinel ".repeat(1200)}),
        json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"e","data":"opaque sentinel ".repeat(1200),"signature":"signature sentinel"}]}),
    ] {
        let (outcome, _) = run_scripts(
            "https://proxy.example/v1",
            "unknown",
            vec![Script::read("one", state), Script::final_reply(json!({}))],
        )
        .await;
        let (plain, _) = run_scripts(
            "https://proxy.example/v1",
            "unknown",
            vec![
                Script::read("one", json!({})),
                Script::final_reply(json!({})),
            ],
        )
        .await;
        assert!(!outcome.failed);
        let raw_estimate =
            crate::estimate_chat_context("", &outcome.messages, &[]).expect("raw estimate");
        let plain_estimate =
            crate::estimate_chat_context("", &plain.messages, &[]).expect("plain estimate");
        assert!(raw_estimate.input_tokens > plain_estimate.input_tokens + 1000);
        let raw_size: usize = outcome
            .messages
            .iter()
            .map(ChatMessage::replay_resident_size_bytes)
            .sum();
        let plain_size: usize = plain
            .messages
            .iter()
            .map(ChatMessage::replay_resident_size_bytes)
            .sum();
        assert!(raw_size > plain_size + 10_000);
        let debug = format!("{outcome:?} {:?} {:?}", outcome.messages, outcome.events);
        for sentinel in ["raw sentinel", "opaque sentinel", "signature sentinel"] {
            assert!(!debug.contains(sentinel));
        }
        assert_eq!(outcome.final_text, "complete");
    }
}

#[tokio::test]
async fn strict_known_and_foreign_responses_state_remain_rejected_before_tools() {
    for (protocol, policy, model) in [
        (
            ReasoningProtocol::DeepSeekChatCompletions,
            ReasoningReplayPolicy::DeepSeekTools,
            "deepseek-v4-pro",
        ),
        (
            ReasoningProtocol::ZhipuChatCompletions,
            ReasoningReplayPolicy::GlmPreserved,
            "glm-5.3",
        ),
    ] {
        let (_root, tools) = fixture();
        let provider = ScriptedProvider::new(
            "https://proxy.example/v1",
            vec![
                Script::read("unsafe", json!({})),
                Script::final_reply(json!({})),
            ],
        );
        let mut request = request(model);
        let mut reasoning = FrozenReasoning::unknown("official", model);
        reasoning.protocol = protocol;
        reasoning.replay_policy = policy;
        request.reasoning = Some(reasoning);
        let outcome = crate::run_agent(&provider, &tools, request, CancellationToken::new())
            .await
            .expect("strict outcome");
        assert_failed_without_tools(&outcome);
    }
    for layout in [false, true] {
        let mut script = Script::read("unsafe", json!({"reasoning":"chat source"}));
        script.extra = if layout {
            vec![ProviderEvent::ResponseReplay(crate::ResponseReplayLayout {
                response_id: Some("foreign".into()),
                slots: vec![],
            })]
        } else {
            vec![ProviderEvent::ReasoningReplay(vec![
                json!({"type":"reasoning","encrypted_content":"foreign opaque"}),
            ])]
        };
        let (outcome, _) = run_scripts(
            "https://proxy.example/v1",
            "unknown",
            vec![script, Script::final_reply(json!({}))],
        )
        .await;
        assert_failed_without_tools(&outcome);
    }
}
