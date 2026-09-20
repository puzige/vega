//! Stable, value-free identity shared by MCP audit, permission and UI cards.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

use super::ToolCall;

/// Settings form. It contains configuration and credential *references*, never
/// token or environment secret values. Saving this form does not connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerForm {
    pub display_name: String,
    pub transport: McpServerTransport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerTransport {
    Local {
        executable: PathBuf,
        args: Vec<String>,
        working_directory: Option<PathBuf>,
        environment: Vec<McpEnvironmentVariable>,
    },
    Remote {
        endpoint: String,
        allow_loopback_http: bool,
        authorization: McpRemoteAuthorization,
    },
}

/// Local variable name only. Its secret lives in a slot owned by this exact
/// MCP server, never an arbitrary provider or another server reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpEnvironmentVariable {
    pub variable: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpRemoteAuthorization {
    None,
    /// Secret is entered separately into owner-only keystore storage.
    Bearer,
    /// A pre-registered client ID or explicit DCR flow. Neither variant may
    /// auto-register or open the browser during form save.
    OAuth {
        client_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerHealth {
    Disabled,
    PendingRemoval,
    Disconnected,
    Ready,
    NeedsCredential,
    NeedsAuthorization,
    /// Content-free code only; no server-returned error text.
    Error(String),
}

/// Stable Settings projection; tool names are visible but not authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerView {
    pub id: String,
    pub config_revision: u64,
    pub enabled: bool,
    pub deleting: bool,
    pub form: McpServerForm,
    /// Whether all explicitly referenced credentials are present locally;
    /// never contains their values.
    pub credential_configured: bool,
    pub health: McpServerHealth,
    pub tool_names: Vec<String>,
    pub rejected_tools: Vec<String>,
}

/// Explicit connection test result. Testing a disabled server never grants it
/// run authority; these names remain a Settings-only preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpConnectionTest {
    pub tool_names: Vec<String>,
    pub rejected_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerDiagnostic {
    pub server_id: String,
    pub code: String,
}

/// Validated OAuth choices from one explicit Settings discovery action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthDiscovery {
    pub resource: String,
    pub issuers: Vec<String>,
    pub requested_scopes: Vec<String>,
}

/// A still-live 403 challenge, already bound to the OAuth server revision by
/// the run dispatcher. It is not a credential and never replays the failed call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthStepUpOffer {
    pub server_id: String,
    pub config_revision: u64,
    pub added_scopes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpOAuthRegistration {
    PreRegistered,
    DynamicRegistration,
    CimdUnavailable,
    Unavailable,
}

/// Exact details visible before the user consents to browser authorization
/// or deprecated DCR. The callback port is already bound in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthPreparation {
    pub flow_id: String,
    pub resource: String,
    pub issuer: String,
    pub redirect_uri: String,
    pub requested_scopes: Vec<String>,
    pub step_up_added_scopes: Vec<String>,
    pub registration: McpOAuthRegistration,
    pub registration_endpoint: Option<String>,
}

/// One ephemeral browser authorization. `flow_id` is not a credential; the
/// PKCE verifier and callback listener remain in the Settings service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthStart {
    pub flow_id: String,
    pub authorization_url: String,
    pub issuer: String,
    pub requested_scopes: Vec<String>,
}

/// Exact identity of one frozen external tool proposal. This is not a grant;
/// every Execute call still requires a fresh one-shot permission decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCallIdentity {
    /// Vega-owned canonical server id.
    pub server_id: String,
    /// Configuration revision fixed at run start.
    pub config_revision: u64,
    /// Exact MCP name, not a server-supplied display label.
    pub exact_tool_name: String,
    /// UTF-8 byte count of the raw model arguments.
    pub arguments_bytes: u64,
    /// SHA-256 of those exact bytes, with no argument values retained here.
    pub arguments_sha256: String,
    /// Bounded field/type summary, never argument values.
    pub argument_preview: String,
}

impl McpCallIdentity {
    /// Deterministic provider-safe name for this canonical server/tool pair.
    pub fn alias(&self) -> String {
        let digest = Sha256::digest(self.exact_tool_name.as_bytes());
        let suffix = digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("mcp_{}_{}", self.server_id, suffix)
    }

    /// Stable, value-free text used to bind the proposal to its prompt.
    pub fn permission_target(&self) -> String {
        format!(
            "server {} · revision {} · tool {} · {} bytes · sha256 {} · fields {}",
            self.server_id,
            self.config_revision,
            self.exact_tool_name,
            self.arguments_bytes,
            self.arguments_sha256,
            self.argument_preview,
        )
    }

