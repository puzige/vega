use super::*;
use crate::ordinary_submission::{OrdinaryLaunch, finish_root};
use vega_conversation::agent::OrdinaryRun;

const ORDINARY_POLL: std::time::Duration = std::time::Duration::from_millis(4);

pub(super) struct OrdinaryPreparationRequest {
    pub(super) registered: OrdinaryWorker,
    pub(super) run: OrdinaryRun,
    database: PathBuf,
    config: Option<PathBuf>,
    draft: Option<Thread>,
    expected: Thread,
    skills: Option<(SkillSettingsService, SkillSelectionIntent)>,
    cancel: tokio_util::sync::CancellationToken,
}

impl OrdinaryPreparationRequest {
    fn prepare(self) -> Result<OrdinaryLaunch, SubmissionFailure> {
        let result = (|| {
            if self.cancel.is_cancelled() {
                return Err(SubmissionFailure::Cancelled);
            }
            preflight_provider(self.config.as_deref(), self.run.submission.raw().model())
                .map_err(|_| SubmissionFailure::PreflightRejected)?;
            if self.cancel.is_cancelled() {
                return Err(SubmissionFailure::Cancelled);
            }
            let store = vega_store::Store::open(&self.database)
                .map_err(|_| SubmissionFailure::PreparationFailed)?;
            if !self.run.worker.admission().verifies(
                store.database_identity(),
                &self.run.submission.snapshot().thread_id,
            ) {
                return Err(SubmissionFailure::PreparationFailed);
            }
            let thread_id = self.run.submission.snapshot().thread_id;
            if let Some(draft) = &self.draft {
                if vega_store::threads::find(store.conn(), &thread_id)
                    .map_err(|_| SubmissionFailure::PreparationFailed)?
                    .is_none()
                {
                    vega_conversation::threads::materialize_draft(&store, draft)
                        .map_err(|_| SubmissionFailure::PreparationFailed)?;
                }
            }
            let thread = vega_conversation::threads::open_thread(&store, &thread_id)
                .map_err(|_| SubmissionFailure::PreparationFailed)?;
            if thread.model != self.run.submission.raw().model()
                || thread.backend != TaskBackend::Native
                || thread.status == ThreadStatus::Archived
                || thread.project_id != self.expected.project_id
                || thread.mode != self.expected.mode
                || thread.permission_mode != self.expected.permission_mode
            {
                return Err(SubmissionFailure::PreparationFailed);
            }
            let workspace = vega_conversation::threads::task_workspace_root(&store, &thread_id)
                .map_err(|_| SubmissionFailure::PreparationFailed)?;
            if let Some((service, intent)) = &self.skills {
                service
                    .ensure_composer_pin_with_owner(&thread_id, intent, self.run.worker.admission())
                    .map_err(|_| SubmissionFailure::PreparationFailed)?;
            }
            if self.cancel.is_cancelled() {
                return Err(SubmissionFailure::Cancelled);
            }
            Ok((workspace, thread))
        })();
        let _ = self
            .registered
            .complete(result.as_ref().map(|_| ()).map_err(|failure| *failure));
        match result {
            Ok((workspace, thread)) => Ok(OrdinaryLaunch::new(
                self.run,
                self.database,
                workspace,
                thread,
                self.cancel,
            )),
            Err(error) => {
                finish_root(self.run, Err(error));
                Err(error)
            }
        }
    }
}

