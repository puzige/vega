use super::*;
use crate::McpServerSettingsService;
use crate::types::{
    ApprovalAudit, ApprovalSource, McpCallIdentity, McpRemoteAuthorization, McpServerForm,
    McpServerTransport, ToolCall,
};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use vega_runtime::RuntimeToolCall;
use vega_runtime::{
    McpReadyServer, RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource,
    RuntimeEvent, RuntimeToolResult,
};

fn proposal() -> (RuntimeToolCall, McpCallIdentity) {
    let identity = McpCallIdentity {
        server_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        config_revision: 3,
        exact_tool_name: "lookup".into(),
        arguments_bytes: 24,
        arguments_sha256: "a".repeat(64),
        argument_preview: "query: string".into(),
    };
    let call = RuntimeToolCall {
        id: "call-mcp".into(),
        name: identity.alias(),
        input_json: serde_json::json!({
            "server_id": identity.server_id,
            "config_revision": identity.config_revision,
            "tool": identity.exact_tool_name,
            "arguments_bytes": identity.arguments_bytes,
            "arguments_sha256": identity.arguments_sha256,
            "argument_preview": identity.argument_preview,
        })
        .to_string(),
    };
    (call, identity)
}

#[test]
fn issue73_external_proposal_and_permission_bind_to_same_safe_identity() {
    let (call, identity) = proposal();
    validate_runtime_proposal(&call).unwrap();
    let request = PermissionRequest {
        call_id: call.id.clone(),
        tool: call.name.clone(),
        display_target: identity.permission_target(),
        danger_rule_id: None,
        danger_reason: None,
        external: Some(identity),
    };
    assert!(valid_permission_request(&request));
    let mut swapped = request.clone();
    swapped.display_target.push('x');
    assert!(!valid_permission_request(&swapped));
    let mut wrong_alias = call;
    wrong_alias.name.push('x');
    assert!(validate_runtime_proposal(&wrong_alias).is_err());
}

#[test]
fn issue73_external_terminal_accepts_one_shot_and_rejects_remembered_grant() {
    let (call, identity) = proposal();
    let audit = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::User,
        danger: None,
    };
    let output = "[Untrusted external MCP result]\nfound one entry";
    let canonical = validate_recovered_projection(
        "project-1",
        "thread-1",
        &call.id,
        &call.name,
        &call.input_json,
        output,
        RuntimeToolStatus::Success,
        &audit,
        None,
        None,
    )
    .unwrap();
    assert_eq!(canonical, call.input_json);
    let state = tool_calls::ToolCallState {
        thread_id: "thread-1".into(),
        tool: call.name.clone(),
        input_json: call.input_json.clone(),
        status: "pending_approval".into(),
        approval: None,
        output_text: None,
        exit_code: None,
        duration_ms: None,
        output_full_path: None,
    };
    let approved = vega_runtime::RuntimeApprovalAudit {
        decision: vega_runtime::RuntimeApprovalDecision::Once,
        note: None,
        source: vega_runtime::RuntimeApprovalSource::User,
        danger: None,
    };
    validate_runtime_approval_event(&state, &call.id, &approved, None).unwrap();
    let remembered = vega_runtime::RuntimeApprovalAudit {
        decision: vega_runtime::RuntimeApprovalDecision::Always,
        ..approved
    };
    assert!(validate_runtime_approval_event(&state, &call.id, &remembered, None).is_err());
    let bad_audit = ApprovalAudit {
        decision: Approval::Always,
        ..audit
    };
    assert!(
        validate_recovered_projection(
            "project-1",
            "thread-1",
            &call.id,
            &call.name,
            &call.input_json,
            output,
            RuntimeToolStatus::Success,
            &bad_audit,
            None,
            None,
        )
        .is_err()
    );
    let projected = ToolCall {
        id: call.id,
        tool: call.name,
        input_json: call.input_json,
    };
    assert_eq!(
        McpCallIdentity::from_tool_call(&projected).unwrap(),
        identity
    );
}

