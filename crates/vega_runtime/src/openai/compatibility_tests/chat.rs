use super::*;

#[test]
fn thinking_dialects_encode_only_their_selected_wire_control() {
    let cases = [
        (
            "openai",
            json!({"supportsReasoningEffort":true}),
            ReasoningChoice::Effort("high".into()),
            json!({"high":"max","off":"none"}),
            vec![("/reasoning_effort", json!("max"))],
        ),
        (
            "openrouter",
            json!({"supportsReasoningEffort":true}),
            ReasoningChoice::Effort("high".into()),
            json!({"high":"high","off":"none"}),
            vec![("/reasoning/effort", json!("high"))],
        ),
        (
            "deepseek",
            json!({"supportsReasoningEffort":true}),
            ReasoningChoice::Effort("high".into()),
            json!({"high":"high","off":"none"}),
            vec![
                ("/thinking/type", json!("enabled")),
                ("/reasoning_effort", json!("high")),
            ],
        ),
        (
            "zai",
            json!({"supportsReasoningEffort":true}),
            ReasoningChoice::Effort("high".into()),
            json!({"high":"high","off":"none"}),
            vec![
                ("/thinking/type", json!("enabled")),
                ("/thinking/clear_thinking", json!(false)),
                ("/reasoning_effort", json!("high")),
            ],
        ),
        (
            "qwen",
            json!({"supportsReasoningEffort":true}),
            ReasoningChoice::Effort("high".into()),
            json!({"high":"high","off":"none"}),
            vec![
                ("/enable_thinking", json!(true)),
                ("/reasoning_effort", json!("high")),
            ],
        ),
        (
            "together",
            json!({"supportsReasoningEffort":true}),
            ReasoningChoice::Effort("high".into()),
            json!({"high":"high","off":"none"}),
            vec![
                ("/reasoning/enabled", json!(true)),
                ("/reasoning_effort", json!("high")),
            ],
        ),
        (
            "baseten",
            json!({"supportsReasoningEffort":true,"chatTemplateArgs":{"enable_thinking":{"$var":"thinking.enabled"}}}),
            ReasoningChoice::Effort("high".into()),
            json!({"high":"high","off":"none"}),
            vec![
                ("/chat_template_args/enable_thinking", json!(true)),
                ("/reasoning_effort", json!("high")),
            ],
        ),
        (
            "ant-ling",
            json!({"supportsReasoningEffort":false}),
            ReasoningChoice::Effort("high".into()),
            json!({"high":"xhigh","off":null,"minimal":null,"low":null,"medium":null,"xhigh":null,"max":null}),
            vec![("/reasoning/effort", json!("xhigh"))],
        ),
        (
            "string-thinking",
            json!({"supportsReasoningEffort":true}),
            ReasoningChoice::Effort("high".into()),
            json!({"high":"enabled","off":"disabled"}),
            vec![("/thinking", json!("enabled"))],
        ),
        (
            "qwen-chat-template",
            json!({"supportsReasoningEffort":false}),
            ReasoningChoice::Enabled,
            json!({"off":"none"}),
            vec![
                ("/chat_template_kwargs/enable_thinking", json!(true)),
                ("/chat_template_kwargs/preserve_thinking", json!(true)),
            ],
        ),
        (
            "chat-template",
            json!({"supportsReasoningEffort":false,"chatTemplateKwargs":{"enable_thinking":{"$var":"thinking.enabled"},"preserve_thinking":true}}),
            ReasoningChoice::Enabled,
            json!({"off":"none"}),
            vec![
                ("/chat_template_kwargs/enable_thinking", json!(true)),
                ("/chat_template_kwargs/preserve_thinking", json!(true)),
            ],
        ),
    ];
    for (format, mut compat, choice, map, expected) in cases {
        compat["thinkingFormat"] = json!(format);
        let model = resolved(json!({"id":format,"compat":compat,"thinkingLevelMap":map}));
        let request = bound_request(&model, choice);
        let body =
            super::super::encode_request("https://fixture.invalid/v1", &request, false).unwrap();
        for (path, value) in expected {
            assert_eq!(body.pointer(path), Some(&value), "{format} {path}");
        }
        assert!(
            body.get("reasoning_effort").is_none()
                || matches!(
                    format,
                    "openai" | "deepseek" | "zai" | "qwen" | "together" | "baseten"
                ),
            "{format}"
        );
        assert!(
            body.get("reasoning").is_none()
                || matches!(format, "openrouter" | "ant-ling" | "together"),
            "{format}"
        );
        assert!(
            body.get("thinking").is_none()
                || matches!(format, "deepseek" | "zai" | "string-thinking"),
            "{format}"
        );
    }
}

