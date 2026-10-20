use super::*;

fn completed(output: Vec<Value>, usage: Option<Value>) -> Value {
    let mut value = json!({"type":"response.completed","response":{"id":"resp-fixture","status":"completed","output":output}});
    if let Some(usage) = usage {
        value["response"]["usage"] = usage;
    }
    value
}

fn message(id: &str, text: &str, phase: &str) -> Value {
    json!({"type":"message","id":id,"role":"assistant","status":"completed","phase":phase,"content":[{"type":"output_text","text":text,"annotations":[]}]})
}

#[test]
fn responses_default_explicit_effort_and_off_keep_separate_wire_meanings() {
    let model = resolved(
        json!({"api":"openai-responses","thinkingLevelMap":{"off":"none","high":"max","max":"max"},"compat":{"supportsStrictMode":true}}),
    );
    for (choice, expected) in [
        (ReasoningChoice::ProviderDefault, None),
        (ReasoningChoice::Effort("high".into()), Some("max")),
        (ReasoningChoice::Disabled, Some("none")),
    ] {
        let mut request = bound_request(&model, choice);
        request.tools = vec![tool("read", true), tool("inspect", false)];
        let body =
            super::super::encode_request("https://fixture.invalid/v1", &request, true).unwrap();
        if let Some(expected) = expected {
            assert_eq!(body["reasoning"]["summary"], "auto");
            assert_eq!(body["reasoning"]["effort"], expected);
        } else {
            assert!(body.get("reasoning").is_none());
        }
        assert_eq!(body["store"], false);
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["tools"][0]["name"], "read");
        assert_eq!(body["tools"][0]["strict"], true);
        assert_eq!(body["tools"][1]["strict"], false);
        assert_eq!(
            body["tools"][0]["parameters"],
            request.tools[0].input_schema
        );
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("thinking").is_none());
    }
}

#[test]
fn responses_known_model_choices_are_metadata_bound_and_unknown_choices_reach_wire() {
    for name in [
        "gpt-5.1",
        "gpt-5.2",
        "gpt-5.3-codex",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gpt-5.4-nano",
        "gpt-5.5",
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
    ] {
        let model = fixed(
            ModelApi::Responses,
            "https://api.openai.com/v1",
            "configured",
            name,
        );
        let body = super::super::encode_request(
            "https://api.openai.com/v1",
            &bound_request(&model, ReasoningChoice::ProviderDefault),
            true,
        )
        .unwrap();
        assert!(body.get("reasoning").is_none(), "{name}");
        assert!(model.capabilities.supports_disabled, "{name}");
        let off = super::super::encode_request(
            "https://api.openai.com/v1",
            &bound_request(&model, ReasoningChoice::Disabled),
            true,
        )
        .unwrap();
        assert_eq!(off["reasoning"]["effort"], "none", "{name}");
    }
    for name in [
        "gpt-5",
        "gpt-5-mini",
        "gpt-5-nano",
        "gpt-5-pro",
        "gpt-5.2-pro",
        "gpt-5.4-pro",
        "gpt-5.5-pro",
    ] {
        let model = fixed(
            ModelApi::Responses,
            "https://api.openai.com/v1",
            "configured",
            name,
        );
        assert!(!model.capabilities.supports_disabled, "{name}");
        assert!(
            freeze_choice(&model, ReasoningChoice::Disabled).is_err(),
            "{name}"
        );
        let body = super::super::encode_request(
            "https://api.openai.com/v1",
            &bound_request(&model, ReasoningChoice::ProviderDefault),
            true,
        )
        .unwrap();
        assert!(body.get("reasoning").is_none());
    }
    let model = fixed(
        ModelApi::Responses,
        "https://unknown.invalid/v1",
        "custom",
        "new-model",
    );
    for effort in ["minimal", "low", "medium", "high", "xhigh", "max"] {
        let body = super::super::encode_request(
            "https://unknown.invalid/v1",
            &bound_request(&model, ReasoningChoice::Effort(effort.into())),
            true,
        )
        .unwrap();
        assert_eq!(body["reasoning"]["effort"], effort);
    }
}

#[test]
fn responses_output_limit_is_preserved_and_chat_only_fields_never_cross_api() {
    let model = resolved(json!({"api":"openai-responses","maxTokens":128,"contextWindow":500}));
    let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
    for limit in [None, Some(32), Some(900)] {
        request.max_tokens = limit;
        let body =
            super::super::encode_request("https://fixture.invalid/v1", &request, true).unwrap();
        assert_eq!(
            body.get("max_output_tokens").and_then(Value::as_u64),
            limit.map(u64::from)
        );
        for field in [
            "max_tokens",
            "max_completion_tokens",
            "stream_options",
            "tool_stream",
            "enable_thinking",
            "reasoning_effort",
        ] {
            assert!(body.get(field).is_none(), "{field}");
        }
    }
}

