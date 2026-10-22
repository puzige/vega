use super::*;
use vega_ui::artifact_card::ArtifactCard;

impl VegaWindow {
    pub(crate) fn artifact_route_is_current(identity: &ArtifactRouteIdentity, cx: &App) -> bool {
        !cx.global::<SettingsOpen>().0
            && cx
                .global::<vega_ui::sidebar::SelectedProject>()
                .0
                .as_deref()
                == Some(identity.project_id.as_str())
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| {
                    thread.id == identity.thread_id && thread.project_id == identity.project_id
                })
    }

    pub(crate) fn close_artifact_route(
        &mut self,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) {
        let retain = code == GitWorkspaceErrorCode::StaleGeneration
            && self
                .artifact_controller
                .active
                .as_ref()
                .is_some_and(|route| {
                    route.agent_generation.is_some_and(|generation| {
                        self.agent_controller.matches(
                            generation,
                            &route.identity.thread_id,
                            &route.identity.stream,
                        )
                    }) || route.terminal_in_flight.is_some()
                        || !route.terminal_queue.is_empty()
                });
        if retain {
            if let Some(mut route) = self.artifact_controller.active.take() {
                Self::cancel_artifact_interactions(&mut route, cx);
                for key in route.cards.keys() {
                    self.workspace.close(&workspace::TabKey::Artifact(*key));
                }
                self.artifact_controller
                    .retained
                    .insert(route.identity.epoch, route);
            }
            return;
        }
        if let Some(active) = self.artifact_controller.close() {
            for key in active.cards.keys() {
                self.workspace.close(&workspace::TabKey::Artifact(*key));
            }
            for card in active.cards.into_values() {
                card.update(cx, |card, cx| card.invalidate(code, cx));
            }
            cx.notify();
        }
    }

    pub(crate) fn close_artifact_if_route_stale(&mut self, cx: &mut Context<Self>) {
        let stale = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| !Self::artifact_route_is_current(&active.identity, cx));
        if stale {
            self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        }
    }

    pub(crate) fn artifact_project_root(
        thread: &Thread,
        cx: &App,
    ) -> Result<PathBuf, GitWorkspaceErrorCode> {
        if thread.is_standalone() {
            return Err(GitWorkspaceErrorCode::InvalidRoot);
        }
        let store = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .map_err(|_| GitWorkspaceErrorCode::InvalidRoot)?;
        let project = vega_store::projects::find(store.conn(), &thread.project_id)
            .map_err(|_| GitWorkspaceErrorCode::InvalidRoot)?
            .ok_or(GitWorkspaceErrorCode::InvalidRoot)?;
        Ok(PathBuf::from(project.path))
    }

    pub(crate) fn enqueue_artifact_workspace_reconcile(
        &mut self,
        project_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.artifact_controller.active.as_mut() else {
            return;
        };
        if active.identity.project_id != project_id
            || !Self::artifact_route_is_current(&active.identity, cx)
        {
            return;
        }
        Self::cancel_artifact_interactions(active, cx);
        let Some(sequence) = active.terminal_sequence.checked_add(1) else {
            self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
            return;
        };
        if active.terminal_queue.len() >= ARTIFACT_ROUTE_CAP {
            self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
            return;
        }
        active.terminal_sequence = sequence;
        active.terminal_queue.push_back(ArtifactTerminalJob {
            sequence,
            work: ArtifactTerminalWork::Refresh,
        });
        self.launch_next_artifact_terminal(cx);
    }

    pub(crate) fn workspace_action_finished(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let diff_identity = self
            .diff_controller
            .active
            .as_ref()
            .filter(|active| active.identity.project_id == project_id)
            .map(|active| active.identity.clone());
        if let Some(identity) = diff_identity {
            self.schedule_diff_refresh(&identity, cx);
        }
        self.enqueue_artifact_workspace_reconcile(project_id, cx);
    }

    pub(crate) fn ensure_artifact_route(
        &mut self,
        thread: &Thread,
        stream: Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        if thread.is_standalone() {
            self.close_artifact_route(GitWorkspaceErrorCode::InvalidRoot, cx);
            return;
        }
        let current = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.identity.thread_id == thread.id
                    && active.identity.project_id == thread.project_id
                    && active.identity.stream == stream
            });
        if current {
            return;
        }
        self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        let retained_epoch = self
            .artifact_controller
            .retained
            .iter()
            .find(|(_, route)| {
                route.identity.thread_id == thread.id && route.identity.stream == stream
            })
            .map(|(epoch, _)| *epoch);
        if let Some(epoch) = retained_epoch {
            self.artifact_controller.active = self.artifact_controller.retained.remove(&epoch);
            return;
        }
        let result = Self::artifact_project_root(thread, cx).and_then(|root| {
            self.artifact_controller
                .begin(thread, stream, root)
                .map(|_| ())
        });
        if result.is_err() {
            self.close_artifact_route(GitWorkspaceErrorCode::InvalidRoot, cx);
        }
    }

    /// Background approved continuations need capture authority without changing the visible route.
    pub(crate) fn ensure_agent_artifact_route(
        &mut self,
        thread: &Thread,
        stream: &Entity<ConversationStream>,
        cx: &App,
    ) {
        if thread.is_standalone()
            || self
                .artifact_controller
                .active
                .iter()
                .chain(self.artifact_controller.retained.values())
                .any(|route| {
                    route.identity.thread_id == thread.id && route.identity.stream == *stream
                })
        {
            return;
        }
        let Ok(root) = Self::artifact_project_root(thread, cx) else {
            return;
        };
        let mut owner = ArtifactController {
            next_route_epoch: self.artifact_controller.next_route_epoch,
            ..Default::default()
        };
        if owner.begin(thread, stream.clone(), root).is_ok()
            && let Some(route) = owner.active.take()
        {
            self.artifact_controller.next_route_epoch = owner.next_route_epoch;
            self.artifact_controller
                .retained
                .insert(route.identity.epoch, route);
        }
    }

    pub(crate) fn begin_artifact_agent_generation(
        &mut self,
        generation: u64,
        stream: &Entity<ConversationStream>,
    ) {
        if let Some(active) = self
            .artifact_controller
            .active
            .iter_mut()
            .chain(self.artifact_controller.retained.values_mut())
            .find(|route| route.identity.stream == *stream)
        {
            active.proposals.clear();
            active.agent_generation = Some(generation);
        }
    }

    pub(crate) fn poison_artifact_agent_generation(
        &mut self,
        generation: u64,
        stream: &Entity<ConversationStream>,
    ) {
        if let Some(identity) = self.artifact_controller.agent_route(generation, stream)
            && let Some(active) = self.artifact_controller.route_mut(&identity)
        {
            active.proposals.clear();
            active.agent_generation = None;
        }
        self.artifact_controller.retire_drained();
    }

    /// Applies one drained batch through the production ownership boundary.
    /// The caller remains responsible for finished-run reload and UI recovery.
    pub(crate) fn apply_agent_batch_ingress(
        &mut self,
        generation: u64,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        batch: AgentBatch,
        cx: &mut Context<Self>,
    ) -> AgentBatchIngress {
        if !self.agent_controller.matches(generation, thread_id, stream) {
            return AgentBatchIngress::Stale;
        }
        if let Some(request) = batch.composer_prediction
            && let Some(active) = self.agent_controller.active.get_mut(thread_id)
        {
            active.composer_prediction = Some(request);
        }
        if let Some((control, owner)) = batch.computer_use_ready
            && let Some(active) = self.agent_controller.active.get_mut(thread_id)
        {
            if active.cancel.is_cancelled() {
                control.stop();
            }
            active.computer_use = Some(control);
            active.computer_use_owner = owner;
            cx.notify();
        }
        if batch.codex_mode_confirmed {
            stream.update(cx, ConversationStream::confirm_codex_mode);
        }
        if !batch.mcp_unavailable.is_empty()
            && let Some(active) = self.agent_controller.active.get_mut(thread_id)
        {
            active.mcp_unavailable.extend(batch.mcp_unavailable);
            stream.update(cx, |stream, cx| {
                stream.apply_mcp_unavailable(&active.mcp_unavailable, cx)
            });
        }
        let mut refresh_sidebar = false;
        for event in batch.events {
            if let ConversationEvent::RunStarted { started_at, .. } = &event {
                self.observe_automation_clock(thread_id, generation, *started_at, cx);
            }
            if matches!(
                event,
                ConversationEvent::ContextCompactionUsageUpdated { .. }
            ) && !self.owns_primary_context_event(generation, stream, cx)
            {
                continue;
            }
            if let ConversationEvent::ContextCompactionStatus { record } = event {
                self.project_automatic_context(generation, stream, record, cx);
                continue;
            }
            if let ConversationEvent::ContextAccounting { message_id, record } = event {
                if self.owns_primary_context_event(generation, stream, cx) {
                    stream.update(cx, |stream, cx| {
                        let model = stream.displayed_model().to_string();
                        stream.apply_context_accounting(thread_id, &model, &message_id, record, cx);
                    });
                }
                continue;
            }
            self.observe_artifact_event(generation, stream, &event, cx);
            if let ConversationEvent::PendingMessageDelivered { pending_id, .. } = &event {
                self.agent_controller
                    .queue_delivered
                    .insert(pending_id.clone());
            }
            if let ConversationEvent::MessageStarted { message_id, .. } = &event {
                refresh_sidebar |= self
                    .agent_controller
                    .active
                    .get(thread_id)
                    .is_some_and(|active| !active.message_ids.contains(message_id));
            }
            if let ConversationEvent::MessageStarted { seq, .. } = &event
                && let Some(content) = self
                    .agent_controller
                    .accept_durable_start(generation, thread_id, stream)
            {
                let delivery = self
                    .agent_controller
                    .active
                    .get_mut(thread_id)
                    .and_then(|active| active.queue_delivery.take());
                if let Some(message) = delivery {
                    self.agent_controller
                        .queue_delivered
                        .insert(message.id.clone());
                    stream.update(cx, |stream, cx| {
                        stream.apply_event(
                            ConversationEvent::PendingMessageDelivered {
                                pending_id: message.id.clone(),
                                message_id: message.id,
                                seq: seq.saturating_sub(1),
                                content: message.content,
                                images: message.images,
                            },
                            cx,
                        )
                    });
                } else {
                    stream.update(cx, |stream, cx| {
                        stream.accept_composer_submission(&content, cx)
                    });
                }
            }
            self.agent_controller
                .observe_terminal_message(generation, thread_id, stream, &event);
            stream.update(cx, |stream, cx| stream.apply_event(event, cx));
        }
        if refresh_sidebar {
            self.sidebar.update(cx, Sidebar::reload_sessions);
        }
        for update in batch.subagents {
            self.apply_subagent_update(thread_id, stream, update, cx);
        }
        if let Some(active) = self.agent_controller.active.get(thread_id) {
            let control = active.computer_use.clone();
            stream.update(cx, |stream, cx| {
                stream.set_computer_use_control(control, cx)
            });
        }
        let Some(success) = batch.finished else {
            return AgentBatchIngress::Running;
        };
        let reference_failure = batch.reference_failure;
        self.poison_artifact_agent_generation(generation, stream);
        let Some(run) = self.agent_controller.finish(generation, thread_id, stream) else {
            return AgentBatchIngress::Stale;
        };

        vega_ui::sidebar::set_thread_running(thread_id, false, cx);
        self.finish_context_primary_owner(generation);
        self.finish_completion_attention(&run, success, cx);

        if let Some(message_id) = &run.terminal_message_id {
            let duration_ms = u64::try_from(run.started.elapsed().as_millis()).ok();
            let projected = match &cx.global::<VegaStore>().0 {
                Ok(store) => vega_conversation::summary::task_cost_summary(
                    store,
                    thread_id,
                    message_id,
                    duration_ms,
                )
                .ok(),
                Err(_) => None,
            };
            if let Some(summary) = projected {
                stream.update(cx, |stream, cx| stream.apply_task_summary(summary, cx));
            }
        }
        AgentBatchIngress::Finished {
            success,
            run: Box::new(run),
            reference_failure,
            credential_failure: batch.credential_failure,
        }
    }

    pub(crate) fn cancel_artifact_interactions(active: &mut ActiveArtifactRoute, cx: &mut App) {
        let preview_card = active.preview_fence.take().map(|fence| fence.card_id);
        let open_card = active.open_fence.take().map(|fence| fence.card_id);
        if let Some(cancel) = active.preview_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = active.open_cancel.take() {
            cancel.cancel();
        }
        for card_id in [preview_card, open_card].into_iter().flatten() {
            if let Some(card) = active.cards.get(&card_id) {
                card.update(cx, |card, cx| {
                    card.fail_request(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
            }
        }
    }

    /// The only production artifact capture ingress: real AgentBatch events
    /// are observed here before ownership moves to ConversationStream.
    pub(crate) fn observe_artifact_event(
        &mut self,
        generation: u64,
        stream: &Entity<ConversationStream>,
        event: &ConversationEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(identity) = self.artifact_controller.agent_route(generation, stream) else {
            return;
        };
        let Some(active) = self.artifact_controller.route_mut(&identity) else {
            return;
        };
        match event {
            ConversationEvent::ToolCallProposed { call }
                if matches!(call.tool.as_str(), "Write" | "Edit" | "write" | "edit") =>
            {
                let owner = stream.update(cx, |stream, _| {
                    stream
                        .register_artifact_call(&call.id)
                        .then(|| stream.artifact_capture_owner(&call.id))
                        .flatten()
                });
                if let Err(failure) = ArtifactService::validate_proposal(call) {
                    stream.update(cx, |stream, cx| {
                        stream.apply_artifact_capture_error(&call.id, failure.code(), cx)
                    });
                    self.fail_artifact_capture_route(&identity, failure.code(), cx);
                    return;
                }
                if let Some(existing) = active.proposals.get_mut(&call.id) {
                    if existing.generation != generation
                        || existing.call.as_ref() != Some(call)
                        || existing.owner != owner
                    {
                        existing.call = None;
                        existing.owner = None;
                    }
                } else if active.proposals.len() < ARTIFACT_ROUTE_CAP {
                    active.proposals.insert(
                        call.id.clone(),
                        ArtifactProposal {
                            generation,
                            call: Some(call.clone()),
                            owner,
                        },
                    );
                } else {
                    stream.update(cx, |stream, cx| {
                        stream.apply_artifact_capture_error(
                            &call.id,
                            GitWorkspaceErrorCode::ArtifactLimit,
                            cx,
                        )
                    });
                    self.fail_artifact_capture_route(
                        &identity,
                        GitWorkspaceErrorCode::ArtifactLimit,
                        cx,
                    );
                }
            }
            ConversationEvent::ToolCallFinished { call_id, result } => {
                Self::cancel_artifact_interactions(active, cx);
                let call = active
                    .proposals
                    .remove(call_id)
                    .filter(|proposal| proposal.generation == generation)
                    .and_then(|proposal| Some((proposal.call?, proposal.owner?)))
                    .filter(|(_, owner)| {
                        stream
                            .read(cx)
                            .matches_artifact_capture_owner(call_id, owner)
                    });
                let work = if let Some((call, owner)) = call {
                    match active.service.prepare_capture(&call, result) {
                        Ok(Some(candidate)) => ArtifactTerminalWork::Capture {
                            call_id: call_id.clone(),
                            candidate,
                            owner,
                        },
                        Ok(None) => ArtifactTerminalWork::Refresh,
                        Err(failure) => {
                            let code = failure.code();
                            stream.update(cx, |stream, cx| {
                                stream.apply_artifact_capture_error(call_id, code, cx)
                            });
                            self.fail_artifact_capture_route(&identity, code, cx);
                            return;
                        }
                    }
                } else {
                    ArtifactTerminalWork::Refresh
                };
                let Some(sequence) = active.terminal_sequence.checked_add(1) else {
                    stream.update(cx, |stream, cx| {
                        stream.apply_artifact_capture_error(
                            call_id,
                            GitWorkspaceErrorCode::ArtifactLimit,
                            cx,
                        )
                    });
                    self.fail_artifact_capture_route(
                        &identity,
                        GitWorkspaceErrorCode::ArtifactLimit,
                        cx,
                    );
                    return;
                };
                if active.terminal_queue.len() >= ARTIFACT_ROUTE_CAP {
                    stream.update(cx, |stream, cx| {
                        stream.apply_artifact_capture_error(
                            call_id,
                            GitWorkspaceErrorCode::ArtifactLimit,
                            cx,
                        )
                    });
                    self.fail_artifact_capture_route(
                        &identity,
                        GitWorkspaceErrorCode::ArtifactLimit,
                        cx,
                    );
                    return;
                }
                active.terminal_sequence = sequence;
                active
                    .terminal_queue
                    .push_back(ArtifactTerminalJob { sequence, work });
                self.launch_artifact_terminal_for(&identity, cx);
            }
            _ => {}
        }
    }

    fn fail_artifact_capture_route(
        &mut self,
        identity: &ArtifactRouteIdentity,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) {
        if self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|route| route.identity == *identity)
        {
            self.close_artifact_route(code, cx);
        } else if let Some(route) = self.artifact_controller.retained.remove(&identity.epoch) {
            route.cancel.cancel();
            for card in route.cards.into_values() {
                card.update(cx, |card, cx| card.invalidate(code, cx));
            }
        }
    }

    pub(crate) fn launch_next_artifact_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(identity) = self
            .artifact_controller
            .active
            .as_ref()
            .map(|route| route.identity.clone())
        else {
            return;
        };
        self.launch_artifact_terminal_for(&identity, cx);
    }

    fn launch_artifact_terminal_for(
        &mut self,
        identity: &ArtifactRouteIdentity,
        cx: &mut Context<Self>,
    ) {
        let Some(dispatch) = self.take_artifact_terminal_for(identity) else {
            return;
        };
        #[cfg(test)]
        if cx.has_global::<tests::ArtifactWorkerInbox>() {
            cx.update_global::<tests::ArtifactWorkerInbox, _>(|inbox, _| {
                inbox.terminals.push_back(dispatch);
            });
            return;
        }
        let ArtifactTerminalDispatch {
            identity,
            workspace,
            service,
            job,
            cancel,
        } = dispatch;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-artifact-terminal".into())
            .spawn(move || run_artifact_terminal_worker(workspace, service, job, cancel, sender));
        if worker.is_err() {
            self.finish_artifact_terminal(&identity, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let result = match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err(GitWorkspaceErrorCode::SpawnFailed))
                    }
                };
                let Some(result) = result else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_artifact_terminal(&identity, result, cx)
                });
                break;
            }
        })
        .detach();
    }

    fn take_artifact_terminal_for(
        &mut self,
        identity: &ArtifactRouteIdentity,
    ) -> Option<ArtifactTerminalDispatch> {
        let active = self.artifact_controller.route_mut(identity)?;
        if active.terminal_in_flight.is_some() {
            return None;
        }
        let job = active.terminal_queue.pop_front()?;
        active.terminal_in_flight = Some(ArtifactTerminalOwner {
            sequence: job.sequence,
            capture: match &job.work {
                ArtifactTerminalWork::Capture { call_id, owner, .. } => {
                    Some((call_id.clone(), owner.clone()))
                }
                ArtifactTerminalWork::Refresh => None,
            },
        });
        Some(ArtifactTerminalDispatch {
            identity: active.identity.clone(),
            workspace: active.workspace.clone(),
            service: active.service.clone(),
            job,
            cancel: active.cancel.child_token(),
        })
    }

    pub(crate) fn finish_artifact_terminal(
        &mut self,
        identity: &ArtifactRouteIdentity,
        result: Result<(u64, ArtifactTerminalResult), GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.artifact_controller.route(identity) else {
            return;
        };
        let Some(expected) = active.terminal_in_flight.as_ref() else {
            return;
        };
        if active.cancel.is_cancelled()
            || result
                .as_ref()
                .is_ok_and(|(sequence, _)| *sequence != expected.sequence)
        {
            return;
        }
        let capture = expected.capture.clone();
        let stream = active.identity.stream.clone();
        let call_id = capture.as_ref().map(|(call_id, _)| call_id);
        if capture.as_ref().is_some_and(|(call_id, owner)| {
            !stream
                .read(cx)
                .matches_artifact_capture_owner(call_id, owner)
        }) {
            if let Err(
                code @ (GitWorkspaceErrorCode::ArtifactConflict
                | GitWorkspaceErrorCode::ArtifactLimit),
            ) = &result
            {
                self.fail_artifact_capture_route(identity, *code, cx);
                return;
            }
            if let Some(active) = self.artifact_controller.route_mut(identity) {
                active.terminal_in_flight = None;
            }
            self.launch_artifact_terminal_for(identity, cx);
            self.artifact_controller.retire_drained();
            return;
        }
        if let Err(code) = &result {
            if let Some(call_id) = &call_id {
                stream.update(cx, |stream, cx| {
                    stream.apply_artifact_capture_error(call_id, *code, cx)
                });
            }
            if matches!(
                code,
                GitWorkspaceErrorCode::ArtifactConflict | GitWorkspaceErrorCode::ArtifactLimit
            ) {
                self.fail_artifact_capture_route(identity, *code, cx);
                return;
            }
        }
        if let Some(active) = self.artifact_controller.route_mut(identity) {
            active.terminal_in_flight = None;
        }
        if let Ok((_, result)) = result {
            if result
                .captured
                .as_ref()
                .is_some_and(|(captured_call, _)| Some(captured_call) != call_id)
            {
                self.launch_artifact_terminal_for(identity, cx);
                self.artifact_controller.retire_drained();
                return;
            }
            for projection in result.cards {
                let card = self
                    .artifact_controller
                    .route(identity)
                    .and_then(|active| active.cards.get(&projection.id).cloned());
                if let Some(card) = card {
                    card.update(cx, |card, cx| {
                        let _ = card.apply_metadata(projection, cx);
                    });
                }
            }
            if let Some((call_id, projection)) = result.captured {
                let existing = self
                    .artifact_controller
                    .route(identity)
                    .and_then(|active| active.cards.get(&projection.id).cloned());
                if let Some(card) = existing {
                    if stream.update(cx, |stream, cx| {
                        stream.apply_artifact_card(&call_id, card.clone(), cx)
                    }) {
                        card.update(cx, |card, cx| {
                            let _ = card.apply_metadata(projection, cx);
                        });
                    }
                } else {
                    let card = cx.new(|cx| {
                        ArtifactCard::new(
                            identity.thread_id.clone(),
                            identity.project_id.clone(),
                            projection.clone(),
                            cx,
                        )
                    });
                    cx.subscribe(&card, |this, card, request, cx| {
                        this.request_artifact_preview(card.clone(), request, cx);
                    })
                    .detach();
                    cx.subscribe(&card, |this, card, request, cx| {
                        this.request_artifact_open(card.clone(), request, cx);
                    })
                    .detach();
                    cx.subscribe(&card, |this, card, request, cx| {
                        this.clear_artifact_requests(card.clone(), request, cx);
                    })
                    .detach();
                    if stream.update(cx, |stream, cx| {
                        stream.apply_artifact_card(&call_id, card.clone(), cx)
                    }) && let Some(active) = self.artifact_controller.route_mut(identity)
                    {
                        active.cards.insert(projection.id, card);
                    }
                }
            }
        }
        self.launch_artifact_terminal_for(identity, cx);
        self.artifact_controller.retire_drained();
    }

    pub(crate) fn request_artifact_preview(
        &mut self,
        card: Entity<ArtifactCard>,
        request: &ArtifactPreviewRequested,
        cx: &mut Context<Self>,
    ) {
        let route_current = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| Self::artifact_route_is_current(&active.identity, cx));
        if !route_current {
            card.update(cx, |card, cx| {
                card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
            });
            self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
            return;
        }
        if self.trusted_actions.is_busy() {
            card.update(cx, |card, cx| {
                card.fail_request(GitWorkspaceErrorCode::BranchOperationInProgress, cx)
            });
            return;
        }
        let (fence, service, cancel) = {
            let Some(active) = self.artifact_controller.active.as_mut() else {
                card.update(cx, |card, cx| {
                    card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
                return;
            };
            let current = active.identity.thread_id == request.thread_id
                && active.identity.project_id == request.project_id
                && active.cards.get(&request.card_id) == Some(&card)
                && card.read(cx).projection().current_file_id == Some(request.file_id)
                && card.read(cx).projection().preview_available;
            if !current {
                card.update(cx, |card, cx| {
                    card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
                return;
            }
            let Some(sequence) = active.preview_sequence.checked_add(1) else {
                self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
                return;
            };
            active.preview_sequence = sequence;
            if let Some(cancel) = active.preview_cancel.take() {
                cancel.cancel();
            }
            let fence = ArtifactPreviewFence {
                route: active.identity.clone(),
                sequence,
                card_id: request.card_id,
                file_id: request.file_id,
            };
            let cancel = active.cancel.child_token();
            active.preview_fence = Some(fence.clone());
            active.preview_cancel = Some(cancel.clone());
            (fence, active.service.clone(), cancel)
        };
        self.workspace
            .open(workspace::TabKey::Artifact(request.card_id));
        #[cfg(test)]
        if cx.has_global::<tests::ArtifactWorkerInbox>() {
            cx.update_global::<tests::ArtifactWorkerInbox, _>(|inbox, _| {
                inbox.previews.push_back(fence);
            });
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("vega-artifact-preview".into())
            .spawn(move || run_artifact_preview_worker(service, worker_fence, cancel, sender));
        if worker.is_err() {
            self.finish_artifact_preview(fence, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let (fence, result) = match receiver.try_recv() {
                    Ok(output) => output,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        (fence, Err(GitWorkspaceErrorCode::SpawnFailed))
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_artifact_preview(fence, result, cx)
                });
                break;
            }
        })
        .detach();
    }

    pub(crate) fn finish_artifact_preview(
        &mut self,
        fence: ArtifactPreviewFence,
        result: Result<ArtifactPreviewProjection, GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if !Self::artifact_route_is_current(&fence.route, cx) {
            return;
        }
        let card = {
            let Some(active) = self.artifact_controller.active.as_mut() else {
                return;
            };
            if active.preview_fence.as_ref() != Some(&fence) {
                return;
            }
            active.preview_fence = None;
            active.preview_cancel = None;
            active.cards.get(&fence.card_id).cloned()
        };
        let Some(card) = card else {
            return;
        };
        if card.read(cx).projection().current_file_id != Some(fence.file_id) {
            card.update(cx, |card, cx| {
                card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
            });
            return;
        }
        match result {
            Ok(preview) => {
                card.update(cx, |card, cx| {
                    let _ = card.apply_preview(preview, cx);
                });
            }
            Err(GitWorkspaceErrorCode::Cancelled | GitWorkspaceErrorCode::StaleGeneration) => {}
            Err(code) => {
                card.update(cx, |card, cx| {
                    let _ = card.apply_preview_error(fence.card_id, fence.file_id, code, cx);
                });
            }
        }
    }

    pub(crate) fn request_artifact_open(
        &mut self,
        card: Entity<ArtifactCard>,
        request: &ArtifactOpenRequested,
        cx: &mut Context<Self>,
    ) {
        let route_current = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| Self::artifact_route_is_current(&active.identity, cx));
        if !route_current {
            card.update(cx, |card, cx| {
                card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
            });
            self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
            return;
        }
        let sequence_overflow = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.identity.thread_id == request.thread_id
                    && active.identity.project_id == request.project_id
                    && active.cards.get(&request.card_id) == Some(&card)
                    && card.read(cx).projection().current_file_id == Some(request.file_id)
                    && active.open_fence.is_none()
                    && active.open_sequence == u64::MAX
            });
        if sequence_overflow {
            card.update(cx, |card, cx| {
                let _ = card.apply_open_error(
                    request.card_id,
                    request.target,
                    GitWorkspaceErrorCode::ArtifactLimit,
                    cx,
                );
            });
            self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
            return;
        }
        let lease_input = self.artifact_controller.active.as_ref().and_then(|active| {
            let current = active.identity.thread_id == request.thread_id
                && active.identity.project_id == request.project_id
                && active.cards.get(&request.card_id) == Some(&card)
                && card.read(cx).projection().current_file_id == Some(request.file_id)
                && active.open_fence.is_none();
            current
                .then(|| {
                    active
                        .open_sequence
                        .checked_add(1)
                        .map(|sequence| (active.identity.clone(), sequence))
                })
                .flatten()
        });
        let Some((open_identity, open_sequence)) = lease_input else {
            let owned = self
                .artifact_controller
                .active
                .as_ref()
                .is_some_and(|active| {
                    active.identity.thread_id == request.thread_id
                        && active.identity.project_id == request.project_id
                        && active.cards.get(&request.card_id) == Some(&card)
                        && card.read(cx).projection().current_file_id == Some(request.file_id)
                });
            if owned {
                card.update(cx, |card, cx| {
                    card.fail_request(GitWorkspaceErrorCode::BranchOperationInProgress, cx)
                });
            } else {
                card.update(cx, |card, cx| {
                    card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
            }
            return;
        };
        if !self.branch_guards_clear(&open_identity.stream, cx) {
            card.update(cx, |card, cx| {
                card.fail_request(GitWorkspaceErrorCode::BranchOperationInProgress, cx)
            });
            return;
        }
        let Some(lease) = self.trusted_actions.acquire(
            TrustedActionKind::ArtifactOpen,
            open_identity.epoch,
            open_sequence,
        ) else {
            card.update(cx, |card, cx| {
                card.fail_request(GitWorkspaceErrorCode::BranchOperationInProgress, cx)
            });
            return;
        };
        open_identity
            .stream
            .update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        let (fence, service, cancel) = {
            let Some(active) = self.artifact_controller.active.as_mut() else {
                let _ = self.trusted_actions.release(lease);
                open_identity
                    .stream
                    .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                card.update(cx, |card, cx| {
                    card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
                return;
            };
            let current = active.identity.thread_id == request.thread_id
                && active.identity.project_id == request.project_id
                && active.cards.get(&request.card_id) == Some(&card)
                && card.read(cx).projection().current_file_id == Some(request.file_id)
                && active.open_fence.is_none();
            if !current {
                let _ = self.trusted_actions.release(lease);
                open_identity
                    .stream
                    .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                card.update(cx, |card, cx| {
                    card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
                return;
            }
            let sequence = open_sequence;
            active.open_sequence = sequence;
            if let Some(cancel) = active.open_cancel.take() {
                cancel.cancel();
            }
            let fence = ArtifactOpenFence {
                route: active.identity.clone(),
                sequence,
                card_id: request.card_id,
                file_id: request.file_id,
                target: request.target,
                lease,
            };
            let cancel = active.cancel.child_token();
            active.open_fence = Some(fence.clone());
            active.open_cancel = Some(cancel.clone());
            (fence, active.service.clone(), cancel)
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("vega-artifact-open".into())
            .spawn(move || run_artifact_open_worker(service, worker_fence, cancel, sender));
        if worker.is_err() {
            self.finish_artifact_open(fence, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let (fence, result) = match receiver.try_recv() {
                    Ok(output) => output,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        (fence, Err(GitWorkspaceErrorCode::SpawnFailed))
                    }
                };
                let _ = this.update(cx, |this, cx| this.finish_artifact_open(fence, result, cx));
                break;
            }
        })
        .detach();
    }

    pub(crate) fn finish_artifact_open(
        &mut self,
        fence: ArtifactOpenFence,
        result: Result<OpenInOutcome, GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        let released = self.trusted_actions.release(fence.lease);
        if released {
            fence
                .route
                .stream
                .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
        }
        if !Self::artifact_route_is_current(&fence.route, cx) {
            return;
        }
        let card = {
            let Some(active) = self.artifact_controller.active.as_mut() else {
                return;
            };
            if active.open_fence.as_ref() != Some(&fence) {
                return;
            }
            active.open_fence = None;
            active.open_cancel = None;
            active.cards.get(&fence.card_id).cloned()
        };
        let Some(card) = card else {
            return;
        };
        if card.read(cx).projection().current_file_id != Some(fence.file_id) {
            card.update(cx, |card, cx| {
                card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
            });
            return;
        }
        match result {
            Ok(outcome) => {
                card.update(cx, |card, cx| {
                    let _ = card.apply_open_outcome(outcome, cx);
                });
            }
            Err(GitWorkspaceErrorCode::Cancelled | GitWorkspaceErrorCode::StaleGeneration) => {
                card.update(cx, |card, cx| card.set_opening(None, cx));
            }
            Err(code) => {
                card.update(cx, |card, cx| {
                    let _ = card.apply_open_error(fence.card_id, fence.target, code, cx);
                });
            }
        }
    }

    pub(crate) fn clear_artifact_requests(
        &mut self,
        card: Entity<ArtifactCard>,
        request: &ArtifactCleared,
        _cx: &mut Context<Self>,
    ) {
        let Some(active) = self.artifact_controller.active.as_mut() else {
            return;
        };
        if active.identity.thread_id != request.thread_id
            || active.identity.project_id != request.project_id
            || active.cards.get(&request.card_id) != Some(&card)
        {
            return;
        }
        if active
            .preview_fence
            .as_ref()
            .is_some_and(|fence| fence.card_id == request.card_id)
        {
            if let Some(cancel) = active.preview_cancel.take() {
                cancel.cancel();
            }
            active.preview_fence = None;
        }
        if active
            .open_fence
            .as_ref()
            .is_some_and(|fence| fence.card_id == request.card_id)
        {
            if let Some(cancel) = active.open_cancel.take() {
                cancel.cancel();
            }
            active.open_fence = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Bounds, TestAppContext, WindowBounds, WindowHandle, WindowOptions, px, size};
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};
    use std::rc::Rc;
    use vega_conversation::types::ArtifactCard as ArtifactProjection;
    use vega_ui::artifact_card::observation::{
        frame, group_painted, group_texts, items, ordered, painted, texts,
    };

    #[derive(Default)]
    pub(super) struct ArtifactWorkerInbox {
        pub(super) terminals: VecDeque<ArtifactTerminalDispatch>,
        pub(super) previews: VecDeque<ArtifactPreviewFence>,
    }

    impl Global for ArtifactWorkerInbox {}

    struct Fixture {
        owner: Entity<VegaWindow>,
        stream: Entity<ConversationStream>,
        thread: Thread,
        generation: u64,
    }

    fn fixture(cx: &TestAppContext) -> Fixture {
        cx.update(|cx| {
            gpui_kit::init(cx);
            vega_ui::init(cx);
            cx.set_global(vega_theme::Theme::light());
            cx.set_reduce_motion(true);
            let thread = Thread {
                parent_thread_id: None,
                id: "artifact-thread".into(),
                backend: TaskBackend::Native,
                project_id: "artifact-project".into(),
                title: "Artifacts".into(),
                mode: ThreadMode::Execute,
                permission_mode: PermissionMode::Confirm,
                model: "test-model".into(),
                status: ThreadStatus::Active,
                pinned: false,
                unread: false,
                created_at: 0,
                updated_at: 0,
            };
            cx.set_global(SettingsOpen(false));
            cx.set_global(SelectedProject(Some(thread.project_id.clone())));
            cx.set_global(OpenedThread(Some(thread.clone())));
            cx.set_global(VegaStore(Err("artifact tests have no store".into())));
            cx.set_global(ArtifactWorkerInbox::default());
            let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
            let owner = cx.new(VegaWindow::for_artifact_tests);
            let workspace = Arc::new(GitWorkspaceService::for_test());
            let service = Arc::new(
                ArtifactService::new(
                    workspace.clone(),
                    thread.project_id.clone(),
                    thread.id.clone(),
                    1,
                )
                .unwrap(),
            );
            let identity = ArtifactRouteIdentity {
                epoch: 1,
                thread_id: thread.id.clone(),
                project_id: thread.project_id.clone(),
                stream: stream.clone(),
            };
            let generation = owner.update(cx, |owner, _| {
                owner.artifact_controller.active = Some(ActiveArtifactRoute {
                    identity,
                    workspace,
                    service,
                    cancel: tokio_util::sync::CancellationToken::new(),
                    agent_generation: None,
                    proposals: HashMap::new(),
                    terminal_sequence: 0,
                    terminal_in_flight: None,
                    terminal_queue: VecDeque::new(),
                    cards: HashMap::new(),
                    preview_sequence: 0,
                    preview_fence: None,
                    preview_cancel: None,
                    open_sequence: 0,
                    open_fence: None,
                    open_cancel: None,
                });
                let (generation, _) = owner
                    .agent_controller
                    .begin(thread.id.clone(), stream.clone(), None, None)
                    .unwrap();
                owner.begin_artifact_agent_generation(generation, &stream);
                generation
            });
            Fixture {
                owner,
                stream,
                thread,
                generation,
            }
        })
    }

    fn ingress(fixture: &Fixture, events: Vec<ConversationEvent>, cx: &TestAppContext) {
        cx.update(|cx| {
            fixture.owner.update(cx, |owner, cx| {
                let outcome = owner.apply_agent_batch_ingress(
                    fixture.generation,
                    &fixture.thread.id,
                    &fixture.stream,
                    AgentBatch {
                        composer_prediction: None,
                        subagents: Vec::new(),
                        events,
                        codex_mode_confirmed: false,
                        computer_use_ready: None,
                        mcp_unavailable: Vec::new(),
                        reference_failure: None,
                        credential_failure: false,
                        finished: None,
                    },
                    cx,
                );
                assert!(matches!(outcome, AgentBatchIngress::Running));
            });
        });
    }

    fn completed_write(thread: &Thread, call_id: &str) -> (ToolCall, ToolResult) {
        let call = ToolCall {
            id: call_id.into(),
            tool: "Write".into(),
            input_json: vega_tools::WriteEditAudit::Write {
                path: "report.txt".into(),
                content_bytes: 6,
                expected_written_bytes: None,
                fingerprint_v1: "a".repeat(64),
            }
            .to_json()
            .unwrap(),
        };
        let checkpoint_ref =
            vega_tools::CheckpointIds::new(&thread.project_id, &thread.id, &call.id)
                .unwrap()
                .checkpoint_ref();
        let result = ToolResult {
            status: ToolCallStatus::Success,
            output: vega_tools::WriteSuccessOutput {
                path: "report.txt".into(),
                bytes_written: 6,
                checkpoint_ref,
            }
            .to_json()
            .unwrap(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: Some(false),
            invalid: None,
        };
        (call, result)
    }

    fn capture_during_running_turn(cx: &TestAppContext) -> (Fixture, Entity<ArtifactCard>) {
        let fixture = fixture(cx);
        let created = Rc::new(RefCell::new(None));
        let observed = created.clone();
        let _subscription = cx.update(|cx| {
            cx.observe_new::<ArtifactCard>(move |_, _, cx| {
                *observed.borrow_mut() = Some(cx.entity());
            })
        });
        let (call, result) = completed_write(&fixture.thread, "write-report");
        ingress(
            &fixture,
            vec![
                ConversationEvent::MessageStarted {
                    message_id: "turn-a".into(),
                    seq: 1,
                },
                ConversationEvent::TextDelta {
                    message_id: "turn-a".into(),
                    delta: "Answer for turn-a.\n".into(),
                },
                ConversationEvent::ToolCallProposed { call: call.clone() },
                ConversationEvent::ToolCallFinished {
                    call_id: call.id,
                    result,
                },
            ],
            cx,
        );
        let dispatch = cx.update(|cx| {
            cx.update_global::<ArtifactWorkerInbox, _>(|inbox, _| {
                inbox.terminals.pop_front().expect("capture was dispatched")
            })
        });
        let ArtifactTerminalWork::Capture { call_id, .. } = dispatch.job.work else {
            panic!("validated write must dispatch capture work");
        };
        let projection = ArtifactProjection {
            id: ArtifactCardId::for_test(dispatch.identity.epoch, 0),
            label: "report.txt".into(),
            source: ArtifactSource::AgentArtifact,
            current_file_id: Some(WorkspaceFileId::for_test(1, 0)),
            preview_available: true,
            size_bytes: Some(6),
        };
        cx.update(|cx| {
            fixture.owner.update(cx, |owner, cx| {
                owner.finish_artifact_terminal(
                    &dispatch.identity,
                    Ok((
                        dispatch.job.sequence,
                        ArtifactTerminalResult {
                            captured: Some((call_id, projection)),
                            cards: Vec::new(),
                        },
                    )),
                    cx,
                );
            });
        });
        let card = created
            .borrow_mut()
            .take()
            .expect("completion created the artifact entity");
        (fixture, card)
    }

    struct StreamHost {
        stream: Entity<ConversationStream>,
    }

    impl Render for StreamHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.stream.clone())
        }
    }

    fn mount(fixture: &Fixture, cx: &TestAppContext) -> WindowHandle<StreamHost> {
        cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(960.), px(2400.)),
                    ))),
                    ..WindowOptions::default()
                },
                |_, cx| {
                    cx.new(|_| StreamHost {
                        stream: fixture.stream.clone(),
                    })
                },
            )
            .unwrap()
        })
    }

    fn rendered(
        handle: WindowHandle<StreamHost>,
        cx: &mut TestAppContext,
        assertion: impl FnOnce(&Window, &App),
    ) {
        cx.update_window(handle.into(), |_, window, cx| {
            frame(window, cx);
            assert!(
                painted().iter().any(|text| text.scope == "assistant"),
                "real assistant paint must be observed before artifact assertions"
            );
            assertion(window, cx);
        })
        .unwrap();
    }

    fn start_turn(fixture: &Fixture, owner: &str, cx: &TestAppContext) {
        ingress(
            fixture,
            vec![
                ConversationEvent::MessageStarted {
                    message_id: owner.into(),
                    seq: if owner == "turn-a" { 1 } else { 2 },
                },
                ConversationEvent::TextDelta {
                    message_id: owner.into(),
                    delta: format!("Answer for {owner}.\n"),
                },
            ],
            cx,
        );
    }

    fn finish_turn(fixture: &Fixture, owner: &str, cx: &TestAppContext) {
        ingress(
            fixture,
            vec![ConversationEvent::MessageFinished {
                message_id: owner.into(),
                stop_reason: ConversationStopReason::End,
                execution_duration_ms: Some(1),
            }],
            cx,
        );
        cx.update(|cx| {
            fixture.stream.update(cx, |stream, cx| {
                stream.apply_task_summary(
                    TaskCostSummary {
                        message_id: owner.into(),
                        outcome: TaskSummaryOutcome::Completed,
                        usage: None,
                        cost: SummaryCost::Unavailable,
                        duration_ms: Some(1),
                        tool_count: 1,
                        cache_hit_percent: None,
                    },
                    cx,
                );
            })
        });
    }

    fn dispatch_write(
        fixture: &Fixture,
        call_id: &str,
        cx: &TestAppContext,
    ) -> ArtifactTerminalDispatch {
        let (call, result) = completed_write(&fixture.thread, call_id);
        ingress(
            fixture,
            vec![
                ConversationEvent::ToolCallProposed { call },
                ConversationEvent::ToolCallFinished {
                    call_id: call_id.into(),
                    result,
                },
            ],
            cx,
        );
        let dispatch = cx.update(|cx| {
            cx.update_global::<ArtifactWorkerInbox, _>(|inbox, _| {
                inbox
                    .terminals
                    .pop_front()
                    .expect("real ingress dispatched capture work")
            })
        });
        assert!(
            matches!(&dispatch.job.work, ArtifactTerminalWork::Capture { call_id: owner, .. } if owner == call_id)
        );
        dispatch
    }

    fn finish_worker(
        fixture: &Fixture,
        identity: &ArtifactRouteIdentity,
        result: Result<(u64, ArtifactTerminalResult), GitWorkspaceErrorCode>,
        cx: &TestAppContext,
    ) {
        cx.update(|cx| {
            fixture.owner.update(cx, |owner, cx| {
                owner.finish_artifact_terminal(identity, result, cx);
            })
        });
    }

    fn assert_error_only(owner: &str, code: GitWorkspaceErrorCode, cx: &App) {
        let expected = match code {
            GitWorkspaceErrorCode::GitUnavailable => {
                "Git 2.40+ was not found. Install Homebrew Git and retry.".into()
            }
            _ => format!("Artifact unavailable ({})", code.as_str()),
        };
        ordered(owner, 1);
        assert_eq!(group_texts(owner, 1), vec![expected]);
        assert!(
            group_painted(owner, 1)
                .iter()
                .all(|text| text.style.as_ref().unwrap().color
                    == gpui_kit::Hsla::from(theme(cx).colors.danger))
        );
    }

    fn assert_no_artifact_controls(window: &Window) {
        assert!(
            gpui_kit::base::test_support::snapshots(window)
                .iter()
                .all(|snapshot| {
                    !matches!(snapshot.label(), Some("Preview file" | "Open file in"))
                        || !snapshot.visible()
                })
        );
    }

    fn request_preview(card: &Entity<ArtifactCard>, cx: &TestAppContext) -> ArtifactPreviewFence {
        cx.update(|cx| card.update(cx, ArtifactCard::preview));
        cx.update(|cx| {
            cx.update_global::<ArtifactWorkerInbox, _>(|inbox, _| {
                inbox
                    .previews
                    .pop_front()
                    .expect("registered card request reached preview execution")
            })
        })
    }

    fn finish_preview(
        fixture: &Fixture,
        fence: ArtifactPreviewFence,
        result: Result<ArtifactPreviewProjection, GitWorkspaceErrorCode>,
        cx: &TestAppContext,
    ) {
        cx.update(|cx| {
            fixture.owner.update(cx, |owner, cx| {
                owner.finish_artifact_preview(fence, result, cx);
            });
        });
    }

    #[gpui_kit::test]
    fn captured_card_is_previewable_before_the_turn_finishes(cx: &mut TestAppContext) {
        let (fixture, card) = capture_during_running_turn(cx);
        cx.read(|cx| assert_eq!(card.read(cx).row_count(), 2));
        let handle = mount(&fixture, cx);
        rendered(handle, cx, |_, _| {
            assert!(
                painted()
                    .iter()
                    .any(|text| text.scope == "assistant"
                        && text.text.contains("Answer for turn-a.")),
                "assistant paint positive control"
            );
            assert!(items("artifact-group").is_empty());
            assert!(!texts().contains(&"report.txt".into()));
        });
        let fence = request_preview(&card, cx);
        let file_id = fence.file_id;
        let preview = ArtifactPreviewProjection::for_test(
            fence.card_id,
            file_id,
            "alpha\nbeta\ngamma".into(),
        );
        finish_preview(&fixture, fence, Ok(preview), cx);
        cx.read(|cx| {
            assert_eq!(card.read(cx).row_count(), 5);
            assert_eq!(card.read(cx).projection().current_file_id, Some(file_id));
            assert!(card.read(cx).projection().preview_available);
        });
        let fence = request_preview(&card, cx);
        finish_preview(
            &fixture,
            fence,
            Err(GitWorkspaceErrorCode::GitUnsupported),
            cx,
        );
        cx.read(|cx| {
            assert_eq!(card.read(cx).row_count(), 3);
            assert_eq!(card.read(cx).projection().current_file_id, Some(file_id));
            assert!(card.read(cx).projection().preview_available);
        });
        rendered(handle, cx, |_, _| {
            assert!(items("artifact-group").is_empty());
            assert!(!texts().contains(&"report.txt".into()));
        });
        finish_turn(&fixture, "turn-a", cx);
        rendered(handle, cx, |_, _| {
            ordered("turn-a", 2);
            assert!(group_texts("turn-a", 2).contains(&"report.txt".into()));
            assert!(
                group_texts("turn-a", 2)
                    .contains(&"Git 2.40+ is required. Upgrade Git and retry.".into())
            );
        });
    }

    #[gpui_kit::test]
    fn rejected_proposal_before_stream_delivery_keeps_its_turn_error(cx: &mut TestAppContext) {
        let fixture = fixture(cx);
        let handle = mount(&fixture, cx);
        start_turn(&fixture, "turn-a", cx);
        let (mut call, _) = completed_write(&fixture.thread, "oversized-proposal");
        call.input_json = "x".repeat(64 * 1024 + 1);
        assert_eq!(
            ArtifactService::validate_proposal(&call)
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::ArtifactLimit
        );
        let route_cancel = cx.read(|cx| {
            fixture
                .owner
                .read(cx)
                .artifact_controller
                .active
                .as_ref()
                .unwrap()
                .cancel
                .clone()
        });
        ingress(
            &fixture,
            vec![ConversationEvent::ToolCallProposed { call }],
            cx,
        );
        assert!(
            route_cancel.is_cancelled(),
            "proposal limit must still fail the route closed"
        );
        rendered(handle, cx, |_, _| {
            assert!(
                painted()
                    .iter()
                    .any(|text| text.scope == "assistant"
                        && text.text.contains("Answer for turn-a."))
            );
            assert!(items("artifact-group").is_empty());
        });
        finish_turn(&fixture, "turn-a", cx);
        rendered(handle, cx, |window, cx| {
            assert_eq!(items("artifact-group").len(), 1);
            assert_error_only("turn-a", GitWorkspaceErrorCode::ArtifactLimit, cx);
            assert_no_artifact_controls(window);
        });
        start_turn(&fixture, "turn-b", cx);
        finish_turn(&fixture, "turn-b", cx);
        rendered(handle, cx, |window, cx| {
            assert_eq!(items("artifact-group").len(), 1);
            assert_error_only("turn-a", GitWorkspaceErrorCode::ArtifactLimit, cx);
            assert!(
                items("artifact-group")
                    .iter()
                    .all(|item| item.owner.as_deref() != Some("turn-b"))
            );
            assert_no_artifact_controls(window);
        });
    }

    #[gpui_kit::test]
    fn delayed_capture_worker_error_stays_with_the_finished_turn(cx: &mut TestAppContext) {
        let fixture = fixture(cx);
        let handle = mount(&fixture, cx);
        start_turn(&fixture, "turn-a", cx);
        let delayed = dispatch_write(&fixture, "delayed-a", cx);
        finish_turn(&fixture, "turn-a", cx);
        start_turn(&fixture, "turn-b", cx);
        finish_worker(
            &fixture,
            &delayed.identity,
            Err(GitWorkspaceErrorCode::GitUnavailable),
            cx,
        );
        rendered(handle, cx, |window, cx| {
            assert_eq!(items("artifact-group").len(), 1);
            assert_error_only("turn-a", GitWorkspaceErrorCode::GitUnavailable, cx);
            assert_no_artifact_controls(window);
        });
        let current = dispatch_write(&fixture, "current-b", cx);
        finish_worker(
            &fixture,
            &current.identity,
            Err(GitWorkspaceErrorCode::SpawnFailed),
            cx,
        );
        finish_turn(&fixture, "turn-b", cx);
        rendered(handle, cx, |window, cx| {
            assert_eq!(items("artifact-group").len(), 2);
            assert_error_only("turn-a", GitWorkspaceErrorCode::GitUnavailable, cx);
            assert_error_only("turn-b", GitWorkspaceErrorCode::SpawnFailed, cx);
            assert_no_artifact_controls(window);
        });
    }

    #[gpui_kit::test]
    fn unknown_and_stale_capture_completions_do_not_adopt_the_current_turn(
        cx: &mut TestAppContext,
    ) {
        let fixture = fixture(cx);
        let handle = mount(&fixture, cx);
        start_turn(&fixture, "turn-a", cx);
        let delayed = dispatch_write(&fixture, "delayed-a", cx);
        finish_turn(&fixture, "turn-a", cx);
        start_turn(&fixture, "turn-b", cx);
        let mut unknown_route = delayed.identity.clone();
        unknown_route.epoch += 1;
        finish_worker(
            &fixture,
            &unknown_route,
            Err(GitWorkspaceErrorCode::GitUnavailable),
            cx,
        );
        let completion = |call_id: &str, label: &str| ArtifactTerminalResult {
            captured: Some((
                call_id.into(),
                ArtifactProjection {
                    id: ArtifactCardId::for_test(delayed.identity.epoch, 99),
                    label: label.into(),
                    source: ArtifactSource::AgentArtifact,
                    current_file_id: Some(WorkspaceFileId::for_test(1, 99)),
                    preview_available: true,
                    size_bytes: Some(6),
                },
            )),
            cards: Vec::new(),
        };
        finish_worker(
            &fixture,
            &delayed.identity,
            Ok((
                delayed.job.sequence + 1,
                completion("delayed-a", "stale-sequence.txt"),
            )),
            cx,
        );
        rendered(handle, cx, |window, _| {
            assert!(items("artifact-group").is_empty());
            assert!(!texts().contains(&"unowned.txt".into()));
            assert!(!texts().contains(&"stale-sequence.txt".into()));
            assert_no_artifact_controls(window);
        });
        finish_worker(
            &fixture,
            &delayed.identity,
            Ok((
                delayed.job.sequence,
                completion("never-proposed", "unowned.txt"),
            )),
            cx,
        );
        rendered(handle, cx, |window, _| {
            assert!(items("artifact-group").is_empty());
            assert!(!texts().contains(&"unowned.txt".into()));
            assert_no_artifact_controls(window);
        });
        let current = dispatch_write(&fixture, "current-b", cx);
        cx.update(|cx| {
            fixture.owner.update(cx, |owner, cx| {
                owner.close_artifact_route(GitWorkspaceErrorCode::ArtifactConflict, cx);
            })
        });
        assert!(delayed.cancel.is_cancelled());
        assert!(current.cancel.is_cancelled());
        finish_worker(
            &fixture,
            &current.identity,
            Err(GitWorkspaceErrorCode::SpawnFailed),
            cx,
        );
        finish_turn(&fixture, "turn-b", cx);
        rendered(handle, cx, |window, _| {
            assert!(items("artifact-group").is_empty());
            assert!(!texts().contains(&"unowned.txt".into()));
            assert_no_artifact_controls(window);
        });
    }

    #[gpui_kit::test]
    fn cancelled_capture_completion_does_not_create_an_error_row(cx: &mut TestAppContext) {
        let fixture = fixture(cx);
        let handle = mount(&fixture, cx);
        start_turn(&fixture, "turn-a", cx);
        let delayed = dispatch_write(&fixture, "cancelled-a", cx);
        ingress(
            &fixture,
            vec![ConversationEvent::Interrupted {
                message_id: "turn-a".into(),
                execution_duration_ms: Some(1),
            }],
            cx,
        );
        cx.update(|cx| {
            fixture.stream.update(cx, |stream, cx| {
                stream.apply_task_summary(
                    TaskCostSummary {
                        message_id: "turn-a".into(),
                        outcome: TaskSummaryOutcome::Interrupted,
                        usage: None,
                        cost: SummaryCost::Unavailable,
                        duration_ms: Some(1),
                        tool_count: 1,
                        cache_hit_percent: None,
                    },
                    cx,
                )
            })
        });
        start_turn(&fixture, "turn-b", cx);
        finish_worker(
            &fixture,
            &delayed.identity,
            Err(GitWorkspaceErrorCode::Cancelled),
            cx,
        );
        let current = dispatch_write(&fixture, "current-b", cx);
        finish_worker(
            &fixture,
            &current.identity,
            Err(GitWorkspaceErrorCode::SpawnFailed),
            cx,
        );
        finish_turn(&fixture, "turn-b", cx);
        rendered(handle, cx, |window, cx| {
            assert_eq!(items("artifact-group").len(), 1);
            assert_error_only("turn-b", GitWorkspaceErrorCode::SpawnFailed, cx);
            assert!(
                items("artifact-group")
                    .iter()
                    .all(|item| item.owner.as_deref() != Some("turn-a"))
            );
            assert!(!texts().iter().any(|text| text.contains("cancelled")));
            assert_no_artifact_controls(window);
        });
    }

    #[gpui_kit::test]
    fn superseded_preview_completion_cannot_replace_the_current_result(cx: &mut TestAppContext) {
        let (fixture, card) = capture_during_running_turn(cx);
        let first = request_preview(&card, cx);
        let second = request_preview(&card, cx);
        let old_preview = ArtifactPreviewProjection::for_test(
            first.card_id,
            first.file_id,
            "obsolete\ncontent\nwith\nextra\nrows".into(),
        );
        finish_preview(&fixture, first, Ok(old_preview), cx);
        cx.read(|cx| assert_eq!(card.read(cx).row_count(), 2));
        let file_id = second.file_id;
        let current_preview =
            ArtifactPreviewProjection::for_test(second.card_id, file_id, "current\ncontent".into());
        finish_preview(&fixture, second, Ok(current_preview), cx);
        cx.read(|cx| {
            assert_eq!(card.read(cx).row_count(), 4);
            assert_eq!(card.read(cx).projection().current_file_id, Some(file_id));
            assert!(card.read(cx).projection().preview_available);
        });
    }
}
