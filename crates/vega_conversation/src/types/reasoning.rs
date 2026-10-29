pub use vega_runtime::{
    ChatCompatibility, FrozenReasoning, REASONING_EFFORTS, ReasoningBudgetScope, ReasoningChoice,
    ReasoningDisabledWire, ReasoningProtocol, ReasoningReplayPolicy, ReasoningSupport,
    chat_compatibility,
};

pub use vega_store::reasoning::ReasoningProfile as SessionReasoningPreference;
use vega_store::reasoning::ReasoningProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReasoningReplaySetting {
    #[default]
    Auto,
    StandardChat,
    DeepSeekTools,
    GlmPreserved,
    GlmInterleaved,
    ResponsesItems,
}

impl ReasoningReplaySetting {
    pub fn runtime_policy(self) -> Option<ReasoningReplayPolicy> {
        match self {
            Self::Auto => None,
            Self::StandardChat => Some(ReasoningReplayPolicy::StandardChat),
            Self::DeepSeekTools => Some(ReasoningReplayPolicy::DeepSeekTools),
            Self::GlmPreserved => Some(ReasoningReplayPolicy::GlmPreserved),
            Self::GlmInterleaved => Some(ReasoningReplayPolicy::GlmInterleaved),
            Self::ResponsesItems => Some(ReasoningReplayPolicy::ResponsesItems),
        }
    }
}

/// UI-safe projection of one exact provider/model profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningProfileProjection {
    pub compatibility: Option<Box<ChatCompatibility>>,
    pub model_snapshot: Option<Box<ResolvedModel>>,
    pub supports_enabled: bool,
    /// Exact provider identifier.
    pub provider: String,
    /// Exact model identifier.
    pub model: String,
    pub protocol: ReasoningProtocol,
    /// Capability support state.
    pub support: ReasoningSupport,
    pub efforts: Vec<String>,
    /// Whether a true disabled operation is available.
    pub supports_disabled: bool,
    pub disabled_wire: Option<ReasoningDisabledWire>,
    pub replay_profile: ReasoningReplaySetting,
    /// Persisted user preference.
    pub preference: ReasoningChoice,
}

impl ReasoningProfileProjection {
    pub fn from_resolved_model(model: &ResolvedModel, preference: ReasoningChoice) -> Self {
        let mut projection = Self::from_chat_compatibility(
            &model.identity.provider,
            &model.identity.model,
            model.compatibility().clone(),
        );
        projection.model_snapshot = Some(Box::new(model.clone()));
        projection.preference = preference;
        projection
    }

    pub fn from_chat_compatibility(
        provider: &str,
        model: &str,
        capabilities: ChatCompatibility,
    ) -> Self {
        let mut profile = Self::unknown(provider, model);
        profile.protocol = capabilities.protocol;
        profile.replay_profile = match capabilities.replay_policy {
            ReasoningReplayPolicy::StandardChat => ReasoningReplaySetting::StandardChat,
            ReasoningReplayPolicy::DeepSeekTools => ReasoningReplaySetting::DeepSeekTools,
            ReasoningReplayPolicy::GlmPreserved => ReasoningReplaySetting::GlmPreserved,
            ReasoningReplayPolicy::GlmInterleaved => ReasoningReplaySetting::GlmInterleaved,
            ReasoningReplayPolicy::ResponsesItems => ReasoningReplaySetting::ResponsesItems,
            ReasoningReplayPolicy::Unknown => ReasoningReplaySetting::Auto,
        };
        profile.support = capabilities.support;
        profile.efforts = capabilities.efforts.clone();
        profile.supports_disabled = capabilities.supports_disabled;
        profile.disabled_wire = capabilities.disabled_wire;
        profile.supports_enabled = capabilities.supports_enabled;
        profile.compatibility = Some(Box::new(capabilities));
        profile
    }

    /// Converts the store representation without guessing undocumented values.
    pub fn from_store(profile: &ReasoningProfile) -> Result<Self, String> {
        profile.validate()?;
        let mut projection = Self::unknown(&profile.provider, &profile.model);
        projection.preference = match profile.preference.as_str() {
            "provider_default" => ReasoningChoice::ProviderDefault,
            "disabled" => ReasoningChoice::Disabled,
            "enabled" => ReasoningChoice::Enabled,
            effort => ReasoningChoice::Effort(effort.to_string()),
        };
        Ok(projection)
    }

    pub fn supports_preference(&self) -> bool {
        match &self.preference {
            ReasoningChoice::Enabled => self.supports_enabled,
            ReasoningChoice::ProviderDefault => true,
            ReasoningChoice::Disabled => self.supports_disabled,
            ReasoningChoice::Effort(effort) => {
                matches!(
                    self.support,
                    ReasoningSupport::Required | ReasoningSupport::Optional
                ) && self.efforts.contains(effort)
            }
        }
    }

    /// Creates the immutable runtime choice used by one run.
    pub fn freeze(&self) -> Result<FrozenReasoning, vega_runtime::VegaError> {
        self.freeze_resolved(
            self.protocol,
            self.replay_profile
                .runtime_policy()
                .unwrap_or(ReasoningReplayPolicy::Unknown),
        )
    }

    pub fn freeze_resolved(
        &self,
        protocol: ReasoningProtocol,
        replay_policy: ReasoningReplayPolicy,
    ) -> Result<FrozenReasoning, vega_runtime::VegaError> {
        let frozen = FrozenReasoning {
            compatibility: self.compatibility.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            protocol,
            replay_policy,
            choice: self.preference.clone(),
            supports_disabled: self.supports_disabled,
            disabled_wire: self.disabled_wire,
            declared_efforts: self.efforts.clone(),
        };
        frozen.validate()?;
        Ok(frozen)
    }

    pub fn unknown(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            compatibility: None,
            model_snapshot: None,
            supports_enabled: false,
            provider: provider.into(),
            model: model.into(),
            protocol: ReasoningProtocol::Unknown,
            support: ReasoningSupport::Unknown,
            efforts: Vec::new(),
            supports_disabled: false,
            disabled_wire: None,
            replay_profile: ReasoningReplaySetting::Auto,
            preference: ReasoningChoice::ProviderDefault,
        }
    }
}

pub use vega_runtime::model_compat::{ModelCapabilityOverride, ModelIdentity, ResolvedModel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilityProjection {
    pub resolved: Box<ResolvedModel>,
    pub user_override: Option<ModelCapabilityOverride>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCapabilityPatch {
    Upsert(ModelCapabilityOverride),
    Reset { identity: ModelIdentity },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCapabilitySettingsErrorCode {
    Io,
    Invalid,
    Conflict,
    Busy,
    MissingModel,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCapabilitySettingsProjection {
    Loading,
    Ready {
        generation: u64,
        models: Vec<ModelCapabilityProjection>,
        error: Option<ModelCapabilitySettingsErrorCode>,
    },
    Saving {
        generation: u64,
        operation_id: u64,
        model: ModelCapabilityProjection,
    },
}
