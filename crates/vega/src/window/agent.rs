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

    pub(crate) fn request_history_page(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &HistoryPageRequested,
        cx: &mut Context<Self>,
    ) {
        if self.owns_subagent_read(&stream, &request.thread_id, cx) {
            self.request_subagent_history(stream, &request.thread_id, request.before, false, cx);
            return;
        }
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
        if self.owns_subagent_read(&stream, &request.thread_id, cx) {
            self.request_subagent_history(stream, &request.thread_id, request.after, true, cx);
            return;
        }
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

    pub(super) fn cancel_agent_for_thread(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        if let Some(sessions) = cx.try_global::<crate::app_agent::AppReplaySessions>() {
            sessions.0.invalidate_thread(thread_id);
        }
        let pending_review = self.agent_controller.pending_review.remove(thread_id);
        let artifact_run = self
            .agent_controller
            .active
            .get(thread_id)
            .map(|active| (active.generation, active.stream.clone()));
        if let Some(active) = self.agent_controller.active.get_mut(thread_id) {
            active.cancellation_reason = Some(RunCancellationReason::User);
            if let Some(control) = &active.computer_use {
                control.stop();
            }
            active.cancel.cancel();
            active.stream.update(cx, |stream, cx| {
                stream.timeout_permission(cx);
                stream.cancel_user_questions(cx);
            });
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
        self.agent_controller
            .request_active_cancel(&request.thread_id);
        self.pause_automation_all(&request.thread_id, "已由你停止", cx);
        if self
            .agent_controller
            .queue_drain_waits
            .remove(&request.thread_id)
            .is_some()
        {
            let mut projection = stream.read(cx).pending_queue().clone();
            projection.paused = true;
            self.project_pending_queue(&request.thread_id, &stream, projection, cx);
            return;
        }
        if self
            .agent_controller
            .ordinary_preparing
            .contains_key(&request.thread_id)
        {
            stream.update(cx, ConversationStream::refresh_ordinary_submissions);
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
        self.start_agent_run_with_admission(stream, thread_id, run, None, cx);
    }

    pub(super) fn start_agent_run_with_admission(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: &str,
        run: PendingAgentRun,
        admission: Option<ThreadAdmission>,
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
                    PendingAgentRun::Automation(_)
                    | PendingAgentRun::ApprovedPlan(_)
                    | PendingAgentRun::CredentialContinuation(_) => {
                        stream.update(cx, ConversationStream::apply_agent_error);
                    }
                }
                return;
            }
        };
        self.start_agent_run_with_authority(stream, thread_id, run, reasoning, admission, cx);
    }

    pub(crate) fn start_agent_run_with_reasoning(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: &str,
        run: PendingAgentRun,
        reasoning: Option<FrozenReasoning>,
        cx: &mut Context<Self>,
    ) {
        self.start_agent_run_with_authority(stream, thread_id, run, reasoning, None, cx);
    }

    fn start_agent_run_with_authority(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: &str,
        run: PendingAgentRun,
        reasoning: Option<FrozenReasoning>,
        supplied_admission: Option<ThreadAdmission>,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, thread_id, cx)
            && !(self.automation_controller.dispatch.contains_key(thread_id)
                && self.automation_controller.streams.get(thread_id) == Some(&stream))
            && !self.owns_pending_dispatch(&stream, thread_id, cx)
            && !self
                .agent_controller
                .pending_review
                .get(thread_id)
                .is_some_and(|pending| pending.stream == stream)
        {
            return;
        }
        if crate::updater::installing()
            || thread_is_running(thread_id, cx)
            || self.agent_controller.active.contains_key(thread_id)
            || self
                .agent_controller
                .queue_operations
                .contains_key(thread_id)
            || stream.read(cx).pending_queue().busy
        {
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
                PendingAgentRun::Automation(_)
                | PendingAgentRun::ApprovedPlan(_)
                | PendingAgentRun::CredentialContinuation(_) => {
                    stream.update(cx, ConversationStream::apply_agent_error);
                }
            }
            return;
        }
        let pending_user_content = match &run {
            PendingAgentRun::Automation(claim) => Some(automation_trigger(claim).into()),
            PendingAgentRun::UserMessage(content) => {
                content.ordinary.is_none().then(|| content.content.clone())
            }
            PendingAgentRun::ApprovedPlan(_) | PendingAgentRun::CredentialContinuation(_) => None,
        };
        let pending_approved_instruction = match &run {
            PendingAgentRun::Automation(_)
            | PendingAgentRun::UserMessage(_)
            | PendingAgentRun::CredentialContinuation(_) => None,
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
                PendingAgentRun::Automation(_)
                | PendingAgentRun::ApprovedPlan(_)
                | PendingAgentRun::CredentialContinuation(_) => {
                    stream.update(cx, ConversationStream::apply_agent_error);
                }
            }
            return;
        }
        let ordinary_launch = match &run {
            PendingAgentRun::UserMessage(submission) => submission.ordinary.as_ref(),
            _ => None,
        };
        let ordinary_cancel = ordinary_launch.map(|launch| launch.cancel.clone());
        let admission = match &cx.global::<VegaStore>().0 {
            Ok(store) => match ordinary_launch.and_then(|launch| launch.run()) {
                Some(run) => Some(run.worker.admission().clone()),
                None => match supplied_admission {
                    Some(owner) => owner
                        .verifies(store.database_identity(), thread_id)
                        .then_some(owner),
                    None => vega_store::thread_admission::try_acquire(
                        store.database_identity(),
                        thread_id,
                    )
                    .ok()
                    .flatten(),
                },
            },
            Err(_) => None,
        };
        let Some(admission) = admission else {
            stream.update(cx, ConversationStream::apply_agent_busy);
            return;
        };
        let prepared = if let Some(launch) = ordinary_launch {
            let valid = match &cx.global::<VegaStore>().0 {
                Ok(store) => launch.run().is_some_and(|run| {
                    run.worker
                        .admission()
                        .verifies(store.database_identity(), thread_id)
                }),
                Err(_) => false,
            };
            if valid && !launch.cancel.is_cancelled() {
                Ok((
                    launch.database.clone(),
                    launch.workspace.clone(),
                    launch.thread.clone(),
                ))
            } else {
                Err("Submission is no longer available".to_owned())
            }
        } else {
            match &cx.global::<VegaStore>().0 {
                Ok(store) => (|| {
                    let database_path = store
                        .database_path()
                        .ok_or_else(|| "agent store is not file-backed".to_string())?
                        .to_path_buf();
                    let thread = vega_conversation::threads::open_thread(store, thread_id)
                        .map_err(|error| error.to_string())?;
                    let workspace =
                        vega_conversation::threads::task_workspace_root(store, thread_id)
                            .map_err(|error| error.to_string())?;
                    Ok((database_path, workspace, thread))
                })(),
                Err(error) => Err(error.clone()),
            }
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

        let pricing_catalog = self.pricing_controller.catalog_for_run(&thread.model);

        let permission_queue = stream.read(cx).permission_queue();
        let user_questions = stream.read(cx).user_question_queue();
        let Some((generation, cancel)) = self.agent_controller.begin(
            thread_id.to_string(),
            stream.clone(),
            pending_user_content,
            pending_approved_instruction,
        ) else {
            stream.update(cx, ConversationStream::apply_agent_busy);
            return;
        };
        self.agent_controller
            .run_drains
            .insert(thread_id.into(), admission.drain());
        let cancel = if let Some(cancel) = ordinary_cancel {
            if let Some(active) = self.agent_controller.active.get_mut(thread_id) {
                active.cancel = cancel.clone();
            }
            cancel
        } else {
            cancel
        };
        self.bind_question_attention(&stream, cx);
        crate::automation_controller::ensure_global(cx);
        let claim = match &run {
            PendingAgentRun::Automation(claim) => Some(claim.as_ref().clone()),
            _ => None,
        };
        cx.update_global::<crate::automation_controller::AutomationGlobal, _>(|global, _| {
            global.runs.insert(
                thread_id.to_string(),
                crate::automation_controller::AutomationRunOwner {
                    started_at: None,
                    generation,
                    owner: Arc::downgrade(&self.window_alive),
                    claim,
                    cancel: cancel.clone(),
                },
            );
        });
        let pending_id = self.agent_controller.queue_dispatches.remove(thread_id);
        let queue_delivery = pending_id.as_ref().and_then(|id| {
            stream
                .read(cx)
                .pending_queue()
                .messages
                .iter()
                .find(|message| &message.id == id)
                .cloned()
        });
        let queue_context = self
            .agent_controller
            .active
            .get_mut(thread_id)
            .map(|active| {
                active.composer_prediction_config_generation =
                    self.composer_prediction_config_generation;
                active.queue_delivery = queue_delivery;
                PendingRunContext {
                    generation: active.queue_generation.clone(),
                    message_id: pending_id,
                }
            });

        #[cfg(test)]
        let (run, admission, cancel, queue_context) = match self.agent_test_launches.as_mut() {
            Some(launches) => {
                launches.push(AgentTestLaunch {
                    run,
                    admission,
                    cancel,
                    queue_context,
                });
                return;
            }
            None => (run, admission, cancel, queue_context),
        };
        vega_ui::sidebar::set_thread_running(thread_id, true, cx);
        self.agent_controller.preparation_stream = None;
        self.begin_context_primary_owner(generation, cx);
        stream.update(cx, ConversationStream::begin_composer_run);
        self.ensure_agent_artifact_route(&thread, &stream, cx);
        self.begin_artifact_agent_generation(generation, &stream);

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
        let config_path = match &run {
            PendingAgentRun::UserMessage(submission) => submission
                .ordinary
                .as_ref()
                .and_then(|launch| launch.run())
                .and_then(|run| crate::ordinary_submission::context(run.submission.id()))
                .map(|context| context.config)
                .unwrap_or_else(|| self.composer_config_path()),
            _ => self.composer_config_path(),
        };
        let mcp_settings = self.mcp_settings.clone();
        let file_read_state = self
            .agent_controller
            .file_read_states
            .entry(thread_id.to_string())
            .or_default()
            .clone();

        let worker_activity = vega_ui::sidebar::register_project_worker(&thread.project_id, cx);
        let replay_registry = cx
            .try_global::<crate::app_agent::AppReplaySessions>()
            .map(|sessions| sessions.0.clone());
        let automation_pauses: Vec<_> = stream
            .read(cx)
            .automation_projection()
            .tasks
            .iter()
            .filter(|task| {
                cx.global::<crate::automation_controller::AutomationGlobal>()
                    .revoked
                    .contains(&(thread_id.to_string(), task.kind))
            })
            .cloned()
            .collect();
        let worker = std::thread::Builder::new()
            .name("vega-agent".into())
            .spawn(move || {
                run_agent_worker_with_mcp(
                    database_path,
                    project_path,
                    thread,
                    run,
                    permission_queue,
                    user_questions,
                    cancel,
                    worker_sender,
                    pricing_catalog,
                    config_path,
                    reasoning,
                    Some(title_sender),
                    mcp_settings,
                    file_read_state,
                    queue_context,
                    automation_pauses,
                    replay_registry,
                    admission,
                );
                drop(worker_activity);
            });
        if worker.is_err() {
            stream.update(cx, |stream, cx| stream.finish_composer_run(false, cx));
            self.poison_artifact_agent_generation(generation, &stream);
            self.detach_finished_computer_use(generation, thread_id, &stream, cx);
            let failed_run = self.agent_controller.finish(generation, thread_id, &stream);
            // A spawn failure releases the run it just registered; the row
            // must not keep spinning for a worker that never started.
            vega_ui::sidebar::set_thread_running(thread_id, false, cx);
            cx.update_global::<crate::automation_controller::AutomationGlobal, _>(|global, _| {
                global.finish_run(thread_id, generation, &self.window_alive);
            });
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
            let mut control_status = None;
            let mut control_check = std::time::Instant::now();
            loop {
                cx.background_executor().timer(AGENT_EVENT_POLL).await;
                let batch = drain_agent_updates(&receiver);
                let keep_running = this
                    .update(cx, |this, cx| {
                        if control_check.elapsed() >= std::time::Duration::from_millis(50) {
                            control_check = std::time::Instant::now();
                            let latest = this
                                .agent_controller
                                .active
                                .get(&thread_id)
                                .filter(|active| active.generation == generation)
                                .and_then(|active| active.computer_use.as_ref())
                                .map(|control| {
                                    let status = control.status();
                                    (
                                        status.phase,
                                        status.epoch,
                                        status.target_label,
                                        status.browser_target,
                                        status.error,
                                    )
                                });
                            if latest != control_status {
                                control_status = latest;
                                stream.update(cx, |_, cx| cx.notify());
                                cx.notify();
                            }
                        }
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
                        if cx
                            .try_global::<crate::automation_controller::AutomationGlobal>()
                            .is_some()
                        {
                            cx.update_global::<crate::automation_controller::AutomationGlobal, _>(
                                |global, _| {
                                    global.finish_run(&thread_id, generation, &this.window_alive);
                                },
                            );
                        }
                        let cancelled = finished_run.cancel.is_cancelled();
                        if this.owns_stream_request(&stream, &thread_id, cx) {
                            this.refresh_context_projection(false, cx);
                        }
                        let cancelled = stream
                            .update(cx, |stream, cx| stream.finish_composer_run(cancelled, cx));
                        let mut queue_ready = success
                            && !cancelled
                            && finished_run.queue_ready
                            && !stream.read(cx).permission_queue().has_pending()
                            && !stream.read(cx).has_pending_user_question();
                        let ActiveAgentRun {
                            composer_prediction,
                            composer_prediction_config_generation,
                            pending_user_content: pending_user,
                            pending_approved_instruction,
                            terminal_failure,
                            terminal_message_id,
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
                        let mut display_stream = stream.clone();
                        if let Ok(refresh) = refresh {
                            queue_ready &= !refresh
                                .plans
                                .iter()
                                .any(|plan| plan.status == PlanStatus::Pending);
                            display_stream = if let Some(current_stream) =
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
                            display_stream.update(cx, |stream, cx| {
                                if let Some(failure) = terminal_failure {
                                    stream.apply_agent_runtime_error_for(
                                        terminal_message_id.as_deref(),
                                        failure,
                                        cx,
                                    );
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
                        let queue_ready =
                            queue_ready && !recovery_projected && pending_review.is_none();
                        if let Some(pending) = pending_review {
                            this.finish_owned_plan_review(pending.stream, &pending.request, cx);
                        }
                        if queue_ready
                            && composer_prediction_config_generation
                                == this.composer_prediction_config_generation
                            && let Some(request) = composer_prediction
                            && let Some(message_id) = terminal_message_id.as_deref()
                        {
                            this.prepare_composer_prediction(
                                &display_stream,
                                request,
                                message_id,
                                cx,
                            );
                        }
                        this.finish_pending_run(&thread_id, &display_stream, queue_ready, cx);
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
        if self
            .agent_controller
            .ordinary_requests
            .contains(&request.request_id)
        {
            return;
        }
        if crate::updater::installing() {
            stream.update(cx, ConversationStream::apply_agent_busy);
            return;
        }
        if (request.content.is_empty() && request.images.is_empty())
            || !self.owns_stream_request(&stream, &request.thread_id, cx)
        {
            return;
        }
        crate::automation_controller::ensure_global(cx);
        cx.update_global::<crate::automation_controller::AutomationGlobal, _>(|global, _| {
            global.register_stream(&request.thread_id, &self.window_alive, &stream);
        });
        if self.intercept_automation(stream.clone(), request, cx) {
            return;
        }
        if stream.read(cx).task_backend() == TaskBackend::Native
            && request.pending_behavior.is_some()
        {
            self.enqueue_composer(stream, request, cx);
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
        if stream.read(cx).task_backend() == TaskBackend::Codex {
            self.submit_codex_composer(stream, request, cx);
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

        if self.trusted_actions.is_busy()
            || self.reasoning_save_pending.is_some()
            || self.model_catalog_loading
            || stream.read(cx).has_pending_model_selection()
            || stream.read(cx).reasoning_unavailable()
        {
            stream.update(cx, ConversationStream::apply_agent_busy);
            return;
        }
        self.submit_ordinary(stream, request, reasoning, cx);
    }

    pub(crate) fn submit_codex_composer(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerSubmitted,
        cx: &mut Context<Self>,
    ) {
        if !request.images.is_empty() {
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream
                    .apply_codex_route_error("Codex ACP 当前仅支持文本输入；移除附件后可重试", cx);
            });
            return;
        }
        if request.skill_intent.is_some() {
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_codex_route_error("Codex ACP 暂不支持 Vega Skills", cx);
            });
            return;
        }
        if cx.global::<SettingsOpen>().0 || !self.is_draft_route(&request.thread_id) {
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_codex_route_error("Codex ACP 本版本只支持新建任务", cx);
            });
            return;
        }
        if self.trusted_actions.is_busy() {
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_agent_busy(cx);
            });
            return;
        }
        let Some(draft) = self.draft_for_route(&request.thread_id) else {
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_codex_route_error("Codex ACP 本版本只支持新建任务", cx);
            });
            return;
        };
        let Some(database_path) = self.file_backed_store_path(cx) else {
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_controller_error(cx);
            });
            return;
        };
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
        let content = request.content.clone();
        let thread_id = request.thread_id.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-codex-preflight".into())
            .spawn(move || {
                let outcome = crate::codex_agent::prepare_codex_execution(
                    config_path.as_deref(),
                    &database_path,
                    &draft,
                );
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
                        Err(crate::codex_agent::CodexPreflightFailure::ProfileUnavailable)
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_codex_preflight(
                        stream.clone(),
                        thread_id.clone(),
                        content.clone(),
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

    fn restore_codex_setup_draft(
        &mut self,
        failed_thread: &Thread,
        failed_stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        if self.draft.is_some()
            || !cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.id == failed_thread.id)
            || self
                .current_cached_stream_for_thread(&failed_thread.id, cx)
                .as_ref()
                != Some(failed_stream)
        {
            return;
        }
        let input = failed_stream.read(cx).composer_input();
        let content = input.read(cx).text().to_owned();
        let mut draft = vega_conversation::threads::draft_thread(
            Some(&failed_thread.project_id),
            &failed_thread.model,
            failed_thread.permission_mode.as_str(),
        );
        draft.backend = TaskBackend::Codex;
        let project_id = draft.project_id.clone();
        let stream = cx.new(|cx| ConversationStream::new(draft.clone(), cx));
        let label = self
            .sidebar
            .read(cx)
            .project_label(&project_id, cx)
            .unwrap_or_default();
        stream.update(cx, |stream, cx| {
            stream.set_draft_route(true, cx);
            stream.set_project_label(label, cx);
            stream
                .composer_input()
                .update(cx, |input, cx| input.set_text(&content, cx));
            stream.apply_codex_route_error(
                "Codex session setup failed before the prompt; your text is ready in a new Codex draft. Check Settings → Agents, then retry.",
                cx,
            );
        });
        self.draft = Some(draft.clone());
        self.stream_view = Some((draft.id.clone(), stream));
        cx.set_global(SelectedProject(Some(project_id)));
        cx.set_global(OpenedThread(Some(draft)));
        self.sync_navigation(cx);
    }

    fn finish_codex_preflight(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: String,
        content: String,
        lease: TrustedActionToken,
        outcome: Result<CodexExecutionSnapshot, crate::codex_agent::CodexPreflightFailure>,
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
        let snapshot = match outcome {
            Ok(snapshot) => snapshot,
            Err(failure) => {
                stream.update(cx, |stream, cx| {
                    stream.reject_composer_submission(cx);
                    stream.apply_codex_route_error(failure.message(), cx);
                });
                return;
            }
        };
        let Some(draft) = self.draft_for_route(&thread_id) else {
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_codex_route_error("Codex ACP 本版本只支持新建任务", cx);
            });
            return;
        };
        let materialized = match &cx.global::<VegaStore>().0 {
            Ok(store) => {
                vega_conversation::codex_tasks::materialize_codex_draft(store, &draft, &snapshot)
                    .map_err(|_| ())
            }
            Err(_) => Err(()),
        };
        let Ok(thread) = materialized else {
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_controller_error(cx);
            });
            return;
        };
        self.draft = None;
        let intent = match &cx.global::<VegaStore>().0 {
            Ok(store) => {
                vega_conversation::codex_tasks::begin_codex_session_creation(store, &thread.id)
            }
            Err(_) => Err(vega_conversation::types::ConversationError::Store(
                "Codex session intent unavailable".into(),
            )),
        };
        let Ok(intent) = intent else {
            self.draft = None;
            stream.update(cx, |stream, cx| {
                stream.set_draft_route(false, cx);
                stream.apply_thread(thread.clone(), cx);
            });
            cx.set_global(OpenedThread(Some(thread.clone())));
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_controller_error(cx);
            });
            return;
        };
        let Some(database_path) = self.file_backed_store_path(cx) else {
            if let Ok(store) = &cx.global::<VegaStore>().0
                && let CodexSessionCreationState::Intent { intent_id } = &intent
            {
                let _ = vega_conversation::codex_tasks::mark_codex_session_definitively_failed(
                    store,
                    &thread.id,
                    intent_id,
                    CodexSessionFailureCode::ProcessStartFailed,
                );
            }
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_controller_error(cx);
            });
            return;
        };
        let permission_queue = stream.read(cx).permission_queue();
        let Some((generation, cancel)) = self.agent_controller.begin(
            thread_id.clone(),
            stream.clone(),
            Some(content.clone()),
            None,
        ) else {
            if let Ok(store) = &cx.global::<VegaStore>().0
                && let CodexSessionCreationState::Intent { intent_id } = &intent
            {
                let _ = vega_conversation::codex_tasks::mark_codex_session_definitively_failed(
                    store,
                    &thread.id,
                    intent_id,
                    CodexSessionFailureCode::ProcessStartFailed,
                );
            }
            self.draft = None;
            stream.update(cx, |stream, cx| {
                stream.set_draft_route(false, cx);
                stream.apply_thread(thread.clone(), cx);
                stream.reject_composer_submission(cx);
                stream.apply_agent_busy(cx);
            });
            cx.set_global(OpenedThread(Some(thread.clone())));
            return;
        };
        self.bind_question_attention(&stream, cx);
        stream.update(cx, |stream, cx| stream.set_draft_route(false, cx));
        vega_ui::sidebar::set_thread_running(&thread_id, true, cx);
        self.begin_context_primary_owner(generation, cx);
        stream.update(cx, ConversationStream::begin_composer_run);
        self.ensure_agent_artifact_route(&thread, &stream, cx);
        self.begin_artifact_agent_generation(generation, &stream);
        let worker_activity = vega_ui::sidebar::register_project_worker(&thread.project_id, cx);
        let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
        let worker_request = crate::codex_agent::CodexWorkerRequest {
            database_path,
            thread: thread.clone(),
            snapshot,
            intent,
            prompt: content,
            permission_queue,
            cancel,
            sender: sender.clone(),
        };
        let intent_id_for_failure = match &worker_request.intent {
            CodexSessionCreationState::Intent { intent_id } => Some(intent_id.clone()),
            _ => None,
        };
        let worker_sender = sender.clone();
        let worker = std::thread::Builder::new()
            .name("vega-codex-agent".into())
            .spawn(move || {
                let success = crate::codex_agent::run_codex_agent_worker(worker_request);
                let _ = worker_sender.send(AgentUpdate::Finished {
                    success,
                    reference_failure: None,
                    credential_failure: false,
                });
                drop(worker_activity);
            });
        if worker.is_err() {
            if let Ok(store) = &cx.global::<VegaStore>().0
                && let Some(intent_id) = intent_id_for_failure.as_ref()
            {
                let _ = vega_conversation::codex_tasks::mark_codex_session_definitively_failed(
                    store,
                    &thread.id,
                    intent_id,
                    CodexSessionFailureCode::ProcessStartFailed,
                );
            }
            let _ = sender.send(AgentUpdate::Finished {
                success: false,
                reference_failure: None,
                credential_failure: false,
            });
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
                        let cancelled = stream
                            .update(cx, |stream, cx| stream.finish_composer_run(cancelled, cx));
                        let recoverable_codex_submission = if !success && !cancelled {
                            match &cx.global::<VegaStore>().0 {
                                Ok(store) => vega_conversation::codex_tasks::codex_task_identity(
                                    store, &thread_id,
                                )
                                .ok()
                                .flatten()
                                .filter(|identity| {
                                    matches!(
                                        identity.session_creation,
                                        CodexSessionCreationState::DefinitivelyFailed { .. }
                                    )
                                })
                                .and(finished_run.pending_user_content.as_ref().map(|_| ())),
                                Err(_) => None,
                            }
                        } else {
                            None
                        };
                        if finished_run.pending_user_content.is_some() {
                            stream.update(cx, ConversationStream::reject_composer_submission);
                        }
                        let refresh = match &cx.global::<VegaStore>().0 {
                            Ok(store) => reload_thread_state(store, &thread_id),
                            Err(error) => Err(error.clone()),
                        };
                        let mut authoritative_thread = None;
                        let mut display_stream = stream.clone();
                        if let Ok(refresh) = refresh {
                            authoritative_thread = Some(refresh.thread.clone());
                            if let Some(current_stream) =
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
                                display_stream = current_stream;
                            } else {
                                Self::apply_stream_state(
                                    &stream,
                                    refresh.thread,
                                    refresh.plans,
                                    refresh.history,
                                    cx,
                                );
                            }
                        }
                        if credential_failure {
                            display_stream.update(cx, ConversationStream::apply_credential_error);
                        } else if !success && !cancelled {
                            if let Some(failure) = finished_run.terminal_failure {
                                display_stream.update(cx, |stream, cx| {
                                    stream.apply_agent_runtime_error_for(
                                        finished_run.terminal_message_id.as_deref(),
                                        failure,
                                        cx,
                                    )
                                });
                            } else {
                                display_stream.update(cx, ConversationStream::apply_agent_error);
                            }
                        }
                        if reference_failure.is_some() {
                            display_stream.update(cx, ConversationStream::apply_agent_error);
                        }
                        this.finish_context_primary_owner(generation);
                        this.agent_controller.pending_review.remove(&thread_id);
                        vega_ui::sidebar::set_thread_running(&thread_id, false, cx);
                        this.resume_deferred_message_location(&thread_id, cx);
                        if let (Some(()), Some(thread)) =
                            (recoverable_codex_submission, authoritative_thread)
                        {
                            this.restore_codex_setup_draft(&thread, &stream, cx);
                        }
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
}

impl VegaWindow {
    pub(crate) fn resume_credential_continuation(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &vega_ui::conversation_stream::CredentialContinuationRequested,
        cx: &mut Context<Self>,
    ) {
        self.start_agent_run(
            stream,
            &request.thread_id,
            PendingAgentRun::CredentialContinuation(request.message_id.clone()),
            cx,
        );
    }
}
