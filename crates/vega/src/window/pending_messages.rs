use super::*;

impl VegaWindow {
    pub(super) fn clear_pending_title_wait(&mut self, thread_id: &str, cx: &mut App) -> bool {
        let Some(wait) = self.agent_controller.queue_drain_waits.remove(thread_id) else {
            return false;
        };
        wait.stream.update(cx, |stream, cx| {
            stream.clear_pending_queue_wait(wait.generation, cx)
        });
        true
    }

    fn queue_database(&self, cx: &App) -> Option<PathBuf> {
        cx.global::<VegaStore>()
            .0
            .as_ref()
            .ok()?
            .database_path()
            .map(std::path::Path::to_path_buf)
    }

    pub(super) fn project_pending_queue(
        &mut self,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        mut projection: PendingQueueProjection,
        cx: &mut Context<Self>,
    ) {
        let error = projection.error.as_deref().map(|error| {
            [
                "ui.ordinary_send.queue_occupied",
                "ui.ordinary_send.queue_finishing",
            ]
            .into_iter()
            .find(|key| *key == error || vega_i18n::t(key) == error)
            .map(vega_ui::localized_copy::LocalizedCopy::new)
            .unwrap_or_else(|| crate::app_localization::localized_error(error))
        });
        let delivered = &self.agent_controller.queue_delivered;
        projection
            .messages
            .retain(|message| !delivered.contains(&message.id));
        if self
            .agent_controller
            .queue_drain_waits
            .get(thread_id)
            .is_some_and(|wait| {
                projection.paused
                    || wait.stream != *stream
                    || projection.messages.first().is_none_or(|message| {
                        message.id != wait.message_id
                            || message.generation != wait.message_generation
                    })
            })
        {
            self.clear_pending_title_wait(thread_id, cx);
        }
        projection.committed_generation = self
            .agent_controller
            .active
            .get(thread_id)
            .filter(|active| !active.cancel.is_cancelled())
            .map(|active| active.queue_generation.clone());
        if projection.messages.is_empty() && !projection.busy {
            self.agent_controller.queue_streams.remove(thread_id);
        } else {
            self.agent_controller
                .queue_streams
                .insert(thread_id.into(), stream.clone());
        }
        stream.update(cx, |stream, cx| {
            stream.apply_pending_queue_localized(projection, error, cx)
        });
    }

    fn reject_pending_queue_action(
        &mut self,
        stream: &Entity<ConversationStream>,
        thread_id: &str,
        error: &str,
        cx: &mut Context<Self>,
    ) {
        let mut projection = stream.read(cx).pending_queue().clone();
        projection.busy = self
            .agent_controller
            .queue_operations
            .contains_key(thread_id);
        projection.error = Some(error.into());
        self.project_pending_queue(thread_id, stream, projection, cx);
    }