#[test]
fn template_effort_mapping_preserves_static_values_and_omits_inactive_control() {
    let model = resolved(json!({
        "thinkingLevelMap":{"high":"default","off":"none"},
        "compat":{"thinkingFormat":"chat-template","supportsReasoningEffort":true,
            "chatTemplateKwargs":{"reasoning_effort":{"$var":"thinking.effort","omitWhenOff":true},"retain":true,"limit":7,"label":"fixture","optional":null}}
    }));
    let body = super::super::encode_request(
        "https://fixture.invalid/v1",
        &bound_request(&model, ReasoningChoice::Effort("high".into())),
        false,
    )
    .unwrap();
    assert_eq!(
        body["chat_template_kwargs"],
        json!({"reasoning_effort":"default","retain":true,"limit":7,"label":"fixture","optional":null})
    );
    assert!(body.get("reasoning_effort").is_none());
    let off = super::super::encode_request(
        "https://fixture.invalid/v1",
        &bound_request(&model, ReasoningChoice::Disabled),
        false,
    )
    .unwrap();
    assert!(
        off["chat_template_kwargs"]
            .get("reasoning_effort")
            .is_none()
    );
    assert_eq!(off["chat_template_kwargs"]["retain"], true);
}

#[test]
fn provider_default_never_becomes_an_explicit_thinking_off_operation() {
    for format in [
        "openai",
        "openrouter",
        "deepseek",
        "zai",
        "qwen",
        "together",
        "baseten",
        "ant-ling",
        "string-thinking",
        "qwen-chat-template",
        "chat-template",
    ] {
        let model = resolved(
            json!({"id":format,"thinkingLevelMap":{"off":"none","high":"high"},"compat":{
                "thinkingFormat":format,"supportsReasoningEffort":true,
                "chatTemplateArgs":{"enable_thinking":{"$var":"thinking.enabled"}},
                "chatTemplateKwargs":{"enable_thinking":{"$var":"thinking.enabled"}}
            }}),
        );
        let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
        for explicit_profile in [true, false] {
            if !explicit_profile {
                request.reasoning = None;
            }
            let body = super::super::encode_request("https://fixture.invalid/v1", &request, false)
                .unwrap();
            for path in [
                "/reasoning_effort",
                "/reasoning/effort",
                "/reasoning/enabled",
                "/thinking/type",
                "/enable_thinking",
                "/chat_template_args/enable_thinking",
                "/chat_template_kwargs/enable_thinking",
            ] {
                assert!(body.pointer(path).is_none(), "{format} {path}");
            }
        }
    }
}

#[test]
fn explicit_disabled_and_boolean_enabled_are_real_separate_operations() {
    for (format, path, on, off) in [
        ("qwen", "/enable_thinking", json!(true), json!(false)),
        ("together", "/reasoning/enabled", json!(true), json!(false)),
        (
            "deepseek",
            "/thinking/type",
            json!("enabled"),
            json!("disabled"),
        ),
        ("zai", "/thinking/type", json!("enabled"), json!("disabled")),
        (
            "baseten",
            "/chat_template_args/enable_thinking",
            json!(true),
            json!(false),
        ),
    ] {
        let model = resolved(
            json!({"id":format,"thinkingLevelMap":{"off":"none"},"compat":{"thinkingFormat":format,"supportsReasoningEffort":false,"chatTemplateArgs":{"enable_thinking":{"$var":"thinking.enabled"}}}}),
        );
        assert!(model.capabilities.allowed_efforts.is_empty(), "{format}");
        for (choice, value) in [
            (ReasoningChoice::Enabled, on),
            (ReasoningChoice::Disabled, off),
        ] {
            let body = super::super::encode_request(
                "https://fixture.invalid/v1",
                &bound_request(&model, choice),
                false,
            )
            .unwrap();
            assert_eq!(body.pointer(path), Some(&value), "{format}");
            assert!(body.get("reasoning_effort").is_none());
        }
    }
}

