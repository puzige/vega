use std::collections::HashSet;
#[cfg(not(any(test, feature = "test-support")))]
use std::process::Stdio;
use std::time::Duration;

#[cfg(any(test, feature = "test-support"))]
use crate::mock::Child;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(not(any(test, feature = "test-support")))]
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
#[cfg(any(test, feature = "test-support"))]
type ChildStdin = tokio::io::WriteHalf<tokio::io::DuplexStream>;
#[cfg(any(test, feature = "test-support"))]
type ChildStdout = tokio::io::ReadHalf<tokio::io::DuplexStream>;
use tokio::time::{Instant, sleep_until, timeout};
use tokio_util::sync::CancellationToken;

use crate::wire::{
    CatalogBuilder, MAX_LINE_OR_EVENT, MAX_RESPONSE, checked_arguments, empty_params,
    initialize_request, is_modern_error, legacy_request, modern_request, parse_discover,
    parse_initialize, parse_tool_result, response_result,
};
use crate::{Catalog, LocalServer, McpError, ProtocolVersion, Tool, ToolResult};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// Owned stdio MCP session. Calls are serialized by its mutable borrow.
pub struct StdioClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    version: ProtocolVersion,
    next_id: u64,
    last_response_bytes: usize,
}

impl StdioClient {
    /// Launch, probe, and negotiate a local server without a shell or inherited environment.
    pub async fn connect(server: LocalServer) -> Result<Self, McpError> {
        timeout(CONNECT_TIMEOUT, Self::connect_within_deadline(server))
            .await
            .map_err(|_| McpError::Timeout)?
    }

    async fn connect_within_deadline(server: LocalServer) -> Result<Self, McpError> {
        if !server.executable.is_absolute() || !server.working_directory.is_absolute() {
            return Err(McpError::InvalidConfig);
        }
        if server.environment.iter().any(|(name, value)| {
            name.to_str().is_none_or(|text| {
                text.is_empty()
                    || !text
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            }) || value
                .to_str()
                .is_none_or(|text| text.as_bytes().contains(&0))
        }) {
            return Err(McpError::InvalidConfig);
        }
        #[cfg(not(any(test, feature = "test-support")))]
        let mut child = Command::new(&server.executable)
            .args(&server.args)
            .current_dir(&server.working_directory)
            .env_clear()
            .envs(server.environment.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| McpError::Transport)?;
        #[cfg(any(test, feature = "test-support"))]
        let mut child = crate::mock::stdio_connect(&server)?;
        let stdin = child.stdin.take().ok_or(McpError::Transport)?;
        let stdout = child.stdout.take().ok_or(McpError::Transport)?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            version: ProtocolVersion::Modern,
            next_id: 1,
            last_response_bytes: 0,
        };
        let probe_id = client.id()?;
        let probe = modern_request(probe_id, "server/discover", empty_params());
        let legacy = match client.exchange(probe, probe_id, CONNECT_TIMEOUT).await {
            Ok(message) => match response_result(&message, probe_id) {
                Ok(result) => {
                    parse_discover(result)?;
                    false
                }
                Err(McpError::Rpc(_)) if is_modern_error(&message) => {
                    return Err(McpError::IncompatibleVersion);
                }
                Err(McpError::Rpc(_)) => true,
                Err(error) => return Err(error),
            },
            Err(McpError::Timeout) => true,
            Err(error) => return Err(error),
        };
        if legacy {
            let id = client.id()?;
            let response = client
                .exchange(initialize_request(id), id, CONNECT_TIMEOUT)
                .await?;
            parse_initialize(response_result(&response, id)?)?;
            client
                .write_message(&json!({"jsonrpc":"2.0", "method":"notifications/initialized"}))
                .await?;
            client.version = ProtocolVersion::Legacy;
        }
        Ok(client)
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
        let mut catalog = CatalogBuilder::new(false, modern);
        let mut seen_cursors = HashSet::new();
        let mut cursor: Option<String> = None;
        for _ in 0..128 {
            let mut params = empty_params();
            if let Some(value) = &cursor
                && let Some(object) = params.as_object_mut()
            {
                object.insert("cursor".into(), Value::String(value.clone()));
            }
            let result = self.request("tools/list", params, CALL_TIMEOUT).await?;
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
        self.call_tool_with_cancel(tool, arguments, &CancellationToken::new(), CALL_TIMEOUT)
            .await
    }

