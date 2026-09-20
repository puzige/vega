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
        if (matches!(
            mode.as_str(),
            "modern" | "cancel-slow" | "cancel-fast" | "exit-on-call"
        ) || mode.starts_with("modern-catalog-wire-")
            || mode.starts_with("modern-line-"))
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
            if mode.starts_with("cancel-") {
                let id = request
                    .get("id")
                    .or_else(|| request.pointer("/params/requestId"));
                writeln!(
                    file,
                    "{method}:{}",
                    id.and_then(Value::as_u64).ok_or("missing id")?
                )?;
            } else {
                writeln!(file, "{method}")?;
            }
        }
        if method == "notifications/cancelled" && mode == "cancel-slow" {
            break;
        }
        let Some(id) = request.get("id") else {
            continue;
        };
        if method == "tools/call" && mode == "cancel-slow" {
            continue;
        }
        if method == "tools/call" && mode == "exit-on-call" {
            break;
        }
        if method == "server/discover" && mode.starts_with("modern-line-") {
            let response = json!({"jsonrpc":"2.0", "id":id, "result":{
                "resultType":"complete", "supportedVersions":["2026-07-28"],
                "capabilities":{"tools":{}}, "ttlMs":0, "cacheScope":"private"
            }})
            .to_string();
            let wire_bytes = 1024 * 1024 + usize::from(mode.ends_with("-over"));
            let padding = wire_bytes - response.len() - 1;
            stdout.write_all(response.as_bytes())?;
            stdout.write_all(" ".repeat(padding).as_bytes())?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
            continue;
        }
        if method == "tools/list" && mode.starts_with("modern-catalog-wire-") {
            let page = request["params"]["cursor"]
                .as_str()
                .and_then(|cursor| cursor.parse::<usize>().ok())
                .unwrap_or(1);
            if !(1..=5).contains(&page) {
                return Err("invalid owned catalog cursor".into());
            }
            let mut result = json!({"resultType":"complete", "tools":if page == 1 {
                vec![json!({"name":"owned", "inputSchema":{"type":"object"}})]
            } else {
                Vec::new()
            }, "ttlMs":0, "cacheScope":"private"});
            if page < 5 {
                result["nextCursor"] = json!((page + 1).to_string());
            }
            let response = json!({"jsonrpc":"2.0", "id":id, "result":result}).to_string();
            // Five valid lines sum to exactly 4 MiB, then the over variant
            // adds one wire byte. Every individual line remains below 1 MiB.
            let wire_bytes = if page < 5 { 838_861 } else { 838_860 }
                + usize::from(page == 5 && mode.ends_with("-over"));
            let padding = wire_bytes - response.len() - 1;
            stdout.write_all(response.as_bytes())?;
            stdout.write_all(" ".repeat(padding).as_bytes())?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
            continue;
        }
        let response = match method {
            "server/discover" if mode == "oversize" => json!({
                "jsonrpc":"2.0", "id":id,
                "result":{"resultType":"complete", "supportedVersions":["2026-07-28"],
                    "capabilities":{"tools":{}}, "ttlMs":0, "cacheScope":"private",
                    "instructions":"x".repeat(1024 * 1024)}
            }),
            "server/discover"
                if matches!(
                    mode.as_str(),
                    "modern" | "cancel-slow" | "cancel-fast" | "exit-on-call"
                ) || mode.starts_with("modern-catalog-wire-") =>
            {
                json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"resultType": "complete", "supportedVersions": ["2026-07-28"],
                        "capabilities": {"tools": {}}, "ttlMs": 0, "cacheScope": "private"}
                })
            }
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
