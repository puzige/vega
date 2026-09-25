use super::*;
use crate::McpServerSettingsService;
use crate::types::{
    ApprovalAudit, ApprovalSource, McpCallIdentity, McpRemoteAuthorization, McpServerForm,
    McpServerTransport, ToolCall, ToolCallStatus, ToolCardInputProjection,
    ToolCardResultProjection,
};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
            None,
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
    assert_eq!(
        messages::finish_streaming(store.conn(), "assistant-mcp", "", "done", None).unwrap(),
        1
    );
    let history = crate::history::latest_history_page(&store, "thread-1", 32).unwrap();
    assert!(
        history.entries.iter().any(|entry| matches!(
            entry,
            crate::history::HistoryEntry::Tool {
                call_id,
                input: Some(ToolCardInputProjection::Mcp { .. }),
                result: Some(ToolCardResultProjection::Mcp { status: ToolCallStatus::Success, .. }),
                ..
            } if call_id == &call.id
        )),
        "restart history keeps the safe MCP result card, not a corrupt placeholder"
    );
}

#[tokio::test]
async fn issue73_rotated_provider_secret_in_historical_mcp_result_is_blocked_after_restart() {
    const SECRET: &str = "fake-rotated-provider-secret-in-old-mcp-result-73";
    let (store, project, data, project_id) = setup_external("confirm");
    let database_path = data.path().join("vega.db");
    let config_root = data.path().join("config");
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "user-before-rotation".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "user".into(),
            kind: "text".into(),
            content: "Find a harmless value".into(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "assistant-before-rotation".into(),
            thread_id: "thread-1".into(),
            seq: 2,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 2,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let (call, _) = proposal();
    let audit = RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Once,
        note: None,
        source: RuntimeApprovalSource::User,
        danger: None,
    };
    let mut sequence = 1;
    for event in [
        RuntimeEvent::ToolCallProposed(call.clone()),
        RuntimeEvent::ToolCallApproved {
            call_id: call.id.clone(),
            audit: audit.clone(),
            remember_rule: None,
        },
        RuntimeEvent::ToolCallRunning {
            call_id: call.id.clone(),
        },
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            call_id: call.id.clone(),
            output: format!("[Untrusted external MCP result]\n{SECRET}"),
            status: RuntimeToolStatus::Success,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: Some(false),
            approval: Some(audit),
            remember_rule: None,
        }),
    ] {
        persist_runtime_event(
            &store,
            &project_id,
            "thread-1",
            "assistant-before-rotation",
            "mock-model",
            false,
            "",
            &mut sequence,
            &event,
            None,
        )
        .unwrap();
    }
    messages::finish_streaming(store.conn(), "assistant-before-rotation", "", "done", None)
        .unwrap();
    drop(store);

    vega_store::keystore::set_key(&config_root, "provider-rotated", SECRET).unwrap();
    let reopened = Store::open(&database_path).unwrap();
    reopened.migrate().unwrap();
    let service = McpServerSettingsService::new(database_path, config_root.clone());
    assert!(
        service.list().unwrap().is_empty(),
        "no MCP server is enabled or configured"
    );
    let owner = service.clone();
    let inner = Arc::new(MockProvider::new(vec![ScriptStep::text(
        "unrelated history still works",
    )]));
    let guarded = OwnerCredentialProvider::new(
        inner.clone(),
        Arc::new(move || owner.current_owner_credential_values().map_err(|_| ())),
    );
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Once,
    };
    let attempt = run_thread_task_with_images_reasoning_and_mcp(
        &reopened,
        &guarded,
        &tools,
        "thread-1",
        "Continue without the old secret",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    )
    .await;
    assert!(attempt.is_err() || attempt.is_ok_and(|run| run.failed));
    assert!(
        inner.requests().is_empty(),
        "no provider request may contain the rotated key"
    );
    assert_eq!(
        tool_calls::find_state(reopened.conn(), &call.id)
            .unwrap()
            .unwrap()
            .output_text
            .as_deref(),
        Some(format!("[Untrusted external MCP result]\n{SECRET}").as_str()),
        "the durable audit remains unchanged"
    );

    vega_store::keystore::set_key(&config_root, "provider-rotated", "different-fake-key-73")
        .unwrap();
    let resumed = run_thread_task_with_images_reasoning_and_mcp(
        &reopened,
        &guarded,
        &tools,
        "thread-1",
        "Continue with unrelated history",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!resumed.failed);
    assert_eq!(inner.requests().len(), 1);
}

