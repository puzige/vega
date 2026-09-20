use super::*;
use crate::agent::mcp_registry::{
    McpCandidate, McpDispatchFailure, McpDispatchOutput, McpToolDispatcher, RunCapabilitySnapshot,
};

struct RecordingDispatcher {
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

impl McpToolDispatcher for RecordingDispatcher {
    fn call(
        &self,
        exact_tool_name: String,
        arguments: Value,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<McpDispatchOutput, McpDispatchFailure>> {
        let calls = self.calls.clone();
        async move {
            calls.lock().unwrap().push((exact_tool_name, arguments));
            Ok(McpDispatchOutput {
                text: "owned-tool-ok".to_string(),
                structured_content: None,
                is_error: false,
            })
        }
        .boxed()
    }
}

struct ExternalDecisionHook {
    decision: RuntimeUserDecision,
    prompts: Arc<Mutex<Vec<RuntimeMcpPermissionPrompt>>>,
}

impl RuntimePermissionHook for ExternalDecisionHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        async { Ok(RuntimeUserDecision::Timeout) }.boxed()
    }

    fn request_mcp(
        &self,
        prompt: RuntimeMcpPermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.prompts.lock().unwrap().push(prompt);
        let decision = self.decision.clone();
        async move { Ok(decision) }.boxed()
    }
}

struct NoopDispatcher;

impl McpToolDispatcher for NoopDispatcher {
    fn call(
        &self,
        _exact_tool_name: String,
        _arguments: Value,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<McpDispatchOutput, McpDispatchFailure>> {
        async {
            Ok(McpDispatchOutput {
                text: String::new(),
                structured_content: None,
                is_error: false,
            })
        }
        .boxed()
    }
}

struct RevokeOnApprovalHook {
    revoke: CancellationToken,
}

struct RotateCredentialOnApprovalHook {
    current: Arc<Mutex<String>>,
    replacement: String,
}

impl RuntimePermissionHook for RotateCredentialOnApprovalHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        async { Ok(RuntimeUserDecision::Timeout) }.boxed()
    }

    fn request_mcp(
        &self,
        _prompt: RuntimeMcpPermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        *self.current.lock().unwrap() = self.replacement.clone();
        async { Ok(RuntimeUserDecision::Once) }.boxed()
    }
}

impl RuntimePermissionHook for RevokeOnApprovalHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        async { Ok(RuntimeUserDecision::Timeout) }.boxed()
    }

    fn request_mcp(
        &self,
        _prompt: RuntimeMcpPermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.revoke.cancel();
        async { Ok(RuntimeUserDecision::Once) }.boxed()
    }
}

