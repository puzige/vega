//! Headless MCP protocol, transport, and bounded OAuth client. UI consent and
//! persistent credential ownership remain with the later #73 integration layer.

use std::ffi::OsString;
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
#[derive(Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    header_params: Vec<wire::HeaderParam>,
}

/// A catalog of usable tools and visible rejections.
#[derive(Clone, Debug)]
pub struct Catalog {
    pub tools: Vec<Tool>,
    pub rejected: Vec<String>,
}

/// The supported text/structured subset of a tool result.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub text: Vec<String>,
    pub structured_content: Option<Value>,
    pub is_error: bool,
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
