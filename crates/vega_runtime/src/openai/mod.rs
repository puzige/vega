//! OpenAI-compatible streaming chat provider (tech-spec §4.1, A3-02 / S4-T19).
//!
//! POSTs `{base_url}/chat/completions` with `stream: true` and
//! `stream_options: { "include_usage": true }`, parses the SSE stream with
//! `eventsource-stream`, aggregates `delta.tool_calls` fragments by index
//! (emitting [`ProviderEvent::ToolUse`] only once fragments complete), and
//! recovers the final usage chunk before `data: [DONE]`.
//!
//! Retry policy (tech-spec §4.1): network errors / 5xx back off exponentially
//! (1s / 2s / 4s, at most 3 retries); 429 honors `Retry-After`. Retries only
//! rebuild the request — the stream is never replayed. Cancellation aborts
//! immediately, mid-backoff or mid-stream, without further events.
//!
//! Red line: the API key is only ever placed into the `Authorization`
//! request header — it never reaches logs, `Debug` output, or error messages
//! (server bodies echoing it are redacted best-effort).

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use eventsource_stream::{Event as RawEvent, EventStreamError, Eventsource};
use futures::future::BoxFuture;
use futures::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::error::VegaError;
use crate::provider::{
    ChatRequest, EventStream, FrozenReasoning, Provider, ProviderEvent, ProviderFailureKind,
    ProviderResponseMetadata, ProviderStream, ReasoningChoice, ReasoningDisabledWire,
    ReasoningProtocol, StopReason,
};
use crate::retry::{RetryPolicy, parse_retry_after};

/// Authorization scheme for OpenAI-compatible APIs (used for the single
/// header construction below; tests reference this constant).
const AUTH_SCHEME: &str = "Bearer";
/// Terminal SSE sentinel of the OpenAI chat-completion stream.
const DONE_SENTINEL: &str = "[DONE]";
/// Base path appended to the configured base URL.
const CHAT_COMPLETIONS_PATH: &str = "/chat/completions";

type RequestAttemptGuard = Arc<dyn Fn(&ChatRequest) -> Result<(), VegaError> + Send + Sync>;

/// OpenAI-compatible streaming chat provider.
///
/// Cloning is cheap (the HTTP client is an internal `Arc`); every clone
/// still redacts the key from its `Debug` output.
#[derive(Clone)]
pub struct OpenAiProvider {
    http: reqwest::Client,
    #[cfg(test)]
    test_transport: Option<tests::TestTransport>,
    /// Endpoint root, e.g. `https://api.openai.com/v1` (trailing slash ok).
    base_url: String,
    /// API key; only ever serialized into the Authorization request header.
    key: String,
    /// Retry schedule applied while establishing the stream.
    retry: RetryPolicy,
    /// Owner-only request inspection. This must run for every HTTP attempt,
    /// including retries after a backoff; it is intentionally omitted from
    /// Debug output and never enters a wire request.
    before_attempt: Option<RequestAttemptGuard>,
}

impl fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 红线：key 永不进日志/调试输出
        f.debug_struct("OpenAiProvider")
            .field("base_url_bytes", &self.base_url.len())
            .field("key", &"<redacted>")
            .field("retry", &self.retry)
            .finish()
    }
}

impl OpenAiProvider {
    /// Builds a provider over an OpenAI-compatible endpoint.
    ///
    /// `key` is the API key; it stays in memory and only ever appears in the
    /// Authorization request header.
    pub fn new(base_url: impl Into<String>, key: impl Into<String>) -> Result<Self, VegaError> {
        let http = reqwest::Client::builder()
            .build()
            .map_err(|e| VegaError::Provider {
                status: None,
                message: format!("failed to build HTTP client: {e}"),
                retryable: false,
            })?;
        Ok(Self {
            http,
            #[cfg(test)]
            test_transport: None,
            base_url: base_url.into(),
            key: key.into(),
            retry: RetryPolicy::default(),
            before_attempt: None,
        })
    }

    /// Overrides the retry schedule (defaults: 1s / 2s / 4s, 3 retries).
    pub fn with_retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Inspect the request immediately before each HTTP attempt, including
    /// internal 429/network retries. The callback must not retain or expose
    /// owner credentials in its error or Debug representation.
    pub fn with_pre_attempt_guard(
        mut self,
        guard: impl Fn(&ChatRequest) -> Result<(), VegaError> + Send + Sync + 'static,
    ) -> Self {
        self.before_attempt = Some(Arc::new(guard));
        self
    }

    /// Best-effort guard so a server echo can never leak the key through an
    /// error message (red line: keys never reach errors or logs).
    fn redact_key(&self, text: &str) -> String {
        if self.key.is_empty() {
            text.to_string()
        } else {
            text.replace(&self.key, "<redacted>")
        }
    }

