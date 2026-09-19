use super::*;

/// Poll interval for the model-selection persistence worker (R1).
pub(crate) const MODEL_SELECTION_POLL: std::time::Duration = std::time::Duration::from_millis(4);

type ReasoningAuthority = (
    vega_store::reasoning::ReasoningConfig,
    vega_store::reasoning::ReasoningFileSnapshot,
);
type ModelCatalogLoadResult = Result<
    (
        Vec<String>,
        Vec<ReasoningProfileProjection>,
        Option<ReasoningSettingsErrorCode>,
        Option<ReasoningAuthority>,
        // R69 R3: the config's `defaults` travel with the catalog so the home
        // draft takes its model/permission from the same `config::load()`
        // source `create_task` used, without any config IO during render.
        NewTaskDefaults,
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
                // R69 R5: the draft is the row materialization will INSERT, so
                // an accepted in-memory model change must land there too.
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
    pub(crate) fn finish_thread_model_selection_from_async(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: String,
        lease: TrustedActionToken,
        request_id: u64,
        outcome: Result<Thread, ()>,
        cx: &mut Context<Self>,
    ) {
        finish_thread_model_selection(self, &stream, &thread_id, lease, request_id, outcome, cx);
    }

    /// R69 R5: mirrors an accepted in-memory thread projection onto the
    /// window's draft. The draft is the value materialization INSERTs (R8), so
    /// every draft-route field update must land here as well as on
    /// `OpenedThread` — otherwise the model/mode/permission the user picked
    /// would be lost at first submit. A durable route is a no-op.
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

    /// Cmd+N entry point: navigates to the home draft route (R69 R14). The
    /// sidebar [新建任务] button and the command palette share this handler.
    /// No durable row is written here: the draft materializes on first submit.
    pub(crate) fn open_new_thread(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.update(cx, Sidebar::create_thread);
    }

    /// R69 R1/R2/R4: resolves the thread the content column renders.
    ///
    /// A durable route resolves to itself. The home route (and the window's
    /// own draft route) resolves to the single lazy draft, which is built on
    /// first use, reused on every re-entry, and re-bound to the current
    /// [`SelectedProject`] **without changing its id** — so the composer draft
    /// text keyed by that id survives a project switch (R4).
    ///
    /// Defaults come from the already-loaded [`NewTaskDefaults`] global, so
    /// this performs no config IO during render (R3 / A2-14). The
    /// `OpenedThread` projection is written only when it actually changes, so
    /// a steady draft route does not notify observers on every frame.
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

    /// R69 R8-R11: persists the draft under **its own id** exactly once.
    ///
    /// The route identity is untouched (`OpenedThread.0.id` and the
    /// `stream_view` cache key are the same before and after), so the cached
    /// `ConversationStream` — composer text and focus included — is never
    /// rebuilt (R9). On failure the draft stays installed so the caller can
    /// preserve the text and surface the error (R10); on success the draft is
    /// released, which is what makes a second submit a no-op (R11).
    ///
    /// R69 M4: the row is stamped at the submit instant inside the conversation
    /// layer. `OpenedThread` is deliberately **not** rewritten here: the only
    /// reader of `created_at` is the sidebar's "Created" sort, which projects
    /// durable rows, and the run's own completion re-reads the authoritative
    /// thread anyway (`reload_thread_state`). Writing the global mid-submit
    /// would fire the `OpenedThread` observer and flip the next render onto the
    /// durable path (starting the branch/artifact controllers) while the run is
    /// still starting — a side effect this route did not have before.
    pub(crate) fn materialize_draft(
        &mut self,
        thread: &Thread,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match &cx.global::<VegaStore>().0 {
            Ok(store) => vega_conversation::threads::materialize_draft(store, thread)
                .map_err(|error| error.to_string())?,
            Err(error) => return Err(error.clone()),
        };
        self.draft = None;
        Ok(())
    }

    /// R1 (A2-14/S8-T47): the in-session model selection. The view emits the
    /// intent only; this handler installs the existing trusted-action
    /// single-flight gate first, then persists the durable `threads.model`
    /// through the conversation service on a worker thread and acknowledges
    /// with the re-read authoritative thread. The app-level
    /// config.defaults.model (Settings 的"新任务默认模型") is NOT touched.
    /// #60 R1: not-uniquely-configured models are rejected before any
    /// write; an active run or another trusted action refuses the change.
    /// Every outcome (ack/fail/worker-lost) releases exactly the owner token
    /// echoed by the request.
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
        // Existing fail-closed gates: an active run or any in-flight trusted
        // action must not race a run-configuration change; the same applies
        // while Settings route is open or an approved plan has not started
        // (pending plan/review/permission per R1 constraint 5).
        if cx.global::<SettingsOpen>().0
            || stream.read(cx).model_selection_blocked(cx)
            || self.agent_controller.active.is_some()
            || self.trusted_actions.is_busy()
        {
            stream.update(cx, |stream, cx| {
                stream.apply_thread_model_failed(&request.thread_id, request.request_id, cx)
            });
            return;
        }
        // #60 R1: optional price metadata does not decide model availability.
        // Unique configured provider gate: the model must resolve to exactly
        // one configured provider (same rule the real submit path applies).
        // This is a file read, so it happens in the bounded worker below,
        // AFTER the busy owner is held — never on the UI thread.
        // Single-flight owner BEFORE any async work: submit, approved-plan
        // starts and conflicting trusted actions are blocked until the
        // exact owner token is released on every exit path.
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
        // R69 R5: on the draft route the same gates and the same worker run
        // (provider uniqueness stays a worker-side config read), but the
        // durable `threads.model` write is replaced by an in-memory
        // projection. The draft carries the new model into the INSERT that
        // first submit performs (R8), so nothing is lost and no row is
        // written before the user sends anything.
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
        let worker_request = request.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        #[cfg(test)]
        let worker_gate = self.model_selection_worker_gate.take();
        let spawned = std::thread::Builder::new()
            .name("vega-thread-model".into())
            .spawn(move || {
                #[cfg(test)]
                if let Some(gate) = worker_gate {
                    gate.wait();
                }
                // R1 constraints 3+7: provider uniqueness and the durable
                // SQLite write both run in this bounded worker on owned
                // profile paths (config file + store file); no Keychain or
                // provider/network access ever happens on this path.
                let outcome = (|| {
                    let config = vega_store::config::read_from(&config_path)
                        .map_err(|error| error.to_string())?;
                    if unique_provider_for_model(&config, &worker_request.model).is_none() {
                        return Err("model is not uniquely configured".to_string());
                    }
                    // R69: the draft route stops after validation and returns
                    // its own thread with the new model. The caller projects it
                    // into `OpenedThread` and the window's draft; the row is
                    // written by first submit.
                    if let Some(draft) = draft {
                        let mut next = draft;
                        next.model = worker_request.model;
                        return Ok(next);
                    }
                    let database_path =
                        database_path.ok_or_else(|| "store unavailable".to_string())?;
                    let store = vega_store::Store::open(&database_path)
                        .map_err(|error| error.to_string())?;
                    vega_conversation::threads::set_thread_model(
                        &store,
                        &worker_request.thread_id,
                        &worker_request.model,
                    )
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

    /// The composer config file path from the current environment (R1): the
    /// worker reads/validates the provider config from this owned file
    /// instead of any synchronous global read on the UI thread. Production
    /// uses the resolved config root; the E2E fixture hands its owned temp
    /// config through the existing cfg(test) seam pattern (no env changes).
    pub(crate) fn composer_config_path(&self) -> Option<std::path::PathBuf> {
        #[cfg(test)]
        if let Some(path) = &self.model_selection_config_override {
            return Some(path.clone());
        }
        vega_store::paths::config_dir().map(|dir| dir.join("config.toml"))
    }

    /// Refreshes the app model catalog after Settings has durably saved a
    /// config mutation. The worker rereads the file and re-applies the
    /// pricing/uniqueness projection; it never changes the current thread's
    /// durable model or emits an R1 acknowledgement.
    pub(crate) fn on_settings_saved(&mut self, cx: &mut Context<Self>) {
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

    /// #60 R1: configured, uniquely resolvable models remain available even
    /// without accounting metadata. Config IO stays in the worker below.
    /// The existing method name is retained for compatibility with callers.
    pub(crate) fn model_options_for_pricing(&self) -> Vec<String> {
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
        #[cfg(test)]
        let worker_gate = self.model_catalog_worker_gate.take();
        let worker = std::thread::Builder::new()
            .name("vega-model-catalog".into())
            .spawn(move || {
                #[cfg(test)]
                if let Some(gate) = worker_gate {
                    gate.wait();
                }
                let result = vega_store::config::read_from(&config_path)
                    .map(|config| {
                        let mut models = Vec::new();
                        let mut reasoning_profiles = Vec::new();
                        let mut reasoning_error = None;
                        let mut reasoning_authority = None;
                        let reasoning_path = config_path.with_file_name("reasoning.toml");
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
                        for provider in config.providers.iter().filter(|provider| provider.enabled)
                        {
                            for model in &provider.models {
                                if !models.contains(model)
                                    && unique_provider_for_model(&config, model).is_some()
                                {
                                    models.push(model.clone());
                                    let profile = reasoning.as_ref().and_then(|reasoning| {
                                        reasoning
                                            .profile(&provider.name, model)
                                            .map(ReasoningProfileProjection::from_store)
                                    });
                                    let profile = match profile {
                                        Some(Ok(profile)) => profile,
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
                                            ReasoningProfileProjection::unknown(
                                                provider.name.clone(),
                                                model.clone(),
                                            )
                                        }
                                        None => ReasoningProfileProjection::unknown(
                                            provider.name.clone(),
                                            model.clone(),
                                        ),
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
        let (models, reasoning_profiles, reasoning_error, reasoning_authority, defaults) =
            match result {
                Ok(result) => result,
                Err(()) => (
                    Vec::new(),
                    Vec::new(),
                    Some(ReasoningSettingsErrorCode::Io),
                    None,
                    // A malformed/unreadable config keeps the missing-config
                    // template values, which is exactly what `create_task`'s
                    // error path would have produced.
                    NewTaskDefaults::default(),
                ),
            };
        self.configured_models = Some(models);
        // R69 R3: the draft's defaults follow the same config source as the
        // eager create path. Only a route that has not started yet takes the
        // new values; an existing draft keeps whatever the user already saw
        // and can edit, so a catalog refresh never rewrites a live draft.
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
            || self.agent_controller.active.is_some()
            || self.trusted_actions.is_busy()
        {
            stream.update(cx, ConversationStream::apply_controller_error);
            return;
        }
        let thread_id = request.thread_id.clone();
        // R69 R5: the draft route updates mode/permission in memory only. The
        // draft is the row first submit INSERTs (R8), so the same field-by-field
        // projection reaches the store then — with no write before the user
        // sends anything. Mode stays `Execute` on the draft: the `+` menu's
        // Plan/Ask entries are durable transitions that the store's pending-plan
        // rules own, so they are refused here exactly as `set_thread_mode`
        // would refuse them for a missing row.
        if let Some(draft) = self.draft_for_route(&thread_id) {
            if request.mode.is_some_and(|mode| mode != draft.mode) {
                stream.update(cx, ConversationStream::apply_controller_error);
                return;
            }
            let mut next = draft;
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
        if self.agent_controller.active.is_some() {
            if self.agent_controller.queue_review(&stream, request) {
                if let Some(active) = self.agent_controller.active.as_ref() {
                    self.poison_artifact_agent_generation(active.generation, &stream);
                }
                stream.update(cx, |stream, cx| stream.timeout_permission(cx));
            } else {
                stream.update(cx, ConversationStream::apply_controller_error);
            }
            return;
        }
        let result = match &cx.global::<VegaStore>().0 {
            Ok(store) => persist_review(store, request),
            Err(error) => Err(error.clone()),
        };
        match result {
            Ok(refresh) => {
                let approved_instruction_id = refresh.approved_instruction_id.clone();
                Self::apply_refresh(&stream, refresh.thread, refresh.plans, cx);
                if let Some(instruction_message_id) = approved_instruction_id {
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
                    Self::apply_refresh(&stream, thread, plans, cx);
                }
                stream.update(cx, ConversationStream::apply_controller_error);
            }
        }
    }
}