#[test]
fn responses_encrypted_reasoning_uses_done_value_and_fills_only_missing_payload() {
    for done_encrypted in [Some("from-item-done"), None] {
        let mut done = json!({"type":"reasoning","id":"rs-one","summary":[]});
        if let Some(value) = done_encrypted {
            done["encrypted_content"] = json!(value);
        }
        let complete = json!({"type":"reasoning","id":"rs-one","summary":[],"encrypted_content":"from-completed"});
        let events=decode_responses(&[
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":"rs-one","summary":[]}}),
            json!({"type":"response.output_item.done","output_index":0,"item":done}),
            completed(vec![complete],None),
        ]).unwrap();
        let replay = events
            .iter()
            .find_map(|e| match e {
                ProviderEvent::ReasoningReplay(items) => Some(items),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            replay[0]["encrypted_content"],
            done_encrypted.unwrap_or("from-completed")
        );
        assert_eq!(replay[0]["id"], "rs-one");
        assert!(!format!("{events:?}").contains("from-completed"));
        assert!(!text(&events).contains("from-item-done"));
    }
}

#[test]
fn responses_fragmented_function_calls_emit_only_final_original_arguments() {
    let events=decode_responses(&[
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc-one","call_id":"call-one","name":"read","arguments":""}}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc-one","delta":"{\"path\":\"README"}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc-one","delta":".md\"}"}),
        json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc-one","call_id":"call-one","name":"read","arguments":"{\"path\":\"README.md\"}"}}),
        completed(vec![json!({"type":"function_call","id":"fc-one","call_id":"call-one","name":"read","arguments":"{\"path\":\"README.md\"}"})],None),
    ]).unwrap();
    assert_eq!(
        calls(&events),
        vec![(
            "call-one".into(),
            "read".into(),
            r#"{"path":"README.md"}"#.into()
        )]
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e,
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse
                }
            ))
            .count(),
        1
    );
    let layout = events
        .iter()
        .find_map(|e| match e {
            ProviderEvent::ResponseReplay(layout) => Some(layout),
            _ => None,
        })
        .unwrap();
    assert_eq!(layout.slots.len(), 1);
    assert_eq!(layout.response_id.as_deref(), Some("resp-fixture"));
}

#[test]
fn responses_terminal_phases_never_replace_authentic_completed_or_failed_state() {
    for (added, done) in [
        ("commentary", "commentary"),
        ("final_answer", "final_answer"),
        ("commentary", "final_answer"),
    ] {
        let item = message("msg-one", "answer", done);
        let events=decode_responses(&[
            json!({"type":"response.output_item.added","output_index":0,"item":message("msg-one","",added)}),
            json!({"type":"response.output_item.done","output_index":0,"item":item.clone()}),
            completed(vec![item],None),
        ]).unwrap();
        assert_eq!(text(&events), "answer");
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(
                    e,
                    ProviderEvent::Done {
                        stop_reason: StopReason::End
                    }
                ))
                .count(),
            1
        );
    }
    for reason in ["max_output_tokens", "content_filter", "max_time_limit"] {
        let result = decode_responses(&[
            json!({"type":"response.output_item.done","output_index":0,"item":message("msg-one","partial","final_answer")}),
            json!({"type":"response.incomplete","response":{"id":"resp-one","status":"incomplete","incomplete_details":{"reason":reason},"usage":{"input_tokens":30,"output_tokens":12}}}),
        ]);
        let error = result.unwrap_err();
        assert!(
            matches!(
                error,
                VegaError::ProviderDiagnostic {
                    kind: ProviderFailureKind::OutputLimit,
                    ..
                }
            ) || reason != "max_output_tokens"
        );
    }
    assert!(decode_responses(&[json!({"type":"response.failed","response":{"status":"failed","error":{"code":"server_error","message":"failure"}}})]).is_err());
}

#[test]
fn responses_usage_preserves_total_input_and_both_cache_counters() {
    let events = decode_responses(&[completed(
        vec![message("msg-one", "OK", "final_answer")],
        Some(json!({
            "input_tokens":20,"output_tokens":7,"total_tokens":27,
            "input_tokens_details":{"cached_tokens":2,"cache_write_tokens":3},
            "output_tokens_details":{"reasoning_tokens":5}
        })),
    )])
    .unwrap();
    assert_eq!(text(&events), "OK");
    assert!(events.iter().any(|e| matches!(
        e,
        ProviderEvent::Usage {
            input: 20,
            output: 7,
            cache_read: Some(2),
            cache_write: Some(3)
        }
    )));
}

