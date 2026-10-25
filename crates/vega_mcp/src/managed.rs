use crate::{LocalServer, McpError};
use base64::Engine;
use serde_json::{Value, json};
use std::{collections::HashMap, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;

const MAX_MANAGED_FRAME: usize = 12 * 1024 * 1024;
const MAX_TEXT: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const BROWSER_CONNECTION_TIMEOUT: Duration = Duration::from_secs(300);

pub struct ManagedImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}
pub struct ManagedResult {
    pub text: Vec<String>,
    pub images: Vec<ManagedImage>,
    pub structured: Option<Value>,
    pub is_error: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ManagedSendEvidence {
    #[default]
    NotStarted,
    WriteStarted,
}

#[cfg(any(test, debug_assertions))]
#[path = "managed_recovery_test_support.rs"]
pub mod recovery_test_support;

pub struct ManagedClient {
    child: Option<Child>,
    stdin: Box<dyn AsyncWrite + Unpin + Send>,
    stdout: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
    next_id: u64,
    tools: HashMap<String, Value>,
    closed: bool,
    browser_connection_started: bool,
    #[cfg(any(test, debug_assertions))]
    before_send: Option<Box<dyn FnOnce() + Send>>,
}

impl ManagedClient {
    pub fn spawn(server: LocalServer) -> Result<Self, McpError> {
        if server.credential_policy.is_some()
            || !server.executable.is_absolute()
            || !server.working_directory.is_absolute()
        {
            return Err(McpError::InvalidConfig);
        }
        let mut child = Command::new(server.executable)
            .args(server.args)
            .current_dir(server.working_directory)
            .env_clear()
            .envs(server.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| McpError::Transport)?;
        let stdin = Box::new(child.stdin.take().ok_or(McpError::Transport)?);
        let stdout = BufReader::new(Box::new(child.stdout.take().ok_or(McpError::Transport)?)
            as Box<dyn AsyncRead + Unpin + Send>);
        Ok(Self {
            child: Some(child),
            stdin,
            stdout,
            next_id: 1,
            tools: HashMap::new(),
            closed: false,
            browser_connection_started: false,
            #[cfg(any(test, debug_assertions))]
            before_send: None,
        })
    }

    pub async fn initialize(
        &mut self,
        version: &'static str,
        required: &[&str],
        cancel: &CancellationToken,
    ) -> Result<(), McpError> {
        let result = self.initialize_inner(version, required, cancel).await;
        if result.is_err() {
            self.shutdown().await;
        }
        result
    }

