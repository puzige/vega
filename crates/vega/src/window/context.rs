//! Context operations use the same single-flight gate as sends/model changes.
//! Every asynchronous result also belongs to one exact route incarnation.
use super::*;
use tokio_util::sync::CancellationToken;
use vega_store::Store;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ContextOwner {
    epoch: u64,
    thread_id: String,
    model: String,
    stream: Entity<ConversationStream>,
}

pub(crate) struct ManualContextRun {
    owner: ContextOwner,
    request_id: u64,
    lease: TrustedActionToken,
    cancel: CancellationToken,
}

#[derive(Default)]
pub(crate) struct ContextController {
    epoch: u64,
    owner: Option<ContextOwner>,
    load_sequence: u64,
    pub(crate) manual: Option<ManualContextRun>,
    saving: Option<(ContextOwner, u64)>,
    // Runtime compaction numbering restarts each run. Only this tuple may
    // reuse a UI operation id; it is never an app-level ownership fence.
    automatic: Option<(u64, u64, u64)>,
    primary_owner: Option<(u64, ContextOwner)>,
    // Observes the real async service completion in app integration tests;
    // it does not alter scheduling, projection or error handling.
    #[cfg(test)]
    pub(crate) last_load_succeeded: Option<bool>,
}

impl ContextController {
    pub(super) fn cancel(&mut self) {
        if let Some(active) = &self.manual {
            active.cancel.cancel();
        }
    }