#[test]
fn responses_native_replay_preserves_message_order_and_opaque_layout() {
    let model = fixed(
        ModelApi::Responses,
        "https://unknown.invalid/v1",
        "fixture",
        "future",
    );
    let output = vec![
        json!({"type":"reasoning","id":"rs-one","summary":[{"type":"summary_text","text":"summary"}],"encrypted_content":"opaque+/="}),
        message("msg-one", "before", "commentary"),
        json!({"type":"function_call","id":"fc-one","call_id":"call-one","name":"read","arguments":"{\"path\":\"README.md\"}"}),
    ];
    let events = decode_responses(&[completed(output.clone(), None)]).unwrap();
    let layout = events
        .iter()
        .find_map(|e| match e {
            ProviderEvent::ResponseReplay(layout) => Some(layout.clone()),
            _ => None,
        })
        .unwrap();
    let reasoning = events
        .iter()
        .find_map(|e| match e {
            ProviderEvent::ReasoningReplay(items) => Some(items.clone()),
            _ => None,
        })
        .unwrap();
    let mut assistant = ChatMessage::assistant_with_tools(
        "before",
        vec![ChatToolCall {
            id: "call-one".into(),
            name: "read".into(),
            input_json: r#"{"path":"README.md"}"#.into(),
        }],
    );
    assistant.response_reasoning = reasoning;
    assistant.response_replay_layout = Some(layout);
    let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
    request.messages = vec![assistant, ChatMessage::tool_result("call-one", "")];
    let body = super::super::encode_request("https://unknown.invalid/v1", &request, true).unwrap();
    let input = body["input"].as_array().unwrap();
    assert_eq!(&input[..3], output.as_slice());
    assert_eq!(input[3]["type"], "function_call_output");
    assert_eq!(input[3]["call_id"], "call-one");
    assert_eq!(input[3]["output"], "(no tool output)");
    assert!(!input[3].to_string().contains("attached image"));
    assert!(!format!("{request:?}").contains("opaque+/="));
}

#[test]
fn foreign_raw_is_rejected_and_cold_projection_never_exposes_private_state() {
    let response_model = fixed(
        ModelApi::Responses,
        "https://unknown.invalid/v1",
        "fixture",
        "future",
    );
    let mut assistant = ChatMessage::assistant_with_tools_and_reasoning(
        "visible",
        Some("private-source".into()),
        vec![],
    );
    let mut request = bound_request(&response_model, ReasoningChoice::ProviderDefault);
    request.messages.push(assistant.clone());
    assert!(super::super::encode_request("https://unknown.invalid/v1", &request, true).is_err());
    assistant.reasoning_content = None;
    assistant.response_reasoning = vec![
        json!({"type":"reasoning","id":"rs-one","encrypted_content":"opaque-source","summary":[]}),
    ];
    let chat_model = fixed(
        ModelApi::ChatCompletions,
        "https://unknown.invalid/v1",
        "fixture",
        "future",
    );
    request = bound_request(&chat_model, ReasoningChoice::ProviderDefault);
    request.messages.push(assistant);
    assert!(super::super::encode_request("https://unknown.invalid/v1", &request, false).is_err());
    let mut system = ChatMessage::new(ChatRole::System, "system");
    system.attach_replay_frame(crate::ReplayFrame::new(
        request.messages.len() + 1,
        crate::ReasoningReplayPolicy::Unknown,
    ));
    request.messages.insert(0, system);
    request
        .messages
        .push(ChatMessage::new(ChatRole::User, "Continue"));
    let projected = crate::project_model_messages(&request.messages).unwrap();
    assert!(projected.iter().all(|m| m.reasoning_content.is_none()
        && m.chat_reasoning.is_none()
        && m.response_reasoning.is_empty()));
    let mut cold = request.clone();
    cold.messages = projected;
    let body = super::super::encode_request("https://unknown.invalid/v1", &cold, false).unwrap();
    assert!(!body.to_string().contains("private-source"));
    assert!(!body.to_string().contains("opaque-source"));
}

#[test]
fn native_tool_image_results_keep_call_association_and_pixel_order() {
    let base = fixed(
        ModelApi::Responses,
        "https://unknown.invalid/v1",
        "fixture",
        "future",
    );
    let model = resolve_model(
        &base.definition,
        Some(&override_for(&base, None, Some(true))),
    )
    .unwrap();
    let mut result = ChatMessage::tool_result("call-image", "caption");
    result.images = vec![pixels(), blue_pixels()];
    let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
    request.messages = vec![
        ChatMessage::assistant_with_tools(
            "",
            vec![ChatToolCall {
                id: "call-image".into(),
                name: "read".into(),
                input_json: r#"{"path":"image.png"}"#.into(),
            }],
        ),
        result,
    ];
    let body = super::super::encode_request("https://unknown.invalid/v1", &request, true).unwrap();
    let output = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["type"] == "function_call_output")
        .unwrap();
    assert_eq!(output["call_id"], "call-image");
    assert_eq!(
        output["output"][0],
        json!({"type":"input_text","text":"caption"})
    );
    assert_eq!(output["output"][1]["type"], "input_image");
    assert_eq!(output["output"][2]["type"], "input_image");
    assert_eq!(output["output"][1]["image_url"], data_url(&pixels()));
    assert_eq!(output["output"][2]["image_url"], data_url(&blue_pixels()));
    assert!(
        !body["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "user")
    );
}

