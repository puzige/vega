use std::collections::HashMap;

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, advance};
use tokio_util::sync::CancellationToken;
use vega_mcp::{HttpClient, McpError, ProtocolVersion};

#[tokio::test]
async fn m04_modern_http_json_and_request_scoped_sse() {
    let (endpoint, mut requests) = start_server(Scenario::ModernMixed).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("modern connect");
    assert_eq!(client.version(), ProtocolVersion::Modern);
    let catalog = client.list_tools().await.expect("HTTP tools/list");
    let result = client
        .call_tool(&catalog.tools[0], json!({"echo": "from sse"}))
        .await
        .expect("HTTP SSE tools/call");
    assert_eq!(result.text, ["from sse"]);
    let discover = requests.recv().await.expect("discover POST");
    let list = requests.recv().await.expect("list POST");
    let call = requests.recv().await.expect("call POST");
    for request in [&discover, &list, &call] {
        assert_eq!(request.verb, "POST");
        assert_eq!(request.path, "/mcp");
        assert_eq!(request.headers["mcp-protocol-version"], "2026-07-28");
        assert!(request.headers["accept"].contains("application/json"));
        assert!(request.headers["accept"].contains("text/event-stream"));
        assert_eq!(
            request.body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"],
            "2026-07-28"
        );
        assert_eq!(request.headers["mcp-method"], request.body["method"]);
    }
    assert_eq!(call.headers["mcp-name"], "echo");
}

#[tokio::test]
async fn m07_anonymous_http_never_sends_authorization() {
    let (endpoint, mut requests) = start_server(Scenario::ModernMixed).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("anonymous connect");
    let catalog = client.list_tools().await.expect("anonymous list");
    client
        .call_tool(&catalog.tools[0], json!({"echo":"anonymous"}))
        .await
        .expect("anonymous call");
    for _ in 0..3 {
        let request = requests.recv().await.expect("owned request");
        assert!(!request.headers.contains_key("authorization"));
    }
}

#[tokio::test]
async fn m12_http_disconnect_is_typed_and_never_replayed_without_new_call() {
    let (endpoint, mut requests) = start_server(Scenario::DisconnectFirstCall).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("owned connect");
    let catalog = client.list_tools().await.expect("owned list");
    let _ = requests.recv().await.expect("probe");
    let _ = requests.recv().await.expect("list");
    assert!(matches!(
        client
            .call_tool(&catalog.tools[0], json!({"echo":"first"}))
            .await,
        Err(McpError::Transport)
    ));
    let first = requests.recv().await.expect("first call");
    assert_eq!(first.body["id"], 3);
    assert!(requests.try_recv().is_err(), "disconnect must not replay");
    let result = client
        .call_tool(&catalog.tools[0], json!({"echo":"explicit second"}))
        .await
        .expect("new explicit call");
    assert_eq!(result.text, ["ok"]);
    let second = requests.recv().await.expect("second call");
    assert_eq!(second.body["id"], 4);
}

#[tokio::test]
async fn m11_non_200_http_json_rpc_error_preserves_only_typed_code() {
    for status in [400, 500] {
        let (endpoint, _requests) = start_server(Scenario::HttpRpcError(status)).await;
        let mut client = HttpClient::connect(&endpoint, true)
            .await
            .expect("owned connect");
        let catalog = client.list_tools().await.expect("owned catalog");
        let error = client
            .call_tool(&catalog.tools[0], json!({"echo":"error"}))
            .await
            .expect_err("non-200 JSON-RPC error");
        assert!(matches!(error, McpError::Rpc(-32042)), "{error:?}");
        assert!(!error.to_string().contains("FAKE_SECRET_SERVER_PROSE"));
    }
}

