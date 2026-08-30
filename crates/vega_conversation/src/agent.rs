//! Conversation-layer orchestration for the headless runtime (S4-T20).

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::FutureExt;
use futures::future::BoxFuture;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    AgentRequest, Provider, RuntimeEvent, RuntimeExactRule, RuntimeMutatingTool,
    RuntimePermissionHook, RuntimePermissionMode, RuntimeRunMode, RuntimeToolConfig,
    RuntimeToolStatus, RuntimeUserDecision, VegaError, run_agent_with_permission_sink,
};
use vega_store::{Store, messages, permissions, token_usage, tool_calls};

use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, ConversationError, ConversationEvent,
    PermissionDecision, PermissionRequest, ThreadMode, approval_audit_from_runtime,
    approval_audit_to_runtime, from_runtime_event, permission_decision_to_runtime,
    permission_request_from_runtime,
};

const HISTORY_WINDOW: usize = 50;
const TEXT_BATCH_MAX_DELAY: Duration = Duration::from_millis(4);
const TEXT_BATCH_MAX_BYTES: usize = 4 * 1024;
const PERSISTENCE_CHANNEL_CAPACITY: usize = 64;

/// Shared cancellable permission boundary implemented by the S5 UI.
pub trait PermissionHook: Send + Sync {
    /// Requests one content-free permission decision.
    fn request(
        &self,
        request: PermissionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>>;
}

struct RejectPermissionHook;

impl PermissionHook for RejectPermissionHook {
    fn request(
        &self,
        _request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        async { Ok(PermissionDecision::Timeout) }.boxed()
    }
}

struct RuntimePermissionAdapter<'a> {
    shared: &'a dyn PermissionHook,
}

impl RuntimePermissionHook for RuntimePermissionAdapter<'_> {
    fn request(
        &self,
        prompt: vega_runtime::RuntimePermissionPrompt,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.shared
            .request(permission_request_from_runtime(&prompt), cancel)
            .map(|decision| decision.map(permission_decision_to_runtime))
            .boxed()
    }
}

#[derive(Clone, Default)]
struct PersistenceActorConfig {
    #[cfg(test)]
    snapshot_writes: Option<Arc<AtomicUsize>>,
    #[cfg(test)]
    command_delay: Option<Duration>,
    #[cfg(test)]
    fail_event: Option<InjectedPersistenceFailure>,
    #[cfg(test)]
    preparation_delay: Option<Duration>,
    #[cfg(test)]
    preparation_query_only: bool,
    #[cfg(test)]
    actor_query_only: bool,
    #[cfg(test)]
    fail_start: bool,
    #[cfg(test)]
    checkpoint_root: Option<PathBuf>,
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum InjectedPersistenceFailure {
    Running,
    Finished,
    PanicRunning,
}

impl PersistenceActorConfig {
    fn delay_command(&self) {
        #[cfg(test)]
        if let Some(delay) = self.command_delay {
            std::thread::sleep(delay);
        }
    }

