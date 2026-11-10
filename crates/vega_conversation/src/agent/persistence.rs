use super::*;

pub(crate) mod database_writer;
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
    pub permission_fence: crate::types::PermissionFence,
    pub run_mode: RuntimeRunMode,
    pub permission_mode: RuntimePermissionMode,
    pub exact_rules: Vec<RuntimeExactRule>,
}

impl DelegatedAuthority {
    pub(crate) fn matches_fence(&self) -> bool {
        let mut rules: Vec<_> = self
            .exact_rules
            .iter()
            .map(|rule| (rule.tool.as_str().to_owned(), rule.pattern.clone()))
            .collect();
        rules.sort();
        let run_mode = match self.run_mode {
            RuntimeRunMode::Ask => "ask",
            RuntimeRunMode::Plan => "plan",
            RuntimeRunMode::Execute => "execute",
        };
        let permission_mode = match self.permission_mode {
            RuntimePermissionMode::ReadOnly => "readonly",
            RuntimePermissionMode::Confirm => "confirm",
            RuntimePermissionMode::Auto => "auto",
            RuntimePermissionMode::FullAccess => "full_access",
        };
        self.permission_fence.is_valid()
            && self.permission_fence.run_mode == run_mode
            && self.permission_fence.permission_mode == permission_mode
            && self.permission_fence.exact_rules == rules
    }
}

#[derive(Clone, Default)]
pub struct PersistenceActorConfig {
    pub(crate) ordinary: Option<super::ordinary::OrdinaryRun>,
    pub(crate) admission: Option<crate::types::ThreadAdmission>,
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
    pub(crate) interrupted_assistant: Option<crate::types::InterruptedAssistant>,
    pub(crate) steering_cancel: Option<CancellationToken>,
    pub(crate) pending_message_id: Option<String>,
    pub(crate) pending_authority: Option<crate::types::PendingInputAuthority>,
    pub(crate) steering_source: Option<Arc<dyn vega_runtime::RuntimeSteeringSource>>,
    pub(crate) preparation_mcp: Vec<vega_runtime::McpReadyServer>,
    pub(crate) preparation_model: Option<Box<vega_runtime::model_compat::ResolvedModel>>,
    pub(crate) steering_authority: Option<Arc<std::sync::OnceLock<crate::types::PermissionFence>>>,
    pub(crate) computer_use: Option<vega_runtime::computer_use::ComputerUseRun>,
    pub automatic_title: Option<crate::types::AutomaticTitleRequest>,
    pub turn_limit: u32,
    pub(crate) credential_exceptions: Option<vega_runtime::CredentialExceptionFilter>,
    pub(crate) owner_credential_reader: Option<vega_runtime::CredentialReader>,
    pub(crate) run_credential_reader: Option<crate::types::RunCredentialReader>,
    pub(crate) precompute_lease: Option<crate::types::ContextPrecomputeLease>,
    pub(crate) replay_registry: Option<super::ReplaySessionRegistry>,
    pub(crate) replay_route_fingerprint: Option<[u8; 32]>,
}

impl PersistenceActorConfig {
    pub fn with_accepted_pending_input(
        mut self,
        authority: Option<crate::types::PendingInputAuthority>,
    ) -> Self {
        self.pending_authority = authority;
        self
    }

    pub fn with_ordinary_submission(
        mut self,
        ordinary: Option<super::ordinary::OrdinaryRun>,
    ) -> Self {
        if let Some(run) = &ordinary {
            self.admission = Some(run.worker.admission().clone());
        }
        self.ordinary = ordinary;
        self
    }

    pub fn with_thread_admission(mut self, admission: crate::types::ThreadAdmission) -> Self {
        self.admission = Some(admission);
        self
    }

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

    pub fn with_context_precompute_lease(
        mut self,
        lease: Option<crate::types::ContextPrecomputeLease>,
    ) -> Self {
        self.precompute_lease = lease;
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
        source: Option<Arc<crate::pending_messages::QueueSteeringSource>>,
    ) -> Self {
        if id.is_none() {
            self.pending_authority = None;
        }
        self.pending_message_id = id;
        self.steering_authority = source.as_ref().map(|source| source.authority_slot());
        self.steering_source =
            source.map(|source| source as Arc<dyn vega_runtime::RuntimeSteeringSource>);
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
        self.run_credential_reader = None;
        self
    }

    pub fn with_run_credential_reader(mut self, reader: crate::types::RunCredentialReader) -> Self {
        self.owner_credential_reader = Some(reader.reader());
        self.run_credential_reader = Some(reader);
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
        operation_id: String,
        content: String,
        ack: oneshot::Sender<Result<(), VegaError>>,
    },
    Event {
        operation_id: String,
        event: Box<RuntimeEvent>,
        content: String,
        execution_duration_ms: Option<i64>,
        ack: oneshot::Sender<Result<Vec<ConversationEvent>, VegaError>>,
    },
    Diagnostic(Box<vega_store::run_diagnostics::NewDiagnosticEvent>),
}

pub(crate) struct PersistenceSender {
    sender: Option<mpsc::Sender<PersistenceCommand>>,
    writer: Arc<database_writer::DatabaseWriter>,
    cancel: Option<CancellationToken>,
}

impl Clone for PersistenceSender {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            writer: self.writer.clone(),
            cancel: self.cancel.clone(),
        }
    }
}

impl Drop for PersistenceSender {
    fn drop(&mut self) {
        drop(self.sender.take());
        self.writer.wake();
    }
}