#[tokio::test]
async fn m11_non_200_http_malformed_or_mismatched_error_remains_transport() {
    for scenario in [
        Scenario::HttpMalformedError,
        Scenario::HttpWrongIdError,
        Scenario::HttpPlainError,
    ] {
        let (endpoint, _requests) = start_server(scenario).await;
        let mut client = HttpClient::connect(&endpoint, true)
            .await
            .expect("owned connect");
        let catalog = client.list_tools().await.expect("owned catalog");
        assert!(matches!(
            client.call_tool(&catalog.tools[0], json!({})).await,
            Err(McpError::Transport)
        ));
    }
}

#[tokio::test]
async fn m07_http_401_and_403_do_not_become_json_rpc_tool_errors() {
    for (status, authorization_required) in [(401, true), (403, false)] {
        let (endpoint, _requests) = start_server(Scenario::HttpRpcError(status)).await;
        let mut client = HttpClient::connect(&endpoint, true)
            .await
            .expect("owned connect");
        let catalog = client.list_tools().await.expect("owned catalog");
        let error = client
            .call_tool(&catalog.tools[0], json!({}))
            .await
            .expect_err("auth HTTP status is not an MCP tool result");
        if authorization_required {
            assert!(matches!(error, McpError::AuthRequired));
        } else {
            assert!(matches!(error, McpError::Transport));
        }
    }
}

#[tokio::test]
async fn m12_dropping_request_scoped_http_sse_closes_the_stream() {
    let (endpoint, started, closed) = start_cancellable_sse_server().await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("owned connect");
    let catalog = client.list_tools().await.expect("owned catalog");
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let call = tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = signal.cancelled() => Err(McpError::Cancelled),
            result = client.call_tool(&catalog.tools[0], json!({"echo":"cancel"})) => result,
        }
    });
    tokio::time::timeout(Duration::from_secs(3), started)
        .await
        .expect("SSE response started")
        .expect("head signal");
    cancel.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), call)
            .await
            .expect("call stopped")
            .expect("task completed"),
        Err(McpError::Cancelled)
    ));
    assert!(
        tokio::time::timeout(Duration::from_secs(3), closed)
            .await
            .expect("stream closed in time")
            .expect("close signal"),
        "dropped request-scoped SSE must release the response body"
    );
}

#[tokio::test(start_paused = true)]
async fn m11_http_request_scoped_sse_has_exact_30s_idle_deadline() {
    let (endpoint, mut requests) = start_server(Scenario::SseIdle).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("owned connect");
    let catalog = client.list_tools().await.expect("owned list");
    let _ = requests.recv().await.expect("probe");
    let _ = requests.recv().await.expect("list");
    let call = tokio::spawn(async move {
        client
            .call_tool(&catalog.tools[0], json!({"echo":"stall"}))
            .await
    });
    let _ = requests.recv().await.expect("SSE header sent");
    for _ in 0..32 {
        tokio::task::yield_now().await;
    }
    let started = tokio::time::Instant::now();
    tokio::time::sleep(Duration::from_secs(29)).await;
    assert!(!call.is_finished(), "29 seconds of silence is allowed");
    tokio::time::sleep(Duration::from_secs(1)).await;
    for _ in 0..32 {
        if call.is_finished() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        call.is_finished(),
        "30-second SSE idle deadline did not fire; elapsed {:?}",
        started.elapsed()
    );
    let result = call.await.expect("call task");
    assert!(
        matches!(result, Err(McpError::Timeout)),
        "{result:?} after {:?}",
        started.elapsed()
    );
}

