use super::*;

#[test]
fn catalog_rejects_invalid_schema_and_conflicting_canonical_identity() {
    let valid = json!({
        "provider":"fixture", "api":"openai-completions", "id":"model",
        "baseUrl":"https://fixture.invalid/v1", "reasoning":true, "input":["text"]
    });
    let mut invalid = vec![json!({}), json!({"version":9,"models":[]})];
    for (field, value) in [
        ("api", json!("other-native")),
        ("baseUrl", json!("https://user:password@fixture.invalid/v1")),
        ("baseUrl", json!("file:///tmp/model")),
        ("input", json!(["audio"])),
        ("reasoning", json!("true")),
        ("thinkingLevelMap", json!({"high":false})),
        ("thinkingLevelMap", json!({"high":""})),
        ("thinkingLevelMap", json!({"unknown":"high"})),
        ("compat", json!({"thinkingFormat":"unknown"})),
        ("compat", json!({"supportsStrictMode":"false"})),
        ("compat", json!({"supportsDeveloperRol":false})),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        invalid.push(json!({"version":1,"models":[bad]}));
    }
    for case in invalid {
        assert!(
            decode_model_catalog(&serde_json::to_vec(&case).unwrap()).is_err(),
            "{case}"
        );
    }
    let mut other = valid.clone();
    other["baseUrl"] = json!("https://FIXTURE.invalid:443/v1/");
    other["reasoning"] = json!(false);
    let bytes = serde_json::to_vec(&json!({"version":1,"models":[valid,other]})).unwrap();
    assert!(decode_model_catalog(&bytes).is_err());
}

#[test]
fn equal_catalog_aliases_resolve_without_order_dependent_model_selection() {
    let first = json!({
        "provider":"a", "api":"openai-completions", "id":"shared",
        "baseUrl":"https://fixture.invalid/v1", "reasoning":true,
        "input":["text","image"], "thinkingLevelMap":{"xhigh":null,"max":"max"}
    });
    let mut alias = first.clone();
    alias["provider"] = json!("b");
    alias["baseUrl"] = json!("https://FIXTURE.invalid:443/v1/");
    let mut fingerprints = Vec::new();
    for models in [vec![first.clone(), alias.clone()], vec![alias, first]] {
        let catalog = decode_model_catalog(
            &serde_json::to_vec(&json!({"version":1,"models":models})).unwrap(),
        )
        .unwrap();
        let model = catalog
            .lookup(
                &route(ModelApi::ChatCompletions, "https://fixture.invalid/v1"),
                &identity("configured", "shared"),
            )
            .unwrap();
        let effective = resolve_model(&model, None).unwrap();
        assert_eq!(effective.identity, identity("configured", "shared"));
        fingerprints.push(effective.fingerprint.clone());
        assert!(effective.capabilities.image_input);
        assert!(
            effective
                .capabilities
                .allowed_efforts
                .contains(&"max".into())
        );
        assert!(
            !effective
                .capabilities
                .allowed_efforts
                .contains(&"xhigh".into())
        );
    }
    assert_eq!(fingerprints[0], fingerprints[1]);
}

#[test]
fn source_map_missing_null_and_mapped_values_remain_distinct() {
    let source = definition(json!({
        "thinkingLevelMap":{"minimal":null,"high":"max","off":null},
        "compat":{"supportsReasoningEffort":true,"thinkingFormat":"openai"}
    }));
    assert_eq!(source.thinking_level_map.get("minimal"), Some(&None));
    assert_eq!(
        source.thinking_level_map.get("high"),
        Some(&Some("max".into()))
    );
    assert_eq!(source.thinking_level_map.get("off"), Some(&None));
    assert!(!source.thinking_level_map.contains_key("low"));
    let effective = resolve_model(&source, None).unwrap();
    assert_eq!(
        effective.capabilities.source_supported_levels,
        vec!["low", "medium", "high"]
    );
    assert_eq!(
        effective.capabilities.encodable_efforts,
        vec!["low", "medium", "high"]
    );
    assert!(!effective.capabilities.supports_disabled);
    let request = bound_request(&effective, ReasoningChoice::Effort("high".into()));
    let body = super::super::encode_request("https://fixture.invalid/v1", &request, false).unwrap();
    assert_eq!(body["reasoning_effort"], "max");
}

