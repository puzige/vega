use super::*;
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;
use vega_store::Connection;

#[cfg(test)]
mod tests;

type NecessaryReceiver = std::sync::mpsc::Receiver<Box<dyn crate::database_writer::WriteRequest>>;

pub(crate) struct DatabaseWriter {
    pub(super) registrations: std::sync::mpsc::SyncSender<RunRegistration>,
    commands: Arc<CommandQueue>,
    wakeup: std::sync::mpsc::SyncSender<()>,
    necessary: Arc<Mutex<Vec<NecessaryReceiver>>>,
    necessary_capacity: Arc<tokio::sync::Semaphore>,
    lifecycle: Arc<WriterLifecycle>,
}

#[derive(Default)]
struct WriterRegistry {
    writers: HashMap<vega_store::DatabaseIdentity, Arc<DatabaseWriter>>,
    closing: bool,
}

static WRITERS: OnceLock<Mutex<WriterRegistry>> = OnceLock::new();

#[derive(Default)]
struct WriterLifecycleState {
    closing: bool,
    stopped: bool,
    settled: bool,
}

#[derive(Default)]
struct WriterLifecycle {
    state: Mutex<WriterLifecycleState>,
    changed: std::sync::Condvar,
}

struct WriterLifetime {
    lifecycle: Arc<WriterLifecycle>,
    settled: bool,
}

impl Drop for WriterLifetime {
    fn drop(&mut self) {
        if let Ok(mut state) = self.lifecycle.state.lock() {
            state.stopped = true;
            state.settled = self.settled;
            self.lifecycle.changed.notify_all();
        }
    }
}

pub(super) struct RunRegistration {
    pub(super) state: RunState,
    pub(super) receiver: mpsc::Receiver<PersistenceCommand>,
    pub(super) ready: oneshot::Sender<Result<(), VegaError>>,
    pub(super) drained: oneshot::Sender<Result<(), VegaError>>,
}

pub(super) struct RunState {
    pub(super) start: PersistenceActorStart,
    pub(super) ordinary_task: Option<super::super::ordinary::OrdinaryTask>,
    pub(super) run_id: String,
    pub(super) active_response_id: Option<String>,
}

struct CommandQueue {
    state: Mutex<CommandQueueState>,
    changed: std::sync::Condvar,
}

#[derive(Default)]
struct CommandQueueState {
    commands: VecDeque<Box<dyn crate::database_writer::WriteRequest>>,
    waiters: VecDeque<u64>,
    next_ticket: u64,
    closed: bool,
}

struct CommandQueueLifetime(Arc<CommandQueue>);

impl Drop for CommandQueueLifetime {
    fn drop(&mut self) {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        let pending = std::mem::take(&mut state.commands);
        self.0.changed.notify_all();
        drop(state);
        drop(pending);
    }
}

impl CommandQueue {
    fn submit(
        &self,
        command: Box<dyn crate::database_writer::WriteRequest>,
        admission: Option<&crate::database_writer::CommitAdmission>,
    ) -> Result<(), crate::database_writer::CommitError> {
        use crate::database_writer::CommitError;
        if let Some(admission) = admission {
            admission.start_wait();
        }
        let mut state = self.state.lock().map_err(|_| CommitError::Unavailable)?;
        let ticket = state.next_ticket;
        state.next_ticket = ticket.checked_add(1).ok_or(CommitError::Unavailable)?;
        state.waiters.push_back(ticket);
        loop {
            let rejection = admission.and_then(|admission| admission.rejection(false));
            if state.closed || rejection.is_some() {
                state.waiters.retain(|waiting| *waiting != ticket);
                self.changed.notify_all();
                return Err(rejection.unwrap_or(CommitError::Unavailable));
            }
            if state.commands.len() < 128 && state.waiters.front() == Some(&ticket) {
                state.waiters.pop_front();
                state.commands.push_back(command);
                self.changed.notify_all();
                return Ok(());
            }
            (state, _) = self
                .changed
                .wait_timeout(state, Duration::from_millis(25))
                .map_err(|_| CommitError::Unavailable)?;
        }
    }

    fn pop(&self) -> Option<Box<dyn crate::database_writer::WriteRequest>> {
        let mut state = self.state.lock().ok()?;
        let command = state.commands.pop_front();
        self.changed.notify_all();
        command
    }
}

