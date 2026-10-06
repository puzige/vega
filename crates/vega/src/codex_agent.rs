use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc as tokio_mpsc;
use tokio_util::sync::CancellationToken;
use vega_acp::{Connection, LaunchConfig, PermissionRequest as AcpPermissionRequest};
use vega_conversation::agent::{PermissionHook, PermissionQueue};
use vega_conversation::types::*;
use vega_store::Store;

use crate::app_agent::AgentUpdate;

#[derive(Clone, Copy)]
pub(crate) enum CodexPreflightFailure {
    ProfileUnavailable,
    InvalidProfile,
    ExecutableUnavailable,
    WorkspaceUnavailable,
}

impl CodexPreflightFailure {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::ProfileUnavailable => "尚未配置 Codex ACP，请前往设置 → Agents 配置",
            Self::InvalidProfile => "Codex ACP 配置无效，请检查 executable 与 argv",
            Self::ExecutableUnavailable => "Codex ACP executable 不可用，请检查路径和权限",
            Self::WorkspaceUnavailable => "所选项目目录不可用，请重新选择项目",
        }
    }
}

pub(crate) fn prepare_codex_execution(
    config_path: Option<&Path>,
    database_path: &Path,
    draft: &Thread,
) -> Result<CodexExecutionSnapshot, CodexPreflightFailure> {
    if draft.backend != TaskBackend::Codex || draft.project_id.is_empty() {
        return Err(CodexPreflightFailure::WorkspaceUnavailable);
    }
    let config_path = config_path.ok_or(CodexPreflightFailure::ProfileUnavailable)?;
    let config = vega_store::config::read_from(config_path)
        .map_err(|_| CodexPreflightFailure::ProfileUnavailable)?;
    let profile = config
        .agent
        .codex_acp_profile
        .ok_or(CodexPreflightFailure::ProfileUnavailable)?;
    profile
        .validate()
        .map_err(|_| CodexPreflightFailure::InvalidProfile)?;
    if !codex_executable_available(Path::new(&profile.executable)) {
        return Err(CodexPreflightFailure::ExecutableUnavailable);
    }
    let arguments = CodexAdapterArgument::from_argv(&profile.args)
        .map_err(|_| CodexPreflightFailure::InvalidProfile)?;
    let store =
        Store::open(database_path).map_err(|_| CodexPreflightFailure::WorkspaceUnavailable)?;
    let project = vega_store::projects::find(store.conn(), &draft.project_id)
        .map_err(|_| CodexPreflightFailure::WorkspaceUnavailable)?
        .ok_or(CodexPreflightFailure::WorkspaceUnavailable)?;
    let cwd = std::fs::canonicalize(project.path)
        .map_err(|_| CodexPreflightFailure::WorkspaceUnavailable)?;
    if !cwd.is_dir() {
        return Err(CodexPreflightFailure::WorkspaceUnavailable);
    }
    Ok(CodexExecutionSnapshot {
        profile: CodexProfileReference {
            id: "codex-acp-default".into(),
            display_name: profile.display_name,
        },
        adapter: CodexAdapterKind::CodexAcp,
        executable: profile.executable,
        arguments,
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
            project_id: Some(draft.project_id.clone()),
            worktree_id: None,
            canonical_working_directory: cwd.to_string_lossy().into_owned(),
            additional_directories: Vec::new(),
        },
    })
}