#[test]
fn logical_levels_and_real_controls_use_independent_capability_sets() {
    let cases = [
        (
            "not-reasoning",
            false,
            json!({}),
            json!({"thinkingFormat":"openai","supportsReasoningEffort":true}),
            vec!["off"],
            vec![],
            false,
            false,
        ),
        (
            "ordinary",
            true,
            json!({}),
            json!({"thinkingFormat":"openai","supportsReasoningEffort":true}),
            vec!["off", "minimal", "low", "medium", "high"],
            vec!["minimal", "low", "medium", "high"],
            false,
            false,
        ),
        (
            "hole",
            true,
            json!({"xhigh":null,"max":"max"}),
            json!({"thinkingFormat":"openai","supportsReasoningEffort":true}),
            vec!["off", "minimal", "low", "medium", "high", "max"],
            vec!["minimal", "low", "medium", "high", "max"],
            false,
            false,
        ),
        (
            "all",
            true,
            json!({"off":"none","xhigh":"xhigh","max":"max"}),
            json!({"thinkingFormat":"openai","supportsReasoningEffort":true}),
            vec!["off", "minimal", "low", "medium", "high", "xhigh", "max"],
            vec!["minimal", "low", "medium", "high", "xhigh", "max"],
            false,
            true,
        ),
        (
            "boolean",
            true,
            json!({"off":"none","minimal":null,"low":null,"medium":null,"high":"high","xhigh":null,"max":null}),
            json!({"thinkingFormat":"qwen","supportsReasoningEffort":false}),
            vec!["off", "high"],
            vec![],
            true,
            true,
        ),
        (
            "fixed",
            true,
            json!({"off":null,"minimal":null,"low":null,"medium":null,"high":"high","xhigh":null,"max":null}),
            json!({"thinkingFormat":"openai","supportsReasoningEffort":false}),
            vec!["high"],
            vec![],
            false,
            false,
        ),
        (
            "required-mapped",
            true,
            json!({"off":null,"minimal":null,"low":null,"medium":"medium","high":"high","xhigh":"xhigh","max":null}),
            json!({"thinkingFormat":"openai","supportsReasoningEffort":true}),
            vec!["medium", "high", "xhigh"],
            vec!["medium", "high", "xhigh"],
            false,
            false,
        ),
    ];
    for (name, reasoning, map, compat, logical, encodable, enabled, disabled) in cases {
        let model = resolved(
            json!({"id":name,"reasoning":reasoning,"thinkingLevelMap":map,"compat":compat}),
        );
        assert_eq!(
            model.capabilities.source_supported_levels, logical,
            "{name}"
        );
        assert_eq!(model.capabilities.encodable_efforts, encodable, "{name}");
        assert_eq!(model.capabilities.allowed_efforts, encodable, "{name}");
        assert_eq!(model.capabilities.supports_enabled, enabled, "{name}");
        assert_eq!(model.capabilities.supports_disabled, disabled, "{name}");
        assert!(
            freeze_choice(&model, ReasoningChoice::ProviderDefault).is_ok(),
            "{name}"
        );
    }
}

#[test]
fn final_raw_map_keeps_explicit_disabled_off_and_mapped_effort() {
    let source = definition(
        json!({"thinkingLevelMap":{"off":null,"minimal":null,"low":null,"medium":null,"xhigh":"max"},"compat":{"supportsReasoningEffort":true}}),
    );
    assert_eq!(source.thinking_level_map.len(), 5);
    assert!(!source.thinking_level_map.contains_key("high"));
    assert!(!source.thinking_level_map.contains_key("max"));
    let model = resolve_model(&source, None).unwrap();
    assert_eq!(
        model.capabilities.source_supported_levels,
        vec!["high", "xhigh"]
    );
    assert_eq!(model.capabilities.allowed_efforts, vec!["high", "xhigh"]);
    assert!(!model.capabilities.supports_disabled);
    let body = super::super::encode_request(
        "https://fixture.invalid/v1",
        &bound_request(&model, ReasoningChoice::Effort("xhigh".into())),
        false,
    )
    .unwrap();
    assert_eq!(body["reasoning_effort"], "max");
    assert!(freeze_choice(&model, ReasoningChoice::Disabled).is_err());
}