#[test]
fn issue73_unapproved_external_call_replays_as_rejected_mcp_card() {
    let (store, _dir, project_id) = setup();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "assistant-mcp-denied".into(),
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
            "assistant-mcp-denied",
            "mock-model",
            false,
            "",
            sequence,
            &event,
            None,
        )
    };
    persist(RuntimeEvent::ToolCallProposed(call.clone()), &mut sequence).unwrap();
    persist(
        RuntimeEvent::ToolCallFinished(RuntimeToolResult {
            call_id: call.id.clone(),
            output: "Tool error: MCP call not approved".into(),
            status: RuntimeToolStatus::Rejected,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            approval: Some(RuntimeApprovalAudit {
                decision: RuntimeApprovalDecision::Deny,
                note: None,
                source: RuntimeApprovalSource::Timeout,
                danger: None,
            }),
            remember_rule: None,
        }),
        &mut sequence,
    )
    .unwrap();
    assert_eq!(
        messages::finish_streaming(store.conn(), "assistant-mcp-denied", "", "done", None).unwrap(),
        1
    );
    let history = crate::history::latest_history_page(&store, "thread-1", 32).unwrap();
    assert!(
        history.entries.iter().any(|entry| matches!(
            entry,
            crate::history::HistoryEntry::Tool {
                call_id,
                input: Some(ToolCardInputProjection::Mcp { .. }),
                result: Some(ToolCardResultProjection::Mcp { status: ToolCallStatus::Rejected, .. }),
                ..
            } if call_id == &call.id
        )),
        "a timed-out approval must replay as rejected MCP, never as a corrupt result"
    );
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
            None,
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

#[test]
fn issue73_typed_mcp_failures_validate_and_restart_without_accepting_server_prose() {
    let (call, _) = proposal();
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::User,
        danger: None,
    };
    for output in [
        "Tool error: MCP protocol error",
        "Tool error: MCP invalid protocol response",
        "Tool error: MCP unsupported transport",
        "Tool error: MCP unsupported result content",
        "Tool error: MCP response limit exceeded",
        "Tool error: MCP request timed out; side-effect outcome unknown",
        "Tool error: MCP transport failed; side-effect outcome unknown",
        "Tool error: MCP authorization required or invalid",
        "Tool error: MCP connection configuration invalid",
    ] {
        assert!(
            validate_recovered_projection(
                "project-1",
                "thread-1",
                &call.id,
                &call.name,
                &call.input_json,
                output,
                RuntimeToolStatus::Failed,
                &approval,
                None,
                None,
            )
            .is_ok(),
            "safe typed failure must validate: {output}"
        );
    }
    assert!(
        validate_recovered_projection(
            "project-1",
            "thread-1",
            &call.id,
            &call.name,
            &call.input_json,
            "Tool error: MCP fake-server-private-prose-73",
            RuntimeToolStatus::Failed,
            &approval,
            None,
            None,
        )
        .is_err()
    );

    let (store, directory, project_id) = setup();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "assistant-typed-failure".into(),
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
            output: "Tool error: MCP unsupported result content".into(),
            status: RuntimeToolStatus::Failed,
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
            "assistant-typed-failure",
            "mock-model",
            false,
            "",
            &mut sequence,
            &event,
            None,
        )
        .unwrap();
    }
    drop(store);
    let reopened = Store::open(directory.path().join("vega.db")).unwrap();
    reopened.migrate().unwrap();
    let card = tool_calls::find_state(reopened.conn(), &call.id)
        .unwrap()
        .unwrap();
    assert_eq!(card.status, "failed");
    assert_eq!(
        card.output_text.as_deref(),
        Some("Tool error: MCP unsupported result content")
    );
}

