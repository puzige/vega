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
use vega_conversation::codex_persistence::{
    CodexSessionOutcome, CodexToolChange, CodexToolProposal,
};

mod writes;
use writes::CodexWrites;

#[cfg(test)]
#[path = "codex_agent/behavior_tests.rs"]
mod behavior_tests;

#[derive(Clone, Copy)]
pub(crate) enum CodexPreflightFailure {
    ProfileUnavailable,
    InvalidProfile,
    ExecutableUnavailable,
    WorkspaceUnavailable,
}

impl CodexPreflightFailure {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::ProfileUnavailable => "app.codex.profile_missing",
            Self::InvalidProfile => "app.codex.profile_invalid",
            Self::ExecutableUnavailable => "app.codex.executable_unavailable",
            Self::WorkspaceUnavailable => "app.codex.workspace_unavailable",
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
    let store = Store::open_read_only(database_path)
        .map_err(|_| CodexPreflightFailure::WorkspaceUnavailable)?;
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
    pub persistence: vega_conversation::codex_persistence::CodexPersistence,
    pub thread: Thread,
    pub snapshot: CodexExecutionSnapshot,
    pub prompt: String,
    pub permission_queue: PermissionQueue,
    pub cancel: CancellationToken,
    pub sender: std_mpsc::SyncSender<AgentUpdate>,
}

pub(crate) fn run_codex_agent_worker(request: CodexWorkerRequest) -> bool {
    let store = CodexWrites::from_startup(request.persistence);
    if request.cancel.is_cancelled() {
        return false;
    }
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
            let _ = store.session(CodexSessionOutcome::DefinitivelyFailed {
                code: CodexSessionFailureCode::ProcessStartFailed,
            });
            return false;
        }
    };
    if request.cancel.is_cancelled() {
        return false;
    }
    if store.validate_external_start().is_err() || request.cancel.is_cancelled() {
        return false;
    }
    store.mark_external_started();
    let connection = match runtime.block_on(Connection::spawn(launch)) {
        Ok(connection) => Arc::new(connection),
        Err(_) => {
            let _ = store.session(CodexSessionOutcome::DefinitivelyFailed {
                code: CodexSessionFailureCode::ProcessStartFailed,
            });
            return false;
        }
    };
    let success = runtime.block_on(run_codex_agent_with_connection(CodexAgentRunContext {
        store: &store,
        connection: connection.clone(),
        thread: &request.thread,
        snapshot: &request.snapshot,
        prompt: &request.prompt,
        permission_queue: &request.permission_queue,
        cancel: request.cancel,
        sender: &request.sender,
    }));
    runtime.block_on(connection.shutdown());
    success && store.healthy()
}