fn codex_executable_available(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub(crate) struct CodexWorkerRequest {
    pub database_path: PathBuf,
    pub thread: Thread,
    pub snapshot: CodexExecutionSnapshot,
    pub intent: CodexSessionCreationState,
    pub prompt: String,
    pub permission_queue: PermissionQueue,
    pub cancel: CancellationToken,
    pub sender: std_mpsc::SyncSender<AgentUpdate>,
}

pub(crate) fn run_codex_agent_worker(request: CodexWorkerRequest) -> bool {
    let intent_id = match &request.intent {
        CodexSessionCreationState::Intent { intent_id } => intent_id.clone(),
        _ => return false,
    };
    let Ok(store) = Store::open(&request.database_path) else {
        return false;
    };
    let arguments = request
        .snapshot
        .arguments
        .iter()
        .flat_map(|argument| argument.as_args().iter().cloned())
        .collect::<Vec<_>>();
    let launch = LaunchConfig::new(
        PathBuf::from(&request.snapshot.executable),
        arguments,
        PathBuf::from(&request.snapshot.workspace.canonical_working_directory),
    );
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            let _ = vega_conversation::codex_tasks::mark_codex_session_definitively_failed(
                &store,
                &request.thread.id,
                &intent_id,
                CodexSessionFailureCode::ProcessStartFailed,
            );
            return false;
        }
    };
    let connection = match runtime.block_on(Connection::spawn(launch)) {
        Ok(connection) => Arc::new(connection),
        Err(_) => {
            let _ = vega_conversation::codex_tasks::mark_codex_session_definitively_failed(
                &store,
                &request.thread.id,
                &intent_id,
                CodexSessionFailureCode::ProcessStartFailed,
            );
            return false;
        }
    };
    let success = runtime.block_on(run_codex_agent_with_connection(CodexAgentRunContext {
        store: &store,
        connection: connection.clone(),
        thread: &request.thread,
        snapshot: &request.snapshot,
        intent_id: &intent_id,
        prompt: &request.prompt,
        permission_queue: &request.permission_queue,
        cancel: request.cancel,
        sender: &request.sender,
    }));
    runtime.block_on(connection.shutdown());
    success
}

pub(crate) struct CodexAgentRunContext<'a> {
    store: &'a Store,
    connection: Arc<Connection>,
    thread: &'a Thread,
    snapshot: &'a CodexExecutionSnapshot,
    intent_id: &'a CodexSessionIntentId,
    prompt: &'a str,
    permission_queue: &'a PermissionQueue,
    cancel: CancellationToken,
    sender: &'a std_mpsc::SyncSender<AgentUpdate>,
}

