use std::collections::HashMap;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;
use vega_mcp::{HttpClient, LocalServer, McpError, ProtocolVersion, StdioClient};

#[tokio::test]
async fn m02_modern_stdio_real_child_round_trip() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let server = LocalServer {
        executable: env!("CARGO_BIN_EXE_owned_stdio_server").into(),
        args: vec!["modern".into()],
        working_directory: temp.path().into(),
        environment: Vec::new(),
    };
    let mut client = StdioClient::connect(server).await.expect("modern connect");
    assert_eq!(client.version(), ProtocolVersion::Modern);
    let catalog = client.list_tools().await.expect("real tools/list");
    assert_eq!(catalog.tools.len(), 1);
    assert_eq!(catalog.tools[0].name, "echo");
    let result = client
        .call_tool(&catalog.tools[0], json!({"echo": "from real child"}))
        .await
        .expect("real tools/call");
    assert_eq!(result.text, ["from real child"]);
    assert!(!result.is_error);
    client.shutdown().await.expect("child reaped");
}

#[tokio::test]
async fn m03_legacy_stdio_only_after_nonmodern_probe() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let trace = temp.path().join("methods.txt");
    let server = LocalServer {
        executable: env!("CARGO_BIN_EXE_owned_stdio_server").into(),
        args: vec!["legacy".into(), trace.to_string_lossy().into_owned()],
        working_directory: temp.path().into(),
        environment: Vec::new(),
    };
    let mut client = StdioClient::connect(server).await.expect("legacy connect");
    assert_eq!(client.version(), ProtocolVersion::Legacy);
    let catalog = client.list_tools().await.expect("legacy tools/list");
    let result = client
        .call_tool(&catalog.tools[0], json!({"echo": "legacy"}))
        .await
        .expect("legacy tools/call");
    assert_eq!(result.text, ["legacy"]);
    client.shutdown().await.expect("child reaped");
    let methods = std::fs::read_to_string(trace).expect("method trace");
    assert!(methods.starts_with("server/discover\ninitialize\nnotifications/initialized\n"));
}

#[tokio::test]
async fn m03_modern_version_error_never_initializes_legacy() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let trace = temp.path().join("methods.txt");
    let server = LocalServer {
        executable: env!("CARGO_BIN_EXE_owned_stdio_server").into(),
        args: vec!["modern-error".into(), trace.to_string_lossy().into_owned()],
        working_directory: temp.path().into(),
        environment: Vec::new(),
    };
    let result = StdioClient::connect(server).await;
    assert!(matches!(result, Err(McpError::IncompatibleVersion)));
    let methods = std::fs::read_to_string(trace).expect("method trace");
    assert_eq!(methods, "server/discover\n");
}

#[tokio::test]
async fn m11_stdio_oversized_line_is_rejected_before_parsing() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let server = LocalServer {
        executable: env!("CARGO_BIN_EXE_owned_stdio_server").into(),
        args: vec!["oversize".into()],
        working_directory: temp.path().into(),
        environment: Vec::new(),
    };
    assert!(matches!(
        StdioClient::connect(server).await,
        Err(McpError::LimitExceeded)
    ));
}