#[test]
fn system_developer_strict_usage_and_store_are_model_transport_facts() {
    for (reasoning, developer, strict, usage, store) in [
        (true, true, true, true, true),
        (false, true, true, true, true),
        (true, false, false, false, false),
        (true, true, false, true, false),
    ] {
        let model = resolved(json!({"reasoning":reasoning,"compat":{
            "supportsDeveloperRole":developer,"supportsStrictMode":strict,
            "supportsUsageInStreaming":usage,"supportsStore":store
        }}));
        let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
        request.tools = vec![tool("read", true), tool("inspect", false)];
        let original = request.tools.clone();
        let body =
            super::super::encode_request("https://fixture.invalid/v1", &request, false).unwrap();
        assert_eq!(
            body["messages"][0]["role"],
            if reasoning && developer {
                "developer"
            } else {
                "system"
            }
        );
        assert_eq!(
            body["tools"][0]["function"].get("strict"),
            strict.then_some(&json!(true))
        );
        assert_eq!(
            body["tools"][1]["function"].get("strict").cloned(),
            strict.then_some(json!(false))
        );
        assert_eq!(body.get("stream_options").is_some(), usage);
        assert_eq!(body.get("store"), store.then_some(&json!(false)));
        assert_eq!(request.tools, original);
        assert_eq!(
            body["tools"][0]["function"]["parameters"],
            request.tools[0].input_schema
        );
    }
    for (enabled, tools) in [(true, true), (true, false), (false, true)] {
        let model = resolved(json!({"compat":{"zaiToolStream":enabled}}));
        let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
        if tools {
            request.tools = vec![tool("read", false)];
        }
        let body =
            super::super::encode_request("https://fixture.invalid/v1", &request, false).unwrap();
        assert_eq!(
            body.get("tool_stream"),
            (enabled && tools).then_some(&json!(true))
        );
    }
}

#[test]
fn output_limit_preserves_caller_authority_without_catalog_or_estimate_clamping() {
    for field in ["max_tokens", "max_completion_tokens"] {
        let model = resolved(
            json!({"contextWindow":500,"maxTokens":100,"compat":{"maxTokensField":field}}),
        );
        let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
        for cap in [None, Some(32), Some(900)] {
            request.max_tokens = cap;
            let body = super::super::encode_request("https://fixture.invalid/v1", &request, false)
                .unwrap();
            assert_eq!(body.get(field).and_then(Value::as_u64), cap.map(u64::from));
            let other = if field == "max_tokens" {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            assert!(body.get(other).is_none());
        }
    }
}

#[test]
fn ordinary_tool_choice_is_encoded_by_both_native_apis_and_validated_locally() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let model = fixed(api, "https://unknown.invalid/v1", "fixture", "future");
        let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
        request.tools = vec![tool("read", true)];
        for (choice, chat, responses) in [
            (ToolChoice::Auto, json!("auto"), json!("auto")),
            (ToolChoice::None, json!("none"), json!("none")),
            (ToolChoice::Required, json!("required"), json!("required")),
            (
                ToolChoice::NamedFunction("read".into()),
                json!({"type":"function","function":{"name":"read"}}),
                json!({"type":"function","name":"read"}),
            ),
        ] {
            request.tool_choice = Some(choice);
            let body = super::super::encode_request(
                "https://unknown.invalid/v1",
                &request,
                api == ModelApi::Responses,
            )
            .unwrap();
            assert_eq!(
                body["tool_choice"],
                if api == ModelApi::Responses {
                    responses
                } else {
                    chat
                }
            );
            assert_eq!(body["tools"].as_array().unwrap().len(), 1);
        }
        request.tool_choice = None;
        assert!(
            super::super::encode_request(
                "https://unknown.invalid/v1",
                &request,
                api == ModelApi::Responses
            )
            .unwrap()
            .get("tool_choice")
            .is_none()
        );
        request.tools.clear();
        request.tool_choice = Some(ToolChoice::None);
        let body = super::super::encode_request(
            "https://unknown.invalid/v1",
            &request,
            api == ModelApi::Responses,
        )
        .unwrap();
        assert_eq!(body["tool_choice"], "none");
        assert!(body.get("tools").is_none());
        for choice in [
            ToolChoice::Required,
            ToolChoice::NamedFunction("read".into()),
        ] {
            request.tool_choice = Some(choice);
            assert!(
                super::super::encode_request(
                    "https://unknown.invalid/v1",
                    &request,
                    api == ModelApi::Responses
                )
                .is_err()
            );
        }
        request.tools = vec![tool("read", true)];
        request.tool_choice = Some(ToolChoice::NamedFunction("undeclared".into()));
        assert!(
            super::super::encode_request(
                "https://unknown.invalid/v1",
                &request,
                api == ModelApi::Responses
            )
            .is_err()
        );
    }
}

