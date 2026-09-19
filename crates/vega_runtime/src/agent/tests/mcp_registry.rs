use super::*;
use crate::agent::mcp_registry::{
    McpCandidate, McpDispatchOutput, McpToolDispatcher, RunCapabilitySnapshot,
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
    ) -> BoxFuture<'static, Result<McpDispatchOutput, VegaError>> {
        let calls = self.calls.clone();
        async move {
            calls.lock().unwrap().push((exact_tool_name, arguments));
            Ok(McpDispatchOutput {
                text: "owned-tool-ok".to_string(),
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
    ) -> BoxFuture<'static, Result<McpDispatchOutput, VegaError>> {
        async {
            Ok(McpDispatchOutput {
                text: String::new(),
                is_error: false,
            })
        }
        .boxed()
    }
}

struct RevokeOnApprovalHook {
    revoke: CancellationToken,
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
    ) -> BoxFuture<'static, Result<McpDispatchOutput, VegaError>> {
        let started = self.started.clone();
        async move {
            if let Some(sender) = started.lock().unwrap().take() {
                let _ = sender.send(());
            }
            cancel.cancelled().await;
            Ok(McpDispatchOutput {
                text: "late-success-must-not-appear".into(),
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
async fn issue73_mcp_always_or_denial_never_dispatches() {
    let server_id = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    for decision in [
        RuntimeUserDecision::Always,
        RuntimeUserDecision::Deny { note: None },
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
