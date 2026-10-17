use super::*;
use crate::provider::{
    ChatMessage, ChatRole, ChatToolCall, FrozenReasoning, ReasoningChoice, ReasoningDisabledWire,
    ReasoningProtocol, ReasoningReplayPolicy, ToolDefinition,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

mod computer_use;
mod network_retry;

const KEY: &str = "vrg-test-key-123";
const MODEL: &str = "test-model";

#[test]
fn follow_up_messages_serialize_exact_tool_call_wire_shape() {
    let request = ChatRequest {
        model: MODEL.into(),
        messages: vec![
            ChatMessage::assistant_with_tools(
                "checking",
                vec![ChatToolCall {
                    id: "call-7".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"src/lib.rs"}"#.into(),
                }],
            ),
            ChatMessage::tool_result("call-7", "1 | fn main() {}"),
        ],
        ..Default::default()
    };

    let wire = build_request_body(&request);
    assert_eq!(
        wire["messages"][0],
        serde_json::json!({
            "role": "assistant",
            "content": "checking",
            "tool_calls": [{
                "id": "call-7",
                "type": "function",
                "function": {
                    "name": "read",
                    "arguments": "{\"path\":\"src/lib.rs\"}"
                }
            }]
        })
    );
    assert_eq!(
        wire["messages"][1],
        serde_json::json!({
            "role": "tool",
            "content": "1 | fn main() {}",
            "tool_call_id": "call-7"
        })
    );
}

#[test]
fn historical_observations_use_user_json_in_both_openai_wires() {
    let mut stored = ChatMessage::assistant_with_tools(
        "checked before restart",
        vec![ChatToolCall {
            id: "old-read".into(),
            name: "read".into(),
            input_json: r#"{"path":"PRIVATE_PATH"}"#.into(),
        }],
    );
    stored
        .mark_stored_tool_calls_with_statuses(vec!["rejected".into()])
        .unwrap();
    let request = ChatRequest {
        model: MODEL.into(),
        messages: vec![
            stored,
            ChatMessage::tool_result("old-read", "historical output"),
            ChatMessage::assistant_with_tools(
                "",
                vec![ChatToolCall {
                    id: "live-read".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"current.txt"}"#.into(),
                }],
            ),
            ChatMessage::tool_result("live-read", "live output"),
        ],
        ..Default::default()
    };
    let mut projected_request = request.clone();
    projected_request.messages = crate::project_model_messages(&request.messages).unwrap();
    let chat = build_request_body(&projected_request);
    assert_eq!(chat["messages"][0]["role"], "assistant");
    assert_eq!(chat["messages"][0]["content"], "checked before restart");
    assert!(chat["messages"][0].get("tool_calls").is_none());
    assert_eq!(chat["messages"][1]["role"], "user");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(chat["messages"][1]["content"].as_str().unwrap())
            .unwrap()["vega_history_observation"]["kind"],
        "tool_activity"
    );
    assert_eq!(chat["messages"][2]["role"], "user");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(chat["messages"][2]["content"].as_str().unwrap())
            .unwrap()["vega_history_observation"]["output"],
        "historical output"
    );
    assert_eq!(
        chat["messages"][3]["tool_calls"][0]["function"]["arguments"],
        r#"{"path":"current.txt"}"#
    );
    assert_eq!(chat["messages"][4]["tool_call_id"], "live-read");

    let responses = responses::build_body(&projected_request).unwrap();
    let input = responses["input"].as_array().unwrap();
    assert_eq!(input[0]["role"], "assistant");
    assert_eq!(input[0]["content"][0]["type"], "output_text");
    assert_eq!(input[1]["role"], "user");
    assert_eq!(input[1]["content"][0]["type"], "input_text");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(input[1]["content"][0]["text"].as_str().unwrap())
            .unwrap()["vega_history_observation"]["kind"],
        "tool_activity"
    );
    assert_eq!(input[2]["role"], "user");
    assert_eq!(input[2]["content"][0]["type"], "input_text");
    assert_eq!(input[3]["type"], "function_call");
    assert_eq!(input[3]["call_id"], "live-read");
    assert_eq!(input[4]["type"], "function_call_output");
    assert_eq!(input[4]["output"], "live output");
    assert!(!chat.to_string().contains("PRIVATE_PATH"));
    assert!(!responses.to_string().contains("PRIVATE_PATH"));
}

#[test]
fn automatic_replay_cold_frame_is_idempotent_across_chat_and_responses_serialization() {
    let build_messages = |policy| {
        let mut system = ChatMessage::new(ChatRole::System, "system");
        system.attach_replay_frame(crate::ReplayFrame::new(5, policy));
        let mut reasoning_only = ChatMessage::new(ChatRole::Assistant, "");
        reasoning_only.reasoning_content = Some("PRIVATE_CHAT_REASONING".into());
        let mut opaque_only = ChatMessage::new(ChatRole::Assistant, "");
        opaque_only
            .response_reasoning
            .push(serde_json::json!({"type":"reasoning","encrypted_content":"PRIVATE_OPAQUE"}));
        vec![
            system,
            reasoning_only,
            opaque_only,
            ChatMessage::new(ChatRole::Assistant, "prior visible answer"),
            ChatMessage::new(ChatRole::User, "current request"),
        ]
    };
    let chat_messages = {
        let mut messages = build_messages(crate::ReasoningReplayPolicy::DeepSeekTools);
        let mut live = ChatMessage::new(ChatRole::Assistant, "live final");
        live.reasoning_content = Some("LIVE_REASONING".into());
        messages.push(live);
        messages
    };
    let chat_projected = crate::project_model_messages(&chat_messages).unwrap();
    assert_eq!(
        chat_projected[0].replay_frame().unwrap().cold_prefix_len(),
        3
    );
    assert_eq!(
        crate::project_model_messages(&chat_projected).unwrap(),
        chat_projected
    );
    let chat_request = ChatRequest {
        model: MODEL.into(),
        messages: chat_projected.clone(),
        ..Default::default()
    };
    let chat = build_request_body(&chat_request);
    assert_eq!(chat["messages"][1]["role"], "user");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(chat["messages"][1]["content"].as_str().unwrap())
            .unwrap()["vega_history_observation"]["text"],
        "prior visible answer"
    );
    assert_eq!(chat["messages"][2]["content"], "current request");
    assert_eq!(chat["messages"][3]["reasoning_content"], "LIVE_REASONING");
    assert!(!chat.to_string().contains("PRIVATE_CHAT_REASONING"));
    assert!(!chat.to_string().contains("PRIVATE_OPAQUE"));
    let chat_wire_estimate = crate::estimate_wire_context(&chat_projected, &[]).unwrap();
    assert_eq!(
        crate::estimate_chat_context("system", &chat_projected[1..], &[]).unwrap(),
        chat_wire_estimate
    );

    let response_messages = {
        let mut messages = build_messages(crate::ReasoningReplayPolicy::ResponsesItems);
        let reasoning_item = serde_json::json!({
            "type":"reasoning",
            "id":"rs_1",
            "phase":"analysis",
            "summary":[],
            "encrypted_content":"LIVE_OPAQUE"
        });
        let message_item = serde_json::json!({
            "type":"message",
            "id":"msg_1",
            "role":"assistant",
            "status":"completed",
            "phase":"final",
            "content":[{"type":"output_text","text":"live final"}]
        });
        let mut live = ChatMessage::new(ChatRole::Assistant, "live final");
        live.response_reasoning = vec![reasoning_item.clone()];
        live.response_replay_layout = Some(crate::ResponseReplayLayout {
            response_id: Some("resp_1".into()),
            slots: vec![
                crate::ResponseReplaySlot::Reasoning(reasoning_item),
                crate::ResponseReplaySlot::Message(message_item),
            ],
        });
        messages.push(live);
        messages
    };
    let response_projected = crate::project_model_messages(&response_messages).unwrap();
    assert_eq!(
        response_projected[0]
            .replay_frame()
            .unwrap()
            .cold_prefix_len(),
        3
    );
    assert_eq!(
        crate::project_model_messages(&response_projected).unwrap(),
        response_projected
    );
    let response_request = ChatRequest {
        model: MODEL.into(),
        messages: response_projected.clone(),
        reasoning: Some(FrozenReasoning {
            provider: "openai".into(),
            model: MODEL.into(),
            protocol: ReasoningProtocol::ResponsesApi,
            replay_policy: ReasoningReplayPolicy::ResponsesItems,
            choice: ReasoningChoice::ProviderDefault,
            supports_disabled: false,
            disabled_wire: None,
            declared_efforts: Vec::new(),
        }),
        ..Default::default()
    };
    let responses = responses::build_body(&response_request).unwrap();
    let input = responses["input"].as_array().unwrap();
    assert_eq!(input[1]["role"], "user");
    assert_eq!(input[2]["content"][0]["text"], "current request");
    assert_eq!(input[3]["phase"], "analysis");
    assert_eq!(input[4]["phase"], "final");
    assert_eq!(input[3]["encrypted_content"], "LIVE_OPAQUE");
    assert!(!responses.to_string().contains("PRIVATE_CHAT_REASONING"));
    assert!(!responses.to_string().contains("PRIVATE_OPAQUE"));
    let response_wire_estimate = crate::estimate_wire_context(&response_projected, &[]).unwrap();
    assert_eq!(
        crate::estimate_chat_context("system", &response_projected[1..], &[]).unwrap(),
        response_wire_estimate
    );
}