#[test]
fn empty_tool_lists_and_real_history_keep_function_contracts_without_fabrication() {
    let model = resolved(json!({}));
    let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
    assert!(
        super::super::encode_request("https://fixture.invalid/v1", &request, false)
            .unwrap()
            .get("tools")
            .is_none()
    );
    request.messages = vec![
        ChatMessage::assistant_with_tools(
            "",
            vec![ChatToolCall {
                id: "original-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"README.md"}"#.into(),
            }],
        ),
        ChatMessage::tool_result("original-call", "Owned file contents"),
        ChatMessage::new(ChatRole::User, "Continue"),
    ];
    let body = super::super::encode_request("https://fixture.invalid/v1", &request, false).unwrap();
    assert_eq!(body["tools"], json!([]));
    let messages = body["messages"].as_array().unwrap();
    let result = messages.iter().find(|m| m["role"] == "tool").unwrap();
    assert_eq!(result["tool_call_id"], "original-call");
    assert_eq!(result["content"], "Owned file contents");
    let last = messages.len() - 1;
    assert_eq!(messages[last]["role"], "user");
    assert_eq!(messages[last - 1]["role"], "tool");
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[0]["tool_calls"][0]["function"]["arguments"],
        r#"{"path":"README.md"}"#
    );
}

#[test]
fn unknown_proxy_effort_selection_is_sent_for_every_fallback_level() {
    for name in [
        "gpt-6-luna",
        "gpt-6.1-sol",
        "gpt-6-astra",
        "new-model",
        "step-5-preview",
        "glm-5.3",
    ] {
        let model = fixed(
            ModelApi::ChatCompletions,
            "https://unknown.invalid/v1",
            "fixture",
            name,
        );
        for effort in ["minimal", "low", "medium", "high", "xhigh", "max"] {
            let body = super::super::encode_request(
                "https://unknown.invalid/v1",
                &bound_request(&model, ReasoningChoice::Effort(effort.into())),
                false,
            )
            .unwrap();
            assert_eq!(body["reasoning_effort"], effort, "{name}");
            assert!(body.get("thinking").is_none());
            assert!(body.get("enable_thinking").is_none());
            assert!(body.get("max_completion_tokens").is_none());
        }
    }
}

#[test]
fn snapshot_api_route_model_and_saved_choice_mismatches_fail_before_encoding() {
    let model = resolved(json!({"thinkingLevelMap":{"high":"high","low":"low","medium":null}}));
    let request = bound_request(&model, ReasoningChoice::Effort("high".into()));
    let mut wrong = request.clone();
    wrong.model = "another-model".into();
    assert!(super::super::encode_request("https://fixture.invalid/v1", &wrong, false).is_err());
    assert!(super::super::encode_request("https://another.invalid/v1", &request, false).is_err());
    assert!(super::super::encode_request("https://fixture.invalid/v1", &request, true).is_err());
    let smaller = resolve_model(
        &model.definition,
        Some(&override_for(&model, Some(&["low"]), None)),
    )
    .unwrap();
    let mut stale = request.clone();
    stale.model_snapshot = Some(Box::new(smaller));
    assert!(super::super::encode_request("https://fixture.invalid/v1", &stale, false).is_err());
}

