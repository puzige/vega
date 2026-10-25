use super::*;
use vega_store::write_coordination::PersistenceCategory;

const PERSISTENCE_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const PERSISTENCE_BUSY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);

thread_local! {
    static PERSISTENCE_BUSY_STARTED: std::cell::Cell<Option<std::time::Instant>> = const {
        std::cell::Cell::new(None)
    };
}

fn persistence_busy_handler(count: i32) -> bool {
    PERSISTENCE_BUSY_STARTED.with(|clock| {
        if count == 0 || clock.get().is_none() {
            clock.set(Some(std::time::Instant::now()));
        }
        let Some(started) = clock.get() else {
            return false;
        };
        let remaining = PERSISTENCE_BUSY_TIMEOUT.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(PERSISTENCE_BUSY_INTERVAL));
        started.elapsed() < PERSISTENCE_BUSY_TIMEOUT
    })
}

#[derive(Clone)]
pub(crate) struct DelegatedAuthority {
    pub run_mode: RuntimeRunMode,
    pub permission_mode: RuntimePermissionMode,
    pub exact_rules: Vec<RuntimeExactRule>,
}

#[derive(Clone, Default)]
pub struct PersistenceActorConfig {
    pub(crate) subagent_host: Option<Arc<dyn crate::types::SubagentHost>>,
    pub(crate) delegated: bool,
    pub(crate) delegated_owner: Option<Arc<vega_store::delegated_ownership::DelegatedOwnership>>,
    pub(crate) stage_hook: Option<Arc<dyn vega_runtime::RuntimeSubagentHook>>,
    pub(crate) delegated_parent: Option<(String, String)>,
    pub(crate) delegated_message_id: Option<String>,
    pub(crate) delegated_authority: Option<DelegatedAuthority>,
    pub(crate) delegated_context_budget: Option<vega_runtime::ContextBudget>,
    pub(crate) user_questions: Option<Arc<dyn vega_runtime::RuntimeUserQuestionHook>>,
    pub(crate) work_time_source: Option<crate::types::WorkTimeSource>,
    pub(crate) automation_claim: Option<crate::types::AutomationRunClaim>,
    pub(crate) automation_hook: Option<Arc<dyn vega_runtime::RuntimeAutomationHook>>,
    pub(crate) credential_continuation_id: Option<String>,
    pub(crate) steering_cancel: Option<CancellationToken>,
    pub(crate) pending_message_id: Option<String>,
    pub(crate) steering_source: Option<Arc<dyn vega_runtime::RuntimeSteeringSource>>,
    pub(crate) computer_use: Option<vega_runtime::computer_use::ComputerUseRun>,
    pub automatic_title: Option<crate::types::AutomaticTitleRequest>,
    pub turn_limit: u32,
    pub(crate) credential_exceptions: Option<vega_runtime::CredentialExceptionFilter>,
    pub(crate) owner_credential_reader: Option<vega_runtime::CredentialReader>,
    pub(crate) replay_registry: Option<super::ReplaySessionRegistry>,
    pub(crate) replay_route_fingerprint: Option<[u8; 32]>,
}

impl PersistenceActorConfig {
    pub fn with_subagent_host(mut self, host: Arc<dyn crate::types::SubagentHost>) -> Self {
        self.subagent_host = Some(host);
        self
    }

    pub fn with_user_questions(
        mut self,
        hook: Arc<dyn vega_runtime::RuntimeUserQuestionHook>,
    ) -> Self {
        self.user_questions = Some(hook);
        self
    }

    pub fn with_work_time_source(mut self, source: crate::types::WorkTimeSource) -> Self {
        self.work_time_source = Some(source);
        self
    }

    pub fn with_automation_claim(
        mut self,
        claim: Option<crate::types::AutomationRunClaim>,
    ) -> Self {
        self.automation_claim = claim;
        self
    }

    pub fn with_replay_session_registry(
        mut self,
        registry: super::ReplaySessionRegistry,
        route_fingerprint: [u8; 32],
    ) -> Self {
        self.replay_registry = Some(registry);
        self.replay_route_fingerprint = Some(route_fingerprint);
        self
    }

