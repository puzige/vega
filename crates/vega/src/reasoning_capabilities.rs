use vega_conversation::types::{
    ChatCompatibility, ReasoningChoice, ReasoningProfileProjection, ReasoningProtocol,
    ReasoningReplaySetting, chat_compatibility,
};
use vega_runtime::{FrozenReasoning, ReasoningReplayPolicy};
use vega_store::config::ProviderApi;

#[cfg(test)]
use vega_conversation::types::{ReasoningDisabledWire, ReasoningSupport};

#[cfg(test)]
#[path = "reasoning_capabilities_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReasoningCapabilityError {
    Invalid,
}

fn replay_setting(policy: ReasoningReplayPolicy) -> ReasoningReplaySetting {
    match policy {
        ReasoningReplayPolicy::StandardChat => ReasoningReplaySetting::StandardChat,
        ReasoningReplayPolicy::DeepSeekTools => ReasoningReplaySetting::DeepSeekTools,
        ReasoningReplayPolicy::GlmPreserved => ReasoningReplaySetting::GlmPreserved,
        ReasoningReplayPolicy::GlmInterleaved => ReasoningReplaySetting::GlmInterleaved,
        ReasoningReplayPolicy::ResponsesItems => ReasoningReplaySetting::ResponsesItems,
        ReasoningReplayPolicy::Unknown => ReasoningReplaySetting::Auto,
    }
}

fn capability_profile(
    provider: &str,
    model: &str,
    capabilities: ChatCompatibility,
) -> ReasoningProfileProjection {
    let mut profile = ReasoningProfileProjection::unknown(provider, model);
    profile.protocol = capabilities.protocol;
    profile.replay_profile = replay_setting(capabilities.replay_policy);
    profile.support = capabilities.support;
    profile.efforts = capabilities
        .efforts
        .iter()
        .map(|effort| (*effort).to_string())
        .collect();
    profile.supports_disabled = capabilities.supports_disabled;
    profile.disabled_wire = capabilities.disabled_wire;
    profile.preference = capabilities
        .default_effort
        .map_or(ReasoningChoice::ProviderDefault, |effort| {
            ReasoningChoice::Effort(effort.to_string())
        });
    profile
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
    let mut profile = if api == ProviderApi::Responses {
        let mut profile = ReasoningProfileProjection::unknown(provider, model);
        profile.protocol = ReasoningProtocol::ResponsesApi;
        profile.replay_profile = ReasoningReplaySetting::ResponsesItems;
        profile
    } else {
        chat_compatibility(Some(base_url), model).map_or_else(
            || ReasoningProfileProjection::unknown(provider, model),
            |capabilities| capability_profile(provider, model, capabilities),
        )
    };
    if let Some(configured) = configured {
        profile.preference = configured.preference;
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