#[test]
fn vision_gate_preserves_real_image_order_or_rejects_each_role_before_send() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let base = fixed(
            api,
            "https://unknown.invalid/v1",
            "fixture",
            "vision-name-is-not-evidence",
        );
        let vision = resolve_model(
            &base.definition,
            Some(&override_for(&base, None, Some(true))),
        )
        .unwrap();
        for role in [ChatRole::User, ChatRole::Tool] {
            let mut image = if role == ChatRole::Tool {
                ChatMessage::tool_result("call-image", "caption")
            } else {
                ChatMessage::new(role, "caption")
            };
            image.images = vec![pixels()];
            let mut request = bound_request(&vision, ReasoningChoice::ProviderDefault);
            request.messages = if role == ChatRole::Tool {
                vec![
                    ChatMessage::assistant_with_tools(
                        "",
                        vec![ChatToolCall {
                            id: "call-image".into(),
                            name: "read".into(),
                            input_json: r#"{"path":"image.png"}"#.into(),
                        }],
                    ),
                    image.clone(),
                ]
            } else {
                vec![image.clone()]
            };
            let body = super::super::encode_request(
                "https://unknown.invalid/v1",
                &request,
                api == ModelApi::Responses,
            )
            .unwrap();
            assert!(body.to_string().contains("data:image/png;base64,"));
            assert!(body.to_string().contains("caption"));
            image.images.push(blue_pixels());
            if role == ChatRole::Tool {
                *request.messages.last_mut().unwrap() = image;
            } else {
                request.messages = vec![image];
            }
            let body = super::super::encode_request(
                "https://unknown.invalid/v1",
                &request,
                api == ModelApi::Responses,
            )
            .unwrap();
            let parts = if api == ModelApi::Responses {
                let items = body["input"].as_array().unwrap();
                let item = items
                    .iter()
                    .find(|item| {
                        if role == ChatRole::Tool {
                            item["type"] == "function_call_output"
                        } else {
                            item["role"] == "user"
                        }
                    })
                    .unwrap();
                item[if role == ChatRole::Tool {
                    "output"
                } else {
                    "content"
                }]
                .as_array()
                .unwrap()
                .iter()
                .filter(|part| part["type"] == "input_image")
                .map(|part| part["image_url"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
            } else {
                body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|message| message["content"].as_array().into_iter().flatten())
                    .filter(|part| part["type"] == "image_url")
                    .map(|part| part["image_url"]["url"].as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            };
            assert_eq!(parts, vec![data_url(&pixels()), data_url(&blue_pixels())]);
            request.model_snapshot = Some(Box::new(base.clone()));
            request.reasoning = None;
            assert!(
                super::super::encode_request(
                    "https://unknown.invalid/v1",
                    &request,
                    api == ModelApi::Responses
                )
                .is_err()
            );
        }
        for role in [ChatRole::System, ChatRole::Assistant] {
            let mut image = ChatMessage::new(role, "caption");
            image.images = vec![pixels()];
            let mut request = bound_request(&vision, ReasoningChoice::ProviderDefault);
            request.messages = vec![image];
            assert!(
                super::super::encode_request(
                    "https://unknown.invalid/v1",
                    &request,
                    api == ModelApi::Responses
                )
                .is_err()
            );
        }
    }
}

#[test]
fn official_chat_transport_chooses_completion_limit_without_borrowing_responses_model_facts() {
    for (endpoint, expected) in [
        ("https://api.openai.com/v1", "max_completion_tokens"),
        ("https://unknown.invalid/v1", "max_tokens"),
        ("https://api.openai.com.evil.invalid/v1", "max_tokens"),
        ("https://api.openai.com/proxy", "max_tokens"),
    ] {
        let model = fixed(ModelApi::ChatCompletions, endpoint, "configured", "gpt-5.4");
        assert_eq!(
            model.capabilities.allowed_efforts,
            vec!["minimal", "low", "medium", "high", "xhigh", "max"]
        );
        assert!(!model.capabilities.image_input);
        let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
        request.max_tokens = Some(1234);
        let body = super::super::encode_request(endpoint, &request, false).unwrap();
        assert_eq!(body[expected], 1234, "{endpoint}");
        assert!(
            body.get(if expected == "max_tokens" {
                "max_completion_tokens"
            } else {
                "max_tokens"
            })
            .is_none()
        );
        request.max_tokens = None;
        let body = super::super::encode_request(endpoint, &request, false).unwrap();
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("max_completion_tokens").is_none());
    }
}