#[test]
fn issue85_vega_tools_emit_strict_chat_completions_wire() {
    let request = ChatRequest {
        model: MODEL.into(),
        tools: crate::tool_definitions(crate::RuntimeRunMode::Execute),
        ..Default::default()
    };

    let wire = build_request_body(&request);
    let functions = wire["tools"].as_array().unwrap();
    assert_eq!(functions.len(), 6);
    assert!(
        functions
            .iter()
            .all(|tool| tool["function"]["strict"] == true)
    );
}

#[test]
fn issue85_non_strict_tools_do_not_claim_strict_wire() {
    let request = ChatRequest {
        model: MODEL.into(),
        tools: vec![ToolDefinition {
            name: "external".into(),
            description: "external tool".into(),
            input_schema: serde_json::json!({"type": "object"}),
            strict: false,
        }],
        ..Default::default()
    };

    let wire = build_request_body(&request);
    assert!(wire["tools"][0]["function"].get("strict").is_none());
}

#[test]
fn glm_preserved_effort_keeps_the_cross_round_clear_switch() {
    let request = ChatRequest {
        model: "glm-5.3".into(),
        reasoning: Some(FrozenReasoning {
            provider: "zhipu".into(),
            model: "glm-5.3".into(),
            protocol: ReasoningProtocol::ZhipuChatCompletions,
            replay_policy: ReasoningReplayPolicy::GlmPreserved,
            choice: ReasoningChoice::Effort("max".into()),
            supports_disabled: false,
            disabled_wire: None,
            declared_efforts: vec!["low".into(), "high".into(), "max".into()],
        }),
        ..Default::default()
    };
    let wire = build_request_body(&request);
    assert_eq!(
        wire["thinking"],
        serde_json::json!({"type": "enabled", "clear_thinking": false})
    );
    assert_eq!(wire["reasoning_effort"], "max");
    assert_eq!(wire["thinking"]["clear_thinking"], false);
}

#[test]
fn glm_preserved_controls_keep_clear_switch_and_interleaved_omits_it() {
    let profiles = [
        FrozenReasoning {
            provider: "zhipu".into(),
            model: "glm-4.7".into(),
            protocol: ReasoningProtocol::ZhipuChatCompletions,
            replay_policy: ReasoningReplayPolicy::GlmPreserved,
            choice: ReasoningChoice::ProviderDefault,
            supports_disabled: true,
            disabled_wire: Some(ReasoningDisabledWire::ThinkingTypeDisabled),
            declared_efforts: vec!["low".into(), "high".into(), "max".into()],
        },
        FrozenReasoning {
            provider: "zhipu".into(),
            model: "glm-4.7".into(),
            protocol: ReasoningProtocol::ZhipuChatCompletions,
            replay_policy: ReasoningReplayPolicy::GlmPreserved,
            choice: ReasoningChoice::Disabled,
            supports_disabled: true,
            disabled_wire: Some(ReasoningDisabledWire::ThinkingTypeDisabled),
            declared_efforts: vec!["low".into(), "high".into(), "max".into()],
        },
        FrozenReasoning {
            provider: "zhipu".into(),
            model: "glm-4.7".into(),
            protocol: ReasoningProtocol::ZhipuChatCompletions,
            replay_policy: ReasoningReplayPolicy::GlmInterleaved,
            choice: ReasoningChoice::Effort("high".into()),
            supports_disabled: true,
            disabled_wire: Some(ReasoningDisabledWire::ThinkingTypeDisabled),
            declared_efforts: vec!["low".into(), "high".into(), "max".into()],
        },
    ];
    let bodies = profiles.map(|reasoning| {
        build_request_body(&ChatRequest {
            model: reasoning.model.clone(),
            reasoning: Some(reasoning),
            ..Default::default()
        })
    });
    assert_eq!(
        bodies[0]["thinking"],
        serde_json::json!({"clear_thinking": false})
    );
    assert_eq!(
        bodies[1]["thinking"],
        serde_json::json!({"type": "disabled", "clear_thinking": false})
    );
    assert_eq!(
        bodies[2]["thinking"],
        serde_json::json!({"type": "enabled"})
    );
}

#[test]
fn false_and_unknown_profiles_omit_reasoning_content_on_follow_up_messages() {
    for reasoning in [
        FrozenReasoning {
            provider: "openai".into(),
            model: MODEL.into(),
            protocol: ReasoningProtocol::OpenAiChatCompletions,
            replay_policy: ReasoningReplayPolicy::StandardChat,
            choice: ReasoningChoice::Effort("low".into()),
            supports_disabled: false,
            disabled_wire: None,
            declared_efforts: vec!["low".into()],
        },
        FrozenReasoning::unknown("custom", MODEL),
    ] {
        let request = ChatRequest {
            model: MODEL.into(),
            messages: vec![ChatMessage::assistant_with_tools_and_reasoning(
                "tool request",
                None,
                vec![ChatToolCall {
                    id: "call-1".into(),
                    name: "read".into(),
                    input_json: "{}".into(),
                }],
            )],
            reasoning: Some(reasoning),
            ..Default::default()
        };
        let message = &build_request_body(&request)["messages"][0];
        assert!(message.get("reasoning_content").is_none());
    }
}

#[test]
fn explicit_disabled_wire_is_only_emitted_for_declared_protocol() {
    let request = ChatRequest {
        model: MODEL.into(),
        reasoning: Some(FrozenReasoning {
            provider: "openai".into(),
            model: MODEL.into(),
            protocol: ReasoningProtocol::OpenAiChatCompletions,
            replay_policy: ReasoningReplayPolicy::StandardChat,
            choice: ReasoningChoice::Disabled,
            supports_disabled: true,
            disabled_wire: Some(ReasoningDisabledWire::ReasoningEffortNone),
            declared_efforts: vec!["low".into()],
        }),
        ..Default::default()
    };
    let wire = build_request_body(&request);
    assert_eq!(wire["reasoning_effort"], "none");
    assert!(wire.get("thinking").is_none());
}

#[test]
fn standard_glm_profile_cannot_claim_disabled_thinking() {
    let reasoning = FrozenReasoning {
        provider: "zhipu".into(),
        model: "glm-5.3-flash".into(),
        protocol: ReasoningProtocol::ZhipuChatCompletions,
        replay_policy: ReasoningReplayPolicy::GlmPreserved,
        choice: ReasoningChoice::ProviderDefault,
        supports_disabled: true,
        disabled_wire: Some(ReasoningDisabledWire::ThinkingTypeDisabled),
        declared_efforts: vec!["low".into(), "high".into(), "max".into()],
    };
    assert!(matches!(
        reasoning.validate(),
        Err(crate::VegaError::ReasoningSelectionInvalid { .. })
    ));
}

// ---------- 纯单元：SseAssembler ----------

fn absorb_all(assembler: &mut SseAssembler, chunks: &[&str]) -> Vec<ProviderEvent> {
    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(assembler.absorb(chunk).unwrap());
    }
    events
}

#[test]
fn text_deltas_with_unknown_fields_ignored() {
    let mut assembler = SseAssembler::default();
    let events = absorb_all(
        &mut assembler,
        &[
            r#"{"id":"c1","object":"chat.completion.chunk","created":1,"model":"m","system_fingerprint":"fp","choices":[{"index":0,"delta":{"content":"Hel"},"logprobs":null,"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{"content":"lo"},"service_tier":"default"}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
    );
    assert_eq!(
        events,
        vec![
            ProviderEvent::TextDelta("Hel".into()),
            ProviderEvent::TextDelta("lo".into()),
        ]
    );
    assert_eq!(
        assembler.finalize().expect("explicit finish reason"),
        vec![
            ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::Absent),
            ProviderEvent::Done {
                stop_reason: StopReason::End
            }
        ]
    );
}

