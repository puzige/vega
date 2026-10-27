use std::fmt;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use vega_credentials::CredentialBinding;

use super::PermissionFence;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ScopeWire")]
pub struct RequestedCommandScope {
    network: bool,
    write_roots: Vec<String>,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeWire {
    #[serde(default)]
    network: bool,
    #[serde(default)]
    write_roots: Vec<String>,
    reason: String,
}

impl TryFrom<ScopeWire> for RequestedCommandScope {
    type Error = CommandScopeError;

    fn try_from(wire: ScopeWire) -> Result<Self, Self::Error> {
        if (!wire.network && wire.write_roots.is_empty())
            || wire.reason.trim().is_empty()
            || wire.reason.len() > 512
            || wire.reason.chars().any(char::is_control)
            || wire.write_roots.iter().any(|root| {
                let path = Path::new(root);
                !path.is_absolute()
                    || root.contains(['"', '\\'])
                    || root.chars().any(char::is_control)
                    || root.split('/').any(|part| matches!(part, "." | ".."))
            })
        {
            return Err(CommandScopeError::InvalidRequest);
        }
        Ok(Self {
            network: wire.network,
            write_roots: wire.write_roots,
            reason: wire.reason,
        })
    }
}

impl From<&vega_tools::BashScopeRequest> for RequestedCommandScope {
    fn from(scope: &vega_tools::BashScopeRequest) -> Self {
        Self {
            network: scope.network(),
            write_roots: scope.write_roots().to_vec(),
            reason: scope.reason().into(),
        }
    }
}

impl RequestedCommandScope {
    pub fn from_json(raw: &str) -> Result<Self, CommandScopeError> {
        serde_json::from_str(raw).map_err(|_| CommandScopeError::InvalidRequest)
    }

    pub fn network(&self) -> bool {
        self.network
    }

    pub fn write_roots(&self) -> &[String] {
        &self.write_roots
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Debug for RequestedCommandScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestedCommandScope")
            .field("network", &self.network)
            .field("write_roots", &self.write_roots.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandScopeDecision {
    AllowOnce,
    Deny,
    Timeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CommandScopeError {
    #[error("invalid command scope request")]
    InvalidRequest,
    #[error("command scope is unavailable")]
    Unavailable,
    #[error("command authority changed")]
    StaleAuthority,
    #[error("command has not been approved")]
    UnapprovedCall,
    #[error("requested command scope is not supported")]
    ScopeUnavailable,
    #[error("command scope request expired")]
    Expired,
    #[error("command scope request was cancelled")]
    Cancelled,
    #[error("command scope run is no longer active")]
    OwnerGone,
    #[error("command scope request conflicts with an existing decision")]
    Conflict,
}

pub type CommandScopeBindingsReader =
    Arc<dyn Fn() -> Result<Vec<CredentialBinding>, CommandScopeError> + Send + Sync>;

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct CommandScopeRequest {
    pub(crate) schema_version: u32,
    pub(crate) request_id: String,
    pub(crate) live_run_id: String,
    pub(crate) run_id: String,
    pub(crate) call_id: String,
    pub(crate) command: String,
    pub(crate) timeout_ms: u64,
    pub(crate) tool_instance: u64,
    pub(crate) project_root: String,
    pub(crate) fence: PermissionFence,
    pub(crate) scope: RequestedCommandScope,
    pub(crate) directories: Vec<vega_tools::BashScopeDirectory>,
    pub(crate) prepared_input_json: String,
    pub(crate) bindings: Vec<CredentialBinding>,
    pub(crate) input_json: String,
    pub(crate) ordinary_approval: String,
}

impl CommandScopeRequest {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    pub fn project_root(&self) -> &str {
        &self.project_root
    }

    pub fn scope(&self) -> &RequestedCommandScope {
        &self.scope
    }

    pub fn directories(&self) -> &[vega_tools::BashScopeDirectory] {
        &self.directories
    }

    pub fn bindings(&self) -> &[CredentialBinding] {
        &self.bindings
    }
}

impl fmt::Debug for CommandScopeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandScopeRequest")
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}