#[test]
fn issue73_external_call_persists_all_critical_transitions_and_safe_card() {
    let (store, _dir, project_id) = setup();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "assistant-mcp".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let (call, _) = proposal();
    let mut sequence = 1;
    let persist = |event: RuntimeEvent, sequence: &mut i64| {
        persist_runtime_event(
            &store,
            &project_id,
            "thread-1",
            "assistant-mcp",
            "mock-model",
            false,
            "",
            sequence,
            &event,
        )
    };
    persist(RuntimeEvent::ToolCallProposed(call.clone()), &mut sequence).unwrap();
    let audit = RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Once,
        note: None,
        source: RuntimeApprovalSource::User,
        danger: None,
    };
    persist(
        RuntimeEvent::ToolCallApproved {
            call_id: call.id.clone(),
            audit: audit.clone(),
            remember_rule: None,
        },
        &mut sequence,
    )
    .unwrap();
    persist(
        RuntimeEvent::ToolCallRunning {
            call_id: call.id.clone(),
        },
        &mut sequence,
    )
    .unwrap();
    persist(
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            call_id: call.id.clone(),
            output: "[Untrusted external MCP result]\nfound one entry".into(),
            status: RuntimeToolStatus::Success,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: Some(false),
            approval: Some(audit),
            remember_rule: None,
        }),
        &mut sequence,
    )
    .unwrap();
    let state = tool_calls::find_state(store.conn(), &call.id)
        .unwrap()
        .unwrap();
    assert_eq!(state.status, "success");
    assert_eq!(state.tool, call.name);
    assert_eq!(state.input_json, call.input_json);
    assert_eq!(
        state.output_text.as_deref(),
        Some("[Untrusted external MCP result]\nfound one entry")
    );
    assert_eq!(sequence, 2);
    assert!(!state.input_json.contains("SECRET_ARGUMENT_VALUE"));
}

#[test]
fn issue73_unknown_external_outcome_remains_explicit_after_store_restart() {
    let (store, directory, project_id) = setup();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "assistant-unknown".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let (call, _) = proposal();
    let mut sequence = 1;
    for event in [
        RuntimeEvent::ToolCallProposed(call.clone()),
        RuntimeEvent::ToolCallApproved {
            call_id: call.id.clone(),
            audit: RuntimeApprovalAudit {
                decision: RuntimeApprovalDecision::Once,
                note: None,
                source: RuntimeApprovalSource::User,
                danger: None,
            },
            remember_rule: None,
        },
        RuntimeEvent::ToolCallRunning {
            call_id: call.id.clone(),
        },
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            call_id: call.id.clone(),
            output: "Tool error: MCP call outcome unknown after cancellation".into(),
            status: RuntimeToolStatus::Cancelled,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            approval: Some(RuntimeApprovalAudit {
                decision: RuntimeApprovalDecision::Once,
                note: None,
                source: RuntimeApprovalSource::User,
                danger: None,
            }),
            remember_rule: None,
        }),
    ] {
        persist_runtime_event(
            &store,
            &project_id,
            "thread-1",
            "assistant-unknown",
            "mock-model",
            false,
            "",
            &mut sequence,
            &event,
        )
        .unwrap();
    }
    drop(store);
    let reopened = Store::open(directory.path().join("vega.db")).unwrap();
    reopened.migrate().unwrap();
    let card = tool_calls::find_state(reopened.conn(), &call.id)
        .unwrap()
        .unwrap();
    assert_eq!(card.status, "cancelled");
    assert_eq!(
        card.output_text.as_deref(),
        Some("Tool error: MCP call outcome unknown after cancellation")
    );
    assert!(!card.input_json.contains("SENTINEL_PRIVATE"));
}

#[tokio::test]
async fn issue73_owned_stdio_reaches_durable_conversation_and_second_provider_round() {
    let (store, project_dir, data_dir, _) = setup_external("confirm");
    let script = data_dir.path().join("owned-mcp.sh");
    let calls_log = data_dir.path().join("calls.log");
    fs::write(
        &script,
        r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}}'
      ;;
    *tools/list*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{"name":"echo","description":"Owned echo fixture","inputSchema":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}]}}'
      ;;
    *tools/call*)
      printf '%s\n' "$request" >> "$1"
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"resultType":"complete","content":[{"type":"text","text":"owned-answer"}],"isError":false}}'
      ;;
  esac
