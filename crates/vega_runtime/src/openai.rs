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
            .field("base_url", &self.base_url)
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
fn build_request_body(req: &ChatRequest) -> serde_json::Value {
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

/// Cancel-aware sleep: cancellation aborts the backoff immediately.
async fn sleep_cancellable(delay: Duration, cancel: &CancellationToken) -> Result<(), VegaError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(VegaError::Cancelled),
        _ = tokio::time::sleep(delay) => Ok(()),
    }
}

/// The raw SSE stream (byte stream wrapped by `eventsource-stream`), boxed so
/// the pipeline state below has a nameable type.
type RawSse =
    Pin<Box<dyn Stream<Item = Result<RawEvent, EventStreamError<reqwest::Error>>> + Send>>;

/// Pipeline state for the SSE → [`ProviderEvent`] stream.
struct SsePipeline {
    inner: RawSse,
    assembler: SseAssembler,
    pending: VecDeque<Result<ProviderEvent, VegaError>>,
    done: bool,
    cancel: CancellationToken,
}

/// Wraps a successful SSE response into the boxed event stream.
///
/// - chunks are parsed incrementally into events;
/// - `data: [DONE]` (or stream end) flushes buffered tool calls and emits
///   the terminal [`ProviderEvent::Done`];
/// - cancellation tears the stream down immediately with no further events;
/// - errors after stream establishment are terminal (retries only apply to
///   request setup — consumed increments cannot be replayed).
fn event_stream(resp: reqwest::Response, cancel: CancellationToken) -> EventStream {
    let inner: RawSse = Box::pin(resp.bytes_stream().eventsource());
    let state = SsePipeline {
        inner,
        assembler: SseAssembler::default(),
        pending: VecDeque::new(),
        done: false,
        cancel,
    };
    let stream = futures::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(item) = st.pending.pop_front() {
                return Some((item, st));
            }
            if st.done {
                return None;
            }
            // 取消与下一个 SSE 事件竞争：取消立即断流且不再产生事件
            let next = tokio::select! {
                biased;
                _ = st.cancel.cancelled() => return None,
                item = st.inner.next() => item,
            };
            match next {
                Some(Ok(event)) => {
                    if event.data.trim() == DONE_SENTINEL {
                        st.done = true;
                        st.pending
                            .extend(st.assembler.finalize().into_iter().map(Ok));
                    } else {
                        match st.assembler.absorb(&event.data) {
                            Ok(events) => st.pending.extend(events.into_iter().map(Ok)),
                            Err(err) => {
                                st.done = true;
                                st.pending.push_back(Err(err));
                            }
                        }
                    }
                }
                Some(Err(err)) => {
                    st.done = true;
                    st.pending.push_back(Err(map_sse_error(err)));
                }
                None => {
                    st.done = true;
                    st.pending
                        .extend(st.assembler.finalize().into_iter().map(Ok));
                }
            }
        }
    });
    Box::pin(stream)
}

fn map_sse_error(err: EventStreamError<reqwest::Error>) -> VegaError {
    VegaError::Provider {
        status: None,
        message: format!("SSE stream error: {err}"),
        retryable: false,
    }
}

/// One `delta.tool_calls` fragment slot, keyed by the wire `index` field.
#[derive(Debug, Default)]
struct ToolFragment {
    id: String,
    name: String,
    arguments: String,
}

/// Incremental assembler from OpenAI chat-completion chunks to
/// [`ProviderEvent`]s (tech-spec §4.1):
///
/// - `choices[0].delta.content` → [`ProviderEvent::TextDelta`]
/// - `choices[0].delta.reasoning_content` → [`ProviderEvent::ThinkingDelta`]
/// - `choices[0].delta.tool_calls` → aggregated by index; [`ProviderEvent::
///   ToolUse`] only once fragments complete (flushed on `finish_reason` or
///   stream finalization)
/// - `usage` → [`ProviderEvent::Usage`]
/// - `choices[0].finish_reason` → the terminal [`StopReason`]
///
/// Unknown JSON fields are ignored.
#[derive(Debug, Default)]
struct SseAssembler {
    tools: BTreeMap<u64, ToolFragment>,
    finish_reason: Option<StopReason>,
}

