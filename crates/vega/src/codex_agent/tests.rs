use super::*;
use serde_json::json;
use std::sync::mpsc as std_mpsc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::oneshot;
use vega_acp::RequestId;

#[test]
fn issue287_permission_option_kinds_are_exact_and_fail_closed() {
    assert_eq!(
        codex_permission_action(Some("allow_once")),
        Some(CodexPermissionAction::Allow)
    );
    assert_eq!(
        codex_permission_action(Some("allow_always")),
        Some(CodexPermissionAction::Allow)
    );
    assert_eq!(
        codex_permission_action(Some("reject_once")),
        Some(CodexPermissionAction::Reject)
    );
    assert_eq!(
        codex_permission_action(Some("reject_always")),
        Some(CodexPermissionAction::Reject)
    );
    assert_eq!(codex_permission_action(None), None);
    assert_eq!(codex_permission_action(Some("approve")), None);
    assert_eq!(codex_permission_action(Some("allow_custom")), None);
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Value {
    let mut frame = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).await.unwrap();
        if byte[0] == b'\n' {
            return serde_json::from_slice(&frame).unwrap();
        }
        frame.push(byte[0]);
    }
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, value: Value) {
    writer
        .write_all(&serde_json::to_vec(&value).unwrap())
        .await
        .unwrap();
    writer.write_all(b"\n").await.unwrap();
}

async fn scripted_v1_session(
    peer: &mut tokio::io::DuplexStream,
    cwd: &str,
    session_id: &str,
) -> Value {
    let initialize = read_frame(peer).await;
    write_frame(
        peer,
        json!({
            "jsonrpc":"2.0",
            "id":initialize["id"],
            "result":{"protocolVersion":1,"agentCapabilities":{},"authMethods":[]}
        }),
    )
    .await;
    let new_session = read_frame(peer).await;
    assert_eq!(new_session["method"], "session/new");
    assert_eq!(new_session["params"]["cwd"], cwd);
    write_frame(
        peer,
        json!({
            "jsonrpc":"2.0",
            "id":new_session["id"],
            "result":{
                "sessionId":session_id,
                "modes":{"availableModes":[{"id":"workspace-write"}]}
            }
        }),
    )
    .await;
    read_frame(peer).await
}

fn codex_creation_state(store: &Store, thread_id: &str) -> CodexSessionCreationState {
    vega_conversation::codex_tasks::codex_task_identity(store, thread_id)
        .unwrap()
        .unwrap()
        .session_creation
}

#[test]
fn issue287_unavailable_executable_fails_preflight_before_draft_materialization() {
    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("vega.db");
    let store = Store::open(&database_path).unwrap();
    store.migrate().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        workspace.to_str().unwrap(),
        "Codex preflight fixture",
        None,
    )
    .unwrap();
    let mut draft =
        vega_conversation::threads::draft_thread(Some(&project.id), "native-default", "confirm");
    draft.backend = TaskBackend::Codex;
    let config_path = directory.path().join("config.toml");
    let missing_executable = directory.path().join("missing-codex-acp");
    let mut config = vega_store::config::AppConfig::default();
    config.agent.codex_acp_profile = Some(vega_store::config::CodexAcpProfileConfig {
        display_name: "Scripted ACP".into(),
        executable: missing_executable.to_string_lossy().into_owned(),
        args: Vec::new(),
    });
    config.save_to(&config_path).unwrap();
    assert!(matches!(
        prepare_codex_execution(Some(&config_path), &database_path, &draft),
        Err(CodexPreflightFailure::ExecutableUnavailable)
    ));
    assert!(
        vega_store::threads::find(store.conn(), &draft.id)
            .unwrap()
            .is_none()
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let non_executable = directory.path().join("non-executable-codex-acp");
        std::fs::write(&non_executable, b"scripted adapter placeholder").unwrap();
        std::fs::set_permissions(&non_executable, std::fs::Permissions::from_mode(0o600)).unwrap();
        config.agent.codex_acp_profile = Some(vega_store::config::CodexAcpProfileConfig {
            display_name: "Scripted ACP".into(),
            executable: non_executable.to_string_lossy().into_owned(),
            args: Vec::new(),
        });
        config.save_to(&config_path).unwrap();
        assert!(matches!(
            prepare_codex_execution(Some(&config_path), &database_path, &draft),
            Err(CodexPreflightFailure::ExecutableUnavailable)
        ));
        assert!(
            vega_store::threads::find(store.conn(), &draft.id)
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn issue287_set_mode_rejection_is_definitive_and_never_prompts() {
    let (store, _directory, thread, snapshot) = issue287_store();
    let CodexSessionCreationState::Intent { intent_id } =
        vega_conversation::codex_tasks::begin_codex_session_creation(&store, &thread.id).unwrap()
    else {
        panic!("expected intent");
    };
    let (client, mut peer) = tokio::io::duplex(65_536);
    let (reader, writer) = tokio::io::split(client);
    let connection = Arc::new(vega_acp::test_support::connection_from_io(reader, writer));
    let cwd = snapshot.workspace.canonical_working_directory.clone();
    let server = tokio::spawn(async move {
        let set_mode = scripted_v1_session(&mut peer, &cwd, "rejected-mode-session").await;
        assert_eq!(set_mode["method"], "session/set_mode");
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "id":set_mode["id"],
                "error":{"code":-32000,"message":"mode rejected"}
            }),
        )
        .await;
        let cancel = read_frame(&mut peer).await;
        assert_eq!(cancel["method"], "session/cancel");
    });
    let (sender, _receiver) = std_mpsc::sync_channel(8);
    assert!(
        !run_codex_agent_with_connection(
            &store,
            connection.clone(),
            &thread,
            &snapshot,
            &intent_id,
            "do not send",
            &PermissionQueue::new(),
            CancellationToken::new(),
            &sender,
        )
        .await
    );
    server.await.unwrap();
    assert!(matches!(
        codex_creation_state(&store, &thread.id),
        CodexSessionCreationState::DefinitivelyFailed {
            code: CodexSessionFailureCode::AdapterRejected,
            ..
        }
    ));
    assert!(
        vega_conversation::codex_tasks::codex_prompt_binding(&store, &thread.id)
            .unwrap()
            .is_none()
    );
    assert!(
        vega_store::messages::recent(store.conn(), &thread.id, 10)
            .unwrap()
            .is_empty()
    );
    connection.shutdown().await;
}