#[test]
fn capability_overrides_are_identity_scoped_immutable_and_resettable() {
    let source = definition(
        json!({"id":"a","input":["text"],"thinkingLevelMap":{"xhigh":"xhigh","max":"max"}}),
    );
    let baseline = resolve_model(&source, None).unwrap();
    let edited = resolve_model(
        &source,
        Some(&override_for(&baseline, Some(&["low", "high"]), Some(true))),
    )
    .unwrap();
    assert_eq!(edited.capabilities.allowed_efforts, vec!["low", "high"]);
    assert!(edited.capabilities.image_input);
    assert_eq!(source.input, Some(vec!["text".into()]));
    assert_eq!(
        resolve_model(&source, None).unwrap().capabilities,
        baseline.capabilities
    );
    assert_ne!(edited.fingerprint, baseline.fingerprint);
    assert_eq!(
        resolve_model(&source, None).unwrap().fingerprint,
        baseline.fingerprint
    );
    for foreign in [identity("other", "a"), identity("fixture", "b")] {
        let mut edit = override_for(&baseline, Some(&["low"]), Some(true));
        edit.identity = foreign;
        assert!(resolve_model(&source, Some(&edit)).is_err());
    }
    let empty = resolve_model(&source, Some(&override_for(&baseline, Some(&[]), None))).unwrap();
    assert!(empty.capabilities.allowed_efforts.is_empty());
    assert!(freeze_choice(&empty, ReasoningChoice::Effort("high".into())).is_err());
    assert!(freeze_choice(&empty, ReasoningChoice::ProviderDefault).is_ok());
}

#[test]
fn empty_effort_subset_preserves_real_boolean_operations() {
    let model = resolved(
        json!({"compat":{"thinkingFormat":"together","supportsReasoningEffort":false},"thinkingLevelMap":{"off":"none"}}),
    );
    let empty = resolve_model(
        &model.definition,
        Some(&override_for(&model, Some(&[]), Some(false))),
    )
    .unwrap();
    assert!(empty.capabilities.allowed_efforts.is_empty());
    assert!(empty.capabilities.supports_enabled);
    assert!(empty.capabilities.supports_disabled);
    assert!(freeze_choice(&empty, ReasoningChoice::Enabled).is_ok());
    assert!(freeze_choice(&empty, ReasoningChoice::Disabled).is_ok());
}

#[test]
fn unsupported_or_duplicate_user_levels_are_rejected_without_clamping() {
    let model = resolved(
        json!({"thinkingLevelMap":{"minimal":null,"low":"low","medium":null,"high":"high","xhigh":null,"max":null}}),
    );
    for levels in [
        &["medium"][..],
        &["xhigh"][..],
        &["off"][..],
        &["low", "low"][..],
        &["unknown"][..],
    ] {
        assert!(
            resolve_model(
                &model.definition,
                Some(&override_for(&model, Some(levels), None))
            )
            .is_err(),
            "{levels:?}"
        );
    }
    for choice in ["minimal", "medium", "xhigh", "max"] {
        assert!(
            freeze_choice(&model, ReasoningChoice::Effort(choice.into())).is_err(),
            "{choice}"
        );
    }
}

#[test]
fn unknown_models_use_six_generic_levels_on_both_native_apis() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        for name in [
            "gpt-6-luna",
            "gpt-6.1-sol",
            "gpt-6-astra",
            "future-model",
            "step-5-preview",
            "glm-5.3",
        ] {
            let model = fixed(api, "https://unknown.invalid/v1", "custom", name);
            assert_eq!(
                model.capabilities.allowed_efforts,
                vec!["minimal", "low", "medium", "high", "xhigh", "max"],
                "{name}"
            );
            assert!(!model.capabilities.image_input);
            assert!(!model.capabilities.supports_disabled);
            assert!(!model.capabilities.supports_enabled);
            let edited = resolve_model(
                &model.definition,
                Some(&override_for(&model, Some(&["low", "high"]), Some(true))),
            )
            .unwrap();
            assert_eq!(edited.capabilities.allowed_efforts, vec!["low", "high"]);
            assert!(edited.capabilities.image_input);
        }
    }
}

#[test]
fn fixed_lookup_uses_exact_endpoint_and_api_before_service_defaults() {
    let qwen = fixed(
        ModelApi::ChatCompletions,
        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
        "configured",
        "qwen3.6-plus",
    );
    assert!(qwen.capabilities.supports_enabled);
    assert!(qwen.capabilities.allowed_efforts.is_empty());
    let proxy = fixed(
        ModelApi::ChatCompletions,
        "https://unknown.invalid/v1",
        "zai",
        "glm-5.3",
    );
    assert_eq!(proxy.capabilities.allowed_efforts.len(), 6);
    let other_api = fixed(
        ModelApi::Responses,
        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
        "configured",
        "qwen3.6-plus",
    );
    assert_eq!(other_api.capabilities.allowed_efforts.len(), 6);
    assert!(!other_api.capabilities.supports_enabled);
    let step = fixed(
        ModelApi::ChatCompletions,
        "https://api.z.ai/api/paas/v4",
        "configured",
        "step-5-preview",
    );
    assert_eq!(step.capabilities.allowed_efforts.len(), 6);
}

