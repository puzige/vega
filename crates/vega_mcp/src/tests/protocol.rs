use std::collections::HashMap;

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use vega_mcp::{HttpClient, McpError, ProtocolVersion};

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

struct StreamDrop(Option<oneshot::Sender<bool>>);
impl Drop for StreamDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(true);
        }
    }
}