done
"##,
    )
    .unwrap();
    let server_id = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    let ready = McpReadyServer::connect_local(
        server_id.into(),
        1,
        vega_mcp::LocalServer {
            executable: "/bin/sh".into(),
            args: vec![
                script.to_string_lossy().to_string(),
                calls_log.to_string_lossy().to_string(),
            ],
            working_directory: data_dir.path().to_path_buf(),
            environment: Vec::new(),
        },
    )
    .await
    .unwrap();
    let alias = McpCallIdentity {
        server_id: server_id.into(),
        config_revision: 1,
        exact_tool_name: "echo".into(),
        arguments_bytes: 0,
        arguments_sha256: "0".repeat(64),
        argument_preview: String::new(),
    }
    .alias();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "owned-stdio-call".into(),
                name: alias.clone(),
                input_json: r#"{"query":"SENTINEL_PRIVATE"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Tool confirmed.".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Once,
    };
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Use the owned echo server",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
        vec![ready.clone()],
    )
    .await
    .unwrap();
    ready.revoke();
    assert!(!run.failed);
    assert_eq!(hook.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fs::read_to_string(&calls_log).unwrap().lines().count(), 1);
    assert!(
        fs::read_to_string(&calls_log)
            .unwrap()
            .contains("SENTINEL_PRIVATE")
    );
    let row = tool_calls::find_state(store.conn(), "owned-stdio-call")
        .unwrap()
        .unwrap();
    assert_eq!(row.status, "success");
    assert_eq!(row.tool, alias);
    assert!(!row.input_json.contains("SENTINEL_PRIVATE"));
    assert!(row.input_json.contains("arguments_sha256"));
    assert!(row.output_text.unwrap().contains("owned-answer"));
    assert!(provider.requests()[1].messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Tool && message.content.contains("owned-answer")
    }));
}

#[tokio::test]
async fn issue73_owned_remote_http_reaches_durable_conversation_and_second_provider_round() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
    let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut stream, request) = read_owned_mcp_http_request(stream).await;
            let id = request["id"].clone();
            let method = request["method"].as_str().unwrap();
            let result = match method {
                "server/discover" => serde_json::json!({
                    "resultType":"complete", "supportedVersions":["2026-07-28"],
                    "capabilities":{"tools":{}}, "ttlMs":0, "cacheScope":"private"
                }),
                "tools/list" => serde_json::json!({
                    "resultType":"complete", "ttlMs":0, "cacheScope":"private",
                    "tools":[{"name":"lookup","inputSchema":{"type":"object",
                        "properties":{"query":{"type":"string"}}, "required":["query"]}}]
                }),
                "tools/call" => {
                    sent.send(request["params"]["arguments"].clone()).unwrap();
                    serde_json::json!({"resultType":"complete", "isError":false,
                        "content":[{"type":"text", "text":"remote-answer"}]})
                }
                other => panic!("unexpected owned MCP method: {other}"),
            };
            let response =
                serde_json::json!({"jsonrpc":"2.0", "id":id, "result":result}).to_string();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let (store, project_dir, _data_dir, _) = setup_external("confirm");
    let server_id = "01K5KK7PZ5J8V2GSBMQKS8W71B";
    let client = vega_mcp::HttpClient::connect(&endpoint, true)
        .await
        .unwrap();
    let ready = McpReadyServer::connect_http(server_id.into(), 4, client)
        .await
        .unwrap();
    let alias = McpCallIdentity {
        server_id: server_id.into(),
        config_revision: 4,
        exact_tool_name: "lookup".into(),
        arguments_bytes: 0,
        arguments_sha256: "0".repeat(64),
        argument_preview: String::new(),
    }
    .alias();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "owned-http-call".into(),
                name: alias.clone(),
                input_json: r#"{"query":"PRIVATE_REMOTE_ARGUMENT"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Remote tool confirmed.".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Once,
    };
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Use the owned remote lookup server",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
        vec![ready.clone()],
    )
    .await
    .unwrap();
    ready.revoke();
    assert!(!run.failed);
    assert_eq!(hook.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        received.try_recv().unwrap()["query"],
        "PRIVATE_REMOTE_ARGUMENT"
    );
    assert!(received.try_recv().is_err());
    let row = tool_calls::find_state(store.conn(), "owned-http-call")
        .unwrap()
        .unwrap();
    assert_eq!(row.status, "success");
    assert_eq!(row.tool, alias);
    assert!(!row.input_json.contains("PRIVATE_REMOTE_ARGUMENT"));
    assert!(row.output_text.unwrap().contains("remote-answer"));
    assert!(provider.requests()[1].messages.iter().any(|message| {
        message.role == vega_runtime::ChatRole::Tool && message.content.contains("remote-answer")
    }));
    server.abort();
}

