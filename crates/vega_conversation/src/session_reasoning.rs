use crate::types::{ConversationError, ReasoningProfileProjection, SessionReasoningPreference};
use vega_store::Store;

fn failure(error: impl std::fmt::Display) -> ConversationError {
    ConversationError::CorruptRow(error.to_string())
}

pub fn initialize(
    store: &Store,
    thread_id: &str,
    defaults: &[SessionReasoningPreference],
) -> Result<Vec<SessionReasoningPreference>, ConversationError> {
    for preference in defaults {
        preference.validate().map_err(failure)?;
    }
    let encoded = serde_json::to_string(defaults).map_err(failure)?;
    vega_store::threads::set_reasoning_preferences(store.conn(), thread_id, &encoded, true)
        .map_err(failure)?;
    crate::threads::get_thread(store, thread_id)?
        .reasoning_preferences
        .ok_or_else(|| failure("session reasoning is not initialized"))
}

pub fn save(
    store: &Store,
    thread_id: &str,
    candidate: &ReasoningProfileProjection,
) -> Result<Vec<SessionReasoningPreference>, ConversationError> {
    let transaction = store.immediate_transaction().map_err(failure)?;
    let mut preferences = crate::threads::get_thread(store, thread_id)?
        .reasoning_preferences
        .ok_or_else(|| failure("session reasoning is not initialized"))?;
    set_preference(&mut preferences, candidate)?;
    let encoded = serde_json::to_string(&preferences).map_err(failure)?;
    if vega_store::threads::set_reasoning_preferences(&transaction, thread_id, &encoded, false)
        .map_err(failure)?
        != 1
    {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    transaction.commit().map_err(failure)?;
    Ok(preferences)
}

pub fn set_preference(
    preferences: &mut Vec<SessionReasoningPreference>,
    candidate: &ReasoningProfileProjection,
) -> Result<(), ConversationError> {
    if !candidate.supports_preference() {
        return Err(failure("unsupported session reasoning preference"));
    }
    candidate.freeze().map_err(failure)?;
    let preference = match &candidate.preference {
        crate::types::ReasoningChoice::Enabled => "enabled".to_string(),
        crate::types::ReasoningChoice::Disabled => "disabled".to_string(),
        crate::types::ReasoningChoice::ProviderDefault => "provider_default".to_string(),
        crate::types::ReasoningChoice::Effort(effort) => effort.clone(),
    };
    let next = SessionReasoningPreference {
        provider: candidate.provider.clone(),
        model: candidate.model.clone(),
        preference,
    };
    next.validate().map_err(failure)?;
    if let Some(existing) = preferences
        .iter_mut()
        .find(|value| value.provider == candidate.provider && value.model == candidate.model)
    {
        *existing = next;
    } else {
        preferences.push(next);
    }
    Ok(())
}

pub fn project(
    mut profile: ReasoningProfileProjection,
    preferences: &[SessionReasoningPreference],
) -> Result<ReasoningProfileProjection, ConversationError> {
    profile.preference = if let Some(preference) = preferences
        .iter()
        .find(|value| value.provider == profile.provider && value.model == profile.model)
    {
        ReasoningProfileProjection::from_store(preference)
            .map_err(failure)?
            .preference
    } else {
        crate::types::ReasoningChoice::ProviderDefault
    };
    Ok(profile)
}

#[cfg(test)]
#[path = "session_reasoning_tests.rs"]
mod tests;
