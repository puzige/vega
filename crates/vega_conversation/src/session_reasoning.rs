use crate::types::{ConversationError, ReasoningProfileProjection, SessionReasoningPreference};
use std::sync::{Mutex, MutexGuard};
use vega_store::Store;

static PREFERENCE_COORDINATION: Mutex<()> = Mutex::new(());

fn coordinate_preferences() -> Result<MutexGuard<'static, ()>, ConversationError> {
    PREFERENCE_COORDINATION
        .lock()
        .map_err(|_| failure("session reasoning coordination unavailable"))
}

pub fn with_current_preferences<T>(
    store: &Store,
    thread_id: &str,
    operation: impl FnOnce(Option<Vec<SessionReasoningPreference>>) -> T,
) -> Result<T, ConversationError> {
    let _coordination = coordinate_preferences()?;
    let preferences = crate::threads::get_thread(store, thread_id)?.reasoning_preferences;
    Ok(operation(preferences))
}

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
    let path = store
        .database_path()
        .ok_or_else(|| failure("database path unavailable"))?;
    crate::database_writer::commit(
        path,
        store.database_identity(),
        InitializePreferences {
            thread_id: thread_id.to_owned(),
            encoded,
        },
    )
    .map_err(failure)?
}

struct InitializePreferences {
    thread_id: String,
    encoded: String,
}

impl crate::database_writer::Sealed for InitializePreferences {}

impl crate::database_writer::DatabaseCommand for InitializePreferences {
    type Output = Result<Vec<SessionReasoningPreference>, ConversationError>;

    fn apply(self, store: &Store) -> Self::Output {
        let _coordination = coordinate_preferences()?;
        let tx = store.immediate_transaction().map_err(failure)?;
        vega_store::threads::set_reasoning_preferences(&tx, &self.thread_id, &self.encoded, true)
            .map_err(failure)?;
        let preferences = crate::threads::get_thread(store, &self.thread_id)?
            .reasoning_preferences
            .ok_or_else(|| failure("session reasoning is not initialized"))?;
        tx.commit().map_err(failure)?;
        Ok(preferences)
    }
}

pub fn save(
    store: &Store,
    thread_id: &str,
    candidate: &ReasoningProfileProjection,
) -> Result<Vec<SessionReasoningPreference>, ConversationError> {
    let mut validated = Vec::new();
    set_preference(&mut validated, candidate)?;
    let preference = validated
        .pop()
        .ok_or_else(|| failure("invalid reasoning preference"))?;
    let path = store
        .database_path()
        .ok_or_else(|| failure("database path unavailable"))?;
    crate::database_writer::commit(
        path,
        store.database_identity(),
        SavePreference {
            thread_id: thread_id.to_owned(),
            preference,
        },
    )
    .map_err(failure)?
}

struct SavePreference {
    thread_id: String,
    preference: SessionReasoningPreference,
}

impl crate::database_writer::Sealed for SavePreference {}

impl crate::database_writer::DatabaseCommand for SavePreference {
    type Output = Result<Vec<SessionReasoningPreference>, ConversationError>;

    fn apply(self, store: &Store) -> Self::Output {
        let _coordination = coordinate_preferences()?;
        let transaction = store.immediate_transaction().map_err(failure)?;
        let mut preferences = crate::threads::get_thread(store, &self.thread_id)?
            .reasoning_preferences
            .ok_or_else(|| failure("session reasoning is not initialized"))?;
        if let Some(existing) = preferences.iter_mut().find(|value| {
            value.provider == self.preference.provider && value.model == self.preference.model
        }) {
            *existing = self.preference;
        } else {
            preferences.push(self.preference);
        }
        let encoded = serde_json::to_string(&preferences).map_err(failure)?;
        if vega_store::threads::set_reasoning_preferences(
            &transaction,
            &self.thread_id,
            &encoded,
            false,
        )
        .map_err(failure)?
            != 1
        {
            return Err(ConversationError::NotFound(self.thread_id));
        }
        transaction.commit().map_err(failure)?;
        Ok(preferences)
    }
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