    fn invalidate(&mut self) {
        self.cancel();
        self.epoch = self.epoch.saturating_add(1);
        self.owner = None;
        self.automatic = None;
        self.primary_owner = None;
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| i64::try_from(time.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

fn context_record(id: u64, status: ContextCompactionStatus) -> ContextCompactionStatusRecord {
    ContextCompactionStatusRecord {
        generation: id,
        status,
        updated_at: now_ms(),
        estimated_tokens: None,
        input_budget: None,
        target_tokens: None,
        source_version: None,
        failure: None,
        usage: ContextCompactionUsageState::Pending,
    }
}

impl VegaWindow {
    pub(crate) fn begin_context_primary_owner(&mut self, generation: u64) {
        self.invalidate_context_load();
        self.context_controller.primary_owner = self
            .context_controller
            .owner
            .clone()
            .map(|owner| (generation, owner));
    }

    pub(crate) fn start_context_compaction(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ContextCompactionRequested,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.context_controller.owner.clone().filter(|owner| {
            owner.stream == stream
                && owner.thread_id == request.thread_id
                && owner.model == request.model
                && self.context_owner_matches(owner, cx)
        }) else {
            return;
        };
        if self
            .context_controller
            .manual
            .as_ref()
            .is_some_and(|active| active.owner == owner && active.request_id == request.request_id)
        {
            return;
        }
        let reasoning = stream.read(cx).frozen_reasoning_for_submit();
        let allowed = self.agent_controller.active.is_none()
            && self.context_controller.manual.is_none()
            && !stream.read(cx).has_pending_model_selection()
            && self.reasoning_save_pending.is_none()
            && !self.model_catalog_loading
            && !stream.read(cx).reasoning_unavailable()
            && reasoning.is_ok();
        let lease = allowed
            .then(|| {
                self.trusted_actions.acquire(
                    TrustedActionKind::ContextCompaction,
                    owner.epoch,
                    request.request_id,
                )
            })
            .flatten();
        let (Some(lease), Some(database), Ok(reasoning)) =
            (lease, self.context_database(cx), reasoning)
        else {
            if let Some(lease) = lease {
                self.trusted_actions.release(lease);
            }
            let mut record = context_record(request.request_id, ContextCompactionStatus::Failed);
            record.failure = Some(ContextCompactionFailureCode::Unavailable);
            stream.update(cx, |stream, cx| {
                stream.apply_context_status(&owner.thread_id, &owner.model, record, cx);
            });
            return;
        };
        let request_id = request.request_id;
        let cancel = CancellationToken::new();
        self.context_controller.manual = Some(ManualContextRun {
            owner: owner.clone(),
            request_id,
            lease,
            cancel: cancel.clone(),
        });
        self.invalidate_context_load();
        stream.update(cx, |stream, cx| {
            stream.set_trusted_action_busy(true, cx);
            stream.apply_context_status(
                &owner.thread_id,
                &owner.model,
                context_record(request_id, ContextCompactionStatus::Compacting),
                cx,
            );
        });
        let config = self.composer_config_path();
        let pricing = self.pricing_controller.catalog_for_run(&owner.model);
        let thread_id = owner.thread_id.clone();
        let model = owner.model.clone();
        let project_id = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.project_id.clone())
            .unwrap_or_default();
        let activity = vega_ui::sidebar::register_project_worker(&project_id, cx);
        let completion_cancel = cancel.clone();
        #[cfg(test)]
        let provider_override = self.agent_provider_override.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        // A dedicated thread mirrors the primary worker. In particular, a
        // blocking Tokio runtime must not occupy GPUI's cooperative executor.
        let worker = std::thread::Builder::new()
            .name("vega-context".into())
            .spawn(move || {
                let _activity = activity;
                let events = run_manual_context_worker(
                    database,
                    config,
                    thread_id,
                    model,
                    request_id,
                    cancel,
                    reasoning,
                    pricing,
                    #[cfg(test)]
                    provider_override,
                );
                let _ = sender.send(events);
            });
        cx.spawn(async move |this, cx| {
            let events = if worker.is_err() {
                Err(())
            } else {
                loop {
                    cx.background_executor().timer(AGENT_EVENT_POLL).await;
                    match receiver.try_recv() {
                        Ok(events) => break events,
                        Err(std::sync::mpsc::TryRecvError::Empty) => continue,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break Err(()),
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                let matches = this
                    .context_controller
                    .manual
                    .as_ref()
                    .is_some_and(|active| {
                        active.owner == owner
                            && active.request_id == request_id
                            && active.lease == lease
                    });
                if !matches {
                    return;
                }
                this.context_controller.manual = None;
                if !this.trusted_actions.release(lease) {
                    return;
                }
                stream.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                if !this.context_owner_matches(&owner, cx) {
                    return;
                }
                match events {
                    Ok(events) => stream.update(cx, |stream, cx| {
                        for event in events {
                            if let ConversationEvent::ContextCompactionStatus { mut record } = event
                            {
                                record.generation = request_id;
                                stream.apply_context_status(
                                    &owner.thread_id,
                                    &owner.model,
                                    record,
                                    cx,
                                );
                            } else {
                                stream.apply_event(event, cx);
                            }
                        }
                    }),
                    Err(()) => stream.update(cx, |stream, cx| {
                        let cancelled = completion_cancel.is_cancelled();
                        let mut record = context_record(
                            request_id,
                            if cancelled {
                                ContextCompactionStatus::Cancelled
                            } else {
                                ContextCompactionStatus::Failed
                            },
                        );
                        record.usage = ContextCompactionUsageState::Unknown;
                        record.failure = Some(if cancelled {
                            ContextCompactionFailureCode::Cancelled
                        } else {
                            ContextCompactionFailureCode::Unavailable
                        });
                        stream.apply_context_status(&owner.thread_id, &owner.model, record, cx);
                    }),
                }
                this.refresh_context_projection(false, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn invalidate_context_load(&mut self) {
        self.context_controller.load_sequence =
            self.context_controller.load_sequence.saturating_add(1);
    }

    pub(crate) fn cancel_manual_context_for_stream(&mut self, stream: &Entity<ConversationStream>) {
        if let Some(active) = &self.context_controller.manual
            && active.owner.stream == *stream
        {
            active.cancel.cancel();
        }
    }
    pub(crate) fn cancel_context_if_route_stale(&mut self, cx: &App) {
        let stale = self.context_controller.owner.as_ref().is_some_and(|owner| {
            cx.global::<SettingsOpen>().0
                || !self.owns_stream_request(&owner.stream, &owner.thread_id, cx)
                || cx
                    .global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_none_or(|thread| thread.model != owner.model)
        });
        if stale {
            self.context_controller.invalidate();
        }
    }

    fn context_owner_matches(&self, owner: &ContextOwner, cx: &App) -> bool {
        self.context_controller.owner.as_ref() == Some(owner)
            && !cx.global::<SettingsOpen>().0
            && self.owns_stream_request(&owner.stream, &owner.thread_id, cx)
            && owner.stream.read(cx).displayed_model() == owner.model
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.model == owner.model)
    }

    /// Render only schedules work on an identity change; no storage is read here.
    pub(crate) fn sync_context_route(
        &mut self,
        stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = cx.global::<OpenedThread>().0.clone() else {
            return;
        };
        if self.is_draft_route(&thread.id) || !self.owns_stream_request(stream, &thread.id, cx) {
            return;
        }
        if self.context_controller.owner.as_ref().is_some_and(|owner| {
            owner.thread_id == thread.id && owner.model == thread.model && owner.stream == *stream
        }) {
            return;
        }
        self.context_controller.invalidate();
        self.context_controller.owner = Some(ContextOwner {
            epoch: self.context_controller.epoch,
            thread_id: thread.id,
            model: thread.model,
            stream: stream.clone(),
        });
        self.refresh_context_projection(true, cx);
    }

    pub(super) fn context_database(&self, cx: &App) -> Option<PathBuf> {
        cx.global::<VegaStore>()
            .0
            .as_ref()
            .ok()?
            .database_path()
            .map(PathBuf::from)
    }

    pub(crate) fn refresh_context_projection(
        &mut self,
        restore_status: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.context_controller.owner.clone() else {
            return;
        };
        let Some(database) = self.context_database(cx) else {
            return;
        };
        self.context_controller.load_sequence =
            self.context_controller.load_sequence.saturating_add(1);
        let sequence = self.context_controller.load_sequence;
        #[cfg(test)]
        {
            self.context_controller.last_load_succeeded = None;
        }
        let worker_owner = owner.clone();
        cx.spawn(async move |this, cx| {
            let projection = cx
                .background_executor()
                .spawn(async move {
                    let store = Store::open(database).map_err(|_| ())?;
                    vega_conversation::agent::read_context_projection(
                        &store,
                        &worker_owner.thread_id,
                        &worker_owner.model,
                        SYSTEM_PROMPT,
                    )
                    .map_err(|_| ())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.context_owner_matches(&owner, cx)
                    || this.context_controller.load_sequence != sequence
                {
                    return;
                }
                #[cfg(test)]
                {
                    this.context_controller.last_load_succeeded = Some(projection.is_ok());
                }
                match projection {
                    Ok(projection) => {
                        owner.stream.update(cx, |stream, cx| {
                            stream.restore_context_unknown_usage(
                                &owner.thread_id,
                                &owner.model,
                                projection.unknown_usage,
                                cx,
                            );
                            stream.apply_context_projection(
                                &owner.thread_id,
                                &owner.model,
                                projection.settings,
                                projection.estimated_tokens,
                                projection.compactable,
                                cx,
                            );
                            if restore_status
                                && let Some(mut record) = projection.last_status
                                && let Some(id) = stream.reserve_context_operation_id(cx)
                            {
                                record.generation = id;
                                stream.apply_context_status(
                                    &owner.thread_id,
                                    &owner.model,
                                    record,
                                    cx,
                                );
                            }
                        });
                    }
                    Err(()) => {
                        owner.stream.update(cx, |stream, cx| {
                            stream.apply_context_load_error(&owner.thread_id, &owner.model, cx);
                        });
                    }
                }
            });
        })
        .detach();
    }

    pub(crate) fn persist_context_settings(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ContextSettingsRequested,
        cx: &mut Context<Self>,
    ) {
        let owner = self.context_controller.owner.clone().filter(|owner| {
            owner.stream == stream
                && owner.thread_id == request.settings.thread_id
                && owner.model == request.settings.model
                && self.context_owner_matches(owner, cx)
        });
        if owner.as_ref().is_some_and(|owner| {
            self.context_controller
                .saving
                .as_ref()
                .is_some_and(|(saving, id)| saving == owner && *id == request.request_id)
        }) {
            return;
        }
        let lease = owner
            .as_ref()
            .filter(|_| self.agent_controller.active.is_none())
            .and_then(|owner| {
                self.trusted_actions.acquire(
                    TrustedActionKind::ContextSettings,
                    owner.epoch,
                    request.request_id,
                )
            });
        let (Some(owner), Some(lease), Some(database)) = (owner, lease, self.context_database(cx))
        else {
            if let Some(lease) = lease {
                self.trusted_actions.release(lease);
            }
            stream.update(cx, |stream, cx| {
                stream.finish_context_settings(
                    &request.settings.thread_id,
                    &request.settings.model,
                    request.request_id,
                    None,
                    cx,
                );
            });
            return;
        };
        let request_id = request.request_id;
        self.context_controller.saving = Some((owner.clone(), request_id));
        let mut settings = request.settings.clone();
        settings.updated_at = now_ms();
        // Invalidate any older metadata load before it can overwrite this ACK.
        self.context_controller.load_sequence =
            self.context_controller.load_sequence.saturating_add(1);
        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    let store = Store::open(database).ok()?;
                    vega_conversation::agent::save_context_settings(&store, &settings).ok()?;
                    vega_conversation::agent::read_context_settings(
                        &store,
                        &settings.thread_id,
                        &settings.model,
                    )
                    .ok()?
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.trusted_actions.release(lease) {
                    return;
                }
                if this.context_controller.saving.as_ref() == Some(&(owner.clone(), request_id)) {
                    this.context_controller.saving = None;
                }
                stream.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                if this.context_owner_matches(&owner, cx) {
                    stream.update(cx, |stream, cx| {
                        stream.finish_context_settings(
                            &owner.thread_id,
                            &owner.model,
                            request_id,
                            saved,
                            cx,
                        );
                    });
                    this.refresh_context_projection(false, cx);
                } else {
                    // Retire only this exact old pending request. Do not apply
                    // saved values across an ABA route or replace newer edits.
                    // The UI rejects this ACK after a model/entity/id change.
                    stream.update(cx, |stream, cx| {
                        stream.finish_context_settings(
                            &owner.thread_id,
                            &owner.model,
                            request_id,
                            None,
                            cx,
                        );
                    });
                }
            });
        })
        .detach();
    }

    pub(crate) fn project_automatic_context(
        &mut self,
        run_generation: u64,
        stream: &Entity<ConversationStream>,
        mut record: ContextCompactionStatusRecord,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_primary_context_event(run_generation, stream, cx) {
            return;
        }
        let Some(owner) = self
            .context_controller
            .owner
            .clone()
            .filter(|owner| owner.stream == *stream && self.context_owner_matches(owner, cx))
        else {
            return;
        };
        if !self
            .agent_controller
            .matches(run_generation, &owner.thread_id, stream)
        {
            return;
        }
        if self.context_controller.primary_owner.as_ref() != Some(&(run_generation, owner.clone()))
        {
            return;
        }
        let runtime_generation = record.generation;
        let id = match self.context_controller.automatic {
            Some((run, operation, id))
                if run == run_generation && operation == runtime_generation =>
            {
                id
            }
            _ => {
                let Some(id) =
                    stream.update(cx, |stream, cx| stream.reserve_context_operation_id(cx))
                else {
                    return;
                };
                self.context_controller.automatic = Some((run_generation, runtime_generation, id));
                id
            }
        };
        record.generation = id;
        stream.update(cx, |stream, cx| {
            stream.apply_context_status(&owner.thread_id, &owner.model, record, cx);
        });
    }

    pub(crate) fn owns_primary_context_event(
        &self,
        generation: u64,
        stream: &Entity<ConversationStream>,
        cx: &App,
    ) -> bool {
        self.context_controller
            .primary_owner
            .as_ref()
            .is_some_and(|(run, owner)| {
                *run == generation
                    && owner.stream == *stream
                    && self.context_owner_matches(owner, cx)
                    && self
                        .agent_controller
                        .matches(generation, &owner.thread_id, stream)
            })
    }

    pub(crate) fn cancel_context_operation(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ContextCompactionCancelRequested,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.context_controller.owner.as_ref().filter(|owner| {
            owner.stream == stream
                && owner.thread_id == request.thread_id
                && owner.model == request.model
                && self.context_owner_matches(owner, cx)
        }) else {
            return;
        };
        if let Some(active) = &self.context_controller.manual
            && active.owner == *owner
            && active.request_id == request.request_id
        {
            active.cancel.cancel();
        } else if let Some((run, _, id)) = self.context_controller.automatic
            && id == request.request_id
            && self
                .agent_controller
                .matches(run, &request.thread_id, &stream)
        {
            self.cancel_active_agent(cx);
        }
    }
}

/// Blocking worker boundary: config, credentials, store and provider are never
/// resolved in GPUI render/update. No payload or credential crosses back to UI.
#[allow(clippy::too_many_arguments)]
fn run_manual_context_worker(
    database: PathBuf,
    config_path: Option<PathBuf>,
    thread_id: String,
    model: String,
    generation: u64,
    cancel: CancellationToken,
    reasoning: Option<vega_runtime::FrozenReasoning>,
    pricing: Option<vega_conversation::PricingCatalog>,
    #[cfg(test)] provider_override: Option<Arc<dyn vega_runtime::Provider>>,
) -> Result<Vec<ConversationEvent>, ()> {
    let config_path = config_path.ok_or(())?;
    let config = vega_store::config::read_from(&config_path).map_err(|_| ())?;
    let configured = unique_provider_for_model(&config, &model).ok_or(())?;
    let reasoning = match reasoning {
        Some(reasoning) => {
            reasoning.validate().map_err(|_| ())?;
            if reasoning.provider != configured.name || reasoning.model != model {
                return Err(());
            }
            reasoning
        }
        None => reasoning_for_provider_model(
            &configured.name,
            &model,
            Some(&config_path.with_file_name(vega_store::reasoning::REASONING_FILE_NAME)),
        )?,
    };
    if cancel.is_cancelled() {
        return Err(());
    }
    let make_provider = || -> Result<Arc<dyn vega_runtime::Provider>, ()> {
        #[cfg(test)]
        if let Some(provider) = provider_override {
            return Ok(provider);
        }
        let key =
            vega_store::keystore::get_key(config_path.parent().ok_or(())?, &configured.key_ref)
                .map_err(|_| ())?;
        let provider =
            vega_runtime::OpenAiProvider::new(configured.base_url, key).map_err(|_| ())?;
        Ok(Arc::new(provider))
    };
    let provider = make_provider()?;
    let store = Store::open(database).map_err(|_| ())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ())?;
    runtime
        .block_on(vega_conversation::agent::compact_thread_manually_accounted(
            &store,
            provider.as_ref(),
            &thread_id,
            &model,
            SYSTEM_PROMPT,
            cancel,
            Some(reasoning),
            pricing,
            generation,
        ))
        .map_err(|_| ())
}
