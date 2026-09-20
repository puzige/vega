use std::collections::HashSet;
use std::time::Duration;

use futures::StreamExt;
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Client, StatusCode, Url};
use serde_json::{Value, json};
use tokio::time::timeout;

use crate::wire::{
    CatalogBuilder, MAX_LINE_OR_EVENT, MAX_RESPONSE, checked_arguments, empty_params, header_value,
    initialize_request, is_modern_error, legacy_request, modern_request, parse_discover,
    parse_initialize, parse_tool_result, response_result,
};
use crate::{BearerCredential, Catalog, McpError, ProtocolVersion, Tool, ToolResult};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

struct HttpReply {
    status: StatusCode,
    message: Option<Value>,
    session_id: Option<String>,
    body_bytes: usize,
}

/// Streamable HTTP MCP client. Bearer credentials must be bound to the exact
/// configured endpoint; this type never follows a redirect or reads provider
/// credentials by itself.
pub struct HttpClient {
    client: Client,
    endpoint: Url,
    version: ProtocolVersion,
    session_id: Option<String>,
    next_id: u64,
    bearer: Option<BearerCredential>,
    last_response_bytes: usize,
}

impl HttpClient {
    /// Probe an exact endpoint; HTTP is allowed only for an explicitly enabled
    /// IP-literal loopback development fixture.
    pub async fn connect(endpoint: &str, allow_loopback_http: bool) -> Result<Self, McpError> {
        Self::connect_inner(endpoint, allow_loopback_http, None).await
    }

    /// Probe an endpoint using an explicitly supplied, exact-endpoint-bound
    /// OAuth or manual bearer credential.
    pub async fn connect_with_bearer(
        endpoint: &str,
        allow_loopback_http: bool,
        bearer: BearerCredential,
    ) -> Result<Self, McpError> {
        Self::connect_inner(endpoint, allow_loopback_http, Some(bearer)).await
    }

    async fn connect_inner(
        endpoint: &str,
        allow_loopback_http: bool,
        bearer: Option<BearerCredential>,
    ) -> Result<Self, McpError> {
        timeout(
            CONNECT_TIMEOUT,
            Self::connect_within_deadline(endpoint, allow_loopback_http, bearer),
        )
        .await
        .map_err(|_| McpError::Timeout)?
    }

    async fn connect_within_deadline(
        endpoint: &str,
        allow_loopback_http: bool,
        bearer: Option<BearerCredential>,
    ) -> Result<Self, McpError> {
        let url = Url::parse(endpoint).map_err(|_| McpError::InvalidConfig)?;
        validate_endpoint(&url, allow_loopback_http)?;
        if bearer.as_ref().is_some_and(|value| !value.bound_to(&url)) {
            return Err(McpError::CredentialBinding);
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|_| McpError::Transport)?;
        let mut connection = Self {
            client,
            endpoint: url,
            version: ProtocolVersion::Modern,
            session_id: None,
            next_id: 1,
            bearer,
            last_response_bytes: 0,
        };
        let id = connection.id()?;
        let probe = modern_request(id, "server/discover", empty_params());
        let reply = connection.post(&probe, None, CONNECT_TIMEOUT).await?;
        let legacy = match reply.status {
            StatusCode::OK => {
                let message = reply.message.ok_or(McpError::InvalidMessage)?;
                match response_result(&message, id) {
                    Ok(result) => {
                        parse_discover(result)?;
                        false
                    }
                    Err(McpError::Rpc(_)) if is_modern_error(&message) => {
                        return Err(McpError::IncompatibleVersion);
                    }
                    Err(error) => return Err(error),
                }
            }
            StatusCode::BAD_REQUEST => {
                if reply.message.as_ref().is_some_and(is_modern_error) {
                    return Err(McpError::IncompatibleVersion);
                }
                true
            }
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => {
                return Err(McpError::UnsupportedTransport);
            }
            _ => return Err(McpError::Transport),
        };
        if legacy {
            connection.version = ProtocolVersion::Legacy;
            let id = connection.id()?;
            let init = initialize_request(id);
            let reply = connection.post(&init, None, CONNECT_TIMEOUT).await?;
            if reply.status != StatusCode::OK {
                return Err(McpError::IncompatibleVersion);
            }
            let message = reply.message.ok_or(McpError::InvalidMessage)?;
            parse_initialize(response_result(&message, id)?)?;
            connection.session_id = reply.session_id;
            let notification = json!({"jsonrpc":"2.0", "method":"notifications/initialized"});
            let accepted = connection
                .post(&notification, None, CONNECT_TIMEOUT)
                .await?;
            if accepted.status != StatusCode::ACCEPTED {
                return Err(McpError::InvalidMessage);
            }
        }
        Ok(connection)
    }

    /// Return the selected wire revision.
    pub fn version(&self) -> ProtocolVersion {
        self.version
    }