impl PersistenceSender {
    async fn send(&self, command: PersistenceCommand) -> Result<(), VegaError> {
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| persistence_actor_error("sender closed"))?;
        let permit = if let Some(cancel) = &self.cancel {
            tokio::select! {
                biased;
                _ = cancel.cancelled(), if matches!(&command, PersistenceCommand::Snapshot { .. }) => return Err(VegaError::Cancelled),
                permit = sender.reserve() => permit,
            }
        } else {
            sender.reserve().await
        }.map_err(|_| persistence_actor_error("writer stopped before enqueue"))?;
        permit.send(command);
        self.writer.wake();
        Ok(())
    }

    pub(crate) fn try_send(&self, command: PersistenceCommand) {
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(command);
        }
        self.writer.wake();
    }
}

pub(crate) struct PersistenceActor {
    pub(crate) sender: PersistenceSender,
    pub(crate) task: tokio::task::JoinHandle<Result<(), VegaError>>,
    automation_hook: Option<Arc<dyn vega_runtime::RuntimeAutomationHook>>,
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
        let ordinary_task = super::ordinary::OrdinaryTask::register(start.config.ordinary.as_ref())
            .map_err(|_| persistence_actor_error("ordinary worker registration unavailable"))?;
        let writer = database_writer::DatabaseWriter::open(start.database_path.clone()).await?;
        let (sender, receiver) = mpsc::channel(PERSISTENCE_CHANNEL_CAPACITY);
        let (ready, opened) = oneshot::channel();
        let (drained, completed) = oneshot::channel();
        let cancel = start.config.steering_cancel.clone();
        let automation_hook = start.config.automation_hook.clone();
        let registration = database_writer::RunRegistration {
            state: database_writer::RunState {
                run_id: start.message_id.clone(),
                start,
                ordinary_task,
                active_response_id: None,
            },
            receiver,
            ready,
            drained,
        };
        let owner = writer.clone();
        tokio::task::spawn_blocking(move || {
            let result = owner.register_run(registration);
            owner.wake();
            result
        })
        .await
        .map_err(|_| persistence_actor_error("writer registration join failed"))?
        .map_err(|_| persistence_actor_error("writer stopped before registration"))?;
        opened
            .await
            .map_err(|_| persistence_actor_error("writer dropped startup receipt"))??;
        let sender = PersistenceSender {
            sender: Some(sender),
            writer: writer.clone(),
            cancel,
        };
        let task = tokio::spawn(async move {
            let result = completed
                .await
                .map_err(|_| persistence_actor_error("writer stopped before drain"))?;
            drop(writer);
            result
        });
        Ok(Self {
            sender,
            task,
            automation_hook,
        })
    }

    pub(crate) async fn snapshot(&self, content: String) -> Result<(), VegaError> {
        let (ack, received) = oneshot::channel();
        self.sender
            .send(PersistenceCommand::Snapshot {
                operation_id: ulid::Ulid::generate().to_string(),
                content,
                ack,
            })
            .await
            .map_err(|error| error.at_runtime_stage(vega_runtime::RuntimeFailureStage::Snapshot))?;
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
    ) -> Result<Vec<ConversationEvent>, VegaError> {
        let stage = runtime_event_stage(&event);
        if let RuntimeEvent::ContextCompactionStatusUpdated { status } = &event
            && status.usage == vega_runtime::ContextCompactionUsageState::Unknown
            && let Some(hook) = &self.automation_hook
        {
            hook.missing_usage();
        }
        let (ack, received) = oneshot::channel();
        self.sender
            .send(PersistenceCommand::Event {
                operation_id: ulid::Ulid::generate().to_string(),
                event: Box::new(event),
                content,
                execution_duration_ms,
                ack,
            })
            .await
            .map_err(|error| error.at_runtime_stage(stage))?;
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
    pub(crate) skill_launch: Option<super::skills::PreparedSkills>,
    pub(crate) permission_fence: crate::types::PermissionFence,
    pub(crate) _admission: crate::types::ThreadAdmission,
    pub(crate) _delegated_owner: Option<Arc<vega_store::delegated_ownership::DelegatedOwnership>>,
    pub(crate) database_path: PathBuf,
    pub(crate) project_id: String,
    pub(crate) model: String,
    pub(crate) is_plan: bool,
    pub(crate) user_message_id: String,
    pub(crate) user_sent_at_ms: Option<i64>,
    pub(crate) user_timestamp_break_before: bool,
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
    context: (
        &str,
        &str,
        &str,
        &vega_store::write_coordination::ObservationScope,
    ),
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
    if let Some(metrics) = &metrics {
        let (thread_id, run_id, message_id, scope) = context;
        let failure = vega_store::persistence_failures::PersistenceFailure {
            thread_id: thread_id.to_owned(),
            run_id: run_id.to_owned(),
            message_id: message_id.to_owned(),
            occurred_at: u64::try_from(now_ms()).unwrap_or_default(),
            category,
            elapsed_ms: started.elapsed().as_millis() as u64,
            sqlite_extended_code: metrics.sqlite_extended_code,
            observations: scope.observations(),
        };
        if let Err(fault) = vega_store::persistence_failures::enqueue(store, failure) {
            tracing::warn!(
                ?category,
                stage = ?fault.stage,
                io_kind = fault.io_kind,
                os_code = fault.os_code,
                outcome = "failure_diagnostic_unavailable"
            );
        }
    }
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