#[test]
fn fixed_native_model_metadata_preserves_reasoning_vision_and_limits() {
    let cases = [
        (
            ModelApi::ChatCompletions,
            "https://api.together.xyz/v1",
            "together",
            "moonshotai/Kimi-K2.6",
            true,
            true,
        ),
        (
            ModelApi::ChatCompletions,
            "https://api.z.ai/api/coding/paas/v4",
            "zai",
            "glm-4.7",
            true,
            false,
        ),
        (
            ModelApi::Responses,
            "https://api.openai.com/v1",
            "openai",
            "gpt-5.4",
            true,
            true,
        ),
        (
            ModelApi::Responses,
            "https://api.x.ai/v1",
            "xai",
            "grok-4.6",
            true,
            true,
        ),
    ];
    for (api, endpoint, provider, name, reasoning, image) in cases {
        let model = fixed(api, endpoint, provider, name);
        assert_eq!(
            model.definition.reasoning,
            Some(reasoning),
            "{provider}/{name}"
        );
        assert_eq!(model.capabilities.image_input, image, "{provider}/{name}");
        assert!(
            model.definition.context_window.is_some_and(|n| n > 0),
            "{name}"
        );
        assert!(
            model.definition.max_output_tokens.is_some_and(|n| n > 0),
            "{name}"
        );
    }
}

#[test]
fn complete_pinned_native_metadata_survives_decoder_lookup_and_fixed_resolution() {
    let bytes = include_bytes!("fixtures/native-models.json");
    let expected: Value = serde_json::from_slice(bytes).unwrap();
    let catalog = decode_model_catalog(bytes).unwrap();
    for raw in expected["models"].as_array().unwrap() {
        let api = if raw["api"] == "openai-responses" {
            ModelApi::Responses
        } else {
            ModelApi::ChatCompletions
        };
        let endpoint = raw["baseUrl"].as_str().unwrap();
        let id = identity(
            raw["provider"].as_str().unwrap(),
            raw["id"].as_str().unwrap(),
        );
        let route = route(api, endpoint);
        let decoded = catalog.lookup(&route, &id).unwrap();
        let fixed = lookup_model_definition(&route, &id).unwrap();
        for definition in [&decoded, &fixed] {
            assert_eq!(definition.route, route, "{}", id.model);
            assert_eq!(definition.identity, id, "{}", id.model);
            assert_eq!(
                definition.reasoning,
                raw.get("reasoning").and_then(Value::as_bool),
                "{}",
                id.model
            );
            let input = raw.get("input").map(|values| {
                values
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            });
            assert_eq!(definition.input, input, "{}", id.model);
            assert_eq!(
                definition.context_window,
                raw.get("contextWindow").and_then(Value::as_u64),
                "{}",
                id.model
            );
            assert_eq!(
                definition.max_output_tokens.map(u64::from),
                raw.get("maxTokens").and_then(Value::as_u64),
                "{}",
                id.model
            );
            let map = raw
                .get("thinkingLevelMap")
                .map(|map| {
                    map.as_object()
                        .unwrap()
                        .iter()
                        .map(|(key, value)| (key.clone(), value.as_str().map(str::to_string)))
                        .collect()
                })
                .unwrap_or_default();
            assert_eq!(definition.thinking_level_map, map, "{}", id.model);
            let actual = serde_json::to_value(&definition.compat).unwrap();
            for (key, value) in raw
                .get("compat")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                assert_eq!(actual.get(key), Some(value), "{} {key}", id.model);
            }
        }
        let resolved = resolve_model(&fixed, None).unwrap();
        assert_eq!(resolved.identity, id);
        assert_eq!(
            resolved.capabilities.image_input,
            raw["input"]
                .as_array()
                .is_some_and(|values| values.contains(&json!("image")))
        );
        assert_eq!(
            resolved.definition.thinking_level_map,
            fixed.thinking_level_map
        );
    }
}