pub(crate) async fn run_codex_agent_with_connection(context: CodexAgentRunContext<'_>) -> bool {
    let CodexAgentRunContext {
        store,
        connection,
        thread,
        snapshot,
        intent_id,
        prompt,
        permission_queue,
        cancel,
        sender,
    } = context;
    let fail_intent = |code| {
        let _ = vega_conversation::codex_tasks::mark_codex_session_definitively_failed(
            store, &thread.id, intent_id, code,
        );
    };
    if connection.initialize().await.is_err() {
        fail_intent(CodexSessionFailureCode::AdapterRejected);
        return false;
    }
    if cancel.is_cancelled() {
        fail_intent(CodexSessionFailureCode::ProcessStartFailed);
        return false;
    }
    let new_session = connection.new_session(&snapshot.workspace.canonical_working_directory);
    tokio::pin!(new_session);
    let session = match tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let _ = vega_conversation::codex_tasks::mark_codex_session_uncertain(
                store,
                &thread.id,
                intent_id,
                CodexSessionUncertaintyCode::OutcomeUnknown,
            );
            return false;
        }
        result = &mut new_session => result,
    } {
        Ok(session) => session,
        Err(vega_acp::Error::AgentRejected { .. }) => {
            fail_intent(CodexSessionFailureCode::AdapterRejected);
            return false;
        }
        Err(_) => {
            let _ = vega_conversation::codex_tasks::mark_codex_session_uncertain(
                store,
                &thread.id,
                intent_id,
                CodexSessionUncertaintyCode::OutcomeUnknown,
            );
            return false;
        }
    };
    let workspace_write_offered = session
        .raw
        .pointer("/modes/availableModes")
        .and_then(Value::as_array)
        .is_some_and(|modes| {
            modes
                .iter()
                .any(|mode| mode.get("id").and_then(Value::as_str) == Some("workspace-write"))
        });
    if !workspace_write_offered {
        fail_intent(CodexSessionFailureCode::AdapterRejected);
        return false;
    }
    let set_mode = connection.set_mode(&session.session_id, "workspace-write");
    tokio::pin!(set_mode);
    let mode_result = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let _ = connection.cancel(&session.session_id).await;
            let _ = vega_conversation::codex_tasks::mark_codex_session_uncertain(
                store,
                &thread.id,
                intent_id,
                CodexSessionUncertaintyCode::OutcomeUnknown,
            );
            return false;
        }
        result = &mut set_mode => result,
    };
    match mode_result {
        Ok(_) => {}
        Err(vega_acp::Error::AgentRejected { .. }) => {
            let _ = connection.cancel(&session.session_id).await;
            fail_intent(CodexSessionFailureCode::AdapterRejected);
            return false;
        }
        Err(_) => {
            let _ = connection.cancel(&session.session_id).await;
            let _ = vega_conversation::codex_tasks::mark_codex_session_uncertain(
                store,
                &thread.id,
                intent_id,
                CodexSessionUncertaintyCode::OutcomeUnknown,
            );
            return false;
        }
    }
    if vega_conversation::codex_tasks::confirm_codex_session_creation(
        store,
        &thread.id,
        intent_id,
        &session.session_id,
    )
    .is_err()
    {
        let _ = vega_conversation::codex_tasks::mark_codex_session_uncertain(
            store,
            &thread.id,
            intent_id,
            CodexSessionUncertaintyCode::OutcomeUnknown,
        );
        return false;
    }
    let _ = sender.send(AgentUpdate::CodexModeConfirmed);
    if cancel.is_cancelled() {
        let _ = connection.cancel(&session.session_id).await;
        return false;
    }
    let Some((user_message_id, assistant_message_id, assistant_seq)) =
        persist_codex_messages(store, thread, prompt)
    else {
        return false;
    };
    if sender
        .send(AgentUpdate::Event(ConversationEvent::MessageStarted {
            message_id: assistant_message_id.clone(),
            seq: assistant_seq as u64,
        }))
        .is_err()
    {
        return false;
    }
    let _ = user_message_id;
    let prompt_handle = match connection.prompt(&session.session_id, prompt).await {
        Ok(handle) => handle,
        Err(_) => {
            let _ = vega_store::messages::finish_streaming(
                store.conn(),
                &assistant_message_id,
                "",
                "failed",
                None,
            );
            send_codex_error(sender, &assistant_message_id);
            return false;
        }
    };
    let mut prompt_wait = Box::pin(prompt_handle.wait());
    let mut assistant_text = String::new();
    let mut cancel_deadline = None;
    let mut tool_activities = HashMap::new();
    let (permission_sender, mut permission_receiver) = tokio_mpsc::unbounded_channel();
    let permission_cancel = cancel.child_token();
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled(), if cancel_deadline.is_none() => {
                let _ = connection.cancel(&session.session_id).await;
                cancel_deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(3));
            }
            _ = async {
                match cancel_deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending().await,
                }
            } => {
                permission_cancel.cancel();
                permission_queue.timeout_active();
                finalize_codex_tool_activities(
                    store,
                    &thread.id,
                    &assistant_message_id,
                    &mut tool_activities,
                    ToolCallStatus::Cancelled,
                    sender,
                );
                finish_codex_message(store, &assistant_message_id, &assistant_text, "interrupted");
                let _ = sender.send(AgentUpdate::Event(ConversationEvent::Interrupted {
                    message_id: assistant_message_id.clone(),
                    execution_duration_ms: None,
                }));
                return false;
            }
            Some(resolution) = permission_receiver.recv() => {
                apply_codex_permission_resolution(
                    store,
                    resolution,
                    &mut tool_activities,
                    sender,
                );
            }
            event = connection.recv_event() => {
                match event {
                    Ok(Some(vega_acp::Event::Notification { method, params }))
                        if method == "session/update"
                            && params.get("sessionId").and_then(Value::as_str) == Some(&session.session_id) =>
                    {
                        if params.pointer("/update/sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk")
                            && params.pointer("/update/content/type").and_then(Value::as_str) == Some("text")
                            && let Some(delta) = params.pointer("/update/content/text").and_then(Value::as_str)
                        {
                            assistant_text.push_str(delta);
                            let _ = vega_store::messages::update_streaming_content(
                                store.conn(),
                                &assistant_message_id,
                                &assistant_text,
                            );
                            let _ = sender.send(AgentUpdate::Event(ConversationEvent::TextDelta {
                                message_id: assistant_message_id.clone(),
                                delta: delta.to_string(),
                            }));
                        }
                        if let Some(update) = params.get("update") {
                            match update.get("sessionUpdate").and_then(Value::as_str) {
                                Some("tool_call") | Some("tool_call_update") => {
                                    if let Some(external_id) = ensure_codex_tool_activity(
                                        store,
                                        &thread.id,
                                        &session.session_id,
                                        &assistant_message_id,
                                        update,
                                        &mut tool_activities,
                                        sender,
                                    ) {
                                        apply_codex_tool_status(
                                            store,
                                            &external_id,
                                            update.get("status").and_then(Value::as_str),
                                            &mut tool_activities,
                                            sender,
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(Some(vega_acp::Event::PermissionRequest(request))) => {
                        if request.session_id != session.session_id {
                            permission_cancel.cancel();
                            permission_queue.timeout_active();
                            let _ = connection.cancel(&session.session_id).await;
                            finalize_codex_tool_activities(
                                store,
                                &thread.id,
                                &assistant_message_id,
                                &mut tool_activities,
                                ToolCallStatus::Failed,
                                sender,
                            );
                            finish_codex_message(store, &assistant_message_id, &assistant_text, "failed");
                            send_codex_error(sender, &assistant_message_id);
                            return false;
                        }
                        let Some(external_id) = ensure_codex_tool_activity(
                            store,
                            &thread.id,
                            &session.session_id,
                            &assistant_message_id,
                            &request.tool_call,
                            &mut tool_activities,
                            sender,
                        ) else {
                            let _ = connection.cancel(&session.session_id).await;
                            continue;
                        };
                        let permission_is_stale = tool_activities
                            .get(&external_id)
                            .is_none_or(|activity| {
                                activity.permission_pending
                                    || activity.status != ToolCallStatus::PendingApproval
                                    || activity.observed_in_progress
                            });
                        if permission_is_stale {
                            permission_cancel.cancel();
                            permission_queue.timeout_active();
                            let _ = connection.cancel(&session.session_id).await;
                            finalize_codex_tool_activities(
                                store,
                                &thread.id,
                                &assistant_message_id,
                                &mut tool_activities,
                                ToolCallStatus::Failed,
                                sender,
                            );
                            finish_codex_message(
                                store,
                                &assistant_message_id,
                                &assistant_text,
                                "failed",
                            );
                            send_codex_error(sender, &assistant_message_id);
                            return false;
                        }
                        if let Some(activity) = tool_activities.get_mut(&external_id) {
                            activity.permission_pending = true;
                        }
                        let Some(activity) = tool_activities.get(&external_id) else {
                            let _ = connection.cancel(&session.session_id).await;
                            continue;
                        };
                        let permission = codex_permission_request(&request, activity);
                        let connection = connection.clone();
                        let queue = permission_queue.clone();
                        let cancel = permission_cancel.clone();
                        let permission_sender = permission_sender.clone();
                        tokio::spawn(async move {
                            let selected = match queue.request(permission, cancel.clone()).await {
                                Ok(PermissionDecision::AcpOption { option_id }) => Some(
                                    request.options.iter().find(|option| option.option_id == option_id)
                                        .map(|option| (option_id, option.kind.clone())),
                                ),
                                _ => None,
                            };
                            let resolved = match selected.flatten() {
                                Some((option_id, kind))
                                    if codex_permission_action(kind.as_deref()).is_some()
                                        && connection
                                            .respond_permission(
                                                request.request_id.clone(),
                                                &option_id,
                                            )
                                            .await
                                            .is_ok() =>
                                {
                                    CodexPermissionResolution::Selected { external_id, kind }
                                }
                                _ => {
                                    if !cancel.is_cancelled() {
                                        let _ = connection.cancel(&request.session_id).await;
                                    }
                                    CodexPermissionResolution::TimedOut { external_id }
                                }
                            };
                            let _ = permission_sender.send(resolved);
                        });
                    }
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => {
                        if let Ok(Ok(result)) = tokio::time::timeout(
                            std::time::Duration::from_millis(50),
                            prompt_wait.as_mut(),
                        )
                        .await
                        {
                            return finish_codex_prompt_result(
                                result,
                                CodexPromptFinishContext {
                                    store,
                                    thread_id: &thread.id,
                                    message_id: &assistant_message_id,
                                    assistant_text: &assistant_text,
                                    activities: &mut tool_activities,
                                    permission_cancel: &permission_cancel,
                                    permission_queue,
                                    sender,
                                },
                            );
                        }
                        permission_cancel.cancel();
                        permission_queue.timeout_active();
                        finalize_codex_tool_activities(
                            store,
                            &thread.id,
                            &assistant_message_id,
                            &mut tool_activities,
                            ToolCallStatus::Failed,
                            sender,
                        );
                        finish_codex_message(store, &assistant_message_id, &assistant_text, "failed");
                        send_codex_error(sender, &assistant_message_id);
                        return false;
                    }
                }
            }
            result = &mut prompt_wait => {
                let Ok(result) = result else {
                    permission_cancel.cancel();
                    permission_queue.timeout_active();
                    finalize_codex_tool_activities(
                        store,
                        &thread.id,
                        &assistant_message_id,
                        &mut tool_activities,
                        ToolCallStatus::Failed,
                        sender,
                    );
                    finish_codex_message(store, &assistant_message_id, &assistant_text, "failed");
                    send_codex_error(sender, &assistant_message_id);
                    return false;
                };
                return finish_codex_prompt_result(
                    result,
                    CodexPromptFinishContext {
                        store,
                        thread_id: &thread.id,
                        message_id: &assistant_message_id,
                        assistant_text: &assistant_text,
                        activities: &mut tool_activities,
                        permission_cancel: &permission_cancel,
                        permission_queue,
                        sender,
                    },
                );
            }
        }
    }
}

struct CodexPromptFinishContext<'a> {
    store: &'a Store,
    thread_id: &'a str,
    message_id: &'a str,
    assistant_text: &'a str,
    activities: &'a mut HashMap<String, CodexToolActivity>,
    permission_cancel: &'a CancellationToken,
    permission_queue: &'a PermissionQueue,
    sender: &'a std_mpsc::SyncSender<AgentUpdate>,
}

fn finish_codex_prompt_result(
    result: vega_acp::PromptResult,
    context: CodexPromptFinishContext<'_>,
) -> bool {
    let CodexPromptFinishContext {
        store,
        thread_id,
        message_id,
        assistant_text,
        activities,
        permission_cancel,
        permission_queue,
        sender,
    } = context;
    permission_cancel.cancel();
    permission_queue.timeout_active();
    if result.stop_reason == "cancelled" {
        finalize_codex_tool_activities(
            store,
            thread_id,
            message_id,
            activities,
            ToolCallStatus::Cancelled,
            sender,
        );
        finish_codex_message(store, message_id, assistant_text, "interrupted");
        let _ = sender.send(AgentUpdate::Event(ConversationEvent::Interrupted {
            message_id: message_id.to_owned(),
            execution_duration_ms: None,
        }));
        return false;
    }
    finalize_codex_tool_activities(
        store,
        thread_id,
        message_id,
        activities,
        ToolCallStatus::Failed,
        sender,
    );
    finish_codex_message(store, message_id, assistant_text, "done");
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::MessageFinished {
        message_id: message_id.to_owned(),
        stop_reason: ConversationStopReason::End,
        execution_duration_ms: None,
    }));
    true
}