#[test]
fn validated_image_bytes_and_existing_budgets_stay_authoritative_after_capability_overrides() {
    for bytes in [
        Vec::new(),
        b"not an image".to_vec(),
        include_bytes!("fixtures/red.png")[..20].to_vec(),
        vec![0; crate::images::MAX_IMAGE_BYTES + 1],
    ] {
        assert!(ImageAttachment::from_bytes(bytes).is_err());
    }
    let red = pixels();
    let blue = blue_pixels();
    assert_ne!(red.bytes(), blue.bytes());
    assert_eq!(
        (red.width(), red.height(), red.mime_type()),
        (2, 2, "image/png")
    );
    assert!(
        crate::images::validate_images(&[red.clone(), blue.clone(), red.clone(), blue.clone()])
            .is_ok()
    );
    assert!(
        crate::images::validate_images(&[
            red.clone(),
            blue.clone(),
            red.clone(),
            blue.clone(),
            red.clone()
        ])
        .is_err()
    );
    let model = fixed(
        ModelApi::ChatCompletions,
        "https://unknown.invalid/v1",
        "fixture",
        "vision",
    );
    let model = resolve_model(
        &model.definition,
        Some(&override_for(&model, None, Some(true))),
    )
    .unwrap();
    let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
    request.messages[1].images = vec![red.clone(), blue.clone(), red.clone(), blue, red];
    assert!(super::super::encode_request("https://unknown.invalid/v1", &request, false).is_err());
}

#[test]
fn fixed_native_model_maps_reach_real_wire_without_clamping_or_service_guessing() {
    for (endpoint, name, choice, expected, absent) in [
        (
            "https://api.groq.com/openai/v1",
            "qwen/qwen3.6-27b",
            ReasoningChoice::Effort("high".into()),
            vec![("/reasoning_effort", json!("default"))],
            vec!["thinking"],
        ),
        (
            "https://api.z.ai/api/coding/paas/v4",
            "glm-5.2",
            ReasoningChoice::Effort("max".into()),
            vec![
                ("/thinking/type", json!("enabled")),
                ("/thinking/clear_thinking", json!(false)),
                ("/reasoning_effort", json!("max")),
                ("/tool_stream", json!(true)),
            ],
            vec![],
        ),
        (
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
            "qwen3.8-max",
            ReasoningChoice::Effort("xhigh".into()),
            vec![
                ("/enable_thinking", json!(true)),
                ("/reasoning_effort", json!("xhigh")),
            ],
            vec!["thinking"],
        ),
        (
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
            "qwen3.8-flash",
            ReasoningChoice::Effort("high".into()),
            vec![
                ("/enable_thinking", json!(true)),
                ("/reasoning_effort", json!("high")),
            ],
            vec!["thinking"],
        ),
        (
            "https://inference.baseten.co/v1",
            "zai-org/GLM-5.2",
            ReasoningChoice::Effort("high".into()),
            vec![
                ("/chat_template_args/enable_thinking", json!(true)),
                ("/reasoning_effort", json!("high")),
            ],
            vec!["thinking"],
        ),
        (
            "https://inference.baseten.co/v1",
            "moonshotai/Kimi-K2.6",
            ReasoningChoice::Enabled,
            vec![("/chat_template_args/enable_thinking", json!(true))],
            vec!["reasoning_effort"],
        ),
        (
            "https://api.fireworks.ai/inference/v1",
            "accounts/fireworks/models/kimi-k3",
            ReasoningChoice::Effort("max".into()),
            vec![("/reasoning_effort", json!("max"))],
            vec!["thinking"],
        ),
        (
            "https://api.together.ai/v1",
            "moonshotai/Kimi-K2.6",
            ReasoningChoice::Enabled,
            vec![("/reasoning/enabled", json!(true))],
            vec!["reasoning_effort"],
        ),
        (
            "https://api.xiaomimimo.com/v1",
            "mimo-v2.5",
            ReasoningChoice::Effort("high".into()),
            vec![
                ("/thinking/type", json!("enabled")),
                ("/reasoning_effort", json!("high")),
            ],
            vec![],
        ),
        (
            "https://api.ant-ling.com/v1",
            "Ring-2.6-1T",
            ReasoningChoice::Effort("high".into()),
            vec![("/reasoning/effort", json!("high"))],
            vec!["reasoning_effort", "thinking"],
        ),
        (
            "https://opencode.ai/zen/go/v1",
            "kimi-k2.6",
            ReasoningChoice::Enabled,
            vec![("/thinking/type", json!("enabled"))],
            vec!["reasoning_effort"],
        ),
        (
            "https://opencode.ai/zen/go/v1",
            "kimi-k2.6",
            ReasoningChoice::Disabled,
            vec![("/thinking/type", json!("disabled"))],
            vec!["reasoning_effort"],
        ),
    ] {
        let model = fixed(ModelApi::ChatCompletions, endpoint, "configured", name);
        let mut request = bound_request(&model, choice);
        request.tools = vec![tool("read", false)];
        let body = super::super::encode_request(endpoint, &request, false).unwrap();
        for (pointer, value) in expected {
            assert_eq!(body.pointer(pointer), Some(&value), "{name} {pointer}");
        }
        for field in absent {
            assert!(body.get(field).is_none(), "{name} {field}");
        }
    }
}