async fn read_owned_mcp_http_request(mut stream: TcpStream) -> (TcpStream, serde_json::Value) {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0, "HTTP request closed before headers");
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() < 1024 * 1024, "owned HTTP request bound");
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
    let content_length = headers
        .split("\r\n")
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    while bytes.len() - header_end < content_length {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0, "HTTP request body complete");
        bytes.extend_from_slice(&chunk[..count]);
    }
    let request = serde_json::from_slice(&bytes[header_end..header_end + content_length]).unwrap();
    (stream, request)
}

fn owned_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("owned UI/run runtime")
}

struct OwnedCallSpec<'a> {
    server_id: &'a str,
    revision: u64,
    tool_name: &'a str,
    call_id: &'a str,
    expected_output: &'a str,
}

async fn assert_two_owned_mcp_calls(
    store: &Store,
    project_dir: &Path,
    ready_servers: Vec<McpReadyServer>,
    spec: OwnedCallSpec<'_>,
) {
    let OwnedCallSpec {
        server_id,
        revision,
        tool_name,
        call_id,
        expected_output,
    } = spec;
    let alias = McpCallIdentity {
        server_id: server_id.into(),
        config_revision: revision,
        exact_tool_name: tool_name.into(),
        arguments_bytes: 0,
        arguments_sha256: "0".repeat(64),
        argument_preview: String::new(),
    }
    .alias();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: call_id.into(),
                name: alias.clone(),
                input_json: r#"{"query":"cross-runtime"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: format!("{call_id}-second"),
                name: alias,
                input_json: r#"{"query":"same-run-second-round"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let tools = vega_tools::Tools::new(project_dir).expect("owned project tools");
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Once,
    };
    let run = tokio::time::timeout(
        Duration::from_secs(8),
        run_thread_task_with_images_reasoning_and_mcp(
            store,
            &provider,
            &tools,
            "thread-1",
            "Call the newly connected MCP server",
            "System",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
            PersistenceActorConfig::default(),
            None,
            None,
            None,
            Vec::new(),
            ready_servers,
        ),
    )
    .await
    .expect("current run runtime must not wait on an old runtime's connection")
    .expect("owned conversation");
    assert!(!run.failed);
    assert_eq!(hook.calls.load(Ordering::SeqCst), 2);
    assert_eq!(provider.requests().len(), 3);
    for id in [call_id.to_owned(), format!("{call_id}-second")] {
        let call = tool_calls::find_state(store.conn(), &id)
            .expect("tool audit")
            .expect("tool card");
        assert_eq!(call.status, "success");
        assert!(!call.input_json.contains("cross-runtime"));
        assert!(!call.input_json.contains("same-run-second-round"));
        assert!(
            call.output_text
                .expect("tool output")
                .contains(expected_output)
        );
    }
}

