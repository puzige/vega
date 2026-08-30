//! Conversation-layer orchestration for the headless runtime (S4-T20).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    AgentRequest, Provider, RuntimeEvent, RuntimeToolStatus, VegaError, run_agent_with_sink,
};
use vega_store::{Store, messages, token_usage, tool_calls};

use crate::types::{ConversationError, ConversationEvent, from_runtime_event};

const HISTORY_WINDOW: usize = 50;
const TEXT_BATCH_MAX_DELAY: Duration = Duration::from_millis(4);
const TEXT_BATCH_MAX_BYTES: usize = 4 * 1024;
const PERSISTENCE_CHANNEL_CAPACITY: usize = 64;

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
        event: RuntimeEvent,
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
                event,
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
    run_thread_task_with_sink(
        store,
        provider,
        tools,
        thread_id,
        user_content,
        system_prompt,
        cancel,
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
    run_thread_task_with_sink_config(
        store,
        provider,
        tools,
        thread_id,
        user_content,
        system_prompt,
        cancel,
        event_sink,
        PersistenceActorConfig::default(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_thread_task_with_sink_config<F>(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    user_content: &str,
    system_prompt: &str,
    cancel: CancellationToken,
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
    let runtime_future = run_agent_with_sink(
        provider,
        tools,
        prepared.request,
        task_cancel,
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
        .map_err(runtime_store_error)?
        .into_iter()
        .map(|(call_id, call)| -> Result<_, ConversationError> {
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
            let completed = vega_runtime::CompletedToolCall {
                tool: call.tool,
                input_json: call.input_json,
                result: vega_runtime::RuntimeToolResult {
                    call_id: call_id.clone(),
                    output: call.output,
                    status,
                    reused: true,
                },
            };
            Ok((call_id, completed))
        })
        .collect::<Result<_, _>>()?;
    let next_tool_seq =
        tool_calls::next_seq(&transaction, &thread_id).map_err(runtime_store_error)?;
    transaction.commit().map_err(runtime_store_error)?;

    Ok(PreparedRun {
        database_path,
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
        },
        next_tool_seq,
    })
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
                Ok(()) => match actor.event(event.clone(), streamed_content.clone()).await {
                    Ok(()) => {
                        if let Some(converted) = from_runtime_event(message_id, &event) {
                            match event_sink(&converted) {
                                Ok(()) => {
                                    events.push(converted);
                                    Ok(())
                                }
                                Err(error) => Err(error),
                            }
                        } else {
                            Ok(())
                        }
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

fn persist_runtime_event(
    store: &Store,
    thread_id: &str,
    message_id: &str,
    model: &str,
    streamed_content: &str,
    next_tool_seq: &mut i64,
    event: &RuntimeEvent,
) -> Result<(), VegaError> {
    match event {
        RuntimeEvent::ToolCallProposed(call) => {
            if let Some(existing) = tool_calls::find_identity(store.conn(), &call.id)? {
                if existing.thread_id != thread_id
                    || existing.tool != call.name
                    || existing.input_json != call.input_json
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
                tool_calls::insert(
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
                )?;
                *next_tool_seq += 1;
            }
        }
        RuntimeEvent::ToolCallApproved { call_id } => {
            ensure_tool_updated(
                tool_calls::update(store.conn(), call_id, "approved", Some("once"), None, None)?,
                call_id,
            )?;
        }
        RuntimeEvent::ToolCallRunning { call_id } => {
            ensure_tool_updated(
                tool_calls::update(store.conn(), call_id, "running", None, None, None)?,
                call_id,
            )?;
        }
        RuntimeEvent::ToolCallFinished(result) if !result.reused => {
            let (status, approval) = match result.status {
                RuntimeToolStatus::Rejected => ("rejected", Some("deny")),
                RuntimeToolStatus::Success => ("success", Some("once")),
                RuntimeToolStatus::Failed => ("failed", Some("once")),
                RuntimeToolStatus::Cancelled => ("cancelled", Some("once")),
            };
            ensure_tool_updated(
                tool_calls::update(
                    store.conn(),
                    &result.call_id,
                    status,
                    approval,
                    Some(&result.output),
                    Some(now_ms()),
                )?,
                &result.call_id,
            )?;
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

fn ensure_tool_updated(updated: usize, call_id: &str) -> Result<(), VegaError> {
    if updated == 0 {
        Err(VegaError::Tool {
            tool: "runtime".to_string(),
            message: format!("tool call row disappeared: {call_id}"),
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
    use std::time::{Duration, Instant};

    use tempfile::tempdir;
    use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason};

    use super::*;
    use crate::types::{ConversationEvent, ToolCallStatus};

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
        assert_eq!(tool.1, "once");
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
                        assert_eq!(status, "running");
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
        for (call_id, status, approval, output, expected) in [
            (
                "failed-call",
                "failed",
                "once",
                "original failed output",
                ToolCallStatus::Failed,
            ),
            (
                "rejected-call",
                "rejected",
                "deny",
                "original rejected output",
                ToolCallStatus::Rejected,
            ),
            (
                "cancelled-call",
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
                    tool: "read",
                    input_json: r#"{"path":"missing.txt"}"#,
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
                        name: "read".into(),
                        input_json: r#"{"path":"missing.txt"}"#.into(),
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
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "shared-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
        let tools = vega_tools::Tools::new(dir.path()).unwrap();

        let error = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Read",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("collides with persisted owner"));
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
        let failed_assistants: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE thread_id = 'thread-1' AND role = 'assistant' AND status = 'failed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed_assistants, 1);
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
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "changed-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
        let tools = vega_tools::Tools::new(dir.path()).unwrap();

        let error = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Read changed input",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("collides with persisted owner"));
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
    async fn rejected_unknown_tool_is_audited_without_execution() {
        let (store, dir, _project_id) = setup();
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
        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Write",
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Rejected && result.output.contains("denied")
        )));
        let status: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, approval FROM tool_calls WHERE id = 'write-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, ("rejected".into(), "deny".into()));
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
}
