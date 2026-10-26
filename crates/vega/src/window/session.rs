use super::*;

pub(crate) const MODEL_SELECTION_POLL: std::time::Duration = std::time::Duration::from_millis(4);

type ReasoningAuthority = (
    vega_store::reasoning::ReasoningConfig,
    vega_store::reasoning::ReasoningFileSnapshot,
);
type ModelCatalogLoadResult = Result<
    (
        Vec<ModelOption>,
        Vec<ReasoningProfileProjection>,
        Option<ReasoningSettingsErrorCode>,
        Option<ReasoningAuthority>,
        NewTaskDefaults,
        Option<vega_store::config::AppConfig>,
    ),
    (),
>;

/// Releases exactly the owner token on every exit path and projects the
/// authoritative thread on success; failures keep the old authoritative
/// value with a bounded error, never a silent success. A callback must first
/// win the exact lease and still own the originating stream request; a late
/// callback cannot clear or publish a newer owner.
fn finish_thread_model_selection(
    window: &mut VegaWindow,
    stream: &Entity<ConversationStream>,
    thread_id: &str,
    lease: TrustedActionToken,
    request_id: u64,
    outcome: Result<Thread, ()>,
    cx: &mut App,
) {
    // The token is the app-level single-flight identity. If it was already
    // released (duplicate callback, close cleanup, or a newer owner), this
    // callback is fully stale and must not touch any UI/global projection.
    let staged = window
        .staged_model_pending
        .as_ref()
        .is_some_and(|(id, owner)| id == thread_id && *owner == lease);
    if staged {
        window.staged_model_pending = None;
    }
    if !window.trusted_actions.release(lease) {
        return;
    }
    // A released token is still paired with its originating stream request.
    // Validate that pair before publishing an authoritative row. A malformed
    // callback clears only the owner encoded in its released token; it cannot
    // clear a newer pending request on the same entity.
    if !stream.read(cx).owns_model_selection_request(request_id) {
        stream.update(cx, |stream, cx| {
            stream.clear_model_selection_owner(lease.request_sequence, cx)
        });
        return;
    }

    if staged {
        match outcome {
            Ok(thread)
                if stream
                    .read(cx)
                    .model_selection_ack_is_valid(request_id, &thread) =>
            {
                stream.update(cx, |stream, cx| {
                    stream.apply_thread_model_staged(thread_id, request_id, &thread.model, cx);
                });
            }
            _ => stream.update(cx, |stream, cx| {
                stream.apply_thread_model_failed(thread_id, request_id, cx);
            }),
        }
        return;
    }

    let replay_model_changed = outcome
        .as_ref()
        .is_ok_and(|thread| stream.read(cx).displayed_model() != thread.model);
    if replay_model_changed
        && let Some(sessions) = cx.try_global::<crate::app_agent::AppReplaySessions>()
    {
        sessions.0.invalidate_thread(thread_id);
    }
    let settings_open = cx.global::<SettingsOpen>().0;
    let current_route = !settings_open && window.owns_stream_request(stream, thread_id, cx);
    if current_route {
        match outcome {
            Ok(thread) => {
                // Check the exact request/model/thread identity before the
                // global update. `apply_*` repeats its own fence, but this
                // guard keeps an invalid result from leaking through the
                // app-level projection first.
                if !stream
                    .read(cx)
                    .model_selection_ack_is_valid(request_id, &thread)
                {
                    stream.update(cx, |stream, cx| {
                        stream.apply_thread_model_failed(thread_id, request_id, cx);
                    });
                    return;
                }
                let refresh = ModelSelectionRefresh::from_thread(&thread);
                let Some(current) = cx
                    .global::<OpenedThread>()
                    .0
                    .clone()
                    .and_then(|current| refresh.merge_into(&current))
                else {
                    stream.update(cx, |stream, cx| {
                        stream.apply_thread_model_failed(thread_id, request_id, cx);
                    });
                    return;
                };
                cx.set_global(OpenedThread(Some(current.clone())));

                window.mirror_draft(&current);
                stream.update(cx, |stream, cx| {
                    stream.apply_thread_model_acknowledged(
                        thread_id,
                        request_id,
                        &refresh.model,
                        current,
                        cx,
                    )
                });
                super::reasoning::apply_reasoning_profile_to_stream_with_app(
                    window,
                    stream,
                    &refresh.model,
                    cx,
                );
            }
            Err(()) => stream.update(cx, |stream, cx| {
                stream.apply_thread_model_failed(thread_id, request_id, cx)
            }),
        }
        return;
    }

    // Always clear the originating entity's exact owner after a route switch
    // or while Settings hides the session. This cleanup never projects the
    // stale entity as current.
    stream.update(cx, |stream, cx| {
        stream.clear_model_selection_owner(request_id, cx)
    });

    let Ok(authoritative) = outcome else {
        return;
    };
    if settings_open {
        // Settings owns the visible route. Defer only the model authority
        // until it closes; sidebar edits remain owned by the live projection.
        window.deferred_model_refresh = Some(ModelSelectionRefresh::from_thread(&authoritative));
        return;
    }

    // A→B→A: if the same thread is current again, reconcile the current
    // entity with the worker's authoritative row. The old entity above is
    // never reused for this projection. A different current route simply
    // drops the result after exact-owner cleanup.
    let current_thread_matches = cx
        .global::<OpenedThread>()
        .0
        .as_ref()
        .is_some_and(|current| current.id == thread_id);
    if current_thread_matches {
        let refresh = ModelSelectionRefresh::from_thread(&authoritative);
        let Some(current) = cx
            .global::<OpenedThread>()
            .0
            .clone()
            .and_then(|current| refresh.merge_into(&current))
        else {
            return;
        };
        let current_stream = window.current_cached_stream_for_thread(thread_id, cx);
        cx.set_global(OpenedThread(Some(current.clone())));
        if let Some(current_stream) = current_stream {
            current_stream.update(cx, |stream, cx| {
                stream.apply_authoritative_model(current, &refresh.model, cx)
            });
        }
    }
}