#[test]
fn issue73_local_enable_then_new_runtime_reconnects_and_calls_tool() {
    let (store, project_dir, data_dir, _) = setup_external("confirm");
    let script = data_dir.path().join("runtime-boundary-mcp.sh");
    let starts = data_dir.path().join("starts.log");
    fs::write(
        &script,
        r##"#!/bin/sh
printf 'started\n' >> "$1"
while IFS= read -r request; do
  case "$request" in
    *server/discover*) printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}}' ;;
    *tools/list*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}' ;;
    *tools/call*)
      id=$(printf '%s\n' "$request" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '{"jsonrpc":"2.0","id":%s,"result":{"resultType":"complete","content":[{"type":"text","text":"current-runtime-stdio"}],"isError":false}}\n' "$id"
      ;;
  esac
done
"##,
    )
    .expect("owned stdio fixture");
    let service = McpServerSettingsService::new(
        data_dir.path().join("vega.db"),
        data_dir.path().join("config"),
    );
    let saved = service
        .create(McpServerForm {
            display_name: "runtime-boundary-stdio".into(),
            transport: McpServerTransport::Local {
                executable: "/bin/sh".into(),
                args: vec![
                    script.to_string_lossy().into_owned(),
                    starts.to_string_lossy().into_owned(),
                ],
                working_directory: Some(data_dir.path().to_path_buf()),
                environment: Vec::new(),
            },
        })
        .expect("disabled local MCP");
    let settings_runtime = owned_runtime();
    let enabled = settings_runtime
        .block_on(service.set_enabled(&saved.id, saved.config_revision, true, true))
        .expect("explicitly enabled local MCP");
    drop(settings_runtime);

    let run_runtime = owned_runtime();
    let readiness = run_runtime
        .block_on(service.ready_for_run())
        .expect("ready in current run runtime");
    assert_eq!(readiness.ready_servers.len(), 1);
    assert!(readiness.unavailable.is_empty());
    assert_eq!(
        fs::read_to_string(&starts)
            .expect("owned process starts")
            .lines()
            .count(),
        2,
        "Settings test and user run must not share a stdio handle across runtimes"
    );
    run_runtime.block_on(assert_two_owned_mcp_calls(
        &store,
        project_dir.path(),
        readiness.ready_servers,
        OwnedCallSpec {
            server_id: &saved.id,
            revision: enabled.config_revision,
            tool_name: "echo",
            call_id: "cross-runtime-stdio-call",
            expected_output: "current-runtime-stdio",
        },
    ));
    assert_eq!(
        fs::read_to_string(&starts)
            .expect("same run must retain its one stdio child")
            .lines()
            .count(),
        2
    );
}

struct OwnedCrossRuntimeHttpFixture {
    endpoint: String,
    discoveries: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl OwnedCrossRuntimeHttpFixture {
    fn start() -> Self {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("owned HTTP fixture");
        listener
            .set_nonblocking(true)
            .expect("bounded fixture accept");
        let endpoint = format!(
            "http://{}/mcp",
            listener.local_addr().expect("fixture port")
        );
        let discoveries = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_discoveries = discoveries.clone();
        let worker_calls = calls.clone();
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("owned fixture accept: {error}"),
                };
                // macOS may inherit O_NONBLOCK from the listener. A first
                // WouldBlock is not an empty HTTP request.
                stream
                    .set_nonblocking(false)
                    .expect("blocking accepted stream");
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("bounded request read");
                let Some(request) = read_owned_sync_http_request(&mut stream) else {
                    continue;
                };
                let method = request["method"].as_str().expect("MCP method");
                let result = match method {
                    "server/discover" => {
                        worker_discoveries.fetch_add(1, Ordering::SeqCst);
                        serde_json::json!({
                            "resultType":"complete", "supportedVersions":["2026-07-28"],
                            "capabilities":{"tools":{}}, "ttlMs":0, "cacheScope":"private"
                        })
                    }
                    "tools/list" => serde_json::json!({
                        "resultType":"complete", "ttlMs":0, "cacheScope":"private",
                        "tools":[{"name":"lookup","inputSchema":{"type":"object"}}]
                    }),
                    "tools/call" => {
                        worker_calls.fetch_add(1, Ordering::SeqCst);
                        serde_json::json!({
                            "resultType":"complete", "isError":false,
                            "content":[{"type":"text", "text":"current-runtime-http"}]
                        })
                    }
                    other => panic!("unexpected MCP method: {other}"),
                };
                let body = serde_json::json!({
                    "jsonrpc":"2.0", "id":request["id"], "result":result
                })
                .to_string();
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(headers.as_bytes())
                    .expect("owned response headers");
                stream.write_all(body.as_bytes()).expect("owned response");
            }
        });
        Self {
            endpoint,
            discoveries,
            calls,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for OwnedCrossRuntimeHttpFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("owned fixture worker");
        }
    }
}