pub(crate) struct CodexAgentRunContext<'a> {
    store: &'a CodexWrites,
    connection: Arc<Connection>,
    thread: &'a Thread,
    snapshot: &'a CodexExecutionSnapshot,
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
        prompt,
        permission_queue,
        cancel,
        sender,
    } = context;
    let fail_intent = |code| {
        let _ = store.session(CodexSessionOutcome::DefinitivelyFailed { code });
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
            let _ = store.session(CodexSessionOutcome::Uncertain { code: CodexSessionUncertaintyCode::OutcomeUnknown });
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
            let _ = store.session(CodexSessionOutcome::Uncertain {
                code: CodexSessionUncertaintyCode::OutcomeUnknown,
            });
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
            let _ = store.session(CodexSessionOutcome::Uncertain { code: CodexSessionUncertaintyCode::OutcomeUnknown });
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
            let _ = store.session(CodexSessionOutcome::Uncertain {
                code: CodexSessionUncertaintyCode::OutcomeUnknown,
            });
            return false;
        }
    }
    if store
        .session(CodexSessionOutcome::Confirmed {
            session_id: session.session_id.clone(),
        })
        .is_err()
    {
        let _ = connection.cancel(&session.session_id).await;
        return false;
    }
    let _ = sender.send(AgentUpdate::CodexModeConfirmed);
    if cancel.is_cancelled() {
        let _ = connection.cancel(&session.session_id).await;
        return false;
    }
    let Some((user_message_id, assistant_message_id, assistant_seq, user_sent_at_ms)) =
        persist_codex_messages(store, thread, prompt)
    else {
        return false;
    };
    let run_started_at = std::time::Instant::now();
    if sender
        .send(AgentUpdate::Event(ConversationEvent::UserMessageAccepted {
            message_id: user_message_id.clone(),
            seq: assistant_seq.saturating_sub(1) as u64,
        }))
        .is_err()
    {
        return false;
    }
    for (message_id, sent_at_ms) in [
        (user_message_id.clone(), Some(user_sent_at_ms)),
        (assistant_message_id.clone(), None),
    ] {
        if sender
            .send(AgentUpdate::Event(ConversationEvent::MessageTimestamp {
                message_id,
                sent_at_ms,
            }))
            .is_err()
        {
            return false;
        }
    }
    if sender
        .send(AgentUpdate::Event(ConversationEvent::MessageStarted {
            message_id: assistant_message_id.clone(),
            seq: assistant_seq as u64,
        }))
        .is_err()
    {
        return false;
    }
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::RunStarted {
        message_id: assistant_message_id.clone(),
        started_at: run_started_at,
        work_clock: None,
    }));
    let prompt_handle = match connection.prompt(&session.session_id, prompt).await {
        Ok(handle) => handle,
        Err(_) => {
            let duration_ms =
                finish_codex_message(store, &assistant_message_id, "", "failed", run_started_at);
            send_codex_error(sender, &assistant_message_id, duration_ms);
            return false;
        }
    };
    let mut prompt_wait = Box::pin(prompt_handle.wait());
    let mut assistant_text = String::new();
    let mut cancel_deadline = None;
    let mut tool_activities = HashMap::new();
    let (permission_sender, mut permission_receiver) = tokio_mpsc::unbounded_channel();
    let permission_cancel = cancel.child_token();
    let mut permission_tasks = tokio::task::JoinSet::new();
    loop {
        if !store.healthy() {
            permission_cancel.cancel();
            permission_queue.timeout_active();
            let _ = connection.cancel(&session.session_id).await;
            settle_codex_permissions(
                store,
                &permission_cancel,
                permission_queue,
                &mut permission_tasks,
                &mut permission_receiver,
                &mut tool_activities,
                sender,
            )
            .await;
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
                run_started_at,
            );
            send_codex_error(
                sender,
                &assistant_message_id,
                u64::try_from(run_started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
            );
            return false;
        }
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
                settle_codex_permissions(store, &permission_cancel, permission_queue, &mut permission_tasks, &mut permission_receiver, &mut tool_activities, sender).await;
                finalize_codex_tool_activities(
                    store,
                    &thread.id,
                    &assistant_message_id,
                    &mut tool_activities,
                    ToolCallStatus::Cancelled,
                    sender,
                );
                let duration_ms = finish_codex_message(store, &assistant_message_id, &assistant_text, "interrupted", run_started_at);
                let _ = sender.send(AgentUpdate::Event(ConversationEvent::Interrupted {
                    message_id: assistant_message_id.clone(),
                    execution_duration_ms: Some(duration_ms),
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
                            if store.stream(&assistant_message_id, &assistant_text).is_err() {
                                continue;
                            }
                            let _ = sender.send(AgentUpdate::Event(ConversationEvent::TextDelta {
                                message_id: assistant_message_id.clone(),
                                delta: delta.to_string(),
                            }));
                        }
                        if params.pointer("/update/sessionUpdate").and_then(Value::as_str) == Some("agent_thought_chunk")
                            && params.pointer("/update/content/type").and_then(Value::as_str) == Some("text")
                            && let Some(delta) = params.pointer("/update/content/text").and_then(Value::as_str)
                        {
                            let _ = sender.send(AgentUpdate::Event(ConversationEvent::ThinkingDelta {
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
                            settle_codex_permissions(store, &permission_cancel, permission_queue, &mut permission_tasks, &mut permission_receiver, &mut tool_activities, sender).await;
                            finalize_codex_tool_activities(
                                store,
                                &thread.id,
                                &assistant_message_id,
                                &mut tool_activities,
                                ToolCallStatus::Failed,
                                sender,
                            );
                            let duration_ms = finish_codex_message(store, &assistant_message_id, &assistant_text, "failed", run_started_at);
                            send_codex_error(sender, &assistant_message_id, duration_ms);
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
                            settle_codex_permissions(store, &permission_cancel, permission_queue, &mut permission_tasks, &mut permission_receiver, &mut tool_activities, sender).await;
                            finalize_codex_tool_activities(
                                store,
                                &thread.id,
                                &assistant_message_id,
                                &mut tool_activities,
                                ToolCallStatus::Failed,
                                sender,
                            );
                            let duration_ms = finish_codex_message(
                                store,
                                &assistant_message_id,
                                &assistant_text,
                                "failed",
                                run_started_at,
                            );
                            send_codex_error(sender, &assistant_message_id, duration_ms);
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
                        permission_tasks.spawn(async move {
                            let selected = select_codex_permission(&queue, permission, cancel.clone()).await;
                            let resolved = match selected {
                                Some((option_id, kind))
                                    if connection
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
                            settle_codex_permissions(store, &permission_cancel, permission_queue, &mut permission_tasks, &mut permission_receiver, &mut tool_activities, sender).await;
                            return finish_codex_prompt_result(
                                result,
                                CodexPromptFinishContext {
                                    store,
                                    thread_id: &thread.id,
                                    message_id: &assistant_message_id,
                                    assistant_text: &assistant_text,
                                    run_started_at,
                                    activities: &mut tool_activities,
                                    permission_cancel: &permission_cancel,
                                    permission_queue,
                                    sender,
                                },
                            );
                        }
                        permission_cancel.cancel();
                        permission_queue.timeout_active();
                        settle_codex_permissions(store, &permission_cancel, permission_queue, &mut permission_tasks, &mut permission_receiver, &mut tool_activities, sender).await;
                        finalize_codex_tool_activities(
                            store,
                            &thread.id,
                            &assistant_message_id,
                            &mut tool_activities,
                            ToolCallStatus::Failed,
                            sender,
                        );
                        let duration_ms = finish_codex_message(store, &assistant_message_id, &assistant_text, "failed", run_started_at);
                        send_codex_error(sender, &assistant_message_id, duration_ms);
                        return false;
                    }
                }
            }
            result = &mut prompt_wait => {
                let Ok(result) = result else {
                    permission_cancel.cancel();
                    permission_queue.timeout_active();
                    settle_codex_permissions(store, &permission_cancel, permission_queue, &mut permission_tasks, &mut permission_receiver, &mut tool_activities, sender).await;
                    finalize_codex_tool_activities(
                        store,
                        &thread.id,
                        &assistant_message_id,
                        &mut tool_activities,
                        ToolCallStatus::Failed,
                        sender,
                    );
                    let duration_ms = finish_codex_message(store, &assistant_message_id, &assistant_text, "failed", run_started_at);
                    send_codex_error(sender, &assistant_message_id, duration_ms);
                    return false;
                };
                settle_codex_permissions(store, &permission_cancel, permission_queue, &mut permission_tasks, &mut permission_receiver, &mut tool_activities, sender).await;
                return finish_codex_prompt_result(
                    result,
                    CodexPromptFinishContext {
                        store,
                        thread_id: &thread.id,
                        message_id: &assistant_message_id,
                        assistant_text: &assistant_text,
                        run_started_at,
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

async fn select_codex_permission(
    queue: &PermissionQueue,
    permission: PermissionRequest,
    cancel: CancellationToken,
) -> Option<(String, Option<String>)> {
    let options = permission.acp_options.clone()?;
    let PermissionDecision::AcpOption { option_id } =
        queue.request(permission, cancel).await.ok()?
    else {
        return None;
    };
    let option = options
        .into_iter()
        .find(|option| option.option_id == option_id)?;
    codex_permission_action(option.kind.as_deref())?;
    Some((option_id, option.kind))
}

async fn settle_codex_permissions(
    store: &CodexWrites,
    cancel: &CancellationToken,
    queue: &PermissionQueue,
    tasks: &mut tokio::task::JoinSet<()>,
    receiver: &mut tokio_mpsc::UnboundedReceiver<CodexPermissionResolution>,
    activities: &mut HashMap<String, CodexToolActivity>,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    cancel.cancel();
    queue.timeout_active();
    while let Some(result) = tasks.join_next().await {
        if result.is_err() {
            store.fail("Codex permission resolution task failed");
        }
    }
    while let Ok(resolution) = receiver.try_recv() {
        apply_codex_permission_resolution(store, resolution, activities, sender);
    }
}

struct CodexPromptFinishContext<'a> {
    store: &'a CodexWrites,
    thread_id: &'a str,
    message_id: &'a str,
    assistant_text: &'a str,
    run_started_at: std::time::Instant,
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
        run_started_at,
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
        let duration_ms = finish_codex_message(
            store,
            message_id,
            assistant_text,
            "interrupted",
            run_started_at,
        );
        let _ = sender.send(AgentUpdate::Event(ConversationEvent::Interrupted {
            message_id: message_id.to_owned(),
            execution_duration_ms: Some(duration_ms),
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
    if result.stop_reason != "end_turn" || !store.healthy() {
        let duration_ms =
            finish_codex_message(store, message_id, assistant_text, "failed", run_started_at);
        send_codex_error(sender, message_id, duration_ms);
        return false;
    }
    let duration_ms =
        finish_codex_message(store, message_id, assistant_text, "done", run_started_at);
    if !store.healthy() {
        send_codex_error(sender, message_id, duration_ms);
        return false;
    }
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::MessageFinished {
        message_id: message_id.to_owned(),
        stop_reason: ConversationStopReason::End,
        execution_duration_ms: Some(duration_ms),
    }));
    true
}

struct CodexToolActivity {
    call_id: String,
    message_id: String,
    observed_terminal: Option<(ToolCallStatus, CodexTerminalReason)>,
    persistence_uncertain: bool,
    identity: CodexAcpActivityIdentity,
    preview: CodexArgumentPreview,
    status: ToolCallStatus,
    permission_pending: bool,
    selected_by_user: bool,
    rejected_by_user: bool,
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
    store: &CodexWrites,
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
    if let Some(activity) = activities.get(&external_id) {
        if raw_tool_call.get("rawInput").is_some() || raw_tool_call.get("input").is_some() {
            let (identity, preview) = codex_tool_identity(raw_tool_call)?;
            if identity != activity.identity || preview != activity.preview {
                store.fail("Codex repeated tool identity changed");
                return None;
            }
        }
        if raw_tool_call
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != activity.identity.kind)
        {
            store.fail("Codex repeated tool kind changed");
            return None;
        }
        return Some(external_id);
    }
    let (identity, preview) = codex_tool_identity(raw_tool_call)?;
    let call_id = ulid::Ulid::generate().to_string();
    let input_json = serde_json::json!({
        "kind": identity.kind,
        "arguments_bytes": identity.arguments_bytes,
        "arguments_sha256": identity.arguments_sha256,
        "argument_preview": identity.argument_preview,
    })
    .to_string();
    store
        .propose(CodexToolProposal {
            call_id: call_id.clone(),
            message_id: message_id.into(),
            identity: identity.clone(),
            preview,
            created_at: unix_millis()?,
        })
        .ok()?;
    let call = ToolCall {
        id: call_id.clone(),
        tool: "codex_acp".into(),
        input_json,
        description: None,
        codex_display: Some(CodexDisplayMetadata {
            preview,
            reason: None,
        }),
    };
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::ToolCallProposed {
        call,
    }));
    activities.insert(
        external_id.clone(),
        CodexToolActivity {
            call_id,
            message_id: message_id.into(),
            observed_terminal: None,
            persistence_uncertain: false,
            identity,
            preview,
            status: ToolCallStatus::PendingApproval,
            permission_pending: false,
            selected_by_user: false,
            rejected_by_user: false,
            observed_in_progress: false,
        },
    );
    Some(external_id)
}

fn codex_tool_identity(
    raw_tool_call: &Value,
) -> Option<(CodexAcpActivityIdentity, CodexArgumentPreview)> {
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
    let preview = match raw_input {
        Value::Object(object) => CodexArgumentPreview::Object {
            count: object.len().try_into().ok()?,
        },
        Value::Array(values) => CodexArgumentPreview::Array {
            count: values.len().try_into().ok()?,
        },
        Value::String(_) => CodexArgumentPreview::String,
        Value::Number(_) => CodexArgumentPreview::Number,
        Value::Bool(_) => CodexArgumentPreview::Bool,
        Value::Null => CodexArgumentPreview::Null,
    };
    let identity = CodexAcpActivityIdentity {
        kind: kind.to_owned(),
        arguments_bytes: raw_bytes.len().try_into().ok()?,
        arguments_sha256: format!("{:x}", Sha256::digest(&raw_bytes)),
        argument_preview: preview.canonical(),
    };
    identity.is_valid().then_some((identity, preview))
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
    store: &CodexWrites,
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
            activity.rejected_by_user =
                codex_permission_action(kind.as_deref()) == Some(CodexPermissionAction::Reject);
            if let Some((status, reason)) = activity.observed_terminal {
                activity.selected_by_user =
                    codex_permission_action(kind.as_deref()) == Some(CodexPermissionAction::Allow);
                finish_codex_tool(store, activity, status, reason, sender);
                return;
            }
            match codex_permission_action(kind.as_deref()) {
                Some(CodexPermissionAction::Reject) => reject_codex_tool(
                    store,
                    activity,
                    ApprovalSource::User,
                    CodexTerminalReason::PermissionRejected,
                    sender,
                ),
                Some(CodexPermissionAction::Allow) => {
                    activity.selected_by_user = true;
                    if let Some((status, reason)) = activity.observed_terminal {
                        finish_codex_tool(store, activity, status, reason, sender);
                    } else if activity.observed_in_progress {
                        start_codex_tool(store, activity, sender);
                    }
                }
                None => reject_codex_tool(
                    store,
                    activity,
                    ApprovalSource::Timeout,
                    CodexTerminalReason::PermissionInvalid,
                    sender,
                ),
            }
        }
        CodexPermissionResolution::TimedOut { external_id } => {
            if let Some(activity) = activities.get_mut(&external_id) {
                activity.permission_pending = false;
                if let Some((status, reason)) = activity.observed_terminal {
                    finish_codex_tool(store, activity, status, reason, sender);
                    return;
                }
                reject_codex_tool(
                    store,
                    activity,
                    ApprovalSource::Timeout,
                    CodexTerminalReason::PermissionEnded,
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
    store: &CodexWrites,
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
            CodexTerminalReason::ToolCompleted,
            sender,
        ),
        Some("failed") => finish_codex_tool(
            store,
            activity,
            ToolCallStatus::Failed,
            CodexTerminalReason::ToolFailed,
            sender,
        ),
        Some("cancelled") => finish_codex_tool(
            store,
            activity,
            ToolCallStatus::Cancelled,
            CodexTerminalReason::ToolCancelled,
            sender,
        ),
        _ => {}
    }
}

fn start_codex_tool(
    store: &CodexWrites,
    activity: &mut CodexToolActivity,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    if activity.persistence_uncertain
        || activity.status != ToolCallStatus::PendingApproval
        || !activity.observed_in_progress
    {
        return;
    }
    if store
        .tool(
            activity,
            CodexToolChange::Start {
                selected_by_user: activity.selected_by_user,
                observed_in_progress: activity.observed_in_progress,
            },
        )
        .is_err()
    {
        activity.persistence_uncertain = true;
        return;
    }
    activity.status = ToolCallStatus::Running;
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::ToolCallApproved {
        call_id: activity.call_id.clone(),
        approval: Approval::Once,
    }));
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::ToolCallRunning {
        call_id: activity.call_id.clone(),
        bash_started_at: None,
    }));
}

fn reject_codex_tool(
    store: &CodexWrites,
    activity: &mut CodexToolActivity,
    source: ApprovalSource,
    reason: CodexTerminalReason,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    if activity.persistence_uncertain || activity.status != ToolCallStatus::PendingApproval {
        return;
    }
    if store
        .tool(activity, CodexToolChange::Reject { source, reason })
        .is_err()
    {
        activity.persistence_uncertain = true;
        return;
    }
    activity.status = ToolCallStatus::Rejected;
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::ToolCallFinished {
        call_id: activity.call_id.clone(),
        result: codex_tool_result(activity, ToolCallStatus::Rejected, reason),
    }));
}