#[test]
fn common_reasoning_metadata_from_other_native_models_keeps_raw_level_semantics_only() {
    for (name, map, levels) in [
        (
            "anthropic-opus-4.6",
            json!({"max":"max"}),
            vec!["off", "minimal", "low", "medium", "high", "max"],
        ),
        (
            "anthropic-opus-4.8",
            json!({"xhigh":"xhigh","max":"max"}),
            vec!["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        ),
        (
            "anthropic-opus-5",
            json!({"xhigh":"xhigh","max":"max"}),
            vec!["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        ),
        (
            "anthropic-sonnet-4.6",
            json!({"max":"max"}),
            vec!["off", "minimal", "low", "medium", "high", "max"],
        ),
        (
            "anthropic-sonnet-5",
            json!({"xhigh":"xhigh","max":"max"}),
            vec!["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        ),
        (
            "anthropic-fable-5",
            json!({"off":null,"xhigh":"xhigh","max":"max"}),
            vec!["minimal", "low", "medium", "high", "xhigh", "max"],
        ),
        (
            "anthropic-sonnet-4.5",
            json!({}),
            vec!["off", "minimal", "low", "medium", "high"],
        ),
        (
            "bedrock-opus-5",
            json!({"xhigh":"xhigh","max":"max"}),
            vec!["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        ),
        (
            "bedrock-fable-5",
            json!({"off":null,"xhigh":"xhigh","max":"max"}),
            vec!["minimal", "low", "medium", "high", "xhigh", "max"],
        ),
        (
            "kimi-coding-k3",
            json!({"off":null,"minimal":null,"low":"low","medium":null,"high":"high","xhigh":null,"max":"max"}),
            vec!["low", "high", "max"],
        ),
    ] {
        let model = resolved(
            json!({"id":name,"reasoning":true,"thinkingLevelMap":map,"compat":{"thinkingFormat":"openai","supportsReasoningEffort":false}}),
        );
        assert_eq!(model.capabilities.source_supported_levels, levels, "{name}");
        assert!(model.capabilities.encodable_efforts.is_empty(), "{name}");
        assert!(model.capabilities.allowed_efforts.is_empty(), "{name}");
    }
}

#[test]
fn verified_native_models_expose_their_exact_logical_holes_and_boolean_or_effort_controls() {
    for (api, endpoint, name, expected) in [
        (
            ModelApi::Responses,
            "https://api.openai.com/v1",
            "gpt-5.6-sol",
            vec!["off", "low", "medium", "high", "xhigh", "max"],
        ),
        (
            ModelApi::Responses,
            "https://api.openai.com/v1",
            "gpt-5.6-terra",
            vec!["off", "low", "medium", "high", "xhigh", "max"],
        ),
        (
            ModelApi::Responses,
            "https://api.openai.com/v1",
            "gpt-5.6-luna",
            vec!["off", "low", "medium", "high", "xhigh", "max"],
        ),
        (
            ModelApi::Responses,
            "https://api.openai.com/v1",
            "gpt-5.5-pro",
            vec!["medium", "high", "xhigh"],
        ),
        (
            ModelApi::Responses,
            "https://api.x.ai/v1",
            "grok-4.6",
            vec!["low", "medium", "high", "xhigh"],
        ),
        (
            ModelApi::ChatCompletions,
            "https://openrouter.ai/api/v1",
            "openai/gpt-5.5-pro",
            vec!["medium", "high", "xhigh"],
        ),
        (
            ModelApi::ChatCompletions,
            "https://api.deepseek.com",
            "deepseek-v4-flash",
            vec!["off", "low", "high", "max"],
        ),
        (
            ModelApi::ChatCompletions,
            "https://opencode.ai/zen/go/v1",
            "deepseek-v4-flash",
            vec!["off", "low", "high", "max"],
        ),
        (
            ModelApi::ChatCompletions,
            "https://opencode.ai/zen/go/v1",
            "kimi-k2.6",
            vec!["off", "high"],
        ),
        (
            ModelApi::ChatCompletions,
            "https://api.moonshot.ai/v1",
            "kimi-k2.7-code",
            vec!["minimal", "low", "medium", "high"],
        ),
        (
            ModelApi::ChatCompletions,
            "https://api.moonshot.cn/v1",
            "kimi-k2.7-code",
            vec!["minimal", "low", "medium", "high"],
        ),
        (
            ModelApi::ChatCompletions,
            "https://api.moonshot.ai/v1",
            "kimi-k3",
            vec!["low", "high", "max"],
        ),
        (
            ModelApi::ChatCompletions,
            "https://api.moonshot.cn/v1",
            "kimi-k3",
            vec!["low", "high", "max"],
        ),
        (
            ModelApi::Responses,
            "https://opencode.ai/zen/v1",
            "grok-build-0.1",
            vec!["high"],
        ),
        (
            ModelApi::ChatCompletions,
            "https://openrouter.ai/api/v1",
            "deepseek/deepseek-v4-flash",
            vec!["off", "high", "xhigh"],
        ),
    ] {
        let model = fixed(api, endpoint, "configured", name);
        assert_eq!(
            model.capabilities.source_supported_levels, expected,
            "{name}"
        );
        assert!(
            model
                .capabilities
                .allowed_efforts
                .iter()
                .all(|effort| expected.contains(&effort.as_str())),
            "{name}"
        );
    }
}
