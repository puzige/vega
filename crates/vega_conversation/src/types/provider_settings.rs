//! Content-free provider settings commands and asynchronous operation identity.
use vega_store::config::ProviderConfig;

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
    #[error("配置读取或保存失败，请重试")]
    Config,
    #[error("供应商配置已更改，请重新读取")]
    Conflict,
    #[error("配置或模型 ID 无效")]
    Invalid,
    #[error("本地凭据不存在或无法安全读取，请重新保存 API Key")]
    Credential,
    #[error("供应商已禁用")]
    Disabled,
    #[error("请求已取消")]
    Cancelled,
    #[error("连接检查超时（15 秒）")]
    Timeout,
    #[error("无法连接供应商")]
    Network,
    #[error("供应商返回 HTTP {0}")]
    Http(u16),
    #[error("供应商返回内容格式无效或为空")]
    Malformed,
    #[error("供应商返回内容超过限制")]
    Limit,
}