    pub fn with_pending_input(
        mut self,
        id: Option<String>,
        source: Option<Arc<dyn vega_runtime::RuntimeSteeringSource>>,
    ) -> Self {
        self.pending_message_id = id;
        self.steering_source = source;
        self
    }
    pub fn with_computer_use(
        mut self,
        run: Option<vega_runtime::computer_use::ComputerUseRun>,
    ) -> Self {
        self.computer_use = run;
        self
    }
    pub fn with_owner_credential_reader(mut self, reader: vega_runtime::CredentialReader) -> Self {
        self.owner_credential_reader = Some(reader);
        self
    }

    pub fn with_credential_exceptions(
        mut self,
        filter: Option<vega_runtime::CredentialExceptionFilter>,
    ) -> Self {
        self.credential_exceptions = filter;
        self
    }

    pub fn with_automatic_title(
        mut self,
        request: Option<crate::types::AutomaticTitleRequest>,
    ) -> Self {
        self.automatic_title = request;
        self
    }

    pub fn with_turn_limit(mut self, limit: u32) -> Self {
        self.turn_limit = limit;
        self
    }

    pub(crate) fn delay_command(&self) {}

    pub(crate) fn record_snapshot(&self) {}

    pub(crate) fn check_event(&self, event: &RuntimeEvent) -> Result<(), VegaError> {
        let _ = event;
        Ok(())
    }
}

pub(crate) enum PersistenceCommand {
    Snapshot {
        content: String,
        ack: oneshot::Sender<Result<(), VegaError>>,
    },
    Event {
        event: Box<RuntimeEvent>,
        content: String,
        execution_duration_ms: Option<i64>,
        ack: oneshot::Sender<Result<(), VegaError>>,
    },
    Diagnostic(Box<vega_store::run_diagnostics::NewDiagnosticEvent>),
}

pub(crate) struct PersistenceActor {
    pub(crate) sender: mpsc::Sender<PersistenceCommand>,
    pub(crate) task: tokio::task::JoinHandle<Result<(), VegaError>>,
}

pub(crate) struct PersistenceActorStart {
    pub(crate) database_path: PathBuf,
    pub(crate) project_id: String,
    pub(crate) thread_id: String,
    pub(crate) message_id: String,
    pub(crate) model: String,
    pub(crate) is_plan: bool,
    pub(crate) next_tool_seq: i64,
    pub(crate) config: PersistenceActorConfig,
}