struct ReturnAfterCancelDispatcher {
    started: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl McpToolDispatcher for ReturnAfterCancelDispatcher {
    fn call(
        &self,
        _exact_tool_name: String,
        _arguments: Value,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<McpDispatchOutput, McpDispatchFailure>> {
        let started = self.started.clone();
        async move {
            if let Some(sender) = started.lock().unwrap().take() {
                let _ = sender.send(());
            }
            cancel.cancelled().await;
            Ok(McpDispatchOutput {
                text: "late-success-must-not-appear".into(),
                structured_content: None,
                is_error: false,
            })
        }
        .boxed()
    }
}

fn candidate(server_id: &str, tool_name: &str) -> McpCandidate {
    McpCandidate::new(
        server_id.to_string(),
        7,
        tool_name.to_string(),
        ToolDefinition {
            name: tool_name.to_string(),
            description: "Untrusted MCP search tool".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        Arc::new(NoopDispatcher),
        CancellationToken::new(),
    )
}

fn fake_mcp_provider(alias: String) -> MockProvider {
    MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "mcp-call-1".to_string(),
                name: alias,
                input_json: r#"{"query":"SENTINEL_PRIVATE"}"#.to_string(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ])
}

#[tokio::test]
async fn issue73_execute_mcp_always_asks_once_and_audits_only_digest() {
    let server_id = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    for mode in [
        RuntimePermissionMode::Confirm,
        RuntimePermissionMode::Auto,
        RuntimePermissionMode::FullAccess,
    ] {
        let project = tempdir().unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut registration = candidate(server_id, "search");
        registration.dispatcher = Arc::new(RecordingDispatcher {
            calls: calls.clone(),
        });
        let alias = RunCapabilitySnapshot::freeze(
            RuntimeRunMode::Execute,
            mode,
            vec![registration.clone()],
        )
        .unwrap()
        .definitions()
        .last()
        .unwrap()
        .name
        .clone();
        let provider = fake_mcp_provider(alias.clone());
        let mut req = request(vec![ChatMessage::new(
            ChatRole::User,
            "Search the owned fixture",
        )]);
        req.tool_config = tool_config(RuntimeRunMode::Execute, mode, project.path().to_path_buf())
            .with_mcp_candidates(vec![registration]);
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let hook = ExternalDecisionHook {
            decision: RuntimeUserDecision::Once,
            prompts: prompts.clone(),
        };
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert_eq!(calls.lock().unwrap().len(), 1);
        assert_eq!(calls.lock().unwrap()[0].0, "search");
        assert_eq!(calls.lock().unwrap()[0].1["query"], "SENTINEL_PRIVATE");
        let prompts = prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].server_id, server_id);
        assert_eq!(prompts[0].config_revision, 7);
        assert_eq!(prompts[0].exact_tool_name, "search");
        assert_eq!(prompts[0].arguments_bytes, 28);
        let proposed = outcome
            .events
            .iter()
            .find_map(|event| match event {
                RuntimeEvent::ToolCallProposed(call) => Some(call),
                _ => None,
            })
            .unwrap();
        assert_eq!(proposed.name, alias);
        assert!(!proposed.input_json.contains("SENTINEL_PRIVATE"));
        assert!(proposed.input_json.contains("arguments_sha256"));
        assert_eq!(provider.requests().len(), 2);
        assert!(provider.requests().iter().all(|request| {
            request
                .tools
                .iter()
                .any(|definition| definition.name == alias)
        }));
    }
}

#[tokio::test]
async fn issue73_mcp_always_denial_or_timeout_never_dispatches() {
    let server_id = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    for decision in [
        RuntimeUserDecision::Always,
        RuntimeUserDecision::Deny { note: None },
        RuntimeUserDecision::Timeout,
    ] {
        let project = tempdir().unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut registration = candidate(server_id, "search");
        registration.dispatcher = Arc::new(RecordingDispatcher {
            calls: calls.clone(),
        });
        let alias = RunCapabilitySnapshot::freeze(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Auto,
            vec![registration.clone()],
        )
        .unwrap()
        .definitions()
        .last()
        .unwrap()
        .name
        .clone();
        let provider = fake_mcp_provider(alias);
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Auto,
            project.path().to_path_buf(),
        )
        .with_mcp_candidates(vec![registration]);
        let hook = ExternalDecisionHook {
            decision,
            prompts: Arc::new(Mutex::new(Vec::new())),
        };
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert_eq!(calls.lock().unwrap().len(), 0);
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                status: RuntimeToolStatus::Rejected,
                ..
            })
        )));
    }
}

#[tokio::test]
async fn issue73_revoke_during_one_shot_approval_denies_old_frozen_run() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut registration = candidate("01K5KK7PZ5J8V2GSBMQKS8W71A", "search");
    registration.dispatcher = Arc::new(RecordingDispatcher {
        calls: calls.clone(),
    });
    let alias = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        vec![registration.clone()],
    )
    .unwrap()
    .definitions()
    .last()
    .unwrap()
    .name
    .clone();
    let mut req = request(Vec::new());
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        project.path().to_path_buf(),
    )
    .with_mcp_candidates(vec![registration.clone()]);
    let outcome = run_agent_with_permission_sink(
        &fake_mcp_provider(alias),
        &tools,
        req,
        CancellationToken::new(),
        &RevokeOnApprovalHook {
            revoke: registration.revoked,
        },
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    assert!(calls.lock().unwrap().is_empty());
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Rejected,
            ..
        })
    )));
}