struct ActiveRun {
    state: RunState,
    receiver: mpsc::Receiver<PersistenceCommand>,
    drained: oneshot::Sender<Result<(), VegaError>>,
}

impl DatabaseWriter {
    pub(crate) fn wake(&self) {
        let _ = self.wakeup.try_send(());
    }

    pub(super) fn register_run(&self, mut registration: RunRegistration) -> Result<(), VegaError> {
        loop {
            let state = self
                .lifecycle
                .state
                .lock()
                .map_err(|_| persistence_actor_error("writer lifecycle unavailable"))?;
            if state.closing || state.stopped {
                return Err(persistence_actor_error("writer registration closed"));
            }
            match self.registrations.try_send(registration) {
                Ok(()) => {
                    self.wake();
                    return Ok(());
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    return Err(persistence_actor_error("writer registration closed"));
                }
                Err(std::sync::mpsc::TrySendError::Full(pending)) => registration = pending,
            }
            let _ = self
                .lifecycle
                .changed
                .wait_timeout(state, Duration::from_millis(25))
                .map_err(|_| persistence_actor_error("writer lifecycle unavailable"))?;
        }
    }

    fn begin_close(&self) -> Result<(), String> {
        let mut state = self
            .lifecycle
            .state
            .lock()
            .map_err(|_| "writer lifecycle unavailable")?;
        state.closing = true;
        self.necessary_capacity.close();
        let mut commands = self
            .commands
            .state
            .lock()
            .map_err(|_| "writer queue unavailable")?;
        commands.closed = true;
        self.commands.changed.notify_all();
        self.wake();
        Ok(())
    }