impl VegaWindow {
    pub(super) fn promote_model_before_run(
        &mut self,
        stream: &Entity<ConversationStream>,
        thread_id: &str,
        admission: &ThreadAdmission,
        cx: &mut Context<Self>,
    ) -> Result<bool, ()> {
        if self.is_draft_route(thread_id) {
            return Ok(false);
        }
        if self
            .staged_model_pending
            .as_ref()
            .is_some_and(|(id, _)| id == thread_id)
        {
            return Err(());
        }
        let store = cx.global::<VegaStore>().0.as_ref().map_err(|_| ())?;
        let pending =
            vega_store::threads::pending_model(store.conn(), thread_id).map_err(|_| ())?;
        let Some(model) = pending else {
            return Ok(false);
        };
        let path = self.composer_config_path().ok_or(())?;
        let config = vega_store::config::read_from(&path).map_err(|_| ())?;
        if unique_provider_for_model(&config, &model).is_none() {
            return Err(());
        }
        let thread =
            vega_conversation::threads::promote_thread_model(store, thread_id, &model, admission)
                .map_err(|_| ())?;
        if let Some(sessions) = cx.try_global::<crate::app_agent::AppReplaySessions>() {
            sessions.0.invalidate_thread(thread_id);
        }
        let refresh = ModelSelectionRefresh::from_thread(&thread);
        if let Some(current) = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .and_then(|current| refresh.merge_into(current))
        {
            cx.set_global(OpenedThread(Some(current)));
        }
        stream.update(cx, |stream, cx| {
            stream.apply_authoritative_model(thread, &model, cx);
        });
        self.apply_reasoning_profile_to_stream(stream, &model, cx);
        Ok(true)
    }