#[test]
fn nonlive_foreign_tool_id_projection_is_unique_bounded_and_result_consistent() {
    let first = format!("call-shared|{}+/=", "item-one".repeat(30));
    let second = format!("call-shared|{}+/=", "item-two".repeat(30));
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let model = fixed(api, "https://unknown.invalid/v1", "fixture", "future");
        let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
        request.messages = vec![
            ChatMessage::assistant_with_tools(
                "",
                vec![
                    ChatToolCall {
                        id: first.clone(),
                        name: "read".into(),
                        input_json: r#"{"path":"one"}"#.into(),
                    },
                    ChatToolCall {
                        id: second.clone(),
                        name: "read".into(),
                        input_json: r#"{"path":"two"}"#.into(),
                    },
                ],
            ),
            ChatMessage::tool_result(&first, "one"),
            ChatMessage::tool_result(&second, "two"),
        ];
        let original = request.messages.clone();
        let body = super::super::encode_request(
            "https://unknown.invalid/v1",
            &request,
            api == ModelApi::Responses,
        )
        .unwrap();
        let (ids, results): (Vec<String>, Vec<String>) = if api == ModelApi::Responses {
            let input = body["input"].as_array().unwrap();
            (
                input
                    .iter()
                    .filter(|i| i["type"] == "function_call")
                    .map(|i| i["call_id"].as_str().unwrap().into())
                    .collect(),
                input
                    .iter()
                    .filter(|i| i["type"] == "function_call_output")
                    .map(|i| i["call_id"].as_str().unwrap().into())
                    .collect(),
            )
        } else {
            let input = body["messages"].as_array().unwrap();
            (
                input
                    .iter()
                    .flat_map(|i| i["tool_calls"].as_array().into_iter().flatten())
                    .map(|i| i["id"].as_str().unwrap().into())
                    .collect(),
                input
                    .iter()
                    .filter(|i| i["role"] == "tool")
                    .map(|i| i["tool_call_id"].as_str().unwrap().into())
                    .collect(),
            )
        };
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert_eq!(ids, results);
        assert!(
            ids.iter()
                .all(|id| id.len() <= if api == ModelApi::Responses { 64 } else { 40 })
        );
        assert!(ids.iter().all(|id| {
            id.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        }));
        assert_eq!(request.messages, original);
    }
}

#[test]
fn nonreasoning_responses_omit_thinking_fields_and_encrypted_includes() {
    let model = resolved(json!({"api":"openai-responses","reasoning":false}));
    for explicit in [true, false] {
        let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
        if !explicit {
            request.reasoning = None;
        }
        let body =
            super::super::encode_request("https://fixture.invalid/v1", &request, true).unwrap();
        assert!(body.get("reasoning").is_none());
        assert!(!body.get("include").is_some_and(|values| {
            values
                .as_array()
                .is_some_and(|values| values.contains(&json!("reasoning.encrypted_content")))
        }));
    }
}

#[test]
fn fixed_xai_responses_models_keep_real_roles_efforts_and_encrypted_replay_request() {
    for (name, choice, effort) in [
        ("grok-4.5", ReasoningChoice::ProviderDefault, None),
        (
            "grok-4.5",
            ReasoningChoice::Effort("medium".into()),
            Some("medium"),
        ),
        (
            "grok-4.6",
            ReasoningChoice::Effort("xhigh".into()),
            Some("xhigh"),
        ),
        (
            "grok-4.3",
            ReasoningChoice::Effort("low".into()),
            Some("low"),
        ),
    ] {
        let model = fixed(
            ModelApi::Responses,
            "https://api.x.ai/v1",
            "configured",
            name,
        );
        let body = super::super::encode_request(
            "https://api.x.ai/v1",
            &bound_request(&model, choice),
            true,
        )
        .unwrap();
        assert_eq!(body["model"], name);
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["input"][0]["role"], "developer");
        if let Some(effort) = effort {
            assert_eq!(body["reasoning"]["effort"], effort);
            assert_eq!(body["reasoning"]["summary"], "auto");
        } else {
            assert!(body.get("reasoning").is_none());
        }
        assert!(body.get("prompt_cache_retention").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }
}