#[tokio::test]
async fn issue287_set_mode_transport_loss_is_uncertain_and_never_prompts() {
    let (store, _directory, thread, snapshot) = issue287_store();
    let CodexSessionCreationState::Intent { intent_id } =
        vega_conversation::codex_tasks::begin_codex_session_creation(&store, &thread.id).unwrap()
    else {
        panic!("expected intent");
    };
    let (client, mut peer) = tokio::io::duplex(65_536);
    let (reader, writer) = tokio::io::split(client);
    let connection = Arc::new(vega_acp::test_support::connection_from_io(reader, writer));
    let cwd = snapshot.workspace.canonical_working_directory.clone();
    let server = tokio::spawn(async move {
        let set_mode = scripted_v1_session(&mut peer, &cwd, "lost-mode-session").await;
        assert_eq!(set_mode["method"], "session/set_mode");
        drop(peer);
    });
    let (sender, _receiver) = std_mpsc::sync_channel(8);
    assert!(
        !run_codex_agent_with_connection(
            &store,
            connection.clone(),
            &thread,
            &snapshot,
            &intent_id,
            "do not send",
            &PermissionQueue::new(),
            CancellationToken::new(),
            &sender,
        )
        .await
    );
    server.await.unwrap();
    assert!(matches!(
        codex_creation_state(&store, &thread.id),
        CodexSessionCreationState::Uncertain {
            code: CodexSessionUncertaintyCode::OutcomeUnknown,
            ..
        }
    ));
    assert!(
        vega_conversation::codex_tasks::codex_prompt_binding(&store, &thread.id)
            .unwrap()
            .is_none()
    );
    assert!(
        vega_store::messages::recent(store.conn(), &thread.id, 10)
            .unwrap()
            .is_empty()
    );
    connection.shutdown().await;
}