    pub(super) fn settle_next_turn_model(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: String,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
                let finished = this
                    .update(cx, |this, cx| {
                        if this.agent_controller.active.contains_key(&thread_id) {
                            return true;
                        }
                        if this
                            .staged_model_pending
                            .as_ref()
                            .is_some_and(|(id, _)| id == &thread_id)
                        {
                            return false;
                        }
                        let owner = cx.global::<VegaStore>().0.as_ref().ok().and_then(|store| {
                            vega_store::thread_admission::try_acquire(
                                store.database_identity(),
                                &thread_id,
                            )
                            .ok()
                            .flatten()
                        });
                        let Some(owner) = owner else {
                            return false;
                        };
                        let current = this
                            .current_cached_stream_for_thread(&thread_id, cx)
                            .unwrap_or_else(|| stream.clone());
                        let result =
                            this.promote_model_before_run(&current, &thread_id, &owner, cx);
                        drop(owner);
                        if result.is_err() {
                            current.update(cx, ConversationStream::apply_controller_error);
                        } else {
                            this.continue_pending_queue(&thread_id, &current, cx);
                        }
                        true
                    })
                    .unwrap_or(true);
                if finished || std::time::Instant::now() >= deadline {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn finish_thread_model_selection_from_async(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: String,
        lease: TrustedActionToken,
        request_id: u64,
        outcome: Result<Thread, ()>,
        cx: &mut Context<Self>,
    ) {
        let staged = self
            .staged_model_pending
            .as_ref()
            .is_some_and(|(id, owner)| id == &thread_id && *owner == lease);
        finish_thread_model_selection(self, &stream, &thread_id, lease, request_id, outcome, cx);
        if staged && !self.agent_controller.active.contains_key(&thread_id) {
            self.settle_next_turn_model(stream, thread_id, cx);
        }
    }

    pub(crate) fn mirror_draft(&mut self, thread: &Thread) {
        if self
            .draft
            .as_ref()
            .is_some_and(|draft| draft.id == thread.id)
        {
            self.draft = Some(thread.clone());
        }
    }

    /// Whether the viewport is narrower than the auto-collapse threshold
    /// (ui-spec §1). Reads the live viewport size: every platform resize is
    /// delivered as an event (`Window::bounds_changed` → redraw), so each
    /// render sees the current size and no polling is involved.
    pub(crate) fn auto_collapsed(&self, window: &Window, cx: &App) -> bool {
        window.viewport_size().width < px(AUTO_COLLAPSE_WIDTH)
            && !cx
                .try_global::<vega_ui::sidebar::SidebarExplicitlyShown>()
                .is_some_and(|v| v.0)
    }

    pub(crate) fn open_new_thread(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.update(cx, Sidebar::create_thread);
    }

    pub(crate) fn select_draft_task_backend(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &TaskBackendSelectionRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.is_draft_route(&request.thread_id)
            || !self.owns_stream_request(&stream, &request.thread_id, cx)
        {
            return;
        }
        let updated = stream.update(cx, |stream, cx| {
            stream.select_task_backend(request.backend, cx)
        });
        let Some(updated) = updated else {
            return;
        };
        self.draft = Some(updated.clone());
        cx.set_global(OpenedThread(Some(updated)));
        cx.notify();
    }

    pub(crate) fn resolve_route_thread(&mut self, cx: &mut Context<Self>) -> Thread {
        let opened = cx
            .try_global::<OpenedThread>()
            .and_then(|global| global.0.clone());
        // A durable route is already resolved; the draft never replaces it.
        if let Some(thread) = &opened
            && !self.is_draft_route(&thread.id)
        {
            return thread.clone();
        }
        let bound = cx.global::<SelectedProject>().0.clone();
        let bound = bound.as_deref();
        let draft = match self.draft.clone() {
            Some(draft) => draft,
            None => {
                let defaults = cx
                    .try_global::<NewTaskDefaults>()
                    .cloned()
                    .unwrap_or_default();
                vega_conversation::threads::draft_thread(
                    bound,
                    &defaults.model,
                    &defaults.permission_mode,
                )
            }
        };
        let draft = if draft.project_binding() == bound {
            draft
        } else {
            Thread {
                project_id: bound.unwrap_or_default().to_string(),
                ..draft
            }
        };
        self.draft = Some(draft.clone());
        if opened.as_ref() != Some(&draft) {
            cx.set_global(OpenedThread(Some(draft.clone())));
        }
        draft
    }

    /// The draft projection for `thread_id`, when this window still holds
    /// that draft. `None` means the route is durable.
    pub(crate) fn draft_for_route(&self, thread_id: &str) -> Option<Thread> {
        self.draft
            .as_ref()
            .filter(|draft| draft.id == thread_id)
            .cloned()
    }

    /// Whether `thread_id` is the window's unpersisted draft route.
    pub(crate) fn is_draft_route(&self, thread_id: &str) -> bool {
        self.draft
            .as_ref()
            .is_some_and(|draft| draft.id == thread_id)
    }

    pub(crate) fn apply_thread_model_selection(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ThreadModelSelectionRequested,
        cx: &mut Context<Self>,
    ) {
        // The event is emitted after the stream installs its pending owner.
        // Duplicate delivery of that same event must not turn the in-flight
        // save into a failure; a different/stale id has no owner to mutate.
        if !stream
            .read(cx)
            .owns_model_selection_request(request.request_id)
        {
            return;
        }
        // Only the exact request-id owner marks a duplicate event. A generic
        // branch/commit/artifact busy state must fall through to the explicit
        // rejection below so this pending request cannot hang forever.
        if stream.read(cx).model_selection_save_busy() {
            return;
        }
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            stream.update(cx, |stream, cx| {
                stream.clear_model_selection_owner(request.request_id, cx)
            });
            return;
        }

        let running = stream.read(cx).run_active()
            || self
                .agent_controller
                .active
                .contains_key(&request.thread_id);
        if cx.global::<SettingsOpen>().0
            || (!running && stream.read(cx).model_selection_blocked(cx))
            || self.trusted_actions.is_busy()
        {
            stream.update(cx, |stream, cx| {
                stream.apply_thread_model_failed(&request.thread_id, request.request_id, cx)
            });
            return;
        }

        let Some(lease) =
            self.trusted_actions
                .acquire(TrustedActionKind::ModelSelection, 0, request.request_id)
        else {
            stream.update(cx, |stream, cx| {
                stream.apply_thread_model_failed(&request.thread_id, request.request_id, cx)
            });
            return;
        };
        let owner_installed = stream.update(cx, |stream, cx| {
            stream.install_model_selection_save_owner(request.request_id, cx)
        });
        if !owner_installed {
            // The lease belongs to this attempt, so release it explicitly;
            // no generic trusted-action lease is touched by this rejection.
            let _ = self.trusted_actions.release(lease);
            stream.update(cx, |stream, cx| {
                stream.apply_thread_model_failed(&request.thread_id, request.request_id, cx)
            });
            return;
        }

        let draft = self.draft_for_route(&request.thread_id);
        let config_path = self.composer_config_path();
        let database_path = if draft.is_some() {
            None
        } else {
            self.file_backed_store_path(cx)
        };
        let Some(config_path) = config_path else {
            self.finish_thread_model_selection_from_async(
                stream,
                request.thread_id.clone(),
                lease,
                request.request_id,
                Err(()),
                cx,
            );
            return;
        };
        if draft.is_none() && database_path.is_none() {
            self.finish_thread_model_selection_from_async(
                stream,
                request.thread_id.clone(),
                lease,
                request.request_id,
                Err(()),
                cx,
            );
            return;
        }
        let occupied = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .ok()
            .is_some_and(|store| {
                matches!(
                    vega_store::thread_admission::try_acquire(
                        store.database_identity(),
                        &request.thread_id
                    ),
                    Ok(None)
                )
            });
        let staged = draft.is_none() && (running || occupied);
        if staged {
            self.staged_model_pending = Some((request.thread_id.clone(), lease));
        }
        let worker_request = request.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("vega-thread-model".into())
            .spawn(move || {
                let outcome = (|| {
                    let config = vega_store::config::read_from(&config_path)
                        .map_err(|error| error.to_string())?;
                    if unique_provider_for_model(&config, &worker_request.model).is_none() {
                        return Err("model is not uniquely configured".to_string());
                    }

                    if let Some(draft) = draft {
                        let mut next = draft;
                        next.model = worker_request.model;
                        return Ok(next);
                    }
                    let database_path =
                        database_path.ok_or_else(|| "store unavailable".to_string())?;
                    let store = vega_store::Store::open(&database_path)
                        .map_err(|error| error.to_string())?;
                    if staged {
                        vega_conversation::threads::stage_thread_model(
                            &store,
                            &worker_request.thread_id,
                            &worker_request.model,
                        )
                    } else {
                        vega_conversation::threads::set_thread_model(
                            &store,
                            &worker_request.thread_id,
                            &worker_request.model,
                        )
                    }
                    .map_err(|error| error.to_string())
                })();
                let _ = sender.send((worker_request.request_id, outcome));
            });
        if spawned.is_err() {
            self.finish_thread_model_selection_from_async(
                stream,
                request.thread_id.clone(),
                lease,
                request.request_id,
                Err(()),
                cx,
            );
            return;
        }
        let thread_id = request.thread_id.clone();
        let fallback_request_id = request.request_id;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(MODEL_SELECTION_POLL).await;
                match receiver.try_recv() {
                    Ok((request_id, outcome)) => {
                        let _ = this.update(cx, |this, cx| {
                            this.finish_thread_model_selection_from_async(
                                stream.clone(),
                                thread_id.clone(),
                                lease,
                                request_id,
                                outcome.map_err(|_| ()),
                                cx,
                            );
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |this, cx| {
                            this.finish_thread_model_selection_from_async(
                                stream.clone(),
                                thread_id.clone(),
                                lease,
                                fallback_request_id,
                                Err(()),
                                cx,
                            );
                        });
                        break;
                    }
                }
            }
        })
        .detach();
    }

    /// The store's file path, when the store global is file-backed; used to
    /// hand the model-selection worker its own short-lived connection.
    pub(super) fn file_backed_store_path(&self, cx: &App) -> Option<std::path::PathBuf> {
        match &cx.global::<VegaStore>().0 {
            Ok(store) => store.database_path().map(|path| path.to_path_buf()),
            Err(_) => None,
        }
    }

    pub(crate) fn composer_config_path(&self) -> Option<std::path::PathBuf> {
        vega_store::paths::config_dir().map(|dir| dir.join("config.toml"))
    }

    pub(crate) fn on_settings_saved(
        &mut self,
        replay_identity_changed: bool,
        cx: &mut Context<Self>,
    ) {
        self.composer_prediction_config_generation =
            self.composer_prediction_config_generation.wrapping_add(1);
        self.cancel_composer_prediction(cx);
        if replay_identity_changed
            && let Some(sessions) = cx.try_global::<crate::app_agent::AppReplaySessions>()
        {
            sessions.0.invalidate_all();
        }
        self.load_running_message_default(cx);
        if self.reasoning_save_pending.is_some() {
            // The reasoning worker owns the independent authority until its
            // exact ack. Do not invalidate it or start a catalog worker whose
            // Ready/Loading projection could overwrite Settings Saving.
            self.model_catalog_refresh_pending = true;
            return;
        }
        self.model_catalog_refresh_pending = false;
        self.invalidate_model_catalog();
        self.start_model_catalog_load(cx);
        // Invalidate advances the generation before the worker starts. Keep
        // the live Settings projection in the same Loading state while that
        // worker owns the new generation; otherwise an edit can emit the old
        // Ready generation and remain stuck in Saving after this handler
        // rejects it as stale.
        if cx.global::<SettingsOpen>().0
            && let Some(settings) = self.settings_view.clone()
        {
            let projection = if self.model_catalog_loading {
                ReasoningSettingsProjection::Loading
            } else {
                self.reasoning_settings_projection()
            };
            self.request_model_capability_reload(settings.clone(), cx);
            settings.update(cx, |settings, cx| {
                settings.apply_reasoning_projection(projection, cx)
            });
        }
    }

    /// Applies a model-save result that completed while Settings was visible
    /// only after the route is visible again. Current thread/entity identity
    /// is checked before both global and stream projection.
    pub(crate) fn apply_deferred_model_refresh(&mut self, cx: &mut Context<Self>) {
        if cx.global::<SettingsOpen>().0 {
            return;
        }
        let Some(thread) = self.deferred_model_refresh.take() else {
            return;
        };
        let current_matches = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .is_some_and(|current| {
                current.id == thread.thread_id && current.project_id == thread.project_id
            });
        if !current_matches {
            return;
        }
        let Some(current) = cx
            .global::<OpenedThread>()
            .0
            .clone()
            .and_then(|current| thread.merge_into(&current))
        else {
            return;
        };
        let current_stream = self.current_cached_stream_for_thread(&thread.thread_id, cx);
        cx.set_global(OpenedThread(Some(current.clone())));
        if let Some(current_stream) = current_stream {
            current_stream.update(cx, |stream, cx| {
                stream.apply_authoritative_model(current, &thread.model, cx)
            });
            self.apply_reasoning_profile_to_stream(&current_stream, &thread.model, cx);
        }
    }

    pub(crate) fn model_options_for_pricing(&self) -> Vec<ModelOption> {
        self.configured_models.clone().unwrap_or_default()
    }

    /// Starts one worker-side read of the configured provider/model list for
    /// the current window. A missing or malformed config produces an empty
    /// selector projection; a later explicit selection still reports the
    /// bounded save error rather than creating or rewriting config data.
    pub(crate) fn start_model_catalog_load(&mut self, cx: &mut Context<Self>) {
        if self.configured_models.is_some() || self.model_catalog_loading {
            return;
        }
        let Some(config_path) = self.composer_config_path() else {
            self.configured_models = Some(Vec::new());
            self.configured_reasoning = Some(Vec::new());
            self.configured_reasoning_error = Some(ReasoningSettingsErrorCode::Io);
            self.configured_reasoning_authority = None;
            return;
        };
        self.model_catalog_loading = true;
        let generation = self.model_catalog_generation;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-model-catalog".into())
            .spawn(move || {
                let result = vega_store::config::read_from(&config_path)
                    .map(|config| {
                        let mut models: Vec<ModelOption> = Vec::new();
                        let mut reasoning_profiles = Vec::new();
                        let mut reasoning_error = None;
                        let mut reasoning_authority = None;
                        let reasoning_path =
                            config_path.with_file_name(vega_store::reasoning::REASONING_FILE_NAME);
                        let reasoning =
                            match super::reasoning::read_reasoning_authority(&reasoning_path) {
                                Ok((reasoning, snapshot)) => {
                                    reasoning_authority = Some((reasoning.clone(), snapshot));
                                    Some(reasoning)
                                }
                                Err(error) => {
                                    reasoning_error =
                                        Some(super::reasoning::reasoning_error_code(&error));
                                    None
                                }
                            };
                        let capabilities =
                            vega_store::model_capabilities::read_from(&config_path.with_file_name(
                                vega_store::model_capabilities::MODEL_CAPABILITY_FILE_NAME,
                            ));
                        if capabilities.is_err() {
                            reasoning_error = Some(ReasoningSettingsErrorCode::Invalid);
                            reasoning_authority = None;
                        }
                        for provider in config.providers.iter().filter(|provider| provider.enabled)
                        {
                            for model in &provider.models {
                                if !models.iter().any(|option| option.model == model.as_str())
                                    && unique_provider_for_model(&config, model).is_some()
                                {
                                    models.push(ModelOption {
                                        provider: provider.name.clone(),
                                        model: model.clone(),
                                    });
                                    let profile = reasoning.as_ref().and_then(|reasoning| {
                                        reasoning
                                            .profile(&provider.name, model)
                                            .map(ReasoningProfileProjection::from_store)
                                    });
                                    let profile = match profile {
                                        Some(Ok(profile)) => Some(profile),
                                        Some(Err(_)) => {
                                            reasoning_error =
                                                Some(ReasoningSettingsErrorCode::Invalid);
                                            // A valid store document should
                                            // project losslessly. If this
                                            // conversion ever drifts from
                                            // the store grammar, do not leave
                                            // a usable-looking authority that
                                            // could silently fall back to the
                                            // provider default.
                                            reasoning_authority = None;
                                            Some(ReasoningProfileProjection::unknown(
                                                provider.name.clone(),
                                                model.clone(),
                                            ))
                                        }
                                        None => None,
                                    };
                                    let profile =
                                        crate::reasoning_capabilities::resolve_reasoning_profile(
                                            &provider.name,
                                            provider.api,
                                            &provider.base_url,
                                            model,
                                            profile,
                                            &capabilities.as_ref().cloned().unwrap_or_default(),
                                        );
                                    let profile = match profile {
                                        Ok(profile) => profile,
                                        Err(_) => {
                                            reasoning_error =
                                                Some(ReasoningSettingsErrorCode::Invalid);
                                            reasoning_authority = None;
                                            ReasoningProfileProjection::unknown(
                                                provider.name.clone(),
                                                model.clone(),
                                            )
                                        }
                                    };
                                    reasoning_profiles.push(profile);
                                }
                            }
                        }
                        (
                            models,
                            reasoning_profiles,
                            reasoning_error,
                            reasoning_authority,
                            NewTaskDefaults {
                                model: config.defaults.model.clone(),
                                permission_mode: config.defaults.permission_mode.clone(),
                            },
                            Some(config),
                        )
                    })
                    .map_err(|_| ());
                let _ = sender.send(result);
            });
        if worker.is_err() {
            self.finish_model_catalog_load(generation, Err(()), cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(MODEL_SELECTION_POLL).await;
                match receiver.try_recv() {
                    Ok(result) => {
                        let _ = this.update(cx, |this, cx| {
                            this.finish_model_catalog_load(generation, result, cx)
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |this, cx| {
                            this.finish_model_catalog_load(generation, Err(()), cx)
                        });
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn finish_model_catalog_load(
        &mut self,
        generation: u64,
        result: ModelCatalogLoadResult,
        cx: &mut Context<Self>,
    ) {
        if generation != self.model_catalog_generation {
            return;
        }
        self.model_catalog_loading = false;
        let (
            models,
            reasoning_profiles,
            reasoning_error,
            reasoning_authority,
            defaults,
            loaded_config,
        ) = match result {
            Ok(result) => result,
            Err(()) => (
                Vec::new(),
                Vec::new(),
                Some(ReasoningSettingsErrorCode::Io),
                None,
                NewTaskDefaults::default(),
                None,
            ),
        };
        if let Some(config) = &loaded_config
            && self.loaded_replay_config.as_ref().is_some_and(|previous| {
                previous.providers != config.providers
                    || previous.credential_bindings != config.credential_bindings
                    || previous.agent != config.agent
                    || previous.defaults != config.defaults
            })
            && let Some(sessions) = cx.try_global::<crate::app_agent::AppReplaySessions>()
        {
            sessions.0.invalidate_all();
        }
        if loaded_config.is_some() {
            self.loaded_replay_config = loaded_config;
        }
        self.configured_models = Some(models);

        let defaults_changed = cx.try_global::<NewTaskDefaults>() != Some(&defaults);
        if defaults_changed {
            cx.set_global(defaults.clone());
            if let Some(draft) = self.draft.as_mut() {
                draft.model = defaults.model;
                draft.permission_mode =
                    vega_conversation::types::PermissionMode::parse(&defaults.permission_mode)
                        .unwrap_or(vega_conversation::types::PermissionMode::Confirm);
                let draft = draft.clone();
                cx.set_global(OpenedThread(Some(draft.clone())));
                if let Some((_, stream)) = &self.stream_view {
                    stream.update(cx, |stream, cx| stream.apply_thread(draft, cx));
                }
            }
        }
        // A reasoning save owns the independent authority until its exact
        // ack. A catalog worker that started before that save may still return
        // an older snapshot; keep it from regressing the pending projection.
        let reasoning_save_pending = self.reasoning_save_pending.is_some();
        if !reasoning_save_pending {
            self.configured_reasoning = Some(reasoning_profiles);
            // A failed reasoning save remains visible while this coalesced
            // provider catalog refresh completes. A fresh file error is more
            // authoritative than the held save error; otherwise retain the
            // typed save failure so the Settings draft stays retryable.
            self.configured_reasoning_error = reasoning_error.or(self.reasoning_error_hold);
            self.configured_reasoning_authority = reasoning_authority;
        }
        let options = self.model_options_for_pricing();
        if let Some((_, stream)) = &self.stream_view {
            stream.update(cx, |stream, cx| stream.apply_model_options(options, cx));
            let model = stream.read(cx).displayed_model().to_string();
            self.apply_reasoning_profile_to_stream(stream, &model, cx);
        }
        if !reasoning_save_pending && let Some(settings) = self.settings_view.clone() {
            settings.update(cx, |settings, cx| {
                settings.apply_reasoning_projection(self.reasoning_settings_projection(), cx)
            });
        }
        self.resume_catalog_composer_predictions(cx);
        cx.notify();
    }

    pub(crate) fn persist_thread_settings(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ThreadSettingsRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        // A model save owns the same thread configuration boundary until its
        // authoritative ack. Reject mode/permission changes at both layers
        // so a late model ack cannot overwrite a newer settings projection.
        if stream.read(cx).has_pending_model_selection()
            || stream.read(cx).model_selection_blocked(cx)
            || self
                .agent_controller
                .active
                .contains_key(&request.thread_id)
            || self.trusted_actions.is_busy()
        {
            stream.update(cx, ConversationStream::apply_controller_error);
            return;
        }
        let thread_id = request.thread_id.clone();
        if let Some(draft) = self.draft_for_route(&thread_id) {
            let mut next = draft;
            if let Some(mode) = request.mode {
                next.mode = mode;
            }
            if let Some(permission_mode) = request.permission_mode {
                next.permission_mode = permission_mode;
            }
            cx.set_global(OpenedThread(Some(next.clone())));
            self.mirror_draft(&next);
            stream.update(cx, |stream, cx| stream.apply_thread(next, cx));
            return;
        }
        let result = match &cx.global::<VegaStore>().0 {
            Ok(store) => (|| {
                if let Some(mode) = request.mode {
                    vega_conversation::threads::set_thread_mode(store, &thread_id, mode)?;
                }
                if let Some(permission_mode) = request.permission_mode {
                    vega_conversation::threads::set_thread_permission_mode(
                        store,
                        &thread_id,
                        permission_mode,
                    )?;
                }
                vega_conversation::threads::open_thread(store, &thread_id)
            })()
            .map_err(|error| error.to_string()),
            Err(error) => Err(error.clone()),
        };
        match result {
            Ok(thread) => {
                cx.set_global(OpenedThread(Some(thread.clone())));
                stream.update(cx, |stream, cx| stream.apply_thread(thread, cx));
            }
            Err(_) => stream.update(cx, ConversationStream::apply_controller_error),
        }
    }

    pub(crate) fn review_plan(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &PlanReviewRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        if stream.read(cx).has_pending_model_selection() || self.trusted_actions.is_busy() {
            stream.update(cx, ConversationStream::apply_controller_error);
            return;
        }
        if self
            .agent_controller
            .active
            .contains_key(&request.thread_id)
        {
            if self.agent_controller.queue_review(&stream, request) {
                if let Some(active) = self.agent_controller.active.get(&request.thread_id) {
                    self.poison_artifact_agent_generation(active.generation, &stream);
                }
                stream.update(cx, |stream, cx| stream.timeout_permission(cx));
            } else {
                stream.update(cx, ConversationStream::apply_controller_error);
            }
            return;
        }
        self.agent_controller.pending_review.insert(
            request.thread_id.clone(),
            PendingPlanReview {
                stream: stream.clone(),
                request: request.clone(),
            },
        );
        self.finish_owned_plan_review(stream, request, cx);
    }

    pub(crate) fn finish_owned_plan_review(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &PlanReviewRequested,
        cx: &mut Context<Self>,
    ) {
        if !self
            .agent_controller
            .pending_review
            .get(&request.thread_id)
            .is_some_and(|pending| pending.stream == stream && pending.request == *request)
        {
            return;
        }
        if stream.read(cx).has_pending_model_selection() || self.trusted_actions.is_busy() {
            self.agent_controller
                .pending_review
                .remove(&request.thread_id);
            stream.update(cx, ConversationStream::apply_controller_error);
            return;
        }
        let result = match &cx.global::<VegaStore>().0 {
            Ok(store) => persist_review(store, request),
            Err(error) => Err(error.clone()),
        };
        match result {
            Ok(refresh) => {
                let approved_instruction_id = refresh.approved_instruction_id.clone();
                if self.owns_stream_request(&stream, &request.thread_id, cx) {
                    cx.set_global(OpenedThread(Some(refresh.thread.clone())));
                }
                Self::apply_thread_and_plans(&stream, refresh.thread, refresh.plans, cx);
                if let Some(instruction_message_id) = approved_instruction_id {
                    self.sidebar.update(cx, Sidebar::reload_sessions);
                    self.start_agent_run(
                        stream,
                        &request.thread_id,
                        PendingAgentRun::ApprovedPlan(instruction_message_id),
                        cx,
                    );
                }
            }
            Err(_) => {
                // A SQLite error may be commit-ambiguous. Reload authoritative
                // state before deciding whether the card may be re-armed.
                let reload = match &cx.global::<VegaStore>().0 {
                    Ok(store) => reload_thread_and_plans(store, &request.thread_id),
                    Err(error) => Err(error.clone()),
                };
                if let Ok((thread, plans)) = reload {
                    if self.owns_stream_request(&stream, &request.thread_id, cx) {
                        cx.set_global(OpenedThread(Some(thread.clone())));
                    }
                    Self::apply_thread_and_plans(&stream, thread, plans, cx);
                }
                stream.update(cx, ConversationStream::apply_controller_error);
            }
        }
        self.agent_controller
            .pending_review
            .remove(&request.thread_id);
    }
}
