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
use std::time::Duration;

use eventsource_stream::{Event as RawEvent, EventStreamError, Eventsource};
use futures::future::BoxFuture;
use futures::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::error::VegaError;
use crate::provider::{ChatRequest, EventStream, Provider, ProviderEvent, StopReason};
use crate::retry::{RetryPolicy, parse_retry_after};

/// Authorization scheme for OpenAI-compatible APIs (used for the single
/// header construction below; tests reference this constant).
const AUTH_SCHEME: &str = "Bearer";
/// Terminal SSE sentinel of the OpenAI chat-completion stream.
const DONE_SENTINEL: &str = "[DONE]";
/// Base path appended to the configured base URL.
const CHAT_COMPLETIONS_PATH: &str = "/chat/completions";

/// OpenAI-compatible streaming chat provider.
///
/// Cloning is cheap (the HTTP client is an internal `Arc`); every clone
/// still redacts the key from its `Debug` output.
#[derive(Clone)]
pub struct OpenAiProvider {
    http: reqwest::Client,
    /// Endpoint root, e.g. `https://api.openai.com/v1` (trailing slash ok).
    base_url: String,
    /// API key; only ever serialized into the Authorization request header.
    key: String,
    /// Retry schedule applied while establishing the stream.
    retry: RetryPolicy,
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
            base_url: base_url.into(),
            key: key.into(),
            retry: RetryPolicy::default(),
        })
    }

    /// Overrides the retry schedule (defaults: 1s / 2s / 4s, 3 retries).
    pub fn with_retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
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
        self.http
            .post(url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("{AUTH_SCHEME} {}", self.key),
            )
            .json(&build_request_body(req))
            .send()
            .await
    }

    /// Sends the request (with retries) and returns the SSE event stream on
    /// the first successful response.
    async fn stream_response(
        self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<EventStream, VegaError> {
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        let mut attempt: u32 = 0;
        loop {
            let send = self.send_attempt(&req);
            let outcome = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(VegaError::Cancelled),
                outcome = send => outcome,
            };
            match outcome {
                Ok(resp) if resp.status().is_success() => {
                    return Ok(event_stream(resp, cancel));
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let retry_after = retry_after_of(&resp);
                    let snippet = self.redact_key(&error_snippet(resp, &cancel).await);
                    let message =
                        format!("chat/completions request failed (HTTP {status}): {snippet}");
                    if !is_retryable_status(status) {
                        // 4xx（除 429）：重试无意义，立即失败
                        return Err(VegaError::Provider {
                            status: Some(status),
                            message,
                            retryable: false,
                        });
                    }
                    if attempt >= self.retry.max_retries {
                        return Err(VegaError::Provider {
                            status: Some(status),
                            message: format!(
                                "chat/completions request failed after {} retries (HTTP {status}): {snippet}",
                                self.retry.max_retries
                            ),
                            retryable: false,
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
                        return Err(VegaError::Provider {
                            status: None,
                            message,
                            retryable: false,
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
    fn chat_stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let this = self.clone();
        Box::pin(async move { this.stream_response(req, cancel).await })
    }
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
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema,
                        },
                    })
                })
                .collect(),
        );
    }
    if let Some(max_tokens) = req.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    body
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