    /// Fetch and validate all tool pages; malformed individual tools are rejected.
    pub async fn list_tools(&mut self) -> Result<Catalog, McpError> {
        timeout(CONNECT_TIMEOUT, self.list_tools_within_deadline())
            .await
            .map_err(|_| McpError::Timeout)?
    }

    async fn list_tools_within_deadline(&mut self) -> Result<Catalog, McpError> {
        let modern = self.version == ProtocolVersion::Modern;
        let mut catalog = CatalogBuilder::new(true, modern);
        let mut seen_cursors = HashSet::new();
        let mut cursor: Option<String> = None;
        for _ in 0..128 {
            let mut params = empty_params();
            if let Some(value) = &cursor
                && let Some(object) = params.as_object_mut()
            {
                object.insert("cursor".into(), Value::String(value.clone()));
            }
            let result = self
                .request("tools/list", params, None, CALL_TIMEOUT)
                .await?;
            cursor = catalog.add_page(&result, self.last_response_bytes)?;
            match &cursor {
                None => return Ok(catalog.finish()),
                Some(value) if !seen_cursors.insert(value.clone()) => {
                    return Err(McpError::InvalidMessage);
                }
                Some(_) => {}
            }
        }
        Err(McpError::LimitExceeded)
    }

    /// Send an already-authorized tool call and decode its supported result subset.
    pub async fn call_tool(
        &mut self,
        tool: &Tool,
        arguments: Value,
    ) -> Result<ToolResult, McpError> {
        checked_arguments(&arguments)?;
        let result = self
            .request(
                "tools/call",
                json!({"name": tool.name, "arguments": arguments}),
                Some(tool),
                CALL_TIMEOUT,
            )
            .await?;
        parse_tool_result(&result, self.version == ProtocolVersion::Modern)
    }

    fn id(&mut self) -> Result<u64, McpError> {
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(McpError::LimitExceeded)?;
        Ok(id)
    }

    async fn request(
        &mut self,
        method: &str,
        params: Value,
        tool: Option<&Tool>,
        limit: Duration,
    ) -> Result<Value, McpError> {
        let id = self.id()?;
        let request = match self.version {
            ProtocolVersion::Modern => modern_request(id, method, params),
            ProtocolVersion::Legacy => legacy_request(id, method, params),
        };
        let reply = self.post(&request, tool, limit).await?;
        if reply.status != StatusCode::OK {
            // A non-auth HTTP failure can still carry a matching JSON-RPC
            // error. Preserve its typed code, never its server-provided prose.
            // 401 and 403 retain the separate authorization behavior.
            if !matches!(
                reply.status,
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) && let Some(response) = reply.message.as_ref()
                && let Err(McpError::Rpc(code)) = response_result(response, id)
            {
                return Err(McpError::Rpc(code));
            }
            return Err(McpError::Transport);
        }
        self.last_response_bytes = reply.body_bytes;
        let response = reply.message.ok_or(McpError::InvalidMessage)?;
        Ok(response_result(&response, id)?.clone())
    }

    async fn post(
        &self,
        message: &Value,
        tool: Option<&Tool>,
        limit: Duration,
    ) -> Result<HttpReply, McpError> {
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .ok_or(McpError::InvalidMessage)?;
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .header("Accept", "application/json, text/event-stream")
            .timeout(limit)
            .json(message);
        if let Some(credential) = &self.bearer {
            if !credential.bound_to(&self.endpoint) {
                return Err(McpError::CredentialBinding);
            }
            if credential.is_expired() {
                return Err(McpError::AuthRequired);
            }
            request = request.bearer_auth(credential.secret());
        }
        match self.version {
            ProtocolVersion::Modern => {
                request = request
                    .header("MCP-Protocol-Version", crate::MODERN_VERSION)
                    .header("Mcp-Method", method);
                if method == "tools/call" {
                    let name = message
                        .pointer("/params/name")
                        .and_then(Value::as_str)
                        .ok_or(McpError::InvalidMessage)?;
                    request = request.header("Mcp-Name", header_value(name));
                    let tool = tool.ok_or(McpError::InvalidMessage)?;
                    let args = message
                        .pointer("/params/arguments")
                        .ok_or(McpError::InvalidMessage)?;
                    for (name, value) in tool.mirrored_headers(args)? {
                        let name = HeaderName::from_bytes(name.as_bytes())
                            .map_err(|_| McpError::InvalidMessage)?;
                        let value =
                            HeaderValue::from_str(&value).map_err(|_| McpError::InvalidMessage)?;
                        request = request.header(name, value);
                    }
                }
            }
            ProtocolVersion::Legacy => {
                if method != "initialize" {
                    request = request.header("MCP-Protocol-Version", crate::LEGACY_VERSION);
                }
                if let Some(session_id) = &self.session_id {
                    request = request.header("Mcp-Session-Id", session_id.as_str());
                }
            }
        }
        let expected_id = message.get("id").and_then(Value::as_u64);
        timeout(limit, async {
            let response = request.send().await.map_err(map_http_error)?;
            let status = response.status();
            if status == StatusCode::UNAUTHORIZED {
                return Err(McpError::AuthRequired);
            }
            if status == StatusCode::FORBIDDEN
                && let Some(credential) = &self.bearer
                && let Some(challenge) =
                    credential.scope_challenge(&self.endpoint, response.headers())?
            {
                return Err(McpError::InsufficientScope(Box::new(challenge)));
            }
            if status.is_redirection() {
                return Err(McpError::InvalidConfig);
            }
            let session_id = response
                .headers()
                .get("mcp-session-id")
                .map(|value| {
                    let text = value.to_str().map_err(|_| McpError::InvalidMessage)?;
                    if text.len() > 256 || !text.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
                        return Err(McpError::InvalidMessage);
                    }
                    Ok(text.to_owned())
                })
                .transpose()?;
            if status == StatusCode::ACCEPTED {
                return Ok(HttpReply {
                    status,
                    message: None,
                    session_id,
                    body_bytes: 0,
                });
            }
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let (message, body_bytes) = if content_type.starts_with("application/json") {
                read_json(response, status.is_success()).await?
            } else if content_type.starts_with("text/event-stream") && status.is_success() {
                let (message, bytes) =
                    read_sse(response, expected_id.ok_or(McpError::InvalidMessage)?).await?;
                (Some(message), bytes)
            } else if status.is_client_error() || status.is_server_error() {
                (None, 0)
            } else {
                return Err(McpError::InvalidMessage);
            };
            Ok(HttpReply {
                status,
                message,
                session_id,
                body_bytes,
            })
        })
        .await
        .map_err(|_| McpError::Timeout)?
    }
}