    fn wait_stopped(&self, deadline: std::time::Instant) -> Result<(), String> {
        let mut state = self
            .lifecycle
            .state
            .lock()
            .map_err(|_| "writer lifecycle unavailable")?;
        while !state.stopped {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(
                    "database writer unsettled: active runs or necessary terminal leases remain"
                        .into(),
                );
            }
            state = self
                .lifecycle
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| "writer lifecycle unavailable")?
                .0;
        }
        if !state.settled {
            return Err("database writer stopped without confirmed drain".into());
        }
        Ok(())
    }

    pub(crate) fn shutdown_all() -> Result<(), String> {
        let writers = {
            let mut registry = WRITERS
                .get_or_init(Default::default)
                .lock()
                .map_err(|_| "writer registry unavailable")?;
            registry.closing = true;
            registry.writers.values().cloned().collect::<Vec<_>>()
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        for writer in &writers {
            writer.begin_close()?;
        }
        let mut unsettled = 0;
        for writer in &writers {
            if writer.wait_stopped(deadline).is_err() {
                unsettled += 1;
            }
        }
        if unsettled != 0 {
            return Err(format!(
                "{unsettled} database writers have unconfirmed settlement"
            ));
        }
        Ok(())
    }

    pub(super) async fn open(path: PathBuf) -> Result<Arc<Self>, VegaError> {
        tokio::task::spawn_blocking(move || Self::open_blocking(path))
            .await
            .map_err(|_| persistence_actor_error("writer startup join failed"))?
    }

    pub(crate) fn submit(
        &self,
        command: Box<dyn crate::database_writer::WriteRequest>,
    ) -> Result<(), VegaError> {
        self.submit_with_admission(command, None)
            .map_err(|_| persistence_actor_error("writer command queue closed"))
    }

    pub(crate) fn submit_with_admission(
        &self,
        command: Box<dyn crate::database_writer::WriteRequest>,
        admission: Option<&crate::database_writer::CommitAdmission>,
    ) -> Result<(), crate::database_writer::CommitError> {
        self.commands.submit(command, admission)?;
        self.wake();
        Ok(())
    }

    pub(crate) fn reserve_necessary(
        self: &Arc<Self>,
    ) -> Result<crate::database_writer::NecessaryLease, String> {
        let lifecycle = self
            .lifecycle
            .state
            .lock()
            .map_err(|_| "writer lifecycle unavailable")?;
        if lifecycle.closing || lifecycle.stopped {
            return Err("necessary terminal registration closed".into());
        }
        let capacity = self
            .necessary_capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| "necessary terminal capacity exhausted".to_string())?;
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        self.necessary
            .lock()
            .map_err(|_| "necessary terminal registry unavailable".to_string())?
            .push(receiver);
        self.wake();
        Ok(crate::database_writer::NecessaryLease::new(
            self.clone(),
            sender,
            capacity,
        ))
    }

    pub(crate) fn open_blocking(path: PathBuf) -> Result<Arc<Self>, VegaError> {
        let store = Store::open_with_busy_timeout(&path, PERSISTENCE_BUSY_TIMEOUT)?;
        let identity = store.database_identity();
        let mut registry = WRITERS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| persistence_actor_error("writer registry unavailable"))?;
        if registry.closing {
            return Err(persistence_actor_error("database writers closing"));
        }
        if let Some(writer) = registry.writers.get(&identity) {
            let state = writer
                .lifecycle
                .state
                .lock()
                .map_err(|_| persistence_actor_error("writer lifecycle unavailable"))?;
            if state.closing || state.stopped {
                return Err(persistence_actor_error("database writer closed"));
            }
            return Ok(writer.clone());
        }
        store.conn().busy_handler(Some(persistence_busy_handler))?;
        let (registrations, receiver) = std::sync::mpsc::sync_channel(128);
        let (wakeup, awakened) = std::sync::mpsc::sync_channel(1);
        let commands = Arc::new(CommandQueue {
            state: Mutex::new(CommandQueueState::default()),
            changed: std::sync::Condvar::new(),
        });
        let pending = commands.clone();
        let necessary = Arc::new(Mutex::new(Vec::new()));
        let lifecycle = Arc::new(WriterLifecycle::default());
        let lifetime = lifecycle.clone();
        let writer = Arc::new(Self {
            registrations,
            commands,
            wakeup,
            necessary: necessary.clone(),
            necessary_capacity: Arc::new(tokio::sync::Semaphore::new(128)),
            lifecycle,
        });
        std::thread::Builder::new()
            .name("vega-database-writer".into())
            .spawn(move || Self::run(store, receiver, pending, awakened, necessary, lifetime))
            .map_err(|_| persistence_actor_error("writer startup failed"))?;
        registry.writers.insert(identity, writer.clone());
        Ok(writer)
    }

    fn run(
        store: Store,
        registrations: std::sync::mpsc::Receiver<RunRegistration>,
        commands: Arc<CommandQueue>,
        awakened: std::sync::mpsc::Receiver<()>,
        necessary: Arc<Mutex<Vec<NecessaryReceiver>>>,
        lifecycle: Arc<WriterLifecycle>,
    ) {
        let mut lifetime = WriterLifetime {
            lifecycle: lifecycle.clone(),
            settled: false,
        };
        let _commands_lifetime = CommandQueueLifetime(commands.clone());
        let mut runs = VecDeque::<ActiveRun>::new();
        let mut terminal_failed = false;
        loop {
            while let Ok(run) = registrations.try_recv() {
                lifecycle.changed.notify_all();
                Self::register(&store, run, &mut runs);
            }
            let count = runs.len();
            let mut progressed = false;
            let terminals = necessary
                .lock()
                .map(|mut receivers| {
                    let mut ready = Vec::new();
                    receivers.retain(|receiver| {
                        if !ready.is_empty() {
                            return true;
                        }
                        match receiver.try_recv() {
                            Ok(command) => {
                                ready.push(command);
                                false
                            }
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
                            Err(std::sync::mpsc::TryRecvError::Empty) => true,
                        }
                    });
                    ready
                })
                .unwrap_or_default();
            for command in terminals {
                terminal_failed |= !command.execute_terminal(&store);
                progressed = true;
            }
            if let Some(command) = commands.pop() {
                command.execute(&store);
                progressed = true;
            }
            for _ in 0..count {
                let Some(mut run) = runs.pop_front() else {
                    break;
                };
                match run.receiver.try_recv() {
                    Ok(command) => {
                        run.state.apply(&store, command);
                        progressed = true;
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        if let Some(task) = run.state.ordinary_task.take() {
                            task.complete(Ok(()));
                        }
                        let _ = run.drained.send(Ok(()));
                        continue;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {}
                }
                runs.push_back(run);
            }
            if !progressed {
                let closing = lifecycle
                    .state
                    .lock()
                    .map(|state| state.closing)
                    .unwrap_or(true);
                let no_terminals = necessary
                    .lock()
                    .map(|receivers| receivers.is_empty())
                    .unwrap_or(false);
                if closing && runs.is_empty() && no_terminals {
                    match registrations.try_recv() {
                        Ok(run) => {
                            lifecycle.changed.notify_all();
                            Self::register(&store, run, &mut runs);
                            continue;
                        }
                        Err(
                            std::sync::mpsc::TryRecvError::Empty
                            | std::sync::mpsc::TryRecvError::Disconnected,
                        ) => {}
                    }
                    if let Some(command) = commands.pop() {
                        command.execute(&store);
                        continue;
                    }
                    lifetime.settled = !terminal_failed;
                    break;
                }
                if awakened.recv().is_err() {
                    break;
                }
            }
        }
    }

    fn register(store: &Store, run: RunRegistration, runs: &mut VecDeque<ActiveRun>) {
        let RunRegistration {
            mut state,
            receiver,
            ready,
            drained,
        } = run;
        if state
            .start
            .config
            .admission
            .as_ref()
            .is_none_or(|admission| {
                !admission.verifies(store.database_identity(), &state.start.thread_id)
            })
        {
            let _ = ready.send(Err(persistence_actor_error("thread admission changed")));
            let _ = drained.send(Err(persistence_actor_error("thread admission changed")));
            return;
        }
        let restored = (|| -> Result<(), VegaError> {
            let mut statement = store.conn().prepare(
                "SELECT operation_id FROM persistence_command_receipts WHERE thread_id=?1 AND run_id=?2 AND kind='event' AND state='committed' ORDER BY rowid DESC LIMIT 1",
            )?;
            let mut rows = statement.query((&state.start.thread_id, &state.run_id))?;
            if let Some(row) = rows.next()? {
                let operation: String = row.get(0)?;
                let (_, seq, message, response) = restore_reply(store.conn(), &operation)?;
                state.start.next_tool_seq = seq;
                state.start.message_id = message;
                state.active_response_id = response;
            }
            Ok(())
        })();
        if let Err(error) = restored {
            let _ = ready.send(Err(error));
            let _ = drained.send(Err(persistence_actor_error("run recovery unavailable")));
            return;
        }
        let _ = ready.send(Ok(()));
        runs.push_back(ActiveRun {
            state,
            receiver,
            drained,
        });
    }
}

