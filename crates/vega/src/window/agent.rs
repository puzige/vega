use super::*;

/// Poll interval for the provider/credential preflight worker. The worker
/// owns all config and keystore IO; the UI only observes its typed result.
const AGENT_PREFLIGHT_POLL: std::time::Duration = std::time::Duration::from_millis(4);

impl VegaWindow {
    pub(crate) fn update_message_location_route(&mut self, thread_id: Option<&str>) {
        if self.message_location_route_thread_id.as_deref() == thread_id {
            return;
        }
        self.message_location_route_thread_id = thread_id.map(str::to_owned);
        self.message_location_route_generation =
            self.message_location_route_generation.saturating_add(1);
        self.deferred_message_location = None;
    }

    fn owns_message_location_request(
        &self,
        stream: &Entity<ConversationStream>,
        request: &MessageLocationWorkerRequest,
        cx: &App,
    ) -> bool {
        self.owns_stream_request(stream, &request.thread_id, cx)
            && self.message_location_route_thread_id.as_deref() == Some(&request.thread_id)
            && self.message_location_route_generation == request.route_generation
            && self.message_location_request_generation == request.request_generation
    }

    fn message_location_run_is_active(
        &self,
        stream: &Entity<ConversationStream>,
        thread_id: &str,
    ) -> bool {
        self.agent_controller
            .active
            .get(thread_id)
            .is_some_and(|active| active.stream == *stream)
    }