#[tokio::test]
async fn issue73_revoke_in_flight_never_reports_racing_late_success() {
    let project = tempdir().unwrap();
    let path = project.path().to_path_buf();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let mut registration = candidate("01K5KK7PZ5J8V2GSBMQKS8W71A", "search");
    registration.dispatcher = Arc::new(ReturnAfterCancelDispatcher {
        started: Arc::new(Mutex::new(Some(started_tx))),
    });
    let alias = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        vec![registration.clone()],
    )
    .unwrap()
    .definitions()
    .last()
    .unwrap()
    .name
    .clone();
    let mut req = request(Vec::new());
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        path.clone(),
    )
    .with_mcp_candidates(vec![registration.clone()]);
    let run = tokio::spawn(async move {
        let tools = vega_tools::Tools::new(&path).unwrap();
        let hook = ExternalDecisionHook {
            decision: RuntimeUserDecision::Once,
            prompts: Arc::new(Mutex::new(Vec::new())),
        };
        run_agent_with_permission_sink(
            &fake_mcp_provider(alias),
            &tools,
            req,
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap()
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), started_rx)
        .await
        .unwrap()
        .unwrap();
    registration.revoked.cancel();
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(3), run)
        .await
        .unwrap()
        .unwrap();
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Cancelled,
            output,
            ..
        }) if output.contains("outcome unknown") && !output.contains("late-success")
    )));
}

#[test]
fn issue73_same_name_on_two_servers_has_stable_distinct_provider_aliases() {
    let first = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    let second = "01K5KK7PZ5J8V2GSBMQKS8W71B";
    let snapshot = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        vec![candidate(first, "search"), candidate(second, "search")],
    )
    .unwrap();
    let aliases = snapshot
        .definitions()
        .iter()
        .filter(|definition| definition.name.starts_with("mcp_"))
        .map(|definition| definition.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(aliases.len(), 2);
    assert_ne!(aliases[0], aliases[1]);
    assert!(aliases[0].contains(first));
    assert!(aliases[1].contains(second));
    for (alias, server) in aliases.iter().zip([first, second]) {
        let entry = snapshot.mcp_tool(alias).unwrap();
        assert_eq!(entry.server_id(), server);
        assert_eq!(entry.config_revision(), 7);
        assert_eq!(entry.exact_tool_name(), "search");
    }
    let again = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        vec![candidate(first, "search"), candidate(second, "search")],
    )
    .unwrap();
    assert_eq!(snapshot.definitions(), again.definitions());
}

#[test]
fn issue73_run_catalog_accepts_exactly_128_tools_and_rejects_129() {
    let first = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    let second = "01K5KK7PZ5J8V2GSBMQKS8W71B";
    let third = "01K5KK7PZ5J8V2GSBMQKS8W71C";
    let mut entries = Vec::new();
    for server in [first, second] {
        for index in 0..64 {
            entries.push(candidate(server, &format!("tool_{index}")));
        }
    }
    let snapshot = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        entries.clone(),
    )
    .unwrap();
    assert_eq!(snapshot.mcp_count(), 128);
    entries.push(candidate(third, "overflow"));
    assert!(
        RunCapabilitySnapshot::freeze(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            entries,
        )
        .is_err()
    );
}

#[tokio::test]
async fn issue73_mcp_dispatch_failures_are_typed_and_never_show_server_prose() {
    for (case, response, expected) in [
        (
            "protocol",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 3,
                "error": {"code": -32000, "message": "fake-server-private-prose-73"},
            }),
            "MCP protocol error",
        ),
        (
            "unsupported",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 3,
                "result": {
                    "resultType": "complete",
                    "content": [{"type": "image", "data": "fake-server-private-prose-73"}],
                    "isError": false,
                },
            }),
            "MCP unsupported result content",
        ),
    ] {
        let owned = tempdir().unwrap();
        let script = owned.path().join(format!("{case}.sh"));
        fs::write(
            &script,
            format!(
                r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{{"tools":{{}}}}}}}}'
      ;;
    *tools/list*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{{"name":"echo","inputSchema":{{"type":"object"}}}}]}}}}'
      ;;
    *tools/call*)
      printf '%s\n' '{response}'
      ;;
  esac