fn issue287_store() -> (Store, tempfile::TempDir, Thread, CodexExecutionSnapshot) {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("vega.db");
    let store = Store::open(database).unwrap();
    store.migrate().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let workspace = std::fs::canonicalize(workspace).unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        workspace.to_str().unwrap(),
        "Codex fixture",
        None,
    )
    .unwrap();
    let mut draft = Thread {
        id: ulid::Ulid::generate().to_string(),
        backend: TaskBackend::Codex,
        project_id: project.id.clone(),
        title: String::new(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        model: "native-default".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    };
    let snapshot = CodexExecutionSnapshot {
        profile: CodexProfileReference {
            id: "codex-acp-default".into(),
            display_name: "Scripted ACP".into(),
        },
        adapter: CodexAdapterKind::CodexAcp,
        executable: "/bin/true".into(),
        arguments: CodexAdapterArgument::from_argv(&["--test-mode".into()]).unwrap(),
        adapter_version: "unknown".into(),
        codex_version: "unknown".into(),
        settings: CodexRunSettings {
            model: None,
            model_provider: None,
            reasoning_effort: None,
            sandbox_mode: CodexSandboxMode::WorkspaceWrite,
            approval_policy: CodexApprovalPolicy::OnRequest,
        },
        workspace: CodexWorkspaceSnapshot {
            project_id: Some(project.id),
            worktree_id: None,
            canonical_working_directory: workspace.to_string_lossy().into_owned(),
            additional_directories: Vec::new(),
        },
    };
    draft =
        vega_conversation::codex_tasks::materialize_codex_draft(&store, &draft, &snapshot).unwrap();
    (store, directory, draft, snapshot)
}

#[tokio::test]
async fn issue287_first_codex_prompt_waits_for_empty_set_mode_ack_and_durable_binding() {
    let (store, _directory, thread, snapshot) = issue287_store();
    let CodexSessionCreationState::Intent { intent_id } =
        vega_conversation::codex_tasks::begin_codex_session_creation(&store, &thread.id).unwrap()
    else {
        panic!("expected intent");
    };
    let (client, mut peer) = tokio::io::duplex(65_536);
    let (reader, writer) = tokio::io::split(client);
    let connection = Arc::new(vega_acp::test_support::connection_from_io(reader, writer));
    let expected_thread_id = thread.id.clone();
    let expected_cwd = snapshot.workspace.canonical_working_directory.clone();
    let server = tokio::spawn(async move {
        let initialize = read_frame(&mut peer).await;
        assert_eq!(initialize["method"], "initialize");
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "id":initialize["id"],
                "result":{"protocolVersion":1,"agentCapabilities":{},"authMethods":[]}
            }),
        )
        .await;
        let new_session = read_frame(&mut peer).await;
        assert_eq!(new_session["method"], "session/new");
        assert_eq!(new_session["params"]["cwd"], expected_cwd);
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "id":new_session["id"],
                "result":{
                    "sessionId":"scripted-session",
                    "modes":{"availableModes":[{"id":"workspace-write"}]}
                }
            }),
        )
        .await;
        let set_mode = read_frame(&mut peer).await;
        assert_eq!(set_mode["method"], "session/set_mode");
        assert_eq!(set_mode["params"]["modeId"], "workspace-write");
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","id":set_mode["id"],"result":{}}),
        )
        .await;
        let prompt = read_frame(&mut peer).await;
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(
            prompt["params"]["prompt"],
            json!([{"type":"text","text":"edit the project"}])
        );
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "method":"session/update",
                "params":{"sessionId":"scripted-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Updated file."}}}
            }),
        )
        .await;
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"end_turn"}}),
        )
        .await;
        (expected_thread_id, prompt)
    });
    let (sender, receiver) = std_mpsc::sync_channel(16);
    let success = run_codex_agent_with_connection(
        &store,
        connection.clone(),
        &thread,
        &snapshot,
        &intent_id,
        "edit the project",
        &PermissionQueue::new(),
        CancellationToken::new(),
        &sender,
    )
    .await;
    assert!(
        success,
        "scripted first run failed: {}",
        safe_run_state(&store, &thread.id)
    );
    let (thread_id, _) = server.await.unwrap();
    let binding = vega_conversation::codex_tasks::codex_prompt_binding(&store, &thread_id)
        .unwrap()
        .unwrap();
    assert_eq!(binding.session_id, "scripted-session");
    let messages = vega_store::messages::recent(store.conn(), &thread_id, 10).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "edit the project");
    assert_eq!(messages[1].content, "Updated file.");
    assert_eq!(messages[1].status, "done");
    let updates = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        updates
            .iter()
            .any(|update| matches!(update, AgentUpdate::CodexModeConfirmed))
    );
    assert!(updates.iter().any(|update| matches!(
        update,
        AgentUpdate::Event(ConversationEvent::TextDelta { delta, .. }) if delta == "Updated file."
    )));
    connection.shutdown().await;
}