#[tokio::test]
async fn m12_stdio_stop_cancels_exact_inflight_call_id() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let trace = temp.path().join("cancel.txt");
    let server = LocalServer {
        executable: env!("CARGO_BIN_EXE_owned_stdio_server").into(),
        args: vec!["cancel-slow".into(), trace.to_string_lossy().into_owned()],
        working_directory: temp.path().into(),
        environment: Vec::new(),
    };
    let mut client = StdioClient::connect(server).await.expect("connect");
    let catalog = client.list_tools().await.expect("tools");
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let trace_for_signal = trace.clone();
    let trigger = tokio::spawn(async move {
        for _ in 0..100 {
            if std::fs::read_to_string(&trace_for_signal)
                .is_ok_and(|value| value.contains("tools/call:3"))
            {
                signal.cancel();
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
        panic!("call never reached owned server");
    });
    let result = client
        .call_tool_with_cancel(
            &catalog.tools[0],
            json!({"echo":"slow"}),
            &cancel,
            Duration::from_secs(2),
        )
        .await;
    trigger.await.expect("trigger task");
    assert!(matches!(result, Err(McpError::Cancelled)));
    client.shutdown().await.expect("child reaped");
    let trace = std::fs::read_to_string(trace).expect("cancel trace");
    assert!(
        trace.contains("tools/call:3\nnotifications/cancelled:3\n"),
        "{trace}"
    );
}

#[tokio::test]
async fn m12_stdio_timeout_cancels_inflight_call_but_completed_call_does_not() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let slow_trace = temp.path().join("timeout.txt");
    let server = LocalServer {
        executable: env!("CARGO_BIN_EXE_owned_stdio_server").into(),
        args: vec![
            "cancel-slow".into(),
            slow_trace.to_string_lossy().into_owned(),
        ],
        working_directory: temp.path().into(),
        environment: Vec::new(),
    };
    let mut client = StdioClient::connect(server).await.expect("connect");
    let catalog = client.list_tools().await.expect("tools");
    assert!(matches!(
        client
            .call_tool_with_cancel(
                &catalog.tools[0],
                json!({"echo":"slow"}),
                &CancellationToken::new(),
                Duration::from_millis(80)
            )
            .await,
        Err(McpError::Timeout)
    ));
    client.shutdown().await.expect("child reaped");
    let trace = std::fs::read_to_string(slow_trace).expect("timeout trace");
    assert!(
        trace.contains("tools/call:3\nnotifications/cancelled:3\n"),
        "{trace}"
    );

    let fast_trace = temp.path().join("fast.txt");
    let server = LocalServer {
        executable: env!("CARGO_BIN_EXE_owned_stdio_server").into(),
        args: vec![
            "cancel-fast".into(),
            fast_trace.to_string_lossy().into_owned(),
        ],
        working_directory: temp.path().into(),
        environment: Vec::new(),
    };
    let mut client = StdioClient::connect(server).await.expect("connect");
    let catalog = client.list_tools().await.expect("tools");
    let cancel = CancellationToken::new();
    client
        .call_tool_with_cancel(
            &catalog.tools[0],
            json!({"echo":"fast"}),
            &cancel,
            Duration::from_secs(2),
        )
        .await
        .expect("completed");
    cancel.cancel();
    assert!(matches!(
        client
            .call_tool_with_cancel(
                &catalog.tools[0],
                json!({"echo":"must not dispatch"}),
                &cancel,
                Duration::from_secs(2),
            )
            .await,
        Err(McpError::Cancelled)
    ));
    client.shutdown().await.expect("child reaped");
    let trace = std::fs::read_to_string(fast_trace).expect("fast trace");
    assert!(trace.contains("tools/call:3\n"), "{trace}");
    assert!(!trace.contains("notifications/cancelled"), "{trace}");
}

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

#[derive(Clone, Copy)]
enum Scenario {
    ModernMixed,
    Legacy,
    ModernError,
    OldSse,
    Paginated,
    CursorCycle,
}

#[derive(Debug)]
struct CapturedRequest {
    verb: String,
    path: String,
    headers: HashMap<String, String>,
    body: Value,
}

async fn start_server(scenario: Scenario) -> (String, mpsc::UnboundedReceiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned listener");
    let endpoint = format!("http://{}/mcp", listener.local_addr().expect("owned addr"));
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.expect("owned accept");
            let (stream, request) = read_request(stream).await;
            let response = response_for(scenario, &request);
            sender.send(request).expect("capture receiver alive");
            write_response(stream, response.0, response.1, response.2, response.3).await;
        }
    });
    (endpoint, receiver)
}

async fn read_request(mut stream: TcpStream) -> (TcpStream, CapturedRequest) {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).await.expect("owned request read");
        assert!(count > 0, "HTTP request not closed before headers");
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() < 1024 * 1024, "owned request bound");
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let head = std::str::from_utf8(&bytes[..header_end]).expect("ASCII headers");
    let mut lines = head.split("\r\n");
    let request_line = lines.next().expect("request line");
    let mut request_parts = request_line.split_whitespace();
    let verb = request_parts.next().expect("verb").to_owned();
    let path = request_parts.next().expect("path").to_owned();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length: usize = headers["content-length"].parse().expect("content length");
    while bytes.len() - header_end < length {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).await.expect("owned body read");
        assert!(count > 0, "HTTP request body complete");
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = serde_json::from_slice(&bytes[header_end..header_end + length]).expect("JSON body");
    (
        stream,
        CapturedRequest {
            verb,
            path,
            headers,
            body,
        },
    )
}

fn response_for(
    scenario: Scenario,
    request: &CapturedRequest,
) -> (u16, String, &'static str, Vec<(&'static str, &'static str)>) {
    let id = request.body.get("id").cloned().unwrap_or(Value::Null);
    let method = request.body["method"].as_str().unwrap_or_default();
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
        (_, "tools/call") => (
            200,
            json!({"resultType":"complete", "content":[{"type":"text", "text": if matches!(scenario, Scenario::Legacy) { request.body["params"]["arguments"]["echo"].clone() } else { json!("ok") }}], "isError":false}),
            "application/json",
            Vec::new(),
        ),
        _ => panic!("unexpected owned request: {method}"),
    };
    let envelope = json!({"jsonrpc":"2.0", "id":id, "result":result});
    let body = if kind == "text/event-stream" {
        format!(": keepalive\n\ndata: {envelope}\n\n")
    } else {
        envelope.to_string()
    };
    (status, body, kind, extra)
}

async fn write_response(
    mut stream: TcpStream,
    status: u16,
    body: String,
    kind: &'static str,
    extra: Vec<(&'static str, &'static str)>,
) {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("head write");
    stream.write_all(body.as_bytes()).await.expect("body write");
}