pub(crate) fn validate_endpoint(url: &Url, allow_loopback_http: bool) -> Result<(), McpError> {
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(McpError::InvalidConfig);
    }
    match url.scheme() {
        "https" => Ok(()),
        "http"
            if allow_loopback_http
                && matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "::1")) =>
        {
            Ok(())
        }
        _ => Err(McpError::InvalidConfig),
    }
}

async fn read_json(
    response: reqwest::Response,
    strict: bool,
) -> Result<(Option<Value>, usize), McpError> {
    let bytes = read_bounded_bytes(response).await?;
    let body_bytes = bytes.len();
    match serde_json::from_slice(&bytes) {
        Ok(message) => Ok((Some(message), body_bytes)),
        Err(_) if !strict => Ok((None, body_bytes)),
        Err(_) => Err(McpError::InvalidMessage),
    }
}

async fn read_bounded_bytes(response: reqwest::Response) -> Result<Vec<u8>, McpError> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = timeout(SSE_IDLE_TIMEOUT, stream.next())
        .await
        .map_err(|_| McpError::Timeout)?
    {
        let chunk = chunk.map_err(map_http_error)?;
        if bytes.len() + chunk.len() > MAX_RESPONSE {
            return Err(McpError::LimitExceeded);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn read_sse(
    response: reqwest::Response,
    expected_id: u64,
) -> Result<(Value, usize), McpError> {
    let mut stream = response.bytes_stream();
    let mut event = Vec::new();
    let mut total = 0usize;
    while let Some(chunk) = timeout(SSE_IDLE_TIMEOUT, stream.next())
        .await
        .map_err(|_| McpError::Timeout)?
    {
        let chunk = chunk.map_err(map_http_error)?;
        total = total
            .checked_add(chunk.len())
            .ok_or(McpError::LimitExceeded)?;
        if total > MAX_RESPONSE {
            return Err(McpError::LimitExceeded);
        }
        for &byte in chunk.iter() {
            event.push(byte);
            if event.len() > MAX_LINE_OR_EVENT {
                return Err(McpError::LimitExceeded);
            }
            let delimiter = if event.ends_with(b"\r\n\r\n") {
                Some(4)
            } else if event.ends_with(b"\n\n") {
                Some(2)
            } else {
                None
            };
            if let Some(length) = delimiter {
                let body_len = event.len() - length;
                if let Some(message) = parse_sse_event(&event[..body_len], expected_id)? {
                    return Ok((message, total));
                }
                event.clear();
            }
        }
    }
    Err(McpError::InvalidMessage)
}

fn map_http_error(error: reqwest::Error) -> McpError {
    if error.is_timeout() {
        McpError::Timeout
    } else {
        McpError::Transport
    }
}

fn parse_sse_event(bytes: &[u8], expected_id: u64) -> Result<Option<Value>, McpError> {
    let text = std::str::from_utf8(bytes).map_err(|_| McpError::InvalidMessage)?;
    let mut data = String::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    if data.is_empty() {
        return Ok(None);
    }
    let message: Value = serde_json::from_str(&data).map_err(|_| McpError::InvalidMessage)?;
    if message.get("method").is_some() {
        return if message.get("id").is_some() {
            Err(McpError::UnsupportedResult)
        } else {
            Ok(None)
        };
    }
    if message.get("id") != Some(&Value::from(expected_id)) {
        return Err(McpError::InvalidMessage);
    }
    Ok(Some(message))
}