done
"##,
                response = response,
            ),
        )
        .unwrap();
        let ready = McpReadyServer::connect_local(
            "01K5KK7PZ5J8V2GSBMQKS8W71A".into(),
            1,
            vega_mcp::LocalServer {
                executable: "/bin/sh".into(),
                args: vec![script.to_string_lossy().into_owned()],
                working_directory: owned.path().into(),
                environment: Vec::new(),
            },
        )
        .await
        .unwrap();
        let alias = RunCapabilitySnapshot::freeze(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            ready.candidates(),
        )
        .unwrap()
        .definitions()
        .last()
        .unwrap()
        .name
        .clone();
        let provider = fake_mcp_provider(alias);
        let tools = vega_tools::Tools::new(owned.path()).unwrap();
        let mut req = request(vec![ChatMessage::new(ChatRole::User, "Call owned MCP")]);
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            owned.path().into(),
        )
        .with_mcp_servers(vec![ready]);
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            CancellationToken::new(),
            &ExternalDecisionHook {
                decision: RuntimeUserDecision::Once,
                prompts: Arc::new(Mutex::new(Vec::new())),
            },
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        let output = outcome
            .events
            .iter()
            .find_map(|event| match event {
                RuntimeEvent::ToolCallFinished(result) => {
                    assert_eq!(result.status, RuntimeToolStatus::Failed);
                    Some(result.output.as_str())
                }
                _ => None,
            })
            .unwrap();
        assert!(output.contains(expected), "{case}: {output}");
        assert!(!output.contains("fake-server-private-prose-73"));
        assert!(provider.requests().iter().all(|request| {
            request
                .messages
                .iter()
                .all(|message| !message.content.contains("fake-server-private-prose-73"))
        }));
    }
}

#[tokio::test]
async fn issue73_catalog_reads_owner_credentials_once_for_multiple_tools() {
    let owned = tempdir().unwrap();
    let script = owned.path().join("two-tools.sh");
    fs::write(
        &script,
        r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}}'
      ;;
    *tools/list*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{"name":"alpha","inputSchema":{"type":"object","properties":{"query":{"type":"string"}}}},{"name":"beta","inputSchema":{"type":"object","properties":{"query":{"type":"string"}}}}]}}'
      ;;
  esac
done
"##,
    )
    .unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = reads.clone();
    let ready = McpReadyServer::connect_local(
        "01K5KK7PZ5J8V2GSBMQKS8W71A".into(),
        1,
        vega_mcp::LocalServer {
            executable: "/bin/sh".into(),
            args: vec![script.to_string_lossy().to_string()],
            working_directory: owned.path().to_path_buf(),
            environment: Vec::new(),
        },
    )
    .await
    .unwrap()
    .with_known_credentials_reader(Arc::new(move || {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(vec!["fake-provider-key-never-in-catalog-73".into()])
    }));
    let catalog = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        ready.candidates(),
    )
    .unwrap();
    assert_eq!(catalog.mcp_count(), 2);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[test]
fn issue73_mcp_is_not_advertised_in_ask_plan_or_execute_readonly() {
    let id = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    for (mode, permission) in [
        (RuntimeRunMode::Ask, RuntimePermissionMode::Confirm),
        (RuntimeRunMode::Plan, RuntimePermissionMode::Confirm),
        (RuntimeRunMode::Execute, RuntimePermissionMode::ReadOnly),
    ] {
        let snapshot =
            RunCapabilitySnapshot::freeze(mode, permission, vec![candidate(id, "search")]).unwrap();
        assert!(
            snapshot
                .definitions()
                .iter()
                .all(|item| !item.name.starts_with("mcp_"))
        );
        assert_eq!(snapshot.mcp_count(), 0);
    }
}

#[test]
fn issue73_run_snapshot_does_not_gain_a_refreshed_schema_or_tool() {
    let id = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    let mut current = vec![candidate(id, "search")];
    let snapshot = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Auto,
        current.clone(),
    )
    .unwrap();
    current[0].definition.input_schema = serde_json::json!({
        "type": "object",
        "properties": { "changed": { "type": "boolean" } }
    });
    current.push(candidate(id, "new_tool"));
    assert_eq!(snapshot.mcp_count(), 1);
    assert_eq!(
        snapshot.definitions()[6].input_schema["properties"]["query"]["type"],
        "string"
    );
    let next = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Auto,
        current,
    )
    .unwrap();
    assert_eq!(next.mcp_count(), 2);
}

#[test]
fn issue73_registry_rejects_duplicate_identity_and_non_object_schema() {
    let id = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    let duplicate = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        vec![candidate(id, "search"), candidate(id, "search")],
    );
    assert!(duplicate.is_err());
    let mut invalid = candidate(id, "bad");
    invalid.definition.input_schema = serde_json::json!({"type": "string"});
    assert!(
        RunCapabilitySnapshot::freeze(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            vec![invalid],
        )
        .is_err()
    );
}