#[tokio::test]
async fn issue287_codex_tool_permission_and_lifecycle_keep_exact_option_once() {
    let (store, _directory, thread, snapshot) = issue287_store();
    let CodexSessionCreationState::Intent { intent_id } =
        vega_conversation::codex_tasks::begin_codex_session_creation(&store, &thread.id).unwrap()
    else {
        panic!("expected intent");
    };
    let (client, mut peer) = tokio::io::duplex(65_536);
    let (reader, writer) = tokio::io::split(client);
    let connection = Arc::new(vega_acp::test_support::connection_from_io(reader, writer));
    let expected_cwd = snapshot.workspace.canonical_working_directory.clone();
    let (close_peer, wait_close_peer) = oneshot::channel();
    let server = tokio::spawn(async move {
        let initialize = read_frame(&mut peer).await;
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","id":initialize["id"],"result":{"protocolVersion":1,"agentCapabilities":{},"authMethods":[]}}),
        )
        .await;
        let new_session = read_frame(&mut peer).await;
        assert_eq!(new_session["params"]["cwd"], expected_cwd);
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","id":new_session["id"],"result":{"sessionId":"permission-session","modes":{"availableModes":[{"id":"workspace-write"}]}}}),
        )
        .await;
        let set_mode = read_frame(&mut peer).await;
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","id":set_mode["id"],"result":{}}),
        )
        .await;
        let prompt = read_frame(&mut peer).await;
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "method":"session/update",
                "params":{"sessionId":"permission-session","update":{"sessionUpdate":"tool_call","toolCallId":"external-call-1","title":"Run command","kind":"execute","status":"pending","rawInput":{"command":"echo do not persist this value"}}}
            }),
        )
        .await;
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "id":88,
                "method":"session/request_permission",
                "params":{
                    "sessionId":"permission-session",
                    "toolCall":{"toolCallId":"external-call-1","title":"Run command","kind":"execute","rawInput":{"command":"echo do not persist this value"}},
                    "options":[
                        {"optionId":"allow_once","name":"Allow once","kind":"allow_once"},
                        {"optionId":"allow_always","name":"Always allow","kind":"allow_always"},
                        {"optionId":"reject_once","name":"Reject","kind":"reject_once"}
                    ]
                }
            }),
        )
        .await;
        let permission_response = read_frame(&mut peer).await;
        assert_eq!(permission_response["id"], 88);
        assert_eq!(
            permission_response["result"]["outcome"]["optionId"],
            "allow_always"
        );
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"permission-session","update":{"sessionUpdate":"tool_call_update","toolCallId":"external-call-1","status":"in_progress"}}}),
        )
        .await;
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"permission-session","update":{"sessionUpdate":"tool_call_update","toolCallId":"external-call-1","status":"completed"}}}),
        )
        .await;
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"permission-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}}}),
        )
        .await;
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"end_turn"}}),
        )
        .await;
        let _ = wait_close_peer.await;
    });
    let queue = PermissionQueue::new();
    let mut listener = queue.subscribe();
    let (sender, receiver) = std_mpsc::sync_channel(32);
    let run = run_codex_agent_with_connection(
        &store,
        connection.clone(),
        &thread,
        &snapshot,
        &intent_id,
        "edit the project",
        &queue,
        CancellationToken::new(),
        &sender,
    );
    let select_permission = async {
        loop {
            assert!(listener.changed().await);
            if let Some(pending) = queue.take_pending() {
                let (request, mut lease) = pending.into_parts().unwrap();
                let options = request.acp_options.unwrap();
                assert_eq!(options[1].option_id, "allow_always");
                assert!(lease.respond(PermissionDecision::AcpOption {
                    option_id: options[1].option_id.clone(),
                }));
                assert!(!lease.respond(PermissionDecision::AcpOption {
                    option_id: options[1].option_id.clone(),
                }));
                break;
            }
        }
    };
    let (success, ()) = tokio::join!(run, select_permission);
    assert!(
        success,
        "scripted tool run failed: {}",
        safe_run_state(&store, &thread.id)
    );
    assert!(matches!(
        connection
            .respond_permission(RequestId::Number("88".into()), "allow_always")
            .await,
        Err(vega_acp::Error::PermissionRequestClosed)
    ));
    let _ = close_peer.send(());
    server.await.unwrap();
    let mut statement = store
        .conn()
        .prepare(
            "SELECT id, tool, input_json, status, approval FROM tool_calls WHERE thread_id = ?1",
        )
        .unwrap();
    let row = statement
        .query_row([&thread.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .unwrap();
    assert_eq!(row.1, "codex_acp");
    assert_eq!(row.3, "success");
    assert!(!row.2.contains("do not persist this value"));
    assert_eq!(
        vega_conversation::types::ApprovalAudit::from_json(&row.4)
            .unwrap()
            .source,
        ApprovalSource::User
    );
    let updates = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    assert!(updates.iter().any(|update| matches!(
        update,
        AgentUpdate::Event(ConversationEvent::ToolCallProposed { call })
            if call.tool == "codex_acp"
    )));
    assert!(updates.iter().any(|update| matches!(
        update,
        AgentUpdate::Event(ConversationEvent::ToolCallRunning { .. })
    )));
    assert!(updates.iter().any(|update| matches!(
        update,
        AgentUpdate::Event(ConversationEvent::ToolCallFinished { result, .. })
            if result.status == ToolCallStatus::Success
    )));
    connection.shutdown().await;
}

#[tokio::test]
async fn issue287_late_permission_after_tool_started_fails_closed() {
    let (store, _directory, thread, snapshot) = issue287_store();
    let CodexSessionCreationState::Intent { intent_id } =
        vega_conversation::codex_tasks::begin_codex_session_creation(&store, &thread.id).unwrap()
    else {
        panic!("expected intent");
    };
    let (client, mut peer) = tokio::io::duplex(65_536);
    let (reader, writer) = tokio::io::split(client);
    let connection = Arc::new(vega_acp::test_support::connection_from_io(reader, writer));
    let cwd = snapshot.workspace.canonical_working_directory.clone();
    let server = tokio::spawn(async move {
        let set_mode = scripted_v1_session(&mut peer, &cwd, "late-permission-session").await;
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","id":set_mode["id"],"result":{}}),
        )
        .await;
        let prompt = read_frame(&mut peer).await;
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "method":"session/update",
                "params":{"sessionId":"late-permission-session","update":{"sessionUpdate":"tool_call_update","toolCallId":"late-permission-call","title":"Run command","kind":"execute","status":"in_progress","rawInput":{"command":"redacted"}}}
            }),
        )
        .await;
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "id":91,
                "method":"session/request_permission",
                "params":{
                    "sessionId":"late-permission-session",
                    "toolCall":{"toolCallId":"late-permission-call","title":"Run command","kind":"execute","rawInput":{"command":"redacted"}},
                    "options":[{"optionId":"allow_once","name":"Allow once","kind":"allow_once"}]
                }
            }),
        )
        .await;
        let cancel = read_frame(&mut peer).await;
        assert_eq!(cancel["method"], "session/cancel");
        assert_eq!(cancel["params"]["sessionId"], "late-permission-session");
        let permission_closed = read_frame(&mut peer).await;
        assert_eq!(permission_closed["id"], 91);
        assert_eq!(
            permission_closed["result"]["outcome"]["outcome"],
            "cancelled"
        );
        let _ = prompt;
    });
    let queue = PermissionQueue::new();
    let (sender, _receiver) = std_mpsc::sync_channel(16);
    assert!(
        !run_codex_agent_with_connection(
            &store,
            connection.clone(),
            &thread,
            &snapshot,
            &intent_id,
            "edit the project",
            &queue,
            CancellationToken::new(),
            &sender,
        )
        .await
    );
    assert!(queue.take_pending().is_none());
    server.await.unwrap();
    let status = store
        .conn()
        .query_row(
            "SELECT status FROM tool_calls WHERE thread_id = ?1",
            [&thread.id],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(status, "failed");
    connection.shutdown().await;
    assert!(
        connection
            .respond_permission(RequestId::Number("91".into()), "allow_once")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn issue287_stop_during_permission_cancels_session_and_closes_card() {
    let (store, _directory, thread, snapshot) = issue287_store();
    let CodexSessionCreationState::Intent { intent_id } =
        vega_conversation::codex_tasks::begin_codex_session_creation(&store, &thread.id).unwrap()
    else {
        panic!("expected intent");
    };
    let (client, mut peer) = tokio::io::duplex(65_536);
    let (reader, writer) = tokio::io::split(client);
    let connection = Arc::new(vega_acp::test_support::connection_from_io(reader, writer));
    let cwd = snapshot.workspace.canonical_working_directory.clone();
    let server = tokio::spawn(async move {
        let set_mode = scripted_v1_session(&mut peer, &cwd, "cancel-session").await;
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","id":set_mode["id"],"result":{}}),
        )
        .await;
        let prompt = read_frame(&mut peer).await;
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "method":"session/update",
                "params":{"sessionId":"cancel-session","update":{"sessionUpdate":"tool_call","toolCallId":"cancel-call","title":"Run command","kind":"execute","status":"pending","rawInput":{"command":"safe to redact"}}}
            }),
        )
        .await;
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "id":89,
                "method":"session/request_permission",
                "params":{
                    "sessionId":"cancel-session",
                    "toolCall":{"toolCallId":"cancel-call","title":"Run command","kind":"execute","rawInput":{"command":"safe to redact"}},
                    "options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"}]
                }
            }),
        )
        .await;
        let cancel = read_frame(&mut peer).await;
        assert_eq!(cancel["method"], "session/cancel");
        assert_eq!(cancel["params"]["sessionId"], "cancel-session");
        let permission_response = read_frame(&mut peer).await;
        assert_eq!(permission_response["id"], 89);
        assert_eq!(
            permission_response["result"]["outcome"]["outcome"],
            "cancelled"
        );
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"cancelled"}}),
        )
        .await;
    });
    let queue = PermissionQueue::new();
    let mut listener = queue.subscribe();
    let cancel = CancellationToken::new();
    let (sender, _receiver) = std_mpsc::sync_channel(32);
    let run = run_codex_agent_with_connection(
        &store,
        connection.clone(),
        &thread,
        &snapshot,
        &intent_id,
        "edit the project",
        &queue,
        cancel.clone(),
        &sender,
    );
    let stop_after_card = async {
        loop {
            assert!(listener.changed().await);
            if let Some(pending) = queue.take_pending() {
                let (_request, lease) = pending.into_parts().unwrap();
                cancel.cancel();
                break lease;
            }
        }
    };
    let (success, mut lease) = tokio::join!(run, stop_after_card);
    assert!(!success);
    assert!(lease.is_resolved());
    assert!(!lease.respond(PermissionDecision::AcpOption {
        option_id: "allow".into(),
    }));
    server.await.unwrap();
    connection.shutdown().await;
}

