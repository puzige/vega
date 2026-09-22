use super::*;
use crate::provider::{
    ChatMessage, ChatRole, ChatToolCall, FrozenReasoning, ReasoningChoice, ReasoningDisabledWire,
    ReasoningProtocol, ToolDefinition,
};
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

#[test]
fn issue85_vega_tools_emit_strict_chat_completions_wire() {
    let request = ChatRequest {
        model: MODEL.into(),
        tools: crate::tool_definitions(crate::RuntimeRunMode::Execute),
        ..Default::default()
    };

    let wire = build_request_body(&request);
    let functions = wire["tools"].as_array().unwrap();
    assert_eq!(functions.len(), 6);
    assert!(
        functions
            .iter()
            .all(|tool| tool["function"]["strict"] == true)
    );
}

#[test]
fn issue85_non_strict_tools_do_not_claim_strict_wire() {
    let request = ChatRequest {
        model: MODEL.into(),
        tools: vec![ToolDefinition {
            name: "external".into(),
            description: "external tool".into(),
            input_schema: serde_json::json!({"type": "object"}),
            strict: false,
        }],
        ..Default::default()
    };

    let wire = build_request_body(&request);
    assert!(wire["tools"][0]["function"].get("strict").is_none());
}

#[test]
fn zhipu_enabled_effort_omits_cross_round_clear_switch() {
    let request = ChatRequest {
        model: "glm-5.3".into(),
        reasoning: Some(FrozenReasoning {
            provider: "zhipu".into(),
            model: "glm-5.3".into(),
            protocol: ReasoningProtocol::ZhipuChatCompletions,
            choice: ReasoningChoice::Effort("max".into()),
            supports_disabled: false,
            preserve_reasoning_content: true,
            disabled_wire: None,
            declared_efforts: vec!["low".into(), "high".into(), "max".into()],
        }),
        ..Default::default()
    };
    let wire = build_request_body(&request);
    assert_eq!(wire["thinking"], serde_json::json!({"type": "enabled"}));
    assert_eq!(wire["reasoning_effort"], "max");
    assert!(wire.get("clear_thinking").is_none());
}

#[test]
fn false_and_unknown_profiles_omit_reasoning_content_on_follow_up_messages() {
    for reasoning in [
        FrozenReasoning {
            provider: "openai".into(),
            model: MODEL.into(),
            protocol: ReasoningProtocol::OpenAiChatCompletions,
            choice: ReasoningChoice::Effort("low".into()),
            supports_disabled: false,
            preserve_reasoning_content: false,
            disabled_wire: None,
            declared_efforts: vec!["low".into()],
        },
        FrozenReasoning::unknown("custom", MODEL),
    ] {
        let request = ChatRequest {
            model: MODEL.into(),
            messages: vec![ChatMessage::assistant_with_tools_and_reasoning(
                "tool request",
                None,
                vec![ChatToolCall {
                    id: "call-1".into(),
                    name: "read".into(),
                    input_json: "{}".into(),
                }],
            )],
            reasoning: Some(reasoning),
            ..Default::default()
        };
        let message = &build_request_body(&request)["messages"][0];
        assert!(message.get("reasoning_content").is_none());
    }
}

#[test]
fn explicit_disabled_wire_is_only_emitted_for_declared_protocol() {
    let request = ChatRequest {
        model: MODEL.into(),
        reasoning: Some(FrozenReasoning {
            provider: "openai".into(),
            model: MODEL.into(),
            protocol: ReasoningProtocol::OpenAiChatCompletions,
            choice: ReasoningChoice::Disabled,
            supports_disabled: true,
            preserve_reasoning_content: false,
            disabled_wire: Some(ReasoningDisabledWire::ReasoningEffortNone),
            declared_efforts: vec!["low".into()],
        }),
        ..Default::default()
    };
    let wire = build_request_body(&request);
    assert_eq!(wire["reasoning_effort"], "none");
    assert!(wire.get("thinking").is_none());
}

#[test]
fn standard_glm_profile_cannot_claim_disabled_thinking() {
    let reasoning = FrozenReasoning {
        provider: "zhipu".into(),
        model: "glm-5.3-flash".into(),
        protocol: ReasoningProtocol::ZhipuChatCompletions,
        choice: ReasoningChoice::ProviderDefault,
        supports_disabled: true,
        preserve_reasoning_content: true,
        disabled_wire: Some(ReasoningDisabledWire::ThinkingTypeDisabled),
        declared_efforts: vec!["low".into(), "high".into(), "max".into()],
    };
    assert!(matches!(
        reasoning.validate(),
        Err(crate::VegaError::ReasoningSelectionInvalid { .. })
    ));
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
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
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
        assembler.finalize().expect("explicit finish reason"),
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
        assembler.finalize().expect("explicit finish reason"),
        vec![ProviderEvent::Done {
            stop_reason: StopReason::ToolUse
        }]
    );
}

