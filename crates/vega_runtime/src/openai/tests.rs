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

// ---------- 集成：请求/响应走本地 TCP ----------

#[tokio::test]
async fn happy_path_sends_openai_wire_format_and_streams_events() {
    let body = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
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
    let stream = tokio::time::timeout(Duration::from_secs(10), provider.chat_stream(req, cancel))
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
    assert!(
        captured[0].authorization == format!("{AUTH_SCHEME} {KEY}"),
        "authorization mismatch"
    );
    let wire = &captured[0].body;
    assert!(wire["model"] == MODEL, "wire model mismatch");
    assert!(wire["stream"] == true, "wire stream flag mismatch");
    assert!(
        wire["stream_options"]["include_usage"] == true,
        "wire usage flag mismatch"
    );
    assert!(wire["max_tokens"] == 64, "wire token cap mismatch");
    assert!(wire["messages"][0]["role"] == "user", "wire role mismatch");
    assert!(
        wire["messages"][0]["content"] == "hello",
        "wire content mismatch"
    );
    assert!(
        wire["tools"][0]["type"] == "function",
        "wire tool type mismatch"
    );
    assert!(
        wire["tools"][0]["function"]["name"] == "read",
        "wire tool name mismatch"
    );
    assert!(
        wire["tools"][0]["function"]["parameters"]["type"] == "object",
        "wire schema mismatch"
    );
}

#[test]
fn captured_request_debug_redacts_distinct_authorization_and_body_sentinels() {
    let sentinels = [
        "VEGA_AUTHORIZATION_SENTINEL",
        "VEGA_MODEL_SENTINEL",
        "VEGA_PROMPT_SENTINEL",
        "VEGA_TOOL_SENTINEL",
    ];
    let captured = CapturedRequest {
        authorization: sentinels[0].into(),
        body: serde_json::json!({
            "model": sentinels[1],
            "messages": [{"content": sentinels[2]}],
            "tools": [{"name": sentinels[3]}],
        }),
    };
    let rendered = format!("{captured:?}");
    for sentinel in sentinels {
        assert!(
            !rendered.contains(sentinel),
            "captured request Debug leaked payload"
        );
    }
    assert!(rendered.contains("authorization_bytes"));
    assert!(rendered.contains("[redacted]"));
}

#[tokio::test]
async fn retry_policy_zero_makes_exactly_one_local_http_attempt() {
    let success = sse_response(
        &[r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#],
        true,
    );
    let server = spawn_server(scripted_server(vec![
        status_response("500 Internal Server Error", &[], "first failure"),
        success,
    ]))
    .await;
    let provider = provider_for(
        &server,
        RetryPolicy {
            max_retries: 0,
            base_delay: Duration::from_millis(1),
            ..RetryPolicy::default()
        },
    );
    let result = provider
        .chat_stream(request(), CancellationToken::new())
        .await;
    assert!(matches!(
        result,
        Err(VegaError::Provider {
            status: Some(500),
            retryable: false,
            ..
        })
    ));
    assert_eq!(server.connection_count(), 1);
    assert_eq!(server.captured().len(), 1);
}

#[tokio::test]
async fn missing_finish_reason_is_protocol_error_for_done_and_raw_eof() {
    let partial = r#"{"choices":[{"delta":{"content":"partial"},"finish_reason":null}]}"#;
    for done in [false, true] {
        let server = spawn_server(scripted_server(vec![sse_response(&[partial], done)])).await;
        let provider = provider_for(&server, fast_policy(25));
        let stream = provider
            .chat_stream(
                ChatRequest {
                    model: MODEL.into(),
                    ..ChatRequest::default()
                },
                CancellationToken::new(),
            )
            .await
            .expect("stream setup");
        let events = collect_events(stream, 4).await;
        assert!(matches!(
            events.as_slice(),
            [Ok(ProviderEvent::TextDelta(text)), Err(VegaError::Provider { retryable: false, .. })]
                if text == "partial"
        ));
    }
}

#[tokio::test]
async fn retries_5xx_with_backoff_then_succeeds() {
    let ok = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    );
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
    let ok = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    );
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
    let ok = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    );
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
            assert!(message.contains("after 3 retries"), "retry summary missing");
            assert!(!message.contains(KEY), "provider message leaked key");
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
                let ok = sse_response(
                    &[
                        r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
                        r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
                    ],
                    true,
                );
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
            assert!(
                message.contains("invalid credentials"),
                "provider detail missing"
            );
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
            assert!(!message.contains(KEY), "provider message leaked key");
            assert!(message.contains("<redacted>"), "redaction marker missing");
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
    let task = tokio::spawn(async move { provider.chat_stream(request(), request_cancel).await });
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