impl RunState {
    fn verify_owner(&self, store: &Store) -> Result<(), VegaError> {
        if self
            .start
            .config
            .admission
            .as_ref()
            .is_none_or(|admission| {
                !admission.verifies(store.database_identity(), &self.start.thread_id)
            })
            || self
                .start
                .config
                .delegated_owner
                .as_ref()
                .is_some_and(|owner| {
                    !owner.verifies(&self.start.database_path, &self.start.thread_id)
                })
        {
            return Err(persistence_actor_error("persistence owner changed"));
        }
        Ok(())
    }

    fn verify_permission(&self, store: &Store) -> Result<(), VegaError> {
        self.verify_owner(store)?;
        let encoded = vega_store::permission_contracts::load_run_fence(
            store.conn(),
            &self.start.message_id,
            &self.start.thread_id,
        )?;
        let Some(encoded) = encoded else {
            return Err(persistence_actor_error("run authority unavailable"));
        };
        let fence: crate::types::PermissionFence = serde_json::from_str(&encoded)
            .map_err(|_| persistence_actor_error("run authority invalid"))?;
        verify_fence(store, &fence)?;
        if let Some(source) = &fence.delegation {
            verify_fence(store, &source.parent_fence)?;
        }
        Ok(())
    }

    fn apply(&mut self, store: &Store, command: PersistenceCommand) {
        let admission = store
            .verify_database_identity(store.database_identity())
            .map_err(VegaError::from)
            .and_then(|()| self.verify_owner(store));
        if let Err(error) = admission {
            match command {
                PersistenceCommand::Snapshot { ack, .. } => {
                    let _ = ack.send(Err(error));
                }
                PersistenceCommand::Event { ack, .. } => {
                    let _ = ack.send(Err(error));
                }
                PersistenceCommand::Diagnostic(_) => {}
            }
            return;
        }
        let project_id = &self.start.project_id;
        let thread_id = &self.start.thread_id;
        let run_id = &self.run_id;
        let model = &self.start.model;
        let is_plan = self.start.is_plan;
        let config = &self.start.config;
        let mut next_tool_seq = self.start.next_tool_seq;
        let mut message_id = self.start.message_id.clone();
        let mut active_response_id = self.active_response_id.clone();
        let _execution = vega_store::write_coordination::ExecutionScope::begin(
            thread_id,
            run_id,
            vega_store::write_coordination::WriteActivity::Persistence,
        );
        match command {
            PersistenceCommand::Snapshot {
                operation_id,
                content,
                ack,
            } => {
                config.delay_command();
                let scope = vega_store::write_coordination::ObservationScope::begin(store.conn());
                let started = std::time::Instant::now();
                let result = (|| -> Result<(), VegaError> {
                    let transaction = store.immediate_transaction()?;
                    match receipt(store.conn(), &operation_id, thread_id, run_id, "snapshot")? {
                        Some(true) => return Ok(()),
                        Some(false) => {
                            return Err(persistence_actor_error(
                                "snapshot commit requires reconciliation",
                            ));
                        }
                        None => {}
                    }
                    ensure_message_updated(
                        messages::update_streaming_content(store.conn(), &message_id, &content)?,
                        &message_id,
                    )?;
                    save_receipt(
                        store.conn(),
                        &operation_id,
                        thread_id,
                        run_id,
                        "snapshot",
                        "committed",
                    )?;
                    transaction.commit()?;
                    Ok(())
                })()
                .map_err(|error| {
                    error.at_runtime_stage(vega_runtime::RuntimeFailureStage::Snapshot)
                });
                if result.is_ok() {
                    config.record_snapshot();
                }
                log_persistence_result(
                    store,
                    PersistenceCategory::Snapshot,
                    (thread_id, run_id, &message_id, &scope),
                    started,
                    &result,
                );
                let _ = ack.send(result);
            }
            PersistenceCommand::Event {
                operation_id,
                event,
                content,
                execution_duration_ms,
                ack,
            } => {
                config.delay_command();
                let scope = vega_store::write_coordination::ObservationScope::begin(store.conn());
                let started = std::time::Instant::now();
                let mut apply = || -> Result<Vec<ConversationEvent>, VegaError> {
                    config.check_event(&event)?;
                    let permission = crate::permission_contracts::coordinate().map_err(|_| {
                        persistence_actor_error("permission coordination unavailable")
                    })?;
                    let transaction = store.immediate_transaction()?;
                    match receipt(store.conn(), &operation_id, thread_id, run_id, "event")? {
                        Some(true) => {
                            return restore_reply(store.conn(), &operation_id)
                                .map(|(metadata, _, _, _)| metadata);
                        }
                        Some(false) => {
                            return Err(persistence_actor_error(
                                "event commit requires reconciliation",
                            ));
                        }
                        None => {}
                    }
                    if matches!(
                        event.as_ref(),
                        RuntimeEvent::ToolCallProposed(_)
                            | RuntimeEvent::ToolCallApproved { .. }
                            | RuntimeEvent::ToolCallRunning { .. }
                    ) {
                        self.verify_permission(store)?;
                    }
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
                    let acceptance_metadata = persist_runtime_event(
                        store,
                        &transaction,
                        project_id,
                        thread_id,
                        &message_id,
                        model,
                        is_plan,
                        &content,
                        &mut next_tool_seq,
                        &event,
                        execution_duration_ms,
                        active_response_id.as_deref(),
                    )?;
                    if usage.is_some()
                        && let Some((parent_thread, parent_message)) = &config.delegated_parent
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
                            thread_id,
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
                        .map_err(|_| VegaError::Tool {
                            tool: "automation".into(),
                            message: "usage persistence unavailable".into(),
                        })?;
                    }
                    let timestamp_owner = match event.as_ref() {
                        RuntimeEvent::AssistantResponseStarted { .. }
                        | RuntimeEvent::ResponseRetrying { .. }
                        | RuntimeEvent::Finished(_) => Some((message_id.as_str(), false)),
                        _ => None,
                    };
                    let mut metadata = acceptance_metadata;
                    if let Some((owner, user)) = timestamp_owner {
                        if user && messages::timestamp_break_before(store.conn(), owner)? {
                            metadata.push(ConversationEvent::TimestampBreakBefore {
                                message_id: owner.to_owned(),
                            });
                        }
                        let clock = if user {
                            messages::find(store.conn(), owner)?.map(|row| row.created_at)
                        } else {
                            messages::last_assistant_response_start(store.conn(), owner)?
                        };
                        let clock = if clock.is_none()
                            && is_plan
                            && matches!(event.as_ref(), RuntimeEvent::Finished(_))
                        {
                            messages::find(store.conn(), owner)?.map(|row| row.created_at)
                        } else {
                            clock
                        };
                        metadata.push(ConversationEvent::MessageTimestamp {
                            message_id: owner.to_owned(),
                            sent_at_ms: crate::types::valid_message_timestamp(
                                clock,
                                crate::types::unix_now_ms(),
                            ),
                        });
                    }
                    if let RuntimeEvent::SteeringAccepted(input) = event.as_ref() {
                        message_id = input.assistant_message_id.clone();
                        active_response_id = None;
                    }
                    if let RuntimeEvent::AssistantResponseStarted { response_id, .. } =
                        event.as_ref()
                    {
                        active_response_id = Some(response_id.clone());
                    }
                    save_event_reply(
                        store.conn(),
                        &operation_id,
                        thread_id,
                        run_id,
                        &metadata,
                        next_tool_seq,
                        &message_id,
                        active_response_id.as_deref(),
                    )?;
                    transaction.commit()?;
                    drop(permission);
                    Ok(metadata)
                };
                let result = if let RuntimeEvent::ScreenshotDelivered {
                    image,
                    control,
                    epoch,
                    cancel,
                    delivered,
                    ..
                } = event.as_ref()
                {
                    crate::attachments::validate_images(std::slice::from_ref(image))
                        .map_err(|_| persistence_actor_error("invalid screenshot"))
                        .and_then(|()| {
                            match control.commit_delivery(*epoch, || {
                                if cancel.is_cancelled() {
                                    return Ok(Vec::new());
                                }
                                let metadata = apply()?;
                                delivered.store(true, std::sync::atomic::Ordering::Release);
                                Ok(metadata)
                            }) {
                                Ok(result) => result,
                                Err(vega_runtime::computer_use::ComputerUseError::Cancelled) => {
                                    Ok(Vec::new())
                                }
                                Err(_) => {
                                    Err(persistence_actor_error("screenshot control unavailable"))
                                }
                            }
                        })
                } else {
                    apply()
                };
                let result =
                    result.map_err(|error| error.at_runtime_stage(runtime_event_stage(&event)));
                log_persistence_result(
                    store,
                    runtime_event_category(&event),
                    (thread_id, run_id, &message_id, &scope),
                    started,
                    &result,
                );
                if result.is_ok() {
                    self.start.next_tool_seq = next_tool_seq;
                    self.start.message_id = message_id.clone();
                    self.active_response_id = active_response_id.clone();
                }
                let _ = ack.send(result);
                return;
            }
            PersistenceCommand::Diagnostic(event) => {
                let scope = vega_store::write_coordination::ObservationScope::begin(store.conn());
                let started = std::time::Instant::now();
                let result =
                    vega_store::run_diagnostics::insert(store.conn(), &event).map_err(|error| {
                        match error {
                            vega_store::run_diagnostics::DiagnosticError::Sql(error) => {
                                VegaError::from(error).at_runtime_stage(
                                    vega_runtime::RuntimeFailureStage::EventPipeline,
                                )
                            }
                            _ => persistence_actor_error("diagnostic persistence failed"),
                        }
                    });
                log_persistence_result(
                    store,
                    PersistenceCategory::Diagnostic,
                    (&event.thread_id, &event.run_id, &message_id, &scope),
                    started,
                    &result,
                );
            }
        }