#[test]
fn empty_tool_identity_continuations_preserve_start_fragment() {
    let mut assembler = SseAssembler::default();
    let events = absorb_all(
        &mut assembler,
        &[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-read","function":{"name":"read","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"","function":{"name":"","arguments":"{\"path\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"","arguments":"\"README.md\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        ],
    );
    assert_eq!(
        events,
        vec![ProviderEvent::ToolUse {
            id: "call-read".into(),
            name: "read".into(),
            input_json: r#"{"path":"README.md"}"#.into(),
        }]
    );
}

#[test]
fn incomplete_tool_identity_fails_atomically_before_tool_use() {
    for incomplete in [
        r#"{"index":1,"id":"","function":{"name":"read","arguments":"{}"}}"#,
        r#"{"index":1,"id":"call-missing-name","function":{"name":"","arguments":"{}"}}"#,
    ] {
        let mut assembler = SseAssembler::default();
        let first = format!(
            "{{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"call-valid\",\"function\":{{\"name\":\"read\",\"arguments\":\"{{}}\"}}}},{}]}}}}]}}",
            incomplete
        );
        assert!(assembler.absorb(&first).unwrap().is_empty());
        let error = assembler
            .absorb(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#)
            .unwrap_err();
        assert!(matches!(
            error,
            VegaError::Provider {
                status: None,
                retryable: false,
                ..
            }
        ));
        assert!(assembler.tools.is_empty(), "no partial tool batch survives");
    }
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
            assembler.finalize().expect("explicit finish reason"),
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

#[test]
fn openai_internal_debug_carriers_redact_distinct_sentinels() {
    let sentinels = [
        "VEGA_ENDPOINT_USERINFO_SENTINEL",
        "VEGA_ENDPOINT_QUERY_SENTINEL",
        "VEGA_KEY_SENTINEL",
        "VEGA_FRAGMENT_ID_SENTINEL",
        "VEGA_FRAGMENT_NAME_SENTINEL",
        "VEGA_FRAGMENT_ARGUMENT_SENTINEL",
    ];
    let provider = OpenAiProvider::new(
        format!(
            "http://{}@127.0.0.1/v1?query={}",
            sentinels[0], sentinels[1]
        ),
        sentinels[2],
    )
    .expect("provider");
    let fragment = ToolFragment {
        id: sentinels[3].into(),
        name: sentinels[4].into(),
        arguments: sentinels[5].into(),
    };
    let mut assembler = SseAssembler::default();
    assembler.tools.insert(0, fragment);
    let rendered = format!("{provider:?} {assembler:?}");
    for sentinel in sentinels {
        assert!(!rendered.contains(sentinel), "OpenAI Debug leaked payload");
    }
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
        Ok(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n"),
        Ok(b"data: [DONE]\n\ndata: {\"ignored\":true}\n\n"),
    ];
    let sse = futures::stream::iter(chunks).eventsource();
    let raw: Vec<_> = sse.collect().await;
    // 4 个完整 SSE 事件：两个内容 chunk、[DONE]、以及 [DONE] 后的 junk 事件
    assert_eq!(raw.len(), 5, "SSE events must reassemble across chunks");

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
    events.extend(assembler.finalize().expect("explicit finish reason"));
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

#[derive(Clone)]
struct CapturedRequest {
    authorization: String,
    body: serde_json::Value,
}

impl fmt::Debug for CapturedRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedRequest")
            .field("authorization_bytes", &self.authorization.len())
            .field("body", &"[redacted]")
            .finish()
    }
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
/// `ProviderEvent: PartialEq`, `Err` items via closed typed fields without
/// formatting provider-controlled payloads.
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
            (Err(a), Err(e)) => assert_error_eq(a, e, i),
            _ => panic!("item #{i} mismatch: {a:?} vs {e:?}"),
        }
    }
}

fn assert_error_eq(actual: &VegaError, expected: &VegaError, index: usize) {
    let equal = match (actual, expected) {
        (
            VegaError::Provider {
                status: actual_status,
                message: actual_message,
                retryable: actual_retryable,
            },
            VegaError::Provider {
                status: expected_status,
                message: expected_message,
                retryable: expected_retryable,
            },
        ) => {
            actual_status == expected_status
                && actual_message == expected_message
                && actual_retryable == expected_retryable
        }
        (VegaError::Cancelled, VegaError::Cancelled) => true,
        (
            VegaError::Tool {
                tool: actual_tool,
                message: actual_message,
            },
            VegaError::Tool {
                tool: expected_tool,
                message: expected_message,
            },
        ) => actual_tool == expected_tool && actual_message == expected_message,
        (VegaError::Io(actual), VegaError::Io(expected)) => actual.kind() == expected.kind(),
        (VegaError::Store(actual), VegaError::Store(expected)) => {
            std::mem::discriminant(actual) == std::mem::discriminant(expected)
        }
        _ => false,
    };
    assert!(equal, "item #{index} error mismatch");
}

mod http;

#[tokio::test]
async fn i71_responses_terminal_failures_never_release_tools() {
    for terminal in [
        None,
        Some("response.failed"),
        Some("response.incomplete"),
        Some("error"),
    ] {
        let added = r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"bad","name":"read","arguments":""}}"#;
        let partial = r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"path\":"}"#;
        let ending = terminal
            .map(|kind| serde_json::json!({"type":kind,"error":{"message":KEY}}).to_string());
        let mut frames = vec![added, partial];
        if let Some(ending) = &ending {
            frames.push(ending);
        }
        let server = spawn_server(scripted_server(vec![sse_response(&frames, false)])).await;
        let stream = provider_for(&server, fast_policy(1))
            .with_responses_api(true)
            .chat_stream(request(), CancellationToken::new())
            .await
            .unwrap();
        let events = collect_events(stream, 20).await;
        assert!(events.iter().any(Result::is_err));
        assert!(!events.iter().any(|event| matches!(
            event,
            Ok(ProviderEvent::ToolUse { .. } | ProviderEvent::Done { .. })
        )));
        assert!(!format!("{events:?}").contains(KEY));
    }
}

