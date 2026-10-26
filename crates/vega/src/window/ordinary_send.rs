use super::*;
use crate::ordinary_submission::{
    OrdinaryLaunch, OrdinaryMaterialization, OrdinaryPreparationOwner, finish_root,
};
use vega_conversation::agent::OrdinaryRun;

const ORDINARY_POLL: std::time::Duration = std::time::Duration::from_millis(4);

#[cfg(test)]
mod tests;

fn take_preparation_owner(
    owners: &mut std::collections::HashMap<String, OrdinaryPreparationOwner>,
    thread_id: &str,
    attempt: AttemptId,
) -> Option<OrdinaryPreparationOwner> {
    if owners.get(thread_id)?.attempt != attempt {
        return None;
    }
    owners.remove(thread_id)
}

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

struct OrdinaryPreparationOutcome {
    materialized: Option<OrdinaryMaterialization>,
    launch: Result<OrdinaryLaunch, SubmissionFailure>,
    _admission: vega_store::thread_admission::ThreadAdmission,
}

impl OrdinaryPreparationRequest {
    fn prepare(self) -> OrdinaryPreparationOutcome {
        let materialized = self.materialize();
        self.finish_preparation(materialized)
    }

    fn materialize(&self) -> Result<(vega_store::Store, Thread), SubmissionFailure> {
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
        let thread = vega_conversation::permission_contracts::prepare_submission_permission(
            &store,
            &self.run.submission,
            &self.run.worker,
            &self.expected,
            self.draft.as_ref(),
        )
        .map_err(|_| SubmissionFailure::PreparationFailed)?;
        Ok((store, thread))
    }

    fn finish_preparation(
        self,
        committed: Result<(vega_store::Store, Thread), SubmissionFailure>,
    ) -> OrdinaryPreparationOutcome {
        let admission = self.run.worker.admission().clone();
        let mut materialized = None;
        let result = committed.and_then(|(store, thread)| {
            let thread_id = self.run.submission.snapshot().thread_id;
            materialized = self.draft.as_ref().map(|source| {
                OrdinaryMaterialization::new(
                    self.run.worker.attempt_id(),
                    source.clone(),
                    thread.clone(),
                )
            });
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
        });
        let _ = self
            .registered
            .complete(result.as_ref().map(|_| ()).map_err(|failure| *failure));
        let launch = match result {
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
        };
        OrdinaryPreparationOutcome {
            materialized,
            launch,
            _admission: admission,
        }
    }
}

impl VegaWindow {
    pub(crate) fn submit_ordinary(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerSubmitted,
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
        if self
            .agent_controller
            .ordinary_preparing
            .contains_key(&request.thread_id)
        {
            stream.update(cx, ConversationStream::reject_composer_submission);
            stream.update(cx, ConversationStream::apply_agent_busy);
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
        let promoted =
            match self.promote_model_before_run(&stream, &request.thread_id, &admission, cx) {
                Ok(promoted) => promoted,
                Err(()) => {
                    stream.update(cx, ConversationStream::reject_composer_submission);
                    stream.update(cx, ConversationStream::apply_controller_error);
                    return;
                }
            };
        let reasoning = if promoted {
            stream.read(cx).frozen_reasoning_for_submit()
        } else {
            request.reasoning.clone()
        };
        let reasoning = match reasoning {
            Ok(reasoning) if !stream.read(cx).reasoning_unavailable() => reasoning,
            _ => {
                stream.update(cx, ConversationStream::reject_composer_submission);
                stream.update(cx, ConversationStream::apply_controller_error);
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
        let attempt = worker.attempt_id();
        self.agent_controller.ordinary_preparing.insert(
            thread_id.clone(),
            OrdinaryPreparationOwner {
                attempt,
                cancel: cancel.clone(),
            },
        );
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
                    if take_preparation_owner(
                        &mut this.agent_controller.ordinary_preparing,
                        &thread_id,
                        attempt,
                    )
                    .is_none()
                    {
                        return;
                    }
                    let released = this.trusted_actions.release(lease);
                    if released
                        && this.agent_controller.preparation_stream.as_ref() == Some(&stream)
                    {
                        this.agent_controller.preparation_stream = None;
                        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                    }
                    let Ok(outcome) = outcome else {
                        return;
                    };
                    if let Some(materialized) = outcome.materialized {
                        let current_route = this.owns_stream_request(&stream, &thread_id, cx);
                        let opened = cx.global::<OpenedThread>().0.as_ref();
                        let route = if current_route { opened } else { None };
                        if let Some(projection) = materialized.reconcile(
                            attempt,
                            &mut this.draft,
                            &stream.read(cx).thread_snapshot(),
                            route,
                        ) {
                            if projection.stream {
                                stream.update(cx, |stream, cx| {
                                    stream.apply_thread(materialized.thread.clone(), cx);
                                    stream.set_draft_route(false, cx);
                                });
                            }
                            if projection.route {
                                cx.set_global(OpenedThread(Some(materialized.thread)));
                            }
                            this.sidebar
                                .update(cx, |sidebar, cx| sidebar.reload_sessions(cx));
                        }
                    }
                    if let Ok(launch) = outcome.launch {
                        if !released
                            || launch.cancel.is_cancelled()
                            || !this.owns_stream_request(&stream, &thread_id, cx)
                            || this.is_draft_route(&thread_id)
                            || cx.global::<SettingsOpen>().0
                        {
                            return;
                        }
                        let reasoning = observed.raw().reasoning().cloned();
                        this.start_agent_run_with_reasoning(
                            stream.clone(),
                            &thread_id,
                            PendingAgentRun::UserMessage(crate::app_agent::UserSubmission {
                                content: observed.raw().text().to_owned(),
                                images: observed.raw().images().to_vec(),
                                ordinary: Some(Box::new(launch)),
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
                self.agent_controller.request_ordinary_cancel(
                    &request.thread_id,
                    request.submission,
                    request.attempt,
                );
            }
            OrdinaryAction::Retry => {
                if !self
                    .agent_controller
                    .ordinary_preparing
                    .contains_key(&request.thread_id)
                    && let Ok(worker) = submission.try_retry(request.attempt)
                {
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
                            "ui.ordinary_send.check_finishing",
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
                let status_stream = stream.clone();
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(ORDINARY_POLL).await;
                        let result = match receiver.try_recv() {
                            Ok(result) => result,
                            Err(mpsc::TryRecvError::Empty) => continue,
                            Err(mpsc::TryRecvError::Disconnected) => None,
                        };
                        let _ = this.update(cx, |this, cx| {
                            if !this.owns_stream_request(&status_stream, &thread, cx) {
                                return;
                            }
                            let message = match result {
                                Some(vega_conversation::agent::OrdinaryHistoryObservation::MatchingUser) => "ui.ordinary_send.check_matching",
                                Some(vega_conversation::agent::OrdinaryHistoryObservation::Absent) => "ui.ordinary_send.check_absent",
                                Some(vega_conversation::agent::OrdinaryHistoryObservation::Conflict) => "ui.ordinary_send.check_conflict",
                                None => "ui.ordinary_send.check_unavailable",
                            };
                            status_stream.update(cx, |stream, cx| {
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
