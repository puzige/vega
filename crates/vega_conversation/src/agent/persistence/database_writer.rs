use super::*;
use std::collections::{HashMap, VecDeque};
use std::sync::{OnceLock, Weak};
use vega_store::Connection;

#[cfg(test)]
mod tests;

pub(crate) struct DatabaseWriter {
    pub(super) registrations: std::sync::mpsc::SyncSender<RunRegistration>,
    commands: std::sync::mpsc::SyncSender<Box<dyn crate::database_writer::WriteRequest>>,
    wakeup: std::sync::mpsc::SyncSender<()>,
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

struct ActiveRun {
    state: RunState,
    receiver: mpsc::Receiver<PersistenceCommand>,
    drained: oneshot::Sender<Result<(), VegaError>>,
}

impl DatabaseWriter {
    pub(super) fn wake(&self) {
        let _ = self.wakeup.try_send(());
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
        self.commands
            .send(command)
            .map_err(|_| persistence_actor_error("writer command queue closed"))?;
        self.wake();
        Ok(())
    }

    pub(crate) fn open_blocking(path: PathBuf) -> Result<Arc<Self>, VegaError> {
        let store = Store::open_with_busy_timeout(&path, PERSISTENCE_BUSY_TIMEOUT)?;
        let identity = store.database_identity();
        static WRITERS: OnceLock<
            Mutex<HashMap<vega_store::DatabaseIdentity, Weak<DatabaseWriter>>>,
        > = OnceLock::new();
        let mut writers = WRITERS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| persistence_actor_error("writer registry unavailable"))?;
        writers.retain(|_, writer| writer.strong_count() > 0);
        if let Some(writer) = writers.get(&identity).and_then(Weak::upgrade) {
            return Ok(writer);
        }
        store.conn().busy_handler(Some(persistence_busy_handler))?;
        let (registrations, receiver) = std::sync::mpsc::sync_channel(128);
        let (wakeup, awakened) = std::sync::mpsc::sync_channel(1);
        let (commands, pending) = std::sync::mpsc::sync_channel(128);
        let writer = Arc::new(Self {
            registrations,
            commands,
            wakeup,
        });
        std::thread::Builder::new()
            .name("vega-database-writer".into())
            .spawn(move || Self::run(store, receiver, pending, awakened))
            .map_err(|_| persistence_actor_error("writer startup failed"))?;
        writers.insert(identity, Arc::downgrade(&writer));
        Ok(writer)
    }

    fn run(
        store: Store,
        registrations: std::sync::mpsc::Receiver<RunRegistration>,
        commands: std::sync::mpsc::Receiver<Box<dyn crate::database_writer::WriteRequest>>,
        awakened: std::sync::mpsc::Receiver<()>,
    ) {
        let mut runs = VecDeque::<ActiveRun>::new();
        loop {
            while let Ok(run) = registrations.try_recv() {
                Self::register(&store, run, &mut runs);
            }
            let count = runs.len();
            let mut progressed = false;
            if let Ok(command) = commands.try_recv() {
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
            if !progressed && awakened.recv().is_err() {
                break;
            }
        }
    }

    fn register(store: &Store, run: RunRegistration, runs: &mut VecDeque<ActiveRun>) {
        let RunRegistration {
            state,
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
        let admission = self.verify_owner(store);
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
                let permission = if matches!(
                    event.as_ref(),
                    RuntimeEvent::ToolCallProposed(_)
                        | RuntimeEvent::ToolCallApproved { .. }
                        | RuntimeEvent::ToolCallRunning { .. }
                ) {
                    self.verify_permission(store)
                } else {
                    Ok(())
                };
                let result = permission
                    .and_then(|()| config.check_event(&event))
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
                        let transaction = if usage.is_some() {
                            Some(store.immediate_transaction()?)
                        } else {
                            None
                        };
                        match receipt(store.conn(), &operation_id, thread_id, run_id, "event")? {
                            Some(true) => return Ok(Vec::new()),
                            Some(false) => {
                                return Err(persistence_actor_error(
                                    "event commit requires reconciliation",
                                ));
                            }
                            None => {}
                        }
                        if transaction.is_none() {
                            save_receipt(
                                store.conn(),
                                &operation_id,
                                thread_id,
                                run_id,
                                "event",
                                "pending",
                            )?;
                        }
                        let acceptance_metadata = persist_runtime_event(
                            store,
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
                        if matches!(event.as_ref(), RuntimeEvent::SteeringAccepted(_)) {
                            save_receipt(
                                store.conn(),
                                &operation_id,
                                thread_id,
                                run_id,
                                "event",
                                "committed",
                            )?;
                            return Ok(acceptance_metadata);
                        }
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
                        save_receipt(
                            store.conn(),
                            &operation_id,
                            thread_id,
                            run_id,
                            "event",
                            "committed",
                        )?;
                        if let Some(transaction) = transaction {
                            transaction.commit()?;
                        }
                        let timestamp_owner = match event.as_ref() {
                            RuntimeEvent::AssistantResponseStarted { .. }
                            | RuntimeEvent::ResponseRetrying { .. }
                            | RuntimeEvent::Finished(_) => Some((message_id.as_str(), false)),
                            _ => None,
                        };
                        let mut metadata = Vec::new();
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
                        Ok(metadata)
                    })
                    .map_err(|error| error.at_runtime_stage(runtime_event_stage(&event)));
                log_persistence_result(
                    store,
                    runtime_event_category(&event),
                    (thread_id, run_id, &message_id, &scope),
                    started,
                    &result,
                );
                if result.is_ok()
                    && let RuntimeEvent::SteeringAccepted(input) = event.as_ref()
                {
                    message_id = input.assistant_message_id.clone();
                    active_response_id = None;
                }
                if result.is_ok()
                    && let RuntimeEvent::AssistantResponseStarted { response_id, .. } =
                        event.as_ref()
                {
                    active_response_id = Some(response_id.clone());
                }
                let _ = ack.send(result);
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