#[test]
fn i71_responses_summary_parts_snapshots_and_refusal() {
    let mut parser = responses::Assembler::default();
    let mut events = Vec::new();
    for event in [
        serde_json::json!({"type":"response.reasoning_summary_text.delta","item_id":"r","summary_index":0,"delta":"First"}),
        serde_json::json!({"type":"response.reasoning_summary_text.done","item_id":"r","summary_index":0,"text":"First"}),
        serde_json::json!({"type":"response.reasoning_summary_text.done","item_id":"r","summary_index":1,"text":"Second"}),
        serde_json::json!({"type":"response.refusal.delta","item_id":"m","content_index":0,"delta":"Cannot do that."}),
        serde_json::json!({"type":"response.refusal.done","item_id":"m","content_index":0,"refusal":"Cannot do that."}),
        serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[]}}),
    ] {
        events.extend(parser.absorb(&event.to_string()).unwrap());
    }
    assert_eq!(
        events,
        vec![
            ProviderEvent::SummaryDelta("First".into()),
            ProviderEvent::SummaryDelta("\n\n".into()),
            ProviderEvent::SummaryDelta("Second".into()),
            ProviderEvent::TextDelta("Cannot do that.".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End
            }
        ]
    );
    assert!(parser.terminal);
}

#[tokio::test]
async fn i71_responses_rejects_incompatible_profile_before_network() {
    let provider = OpenAiProvider::new("http://127.0.0.1:1", KEY)
        .unwrap()
        .with_responses_api(true);
    let mut req = request();
    req.reasoning = Some(FrozenReasoning {
        provider: "owned".into(),
        model: MODEL.into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        choice: ReasoningChoice::ProviderDefault,
        supports_disabled: false,
        preserve_reasoning_content: true,
        disabled_wire: None,
        declared_efforts: vec![],
    });
    assert!(matches!(
        provider.chat_stream(req, CancellationToken::new()).await,
        Err(VegaError::ReasoningSelectionInvalid { .. })
    ));
}

