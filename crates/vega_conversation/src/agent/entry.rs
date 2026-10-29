use super::*;

struct WorkingUserQuestionHook {
    inner: Arc<dyn vega_runtime::RuntimeUserQuestionHook>,
    clock: crate::types::WorkClock,
}

impl vega_runtime::RuntimeUserQuestionHook for WorkingUserQuestionHook {
    fn request(
        &self,
        request: vega_runtime::RuntimeUserQuestionRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, vega_runtime::RuntimeUserQuestionResponse> {
        let pause = Arc::new(self.clock.pause());
        let resolution_pause = Arc::downgrade(&pause);
        let response = self.inner.request_with_resolution(
            request,
            cancel,
            Arc::new(move || {
                if let Some(pause) = resolution_pause.upgrade() {
                    pause.finish();
                }
            }),
        );
        async move {
            let response = response.await;
            drop(pause);
            response
        }
        .boxed()
    }
}

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

/// Runs an approved Plan task with both the frozen pricing capability and the
/// frozen provider/model reasoning selection.  The selection is passed once
/// into run preparation and is then reused by every runtime round.
#[allow(clippy::too_many_arguments)]
pub async fn run_approved_plan_task_with_pricing_and_reasoning<F>(
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
    reasoning: Option<FrozenReasoning>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    run_thread_task_with_permission_config_and_reasoning(
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
        reasoning,
    )
    .await
}

/// Approved-plan resume with the same run-start MCP authority as an ordinary
/// submitted task. The persisted instruction is reused exactly once; no new
/// user message or separate capability registry is created.
#[allow(clippy::too_many_arguments)]
pub async fn run_approved_plan_task_with_pricing_reasoning_and_mcp<F>(
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
    reasoning: Option<FrozenReasoning>,
    mcp_servers: Vec<vega_runtime::McpReadyServer>,
    actor_config: PersistenceActorConfig,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    run_thread_task_with_images_reasoning_and_mcp(
        store,
        provider,
        tools,
        thread_id,
        crate::plans::APPROVAL_INSTRUCTION,
        system_prompt,
        cancel,
        permission_hook,
        event_sink,
        actor_config,
        Some(instruction_message_id.to_string()),
        pricing_catalog,
        reasoning,
        Vec::new(),
        mcp_servers,
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

/// Runs a thread task with frozen pricing and reasoning capabilities.  This
/// is the production entry point used by the app worker after it has resolved
/// the exact provider/model profile at run start.
#[allow(clippy::too_many_arguments)]
pub async fn run_thread_task_with_pricing_and_reasoning<F>(
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
    reasoning: Option<FrozenReasoning>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    run_thread_task_with_permission_config_and_reasoning(
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
        reasoning,
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
    event_sink: F,
    actor_config: PersistenceActorConfig,
    persisted_user_message_id: Option<String>,
    pricing_catalog: Option<vega_token::PricingCatalog>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    run_thread_task_with_permission_config_and_reasoning(
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
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_thread_task_with_permission_config_and_reasoning<F>(
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
    reasoning: Option<FrozenReasoning>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    run_thread_task_with_images_and_reasoning(
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
        reasoning,
        Vec::new(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_thread_task_with_images_and_reasoning<F>(
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
    reasoning: Option<FrozenReasoning>,
    images: Vec<crate::types::ImageAttachment>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    run_thread_task_with_images_reasoning_and_mcp(
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
        reasoning,
        images,
        Vec::new(),
    )
    .await
}

/// Runs an ordinary task with ready external servers supplied by the app's
/// Settings controller. The runtime freezes their exact schemas and aliases
/// before provider round one; no UI or SQLite layer can inject later tools.
#[allow(clippy::too_many_arguments)]
pub async fn run_thread_task_with_images_reasoning_and_mcp<F>(
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
    reasoning: Option<FrozenReasoning>,
    images: Vec<crate::types::ImageAttachment>,
    mcp_servers: Vec<vega_runtime::McpReadyServer>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    let mut actor_config = actor_config;
    let admission =
        super::ordinary::verified_admission(store, thread_id, actor_config.admission.as_ref())?;
    actor_config.admission = Some(admission.clone());
    let binding = if actor_config.ordinary.is_some() {
        if actor_config.automation_claim.is_some() {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        None
    } else if let Some(path) = store.database_path() {
        let path = path.to_path_buf();
        let worker_path = path.clone();
        let thread = thread_id.to_string();
        let existing = actor_config.automation_claim.clone();
        let claim_admission = admission.clone();
        let claim = tokio::task::spawn_blocking(move || {
            let _admission = claim_admission;
            let store = Store::open(worker_path).map_err(|_| {
                vega_i18n::t("domain.automation.error.store_unavailable").to_string()
            })?;
            if let Some(claim) = existing {
                if let Some(claim) = crate::automation::activate_claim(&store, &claim)? {
                    Ok(Some(claim))
                } else {
                    crate::automation::finish_unsettled_claim(
                        &store,
                        &claim,
                        crate::automation::now_ms(),
                    )?;
                    Err(vega_i18n::t("domain.automation.error.cancelled").into())
                }
            } else {
                crate::automation::claim(&store, &thread, None, crate::automation::now_ms())
            }
        })
        .await
        .map_err(|_| {
            ConversationError::CorruptRow(vega_i18n::t("domain.automation.error.processing").into())
        })?
        .map_err(ConversationError::CorruptRow)?;
        claim.map(|claim| Arc::new(crate::automation::AutomationBinding::new(path, claim)))
    } else {
        None
    };
    actor_config.automation_claim = binding.as_ref().map(|binding| binding.claim.clone());
    actor_config.automation_hook = binding
        .clone()
        .map(|hook| hook as Arc<dyn vega_runtime::RuntimeAutomationHook>);
    let started = std::time::Instant::now();
    let result = run_thread_task_with_automation_inner(
        store,
        provider,
        tools,
        thread_id,
        user_content,
        system_prompt,
        cancel,
        permission_hook,
        &mut event_sink,
        actor_config,
        persisted_user_message_id,
        pricing_catalog,
        reasoning,
        images,
        mcp_servers,
    )
    .await;
    if let Some(binding) = binding {
        let success = result.as_ref().is_ok_and(|run| {
            !run.failed
                && !run.interrupted
                && run.events.iter().rev().find_map(|event| match event {
                    ConversationEvent::MessageFinished { stop_reason, .. } => Some(*stop_reason),
                    _ => None,
                }) == Some(crate::types::ConversationStopReason::End)
        });
        let complete = binding.usage_complete();
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        if let Some(path) = store.database_path() {
            let path = path.to_path_buf();
            tokio::task::spawn_blocking(move || {
                let store = Store::open(path).map_err(|_| {
                    vega_i18n::t("domain.automation.error.store_unavailable").to_string()
                })?;
                crate::automation::finish(
                    &store,
                    &binding.claim,
                    success,
                    complete,
                    elapsed,
                    crate::automation::now_ms(),
                )
            })
            .await
            .map_err(|_| {
                ConversationError::CorruptRow(
                    vega_i18n::t("domain.automation.error.accounting").into(),
                )
            })?
            .map_err(ConversationError::CorruptRow)?;
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_thread_task_with_automation_inner<F>(
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
    reasoning: Option<FrozenReasoning>,
    images: Vec<crate::types::ImageAttachment>,
    mcp_servers: Vec<vega_runtime::McpReadyServer>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    let _live_thread = store
        .database_path()
        .map(|path| crate::subagents::LiveDelegation::new(path, thread_id))
        .transpose()?;
    let mut actor_config = actor_config;
    let ordinary = actor_config.ordinary.clone();
    if let Some(ordinary) = &ordinary {
        ordinary.verify(store, thread_id)?;
        let accepted = ordinary
            .submission
            .snapshot()
            .accepted
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        if accepted.text() != user_content || accepted.images() != images.as_slice() {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        let model = provider
            .model_snapshot()
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        ordinary
            .submission
            .raw()
            .validate_model(&model)
            .map_err(super::ordinary::fact_error)?;
        if persisted_user_message_id.is_some()
            || actor_config.delegated
            || actor_config.delegated_message_id.is_some()
            || actor_config.credential_continuation_id.is_some()
            || actor_config.pending_message_id.is_some()
            || actor_config.automation_claim.is_some()
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
    }
    let admission =
        super::ordinary::verified_admission(store, thread_id, actor_config.admission.as_ref())?;
    let precompute_lease = actor_config.precompute_lease.take().filter(|lease| {
        lease
            .verifies(store.database_identity(), thread_id, &admission)
            .unwrap_or(false)
    });
    actor_config.admission = Some(admission.clone());
    actor_config.steering_cancel = Some(cancel.clone());
    crate::attachments::validate_images(&images)
        .map_err(|error| ConversationError::CorruptRow(error.to_string()))?;
    if persisted_user_message_id.is_some() && !images.is_empty() {
        return Err(ConversationError::CorruptRow(
            "cannot replace persisted instruction images".into(),
        ));
    }
    let continuation = actor_config
        .credential_continuation_id
        .as_deref()
        .map(|id| {
            vega_store::credential_continuation::find(store.conn(), id).map_err(runtime_store_error)
        })
        .transpose()?
        .flatten();
    let user_message_id = ordinary
        .as_ref()
        .map(|run| run.submission.user_message_id().to_owned())
        .or_else(|| {
            continuation
                .as_ref()
                .map(|receipt| receipt.user_message_id.clone())
        })
        .or_else(|| persisted_user_message_id.clone())
        .or_else(|| actor_config.pending_message_id.clone())
        .unwrap_or_else(|| ulid::Ulid::generate().to_string());
    let assistant_message_id = match &ordinary {
        Some(run) => run.assistant_id()?,
        None => actor_config
            .delegated_message_id
            .clone()
            .unwrap_or_else(|| ulid::Ulid::generate().to_string()),
    };
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
    let preparation_reasoning = reasoning;
    let ordinary_preparation = ordinary
        .as_ref()
        .map(super::ordinary::OrdinaryPreparation::register)
        .transpose()?;
    let mut prepared = match tokio::task::spawn_blocking(move || {
        let result = prepare_run_with_images_and_reasoning(
            preparation_path,
            preparation_thread_id,
            preparation_user_content,
            preparation_system_prompt,
            preparation_user_id,
            preparation_assistant_id,
            preparation_config,
            preparation_uses_existing_user,
            preparation_pricing,
            preparation_reasoning,
            images,
            ordinary_preparation.as_ref(),
        );
        if let Some(preparation) = ordinary_preparation {
            preparation.complete(result.is_ok());
        }
        result
    })
    .await
    {
        Ok(Ok(prepared)) => prepared,
        Ok(Err(ConversationError::Runtime(error))) => {
            let error = match Arc::try_unwrap(error) {
                Ok(error) => Arc::new(
                    error.at_runtime_stage(vega_runtime::RuntimeFailureStage::RunPreparation),
                ),
                Err(error) => error,
            };
            forward_pipeline_error(&mut event_sink, None, error.clone());
            return Err(ConversationError::Runtime(error));
        }
        Ok(Err(error)) => return Err(error),
        Err(error) => {
            let error = Arc::new(
                persistence_actor_error(format!("preparation task join failed: {error}"))
                    .at_runtime_stage(vega_runtime::RuntimeFailureStage::RunPreparation),
            );
            forward_pipeline_error(&mut event_sink, None, error.clone());
            return Err(ConversationError::Runtime(error));
        }
    };

    if actor_config.delegated_authority.is_some() {
        prepared.request.context_budget = actor_config.delegated_context_budget;
    }
    let subagent_mcp = mcp_servers.clone();
    prepared.request.tool_config = prepared
        .request
        .tool_config
        .with_mcp_servers(mcp_servers)
        .with_computer_use(actor_config.computer_use.clone())
        .with_stage(actor_config.stage_hook.clone())
        .with_delegated_compaction(actor_config.delegated);
    let skill_launch = super::skills::prepare_skill_run(
        store,
        &prepared.project_id,
        thread_id,
        &prepared.assistant_message_id,
        persisted_user_message_id.is_none()
            && continuation.is_none()
            && actor_config.interrupted_assistant.is_none(),
    );
    let skill_launch = match skill_launch {
        Ok(launch) => launch,
        Err(error) => {
            let cleanup = finish_prepared_terminal(
                prepared.database_path.clone(),
                prepared.assistant_message_id.clone(),
                "failed",
                ordinary.clone(),
                admission.clone(),
            )
            .await;
            super::ordinary::record_cleanup(ordinary.as_ref(), cleanup.is_ok());
            forward_pipeline_error(
                &mut event_sink,
                Some(prepared.assistant_message_id.clone()),
                Arc::new(VegaError::Tool {
                    tool: "skill".to_string(),
                    message: "Skill selection unavailable or settings invalid".to_string(),
                }),
            );
            return Err(error);
        }
    };
    let skill_authority = skill_launch
        .as_ref()
        .and_then(|launch| launch.run.binding())
        .map(|binding| super::skills::authority_probe(database_path.clone(), binding));
    if let Some(launch) = skill_launch {
        prepared.request.tool_config = std::mem::take(&mut prepared.request.tool_config)
            .with_skill_run(launch.run, launch.explicit);
        if let Some(authority) = &skill_authority {
            prepared.request.tool_config = std::mem::take(&mut prepared.request.tool_config)
                .with_skill_authority_probe(Arc::clone(&authority.probe));
        }
    }

    let replay_registry = actor_config.replay_registry.clone();
    if let (Some(route), Some(frame)) = (
        actor_config.replay_route_fingerprint,
        prepared.request.replay_frame.as_mut(),
    ) {
        frame.attach_route_fingerprint(route);
    }
    if let (Some(lease), Some(frame)) = (
        prepared.replay_lease.as_ref(),
        prepared.request.replay_frame.as_mut(),
    ) && let Some(session) = lease.candidate()
        && session.frame().is_some_and(|frame| {
            frame.seal_eligible()
                && frame.environment_fingerprint() == Some(session.environment_fingerprint())
        })
    {
        frame.attach_candidate(
            session.messages().to_vec(),
            session.environment_fingerprint(),
        );
    }

    let subagent_host = actor_config.subagent_host.clone().filter(|_| {
        !actor_config.delegated && prepared.request.tool_config.run_mode == RuntimeRunMode::Execute
    });
    let subagent_host = subagent_host.filter(|_| {
        vega_store::threads::find(store.conn(), thread_id).is_ok_and(|thread| {
            thread.is_some_and(|thread| {
                thread.parent_thread_id.is_none() && thread.backend == "native"
            })
        })
    });
    let mut supervisor = subagent_host
        .as_ref()
        .map(|_| super::subagents::Supervisor::new());
    let child_launch = subagent_host.map(|host| super::subagents::ChildLaunch {
        provider,
        tools,
        database: prepared.database_path.clone(),
        parent_thread: thread_id.to_string(),
        parent_message: prepared.assistant_message_id.clone(),
        settings: super::subagents::ChildSettings::new(
            &prepared.request,
            &actor_config,
            &prepared.permission_fence,
        ),
        mcp_servers: subagent_mcp,
        host,
        cancel: cancel.child_token(),
    });
    if let Some(supervisor) = &supervisor {
        prepared.request.tool_config = prepared
            .request
            .tool_config
            .with_subagents(Some(supervisor.hook()));
    }

    let work_time_source = actor_config.work_time_source.clone();
    let user_questions = actor_config.user_questions.clone();
    let full_on_threshold = actor_config.delegated;
    let precompute_attribution = if actor_config.delegated {
        super::compaction::precompute::PrecomputeUsageAttribution {
            parent: actor_config.delegated_parent.clone(),
            claim: actor_config.automation_claim.clone(),
            observer: actor_config.automation_hook.clone(),
        }
    } else {
        Default::default()
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
            let cleanup = finish_prepared_terminal(
                prepared.database_path.clone(),
                prepared.assistant_message_id.clone(),
                "failed",
                ordinary.clone(),
                admission.clone(),
            )
            .await;
            super::ordinary::record_cleanup(ordinary.as_ref(), cleanup.is_ok());
            forward_pipeline_error(
                &mut event_sink,
                Some(prepared.assistant_message_id.clone()),
                error.clone(),
            );
            return Err(ConversationError::Runtime(error));
        }
    };

    if cancel.is_cancelled() {
        let cleanup = finish_prepared_terminal(
            prepared.database_path.clone(),
            prepared.assistant_message_id.clone(),
            "interrupted",
            ordinary.clone(),
            admission.clone(),
        )
        .await;
        let closed = actor.close().await;
        super::ordinary::record_cleanup(ordinary.as_ref(), cleanup.is_ok() && closed.is_ok());
        // The user stopped the turn before the runtime started: report the same
        // terminal stop event a runtime interruption produces so the turn keeps
        // its stop line instead of vanishing from the conversation flow.
        forward_pipeline_error_with_duration(
            &mut event_sink,
            Some(prepared.assistant_message_id.clone()),
            Arc::new(VegaError::Cancelled),
            None,
        );
        return Err(ConversationError::Runtime(Arc::new(VegaError::Cancelled)));
    }
    let runtime_started_at = std::time::Instant::now();
    let work_clock = crate::types::WorkClock::new(work_time_source);
    prepared.request.tool_config =
        prepared
            .request
            .tool_config
            .with_user_questions(user_questions.map(|inner| {
                Arc::new(WorkingUserQuestionHook {
                    inner,
                    clock: work_clock.clone(),
                }) as Arc<dyn vega_runtime::RuntimeUserQuestionHook>
            }));
    let started = ConversationEvent::MessageStarted {
        message_id: prepared.assistant_message_id.clone(),
        seq: prepared.assistant_seq as u64,
    };
    if let Err(error) = event_sink(&started) {
        let error = Arc::new(error);
        let cleanup = actor
            .event(RuntimeEvent::Error(error.clone()), String::new(), None)
            .await;
        forward_pipeline_error(
            &mut event_sink,
            Some(prepared.assistant_message_id.clone()),
            error.clone(),
        );
        let closed = actor.close().await;
        super::ordinary::record_cleanup(ordinary.as_ref(), cleanup.is_ok() && closed.is_ok());
        return Err(ConversationError::Runtime(error));
    }
    let clock = ConversationEvent::RunStarted {
        message_id: prepared.assistant_message_id.clone(),
        started_at: runtime_started_at,
        work_clock: Some(work_clock.clone()),
    };
    if let Err(error) = event_sink(&clock) {
        let error = Arc::new(error);
        let duration = u64::try_from(work_clock.finish().as_millis()).unwrap_or(u64::MAX);
        let cleanup = actor
            .event(
                RuntimeEvent::Error(error.clone()),
                String::new(),
                Some(i64::try_from(duration).unwrap_or(i64::MAX)),
            )
            .await;
        forward_pipeline_error_with_duration(
            &mut event_sink,
            Some(prepared.assistant_message_id.clone()),
            error.clone(),
            Some(duration),
        );
        let closed = actor.close().await;
        super::ordinary::record_cleanup(ordinary.as_ref(), cleanup.is_ok() && closed.is_ok());
        return Err(ConversationError::Runtime(error));
    }
    let mut events = vec![started, clock];

    let (runtime_sender, runtime_receiver) = mpsc::channel(PERSISTENCE_CHANNEL_CAPACITY);
    let task_cancel = cancel.child_token();
    let processor_cancel = task_cancel.clone();
    let skill_watch_stop = CancellationToken::new();
    let skill_task = if skill_authority.is_some() {
        super::ordinary::OrdinaryTask::register(ordinary.as_ref())?
    } else {
        None
    };
    let skill_watch = skill_authority.as_ref().map(|authority| {
        let probe = Arc::clone(&authority.probe);
        let cancel = task_cancel.clone();
        let stop = skill_watch_stop.clone();
        let admission = admission.clone();
        tokio::spawn(async move {
            let _admission = admission;
            super::skills::watch_authority(probe, cancel, stop).await;
            if let Some(task) = skill_task {
                task.complete(Ok(()));
            }
        })
    });
    let mut streamed_content = String::new();
    let permission_adapter = RuntimePermissionAdapter {
        shared: permission_hook,
        scope: super::scoped_bash::ScopedBashAuthority {
            database_path: prepared.database_path.clone(),
            fence: prepared.permission_fence.clone(),
            run_id: prepared.assistant_message_id.clone(),
        },
    };
    let diagnostics = Arc::new(
        super::diagnostics::DiagnosticsWriter::for_persistence_actor(actor.sender.clone()),
    );
    let root_attempt_id = ulid::Ulid::generate().to_string();
    diagnostics.root_event(
        thread_id,
        &prepared.assistant_message_id,
        &root_attempt_id,
        vega_store::run_diagnostics::DiagnosticState::Started,
        (None, Default::default()),
        runtime_started_at,
    );
    let diagnostics_context = super::diagnostics::DiagnosticsContext {
        writer: diagnostics.clone(),
        thread_id: thread_id.to_string(),
        run_id: prepared.assistant_message_id.clone(),
        root_attempt_id: root_attempt_id.clone(),
    };
    let context_hook = prepared.request.context_budget.map(|_| {
        ConversationCompactionHook::new(
            provider,
            prepared.database_path.clone(),
            thread_id,
            prepared.model.clone(),
            prepared.request.reasoning.clone(),
            prepared.request.pricing_catalog.clone(),
        )
        .with_full_on_threshold(full_on_threshold)
        .with_precompute_attribution(precompute_attribution)
        .with_precompute_lease(precompute_lease)
        .with_thread_admission(admission.clone())
        .with_credential_reader(prepared.request.tool_config.credential_reader())
        .with_credential_exceptions(prepared.request.tool_config.credential_exceptions())
        .with_diagnostics(diagnostics_context.clone())
    });
    let context_hook_ref = context_hook
        .as_ref()
        .map(|hook| hook as &dyn vega_runtime::ContextCompactionHook);
    let runtime_request = prepared.request.clone();
    let cancelled = cancel.is_cancelled();
    if cancelled
        || ordinary
            .as_ref()
            .is_some_and(|run| run.worker.mark_primary_started().is_err())
    {
        skill_watch_stop.cancel();
        if let Some(watch) = skill_watch {
            let _ = watch.await;
        }
        diagnostics.close().await;
        let cleanup = if cancelled {
            actor
                .event(RuntimeEvent::Interrupted, String::new(), None)
                .await
        } else {
            actor
                .event(
                    RuntimeEvent::Error(Arc::new(VegaError::Cancelled)),
                    String::new(),
                    None,
                )
                .await
        };
        let closed = actor.close().await;
        super::ordinary::record_cleanup(ordinary.as_ref(), cleanup.is_ok() && closed.is_ok());
        if cancelled {
            // A user stop before the runtime starts must still reach the UI as a
            // terminal stop, otherwise the turn has no visible stop state.
            forward_pipeline_error(
                &mut event_sink,
                Some(prepared.assistant_message_id.clone()),
                Arc::new(VegaError::Cancelled),
            );
        }
        return Err(ConversationError::Runtime(Arc::new(VegaError::Cancelled)));
    }
    let runtime_future = run_agent_with_permission_sink_and_context(
        provider,
        tools,
        runtime_request,
        task_cancel,
        &permission_adapter,
        context_hook_ref,
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
    )
    .boxed_local();
    let processor_future = process_runtime_events(
        runtime_receiver,
        &actor,
        RuntimeEventContext {
            message_id: &prepared.assistant_message_id,
            streamed_content: &mut streamed_content,
            events: &mut events,
            event_sink: &mut event_sink,
            cancel: processor_cancel,
            run_started_at: runtime_started_at,
            work_clock: &work_clock,
            diagnostics: &diagnostics,
            thread_id,
            run_id: &prepared.assistant_message_id,
            root_attempt_id: &root_attempt_id,
            subagent_receipts: supervisor
                .as_ref()
                .map(super::subagents::Supervisor::receipt_observer),
        },
    )
    .boxed_local();
    let pipeline = async move { tokio::join!(runtime_future, processor_future) }.boxed_local();
    let (runtime_result, processor_result) =
        if let (Some(supervisor), Some(launch)) = (&mut supervisor, child_launch) {
            supervisor.run(pipeline, launch).await
        } else {
            pipeline.await
        };
    skill_watch_stop.cancel();
    if let Some(watch) = skill_watch {
        let _ = watch.await;
    }
    let final_assistant_message_id = events
        .iter()
        .rev()
        .find_map(|event| match event {
            ConversationEvent::MessageStarted { message_id, .. } => Some(message_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| prepared.assistant_message_id.clone());
    let mut successful_replay_messages = None;
    let (content, interrupted, failed) = match (processor_result, runtime_result) {
        (Ok(()), Ok(outcome)) => {
            let replay_run_succeeded = !outcome.interrupted
                && !outcome.failed
                && outcome.events.iter().any(|event| {
                    matches!(
                        event,
                        RuntimeEvent::Finished(vega_runtime::RuntimeFinishReason::End)
                    )
                })
                && !outcome
                    .events
                    .iter()
                    .any(|event| matches!(event, RuntimeEvent::Error(_)));
            if replay_run_succeeded {
                successful_replay_messages = Some(outcome.messages);
            }
            let (state, failure_code) = if outcome.interrupted {
                (
                    vega_store::run_diagnostics::DiagnosticState::Cancelled,
                    None,
                )
            } else if outcome.failed {
                let failure_code = outcome
                    .events
                    .iter()
                    .rev()
                    .find_map(|event| match event {
                        RuntimeEvent::Error(error) => Some(super::diagnostics::run_failure(error)),
                        _ => None,
                    })
                    .unwrap_or(
                        vega_store::run_diagnostics::DiagnosticFailureCode::UnknownSafeFailure,
                    );
                (
                    vega_store::run_diagnostics::DiagnosticState::Failed,
                    Some(failure_code),
                )
            } else {
                (
                    vega_store::run_diagnostics::DiagnosticState::Succeeded,
                    None,
                )
            };
            diagnostics.root_event(
                thread_id,
                &prepared.assistant_message_id,
                &root_attempt_id,
                state,
                (
                    failure_code,
                    outcome
                        .events
                        .iter()
                        .rev()
                        .find_map(|event| match event {
                            RuntimeEvent::Error(error) => {
                                Some(super::diagnostics::failure_metrics(error))
                            }
                            _ => None,
                        })
                        .unwrap_or_default(),
                ),
                runtime_started_at,
            );
            diagnostics.close().await;
            let closed = actor.close().await;
            super::ordinary::record_cleanup(ordinary.as_ref(), closed.is_ok());
            if let Err(error) = closed {
                let error = Arc::new(error);
                let duration = u64::try_from(work_clock.finish().as_millis()).unwrap_or(u64::MAX);
                forward_pipeline_error_with_duration(
                    &mut event_sink,
                    Some(final_assistant_message_id.clone()),
                    error.clone(),
                    Some(duration),
                );
                return Err(ConversationError::Runtime(error));
            }
            (outcome.final_text, outcome.interrupted, outcome.failed)
        }
        (processor, runtime) => {
            let error = pipeline_terminal_error(processor.err(), runtime.err());
            let diagnostic_state = if matches!(error, VegaError::Cancelled) {
                vega_store::run_diagnostics::DiagnosticState::Cancelled
            } else {
                vega_store::run_diagnostics::DiagnosticState::Failed
            };
            let error = Arc::new(error);
            let failure_event = pipeline_terminal_event(error.clone());
            let execution_duration_ms =
                u64::try_from(work_clock.finish().as_millis()).unwrap_or(u64::MAX);
            let persistence_result = actor
                .event(
                    failure_event,
                    streamed_content.clone(),
                    Some(i64::try_from(execution_duration_ms).unwrap_or(i64::MAX)),
                )
                .await;
            diagnostics.root_event(
                thread_id,
                &prepared.assistant_message_id,
                &root_attempt_id,
                diagnostic_state,
                (
                    (diagnostic_state == vega_store::run_diagnostics::DiagnosticState::Failed)
                        .then(|| super::diagnostics::run_failure(&error)),
                    super::diagnostics::failure_metrics(&error),
                ),
                runtime_started_at,
            );
            diagnostics.close().await;
            forward_pipeline_error_with_duration(
                &mut event_sink,
                Some(final_assistant_message_id.clone()),
                error.clone(),
                Some(execution_duration_ms),
            );
            let close_result = actor.close().await;
            super::ordinary::record_cleanup(
                ordinary.as_ref(),
                persistence_result.is_ok() && close_result.is_ok(),
            );
            if matches!(error.as_ref(), VegaError::Cancelled) {
                if let Err(error) = persistence_result.and(close_result) {
                    let error = Arc::new(error);
                    forward_pipeline_error_with_duration(
                        &mut event_sink,
                        Some(final_assistant_message_id.clone()),
                        error.clone(),
                        Some(execution_duration_ms),
                    );
                    return Err(ConversationError::Runtime(error));
                }
                events.push(ConversationEvent::Interrupted {
                    message_id: final_assistant_message_id.clone(),
                    execution_duration_ms: Some(execution_duration_ms),
                });
                (streamed_content, true, false)
            } else {
                return Err(ConversationError::Runtime(error));
            }
        }
    };
    if interrupted
        && skill_authority
            .as_ref()
            .is_some_and(super::skills::SkillAuthority::observed_revocation)
    {
        let summaries = prepared
            .request
            .tool_config
            .active_skill_summaries()
            .map_err(|error| ConversationError::Runtime(Arc::new(error)))?;
        for summary in summaries {
            let scope = match summary.source_scope {
                vega_runtime::skills::SourceScope::Project => "project",
                vega_runtime::skills::SourceScope::VegaGlobal => "vega_global",
                vega_runtime::skills::SourceScope::Imported => "imported",
            };
            vega_store::skills::append_revocation_audit_once(
                store.conn(),
                vega_store::skills::NewSkillRevocationAudit {
                    run_id: &prepared.assistant_message_id,
                    thread_id,
                    name: &summary.name,
                    source_scope: scope,
                    content_sha256: &summary.content_sha256,
                    created_at: now_ms(),
                },
            )
            .map_err(|_| ConversationError::Store("Skill revocation audit unavailable".into()))?;
        }
    }
    if let (Some(messages), Some(registry), Some(lease)) = (
        successful_replay_messages,
        replay_registry,
        prepared.replay_lease.take(),
    ) {
        let database_path = prepared.database_path.clone();
        let durable_thread_id = thread_id.to_owned();
        let durable_owner_id = final_assistant_message_id.clone();
        let durable_result = tokio::task::spawn_blocking(move || {
            let fence = durable_replay_fence(
                &database_path,
                &durable_thread_id,
                &durable_owner_id,
                &messages,
            );
            (fence, messages)
        })
        .await;
        if let Ok((Some((source_version, source_fingerprint, checkpoint_fence)), messages)) =
            durable_result
        {
            registry.seal(
                lease,
                source_version,
                source_fingerprint,
                checkpoint_fence,
                final_assistant_message_id.clone(),
                messages,
            );
        }
    }
    Ok(ConversationRun {
        user_message_id: prepared.user_message_id,
        assistant_message_id: final_assistant_message_id,
        events,
        content,
        interrupted,
        failed,
    })
}

fn durable_replay_fence(
    database_path: &std::path::Path,
    thread_id: &str,
    owner_message_id: &str,
    runtime_messages: &[vega_runtime::ChatMessage],
) -> Option<(u64, String, ReplayCheckpointFence)> {
    let store = Store::open_read_only(database_path).ok()?;
    let thread = vega_store::threads::find(store.conn(), thread_id).ok()??;
    let (source, checkpoint) = vega_store::context_compaction::load_source_and_latest_checkpoint(
        store.conn(),
        thread_id,
        &thread.model,
    )
    .ok()?;
    let owner = source
        .messages
        .iter()
        .find(|message| message.id == owner_message_id)?;
    if owner.thread_id != thread_id
        || owner.role != "assistant"
        || owner.status != "done"
        || source
            .messages
            .last()
            .is_none_or(|message| message.id != owner_message_id)
        || source.source_version != u64::try_from(owner.seq).ok()?
        || source.tool_calls.iter().any(|call| {
            !matches!(
                call.status.as_str(),
                "success" | "failed" | "rejected" | "cancelled"
            )
        })
    {
        return None;
    }
    let mut durable = super::pipeline::primary_history_from_context_source_with_checkpoint(
        &source,
        checkpoint.as_ref(),
        "",
    )
    .ok()?;
    let mut runtime = runtime_messages
        .iter()
        .filter(|message| message.role != vega_runtime::ChatRole::System)
        .cloned()
        .collect::<Vec<_>>();
    vega_runtime::computer_use::redact_ephemeral_messages(&mut durable);
    vega_runtime::computer_use::redact_ephemeral_messages(&mut runtime);
    if runtime
        .last()
        .is_none_or(|message| message.role != vega_runtime::ChatRole::Assistant)
    {
        return None;
    }
    let durable = durable.iter().collect::<Vec<_>>();
    let mut runtime = runtime.iter().collect::<Vec<_>>();
    if runtime.len() == durable.len().saturating_add(1)
        && durable
            .last()
            .is_some_and(|message| message.role == vega_runtime::ChatRole::Tool)
        && runtime.last().is_some_and(|message| {
            message.content.is_empty()
                && message.images.is_empty()
                && message.tool_call_id.is_none()
                && message.tool_calls.is_empty()
        })
    {
        runtime.pop();
    }
    if !super::compaction::durable_messages_equal(&durable, &runtime) {
        return None;
    }
    let checkpoint_fence = ReplayCheckpointFence::from_checkpoint(checkpoint.as_ref());
    let source_version = source.source_version;
    let source_fingerprint = source.fingerprint;
    Some((source_version, source_fingerprint, checkpoint_fence))
}

#[allow(clippy::too_many_arguments)]
pub async fn resume_credential_continuation_with_mcp<F>(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    receipt_id: &str,
    system_prompt: &str,
    cancel: CancellationToken,
    permission_hook: &dyn PermissionHook,
    event_sink: F,
    mut actor_config: PersistenceActorConfig,
    pricing_catalog: Option<vega_token::PricingCatalog>,
    reasoning: Option<FrozenReasoning>,
    mcp_servers: Vec<vega_runtime::McpReadyServer>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    actor_config.credential_continuation_id = Some(receipt_id.to_owned());
    actor_config.pending_message_id = None;
    actor_config.pending_authority = None;
    actor_config.automatic_title = None;
    run_thread_task_with_images_reasoning_and_mcp(
        store,
        provider,
        tools,
        thread_id,
        "",
        system_prompt,
        cancel,
        permission_hook,
        event_sink,
        actor_config,
        None,
        pricing_catalog,
        reasoning,
        Vec::new(),
        mcp_servers,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn resume_interrupted_assistant_with_mcp<F>(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    thread_id: &str,
    expected: &crate::types::InterruptedAssistant,
    system_prompt: &str,
    cancel: CancellationToken,
    permission_hook: &dyn PermissionHook,
    event_sink: F,
    mut actor_config: PersistenceActorConfig,
    pricing_catalog: Option<vega_token::PricingCatalog>,
    reasoning: Option<FrozenReasoning>,
    mcp_servers: Vec<vega_runtime::McpReadyServer>,
) -> Result<ConversationRun, ConversationError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    if actor_config.credential_continuation_id.is_some()
        || actor_config.pending_message_id.is_some()
        || actor_config.delegated
        || actor_config.automation_claim.is_some()
    {
        return Err(ConversationError::CorruptRow(
            "interrupted resume identity mismatch".into(),
        ));
    }
    actor_config.interrupted_assistant = Some(expected.clone());
    actor_config.pending_message_id = None;
    actor_config.pending_authority = None;
    actor_config.automatic_title = None;
    run_thread_task_with_automation_inner(
        store,
        provider,
        tools,
        thread_id,
        "",
        system_prompt,
        cancel,
        permission_hook,
        event_sink,
        actor_config,
        None,
        pricing_catalog,
        reasoning,
        Vec::new(),
        mcp_servers,
    )
    .await
}

fn pipeline_terminal_error(processor: Option<VegaError>, runtime: Option<VegaError>) -> VegaError {
    match (processor, runtime) {
        (Some(VegaError::Cancelled), Some(error)) => error,
        (Some(error), _) | (None, Some(error)) => error,
        (None, None) => persistence_actor_error("agent event pipeline stopped"),
    }
}

fn pipeline_terminal_event(error: Arc<VegaError>) -> RuntimeEvent {
    if matches!(error.as_ref(), VegaError::Cancelled) {
        RuntimeEvent::Interrupted
    } else {
        RuntimeEvent::Error(error)
    }
}