    async fn initialize_inner(
        &mut self,
        version: &'static str,
        required: &[&str],
        cancel: &CancellationToken,
    ) -> Result<(), McpError> {
        if !matches!(version, "2025-06-18" | "2025-11-25") {
            return Err(McpError::IncompatibleVersion);
        }
        let result = self.request("initialize", json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"Vega managed computer use","version":env!("CARGO_PKG_VERSION")}}), cancel).await?;
        if result["protocolVersion"] != version {
            return Err(McpError::IncompatibleVersion);
        }
        tokio::select! { biased;
            _ = cancel.cancelled() => return Err(McpError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(5),self.write(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))) => result.map_err(|_|McpError::Timeout)??,
        }
        let result = self.request("tools/list", json!({}), cancel).await?;
        let entries = result["tools"].as_array().ok_or(McpError::InvalidMessage)?;
        if entries.len() > 256
            || result
                .get("nextCursor")
                .is_some_and(|value| !value.is_null())
        {
            return Err(McpError::LimitExceeded);
        }
        for entry in entries {
            let name = entry["name"].as_str().ok_or(McpError::InvalidMessage)?;
            if required.contains(&name)
                && (entry["inputSchema"]["type"] != "object"
                    || self
                        .tools
                        .insert(name.to_owned(), entry["inputSchema"].clone())
                        .is_some())
            {
                return Err(McpError::InvalidMessage);
            }
        }
        if required.iter().any(|name| !self.tools.contains_key(*name)) {
            return Err(McpError::UnsupportedResult);
        }
        Ok(())
    }

    pub fn require_properties(&self, name: &str, properties: &[&str]) -> Result<(), McpError> {
        let schema = self.schema(name).ok_or(McpError::UnsupportedResult)?;
        if properties
            .iter()
            .any(|key| schema["properties"].get(*key).is_none())
        {
            return Err(McpError::UnsupportedResult);
        }
        Ok(())
    }

    pub fn schema(&self, name: &str) -> Option<&Value> {
        self.tools.get(name)
    }

    pub async fn call(
        &mut self,
        name: &str,
        arguments: Value,
        cancel: &CancellationToken,
    ) -> Result<ManagedResult, McpError> {
        self.call_with_timeout(name, arguments, cancel, REQUEST_TIMEOUT)
            .await
    }

    pub async fn call_with_send_evidence(
        &mut self,
        name: &str,
        arguments: Value,
        cancel: &CancellationToken,
        send: &mut ManagedSendEvidence,
    ) -> Result<ManagedResult, McpError> {
        self.call_with_timeout_and_send_evidence(name, arguments, cancel, REQUEST_TIMEOUT, send)
            .await
    }

    pub async fn connect_browser_target(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<ManagedResult, McpError> {
        if self.browser_connection_started {
            return Err(McpError::InvalidConfig);
        }
        self.browser_connection_started = true;
        self.call_with_timeout(
            "browser_tabs",
            json!({"action":"list","_meta":{"raw":true}}),
            cancel,
            BROWSER_CONNECTION_TIMEOUT,
        )
        .await
    }

    pub async fn current_browser_target(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<ManagedResult, McpError> {
        self.call(
            "browser_tabs",
            json!({"action":"list","_meta":{"raw":true}}),
            cancel,
        )
        .await
    }

    async fn call_with_timeout(
        &mut self,
        name: &str,
        arguments: Value,
        cancel: &CancellationToken,
        timeout: Duration,
    ) -> Result<ManagedResult, McpError> {
        self.call_with_timeout_and_send_evidence(
            name,
            arguments,
            cancel,
            timeout,
            &mut ManagedSendEvidence::NotStarted,
        )
        .await
    }

    async fn call_with_timeout_and_send_evidence(
        &mut self,
        name: &str,
        arguments: Value,
        cancel: &CancellationToken,
        timeout: Duration,
        send: &mut ManagedSendEvidence,
    ) -> Result<ManagedResult, McpError> {
        if !self.tools.contains_key(name) {
            return Err(McpError::UnsupportedResult);
        }
        crate::wire::checked_arguments(&arguments)?;
        let value = self
            .request_with_timeout(
                "tools/call",
                json!({"name":name,"arguments":arguments}),
                cancel,
                timeout,
                send,
            )
            .await?;
        parse_result(value)
    }

    async fn request(
        &mut self,
        method: &str,
        params: Value,
        cancel: &CancellationToken,
    ) -> Result<Value, McpError> {
        self.request_with_timeout(
            method,
            params,
            cancel,
            REQUEST_TIMEOUT,
            &mut ManagedSendEvidence::NotStarted,
        )
        .await
    }

    async fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        cancel: &CancellationToken,
        timeout: Duration,
        send: &mut ManagedSendEvidence,
    ) -> Result<Value, McpError> {
        if self.closed {
            return Err(McpError::Transport);
        }
        if cancel.is_cancelled() {
            return Err(McpError::Cancelled);
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(McpError::LimitExceeded)?;
        #[cfg(any(test, debug_assertions))]
        if let Some(before_send) = self.before_send.take() {
            before_send();
        }
        let exchange = async {
            self.write_with_send_evidence(
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
                send,
            )
            .await?;
            let mut total = 0usize;
            loop {
                let mut frame = Vec::new();
                loop {
                    let buffer = self
                        .stdout
                        .fill_buf()
                        .await
                        .map_err(|_| McpError::Transport)?;
                    if buffer.is_empty() {
                        return Err(McpError::Transport);
                    }
                    let count = buffer
                        .iter()
                        .position(|byte| *byte == b'\n')
                        .map_or(buffer.len(), |index| index + 1);
                    total = total.checked_add(count).ok_or(McpError::LimitExceeded)?;
                    if total > MAX_MANAGED_FRAME {
                        return Err(McpError::LimitExceeded);
                    }
                    let done = buffer[count - 1] == b'\n';
                    frame.extend_from_slice(&buffer[..count]);
                    self.stdout.consume(count);
                    if done {
                        break;
                    }
                }
                let value: Value =
                    serde_json::from_slice(&frame).map_err(|_| McpError::InvalidMessage)?;
                if value.get("id").is_none() && value.get("method").is_some() {
                    continue;
                }
                if value["id"].as_u64() != Some(id) {
                    return Err(McpError::InvalidMessage);
                }
                if value.get("error").is_some() {
                    return Err(McpError::Rpc(
                        value["error"]["code"].as_i64().unwrap_or(-32603),
                    ));
                }
                return value.get("result").cloned().ok_or(McpError::InvalidMessage);
            }
        };
        let result = tokio::select! { biased; _ = cancel.cancelled() => Err(McpError::Cancelled), result = tokio::time::timeout(timeout, exchange) => result.unwrap_or(Err(McpError::Timeout)) };
        if result.is_err() {
            self.shutdown().await;
        }
        result
    }

    async fn write(&mut self, value: Value) -> Result<(), McpError> {
        self.write_with_send_evidence(value, &mut ManagedSendEvidence::NotStarted)
            .await
    }

    async fn write_with_send_evidence(
        &mut self,
        value: Value,
        send: &mut ManagedSendEvidence,
    ) -> Result<(), McpError> {
        let mut bytes = serde_json::to_vec(&value).map_err(|_| McpError::InvalidMessage)?;
        bytes.push(b'\n');
        let mut offset = 0;
        while offset < bytes.len() {
            *send = ManagedSendEvidence::WriteStarted;
            let written = self
                .stdin
                .write(&bytes[offset..])
                .await
                .map_err(|_| McpError::Transport)?;
            if written == 0 {
                return Err(McpError::Transport);
            }
            offset += written;
        }
        self.stdin.flush().await.map_err(|_| McpError::Transport)
    }

    pub async fn shutdown(&mut self) -> bool {
        self.closed = true;
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
            child.wait().await.is_ok()
        } else {
            self.stdin.shutdown().await.is_ok()
        }
    }
}

