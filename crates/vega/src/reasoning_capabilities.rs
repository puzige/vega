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