#[tokio::test(start_paused = true)]
async fn m11_http_call_has_exact_120s_deadline() {
    let (endpoint, mut requests) = start_server(Scenario::CallDeadline).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("owned connect");
    let catalog = client.list_tools().await.expect("owned list");
    let _ = requests.recv().await.expect("probe");
    let _ = requests.recv().await.expect("list");
    let call = tokio::spawn(async move {
        client
            .call_tool(&catalog.tools[0], json!({"echo":"stall"}))
            .await
    });
    let _ = requests.recv().await.expect("call reached server");
    for _ in 0..32 {
        tokio::task::yield_now().await;
    }
    let started = tokio::time::Instant::now();
    advance(Duration::from_secs(119)).await;
    assert!(!call.is_finished(), "119 seconds is inside call deadline");
    advance(Duration::from_secs(1)).await;
    for _ in 0..32 {
        if call.is_finished() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        call.is_finished(),
        "120-second call deadline did not fire; elapsed {:?}",
        started.elapsed()
    );
    let result = call.await.expect("call task");
    assert!(
        matches!(result, Err(McpError::Timeout)),
        "{result:?} after {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn m05_legacy_streamable_http_retains_session_but_no_old_sse_get() {
    let (endpoint, mut requests) = start_server(Scenario::Legacy).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("legacy connect");
    assert_eq!(client.version(), ProtocolVersion::Legacy);
    let catalog = client.list_tools().await.expect("legacy HTTP list");
    let result = client
        .call_tool(&catalog.tools[0], json!({"echo": "old era"}))
        .await
        .expect("legacy HTTP call");
    assert_eq!(result.text, ["old era"]);
    let mut seen = Vec::new();
    for _ in 0..5 {
        seen.push(requests.recv().await.expect("expected POST"));
    }
    assert_eq!(
        seen.iter()
            .map(|r| r.body["method"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        [
            "server/discover",
            "initialize",
            "notifications/initialized",
            "tools/list",
            "tools/call"
        ]
    );
    assert!(seen.iter().all(|r| r.verb == "POST"));
    for request in &seen[3..] {
        assert_eq!(request.headers["mcp-session-id"], "owned-session");
        assert_eq!(request.headers["mcp-protocol-version"], "2025-11-25");
    }
}

#[tokio::test]
async fn m05_deprecated_standalone_http_sse_is_not_followed() {
    let (endpoint, mut requests) = start_server(Scenario::OldSse).await;
    let result = HttpClient::connect(&endpoint, true).await;
    assert!(matches!(result, Err(McpError::UnsupportedTransport)));
    let probe = requests.recv().await.expect("one probe");
    assert_eq!(probe.verb, "POST");
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn m05_modern_http_error_never_initializes_legacy() {
    let (endpoint, mut requests) = start_server(Scenario::ModernError).await;
    let result = HttpClient::connect(&endpoint, true).await;
    assert!(matches!(result, Err(McpError::IncompatibleVersion)));
    let probe = requests.recv().await.expect("one modern probe");
    assert_eq!(probe.body["method"], "server/discover");
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn m08_paginated_catalog_rejects_bad_header_and_mirrors_safe_header() {
    let (endpoint, mut requests) = start_server(Scenario::Paginated).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("modern connect");
    let catalog = client.list_tools().await.expect("two pages");
    assert_eq!(
        catalog
            .tools
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["search", "Search"]
    );
    assert_eq!(catalog.rejected.len(), 1);
    let result = client
        .call_tool(&catalog.tools[0], json!({"region": "Hello, 世界"}))
        .await
        .expect("mirrored call");
    assert_eq!(result.text, ["ok"]);
    let _ = requests.recv().await.expect("discover");
    let page1 = requests.recv().await.expect("page one");
    let page2 = requests.recv().await.expect("page two");
    let call = requests.recv().await.expect("call");
    assert_eq!(page1.body["method"], "tools/list");
    assert_eq!(page2.body["params"]["cursor"], "page2");
    assert_eq!(
        call.headers["mcp-param-region"],
        "=?base64?SGVsbG8sIOS4lueVjA==?="
    );
}

#[tokio::test]
async fn m08_cursor_cycle_is_a_closed_failure() {
    let (endpoint, _requests) = start_server(Scenario::CursorCycle).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("modern connect");
    assert!(client.list_tools().await.is_err());
}

#[tokio::test]
async fn m11_paginated_catalog_counts_exact_raw_wire_bytes() {
    let (endpoint, _requests) = start_server(Scenario::CatalogWireExact).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("owned connect");
    let catalog = client.list_tools().await.expect("exactly 4 MiB accepted");
    assert_eq!(catalog.tools.len(), 2);

    let (endpoint, _requests) = start_server(Scenario::CatalogWireOver).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("owned connect");
    assert!(matches!(
        client.list_tools().await,
        Err(McpError::LimitExceeded)
    ));
}

#[tokio::test]
async fn m11_owned_catalog_enforces_64_tools_and_256k_schema() {
    let (endpoint, _requests) = start_server(Scenario::CatalogCount(64)).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("owned connect");
    assert_eq!(client.list_tools().await.expect("64 tools").tools.len(), 64);

    let (endpoint, _requests) = start_server(Scenario::CatalogCount(65)).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("owned connect");
    assert!(matches!(
        client.list_tools().await,
        Err(McpError::LimitExceeded)
    ));

    let (endpoint, _requests) = start_server(Scenario::SchemaBytes { over: false }).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("owned connect");
    assert_eq!(
        client
            .list_tools()
            .await
            .expect("256 KiB schema")
            .tools
            .len(),
        1
    );

    let (endpoint, _requests) = start_server(Scenario::SchemaBytes { over: true }).await;
    let mut client = HttpClient::connect(&endpoint, true)
        .await
        .expect("owned connect");
    assert!(matches!(
        client.list_tools().await,
        Err(McpError::LimitExceeded)
    ));
}

#[tokio::test]
async fn m11_owned_http_sse_event_enforces_exact_1m_boundary() {
    for over in [false, true] {
        let (endpoint, _requests) = start_server(Scenario::SseEvent { over }).await;
        let mut client = HttpClient::connect(&endpoint, true)
            .await
            .expect("owned connect");
        let catalog = client.list_tools().await.expect("owned catalog");
        let result = client
            .call_tool(&catalog.tools[0], json!({"echo":"bounded"}))
            .await;
        if over {
            assert!(matches!(result, Err(McpError::LimitExceeded)));
        } else {
            assert_eq!(result.expect("exact event accepted").text, ["ok"]);
        }
    }
}

#[tokio::test]
async fn m11_owned_http_json_and_sse_enforce_exact_8m_response_boundary() {
    for sse in [false, true] {
        for over in [false, true] {
            let (endpoint, _requests) = start_server(Scenario::ResponseBytes { sse, over }).await;
            let mut client = HttpClient::connect(&endpoint, true)
                .await
                .expect("owned connect");
            let catalog = client.list_tools().await.expect("owned catalog");
            let result = client
                .call_tool(&catalog.tools[0], json!({"echo":"bounded"}))
                .await;
            if over {
                assert!(matches!(result, Err(McpError::LimitExceeded)), "sse={sse}");
            } else {
                assert_eq!(result.expect("exact response accepted").text, ["ok"]);
            }
        }
    }
}

#[tokio::test]
async fn m11_owned_http_malformed_json_and_sse_fail_closed() {
    for scenario in [Scenario::MalformedJson, Scenario::MalformedSse] {
        let (endpoint, _requests) = start_server(scenario).await;
        let mut client = HttpClient::connect(&endpoint, true)
            .await
            .expect("owned connect");
        let catalog = client.list_tools().await.expect("owned catalog");
        assert!(matches!(
            client
                .call_tool(&catalog.tools[0], json!({"echo":"bounded"}))
                .await,
            Err(McpError::InvalidMessage)
        ));
    }
}

#[derive(Clone, Copy)]
enum Scenario {
    ModernMixed,
    Legacy,
    ModernError,
    OldSse,
    Paginated,
    CursorCycle,
    CatalogWireExact,
    CatalogWireOver,
    CatalogCount(usize),
    SchemaBytes { over: bool },
    SseEvent { over: bool },
    ResponseBytes { sse: bool, over: bool },
    MalformedJson,
    MalformedSse,
    HttpRpcError(u16),
    HttpMalformedError,
    HttpWrongIdError,
    HttpPlainError,
    DisconnectFirstCall,
    SseIdle,
    CallDeadline,
}

#[derive(Debug)]
struct CapturedRequest {
    verb: String,
    path: String,
    headers: HashMap<String, String>,
    body: Value,
}

fn capture(request: vega_mcp::mock::Request) -> CapturedRequest {
    CapturedRequest {
        verb: request.method().to_string(),
        path: request.url().path().to_string(),
        headers: request
            .headers()
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string()))
            .collect(),
        body: serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap(),
    }
}
async fn start_server(
    scenario: Scenario,
) -> (
    vega_mcp::mock::Endpoint,
    mpsc::UnboundedReceiver<CapturedRequest>,
) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let endpoint = vega_mcp::mock::Endpoint::new("/mcp", |_| {
        std::sync::Arc::new(move |request| {
            let request = capture(request);
            let is_call = request.body["method"] == "tools/call";
            let reply = response_for(scenario, &request);
            sender.send(request).unwrap();
            let first = is_call && calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
            Box::pin(async move {
                if is_call {
                    match scenario {
                        Scenario::DisconnectFirstCall if first => {
                            return Err(vega_mcp::mock::transport_error());
                        }
                        Scenario::SseIdle => {
                            return Ok(vega_mcp::mock::stream_response(futures::stream::pending()));
                        }
                        Scenario::CallDeadline => return std::future::pending().await,
                        _ => {}
                    }
                }
                Ok(vega_mcp::mock::response(
                    reply.0,
                    reply.2,
                    reply.1,
                    reply
                        .3
                        .into_iter()
                        .map(|(k, v)| (k, v.to_owned()))
                        .collect(),
                ))
            })
        })
    });
    (endpoint, receiver)
}
struct StreamDrop(Option<oneshot::Sender<bool>>);
impl Drop for StreamDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(true);
        }
    }
}
async fn start_cancellable_sse_server() -> (
    vega_mcp::mock::Endpoint,
    oneshot::Receiver<()>,
    oneshot::Receiver<bool>,
) {
    let (started_tx, started_rx) = oneshot::channel();
    let (closed_tx, closed_rx) = oneshot::channel();
    let channels = std::sync::Mutex::new(Some((started_tx, closed_tx)));
    let endpoint = vega_mcp::mock::Endpoint::new("/mcp", |_| {
        std::sync::Arc::new(move |request| {
            let request = capture(request);
            if request.body["method"] == "tools/call" {
                let (started, closed) = channels.lock().unwrap().take().unwrap();
                let _ = started.send(());
                let stream = futures::stream::unfold(
                    StreamDrop(Some(closed)),
                    |guard| async move {
                        let _guard = guard;
                        std::future::pending::<Option<(Result<Vec<u8>, std::io::Error>, StreamDrop)>>().await
                    },
                );
                return Box::pin(async move { Ok(vega_mcp::mock::stream_response(stream)) });
            }
            let reply = response_for(Scenario::ModernMixed, &request);
            Box::pin(async move {
                Ok(vega_mcp::mock::response(
                    reply.0,
                    reply.2,
                    reply.1,
                    reply
                        .3
                        .into_iter()
                        .map(|(k, v)| (k, v.to_owned()))
                        .collect(),
                ))
            })
        })
    });
    (endpoint, started_rx, closed_rx)
}