fn parse_result(value: Value) -> Result<ManagedResult, McpError> {
    let is_error = match value.get("isError") {
        None => false,
        Some(value) => value.as_bool().ok_or(McpError::InvalidMessage)?,
    };
    let mut result = ManagedResult {
        text: Vec::new(),
        images: Vec::new(),
        structured: value.get("structuredContent").cloned(),
        is_error,
    };
    let mut bytes = result
        .structured
        .as_ref()
        .map_or(0, |value| value.to_string().len());
    if bytes > MAX_TEXT {
        return Err(McpError::LimitExceeded);
    }
    for block in value["content"]
        .as_array()
        .ok_or(McpError::InvalidMessage)?
    {
        match block["type"].as_str() {
            Some("text") => {
                let text = block["text"].as_str().ok_or(McpError::InvalidMessage)?;
                bytes = bytes.saturating_add(text.len());
                if bytes > MAX_TEXT {
                    return Err(McpError::LimitExceeded);
                }
                result.text.push(text.to_owned());
            }
            Some("image") if result.images.is_empty() => {
                let mime = block["mimeType"]
                    .as_str()
                    .filter(|mime| matches!(*mime, "image/png" | "image/jpeg" | "image/webp"))
                    .ok_or(McpError::UnsupportedResult)?;
                let data = block["data"].as_str().ok_or(McpError::InvalidMessage)?;
                if data.len() > 11_184_812 {
                    return Err(McpError::LimitExceeded);
                }
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|_| McpError::InvalidMessage)?;
                if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
                    return Err(McpError::LimitExceeded);
                }
                result.images.push(ManagedImage {
                    bytes,
                    mime_type: mime.to_owned(),
                });
            }
            _ => return Err(McpError::UnsupportedResult),
        }
    }
    Ok(result)
}
