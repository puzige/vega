use vega_conversation::types::{
    ReasoningChoice, ReasoningDisabledWire, ReasoningProfileProjection, ReasoningProtocol,
    ReasoningReplaySetting, ReasoningSupport,
};
use vega_runtime::{FrozenReasoning, ReasoningReplayPolicy};
use vega_store::config::ProviderApi;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReasoningCapabilityError {
    Invalid,
    Conflict,
}

#[derive(Clone, PartialEq, Eq)]
struct EndpointKey {
    username: String,
    password: Option<String>,
    host: String,
    port: Option<u16>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

impl EndpointKey {
    fn parse(value: &str) -> Option<Self> {
        let url = reqwest::Url::parse(value).ok()?;
        if url.scheme() != "https" {
            return None;
        }
        let host = url.host_str()?.to_string();
        let mut path = url.path().to_string();
        if path.ends_with('/') {
            path.pop();
        }
        Some(Self {
            username: url.username().to_string(),
            password: url.password().map(str::to_string),
            host,
            port: url.port(),
            path,
            query: url.query().map(str::to_string),
            fragment: url.fragment().map(str::to_string),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CapabilitySeed {
    protocol: ReasoningProtocol,
    policy: ReasoningReplayPolicy,
}

fn registry_seed(api: ProviderApi, base_url: &str, model: &str) -> Option<CapabilitySeed> {
    let endpoint = EndpointKey::parse(base_url)?;
    let deepseek_endpoint = ["https://api.deepseek.com", "https://api.deepseek.com/v1"]
        .into_iter()
        .filter_map(EndpointKey::parse)
        .any(|candidate| candidate == endpoint);
    if api == ProviderApi::ChatCompletions
        && deepseek_endpoint
        && matches!(model, "deepseek-flash" | "deepseek-v4-pro")
    {
        return Some(CapabilitySeed {
            protocol: ReasoningProtocol::DeepSeekChatCompletions,
            policy: ReasoningReplayPolicy::DeepSeekTools,
        });
    }
    let glm_endpoint = [
        "https://api.z.ai/api/paas/v4",
        "https://api.z.ai/api/coding/paas/v4",
        "https://open.bigmodel.cn/api/paas/v4",
        "https://open.bigmodel.cn/api/coding/paas/v4",
    ]
    .into_iter()
    .filter_map(EndpointKey::parse)
    .any(|candidate| candidate == endpoint);
    if api == ProviderApi::ChatCompletions
        && glm_endpoint
        && matches!(model, "glm-4.7" | "glm-5.3" | "glm-5.3-flash")
    {
        return Some(CapabilitySeed {
            protocol: ReasoningProtocol::ZhipuChatCompletions,
            policy: ReasoningReplayPolicy::GlmPreserved,
        });
    }
    None
}

fn seed_profile(provider: &str, model: &str, seed: CapabilitySeed) -> ReasoningProfileProjection {
    let mut profile = ReasoningProfileProjection::unknown(provider, model);
    profile.protocol = seed.protocol;
    profile.replay_profile = match seed.policy {
        ReasoningReplayPolicy::StandardChat => ReasoningReplaySetting::StandardChat,
        ReasoningReplayPolicy::DeepSeekTools => ReasoningReplaySetting::DeepSeekTools,
        ReasoningReplayPolicy::GlmPreserved => ReasoningReplaySetting::GlmPreserved,
        ReasoningReplayPolicy::GlmInterleaved => ReasoningReplaySetting::GlmInterleaved,
        ReasoningReplayPolicy::ResponsesItems => ReasoningReplaySetting::ResponsesItems,
        ReasoningReplayPolicy::LegacyTextTools => ReasoningReplaySetting::LegacyTextTools,
        ReasoningReplayPolicy::Unknown => ReasoningReplaySetting::Auto,
    };
    match seed.policy {
        ReasoningReplayPolicy::DeepSeekTools => {
            profile.support = ReasoningSupport::Optional;
            profile.efforts = ["low", "high", "max"]
                .into_iter()
                .map(str::to_string)
                .collect();
            profile.supports_disabled = true;
            profile.disabled_wire = Some(ReasoningDisabledWire::ThinkingTypeDisabled);
        }
        ReasoningReplayPolicy::GlmPreserved | ReasoningReplayPolicy::GlmInterleaved => {
            let forced = matches!(model, "glm-5.3" | "glm-5.3-flash");
            profile.support = if forced {
                ReasoningSupport::Required
            } else {
                ReasoningSupport::Optional
            };
            profile.efforts = ["low", "high", "max"]
                .into_iter()
                .map(str::to_string)
                .collect();
            profile.preference = ReasoningChoice::Effort("high".to_string());
            if !forced {
                profile.supports_disabled = true;
                profile.disabled_wire = Some(ReasoningDisabledWire::ThinkingTypeDisabled);
            }
        }
        ReasoningReplayPolicy::ResponsesItems => {
            profile.protocol = ReasoningProtocol::ResponsesApi;
            profile.replay_profile = ReasoningReplaySetting::ResponsesItems;
        }
        ReasoningReplayPolicy::StandardChat
        | ReasoningReplayPolicy::LegacyTextTools
        | ReasoningReplayPolicy::Unknown => {}
    }
    profile
}

fn supports_seed_preference(
    profile: &ReasoningProfileProjection,
    seed: CapabilitySeed,
    existing: &ReasoningProfileProjection,
) -> Result<ReasoningProfileProjection, ReasoningCapabilityError> {
    let mut merged = profile.clone();
    if matches!(
        seed.policy,
        ReasoningReplayPolicy::DeepSeekTools
            | ReasoningReplayPolicy::GlmPreserved
            | ReasoningReplayPolicy::GlmInterleaved
    ) {
        let glm_forced = seed.protocol == ReasoningProtocol::ZhipuChatCompletions
            && matches!(existing.model.as_str(), "glm-5.3" | "glm-5.3-flash");
        let invalid_effort = existing.efforts.iter().any(|effort| {
            if seed.protocol == ReasoningProtocol::DeepSeekChatCompletions {
                !matches!(
                    effort.as_str(),
                    "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                )
            } else if glm_forced {
                !matches!(effort.as_str(), "low" | "high" | "max")
            } else {
                false
            }
        });
        if invalid_effort {
            return Err(ReasoningCapabilityError::Conflict);
        }
        if glm_forced
            && (existing.preference == ReasoningChoice::Disabled
                || existing.supports_disabled
                || existing.disabled_wire.is_some())
        {
            return Err(ReasoningCapabilityError::Conflict);
        }
        if seed.protocol == ReasoningProtocol::ZhipuChatCompletions
            && !glm_forced
            && existing.supports_disabled
            && existing.disabled_wire != Some(ReasoningDisabledWire::ThinkingTypeDisabled)
        {
            return Err(ReasoningCapabilityError::Conflict);
        }
        let declared = if existing.efforts.is_empty() {
            Vec::new()
        } else {
            existing.efforts.clone()
        };
        if !declared.is_empty() {
            merged.efforts = declared;
        }
        if seed.protocol == ReasoningProtocol::ZhipuChatCompletions
            && !glm_forced
            && existing.support != ReasoningSupport::Unknown
        {
            merged.support = existing.support;
        }
        if seed.policy == ReasoningReplayPolicy::GlmPreserved
            || seed.policy == ReasoningReplayPolicy::GlmInterleaved
        {
            merged.supports_disabled = !glm_forced && existing.supports_disabled;
            merged.disabled_wire = if merged.supports_disabled {
                existing
                    .disabled_wire
                    .or(Some(ReasoningDisabledWire::ThinkingTypeDisabled))
            } else {
                None
            };
        }
        merged.preference = match &existing.preference {
            ReasoningChoice::Effort(effort) if merged.efforts.contains(effort) => {
                ReasoningChoice::Effort(effort.clone())
            }
            ReasoningChoice::ProviderDefault => ReasoningChoice::ProviderDefault,
            ReasoningChoice::Disabled if merged.supports_disabled => ReasoningChoice::Disabled,
            ReasoningChoice::Disabled => return Err(ReasoningCapabilityError::Conflict),
            ReasoningChoice::Effort(_) => return Err(ReasoningCapabilityError::Conflict),
        };
    } else {
        merged.efforts = existing.efforts.clone();
        merged.support = existing.support;
        merged.supports_disabled = existing.supports_disabled;
        merged.disabled_wire = existing.disabled_wire;
        merged.preference = existing.preference.clone();
    }
    Ok(merged)
}

pub(crate) fn reconcile_catalog_profile(
    catalog: &ReasoningProfileProjection,
    configured: Option<ReasoningProfileProjection>,
) -> Result<ReasoningProfileProjection, ReasoningCapabilityError> {
    let Some(configured) = configured else {
        return Ok(catalog.clone());
    };
    if configured.provider != catalog.provider || configured.model != catalog.model {
        return Err(ReasoningCapabilityError::Invalid);
    }
    let seed = match catalog.replay_profile {
        ReasoningReplaySetting::DeepSeekTools => Some(CapabilitySeed {
            protocol: ReasoningProtocol::DeepSeekChatCompletions,
            policy: ReasoningReplayPolicy::DeepSeekTools,
        }),
        ReasoningReplaySetting::GlmPreserved => Some(CapabilitySeed {
            protocol: ReasoningProtocol::ZhipuChatCompletions,
            policy: ReasoningReplayPolicy::GlmPreserved,
        }),
        ReasoningReplaySetting::GlmInterleaved => Some(CapabilitySeed {
            protocol: ReasoningProtocol::ZhipuChatCompletions,
            policy: ReasoningReplayPolicy::GlmInterleaved,
        }),
        ReasoningReplaySetting::ResponsesItems => Some(CapabilitySeed {
            protocol: ReasoningProtocol::ResponsesApi,
            policy: ReasoningReplayPolicy::ResponsesItems,
        }),
        _ => None,
    };
    let Some(mut seed) = seed else {
        return Ok(configured);
    };
    if let Some(policy) = explicit_policy(configured.replay_profile) {
        if !compatible_policy(seed.protocol, policy) {
            return Err(ReasoningCapabilityError::Conflict);
        }
        seed.policy = policy;
    }
    let mut reconciled = supports_seed_preference(catalog, seed, &configured)?;
    reconciled.protocol = seed.protocol;
    reconciled.replay_profile = match seed.policy {
        ReasoningReplayPolicy::StandardChat => ReasoningReplaySetting::StandardChat,
        ReasoningReplayPolicy::DeepSeekTools => ReasoningReplaySetting::DeepSeekTools,
        ReasoningReplayPolicy::GlmPreserved => ReasoningReplaySetting::GlmPreserved,
        ReasoningReplayPolicy::GlmInterleaved => ReasoningReplaySetting::GlmInterleaved,
        ReasoningReplayPolicy::ResponsesItems => ReasoningReplaySetting::ResponsesItems,
        ReasoningReplayPolicy::LegacyTextTools => ReasoningReplaySetting::LegacyTextTools,
        ReasoningReplayPolicy::Unknown => ReasoningReplaySetting::Auto,
    };
    Ok(reconciled)
}

fn explicit_policy(setting: ReasoningReplaySetting) -> Option<ReasoningReplayPolicy> {
    match setting {
        ReasoningReplaySetting::Auto | ReasoningReplaySetting::LegacyTextTools => None,
        _ => setting.runtime_policy(),
    }
}

fn compatible_policy(protocol: ReasoningProtocol, policy: ReasoningReplayPolicy) -> bool {
    match policy {
        ReasoningReplayPolicy::Unknown => true,
        ReasoningReplayPolicy::StandardChat => protocol == ReasoningProtocol::OpenAiChatCompletions,
        ReasoningReplayPolicy::DeepSeekTools => {
            protocol == ReasoningProtocol::DeepSeekChatCompletions
        }
        ReasoningReplayPolicy::GlmPreserved | ReasoningReplayPolicy::GlmInterleaved => {
            protocol == ReasoningProtocol::ZhipuChatCompletions
        }
        ReasoningReplayPolicy::ResponsesItems => protocol == ReasoningProtocol::ResponsesApi,
        ReasoningReplayPolicy::LegacyTextTools => true,
    }
}

pub(crate) fn resolve_reasoning_profile(
    provider: &str,
    api: ProviderApi,
    base_url: &str,
    model: &str,
    configured: Option<ReasoningProfileProjection>,
) -> Result<ReasoningProfileProjection, ReasoningCapabilityError> {
    if configured
        .as_ref()
        .is_some_and(|profile| profile.provider != provider || profile.model != model)
    {
        return Err(ReasoningCapabilityError::Invalid);
    }
    if api == ProviderApi::Responses {
        let seed = CapabilitySeed {
            protocol: ReasoningProtocol::ResponsesApi,
            policy: ReasoningReplayPolicy::ResponsesItems,
        };
        if let Some(profile) = &configured {
            if !matches!(
                profile.protocol,
                ReasoningProtocol::Unknown | ReasoningProtocol::ResponsesApi
            ) && profile.replay_profile != ReasoningReplaySetting::LegacyTextTools
            {
                return Err(ReasoningCapabilityError::Conflict);
            }
            if explicit_policy(profile.replay_profile)
                .is_some_and(|policy| policy != ReasoningReplayPolicy::ResponsesItems)
            {
                return Err(ReasoningCapabilityError::Conflict);
            }
            if !matches!(
                profile.replay_profile,
                ReasoningReplaySetting::Auto
                    | ReasoningReplaySetting::LegacyTextTools
                    | ReasoningReplaySetting::ResponsesItems
            ) {
                return Err(ReasoningCapabilityError::Conflict);
            }
        }
        let base = seed_profile(provider, model, seed);
        if let Some(profile) = configured {
            return supports_seed_preference(&base, seed, &profile);
        }
        return Ok(base);
    }
    if let Some(mut seed) = registry_seed(api, base_url, model) {
        let configured_policy = configured
            .as_ref()
            .and_then(|profile| explicit_policy(profile.replay_profile));
        if configured_policy.is_some_and(|policy| !compatible_policy(seed.protocol, policy)) {
            return Err(ReasoningCapabilityError::Conflict);
        }
        seed.policy = configured_policy.unwrap_or(seed.policy);
        let base = seed_profile(provider, model, seed);
        return match configured {
            Some(profile) => supports_seed_preference(&base, seed, &profile),
            None => Ok(base),
        };
    }
    let Some(profile) = configured else {
        return Ok(ReasoningProfileProjection::unknown(provider, model));
    };
    if explicit_policy(profile.replay_profile)
        .is_some_and(|policy| !compatible_policy(profile.protocol, policy))
    {
        return Err(ReasoningCapabilityError::Conflict);
    }
    Ok(profile)
}

pub(crate) fn freeze_reasoning_profile(
    profile: ReasoningProfileProjection,
) -> Result<FrozenReasoning, ReasoningCapabilityError> {
    let replay_policy = profile
        .replay_profile
        .runtime_policy()
        .unwrap_or(ReasoningReplayPolicy::Unknown);
    profile
        .freeze_resolved(profile.protocol, replay_policy)
        .map_err(|_| ReasoningCapabilityError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::StreamExt;
    use vega_runtime::{ChatMessage, ChatRole, ChatToolCall, OpenAiProvider, Provider};

    fn route(api: ProviderApi, endpoint: &str, model: &str) -> ReasoningProfileProjection {
        resolve_reasoning_profile("custom", api, endpoint, model, None).unwrap()
    }

    #[test]
    fn exact_registry_normalizes_only_endpoint_host_default_port_and_terminal_slash() {
        for endpoint in [
            "https://API.DEEPSEEK.COM:443/",
            "https://api.deepseek.com/v1/",
        ] {
            let profile = route(ProviderApi::ChatCompletions, endpoint, "deepseek-v4-pro");
            assert_eq!(profile.protocol, ReasoningProtocol::DeepSeekChatCompletions);
            assert_eq!(
                profile.replay_profile,
                ReasoningReplaySetting::DeepSeekTools
            );
        }
        for endpoint in [
            "https://api.deepseek.com/v1?route=custom",
            "https://user@api.deepseek.com/v1",
            "https://api.deepseek.com/v1/extra",
            "https://api.deepseek.com.evil.invalid/v1",
        ] {
            let profile = route(ProviderApi::ChatCompletions, endpoint, "deepseek-v4-pro");
            assert_eq!(profile.protocol, ReasoningProtocol::Unknown);
        }
    }

    #[test]
    fn sensitive_endpoint_components_are_not_normalized_or_exposed() {
        for endpoint in [
            "https://user:password-canary@api.deepseek.com/v1",
            "https://api.deepseek.com/v1?token=query-canary",
            "https://api.deepseek.com/v1#fragment-canary",
        ] {
            let profile = route(ProviderApi::ChatCompletions, endpoint, "deepseek-v4-pro");
            assert_eq!(profile.protocol, ReasoningProtocol::Unknown);
            let diagnostic = format!("{:?}", ReasoningCapabilityError::Conflict);
            assert!(!diagnostic.contains("canary"));
        }
    }

    #[test]
    fn registry_uses_exact_api_and_model_pairs_and_does_not_trust_provider_name() {
        let glm = route(
            ProviderApi::ChatCompletions,
            "https://open.bigmodel.cn/api/coding/paas/v4",
            "glm-5.3",
        );
        assert_eq!(glm.protocol, ReasoningProtocol::ZhipuChatCompletions);
        assert_eq!(glm.replay_profile, ReasoningReplaySetting::GlmPreserved);
        for (api, endpoint, model) in [
            (
                ProviderApi::ChatCompletions,
                "https://api.deepseek.com",
                "deepseek-v4-pro-preview",
            ),
            (
                ProviderApi::Responses,
                "https://api.deepseek.com",
                "deepseek-v4-pro",
            ),
        ] {
            let profile = route(api, endpoint, model);
            assert_ne!(
                profile.replay_profile,
                ReasoningReplaySetting::DeepSeekTools
            );
        }
    }

    #[tokio::test]
    async fn automatic_replay_migrated_v1_custom_true_resolves_validates_and_replays_on_the_chat_wire()
     {
        let config_dir = tempfile::tempdir().unwrap();
        let reasoning_path = config_dir.path().join("reasoning.toml");
        std::fs::write(
            &reasoning_path,
            r#"version = 1

[[profiles]]
provider = "custom"
model = "migrated-model"
protocol = "unknown"
support = "unknown"
preserve_reasoning_content = true
"#,
        )
        .unwrap();
        let stored = vega_store::reasoning::read_from(&reasoning_path).unwrap();
        let projection = ReasoningProfileProjection::from_store(
            stored.profile("custom", "migrated-model").unwrap(),
        )
        .unwrap();
        let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
        let capture = captured.clone();
        let frozen = resolve_reasoning_profile(
            "custom",
            ProviderApi::ChatCompletions,
            "https://fixture.invalid",
            "migrated-model",
            Some(projection),
        )
        .and_then(freeze_reasoning_profile)
        .unwrap();
        frozen.validate().unwrap();
        let provider = OpenAiProvider::new("https://fixture.invalid", "test-key")
            .unwrap()
            .with_test_sse_responder(move |request| {
                *capture.lock().unwrap() = Some(
                    serde_json::from_slice::<serde_json::Value>(
                        request.body().unwrap().as_bytes().unwrap(),
                    )
                    .unwrap(),
                );
                "data: [DONE]\n\n".into()
            });
        let mut request = ChatMessage::assistant_with_tools(
            "working",
            vec![ChatToolCall {
                id: "call-1".into(),
                name: "read".into(),
                input_json: "{}".into(),
            }],
        );
        request.reasoning_content = Some("migrated private reasoning".into());
        let mut stream = provider
            .chat_stream_once(
                vega_runtime::ChatRequest {
                    model: "migrated-model".into(),
                    messages: vec![ChatMessage::new(ChatRole::User, "continue"), request],
                    reasoning: Some(frozen),
                    ..Default::default()
                },
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        let body = captured.lock().unwrap().take().unwrap();
        assert_eq!(body["model"], "migrated-model");
        assert_eq!(
            body["messages"][1]["reasoning_content"],
            "migrated private reasoning"
        );
        assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "call-1");
        assert!(body.get("thinking").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn explicit_custom_policy_wins_but_conflicts_with_known_routes_fail_closed() {
        let mut custom = ReasoningProfileProjection::unknown("custom", "relay-model");
        custom.protocol = ReasoningProtocol::ZhipuChatCompletions;
        custom.replay_profile = ReasoningReplaySetting::GlmInterleaved;
        assert_eq!(
            resolve_reasoning_profile(
                "custom",
                ProviderApi::ChatCompletions,
                "https://relay.invalid/v1",
                "relay-model",
                Some(custom),
            )
            .unwrap()
            .replay_profile,
            ReasoningReplaySetting::GlmInterleaved
        );
        let mut conflict = ReasoningProfileProjection::unknown("custom", "deepseek-v4-pro");
        conflict.protocol = ReasoningProtocol::OpenAiChatCompletions;
        conflict.replay_profile = ReasoningReplaySetting::StandardChat;
        assert_eq!(
            resolve_reasoning_profile(
                "custom",
                ProviderApi::ChatCompletions,
                "https://api.deepseek.com",
                "deepseek-v4-pro",
                Some(conflict),
            ),
            Err(ReasoningCapabilityError::Conflict)
        );
    }

    #[test]
    fn legacy_migration_is_below_known_route_and_keeps_custom_wire_fallback() {
        let mut old_custom = ReasoningProfileProjection::unknown("relay", "model");
        old_custom.replay_profile = ReasoningReplaySetting::LegacyTextTools;
        let migrated = resolve_reasoning_profile(
            "relay",
            ProviderApi::ChatCompletions,
            "https://relay.invalid/v1",
            "model",
            Some(old_custom),
        )
        .unwrap();
        assert_eq!(
            migrated.replay_profile,
            ReasoningReplaySetting::LegacyTextTools
        );
        let mut old_known = ReasoningProfileProjection::unknown("relay", "deepseek-v4-pro");
        old_known.replay_profile = ReasoningReplaySetting::LegacyTextTools;
        let known = resolve_reasoning_profile(
            "relay",
            ProviderApi::ChatCompletions,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            Some(old_known),
        )
        .unwrap();
        assert_eq!(known.replay_profile, ReasoningReplaySetting::DeepSeekTools);
    }

    #[test]
    fn automatic_replay_v1_true_and_false_declarations_cannot_override_exact_routes() {
        let config_dir = tempfile::tempdir().unwrap();
        let path = config_dir.path().join("reasoning.toml");
        for legacy in [true, false] {
            std::fs::write(
                &path,
                format!(
                    "version = 1\n\n[[profiles]]\nprovider = \"relay\"\nmodel = \"deepseek-v4-pro\"\nprotocol = \"unknown\"\nsupport = \"unknown\"\npreserve_reasoning_content = {legacy}\n"
                ),
            )
            .unwrap();
            let stored = vega_store::reasoning::read_from(&path).unwrap();
            let old = ReasoningProfileProjection::from_store(
                stored.profile("relay", "deepseek-v4-pro").unwrap(),
            )
            .unwrap();
            let deepseek = resolve_reasoning_profile(
                "relay",
                ProviderApi::ChatCompletions,
                "https://api.deepseek.com",
                "deepseek-v4-pro",
                Some(old.clone()),
            )
            .unwrap();
            assert_eq!(
                deepseek.replay_profile,
                ReasoningReplaySetting::DeepSeekTools
            );
            assert_eq!(
                deepseek.protocol,
                ReasoningProtocol::DeepSeekChatCompletions
            );

            let mut responses_old = old;
            responses_old.model = "responses-model".into();
            let responses = resolve_reasoning_profile(
                "relay",
                ProviderApi::Responses,
                "https://relay.invalid/v1",
                "responses-model",
                Some(responses_old),
            )
            .unwrap();
            assert_eq!(responses.protocol, ReasoningProtocol::ResponsesApi);
            assert_eq!(
                responses.replay_profile,
                ReasoningReplaySetting::ResponsesItems
            );
        }
    }

    #[test]
    fn known_route_reconciles_legacy_preferences_without_promoting_legacy_policy() {
        let mut legacy_deepseek = ReasoningProfileProjection::unknown("relay", "deepseek-v4-pro");
        legacy_deepseek.replay_profile = ReasoningReplaySetting::LegacyTextTools;
        legacy_deepseek.efforts = vec!["medium".into()];
        legacy_deepseek.preference = ReasoningChoice::Effort("medium".into());
        let resolved = resolve_reasoning_profile(
            "relay",
            ProviderApi::ChatCompletions,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            Some(legacy_deepseek),
        )
        .unwrap();
        assert_eq!(
            resolved.replay_profile,
            ReasoningReplaySetting::DeepSeekTools
        );
        assert_eq!(
            resolved.preference,
            ReasoningChoice::Effort("medium".into())
        );
        assert_eq!(resolved.efforts, ["medium"]);
    }

    #[test]
    fn glm_disabled_wire_and_preferences_are_preserved_only_when_supported() {
        let mut glm47 = ReasoningProfileProjection::unknown("zhipu", "glm-4.7");
        glm47.protocol = ReasoningProtocol::ZhipuChatCompletions;
        glm47.support = ReasoningSupport::Optional;
        glm47.efforts = vec!["low".into(), "high".into(), "max".into()];
        glm47.supports_disabled = true;
        glm47.disabled_wire = Some(ReasoningDisabledWire::ThinkingTypeDisabled);
        glm47.preference = ReasoningChoice::Disabled;
        let resolved = resolve_reasoning_profile(
            "zhipu",
            ProviderApi::ChatCompletions,
            "https://open.bigmodel.cn/api/paas/v4",
            "glm-4.7",
            Some(glm47),
        )
        .unwrap();
        assert_eq!(resolved.preference, ReasoningChoice::Disabled);
        assert!(resolved.supports_disabled);
        assert_eq!(
            resolved.disabled_wire,
            Some(ReasoningDisabledWire::ThinkingTypeDisabled)
        );
        let mut glm53 = resolved;
        glm53.model = "glm-5.3".into();
        assert!(
            resolve_reasoning_profile(
                "zhipu",
                ProviderApi::ChatCompletions,
                "https://open.bigmodel.cn/api/paas/v4",
                "glm-5.3",
                Some(glm53),
            )
            .is_err()
        );
    }

    #[test]
    fn explicit_glm_interleaved_authority_survives_catalog_reconciliation() {
        let catalog = route(
            ProviderApi::ChatCompletions,
            "https://open.bigmodel.cn/api/paas/v4",
            "glm-4.7",
        );
        let mut configured = catalog.clone();
        configured.replay_profile = ReasoningReplaySetting::GlmInterleaved;
        let reconciled = reconcile_catalog_profile(&catalog, Some(configured)).unwrap();
        assert_eq!(
            reconciled.replay_profile,
            ReasoningReplaySetting::GlmInterleaved
        );
        assert_eq!(
            freeze_reasoning_profile(reconciled).unwrap().replay_policy,
            ReasoningReplayPolicy::GlmInterleaved
        );
    }

    #[test]
    fn responses_transport_always_resolves_to_response_items_and_rejects_other_templates() {
        let profile = route(ProviderApi::Responses, "https://relay.invalid/v1", "model");
        assert_eq!(profile.protocol, ReasoningProtocol::ResponsesApi);
        assert_eq!(
            profile.replay_profile,
            ReasoningReplaySetting::ResponsesItems
        );
        let mut invalid = ReasoningProfileProjection::unknown("custom", "model");
        invalid.protocol = ReasoningProtocol::OpenAiChatCompletions;
        invalid.replay_profile = ReasoningReplaySetting::StandardChat;
        assert_eq!(
            resolve_reasoning_profile(
                "custom",
                ProviderApi::Responses,
                "https://relay.invalid/v1",
                "model",
                Some(invalid),
            ),
            Err(ReasoningCapabilityError::Conflict)
        );
    }
}