    pub(crate) fn load_pending_queue(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: String,
        cx: &mut Context<Self>,
    ) {
        if stream.read(cx).task_backend() != TaskBackend::Native {
            return;
        }
        self.bind_automation(stream.clone(), thread_id.clone(), cx);
        let Some(database) = self.queue_database(cx) else {
            return;
        };
        let Some(operation) = self.agent_controller.begin_queue_operation(&thread_id) else {
            return;
        };
        let running = self.agent_controller.active.contains_key(&thread_id);
        let mut projection = stream.read(cx).pending_queue().clone();
        projection.busy = true;
        self.project_pending_queue(&thread_id, &stream, projection, cx);
        let worker_thread = thread_id.clone();
        let cached = stream.read(cx).pending_queue().messages.clone();
        let worker = cx.background_executor().spawn(async move {
            let store = vega_store::Store::open(database).map_err(|_| ())?;
            if running {
                vega_conversation::pending_messages::load_cached(&store, &worker_thread, &cached)
                    .map_err(|_| ())
            } else {
                vega_conversation::pending_messages::recover_cached(&store, &worker_thread, &cached)
                    .map_err(|_| ())
            }
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !this
                    .agent_controller
                    .finish_queue_operation(&thread_id, operation)
                {
                    return;
                }
                let mut projection = stream.read(cx).pending_queue().clone();
                projection.busy = false;
                projection.paused |= !running;
                match result {
                    Ok(messages) => {
                        projection.messages = messages;
                        projection.error = None;
                    }
                    Err(()) => {
                        projection.paused = true;
                        projection.error = Some(vega_i18n::t("app.pending.read_reopen").into());
                    }
                }
                this.project_pending_queue(&thread_id, &stream, projection, cx);
                if running {
                    this.settle_pending_operation(&thread_id, &stream, cx);
                }
            });
        })
        .detach();
    }

    pub(crate) fn enqueue_composer(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerSubmitted,
        cx: &mut Context<Self>,
    ) {
        let Some(behavior) = request.pending_behavior else {
            return;
        };
        if !self.owns_stream_request(&stream, &request.thread_id, cx)
            || stream.read(cx).task_backend() != TaskBackend::Native
        {
            return;
        }
        let Some(database) = self.queue_database(cx) else {
            stream.update(cx, |stream, cx| {
                stream.reject_composer_submission(cx);
                stream.apply_controller_error(cx);
            });
            return;
        };
        let thread_id = request.thread_id.clone();
        let Some(operation) = self.agent_controller.begin_queue_operation(&thread_id) else {
            stream.update(cx, ConversationStream::reject_composer_submission);
            return;
        };
        let active = self.agent_controller.active.get(&thread_id);
        let generation = active
            .map(|active| active.queue_generation.clone())
            .unwrap_or_default();
        let paused = if stream.read(cx).pending_queue().messages.is_empty() {
            active
                .map(|active| active.cancel.is_cancelled())
                .unwrap_or_else(|| {
                    !self
                        .agent_controller
                        .queue_continue
                        .get(&thread_id)
                        .copied()
                        .unwrap_or(false)
                })
        } else {
            stream.read(cx).pending_queue().paused
        };
        let message = PendingMessage {
            id: ulid::Ulid::generate().to_string(),
            thread_id: thread_id.clone(),
            generation,
            behavior,
            content: request.content.clone(),
            images: request.images.clone(),
        };
        let content = message.content.clone();
        let mut projection = stream.read(cx).pending_queue().clone();
        projection.busy = true;
        projection.paused = paused;
        projection.error = None;
        self.project_pending_queue(&thread_id, &stream, projection, cx);
        let cached = stream.read(cx).pending_queue().messages.clone();
        let config_path = self.composer_config_path();
        let model = stream.read(cx).displayed_model().to_owned();
        let mcp_settings = self.mcp_settings.clone();
        let worker = cx.background_executor().spawn(async move {
            let mut credentials = mcp_settings
                .as_ref()
                .map(|service| service.current_owner_credential_values())
                .transpose()
                .map_err(|_| vega_i18n::t("app.pending.credentials_read").to_string())?
                .unwrap_or_default();

            let path = config_path
                .as_deref()
                .ok_or_else(|| vega_i18n::t("app.error.provider_required").to_string())?;
            let config = vega_store::config::read_from(path)
                .map_err(|_| vega_i18n::t("app.error.config_read").to_string())?;
            let provider = unique_provider_for_model(&config, &model)
                .ok_or_else(|| vega_i18n::t("app.error.provider_model").to_string())?;
            let root = path
                .parent()
                .ok_or_else(|| vega_i18n::t("app.error.config_read").to_string())?;
            credentials.push(
                vega_store::keystore::get_key(root, &provider.key_ref)
                    .map_err(|_| vega_i18n::t("app.error.credentials_read").to_string())?,
            );

            if vega_runtime::contains_sensitive_credential(&message.content, &credentials) {
                return Err(vega_i18n::t("app.pending.credentials_in_message").into());
            }
            let store = vega_store::Store::open(database)
                .map_err(|_| vega_i18n::t("app.pending.save_failed").to_string())?;
            vega_conversation::pending_messages::enqueue_cached(&store, &message, &cached)
                .map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !this
                    .agent_controller
                    .finish_queue_operation(&thread_id, operation)
                {
                    return;
                }
                let mut projection = stream.read(cx).pending_queue().clone();
                projection.busy = false;
                match result {
                    Ok(messages) => {
                        projection.messages = messages;
                        if this
                            .agent_controller
                            .active
                            .get(&thread_id)
                            .is_some_and(|active| active.cancel.is_cancelled())
                        {
                            projection.paused = true;
                        } else if !this.agent_controller.active.contains_key(&thread_id) {
                            projection.paused |= !this
                                .agent_controller
                                .queue_continue
                                .get(&thread_id)
                                .copied()
                                .unwrap_or(false);
                        }
                        stream.update(cx, |stream, cx| {
                            stream.accept_queued_submission(&content, cx)
                        });
                    }
                    Err(error) => {
                        projection.paused = true;
                        projection.error = Some(error);
                        stream.update(cx, ConversationStream::reject_composer_submission);
                    }
                }
                this.project_pending_queue(&thread_id, &stream, projection, cx);
                this.settle_pending_operation(&thread_id, &stream, cx);
            });
        })
        .detach();
    }

    pub(crate) fn pending_queue_requested(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &PendingQueueRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx)
            || stream.read(cx).task_backend() != TaskBackend::Native
        {
            return;
        }
        if matches!(request.action, PendingQueueAction::ToggleDefault) {
            self.toggle_running_message_default(stream, &request.thread_id, cx);
            return;
        }
        let Some(database) = self.queue_database(cx) else {
            self.reject_pending_queue_action(
                &stream,
                &request.thread_id,
                vega_i18n::t("app.pending.unavailable"),
                cx,
            );
            return;
        };
        let generation = self
            .agent_controller
            .active
            .get(&request.thread_id)
            .filter(|active| !active.cancel.is_cancelled())
            .map(|active| active.queue_generation.clone());
        let request = request.clone();
        let thread_id = request.thread_id.clone();
        let resume = matches!(request.action, PendingQueueAction::Resume);
        let steer_id = match &request.action {
            PendingQueueAction::Steer(id) => Some(id.clone()),
            _ => None,
        };
        if steer_id.is_some() && stream.read(cx).has_pending_user_question() {
            self.reject_pending_queue_action(
                &stream,
                &thread_id,
                vega_i18n::t("app.pending.answer_first"),
                cx,
            );
            return;
        }
        let promoted_generation = generation.clone();
        if resume && self.agent_controller.active.contains_key(&thread_id) {
            self.reject_pending_queue_action(
                &stream,
                &thread_id,
                vega_i18n::t("app.pending.running"),
                cx,
            );
            return;
        }
        let Some(operation) = self.agent_controller.begin_queue_operation(&thread_id) else {
            return;
        };
        let mut projection = stream.read(cx).pending_queue().clone();
        projection.busy = true;
        projection.error = None;
        self.project_pending_queue(&thread_id, &stream, projection, cx);
        let edit = matches!(request.action, PendingQueueAction::Edit(_));
        let cached = stream.read(cx).pending_queue().messages.clone();
        let worker = cx.background_executor().spawn(async move {
            let mut removed = None;
            let result = (|| {
                let store = vega_store::Store::open(database)
                    .map_err(|_| vega_i18n::t("app.pending.save_failed").to_string())?;
                match request.action {
                    PendingQueueAction::Remove(id) | PendingQueueAction::Edit(id) => {
                        removed = Some(
                            vega_conversation::pending_messages::remove(
                                &store,
                                &request.thread_id,
                                &id,
                            )
                            .map_err(|error| error.to_string())?,
                        );
                    }
                    PendingQueueAction::Steer(id) => {
                        let generation = generation
                            .ok_or_else(|| vega_i18n::t("app.pending.finished").to_string())?;
                        vega_conversation::pending_messages::promote(
                            &store,
                            &request.thread_id,
                            &id,
                            &generation,
                        )
                        .map_err(|error| error.to_string())?;
                    }
                    PendingQueueAction::Resume => {
                        if vega_conversation::plans::list_plans(&store, &request.thread_id)
                            .map_err(|_| vega_i18n::t("app.pending.plan_read").to_string())?
                            .iter()
                            .any(|plan| plan.status == PlanStatus::Pending)
                            || vega_conversation::plans::recoverable_approved_instruction(
                                &store,
                                &request.thread_id,
                            )
                            .map_err(|_| vega_i18n::t("app.pending.state_read").to_string())?
                            .is_some()
                        {
                            return Err(vega_i18n::t("app.pending.resolve_plan").into());
                        }
                        vega_conversation::pending_messages::recover_cached(
                            &store,
                            &request.thread_id,
                            &cached,
                        )
                        .map_err(|error| error.to_string())?;
                    }
                    PendingQueueAction::ToggleDefault => {}
                }
                vega_conversation::pending_messages::load_cached(
                    &store,
                    &request.thread_id,
                    &cached,
                )
                .map_err(|error| error.to_string())
            })();
            (removed, result)
        });
        cx.spawn(async move |this, cx| {
            let (removed, result) = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !this
                    .agent_controller
                    .finish_queue_operation(&thread_id, operation)
                {
                    return;
                }
                let mut projection = stream.read(cx).pending_queue().clone();
                projection.busy = false;
                if let Some(message) = removed {
                    projection
                        .messages
                        .retain(|pending| pending.id != message.id);
                    if edit {
                        stream.update(cx, |stream, cx| stream.accept_pending_edit(message, cx));
                    }
                }
                match result {
                    Ok(messages) => {
                        if let (Some(pending_id), Some(generation)) =
                            (steer_id.as_deref(), promoted_generation.as_deref())
                        {
                            stream.update(cx, |stream, cx| {
                                stream.apply_pending_promotion(pending_id, generation, cx)
                            });
                        }
                        projection.messages = messages;
                        projection.error = None;
                        if resume {
                            projection.paused = false;
                            this.agent_controller
                                .queue_continue
                                .insert(thread_id.clone(), true);
                        }
                    }
                    Err(error) => projection.error = Some(error),
                }
                this.project_pending_queue(&thread_id, &stream, projection, cx);
                this.settle_pending_operation(&thread_id, &stream, cx);
            });
        })
        .detach();
    }

    fn settle_pending_operation(
        &mut self,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        if !self.agent_controller.active.contains_key(thread_id) {
            let can_continue = self
                .agent_controller
                .queue_continue
                .get(thread_id)
                .copied()
                .unwrap_or(false);
            self.finish_pending_run(thread_id, stream, can_continue, cx);
        }
    }

    pub(crate) fn finish_pending_run(
        &mut self,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        can_continue: bool,
        cx: &mut Context<Self>,
    ) {
        self.agent_controller
            .queue_continue
            .insert(thread_id.into(), can_continue);
        let mut projection = stream.read(cx).pending_queue().clone();
        projection.paused |= !can_continue;
        self.project_pending_queue(thread_id, stream, projection, cx);
        if self.agent_controller.active.contains_key(thread_id) {
            return;
        }
        let Some(database) = self.queue_database(cx) else {
            return;
        };
        let Some(operation) = self.agent_controller.begin_queue_operation(thread_id) else {
            return;
        };
        let thread_id = thread_id.to_string();
        let worker_thread = thread_id.clone();
        let stream = stream.clone();
        let mut projection = stream.read(cx).pending_queue().clone();
        projection.busy = true;
        self.project_pending_queue(&thread_id, &stream, projection, cx);
        let cached = stream.read(cx).pending_queue().messages.clone();
        let worker = cx.background_executor().spawn(async move {
            let store = vega_store::Store::open(database).map_err(|_| ())?;
            vega_conversation::pending_messages::recover_cached(&store, &worker_thread, &cached)
                .map_err(|_| ())
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !this
                    .agent_controller
                    .finish_queue_operation(&thread_id, operation)
                {
                    return;
                }
                let released = result.is_ok();
                let mut projection = stream.read(cx).pending_queue().clone();
                projection.busy = false;
                match result {
                    Ok(messages) => projection.messages = messages,
                    Err(()) => {
                        projection.paused = true;
                        projection.error = Some(vega_i18n::t("app.pending.read_failed").into());
                    }
                }
                this.project_pending_queue(&thread_id, &stream, projection, cx);
                this.continue_pending_queue(&thread_id, &stream, cx);
                this.start_pending_composer_prediction(&thread_id, &stream, cx);
                if !this
                    .agent_controller
                    .queue_operations
                    .contains_key(&thread_id)
                    && let Some(replies) =
                        this.agent_controller.subagent_releases.remove(&thread_id)
                {
                    for reply in replies {
                        let result = if released {
                            Ok(())
                        } else {
                            Err(vega_runtime::VegaError::Cancelled)
                        };
                        let _ = reply.send(result);
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn continue_pending_queue(
        &mut self,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        if self
            .agent_controller
            .queue_drain_waits
            .get(thread_id)
            .is_some_and(|wait| wait.cancel.is_cancelled())
        {
            self.clear_pending_title_wait(thread_id, cx);
        }
        let projection = stream.read(cx).pending_queue();
        if projection.paused
            || projection.busy
            || self.agent_controller.active.contains_key(thread_id)
            || self
                .agent_controller
                .queue_operations
                .contains_key(thread_id)
            || self
                .agent_controller
                .queue_dispatches
                .contains_key(thread_id)
            || stream.read(cx).permission_queue().has_pending()
            || stream.read(cx).has_pending_user_question()
        {
            return;
        }
        let Some(message) = projection.messages.first().cloned() else {
            return;
        };
        if self
            .agent_controller
            .queue_drain_waits
            .contains_key(thread_id)
        {
            return;
        }
        let database = match &cx.global::<VegaStore>().0 {
            Ok(store) => store.database_identity(),
            Err(_) => return,
        };
        let admission = match queue_start_admission(
            database,
            thread_id,
            self.agent_controller.run_drains.get(thread_id),
        ) {
            Ok(QueueStartAdmission::Ready(owner)) => owner,
            Ok(QueueStartAdmission::TitleDrain(drain)) => {
                self.wait_for_pending_title_drain(thread_id, stream, message, drain, cx);
                return;
            }
            Ok(QueueStartAdmission::Blocked) | Err(_) => {
                let mut projection = stream.read(cx).pending_queue().clone();
                projection.paused = true;
                projection.error = Some(vega_i18n::t("ui.ordinary_send.queue_occupied").into());
                self.project_pending_queue(thread_id, stream, projection, cx);
                return;
            }
        };
        self.agent_controller
            .queue_dispatches
            .insert(thread_id.into(), message.id.clone());
        self.start_agent_run_with_admission(
            stream.clone(),
            thread_id,
            PendingAgentRun::UserMessage(UserSubmission {
                content: message.content,
                images: message.images,
                ordinary: None,
            }),
            Some(admission),
            cx,
        );
        if !self.agent_controller.active.contains_key(thread_id) {
            self.agent_controller.queue_dispatches.remove(thread_id);
            let mut projection = stream.read(cx).pending_queue().clone();
            projection.paused = true;
            self.project_pending_queue(thread_id, stream, projection, cx);
        }
    }

    fn wait_for_pending_title_drain(
        &mut self,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        message: PendingMessage,
        drain: vega_store::thread_admission::AdmissionDrain,
        cx: &mut Context<Self>,
    ) {
        let Some((generation, cancel)) =
            self.agent_controller
                .begin_queue_drain_wait(thread_id, stream.clone(), &message)
        else {
            return;
        };
        stream.update(cx, |stream, cx| {
            stream.set_pending_queue_wait(generation, cancel.clone(), cx)
        });
        let thread_id = thread_id.to_owned();
        let stream = stream.clone();
        let started = cx.background_executor().now();
        cx.spawn(async move |this, cx| {
            loop {
                if cancel.is_cancelled() {
                    let _ = this.update(cx, |this, cx| {
                        if this
                            .agent_controller
                            .queue_drain_waits
                            .get(&thread_id)
                            .is_some_and(|wait| wait.generation == generation)
                        {
                            this.clear_pending_title_wait(&thread_id, cx);
                        }
                    });
                    return;
                }
                let released = drain.is_released();
                let expired = cx
                    .background_executor()
                    .now()
                    .saturating_duration_since(started)
                    >= std::time::Duration::from_secs(30);
                if released || expired {
                    let _ = this.update(cx, |this, cx| {
                        let valid = this
                            .agent_controller
                            .queue_drain_waits
                            .get(&thread_id)
                            .is_some_and(|wait| {
                                wait.generation == generation
                                    && wait.stream == stream
                                    && wait.message_id == message.id
                                    && wait.message_generation == message.generation
                                    && !wait.cancel.is_cancelled()
                            });
                        if !valid {
                            return;
                        }
                        this.clear_pending_title_wait(&thread_id, cx);
                        let projection = stream.read(cx).pending_queue();
                        if this.agent_controller.queue_streams.get(&thread_id) != Some(&stream)
                            || projection.messages.first().is_none_or(|head| {
                                head.id != message.id || head.generation != message.generation
                            })
                            || !this
                                .agent_controller
                                .queue_continue
                                .get(&thread_id)
                                .copied()
                                .unwrap_or(false)
                            || projection.paused
                            || projection.busy
                            || this.agent_controller.active.contains_key(&thread_id)
                            || this
                                .agent_controller
                                .queue_operations
                                .contains_key(&thread_id)
                            || stream.read(cx).permission_queue().has_pending()
                            || stream.read(cx).has_pending_user_question()
                        {
                            return;
                        }
                        if released {
                            this.continue_pending_queue(&thread_id, &stream, cx);
                        } else {
                            let mut projection = stream.read(cx).pending_queue().clone();
                            projection.paused = true;
                            projection.error =
                                Some(vega_i18n::t("ui.ordinary_send.queue_finishing").into());
                            this.project_pending_queue(&thread_id, &stream, projection, cx);
                        }
                    });
                    return;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(10))
                    .await;
            }
        })
        .detach();
    }

    pub(crate) fn owns_pending_dispatch(
        &self,
        stream: &Entity<ConversationStream>,
        thread_id: &str,
        cx: &App,
    ) -> bool {
        self.agent_controller.queue_streams.get(thread_id) == Some(stream)
            && self
                .agent_controller
                .queue_dispatches
                .get(thread_id)
                .is_some_and(|id| {
                    stream
                        .read(cx)
                        .pending_queue()
                        .messages
                        .first()
                        .is_some_and(|message| &message.id == id && message.thread_id == thread_id)
                })
    }

    fn toggle_running_message_default(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.composer_config_path() else {
            self.reject_pending_queue_action(
                &stream,
                thread_id,
                vega_i18n::t("app.pending.default_save"),
                cx,
            );
            return;
        };
        let thread_id = thread_id.to_string();
        let Some(operation) = self.agent_controller.begin_queue_operation(&thread_id) else {
            return;
        };
        let mut projection = stream.read(cx).pending_queue().clone();
        projection.busy = true;
        self.project_pending_queue(&thread_id, &stream, projection, cx);
        self.agent_controller.running_message_default_generation = self
            .agent_controller
            .running_message_default_generation
            .saturating_add(1);
        let worker = cx.background_executor().spawn(async move {
            let mut edit = vega_store::config::begin_edit(&path).map_err(|_| ())?;
            edit.config.agent.running_message_behavior =
                if edit.config.agent.running_message_behavior == "steer" {
                    "queue"
                } else {
                    "steer"
                }
                .into();
            edit.save().map_err(|_| ())?;
            Ok::<_, ()>(edit.config.agent.running_message_behavior)
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !this
                    .agent_controller
                    .finish_queue_operation(&thread_id, operation)
                {
                    return;
                }
                let mut projection = stream.read(cx).pending_queue().clone();
                projection.busy = false;
                match result {
                    Ok(value) => {
                        cx.set_global(RunningMessageDefault(if value == "steer" {
                            PendingMessageBehavior::Steer
                        } else {
                            PendingMessageBehavior::Queue
                        }));
                        this.on_settings_saved(false, cx);
                    }
                    Err(()) => {
                        projection.error = Some(vega_i18n::t("app.pending.default_save").into())
                    }
                }
                this.project_pending_queue(&thread_id, &stream, projection, cx);
                this.settle_pending_operation(&thread_id, &stream, cx);
            });
        })
        .detach();
    }

    pub(crate) fn load_running_message_default(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.composer_config_path() else {
            return;
        };
        self.agent_controller.running_message_default_generation = self
            .agent_controller
            .running_message_default_generation
            .saturating_add(1);
        let generation = self.agent_controller.running_message_default_generation;
        let worker = cx.background_executor().spawn(async move {
            vega_store::config::read_from(&path)
                .map(|config| {
                    (
                        config.agent.running_message_behavior,
                        config.agent.composer_predictions_enabled,
                    )
                })
                .ok()
        });
        cx.spawn(async move |this, cx| {
            if let Some((value, predictions_enabled)) = worker.await {
                let _ = this.update(cx, |this, cx| {
                    if this.agent_controller.running_message_default_generation != generation {
                        return;
                    }
                    set_composer_predictions_enabled(predictions_enabled, cx);
                    cx.set_global(RunningMessageDefault(if value == "steer" {
                        PendingMessageBehavior::Steer
                    } else {
                        PendingMessageBehavior::Queue
                    }));
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