struct CodexToolActivity {
    call_id: String,
    identity: CodexAcpActivityIdentity,
    status: ToolCallStatus,
    permission_pending: bool,
    selected_by_user: bool,
    observed_in_progress: bool,
}

enum CodexPermissionResolution {
    Selected {
        external_id: String,
        kind: Option<String>,
    },
    TimedOut {
        external_id: String,
    },
}

fn ensure_codex_tool_activity(
    store: &Store,
    thread_id: &str,
    session_id: &str,
    message_id: &str,
    raw_tool_call: &Value,
    activities: &mut HashMap<String, CodexToolActivity>,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) -> Option<String> {
    let acp_tool_call_id = raw_tool_call
        .get("toolCallId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))?
        .to_owned();
    let external_id = format!("{thread_id}\0{session_id}\0{acp_tool_call_id}");
    if activities.contains_key(&external_id) {
        return Some(external_id);
    }
    let identity = codex_tool_identity(raw_tool_call)?;
    let call_id = ulid::Ulid::generate().to_string();
    let input_json = serde_json::json!({
        "kind": identity.kind,
        "arguments_bytes": identity.arguments_bytes,
        "arguments_sha256": identity.arguments_sha256,
        "argument_preview": identity.argument_preview,
    })
    .to_string();
    let seq = vega_store::tool_calls::next_seq(store.conn(), thread_id).ok()?;
    let created_at = unix_millis()?;
    vega_store::tool_calls::insert_pending(
        store.conn(),
        vega_store::tool_calls::NewToolCall {
            id: &call_id,
            thread_id,
            message_id,
            seq,
            tool: "codex_acp",
            input_json: &input_json,
            status: "pending_approval",
            created_at,
        },
    )
    .ok()?;
    let call = ToolCall {
        id: call_id.clone(),
        tool: "codex_acp".into(),
        input_json,
    };
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::ToolCallProposed {
        call,
    }));
    activities.insert(
        external_id.clone(),
        CodexToolActivity {
            call_id,
            identity,
            status: ToolCallStatus::PendingApproval,
            permission_pending: false,
            selected_by_user: false,
            observed_in_progress: false,
        },
    );
    Some(external_id)
}