impl SseAssembler {
    /// Absorbs one SSE `data:` payload (one JSON chunk; the `[DONE]`
    /// sentinel is handled by the caller). Returns that chunk's events.
    fn absorb(&mut self, data: &str) -> Result<Vec<ProviderEvent>, VegaError> {
        let chunk: serde_json::Value =
            serde_json::from_str(data).map_err(|e| VegaError::Provider {
                status: None,
                message: format!("invalid SSE chunk JSON: {e}"),
                retryable: false,
            })?;
        let mut events = Vec::new();
        // usage：stream_options.include_usage 的最终 chunk（choices 为空数组）
        if let Some(usage) = chunk.get("usage").filter(|u| !u.is_null()) {
            events.push(usage_event(usage));
        }
        if let Some(choice) = chunk.get("choices").and_then(|c| c.get(0)) {
            if let Some(delta) = choice.get("delta") {
                if let Some(text) = str_field(delta, "content")
                    && !text.is_empty()
                {
                    events.push(ProviderEvent::TextDelta(text.to_string()));
                }
                if let Some(text) = str_field(delta, "reasoning_content")
                    && !text.is_empty()
                {
                    events.push(ProviderEvent::ThinkingDelta(text.to_string()));
                }
                if let Some(calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                    for call in calls {
                        self.absorb_tool_call(call);
                    }
                }
            }
            if let Some(reason) = str_field(choice, "finish_reason") {
                self.finish_reason = Some(map_finish_reason(reason));
                // tool_calls 收敛：分片聚合完整后才发 ToolUse
                if self.finish_reason == Some(StopReason::ToolUse) {
                    events.extend(self.flush_tools());
                }
            }
        }
        Ok(events)
    }

    fn absorb_tool_call(&mut self, call: &serde_json::Value) {
        let index = call.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
        let fragment = self.tools.entry(index).or_default();
        if let Some(id) = str_field(call, "id") {
            fragment.id = id.to_string();
        }
        let function = call.get("function");
        if let Some(name) = function
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
        {
            fragment.name = name.to_string();
        }
        if let Some(args) = function
            .and_then(|f| f.get("arguments"))
            .and_then(|a| a.as_str())
        {
            // 分片拼装：只拼字符串，不做半截 JSON 解析
            fragment.arguments.push_str(args);
        }
    }

    /// Emits the terminal events: any still-buffered tool calls (in index
    /// order), then `Done` with the mapped finish reason (default `End`).
    fn finalize(&mut self) -> Vec<ProviderEvent> {
        let mut events = self.flush_tools();
        events.push(ProviderEvent::Done {
            stop_reason: self.finish_reason.unwrap_or(StopReason::End),
        });
        events
    }

    fn flush_tools(&mut self) -> Vec<ProviderEvent> {
        let tools = std::mem::take(&mut self.tools);
        tools
            .into_values()
            .map(|fragment| ProviderEvent::ToolUse {
                id: fragment.id,
                name: fragment.name,
                input_json: fragment.arguments,
            })
            .collect()
    }
}

fn str_field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

/// Maps the wire `finish_reason` onto the minimal [`StopReason`] set.
fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" | "max_tokens" => StopReason::Length,
        _ => StopReason::End,
    }
}