impl VegaWindow {
    pub(crate) fn submit_ordinary(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerSubmitted,
        reasoning: Option<FrozenReasoning>,
        cx: &mut Context<Self>,
    ) {
        let cached = match &cx.global::<VegaStore>().0 {
            Ok(store) => store
                .database_path()
                .map(|path| (store.database_identity(), path.to_path_buf())),
            Err(_) => None,
        };
        let Some((database, _)) = cached else {
            stream.update(cx, ConversationStream::reject_composer_submission);
            stream.update(cx, ConversationStream::apply_controller_error);
            return;
        };
        if self
            .agent_controller
            .ordinary_requests
            .contains(&request.request_id)
        {
            return;
        }
        let admission =
            match vega_store::thread_admission::try_acquire(database, &request.thread_id) {
                Ok(Some(admission)) => admission,
                _ => {
                    stream.update(cx, ConversationStream::reject_composer_submission);
                    stream.update(cx, ConversationStream::apply_agent_busy);
                    return;
                }
            };
        let snapshot = match stream.read(cx).model_snapshot_for_submit() {
            Ok(snapshot) => snapshot,
            Err(_) => {
                stream.update(cx, ConversationStream::reject_composer_submission);
                stream.update(cx, ConversationStream::apply_controller_error);
                return;
            }
        };
        let raw = RawSubmission::new(
            request.content.clone(),
            request.images.clone(),
            request.draft_revision,
            stream.read(cx).displayed_model().to_owned(),
            snapshot,
            reasoning,
        );
        let (submission, worker) =
            match OrdinarySubmission::begin(database, request.thread_id.clone(), raw, admission) {
                Ok(result) => result,
                Err(_) => {
                    stream.update(cx, ConversationStream::reject_composer_submission);
                    stream.update(cx, ConversationStream::apply_agent_busy);
                    return;
                }
            };
        self.agent_controller
            .ordinary_requests
            .insert(request.request_id);
        if !stream.update(cx, |stream, cx| {
            stream.begin_ordinary_pending(submission.clone(), request, cx)
        }) {
            let attempt = worker.attempt_id();
            let _ = worker.complete(Err(SubmissionFailure::Cancelled));
            let _ = submission.seal(attempt);
            return;
        }
        let context = crate::ordinary_submission::OrdinaryContext::new(
            stream.read(cx).thread_snapshot(),
            self.draft_for_route(&request.thread_id),
            request.skill_intent.clone(),
            self.composer_config_path(),
        );
        if crate::ordinary_submission::retain_context(submission.id(), context).is_err() {
            finish_root(
                OrdinaryRun::new(submission, Arc::new(worker)),
                Err(SubmissionFailure::PreparationFailed),
            );
            stream.update(cx, ConversationStream::refresh_ordinary_submissions);
            return;
        }
        self.prepare_ordinary(stream, submission, worker, cx);
    }

