use vega_conversation::types::{
    ReasoningProfileProjection, ReasoningProtocol, ReasoningReplaySetting, chat_compatibility,
};
use vega_runtime::{FrozenReasoning, ReasoningReplayPolicy};
use vega_store::config::ProviderApi;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReasoningCapabilityError {
    Invalid,
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
        profile.support = vega_runtime::ReasoningSupport::Optional;
        profile.efforts = vega_runtime::REASONING_EFFORTS
            .iter()
            .map(|effort| (*effort).into())
            .collect();
        profile
    } else {
        chat_compatibility(Some(base_url), model).map_or_else(
            || ReasoningProfileProjection::unknown(provider, model),
            |capabilities| {
                ReasoningProfileProjection::from_chat_compatibility(provider, model, capabilities)
            },
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
