use super::*;

fn caller_step_profile(preference: ReasoningChoice) -> ReasoningProfileProjection {
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
fn step_capabilities_are_internal_and_only_the_selected_preference_is_applied() {
    for configured in [
        None,
        Some(caller_step_profile(ReasoningChoice::Effort("high".into()))),
    ] {
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
fn unsupported_step_preferences_remain_visible_and_fail_freeze_until_repaired() {
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
            Some(caller_step_profile(choice.clone())),
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
fn caller_capability_fields_cannot_override_internal_profiles_or_the_selected_api() {
    let mut supplied = caller_step_profile(ReasoningChoice::ProviderDefault);
    supplied.protocol = ReasoningProtocol::ZhipuChatCompletions;
    supplied.replay_profile = ReasoningReplaySetting::GlmInterleaved;
    supplied.disabled_wire = Some(ReasoningDisabledWire::ThinkingTypeDisabled);
    let resolved = resolve_reasoning_profile(
        "proxy",
        ProviderApi::ChatCompletions,
        "https://gateway.example/v1",
        "step-5-preview",
        Some(supplied.clone()),
    )
    .expect("internal model facts");
    assert_eq!(resolved.protocol, ReasoningProtocol::OpenAiChatCompletions);
    assert_eq!(resolved.efforts, ["low", "medium", "high"]);
    assert!(!resolved.supports_disabled);
    assert_eq!(resolved.preference, ReasoningChoice::ProviderDefault);
    supplied.model = "custom-glm".into();
    let generic = resolve_reasoning_profile(
        "proxy",
        ProviderApi::ChatCompletions,
        "https://gateway.example/v1",
        "custom-glm",
        Some(supplied),
    )
    .expect("generic internal profile");
    let generic = freeze_reasoning_profile(generic).expect("generic default");
    assert_eq!(generic.protocol, ReasoningProtocol::Unknown);
    assert!(generic.declared_efforts.is_empty());
    assert!(!generic.supports_disabled);
    let response = resolve_reasoning_profile(
        "proxy",
        ProviderApi::Responses,
        "https://gateway.example/v1",
        "step-5-preview",
        Some(caller_step_profile(ReasoningChoice::ProviderDefault)),
    )
    .expect("actual Responses API");
    let frozen = freeze_reasoning_profile(response).expect("Responses default");
    assert_eq!(frozen.protocol, ReasoningProtocol::ResponsesApi);
    assert_eq!(frozen.replay_policy, ReasoningReplayPolicy::ResponsesItems);
}

fn new_preference(preference: &str) -> String {
    format!(
        "version = 1\n[[preferences]]\nprovider = \"proxy\"\nmodel = \"step-5-preview\"\npreference = \"{preference}\"\n"
    )
}

#[test]
fn old_reasoning_files_never_affect_real_app_loading_or_create_preferences() {
    let old_files = [
        "version = 1\n[[profiles]]\nprovider = \"proxy\"\nmodel = \"step-5-preview\"\nprotocol = \"openai_chat_completions\"\nsupport = \"optional\"\nefforts = [\"high\"]\npreference = \"high\"\n",
        "version = 2\n[[profiles]]\nprovider = \"proxy\"\nmodel = \"step-5-preview\"\nprotocol = \"openai_chat_completions\"\nsupport = \"optional\"\nefforts = [\"max\"]\nreplay_profile = \"standard_chat\"\npreference = \"max\"\n",
        "version = [broken",
        "version = 2\n[[profiles]]\nprovider = \"proxy\"\nmodel = \"step-5-preview\"\nprotocol = \"zhipu_chat_completions\"\nsupport = \"optional\"\nefforts = [\"max\"]\nsupports_disabled = true\ndisabled_wire = \"thinking_type_disabled\"\nreplay_profile = \"glm_interleaved\"\npreference = \"max\"\n",
    ];
    for source in old_files {
        let root = tempfile::tempdir().expect("owned app configuration");
        let old_path = root.path().join("reasoning.toml");
        let preferences_path = root.path().join("reasoning-preferences.toml");
        std::fs::write(&old_path, source).expect("untrusted old file");
        let runtime_path = root
            .path()
            .join("config.toml")
            .with_file_name(vega_store::reasoning::REASONING_FILE_NAME);
        let frozen = crate::app_agent::reasoning_for_provider_model(
            "proxy",
            ProviderApi::ChatCompletions,
            "https://gateway.example/v1",
            "step-5-preview",
            Some(&runtime_path),
        )
        .expect("old file cannot block loading");
        assert_eq!(frozen.choice, ReasoningChoice::ProviderDefault);
        assert_eq!(frozen.declared_efforts, ["low", "medium", "high"]);
        assert_eq!(frozen.protocol, ReasoningProtocol::OpenAiChatCompletions);
        assert!(!frozen.supports_disabled);
        assert_eq!(
            std::fs::read_to_string(&old_path).expect("old bytes"),
            source
        );
        assert!(!preferences_path.exists());
        assert_eq!(
            std::fs::read_dir(root.path())
                .expect("fixture directory")
                .count(),
            1
        );
    }
}

#[test]
fn real_app_loads_only_new_preferences_and_does_not_rewrite_either_file() {
    let root = tempfile::tempdir().expect("owned new preferences");
    let old_path = root.path().join("reasoning.toml");
    let new_path = root.path().join("reasoning-preferences.toml");
    let old = "injected = [broken legacy configuration";
    let preferences = new_preference("high");
    std::fs::write(&old_path, old).expect("irrelevant old file");
    std::fs::write(&new_path, &preferences).expect("new preference file");
    let runtime_path = root
        .path()
        .join("config.toml")
        .with_file_name(vega_store::reasoning::REASONING_FILE_NAME);
    let frozen = crate::app_agent::reasoning_for_provider_model(
        "proxy",
        ProviderApi::ChatCompletions,
        "https://gateway.example/v1",
        "step-5-preview",
        Some(&runtime_path),
    )
    .expect("new preference loading");
    assert_eq!(frozen.choice, ReasoningChoice::Effort("high".into()));
    assert_eq!(frozen.declared_efforts, ["low", "medium", "high"]);
    assert!(!frozen.supports_disabled);
    assert_eq!(
        std::fs::read_to_string(&old_path).expect("old unchanged"),
        old
    );
    assert_eq!(
        std::fs::read_to_string(&new_path).expect("new unchanged"),
        preferences
    );
}