#[test]
fn empty_tool_outputs_use_a_truthful_placeholder_and_chat_images_follow_consecutive_results() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let model = fixed(api, "https://unknown.invalid/v1", "fixture", "future");
        let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
        request.messages = vec![
            ChatMessage::assistant_with_tools(
                "",
                vec![ChatToolCall {
                    id: "empty-result".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"empty"}"#.into(),
                }],
            ),
            ChatMessage::tool_result("empty-result", ""),
        ];
        let body = super::super::encode_request(
            "https://unknown.invalid/v1",
            &request,
            api == ModelApi::Responses,
        )
        .unwrap();
        if api == ModelApi::Responses {
            assert_eq!(body["input"][1]["call_id"], "empty-result");
            assert_eq!(body["input"][1]["output"], "(no tool output)");
        } else {
            assert_eq!(body["messages"][1]["tool_call_id"], "empty-result");
            assert_eq!(body["messages"][1]["content"], "(no tool output)");
        }
    }
    let base = fixed(
        ModelApi::ChatCompletions,
        "https://unknown.invalid/v1",
        "fixture",
        "future",
    );
    let model = resolve_model(
        &base.definition,
        Some(&override_for(&base, None, Some(true))),
    )
    .unwrap();
    let mut request = bound_request(&model, ReasoningChoice::ProviderDefault);
    let mut first = ChatMessage::tool_result("first", "first-caption");
    first.images = vec![pixels()];
    let mut second = ChatMessage::tool_result("second", "second-caption");
    second.images = vec![blue_pixels()];
    request.messages = vec![
        ChatMessage::assistant_with_tools(
            "",
            vec![
                ChatToolCall {
                    id: "first".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"red.png"}"#.into(),
                },
                ChatToolCall {
                    id: "second".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"blue.png"}"#.into(),
                },
            ],
        ),
        first,
        second,
    ];
    let body = super::super::encode_request("https://unknown.invalid/v1", &request, false).unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(
        messages
            .iter()
            .map(|message| message["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["assistant", "tool", "tool", "user"]
    );
    assert_eq!(messages[1]["tool_call_id"], "first");
    assert_eq!(messages[2]["tool_call_id"], "second");
    let images = messages[3]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|part| part["type"] == "image_url")
        .map(|part| part["image_url"]["url"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(images, vec![data_url(&pixels()), data_url(&blue_pixels())]);
}
