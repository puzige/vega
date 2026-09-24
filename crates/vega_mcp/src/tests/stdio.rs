use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;
use vega_mcp::{LocalServer, McpError, ProtocolVersion, StdioClient};

#[tokio::test]
async fn m02_modern_stdio_duplex_round_trip() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let fixture = owned_stdio_fixture(temp.path());
    let server = LocalServer {
        executable: fixture.path().into(),
        args: vec!["modern".into()],
        working_directory: temp.path().into(),
        environment: Vec::new(),
    };
    let mut client = StdioClient::connect(server).await.expect("modern connect");
    assert_eq!(client.version(), ProtocolVersion::Modern);
    let catalog = client.list_tools().await.expect("protocol tools/list");
    assert_eq!(catalog.tools.len(), 1);
    assert_eq!(catalog.tools[0].name, "echo");
    let result = client
        .call_tool(&catalog.tools[0], json!({"echo": "from duplex peer"}))
        .await
        .expect("protocol tools/call");
    assert_eq!(result.text, ["from duplex peer"]);
    assert!(!result.is_error);
    client.shutdown().await.expect("peer completed");
}

#[tokio::test]
async fn m03_legacy_stdio_only_after_nonmodern_probe() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let fixture = owned_stdio_fixture(temp.path());
    let trace = temp.path().join("methods.txt");
    let server = LocalServer {
        executable: fixture.path().into(),
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
    client.shutdown().await.expect("peer completed");
    let methods = std::fs::read_to_string(trace).expect("method trace");
    assert!(methods.starts_with("server/discover\ninitialize\nnotifications/initialized\n"));
}

#[tokio::test]
async fn m03_modern_version_error_never_initializes_legacy() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let fixture = owned_stdio_fixture(temp.path());
    let trace = temp.path().join("methods.txt");
    let server = LocalServer {
        executable: fixture.path().into(),
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
    let fixture = owned_stdio_fixture(temp.path());
    let server = LocalServer {
        executable: fixture.path().into(),
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
async fn m11_stdio_line_accepts_exact_1m_and_rejects_plus_one() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let fixture = owned_stdio_fixture(temp.path());
    for (mode, allowed) in [("modern-line-exact", true), ("modern-line-over", false)] {
        let server = LocalServer {
            executable: fixture.path().into(),
            args: vec![mode.into()],
            working_directory: temp.path().into(),
            environment: Vec::new(),
        };
        let result = StdioClient::connect(server).await;
        if allowed {
            let client = result.expect("exactly 1 MiB line accepted");
            client.shutdown().await.expect("owned peer completed");
        } else {
            assert!(matches!(result, Err(McpError::LimitExceeded)));
        }
    }
}

#[tokio::test]
async fn m12_stdio_stop_cancels_exact_inflight_call_id() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let fixture = owned_stdio_fixture(temp.path());
    let trace = temp.path().join("cancel.txt");
    let server = LocalServer {
        executable: fixture.path().into(),
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
    client.shutdown().await.expect("peer completed");
    let trace = std::fs::read_to_string(trace).expect("cancel trace");
    assert!(
        trace.contains("tools/call:3\nnotifications/cancelled:3\n"),
        "{trace}"
    );
}

#[tokio::test]
async fn m12_stdio_timeout_cancels_inflight_call_but_completed_call_does_not() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let fixture = owned_stdio_fixture(temp.path());
    let slow_trace = temp.path().join("timeout.txt");
    let server = LocalServer {
        executable: fixture.path().into(),
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
    client.shutdown().await.expect("peer completed");
    let trace = std::fs::read_to_string(slow_trace).expect("timeout trace");
    assert!(
        trace.contains("tools/call:3\nnotifications/cancelled:3\n"),
        "{trace}"
    );

    let fast_trace = temp.path().join("fast.txt");
    let server = LocalServer {
        executable: fixture.path().into(),
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
    client.shutdown().await.expect("peer completed");
    let trace = std::fs::read_to_string(fast_trace).expect("fast trace");
    assert!(trace.contains("tools/call:3\n"), "{trace}");
    assert!(!trace.contains("notifications/cancelled"), "{trace}");
}

#[tokio::test]
async fn m12_stdio_peer_eof_after_dispatch_is_transport_failure_without_replay() {
    let temp = tempfile::tempdir().expect("owned cwd");
    let fixture = owned_stdio_fixture(temp.path());
    let trace = temp.path().join("exited.txt");
    let server = LocalServer {
        executable: fixture.path().into(),
        args: vec!["exit-on-call".into(), trace.to_string_lossy().into_owned()],
        working_directory: temp.path().into(),
        environment: Vec::new(),
    };
    let mut client = StdioClient::connect(server).await.expect("owned connect");
    let catalog = client.list_tools().await.expect("owned catalog");
    assert!(matches!(
        client
            .call_tool(&catalog.tools[0], json!({"echo":"side effect unknown"}))
            .await,
        Err(McpError::Transport)
    ));
    client.shutdown().await.expect("exited peer completed");
    let methods = std::fs::read_to_string(trace).expect("owned trace");
    assert_eq!(methods.matches("tools/call").count(), 1);
}

#[tokio::test]
async fn m11_stdio_paginated_catalog_counts_exact_raw_wire_bytes() {
    for over in [false, true] {
        let temp = tempfile::tempdir().expect("owned cwd");
        let fixture = owned_stdio_fixture(temp.path());
        let server = LocalServer {
            executable: fixture.path().into(),
            args: vec![if over {
                "modern-catalog-wire-over".into()
            } else {
                "modern-catalog-wire-exact".into()
            }],
            working_directory: temp.path().into(),
            environment: Vec::new(),
        };
        let mut client = StdioClient::connect(server).await.expect("owned connect");
        let result = client.list_tools().await;
        if over {
            assert!(matches!(result, Err(McpError::LimitExceeded)));
        } else {
            assert_eq!(result.expect("exact raw 4 MiB").tools.len(), 1);
        }
        client.shutdown().await.expect("owned peer completed");
    }
}

fn owned_stdio_fixture(root: &std::path::Path) -> vega_mcp::mock::StdioFixture {
    vega_mcp::mock::StdioFixture::new(
        root.join("owned-stdio"),
        std::sync::Arc::new(|server, stream| {
            Box::pin(async move {
                let _ = run_stdio_fixture(server, stream).await;
            })
        }),
    )
}
async fn run_stdio_fixture(
    server: LocalServer,
    stream: tokio::io::DuplexStream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = server.args.into_iter();
    let mode = args.next().ok_or("missing mode")?;
    let trace = args.next();
    let (reader, mut stdout) = tokio::io::split(stream);
    let mut lines = tokio::io::BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        let request: Value = serde_json::from_str(&line)?;
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
            use std::io::Write;
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
            stdout.write_all(response.as_bytes()).await?;
            stdout.write_all(" ".repeat(padding).as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
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
            let wire_bytes = if page < 5 { 838_861 } else { 838_860 }
                + usize::from(page == 5 && mode.ends_with("-over"));
            let padding = wire_bytes - response.len() - 1;
            stdout.write_all(response.as_bytes()).await?;
            stdout.write_all(" ".repeat(padding).as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
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
        stdout.write_all(format!("{response}\n").as_bytes()).await?;
        stdout.flush().await?;
    }
    Ok(())
}