    fn prepare_ordinary(
        &mut self,
        stream: Entity<ConversationStream>,
        submission: OrdinarySubmission,
        worker: OrdinaryWorker,
        cx: &mut Context<Self>,
    ) {
        let thread_id = submission.snapshot().thread_id;
        let database = match &cx.global::<VegaStore>().0 {
            Ok(store) => store.database_path().map(PathBuf::from),
            Err(_) => None,
        };
        let Some(database) = database else {
            finish_root(
                OrdinaryRun::new(submission, Arc::new(worker)),
                Err(SubmissionFailure::PreparationFailed),
            );
            return;
        };
        let Some(context) = crate::ordinary_submission::context(submission.id()) else {
            finish_root(
                OrdinaryRun::new(submission, Arc::new(worker)),
                Err(SubmissionFailure::Unknown),
            );
            stream.update(cx, ConversationStream::refresh_ordinary_submissions);
            return;
        };
        let skills = match context.skill.clone() {
            Some(intent) => {
                let service = context
                    .config
                    .as_deref()
                    .and_then(std::path::Path::parent)
                    .map(|root| {
                        SkillSettingsService::new(
                            database.clone(),
                            root.to_path_buf(),
                            context.thread.project_binding().map(str::to_owned),
                        )
                    });
                let Some(service) = service else {
                    finish_root(
                        OrdinaryRun::new(submission, Arc::new(worker)),
                        Err(SubmissionFailure::PreparationFailed),
                    );
                    stream.update(cx, ConversationStream::refresh_ordinary_submissions);
                    return;
                };
                Some((service, intent))
            }
            None => None,
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        if crate::ordinary_submission::set_cancel(
            submission.id(),
            worker.attempt_id(),
            cancel.clone(),
        )
        .is_err()
        {
            finish_root(
                OrdinaryRun::new(submission, Arc::new(worker)),
                Err(SubmissionFailure::Unknown),
            );
            stream.update(cx, ConversationStream::refresh_ordinary_submissions);
            return;
        }
        self.agent_controller
            .ordinary_preparing
            .insert(thread_id.clone(), cancel.clone());
        let registered = match worker.register_worker() {
            Ok(registered) => registered,
            Err(_) => {
                finish_root(
                    OrdinaryRun::new(submission, Arc::new(worker)),
                    Err(SubmissionFailure::PreparationFailed),
                );
                return;
            }
        };
        let Some(lease) = self.trusted_actions.acquire(
            TrustedActionKind::AgentPreflight,
            0,
            worker.attempt_id().number(),
        ) else {
            let _ = registered.complete(Err(SubmissionFailure::LaunchFailed));
            finish_root(
                OrdinaryRun::new(submission, Arc::new(worker)),
                Err(SubmissionFailure::LaunchFailed),
            );
            stream.update(cx, ConversationStream::refresh_ordinary_submissions);
            return;
        };
        self.agent_controller.preparation_stream = Some(stream.clone());
        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        let request = OrdinaryPreparationRequest {
            run: OrdinaryRun::new(submission.clone(), Arc::new(worker)),
            registered,
            database,
            config: context.config,
            draft: context.draft,
            expected: context.thread,
            skills,
            cancel,
        };
        #[cfg(test)]
        let request = match self.ordinary_test_preparations.as_mut() {
            Some(preparations) => {
                preparations.push(request);
                return;
            }
            None => request,
        };
        let unpublished = Arc::new(Mutex::new(Some(request)));
        let published = unpublished.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let started = std::thread::Builder::new()
            .name("vega-ordinary-prepare".into())
            .spawn(move || {
                let request = published.lock().ok().and_then(|mut request| request.take());
                if let Some(request) = request {
                    let outcome = request.prepare();
                    let _ = sender.send(outcome);
                }
            });
        if started.is_err() {
            let _ = self.trusted_actions.release(lease);
            self.agent_controller.preparation_stream = None;
            stream.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
            self.agent_controller.ordinary_preparing.remove(&thread_id);
            if let Ok(mut request) = unpublished.lock()
                && let Some(request) = request.take()
            {
                let _ = request
                    .registered
                    .complete(Err(SubmissionFailure::SpawnFailed));
                finish_root(request.run, Err(SubmissionFailure::SpawnFailed));
            }
            stream.update(cx, ConversationStream::refresh_ordinary_submissions);
            return;
        }
        let observed = submission.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(ORDINARY_POLL).await;
                let outcome = receiver.try_recv();
                let waiting = matches!(outcome, Err(mpsc::TryRecvError::Empty));
                let result = this.update(cx, |this, cx| {
                    stream.update(cx, ConversationStream::refresh_ordinary_submissions);
                    if waiting {
                        return;
                    }
                    this.agent_controller.ordinary_preparing.remove(&thread_id);
                    let released = this.trusted_actions.release(lease);
                    if this.agent_controller.preparation_stream.as_ref() == Some(&stream) {
                        this.agent_controller.preparation_stream = None;
                        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                    }
                    if let Ok(Ok(launch)) = outcome {
                        if !released {
                            return;
                        }
                        if !this.owns_stream_request(&stream, &thread_id, cx)
                            || cx.global::<SettingsOpen>().0
                        {
                            return;
                        }
                        if this.draft_for_route(&thread_id).is_some() {
                            this.draft = None;
                        }
                        stream.update(cx, |stream, cx| stream.set_draft_route(false, cx));
                        let reasoning = observed.raw().reasoning().cloned();
                        this.start_agent_run_with_reasoning(
                            stream.clone(),
                            &thread_id,
                            PendingAgentRun::UserMessage(crate::app_agent::UserSubmission {
                                content: observed.raw().text().to_owned(),
                                images: observed.raw().images().to_vec(),
                                ordinary: Some(launch),
                            }),
                            reasoning,
                            cx,
                        );
                    }
                    stream.update(cx, ConversationStream::refresh_ordinary_submissions);
                });
                if result.is_err() || !waiting {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn ordinary_action(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &OrdinaryActionRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let (database, path) = match &cx.global::<VegaStore>().0 {
            Ok(store) => (
                store.database_identity(),
                store.database_path().map(PathBuf::from),
            ),
            Err(_) => return,
        };
        let records =
            vega_conversation::ordinary_submissions::for_thread(database, &request.thread_id)
                .unwrap_or_default();
        let Some(submission) = records
            .into_iter()
            .find(|record| record.id() == request.submission)
        else {
            return;
        };
        if submission
            .snapshot()
            .attempts
            .last()
            .map(|attempt| attempt.id)
            != Some(request.attempt)
        {
            return;
        }
        match request.action {
            OrdinaryAction::Cancel => {
                crate::ordinary_submission::cancel(request.submission, request.attempt);
                self.agent_controller
                    .request_active_cancel(&request.thread_id);
            }
            OrdinaryAction::Retry => {
                if let Ok(worker) = submission.try_retry(request.attempt) {
                    self.prepare_ordinary(stream.clone(), submission, worker, cx);
                }
            }
            OrdinaryAction::CheckStatus => {
                let Some(path) = path else {
                    return;
                };
                if submission
                    .snapshot()
                    .attempts
                    .last()
                    .is_none_or(|attempt| attempt.workers != 0)
                {
                    stream.update(cx, |stream, cx| {
                        stream.ordinary_status_checked(
                            request.submission,
                            request.attempt,
                            "正在结束本次发送；结束后可再次核实",
                            cx,
                        )
                    });
                    return;
                }
                let identity = request.submission;
                let attempt = request.attempt;
                let thread = request.thread_id.clone();
                let (sender, receiver) = mpsc::sync_channel(1);
                let started = std::thread::Builder::new()
                    .name("vega-ordinary-status".into())
                    .spawn(move || {
                        let result =
                            vega_store::Store::open_read_only(path)
                                .ok()
                                .and_then(|store| {
                                    vega_conversation::agent::observe_ordinary_history(
                                        &store,
                                        &submission,
                                        attempt,
                                    )
                                    .ok()
                                });
                        let _ = sender.send(result);
                    });
                if started.is_err() {
                    return;
                }
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(ORDINARY_POLL).await;
                        let result = match receiver.try_recv() {
                            Ok(result) => result,
                            Err(mpsc::TryRecvError::Empty) => continue,
                            Err(mpsc::TryRecvError::Disconnected) => None,
                        };
                        let _ = this.update(cx, |this, cx| {
                            if !this.owns_stream_request(&stream, &thread, cx) {
                                return;
                            }
                            let message = match result {
                                Some(vega_conversation::agent::OrdinaryHistoryObservation::MatchingUser) => "历史中已有本次消息；启动结果未确认，不能自动重试",
                                Some(vega_conversation::agent::OrdinaryHistoryObservation::Absent) => "历史中未找到本次消息；缺少安全重试证明",
                                Some(vega_conversation::agent::OrdinaryHistoryObservation::Conflict) => "历史与本次发送不匹配；请查看会话历史，不能自动重试",
                                None => "无法核实；请稍后重试核实或查看会话历史",
                            };
                            stream.update(cx, |stream, cx| {
                                stream.ordinary_status_checked(identity, attempt, message, cx)
                            });
                        });
                        break;
                    }
                })
                .detach();
            }
        }
        stream.update(cx, ConversationStream::refresh_ordinary_submissions);
    }

    pub(crate) fn rehydrate_ordinary(
        &self,
        stream: &Entity<ConversationStream>,
        thread: &str,
        cx: &mut Context<Self>,
    ) {
        if let Ok(store) = &cx.global::<VegaStore>().0
            && let Ok(records) = vega_conversation::ordinary_submissions::for_thread(
                store.database_identity(),
                thread,
            )
        {
            stream.update(cx, |stream, cx| {
                for record in records {
                    stream.project_ordinary_submission(record, cx);
                }
                stream.refresh_ordinary_submissions(cx);
            });
        }
    }
}
