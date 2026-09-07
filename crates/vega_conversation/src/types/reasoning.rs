//! Shared typed projection for provider/model thinking capabilities.
//!
//! The runtime owns the headless frozen request type because the dependency
//! direction cannot be reversed. Re-exporting it here keeps UI consumers on
//! the existing conversation/types boundary; persistence remains raw store
//! data and is converted at this seam.

pub use vega_runtime::{
    FrozenReasoning, ReasoningBudgetScope, ReasoningChoice, ReasoningDisabledWire,
    ReasoningProtocol,
};

use vega_store::reasoning::ReasoningProfile;

/// Explicit capability support state presented by Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningSupport {
    /// Provider always enables thinking for this profile.
    Required,
    /// Provider allows either provider default or a declared choice.
    Optional,
    /// Provider does not support thinking controls.
    Unsupported,
    /// No protocol/capability declaration is available.
    Unknown,
}

/// UI-safe projection of one exact provider/model profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningProfileProjection {
    /// Exact provider identifier.
    pub provider: String,
    /// Exact model identifier.
    pub model: String,
    /// Explicit protocol declaration.
    pub protocol: ReasoningProtocol,
    /// Capability support state.
    pub support: ReasoningSupport,
    /// Declared effort subset.
    pub efforts: Vec<String>,
    /// Whether a true disabled operation is available.
    pub supports_disabled: bool,
    /// Explicit disabled operation, if any.
    pub disabled_wire: Option<ReasoningDisabledWire>,
    /// Whether original reasoning content must be replayed through tools.
    pub preserve_reasoning_content: bool,
    /// Persisted user preference.
    pub preference: ReasoningChoice,
}

impl ReasoningProfileProjection {
    /// Converts the store representation without guessing undocumented values.
    pub fn from_store(profile: &ReasoningProfile) -> Result<Self, String> {
        profile.validate()?;
        let protocol = match profile.protocol.as_str() {
            "openai_chat_completions" => ReasoningProtocol::OpenAiChatCompletions,
            "zhipu_chat_completions" => ReasoningProtocol::ZhipuChatCompletions,
            "unknown" => ReasoningProtocol::Unknown,
            _ => return Err("reasoning protocol is not declared".to_string()),
        };
        let support = match profile.support.as_str() {
            "required" => ReasoningSupport::Required,
            "optional" => ReasoningSupport::Optional,
            "unsupported" => ReasoningSupport::Unsupported,
            "unknown" => ReasoningSupport::Unknown,
            _ => return Err("reasoning support is not declared".to_string()),
        };
        let disabled_wire = match profile.disabled_wire.as_deref() {
            Some("thinking_type_disabled") => Some(ReasoningDisabledWire::ThinkingTypeDisabled),
            Some("reasoning_effort_none") => Some(ReasoningDisabledWire::ReasoningEffortNone),
            None => None,
            Some(_) => return Err("reasoning disabled wire is not declared".to_string()),
        };
        let preference = match profile.preference.as_str() {
            "provider_default" => ReasoningChoice::ProviderDefault,
            "disabled" => ReasoningChoice::Disabled,
            effort => ReasoningChoice::Effort(effort.to_string()),
        };
        Ok(Self {
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            protocol,
            support,
            efforts: profile.efforts.clone(),
            supports_disabled: profile.supports_disabled,
            disabled_wire,
            preserve_reasoning_content: profile.preserve_reasoning_content,
            preference,
        })
    }

    /// Creates the immutable runtime choice used by one run.
    pub fn freeze(&self) -> Result<FrozenReasoning, vega_runtime::VegaError> {
        let frozen = FrozenReasoning {
            provider: self.provider.clone(),
            model: self.model.clone(),
            protocol: self.protocol,
            choice: self.preference.clone(),
            supports_disabled: self.supports_disabled,
            preserve_reasoning_content: self.preserve_reasoning_content,
            disabled_wire: self.disabled_wire,
            declared_efforts: self.efforts.clone(),
        };
        frozen.validate()?;
        Ok(frozen)
    }

    /// Unknown profile projection used when no exact provider/model record is
    /// present. The UI must show provider default rather than off.
    pub fn unknown(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            protocol: ReasoningProtocol::Unknown,
            support: ReasoningSupport::Unknown,
            efforts: Vec::new(),
            supports_disabled: false,
            disabled_wire: None,
            preserve_reasoning_content: false,
            preference: ReasoningChoice::ProviderDefault,
        }
    }
}