    async fn send_attempt(&self, req: &ChatRequest) -> Result<reqwest::Response, reqwest::Error> {
        let url = format!(
            "{}{CHAT_COMPLETIONS_PATH}",
            self.base_url.trim_end_matches('/')
        );
        let request = self
            .http
            .post(url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("{AUTH_SCHEME} {}", self.key),
            )
            .json(&build_request_body(req))
            .build()?;
        #[cfg(test)]
        {
            let transport = self
                .test_transport
                .as_ref()
                .expect("OpenAI test request requires a registered transport");
            transport(request).await
        }
        #[cfg(all(not(test), feature = "test-support"))]
        {
            let _ = request;
            panic!("OpenAI test request requires an in-process provider fixture");
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            self.http.execute(request).await
        }
    }

    /// Sends the request (with retries) and returns the SSE event stream on
    /// the first successful response.
    async fn stream_response(
        self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, VegaError> {
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        crate::images::validate_messages(&req.messages)?;
        if let Some(reasoning) = &req.reasoning {
            reasoning.validate()?;
            if reasoning.model != req.model {
                return Err(VegaError::ReasoningSelectionInvalid {
                    message: "reasoning profile model does not match request model".to_string(),
                });
            }
        }
        let mut attempt: u32 = 0;
        loop {
            if let Some(guard) = &self.before_attempt {
                guard(&req)?;
            }
            let send = self.send_attempt(&req);
            let outcome = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(VegaError::Cancelled),
                outcome = send => outcome,
            };
            match outcome {
                Ok(resp) if resp.status().is_success() => {
                    let metadata = ProviderResponseMetadata {
                        http_status: Some(resp.status().as_u16()),
                        request_id: request_id_from_headers(resp.headers()),
                        retry_count: Some(attempt),
                    };
                    return Ok(ProviderStream {
                        events: event_stream(resp, cancel),
                        metadata,
                    });
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let request_id = request_id_from_headers(resp.headers());
                    let retry_after = retry_after_of(&resp);
                    let snippet = self.redact_key(&error_snippet(resp, &cancel).await);
                    let message =
                        format!("chat/completions request failed (HTTP {status}): {snippet}");
                    if !is_retryable_status(status) {
                        // 4xx（除 429）：重试无意义，立即失败
                        return Err(VegaError::ProviderDiagnostic {
                            kind: ProviderFailureKind::Rejected,
                            status: Some(status),
                            message,
                            retryable: false,
                            retry_count: Some(attempt),
                            request_id,
                        });
                    }
                    if attempt >= self.retry.max_retries {
                        return Err(VegaError::ProviderDiagnostic {
                            kind: ProviderFailureKind::Http,
                            status: Some(status),
                            message: format!(
                                "chat/completions request failed after {} retries (HTTP {status}): {snippet}",
                                self.retry.max_retries
                            ),
                            retryable: false,
                            retry_count: Some(attempt),
                            request_id,
                        });
                    }
                    sleep_cancellable(self.retry.delay_for(attempt, retry_after), &cancel).await?;
                    attempt += 1;
                }
                Err(err) => {
                    // 网络错误：指数退避后重建请求
                    if attempt >= self.retry.max_retries {
                        let message = self.redact_key(&format!(
                            "chat/completions request failed after {} retries: {err}",
                            self.retry.max_retries
                        ));
                        return Err(VegaError::ProviderDiagnostic {
                            kind: ProviderFailureKind::Transport,
                            status: None,
                            message,
                            retryable: false,
                            retry_count: Some(attempt),
                            request_id: None,
                        });
                    }
                    sleep_cancellable(self.retry.backoff(attempt), &cancel).await?;
                    attempt += 1;
                }
            }
        }
    }
}

impl Provider for OpenAiProvider {
    fn chat_stream_once(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let mut this = self.clone();
        this.retry.max_retries = 0;
        Box::pin(async move {
            this.stream_response(req, cancel)
                .await
                .map(|call| call.events)
        })
    }
    fn chat_stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let this = self.clone();
        Box::pin(async move {
            this.stream_response(req, cancel)
                .await
                .map(|call| call.events)
        })
    }

    fn chat_stream_once_with_metadata(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        let mut this = self.clone();
        this.retry.max_retries = 0;
        Box::pin(async move { this.stream_response(req, cancel).await })
    }

    fn chat_stream_with_metadata(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        let this = self.clone();
        Box::pin(async move { this.stream_response(req, cancel).await })
    }
}