#[tokio::test]
async fn issue73_mock_provider_calls_one_real_owned_stdio_mcp_server() {
    let owned = tempdir().unwrap();
    let script = owned.path().join("owned-mcp.sh");
    let calls_log = owned.path().join("calls.log");
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
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"resultType":"complete","content":[{"type":"text","text":"server-says-ok"}],"isError":false}}'
      ;;
  esac
done
"##,
    )
    .unwrap();
    let server_id = "01K5KK7PZ5J8V2GSBMQKS8W71A".to_string();
    let ready = McpReadyServer::connect_local(
        server_id.clone(),
        1,
        vega_mcp::LocalServer {
            executable: "/bin/sh".into(),
            args: vec![
                script.to_string_lossy().to_string(),
                calls_log.to_string_lossy().to_string(),
            ],
            working_directory: owned.path().to_path_buf(),
            environment: Vec::new(),
        },
    )
    .await
    .unwrap();
    let catalog = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        ready.candidates(),
    )
    .unwrap();
    let alias = catalog.definitions().last().unwrap().name.clone();
    let provider = fake_mcp_provider(alias);
    let tools = vega_tools::Tools::new(owned.path()).unwrap();
    let mut req = request(vec![ChatMessage::new(
        ChatRole::User,
        "Use the owned echo fixture",
    )]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        owned.path().to_path_buf(),
    )
    .with_mcp_servers(vec![ready]);
    let hook = ExternalDecisionHook {
        decision: RuntimeUserDecision::Once,
        prompts: Arc::new(Mutex::new(Vec::new())),
    };
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        req,
        CancellationToken::new(),
        &hook,
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    let server_calls = fs::read_to_string(calls_log).unwrap();
    assert_eq!(server_calls.lines().count(), 1);
    assert!(server_calls.contains("SENTINEL_PRIVATE"));
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Success,
            output,
            ..
        }) if output.contains("server-says-ok")
    )));
    assert!(provider.requests()[1].messages.iter().any(|message| {
        message.role == ChatRole::Tool && message.content.contains("server-says-ok")
    }));
}

#[tokio::test]
async fn issue73_mcp_catalog_echoing_owner_secret_is_never_advertised() {
    const SECRET: &str = "fake-owner-only-catalog-credential-73";
    let owned = tempdir().unwrap();
    let script = owned.path().join("catalog-echo.sh");
    fs::write(
        &script,
        format!(
            r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{{"tools":{{}}}}}}}}'
      ;;
    *tools/list*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{{"name":"echo","description":"{SECRET}","inputSchema":{{"type":"object","properties":{{"query":{{"type":"string"}}}}}}}}]}}}}'
      ;;
  esac
done
"##
        ),
    )
    .unwrap();
    let ready = McpReadyServer::connect_local(
        "01K5KK7PZ5J8V2GSBMQKS8W71A".into(),
        1,
        vega_mcp::LocalServer {
            executable: "/bin/sh".into(),
            args: vec![script.to_string_lossy().to_string()],
            working_directory: owned.path().to_path_buf(),
            environment: vec![("MCP_SECRET".into(), SECRET.into())],
        },
    )
    .await
    .unwrap();
    let tools = vega_tools::Tools::new(owned.path()).unwrap();
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "Use MCP")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        owned.path().to_path_buf(),
    )
    .with_mcp_servers(vec![ready]);
    let provider =
        MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]]);
    let result = run_agent_with_permission_sink(
        &provider,
        &tools,
        req,
        CancellationToken::new(),
        &ExternalDecisionHook {
            decision: RuntimeUserDecision::Once,
            prompts: Arc::new(Mutex::new(Vec::new())),
        },
        |_| async { Ok(()) },
    )
    .await;
    assert!(result.is_err(), "secret-bearing catalog must fail closed");
    assert!(
        provider.requests().is_empty(),
        "no provider projection is allowed"
    );
}

#[tokio::test]
async fn issue73_mcp_schema_secret_with_json_escapes_is_never_advertised() {
    const SECRET: &str = "fake-\"quoted\\schema-73";
    let owned = tempdir().unwrap();
    let script = owned.path().join("schema-echo.sh");
    let catalog_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "result": {
            "resultType": "complete",
            "ttlMs": 0,
            "cacheScope": "private",
            "tools": [{
                "name": "echo",
                "description": "benign",
                "inputSchema": {
                    "type": "object",
                    "properties": {"query": {"type": "string", "default": SECRET}},
                },
            }],
        },
    })
    .to_string();
    fs::write(
        &script,
        format!(
            r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{{"tools":{{}}}}}}}}'
      ;;
    *tools/list*)
      printf '%s\n' '{catalog_response}'
      ;;
  esac
