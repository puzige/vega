use super::*;
use crate::automation_controller::{AutomationClockSnapshot, AutomationGlobal, ensure_global};

impl VegaWindow {
    pub(super) fn observe_automation_clock(
        &self,
        thread: &str,
        generation: u64,
        started_at: std::time::Instant,
        cx: &mut Context<Self>,
    ) {
        ensure_global(cx);
        cx.update_global::<AutomationGlobal, _>(|global, _| {
            if let Some(run) = global.runs.get_mut(thread)
                && run.generation == generation
                && run.owner.ptr_eq(&Arc::downgrade(&self.window_alive))
                && run.started_at.is_none()
            {
                run.started_at = Some(started_at);
            }
        });
    }

    fn automation_database(&self, cx: &App) -> Option<PathBuf> {
        cx.global::<VegaStore>()
            .0
            .as_ref()
            .ok()?
            .database_path()
            .map(std::path::Path::to_path_buf)
    }
    pub(super) fn automation_error(
        &self,
        stream: &Entity<ConversationStream>,
        error: String,
        cx: &mut Context<Self>,
    ) {
        let mut projection = stream.read(cx).automation_projection().clone();
        projection.busy = false;
        projection.error = Some(error);
        stream.update(cx, |stream, cx| {
            stream.reject_composer_submission(cx);
            crate::app_localization::apply_automation(stream, projection, cx);
        });
    }
    pub(crate) fn intercept_automation(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerSubmitted,
        cx: &mut Context<Self>,
    ) -> bool {
        let command = match vega_conversation::automation::parse_command(&request.content) {
            Ok(None) => return false,
            Ok(Some(command)) => command,
            Err(error) => {
                self.automation_error(&stream, error, cx);
                return true;
            }
        };
        if stream.read(cx).task_backend() != TaskBackend::Native {
            self.automation_error(
                &stream,
                vega_i18n::t("app.automation.native_commands").into(),
                cx,
            );
        } else if !request.images.is_empty() || request.skill_intent.is_some() {
            self.automation_error(
                &stream,
                vega_i18n::t("app.automation.no_attachments").into(),
                cx,
            );
        } else {
            self.change_automation(
                stream,
                &request.thread_id,
                command,
                Some(request.content.clone()),
                None,
                cx,
            );
        }
        true
    }
    pub(crate) fn automation_requested(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &AutomationRequested,
        cx: &mut Context<Self>,
    ) {
        if self.owns_stream_request(&stream, &request.thread_id, cx) {
            self.change_automation(
                stream,
                &request.thread_id,
                request.command.clone(),
                None,
                None,
                cx,
            );
        }
    }
    pub(super) fn change_automation(
        &mut self,
        stream: Entity<ConversationStream>,
        thread: &str,
        command: AutomationCommand,
        original: Option<String>,
        editor: Option<(
            Entity<vega_ui::automation_editor::AutomationEditor>,
            vega_ui::automation_editor::AutomationEditSubmission,
        )>,
        cx: &mut Context<Self>,
    ) {
        ensure_global(cx);
        if stream.read(cx).permission_change_pending() {
            self.automation_edit_error(
                &stream,
                editor.as_ref(),
                vega_i18n::t("app.automation.saving").into(),
                cx,
            );
            return;
        }
        if let AutomationCommand::Control(kind, AutomationControl::Resume) = &command
            && stream.read(cx).thread_snapshot().permission_mode == PermissionMode::FullAccess
            && stream.read(cx).task_backend() == TaskBackend::Native
        {
            let task = stream
                .read(cx)
                .automation_projection()
                .tasks
                .iter()
                .find(|task| task.kind == *kind)
                .cloned();
            if let Some(task) = task {
                stream.update(cx, |stream, cx| {
                    stream.reject_composer_submission(cx);
                    stream.request_automation_permission(task, true, original, cx);
                });
            }
            return;
        }
        if stream.read(cx).task_backend() != TaskBackend::Native {
            self.automation_edit_error(
                &stream,
                editor.as_ref(),
                vega_i18n::t("app.automation.native_only").into(),
                cx,
            );
            return;
        }
        if let AutomationCommand::Status(kind) = command {
            if stream
                .read(cx)
                .automation_projection()
                .tasks
                .iter()
                .any(|task| task.kind == kind)
            {
                stream.update(cx, |stream, cx| stream.reject_composer_submission(cx));
                self.bind_automation(stream, thread.to_string(), cx);
            } else {
                self.automation_error(
                    &stream,
                    if kind == AutomationKind::Goal {
                        vega_i18n::t("app.automation.goal_usage")
                    } else {
                        vega_i18n::t("app.automation.loop_usage")
                    }
                    .into(),
                    cx,
                );
            }
            return;
        }
        if cx
            .global::<AutomationGlobal>()
            .mutations
            .contains_key(thread)
        {
            self.automation_edit_error(
                &stream,
                editor.as_ref(),
                vega_i18n::t("app.automation.saving").into(),
                cx,
            );
            return;
        }
        let Some(database) = self.automation_database(cx) else {
            self.automation_edit_error(
                &stream,
                editor.as_ref(),
                vega_i18n::t("app.automation.store_reopen").into(),
                cx,
            );
            return;
        };
        let draft = self.draft_for_route(thread);
        if draft.is_some() && !matches!(command, AutomationCommand::Create { .. }) {
            self.automation_edit_error(
                &stream,
                editor.as_ref(),
                vega_i18n::t("app.automation.not_set").into(),
                cx,
            );
            return;
        }
        let creation_lease = if draft.is_some() {
            if !self.owns_stream_request(&stream, thread, cx)
                || stream.read(cx).has_pending_model_selection()
                || stream.read(cx).run_active()
                || self.automation_controller.draft_creation.is_some()
            {
                self.automation_edit_error(
                    &stream,
                    editor.as_ref(),
                    vega_i18n::t("app.automation.saving").into(),
                    cx,
                );
                return;
            }
            let Some(lease) =
                self.trusted_actions
                    .acquire(TrustedActionKind::DraftAutomationCreation, 0, 0)
            else {
                self.automation_edit_error(
                    &stream,
                    editor.as_ref(),
                    vega_i18n::t("app.automation.saving").into(),
                    cx,
                );
                return;
            };
            self.automation_controller.draft_creation = Some((thread.into(), lease));
            Some(lease)
        } else {
            None
        };
        let creation_source = draft.clone();
        let edit_fence = match &command {
            AutomationCommand::Edit {
                kind,
                identity,
                revision,
                ..
            } => Some((*kind, identity.clone(), *revision)),
            _ => None,
        };
        let cancelled_kind = match command {
            AutomationCommand::Control(
                kind,
                AutomationControl::Pause | AutomationControl::Clear,
            ) => Some(kind),
            _ => None,
        };
        if let Some(kind) = cancelled_kind
            && let Some(run) = cx.global::<AutomationGlobal>().runs.get(thread)
            && run.claim.as_ref().and_then(|claim| claim.source) == Some(kind)
        {
            run.cancel.cancel();
        }
        let epoch = cx.update_global::<AutomationGlobal, _>(|global, _| {
            if let Some(kind) = cancelled_kind {
                global.revoked.insert((thread.into(), kind));
            }
            let epoch = global.invalidate(thread);
            global.mutations.insert(thread.into(), epoch);
            epoch
        });
        let counter = cx.update_global::<AutomationGlobal, _>(|global, _| global.counter(thread));
        let lane = cx.global::<AutomationGlobal>().mutation_lane.clone();
        let pending_pauses: Vec<_> = cx
            .global::<AutomationGlobal>()
            .revoked
            .iter()
            .filter(|(id, _)| id == thread)
            .map(|(_, kind)| *kind)
            .collect();
        let enabled_kind = match &command {
            AutomationCommand::Create { kind, .. }
            | AutomationCommand::Control(kind, AutomationControl::Resume) => Some(*kind),
            _ => None,
        };
        let mut projection = stream.read(cx).automation_projection().clone();
        projection.busy = true;
        projection.error = None;
        stream.update(cx, |stream, cx| {
            if creation_lease.is_some() {
                stream.set_trusted_action_busy(true, cx);
            }
            crate::app_localization::apply_automation(stream, projection, cx)
        });
        self.automation_controller
            .streams
            .insert(thread.into(), stream.clone());
        self.start_automation_timer(cx);
        let thread = thread.to_string();
        let worker_thread = thread.clone();
        let config_path = self.composer_config_path();
        let model = stream.read(cx).displayed_model().to_owned();
        let mcp_settings = self.mcp_settings.clone();
        let worker = cx.background_executor().spawn(async move {
            let _lane = lane
                .lock()
                .map_err(|_| vega_i18n::t("app.automation.save_failed").to_string())?;
            if counter.load(Ordering::SeqCst) != epoch {
                return Err(vega_i18n::t("app.automation.cancelled").into());
            }
            let store = vega_store::Store::open(database)
                .map_err(|_| vega_i18n::t("app.automation.store_unavailable").to_string())?;
            let pending: Vec<_> = vega_conversation::automation::load(&store, &worker_thread)?
                .into_iter()
                .filter(|task| pending_pauses.contains(&task.kind))
                .collect();
            if !matches!(command, AutomationCommand::Edit { .. }) {
                vega_conversation::automation::pause_matching(
                    &store,
                    &worker_thread,
                    &pending,
                    "user_paused",
                )?;
            }
            if let AutomationCommand::Create { instruction, .. }
            | AutomationCommand::Edit { instruction, .. } = &command
            {
                let mut credentials = mcp_settings
                    .as_ref()
                    .map(|service| service.current_owner_credential_values())
                    .transpose()
                    .map_err(|_| vega_i18n::t("app.automation.credential_check").to_string())?
                    .unwrap_or_default();
                let path = config_path
                    .as_deref()
                    .ok_or(vega_i18n::t("app.error.provider_required"))?;
                let config = vega_store::config::read_from(path)
                    .map_err(|_| vega_i18n::t("app.error.config_read"))?;
                let provider = unique_provider_for_model(&config, &model)
                    .ok_or(vega_i18n::t("app.error.provider_model"))?;
                let root = path.parent().ok_or(vega_i18n::t("app.error.config_read"))?;
                credentials.push(
                    vega_store::keystore::get_key(root, &provider.key_ref)
                        .map_err(|_| vega_i18n::t("app.error.credentials_read"))?,
                );
                if vega_runtime::contains_sensitive_credential(instruction, &credentials) {
                    return Err(vega_i18n::t("app.automation.credentials_in_task").into());
                }
            }
            let now = vega_conversation::automation::now_ms();
            let result = if let Some(draft) = &draft {
                let (tasks, thread) =
                    vega_conversation::automation::command_for_draft(&store, draft, &command, now)?;
                (tasks, Some(thread))
            } else {
                (
                    vega_conversation::automation::command(&store, &worker_thread, &command, now)?,
                    None,
                )
            };
            Ok::<_, String>(result)
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            cx.update_global::<AutomationGlobal, _>(|global, _| {
                if global.mutations.get(&thread) == Some(&epoch) {
                    global.mutations.remove(&thread);
                }
            });
            let _ = this.update(cx, |this, cx| {
                if let Some(lease) = creation_lease {
                    if !this
                        .automation_controller
                        .draft_creation
                        .as_ref()
                        .is_some_and(|(id, owner)| id == &thread && *owner == lease)
                        || !this.trusted_actions.release(lease)
                    {
                        return;
                    }
                    this.automation_controller.draft_creation = None;
                    stream.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                    if let (Some(source), Ok((_, Some(materialized)))) = (&creation_source, &result)
                        && materialized.id == source.id
                        && materialized.project_id == source.project_id
                    {
                        let current_route = this.owns_stream_request(&stream, &thread, cx)
                            && cx.global::<OpenedThread>().0.as_ref() == Some(source);
                        this.retained_drafts.remove(&source.id);
                        this.publish_retained_drafts(cx);
                        if this.draft.as_ref() == Some(source) {
                            this.draft = None;
                        }
                        if stream.read(cx).thread_snapshot() == *source {
                            stream.update(cx, |stream, cx| {
                                stream.apply_thread(materialized.clone(), cx);
                                stream.set_draft_route(false, cx);
                            });
                        }
                        if current_route {
                            cx.set_global(OpenedThread(Some(materialized.clone())));
                        }
                        this.sidebar
                            .update(cx, |sidebar, cx| sidebar.reload_sessions(cx));
                    }
                }
                if let Some((view, submitted)) = &editor {
                    view.update(cx, |view, cx| {
                        view.finish_save_localized(
                            submitted,
                            result
                                .as_ref()
                                .map(|(tasks, _)| tasks.clone())
                                .map_err(|error| crate::app_localization::localized_error(error)),
                            cx,
                        )
                    });
                }
                if let (Some((kind, identity, revision)), Ok((tasks, _))) = (&edit_fence, &result)
                    && tasks.iter().any(|task| {
                        task.kind == *kind
                            && task.identity == *identity
                            && task.revision != *revision
                            && task.status == AutomationStatus::Active
                    })
                    && let Some(run) = cx.global::<AutomationGlobal>().runs.get(&thread)
                    && run.claim.as_ref().is_some_and(|claim| {
                        claim.source == Some(*kind)
                            && claim.tasks.iter().any(|task| {
                                task.identity == *identity && task.revision == *revision
                            })
                    })
                {
                    run.cancel.cancel();
                }
                if cx.global::<AutomationGlobal>().epoch(&thread) != epoch {
                    return;
                }
                match result {
                    Ok((tasks, _)) => {
                        if let Some(kind) = enabled_kind {
                            cx.update_global::<AutomationGlobal, _>(|global, _| {
                                global.revoked.remove(&(thread.clone(), kind));
                            });
                        }
                        stream.update(cx, |stream, cx| {
                            crate::app_localization::apply_automation(
                                stream,
                                AutomationProjection {
                                    tasks,
                                    busy: false,
                                    error: None,
                                },
                                cx,
                            );
                            if let Some(original) = original {
                                stream.accept_automation_command(&original, cx);
                            }
                        });
                        this.bind_automation(stream.clone(), thread.clone(), cx);
                        this.sidebar
                            .update(cx, |sidebar, cx| sidebar.reload_sessions(cx));
                        cx.notify();
                    }
                    Err(error) => {
                        if editor.is_some() {
                            let mut projection = stream.read(cx).automation_projection().clone();
                            projection.busy = false;
                            stream.update(cx, |stream, cx| {
                                crate::app_localization::apply_automation(stream, projection, cx)
                            });
                        } else {
                            this.automation_error(&stream, error, cx);
                        }
                    }
                }
            });
        })
        .detach();
    }
    pub(crate) fn pause_automation_all(
        &mut self,
        thread: &str,
        reason: &str,
        cx: &mut Context<Self>,
    ) {
        ensure_global(cx);
        let epoch = cx.update_global::<AutomationGlobal, _>(|global, _| {
            let epoch = global.invalidate(thread);
            global.mutations.insert(thread.into(), epoch);
            global.revoked.extend([
                (thread.into(), AutomationKind::Goal),
                (thread.into(), AutomationKind::Loop),
            ]);
            if let Some(run) = global.runs.get(thread) {
                run.cancel.cancel();
            }
            epoch
        });
        let counter = cx.update_global::<AutomationGlobal, _>(|global, _| global.counter(thread));
        let lane = cx.global::<AutomationGlobal>().mutation_lane.clone();
        let Some(database) = self.automation_database(cx) else {
            cx.update_global::<AutomationGlobal, _>(|global, _| {
                if global.mutations.get(thread) == Some(&epoch) {
                    global.mutations.remove(thread);
                }
            });
            if let Some(stream) = self.automation_controller.streams.get(thread) {
                self.automation_error(
                    stream,
                    vega_i18n::t("app.automation.store_reopen").into(),
                    cx,
                );
            }
            return;
        };
        let thread = thread.to_string();
        let reason = reason.to_string();
        let worker_thread = thread.clone();
        let worker = cx.background_executor().spawn(async move {
            let _lane = lane
                .lock()
                .map_err(|_| vega_i18n::t("app.automation.pause_failed").to_string())?;
            if counter.load(Ordering::SeqCst) != epoch {
                return Err(vega_i18n::t("app.automation.cancelled").into());
            }
            let store = vega_store::Store::open(database)
                .map_err(|_| vega_i18n::t("app.automation.store_unavailable").to_string())?;
            vega_conversation::automation::pause_all(&store, &worker_thread, &reason)
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            cx.update_global::<AutomationGlobal, _>(|global, _| {
                if global.mutations.get(&thread) == Some(&epoch) {
                    global.mutations.remove(&thread);
                }
            });
            let _ = this.update(cx, |this, cx| {
                if cx.global::<AutomationGlobal>().epoch(&thread) != epoch {
                    return;
                }
                if let Some(stream) = this.automation_controller.streams.get(&thread).cloned() {
                    match result {
                        Ok(tasks) => stream.update(cx, |stream, cx| {
                            crate::app_localization::apply_automation(
                                stream,
                                AutomationProjection {
                                    tasks,
                                    busy: false,
                                    error: None,
                                },
                                cx,
                            )
                        }),
                        Err(error) => this.automation_error(&stream, error, cx),
                    }
                }
            });
        })
        .detach();
    }
    pub(crate) fn bind_automation(
        &mut self,
        stream: Entity<ConversationStream>,
        thread: String,
        cx: &mut Context<Self>,
    ) {
        if stream.read(cx).task_backend() != TaskBackend::Native {
            return;
        }
        ensure_global(cx);
        cx.update_global::<AutomationGlobal, _>(|global, _| {
            global.register_stream(&thread, &self.window_alive, &stream);
        });
        self.automation_controller
            .streams
            .insert(thread.clone(), stream);
        self.start_automation_timer(cx);
        self.poll_automation_thread(&thread, cx);
    }
    fn start_automation_timer(&mut self, cx: &mut Context<Self>) {
        if self.automation_controller.timer_started {
            return;
        }
        self.automation_controller.timer_started = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        let threads: Vec<_> =
                            this.automation_controller.streams.keys().cloned().collect();
                        for thread in threads {
                            this.poll_automation_thread(&thread, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
    fn automation_ready(
        &self,
        thread: &str,
        stream: &Entity<ConversationStream>,
        cx: &App,
    ) -> bool {
        self.window_alive.load(Ordering::SeqCst)
            && cx.global::<AutomationGlobal>().ready
            && cx.global::<AutomationGlobal>().streams_ready(thread, cx)
            && !crate::updater::installing()
            && !cx.global::<SettingsOpen>().0
            && !self.trusted_actions.is_busy()
            && self.reasoning_save_pending.is_none()
            && !self.model_catalog_loading
            && !self.agent_controller.active.contains_key(thread)
            && !thread_is_running(thread, cx)
            && !self.agent_controller.queue_operations.contains_key(thread)
            && !self.agent_controller.queue_dispatches.contains_key(thread)
            && !self.agent_controller.pending_review.contains_key(thread)
            && !cx
                .global::<AutomationGlobal>()
                .mutations
                .contains_key(thread)
            && stream.read(cx).automation_can_run(cx)
    }
    fn poll_automation_thread(&mut self, thread: &str, cx: &mut Context<Self>) {
        if self.automation_controller.pending.contains(thread) {
            return;
        }
        let Some(stream) = self.automation_controller.streams.get(thread).cloned() else {
            return;
        };
        let Some(database) = self.automation_database(cx) else {
            return;
        };
        let owns = cx.update_global::<AutomationGlobal, _>(|global, _| {
            global.acquire(thread, &self.window_alive)
        });
        let eligible = owns && self.automation_ready(thread, &stream, cx);
        let epoch = cx.global::<AutomationGlobal>().epoch(thread);
        let counter = cx.update_global::<AutomationGlobal, _>(|global, _| global.counter(thread));
        let lane = cx.global::<AutomationGlobal>().mutation_lane.clone();
        let observed_run = cx
            .global::<AutomationGlobal>()
            .runs
            .get(thread)
            .and_then(|run| {
                run.claim
                    .clone()
                    .filter(|claim| claim.source.is_some())
                    .map(|claim| (run.generation, claim))
            });
        let clock = cx
            .global::<AutomationGlobal>()
            .runs
            .get(thread)
            .and_then(|run| {
                run.started_at.map(|started| AutomationClockSnapshot {
                    generation: run.generation,
                    started,
                    claim_id: run.claim.as_ref().map(|claim| claim.id.clone()),
                })
            });
        let revoked: Vec<_> = cx
            .global::<AutomationGlobal>()
            .revoked
            .iter()
            .filter(|(id, _)| id == thread)
            .map(|(_, kind)| *kind)
            .collect();
        self.automation_controller.pending.insert(thread.into());
        let thread = thread.to_string();
        let worker_thread = thread.clone();
        let worker = cx.background_executor().spawn(async move {
            let _lane = lane
                .lock()
                .map_err(|_| vega_i18n::t("app.automation.read_failed").to_string())?;
            let store = vega_store::Store::open(&database)
                .map_err(|_| vega_i18n::t("app.automation.read_reopen").to_string())?;
            let now = vega_conversation::automation::now_ms();
            let tasks = vega_conversation::automation::refresh(&store, &worker_thread, now)?;
            let eligible = eligible && counter.load(Ordering::SeqCst) == epoch;
            if eligible && !revoked.is_empty() {
                let pending: Vec<_> = tasks
                    .iter()
                    .filter(|task| revoked.contains(&task.kind))
                    .cloned()
                    .collect();
                vega_conversation::automation::pause_matching(
                    &store,
                    &worker_thread,
                    &pending,
                    "user_paused",
                )?;
            }
            let sampled_at = std::time::Instant::now();
            let (tasks, active_claim) =
                vega_conversation::automation::timing_snapshot(&store, &worker_thread)?;
            let invalid_run = observed_run.and_then(|(generation, claim)| {
                let finished =
                    vega_store::automation::run(store.conn(), &claim.thread_id, &claim.id)
                        .ok()
                        .flatten()
                        .is_some_and(|(finished, _)| finished);
                (!finished
                    && !vega_conversation::automation::current_claim(&store, &claim)
                        .unwrap_or(false))
                .then_some((generation, claim.id))
            });
            let eligible_tasks: Vec<_> = tasks
                .iter()
                .filter(|task| !revoked.contains(&task.kind))
                .cloned()
                .collect();
            let claim = if eligible
                && let Some(kind) = vega_conversation::automation::due_kind(&eligible_tasks, now)
            {
                vega_conversation::automation::claim(&store, &worker_thread, Some(kind), now)?
            } else {
                None
            };
            Ok::<_, String>((
                tasks,
                claim,
                database,
                invalid_run,
                active_claim,
                sampled_at,
            ))
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let cleanup = result
                .as_ref()
                .ok()
                .and_then(|(_, claim, database, _, _, _)| {
                    claim.clone().map(|claim| (database.clone(), claim))
                });
            let mut accepted = false;
            let mut failed_start = false;
            let _ = this.update(cx, |this, cx| {
                this.automation_controller.pending.remove(&thread);
                match result {
                    Err(error) => {
                        this.automation_error(&stream, error, cx);
                        this.automation_controller.streams.remove(&thread);
                        cx.update_global::<AutomationGlobal, _>(|global, _| {
                            global.release(&thread, &this.window_alive)
                        });
                    }
                    Ok((tasks, claim, _, invalid_run, active_claim, sampled_at)) => {
                        if let Some((generation, id)) = invalid_run
                            && let Some(run) = cx.global::<AutomationGlobal>().runs.get(&thread)
                            && run.generation == generation
                            && run.claim.as_ref().is_some_and(|claim| claim.id == id)
                        {
                            run.cancel.cancel();
                        }
                        let current = cx.global::<AutomationGlobal>().epoch(&thread) == epoch;
                        let timings = cx.global::<AutomationGlobal>().timing_projection(
                            &thread,
                            &tasks,
                            active_claim.as_ref(),
                            clock.as_ref(),
                            sampled_at,
                        );
                        if current
                            && !cx
                                .global::<AutomationGlobal>()
                                .mutations
                                .contains_key(&thread)
                            && let Some(timings) = timings
                        {
                            let error = if !cx.global::<AutomationGlobal>().ready {
                                Some(vega_i18n::t("app.automation.recovery_failed").into())
                            } else {
                                stream.read(cx).automation_projection().error.clone()
                            };
                            this.refresh_automation_editors(&thread, &tasks, &timings, cx);
                            stream.update(cx, |stream, cx| {
                                crate::app_localization::apply_automation(
                                    stream,
                                    AutomationProjection {
                                        tasks: tasks.clone(),
                                        busy: false,
                                        error,
                                    },
                                    cx,
                                );
                                stream.apply_automation_timings(timings, cx);
                            });
                        }
                        if let Some(claim) = claim
                            && current
                            && this.automation_ready(&thread, &stream, cx)
                        {
                            this.automation_controller
                                .dispatch
                                .insert(thread.clone(), claim.clone());
                            this.start_agent_run(
                                stream.clone(),
                                &thread,
                                PendingAgentRun::Automation(Box::new(claim.clone())),
                                cx,
                            );
                            this.automation_controller.dispatch.remove(&thread);
                            if this.agent_controller.active.contains_key(&thread) {
                                accepted = true;
                                return;
                            }
                            failed_start = true;
                            this.automation_error(
                                &stream,
                                vega_i18n::t("app.automation.start_failed").into(),
                                cx,
                            );
                        }
                        if tasks.is_empty() {
                            this.automation_controller.streams.remove(&thread);
                            cx.update_global::<AutomationGlobal, _>(|global, _| {
                                global.release(&thread, &this.window_alive)
                            });
                        }
                    }
                }
            });
            if !accepted && let Some((database, claim)) = cleanup {
                cx.background_executor()
                    .spawn(async move {
                        if let Ok(store) = vega_store::Store::open(database) {
                            if failed_start {
                                let _ = vega_conversation::automation::finish(
                                    &store,
                                    &claim,
                                    false,
                                    true,
                                    0,
                                    vega_conversation::automation::now_ms(),
                                );
                            } else {
                                let _ =
                                    vega_conversation::automation::release_claim(&store, &claim);
                            }
                        }
                    })
                    .detach();
            }
        })
        .detach();
    }
}
