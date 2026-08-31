use super::*;

/// Persisted task result and the sole event stream exposed to UI/store
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

impl fmt::Debug for ConversationRun {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConversationRun")
            .field("user_message_id_bytes", &self.user_message_id.len())
            .field(
                "assistant_message_id_bytes",
                &self.assistant_message_id.len(),
            )
            .field("event_count", &self.events.len())
            .field("content_bytes", &self.content.len())
            .field("interrupted", &self.interrupted)
            .field("failed", &self.failed)
            .finish()
    }
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

/// Starts the Execute turn created by a committed Plan approval without
/// inserting a duplicate user instruction. The durable instruction is loaded
/// and validated before the runtime starts.
pub async fn run_approved_plan_task(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    instruction_message_id: &str,
    system_prompt: &str,
    cancel: CancellationToken,
) -> Result<ConversationRun, ConversationError> {
    run_approved_plan_task_with_permission_sink(
        store,
        provider,
        tools,
        thread_id,
        instruction_message_id,
        system_prompt,
        cancel,
        &RejectPermissionHook,
        |_| Ok(()),
    )
    .await
}

/// Permission-aware variant of [`run_approved_plan_task`]. The review
/// transaction has already committed before this function can call provider.
#[allow(clippy::too_many_arguments)]
pub async fn run_approved_plan_task_with_permission_sink<F>(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    instruction_message_id: &str,
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
        crate::plans::APPROVAL_INSTRUCTION,
        system_prompt,
        cancel,
        permission_hook,
        event_sink,
        PersistenceActorConfig::default(),
        Some(instruction_message_id.to_string()),
        None,
    )
    .await
}

/// Runs an approved Plan task with a frozen pricing capability (S7-T39/C3
/// app wiring): identical to
/// [`run_approved_plan_task_with_permission_sink`] except the immutable
/// run-start selection rides into every agentic round, so each provider call
/// is priced exactly once with the frozen model and UTC timestamp.
#[allow(clippy::too_many_arguments)]
pub async fn run_approved_plan_task_with_pricing<F>(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    instruction_message_id: &str,
    system_prompt: &str,
    cancel: CancellationToken,
    permission_hook: &dyn PermissionHook,
    event_sink: F,
    pricing_catalog: Option<vega_token::PricingCatalog>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    run_thread_task_with_permission_config(
        store,
        provider,
        tools,
        thread_id,
        crate::plans::APPROVAL_INSTRUCTION,
        system_prompt,
        cancel,
        permission_hook,
        event_sink,
        PersistenceActorConfig::default(),
        Some(instruction_message_id.to_string()),
        pricing_catalog,
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
        None,
        None,
    )
    .await
}

/// Runs a thread task with a frozen pricing capability (S7-T38/C3): the run
/// preflight resolves the durable `Thread.model` against the injected catalog
/// before any provider request; the selection is immutable for the whole run
/// (Settings changes mid-run never swap it).
///
/// `pricing_catalog: None` keeps legacy unpriced rows (zero cost, NULL
/// provenance columns). A model missing from the catalog is not an error:
/// per C3 the run proceeds unpriced so the user can be guided to Settings.
#[allow(clippy::too_many_arguments)]
pub async fn run_thread_task_with_pricing<F>(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    user_content: &str,
    system_prompt: &str,
    cancel: CancellationToken,
    permission_hook: &dyn PermissionHook,
    event_sink: F,
    actor_config: PersistenceActorConfig,
    persisted_user_message_id: Option<String>,
    pricing_catalog: Option<vega_token::PricingCatalog>,
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
        actor_config,
        persisted_user_message_id,
        pricing_catalog,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) async fn run_thread_task_with_sink_config<F>(
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
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_thread_task_with_permission_config<F>(
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
    persisted_user_message_id: Option<String>,
    pricing_catalog: Option<vega_token::PricingCatalog>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    let user_message_id = persisted_user_message_id
        .clone()
        .unwrap_or_else(|| ulid::Ulid::generate().to_string());
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
    let preparation_uses_existing_user = persisted_user_message_id.is_some();
    let preparation_pricing = pricing_catalog;
    let prepared = match tokio::task::spawn_blocking(move || {
        prepare_run(
            preparation_path,
            preparation_thread_id,
            preparation_user_content,
            preparation_system_prompt,
            preparation_user_id,
            preparation_assistant_id,
            preparation_config,
            preparation_uses_existing_user,
            preparation_pricing,
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

    let actor = match PersistenceActor::start(PersistenceActorStart {
        database_path: prepared.database_path.clone(),
        project_id: prepared.project_id.clone(),
        thread_id: thread_id.to_string(),
        message_id: prepared.assistant_message_id.clone(),
        model: prepared.model.clone(),
        is_plan: prepared.is_plan,
        next_tool_seq: prepared.next_tool_seq,
        config: actor_config,
    })
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