done
"##
        ),
    )
    .unwrap();
    let ready = McpReadyServer::connect_local(
        "01K5KK7PZ5J8V2GSBMQKS8W71A".into(),
        1,
        vega_mcp::LocalServer {
            executable: "/bin/sh".into(),
            args: vec![script.to_string_lossy().to_string()],
            working_directory: owned.path().to_path_buf(),
            environment: vec![("MCP_SECRET".into(), SECRET.into())],
        },
    )
    .await
    .unwrap();
    let tools = vega_tools::Tools::new(owned.path()).unwrap();
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "Use MCP")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        owned.path().to_path_buf(),
    )
    .with_mcp_servers(vec![ready]);
    let provider =
        MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]]);
    let result = run_agent_with_permission_sink(
        &provider,
        &tools,
        req,
        CancellationToken::new(),
        &ExternalDecisionHook {
            decision: RuntimeUserDecision::Once,
            prompts: Arc::new(Mutex::new(Vec::new())),
        },
        |_| async { Ok(()) },
    )
    .await;
    assert!(result.is_err());
    assert!(provider.requests().is_empty());
}

#[tokio::test]
async fn issue73_mcp_success_result_echoing_owner_secret_is_never_published() {
    const SECRET: &str = "fake-owner-only-result-credential-73";
    let owned = tempdir().unwrap();
    let script = owned.path().join("result-echo.sh");
    fs::write(
        &script,
        format!(
            r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{{"tools":{{}}}}}}}}'
      ;;
    *tools/list*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{{"name":"echo","description":"Owned fixture","inputSchema":{{"type":"object","properties":{{"query":{{"type":"string"}}}}}}}}]}}}}'
      ;;
    *tools/call*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"resultType":"complete","content":[{{"type":"text","text":"{SECRET}"}}],"isError":false}}}}'
      ;;
  esac
done
"##
        ),
    )
    .unwrap();
    let ready = McpReadyServer::connect_local(
        "01K5KK7PZ5J8V2GSBMQKS8W71A".into(),
        1,
        vega_mcp::LocalServer {
            executable: "/bin/sh".into(),
            args: vec![script.to_string_lossy().to_string()],
            working_directory: owned.path().to_path_buf(),
            environment: vec![("MCP_SECRET".into(), SECRET.into())],
        },
    )
    .await
    .unwrap();
    let alias = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        ready.candidates(),
    )
    .unwrap()
    .definitions()
    .last()
    .unwrap()
    .name
    .clone();
    let provider = fake_mcp_provider(alias);
    let tools = vega_tools::Tools::new(owned.path()).unwrap();
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "Use MCP")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        owned.path().to_path_buf(),
    )
    .with_mcp_servers(vec![ready]);
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        req,
        CancellationToken::new(),
        &ExternalDecisionHook {
            decision: RuntimeUserDecision::Once,
            prompts: Arc::new(Mutex::new(Vec::new())),
        },
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Failed,
            ..
        })
    )));
    for event in &outcome.events {
        if let RuntimeEvent::ToolCallFinished(result) = event {
            assert!(!result.output.contains(SECRET));
        }
    }
    assert!(
        outcome
            .messages
            .iter()
            .all(|message| !message.content.contains(SECRET))
    );
    assert!(provider.requests().iter().all(|request| {
        request
            .messages
            .iter()
            .all(|message| !message.content.contains(SECRET))
    }));
}

#[tokio::test]
async fn issue73_mcp_result_rechecks_owner_secret_after_concurrent_rotation() {
    const OLD: &str = "fake-oauth-access-before-refresh-73";
    const NEW: &str = "fake-oauth-access-after-refresh-73";
    let owned = tempdir().unwrap();
    let script = owned.path().join("rotated-result.sh");
    fs::write(
        &script,
        format!(
            r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{{"tools":{{}}}}}}}}'
      ;;
    *tools/list*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{{"name":"echo","inputSchema":{{"type":"object"}}}}]}}}}'
      ;;
    *tools/call*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"resultType":"complete","content":[{{"type":"text","text":"{NEW}"}}],"isError":false}}}}'
      ;;
  esac