    pub(crate) fn request_message_location(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &MessageLocationRequested,
        cx: &mut Context<Self>,
    ) {
        let route_thread_id = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone());
        self.update_message_location_route(route_thread_id.as_deref());
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let Some(request_generation) = self.message_location_request_generation.checked_add(1)
        else {
            stream.update(cx, |stream, cx| {
                stream.apply_message_location_status(MessageLocationStatus::Failed, cx)
            });
            return;
        };
        self.message_location_request_generation = request_generation;
        self.deferred_message_location = None;
        let worker_request = MessageLocationWorkerRequest {
            thread_id: request.thread_id.clone(),
            message_id: request.message_id.clone(),
            route_generation: self.message_location_route_generation,
            request_generation,
            restore_anchor: request.restore_anchor.clone(),
        };
        if stream.update(cx, |stream, cx| {
            if let Some(anchor) = request.restore_anchor.as_ref()
                && stream.restore_scroll_anchor(anchor)
            {
                stream.apply_message_location_status(MessageLocationStatus::Located, cx);
                true
            } else {
                stream.reveal_loaded_message(&request.message_id, cx)
            }
        }) {
            return;
        }
        if self.message_location_run_is_active(&stream, &request.thread_id) {
            self.deferred_message_location = Some(DeferredMessageLocation {
                stream: stream.clone(),
                request: worker_request,
            });
            stream.update(cx, |stream, cx| {
                stream.apply_message_location_status(MessageLocationStatus::Deferred, cx)
            });
            return;
        }
        self.start_message_location_worker(stream, worker_request, cx);
    }

    /// #65 R6: independent of primary Finished/generation/route polling.
    pub(crate) fn watch_automatic_titles(
        &mut self,
        database: std::path::PathBuf,
        owner: String,
        receiver: mpsc::Receiver<()>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let mut retry = false;
            let mut io_failures = 0;
            loop {
                cx.background_executor().timer(AGENT_EVENT_POLL).await;
                match if retry { Ok(()) } else { receiver.try_recv() } {
                    Ok(()) => {
                        let Ok(epoch) = this.update(cx, |_, cx| {
                            if !retry {
                                vega_ui::navigation::begin_task_mutation(cx);
                                vega_ui::navigation::finish_task_mutation(cx);
                            }
                            cx.global::<vega_ui::navigation::TaskMutationState>().epoch
                        }) else {
                            break;
                        };
                        let path = database.clone();
                        let read_owner = owner.clone();
                        let title = cx
                            .background_executor()
                            .spawn(async move {
                                let store = vega_store::Store::open(path).map_err(|_| ())?;
                                vega_conversation::threads::read_thread_title(&store, &read_owner)
                                    .map_err(|_| ())
                            })
                            .await;
                        let title = match title {
                            Ok(title) => {
                                io_failures = 0;
                                title
                            }
                            Err(()) => {
                                io_failures += 1;
                                retry = io_failures < 3;
                                if !retry {
                                    tracing::warn!(
                                        operation = "read_title",
                                        "automatic title refresh failed"
                                    );
                                    io_failures = 0;
                                }
                                continue;
                            }
                        };
                        let applied = this.update(cx, |this, cx| {
                            this.apply_automatic_title(&owner, epoch, title, cx)
                        });
                        let Ok(applied) = applied else {
                            break;
                        };
                        // Coalesce a fresh read on next tick after any intervening mutation,
                        // even if the naming worker has already disconnected.
                        retry = !applied;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            }
        })
        .detach();
    }

    pub(crate) fn apply_automatic_title(
        &mut self,
        owner: &str,
        epoch: u64,
        title: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if cx.global::<vega_ui::navigation::TaskMutationState>().epoch != epoch {
            return false;
        }
        if let Some(title) = title {
            let mut opened = OpenedThread(cx.global::<OpenedThread>().0.clone());
            if let Some(thread) = opened.0.as_mut()
                && thread.id == owner
            {
                thread.title = title;
                cx.set_global(opened);
            }
        }
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.reload_sessions(cx));
        cx.notify();
        true
    }

    pub(crate) fn workspace_tool_terminal(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &WorkspaceToolTerminal,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let identity = self
            .diff_controller
            .active
            .as_ref()
            .filter(|active| {
                active.identity.thread_id == request.thread_id
                    && active.identity.project_id == request.project_id
            })
            .map(|active| active.identity.clone());
        if let Some(identity) = identity {
            self.schedule_diff_refresh(&identity, cx);
        }
    }

    pub(crate) fn owns_stream_request(
        &self,
        stream: &Entity<ConversationStream>,
        thread_id: &str,
        cx: &App,
    ) -> bool {
        let current_matches = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .is_some_and(|thread| thread.id == thread_id);
        current_matches
            && self
                .stream_view
                .as_ref()
                .is_some_and(|(cached_id, cached)| cached_id == thread_id && cached == stream)
    }

    /// Scroll-up hydration (S8-T45/C7): one page read per request on a
    /// worker thread; the store global itself stays on the main thread. The
    /// route fence is checked before spawning so a request from a stale
    /// view never reaches the store, and again on completion so a page that
    /// finished after a route switch is dropped (A→B→A 晚到页丢弃).
    pub(crate) fn request_history_page(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &HistoryPageRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let route_generation = self.message_location_route_generation;
        let database_path = match &cx.global::<VegaStore>().0 {
            Ok(store) => match store.database_path() {
                Some(path) => path.to_path_buf(),
                None => return,
            },
            Err(_) => return,
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_request = request.clone();
        let worker = std::thread::Builder::new()
            .name("vega-history-page".into())
            .spawn(move || run_history_page_worker(database_path, worker_request, sender));
        if worker.is_err() {
            stream.update(cx, |stream, cx| stream.apply_history_load_failed(cx));
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let outcome = match receiver.try_recv() {
                    Ok((_, outcome)) => outcome,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Err(HistoryPageFailure::Store("history page worker lost".into()))
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_history_page(stream.clone(), route_generation, outcome, cx)
                });
                break;
            }
        })
        .detach();
    }

    pub(crate) fn request_newer_history_page(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &NewerHistoryPageRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let route_generation = self.message_location_route_generation;
        let database_path = match &cx.global::<VegaStore>().0 {
            Ok(store) => match store.database_path() {
                Some(path) => path.to_path_buf(),
                None => return,
            },
            Err(_) => return,
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_request = request.clone();
        let fallback_request = request.clone();
        let worker = std::thread::Builder::new()
            .name("vega-newer-history-page".into())
            .spawn(move || run_newer_history_page_worker(database_path, worker_request, sender));
        if worker.is_err() {
            stream.update(cx, ConversationStream::apply_newer_history_load_failed);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let result = match receiver.try_recv() {
                    Ok((request, outcome)) => (request, outcome),
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => (
                        fallback_request,
                        Err(HistoryPageFailure::Store("history page worker lost".into())),
                    ),
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_newer_history_page(
                        stream.clone(),
                        route_generation,
                        result.0,
                        result.1,
                        cx,
                    )
                });
                break;
            }
        })
        .detach();
    }

    fn start_message_location_worker(
        &mut self,
        stream: Entity<ConversationStream>,
        request: MessageLocationWorkerRequest,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_message_location_request(&stream, &request, cx) {
            return;
        }
        let database_path = match &cx.global::<VegaStore>().0 {
            Ok(store) => match store.database_path() {
                Some(path) => path.to_path_buf(),
                None => {
                    stream.update(cx, |stream, cx| {
                        stream.apply_message_location_status(MessageLocationStatus::Failed, cx)
                    });
                    return;
                }
            },
            Err(_) => {
                stream.update(cx, |stream, cx| {
                    stream.apply_message_location_status(MessageLocationStatus::Failed, cx)
                });
                return;
            }
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_request = request.clone();
        let fallback_request = request.clone();
        let worker = std::thread::Builder::new()
            .name("vega-message-location".into())
            .spawn(move || run_message_location_worker(database_path, worker_request, sender));
        if worker.is_err() {
            stream.update(cx, |stream, cx| {
                stream.apply_message_location_status(MessageLocationStatus::Failed, cx)
            });
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let result = match receiver.try_recv() {
                    Ok((request, outcome)) => (request, outcome),
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => (
                        fallback_request,
                        Err(HistoryPageFailure::Store(
                            "message location worker lost".into(),
                        )),
                    ),
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_message_location_worker(stream.clone(), result.0, result.1, cx)
                });
                break;
            }
        })
        .detach();
    }

    pub(crate) fn finish_message_location_worker(
        &mut self,
        stream: Entity<ConversationStream>,
        request: MessageLocationWorkerRequest,
        outcome: MessageLocationOutcome,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_message_location_request(&stream, &request, cx) {
            return;
        }
        if self.message_location_run_is_active(&stream, &request.thread_id) {
            self.deferred_message_location = Some(DeferredMessageLocation {
                stream: stream.clone(),
                request,
            });
            stream.update(cx, |stream, cx| {
                stream.apply_message_location_status(MessageLocationStatus::Deferred, cx)
            });
            return;
        }
        match outcome {
            Ok(Some(page)) => {
                stream.update(cx, |stream, cx| {
                    stream.replace_history_window_with_anchor(
                        page,
                        &request.message_id,
                        request.restore_anchor.as_ref(),
                        cx,
                    );
                });
            }
            Ok(None) => stream.update(cx, |stream, cx| {
                stream.apply_message_location_status(MessageLocationStatus::NotFound, cx)
            }),
            Err(_) => stream.update(cx, |stream, cx| {
                stream.apply_message_location_status(MessageLocationStatus::Failed, cx)
            }),
        }
    }

    pub(crate) fn resume_deferred_message_location(
        &mut self,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(deferred) = self.deferred_message_location.take() else {
            return;
        };
        if deferred.request.thread_id != thread_id
            || !self.owns_message_location_request(&deferred.stream, &deferred.request, cx)
        {
            return;
        }
        if self.message_location_run_is_active(&deferred.stream, thread_id) {
            self.deferred_message_location = Some(deferred);
            return;
        }
        self.start_message_location_worker(deferred.stream, deferred.request, cx);
    }

    /// Applies a finished hydration page to its requesting stream, gated by
    /// the same route fence as the request: only the currently open thread's
    /// cached stream may take a page.
    pub(crate) fn finish_history_page(
        &mut self,
        stream: Entity<ConversationStream>,
        route_generation: u64,
        outcome: HistoryPageOutcome,
        cx: &mut Context<Self>,
    ) {
        let Some(opened) = cx.global::<OpenedThread>().0.clone() else {
            return;
        };
        if !self.owns_stream_request(&stream, &opened.id, cx)
            || self.message_location_route_generation != route_generation
        {
            return;
        }
        stream.update(cx, |stream, cx| match outcome {
            Ok(page) => stream.apply_history_page(page, cx),
            Err(_) => stream.apply_history_load_failed(cx),
        });
    }

    pub(crate) fn finish_newer_history_page(
        &mut self,
        stream: Entity<ConversationStream>,
        route_generation: u64,
        request: NewerHistoryPageRequested,
        outcome: HistoryPageOutcome,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx)
            || self.message_location_route_generation != route_generation
            || stream.read(cx).newer_history_cursor() != Some(request.after)
        {
            return;
        }
        stream.update(cx, |stream, cx| match outcome {
            Ok(page) => stream.apply_newer_history_page(page, cx),
            Err(_) => stream.apply_newer_history_load_failed(cx),
        });
    }

    pub(crate) fn apply_refresh(
        stream: &Entity<ConversationStream>,
        thread: Thread,
        plans: Vec<Plan>,
        cx: &mut Context<Self>,
    ) {
        cx.set_global(OpenedThread(Some(thread.clone())));
        Self::apply_thread_and_plans(stream, thread, plans, cx);
    }

    pub(crate) fn apply_thread_and_plans(
        stream: &Entity<ConversationStream>,
        thread: Thread,
        plans: Vec<Plan>,
        cx: &mut Context<Self>,
    ) {
        stream.update(cx, |stream, cx| {
            stream.apply_thread(thread, cx);
            for plan in plans {
                stream.apply_plan(plan, cx);
            }
        });
    }

    pub(crate) fn apply_stream_state(
        stream: &Entity<ConversationStream>,
        thread: Thread,
        plans: Vec<Plan>,
        history: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let thread_id = thread.id.clone();
        stream.update(cx, |stream, cx| {
            stream.apply_thread(thread, cx);
            for plan in plans {
                stream.apply_plan(plan, cx);
            }
            stream.apply_composer_history(&thread_id, history, cx);
        });
    }

    pub(crate) fn current_cached_stream_for_thread(
        &self,
        thread_id: &str,
        cx: &App,
    ) -> Option<Entity<ConversationStream>> {
        let opened_id = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.as_str());
        let cached_id = self
            .stream_view
            .as_ref()
            .map(|(cached_id, _)| cached_id.as_str());
        if !current_cache_matches(opened_id, cached_id, thread_id) {
            return None;
        }
        self.stream_view.as_ref().map(|(_, stream)| stream.clone())
    }

    pub(crate) fn cancel_active_agent(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone())
        else {
            return;
        };
        self.cancel_agent_for_thread(&thread_id, cx);
    }

    fn cancel_agent_for_thread(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let pending_review = self.agent_controller.pending_review.remove(thread_id);
        let artifact_run = self
            .agent_controller
            .active
            .get(thread_id)
            .map(|active| (active.generation, active.stream.clone()));
        if let Some(active) = self.agent_controller.active.get(thread_id) {
            active.cancel.cancel();
            active
                .stream
                .update(cx, |stream, cx| stream.timeout_permission(cx));
        }
        if let Some((generation, stream)) = artifact_run {
            self.poison_artifact_agent_generation(generation, &stream);
        }
        if let Some(pending) = pending_review
            && self.owns_stream_request(&pending.stream, &pending.request.thread_id, cx)
        {
            let refresh = match &cx.global::<VegaStore>().0 {
                Ok(store) => reload_thread_and_plans(store, &pending.request.thread_id),
                Err(error) => Err(error.clone()),
            };
            if let Ok((thread, plans)) = refresh {
                Self::apply_refresh(&pending.stream, thread, plans, cx);
            } else {
                pending
                    .stream
                    .update(cx, ConversationStream::apply_controller_error);
            }
        }
    }

    pub(crate) fn stop_composer(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerStopRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        if self
            .agent_controller
            .active
            .get(&request.thread_id)
            .is_some_and(|run| run.stream == stream)
        {
            self.cancel_agent_for_thread(&request.thread_id, cx);
            return;
        }
        if self.agent_controller.preparation_stream.as_ref() == Some(&stream)
            && self.trusted_actions.cancel_agent_preparation()
        {
            self.agent_controller.preparation_stream = None;
            stream.update(cx, |stream, cx| {
                stream.set_trusted_action_busy(false, cx);
                stream.reject_composer_submission(cx);
                stream.finish_composer_run(true, cx);
            });
            return;
        }
        self.cancel_manual_context_for_stream(&stream);
    }

    pub(crate) fn start_agent_run(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: &str,
        run: PendingAgentRun,
        cx: &mut Context<Self>,
    ) {
        // Programmatic/Plan callers use the same run-start freeze as the
        // Composer path. An invalid declared profile is rejected while the
        // draft remains in the stream; it must never silently become a
        // provider-default request in the worker.
        let reasoning = match stream.read(cx).frozen_reasoning_for_submit() {
            Ok(reasoning) => reasoning,
            Err(_) => {
                match &run {
                    PendingAgentRun::UserMessage(_) => {
                        stream.update(cx, ConversationStream::reject_composer_submission);
                        stream.update(cx, ConversationStream::apply_agent_error);
                    }
                    PendingAgentRun::ApprovedPlan(_) => {
                        stream.update(cx, ConversationStream::apply_agent_error);
                    }
                }
                return;
            }
        };
        self.start_agent_run_with_reasoning(stream, thread_id, run, reasoning, cx);
    }

    pub(crate) fn start_agent_run_with_reasoning(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: &str,
        run: PendingAgentRun,
        reasoning: Option<FrozenReasoning>,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, thread_id, cx)
            && !self
                .agent_controller
                .pending_review
                .get(thread_id)
                .is_some_and(|pending| pending.stream == stream)
        {
            return;
        }
        if crate::updater::installing() || self.agent_controller.active.contains_key(thread_id) {
            stream.update(cx, ConversationStream::apply_agent_busy);
            return;
        }
        let reasoning_invalid = reasoning.as_ref().is_some_and(|reasoning| {
            reasoning.validate().is_err() || reasoning.model != stream.read(cx).displayed_model()
        });
        if reasoning_invalid {
            match &run {
                PendingAgentRun::UserMessage(_) => {
                    stream.update(cx, ConversationStream::reject_composer_submission);
                    stream.update(cx, ConversationStream::apply_agent_error);
                }
                PendingAgentRun::ApprovedPlan(_) => {
                    stream.update(cx, ConversationStream::apply_agent_error);
                }
            }
            return;
        }
        let pending_user_content = match &run {
            PendingAgentRun::UserMessage(content) => Some(content.content.clone()),
            PendingAgentRun::ApprovedPlan(_) => None,
        };
        let pending_approved_instruction = match &run {
            PendingAgentRun::UserMessage(_) => None,
            PendingAgentRun::ApprovedPlan(instruction_id) => Some(instruction_id.clone()),
        };
        if stream.read(cx).has_pending_model_selection()
            || self.trusted_actions.is_busy()
            || self.reasoning_save_pending.is_some()
            || self.model_catalog_loading
            || stream.read(cx).reasoning_unavailable()
        {
            match run {
                PendingAgentRun::UserMessage(_) => {
                    stream.update(cx, ConversationStream::reject_composer_submission);
                    stream.update(cx, ConversationStream::apply_agent_error);
                }
                PendingAgentRun::ApprovedPlan(_) => {
                    stream.update(cx, ConversationStream::apply_agent_error);
                }
            }
            return;
        }
        let prepared = match &cx.global::<VegaStore>().0 {
            Ok(store) => (|| {
                let database_path = store
                    .database_path()
                    .ok_or_else(|| "agent store is not file-backed".to_string())?
                    .to_path_buf();
                let thread = vega_conversation::threads::open_thread(store, thread_id)
                    .map_err(|error| error.to_string())?;
                // Project tasks run in their registered folder; standalone
                // tasks get an app-owned scratch root scoped to this thread.
                // Keeping this resolution in the conversation layer makes
                // the workspace fence identical for UI-created and resumed
                // runs, without introducing a synthetic project row.
                let workspace = vega_conversation::threads::task_workspace_root(store, thread_id)
                    .map_err(|error| error.to_string())?;
                Ok((database_path, workspace, thread))
            })(),
            Err(error) => Err(error.clone()),
        };
        let Ok((database_path, project_path, thread)) = prepared else {
            if pending_user_content.is_some() {
                stream.update(cx, ConversationStream::reject_composer_submission);
            }
            if pending_approved_instruction.is_some() {
                stream.update(cx, ConversationStream::apply_approved_not_started);
            } else {
                stream.update(cx, ConversationStream::apply_agent_error);
            }
            return;
        };

        // #60 R2/R3: missing prices are not a send gate. Runtime and meter
        // share this one immutable optional snapshot; provider/permission
        // validation still runs independently through its existing path.
        let pricing_catalog = self.pricing_controller.catalog_for_run(&thread.model);

        let permission_queue = stream.read(cx).permission_queue();
        let Some((generation, cancel)) = self.agent_controller.begin(
            thread_id.to_string(),
            stream.clone(),
            pending_user_content,
            pending_approved_instruction,
        ) else {
            stream.update(cx, ConversationStream::apply_agent_busy);
            return;
        };
        // R8: an accepted run is mirrored into the app-wide thread liveness
        // projection immediately, so the sidebar row lights up while the
        // worker is still preparing its first durable message.
        vega_ui::sidebar::set_thread_running(thread_id, true, cx);
        self.agent_controller.preparation_stream = None;
        self.begin_context_primary_owner(generation, cx);
        stream.update(cx, ConversationStream::begin_composer_run);
        self.ensure_agent_artifact_route(&thread, &stream, cx);
        self.begin_artifact_agent_generation(generation, &stream);
        // S7-T39/C3: the provisional estimator freezes the run-start
        // selection; it never re-reads pricing files or the live authority.
        let meter_estimator = pricing_catalog.clone().and_then(|catalog| {
            vega_conversation::types::RunUsageEstimator::new(&thread.model, catalog)
        });
        stream.update(cx, |stream, cx| {
            stream.install_meter_estimator(meter_estimator, cx)
        });
        let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
        let (title_sender, title_receiver) = mpsc::channel();
        self.watch_automatic_titles(
            database_path.clone(),
            thread_id.to_string(),
            title_receiver,
            cx,
        );
        let worker_sender = sender.clone();
        let config_path = self.composer_config_path();
        let mcp_settings = self.mcp_settings.clone();
        let file_read_state = self
            .agent_controller
            .file_read_states
            .entry(thread_id.to_string())
            .or_default()
            .clone();
        // A8-02: register synchronously before spawn. The worker closure owns
        // the token through its last tool/provider call, even if this window
        // cancels or closes before the worker can acknowledge termination.
        let worker_activity = vega_ui::sidebar::register_project_worker(&thread.project_id, cx);
        #[cfg(test)]
        let provider_override = self.agent_provider_override.clone();
        #[cfg(test)]
        let worker_start_probe = self.agent_worker_start_probe.clone();
        let worker = std::thread::Builder::new()
            .name("vega-agent".into())
            .spawn(move || {
                run_agent_worker_with_mcp(
                    database_path,
                    project_path,
                    thread,
                    run,
                    permission_queue,
                    cancel,
                    worker_sender,
                    pricing_catalog,
                    config_path,
                    reasoning,
                    Some(title_sender),
                    mcp_settings,
                    file_read_state,
                    #[cfg(test)]
                    provider_override,
                    #[cfg(test)]
                    worker_start_probe,
                );
                drop(worker_activity);
            });
        if worker.is_err() {
            stream.update(cx, |stream, cx| stream.finish_composer_run(false, cx));
            self.poison_artifact_agent_generation(generation, &stream);
            let failed_run = self.agent_controller.finish(generation, thread_id, &stream);
            // A spawn failure releases the run it just registered; the row
            // must not keep spinning for a worker that never started.
            vega_ui::sidebar::set_thread_running(thread_id, false, cx);
            self.finish_context_primary_owner(generation);
            if failed_run
                .as_ref()
                .is_some_and(|active| active.pending_user_content.is_some())
            {
                stream.update(cx, ConversationStream::reject_composer_submission);
            }
            stream.update(cx, |stream, cx| stream.timeout_permission(cx));
            if failed_run.is_some_and(|active| active.pending_approved_instruction.is_some()) {
                stream.update(cx, ConversationStream::apply_approved_not_started);
            } else {
                stream.update(cx, ConversationStream::apply_agent_error);
            }
            return;
        }
        drop(sender);

        let thread_id = thread_id.to_string();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(AGENT_EVENT_POLL).await;
                let batch = drain_agent_updates(&receiver);
                let keep_running = this
                    .update(cx, |this, cx| {
                        let (success, finished_run, reference_failure, credential_failure) =
                            match this.apply_agent_batch_ingress(
                                generation, &thread_id, &stream, batch, cx,
                            ) {
                                AgentBatchIngress::Stale => return false,
                                AgentBatchIngress::Running => return true,
                                AgentBatchIngress::Finished {
                                    success,
                                    run,
                                    reference_failure,
                                    credential_failure,
                                } => (success, run, reference_failure, credential_failure),
                            };
                        let cancelled = finished_run.cancel.is_cancelled();
                        if this.owns_stream_request(&stream, &thread_id, cx) {
                            this.refresh_context_projection(false, cx);
                        }
                        let cancelled = stream
                            .update(cx, |stream, cx| stream.finish_composer_run(cancelled, cx));
                        let ActiveAgentRun {
                            pending_user_content: pending_user,
                            pending_approved_instruction,
                            terminal_failure,
                            mcp_unavailable,
                            ..
                        } = *finished_run;
                        let approved_not_started = pending_approved_instruction.is_some();
                        if pending_user.is_some() {
                            stream.update(cx, ConversationStream::reject_composer_submission);
                        }
                        let pending_review = this
                            .agent_controller
                            .pending_review
                            .get(&thread_id)
                            .cloned();
                        let refresh = match &cx.global::<VegaStore>().0 {
                            Ok(store) => reload_thread_state(store, &thread_id),
                            Err(error) => Err(error.clone()),
                        };
                        let mut recovery_projected = approved_not_started;
                        if let Ok(refresh) = refresh {
                            let display_stream = if let Some(current_stream) =
                                this.current_cached_stream_for_thread(&thread_id, cx)
                            {
                                cx.set_global(OpenedThread(Some(refresh.thread.clone())));
                                Self::apply_stream_state(
                                    &current_stream,
                                    refresh.thread,
                                    refresh.plans,
                                    refresh.history,
                                    cx,
                                );
                                current_stream
                            } else {
                                Self::apply_stream_state(
                                    &stream,
                                    refresh.thread,
                                    refresh.plans,
                                    refresh.history,
                                    cx,
                                );
                                stream.clone()
                            };
                            recovery_projected |=
                                refresh.recoverable_approved_instruction.is_some();
                            if recovery_projected {
                                display_stream
                                    .update(cx, ConversationStream::apply_approved_not_started);
                            }
                            if let Some(code) = reference_failure {
                                display_stream.update(cx, |stream, cx| {
                                    stream.apply_reference_error(code, cx)
                                });
                            }
                        } else if approved_not_started {
                            stream.update(cx, ConversationStream::apply_approved_not_started);
                        } else if let Some(code) = reference_failure {
                            stream.update(cx, |stream, cx| stream.apply_reference_error(code, cx));
                        } else {
                            stream.update(cx, ConversationStream::apply_agent_error);
                        }
                        if credential_failure {
                            stream.update(cx, ConversationStream::apply_credential_error);
                        }
                        if !success
                            && !cancelled
                            && !recovery_projected
                            && reference_failure.is_none()
                            && !credential_failure
                        {
                            stream.update(cx, |stream, cx| {
                                if let Some(failure) = terminal_failure {
                                    stream.apply_agent_runtime_error(failure, cx);
                                } else {
                                    stream.apply_agent_error(cx);
                                }
                            });
                        }
                        if !mcp_unavailable.is_empty() {
                            let warning_stream = this
                                .current_cached_stream_for_thread(&thread_id, cx)
                                .unwrap_or_else(|| stream.clone());
                            warning_stream.update(cx, |stream, cx| {
                                stream.apply_mcp_unavailable(&mcp_unavailable, cx)
                            });
                        }
                        if let Some(pending) = pending_review {
                            this.finish_owned_plan_review(pending.stream, &pending.request, cx);
                        }
                        this.resume_deferred_message_location(&thread_id, cx);
                        false
                    })
                    .unwrap_or(false);
                if !keep_running {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn submit_composer(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerSubmitted,
        cx: &mut Context<Self>,
    ) {
        if crate::updater::installing() {
            stream.update(cx, ConversationStream::apply_agent_busy);
            return;
        }
        if (request.content.is_empty() && request.images.is_empty())
            || !self.owns_stream_request(&stream, &request.thread_id, cx)
        {
            return;
        }
        if self
            .agent_controller
            .active
            .contains_key(&request.thread_id)
            || (self.agent_controller.preparation_stream.as_ref() == Some(&stream)
                && self.trusted_actions.is_busy())
        {
            stream.update(cx, ConversationStream::apply_agent_busy);
            return;
        }
        let reasoning = match request.reasoning.clone() {
            Ok(reasoning) => reasoning,
            Err(_) => {
                // Preserve the draft and make the invalid explicit profile
                // visible. Never turn a failed declared profile into a
                // provider-default request.
                stream.update(cx, |stream, cx| {
                    stream.reject_composer_submission(cx);
                    stream.apply_controller_error(cx);
                });
                return;
            }
        };
        // A7-01: hold the existing draft/input in memory while a bounded
        // worker reads the owned config and credential store. Materialization
        // and agent start happen only after this typed readiness result; no
        // config or keystore IO is performed on the UI submit path.
        let Some(lease) = self
            .trusted_actions
            .acquire(TrustedActionKind::AgentPreflight, 0, 0)
        else {
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_controller_error(cx);
            });
            return;
        };
        self.agent_controller.preparation_stream = Some(stream.clone());
        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        let config_path = self.composer_config_path();
        let model = stream.read(cx).displayed_model().to_owned();
        let content = request.content.clone();
        let images = request.images.clone();
        let skill_intent = request.skill_intent.clone();
        let thread_id = request.thread_id.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_model = model.clone();
        let worker = std::thread::Builder::new()
            .name("vega-agent-preflight".into())
            .spawn(move || {
                let outcome = preflight_provider(config_path.as_deref(), &worker_model);
                let _ = sender.send(outcome);
            });
        if worker.is_err() {
            let _ = self.trusted_actions.release(lease);
            self.agent_controller.preparation_stream = None;
            stream.update(cx, |stream, cx| {
                stream.set_trusted_action_busy(false, cx);
                stream.reject_composer_submission(cx);
                stream.apply_controller_error(cx);
            });
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(AGENT_PREFLIGHT_POLL).await;
                let outcome = match receiver.try_recv() {
                    Ok(outcome) => outcome,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Err(ProviderPreflightFailure::ProviderUnavailable {
                            model: model.clone(),
                            providers: Vec::new(),
                        })
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_agent_preflight(
                        stream.clone(),
                        thread_id.clone(),
                        content.clone(),
                        images.clone(),
                        skill_intent.clone(),
                        reasoning.clone(),
                        lease,
                        outcome,
                        cx,
                    )
                });
                break;
            }
        })
        .detach();
    }

    /// Applies the worker's typed readiness result. The route fence and the
    /// single-flight lease are checked before any materialization or run
    /// start, so late/stale results cannot create a task.
    #[allow(clippy::too_many_arguments)]
    fn finish_agent_preflight(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: String,
        content: String,
        images: Vec<vega_conversation::types::ImageAttachment>,
        skill_intent: Option<SkillSelectionIntent>,
        reasoning: Option<FrozenReasoning>,
        lease: TrustedActionToken,
        outcome: Result<(), ProviderPreflightFailure>,
        cx: &mut Context<Self>,
    ) {
        if !self.trusted_actions.release(lease) {
            return;
        }
        self.agent_controller.preparation_stream = None;
        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
        if cx.global::<SettingsOpen>().0 || !self.owns_stream_request(&stream, &thread_id, cx) {
            stream.update(cx, ConversationStream::reject_composer_submission);
            return;
        }
        if let Err(failure) = outcome {
            stream.update(cx, |stream, cx| {
                stream.apply_provider_preflight_error(failure, cx)
            });
            return;
        }
        // R69 R8-R11: first submit materializes the lazy draft under its own
        // id only after readiness passes. Route identity is untouched
        // (`OpenedThread.0.id` and the `stream_view` key are unchanged), so
        // the cached stream — with its composer text and focus — is never
        // rebuilt. A materialization failure keeps the draft installed and
        // surfaces the existing controller error for retry.
        if let Some(draft) = self.draft_for_route(&thread_id) {
            // #60 R2: provider readiness was checked above; optional prices
            // must not prevent the first submit from materializing this ID.
            if self.materialize_draft(&draft, cx).is_err() {
                stream.update(cx, |stream, cx| {
                    stream.reject_composer_submission(cx);
                    stream.apply_controller_error(cx);
                });
                return;
            }
        }
        if let Some(intent) = skill_intent {
            self.start_skill_pin_before_agent(
                stream, thread_id, content, images, reasoning, intent, cx,
            );
            return;
        }
        self.start_agent_run_with_reasoning(
            stream,
            &thread_id,
            PendingAgentRun::UserMessage(crate::app_agent::UserSubmission { content, images }),
            reasoning,
            cx,
        );
    }
}