fn finish_codex_tool(
    store: &CodexWrites,
    activity: &mut CodexToolActivity,
    status: ToolCallStatus,
    reason: CodexTerminalReason,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    if reason != CodexTerminalReason::ToolIncomplete {
        activity.observed_terminal = Some((status, reason));
    }
    if activity.persistence_uncertain
        || activity.permission_pending
        || !matches!(
            activity.status,
            ToolCallStatus::PendingApproval | ToolCallStatus::Running
        )
    {
        return;
    }
    if activity.status == ToolCallStatus::PendingApproval
        && reason == CodexTerminalReason::ToolIncomplete
    {
        reject_codex_tool(
            store,
            activity,
            ApprovalSource::Timeout,
            CodexTerminalReason::PermissionIncomplete,
            sender,
        );
        return;
    }
    if store
        .tool(
            activity,
            CodexToolChange::Finish {
                status,
                reason,
                selected_by_user: activity.selected_by_user,
                rejected_by_user: activity.rejected_by_user,
            },
        )
        .is_ok()
    {
        activity.status = status;
        let _ = sender.send(AgentUpdate::Event(ConversationEvent::ToolCallFinished {
            call_id: activity.call_id.clone(),
            result: codex_tool_result(activity, status, reason),
        }));
    } else {
        activity.persistence_uncertain = true;
    }
}

