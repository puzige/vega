//! Owned wire-level fixture for vega_mcp integration tests.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or("missing mode")?;
    let trace = args.next();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let request: Value = serde_json::from_str(&line?)?;
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .ok_or("missing method")?;
        if mode == "modern"
            && request.get("id").is_some()
            && request["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] != "2026-07-28"
        {
            return Err("missing modern request metadata".into());
        }
        if let Some(path) = trace.as_ref() {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            writeln!(file, "{method}")?;
        }
        let Some(id) = request.get("id") else {
            continue;
        };
        let response = match method {
            "server/discover" if mode == "oversize" => json!({
                "jsonrpc":"2.0", "id":id,
                "result":{"resultType":"complete", "supportedVersions":["2026-07-28"],
                    "capabilities":{"tools":{}}, "ttlMs":0, "cacheScope":"private",
                    "instructions":"x".repeat(1024 * 1024)}
            }),
            "server/discover" if mode == "modern" => json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"resultType": "complete", "supportedVersions": ["2026-07-28"],
                    "capabilities": {"tools": {}}, "ttlMs": 0, "cacheScope": "private"}
            }),
            "server/discover" if mode == "modern-error" => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32022, "message": "unsupported version",
                    "data": {"requested": "2026-07-28", "supported": ["2027-01-01"]}}
            }),
            "server/discover" => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": "unknown method"}
            }),
            "initialize" if mode == "legacy" => json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"protocolVersion": "2025-11-25", "capabilities": {"tools": {}},
                    "serverInfo": {"name": "owned-stdio", "version": "1"}}
            }),
            "tools/list" => json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"resultType": "complete", "tools": [{"name": "echo", "description": "Echo text",
                    "inputSchema": {"type": "object", "properties": {"echo": {"type": "string"}}}}],
                    "ttlMs": 0, "cacheScope": "private"}
            }),
            "tools/call" => json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"resultType": "complete", "content": [{"type": "text",
                    "text": request["params"]["arguments"]["echo"]}], "isError": false}
            }),
            _ => json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": "unknown method"}}),
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}