#[test]
fn i71_responses_done_reconciles_empty_and_partial_deltas_without_duplicates() {
    let mut parser = responses::Assembler::default();
    let mut out = Vec::new();
    for (kind, text) in [
        ("delta", ""),
        ("done", "hello"),
        ("done", "hello"),
        ("delta", " world"),
        ("done", "hello world!"),
    ] {
        let mut event = serde_json::json!({"type":format!("response.reasoning_summary_text.{kind}"),"item_id":"r","summary_index":0});
        event[if kind == "delta" { "delta" } else { "text" }] = serde_json::json!(text);
        out.extend(parser.absorb(&event.to_string()).unwrap());
    }
    assert_eq!(
        out,
        vec![
            ProviderEvent::SummaryDelta("hello".into()),
            ProviderEvent::SummaryDelta(" world".into()),
            ProviderEvent::SummaryDelta("!".into())
        ]
    );
    assert!(
        !format!(
            "{:?}",
            ProviderEvent::ReasoningReplay(vec![
                serde_json::json!({"encrypted_content":"private opaque"})
            ])
        )
        .contains("private opaque")
    );
    let mut parser = responses::Assembler::default();
    let text = "界".repeat(40_000);
    let event = serde_json::json!({"type":"response.reasoning_summary_text.done","item_id":"r","summary_index":0,"text":text});
    let out = parser.absorb(&event.to_string()).unwrap();
    let combined: String = out
        .into_iter()
        .map(|event| match event {
            ProviderEvent::SummaryDelta(text) => {
                assert!(text.len() <= 64 * 1024);
                text
            }
            _ => panic!("unexpected event"),
        })
        .collect();
    assert_eq!(combined, text);
}

#[tokio::test]
async fn i71_responses_cancellation_discards_pending_tools() {
    let completed = r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","call_id":"owned","name":"read","arguments":"{}"}]}}"#;
    let server = spawn_server(scripted_server(vec![sse_response(&[completed], false)])).await;
    let cancel = CancellationToken::new();
    let mut stream = provider_for(&server, fast_policy(1))
        .with_responses_api(true)
        .chat_stream(request(), cancel.clone())
        .await
        .unwrap();
    cancel.cancel();
    assert!(stream.next().await.is_none());
}

#[test]
fn i71_responses_request_effort_off_tools_and_chat_compatibility() {
    let mut req = request();
    req.reasoning = Some(FrozenReasoning {
        provider: "owned".into(),
        model: MODEL.into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        choice: ReasoningChoice::Effort("low".into()),
        supports_disabled: true,
        preserve_reasoning_content: false,
        disabled_wire: Some(ReasoningDisabledWire::ReasoningEffortNone),
        declared_efforts: vec!["low".into()],
    });
    req.tools = vec![ToolDefinition {
        name: "read".into(),
        description: "Read".into(),
        input_schema: serde_json::json!({"type":"object"}),
        strict: true,
    }];
    let body = responses::build_body(&req).unwrap();
    assert_eq!(
        body["reasoning"],
        serde_json::json!({"summary":"auto","effort":"low"})
    );
    assert_eq!(body["tools"][0]["strict"], true);
    assert_eq!(body["tools"][0]["type"], "function");
    req.tools[0].strict = false;
    req.tools[0].input_schema =
        serde_json::json!({"type":"object","properties":{"optional":{"type":"string"}}});
    let non_strict = responses::build_body(&req).unwrap();
    assert_eq!(non_strict["tools"][0]["strict"], false);
    assert_eq!(
        non_strict["tools"][0]["parameters"],
        req.tools[0].input_schema
    );
    assert!(
        non_strict["tools"][0]["parameters"]
            .get("required")
            .is_none()
    );
    req.reasoning.as_mut().unwrap().choice = ReasoningChoice::Disabled;
    assert_eq!(
        responses::build_body(&req).unwrap()["reasoning"]["effort"],
        "none"
    );
    let chat = build_request_body(&req);
    assert_eq!(chat["reasoning_effort"], "none");
    assert!(chat.get("reasoning").is_none());
    assert!(chat.get("input").is_none());
    req.reasoning.as_mut().unwrap().disabled_wire =
        Some(ReasoningDisabledWire::ThinkingTypeDisabled);
    assert!(responses::build_body(&req).is_err());
}

#[test]
fn i71_responses_item_and_terminal_snapshots_recover_missing_deltas_once() {
    let mut parser = responses::Assembler::default();
    let reasoning = serde_json::json!({"type":"reasoning","id":"r","summary":[{"type":"summary_text","text":"Summary"}],"encrypted_content":"opaque"});
    let message = serde_json::json!({"type":"message","id":"m","role":"assistant","content":[{"type":"output_text","text":"Answer"}]});
    let mut out = parser.absorb(&serde_json::json!({"type":"response.output_item.done","output_index":0,"item":reasoning}).to_string()).unwrap();
    out.extend(parser.absorb(&serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[reasoning,message]}}).to_string()).unwrap());
    assert_eq!(
        out.iter()
            .filter(|event| matches!(event, ProviderEvent::SummaryDelta(_)))
            .count(),
        1
    );
    assert_eq!(
        out.iter()
            .filter(|event| matches!(event,ProviderEvent::TextDelta(text) if text == "Answer"))
            .count(),
        1
    );
    assert!(matches!(out.last(), Some(ProviderEvent::Done { .. })));
}