#[tokio::test]
async fn issue73_owned_stdio_reaches_durable_conversation_and_second_provider_round() {
    let (store, project_dir, data_dir, _) = setup_external("confirm");
    let script = data_dir.path().join("owned-mcp.sh");
    let calls_log = data_dir.path().join("calls.log");
    let _fixture = vega_mcp::mock::StdioFixture::new(
        &script,
        Arc::new(|server, stream| {
            Box::pin(vega_mcp::mock::serve_json(stream, move |request| {
                if request["method"] == "tools/call" {
                    use std::io::Write;
                    writeln!(
                        std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&server.args[1])
                            .unwrap(),
                        "{request}"
                    )
                    .unwrap();
                }
                std::future::ready(vega_mcp::mock::catalog_reply(
                    &request,
                    &serde_json::json!([{"name":"echo","description":"Owned echo fixture","inputSchema":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}]),
                    "owned-answer",
                ))
            }))
        }),
    );
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
    let mut live_events = Vec::new();
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Use the owned echo server",
        "System",
        CancellationToken::new(),
        &hook,
        |event| {
            live_events.push(event.clone());
            Ok(())
        },
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
    let live_proposal = live_events.iter().find_map(|event| match event {
        ConversationEvent::ToolCallProposed { call } => Some(call),
        _ => None,
    });
    let live_proposal = live_proposal.expect("owned local MCP proposal reaches the UI event sink");
    assert_eq!(live_proposal.tool, alias);
    assert!(!live_proposal.input_json.contains("SENTINEL_PRIVATE"));
    assert!(matches!(
        crate::types::tool_card_input_projection(live_proposal),
        ToolCardInputProjection::Mcp { .. }
    ));
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
async fn issue73_owner_secret_echo_never_reaches_live_events_provider_or_restart_history() {
    const SECRET: &str = "fake-durable-owner-credential-73";
    let (store, project_dir, data_dir, _) = setup_external("confirm");
    let script = data_dir.path().join("secret-echo-mcp.sh");
    let _fixture = vega_mcp::mock::catalog_stdio(
        &script,
        serde_json::json!([{"name":"echo","description":"Owned fixture","inputSchema":{"type":"object"}}]),
        SECRET.into(),
        Arc::new(|_| {}),
    );
    let server_id = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    let ready = McpReadyServer::connect_local(
        server_id.into(),
        1,
        vega_mcp::LocalServer {
            executable: "/bin/sh".into(),
            args: vec![script.to_string_lossy().into_owned()],
            working_directory: data_dir.path().to_path_buf(),
            environment: vec![("MCP_SECRET".into(), SECRET.into())],
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
                id: "secret-echo-call".into(),
                name: alias.clone(),
                input_json: "{}".into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Secret refused.".into()),
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
    let mut live_events = Vec::new();
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Call owned MCP",
        "System",
        CancellationToken::new(),
        &hook,
        |event| {
            live_events.push(event.clone());
            Ok(())
        },
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
    assert!(live_events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.status == ToolCallStatus::Failed && !result.output.contains(SECRET)
    )));
    assert!(live_events.iter().all(|event| match event {
        ConversationEvent::ToolCallOutput { chunk, .. } => !chunk.0.contains(SECRET),
        ConversationEvent::ToolCallFinished { result, .. } => !result.output.contains(SECRET),
        _ => true,
    }));
    let row = tool_calls::find_state(store.conn(), "secret-echo-call")
        .unwrap()
        .unwrap();
    assert_eq!(row.status, "failed");
    assert!(!row.output_text.unwrap_or_default().contains(SECRET));
    assert!(provider.requests().iter().all(|request| {
        request
            .messages
            .iter()
            .all(|message| !message.content.contains(SECRET))
    }));
    let history = crate::history::latest_history_page(&store, "thread-1", 32).unwrap();
    assert!(history.entries.iter().any(|entry| matches!(
        entry,
        crate::history::HistoryEntry::Tool {
            call_id,
            result: Some(ToolCardResultProjection::Mcp {
                status: ToolCallStatus::Failed,
                ..
            }),
            ..
        } if call_id == "secret-echo-call"
    )));
}