fn response_for(
    scenario: Scenario,
    request: &CapturedRequest,
) -> (u16, String, &'static str, Vec<(&'static str, &'static str)>) {
    let id = request.body.get("id").cloned().unwrap_or(Value::Null);
    let method = request.body["method"].as_str().unwrap_or_default();
    if method == "tools/call" {
        match scenario {
            Scenario::HttpRpcError(status) => {
                return (
                    status,
                    json!({"jsonrpc":"2.0", "id":id, "error":{
                        "code":-32042, "message":"FAKE_SECRET_SERVER_PROSE"
                    }})
                    .to_string(),
                    "application/json",
                    Vec::new(),
                );
            }
            Scenario::HttpMalformedError => {
                return (400, "{invalid".into(), "application/json", Vec::new());
            }
            Scenario::HttpWrongIdError => {
                return (
                    400,
                    json!({"jsonrpc":"2.0", "id":id.as_u64().unwrap_or(0) + 1,
                        "error":{"code":-32042,"message":"FAKE_SECRET_SERVER_PROSE"}})
                    .to_string(),
                    "application/json",
                    Vec::new(),
                );
            }
            Scenario::HttpPlainError => {
                return (500, "upstream failed".into(), "text/plain", Vec::new());
            }
            Scenario::MalformedJson => {
                return (200, "{invalid".into(), "application/json", Vec::new());
            }
            Scenario::MalformedSse => {
                return (
                    200,
                    "data: {invalid\n\n".into(),
                    "text/event-stream",
                    Vec::new(),
                );
            }
            _ => {}
        }
    }
    let (status, result, kind, extra) = match (scenario, method) {
        (Scenario::OldSse, _) => return (404, String::new(), "text/plain", Vec::new()),
        (Scenario::Legacy, "server/discover") => {
            return (
                400,
                "legacy unknown method".into(),
                "text/plain",
                Vec::new(),
            );
        }
        (Scenario::ModernError, "server/discover") => {
            return (
                400,
                json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32022,
                    "message":"unsupported version", "data":{"requested":"2026-07-28", "supported":["2027-01-01"]}}}).to_string(),
                "application/json",
                Vec::new(),
            );
        }
        (Scenario::Legacy, "initialize") => (
            200,
            json!({"protocolVersion":"2025-11-25", "capabilities":{"tools":{}}, "serverInfo":{"name":"owned-http","version":"1"}}),
            "application/json",
            vec![("Mcp-Session-Id", "owned-session")],
        ),
        (Scenario::Legacy, "notifications/initialized") => {
            return (202, String::new(), "application/json", Vec::new());
        }
        (_, "server/discover") => (
            200,
            json!({"resultType":"complete", "supportedVersions":["2026-07-28"], "capabilities":{"tools":{}}, "ttlMs":0, "cacheScope":"private"}),
            "application/json",
            Vec::new(),
        ),
        (Scenario::Paginated, "tools/list") if request.body["params"].get("cursor").is_none() => (
            200,
            json!({"resultType":"complete", "tools":[{"name":"search", "inputSchema":{"type":"object", "properties":{"region":{"type":"string", "x-mcp-header":"Region"}}}}], "nextCursor":"page2", "ttlMs":0, "cacheScope":"private"}),
            "application/json",
            Vec::new(),
        ),
        (Scenario::Paginated, "tools/list") => (
            200,
            json!({"resultType":"complete", "tools":[{"name":"Search", "inputSchema":{"type":"object"}}, {"name":"bad", "inputSchema":{"type":"object", "properties":{"x":{"type":"number", "x-mcp-header":"Bad"}}}}], "ttlMs":0, "cacheScope":"private"}),
            "application/json",
            Vec::new(),
        ),
        (Scenario::CursorCycle, "tools/list") => (
            200,
            json!({"resultType":"complete", "tools":[], "nextCursor":"same", "ttlMs":0, "cacheScope":"private"}),
            "application/json",
            Vec::new(),
        ),
        (Scenario::CatalogWireExact | Scenario::CatalogWireOver, "tools/list")
            if request.body["params"].get("cursor").is_none() =>
        {
            (
                200,
                json!({"resultType":"complete", "tools":[{"name":"first", "inputSchema":{"type":"object"}}], "nextCursor":"page2", "ttlMs":0, "cacheScope":"private"}),
                "application/json",
                Vec::new(),
            )
        }
        (Scenario::CatalogWireExact | Scenario::CatalogWireOver, "tools/list") => (
            200,
            json!({"resultType":"complete", "tools":[{"name":"second", "inputSchema":{"type":"object"}}], "ttlMs":0, "cacheScope":"private"}),
            "application/json",
            Vec::new(),
        ),
        (Scenario::CatalogCount(count), "tools/list") => (
            200,
            json!({"resultType":"complete", "tools":(0..count).map(|index| json!({"name":format!("tool_{index}"),"inputSchema":{"type":"object"}})).collect::<Vec<_>>(), "ttlMs":0, "cacheScope":"private"}),
            "application/json",
            Vec::new(),
        ),
        (Scenario::SchemaBytes { over }, "tools/list") => {
            let mut schema = json!({"type":"object","description":""});
            let base = serde_json::to_vec(&schema).expect("owned schema").len();
            schema["description"] =
                Value::String("x".repeat(256 * 1024 + usize::from(over) - base));
            (
                200,
                json!({"resultType":"complete", "tools":[{"name":"bounded","inputSchema":schema}], "ttlMs":0, "cacheScope":"private"}),
                "application/json",
                Vec::new(),
            )
        }
        (_, "tools/list") => (
            200,
            json!({"resultType":"complete", "tools":[{"name":"echo", "inputSchema":{"type":"object", "properties":{"echo":{"type":"string"}}}}], "ttlMs":0, "cacheScope":"private"}),
            "application/json",
            Vec::new(),
        ),
        (Scenario::ModernMixed, "tools/call") => (
            200,
            json!({"resultType":"complete", "content":[{"type":"text", "text":request.body["params"]["arguments"]["echo"]}], "isError":false}),
            "text/event-stream",
            Vec::new(),
        ),
        (Scenario::SseEvent { .. } | Scenario::ResponseBytes { sse: true, .. }, "tools/call") => (
            200,
            json!({"resultType":"complete", "content":[{"type":"text","text":"ok"}], "isError":false}),
            "text/event-stream",
            Vec::new(),
        ),
        (_, "tools/call") => (
            200,
            json!({"resultType":"complete", "content":[{"type":"text", "text": if matches!(scenario, Scenario::Legacy) { request.body["params"]["arguments"]["echo"].clone() } else { json!("ok") }}], "isError":false}),
            "application/json",
            Vec::new(),
        ),
        _ => panic!("unexpected owned request: {method}"),
    };
    let envelope = json!({"jsonrpc":"2.0", "id":id, "result":result});
    let mut body = if kind == "text/event-stream" {
        format!(": keepalive\n\ndata: {envelope}\n\n")
    } else {
        envelope.to_string()
    };
    if matches!(
        scenario,
        Scenario::CatalogWireExact | Scenario::CatalogWireOver
    ) && method == "tools/list"
    {
        let second = request.body["params"].get("cursor").is_some();
        let target =
            2 * 1024 * 1024 + usize::from(second && matches!(scenario, Scenario::CatalogWireOver));
        assert!(body.len() < target, "owned catalog page must fit target");
        body.push_str(&" ".repeat(target - body.len()));
    }
    if method == "tools/call" {
        match scenario {
            Scenario::SseEvent { over } => {
                let target = 1024 * 1024 + usize::from(over);
                let event = format!("data: {envelope}\n\n");
                assert!(event.len() < target);
                body = format!("data: {envelope}{}\n\n", " ".repeat(target - event.len()));
            }
            Scenario::ResponseBytes { sse: false, over } => {
                let target = 8 * 1024 * 1024 + usize::from(over);
                assert!(body.len() < target);
                body.push_str(&" ".repeat(target - body.len()));
            }
            Scenario::ResponseBytes { sse: true, over } => {
                let target = 8 * 1024 * 1024 + usize::from(over);
                let result_event = format!("data: {envelope}\n\n");
                let filler_event = format!(":{}\n\n", " ".repeat(512 * 1024 - 3));
                let mut events = filler_event.repeat(15);
                let last_comment = target - events.len() - result_event.len();
                assert!(last_comment > 3 && last_comment <= 1024 * 1024);
                events.push_str(&format!(":{}\n\n", " ".repeat(last_comment - 3)));
                events.push_str(&result_event);
                assert_eq!(events.len(), target);
                body = events;
            }
            _ => {}
        }
    }
    (status, body, kind, extra)
}