done
"##
        ),
    )
    .unwrap();
    let current = Arc::new(Mutex::new(OLD.to_string()));
    let current_for_reader = current.clone();
    let ready = McpReadyServer::connect_local(
        "01K5KK7PZ5J8V2GSBMQKS8W71A".into(),
        1,
        vega_mcp::LocalServer {
            executable: "/bin/sh".into(),
            args: vec![script.to_string_lossy().to_string()],
            working_directory: owned.path().to_path_buf(),
            environment: Vec::new(),
        },
    )
    .await
    .unwrap()
    .with_known_credentials(vec![OLD.into()])
    .with_known_credentials_reader(Arc::new(move || {
        Ok(vec![current_for_reader.lock().map_err(|_| ())?.clone()])
    }));
    let alias = RunCapabilitySnapshot::freeze(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        ready.candidates(),
    )
    .unwrap()
    .definitions()
    .last()
    .unwrap()
    .name
    .clone();
    let provider = fake_mcp_provider(alias);
    let tools = vega_tools::Tools::new(owned.path()).unwrap();
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "Use MCP")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        owned.path().to_path_buf(),
    )
    .with_mcp_servers(vec![ready]);
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        req,
        CancellationToken::new(),
        &RotateCredentialOnApprovalHook {
            current,
            replacement: NEW.into(),
        },
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            status: RuntimeToolStatus::Failed,
            output,
            ..
        }) if !output.contains(NEW)
    )));
    assert!(provider.requests().iter().all(|request| {
        request
            .messages
            .iter()
            .all(|message| !message.content.contains(NEW))
    }));
}

#[tokio::test]
async fn issue73_mcp_structured_secret_with_json_escapes_is_rejected_for_both_error_flags() {
    const SECRET: &str = "fake-\"quoted\\credential-73";
    for is_error in [false, true] {
        let owned = tempdir().unwrap();
        let script = owned.path().join("structured-echo.sh");
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": {
                "resultType": "complete",
                "content": [{"type": "text", "text": "safe-prefix"}],
                "structuredContent": {"nested": {"credential": SECRET}},
                "isError": is_error,
            },
        })
        .to_string();
        fs::write(
            &script,
            format!(
                r##"#!/bin/sh
while IFS= read -r request; do
  case "$request" in
    *server/discover*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","supportedVersions":["2026-07-28"],"capabilities":{{"tools":{{}}}}}}}}'
      ;;
    *tools/list*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{{"name":"echo","inputSchema":{{"type":"object"}}}}]}}}}'
      ;;
    *tools/call*)
      printf '%s\n' '{response}'
      ;;
  esac
done
"##
            ),
        )
        .unwrap();
        let ready = McpReadyServer::connect_local(
            "01K5KK7PZ5J8V2GSBMQKS8W71A".into(),
            1,
            vega_mcp::LocalServer {
                executable: "/bin/sh".into(),
                args: vec![script.to_string_lossy().to_string()],
                working_directory: owned.path().to_path_buf(),
                environment: vec![("MCP_SECRET".into(), SECRET.into())],
            },
        )
        .await
        .unwrap();
        let alias = RunCapabilitySnapshot::freeze(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            ready.candidates(),
        )
        .unwrap()
        .definitions()
        .last()
        .unwrap()
        .name
        .clone();
        let provider = fake_mcp_provider(alias);
        let tools = vega_tools::Tools::new(owned.path()).unwrap();
        let mut req = request(vec![ChatMessage::new(ChatRole::User, "Use MCP")]);
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            owned.path().to_path_buf(),
        )
        .with_mcp_servers(vec![ready]);
        let outcome = run_agent_with_permission_sink(
            &provider,
            &tools,
            req,
            CancellationToken::new(),
            &ExternalDecisionHook {
                decision: RuntimeUserDecision::Once,
                prompts: Arc::new(Mutex::new(Vec::new())),
            },
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                status: RuntimeToolStatus::Failed,
                output,
                ..
            }) if !output.contains("fake-")
        )));
        assert!(provider.requests().iter().all(|request| {
            request
                .messages
                .iter()
                .all(|message| !message.content.contains("fake-"))
        }));
    }
}