#[tokio::test]
async fn issue73_owned_remote_http_reaches_durable_conversation_and_second_provider_round() {
    let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
    let endpoint = remote_fixture(
        serde_json::json!([{"name":"lookup","inputSchema":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}]),
        "remote-answer",
        Arc::new(move |request| {
            if request["method"] == "tools/call" {
                sent.send(request["params"]["arguments"].clone()).unwrap();
            }
        }),
    );
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
    let mut live_events = Vec::new();
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Use the owned remote lookup server",
        "System",
        CancellationToken::new(),
        &hook,
        |event| {
            live_events.push(event.clone());
            Ok(())
        },
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
    let live_proposal = live_events.iter().find_map(|event| match event {
        ConversationEvent::ToolCallProposed { call } => Some(call),
        _ => None,
    });
    let live_proposal = live_proposal.expect("owned HTTP MCP proposal reaches the UI event sink");
    assert_eq!(live_proposal.tool, alias);
    assert!(!live_proposal.input_json.contains("PRIVATE_REMOTE_ARGUMENT"));
    assert!(matches!(
        crate::types::tool_card_input_projection(live_proposal),
        ToolCardInputProjection::Mcp { .. }
    ));
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
}

fn owned_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("owned UI/run runtime")
}

#[tokio::test]
async fn issue73_production_entry_exposes_same_name_server_provenance_to_model() {
    let (store, project_dir, data_dir, _) = setup_external("confirm");
    let script = data_dir.path().join("same-name-provenance-mcp.sh");
    let _fixture = vega_mcp::mock::catalog_stdio(
        &script,
        serde_json::json!([{"name":"echo","description":"Same-name owned fixture","inputSchema":{"type":"object"}}]),
        "same-name-answer".into(),
        Arc::new(|_| {}),
    );
    let local_id = "01K5KK7PZ5J8V2GSBMQKS8W71A";
    let remote_id = "01K5KK7PZ5J8V2GSBMQKS8W71B";
    let script_path = script.to_string_lossy().into_owned();
    let working_directory = data_dir.path().to_path_buf();
    let make_ready = |server_id: &'static str| {
        let script_path = script_path.clone();
        let working_directory = working_directory.clone();
        async move {
            McpReadyServer::connect_local(
                server_id.to_owned(),
                1,
                vega_mcp::LocalServer {
                    executable: "/bin/sh".into(),
                    args: vec![script_path],
                    working_directory,
                    environment: Vec::new(),
                },
            )
            .await
            .expect("same-name MCP fixture connection")
        }
    };
    let local = make_ready(local_id).await;
    let endpoint = remote_fixture(
        serde_json::json!([{"name":"echo","description":"Same-name owned fixture","inputSchema":{"type":"object"}}]),
        "",
        Arc::new(|_| {}),
    );
    let remote_client = vega_mcp::HttpClient::connect(&endpoint, true)
        .await
        .expect("same-name remote MCP fixture connection");
    let remote = McpReadyServer::connect_http(remote_id.to_owned(), 1, remote_client)
        .await
        .expect("same-name remote MCP discovery");

    let provider =
        MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]]);
    let tools = vega_tools::Tools::new(project_dir.path()).expect("owned project tools");
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Once,
    };
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Use the local same-name echo tool",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
        vec![local, remote],
    )
    .await
    .expect("production conversation entry");
    assert!(!run.failed);

    let requests = provider.requests();
    let definitions = requests[0]
        .tools
        .iter()
        .filter(|definition| definition.name.starts_with("mcp_"))
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 2);
    let local_alias = McpCallIdentity {
        server_id: local_id.into(),
        config_revision: 1,
        exact_tool_name: "echo".into(),
        arguments_bytes: 0,
        arguments_sha256: "0".repeat(64),
        argument_preview: String::new(),
    }
    .alias();
    let remote_alias = McpCallIdentity {
        server_id: remote_id.into(),
        config_revision: 1,
        exact_tool_name: "echo".into(),
        arguments_bytes: 0,
        arguments_sha256: "0".repeat(64),
        argument_preview: String::new(),
    }
    .alias();
    assert_ne!(local_alias, remote_alias);
    let local_definition = definitions
        .iter()
        .find(|definition| definition.name == local_alias)
        .expect("local same-name alias");
    let remote_definition = definitions
        .iter()
        .find(|definition| definition.name == remote_alias)
        .expect("remote same-name alias");
    assert!(local_definition.description.contains(local_id));
    assert!(
        local_definition
            .description
            .contains("transport: stdio (local process)")
    );
    assert!(local_definition.description.contains("exact tool: echo"));
    assert!(remote_definition.description.contains(remote_id));
    assert!(
        remote_definition
            .description
            .contains("transport: Streamable HTTP (remote endpoint)")
    );
    assert!(remote_definition.description.contains("exact tool: echo"));
}