fn request_id_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    ["x-request-id", "request-id", "openai-request-id"]
        .into_iter()
        .filter_map(|name| headers.get(name))
        .filter_map(|value| value.to_str().ok())
        .find(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        })
        .map(str::to_owned)
}

/// Serializes a [`ChatRequest`] into the OpenAI chat-completion body:
/// `stream: true` + `stream_options.include_usage` always on.
pub(crate) fn build_request_body(req: &ChatRequest) -> serde_json::Value {
    let messages = req
        .messages
        .iter()
        .map(|message| {
            let mut wire = serde_json::json!({
                "role": message.role.as_str(),
                "content": message.content,
            });
            // Issue 63 R7: keep text-only protocol unchanged; images remain in
            // the immutable user history for every subsequent tool round.
            if message.role == crate::ChatRole::User && !message.images.is_empty() {
                use base64::Engine;
                let mut parts = Vec::new();
                if !message.content.is_empty() {
                    parts.push(serde_json::json!({"type": "text", "text": message.content}));
                }
                for image in &message.images {
                    let encoded = base64::engine::general_purpose::STANDARD.encode(image.bytes());
                    parts.push(serde_json::json!({"type": "image_url", "image_url": {
                        "url": format!("data:{};base64,{encoded}", image.mime_type())
                    }}));
                }
                wire["content"] = serde_json::Value::Array(parts);
            }
            if let Some(reasoning_content) = &message.reasoning_content {
                wire["reasoning_content"] = serde_json::Value::String(reasoning_content.clone());
            }
            if let Some(call_id) = &message.tool_call_id {
                wire["tool_call_id"] = serde_json::Value::String(call_id.clone());
            }
            if !message.tool_calls.is_empty() {
                wire["tool_calls"] = serde_json::Value::Array(
                    message
                        .tool_calls
                        .iter()
                        .map(|call| {
                            serde_json::json!({
                                "id": call.id,
                                "type": "function",
                                "function": {
                                    "name": call.name,
                                    "arguments": call.input_json,
                                }
                            })
                        })
                        .collect(),
                );
            }
            wire
        })
        .collect::<Vec<_>>();
    let mut body = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !req.tools.is_empty() {
        body["tools"] = serde_json::Value::Array(
            req.tools
                .iter()
                .map(|t| {
                    let mut function = serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    });
                    if t.strict {
                        function["strict"] = serde_json::Value::Bool(true);
                    }
                    serde_json::json!({
                        "type": "function",
                        "function": function,
                    })
                })
                .collect(),
        );
    }
    if let Some(max_tokens) = req.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    if let Some(reasoning) = &req.reasoning {
        apply_reasoning_wire(&mut body, reasoning);
    }
    body
}

fn apply_reasoning_wire(body: &mut serde_json::Value, reasoning: &FrozenReasoning) {
    match &reasoning.choice {
        ReasoningChoice::ProviderDefault => {}
        ReasoningChoice::Effort(effort) => match reasoning.protocol {
            ReasoningProtocol::OpenAiChatCompletions => {
                body["reasoning_effort"] = serde_json::Value::String(effort.clone());
            }
            ReasoningProtocol::ZhipuChatCompletions => {
                // `reasoning_content` is retained and replayed by the bounded
                // in-memory run accumulator.  That is separate from Zhipu's
                // cross-round `clear_thinking` protocol switch, so never infer
                // the latter from the former.
                body["thinking"] = serde_json::json!({"type": "enabled"});
                body["reasoning_effort"] = serde_json::Value::String(effort.clone());
            }
            ReasoningProtocol::Unknown => {}
        },
        ReasoningChoice::Disabled => match reasoning.disabled_wire {
            Some(ReasoningDisabledWire::ThinkingTypeDisabled) => {
                body["thinking"] = serde_json::json!({"type": "disabled"});
            }
            Some(ReasoningDisabledWire::ReasoningEffortNone) => {
                body["reasoning_effort"] = serde_json::Value::String("none".to_string());
            }
            None => {}
        },
    }
}

fn is_retryable_status(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

fn retry_after_of(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_retry_after)
}

/// Reads a bounded snippet of an error response body for the error message.
async fn error_snippet(resp: reqwest::Response, cancel: &CancellationToken) -> String {
    let bytes = tokio::select! {
        biased;
        _ = cancel.cancelled() => return String::from("<cancelled while reading error body>"),
        result = resp.bytes() => match result {
            Ok(bytes) => bytes,
            Err(e) => return format!("<error body unavailable: {e}>"),
        },
    };
    String::from_utf8_lossy(&bytes).chars().take(512).collect()
}

mod sse;

pub(crate) use sse::*;

#[cfg(test)]
mod tests;