fn codex_tool_identity(raw_tool_call: &Value) -> Option<CodexAcpActivityIdentity> {
    let raw_input = raw_tool_call
        .get("rawInput")
        .or_else(|| raw_tool_call.get("input"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let raw_bytes = serde_json::to_vec(&raw_input).ok()?;
    if raw_bytes.len() > 256 * 1024 {
        return None;
    }
    let kind = raw_tool_call
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| {
            matches!(
                *kind,
                "read"
                    | "edit"
                    | "delete"
                    | "move"
                    | "search"
                    | "execute"
                    | "think"
                    | "fetch"
                    | "other"
            )
        })
        .unwrap_or("other");
    let identity = CodexAcpActivityIdentity {
        kind: kind.to_owned(),
        arguments_bytes: raw_bytes.len().try_into().ok()?,
        arguments_sha256: format!("{:x}", Sha256::digest(&raw_bytes)),
        argument_preview: match raw_input {
            Value::Object(object) => format!("object with {} fields", object.len()),
            Value::Array(values) => format!("array with {} items", values.len()),
            Value::String(_) => "string value".into(),
            Value::Number(_) => "number value".into(),
            Value::Bool(_) => "boolean value".into(),
            Value::Null => "null value".into(),
        },
    };
    identity.is_valid().then_some(identity)
}

fn codex_permission_request(
    request: &AcpPermissionRequest,
    activity: &CodexToolActivity,
) -> PermissionRequest {
    PermissionRequest {
        call_id: activity.call_id.clone(),
        tool: "codex_acp".into(),
        display_target: activity.identity.permission_target(),
        danger_rule_id: None,
        danger_reason: None,
        external: None,
        acp_options: Some(
            request
                .options
                .iter()
                .map(|option| PermissionOptionChoice {
                    option_id: option.option_id.clone(),
                    name: option.name.clone(),
                    kind: option.kind.clone(),
                })
                .collect(),
        ),
    }
}

fn apply_codex_permission_resolution(
    store: &Store,
    resolution: CodexPermissionResolution,
    activities: &mut HashMap<String, CodexToolActivity>,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    match resolution {
        CodexPermissionResolution::Selected { external_id, kind } => {
            let Some(activity) = activities.get_mut(&external_id) else {
                return;
            };
            activity.permission_pending = false;
            match codex_permission_action(kind.as_deref()) {
                Some(CodexPermissionAction::Reject) => reject_codex_tool(
                    store,
                    activity,
                    ApprovalSource::User,
                    "Codex tool permission rejected",
                    sender,
                ),
                Some(CodexPermissionAction::Allow) => {
                    activity.selected_by_user = true;
                    if activity.observed_in_progress {
                        start_codex_tool(store, activity, sender);
                    }
                }
                None => reject_codex_tool(
                    store,
                    activity,
                    ApprovalSource::Timeout,
                    "Codex tool permission option was invalid",
                    sender,
                ),
            }
        }
        CodexPermissionResolution::TimedOut { external_id } => {
            if let Some(activity) = activities.get_mut(&external_id) {
                activity.permission_pending = false;
                reject_codex_tool(
                    store,
                    activity,
                    ApprovalSource::Timeout,
                    "Codex tool permission request ended",
                    sender,
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodexPermissionAction {
    Allow,
    Reject,
}

fn codex_permission_action(kind: Option<&str>) -> Option<CodexPermissionAction> {
    match kind? {
        "allow_once" | "allow_always" => Some(CodexPermissionAction::Allow),
        "reject_once" | "reject_always" => Some(CodexPermissionAction::Reject),
        _ => None,
    }
}

fn apply_codex_tool_status(
    store: &Store,
    external_id: &str,
    status: Option<&str>,
    activities: &mut HashMap<String, CodexToolActivity>,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    let Some(activity) = activities.get_mut(external_id) else {
        return;
    };
    match status {
        Some("in_progress") => {
            activity.observed_in_progress = true;
            if !activity.permission_pending {
                start_codex_tool(store, activity, sender);
            }
        }
        Some("completed") => finish_codex_tool(
            store,
            activity,
            ToolCallStatus::Success,
            "Codex completed tool activity",
            sender,
        ),
        Some("failed") => finish_codex_tool(
            store,
            activity,
            ToolCallStatus::Failed,
            "Codex tool activity failed",
            sender,
        ),
        Some("cancelled") => finish_codex_tool(
            store,
            activity,
            ToolCallStatus::Cancelled,
            "Codex tool activity cancelled",
            sender,
        ),
        _ => {}
    }
}

fn start_codex_tool(
    store: &Store,
    activity: &mut CodexToolActivity,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    if activity.status == ToolCallStatus::PendingApproval {
        let source = if activity.selected_by_user {
            ApprovalSource::User
        } else {
            ApprovalSource::Auto
        };
        let audit = ApprovalAudit {
            decision: Approval::Once,
            note: None,
            source,
            danger: None,
        };
        let Ok(approval_json) = audit.to_json() else {
            return;
        };
        if vega_store::tool_calls::approve(
            store.conn(),
            &activity.call_id,
            &approval_json,
            None,
            unix_millis().unwrap_or_default(),
        )
        .is_err()
        {
            return;
        }
        activity.status = ToolCallStatus::Approved;
        let _ = sender.send(AgentUpdate::Event(ConversationEvent::ToolCallApproved {
            call_id: activity.call_id.clone(),
            approval: Approval::Once,
        }));
    }
    if activity.status == ToolCallStatus::Approved
        && vega_store::tool_calls::mark_running(store.conn(), &activity.call_id).is_ok()
    {
        activity.status = ToolCallStatus::Running;
        let _ = sender.send(AgentUpdate::Event(ConversationEvent::ToolCallRunning {
            call_id: activity.call_id.clone(),
        }));
    }
}

fn reject_codex_tool(
    store: &Store,
    activity: &mut CodexToolActivity,
    source: ApprovalSource,
    output: &str,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    if activity.status != ToolCallStatus::PendingApproval {
        return;
    }
    let audit = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source,
        danger: None,
    };
    let Ok(approval_json) = audit.to_json() else {
        return;
    };
    if vega_store::tool_calls::reject(
        store.conn(),
        &activity.call_id,
        &approval_json,
        output,
        unix_millis().unwrap_or_default(),
        None,
    )
    .is_err()
    {
        return;
    }
    activity.status = ToolCallStatus::Rejected;
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::ToolCallFinished {
        call_id: activity.call_id.clone(),
        result: codex_tool_result(ToolCallStatus::Rejected, output),
    }));
}

fn finish_codex_tool(
    store: &Store,
    activity: &mut CodexToolActivity,
    status: ToolCallStatus,
    output: &str,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    if activity.status == ToolCallStatus::PendingApproval {
        if activity.permission_pending {
            return;
        }
        start_codex_tool(store, activity, sender);
    }
    if activity.status == ToolCallStatus::Approved {
        start_codex_tool(store, activity, sender);
    }
    if activity.status != ToolCallStatus::Running {
        return;
    }
    if vega_store::tool_calls::finish(
        store.conn(),
        vega_store::tool_calls::FinishToolCall {
            id: &activity.call_id,
            status: match status {
                ToolCallStatus::Success => "success",
                ToolCallStatus::Cancelled => "cancelled",
                _ => "failed",
            },
            output_text: output,
            exit_code: None,
            duration_ms: None,
            finished_at: unix_millis().unwrap_or_default(),
        },
    )
    .is_ok()
    {
        activity.status = status;
        let _ = sender.send(AgentUpdate::Event(ConversationEvent::ToolCallFinished {
            call_id: activity.call_id.clone(),
            result: codex_tool_result(status, output),
        }));
    }
}

fn finalize_codex_tool_activities(
    store: &Store,
    thread_id: &str,
    message_id: &str,
    activities: &mut HashMap<String, CodexToolActivity>,
    terminal: ToolCallStatus,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    for activity in activities.values_mut() {
        if activity.status == ToolCallStatus::PendingApproval {
            if activity.permission_pending {
                activity.permission_pending = false;
                reject_codex_tool(
                    store,
                    activity,
                    ApprovalSource::Timeout,
                    "Codex stopped before permission completed",
                    sender,
                );
            } else {
                finish_codex_tool(
                    store,
                    activity,
                    terminal,
                    "Codex ended before tool activity completed",
                    sender,
                );
            }
        } else if matches!(
            activity.status,
            ToolCallStatus::Approved | ToolCallStatus::Running
        ) {
            finish_codex_tool(
                store,
                activity,
                terminal,
                "Codex ended before tool activity completed",
                sender,
            );
        }
    }
    let _ = (thread_id, message_id);
}

fn codex_tool_result(status: ToolCallStatus, output: &str) -> ToolResult {
    ToolResult {
        status,
        output: output.to_owned(),
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: (status == ToolCallStatus::Success).then_some(false),
        invalid: None,
    }
}

fn unix_millis() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()
}

fn persist_codex_messages(
    store: &Store,
    thread: &Thread,
    prompt: &str,
) -> Option<(String, String, i64)> {
    let user_message_id = ulid::Ulid::generate().to_string();
    let assistant_message_id = ulid::Ulid::generate().to_string();
    let transaction = store.conn().unchecked_transaction().ok()?;
    let user_seq = vega_store::messages::next_seq(&transaction, &thread.id).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()?;
    vega_store::messages::insert(
        &transaction,
        &vega_store::messages::MessageRow {
            id: user_message_id.clone(),
            thread_id: thread.id.clone(),
            seq: user_seq,
            role: "user".into(),
            kind: "text".into(),
            content: prompt.to_owned(),
            status: "done".into(),
            created_at: now,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .ok()?;
    let assistant_seq = vega_store::messages::next_seq(&transaction, &thread.id).ok()?;
    vega_store::messages::insert(
        &transaction,
        &vega_store::messages::MessageRow {
            id: assistant_message_id.clone(),
            thread_id: thread.id.clone(),
            seq: assistant_seq,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: now,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .ok()?;
    transaction.commit().ok()?;
    Some((user_message_id, assistant_message_id, assistant_seq))
}

fn finish_codex_message(store: &Store, message_id: &str, content: &str, status: &str) {
    let _ = vega_store::messages::finish_streaming(store.conn(), message_id, content, status, None);
}

fn send_codex_error(sender: &std_mpsc::SyncSender<AgentUpdate>, message_id: &str) {
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::Error {
        message_id: Some(message_id.to_string()),
        error: Arc::new(vega_runtime::VegaError::Io(std::io::Error::other(
            "Codex ACP run failed",
        ))),
        execution_duration_ms: None,
    }));
}

#[cfg(test)]
mod tests;