#[tokio::test]
async fn issue287_foreign_permission_session_cancels_only_owned_session() {
    let (store, _directory, thread, snapshot) = issue287_store();
    let CodexSessionCreationState::Intent { intent_id } =
        vega_conversation::codex_tasks::begin_codex_session_creation(&store, &thread.id).unwrap()
    else {
        panic!("expected intent");
    };
    let (client, mut peer) = tokio::io::duplex(65_536);
    let (reader, writer) = tokio::io::split(client);
    let connection = Arc::new(vega_acp::test_support::connection_from_io(reader, writer));
    let cwd = snapshot.workspace.canonical_working_directory.clone();
    let server = tokio::spawn(async move {
        let set_mode = scripted_v1_session(&mut peer, &cwd, "owned-session").await;
        write_frame(
            &mut peer,
            json!({"jsonrpc":"2.0","id":set_mode["id"],"result":{}}),
        )
        .await;
        let prompt = read_frame(&mut peer).await;
        write_frame(
            &mut peer,
            json!({
                "jsonrpc":"2.0",
                "id":90,
                "method":"session/request_permission",
                "params":{
                    "sessionId":"foreign-session",
                    "toolCall":{"toolCallId":"foreign-call","title":"Run command","kind":"execute","rawInput":{"command":"redacted"}},
                    "options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"}]
                }
            }),
        )
        .await;
        let cancel = read_frame(&mut peer).await;
        assert_eq!(cancel["method"], "session/cancel");
        assert_eq!(cancel["params"]["sessionId"], "owned-session");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), read_frame(&mut peer))
                .await
                .is_err()
        );
        let _ = prompt;
    });
    let (sender, _receiver) = std_mpsc::sync_channel(16);
    assert!(
        !run_codex_agent_with_connection(
            &store,
            connection.clone(),
            &thread,
            &snapshot,
            &intent_id,
            "edit the project",
            &PermissionQueue::new(),
            CancellationToken::new(),
            &sender,
        )
        .await
    );
    server.await.unwrap();
    connection.shutdown().await;
}

fn safe_run_state(store: &Store, thread_id: &str) -> String {
    let creation = store
        .conn()
        .query_row(
            "SELECT state, failure_code, uncertainty_code, session_id FROM codex_session_creations WHERE thread_id = ?1",
            [thread_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .unwrap();
    let message_statuses = store
        .conn()
        .prepare("SELECT status FROM messages WHERE thread_id = ?1 ORDER BY seq")
        .unwrap()
        .query_map([thread_id], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    format!("creation={creation:?}, message_statuses={message_statuses:?}")
}
