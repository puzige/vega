use super::*;

impl VegaWindow {
    pub(crate) fn diff_route_is_current(identity: &DiffRouteIdentity, cx: &App) -> bool {
        !cx.global::<SettingsOpen>().0
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| {
                    thread.id == identity.thread_id && thread.project_id == identity.project_id
                })
    }

    pub(crate) fn close_diff_if_route_stale(&mut self, cx: &mut Context<Self>) {
        let stale = self
            .diff_controller
            .active
            .as_ref()
            .is_some_and(|active| !Self::diff_route_is_current(&active.identity, cx));
        if stale {
            self.diff_controller.close();
            cx.notify();
        }
    }

    pub(crate) fn diff_project_root(
        &self,
        identity: &DiffRouteIdentity,
        cx: &App,
    ) -> Result<PathBuf, GitWorkspaceErrorCode> {
        let thread_matches = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .is_some_and(|thread| {
                thread.id == identity.thread_id && thread.project_id == identity.project_id
            });
        if !thread_matches {
            return Err(GitWorkspaceErrorCode::Cancelled);
        }
        let store = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .map_err(|_| GitWorkspaceErrorCode::InvalidRoot)?;
        let project = vega_store::projects::find(store.conn(), &identity.project_id)
            .map_err(|_| GitWorkspaceErrorCode::InvalidRoot)?
            .ok_or(GitWorkspaceErrorCode::InvalidRoot)?;
        Ok(PathBuf::from(project.path))
    }

    pub(crate) fn open_workspace_diff(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &OpenWorkspaceDiffRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let project_matches = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .is_some_and(|thread| thread.project_id == request.project_id);
        if !project_matches {
            return;
        }
        let view =
            cx.new(|cx| DiffView::new(request.thread_id.clone(), request.project_id.clone(), cx));
        cx.subscribe(&view, |this, view, request, cx| {
            this.request_diff_projection(view.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&view, |this, view, request, cx| {
            this.retry_workspace_diff(view.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&view, |this, view, request, cx| {
            this.close_workspace_diff(view.clone(), request, cx);
        })
        .detach();
        let Some(identity) = self.diff_controller.begin(
            request.thread_id.clone(),
            request.project_id.clone(),
            view.clone(),
        ) else {
            view.update(cx, |view, cx| {
                view.apply_refresh_error(GitWorkspaceErrorCode::OutputTooLarge, cx)
            });
            return;
        };
        self.schedule_diff_refresh(&identity, cx);
        self.start_diff_poll(identity, view, cx);
        cx.notify();
    }

    pub(crate) fn start_diff_poll(
        &mut self,
        identity: DiffRouteIdentity,
        view: Entity<DiffView>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_REFRESH_INTERVAL).await;
                let keep_polling = this
                    .update(cx, |this, cx| {
                        let visible = this.diff_controller.active.as_ref().is_some_and(|active| {
                            active.identity == identity && active.view == view
                        }) && Self::diff_route_is_current(&identity, cx);
                        if visible {
                            this.schedule_diff_refresh(&identity, cx);
                        } else if this.diff_controller.matches(&identity) {
                            this.diff_controller.close();
                            cx.notify();
                        }
                        visible
                    })
                    .unwrap_or(false);
                if !keep_polling {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn schedule_diff_refresh(
        &mut self,
        identity: &DiffRouteIdentity,
        cx: &mut Context<Self>,
    ) {
        if !Self::diff_route_is_current(identity, cx) {
            if self.diff_controller.matches(identity) {
                self.diff_controller.close();
                cx.notify();
            }
            return;
        }
        let request_seq = {
            let Some(active) = self.diff_controller.active.as_mut() else {
                return;
            };
            if active.identity != *identity {
                return;
            }
            let request_seq = match active.request_refresh() {
                DiffRefreshDecision::Start(request_seq) => request_seq,
                DiffRefreshDecision::Coalesced => return,
                DiffRefreshDecision::Overflow => {
                    self.diff_controller.close();
                    cx.notify();
                    return;
                }
            };
            active
                .view
                .update(cx, |view, cx| view.set_refreshing(true, cx));
            request_seq
        };
        self.launch_diff_refresh(identity, request_seq, cx);
    }

    pub(crate) fn launch_diff_refresh(
        &mut self,
        identity: &DiffRouteIdentity,
        request_seq: u64,
        cx: &mut Context<Self>,
    ) {
        if !Self::diff_route_is_current(identity, cx) {
            if self.diff_controller.matches(identity) {
                self.diff_controller.close();
                cx.notify();
            }
            return;
        }
        let (service, cancel) = {
            let Some(active) = self.diff_controller.active.as_ref() else {
                return;
            };
            if active.identity != *identity || active.refresh_in_flight != Some(request_seq) {
                return;
            }
            (active.service.clone(), active.cancel.child_token())
        };
        let root = if service.is_none() {
            match self.diff_project_root(identity, cx) {
                Ok(root) => Some(root),
                Err(code) => {
                    self.finish_diff_refresh(
                        identity,
                        request_seq,
                        DiffRefreshWorkerResult::Failed(code),
                        cx,
                    );
                    return;
                }
            }
        } else {
            None
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-diff-refresh".into())
            .spawn(move || run_diff_refresh_worker(service, root, cancel, sender));
        if worker.is_err() {
            self.finish_diff_refresh(
                identity,
                request_seq,
                DiffRefreshWorkerResult::Failed(GitWorkspaceErrorCode::SpawnFailed),
                cx,
            );
            return;
        }
        let identity = identity.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let result = match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => Some(DiffRefreshWorkerResult::Failed(
                        GitWorkspaceErrorCode::SpawnFailed,
                    )),
                };
                let Some(result) = result else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_diff_refresh(&identity, request_seq, result, cx)
                });
                break;
            }
        })
        .detach();
    }

    pub(crate) fn finish_diff_refresh(
        &mut self,
        identity: &DiffRouteIdentity,
        request_seq: u64,
        result: DiffRefreshWorkerResult,
        cx: &mut Context<Self>,
    ) {
        if !Self::diff_route_is_current(identity, cx) {
            if self.diff_controller.matches(identity) {
                self.diff_controller.close();
                cx.notify();
            }
            return;
        }
        enum RefreshUi {
            Snapshot(Entity<DiffView>, WorkspaceSnapshot),
            Error(Entity<DiffView>, GitWorkspaceErrorCode),
            Drop(Entity<DiffView>),
        }

        let completion = {
            let Some(active) = self.diff_controller.active.as_mut() else {
                return;
            };
            if active.identity != *identity {
                return;
            }
            let Some(completion) = active.complete_refresh(request_seq) else {
                return;
            };
            completion
        };
        if let DiffRefreshCompletion::Superseded(rerun_seq) = completion {
            if let DiffRefreshWorkerResult::Ready { service, .. } = result
                && let Some(active) = self.diff_controller.active.as_mut()
                && active.identity == *identity
            {
                active.service = Some(service);
            }
            if let Some(next) = rerun_seq {
                self.launch_diff_refresh(identity, next, cx);
            }
            return;
        }

        let (ui, pending) = {
            let Some(active) = self.diff_controller.active.as_mut() else {
                return;
            };
            if active.identity != *identity {
                return;
            }
            let view = active.view.clone();
            let mut pending = None;
            let ui = match result {
                DiffRefreshWorkerResult::Ready { service, snapshot } => {
                    let generation_changed =
                        active.snapshot_generation != Some(snapshot.generation);
                    active.service = Some(service);
                    active.snapshot_generation = Some(snapshot.generation);
                    if generation_changed {
                        if let Some(cancel) = active.projection_cancel.take() {
                            cancel.cancel();
                        }
                        active.requested_file = None;
                        active.pending_projection = None;
                    } else {
                        pending = active.pending_projection.take();
                    }
                    RefreshUi::Snapshot(view, snapshot)
                }
                DiffRefreshWorkerResult::Failed(
                    GitWorkspaceErrorCode::Cancelled | GitWorkspaceErrorCode::StaleGeneration,
                ) => RefreshUi::Drop(view),
                DiffRefreshWorkerResult::Failed(code) => {
                    active.snapshot_generation = None;
                    active.requested_file = None;
                    active.pending_projection = None;
                    if let Some(cancel) = active.projection_cancel.take() {
                        cancel.cancel();
                    }
                    RefreshUi::Error(view, code)
                }
            };
            (ui, pending)
        };
        match ui {
            RefreshUi::Snapshot(view, snapshot) => view.update(cx, |view, cx| {
                view.set_refreshing(false, cx);
                view.apply_snapshot(snapshot, cx);
            }),
            RefreshUi::Error(view, code) => view.update(cx, |view, cx| {
                view.set_refreshing(false, cx);
                view.apply_refresh_error(code, cx);
            }),
            RefreshUi::Drop(view) => {
                view.update(cx, |view, cx| view.set_refreshing(false, cx));
            }
        }
        if let Some(pending) = pending {
            self.apply_diff_projection_result(pending.fence, pending.result, cx);
        }
    }

    pub(crate) fn request_diff_projection(
        &mut self,
        view: Entity<DiffView>,
        request: &DiffProjectionRequested,
        cx: &mut Context<Self>,
    ) {
        let route_is_current = self.diff_controller.active.as_ref().is_some_and(|active| {
            active.view == view && Self::diff_route_is_current(&active.identity, cx)
        });
        if !route_is_current {
            self.close_diff_if_route_stale(cx);
            return;
        }
        let sequence_exhausted = self
            .diff_controller
            .active
            .as_ref()
            .is_some_and(|active| active.file_request_seq == u64::MAX);
        if sequence_exhausted {
            self.diff_controller.close();
            cx.notify();
            return;
        }
        let (fence, service, cancel) = {
            let Some(active) = self.diff_controller.active.as_mut() else {
                return;
            };
            if active.view != view
                || active.identity.thread_id != request.thread_id
                || active.identity.project_id != request.project_id
            {
                return;
            }
            let Some(service) = active.service.clone() else {
                return;
            };
            let Some(fence) = active.next_projection_fence(request.generation, request.file_id)
            else {
                return;
            };
            if let Some(cancel) = active.projection_cancel.take() {
                cancel.cancel();
            }
            active.pending_projection = None;
            let cancel = active.cancel.child_token();
            active.projection_cancel = Some(cancel.clone());
            (fence, service, cancel)
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let file_id = request.file_id;
        let worker = std::thread::Builder::new()
            .name("vega-diff-projection".into())
            .spawn(move || run_diff_projection_worker(service, file_id, cancel, sender));
        if worker.is_err() {
            self.apply_diff_projection_result(fence, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
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
                    this.apply_diff_projection_result(fence, result, cx)
                });
                break;
            }
        })
        .detach();
    }

    pub(crate) fn apply_diff_projection_result(
        &mut self,
        fence: DiffProjectionFence,
        result: Result<DiffTextProjection, GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if !Self::diff_route_is_current(&fence.route, cx) {
            if self.diff_controller.matches(&fence.route) {
                self.diff_controller.close();
                cx.notify();
            }
            return;
        }
        let view = {
            let Some(active) = self.diff_controller.active.as_mut() else {
                return;
            };
            let disposition = active.projection_disposition(&fence);
            if disposition == DiffProjectionDisposition::Drop {
                return;
            }
            active.projection_cancel = None;
            if disposition == DiffProjectionDisposition::Defer {
                active.pending_projection = Some(PendingDiffProjection { fence, result });
                return;
            }
            active.view.clone()
        };
        match result {
            Ok(projection) => {
                view.update(cx, |view, cx| {
                    let _ = view.apply_projection(projection, cx);
                });
            }
            Err(GitWorkspaceErrorCode::Cancelled | GitWorkspaceErrorCode::StaleGeneration) => {}
            Err(code) => {
                view.update(cx, |view, cx| {
                    view.apply_projection_error(fence.file_id, code, cx)
                });
            }
        }
    }

    pub(crate) fn retry_workspace_diff(
        &mut self,
        view: Entity<DiffView>,
        request: &DiffRetryRequested,
        cx: &mut Context<Self>,
    ) {
        let identity = self
            .diff_controller
            .active
            .as_ref()
            .filter(|active| {
                active.view == view
                    && active.identity.thread_id == request.thread_id
                    && active.identity.project_id == request.project_id
            })
            .map(|active| active.identity.clone());
        if let Some(identity) = identity {
            self.schedule_diff_refresh(&identity, cx);
        }
    }

    pub(crate) fn close_workspace_diff(
        &mut self,
        view: Entity<DiffView>,
        request: &DiffClosed,
        cx: &mut Context<Self>,
    ) {
        let matches = self.diff_controller.active.as_ref().is_some_and(|active| {
            active.view == view
                && active.identity.thread_id == request.thread_id
                && active.identity.project_id == request.project_id
        });
        if matches {
            self.diff_controller.close();
            cx.notify();
        }
    }
}