        self.start.next_tool_seq = next_tool_seq;
        self.start.message_id = message_id;
        self.active_response_id = active_response_id;
    }
}

fn receipt(
    conn: &Connection,
    operation: &str,
    thread: &str,
    run: &str,
    kind: &str,
) -> Result<Option<bool>, VegaError> {
    let mut statement = conn.prepare("SELECT thread_id,run_id,kind,state FROM persistence_command_receipts WHERE operation_id=?1")?;
    let mut rows = statement.query([operation])?;
    let row: Option<(String, String, String, String)> = match rows.next()? {
        Some(row) => Some((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        None => None,
    };
    match row {
        Some((owner, execution, category, state))
            if owner == thread && execution == run && category == kind =>
        {
            Ok(Some(state == "committed"))
        }
        Some(_) => Err(persistence_actor_error("operation identity mismatch")),
        None => Ok(None),
    }
}

fn verify_fence(store: &Store, fence: &crate::types::PermissionFence) -> Result<(), VegaError> {
    let state = vega_store::permission_contracts::load_state(store.conn())?;
    let thread = vega_store::threads::find(store.conn(), &fence.thread_id)?
        .ok_or_else(|| persistence_actor_error("authority thread missing"))?;
    if !fence.is_valid()
        || state.reset_pending
        || state.authority_epoch != fence.authority_epoch
        || fence.database_identity != store.database_identity().permission_fence_key()
        || thread.project_id != fence.project_id
        || thread.mode != fence.run_mode
        || thread.permission_mode != fence.permission_mode
        || thread.permission_revision != fence.permission_revision
        || thread.status != "active"
    {
        return Err(persistence_actor_error("authority changed while queued"));
    }
    Ok(())
}

fn save_receipt(
    conn: &Connection,
    operation: &str,
    thread: &str,
    run: &str,
    kind: &str,
    state: &str,
) -> Result<(), VegaError> {
    let _permit = vega_store::write_coordination::acquire(
        conn,
        vega_store::write_coordination::WriteCategory::RuntimeEvent,
    )?;
    conn.execute("INSERT INTO persistence_command_receipts(operation_id,thread_id,run_id,kind,state,created_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(operation_id) DO UPDATE SET state=excluded.state WHERE thread_id=excluded.thread_id AND run_id=excluded.run_id AND kind=excluded.kind",
        (operation,thread,run,kind,state,now_ms()))?;
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
enum ReplyEvent {
    Accepted {
        message_id: String,
        seq: u64,
    },
    Timestamp {
        message_id: String,
        sent_at_ms: Option<i64>,
    },
    Boundary {
        message_id: String,
    },
}

#[allow(clippy::too_many_arguments)]
fn save_event_reply(
    conn: &Connection,
    operation: &str,
    thread: &str,
    run: &str,
    metadata: &[ConversationEvent],
    next_seq: i64,
    message: &str,
    response: Option<&str>,
) -> Result<(), VegaError> {
    let events = metadata
        .iter()
        .map(|event| match event {
            ConversationEvent::UserMessageAccepted { message_id, seq } => {
                Ok(ReplyEvent::Accepted {
                    message_id: message_id.clone(),
                    seq: *seq,
                })
            }
            ConversationEvent::MessageTimestamp {
                message_id,
                sent_at_ms,
            } => Ok(ReplyEvent::Timestamp {
                message_id: message_id.clone(),
                sent_at_ms: *sent_at_ms,
            }),
            ConversationEvent::TimestampBreakBefore { message_id } => Ok(ReplyEvent::Boundary {
                message_id: message_id.clone(),
            }),
            _ => Err(persistence_actor_error("unsupported persistence reply")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let encoded = serde_json::to_string(&events)
        .map_err(|_| persistence_actor_error("reply encoding failed"))?;
    save_receipt(conn, operation, thread, run, "event", "committed")?;
    conn.execute("UPDATE persistence_command_receipts SET reply_json=?1,next_tool_seq=?2,active_message_id=?3,active_response_id=?4 WHERE operation_id=?5", (&encoded,next_seq,message,response,operation))?;
    Ok(())
}

type RestoredReply = (Vec<ConversationEvent>, i64, String, Option<String>);

fn restore_reply(conn: &Connection, operation: &str) -> Result<RestoredReply, VegaError> {
    let (encoded,seq,owner,active): (String,i64,String,Option<String>) = conn.query_row("SELECT reply_json,next_tool_seq,active_message_id,active_response_id FROM persistence_command_receipts WHERE operation_id=?1", [operation], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)))?;
    let events: Vec<ReplyEvent> = serde_json::from_str(&encoded)
        .map_err(|_| persistence_actor_error("reply recovery failed"))?;
    let metadata = events
        .into_iter()
        .map(|event| match event {
            ReplyEvent::Accepted { message_id, seq } => {
                ConversationEvent::UserMessageAccepted { message_id, seq }
            }
            ReplyEvent::Timestamp {
                message_id,
                sent_at_ms,
            } => ConversationEvent::MessageTimestamp {
                message_id,
                sent_at_ms,
            },
            ReplyEvent::Boundary { message_id } => {
                ConversationEvent::TimestampBreakBefore { message_id }
            }
        })
        .collect();
    Ok((metadata, seq, owner, active))
}
