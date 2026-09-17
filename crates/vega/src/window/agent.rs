use super::*;

/// Poll interval for the provider/credential preflight worker. The worker
/// owns all config and keystore IO; the UI only observes its typed result.
const AGENT_PREFLIGHT_POLL: std::time::Duration = std::time::Duration::from_millis(4);

impl VegaWindow {
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
                    this.finish_history_page(stream.clone(), outcome, cx)
                });
                break;
            }
        })
        .detach();
    }

    /// Applies a finished hydration page to its requesting stream, gated by
    /// the same route fence as the request: only the currently open thread's
    /// cached stream may take a page.
    pub(crate) fn finish_history_page(
        &mut self,
        stream: Entity<ConversationStream>,
        outcome: HistoryPageOutcome,
        cx: &mut Context<Self>,
    ) {
        let Some(opened) = cx.global::<OpenedThread>().0.clone() else {
            return;
        };
        if !self.owns_stream_request(&stream, &opened.id, cx) {
            return;
        }
        stream.update(cx, |stream, cx| match outcome {
            Ok(page) => stream.apply_history_page(page, cx),
            Err(_) => stream.apply_history_load_failed(cx),
        });
    }

    pub(crate) fn apply_refresh(
        stream: &Entity<ConversationStream>,
        thread: Thread,
        plans: Vec<Plan>,
        cx: &mut Context<Self>,
    ) {
        cx.set_global(OpenedThread(Some(thread.clone())));
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
        let pending_review = self.agent_controller.pending_review.take();
        let artifact_run = self
            .agent_controller
            .active
            .as_ref()
            .map(|active| (active.generation, active.stream.clone()));
        if let Some(active) = &self.agent_controller.active {
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
            .as_ref()
            .is_some_and(|active| active.thread_id == request.thread_id && active.stream == stream)
        {
            self.cancel_active_agent(cx);
        }
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

    /// Shows the existing Pricing repair route while preserving the current
    /// authority/draft. Both durable runs and A7 first submits use this one
    /// projection; only the first-submit caller must check before INSERT.
    fn open_pricing_repair(&mut self, code: PricingSettingsErrorCode, cx: &mut Context<Self>) {
        if let PricingControllerState::Ready { error, .. } = &mut self.pricing_controller.state {
            *error = Some(code);
        }
        cx.set_global(vega_ui::settings::PricingSettingsRequested(true));
        cx.set_global(SettingsOpen(true));
        self.push_pricing_projection(cx);
    }

    pub(crate) fn start_agent_run_with_reasoning(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: &str,
        run: PendingAgentRun,
        reasoning: Option<FrozenReasoning>,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, thread_id, cx) {
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
            PendingAgentRun::UserMessage(content) => Some(content.clone()),
            PendingAgentRun::ApprovedPlan(_) => None,
        };
        let pending_approved_instruction = match &run {
            PendingAgentRun::UserMessage(_) => None,
            PendingAgentRun::ApprovedPlan(instruction_id) => Some(instruction_id.clone()),
        };
        if stream.read(cx).has_pending_model_selection()
            || self.agent_controller.active.is_some()
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

        // T37 gate: durable Thread.model must resolve against the app-owned
        // Ready authority before begin, channel/worker spawn, config,
        // Keychain, or provider construction. T39 carries the returned
        // immutable capability into the runtime run (exact pricing for every
        // provider call) and into the Composer meter's provisional estimator.
        let pricing_catalog = match self.pricing_controller.select_exact(&thread.model) {
            Ok(selection) => selection.catalog(),
            Err(code) => {
                if pending_user_content.is_some() {
                    stream.update(cx, ConversationStream::reject_composer_submission);
                }
                if pending_approved_instruction.is_some() {
                    stream.update(cx, ConversationStream::apply_approved_not_started);
                } else {
                    stream.update(cx, ConversationStream::apply_agent_error);
                }
                self.open_pricing_repair(code, cx);
                return;
            }
        };

        let permission_queue = stream.read(cx).permission_queue();
        let (generation, cancel) = self.agent_controller.begin(
            thread_id.to_string(),
            stream.clone(),
            pending_user_content,
            pending_approved_instruction,
        );
        stream.update(cx, ConversationStream::begin_composer_run);
        self.begin_artifact_agent_generation(generation, &stream);
        // S7-T39/C3: the provisional estimator freezes the run-start
        // selection; it never re-reads pricing files or the live authority.
        let meter_estimator = vega_conversation::types::RunUsageEstimator::new(
            &thread.model,
            pricing_catalog.clone(),
        );
        stream.update(cx, |stream, cx| {
            stream.install_meter_estimator(meter_estimator, cx)
        });
        let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
        let worker_sender = sender.clone();
        let config_path = self.composer_config_path();
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
                run_agent_worker(
                    database_path,
                    project_path,
                    thread,
                    run,
                    permission_queue,
                    cancel,
                    worker_sender,
                    Some(pricing_catalog),
                    config_path,
                    reasoning,
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
            let failed_run = self.agent_controller.active.take();
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
                        let cancelled = stream
                            .update(cx, |stream, cx| stream.finish_composer_run(cancelled, cx));
                        let ActiveAgentRun {
                            pending_user_content: pending_user,
                            pending_approved_instruction,
                            terminal_failure,
                            ..
                        } = finished_run;
                        let approved_not_started = pending_approved_instruction.is_some();
                        if pending_user.is_some() {
                            stream.update(cx, ConversationStream::reject_composer_submission);
                        }
                        let pending_review = this.agent_controller.pending_review.take();
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
                        if let Some(pending) = pending_review {
                            this.review_plan(pending.stream, &pending.request, cx);
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

    pub(crate) fn submit_composer(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerSubmitted,
        cx: &mut Context<Self>,
    ) {
        if request.content.is_empty() || !self.owns_stream_request(&stream, &request.thread_id, cx)
        {
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
        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        let config_path = self.composer_config_path();
        let model = stream.read(cx).displayed_model().to_owned();
        let content = request.content.clone();
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
        reasoning: Option<FrozenReasoning>,
        lease: TrustedActionToken,
        outcome: Result<(), ProviderPreflightFailure>,
        cx: &mut Context<Self>,
    ) {
        if !self.trusted_actions.release(lease) {
            return;
        }
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
            // A7-01 rule 8: the durable-thread T37 pricing gate below is too
            // late for a lazy draft. Check its exact model against the same
            // Ready authority before INSERT, so a missing price opens the
            // repair page without creating an empty task or losing the text.
            if let Err(code) = self.pricing_controller.select_exact(&draft.model) {
                stream.update(cx, |stream, cx| {
                    stream.reject_composer_submission(cx);
                    stream.apply_agent_error(cx);
                });
                self.open_pricing_repair(code, cx);
                return;
            }
            if self.materialize_draft(&draft, cx).is_err() {
                stream.update(cx, |stream, cx| {
                    stream.reject_composer_submission(cx);
                    stream.apply_controller_error(cx);
                });
                return;
            }
        }
        self.start_agent_run_with_reasoning(
            stream,
            &thread_id,
            PendingAgentRun::UserMessage(content),
            reasoning,
            cx,
        );
    }
}