fn finalize_codex_tool_activities(
    store: &CodexWrites,
    thread_id: &str,
    message_id: &str,
    activities: &mut HashMap<String, CodexToolActivity>,
    terminal: ToolCallStatus,
    sender: &std_mpsc::SyncSender<AgentUpdate>,
) {
    for activity in activities.values_mut() {
        if activity.persistence_uncertain {
            continue;
        }
        if let Some((status, reason)) = activity.observed_terminal
            && activity.status == ToolCallStatus::PendingApproval
        {
            activity.permission_pending = false;
            finish_codex_tool(store, activity, status, reason, sender);
            continue;
        }
        if activity.status == ToolCallStatus::PendingApproval {
            if activity.permission_pending {
                activity.permission_pending = false;
                reject_codex_tool(
                    store,
                    activity,
                    ApprovalSource::Timeout,
                    CodexTerminalReason::PermissionIncomplete,
                    sender,
                );
            } else {
                finish_codex_tool(
                    store,
                    activity,
                    terminal,
                    CodexTerminalReason::ToolIncomplete,
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
                CodexTerminalReason::ToolIncomplete,
                sender,
            );
        }
    }
    let _ = (thread_id, message_id);
}

fn codex_tool_result(
    activity: &CodexToolActivity,
    status: ToolCallStatus,
    reason: CodexTerminalReason,
) -> ToolResult {
    ToolResult {
        status,
        output: reason.canonical_output().to_owned(),
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: (status == ToolCallStatus::Success).then_some(false),
        invalid: None,
        codex_display: Some(CodexDisplayMetadata {
            preview: activity.preview,
            reason: Some(reason),
        }),
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
    store: &CodexWrites,
    thread: &Thread,
    prompt: &str,
) -> Option<(String, String, i64, i64)> {
    let _ = thread;
    let receipt = store.messages(prompt).ok()?;
    Some((
        receipt.user_message_id,
        receipt.assistant_message_id,
        receipt.assistant_seq,
        receipt.created_at,
    ))
}

fn finish_codex_message(
    store: &CodexWrites,
    message_id: &str,
    content: &str,
    status: &str,
    started_at: std::time::Instant,
) -> u64 {
    let duration_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    let stored_duration = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let _ = store.finish(message_id, content, status, stored_duration);
    duration_ms
}

fn send_codex_error(
    sender: &std_mpsc::SyncSender<AgentUpdate>,
    message_id: &str,
    duration_ms: u64,
) {
    let _ = sender.send(AgentUpdate::Event(ConversationEvent::Error {
        message_id: Some(message_id.to_string()),
        error: Arc::new(vega_runtime::VegaError::Io(std::io::Error::other(
            "Codex ACP run failed",
        ))),
        execution_duration_ms: Some(duration_ms),
    }));
}
