//! Content-free provider settings commands and asynchronous operation identity.
pub use vega_store::config::ProviderApi;
use vega_store::config::ProviderConfig;

/// Content-free readiness failures raised before a composer submission is
/// materialized or an agent worker is started.
///
/// The provider names and model ids are configuration identifiers only. A
/// credential value never crosses this boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderPreflightFailure {
    /// The selected model has no unique, enabled, usable provider baseline.
    ProviderUnavailable {
        /// Model selected by the draft or existing conversation.
        model: String,
        /// Provider names that mention the model, if any.
        providers: Vec<String>,
    },
    /// The unique enabled provider exists, but its local credential cannot be
    /// read from the owner-only keystore.
    CredentialUnavailable {
        /// Model selected by the draft or existing conversation.
        model: String,
        /// Unique enabled provider selected for the model.
        provider: String,
    },
}

impl ProviderPreflightFailure {
    /// Projects one actionable, secret-free UI message.
    pub fn message(&self) -> String {
        match self {
            Self::ProviderUnavailable { model, providers } => {
                if providers.is_empty() {
                    vega_i18n::t_args("domain.provider.preflight_missing", &[("model", model)])
                } else {
                    vega_i18n::t_args(
                        "domain.provider.preflight_unavailable",
                        &[("model", model), ("providers", &providers.join(", "))],
                    )
                }
            }
            Self::CredentialUnavailable { model, provider } => vega_i18n::t_args(
                "domain.provider.preflight_credential",
                &[("model", model), ("provider", provider)],
            ),
        }
    }
}

/// Exact provider baseline; contains credential references, never credential values.
pub type ProviderSnapshot = ProviderConfig;

/// User-requested network action. Discovery never implies model test success.
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderNetworkAction {
    DiscoverModels,
    TestModel { model: String },
}

/// Immutable target and UI identity carried through a background operation.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderNetworkRequest {
    pub operation_id: u64,
    pub generation: u64,
    pub provider: ProviderSnapshot,
    pub action: ProviderNetworkAction,
}

/// Successful bounded network response.
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderNetworkOutcome {
    Models(Vec<String>),
    ModelTestSucceeded,
}

/// A result retains its exact request for stale-result rejection.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderNetworkResult {
    pub request: ProviderNetworkRequest,
    pub outcome: Result<ProviderNetworkOutcome, ProviderSettingsError>,
}

/// Narrow config mutations; moving validates the complete current provider order.
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderPatchAction {
    SetEnabled(bool),
    Move {
        expected_order: Vec<String>,
        ordered_names: Vec<String>,
    },
    EditModels(Vec<String>),
    ImportModels(Vec<String>),
}

/// Optimistic provider baseline and requested field mutation.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderPatchRequest {
    pub provider: ProviderSnapshot,
    pub action: ProviderPatchAction,
}

/// Stable errors never include URLs, credentials, or remote response text.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProviderSettingsError {
    #[error("{}", vega_i18n::t("domain.provider.config"))]
    Config,
    #[error("{}", vega_i18n::t("domain.provider.conflict"))]
    Conflict,
    #[error("{}", vega_i18n::t("domain.provider.invalid"))]
    Invalid,
    #[error("{}", vega_i18n::t("domain.provider.credential"))]
    Credential,
    #[error("{}", vega_i18n::t("domain.provider.disabled"))]
    Disabled,
    #[error("{}", vega_i18n::t("domain.provider.cancelled"))]
    Cancelled,
    #[error("{}", vega_i18n::t("domain.provider.timeout"))]
    Timeout,
    #[error("{}", vega_i18n::t("domain.provider.network"))]
    Network,
    #[error("{}", vega_i18n::t_args("domain.provider.http", &[("status", &.0.to_string())]))]
    Http(u16),
    #[error("{}", vega_i18n::t("domain.provider.malformed"))]
    Malformed,
    #[error("{}", vega_i18n::t("domain.provider.limit"))]
    Limit,
}