#[tokio::test]
async fn issue73_production_entry_exposes_same_transport_server_labels_to_model() {
    let (store, project_dir, data_dir, _) = setup_external("confirm");
    let script = data_dir.path().join("same-transport-label-mcp.sh");
    let _fixture = vega_mcp::mock::catalog_stdio(
        &script,
        serde_json::json!([{"name":"echo","description":"Same-name owned fixture","inputSchema":{"type":"object"}}]),
        String::new(),
        Arc::new(|_| {}),
    );
    let script_path = script.to_string_lossy().into_owned();
    let working_directory = data_dir.path().to_path_buf();
    let service = McpServerSettingsService::new(
        data_dir.path().join("vega.db"),
        data_dir.path().join("config"),
    );
    let make_form = |display_name: &str| McpServerForm {
        display_name: display_name.into(),
        transport: McpServerTransport::Local {
            executable: "/bin/sh".into(),
            args: vec![script_path.clone()],
            working_directory: Some(working_directory.clone()),
            environment: Vec::new(),
        },
    };
    let first = service
        .create(make_form("Local \"Primary\""))
        .expect("first same-transport MCP");
    let second = service
        .create(make_form("Local Secondary"))
        .expect("second same-transport MCP");
    service
        .set_enabled(&first.id, first.config_revision, true, true)
        .await
        .expect("enable first same-transport MCP");
    service
        .set_enabled(&second.id, second.config_revision, true, true)
        .await
        .expect("enable second same-transport MCP");
    let readiness = service
        .ready_for_run()
        .await
        .expect("same-transport MCP readiness");
    assert_eq!(readiness.ready_servers.len(), 2);
    assert!(readiness.unavailable.is_empty());

    let provider =
        MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]]);
    let tools = vega_tools::Tools::new(project_dir.path()).expect("owned project tools");
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Once,
    };
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Use the local same-name echo tool",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
        readiness.ready_servers,
    )
    .await
    .expect("production conversation entry");
    assert!(!run.failed);

    let requests = provider.requests();
    let definitions = requests[0]
        .tools
        .iter()
        .filter(|definition| definition.name.starts_with("mcp_"))
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 2);
    assert!(definitions.iter().any(|definition| {
        definition
            .description
            .contains(r#"Configured server label (untrusted): "Local \"Primary\"";"#)
            && definition
                .description
                .contains("transport: stdio (local process)")
            && definition.description.contains("exact tool: echo")
    }));
    assert!(definitions.iter().any(|definition| {
        definition
            .description
            .contains("Configured server label (untrusted): \"Local Secondary\";")
            && definition
                .description
                .contains("transport: stdio (local process)")
            && definition.description.contains("exact tool: echo")
    }));
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
    let _fixture = vega_mcp::mock::catalog_stdio(
        &script,
        serde_json::json!([{"name":"echo","inputSchema":{"type":"object"}}]),
        "current-runtime-stdio".into(),
        Arc::new(|server| {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&server.args[1])
                    .unwrap(),
                "started"
            )
            .unwrap();
        }),
    );
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
