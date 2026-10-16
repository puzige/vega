use std::collections::HashMap;

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

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
struct StreamDrop(Option<oneshot::Sender<bool>>);
impl Drop for StreamDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(true);
        }
    }
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