#[test]
fn reasoning_content_becomes_thinking_delta() {
    let mut assembler = SseAssembler::default();
    let events = absorb_all(
        &mut assembler,
        &[r#"{"choices":[{"delta":{"reasoning_content":"pondering"}}]}"#],
    );
    assert_eq!(
        events,
        vec![ProviderEvent::ThinkingDelta("pondering".into())]
    );
}

#[test]
fn chat_reasoning_raw_presence_preserves_absent_empty_and_nonempty_without_debug_leaks() {
    for (chunks, expected) in [
        (
            vec![r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#],
            crate::ChatReasoningPresence::Absent,
        ),
        (
            vec![r#"{"choices":[{"delta":{"reasoning_content":""},"finish_reason":"stop"}]}"#],
            crate::ChatReasoningPresence::Empty,
        ),
        (
            vec![
                r#"{"choices":[{"delta":{"reasoning_content":"PRIVATE_"}}]}"#,
                r#"{"choices":[{"delta":{"reasoning_content":"REPLAY"},"finish_reason":"tool_calls"}]}"#,
            ],
            crate::ChatReasoningPresence::NonEmpty("PRIVATE_REPLAY".into()),
        ),
    ] {
        let mut assembler = SseAssembler::default();
        for chunk in chunks {
            assembler.absorb(chunk).unwrap();
        }
        let events = assembler.finalize().unwrap();
        assert!(
            matches!(events.first(), Some(ProviderEvent::ChatReasoningContent(actual)) if *actual == expected)
        );
        assert!(matches!(events.last(), Some(ProviderEvent::Done { .. })));
        assert!(!format!("{events:?}").contains("PRIVATE_REPLAY"));
    }
}

#[test]
fn automatic_replay_completed_binding_loss_becomes_observation_on_both_wires() {
    let image = || {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        crate::ImageAttachment::from_bytes(bytes.into_inner()).unwrap()
    };
    let calls = [
        ("observe-1", "ComputerObserve", r#"{"action":"observe"}"#),
        ("observe-2", "ComputerObserve", r#"{"action":"observe"}"#),
    ];
    let tool_calls = calls
        .iter()
        .map(|(id, name, _)| ChatToolCall {
            id: (*id).into(),
            name: (*name).into(),
            input_json: crate::computer_use::ComputerUseAudit::for_tool(name, "{}"),
        })
        .collect::<Vec<_>>();
    let mut assistant = ChatMessage::assistant_with_tools_and_reasoning(
        "I inspected the current screen.",
        Some("PRIVATE_CHAT_REASONING".into()),
        tool_calls,
    );
    let response_reasoning = serde_json::json!({
        "type":"reasoning",
        "id":"reasoning-1",
        "phase":"analysis",
        "encrypted_content":"PRIVATE_RESPONSE_REASONING"
    });
    assistant.response_reasoning = vec![response_reasoning.clone()];
    assistant.response_replay_layout = Some(crate::ResponseReplayLayout {
        response_id: Some("response-1".into()),
        slots: vec![
            crate::ResponseReplaySlot::Reasoning(response_reasoning),
            crate::ResponseReplaySlot::Message(serde_json::json!({
                "type":"message",
                "id":"message-1",
                "role":"assistant",
                "status":"completed",
                "phase":"commentary",
                "content":[{"type":"output_text","text":"I inspected the current screen."}]
            })),
            crate::ResponseReplaySlot::FunctionCall {
                item: serde_json::json!({
                    "type":"function_call",
                    "call_id":"observe-1",
                    "name":"ComputerObserve",
                    "arguments":"{\"action\":\"observe\"}",
                    "phase":"commentary"
                }),
                call_id: "observe-1".into(),
                name: "ComputerObserve".into(),
                arguments: calls[0].2.into(),
            },
            crate::ResponseReplaySlot::FunctionCall {
                item: serde_json::json!({
                    "type":"function_call",
                    "call_id":"observe-2",
                    "name":"ComputerObserve",
                    "arguments":"{\"action\":\"observe\"}",
                    "phase":"commentary"
                }),
                call_id: "observe-2".into(),
                name: "ComputerObserve".into(),
                arguments: calls[1].2.into(),
            },
        ],
    });
    assistant
        .bind_live_tool_calls(
            calls
                .iter()
                .map(|(id, name, arguments)| {
                    Some(ChatToolCall {
                        id: (*id).into(),
                        name: (*name).into(),
                        input_json: (*arguments).into(),
                    })
                })
                .collect(),
        )
        .unwrap();
    assistant.discard_computer_use_arguments();
    let mut first_result = ChatMessage::tool_result("observe-1", "first screen result");
    first_result.images.push(image());
    let mut second_result = ChatMessage::tool_result("observe-2", "second screen result");
    second_result.images.push(image());
    let messages = vec![
        assistant,
        first_result,
        second_result,
        ChatMessage::new(ChatRole::User, "continue from the observation"),
    ];

    let mut chat = ChatRequest {
        model: MODEL.into(),
        messages: messages.clone(),
        ..Default::default()
    };
    chat.messages = crate::project_model_messages(&chat.messages).unwrap();
    assert_eq!(chat.messages[0].content, "I inspected the current screen.");
    assert!(chat.messages[0].tool_calls.is_empty());
    assert!(chat.messages[0].reasoning_content.is_none());
    assert!(chat.messages[0].response_reasoning.is_empty());
    assert!(chat.messages[0].response_replay_layout.is_none());
    assert_eq!(chat.messages[1].role, ChatRole::User);
    assert_eq!(chat.messages[2].images.len(), 1);
    assert_eq!(chat.messages[3].images.len(), 1);
    let chat_wire = build_request_body(&chat);
    assert_eq!(
        chat_wire["messages"][0]["content"],
        "I inspected the current screen."
    );
    assert!(chat_wire["messages"][0].get("reasoning_content").is_none());
    assert_eq!(chat_wire["messages"][1]["role"], "user");
    let activity = serde_json::from_str::<serde_json::Value>(
        chat_wire["messages"][1]["content"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(
        activity["vega_history_observation"]["calls"],
        serde_json::json!([
            {"tool":"ComputerObserve","call_id":"observe-1","arguments_available":false,"status":null},
            {"tool":"ComputerObserve","call_id":"observe-2","arguments_available":false,"status":null}
        ])
    );
    for (index, expected) in [(2, "first screen result"), (3, "second screen result")] {
        let observation = serde_json::from_str::<serde_json::Value>(
            chat_wire["messages"][index]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(observation["vega_history_observation"]["output"], expected);
        assert_eq!(
            chat_wire["messages"][index]["content"][1]["type"],
            "image_url"
        );
    }
    assert!(!chat_wire.to_string().contains("PRIVATE_CHAT_REASONING"));
    assert!(!chat_wire.to_string().contains("PRIVATE_RESPONSE_REASONING"));

    let mut responses = ChatRequest {
        model: MODEL.into(),
        messages,
        reasoning: Some(FrozenReasoning {
            provider: "custom".into(),
            model: MODEL.into(),
            protocol: ReasoningProtocol::ResponsesApi,
            replay_policy: ReasoningReplayPolicy::ResponsesItems,
            choice: ReasoningChoice::ProviderDefault,
            supports_disabled: false,
            disabled_wire: None,
            declared_efforts: Vec::new(),
        }),
        ..Default::default()
    };
    responses.messages = crate::project_model_messages(&responses.messages).unwrap();
    assert!(responses.messages[0].response_replay_layout.is_none());
    let responses_wire = responses::build_body(&responses).unwrap();
    let response_input = responses_wire["input"].as_array().unwrap();
    assert_eq!(response_input[0]["role"], "assistant");
    assert_eq!(
        response_input[0]["content"][0]["text"],
        "I inspected the current screen."
    );
    assert_eq!(response_input[1]["role"], "user");
    for (index, expected) in [(2, "first screen result"), (3, "second screen result")] {
        let observation = serde_json::from_str::<serde_json::Value>(
            response_input[index]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(observation["vega_history_observation"]["output"], expected);
        assert_eq!(response_input[index]["content"][1]["type"], "input_image");
    }
    assert!(
        !responses_wire
            .to_string()
            .contains("PRIVATE_CHAT_REASONING")
    );
    assert!(
        !responses_wire
            .to_string()
            .contains("PRIVATE_RESPONSE_REASONING")
    );
}

#[test]
fn automatic_replay_pending_binding_loss_is_rejected_before_either_serializer() {
    let calls = [
        ("pending-1", "ComputerObserve", r#"{"action":"observe"}"#),
        ("pending-2", "ComputerObserve", r#"{"action":"observe"}"#),
    ];
    let tool_calls = calls
        .iter()
        .map(|(id, name, _)| ChatToolCall {
            id: (*id).into(),
            name: (*name).into(),
            input_json: crate::computer_use::ComputerUseAudit::for_tool(name, "{}"),
        })
        .collect::<Vec<_>>();
    let mut assistant = ChatMessage::assistant_with_tools_and_reasoning(
        "pending observation",
        Some("PRIVATE_PENDING_REASONING".into()),
        tool_calls,
    );
    assistant.response_replay_layout = Some(crate::ResponseReplayLayout {
        response_id: Some("pending-response".into()),
        slots: Vec::new(),
    });
    assistant.bind_live_tool_calls(vec![None, None]).unwrap();
    let messages = vec![
        assistant,
        ChatMessage::tool_result("pending-1", "only one result"),
    ];
    let invalid = |messages: &[ChatMessage]| {
        matches!(
            crate::project_model_messages(messages),
            Err(VegaError::Context(
                crate::ContextRuntimeError::InvalidProjection
            ))
        )
    };
    assert!(invalid(&messages));
    let chat = ChatRequest {
        model: MODEL.into(),
        messages: messages.clone(),
        ..Default::default()
    };
    assert!(invalid(&chat.messages));
    let responses = ChatRequest {
        model: MODEL.into(),
        messages,
        reasoning: Some(FrozenReasoning {
            provider: "custom".into(),
            model: MODEL.into(),
            protocol: ReasoningProtocol::ResponsesApi,
            replay_policy: ReasoningReplayPolicy::ResponsesItems,
            choice: ReasoningChoice::ProviderDefault,
            supports_disabled: false,
            disabled_wire: None,
            declared_efforts: Vec::new(),
        }),
        ..Default::default()
    };
    assert!(invalid(&responses.messages));
}

#[test]
fn responses_replay_layout_keeps_reasoning_message_and_calls_in_wire_order() {
    let mut parser = responses::Assembler::default();
    let output = vec![
        serde_json::json!({"id":"reasoning-1","type":"reasoning","encrypted_content":"opaque-one","phase":"analysis"}),
        serde_json::json!({"id":"message-1","type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"checking"}]}),
        serde_json::json!({"id":"call-item","type":"function_call","call_id":"call-1","name":"read","arguments":"{\"path\":\"README.md\"}","phase":"commentary"}),
        serde_json::json!({"id":"reasoning-2","type":"reasoning","encrypted_content":"opaque-two","phase":"analysis"}),
    ];
    let response = serde_json::json!({
        "type":"response.completed",
        "response":{"id":"resp-1","status":"completed","output":output}
    });
    let events = parser.absorb(&response.to_string()).unwrap();
    let layout = events
        .iter()
        .find_map(|event| match event {
            ProviderEvent::ResponseReplay(layout) => Some(layout.clone()),
            _ => None,
        })
        .expect("validated response layout");
    assert_eq!(layout.response_id.as_deref(), Some("resp-1"));
    assert_eq!(layout.slots.len(), 4);
    assert!(
        matches!(&layout.slots[0], crate::ResponseReplaySlot::Reasoning(item) if item["phase"] == "analysis")
    );
    assert!(
        matches!(&layout.slots[1], crate::ResponseReplaySlot::Message(item) if item["phase"] == "commentary")
    );
    assert!(
        matches!(&layout.slots[2], crate::ResponseReplaySlot::FunctionCall { call_id, name, arguments, item } if call_id == "call-1" && name == "read" && arguments == r#"{"path":"README.md"}"# && item["phase"] == "commentary")
    );
    assert!(
        matches!(&layout.slots[3], crate::ResponseReplaySlot::Reasoning(item) if item["encrypted_content"] == "opaque-two")
    );
    assert!(!format!("{layout:?}").contains("opaque-one"));

    let mut request = ChatRequest {
        reasoning: Some(FrozenReasoning {
            provider: "custom".into(),
            model: MODEL.into(),
            protocol: ReasoningProtocol::ResponsesApi,
            replay_policy: crate::ReasoningReplayPolicy::ResponsesItems,
            choice: ReasoningChoice::ProviderDefault,
            supports_disabled: false,
            disabled_wire: None,
            declared_efforts: Vec::new(),
        }),
        ..Default::default()
    };
    let mut message = ChatMessage::assistant_with_tools(
        "checking",
        vec![ChatToolCall {
            id: "call-1".into(),
            name: "read".into(),
            input_json: r#"{"path":"README.md"}"#.into(),
        }],
    );
    message.response_reasoning = layout
        .slots
        .iter()
        .filter_map(|slot| match slot {
            crate::ResponseReplaySlot::Reasoning(item) => Some(item.clone()),
            _ => None,
        })
        .collect();
    message.response_replay_layout = Some(layout);
    message
        .bind_live_tool_calls(vec![Some(message.tool_calls[0].clone())])
        .unwrap();
    request.messages.push(message);
    request
        .messages
        .push(ChatMessage::tool_result("call-1", "observed"));
    let body = responses::build_body(&request).unwrap();
    let items = body["input"].as_array().unwrap();
    assert_eq!(items[0]["type"], "reasoning");
    assert_eq!(items[0]["phase"], "analysis");
    assert_eq!(items[1]["type"], "message");
    assert_eq!(items[1]["phase"], "commentary");
    assert_eq!(items[2]["type"], "function_call");
    assert_eq!(items[2]["phase"], "commentary");
    assert_eq!(items[3]["type"], "reasoning");
    assert_eq!(items[3]["encrypted_content"], "opaque-two");
    assert_eq!(items[4]["type"], "function_call_output");
    assert_eq!(body["store"], false);
    assert_eq!(
        body["include"],
        serde_json::json!(["reasoning.encrypted_content"])
    );
}

#[test]
fn tool_call_fragments_aggregate_into_single_tool_use() {
    let mut assembler = SseAssembler::default();
    // 同一 tool_call 的 arguments 分 3 片到达
    let events = absorb_all(
        &mut assembler,
        &[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_9","type":"function","function":{"name":"read","arguments":"{\"pa"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"src"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"}"}}]}}]}"#,
        ],
    );
    assert!(events.is_empty(), "fragments must not emit partial events");
    // finish_reason 才触发完整 ToolUse
    let flushed = absorb_all(
        &mut assembler,
        &[r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#],
    );
    assert_eq!(
        flushed,
        vec![ProviderEvent::ToolUse {
            id: "call_9".into(),
            name: "read".into(),
            input_json: r#"{"path":"src"}"#.into(),
        }]
    );
    assert_eq!(
        assembler.finalize().expect("explicit finish reason"),
        vec![
            ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::Absent),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse
            }
        ]
    );
}

#[test]
fn empty_tool_identity_continuations_preserve_start_fragment() {
    let mut assembler = SseAssembler::default();
    let events = absorb_all(
        &mut assembler,
        &[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-read","function":{"name":"read","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"","function":{"name":"","arguments":"{\"path\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"","arguments":"\"README.md\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        ],
    );
    assert_eq!(
        events,
        vec![ProviderEvent::ToolUse {
            id: "call-read".into(),
            name: "read".into(),
            input_json: r#"{"path":"README.md"}"#.into(),
        }]
    );
}

#[test]
fn incomplete_tool_identity_fails_atomically_before_tool_use() {
    for incomplete in [
        r#"{"index":1,"id":"","function":{"name":"read","arguments":"{}"}}"#,
        r#"{"index":1,"id":"call-missing-name","function":{"name":"","arguments":"{}"}}"#,
    ] {
        let mut assembler = SseAssembler::default();
        let first = format!(
            "{{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call-valid\",\"function\":{{\"name\":\"read\",\"arguments\":\"{{}}\"}}}},{}]}}}}]}}",
            incomplete
        );
        assert!(assembler.absorb(&first).unwrap().is_empty());
        let error = assembler
            .absorb(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#)
            .unwrap_err();
        assert!(matches!(
            error,
            VegaError::ProviderDiagnostic {
                status: None,
                retryable: false,
                ..
            }
        ));
        assert!(assembler.tools.is_empty(), "no partial tool batch survives");
    }
}

#[test]
fn multiple_tool_calls_flush_in_index_order() {
    let mut assembler = SseAssembler::default();
    let events = absorb_all(
        &mut assembler,
        &[
            r#"{"choices":[{"delta":{"tool_calls":[
                {"index":1,"id":"b","function":{"name":"grep","arguments":"{}"}},
                {"index":0,"id":"a","function":{"name":"read","arguments":"{}"}}
            ]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        ],
    );
    let ids: Vec<&str> = events
        .iter()
        .map(|e| match e {
            ProviderEvent::ToolUse { id, .. } => id.as_str(),
            other => panic!("unexpected event {other:?}"),
        })
        .collect();
    assert_eq!(ids, vec!["a", "b"], "BTreeMap keeps index order");
}

#[test]
fn usage_chunk_maps_to_usage_event() {
    let mut assembler = SseAssembler::default();
    let events = absorb_all(
        &mut assembler,
        &[
            r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":21,"prompt_tokens_details":{"cached_tokens":80}}}"#,
        ],
    );
    assert_eq!(
        events,
        vec![ProviderEvent::Usage {
            input: 100,
            output: 21,
            cache_read: Some(80),
            cache_write: None,
        }]
    );
}

#[test]
fn usage_cache_absence_stays_distinct_from_explicit_zero() {
    let mut assembler = SseAssembler::default();
    let missing = absorb_all(
        &mut assembler,
        &[r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":21}}"#],
    );
    assert_eq!(
        missing,
        vec![ProviderEvent::Usage {
            input: 100,
            output: 21,
            cache_read: None,
            cache_write: None,
        }]
    );
    let mut assembler = SseAssembler::default();
    let explicit_zero = absorb_all(
        &mut assembler,
        &[
            r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":21,"prompt_tokens_details":{"cached_tokens":0}}}"#,
        ],
    );
    assert_eq!(
        explicit_zero,
        vec![ProviderEvent::Usage {
            input: 100,
            output: 21,
            cache_read: Some(0),
            cache_write: None,
        }]
    );
}

#[test]
fn deepseek_usage_keeps_hit_and_checks_reported_miss() {
    let mut assembler = SseAssembler::default();
    let events = absorb_all(
        &mut assembler,
        &[
            r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":21,"prompt_tokens_details":{"cached_tokens":60},"prompt_cache_hit_tokens":60,"prompt_cache_miss_tokens":40}}"#,
        ],
    );
    assert_eq!(
        events,
        vec![ProviderEvent::Usage {
            input: 100,
            output: 21,
            cache_read: Some(60),
            cache_write: None,
        }]
    );
    let mut assembler = SseAssembler::default();
    assert!(assembler
        .absorb(r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":21,"prompt_cache_hit_tokens":60,"prompt_cache_miss_tokens":39}}"#)
        .is_err());
    let mut assembler = SseAssembler::default();
    assert!(assembler
        .absorb(r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":21,"prompt_cache_hit_tokens":101}}"#)
        .is_err());
}

#[test]
fn responses_usage_preserves_missing_and_explicit_zero_cache_counts() {
    let mut parser = responses::Assembler::default();
    let missing = parser
        .absorb(
            &serde_json::json!({
                "type":"response.completed",
                "response":{
                    "status":"completed",
                    "output":[],
                    "usage":{"input_tokens":100,"output_tokens":21}
                }
            })
            .to_string(),
        )
        .unwrap();
    assert!(missing.contains(&ProviderEvent::Usage {
        input: 100,
        output: 21,
        cache_read: None,
        cache_write: None,
    }));

    let mut parser = responses::Assembler::default();
    let explicit_zero = parser
        .absorb(
            &serde_json::json!({
                "type":"response.completed",
                "response":{
                    "status":"completed",
                    "output":[],
                    "usage":{
                        "input_tokens":100,
                        "output_tokens":21,
                        "input_tokens_details":{"cached_tokens":0}
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
    assert!(explicit_zero.contains(&ProviderEvent::Usage {
        input: 100,
        output: 21,
        cache_read: Some(0),
        cache_write: None,
    }));
}

#[test]
fn responses_usage_rejects_cache_read_above_input() {
    let mut parser = responses::Assembler::default();
    assert!(
        parser
            .absorb(
                &serde_json::json!({
                    "type":"response.completed",
                    "response":{
                        "status":"completed",
                        "output":[],
                        "usage":{
                            "input_tokens":100,
                            "output_tokens":21,
                            "input_tokens_details":{"cached_tokens":101}
                        }
                    }
                })
                .to_string(),
            )
            .is_err()
    );
}

#[test]
fn missing_usage_emits_no_usage_event() {
    let mut assembler = SseAssembler::default();
    let events = absorb_all(
        &mut assembler,
        &[
            r#"{"choices":[{"delta":{"content":"hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ProviderEvent::Usage { .. }))
    );
}

#[test]
fn finish_reason_maps_to_minimal_stop_reasons() {
    for (wire, expected) in [
        ("stop", StopReason::End),
        ("tool_calls", StopReason::ToolUse),
        ("function_call", StopReason::ToolUse),
        ("length", StopReason::Length),
        ("max_tokens", StopReason::Length),
        ("content_filter", StopReason::Unknown),
        ("future_provider_reason", StopReason::Unknown),
        ("", StopReason::Unknown),
    ] {
        let mut assembler = SseAssembler::default();
        assembler
            .absorb(&format!(
                r#"{{"choices":[{{"delta":{{}},"finish_reason":"{wire}"}}]}}"#
            ))
            .unwrap();
        assert_eq!(
            assembler.finalize().expect("explicit finish reason"),
            vec![
                ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::Absent),
                ProviderEvent::Done {
                    stop_reason: expected
                }
            ]
        );
    }
}

#[test]
fn invalid_json_chunk_is_a_provider_error() {
    let mut assembler = SseAssembler::default();
    let err = assembler.absorb("not json").unwrap_err();
    assert!(matches!(
        err,
        VegaError::ProviderDiagnostic {
            retryable: false,
            ..
        }
    ));
}

#[test]
fn empty_content_delta_is_skipped() {
    let mut assembler = SseAssembler::default();
    let events = absorb_all(
        &mut assembler,
        &[r#"{"choices":[{"delta":{"content":""}}]}"#],
    );
    assert!(events.is_empty());
}

#[test]
fn openai_internal_debug_carriers_redact_distinct_sentinels() {
    let sentinels = [
        "VEGA_ENDPOINT_USERINFO_SENTINEL",
        "VEGA_ENDPOINT_QUERY_SENTINEL",
        "VEGA_KEY_SENTINEL",
        "VEGA_FRAGMENT_ID_SENTINEL",
        "VEGA_FRAGMENT_NAME_SENTINEL",
        "VEGA_FRAGMENT_ARGUMENT_SENTINEL",
    ];
    let provider = OpenAiProvider::new(
        format!(
            "http://{}@127.0.0.1/v1?query={}",
            sentinels[0], sentinels[1]
        ),
        sentinels[2],
    )
    .expect("provider");
    let fragment = ToolFragment {
        id: sentinels[3].into(),
        name: sentinels[4].into(),
        arguments: sentinels[5].into(),
    };
    let mut assembler = SseAssembler::default();
    assembler.tools.insert(0, fragment);
    let rendered = format!("{provider:?} {assembler:?}");
    for sentinel in sentinels {
        assert!(!rendered.contains(sentinel), "OpenAI Debug leaked payload");
    }
}

// ---------- 纯单元：跨 chunk SSE 切分 ----------

#[tokio::test]
async fn sse_events_reassemble_across_chunk_boundaries() {
    // 切点故意落在 JSON token / 键名 / data: 前缀 / 空行中间
    let chunks: Vec<Result<&[u8], std::io::Error>> = vec![
        Ok(b"data: {\"choi"),
        Ok(b"ces\":[{\"delta\":{\"content\":\"He"),
        Ok(b"llo\"}}]}\n"),
        Ok(b"\n"),
        Ok(b"da"),
        Ok(b"ta: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n"),
        Ok(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n"),
        Ok(b"data: [DONE]\n\ndata: {\"ignored\":true}\n\n"),
    ];
    let sse = futures::stream::iter(chunks).eventsource();
    let raw: Vec<_> = sse.collect().await;
    // 4 个完整 SSE 事件：两个内容 chunk、[DONE]、以及 [DONE] 后的 junk 事件
    assert_eq!(raw.len(), 5, "SSE events must reassemble across chunks");

    let mut assembler = SseAssembler::default();
    let mut events = Vec::new();
    for item in raw {
        let event = item.unwrap();
        if event.data.trim() == DONE_SENTINEL {
            break;
        }
        events.extend(assembler.absorb(&event.data).unwrap());
    }
    // [DONE] 后的 junk 事件被丢弃（不在 events 里，也不再 absorb）
    events.extend(assembler.finalize().expect("explicit finish reason"));
    assert_eq!(
        events,
        vec![
            ProviderEvent::TextDelta("Hello".into()),
            ProviderEvent::TextDelta(" world".into()),
            ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::Absent),
            ProviderEvent::Done {
                stop_reason: StopReason::End
            },
        ]
    );
}

type Handler = Arc<dyn Fn(u64) -> Result<reqwest::Response, reqwest::Error> + Send + Sync>;
pub(super) type TestTransport = Arc<
    dyn Fn(reqwest::Request) -> BoxFuture<'static, Result<reqwest::Response, reqwest::Error>>
        + Send
        + Sync,
>;

#[derive(Clone)]
struct CapturedRequest {
    authorization: String,
    body: serde_json::Value,
}

impl fmt::Debug for CapturedRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedRequest")
            .field("authorization_bytes", &self.authorization.len())
            .field("body", &"[redacted]")
            .finish()
    }
}

struct MockTransport {
    connections: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    transport: TestTransport,
}
impl MockTransport {
    fn attempt_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
    fn captured(&self) -> Vec<CapturedRequest> {
        self.requests.lock().unwrap().clone()
    }
}
async fn mock_transport(handler: Handler) -> MockTransport {
    let connections = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let count = connections.clone();
    let captured = requests.clone();
    let transport: TestTransport = Arc::new(move |request| {
        let index = count.fetch_add(1, Ordering::SeqCst) as u64;
        captured.lock().unwrap().push(CapturedRequest {
            authorization: request
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
            body: serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap(),
        });
        let result = handler(index);
        Box::pin(async move { result })
    });
    MockTransport {
        connections,
        requests,
        transport,
    }
}
fn fixture_response(bytes: Vec<u8>) -> reqwest::Response {
    let split = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    let head = std::str::from_utf8(&bytes[..split]).unwrap();
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut response = ::http::Response::builder().status(status);
    for line in lines {
        let (key, value) = line.split_once(':').unwrap();
        response = response.header(key, value.trim());
    }
    response.body(bytes[split + 4..].to_vec()).unwrap().into()
}
fn simulated_transport_error() -> reqwest::Error {
    reqwest::Client::new()
        .get("invalid url")
        .build()
        .unwrap_err()
}
fn http_head(status: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    head.into_bytes()
}

/// Builds a full SSE response (body terminated by connection close).
fn sse_response(events: &[&str], done: bool) -> Vec<u8> {
    let mut resp = http_head("200 OK", &[("Content-Type", "text/event-stream")]);
    for event in events {
        resp.extend_from_slice(format!("data: {event}\n\n").as_bytes());
    }
    if done {
        resp.extend_from_slice(b"data: [DONE]\n\n");
    }
    resp
}

fn status_response(status: &str, extra: &[(&str, &str)], body: &str) -> Vec<u8> {
    let mut resp = http_head(status, extra);
    resp.extend_from_slice(body.as_bytes());
    resp
}

fn scripted_responses(responses: Vec<Vec<u8>>) -> Handler {
    Arc::new(move |index| {
        responses
            .get(index as usize)
            .cloned()
            .map(fixture_response)
            .ok_or_else(simulated_transport_error)
    })
}
fn fast_policy(base_ms: u64) -> RetryPolicy {
    RetryPolicy {
        base_delay: Duration::from_millis(base_ms),
        ..RetryPolicy::default()
    }
}

fn provider_for(server: &MockTransport, policy: RetryPolicy) -> OpenAiProvider {
    let mut provider = OpenAiProvider::new("http://fixture.invalid", KEY)
        .unwrap()
        .with_retry_policy(policy);
    provider.test_transport = Some(server.transport.clone());
    provider
}

fn request() -> ChatRequest {
    ChatRequest {
        model: MODEL.to_string(),
        messages: vec![ChatMessage::new(ChatRole::User, "hello")],
        ..Default::default()
    }
}

fn usage_chunk() -> &'static str {
    r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":6}}}"#
}

async fn collect_events(
    stream: EventStream,
    limit: usize,
) -> Vec<Result<ProviderEvent, VegaError>> {
    let mut stream = stream;
    let mut out = Vec::new();
    for _ in 0..limit {
        match tokio::time::timeout(Duration::from_secs(10), stream.next()).await {
            Ok(Some(item)) => out.push(item),
            Ok(None) => break,
            Err(_) => panic!("stream stalled beyond 10s"),
        }
    }
    out
}

/// Element-wise comparison of stream items: `Ok` items via
/// `ProviderEvent: PartialEq`, `Err` items via closed typed fields without
/// formatting provider-controlled payloads.
fn assert_items_eq(
    actual: &[Result<ProviderEvent, VegaError>],
    expected: &[Result<ProviderEvent, VegaError>],
) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "item count mismatch: {actual:?} vs {expected:?}"
    );
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        match (a, e) {
            (Ok(a), Ok(e)) => assert_eq!(a, e, "item #{i}"),
            (Err(a), Err(e)) => assert_error_eq(a, e, i),
            _ => panic!("item #{i} mismatch: {a:?} vs {e:?}"),
        }
    }
}

fn assert_error_eq(actual: &VegaError, expected: &VegaError, index: usize) {
    let equal = match (actual, expected) {
        (
            VegaError::ProviderDiagnostic {
                status: actual_status,
                message: actual_message,
                retryable: actual_retryable,
                ..
            },
            VegaError::ProviderDiagnostic {
                status: expected_status,
                message: expected_message,
                retryable: expected_retryable,
                ..
            },
        ) => {
            actual_status == expected_status
                && actual_message == expected_message
                && actual_retryable == expected_retryable
        }
        (VegaError::Cancelled, VegaError::Cancelled) => true,
        (
            VegaError::Tool {
                tool: actual_tool,
                message: actual_message,
            },
            VegaError::Tool {
                tool: expected_tool,
                message: expected_message,
            },
        ) => actual_tool == expected_tool && actual_message == expected_message,
        (VegaError::Io(actual), VegaError::Io(expected)) => actual.kind() == expected.kind(),
        (VegaError::Store(actual), VegaError::Store(expected)) => {
            std::mem::discriminant(actual) == std::mem::discriminant(expected)
        }
        _ => false,
    };
    assert!(equal, "item #{index} error mismatch");
}

mod http;

#[tokio::test]
async fn i71_responses_terminal_failures_never_release_tools() {
    for terminal in [
        None,
        Some("response.failed"),
        Some("response.incomplete"),
        Some("error"),
    ] {
        let added = r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"bad","name":"read","arguments":""}}"#;
        let partial = r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"path\":"}"#;
        let ending = terminal
            .map(|kind| serde_json::json!({"type":kind,"error":{"message":KEY}}).to_string());
        let mut frames = vec![added, partial];
        if let Some(ending) = &ending {
            frames.push(ending);
        }
        let response = sse_response(&frames, false);
        let server =
            mock_transport(Arc::new(move |_| Ok(fixture_response(response.clone())))).await;
        let stream = provider_for(&server, fast_policy(1))
            .with_responses_api(true)
            .chat_stream(request(), CancellationToken::new())
            .await
            .unwrap();
        let events = collect_events(stream, 20).await;
        assert!(events.iter().any(Result::is_err));
        assert!(!events.iter().any(|event| matches!(
            event,
            Ok(ProviderEvent::ToolUse { .. } | ProviderEvent::Done { .. })
        )));
        assert!(!format!("{events:?}").contains(KEY));
    }
}

#[test]
fn i71_responses_summary_parts_snapshots_and_refusal() {
    let mut parser = responses::Assembler::default();
    let mut events = Vec::new();
    for event in [
        serde_json::json!({"type":"response.reasoning_summary_text.delta","item_id":"r","summary_index":0,"delta":"First"}),
        serde_json::json!({"type":"response.reasoning_summary_text.done","item_id":"r","summary_index":0,"text":"First"}),
        serde_json::json!({"type":"response.reasoning_summary_text.done","item_id":"r","summary_index":1,"text":"Second"}),
        serde_json::json!({"type":"response.refusal.delta","item_id":"m","content_index":0,"delta":"Cannot do that."}),
        serde_json::json!({"type":"response.refusal.done","item_id":"m","content_index":0,"refusal":"Cannot do that."}),
        serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[]}}),
    ] {
        events.extend(parser.absorb(&event.to_string()).unwrap());
    }
    assert_eq!(
        events,
        vec![
            ProviderEvent::SummaryDelta("First".into()),
            ProviderEvent::SummaryDelta("\n\n".into()),
            ProviderEvent::SummaryDelta("Second".into()),
            ProviderEvent::TextDelta("Cannot do that.".into()),
            ProviderEvent::ResponseReplay(crate::ResponseReplayLayout {
                response_id: None,
                slots: Vec::new()
            }),
            ProviderEvent::Done {
                stop_reason: StopReason::End
            }
        ]
    );
    assert!(parser.terminal);
}

#[tokio::test]
async fn issue296_responses_completed_snapshot_remains_a_trusted_final() {
    let mut parser = responses::Assembler::default();
    let events = parser
        .absorb(
            &serde_json::json!({
                "type":"response.completed",
                "response":{"status":"completed","output":[{
                    "id":"answer", "type":"message", "role":"assistant",
                    "content":[{"type":"output_text","text":"Complete answer."}]
                }]}
            })
            .to_string(),
        )
        .unwrap();
    let provider = crate::MockProvider::new(vec![crate::ScriptStep::events(events)]);
    let directory = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    let result = crate::run_agent(
        &provider,
        &tools,
        crate::AgentRequest {
            model: MODEL.into(),
            system_prompt: "Be precise.".into(),
            history: Vec::new(),
            max_tokens: None,
            completed_tool_results: Default::default(),
            tool_config: Default::default(),
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
    assert_eq!(result.final_text, "Complete answer.");
    assert!(result.events.iter().any(|event| matches!(event,
        crate::RuntimeEvent::AssistantResponseCompleted {
            kind: crate::RuntimeAssistantResponseKind::Final, text_start_bytes: 0, text_end_bytes, ..
        } if *text_end_bytes == result.final_text.len()
    )));
}

#[tokio::test]
async fn automatic_replay_empty_response_accepts_only_consistent_completed_output() {
    for streamed_text in [None, Some("not in the completed snapshot")] {
        let mut parser = responses::Assembler::default();
        let mut events = Vec::new();
        if let Some(text) = streamed_text {
            events.extend(
                parser
                    .absorb(
                        &serde_json::json!({
                            "type":"response.output_text.delta", "item_id":"m", "content_index":0,
                            "delta":text
                        })
                        .to_string(),
                    )
                    .unwrap(),
            );
        }
        events.extend(
            parser
                .absorb(
                    &serde_json::json!({
                        "type":"response.completed", "response":{"status":"completed","output":[]}
                    })
                    .to_string(),
                )
                .unwrap(),
        );
        let provider = crate::MockProvider::new(vec![crate::ScriptStep::events(events)]);
        let directory = tempfile::tempdir().unwrap();
        let tools = vega_tools::Tools::new(directory.path()).unwrap();
        let result = crate::run_agent(
            &provider,
            &tools,
            crate::AgentRequest {
                model: MODEL.into(),
                system_prompt: "Be precise.".into(),
                history: vec![ChatMessage::new(ChatRole::User, "Finish the task.")],
                max_tokens: None,
                completed_tool_results: Default::default(),
                tool_config: Default::default(),
                pricing_catalog: None,
                reasoning: Some(crate::FrozenReasoning {
                    provider: "fixture".into(),
                    model: MODEL.into(),
                    protocol: crate::ReasoningProtocol::ResponsesApi,
                    replay_policy: crate::ReasoningReplayPolicy::ResponsesItems,
                    choice: crate::ReasoningChoice::ProviderDefault,
                    supports_disabled: false,
                    disabled_wire: None,
                    declared_efforts: Vec::new(),
                }),
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
        assert_eq!(result.failed, streamed_text.is_some());
        assert_eq!(result.executed_tool_call_count, 0);
        assert!(!result.events.iter().any(|event| matches!(
            event,
            crate::RuntimeEvent::AssistantResponseCompleted {
                kind: crate::RuntimeAssistantResponseKind::Final,
                ..
            }
        )));
        if streamed_text.is_none() {
            assert!(result.final_text.is_empty());
            assert!(
                result
                    .messages
                    .iter()
                    .filter(|message| message.role == ChatRole::Assistant)
                    .all(|message| message.content.is_empty()
                        && message.reasoning_content.is_none()
                        && message.tool_calls.is_empty())
            );
            assert!(result.messages[0].replay_frame().unwrap().seal_eligible());
        }
    }
}

#[tokio::test]
async fn i71_responses_rejects_incompatible_profile_before_network() {
    let provider = OpenAiProvider::new("http://127.0.0.1:1", KEY)
        .unwrap()
        .with_responses_api(true);
    let mut req = request();
    req.reasoning = Some(FrozenReasoning {
        provider: "owned".into(),
        model: MODEL.into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        replay_policy: ReasoningReplayPolicy::StandardChat,
        choice: ReasoningChoice::ProviderDefault,
        supports_disabled: false,
        disabled_wire: None,
        declared_efforts: vec![],
    });
    assert!(matches!(
        provider.chat_stream(req, CancellationToken::new()).await,
        Err(VegaError::ReasoningSelectionInvalid { .. })
    ));
}

#[test]
fn i71_responses_done_reconciles_empty_and_partial_deltas_without_duplicates() {
    let mut parser = responses::Assembler::default();
    let mut out = Vec::new();
    for (kind, text) in [
        ("delta", ""),
        ("done", "hello"),
        ("done", "hello"),
        ("delta", " world"),
        ("done", "hello world!"),
    ] {
        let mut event = serde_json::json!({"type":format!("response.reasoning_summary_text.{kind}"),"item_id":"r","summary_index":0});
        event[if kind == "delta" { "delta" } else { "text" }] = serde_json::json!(text);
        out.extend(parser.absorb(&event.to_string()).unwrap());
    }
    assert_eq!(
        out,
        vec![
            ProviderEvent::SummaryDelta("hello".into()),
            ProviderEvent::SummaryDelta(" world".into()),
            ProviderEvent::SummaryDelta("!".into())
        ]
    );
    assert!(
        !format!(
            "{:?}",
            ProviderEvent::ReasoningReplay(vec![
                serde_json::json!({"encrypted_content":"private opaque"})
            ])
        )
        .contains("private opaque")
    );
    let mut parser = responses::Assembler::default();
    let text = "界".repeat(100_000);
    let event = serde_json::json!({"type":"response.reasoning_summary_text.done","item_id":"r","summary_index":0,"text":text});
    let out = parser.absorb(&event.to_string()).unwrap();
    let combined: String = out
        .into_iter()
        .map(|event| match event {
            ProviderEvent::SummaryDelta(text) => {
                assert!(text.len() <= 64 * 1024);
                text
            }
            _ => panic!("unexpected event"),
        })
        .collect();
    assert_eq!(combined, text);
}

#[tokio::test]
async fn i71_responses_cancellation_discards_pending_tools() {
    let completed = r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","call_id":"owned","name":"read","arguments":"{}"}]}}"#;
    let response = sse_response(&[completed], false);
    let server = mock_transport(Arc::new(move |_| Ok(fixture_response(response.clone())))).await;
    let cancel = CancellationToken::new();
    let mut stream = provider_for(&server, fast_policy(1))
        .with_responses_api(true)
        .chat_stream(request(), cancel.clone())
        .await
        .unwrap();
    cancel.cancel();
    assert!(stream.next().await.is_none());
}

#[test]
fn i71_responses_request_effort_off_tools_and_chat_compatibility() {
    let mut req = request();
    req.reasoning = Some(FrozenReasoning {
        provider: "owned".into(),
        model: MODEL.into(),
        protocol: ReasoningProtocol::ResponsesApi,
        replay_policy: ReasoningReplayPolicy::ResponsesItems,
        choice: ReasoningChoice::Effort("low".into()),
        supports_disabled: true,
        disabled_wire: Some(ReasoningDisabledWire::ReasoningEffortNone),
        declared_efforts: vec!["low".into()],
    });
    req.tools = vec![ToolDefinition {
        name: "read".into(),
        description: "Read".into(),
        input_schema: serde_json::json!({"type":"object"}),
        strict: true,
    }];
    let body = responses::build_body(&req).unwrap();
    assert_eq!(
        body["reasoning"],
        serde_json::json!({"summary":"auto","effort":"low"})
    );
    assert_eq!(body["tools"][0]["strict"], true);
    assert_eq!(body["tools"][0]["type"], "function");
    req.tools[0].strict = false;
    req.tools[0].input_schema =
        serde_json::json!({"type":"object","properties":{"optional":{"type":"string"}}});
    let non_strict = responses::build_body(&req).unwrap();
    assert_eq!(non_strict["tools"][0]["strict"], false);
    assert_eq!(
        non_strict["tools"][0]["parameters"],
        req.tools[0].input_schema
    );
    assert!(
        non_strict["tools"][0]["parameters"]
            .get("required")
            .is_none()
    );
    req.reasoning.as_mut().unwrap().choice = ReasoningChoice::Disabled;
    assert_eq!(
        responses::build_body(&req).unwrap()["reasoning"]["effort"],
        "none"
    );
    let mut chat_request = req.clone();
    chat_request.reasoning.as_mut().unwrap().protocol = ReasoningProtocol::OpenAiChatCompletions;
    chat_request.reasoning.as_mut().unwrap().replay_policy = ReasoningReplayPolicy::StandardChat;
    let chat = build_request_body(&chat_request);
    assert_eq!(chat["reasoning_effort"], "none");
    assert!(chat.get("reasoning").is_none());
    assert!(chat.get("input").is_none());
    req.reasoning.as_mut().unwrap().disabled_wire =
        Some(ReasoningDisabledWire::ThinkingTypeDisabled);
    assert!(responses::build_body(&req).is_err());
}

#[test]
fn i71_responses_item_and_terminal_snapshots_recover_missing_deltas_once() {
    let mut parser = responses::Assembler::default();
    let reasoning = serde_json::json!({"type":"reasoning","id":"r","summary":[{"type":"summary_text","text":"Summary"}],"encrypted_content":"opaque"});
    let message = serde_json::json!({"type":"message","id":"m","role":"assistant","content":[{"type":"output_text","text":"Answer"}]});
    let mut out = parser.absorb(&serde_json::json!({"type":"response.output_item.done","output_index":0,"item":reasoning}).to_string()).unwrap();
    out.extend(parser.absorb(&serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[reasoning,message]}}).to_string()).unwrap());
    assert_eq!(
        out.iter()
            .filter(|event| matches!(event, ProviderEvent::SummaryDelta(_)))
            .count(),
        1
    );
    assert_eq!(
        out.iter()
            .filter(|event| matches!(event,ProviderEvent::TextDelta(text) if text == "Answer"))
            .count(),
        1
    );
    assert!(matches!(out.last(), Some(ProviderEvent::Done { .. })));
}

#[tokio::test]
async fn automatic_replay_rejects_non_assistant_responses_message_before_tools_or_resend() {
    let bad_message = serde_json::json!({
        "type":"message",
        "id":"untrusted-message",
        "role":"user",
        "status":"completed",
        "phase":"commentary",
        "content":[{"type":"output_text","text":"untrusted"}]
    });
    let mut parser = responses::Assembler::default();
    assert!(
        parser
            .absorb(
                &serde_json::json!({
                    "type":"response.completed",
                    "response":{"status":"completed","output":[bad_message.clone()]}
                })
                .to_string()
            )
            .is_err()
    );

    let layout = crate::ResponseReplayLayout {
        response_id: Some("untrusted-response".into()),
        slots: vec![crate::ResponseReplaySlot::Message(bad_message)],
    };
    let provider = crate::MockProvider::new(vec![crate::ScriptStep::events(vec![
        ProviderEvent::ResponseReplay(layout.clone()),
        ProviderEvent::ToolUse {
            id: "must-not-run".into(),
            name: "read".into(),
            input_json: r#"{"path":"lib.rs"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let directory = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    let outcome = crate::run_agent(
        &provider,
        &tools,
        crate::AgentRequest {
            model: MODEL.into(),
            system_prompt: "Be precise.".into(),
            history: Vec::new(),
            max_tokens: None,
            completed_tool_results: Default::default(),
            tool_config: Default::default(),
            pricing_catalog: None,
            reasoning: Some(FrozenReasoning {
                provider: "custom".into(),
                model: MODEL.into(),
                protocol: ReasoningProtocol::ResponsesApi,
                replay_policy: ReasoningReplayPolicy::ResponsesItems,
                choice: ReasoningChoice::ProviderDefault,
                supports_disabled: false,
                disabled_wire: None,
                declared_efforts: Vec::new(),
            }),
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
    assert!(outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert_eq!(provider.requests().len(), 1);

    let mut next = request();
    next.reasoning = Some(FrozenReasoning {
        provider: "custom".into(),
        model: MODEL.into(),
        protocol: ReasoningProtocol::ResponsesApi,
        replay_policy: ReasoningReplayPolicy::ResponsesItems,
        choice: ReasoningChoice::ProviderDefault,
        supports_disabled: false,
        disabled_wire: None,
        declared_efforts: Vec::new(),
    });
    let mut assistant = ChatMessage::new(ChatRole::Assistant, "untrusted");
    assistant.response_replay_layout = Some(layout);
    next.messages.push(assistant);
    assert!(responses::build_body(&next).is_err());
}

#[test]
fn large_reasoning_deltas_done_and_replay_preserve_protocol_data() {
    let mut parser = responses::Assembler::default();
    let chunk = "思考🧠".repeat(30_000);
    let mut observed = String::new();
    for _ in 0..5 {
        let event = serde_json::json!({"type":"response.reasoning_text.delta","item_id":"reasoning","delta":chunk});
        for event in parser.absorb(&event.to_string()).unwrap() {
            if let ProviderEvent::ThinkingDelta(text) = event {
                observed.push_str(&text);
            }
        }
    }
    assert_eq!(observed, chunk.repeat(5));
    let done = serde_json::json!({"type":"response.reasoning_text.done","item_id":"reasoning","text":observed});
    assert!(parser.absorb(&done.to_string()).unwrap().is_empty());
    let wrong = serde_json::json!({"type":"response.reasoning_text.done","item_id":"reasoning","text":"x".repeat(observed.len())});
    assert!(parser.absorb(&wrong.to_string()).is_err());
    let replay = (0..300).map(|index| serde_json::json!({"id":format!("r-{index}"),"type":"reasoning","encrypted_content":"x".repeat(2048)})).collect::<Vec<_>>();
    for (index, item) in replay.iter().enumerate() {
        let event = serde_json::json!({"type":"response.output_item.done","output_index":index,"item":item});
        assert!(parser.absorb(&event.to_string()).unwrap().is_empty());
    }
    let completed = serde_json::json!({"type":"response.completed","response":{"status":"completed","output":replay}});
    let events = parser.absorb(&completed.to_string()).unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event,ProviderEvent::ReasoningReplay(items) if *items == replay))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::Done {
            stop_reason: StopReason::End
        }
    )));
    let mut req = request();
    let mut message = ChatMessage::new(ChatRole::Assistant, "");
    message.response_reasoning = replay.clone();
    req.messages.push(message);
    let body = responses::build_body(&req).unwrap();
    let actual = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "reasoning")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(actual, replay);
}

#[tokio::test]
async fn long_responses_stream_continues_past_old_cumulative_wire_budget() {
    let text = "reasoning ".repeat(30_000);
    let mut frames = (0..20).map(|_| serde_json::json!({"type":"response.reasoning_text.delta","item_id":"reasoning","delta":text}).to_string()).collect::<Vec<_>>();
    frames.push(serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[{"id":"answer","type":"message","role":"assistant","content":[{"type":"output_text","text":"finished"}]}]}}).to_string());
    let refs = frames.iter().map(String::as_str).collect::<Vec<_>>();
    let response = sse_response(&refs, false);
    let server = mock_transport(Arc::new(move |_| Ok(fixture_response(response.clone())))).await;
    let provider = provider_for(&server, fast_policy(1)).with_responses_api(true);
    let mut stream = provider
        .chat_stream(request(), CancellationToken::new())
        .await
        .unwrap();
    let mut thinking_bytes = 0;
    let mut answer = String::new();
    let mut done = false;
    while let Some(event) = stream.next().await {
        match event.unwrap() {
            ProviderEvent::ThinkingDelta(delta) => thinking_bytes += delta.len(),
            ProviderEvent::TextDelta(delta) => answer.push_str(&delta),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            } => done = true,
            _ => {}
        }
    }
    assert_eq!(thinking_bytes, 20 * text.len());
    assert!(thinking_bytes > 4 * 1024 * 1024);
    assert_eq!(answer, "finished");
    assert!(done);
    assert_eq!(server.attempt_count(), 1);
}

#[tokio::test]
async fn oversized_responses_frame_keeps_safe_size_diagnostics() {
    let event = serde_json::json!({"type":"response.reasoning_text.delta","item_id":"reasoning","delta":"x".repeat(4*1024*1024)}).to_string();
    let response = sse_response(&[&event], false);
    let server = mock_transport(Arc::new(move |_| Ok(fixture_response(response.clone())))).await;
    let provider = provider_for(&server, fast_policy(1)).with_responses_api(true);
    let mut stream = provider
        .chat_stream(request(), CancellationToken::new())
        .await
        .unwrap();
    let error = stream.next().await.unwrap().unwrap_err();
    assert!(
        matches!(error,VegaError::ProviderPayloadTooLarge {observed_bytes,limit_bytes} if observed_bytes > limit_bytes && limit_bytes == 4*1024*1024)
    );
    assert!(stream.next().await.is_none());
    assert_eq!(server.attempt_count(), 1);
}
