use vega_conversation::types::ReasoningProfileProjection;
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
    capabilities: &vega_store::model_capabilities::ModelCapabilityConfig,
) -> Result<ReasoningProfileProjection, ReasoningCapabilityError> {
    if configured
        .as_ref()
        .is_some_and(|profile| profile.provider != provider || profile.model != model)
    {
        return Err(ReasoningCapabilityError::Invalid);
    }
    let definition = vega_runtime::model_compat::lookup_model_definition(
        &vega_runtime::model_compat::ModelRoute::new(
            vega_conversation::model_capabilities::api(api),
            base_url,
        )
        .map_err(|_| ReasoningCapabilityError::Invalid)?,
        &vega_runtime::model_compat::ModelIdentity {
            provider: provider.into(),
            model: model.into(),
        },
    )
    .map_err(|_| ReasoningCapabilityError::Invalid)?;
    let user_override = capabilities.record(provider, model).map(|record| {
        vega_runtime::model_compat::ModelCapabilityOverride {
            identity: definition.identity.clone(),
            allowed_efforts: record.allowed_efforts.clone(),
            image_input: record.image_input,
        }
    });
    let resolved = vega_runtime::model_compat::resolve_model(&definition, user_override.as_ref())
        .map_err(|_| ReasoningCapabilityError::Invalid)?;
    let profile = ReasoningProfileProjection::from_resolved_model(
        &resolved,
        configured.map_or(vega_runtime::ReasoningChoice::ProviderDefault, |profile| {
            profile.preference
        }),
    );
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
