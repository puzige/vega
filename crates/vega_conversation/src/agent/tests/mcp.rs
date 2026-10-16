use super::*;
use crate::McpServerSettingsService;
use crate::types::{
    ApprovalAudit, ApprovalSource, McpCallIdentity, McpRemoteAuthorization, McpServerForm,
    McpServerTransport, ToolCall, ToolCallStatus, ToolCardInputProjection,
    ToolCardResultProjection,
};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use vega_runtime::RuntimeToolCall;
use vega_runtime::{
    McpReadyServer, RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource,
    RuntimeEvent, RuntimeToolResult,
};

#[derive(Clone)]
struct M07LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for M07LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct OwnedCrossRuntimeHttpFixture {
    endpoint: String,
    _registration: vega_mcp::mock::Endpoint,
    discoveries: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}
impl OwnedCrossRuntimeHttpFixture {
    fn start() -> Self {
        let discoveries = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let worker_discoveries = discoveries.clone();
        let worker_calls = calls.clone();
        let endpoint = remote_fixture(
            serde_json::json!([{"name":"lookup","inputSchema":{"type":"object"}}]),
            "current-runtime-http",
            Arc::new(move |request| match request["method"].as_str().unwrap() {
                "server/discover" => {
                    worker_discoveries.fetch_add(1, Ordering::SeqCst);
                }
                "tools/call" => {
                    worker_calls.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }),
        );
        Self {
            endpoint: endpoint.to_string(),
            _registration: endpoint,
            discoveries,
            calls,
        }
    }
}
fn remote_fixture(
    tools: serde_json::Value,
    answer: &str,
    observe: Arc<dyn Fn(&serde_json::Value) + Send + Sync>,
) -> vega_mcp::mock::Endpoint {
    let answer = answer.to_owned();
    vega_mcp::mock::Endpoint::new("/mcp", |_| {
        Arc::new(move |request| {
            let request: serde_json::Value =
                serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
            observe(&request);
            let reply = vega_mcp::mock::catalog_reply(&request, &tools, &answer)
                .unwrap()
                .to_string();
            Box::pin(async move {
                Ok(vega_mcp::mock::response(
                    200,
                    "application/json",
                    reply,
                    Vec::new(),
                ))
            })
        })
    })
}
