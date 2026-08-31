use super::*;

#[derive(Clone, Default)]
pub struct PersistenceActorConfig {
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
    Running,
    Finished,
    PanicRunning,
}

impl PersistenceActorConfig {
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
        ack: oneshot::Sender<Result<(), VegaError>>,
    },
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
                                is_plan,
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
    ) -> Result<(), VegaError> {
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
            | RuntimeEvent::ToolCallOutput { .. }
    )
}
