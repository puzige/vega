use super::*;

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
    pub(crate) delegated_parent: Option<(String, String)>,
    pub(crate) delegated_message_id: Option<String>,
    pub(crate) delegated_authority: Option<DelegatedAuthority>,
    pub(crate) delegated_context_budget: Option<vega_runtime::ContextBudget>,
    pub(crate) user_questions: Option<Arc<dyn vega_runtime::RuntimeUserQuestionHook>>,
    pub(crate) automation_claim: Option<crate::types::AutomationRunClaim>,
    pub(crate) automation_hook: Option<Arc<dyn vega_runtime::RuntimeAutomationHook>>,
    pub(crate) credential_continuation_id: Option<String>,
    pub(crate) steering_cancel: Option<CancellationToken>,
    pub(crate) pending_message_id: Option<String>,
    pub(crate) steering_source: Option<Arc<dyn vega_runtime::RuntimeSteeringSource>>,
    pub(crate) computer_use: Option<vega_runtime::computer_use::ComputerUseRun>,
    pub automatic_title: Option<crate::types::AutomaticTitleRequest>,
    /// Maximum agentic turns (provider round-trips) for this run; `0` means
    /// unlimited (Issue #114, mirrors Claude Code's `--max-turns`). Carried
    /// from `[agent] turn_limit` in config.toml through to the runtime.
    pub turn_limit: u32,
    pub(crate) credential_exceptions: Option<vega_runtime::CredentialExceptionFilter>,
    pub(crate) owner_credential_reader: Option<vega_runtime::CredentialReader>,
    pub(crate) replay_registry: Option<super::ReplaySessionRegistry>,
    pub(crate) replay_route_fingerprint: Option<[u8; 32]>,
    #[cfg(test)]
    pub(crate) snapshot_writes: Option<Arc<AtomicUsize>>,
    #[cfg(test)]
    pub(crate) command_delay: Option<Duration>,
    #[cfg(test)]
    pub(crate) fail_event: Option<InjectedPersistenceFailure>,
    #[cfg(test)]
    pub(crate) preparation_delay: Option<Duration>,
    #[cfg(test)]
    pub(crate) preparation_query_only: bool,
    #[cfg(test)]
    pub(crate) actor_query_only: bool,
    #[cfg(test)]
    pub(crate) fail_start: bool,
    #[cfg(test)]
    pub(crate) checkpoint_root: Option<PathBuf>,
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InjectedPersistenceFailure {
    Retrying,
    Running,
    Finished,
    PanicRunning,
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

    /// Sets the maximum number of agentic turns for this run; `0` means
    /// unlimited (Issue #114).
    pub fn with_turn_limit(mut self, limit: u32) -> Self {
        self.turn_limit = limit;
        self
    }

    pub(crate) fn delay_command(&self) {
        #[cfg(test)]
        if let Some(delay) = self.command_delay {
            std::thread::sleep(delay);
        }
    }

    pub(crate) fn record_snapshot(&self) {
        #[cfg(test)]
        if let Some(writes) = &self.snapshot_writes {
            writes.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub(crate) fn check_event(&self, event: &RuntimeEvent) -> Result<(), VegaError> {
        #[cfg(test)]
        {
            let matches_failure = matches!(
                (self.fail_event, event),
                (
                    Some(InjectedPersistenceFailure::Retrying),
                    RuntimeEvent::ResponseRetrying { .. }
                ) | (
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
            let mut message_id = message_id;
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
                        execution_duration_ms,
                        ack,
                    } => {
                        config.delay_command();
                        let result = config.check_event(&event).and_then(|()| {
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
                                        usage.cache_read.is_some() && usage.cache_write.is_some()
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
                                .map_err(|message| {
                                    VegaError::Tool {
                                        tool: "automation".into(),
                                        message,
                                    }
                                })?;
                            }
                            if let Some(transaction) = transaction {
                                transaction.commit()?;
                            }
                            Ok(())
                        });
                        if result.is_ok()
                            && let RuntimeEvent::SteeringAccepted(input) = event.as_ref()
                        {
                            message_id = input.assistant_message_id.clone();
                        }
                        let _ = ack.send(result);
                    }
                    PersistenceCommand::Diagnostic(event) => {
                        let _ = vega_store::run_diagnostics::insert(store.conn(), &event);
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

    pub(crate) async fn snapshot(&self, content: String) -> Result<(), VegaError> {
        let (ack, received) = oneshot::channel();
        self.sender
            .send(PersistenceCommand::Snapshot { content, ack })
            .await
            .map_err(|_| persistence_actor_error("DB actor stopped before snapshot"))?;
        received
            .await
            .map_err(|_| persistence_actor_error("DB actor dropped snapshot acknowledgement"))?
    }

    pub(crate) async fn event(
        &self,
        event: RuntimeEvent,
        content: String,
        execution_duration_ms: Option<i64>,
    ) -> Result<(), VegaError> {
        let (ack, received) = oneshot::channel();
        self.sender
            .send(PersistenceCommand::Event {
                event: Box::new(event),
                content,
                execution_duration_ms,
                ack,
            })
            .await
            .map_err(|_| persistence_actor_error("DB actor stopped before event persistence"))?;
        received
            .await
            .map_err(|_| persistence_actor_error("DB actor dropped event acknowledgement"))?
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

pub(crate) fn persistence_actor_error(message: impl Into<String>) -> VegaError {
    VegaError::Io(std::io::Error::other(format!(
        "persistence actor: {}",
        message.into()
    )))
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
