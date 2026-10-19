use super::*;

fn old_step(preference: ReasoningChoice) -> ReasoningProfileProjection {
    ReasoningProfileProjection {
        provider: "proxy".into(),
        model: "step-5-preview".into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        replay_profile: ReasoningReplaySetting::StandardChat,
        support: ReasoningSupport::Optional,
        supports_disabled: true,
        disabled_wire: Some(ReasoningDisabledWire::ReasoningEffortNone),
        efforts: ["minimal", "low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        preference,
    }
}

#[test]
fn step_capabilities_are_automatic_on_a_custom_chat_proxy_and_preserve_high() {
    for configured in [None, Some(old_step(ReasoningChoice::Effort("high".into())))] {
        let profile = resolve_reasoning_profile(
            "proxy",
            ProviderApi::ChatCompletions,
            "https://gateway.example/v1",
            "step-5-preview",
            configured.clone(),
        )
        .expect("resolved profile");
        assert_eq!(profile.protocol, ReasoningProtocol::OpenAiChatCompletions);
        assert_eq!(profile.support, ReasoningSupport::Optional);
        assert_eq!(profile.efforts, ["low", "medium", "high"]);
        assert!(!profile.supports_disabled);
        assert_eq!(profile.disabled_wire, None);
        assert_eq!(
            profile.preference,
            configured.map_or(ReasoningChoice::ProviderDefault, |profile| profile
                .preference)
        );
        let frozen = freeze_reasoning_profile(profile).expect("valid Step choice");
        assert_eq!(frozen.protocol, ReasoningProtocol::OpenAiChatCompletions);
        assert!(!frozen.replay_policy.requires_chat_replay_for_tools());
    }
}

#[test]
fn obsolete_step_preferences_remain_visible_and_fail_freeze_until_repaired() {
    for choice in [
        ReasoningChoice::Disabled,
        ReasoningChoice::Effort("minimal".into()),
        ReasoningChoice::Effort("xhigh".into()),
        ReasoningChoice::Effort("max".into()),
    ] {
        let profile = resolve_reasoning_profile(
            "proxy",
            ProviderApi::ChatCompletions,
            "https://gateway.example/v1",
            "step-5-preview",
            Some(old_step(choice.clone())),
        )
        .expect("repairable projection");
        assert_eq!(profile.preference, choice);
        assert_eq!(profile.efforts, ["low", "medium", "high"]);
        assert!(freeze_reasoning_profile(profile.clone()).is_err());
        for repair in [
            ReasoningChoice::ProviderDefault,
            ReasoningChoice::Effort("medium".into()),
        ] {
            let mut repaired = profile.clone();
            repaired.preference = repair.clone();
            let frozen = freeze_reasoning_profile(repaired).expect("legal repair");
            assert_eq!(frozen.choice, repair);
        }
    }
}

#[test]
fn exact_known_services_keep_strict_facts_and_lookalike_endpoints_stay_generic() {
    for (endpoint, model, protocol, policy) in [
        (
            "https://api.deepseek.com/v1/",
            "deepseek-v4-pro",
            ReasoningProtocol::DeepSeekChatCompletions,
            ReasoningReplayPolicy::DeepSeekTools,
        ),
        (
            "https://api.z.ai/api/paas/v4",
            "glm-5.3",
            ReasoningProtocol::ZhipuChatCompletions,
            ReasoningReplayPolicy::GlmPreserved,
        ),
        (
            "https://open.bigmodel.cn/api/coding/paas/v4",
            "glm-4.7",
            ReasoningProtocol::ZhipuChatCompletions,
            ReasoningReplayPolicy::GlmPreserved,
        ),
    ] {
        let profile = resolve_reasoning_profile(
            "official",
            ProviderApi::ChatCompletions,
            endpoint,
            model,
            None,
        )
        .expect("official profile");
        let frozen = freeze_reasoning_profile(profile).expect("official choice");
        assert_eq!(frozen.protocol, protocol);
        assert_eq!(frozen.replay_policy, policy);
    }
    for (endpoint, model) in [
        (
            "https://api.deepseek.com.attacker.example/v1",
            "deepseek-v4-pro",
        ),
        (
            "https://gateway.example/api.deepseek.com/v1",
            "deepseek-v4-pro",
        ),
        (
            "https://api.deepseek.com/v1?source=proxy",
            "deepseek-v4-pro",
        ),
        ("https://api.deepseek.com/v1", "deepseek-not-declared"),
        ("http://api.z.ai/api/paas/v4", "glm-5.3"),
        ("https://user@api.z.ai/api/paas/v4", "glm-5.3"),
        ("https://api.z.ai/api/paas/v4#proxy", "glm-5.3"),
        ("https://gateway.example/v1", "step-5-preview-extra"),
    ] {
        let profile = resolve_reasoning_profile(
            "custom",
            ProviderApi::ChatCompletions,
            endpoint,
            model,
            None,
        )
        .expect("generic profile");
        let frozen = freeze_reasoning_profile(profile).expect("generic default");
        assert_eq!(
            frozen.protocol,
            ReasoningProtocol::Unknown,
            "{endpoint} / {model}"
        );
        assert_eq!(frozen.choice, ReasoningChoice::ProviderDefault);
        assert!(frozen.declared_efforts.is_empty());
        assert!(!frozen.supports_disabled);
    }
}

#[test]
fn imported_custom_vendor_profiles_keep_internal_semantics_and_responses_remains_api_driven() {
    let mut imported = old_step(ReasoningChoice::Effort("high".into()));
    imported.model = "custom-glm".into();
    imported.protocol = ReasoningProtocol::ZhipuChatCompletions;
    imported.replay_profile = ReasoningReplaySetting::GlmInterleaved;
    imported.efforts = vec!["low".into(), "high".into()];
    imported.disabled_wire = Some(ReasoningDisabledWire::ThinkingTypeDisabled);
    let resolved = resolve_reasoning_profile(
        "proxy",
        ProviderApi::ChatCompletions,
        "https://gateway.example/v1",
        "custom-glm",
        Some(imported.clone()),
    )
    .expect("imported profile");
    assert_eq!(resolved, imported);
    assert_eq!(
        freeze_reasoning_profile(resolved)
            .expect("imported frozen")
            .replay_policy,
        ReasoningReplayPolicy::GlmInterleaved
    );
    let response = resolve_reasoning_profile(
        "response",
        ProviderApi::Responses,
        "https://gateway.example/v1",
        "step-5-preview",
        None,
    )
    .expect("Responses profile");
    let frozen = freeze_reasoning_profile(response).expect("Responses freeze");
    assert_eq!(frozen.protocol, ReasoningProtocol::ResponsesApi);
    assert_eq!(frozen.replay_policy, ReasoningReplayPolicy::ResponsesItems);
}