    fn record_snapshot(&self) {
        #[cfg(test)]
        if let Some(writes) = &self.snapshot_writes {
            writes.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn check_event(&self, event: &RuntimeEvent) -> Result<(), VegaError> {
        #[cfg(test)]
        {
            let matches_failure = matches!(
                (self.fail_event, event),
                (
                    Some(InjectedPersistenceFailure::Running),
                    RuntimeEvent::ToolCallRunning { .. }
                ) | (
                    Some(InjectedPersistenceFailure::Finished),
                    RuntimeEvent::ToolCallFinished(_)
                )
            );
            if matches_failure {
                return Err(persistence_actor_error(
                    "injected critical persistence failure",
                ));
            }
            if matches!(
                (self.fail_event, event),
                (
                    Some(InjectedPersistenceFailure::PanicRunning),
                    RuntimeEvent::ToolCallRunning { .. }
                )
            ) {
                panic!("injected persistence actor panic");
            }
        }
        let _ = event;
        Ok(())
    }
}

enum PersistenceCommand {
    Snapshot {
        content: String,
        ack: oneshot::Sender<Result<(), VegaError>>,
    },
    Event {
        event: Box<RuntimeEvent>,
        content: String,
        ack: oneshot::Sender<Result<(), VegaError>>,
    },
}

struct PersistenceActor {
    sender: mpsc::Sender<PersistenceCommand>,
    task: tokio::task::JoinHandle<Result<(), VegaError>>,
}

impl PersistenceActor {
    async fn start(
        database_path: PathBuf,
        project_id: String,
        thread_id: String,
        message_id: String,
        model: String,
        next_tool_seq: i64,
        config: PersistenceActorConfig,
    ) -> Result<Self, VegaError> {
        let (sender, mut receiver) = mpsc::channel(PERSISTENCE_CHANNEL_CAPACITY);
        let (ready, opened) = oneshot::channel();
        let task = tokio::task::spawn_blocking(move || {
            let store = match Store::open(database_path) {
                Ok(store) => store,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return Err(VegaError::Store(error));
                }
            };
            #[cfg(test)]
            if config.actor_query_only
                && let Err(error) = store.conn().execute_batch("PRAGMA query_only = ON")
            {
                let _ = ready.send(Err(error.to_string()));
                return Err(VegaError::Store(error));
            }
            #[cfg(test)]
            if config.fail_start {
                let error = persistence_actor_error("injected startup failure");
                let _ = ready.send(Err(error.to_string()));
                return Err(error);
            }
            let _ = ready.send(Ok(()));
            let mut next_tool_seq = next_tool_seq;
            while let Some(command) = receiver.blocking_recv() {
                match command {
                    PersistenceCommand::Snapshot { content, ack } => {
                        config.delay_command();
                        let result =
                            messages::update_streaming_content(store.conn(), &message_id, &content)
                                .map_err(VegaError::from)
                                .and_then(|updated| ensure_message_updated(updated, &message_id));
                        if result.is_ok() {
                            config.record_snapshot();
                        }
                        let _ = ack.send(result);
                    }
                    PersistenceCommand::Event {
                        event,
                        content,
                        ack,
                    } => {
                        config.delay_command();
                        let result = config.check_event(&event).and_then(|()| {
                            persist_runtime_event(
                                &store,
                                &project_id,
                                &thread_id,
                                &message_id,
                                &model,
                                &content,
                                &mut next_tool_seq,
                                &event,
                            )
                        });
                        let _ = ack.send(result);
                    }
                }
            }
            Ok(())
        });
        match opened.await {
            Ok(Ok(())) => {}
            Ok(Err(display)) => {
                return match task.await {
                    Ok(Err(error)) => Err(error),
                    Ok(Ok(())) => Err(persistence_actor_error(display)),
                    Err(error) => Err(persistence_actor_error(format!(
                        "DB actor join failed after open error: {error}"
                    ))),
                };
            }
            Err(_) => {
                return match task.await {
                    Ok(Err(error)) => Err(error),
                    Ok(Ok(())) => Err(persistence_actor_error(
                        "DB actor dropped startup acknowledgement",
                    )),
                    Err(error) => Err(persistence_actor_error(format!(
                        "DB actor startup join failed: {error}"
                    ))),
                };
            }
        }
        Ok(Self { sender, task })
    }

    async fn snapshot(&self, content: String) -> Result<(), VegaError> {
        let (ack, received) = oneshot::channel();
        self.sender
            .send(PersistenceCommand::Snapshot { content, ack })
            .await
            .map_err(|_| persistence_actor_error("DB actor stopped before snapshot"))?;
        received
            .await
            .map_err(|_| persistence_actor_error("DB actor dropped snapshot acknowledgement"))?
    }

    async fn event(&self, event: RuntimeEvent, content: String) -> Result<(), VegaError> {
        let (ack, received) = oneshot::channel();
        self.sender
            .send(PersistenceCommand::Event {
                event: Box::new(event),
                content,
                ack,
            })
            .await
            .map_err(|_| persistence_actor_error("DB actor stopped before event persistence"))?;
        received
            .await
            .map_err(|_| persistence_actor_error("DB actor dropped event acknowledgement"))?
    }

    async fn close(self) -> Result<(), VegaError> {
        drop(self.sender);
        self.task
            .await
            .map_err(|error| persistence_actor_error(format!("DB actor join failed: {error}")))?
    }
}

struct RuntimeEnvelope {
    event: RuntimeEvent,
    ack: Option<oneshot::Sender<Result<(), VegaError>>>,
}

struct PreparedRun {
    database_path: PathBuf,
    project_id: String,
    model: String,
    user_message_id: String,
    assistant_message_id: String,
    assistant_seq: i64,
    request: AgentRequest,
    next_tool_seq: i64,
}

fn persistence_actor_error(message: impl Into<String>) -> VegaError {
    VegaError::Io(std::io::Error::other(format!(
        "persistence actor: {}",
        message.into()
    )))
}

fn runtime_event_requires_ack(event: &RuntimeEvent) -> bool {
    !matches!(
        event,
        RuntimeEvent::TextDelta(_)
            | RuntimeEvent::ThinkingDelta(_)
            | RuntimeEvent::ToolCallOutput { .. }
    )
}

/// Persisted task result and the sole event stream exposed to UI/store
/// consumers.
#[derive(Debug)]
pub struct ConversationRun {
    /// User message id.
    pub user_message_id: String,
    /// Assistant message id.
    pub assistant_message_id: String,
    /// Ordered conversation events.
    pub events: Vec<ConversationEvent>,
    /// Final visible assistant Markdown.
    pub content: String,
    /// Whether cancellation interrupted the message.
    pub interrupted: bool,
    /// Whether a provider/runtime error failed the message.
    pub failed: bool,
}

/// Persists a user turn, drives the runtime, converts every runtime event,
/// and audits messages/tool calls/usage in the existing six-table schema.
pub async fn run_thread_task(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    user_content: &str,
    system_prompt: &str,
    cancel: CancellationToken,
) -> Result<ConversationRun, ConversationError> {
    run_thread_task_with_permission_sink(
        store,
        provider,
        tools,
        thread_id,
        user_content,
        system_prompt,
        cancel,
        &RejectPermissionHook,
        |_| Ok(()),
    )
    .await
}

/// Runs a thread task while forwarding each shared event at the actual
/// runtime boundary. Critical persistence completes before `event_sink` is
/// invoked; returning an error from the sink stops the task.
#[allow(clippy::too_many_arguments)]
pub async fn run_thread_task_with_sink<F>(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    user_content: &str,
    system_prompt: &str,
    cancel: CancellationToken,
    event_sink: F,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    run_thread_task_with_permission_sink(
        store,
        provider,
        tools,
        thread_id,
        user_content,
        system_prompt,
        cancel,
        &RejectPermissionHook,
        event_sink,
    )
    .await
}

/// Runs a thread task with the shared S5 permission hook and event sink.
#[allow(clippy::too_many_arguments)]
pub async fn run_thread_task_with_permission_sink<F>(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    user_content: &str,
    system_prompt: &str,
    cancel: CancellationToken,
    permission_hook: &dyn PermissionHook,
    event_sink: F,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    run_thread_task_with_permission_config(
        store,
        provider,
        tools,
        thread_id,
        user_content,
        system_prompt,
        cancel,
        permission_hook,
        event_sink,
        PersistenceActorConfig::default(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
async fn run_thread_task_with_sink_config<F>(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    user_content: &str,
    system_prompt: &str,
    cancel: CancellationToken,
    event_sink: F,
    actor_config: PersistenceActorConfig,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    run_thread_task_with_permission_config(
        store,
        provider,
        tools,
        thread_id,
        user_content,
        system_prompt,
        cancel,
        &RejectPermissionHook,
        event_sink,
        actor_config,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_thread_task_with_permission_config<F>(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    user_content: &str,
    system_prompt: &str,
    cancel: CancellationToken,
    permission_hook: &dyn PermissionHook,
    mut event_sink: F,
    actor_config: PersistenceActorConfig,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    let user_message_id = ulid::Ulid::generate().to_string();
    let assistant_message_id = ulid::Ulid::generate().to_string();
    let database_path = match store.database_path() {
        Some(path) => path.to_path_buf(),
        None => {
            let error = Arc::new(VegaError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "live persistence requires a file-backed SQLite store",
            )));
            forward_pipeline_error(&mut event_sink, None, error.clone());
            return Err(ConversationError::Runtime(error));
        }
    };

    let preparation_path = database_path.clone();
    let preparation_thread_id = thread_id.to_string();
    let preparation_user_content = user_content.to_string();
    let preparation_system_prompt = system_prompt.to_string();
    let preparation_user_id = user_message_id.clone();
    let preparation_assistant_id = assistant_message_id.clone();
    let preparation_config = actor_config.clone();
    let prepared = match tokio::task::spawn_blocking(move || {
        prepare_run(
            preparation_path,
            preparation_thread_id,
            preparation_user_content,
            preparation_system_prompt,
            preparation_user_id,
            preparation_assistant_id,
            preparation_config,
        )
    })
    .await
    {
        Ok(Ok(prepared)) => prepared,
        Ok(Err(ConversationError::Runtime(error))) => {
            forward_pipeline_error(&mut event_sink, None, error.clone());
            return Err(ConversationError::Runtime(error));
        }
        Ok(Err(error)) => return Err(error),
        Err(error) => {
            let error = Arc::new(persistence_actor_error(format!(
                "preparation task join failed: {error}"
            )));
            forward_pipeline_error(&mut event_sink, None, error.clone());
            return Err(ConversationError::Runtime(error));
        }
    };

    let actor = match PersistenceActor::start(
        prepared.database_path.clone(),
        prepared.project_id.clone(),
        thread_id.to_string(),
        prepared.assistant_message_id.clone(),
        prepared.model.clone(),
        prepared.next_tool_seq,
        actor_config,
    )
    .await
    {
        Ok(actor) => actor,
        Err(error) => {
            let error = Arc::new(error);
            let _ = finish_prepared_failure(
                prepared.database_path.clone(),
                prepared.assistant_message_id.clone(),
            )
            .await;
            forward_pipeline_error(
                &mut event_sink,
                Some(prepared.assistant_message_id.clone()),
                error.clone(),
            );
            return Err(ConversationError::Runtime(error));
        }
    };

    let started = ConversationEvent::MessageStarted {
        message_id: prepared.assistant_message_id.clone(),
        seq: prepared.assistant_seq as u64,
    };
    if let Err(error) = event_sink(&started) {
        let error = Arc::new(error);
        let _ = actor
            .event(RuntimeEvent::Error(error.clone()), String::new())
            .await;
        forward_pipeline_error(
            &mut event_sink,
            Some(prepared.assistant_message_id.clone()),
            error.clone(),
        );
        let _ = actor.close().await;
        return Err(ConversationError::Runtime(error));
    }
    let mut events = vec![started];

    let (runtime_sender, runtime_receiver) = mpsc::channel(PERSISTENCE_CHANNEL_CAPACITY);
    let task_cancel = cancel.child_token();
    let processor_cancel = task_cancel.clone();
    let mut streamed_content = String::new();
    let permission_adapter = RuntimePermissionAdapter {
        shared: permission_hook,
    };
    let runtime_future = run_agent_with_permission_sink(
        provider,
        tools,
        prepared.request,
        task_cancel,
        &permission_adapter,
        move |event| {
            let sender = runtime_sender.clone();
            async move {
                if runtime_event_requires_ack(&event) {
                    let (ack, received) = oneshot::channel();
                    sender
                        .send(RuntimeEnvelope {
                            event,
                            ack: Some(ack),
                        })
                        .await
                        .map_err(|_| persistence_actor_error("event processor stopped"))?;
                    received
                        .await
                        .map_err(|_| persistence_actor_error("event acknowledgement was dropped"))?
                } else {
                    sender
                        .send(RuntimeEnvelope { event, ack: None })
                        .await
                        .map_err(|_| persistence_actor_error("event processor stopped"))
                }
            }
        },
    );
    let processor_future = process_runtime_events(
        runtime_receiver,
        &actor,
        &prepared.assistant_message_id,
        &mut streamed_content,
        &mut events,
        &mut event_sink,
        processor_cancel,
    );
    let (runtime_result, processor_result) = tokio::join!(runtime_future, processor_future);
    let outcome = match (processor_result, runtime_result) {
        (Ok(()), Ok(outcome)) => {
            if let Err(error) = actor.close().await {
                let error = Arc::new(error);
                forward_pipeline_error(
                    &mut event_sink,
                    Some(prepared.assistant_message_id.clone()),
                    error.clone(),
                );
                return Err(ConversationError::Runtime(error));
            }
            outcome
        }
        (processor, runtime) => {
            let error = processor
                .err()
                .or_else(|| runtime.err())
                .unwrap_or_else(|| persistence_actor_error("agent event pipeline stopped"));
            let error = Arc::new(error);
            let failure_event = RuntimeEvent::Error(error.clone());
            let _ = actor.event(failure_event, streamed_content.clone()).await;
            forward_pipeline_error(
                &mut event_sink,
                Some(prepared.assistant_message_id.clone()),
                error.clone(),
            );
            let _ = actor.close().await;
            return Err(ConversationError::Runtime(error));
        }
    };
    Ok(ConversationRun {
        user_message_id: prepared.user_message_id,
        assistant_message_id: prepared.assistant_message_id,
        events,
        content: outcome.final_text,
        interrupted: outcome.interrupted,
        failed: outcome.failed,
    })
}

#[allow(clippy::too_many_arguments)]
fn prepare_run(
    database_path: PathBuf,
    thread_id: String,
    user_content: String,
    system_prompt: String,
    user_message_id: String,
    assistant_message_id: String,
    config: PersistenceActorConfig,
) -> Result<PreparedRun, ConversationError> {
    #[cfg(not(test))]
    let _ = &config;
    #[cfg(test)]
    if let Some(delay) = config.preparation_delay {
        std::thread::sleep(delay);
    }

    let store = Store::open(&database_path).map_err(runtime_store_error)?;
    #[cfg(test)]
    if config.preparation_query_only {
        store
            .conn()
            .execute_batch("PRAGMA query_only = ON")
            .map_err(runtime_store_error)?;
    }
    vega_store::recovery::recover_thread(store.conn(), &thread_id, now_ms())
        .map_err(runtime_store_error)?;
    let transaction = store
        .conn()
        .unchecked_transaction()
        .map_err(runtime_store_error)?;
    let thread = vega_store::threads::find(&transaction, &thread_id)
        .map_err(runtime_store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.clone()))?;
    let run_mode = ThreadMode::parse(&thread.mode)
        .ok_or_else(|| ConversationError::CorruptRow(format!("run mode: {}", thread.mode)))?;
    let permission_mode =
        crate::types::PermissionMode::parse(&thread.permission_mode).ok_or_else(|| {
            ConversationError::CorruptRow(format!("permission_mode: {}", thread.permission_mode))
        })?;
    #[cfg(test)]
    let checkpoint_root = config.checkpoint_root.clone().unwrap_or_else(|| {
        database_path
            .parent()
            .map_or_else(PathBuf::new, |parent| parent.join("checkpoints"))
    });
    #[cfg(not(test))]
    let checkpoint_root = database_path
        .parent()
        .ok_or_else(|| ConversationError::CorruptRow("database path has no parent".to_string()))?
        .join("checkpoints");
    if checkpoint_root.as_os_str().is_empty() {
        return Err(ConversationError::CorruptRow(
            "database path has no parent".to_string(),
        ));
    }
    if run_mode == ThreadMode::Execute {
        fs::create_dir_all(&checkpoint_root).map_err(|_| {
            ConversationError::Runtime(Arc::new(VegaError::Io(std::io::Error::other(
                "checkpoint root unavailable",
            ))))
        })?;
    }
    let exact_rules = permissions::list_exact(&transaction, &thread.project_id)
        .map_err(|error| runtime_store_error(std::io::Error::other(error.to_string())))?
        .into_iter()
        .map(|rule| {
            if rule.pattern.is_empty() {
                return Err(ConversationError::CorruptRow(
                    "permission rule has empty exact pattern".to_string(),
                ));
            }
            let tool = match rule.tool.as_str() {
                "bash" => RuntimeMutatingTool::Bash,
                "write" => RuntimeMutatingTool::Write,
                "edit" => RuntimeMutatingTool::Edit,
                _ => {
                    return Err(ConversationError::CorruptRow(
                        "permission rule has unsupported tool".to_string(),
                    ));
                }
            };
            Ok(RuntimeExactRule {
                tool,
                pattern: rule.pattern,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let user_seq = messages::next_seq(&transaction, &thread_id).map_err(runtime_store_error)?;
    let now = now_ms();
    messages::insert(
        &transaction,
        &messages::MessageRow {
            id: user_message_id.clone(),
            thread_id: thread_id.clone(),
            seq: user_seq,
            role: "user".to_string(),
            kind: "text".to_string(),
            content: user_content,
            status: "done".to_string(),
            created_at: now,
        },
    )
    .map_err(runtime_store_error)?;
    let assistant_seq = user_seq + 1;
    messages::insert(
        &transaction,
        &messages::MessageRow {
            id: assistant_message_id.clone(),
            thread_id: thread_id.clone(),
            seq: assistant_seq,
            role: "assistant".to_string(),
            kind: "text".to_string(),
            content: String::new(),
            status: "streaming".to_string(),
            created_at: now,
        },
    )
    .map_err(runtime_store_error)?;

    let history = messages::recent(&transaction, &thread_id, HISTORY_WINDOW)
        .map_err(runtime_store_error)?
        .into_iter()
        .filter_map(|message| {
            let role = match message.role.as_str() {
                "user" => Some(vega_runtime::ChatRole::User),
                "assistant" => Some(vega_runtime::ChatRole::Assistant),
                _ => None,
            }?;
            Some(vega_runtime::ChatMessage::new(role, message.content))
        })
        .collect();
    let completed_tool_results = tool_calls::terminal_results(&transaction, &thread_id)
        .map_err(|error| runtime_store_error(std::io::Error::other(error.to_string())))?
        .into_iter()
        .map(|(call_id, call)| -> Result<_, ConversationError> {
            let approval = ApprovalAudit::from_json(&call.approval).map_err(|_| {
                ConversationError::CorruptRow(format!(
                    "terminal tool call {call_id} has invalid approval"
                ))
            })?;
            let status = match call.status.as_str() {
                "success" => RuntimeToolStatus::Success,
                "failed" => RuntimeToolStatus::Failed,
                "rejected" => RuntimeToolStatus::Rejected,
                "cancelled" => RuntimeToolStatus::Cancelled,
                other => {
                    return Err(ConversationError::CorruptRow(format!(
                        "terminal tool call {call_id} has status {other}"
                    )));
                }
            };
            let canonical_input = validate_recovered_projection(
                &thread.project_id,
                &thread_id,
                &call_id,
                &call.tool,
                &call.input_json,
                &call.output,
                status,
                &approval,
                call.exit_code,
                call.duration_ms,
            )?;
            let completed = vega_runtime::CompletedToolCall {
                tool: call.tool,
                input_json: canonical_input,
                result: vega_runtime::RuntimeToolResult {
                    call_id: call_id.clone(),
                    output: call.output,
                    status,
                    reused: true,
                    exit_code: call.exit_code,
                    duration_ms: call.duration_ms,
                    truncated: None,
                    approval: Some(approval_audit_to_runtime(&approval)),
                    remember_rule: None,
                },
            };
            Ok((call_id, completed))
        })
        .collect::<Result<_, _>>()?;
    let next_tool_seq =
        tool_calls::next_seq(&transaction, &thread_id).map_err(runtime_store_error)?;
    let foreign_call_ids =
        tool_calls::foreign_call_ids(&transaction, &thread_id).map_err(runtime_store_error)?;
    transaction.commit().map_err(runtime_store_error)?;

    Ok(PreparedRun {
        database_path,
        project_id: thread.project_id.clone(),
        model: thread.model.clone(),
        user_message_id,
        assistant_message_id,
        assistant_seq,
        request: AgentRequest {
            model: thread.model,
            system_prompt,
            history,
            max_tokens: None,
            completed_tool_results,
            tool_config: RuntimeToolConfig::new(
                match run_mode {
                    ThreadMode::Ask => RuntimeRunMode::Ask,
                    ThreadMode::Plan => RuntimeRunMode::Plan,
                    ThreadMode::Execute => RuntimeRunMode::Execute,
                },
                match permission_mode {
                    crate::types::PermissionMode::ReadOnly => RuntimePermissionMode::ReadOnly,
                    crate::types::PermissionMode::Confirm => RuntimePermissionMode::Confirm,
                    crate::types::PermissionMode::Auto => RuntimePermissionMode::Auto,
                },
                thread.project_id,
                thread_id,
                checkpoint_root,
                exact_rules,
            )
            .with_foreign_call_ids(foreign_call_ids),
        },
        next_tool_seq,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_recovered_projection(
    project_id: &str,
    thread_id: &str,
    call_id: &str,
    tool: &str,
    input_json: &str,
    output: &str,
    status: RuntimeToolStatus,
    approval: &ApprovalAudit,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
) -> Result<String, ConversationError> {
    let corrupt = || {
        ConversationError::CorruptRow(format!(
            "terminal tool call {call_id} has invalid safe projection"
        ))
    };
    match tool {
        "write" | "edit" => {
            if exit_code.is_some() || duration_ms.is_some() {
                return Err(corrupt());
            }
            if let Ok(valid) = vega_tools::WriteEditAudit::from_json(input_json) {
                if valid.tool().as_str() != tool
                    || approval.source == ApprovalSource::Validation
                    || !approval_source_matches(tool, status, approval.source, false)
                {
                    return Err(corrupt());
                }
                let decision_valid = match status {
                    RuntimeToolStatus::Rejected => {
                        approval.decision == Approval::Deny
                            && (approval.source != ApprovalSource::Recovery
                                || output == vega_store::recovery::RECOVERY_REJECTED_OUTPUT)
                    }
                    RuntimeToolStatus::Success
                    | RuntimeToolStatus::Failed
                    | RuntimeToolStatus::Cancelled => approval.decision != Approval::Deny,
                };
                if !decision_valid {
                    return Err(corrupt());
                }
                let output_valid = match status {
                    RuntimeToolStatus::Success if tool == "write" => {
                        mutation_success_matches(&valid, project_id, thread_id, call_id, output)
                    }
                    RuntimeToolStatus::Success => {
                        mutation_success_matches(&valid, project_id, thread_id, call_id, output)
                    }
                    RuntimeToolStatus::Failed => {
                        output == format!("Tool error: {tool} failed")
                            || output == "Tool error: tool worker failed"
                            || output == "Tool error: invalid mutation result"
                    }
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::RunMode => {
                        output == "Tool error: denied by run mode"
                    }
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::Recovery => {
                        output == vega_store::recovery::RECOVERY_REJECTED_OUTPUT
                    }
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::Legacy => {
                        output == legacy_unavailable_output(tool)
                            || output == "Tool error: permission denied"
                    }
                    RuntimeToolStatus::Rejected => output == "Tool error: permission denied",
                    RuntimeToolStatus::Cancelled
                        if output == vega_store::recovery::RECOVERY_CANCELLED_OUTPUT =>
                    {
                        true
                    }
                    RuntimeToolStatus::Cancelled
                        if output == vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT =>
                    {
                        true
                    }
                    RuntimeToolStatus::Cancelled if tool == "write" => {
                        mutation_success_matches(&valid, project_id, thread_id, call_id, output)
                            || output == "Tool error: write failed"
                            || output == "Tool error: tool worker failed"
                    }
                    RuntimeToolStatus::Cancelled => {
                        mutation_success_matches(&valid, project_id, thread_id, call_id, output)
                            || output == "Tool error: edit failed"
                            || output == "Tool error: tool worker failed"
                    }
                };
                if !output_valid {
                    return Err(corrupt());
                }
                return valid.to_json().map_err(|_| corrupt());
            }
            if let Ok(invalid) = vega_tools::InvalidWriteEditAudit::from_json(input_json) {
                let expected = format!(
                    "Tool error: invalid {tool} input ({})",
                    invalid.validation_error_code().as_str()
                );
                if invalid.tool().as_str() != tool
                    || status != RuntimeToolStatus::Rejected
                    || approval.decision != Approval::Deny
                    || approval.source != ApprovalSource::Validation
                    || output != expected
                {
                    return Err(corrupt());
                }
                return invalid.to_json().map_err(|_| corrupt());
            }
            Err(corrupt())
        }
        "read" | "glob" | "grep" | "bash" => {
            if !approval_source_matches(tool, status, approval.source, false) {
                return Err(corrupt());
            }
            if tool == "bash" && !bash_danger_audit_matches(input_json, approval) {
                return Err(corrupt());
            }
            let decision_valid = match status {
                RuntimeToolStatus::Rejected => approval.decision == Approval::Deny,
                RuntimeToolStatus::Success
                | RuntimeToolStatus::Failed
                | RuntimeToolStatus::Cancelled => approval.decision != Approval::Deny,
            };
            if !decision_valid || (tool != "bash" && (exit_code.is_some() || duration_ms.is_some()))
            {
                return Err(corrupt());
            }
            if tool != "bash"
                && status == RuntimeToolStatus::Rejected
                && !matches!(
                    (approval.source, output),
                    (
                        ApprovalSource::Recovery,
                        vega_store::recovery::RECOVERY_REJECTED_OUTPUT
                    ) | (ApprovalSource::Timeout, "Tool error: permission denied")
                )
            {
                return Err(corrupt());
            }
            if tool == "bash" {
                let metadata_valid = match status {
                    RuntimeToolStatus::Success => exit_code.is_some() && duration_ms.is_some(),
                    RuntimeToolStatus::Failed
                    | RuntimeToolStatus::Rejected
                    | RuntimeToolStatus::Cancelled => exit_code.is_none() && duration_ms.is_none(),
                };
                let output_valid = match status {
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::RunMode => {
                        output == "Tool error: denied by run mode"
                    }
                    RuntimeToolStatus::Rejected
                        if approval.source == ApprovalSource::Validation =>
                    {
                        output == "Tool error: invalid bash input (invalid_input)"
                    }
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::Recovery => {
                        output == vega_store::recovery::RECOVERY_REJECTED_OUTPUT
                    }
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::Legacy => {
                        output == legacy_unavailable_output(tool)
                            || output == "Tool error: permission denied"
                    }
                    RuntimeToolStatus::Rejected => output == "Tool error: permission denied",
                    RuntimeToolStatus::Failed => is_safe_bash_failure(output),
                    RuntimeToolStatus::Cancelled => {
                        output == "Tool error: bash failed (cancelled)"
                            || output == vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT
                            || output == vega_store::recovery::RECOVERY_CANCELLED_OUTPUT
                    }
                    RuntimeToolStatus::Success => true,
                };
                if !metadata_valid || !output_valid {
                    return Err(corrupt());
                }
            }
            Ok(input_json.to_string())
        }
        _ if status == RuntimeToolStatus::Rejected
            && approval.decision == Approval::Deny
            && ((approval.source == ApprovalSource::RunMode
                && output == "Tool error: denied: unavailable tool")
                || (approval.source == ApprovalSource::Recovery
                    && output == vega_store::recovery::RECOVERY_REJECTED_OUTPUT)
                || (approval.source == ApprovalSource::Legacy
                    && output == legacy_unavailable_output(tool)))
            && input_json == "{}"
            && exit_code.is_none()
            && duration_ms.is_none() =>
        {
            Ok(input_json.to_string())
        }
        _ => Err(corrupt()),
    }
}

fn mutation_success_matches(
    audit: &vega_tools::WriteEditAudit,
    project_id: &str,
    thread_id: &str,
    call_id: &str,
    output: &str,
) -> bool {
    let Ok(ids) = vega_tools::CheckpointIds::new(project_id, thread_id, call_id) else {
        return false;
    };
    let expected_ref = ids.checkpoint_ref();
    match audit {
        vega_tools::WriteEditAudit::Write {
            path,
            content_bytes,
            ..
        } => vega_tools::WriteSuccessOutput::from_json(output)
            .ok()
            .is_some_and(|success| {
                success.path == *path
                    && success.bytes_written == *content_bytes
                    && success.checkpoint_ref == expected_ref
            }),
        vega_tools::WriteEditAudit::Edit { path, .. } => {
            vega_tools::EditSuccessOutput::from_json(output)
                .ok()
                .is_some_and(|success| {
                    success.path == *path && success.checkpoint_ref == expected_ref
                })
        }
    }
}

fn is_safe_bash_failure(output: &str) -> bool {
    const CODES: [&str; 9] = [
        "scope_mismatch",
        "hardlink_preflight",
        "sandbox_unavailable",
        "temp_unavailable",
        "cleanup_failed",
        "spawn_failed",
        "process_control_failed",
        "output_failed",
        "timed_out",
    ];
    CODES
        .iter()
        .any(|code| output == format!("Tool error: bash failed ({code})"))
}

fn legacy_unavailable_output(tool: &str) -> String {
    format!("Tool error: denied: tool '{tool}' is unavailable until the S5 permission gate")
}

fn bash_danger_audit_matches(input_json: &str, approval: &ApprovalAudit) -> bool {
    let Ok(command) = vega_tools::bash_permission_signature(input_json) else {
        return matches!(
            approval.source,
            ApprovalSource::Validation
                | ApprovalSource::RunMode
                | ApprovalSource::Recovery
                | ApprovalSource::Legacy
        ) && approval.danger.is_none();
    };
    let Ok(danger) = vega_tools::danger::detect_danger(&command) else {
        return false;
    };
    let Some(danger) = danger else {
        return approval.source != ApprovalSource::Danger && approval.danger.is_none();
    };
    if matches!(
        approval.source,
        ApprovalSource::RunMode | ApprovalSource::Recovery
    ) {
        return approval.danger.is_none();
    }
    if approval.source == ApprovalSource::Legacy {
        return approval.decision == Approval::Deny && approval.danger.is_none();
    }
    let Some(audit) = &approval.danger else {
        return false;
    };
    if audit.rule_id != danger.rule_id {
        return false;
    }
    match approval.source {
        ApprovalSource::Danger => {
            approval.decision == audit.decision && approval.note == audit.note
        }
        ApprovalSource::ReadOnly => {
            approval.decision == Approval::Deny
                && approval.note.is_none()
                && matches!(audit.decision, Approval::Once | Approval::Always)
        }
        ApprovalSource::Timeout => {
            approval.decision == Approval::Deny
                && approval.note.is_none()
                && audit.decision == Approval::Deny
                && audit.note.is_none()
        }
        ApprovalSource::RunMode
        | ApprovalSource::Rule
        | ApprovalSource::Auto
        | ApprovalSource::User
        | ApprovalSource::Validation
        | ApprovalSource::ReadonlyTool
        | ApprovalSource::Recovery
        | ApprovalSource::Legacy => false,
    }
}

fn approval_source_matches(
    tool: &str,
    status: RuntimeToolStatus,
    source: ApprovalSource,
    invalid_projection: bool,
) -> bool {
    if invalid_projection {
        return status == RuntimeToolStatus::Rejected && source == ApprovalSource::Validation;
    }
    match tool {
        "read" | "glob" | "grep" => {
            (status == RuntimeToolStatus::Rejected
                && matches!(source, ApprovalSource::Recovery | ApprovalSource::Timeout))
                || (status != RuntimeToolStatus::Rejected
                    && matches!(
                        source,
                        ApprovalSource::ReadonlyTool | ApprovalSource::Legacy
                    ))
        }
        "write" | "edit" => match status {
            RuntimeToolStatus::Rejected => matches!(
                source,
                ApprovalSource::RunMode
                    | ApprovalSource::ReadOnly
                    | ApprovalSource::User
                    | ApprovalSource::Timeout
                    | ApprovalSource::Legacy
                    | ApprovalSource::Recovery
            ),
            RuntimeToolStatus::Success
            | RuntimeToolStatus::Failed
            | RuntimeToolStatus::Cancelled => matches!(
                source,
                ApprovalSource::User
                    | ApprovalSource::Rule
                    | ApprovalSource::Auto
                    | ApprovalSource::Legacy
            ),
        },
        "bash" => match status {
            RuntimeToolStatus::Rejected => matches!(
                source,
                ApprovalSource::RunMode
                    | ApprovalSource::Validation
                    | ApprovalSource::Danger
                    | ApprovalSource::ReadOnly
                    | ApprovalSource::User
                    | ApprovalSource::Timeout
                    | ApprovalSource::Legacy
                    | ApprovalSource::Recovery
            ),
            RuntimeToolStatus::Success
            | RuntimeToolStatus::Failed
            | RuntimeToolStatus::Cancelled => matches!(
                source,
                ApprovalSource::User
                    | ApprovalSource::Rule
                    | ApprovalSource::Auto
                    | ApprovalSource::Danger
                    | ApprovalSource::Legacy
            ),
        },
        _ => status == RuntimeToolStatus::Rejected && source == ApprovalSource::RunMode,
    }
}

async fn finish_prepared_failure(
    database_path: PathBuf,
    assistant_message_id: String,
) -> Result<(), VegaError> {
    tokio::task::spawn_blocking(move || {
        let store = Store::open(database_path).map_err(VegaError::Store)?;
        messages::finish_streaming(store.conn(), &assistant_message_id, "", "failed")
            .map_err(VegaError::Store)
            .and_then(|updated| ensure_message_updated(updated, &assistant_message_id))
    })
    .await
    .map_err(|error| persistence_actor_error(format!("failure cleanup join failed: {error}")))?
}

fn forward_pipeline_error<F>(event_sink: &mut F, message_id: Option<String>, error: Arc<VegaError>)
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    let _ = event_sink(&ConversationEvent::Error { message_id, error });
}

async fn process_runtime_events<F>(
    mut receiver: mpsc::Receiver<RuntimeEnvelope>,
    actor: &PersistenceActor,
    message_id: &str,
    streamed_content: &mut String,
    events: &mut Vec<ConversationEvent>,
    event_sink: &mut F,
    cancel: CancellationToken,
) -> Result<(), VegaError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    let mut pending_text = Vec::new();
    let mut pending_text_bytes = 0usize;
    let mut batch_deadline = None;

    loop {
        let received = if let Some(deadline) = batch_deadline {
            match tokio::time::timeout_at(deadline, receiver.recv()).await {
                Ok(envelope) => envelope,
                Err(_) => {
                    if let Err(error) = flush_text_batch(
                        actor,
                        streamed_content,
                        &mut pending_text,
                        events,
                        event_sink,
                    )
                    .await
                    {
                        cancel.cancel();
                        return Err(error);
                    }
                    batch_deadline = None;
                    pending_text_bytes = 0;
                    continue;
                }
            }
        } else {
            receiver.recv().await
        };

        let Some(envelope) = received else {
            if let Err(error) = flush_text_batch(
                actor,
                streamed_content,
                &mut pending_text,
                events,
                event_sink,
            )
            .await
            {
                cancel.cancel();
                return Err(error);
            }
            return Ok(());
        };

        let RuntimeEnvelope { event, ack } = envelope;
        let result = if let RuntimeEvent::TextDelta(delta) = &event {
            streamed_content.push_str(delta);
            pending_text_bytes = pending_text_bytes.saturating_add(delta.len());
            if let Some(converted) = from_runtime_event(message_id, &event) {
                pending_text.push(converted);
            }
            if pending_text_bytes >= TEXT_BATCH_MAX_BYTES {
                let result = flush_text_batch(
                    actor,
                    streamed_content,
                    &mut pending_text,
                    events,
                    event_sink,
                )
                .await;
                pending_text_bytes = 0;
                batch_deadline = None;
                result
            } else {
                if batch_deadline.is_none() {
                    batch_deadline = Some(tokio::time::Instant::now() + TEXT_BATCH_MAX_DELAY);
                }
                Ok(())
            }
        } else {
            let flushed = flush_text_batch(
                actor,
                streamed_content,
                &mut pending_text,
                events,
                event_sink,
            )
            .await;
            batch_deadline = None;
            pending_text_bytes = 0;
            match flushed {
                Ok(()) if matches!(event, RuntimeEvent::ToolCallOutput { .. }) => Ok(()),
                Ok(()) => match actor.event(event.clone(), streamed_content.clone()).await {
                    Ok(()) => {
                        let terminal_output = match &event {
                            RuntimeEvent::ToolCallFinished(result)
                            | RuntimeEvent::ToolCallValidationRejected { result, .. }
                            | RuntimeEvent::ToolCallConflict { result, .. } => {
                                Some(ConversationEvent::ToolCallOutput {
                                    call_id: result.call_id.clone(),
                                    chunk: crate::types::ToolOutputChunk(result.output.clone()),
                                })
                            }
                            _ => None,
                        };
                        if let Some(output) = terminal_output {
                            event_sink(&output)?;
                            events.push(output);
                        }
                        if let Some(converted) = from_runtime_event(message_id, &event) {
                            event_sink(&converted)?;
                            events.push(converted);
                        }
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        };

        match result {
            Ok(()) => {
                if let Some(ack) = ack {
                    let _ = ack.send(Ok(()));
                }
            }
            Err(error) => {
                if let Some(ack) = ack {
                    let _ = ack.send(Err(persistence_actor_error(error.to_string())));
                }
                cancel.cancel();
                return Err(error);
            }
        }
    }
}

async fn flush_text_batch<F>(
    actor: &PersistenceActor,
    streamed_content: &str,
    pending_text: &mut Vec<ConversationEvent>,
    events: &mut Vec<ConversationEvent>,
    event_sink: &mut F,
) -> Result<(), VegaError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    if pending_text.is_empty() {
        return Ok(());
    }
    actor.snapshot(streamed_content.to_string()).await?;
    for event in pending_text.drain(..) {
        event_sink(&event)?;
        events.push(event);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn persist_runtime_event(
    store: &Store,
    project_id: &str,
    thread_id: &str,
    message_id: &str,
    model: &str,
    streamed_content: &str,
    next_tool_seq: &mut i64,
    event: &RuntimeEvent,
) -> Result<(), VegaError> {
    match event {
        RuntimeEvent::ToolCallProposed(call) => {
            validate_runtime_proposal(call)?;
            if let Some(existing) = tool_calls::find_identity(store.conn(), &call.id)? {
                if existing.thread_id != thread_id
                    || existing.tool != call.name
                    || !tool_inputs_semantically_equal(
                        &call.name,
                        &existing.input_json,
                        &call.input_json,
                    )
                {
                    return Err(VegaError::Tool {
                        tool: call.name.clone(),
                        message: format!(
                            "call id '{}' collides with persisted owner/tool/input",
                            call.id
                        ),
                    });
                }
            } else {
                tool_calls::insert_pending(
                    store.conn(),
                    tool_calls::NewToolCall {
                        id: &call.id,
                        thread_id,
                        message_id,
                        seq: *next_tool_seq,
                        tool: &call.name,
                        input_json: &call.input_json,
                        status: "pending_approval",
                        created_at: now_ms(),
                    },
                )
                .map_err(tool_transition_error)?;
                *next_tool_seq += 1;
            }
        }
        RuntimeEvent::ToolCallValidationRejected { call, result } => {
            validate_runtime_validation_event(call, result)?;
            if result.reused {
                let state = required_tool_state(store, &call.id, thread_id)?;
                validate_reused_terminal(project_id, thread_id, &state, result)?;
            }
            if !result.reused {
                let approval = result.approval.as_ref().ok_or_else(|| VegaError::Tool {
                    tool: call.name.clone(),
                    message: "validation rejection missing approval audit".to_string(),
                })?;
                let approval_json = approval_audit_from_runtime(approval)
                    .to_json()
                    .map_err(|_| safe_audit_error(&call.name))?;
                tool_calls::insert_validation_rejected(
                    store.conn(),
                    tool_calls::ValidationRejectedToolCall {
                        call: tool_calls::NewToolCall {
                            id: &call.id,
                            thread_id,
                            message_id,
                            seq: *next_tool_seq,
                            tool: &call.name,
                            input_json: &call.input_json,
                            status: "rejected",
                            created_at: now_ms(),
                        },
                        approval_json: &approval_json,
                        output_text: &result.output,
                        finished_at: now_ms(),
                    },
                )
                .map_err(tool_transition_error)?;
                *next_tool_seq += 1;
            }
        }
        RuntimeEvent::ToolCallConflict { call, result } => {
            validate_runtime_conflict_event(call, result)?;
            let existing = tool_calls::find_identity(store.conn(), &call.id)?.ok_or_else(|| {
                VegaError::Tool {
                    tool: "persistence".to_string(),
                    message: "call id conflict has no persisted identity".to_string(),
                }
            })?;
            let is_same_identity = existing.thread_id == thread_id
                && existing.tool == call.name
                && tool_inputs_semantically_equal(
                    &call.name,
                    &existing.input_json,
                    &call.input_json,
                );
            if is_same_identity {
                return Err(safe_audit_error(&call.name));
            }
        }
        RuntimeEvent::ToolCallApproved {
            call_id,
            audit,
            remember_rule,
        } => {
            let state = required_tool_state(store, call_id, thread_id)?;
            validate_runtime_approval_event(&state, call_id, audit, remember_rule.as_ref())?;
            let approval_json = approval_audit_from_runtime(audit)
                .to_json()
                .map_err(|_| safe_audit_error("permission"))?;
            let remember = remember_rule
                .as_ref()
                .map(|target| tool_calls::RememberExactRule {
                    project_id,
                    tool: target.tool.as_str(),
                    pattern: &target.exact_pattern,
                });
            tool_calls::approve(store.conn(), call_id, &approval_json, remember, now_ms())
                .map_err(tool_transition_error)?;
        }
        RuntimeEvent::ToolCallRunning { call_id } => {
            let state = required_tool_state(store, call_id, thread_id)?;
            if state.status != "approved" {
                return Err(safe_audit_error(&state.tool));
            }
            tool_calls::mark_running(store.conn(), call_id).map_err(tool_transition_error)?;
        }
        RuntimeEvent::ToolCallFinished(result) if result.reused => {
            let state = required_tool_state(store, &result.call_id, thread_id)?;
            validate_reused_terminal(project_id, thread_id, &state, result)?;
        }
        RuntimeEvent::ToolCallFinished(result) if !result.reused => {
            let state = required_tool_state(store, &result.call_id, thread_id)?;
            if result.status == RuntimeToolStatus::Rejected {
                let approval = result.approval.as_ref().ok_or_else(|| VegaError::Tool {
                    tool: "permission".to_string(),
                    message: "rejection missing approval audit".to_string(),
                })?;
                let approval_json = approval_audit_from_runtime(approval)
                    .to_json()
                    .map_err(|_| safe_audit_error("permission"))?;
                validate_runtime_terminal(
                    project_id,
                    thread_id,
                    &result.call_id,
                    &state,
                    result,
                    &approval_audit_from_runtime(approval),
                )?;
                validate_rejected_remember(&state, result, &approval_audit_from_runtime(approval))?;
                tool_calls::reject(
                    store.conn(),
                    &result.call_id,
                    &approval_json,
                    &result.output,
                    now_ms(),
                    result
                        .remember_rule
                        .as_ref()
                        .map(|target| tool_calls::RememberExactRule {
                            project_id,
                            tool: target.tool.as_str(),
                            pattern: &target.exact_pattern,
                        }),
                )
                .map_err(tool_transition_error)?;
            } else {
                let approval_json = state
                    .approval
                    .as_deref()
                    .ok_or_else(|| safe_audit_error(&state.tool))?;
                let approval = ApprovalAudit::from_json(approval_json)
                    .map_err(|_| safe_audit_error(&state.tool))?;
                validate_runtime_terminal(
                    project_id,
                    thread_id,
                    &result.call_id,
                    &state,
                    result,
                    &approval,
                )?;
                let status = match result.status {
                    RuntimeToolStatus::Success => "success",
                    RuntimeToolStatus::Failed => "failed",
                    RuntimeToolStatus::Cancelled => "cancelled",
                    RuntimeToolStatus::Rejected => return Err(safe_audit_error("permission")),
                };
                tool_calls::finish(
                    store.conn(),
                    tool_calls::FinishToolCall {
                        id: &result.call_id,
                        status,
                        output_text: &result.output,
                        exit_code: result.exit_code,
                        duration_ms: result.duration_ms,
                        finished_at: now_ms(),
                    },
                )
                .map_err(tool_transition_error)?;
            }
        }
        RuntimeEvent::UsageUpdated {
            usage,
            cost_microcents,
        } => {
            token_usage::insert(
                store.conn(),
                token_usage::NewTokenUsage {
                    thread_id,
                    message_id: Some(message_id),
                    model,
                    input_tokens: usage.input,
                    output_tokens: usage.output,
                    cache_read_tokens: usage.cache_read,
                    cache_write_tokens: usage.cache_write,
                    cost_microcents: *cost_microcents,
                    created_at: now_ms(),
                },
            )?;
        }
        RuntimeEvent::Finished(_) => {
            ensure_message_updated(
                messages::finish_streaming(store.conn(), message_id, streamed_content, "done")?,
                message_id,
            )?;
            vega_store::threads::open_thread(store.conn(), thread_id, now_ms())?;
        }
        RuntimeEvent::Interrupted => {
            ensure_message_updated(
                messages::finish_streaming(
                    store.conn(),
                    message_id,
                    streamed_content,
                    "interrupted",
                )?,
                message_id,
            )?;
            vega_store::threads::open_thread(store.conn(), thread_id, now_ms())?;
        }
        RuntimeEvent::Error(_) => {
            ensure_message_updated(
                messages::finish_streaming(store.conn(), message_id, streamed_content, "failed")?,
                message_id,
            )?;
            vega_store::threads::open_thread(store.conn(), thread_id, now_ms())?;
        }
        RuntimeEvent::TextDelta(_)
        | RuntimeEvent::ThinkingDelta(_)
        | RuntimeEvent::ToolCallOutput { .. }
        | RuntimeEvent::ToolCallFinished(_) => {}
    }
    Ok(())
}

fn tool_transition_error(error: tool_calls::ToolCallTransitionError) -> VegaError {
    VegaError::Tool {
        tool: "persistence".to_string(),
        message: error.to_string(),
    }
}

fn safe_audit_error(tool: &str) -> VegaError {
    VegaError::Tool {
        tool: tool.to_string(),
        message: "strict approval audit failed".to_string(),
    }
}

fn required_tool_state(
    store: &Store,
    call_id: &str,
    thread_id: &str,
) -> Result<tool_calls::ToolCallState, VegaError> {
    let state = tool_calls::find_state(store.conn(), call_id)?.ok_or_else(|| VegaError::Tool {
        tool: "persistence".to_string(),
        message: "tool call state is missing".to_string(),
    })?;
    if state.thread_id != thread_id {
        return Err(VegaError::Tool {
            tool: "persistence".to_string(),
            message: "tool call ownership mismatch".to_string(),
        });
    }
    Ok(state)
}

fn validate_runtime_proposal(call: &vega_runtime::RuntimeToolCall) -> Result<(), VegaError> {
    if matches!(call.name.as_str(), "write" | "edit") {
        let audit = vega_tools::WriteEditAudit::from_json(&call.input_json)
            .map_err(|_| safe_audit_error(&call.name))?;
        if audit.tool().as_str() != call.name {
            return Err(safe_audit_error(&call.name));
        }
    } else if !matches!(call.name.as_str(), "read" | "glob" | "grep" | "bash")
        && call.input_json != "{}"
    {
        return Err(safe_audit_error(&call.name));
    }
    Ok(())
}

fn tool_inputs_semantically_equal(tool: &str, left: &str, right: &str) -> bool {
    if !matches!(tool, "write" | "edit") {
        return left == right;
    }
    if let (Ok(left), Ok(right)) = (
        vega_tools::WriteEditAudit::from_json(left),
        vega_tools::WriteEditAudit::from_json(right),
    ) {
        return left.tool().as_str() == tool && right.tool().as_str() == tool && left == right;
    }
    if let (Ok(left), Ok(right)) = (
        vega_tools::InvalidWriteEditAudit::from_json(left),
        vega_tools::InvalidWriteEditAudit::from_json(right),
    ) {
        return left.tool().as_str() == tool && right.tool().as_str() == tool && left == right;
    }
    false
}

fn validate_runtime_validation_event(
    call: &vega_runtime::RuntimeToolCall,
    result: &vega_runtime::RuntimeToolResult,
) -> Result<(), VegaError> {
    let invalid = vega_tools::InvalidWriteEditAudit::from_json(&call.input_json)
        .map_err(|_| safe_audit_error(&call.name))?;
    let approval = result
        .approval
        .as_ref()
        .ok_or_else(|| safe_audit_error(&call.name))?;
    let expected = format!(
        "Tool error: invalid {} input ({})",
        call.name,
        invalid.validation_error_code().as_str()
    );
    if invalid.tool().as_str() != call.name
        || result.call_id != call.id
        || result.status != RuntimeToolStatus::Rejected
        || result.output != expected
        || result.exit_code.is_some()
        || result.duration_ms.is_some()
        || result.truncated.is_some()
        || result.remember_rule.is_some()
        || approval.decision != vega_runtime::RuntimeApprovalDecision::Deny
        || approval.source != vega_runtime::RuntimeApprovalSource::Validation
    {
        return Err(safe_audit_error(&call.name));
    }
    Ok(())
}

fn validate_runtime_conflict_event(
    call: &vega_runtime::RuntimeToolCall,
    result: &vega_runtime::RuntimeToolResult,
) -> Result<(), VegaError> {
    if matches!(call.name.as_str(), "write" | "edit") {
        let valid = vega_tools::WriteEditAudit::from_json(&call.input_json)
            .ok()
            .is_some_and(|audit| audit.tool().as_str() == call.name);
        let invalid = vega_tools::InvalidWriteEditAudit::from_json(&call.input_json)
            .ok()
            .is_some_and(|audit| audit.tool().as_str() == call.name);
        if !valid && !invalid {
            return Err(safe_audit_error(&call.name));
        }
    } else {
        validate_runtime_proposal(call)?;
    }
    if result.call_id != call.id
        || result.output != vega_runtime::CALL_ID_CONFLICT_OUTPUT
        || result.status != RuntimeToolStatus::Failed
        || result.reused
        || result.exit_code.is_some()
        || result.duration_ms.is_some()
        || result.truncated.is_some()
        || result.approval.is_some()
        || result.remember_rule.is_some()
    {
        return Err(safe_audit_error(&call.name));
    }
    Ok(())
}

fn validate_runtime_approval_event(
    state: &tool_calls::ToolCallState,
    call_id: &str,
    audit: &vega_runtime::RuntimeApprovalAudit,
    remember: Option<&vega_runtime::RuntimePermissionTarget>,
) -> Result<(), VegaError> {
    if state.status != "pending_approval"
        || audit.decision == vega_runtime::RuntimeApprovalDecision::Deny
    {
        return Err(safe_audit_error(&state.tool));
    }
    let shared = approval_audit_from_runtime(audit);
    shared
        .to_json()
        .map_err(|_| safe_audit_error(&state.tool))?;
    if !approval_source_matches(
        &state.tool,
        RuntimeToolStatus::Success,
        shared.source,
        false,
    ) {
        return Err(safe_audit_error(&state.tool));
    }
    if state.tool == "bash" && !bash_danger_audit_matches(&state.input_json, &shared) {
        return Err(safe_audit_error(&state.tool));
    }
    let expects_remember = audit.decision == vega_runtime::RuntimeApprovalDecision::Always
        && matches!(
            audit.source,
            vega_runtime::RuntimeApprovalSource::User | vega_runtime::RuntimeApprovalSource::Danger
        );
    if remember.is_some() != expects_remember {
        return Err(safe_audit_error(&state.tool));
    }
    if let Some(target) = remember
        && !target_matches_state(state, call_id, target)
    {
        return Err(safe_audit_error(&state.tool));
    }
    Ok(())
}

fn validate_rejected_remember(
    state: &tool_calls::ToolCallState,
    result: &vega_runtime::RuntimeToolResult,
    approval: &ApprovalAudit,
) -> Result<(), VegaError> {
    let expects_remember = approval.source == ApprovalSource::ReadOnly
        && approval
            .danger
            .as_ref()
            .is_some_and(|danger| danger.decision == Approval::Always);
    if result.remember_rule.is_some() != expects_remember {
        return Err(safe_audit_error(&state.tool));
    }
    if let Some(target) = &result.remember_rule
        && !target_matches_state(state, &result.call_id, target)
    {
        return Err(safe_audit_error(&state.tool));
    }
    Ok(())
}

fn target_matches_state(
    state: &tool_calls::ToolCallState,
    call_id: &str,
    target: &vega_runtime::RuntimePermissionTarget,
) -> bool {
    let exact_matches_input = match state.tool.as_str() {
        "write" | "edit" => vega_tools::WriteEditAudit::from_json(&state.input_json)
            .ok()
            .is_some_and(|audit| {
                audit.tool().as_str() == state.tool && audit.path() == target.exact_pattern
            }),
        "bash" => vega_tools::bash_permission_signature(&state.input_json)
            .ok()
            .is_some_and(|command| command == target.exact_pattern),
        _ => false,
    };
    target.call_id == call_id
        && target.tool.as_str() == state.tool
        && !target.exact_pattern.is_empty()
        && target.exact_pattern == target.display_target
        && exact_matches_input
}

fn validate_runtime_terminal(
    project_id: &str,
    thread_id: &str,
    call_id: &str,
    state: &tool_calls::ToolCallState,
    result: &vega_runtime::RuntimeToolResult,
    approval: &ApprovalAudit,
) -> Result<(), VegaError> {
    let result_approval = result.approval.as_ref().map(approval_audit_from_runtime);
    if result_approval.as_ref() != Some(approval)
        || result.call_id != call_id
        || (result.status == RuntimeToolStatus::Rejected && state.status != "pending_approval")
        || (result.status != RuntimeToolStatus::Rejected && state.status != "running")
        || (result.status != RuntimeToolStatus::Rejected && result.remember_rule.is_some())
        || matches!(
            result.status,
            RuntimeToolStatus::Rejected | RuntimeToolStatus::Failed
        ) && result.truncated.is_some()
        || result.status == RuntimeToolStatus::Success && result.truncated.is_none()
        || result.status == RuntimeToolStatus::Cancelled
            && state.tool == "bash"
            && result.truncated.is_some()
    {
        return Err(safe_audit_error(&state.tool));
    }
    let canonical = validate_recovered_projection(
        project_id,
        thread_id,
        call_id,
        &state.tool,
        &state.input_json,
        &result.output,
        result.status,
        approval,
        result.exit_code,
        result.duration_ms,
    )
    .map_err(|_| safe_audit_error(&state.tool))?;
    if !tool_inputs_semantically_equal(&state.tool, &canonical, &state.input_json) {
        return Err(safe_audit_error(&state.tool));
    }
    Ok(())
}

fn validate_reused_terminal(
    project_id: &str,
    thread_id: &str,
    state: &tool_calls::ToolCallState,
    result: &vega_runtime::RuntimeToolResult,
) -> Result<(), VegaError> {
    let expected_status = match result.status {
        RuntimeToolStatus::Rejected => "rejected",
        RuntimeToolStatus::Success => "success",
        RuntimeToolStatus::Failed => "failed",
        RuntimeToolStatus::Cancelled => "cancelled",
    };
    let approval_json = state
        .approval
        .as_deref()
        .ok_or_else(|| safe_audit_error(&state.tool))?;
    let approval =
        ApprovalAudit::from_json(approval_json).map_err(|_| safe_audit_error(&state.tool))?;
    let runtime_approval = result
        .approval
        .as_ref()
        .map(approval_audit_from_runtime)
        .ok_or_else(|| safe_audit_error(&state.tool))?;
    let persisted_duration = state
        .duration_ms
        .map(u64::try_from)
        .transpose()
        .map_err(|_| safe_audit_error(&state.tool))?;
    if state.status != expected_status
        || runtime_approval != approval
        || result.remember_rule.is_some()
        || result.truncated.is_some()
        || state.output_full_path.is_some()
        || state.output_text.as_deref() != Some(result.output.as_str())
        || state.exit_code != result.exit_code
        || persisted_duration != result.duration_ms
    {
        return Err(safe_audit_error(&state.tool));
    }
    let canonical = validate_recovered_projection(
        project_id,
        thread_id,
        &result.call_id,
        &state.tool,
        &state.input_json,
        &result.output,
        result.status,
        &approval,
        result.exit_code,
        result.duration_ms,
    )
    .map_err(|_| safe_audit_error(&state.tool))?;
    if !tool_inputs_semantically_equal(&state.tool, &canonical, &state.input_json) {
        return Err(safe_audit_error(&state.tool));
    }
    Ok(())
}

fn ensure_message_updated(updated: usize, message_id: &str) -> Result<(), VegaError> {
    if updated == 0 {
        Err(VegaError::Tool {
            tool: "runtime".to_string(),
            message: format!("streaming message row disappeared or became terminal: {message_id}"),
        })
    } else {
        Ok(())
    }
}

fn runtime_store_error(error: impl Into<VegaError>) -> ConversationError {
    ConversationError::Runtime(Arc::new(error.into()))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;
    use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason};

    use super::*;
    use crate::types::{ConversationEvent, ToolCallStatus};

    struct FixedPermissionHook {
        calls: Arc<AtomicUsize>,
        decision: PermissionDecision,
    }

    impl PermissionHook for FixedPermissionHook {
        fn request(
            &self,
            _request: PermissionRequest,
            _cancel: CancellationToken,
        ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let decision = self.decision.clone();
            async move { Ok(decision) }.boxed()
        }
    }

    fn setup() -> (Store, tempfile::TempDir, String) {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "// TODO: persist me\n").unwrap();
        let store = Store::open(dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            dir.path().to_str().unwrap(),
            "fixture",
            Some("master"),
        )
        .unwrap();
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                id: "thread-1",
                project_id: &project.id,
                title: "",
                mode: "execute",
                permission_mode: "confirm",
                model: "mock-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        (store, dir, project.id)
    }

    fn setup_external(
        permission_mode: &str,
    ) -> (Store, tempfile::TempDir, tempfile::TempDir, String) {
        let project_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let store = Store::open(data_dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            project_dir.path().to_str().unwrap(),
            "external-fixture",
            Some("master"),
        )
        .unwrap();
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                id: "thread-1",
                project_id: &project.id,
                title: "",
                mode: "execute",
                permission_mode,
                model: "mock-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        (store, project_dir, data_dir, project.id)
    }

    fn scripted_provider(call_id: &str, input_json: &str) -> MockProvider {
        MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("Checking. ".into()),
                ProviderEvent::ThinkingDelta("Need grep".into()),
                ProviderEvent::ToolUse {
                    id: call_id.into(),
                    name: "grep".into(),
                    input_json: input_json.into(),
                },
                ProviderEvent::Usage {
                    input: 12,
                    output: 3,
                    cache_read: 2,
                    cache_write: 1,
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("Found the TODO.".into()),
                ProviderEvent::Usage {
                    input: 20,
                    output: 5,
                    cache_read: 0,
                    cache_write: 0,
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
        ])
    }

    #[tokio::test]
    async fn persists_messages_tool_lifecycle_and_zero_cost_usage() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = scripted_provider("call-1", r#"{"pattern":"TODO","path":"lib.rs"}"#);
        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Find TODO",
            "You inspect repositories.",
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(run.content, "Checking. Found the TODO.");
        assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ThinkingDelta { delta, .. } if delta == "Need grep")));
        assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallApproved { call_id, .. } if call_id == "call-1")));
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Success && result.output.contains("lib.rs:1:// TODO")
        )));

        let assistant: (String, String, i64) = store
            .conn()
            .query_row(
                "SELECT content, status, seq FROM messages WHERE id = ?1",
                [&run.assistant_message_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            assistant,
            ("Checking. Found the TODO.".into(), "done".into(), 2)
        );
        let tool: (String, String, String, String) = store
            .conn()
            .query_row(
                "SELECT status, approval, output_text, input_json FROM tool_calls WHERE id = 'call-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(tool.0, "success");
        let approval = ApprovalAudit::from_json(&tool.1).unwrap();
        assert_eq!(approval.decision, Approval::Once);
        assert_eq!(approval.source, ApprovalSource::ReadonlyTool);
        assert!(tool.2.contains("lib.rs:1:// TODO"));
        assert_eq!(tool.3, r#"{"pattern":"TODO","path":"lib.rs"}"#);
        let usage: (i64, i64, i64, i64, i64, String) = store
            .conn()
            .query_row(
                "SELECT input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
                 cost_microcents, model FROM token_usage",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(usage, (12, 3, 2, 1, 0, "mock-model".into()));
        let usage_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM token_usage", [], |row| row.get(0))
            .unwrap();
        let nonzero_cost_count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM token_usage WHERE cost_microcents != 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(usage_count, 2);
        assert_eq!(nonzero_cost_count, 0);
        let updated_at: i64 = store
            .conn()
            .query_row(
                "SELECT updated_at FROM threads WHERE id = 'thread-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(updated_at > 1);

        let tables: Vec<String> = {
            let mut stmt = store
                .conn()
                .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(
            tables,
            vec![
                "messages",
                "permissions",
                "projects",
                "threads",
                "token_usage",
                "tool_calls"
            ]
        );
    }

    #[tokio::test]
    async fn forwards_events_live_only_after_critical_state_is_persisted() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = scripted_provider("live-call", r#"{"pattern":"TODO","path":"lib.rs"}"#);
        let mut observed = Vec::new();
        let mut expected_text = String::new();

        let run = run_thread_task_with_sink(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Find TODO",
            "System",
            CancellationToken::new(),
            |event| {
                match event {
                    ConversationEvent::MessageStarted { message_id, .. } => {
                        let row = messages::find(store.conn(), message_id)?.ok_or_else(|| {
                            VegaError::Tool {
                                tool: "test".into(),
                                message: "started message missing".into(),
                            }
                        })?;
                        assert_eq!(row.status, "streaming");
                        observed.push("started");
                    }
                    ConversationEvent::TextDelta { message_id, delta } => {
                        expected_text.push_str(delta);
                        let row = messages::find(store.conn(), message_id)?.ok_or_else(|| {
                            VegaError::Tool {
                                tool: "test".into(),
                                message: "streaming message missing".into(),
                            }
                        })?;
                        assert_eq!(row.status, "streaming");
                        assert_eq!(row.content, expected_text);
                        observed.push("text");
                    }
                    ConversationEvent::ToolCallApproved { call_id, .. } => {
                        let status: String = store.conn().query_row(
                            "SELECT status FROM tool_calls WHERE id = ?1",
                            [call_id],
                            |row| row.get(0),
                        )?;
                        assert_eq!(status, "approved");
                        observed.push("approved");
                    }
                    ConversationEvent::ToolCallFinished {
                        call_id, result, ..
                    } => {
                        let persisted: (String, String) = store.conn().query_row(
                            "SELECT status, output_text FROM tool_calls WHERE id = ?1",
                            [call_id],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )?;
                        assert_eq!(persisted.0, "success");
                        assert_eq!(persisted.1, result.output);
                        observed.push("tool-finished");
                    }
                    ConversationEvent::ToolCallOutput { call_id, .. } => {
                        let status: String = store.conn().query_row(
                            "SELECT status FROM tool_calls WHERE id = ?1",
                            [call_id],
                            |row| row.get(0),
                        )?;
                        assert_eq!(status, "success");
                        observed.push("output");
                    }
                    ConversationEvent::UsageUpdated { .. } => {
                        let count: i64 = store.conn().query_row(
                            "SELECT COUNT(*) FROM token_usage",
                            [],
                            |row| row.get(0),
                        )?;
                        assert!(count >= 1);
                        observed.push("usage");
                    }
                    ConversationEvent::MessageFinished { message_id, .. } => {
                        let row = messages::find(store.conn(), message_id)?.ok_or_else(|| {
                            VegaError::Tool {
                                tool: "test".into(),
                                message: "finished message missing".into(),
                            }
                        })?;
                        assert_eq!(row.status, "done");
                        assert_eq!(row.content, "Checking. Found the TODO.");
                        observed.push("finished");
                    }
                    _ => {}
                }
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(run.content, "Checking. Found the TODO.");
        assert_eq!(expected_text, "Checking. Found the TODO.");
        let approved = observed
            .iter()
            .position(|item| *item == "approved")
            .unwrap();
        let terminal = observed
            .iter()
            .position(|item| *item == "tool-finished")
            .unwrap();
        let output = observed.iter().position(|item| *item == "output").unwrap();
        let finished = observed
            .iter()
            .position(|item| *item == "finished")
            .unwrap();
        assert!(approved < output && output < terminal && terminal < finished);
    }

    #[tokio::test]
    async fn fast_text_deltas_coalesce_and_each_displayed_delta_is_durable() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let mut provider_events = (0..100)
            .map(|_| ProviderEvent::TextDelta("x".into()))
            .collect::<Vec<_>>();
        provider_events.push(ProviderEvent::Done {
            stop_reason: StopReason::End,
        });
        let provider = MockProvider::new(vec![ScriptStep::events(provider_events)]);
        let writes = Arc::new(AtomicUsize::new(0));
        let config = PersistenceActorConfig {
            snapshot_writes: Some(writes.clone()),
            ..PersistenceActorConfig::default()
        };
        let mut displayed = String::new();

        let run = run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Stream",
            "System",
            CancellationToken::new(),
            |event| {
                if let ConversationEvent::TextDelta { message_id, delta } = event {
                    displayed.push_str(delta);
                    let durable = messages::find(store.conn(), message_id)?
                        .ok_or_else(|| persistence_actor_error("streaming message missing"))?;
                    assert!(durable.content.starts_with(&displayed));
                }
                Ok(())
            },
            config,
        )
        .await
        .unwrap();

        assert_eq!(run.content.len(), 100);
        assert_eq!(displayed.len(), 100);
        let write_count = writes.load(Ordering::SeqCst);
        assert!(write_count > 0 && write_count < 100, "writes={write_count}");
    }

    #[tokio::test]
    async fn ten_thousand_deltas_keep_the_channel_bounded_and_writes_coalesced() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let mut provider_events = (0..10_000)
            .map(|_| ProviderEvent::TextDelta("x".into()))
            .collect::<Vec<_>>();
        provider_events.push(ProviderEvent::Done {
            stop_reason: StopReason::End,
        });
        let provider = MockProvider::new(vec![ScriptStep::events(provider_events)]);
        let writes = Arc::new(AtomicUsize::new(0));
        let config = PersistenceActorConfig {
            snapshot_writes: Some(writes.clone()),
            ..PersistenceActorConfig::default()
        };

        let run = run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Stream",
            "System",
            CancellationToken::new(),
            |_| Ok(()),
            config,
        )
        .await
        .unwrap();

        assert_eq!(run.content.len(), 10_000);
        assert_eq!(
            run.events
                .iter()
                .filter(|event| matches!(event, ConversationEvent::TextDelta { .. }))
                .count(),
            10_000
        );
        let write_count = writes.load(Ordering::SeqCst);
        assert!(write_count > 0 && write_count < 100, "writes={write_count}");
    }

    #[tokio::test]
    async fn lone_text_delta_flushes_during_provider_stall_within_sixteen_ms() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![
            ScriptStep::events(vec![ProviderEvent::TextDelta("partial".into())]),
            ScriptStep::delay(Duration::from_millis(100)),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ]);
        let mut started = None;
        let mut display_delay = None;

        run_thread_task_with_sink(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Stream",
            "System",
            CancellationToken::new(),
            |event| {
                match event {
                    ConversationEvent::MessageStarted { .. } => started = Some(Instant::now()),
                    ConversationEvent::TextDelta { message_id, .. } => {
                        let began = started.ok_or_else(|| {
                            persistence_actor_error("message start was not observed")
                        })?;
                        display_delay = Some(began.elapsed());
                        let durable = messages::find(store.conn(), message_id)?
                            .ok_or_else(|| persistence_actor_error("streaming message missing"))?;
                        assert_eq!(durable.content, "partial");
                    }
                    _ => {}
                }
                Ok(())
            },
        )
        .await
        .unwrap();

        assert!(display_delay.unwrap() < Duration::from_millis(16));
    }

    #[tokio::test]
    async fn running_persistence_failure_prevents_tool_start_and_next_round() {
        let (store, dir, _project_id) = setup();
        let fifo = dir.path().join("never-read");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "blocked-call".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"never-read"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let config = PersistenceActorConfig {
            fail_event: Some(InjectedPersistenceFailure::Running),
            ..PersistenceActorConfig::default()
        };

        let result = tokio::time::timeout(
            Duration::from_millis(500),
            run_thread_task_with_sink_config(
                &store,
                &provider,
                &tools,
                "thread-1",
                "Read",
                "System",
                CancellationToken::new(),
                |_| Ok(()),
                config,
            ),
        )
        .await
        .expect("running barrier failure must not enter the blocking FIFO")
        .unwrap_err();

        assert!(result.to_string().contains("critical persistence failure"));
        assert_eq!(provider.requests().len(), 1);
        let status: String = store
            .conn()
            .query_row(
                "SELECT status FROM tool_calls WHERE id = 'blocked-call'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "approved");
    }

    #[tokio::test]
    async fn terminal_persistence_failure_prevents_the_next_provider_round() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = scripted_provider("terminal-fail", r#"{"pattern":"TODO"}"#);
        let config = PersistenceActorConfig {
            fail_event: Some(InjectedPersistenceFailure::Finished),
            ..PersistenceActorConfig::default()
        };

        let error = run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Find",
            "System",
            CancellationToken::new(),
            |_| Ok(()),
            config,
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("critical persistence failure"));
        assert_eq!(provider.requests().len(), 1);
        let status: String = store
            .conn()
            .query_row(
                "SELECT status FROM tool_calls WHERE id = 'terminal-fail'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "running");
    }

    #[tokio::test]
    async fn actor_panic_closes_ack_and_returns_without_hanging() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "panic-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
        let config = PersistenceActorConfig {
            fail_event: Some(InjectedPersistenceFailure::PanicRunning),
            ..PersistenceActorConfig::default()
        };
        let mut surfaced = None;

        let error = tokio::time::timeout(
            Duration::from_millis(500),
            run_thread_task_with_sink_config(
                &store,
                &provider,
                &tools,
                "thread-1",
                "Read",
                "System",
                CancellationToken::new(),
                |event| {
                    if let ConversationEvent::Error { error, .. } = event {
                        surfaced = Some(error.clone());
                    }
                    Ok(())
                },
                config,
            ),
        )
        .await
        .expect("actor panic must close its async acknowledgement")
        .unwrap_err();

        assert!(error.to_string().contains("dropped event acknowledgement"));
        assert!(matches!(
            error,
            ConversationError::Runtime(ref error)
                if matches!(error.as_ref(), VegaError::Io(_))
        ));
        assert!(matches!(surfaced.as_deref(), Some(VegaError::Io(_))));
        assert_eq!(provider.requests().len(), 1);
    }

    #[tokio::test]
    async fn delayed_actor_does_not_block_the_tokio_executor() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("heartbeat".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]);
        let beats = Arc::new(AtomicUsize::new(0));
        let beat_counter = beats.clone();
        let heartbeat_cancel = CancellationToken::new();
        let heartbeat_stop = heartbeat_cancel.clone();
        let heartbeat = tokio::spawn(async move {
            while !heartbeat_stop.is_cancelled() {
                tokio::time::sleep(Duration::from_millis(1)).await;
                beat_counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        let config = PersistenceActorConfig {
            command_delay: Some(Duration::from_millis(30)),
            ..PersistenceActorConfig::default()
        };

        run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Stream",
            "System",
            CancellationToken::new(),
            |_| Ok(()),
            config,
        )
        .await
        .unwrap();
        heartbeat_cancel.cancel();
        heartbeat.await.unwrap();

        assert!(beats.load(Ordering::SeqCst) >= 10);
    }

    #[tokio::test]
    async fn delayed_preparation_does_not_block_the_tokio_executor() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let beats = Arc::new(AtomicUsize::new(0));
        let beat_counter = beats.clone();
        let heartbeat_cancel = CancellationToken::new();
        let heartbeat_stop = heartbeat_cancel.clone();
        let heartbeat = tokio::spawn(async move {
            while !heartbeat_stop.is_cancelled() {
                tokio::time::sleep(Duration::from_millis(1)).await;
                beat_counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        let config = PersistenceActorConfig {
            preparation_delay: Some(Duration::from_millis(30)),
            ..PersistenceActorConfig::default()
        };

        run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Prepare",
            "System",
            CancellationToken::new(),
            |_| Ok(()),
            config,
        )
        .await
        .unwrap();
        heartbeat_cancel.cancel();
        heartbeat.await.unwrap();

        assert!(beats.load(Ordering::SeqCst) >= 10);
    }

    #[tokio::test]
    async fn preparation_store_failure_is_structured_and_forwarded() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let beats = Arc::new(AtomicUsize::new(0));
        let beat_counter = beats.clone();
        let heartbeat_cancel = CancellationToken::new();
        let heartbeat_stop = heartbeat_cancel.clone();
        let heartbeat = tokio::spawn(async move {
            while !heartbeat_stop.is_cancelled() {
                tokio::time::sleep(Duration::from_millis(1)).await;
                beat_counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        let config = PersistenceActorConfig {
            preparation_delay: Some(Duration::from_millis(30)),
            preparation_query_only: true,
            ..PersistenceActorConfig::default()
        };
        let mut surfaced = None;

        let error = tokio::time::timeout(
            Duration::from_millis(500),
            run_thread_task_with_sink_config(
                &store,
                &provider,
                &tools,
                "thread-1",
                "Prepare",
                "System",
                CancellationToken::new(),
                |event| {
                    if let ConversationEvent::Error { error, .. } = event {
                        surfaced = Some(error.clone());
                    }
                    Ok(())
                },
                config,
            ),
        )
        .await
        .expect("failing preparation must not block the executor")
        .unwrap_err();
        heartbeat_cancel.cancel();
        heartbeat.await.unwrap();

        assert!(matches!(
            error,
            ConversationError::Runtime(ref error)
                if matches!(error.as_ref(), VegaError::Store(_))
        ));
        assert!(matches!(surfaced.as_deref(), Some(VegaError::Store(_))));
        assert!(provider.requests().is_empty());
        assert!(beats.load(Ordering::SeqCst) >= 10);
        let message_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(message_count, 0);
    }

    #[tokio::test]
    async fn actor_store_failure_is_structured_and_forwarded() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("partial".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]);
        let config = PersistenceActorConfig {
            actor_query_only: true,
            ..PersistenceActorConfig::default()
        };
        let mut surfaced = None;

        let error = run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Stream",
            "System",
            CancellationToken::new(),
            |event| {
                if let ConversationEvent::Error { error, .. } = event {
                    surfaced = Some(error.clone());
                }
                Ok(())
            },
            config,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            ConversationError::Runtime(ref error)
                if matches!(error.as_ref(), VegaError::Store(_))
        ));
        assert!(matches!(surfaced.as_deref(), Some(VegaError::Store(_))));
    }

    #[tokio::test]
    async fn actor_start_failure_uses_background_cleanup_and_structured_error() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let config = PersistenceActorConfig {
            fail_start: true,
            ..PersistenceActorConfig::default()
        };
        let mut surfaced = None;

        let error = tokio::time::timeout(
            Duration::from_millis(500),
            run_thread_task_with_sink_config(
                &store,
                &provider,
                &tools,
                "thread-1",
                "Start actor",
                "System",
                CancellationToken::new(),
                |event| {
                    if let ConversationEvent::Error { error, .. } = event {
                        surfaced = Some(error.clone());
                    }
                    Ok(())
                },
                config,
            ),
        )
        .await
        .expect("actor startup failure cleanup must not block the executor")
        .unwrap_err();

        assert!(matches!(
            error,
            ConversationError::Runtime(ref error)
                if matches!(error.as_ref(), VegaError::Io(_))
        ));
        assert!(matches!(surfaced.as_deref(), Some(VegaError::Io(_))));
        assert!(provider.requests().is_empty());
        let assistant_status: String = store
            .conn()
            .query_row(
                "SELECT status FROM messages WHERE role = 'assistant' ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assistant_status, "failed");
    }

    #[tokio::test]
    async fn retry_reuses_persisted_call_id_without_running_the_tool() {
        let (store, dir, _project_id) = setup();
        let database_path = dir.path().join("vega.db");
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let first = scripted_provider("stable-call", r#"{"pattern":"TODO","path":"lib.rs"}"#);
        run_thread_task(
            &store,
            &first,
            &tools,
            "thread-1",
            "First",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        drop(store);
        let store = Store::open(&database_path).unwrap();
        store.migrate().unwrap();
        fs::remove_file(dir.path().join("lib.rs")).unwrap();

        let retry = scripted_provider("stable-call", r#"{"pattern":"TODO","path":"lib.rs"}"#);
        let run = run_thread_task(
            &store,
            &retry,
            &tools,
            "thread-1",
            "Retry",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.reused
                    && result.status == ToolCallStatus::Success
                    && result.output.contains("persist me")
        )));
        let count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE id = 'stable-call'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn retry_reuses_failed_and_rejected_terminal_results_verbatim() {
        for (call_id, tool, input, status, approval, output, expected) in [
            (
                "failed-call",
                "read",
                r#"{"path":"missing.txt"}"#,
                "failed",
                "once",
                "original failed output",
                ToolCallStatus::Failed,
            ),
            (
                "rejected-call",
                "unknown",
                "{}",
                "rejected",
                r#"{"decision":"deny","note":null,"source":"run_mode","danger":null}"#,
                "Tool error: denied: unavailable tool",
                ToolCallStatus::Rejected,
            ),
            (
                "cancelled-call",
                "read",
                r#"{"path":"missing.txt"}"#,
                "cancelled",
                "once",
                "original cancelled output",
                ToolCallStatus::Cancelled,
            ),
        ] {
            let (store, dir, _project_id) = setup();
            messages::insert(
                store.conn(),
                &messages::MessageRow {
                    id: "prior-assistant".into(),
                    thread_id: "thread-1".into(),
                    seq: 1,
                    role: "assistant".into(),
                    kind: "text".into(),
                    content: String::new(),
                    status: "done".into(),
                    created_at: 1,
                },
            )
            .unwrap();
            tool_calls::insert(
                store.conn(),
                tool_calls::NewToolCall {
                    id: call_id,
                    thread_id: "thread-1",
                    message_id: "prior-assistant",
                    seq: 1,
                    tool,
                    input_json: input,
                    status: "pending_approval",
                    created_at: 1,
                },
            )
            .unwrap();
            tool_calls::update(
                store.conn(),
                call_id,
                status,
                Some(approval),
                Some(output),
                Some(2),
            )
            .unwrap();
            let provider = MockProvider::new_rounds(vec![
                vec![ScriptStep::events(vec![
                    ProviderEvent::ToolUse {
                        id: call_id.into(),
                        name: tool.into(),
                        input_json: input.into(),
                    },
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ])],
                vec![ScriptStep::events(vec![ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }])],
            ]);
            let tools = vega_tools::Tools::new(dir.path()).unwrap();

            let run = run_thread_task(
                &store,
                &provider,
                &tools,
                "thread-1",
                "Retry",
                "System",
                CancellationToken::new(),
            )
            .await
            .unwrap();

            assert!(run.events.iter().any(|event| matches!(
                event,
                ConversationEvent::ToolCallFinished { result, .. }
                    if result.reused && result.status == expected && result.output == output
            )));
            let persisted: (String, String) = store
                .conn()
                .query_row(
                    "SELECT status, output_text FROM tool_calls WHERE id = ?1",
                    [call_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(persisted, (status.into(), output.into()));
        }
    }

    #[tokio::test]
    async fn startup_recovery_survives_reopen_and_allows_a_new_turn() {
        let (store, dir, _project_id) = setup();
        let database_path = dir.path().join("vega.db");
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: "stale-assistant".into(),
                thread_id: "thread-1".into(),
                seq: 1,
                role: "assistant".into(),
                kind: "text".into(),
                content: "partial".into(),
                status: "streaming".into(),
                created_at: 1,
            },
        )
        .unwrap();
        for (index, status) in [(1, "pending_approval"), (2, "approved"), (3, "running")] {
            let call_id = format!("stale-{status}");
            tool_calls::insert(
                store.conn(),
                tool_calls::NewToolCall {
                    id: &call_id,
                    thread_id: "thread-1",
                    message_id: "stale-assistant",
                    seq: index,
                    tool: "read",
                    input_json: r#"{"path":"lib.rs"}"#,
                    status,
                    created_at: 1,
                },
            )
            .unwrap();
            if status != "pending_approval" {
                tool_calls::update(store.conn(), &call_id, status, Some("once"), None, None)
                    .unwrap();
            }
        }
        drop(store);
        let store = Store::open(&database_path).unwrap();
        store.migrate().unwrap();
        fs::remove_file(dir.path().join("lib.rs")).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "stale-pending_approval".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"lib.rs"}"#.into(),
                },
                ProviderEvent::ToolUse {
                    id: "stale-approved".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"lib.rs"}"#.into(),
                },
                ProviderEvent::ToolUse {
                    id: "stale-running".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"lib.rs"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("resumed".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
        ]);
        let tools = vega_tools::Tools::new(dir.path()).unwrap();

        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Continue",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(run.content, "resumed");
        let stale_status: String = store
            .conn()
            .query_row(
                "SELECT status FROM messages WHERE id = 'stale-assistant'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stale_status, "interrupted");
        for (call_id, status, approval, expected_status) in [
            (
                "stale-pending_approval",
                "rejected",
                vega_store::recovery::RECOVERY_DENIAL_APPROVAL_JSON,
                ToolCallStatus::Rejected,
            ),
            (
                "stale-approved",
                "cancelled",
                "once",
                ToolCallStatus::Cancelled,
            ),
            (
                "stale-running",
                "cancelled",
                "once",
                ToolCallStatus::Cancelled,
            ),
        ] {
            let persisted: (String, String, String, i64) = store
                .conn()
                .query_row(
                    "SELECT status, approval, output_text, finished_at FROM tool_calls WHERE id = ?1",
                    [call_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
            assert_eq!(persisted.0, status);
            assert_eq!(persisted.1, approval);
            assert!(persisted.2.contains("startup recovery"));
            assert!(persisted.3 > 0);
            assert!(run.events.iter().any(|event| matches!(
                event,
                ConversationEvent::ToolCallFinished { call_id: event_id, result }
                    if event_id == call_id
                        && result.reused
                        && result.status == expected_status
                        && result.output == persisted.2
            )));
        }
        assert_eq!(provider.requests().len(), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn crash_child_runtime_fixture() {
        let Some(root) = std::env::var_os("VEGA_T20_CRASH_CHILD_ROOT") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let store = Store::open(root.join("vega.db")).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            root.to_str().unwrap(),
            "crash-fixture",
            Some("master"),
        )
        .unwrap();
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                id: "thread-1",
                project_id: &project.id,
                title: "",
                mode: "execute",
                permission_mode: "confirm",
                model: "mock-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        let fifo = root.join("never-read");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        let tools = vega_tools::Tools::new(&root).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("durable partial".into()),
            ProviderEvent::ToolUse {
                id: "crash-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"never-read"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);

        let _ = run_thread_task_with_sink(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Crash",
            "System",
            CancellationToken::new(),
            |event| {
                if let ConversationEvent::TextDelta { message_id, .. } = event {
                    let durable = messages::find(store.conn(), message_id)?
                        .ok_or_else(|| persistence_actor_error("streaming message missing"))?;
                    assert_eq!(durable.content, "durable partial");
                    fs::write(root.join("displayed.marker"), durable.content)
                        .map_err(VegaError::from)?;
                }
                Ok(())
            },
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn killed_child_recovers_only_displayed_content_and_reuses_running_call() {
        let dir = tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let mut child = std::process::Command::new(executable)
            .arg("--exact")
            .arg("agent::tests::crash_child_runtime_fixture")
            .arg("--nocapture")
            .env("VEGA_T20_CRASH_CHILD_ROOT", dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let marker = dir.path().join("displayed.marker");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !marker.exists() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if !marker.exists() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child never displayed a durable delta");
        }
        let observer = Store::open(dir.path().join("vega.db")).unwrap();
        let mut saw_running = false;
        while Instant::now() < deadline {
            let running: i64 = observer
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM tool_calls WHERE id = 'crash-call' AND status = 'running'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            if running == 1 {
                saw_running = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if !saw_running {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child never crossed the durable running barrier");
        }
        child.kill().unwrap();
        child.wait().unwrap();
        drop(observer);

        let store = Store::open(dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "crash-call".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"never-read"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("continued".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
        ]);

        let run = tokio::time::timeout(
            Duration::from_secs(1),
            run_thread_task(
                &store,
                &provider,
                &tools,
                "thread-1",
                "Continue",
                "System",
                CancellationToken::new(),
            ),
        )
        .await
        .expect("recovered call id must not re-enter the blocking FIFO")
        .unwrap();

        assert_eq!(run.content, "continued");
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { call_id, result }
                if call_id == "crash-call"
                    && result.reused
                    && result.status == ToolCallStatus::Cancelled
                    && result.output.contains("startup recovery")
        )));
        let old_message: (String, String) = store
            .conn()
            .query_row(
                "SELECT content, status FROM messages WHERE role = 'assistant' ORDER BY seq ASC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            old_message,
            ("durable partial".into(), "interrupted".into())
        );
        let recovered_tool: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, output_text FROM tool_calls WHERE id = 'crash-call'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(recovered_tool.0, "cancelled");
        assert!(recovered_tool.1.contains("startup recovery"));
        assert_eq!(provider.requests().len(), 2);
    }

    #[tokio::test]
    async fn cross_thread_call_id_collision_fails_before_execution() {
        let (store, dir, project_id) = setup();
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                id: "thread-2",
                project_id: &project_id,
                title: "",
                mode: "execute",
                permission_mode: "confirm",
                model: "mock-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id: "shared-call",
                thread_id: "thread-2",
                message_id: "other-message",
                seq: 1,
                tool: "read",
                input_json: r#"{"path":"lib.rs"}"#,
                status: "pending_approval",
                created_at: 1,
            },
        )
        .unwrap();
        tool_calls::update(
            store.conn(),
            "shared-call",
            "success",
            Some("once"),
            Some("other output"),
            Some(2),
        )
        .unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "shared-call".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"lib.rs"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let tools = vega_tools::Tools::new(dir.path()).unwrap();

        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Read",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Failed
                    && !result.reused
                    && result.output == vega_runtime::CALL_ID_CONFLICT_OUTPUT
        )));
        let persisted: (String, String, String) = store
            .conn()
            .query_row(
                "SELECT thread_id, status, output_text FROM tool_calls WHERE id = 'shared-call'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            persisted,
            ("thread-2".into(), "success".into(), "other output".into())
        );
        let done_assistants: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE thread_id = 'thread-1' AND role = 'assistant' AND status = 'done'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(done_assistants, 1);
    }

    #[tokio::test]
    async fn same_thread_call_id_with_changed_input_cannot_overwrite_audit_row() {
        let (store, dir, _project_id) = setup();
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id: "changed-call",
                thread_id: "thread-1",
                message_id: "prior-message",
                seq: 1,
                tool: "read",
                input_json: r#"{"path":"original.txt"}"#,
                status: "pending_approval",
                created_at: 1,
            },
        )
        .unwrap();
        tool_calls::update(
            store.conn(),
            "changed-call",
            "failed",
            Some("once"),
            Some("original audit output"),
            Some(2),
        )
        .unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "changed-call".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"lib.rs"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let tools = vega_tools::Tools::new(dir.path()).unwrap();

        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Read changed input",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Failed
                    && result.output == vega_runtime::CALL_ID_CONFLICT_OUTPUT
        )));
        let persisted: (String, String, String) = store
            .conn()
            .query_row(
                "SELECT input_json, status, output_text FROM tool_calls WHERE id = 'changed-call'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            persisted,
            (
                r#"{"path":"original.txt"}"#.into(),
                "failed".into(),
                "original audit output".into()
            )
        );
    }

    #[tokio::test]
    async fn invalid_write_is_atomically_audited_without_execution() {
        let (store, dir, _project_id) = setup();
        let data = tempdir().unwrap();
        let checkpoint_root = data.path().join("checkpoints");
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "write-1".into(),
                    name: "write".into(),
                    input_json: "{}".into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let run = run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Write",
            "System",
            CancellationToken::new(),
            |_| Ok(()),
            PersistenceActorConfig {
                checkpoint_root: Some(checkpoint_root),
                ..PersistenceActorConfig::default()
            },
        )
        .await
        .unwrap();
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Rejected && result.output.contains("invalid write input")
        )));
        let status: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, approval FROM tool_calls WHERE id = 'write-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let approval = ApprovalAudit::from_json(&status.1).unwrap();
        assert_eq!(status.0, "rejected");
        assert_eq!(approval.decision, Approval::Deny);
        assert_eq!(approval.source, ApprovalSource::Validation);
        assert!(!run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Failed
                    || matches!(result.status, ToolCallStatus::Approved | ToolCallStatus::Running)
        )));
    }

    #[tokio::test]
    async fn cancellation_is_persisted_as_interrupted_under_one_second() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![
            ScriptStep::delay(Duration::from_secs(30)),
            ScriptStep::text("late"),
        ]);
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            trigger.cancel();
        });
        let started = Instant::now();
        let run = run_thread_task(
            &store, &provider, &tools, "thread-1", "Wait", "System", cancel,
        )
        .await
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(run.interrupted);
        assert!(matches!(
            run.events.last(),
            Some(ConversationEvent::Interrupted { .. })
        ));
        let status: String = store
            .conn()
            .query_row(
                "SELECT status FROM messages WHERE id = ?1",
                [&run.assistant_message_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "interrupted");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_during_tool_persists_output_as_cancelled_and_starts_nothing_else() {
        let (store, dir, _project_id) = setup();
        let slow_path = dir.path().join("slow.txt");
        let status = std::process::Command::new("mkfifo")
            .arg(&slow_path)
            .status()
            .unwrap();
        assert!(status.success());
        let writer = std::thread::spawn(move || {
            let mut pipe = fs::OpenOptions::new().write(true).open(slow_path).unwrap();
            pipe.write_all(b"auditable output\n").unwrap();
            std::thread::sleep(Duration::from_millis(50));
        });
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "slow-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"slow.txt"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "must-not-start".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        let mut cancellation_scheduled = false;

        let run = run_thread_task_with_sink(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Read slowly",
            "System",
            cancel,
            |event| {
                if !cancellation_scheduled
                    && matches!(event, ConversationEvent::ToolCallApproved { .. })
                {
                    cancellation_scheduled = true;
                    let trigger = trigger.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        trigger.cancel();
                    });
                }
                Ok(())
            },
        )
        .await
        .unwrap();
        writer.join().unwrap();

        assert!(run.interrupted);
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { call_id, result }
                if call_id == "slow-call"
                    && result.status == ToolCallStatus::Cancelled
                    && !result.output.is_empty()
        )));
        assert!(matches!(
            run.events.last(),
            Some(ConversationEvent::Interrupted { .. })
        ));
        let persisted: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, output_text FROM tool_calls WHERE id = 'slow-call'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted.0, "cancelled");
        assert!(!persisted.1.is_empty());
        let second_count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE id = 'must-not-start'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(second_count, 0);
        let assistant_status: String = store
            .conn()
            .query_row(
                "SELECT status FROM messages WHERE id = ?1",
                [&run.assistant_message_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assistant_status, "interrupted");
        assert_eq!(provider.requests().len(), 1);
    }

    #[tokio::test]
    async fn live_sink_failure_stops_runtime_and_marks_assistant_failed() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("partial".into()),
            ProviderEvent::ToolUse {
                id: "must-not-run".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);

        let error = run_thread_task_with_sink(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Fail sink",
            "System",
            CancellationToken::new(),
            |event| {
                if matches!(event, ConversationEvent::TextDelta { .. }) {
                    return Err(VegaError::Tool {
                        tool: "event-sink".into(),
                        message: "consumer unavailable".into(),
                    });
                }
                Ok(())
            },
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("consumer unavailable"));
        let assistant: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, content FROM messages WHERE role = 'assistant' ORDER BY seq DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(assistant, ("failed".into(), "partial".into()));
        let tool_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))
            .unwrap();
        assert_eq!(tool_count, 0);
    }

    #[tokio::test]
    async fn message_started_sink_failure_is_finalized_without_hanging() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let mut surfaced = None;

        let error = tokio::time::timeout(
            Duration::from_millis(500),
            run_thread_task_with_sink(
                &store,
                &provider,
                &tools,
                "thread-1",
                "Fail start sink",
                "System",
                CancellationToken::new(),
                |event| match event {
                    ConversationEvent::MessageStarted { .. } => Err(VegaError::Tool {
                        tool: "event-sink".into(),
                        message: "start consumer unavailable".into(),
                    }),
                    ConversationEvent::Error { error, .. } => {
                        surfaced = Some(error.clone());
                        Ok(())
                    }
                    _ => Ok(()),
                },
            ),
        )
        .await
        .expect("MessageStarted sink failure must not hang")
        .unwrap_err();

        assert!(matches!(
            error,
            ConversationError::Runtime(ref error)
                if matches!(
                    error.as_ref(),
                    VegaError::Tool { tool, message }
                        if tool == "event-sink" && message == "start consumer unavailable"
                )
        ));
        assert!(matches!(
            surfaced.as_deref(),
            Some(VegaError::Tool { tool, message })
                if tool == "event-sink" && message == "start consumer unavailable"
        ));
        assert!(provider.requests().is_empty());
        let assistant_status: String = store
            .conn()
            .query_row(
                "SELECT status FROM messages WHERE role = 'assistant' ORDER BY seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assistant_status, "failed");
    }

    #[tokio::test]
    async fn provider_error_maps_to_error_event_and_failed_message() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![
            ScriptStep::text("partial"),
            ScriptStep::Error {
                status: Some(503),
                message: "unavailable".into(),
                retryable: false,
            },
        ]);
        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Fail",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(run.failed);
        assert!(matches!(
            run.events.as_slice(),
            [
                ConversationEvent::MessageStarted { .. },
                ConversationEvent::TextDelta { delta, .. },
                ConversationEvent::Error { error, .. }
            ]
                if matches!(
                    error.as_ref(),
                    VegaError::Provider {
                        status: Some(503),
                        message,
                        retryable: false,
                    } if message == "unavailable"
                ) && delta == "partial"
        ));
        let persisted: (String, String) = store
            .conn()
            .query_row(
                "SELECT content, status FROM messages WHERE id = ?1",
                [&run.assistant_message_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted, ("partial".into(), "failed".into()));
    }

    #[tokio::test]
    async fn tool_failure_is_persisted_and_the_model_can_still_converge() {
        let (store, dir, _project_id) = setup();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "missing-read".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"missing.txt"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("Handled the missing file.".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
        ]);
        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Read missing",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(run.content, "Handled the missing file.");
        let persisted: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, output_text FROM tool_calls WHERE id = 'missing-read'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(persisted.0, "failed");
        assert!(persisted.1.contains("not found"));
    }

    #[tokio::test]
    async fn assembles_system_and_history_by_sequence_with_current_user_last() {
        let (store, dir, _project_id) = setup();
        for (id, seq, role, content, status) in [
            ("history-3", 3, "assistant", "failed answer", "failed"),
            ("history-1", 1, "user", "old question", "done"),
            ("history-2", 2, "assistant", "partial answer", "interrupted"),
        ] {
            messages::insert(
                store.conn(),
                &messages::MessageRow {
                    id: id.into(),
                    thread_id: "thread-1".into(),
                    seq,
                    role: role.into(),
                    kind: "text".into(),
                    content: content.into(),
                    status: status.into(),
                    created_at: seq,
                },
            )
            .unwrap();
        }
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("answer".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]);
        run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "current question",
            "system first",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let requests = provider.requests();
        let history: Vec<(vega_runtime::ChatRole, &str)> = requests[0]
            .messages
            .iter()
            .map(|message| (message.role, message.content.as_str()))
            .collect();
        assert_eq!(
            history,
            vec![
                (vega_runtime::ChatRole::System, "system first"),
                (vega_runtime::ChatRole::User, "old question"),
                (vega_runtime::ChatRole::Assistant, "partial answer"),
                (vega_runtime::ChatRole::Assistant, "failed answer"),
                (vega_runtime::ChatRole::User, "current question"),
            ]
        );
    }

    #[tokio::test]
    async fn always_permission_and_rule_are_durable_before_second_write() {
        let (store, project_dir, _data_dir, project_id) = setup_external("confirm");
        let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "write-first".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"same.txt","content":"first-secret"}"#.into(),
                },
                ProviderEvent::ToolUse {
                    id: "write-second".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"same.txt","content":"second-secret"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let calls = Arc::new(AtomicUsize::new(0));
        let hook = FixedPermissionHook {
            calls: calls.clone(),
            decision: PermissionDecision::Always,
        };
        let run = run_thread_task_with_permission_sink(
            &store,
            &provider,
            &tools,
            "thread-1",
            "write twice",
            "system",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            fs::read_to_string(project_dir.path().join("same.txt")).unwrap(),
            "second-secret"
        );
        let rules = permissions::list_exact(store.conn(), &project_id).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].tool, "write");
        assert_eq!(rules[0].pattern, "same.txt");
        let approvals = ["write-first", "write-second"].map(|id| {
            let json: String = store
                .conn()
                .query_row(
                    "SELECT approval FROM tool_calls WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .unwrap();
            ApprovalAudit::from_json(&json).unwrap()
        });
        assert_eq!(approvals[0].source, ApprovalSource::User);
        assert_eq!(approvals[0].decision, Approval::Always);
        assert_eq!(approvals[1].source, ApprovalSource::Rule);
        assert_eq!(approvals[1].decision, Approval::Always);
        assert!(run.events.iter().all(|event| {
            !format!("{event:?}").contains("first-secret")
                && !format!("{event:?}").contains("second-secret")
        }));
    }

    #[tokio::test]
    async fn danger_readonly_always_rejects_and_persists_rule_atomically() {
        let (store, project_dir, data_dir, project_id) = setup_external("readonly");
        let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "danger-1".into(),
                    name: "bash".into(),
                    input_json: r#"{"cmd":"rm -rf /"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let calls = Arc::new(AtomicUsize::new(0));
        let hook = FixedPermissionHook {
            calls: calls.clone(),
            decision: PermissionDecision::Always,
        };
        let run = run_thread_task_with_permission_sink(
            &store,
            &provider,
            &tools,
            "thread-1",
            "danger",
            "system",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let (status, approval_json): (String, String) = store
            .conn()
            .query_row(
                "SELECT status, approval FROM tool_calls WHERE id = 'danger-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "rejected");
        let approval = ApprovalAudit::from_json(&approval_json).unwrap();
        assert_eq!(approval.source, ApprovalSource::ReadOnly);
        assert_eq!(approval.decision, Approval::Deny);
        assert_eq!(
            approval.danger.as_ref().map(|danger| danger.decision),
            Some(Approval::Always)
        );
        let rules = permissions::list_exact(store.conn(), &project_id).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].tool, "bash");
        assert_eq!(rules[0].pattern, "rm -rf /");
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Rejected
        )));
        assert_eq!(
            fs::read_dir(data_dir.path().join("checkpoints"))
                .unwrap()
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn write_edit_and_bash_execute_serially_with_strict_db_results() {
        let (store, project_dir, data_dir, _project_id) = setup_external("auto");
        fs::write(project_dir.path().join("serial.txt"), "initial").unwrap();
        let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "write-1".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"serial.txt","content":"hello"}"#.into(),
                },
                ProviderEvent::ToolUse {
                    id: "edit-1".into(),
                    name: "edit".into(),
                    input_json:
                        r#"{"path":"serial.txt","old_string":"hello","new_string":"world"}"#.into(),
                },
                ProviderEvent::ToolUse {
                    id: "bash-1".into(),
                    name: "bash".into(),
                    input_json: r#"{"cmd":"cat serial.txt"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let run = run_thread_task_with_permission_sink(
            &store,
            &provider,
            &tools,
            "thread-1",
            "serial tools",
            "system",
            CancellationToken::new(),
            &FixedPermissionHook {
                calls: Arc::new(AtomicUsize::new(0)),
                decision: PermissionDecision::Deny { note: None },
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read_to_string(project_dir.path().join("serial.txt")).unwrap(),
            "world"
        );
        let finished = run
            .events
            .iter()
            .filter_map(|event| match event {
                ConversationEvent::ToolCallFinished { call_id, result } => {
                    Some((call_id.as_str(), result.status))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            finished,
            vec![
                ("write-1", ToolCallStatus::Success),
                ("edit-1", ToolCallStatus::Success),
                ("bash-1", ToolCallStatus::Success),
            ]
        );
        let rows = ["write-1", "edit-1", "bash-1"].map(|id| {
            store
                .conn()
                .query_row(
                    "SELECT status, output_text, exit_code, duration_ms, output_full_path FROM tool_calls WHERE id = ?1",
                    [id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<i32>>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .unwrap()
        });
        assert!(rows.iter().all(|row| row.0 == "success" && row.4.is_none()));
        assert!(vega_tools::WriteSuccessOutput::from_json(&rows[0].1).is_ok());
        assert!(vega_tools::EditSuccessOutput::from_json(&rows[1].1).is_ok());
        assert!(rows[2].1.contains("world"));
        assert_eq!(rows[2].2, Some(0));
        assert!(rows[2].3.is_some());
        assert!(data_dir.path().join("checkpoints").exists());
    }

    #[test]
    fn strict_projection_validation_is_semantic_and_binds_results_and_danger() {
        let project = tempdir().unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let audit = tools
            .audit_write_json(r#"{"path":"bound.txt","content":"body"}"#)
            .unwrap();
        let canonical = audit.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&canonical).unwrap();
        let reordered = format!(
            r#"{{"fingerprint_v1":{},"content_bytes":{},"path":{},"tool":{},"audit_version":{}}}"#,
            value["fingerprint_v1"],
            value["content_bytes"],
            value["path"],
            value["tool"],
            value["audit_version"]
        );
        assert!(tool_inputs_semantically_equal(
            "write", &canonical, &reordered
        ));

        let invalid = vega_tools::InvalidMutation::from_raw(
            vega_tools::MutationTool::Write,
            r#"{"path":"x","content":"secret","extra":true}"#,
            vega_tools::MutationErrorCode::UnexpectedField,
        )
        .unwrap();
        let invalid_json = invalid.audit().to_json().unwrap();
        let invalid_value: serde_json::Value = serde_json::from_str(&invalid_json).unwrap();
        let invalid_reordered = format!(
            r#"{{"validation_error_code":{},"raw_input_sha256":{},"raw_input_bytes":{},"tool":{},"audit_version":{}}}"#,
            invalid_value["validation_error_code"],
            invalid_value["raw_input_sha256"],
            invalid_value["raw_input_bytes"],
            invalid_value["tool"],
            invalid_value["audit_version"]
        );
        assert!(tool_inputs_semantically_equal(
            "write",
            &invalid_json,
            &invalid_reordered
        ));

        let ids = vega_tools::CheckpointIds::new("project", "thread", "call").unwrap();
        let success = vega_tools::WriteSuccessOutput {
            path: "bound.txt".to_string(),
            bytes_written: 4,
            checkpoint_ref: ids.checkpoint_ref(),
        };
        let success_json = success.to_json().unwrap();
        let auto = ApprovalAudit {
            decision: Approval::Once,
            note: None,
            source: ApprovalSource::Auto,
            danger: None,
        };
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "call",
                "write",
                &reordered,
                &success_json,
                RuntimeToolStatus::Success,
                &auto,
                None,
                None,
            )
            .is_ok()
        );
        for corrupt_output in [
            vega_tools::WriteSuccessOutput {
                path: "other.txt".to_string(),
                ..success.clone()
            }
            .to_json()
            .unwrap(),
            vega_tools::WriteSuccessOutput {
                bytes_written: 5,
                ..success.clone()
            }
            .to_json()
            .unwrap(),
            vega_tools::WriteSuccessOutput {
                checkpoint_ref: vega_tools::CheckpointIds::new("project", "thread", "other")
                    .unwrap()
                    .checkpoint_ref(),
                ..success.clone()
            }
            .to_json()
            .unwrap(),
            "SECRET_RECOVERY_BODY".to_string(),
        ] {
            assert!(
                validate_recovered_projection(
                    "project",
                    "thread",
                    "call",
                    "write",
                    &canonical,
                    &corrupt_output,
                    RuntimeToolStatus::Success,
                    &auto,
                    None,
                    None,
                )
                .is_err()
            );
        }

        let validation = ApprovalAudit {
            decision: Approval::Deny,
            note: None,
            source: ApprovalSource::Validation,
            danger: None,
        };
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "invalid",
                "write",
                &invalid_reordered,
                invalid.tool_result(),
                RuntimeToolStatus::Rejected,
                &validation,
                None,
                None,
            )
            .is_ok()
        );

        let dangerous = r#"{"cmd":"rm -rf /"}"#;
        let safe = r#"{"cmd":"printf safe"}"#;
        let wrong_danger = crate::types::DangerAudit {
            rule_id: "wrong".to_string(),
            decision: Approval::Once,
            note: None,
        };
        for (input, approval) in [
            (dangerous, auto.clone()),
            (
                dangerous,
                ApprovalAudit {
                    decision: Approval::Once,
                    note: None,
                    source: ApprovalSource::Danger,
                    danger: Some(wrong_danger.clone()),
                },
            ),
            (
                dangerous,
                ApprovalAudit {
                    decision: Approval::Once,
                    note: None,
                    source: ApprovalSource::Legacy,
                    danger: None,
                },
            ),
            (
                safe,
                ApprovalAudit {
                    decision: Approval::Once,
                    note: None,
                    source: ApprovalSource::Danger,
                    danger: Some(wrong_danger),
                },
            ),
        ] {
            assert!(
                validate_recovered_projection(
                    "project",
                    "thread",
                    "bash-call",
                    "bash",
                    input,
                    "output",
                    RuntimeToolStatus::Success,
                    &approval,
                    Some(0),
                    Some(1),
                )
                .is_err()
            );
        }

        let recovery = ApprovalAudit {
            decision: Approval::Deny,
            note: None,
            source: ApprovalSource::Recovery,
            danger: None,
        };
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "unknown",
                "future_tool",
                "{}",
                vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
                RuntimeToolStatus::Rejected,
                &recovery,
                None,
                None,
            )
            .is_ok()
        );
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "unknown",
                "future_tool",
                "{\"secret\":true}",
                vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
                RuntimeToolStatus::Rejected,
                &recovery,
                None,
                None,
            )
            .is_err()
        );

        let legacy_deny = ApprovalAudit {
            decision: Approval::Deny,
            note: None,
            source: ApprovalSource::Legacy,
            danger: None,
        };
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "legacy",
                "write",
                &canonical,
                &legacy_unavailable_output("write"),
                RuntimeToolStatus::Rejected,
                &legacy_deny,
                None,
                None,
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn file_backed_recovery_reuses_write_edit_and_unknown_without_execution() {
        let (store, project_dir, data_dir, _project_id) = setup_external("auto");
        fs::write(project_dir.path().join("edit.txt"), "old").unwrap();
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: "old-assistant".into(),
                thread_id: "thread-1".into(),
                seq: 1,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "interrupted".into(),
                created_at: 1,
            },
        )
        .unwrap();
        let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
        let write_raw = r#"{"path":"new.txt","content":"recovery-secret"}"#;
        let edit_raw = r#"{"path":"edit.txt","old_string":"old","new_string":"recovery-new"}"#;
        let write_audit = tools
            .audit_write_json(write_raw)
            .unwrap()
            .to_json()
            .unwrap();
        let edit_audit = tools.audit_edit_json(edit_raw).unwrap().to_json().unwrap();
        for (seq, (id, tool, input, status)) in [
            (
                "recover-write",
                "write",
                write_audit.as_str(),
                "pending_approval",
            ),
            ("recover-edit", "edit", edit_audit.as_str(), "running"),
            ("recover-unknown", "future_tool", "{}", "pending_approval"),
        ]
        .into_iter()
        .enumerate()
        {
            tool_calls::insert(
                store.conn(),
                tool_calls::NewToolCall {
                    id,
                    thread_id: "thread-1",
                    message_id: "old-assistant",
                    seq: i64::try_from(seq + 1).unwrap(),
                    tool,
                    input_json: input,
                    status,
                    created_at: 1,
                },
            )
            .unwrap();
        }
        let auto_json = ApprovalAudit {
            decision: Approval::Once,
            note: None,
            source: ApprovalSource::Auto,
            danger: None,
        }
        .to_json()
        .unwrap();
        tool_calls::update(
            store.conn(),
            "recover-edit",
            "running",
            Some(&auto_json),
            None,
            None,
        )
        .unwrap();
        let database_path = data_dir.path().join("vega.db");
        drop(store);
        let reopened = Store::open(&database_path).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "recover-write".into(),
                    name: "write".into(),
                    input_json: write_raw.into(),
                },
                ProviderEvent::ToolUse {
                    id: "recover-edit".into(),
                    name: "edit".into(),
                    input_json: edit_raw.into(),
                },
                ProviderEvent::ToolUse {
                    id: "recover-unknown".into(),
                    name: "future_tool".into(),
                    input_json: r#"{"secret":"must-not-survive"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let run = run_thread_task(
            &reopened,
            &provider,
            &tools,
            "thread-1",
            "resume",
            "system",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let reused = run
            .events
            .iter()
            .filter_map(|event| match event {
                ConversationEvent::ToolCallFinished { call_id, result } if result.reused => {
                    Some((call_id.as_str(), result.status))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reused,
            vec![
                ("recover-write", ToolCallStatus::Rejected),
                ("recover-edit", ToolCallStatus::Cancelled),
                ("recover-unknown", ToolCallStatus::Rejected),
            ]
        );
        assert!(!project_dir.path().join("new.txt").exists());
        assert_eq!(
            fs::read_to_string(project_dir.path().join("edit.txt")).unwrap(),
            "old"
        );
        let wire = format!("{:?}", provider.requests());
        assert!(!wire.contains("recovery-secret"));
        assert!(!wire.contains("recovery-new"));
        assert!(!wire.contains("must-not-survive"));
    }
}