    /// Requires a canonical server ID and bounded, printable values. The
    /// digest authenticates equality of a proposal, not a server's behavior.
    pub fn is_valid(&self) -> bool {
        let canonical_id = self
            .server_id
            .parse::<ulid::Ulid>()
            .is_ok_and(|id| id.to_string() == self.server_id);
        canonical_id
            && !self.exact_tool_name.is_empty()
            && self.exact_tool_name.len() <= 128
            && !self.exact_tool_name.chars().any(char::is_control)
            && self.arguments_bytes <= 256 * 1024
            && self.arguments_sha256.len() == 64
            && self
                .arguments_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            && self.argument_preview.len() <= 2048
            && !self.argument_preview.chars().any(char::is_control)
    }

    /// Strictly decodes the safe runtime projection: unknown fields, raw
    /// argument values and a mismatched alias are rejected before any card.
    pub fn from_tool_call(call: &ToolCall) -> Option<Self> {
        let value: Value = serde_json::from_str(&call.input_json).ok()?;
        let object = value.as_object()?;
        let expected = [
            "server_id",
            "config_revision",
            "tool",
            "arguments_bytes",
            "arguments_sha256",
            "argument_preview",
        ];
        if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
            return None;
        }
        let identity = Self {
            server_id: object.get("server_id")?.as_str()?.to_string(),
            config_revision: object.get("config_revision")?.as_u64()?,
            exact_tool_name: object.get("tool")?.as_str()?.to_string(),
            arguments_bytes: object.get("arguments_bytes")?.as_u64()?,
            arguments_sha256: object.get("arguments_sha256")?.as_str()?.to_string(),
            argument_preview: object.get("argument_preview")?.as_str()?.to_string(),
        };
        (identity.is_valid() && identity.alias() == call.tool).then_some(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ToolCardInputProjection, tool_card_input_projection};

    fn identity() -> McpCallIdentity {
        McpCallIdentity {
            server_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            config_revision: 7,
            exact_tool_name: "lookup".into(),
            arguments_bytes: 23,
            arguments_sha256: "a".repeat(64),
            argument_preview: "query: string".into(),
        }
    }

    #[test]
    fn issue73_safe_projection_round_trips_without_argument_values() {
        let identity = identity();
        let call = ToolCall {
            id: "call-1".into(),
            tool: identity.alias(),
            input_json: serde_json::json!({
                "server_id": identity.server_id,
                "config_revision": identity.config_revision,
                "tool": identity.exact_tool_name,
                "arguments_bytes": identity.arguments_bytes,
                "arguments_sha256": identity.arguments_sha256,
                "argument_preview": identity.argument_preview,
            })
            .to_string(),
        };
        assert!(!call.input_json.contains("SECRET_ARGUMENT_VALUE"));
        let decoded = McpCallIdentity::from_tool_call(&call).unwrap();
        assert_eq!(decoded.permission_target(), identity.permission_target());
        assert!(matches!(
            tool_card_input_projection(&call),
            ToolCardInputProjection::Mcp { .. }
        ));
    }

    #[test]
    fn issue73_safe_projection_rejects_alias_swap_and_extra_raw_fields() {
        let identity = identity();
        let safe_json = serde_json::json!({
            "server_id": identity.server_id,
            "config_revision": identity.config_revision,
            "tool": identity.exact_tool_name,
            "arguments_bytes": identity.arguments_bytes,
            "arguments_sha256": identity.arguments_sha256,
            "argument_preview": identity.argument_preview,
        });
        let swapped = ToolCall {
            id: "call-1".into(),
            tool: format!("{}_swapped", identity.alias()),
            input_json: safe_json.to_string(),
        };
        assert!(McpCallIdentity::from_tool_call(&swapped).is_none());
        let mut extra = safe_json;
        extra.as_object_mut().unwrap().insert(
            "arguments".into(),
            serde_json::json!({"token":"SECRET_ARGUMENT_VALUE"}),
        );
        let call = ToolCall {
            id: "call-2".into(),
            tool: identity.alias(),
            input_json: extra.to_string(),
        };
        assert!(McpCallIdentity::from_tool_call(&call).is_none());
        assert!(matches!(
            tool_card_input_projection(&call),
            ToolCardInputProjection::Corrupt
        ));
    }
}