impl PersistenceActor {
    pub(crate) async fn start(start: PersistenceActorStart) -> Result<Self, VegaError> {
        let PersistenceActorStart {
            database_path,
            project_id,
            thread_id,
            message_id,
            model,
            is_plan,
            next_tool_seq,
            config,
        } = start;
        let (sender, mut receiver) = mpsc::channel(PERSISTENCE_CHANNEL_CAPACITY);
        let (ready, opened) = oneshot::channel();
        let task = tokio::task::spawn_blocking(move || {
            let store = match Store::open_with_busy_timeout(database_path, PERSISTENCE_BUSY_TIMEOUT)
                .and_then(|store| {
                    store.conn().busy_handler(Some(persistence_busy_handler))?;
                    Ok(store)
                }) {
                Ok(store) => store,
                Err(error) => {
                    let _ = ready.send(Err(()));
                    return Err(VegaError::Store(error)
                        .at_runtime_stage(vega_runtime::RuntimeFailureStage::EventPipeline));
                }
            };
            let _ = ready.send(Ok(()));
            let mut next_tool_seq = next_tool_seq;
            let mut message_id = message_id;
            while let Some(command) = receiver.blocking_recv() {
                match command {
                    PersistenceCommand::Snapshot { content, ack } => {
                        config.delay_command();
                        let started = std::time::Instant::now();
                        let result =
                            messages::update_streaming_content(store.conn(), &message_id, &content)
                                .map_err(VegaError::from)
                                .and_then(|updated| ensure_message_updated(updated, &message_id))
                                .map_err(|error| {
                                    error.at_runtime_stage(
                                        vega_runtime::RuntimeFailureStage::Snapshot,
                                    )
                                });
                        if result.is_ok() {
                            config.record_snapshot();
                        }
                        log_persistence_result(
                            &store,
                            PersistenceCategory::Snapshot,
                            started,
                            &result,
                        );
                        let _ = ack.send(result);
                    }
                    PersistenceCommand::Event {
                        event,
                        content,
                        execution_duration_ms,
                        ack,
                    } => {
                        config.delay_command();
                        let started = std::time::Instant::now();
                        let result = config
                            .check_event(&event)
                            .and_then(|()| {
                                if matches!(event.as_ref(), RuntimeEvent::SteeringAccepted(_))
                                    && config
                                        .steering_cancel
                                        .as_ref()
                                        .is_some_and(CancellationToken::is_cancelled)
                                {
                                    return Err(VegaError::Cancelled);
                                }
                                let usage = match event.as_ref() {
                                    RuntimeEvent::UsageUpdated { usage, .. } => {
                                        Some((usage.input, usage.output))
                                    }
                                    RuntimeEvent::ContextCompactionUsageUpdated { usage } => {
                                        Some((usage.usage.input, usage.usage.output))
                                    }
                                    _ => None,
                                };
                                if let RuntimeEvent::ContextCompactionStatusUpdated { status } =
                                    event.as_ref()
                                    && status.usage
                                        == vega_runtime::ContextCompactionUsageState::Unknown
                                    && let Some(hook) = &config.automation_hook
                                {
                                    hook.missing_usage();
                                }
                                let transaction = if usage.is_some()
                                    && (config.automation_claim.is_some()
                                        || config.delegated_parent.is_some())
                                {
                                    Some(store.immediate_transaction()?)
                                } else {
                                    None
                                };
                                persist_runtime_event(
                                    &store,
                                    &project_id,
                                    &thread_id,
                                    &message_id,
                                    &model,
                                    is_plan,
                                    &content,
                                    &mut next_tool_seq,
                                    &event,
                                    execution_duration_ms,
                                )?;
                                if usage.is_some()
                                    && let Some((parent_thread, parent_message)) =
                                        &config.delegated_parent
                                {
                                    let cache_known = match event.as_ref() {
                                        RuntimeEvent::UsageUpdated { usage, .. } => {
                                            usage.cache_read.is_some()
                                                && usage.cache_write.is_some()
                                        }
                                        RuntimeEvent::ContextCompactionUsageUpdated { usage } => {
                                            usage.usage.cache_read.is_some()
                                                && usage.usage.cache_write.is_some()
                                        }
                                        _ => false,
                                    };
                                    vega_store::subagents::attribute_usage(
                                        store.conn(),
                                        store.conn().last_insert_rowid(),
                                        &thread_id,
                                        parent_thread,
                                        parent_message,
                                        cache_known,
                                    )?;
                                }
                                if let (Some((input, output)), Some(claim)) =
                                    (usage, config.automation_claim.as_ref())
                                {
                                    crate::automation::record_usage(
                                        store.conn(),
                                        claim,
                                        store.conn().last_insert_rowid(),
                                        input,
                                        output,
                                    )
                                    .map_err(|_| {
                                        VegaError::Tool {
                                            tool: "automation".into(),
                                            message: "usage persistence unavailable".into(),
                                        }
                                    })?;
                                }
                                if let Some(transaction) = transaction {
                                    transaction.commit()?;
                                }
                                Ok(())
                            })
                            .map_err(|error| error.at_runtime_stage(runtime_event_stage(&event)));
                        log_persistence_result(
                            &store,
                            runtime_event_category(&event),
                            started,
                            &result,
                        );
                        if result.is_ok()
                            && let RuntimeEvent::SteeringAccepted(input) = event.as_ref()
                        {
                            message_id = input.assistant_message_id.clone();
                        }
                        let _ = ack.send(result);
                    }
                    PersistenceCommand::Diagnostic(event) => {
                        let started = std::time::Instant::now();
                        let result = vega_store::run_diagnostics::insert(store.conn(), &event)
                            .map_err(|error| match error {
                                vega_store::run_diagnostics::DiagnosticError::Sql(error) => {
                                    VegaError::from(error).at_runtime_stage(
                                        vega_runtime::RuntimeFailureStage::EventPipeline,
                                    )
                                }
                                _ => persistence_actor_error("diagnostic persistence failed"),
                            });
                        log_persistence_result(
                            &store,
                            PersistenceCategory::Diagnostic,
                            started,
                            &result,
                        );
                    }
                }
            }
            Ok(())
        });
        match opened.await {
            Ok(Ok(())) => {}
            Ok(Err(())) => {
                return match task.await {
                    Ok(Err(error)) => Err(error),
                    Ok(Ok(())) => Err(persistence_actor_error("DB actor open failed")),
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

    pub(crate) async fn snapshot(&self, content: String) -> Result<(), VegaError> {
        let (ack, received) = oneshot::channel();
        self.sender
            .send(PersistenceCommand::Snapshot { content, ack })
            .await
            .map_err(|_| {
                persistence_actor_error("DB actor stopped before snapshot")
                    .at_runtime_stage(vega_runtime::RuntimeFailureStage::Snapshot)
            })?;
        received.await.map_err(|_| {
            persistence_actor_error("DB actor dropped snapshot acknowledgement")
                .at_runtime_stage(vega_runtime::RuntimeFailureStage::Snapshot)
        })?
    }

    pub(crate) async fn event(
        &self,
        event: RuntimeEvent,
        content: String,
        execution_duration_ms: Option<i64>,
    ) -> Result<(), VegaError> {
        let stage = runtime_event_stage(&event);
        let (ack, received) = oneshot::channel();
        self.sender
            .send(PersistenceCommand::Event {
                event: Box::new(event),
                content,
                execution_duration_ms,
                ack,
            })
            .await
            .map_err(|_| {
                persistence_actor_error("DB actor stopped before event persistence")
                    .at_runtime_stage(stage)
            })?;
        received.await.map_err(|_| {
            persistence_actor_error("DB actor dropped event acknowledgement")
                .at_runtime_stage(stage)
        })?
    }

    pub(crate) async fn close(self) -> Result<(), VegaError> {
        drop(self.sender);
        self.task
            .await
            .map_err(|error| persistence_actor_error(format!("DB actor join failed: {error}")))?
    }
}

pub(crate) struct RuntimeEnvelope {
    pub(crate) event: RuntimeEvent,
    pub(crate) ack: Option<oneshot::Sender<Result<(), VegaError>>>,
}

pub(crate) struct PreparedRun {
    pub(crate) _delegated_owner: Option<Arc<vega_store::delegated_ownership::DelegatedOwnership>>,
    pub(crate) database_path: PathBuf,
    pub(crate) project_id: String,
    pub(crate) model: String,
    pub(crate) is_plan: bool,
    pub(crate) user_message_id: String,
    pub(crate) assistant_message_id: String,
    pub(crate) assistant_seq: i64,
    pub(crate) request: AgentRequest,
    pub(crate) next_tool_seq: i64,
    pub(crate) replay_lease: Option<super::ReplaySessionLease>,
}

pub(crate) fn persistence_actor_error(_message: impl Into<String>) -> VegaError {
    VegaError::RuntimeFailure(vega_runtime::RuntimeFailureDetail::new(
        vega_runtime::RuntimeFailureReason::Pipeline,
    ))
}

fn log_persistence_result<T>(
    store: &Store,
    category: PersistenceCategory,
    started: std::time::Instant,
    result: &Result<T, VegaError>,
) {
    let metrics = result.as_ref().err().map(VegaError::diagnostic_metrics);
    vega_store::write_coordination::record_persistence(
        store.conn(),
        category,
        started,
        result.is_ok(),
        metrics
            .as_ref()
            .and_then(|metrics| metrics.sqlite_extended_code),
    );
    tracing::debug!(
        ?category,
        outcome = if result.is_ok() {
            "succeeded"
        } else {
            "failed"
        },
        elapsed_ms = started.elapsed().as_millis() as u64,
        runtime_stage = metrics.as_ref().and_then(|metrics| metrics.runtime_stage),
        sqlite_primary_code = metrics.as_ref().and_then(|metrics| metrics.sqlite_code),
        sqlite_extended_code = metrics
            .as_ref()
            .and_then(|metrics| metrics.sqlite_extended_code),
    );
}

fn runtime_event_category(event: &RuntimeEvent) -> PersistenceCategory {
    use PersistenceCategory as Category;
    match event {
        RuntimeEvent::AssistantResponseStarted { .. } => Category::ResponseStarted,
        RuntimeEvent::ReasoningCompleted { .. } => Category::ReasoningCompleted,
        RuntimeEvent::ResponseRetrying { .. } => Category::ResponseRetrying,
        RuntimeEvent::SteeringAccepted(_) => Category::Steering,
        RuntimeEvent::BuiltinSkillActivated(_) => Category::BuiltinSkill,
        RuntimeEvent::ScreenshotDelivered { .. } => Category::Screenshot,
        RuntimeEvent::ToolRoundCompleted { .. } => Category::ToolRound,
        RuntimeEvent::AssistantResponseCompleted { .. } | RuntimeEvent::GeneratedNotice { .. } => {
            Category::ResponseCompleted
        }
        RuntimeEvent::DiagnosticAttempt(_) => Category::DiagnosticAttempt,
        RuntimeEvent::ReasoningRetentionReleased => Category::ReasoningReleased,
        RuntimeEvent::TextDelta(_) => Category::TextDelta,
        RuntimeEvent::ThinkingDelta(_) => Category::ThinkingDelta,
        RuntimeEvent::SummaryDelta(_) => Category::SummaryDelta,
        RuntimeEvent::ToolCallProposed(_) => Category::ToolProposal,
        RuntimeEvent::ToolCallValidationRejected { .. } => Category::ToolValidation,
        RuntimeEvent::ToolCallConflict { .. } => Category::ToolConflict,
        RuntimeEvent::ToolCallApproved { .. } => Category::ToolApproval,
        RuntimeEvent::ToolCallRunning { .. } => Category::ToolRunning,
        RuntimeEvent::ToolCallOutput { .. } => Category::ToolOutput,
        RuntimeEvent::ToolCallFinished(_) | RuntimeEvent::ToolReadFinished { .. } => {
            Category::ToolTerminal
        }
        RuntimeEvent::StageHandoff { .. } => Category::StageHandoff,
        RuntimeEvent::SkillActivation { .. } => Category::SkillActivation,
        RuntimeEvent::SkillSnapshot { .. } => Category::SkillSnapshot,
        RuntimeEvent::UsageUpdated { .. } => Category::Usage,
        RuntimeEvent::ContextCompactionUsageUpdated { .. } => Category::CompactionUsage,
        RuntimeEvent::ContextCompactionStatusUpdated { .. } => Category::CompactionStatus,
        RuntimeEvent::ContextAccountingUpdated(_) => Category::ContextAccounting,
        RuntimeEvent::Finished(_) => Category::Finished,
        RuntimeEvent::Interrupted => Category::Interrupted,
        RuntimeEvent::Error(_) => Category::Error,
    }
}

fn runtime_event_stage(event: &RuntimeEvent) -> vega_runtime::RuntimeFailureStage {
    use vega_runtime::RuntimeFailureStage as Stage;
    match event {
        RuntimeEvent::ToolCallProposed(_) => Stage::ToolProposal,
        RuntimeEvent::ToolCallApproved { .. } => Stage::ToolApproval,
        RuntimeEvent::ToolCallRunning { .. } => Stage::ToolRunning,
        RuntimeEvent::ToolCallFinished(_)
        | RuntimeEvent::ToolReadFinished { .. }
        | RuntimeEvent::ToolCallValidationRejected { .. }
        | RuntimeEvent::ToolCallConflict { .. } => Stage::ToolTerminal,
        _ => Stage::EventPipeline,
    }
}

pub(crate) fn runtime_event_requires_ack(event: &RuntimeEvent) -> bool {
    !matches!(
        event,
        RuntimeEvent::TextDelta(_)
            | RuntimeEvent::ThinkingDelta(_)
            | RuntimeEvent::SummaryDelta(_)
            | RuntimeEvent::ToolCallOutput { .. }
            | RuntimeEvent::DiagnosticAttempt(_)
    )
}
