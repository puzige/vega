//! Headless MCP protocol, transport, and bounded OAuth client. UI consent and
//! persistent credential ownership remain with the later #73 integration layer.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use serde_json::Value;

mod auth;
mod http;
mod stdio;
mod wire;

pub use auth::{
    AuthorizationRequest, AuthorizationServer, BearerCredential, OAuthClient, OAuthTokens,
    ResourceAuthorization, ScopeChallenge,
};
pub use http::HttpClient;
pub use stdio::StdioClient;

/// Validate an exact endpoint in Settings without sending any network request.
/// Connection performs the same check again before dispatch.
pub fn validate_endpoint_url(endpoint: &str, allow_loopback_http: bool) -> Result<(), McpError> {
    let url = reqwest::Url::parse(endpoint).map_err(|_| McpError::InvalidConfig)?;
    http::validate_endpoint(&url, allow_loopback_http)
}

/// The preferred modern MCP revision.
pub const MODERN_VERSION: &str = "2026-07-28";
/// The only initialization-based revision supported by this client.
pub const LEGACY_VERSION: &str = "2025-11-25";

/// The supported MCP wire revisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolVersion {
    Modern,
    Legacy,
}

/// Explicit local command; no shell or inherited credential environment.
#[derive(Clone)]
pub struct LocalServer {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
    /// Explicitly selected environment only; values must never be logged.
    pub environment: Vec<(OsString, OsString)>,
}

/// A validated tool exposed by a server.
#[derive(Clone)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    header_params: Vec<wire::HeaderParam>,
}

impl fmt::Debug for Tool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tool")
            .field("name_bytes", &self.name.len())
            .field(
                "description_bytes",
                &self.description.as_ref().map(String::len),
            )
            .field("input_schema_type", &json_type(&self.input_schema))
            .field("header_params_count", &self.header_params.len())
            .finish()
    }
}

/// A catalog of usable tools and visible rejections.
#[derive(Clone)]
pub struct Catalog {
    pub tools: Vec<Tool>,
    pub rejected: Vec<String>,
}

impl fmt::Debug for Catalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Catalog")
            .field("tools_count", &self.tools.len())
            .field("rejected_count", &self.rejected.len())
            .finish()
    }
}

/// The supported text/structured subset of a tool result.
#[derive(Clone, PartialEq)]
pub struct ToolResult {
    pub text: Vec<String>,
    pub structured_content: Option<Value>,
    pub is_error: bool,
}

impl fmt::Debug for ToolResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text_bytes = self
            .text
            .iter()
            .fold(0usize, |total, text| total.saturating_add(text.len()));
        formatter
            .debug_struct("ToolResult")
            .field("text_count", &self.text.len())
            .field("text_bytes", &text_bytes)
            .field(
                "structured_content_type",
                &self.structured_content.as_ref().map(json_type),
            )
            .field("is_error", &self.is_error)
            .finish()
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Safe protocol/transport failure; no server-provided secret-bearing prose.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("invalid MCP endpoint or local command")]
    InvalidConfig,
    #[error("MCP server protocol incompatible")]
    IncompatibleVersion,
    #[error("deprecated standalone HTTP+SSE transport is unsupported")]
    UnsupportedTransport,
    #[error("MCP transport failed")]
    Transport,
    #[error("MCP request timed out")]
    Timeout,
    #[error("MCP request cancelled")]
    Cancelled,
    #[error("MCP server returned an invalid protocol message")]
    InvalidMessage,
    #[error("MCP message exceeded a size or count limit")]
    LimitExceeded,
    #[error("MCP server requires authorization")]
    AuthRequired,
    #[error("MCP authorization metadata is invalid or unavailable")]
    AuthDiscovery,
    #[error("MCP authorization failed a security validation")]
    AuthSecurity,
    #[error("MCP authorization server granted scope beyond the approved request")]
    ScopeEscalation,
    #[error("MCP server requires additional authorization scopes")]
    InsufficientScope(Box<ScopeChallenge>),
    #[error("MCP authorization requires an explicit user confirmation")]
    ConsentRequired,
    #[error("this authorization server requires a public HTTPS Client ID Metadata identity")]
    CimdUnavailable,
    #[error("MCP dynamic client registration failed")]
    Registration,
    #[error("MCP credential is not bound to this exact endpoint")]
    CredentialBinding,
    #[error("MCP server returned JSON-RPC error code {0}")]
    Rpc(i64),
    #[error("MCP result requires an unsupported content capability")]
    UnsupportedResult,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Catalog, Tool, ToolResult};

    #[test]
    fn debug_output_never_contains_server_provided_catalog_or_result_content() {
        const SENTINEL: &str = "FAKE_SECRET_SENTINEL_DO_NOT_LOG_7301";
        let tool = Tool {
            name: SENTINEL.into(),
            description: Some(SENTINEL.into()),
            input_schema: json!({"type":"object", "description":SENTINEL}),
            header_params: Vec::new(),
        };
        let catalog = Catalog {
            tools: vec![tool.clone()],
            rejected: vec![SENTINEL.into()],
        };
        let result = ToolResult {
            text: vec![SENTINEL.into()],
            structured_content: Some(json!({"secret":SENTINEL})),
            is_error: true,
        };
        for debug in [
            format!("{tool:?}"),
            format!("{tool:#?}"),
            format!("{catalog:?}"),
            format!("{catalog:#?}"),
            format!("{result:?}"),
            format!("{result:#?}"),
        ] {
            assert!(!debug.contains(SENTINEL), "unsafe Debug: {debug}");
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod mock;
mod transport;
#[cfg(test)]
extern crate self as vega_mcp;
#[cfg(test)]
#[path = "tests/oauth.rs"]
mod oauth_tests;

#[cfg(test)]
#[path = "tests/protocol.rs"]
mod protocol_tests;

#[cfg(test)]
#[path = "tests/stdio.rs"]
mod stdio_tests;