/// Maps the final usage chunk onto [`ProviderEvent::Usage`].
///
/// `prompt_tokens_details.cached_tokens` is the OpenAI-compatible cache-hit
/// counter; cache-write reporting has no cross-vendor wire field, so it
/// stays 0 until a provider documents one.
fn usage_event(usage: &serde_json::Value) -> ProviderEvent {
    let get = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    ProviderEvent::Usage {
        input: get("prompt_tokens"),
        output: get("completion_tokens"),
        cache_read,
        cache_write: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatMessage, ChatRole, ChatToolCall, ToolDefinition};
    use std::future::Future;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    const KEY: &str = "vrg-test-key-123";
    const MODEL: &str = "test-model";

    #[test]
    fn follow_up_messages_serialize_exact_tool_call_wire_shape() {
        let request = ChatRequest {
            model: MODEL.into(),
            messages: vec![
                ChatMessage::assistant_with_tools(
                    "checking",
                    vec![ChatToolCall {
                        id: "call-7".into(),
                        name: "read".into(),
                        input_json: r#"{"path":"src/lib.rs"}"#.into(),
                    }],
                ),
                ChatMessage::tool_result("call-7", "1 | fn main() {}"),
            ],
            ..Default::default()
        };

        let wire = build_request_body(&request);
        assert_eq!(
            wire["messages"][0],
            serde_json::json!({
                "role": "assistant",
                "content": "checking",
                "tool_calls": [{
                    "id": "call-7",
                    "type": "function",
                    "function": {
                        "name": "read",
                        "arguments": "{\"path\":\"src/lib.rs\"}"
                    }
                }]
            })
        );
        assert_eq!(
            wire["messages"][1],
            serde_json::json!({
                "role": "tool",
                "content": "1 | fn main() {}",
                "tool_call_id": "call-7"
            })
        );
    }

    // ---------- 纯单元：SseAssembler ----------

    fn absorb_all(assembler: &mut SseAssembler, chunks: &[&str]) -> Vec<ProviderEvent> {
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(assembler.absorb(chunk).unwrap());
        }
        events
    }

    #[test]
    fn text_deltas_with_unknown_fields_ignored() {
        let mut assembler = SseAssembler::default();
        let events = absorb_all(
            &mut assembler,
            &[
                r#"{"id":"c1","object":"chat.completion.chunk","created":1,"model":"m","system_fingerprint":"fp","choices":[{"index":0,"delta":{"content":"Hel"},"logprobs":null,"finish_reason":null}]}"#,
                r#"{"choices":[{"delta":{"content":"lo"},"service_tier":"default"}]}"#,
            ],
        );
        assert_eq!(
            events,
            vec![
                ProviderEvent::TextDelta("Hel".into()),
                ProviderEvent::TextDelta("lo".into()),
            ]
        );
        assert_eq!(
            assembler.finalize(),
            vec![ProviderEvent::Done {
                stop_reason: StopReason::End
            }]
        );
    }

    #[test]
    fn reasoning_content_becomes_thinking_delta() {
        let mut assembler = SseAssembler::default();
        let events = absorb_all(
            &mut assembler,
            &[r#"{"choices":[{"delta":{"reasoning_content":"pondering"}}]}"#],
        );
        assert_eq!(
            events,
            vec![ProviderEvent::ThinkingDelta("pondering".into())]
        );
    }

    #[test]
    fn tool_call_fragments_aggregate_into_single_tool_use() {
        let mut assembler = SseAssembler::default();
        // 同一 tool_call 的 arguments 分 3 片到达
        let events = absorb_all(
            &mut assembler,
            &[
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_9","type":"function","function":{"name":"read","arguments":"{\"pa"}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"src"}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"}"}}]}}]}"#,
            ],
        );
        assert!(events.is_empty(), "fragments must not emit partial events");
        // finish_reason 才触发完整 ToolUse
        let flushed = absorb_all(
            &mut assembler,
            &[r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#],
        );
        assert_eq!(
            flushed,
            vec![ProviderEvent::ToolUse {
                id: "call_9".into(),
                name: "read".into(),
                input_json: r#"{"path":"src"}"#.into(),
            }]
        );
        assert_eq!(
            assembler.finalize(),
            vec![ProviderEvent::Done {
                stop_reason: StopReason::ToolUse
            }]
        );
    }

    #[test]
    fn multiple_tool_calls_flush_in_index_order() {
        let mut assembler = SseAssembler::default();
        let events = absorb_all(
            &mut assembler,
            &[
                r#"{"choices":[{"delta":{"tool_calls":[
                    {"index":1,"id":"b","function":{"name":"grep","arguments":"{}"}},
                    {"index":0,"id":"a","function":{"name":"read","arguments":"{}"}}
                ]}}]}"#,
                r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            ],
        );
        let ids: Vec<&str> = events
            .iter()
            .map(|e| match e {
                ProviderEvent::ToolUse { id, .. } => id.as_str(),
                other => panic!("unexpected event {other:?}"),
            })
            .collect();
        assert_eq!(ids, vec!["a", "b"], "BTreeMap keeps index order");
    }

    #[test]
    fn usage_chunk_maps_to_usage_event() {
        let mut assembler = SseAssembler::default();
        let events = absorb_all(
            &mut assembler,
            &[
                r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":21,"prompt_tokens_details":{"cached_tokens":80}}}"#,
            ],
        );
        assert_eq!(
            events,
            vec![ProviderEvent::Usage {
                input: 100,
                output: 21,
                cache_read: 80,
                cache_write: 0,
            }]
        );
    }

    #[test]
    fn missing_usage_emits_no_usage_event() {
        let mut assembler = SseAssembler::default();
        let events = absorb_all(
            &mut assembler,
            &[
                r#"{"choices":[{"delta":{"content":"hi"}}]}"#,
                r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            ],
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ProviderEvent::Usage { .. }))
        );
    }

    #[test]
    fn finish_reason_maps_to_minimal_stop_reasons() {
        for (wire, expected) in [
            ("stop", StopReason::End),
            ("tool_calls", StopReason::ToolUse),
            ("function_call", StopReason::ToolUse),
            ("length", StopReason::Length),
            ("max_tokens", StopReason::Length),
            ("content_filter", StopReason::End),
        ] {
            let mut assembler = SseAssembler::default();
            assembler
                .absorb(&format!(
                    r#"{{"choices":[{{"delta":{{}},"finish_reason":"{wire}"}}]}}"#
                ))
                .unwrap();
            assert_eq!(
                assembler.finalize(),
                vec![ProviderEvent::Done {
                    stop_reason: expected
                }]
            );
        }
    }

    #[test]
    fn invalid_json_chunk_is_a_provider_error() {
        let mut assembler = SseAssembler::default();
        let err = assembler.absorb("not json").unwrap_err();
        assert!(matches!(
            err,
            VegaError::Provider {
                retryable: false,
                ..
            }
        ));
    }

    #[test]
    fn empty_content_delta_is_skipped() {
        let mut assembler = SseAssembler::default();
        let events = absorb_all(
            &mut assembler,
            &[r#"{"choices":[{"delta":{"content":""}}]}"#],
        );
        assert!(events.is_empty());
    }

    // ---------- 纯单元：跨 chunk SSE 切分 ----------

    #[tokio::test]
    async fn sse_events_reassemble_across_chunk_boundaries() {
        // 切点故意落在 JSON token / 键名 / data: 前缀 / 空行中间
        let chunks: Vec<Result<&[u8], std::io::Error>> = vec![
            Ok(b"data: {\"choi"),
            Ok(b"ces\":[{\"delta\":{\"content\":\"He"),
            Ok(b"llo\"}}]}\n"),
            Ok(b"\n"),
            Ok(b"da"),
            Ok(b"ta: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n"),
            Ok(b"data: [DONE]\n\ndata: {\"ignored\":true}\n\n"),
        ];
        let sse = futures::stream::iter(chunks).eventsource();
        let raw: Vec<_> = sse.collect().await;
        // 4 个完整 SSE 事件：两个内容 chunk、[DONE]、以及 [DONE] 后的 junk 事件
        assert_eq!(raw.len(), 4, "SSE events must reassemble across chunks");

        let mut assembler = SseAssembler::default();
        let mut events = Vec::new();
        for item in raw {
            let event = item.unwrap();
            if event.data.trim() == DONE_SENTINEL {
                break;
            }
            events.extend(assembler.absorb(&event.data).unwrap());
        }
        // [DONE] 后的 junk 事件被丢弃（不在 events 里，也不再 absorb）
        events.extend(assembler.finalize());
        assert_eq!(
            events,
            vec![
                ProviderEvent::TextDelta("Hello".into()),
                ProviderEvent::TextDelta(" world".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End
                },
            ]
        );
    }

    // ---------- 本地 HTTP 服务器 ----------

    type HandlerFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
    type Handler = Arc<dyn Fn(u64, TcpStream) -> HandlerFuture + Send + Sync>;

    #[derive(Debug, Clone)]
    struct CapturedRequest {
        authorization: String,
        body: serde_json::Value,
    }

    struct TestServer {
        addr: SocketAddr,
        connections: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<CapturedRequest>>>,
    }

    impl TestServer {
        fn connection_count(&self) -> usize {
            self.connections.load(Ordering::SeqCst)
        }

        fn captured(&self) -> Vec<CapturedRequest> {
            let requests = self.requests.lock().unwrap();
            Vec::clone(&requests)
        }
    }

    async fn read_request(stream: &mut TcpStream) -> std::io::Result<(String, Vec<u8>)> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end;
        loop {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "client closed before request head completed",
                ));
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = find(&buf, b"\r\n\r\n") {
                head_end = pos;
                break;
            }
        }
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let content_length: usize = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.trim().eq_ignore_ascii_case("content-length") {
                    value.trim().parse().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        let body_start = head_end + 4;
        while buf.len() < body_start + content_length {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let body = buf[body_start..(body_start + content_length).min(buf.len())].to_vec();
        Ok((head, body))
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    async fn spawn_server(handler: Handler) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let requests: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let t_connections = Arc::clone(&connections);
        let t_requests = Arc::clone(&requests);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let idx = t_connections.fetch_add(1, Ordering::SeqCst) as u64;
                let handler = Arc::clone(&handler);
                let requests = Arc::clone(&t_requests);
                tokio::spawn(async move {
                    let Ok((head, body)) = read_request(&mut stream).await else {
                        return;
                    };
                    let authorization = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.trim().eq_ignore_ascii_case("authorization") {
                                Some(value.trim().to_string())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_default();
                    let body = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
                    requests.lock().unwrap().push(CapturedRequest {
                        authorization,
                        body,
                    });
                    handler(idx, stream).await;
                });
            }
        });
        TestServer {
            addr,
            connections,
            requests,
        }
    }

    fn http_head(status: &str, extra: &[(&str, &str)]) -> Vec<u8> {
        let mut head = format!("HTTP/1.1 {status}\r\n");
        for (name, value) in extra {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("Connection: close\r\n\r\n");
        head.into_bytes()
    }

    /// Builds a full SSE response (body terminated by connection close).
    fn sse_response(events: &[&str], done: bool) -> Vec<u8> {
        let mut resp = http_head("200 OK", &[("Content-Type", "text/event-stream")]);
        for event in events {
            resp.extend_from_slice(format!("data: {event}\n\n").as_bytes());
        }
        if done {
            resp.extend_from_slice(b"data: [DONE]\n\n");
        }
        resp
    }

    fn status_response(status: &str, extra: &[(&str, &str)], body: &str) -> Vec<u8> {
        let mut resp = http_head(status, extra);
        resp.extend_from_slice(body.as_bytes());
        resp
    }

    /// Server that answers each connection with the next canned response
    /// (or closes silently once the script is exhausted).
    fn scripted_server(responses: Vec<Vec<u8>>) -> Handler {
        Arc::new(move |idx: u64, mut stream: TcpStream| {
            let response = responses.get(idx as usize).cloned();
            Box::pin(async move {
                if let Some(response) = response {
                    let _ = stream.write_all(&response).await;
                    let _ = stream.flush().await;
                }
            }) as HandlerFuture
        })
    }

    fn fast_policy(base_ms: u64) -> RetryPolicy {
        RetryPolicy {
            base_delay: Duration::from_millis(base_ms),
            ..RetryPolicy::default()
        }
    }

    fn provider_for(server: &TestServer, policy: RetryPolicy) -> OpenAiProvider {
        OpenAiProvider::new(format!("http://{}", server.addr), KEY)
            .unwrap()
            .with_retry_policy(policy)
    }

    fn request() -> ChatRequest {
        ChatRequest {
            model: MODEL.to_string(),
            messages: vec![ChatMessage::new(ChatRole::User, "hello")],
            ..Default::default()
        }
    }

    fn usage_chunk() -> &'static str {
        r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":6}}}"#
    }

    async fn collect_events(
        stream: EventStream,
        limit: usize,
    ) -> Vec<Result<ProviderEvent, VegaError>> {
        let mut stream = stream;
        let mut out = Vec::new();
        for _ in 0..limit {
            match tokio::time::timeout(Duration::from_secs(10), stream.next()).await {
                Ok(Some(item)) => out.push(item),
                Ok(None) => break,
                Err(_) => panic!("stream stalled beyond 10s"),
            }
        }
        out
    }

    /// Element-wise comparison of stream items: `Ok` items via
    /// `ProviderEvent: PartialEq`, `Err` items via `VegaError`'s `Display`
    /// (it wraps non-`PartialEq` std errors).
    fn assert_items_eq(
        actual: &[Result<ProviderEvent, VegaError>],
        expected: &[Result<ProviderEvent, VegaError>],
    ) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "item count mismatch: {actual:?} vs {expected:?}"
        );
        for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            match (a, e) {
                (Ok(a), Ok(e)) => assert_eq!(a, e, "item #{i}"),
                (Err(a), Err(e)) => assert_eq!(a.to_string(), e.to_string(), "item #{i}"),
                _ => panic!("item #{i} mismatch: {a:?} vs {e:?}"),
            }
        }
    }

    // ---------- 集成：请求/响应走本地 TCP ----------

    #[tokio::test]
    async fn happy_path_sends_openai_wire_format_and_streams_events() {
        let body = sse_response(
            &[
                r#"{"choices":[{"delta":{"content":"Hi"},"finish_reason":null}]}"#,
                usage_chunk(),
                r#"{"junk-after-usage":true}"#,
            ],
            true,
        );
        let server = spawn_server(scripted_server(vec![body])).await;
        let provider = provider_for(&server, fast_policy(25));

        let req = ChatRequest {
            model: MODEL.to_string(),
            messages: vec![ChatMessage::new(ChatRole::User, "hello")],
            tools: vec![ToolDefinition {
                name: "read".into(),
                description: "read a file".into(),
                input_schema: serde_json::json!({"type":"object"}),
            }],
            max_tokens: Some(64),
        };
        let cancel = CancellationToken::new();
        let stream =
            tokio::time::timeout(Duration::from_secs(10), provider.chat_stream(req, cancel))
                .await
                .expect("chat_stream stalled")
                .unwrap();
        let events = collect_events(stream, 8).await;
        assert_items_eq(
            &events,
            &[
                Ok(ProviderEvent::TextDelta("Hi".into())),
                Ok(ProviderEvent::Usage {
                    input: 10,
                    output: 2,
                    cache_read: 6,
                    cache_write: 0,
                }),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ],
        );

        let captured = server.captured();
        assert_eq!(captured.len(), 1);
        // Authorization 只出现在请求头；body/路径不带 key
        assert_eq!(captured[0].authorization, format!("{AUTH_SCHEME} {KEY}"));
        let wire = &captured[0].body;
        assert_eq!(wire["model"], MODEL);
        assert_eq!(wire["stream"], true);
        assert_eq!(wire["stream_options"]["include_usage"], true);
        assert_eq!(wire["max_tokens"], 64);
        assert_eq!(wire["messages"][0]["role"], "user");
        assert_eq!(wire["messages"][0]["content"], "hello");
        assert_eq!(wire["tools"][0]["type"], "function");
        assert_eq!(wire["tools"][0]["function"]["name"], "read");
        assert_eq!(wire["tools"][0]["function"]["parameters"]["type"], "object");
    }

    #[tokio::test]
    async fn retries_5xx_with_backoff_then_succeeds() {
        let ok = sse_response(&[r#"{"choices":[{"delta":{"content":"Hi"}}]}"#], true);
        let server = spawn_server(scripted_server(vec![
            status_response("500 Internal Server Error", &[], "boom"),
            status_response("500 Internal Server Error", &[], "boom"),
            ok,
        ]))
        .await;
        let started = std::time::Instant::now();
        let provider = provider_for(&server, fast_policy(25));
        let stream = tokio::time::timeout(
            Duration::from_secs(10),
            provider.chat_stream(request(), CancellationToken::new()),
        )
        .await
        .expect("chat_stream stalled")
        .unwrap();
        let events = collect_events(stream, 4).await;
        assert_items_eq(
            &events,
            &[
                Ok(ProviderEvent::TextDelta("Hi".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ],
        );
        assert_eq!(server.connection_count(), 3, "2 failures + 1 success");
        // 退避被调用：两次延迟 25ms + 50ms（下界校验，tokio sleep 不会提前触发）
        assert!(started.elapsed() >= Duration::from_millis(70));
    }

    #[tokio::test]
    async fn retry_429_honors_retry_after_header() {
        let ok = sse_response(&[r#"{"choices":[{"delta":{"content":"Hi"}}]}"#], true);
        let server = spawn_server(scripted_server(vec![
            status_response(
                "429 Too Many Requests",
                &[("Retry-After", "0")],
                "slow down",
            ),
            ok,
        ]))
        .await;
        let started = std::time::Instant::now();
        // 指数退避会是 1s；尊重 Retry-After: 0 应几乎立即重试
        let provider = provider_for(&server, RetryPolicy::default());
        let stream = tokio::time::timeout(
            Duration::from_secs(10),
            provider.chat_stream(request(), CancellationToken::new()),
        )
        .await
        .expect("chat_stream stalled")
        .unwrap();
        let events = collect_events(stream, 4).await;
        assert_eq!(events.len(), 2, "expected the retried stream to succeed");
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "Retry-After: 0 must be honored over the 1s exponential schedule"
        );
        assert_eq!(server.connection_count(), 2);
    }

    #[tokio::test]
    async fn retry_429_without_retry_after_falls_back_to_backoff() {
        let ok = sse_response(&[r#"{"choices":[{"delta":{"content":"Hi"}}]}"#], true);
        let server = spawn_server(scripted_server(vec![
            status_response("429 Too Many Requests", &[], "slow down"),
            ok,
        ]))
        .await;
        let started = std::time::Instant::now();
        let provider = provider_for(&server, fast_policy(25));
        let stream = tokio::time::timeout(
            Duration::from_secs(10),
            provider.chat_stream(request(), CancellationToken::new()),
        )
        .await
        .expect("chat_stream stalled")
        .unwrap();
        collect_events(stream, 4).await;
        assert_eq!(server.connection_count(), 2);
        assert!(started.elapsed() >= Duration::from_millis(20));
    }

    #[tokio::test]
    async fn retries_exhausted_returns_non_retryable_provider_error() {
        let server = spawn_server(scripted_server(vec![
            status_response("500 Internal Server Error", &[], "boom"),
            status_response("500 Internal Server Error", &[], "boom"),
            status_response("500 Internal Server Error", &[], "boom"),
            status_response("500 Internal Server Error", &[], "boom"),
        ]))
        .await;
        let provider = provider_for(&server, fast_policy(1));
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            provider.chat_stream(request(), CancellationToken::new()),
        )
        .await
        .expect("chat_stream stalled");
        let Err(err) = result else {
            panic!("expected exhausted provider error, got a successful stream");
        };
        match err {
            VegaError::Provider {
                status,
                message,
                retryable,
            } => {
                assert_eq!(status, Some(500));
                assert!(
                    !retryable,
                    "exhausted retries must not advertise retryability"
                );
                assert!(message.contains("after 3 retries"), "{message}");
                assert!(!message.contains(KEY), "key must never leak: {message}");
            }
            other => panic!("expected exhausted provider error, got {other:?}"),
        }
        assert_eq!(server.connection_count(), 4, "1 initial + 3 retries");
    }

    #[tokio::test]
    async fn network_error_is_retried_then_succeeds() {
        // 连接 0：读完请求后直接断开（模拟网络错误）；连接 1：正常响应
        let handler: Handler = Arc::new(move |idx: u64, mut stream: TcpStream| {
            Box::pin(async move {
                if idx == 1 {
                    let ok = sse_response(&[r#"{"choices":[{"delta":{"content":"Hi"}}]}"#], true);
                    let _ = stream.write_all(&ok).await;
                    let _ = stream.flush().await;
                }
                // idx == 0：直接 drop，客户端读到连接中断
            }) as HandlerFuture
        });
        let server = spawn_server(handler).await;
        let provider = provider_for(&server, fast_policy(5));
        let stream = tokio::time::timeout(
            Duration::from_secs(10),
            provider.chat_stream(request(), CancellationToken::new()),
        )
        .await
        .expect("chat_stream stalled")
        .unwrap();
        let events = collect_events(stream, 4).await;
        assert_items_eq(
            &events,
            &[
                Ok(ProviderEvent::TextDelta("Hi".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ],
        );
        assert_eq!(server.connection_count(), 2);
    }

    #[tokio::test]
    async fn non_retryable_4xx_fails_without_retry() {
        let server = spawn_server(scripted_server(vec![status_response(
            "401 Unauthorized",
            &[],
            r#"{"error":{"message":"invalid credentials"}}"#,
        )]))
        .await;
        let provider = provider_for(&server, fast_policy(25));
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            provider.chat_stream(request(), CancellationToken::new()),
        )
        .await
        .expect("chat_stream stalled");
        let Err(err) = result else {
            panic!("expected 401 provider error, got a successful stream");
        };
        match err {
            VegaError::Provider {
                status,
                message,
                retryable,
            } => {
                assert_eq!(status, Some(401));
                assert!(!retryable);
                assert!(message.contains("invalid credentials"), "{message}");
            }
            other => panic!("expected 401 provider error, got {other:?}"),
        }
        assert_eq!(server.connection_count(), 1, "4xx must not be retried");
    }

    #[tokio::test]
    async fn error_body_echoing_the_key_is_redacted() {
        let server = spawn_server(scripted_server(vec![status_response(
            "400 Bad Request",
            &[],
            &format!(r#"{{"error":{{"message":"bad key: {KEY}"}}}}"#),
        )]))
        .await;
        let provider = provider_for(&server, fast_policy(25));
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            provider.chat_stream(request(), CancellationToken::new()),
        )
        .await
        .expect("chat_stream stalled");
        let Err(err) = result else {
            panic!("expected provider error, got a successful stream");
        };
        match err {
            VegaError::Provider { message, .. } => {
                assert!(!message.contains(KEY), "key must never leak: {message}");
                assert!(message.contains("<redacted>"), "{message}");
            }
            other => panic!("expected provider error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn already_cancelled_token_fails_fast_without_connecting() {
        let server = spawn_server(scripted_server(vec![])).await;
        let provider = provider_for(&server, fast_policy(1));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = provider.chat_stream(request(), cancel).await;
        assert!(matches!(result, Err(VegaError::Cancelled)));
        assert_eq!(
            server.connection_count(),
            0,
            "cancelled request must not connect"
        );
    }

    #[tokio::test]
    async fn cancel_during_backoff_aborts_without_another_request() {
        // 永远 503；取消发生在第一次退避期间
        let handler: Handler = Arc::new(|_idx: u64, mut stream: TcpStream| {
            Box::pin(async move {
                let resp = status_response("503 Service Unavailable", &[], "unavailable");
                let _ = stream.write_all(&resp).await;
                let _ = stream.flush().await;
            }) as HandlerFuture
        });
        let server = spawn_server(handler).await;
        let provider = provider_for(
            &server,
            RetryPolicy {
                base_delay: Duration::from_secs(30),
                ..RetryPolicy::default()
            },
        );
        let cancel = CancellationToken::new();
        let request_cancel = cancel.clone();
        let started = std::time::Instant::now();
        let task =
            tokio::spawn(async move { provider.chat_stream(request(), request_cancel).await });
        // 等第一个 503 处理完（进入 30s 退避），再取消
        tokio::time::sleep(Duration::from_millis(500)).await;
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("cancel during backoff must abort immediately")
            .unwrap();
        assert!(matches!(result, Err(VegaError::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            server.connection_count(),
            1,
            "no request after cancellation"
        );
    }

    #[tokio::test]
    async fn cancel_mid_stream_stops_immediately_with_no_further_events() {
        // 服务器发出第一个事件后挂住连接不关闭
        let handler: Handler = Arc::new(|_idx: u64, mut stream: TcpStream| {
            Box::pin(async move {
                let head = sse_response(&[r#"{"choices":[{"delta":{"content":"Hel"}}]}"#], false);
                let _ = stream.write_all(&head).await;
                let _ = stream.flush().await;
                // 保持连接打开，客户端在流中取消
                tokio::time::sleep(Duration::from_secs(30)).await;
            }) as HandlerFuture
        });
        let server = spawn_server(handler).await;
        let provider = provider_for(&server, fast_policy(1));
        let cancel = CancellationToken::new();
        let mut stream = tokio::time::timeout(
            Duration::from_secs(10),
            provider.chat_stream(request(), cancel.clone()),
        )
        .await
        .expect("chat_stream stalled")
        .unwrap();
        match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
            Ok(Some(Ok(ev))) => assert_eq!(ev, ProviderEvent::TextDelta("Hel".into())),
            other => panic!("expected first text delta, got {other:?}"),
        }
        // 流中取消：立即断且不再产生任何事件
        cancel.cancel();
        let started = std::time::Instant::now();
        let rest = collect_events(stream, 4).await;
        assert!(
            rest.is_empty(),
            "no events after cancellation, got {rest:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "cancel must tear the stream down immediately"
        );
        drop(server);
    }
}