fn read_owned_sync_http_request(stream: &mut std::net::TcpStream) -> Option<serde_json::Value> {
    use std::io::Read;
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 || bytes.len() + count > 1024 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).ok()?;
    let length = headers.split("\r\n").find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    })?;
    while bytes.len() - header_end < length {
        let mut chunk = [0u8; 4096];
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 || bytes.len() + count > 1024 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + length]).ok()
}

#[test]
fn issue73_http_enable_then_new_runtime_reconnects_and_calls_tool() {
    let fixture = OwnedCrossRuntimeHttpFixture::start();
    let (store, project_dir, data_dir, _) = setup_external("confirm");
    let service = McpServerSettingsService::new(
        data_dir.path().join("vega.db"),
        data_dir.path().join("config"),
    );
    let saved = service
        .create(McpServerForm {
            display_name: "runtime-boundary-http".into(),
            transport: McpServerTransport::Remote {
                endpoint: fixture.endpoint.clone(),
                allow_loopback_http: true,
                authorization: McpRemoteAuthorization::None,
            },
        })
        .expect("disabled remote MCP");
    let settings_runtime = owned_runtime();
    let enabled = settings_runtime
        .block_on(service.set_enabled(&saved.id, saved.config_revision, true, true))
        .expect("explicitly enabled remote MCP");
    drop(settings_runtime);

    let run_runtime = owned_runtime();
    let readiness = run_runtime
        .block_on(service.ready_for_run())
        .expect("ready in current run runtime");
    assert_eq!(readiness.ready_servers.len(), 1);
    assert!(readiness.unavailable.is_empty());
    assert_eq!(
        fixture.discoveries.load(Ordering::SeqCst),
        2,
        "Settings test and user run must not share an HTTP client across runtimes"
    );
    let old_run = readiness.ready_servers[0].clone();
    run_runtime.block_on(assert_two_owned_mcp_calls(
        &store,
        project_dir.path(),
        readiness.ready_servers,
        OwnedCallSpec {
            server_id: &saved.id,
            revision: enabled.config_revision,
            tool_name: "lookup",
            call_id: "cross-runtime-http-call",
            expected_output: "current-runtime-http",
        },
    ));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.discoveries.load(Ordering::SeqCst), 2);
    run_runtime
        .block_on(service.set_enabled(&saved.id, enabled.config_revision, false, false))
        .expect("explicit disable revokes the old run");
    assert!(old_run.is_revoked());
    let alias = McpCallIdentity {
        server_id: saved.id,
        config_revision: enabled.config_revision,
        exact_tool_name: "lookup".into(),
        arguments_bytes: 0,
        arguments_sha256: "0".repeat(64),
        argument_preview: String::new(),
    }
    .alias();
    let stale_provider = MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "stale-cross-runtime-call".into(),
            name: alias,
            input_json: r#"{"query":"must-not-run"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]]);
    let stale_tools = vega_tools::Tools::new(project_dir.path()).expect("owned project tools");
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Once,
    };
    let stale_result = run_runtime.block_on(run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &stale_provider,
        &stale_tools,
        "thread-1",
        "Try the revoked server",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
        vec![old_run],
    ));
    assert!(stale_result.map(|run| run.failed).unwrap_or(true));
    assert_eq!(hook.calls.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 2);
}