    /// Send one approved call with an explicit stop signal and at most the
    /// v1 call deadline. A stopped or timed-out child is never reused.
    pub async fn call_tool_with_cancel(
        &mut self,
        tool: &Tool,
        arguments: Value,
        cancel: &CancellationToken,
        limit: Duration,
    ) -> Result<ToolResult, McpError> {
        checked_arguments(&arguments)?;
        if cancel.is_cancelled() {
            return Err(McpError::Cancelled);
        }
        let limit = limit.min(CALL_TIMEOUT);
        if limit.is_zero() {
            return Err(McpError::Timeout);
        }
        let deadline = Instant::now() + limit;
        let id = self.id()?;
        let params = json!({"name": tool.name, "arguments": arguments});
        let request = match self.version {
            ProtocolVersion::Modern => modern_request(id, "tools/call", params),
            ProtocolVersion::Legacy => legacy_request(id, "tools/call", params),
        };
        match timeout(limit, self.write_message(&request)).await {
            Ok(result) => result?,
            Err(_) => {
                // A partially written frame cannot be safely followed by a
                // cancellation frame. Reap the child without guessing its ID.
                self.reap_cancelled_child().await;
                return Err(McpError::Timeout);
            }
        }
        let response = tokio::select! {
            biased;
            response = self.read_response(id) => response,
            _ = cancel.cancelled() => Err(McpError::Cancelled),
            _ = sleep_until(deadline) => Err(McpError::Timeout),
        };
        let response = match response {
            Ok(value) => value,
            Err(error @ (McpError::Cancelled | McpError::Timeout)) => {
                let reason = if matches!(error, McpError::Timeout) {
                    "Call timed out"
                } else {
                    "User requested cancellation"
                };
                let notification = json!({"jsonrpc":"2.0", "method":"notifications/cancelled",
                    "params":{"requestId":id, "reason":reason}});
                let _ = timeout(SHUTDOWN_TIMEOUT, self.write_message(&notification)).await;
                self.reap_cancelled_child().await;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let result = response_result(&response, id)?.clone();
        parse_tool_result(&result, self.version == ProtocolVersion::Modern)
    }

    async fn reap_cancelled_child(&mut self) {
        if !matches!(
            timeout(SHUTDOWN_TIMEOUT, self.child.wait()).await,
            Ok(Ok(_))
        ) {
            let _ = self.child.kill().await;
            let _ = self.child.wait().await;
        }
    }

    /// Close stdin, wait briefly for an orderly exit, then force-reap if necessary.
    pub async fn shutdown(self) -> Result<(), McpError> {
        let Self {
            mut child, stdin, ..
        } = self;
        #[cfg(any(test, feature = "test-support"))]
        let mut stdin = stdin;
        #[cfg(any(test, feature = "test-support"))]
        let _ = stdin.shutdown().await;
        drop(stdin);
        match timeout(SHUTDOWN_TIMEOUT, child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) => Err(McpError::Transport),
            Err(_) => {
                child.kill().await.map_err(|_| McpError::Transport)?;
                child.wait().await.map_err(|_| McpError::Transport)?;
                Ok(())
            }
        }
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
        limit: Duration,
    ) -> Result<Value, McpError> {
        let id = self.id()?;
        let message = match self.version {
            ProtocolVersion::Modern => modern_request(id, method, params),
            ProtocolVersion::Legacy => legacy_request(id, method, params),
        };
        let response = self.exchange(message, id, limit).await?;
        Ok(response_result(&response, id)?.clone())
    }

    async fn exchange(
        &mut self,
        request: Value,
        id: u64,
        limit: Duration,
    ) -> Result<Value, McpError> {
        timeout(limit, async {
            self.write_message(&request).await?;
            self.read_response(id).await
        })
        .await
        .map_err(|_| McpError::Timeout)?
    }

    async fn read_response(&mut self, id: u64) -> Result<Value, McpError> {
        let mut bytes_read = 0usize;
        loop {
            let line = self.read_line_limited().await?;
            bytes_read = bytes_read
                .checked_add(line.len())
                .ok_or(McpError::LimitExceeded)?;
            if bytes_read > MAX_RESPONSE {
                return Err(McpError::LimitExceeded);
            }
            let response: Value =
                serde_json::from_slice(&line).map_err(|_| McpError::InvalidMessage)?;
            if response.get("method").is_some() {
                if response.get("id").is_some() {
                    return Err(McpError::UnsupportedResult);
                }
                continue;
            }
            if response.get("id") == Some(&Value::from(id)) {
                self.last_response_bytes = bytes_read;
                return Ok(response);
            }
        }
    }

    async fn write_message(&mut self, message: &Value) -> Result<(), McpError> {
        let mut data = serde_json::to_vec(message).map_err(|_| McpError::InvalidMessage)?;
        if data.len() + 1 > MAX_LINE_OR_EVENT {
            return Err(McpError::LimitExceeded);
        }
        data.push(b'\n');
        self.stdin
            .write_all(&data)
            .await
            .map_err(|_| McpError::Transport)?;
        self.stdin.flush().await.map_err(|_| McpError::Transport)
    }

    async fn read_line_limited(&mut self) -> Result<Vec<u8>, McpError> {
        let mut line = Vec::new();
        loop {
            let available = self
                .stdout
                .fill_buf()
                .await
                .map_err(|_| McpError::Transport)?;
            if available.is_empty() {
                return Err(McpError::Transport);
            }
            let count = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |position| position + 1);
            if line.len() + count > MAX_LINE_OR_EVENT {
                return Err(McpError::LimitExceeded);
            }
            let ended = available[count - 1] == b'\n';
            line.extend_from_slice(&available[..count]);
            self.stdout.consume(count);
            if ended {
                return Ok(line);
            }
        }
    }
}
