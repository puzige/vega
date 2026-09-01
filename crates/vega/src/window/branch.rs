use super::*;

impl VegaWindow {
    pub(crate) fn branch_route_is_current(identity: &BranchRouteIdentity, cx: &App) -> bool {
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

    pub(crate) fn close_branch_route(
        &mut self,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) {
        let pending = self
            .branch_controller
            .active
            .as_ref()
            .and_then(|active| active.identity.selector.read(cx).pending_key());
        if let Some(active) = self.branch_controller.close() {
            active.identity.selector.update(cx, |selector, cx| {
                if let Some((operation, generation, branch_id)) = pending {
                    let _ = selector.clear_pending(operation, generation, branch_id, cx);
                }
                selector.close_route(code, cx);
            });
            cx.notify();
        }
    }

    pub(crate) fn close_branch_if_route_stale(&mut self, cx: &mut Context<Self>) {
        let stale = self
            .branch_controller
            .active
            .as_ref()
            .is_some_and(|active| !Self::branch_route_is_current(&active.identity, cx));
        if stale {
            self.close_branch_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        }
    }

    pub(crate) fn ensure_branch_route(
        &mut self,
        thread: &Thread,
        stream: Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        let selector = stream.read(cx).branch_selector();
        let current = self
            .branch_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.identity.thread_id == thread.id
                    && active.identity.project_id == thread.project_id
                    && active.identity.stream == stream
                    && active.identity.selector == selector
            });
        if current {
            return;
        }
        self.close_branch_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        let result = Self::artifact_project_root(thread, cx).and_then(|root| {
            self.branch_controller
                .begin(thread, stream, selector, root)
                .map(|_| ())
        });
        if result.is_err() {
            self.close_branch_route(GitWorkspaceErrorCode::InvalidRoot, cx);
        }
    }

    pub(crate) fn request_branch_list(
        &mut self,
        selector: Entity<BranchSelector>,
        request: &BranchListRequested,
        cx: &mut Context<Self>,
    ) {
        let (fence, service, cancel) = {
            let Some(active) = self.branch_controller.active.as_mut() else {
                selector.update(cx, |selector, cx| {
                    selector.apply_error(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
                return;
            };
            if !Self::branch_route_is_current(&active.identity, cx)
                || active.identity.selector != selector
                || active.identity.thread_id != request.thread_id
                || active.identity.project_id != request.project_id
                || active.prepare_fence.is_some()
                || active.switch_fence.is_some()
            {
                return;
            }
            let Some(sequence) = active.list_sequence.checked_add(1) else {
                self.close_branch_route(GitWorkspaceErrorCode::OutputTooLarge, cx);
                return;
            };
            active.list_sequence = sequence;
            if let Some(cancel) = active.list_cancel.take() {
                cancel.cancel();
            }
            let fence = BranchListFence {
                route: active.identity.clone(),
                sequence,
            };
            let cancel = active.cancel.child_token();
            active.list_fence = Some(fence.clone());
            active.list_cancel = Some(cancel.clone());
            (fence, active.service.clone(), cancel)
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("vega-branch-list".into())
            .spawn(move || run_branch_list_worker(service, worker_fence, cancel, sender));
        if worker.is_err() {
            self.finish_branch_list(fence, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
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
                let _ = this.update(cx, |this, cx| this.finish_branch_list(fence, result, cx));
                break;
            }
        })
        .detach();
    }

    pub(crate) fn finish_branch_list(
        &mut self,
        fence: BranchListFence,
        result: Result<BranchSnapshot, GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if !Self::branch_route_is_current(&fence.route, cx) {
            return;
        }
        let selector = {
            let Some(active) = self.branch_controller.active.as_mut() else {
                return;
            };
            if active.list_fence.as_ref() != Some(&fence) {
                return;
            }
            active.list_fence = None;
            active.list_cancel = None;
            active.identity.selector.clone()
        };
        match result {
            Ok(snapshot) => {
                selector.update(cx, |selector, cx| {
                    let _ = selector.apply_snapshot(snapshot, cx);
                });
            }
            Err(GitWorkspaceErrorCode::Cancelled | GitWorkspaceErrorCode::StaleGeneration) => {}
            Err(code) => selector.update(cx, |selector, cx| selector.apply_error(code, cx)),
        }
    }

    pub(crate) fn branch_guards_clear(
        &self,
        stream: &Entity<ConversationStream>,
        cx: &App,
    ) -> bool {
        !self.trusted_actions.is_busy()
            && !self.commit_controller.is_open()
            && self.agent_controller.active.is_none()
            && !stream.read(cx).has_active_agent()
            && !stream.read(cx).has_pending_permission()
            && !stream.read(cx).has_pending_plan_review(cx)
    }

    pub(crate) fn request_branch_switch(
        &mut self,
        selector: Entity<BranchSelector>,
        request: &BranchSwitchRequested,
        cx: &mut Context<Self>,
    ) {
        let identity = self
            .branch_controller
            .active
            .as_ref()
            .filter(|active| {
                Self::branch_route_is_current(&active.identity, cx)
                    && active.identity.selector == selector
                    && active.identity.thread_id == request.thread_id
                    && active.identity.project_id == request.project_id
                    && active.prepare_fence.is_none()
                    && active.switch_fence.is_none()
                    && selector.read(cx).owns_pending(
                        request.operation_id,
                        request.snapshot_generation,
                        request.branch_id,
                    )
                    && selector
                        .read(cx)
                        .contains_switchable(request.snapshot_generation, request.branch_id)
            })
            .map(|active| active.identity.clone());
        let Some(identity) = identity else {
            selector.update(cx, |selector, cx| {
                let _ = selector.reject_switch(
                    request.operation_id,
                    request.snapshot_generation,
                    request.branch_id,
                    GitWorkspaceErrorCode::StaleGeneration,
                    cx,
                );
            });
            return;
        };
        if !self.branch_guards_clear(&identity.stream, cx) {
            selector.update(cx, |selector, cx| {
                let _ = selector.reject_switch(
                    request.operation_id,
                    request.snapshot_generation,
                    request.branch_id,
                    GitWorkspaceErrorCode::BranchOperationInProgress,
                    cx,
                );
            });
            return;
        }
        let Some(sequence) = self
            .branch_controller
            .active
            .as_ref()
            .and_then(|active| active.switch_sequence.checked_add(1))
        else {
            self.close_branch_route(GitWorkspaceErrorCode::OutputTooLarge, cx);
            return;
        };
        let (fence, service, cancel) = {
            let Some(active) = self.branch_controller.active.as_mut() else {
                return;
            };
            active.switch_sequence = sequence;
            let fence = BranchPrepareFence {
                route: identity,
                sequence,
                snapshot_generation: request.snapshot_generation,
                branch_id: request.branch_id,
                operation_id: request.operation_id,
            };
            let cancel = active.cancel.child_token();
            active.prepare_fence = Some(fence.clone());
            active.switch_cancel = Some(cancel.clone());
            (fence, active.service.clone(), cancel)
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("vega-branch-preflight".into())
            .spawn(move || run_branch_prepare_worker(service, worker_fence, cancel, sender));
        if worker.is_err() {
            self.finish_branch_prepare(fence, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
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
                let _ = this.update(cx, |this, cx| this.finish_branch_prepare(fence, result, cx));
                break;
            }
        })
        .detach();
    }

    pub(crate) fn finish_branch_prepare(
        &mut self,
        fence: BranchPrepareFence,
        result: Result<BranchSwitchPermit, GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if !self.branch_controller.claim_prepare(&fence) {
            return;
        }
        let current = Self::branch_route_is_current(&fence.route, cx)
            && fence.route.selector.read(cx).is_open()
            && fence.route.selector.read(cx).is_pending()
            && fence.route.selector.read(cx).owns_pending(
                fence.operation_id,
                fence.snapshot_generation,
                fence.branch_id,
            )
            && fence
                .route
                .selector
                .read(cx)
                .contains_switchable(fence.snapshot_generation, fence.branch_id);
        if !current {
            if let Some(active) = self.branch_controller.active.as_mut()
                && active.identity == fence.route
            {
                active.switch_cancel = None;
            }
            fence.route.selector.update(cx, |selector, cx| {
                let _ = selector.clear_pending(
                    fence.operation_id,
                    fence.snapshot_generation,
                    fence.branch_id,
                    cx,
                );
            });
            return;
        }
        let permit = match result {
            Ok(permit) => permit,
            Err(code) => {
                if let Some(active) = self.branch_controller.active.as_mut()
                    && active.identity == fence.route
                {
                    active.switch_cancel = None;
                }
                fence.route.selector.update(cx, |selector, cx| {
                    let _ = selector.finish_switch(
                        fence.operation_id,
                        fence.snapshot_generation,
                        fence.branch_id,
                        None,
                        Some(code),
                        cx,
                    );
                });
                return;
            }
        };
        if !self.branch_guards_clear(&fence.route.stream, cx) {
            if let Some(active) = self.branch_controller.active.as_mut()
                && active.identity == fence.route
            {
                active.switch_cancel = None;
            }
            fence.route.selector.update(cx, |selector, cx| {
                let _ = selector.reject_switch(
                    fence.operation_id,
                    fence.snapshot_generation,
                    fence.branch_id,
                    GitWorkspaceErrorCode::BranchOperationInProgress,
                    cx,
                );
            });
            return;
        }
        let Some(lease) = self.trusted_actions.acquire(
            TrustedActionKind::BranchSwitch,
            fence.route.epoch,
            fence.sequence,
        ) else {
            if let Some(active) = self.branch_controller.active.as_mut()
                && active.identity == fence.route
            {
                active.switch_cancel = None;
            }
            fence.route.selector.update(cx, |selector, cx| {
                let _ = selector.reject_switch(
                    fence.operation_id,
                    fence.snapshot_generation,
                    fence.branch_id,
                    GitWorkspaceErrorCode::BranchOperationInProgress,
                    cx,
                );
            });
            return;
        };
        fence
            .route
            .stream
            .update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        if let Some(active) = self.diff_controller.active.as_mut() {
            if let Some(cancel) = active.projection_cancel.take() {
                cancel.cancel();
            }
            active.pending_projection = None;
        }
        if let Some(active) = self.artifact_controller.active.as_mut() {
            Self::cancel_artifact_interactions(active, cx);
        }
        let execute_fence = BranchSwitchFence {
            route: fence.route,
            sequence: fence.sequence,
            snapshot_generation: fence.snapshot_generation,
            branch_id: fence.branch_id,
            operation_id: fence.operation_id,
            lease,
        };
        let (service, cancel) = {
            let Some(active) = self.branch_controller.active.as_mut() else {
                let _ = self.trusted_actions.release(lease);
                execute_fence
                    .route
                    .stream
                    .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                return;
            };
            let cancel = active.cancel.child_token();
            active.switch_fence = Some(execute_fence.clone());
            active.switch_cancel = Some(cancel.clone());
            (active.service.clone(), cancel)
        };
        self.launch_branch_execute(service, permit, execute_fence, cancel, cx);
    }

    pub(crate) fn launch_branch_execute(
        &mut self,
        service: Arc<BranchWorkspaceService>,
        permit: BranchSwitchPermit,
        fence: BranchSwitchFence,
        cancel: tokio_util::sync::CancellationToken,
        cx: &mut Context<Self>,
    ) {
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("vega-branch-switch".into())
            .spawn(move || run_branch_switch_worker(service, permit, worker_fence, cancel, sender));
        if worker.is_err() {
            self.finish_branch_switch(
                fence,
                BranchSwitchCompletion {
                    outcome: BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::SpawnFailed),
                    snapshot: None,
                },
                cx,
            );
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let (fence, completion) = match receiver.try_recv() {
                    Ok(output) => output,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => (
                        fence,
                        BranchSwitchCompletion {
                            outcome: BranchSwitchOutcome::Failed(
                                GitWorkspaceErrorCode::SpawnFailed,
                            ),
                            snapshot: None,
                        },
                    ),
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_branch_switch(fence, completion, cx)
                });
                break;
            }
        })
        .detach();
    }

    pub(crate) fn branch_selector_closed(
        &mut self,
        selector: Entity<BranchSelector>,
        request: &BranchSelectorClosed,
        _cx: &mut Context<Self>,
    ) {
        let Some(active) = self.branch_controller.active.as_mut() else {
            return;
        };
        if active.identity.selector != selector
            || active.identity.thread_id != request.thread_id
            || active.identity.project_id != request.project_id
        {
            return;
        }
        if let Some(cancel) = active.list_cancel.take() {
            cancel.cancel();
        }
        active.list_fence = None;
        if let Some(cancel) = &active.switch_cancel {
            // The owner future stays alive and performs its authoritative cleanup.
            cancel.cancel();
        }
    }

    pub(crate) fn finish_branch_switch(
        &mut self,
        fence: BranchSwitchFence,
        completion: BranchSwitchCompletion,
        cx: &mut Context<Self>,
    ) {
        if !self.branch_controller.claim_terminal(&fence) {
            return;
        }
        let error = match completion.outcome {
            BranchSwitchOutcome::Switched => None,
            BranchSwitchOutcome::Failed(code) => Some(code),
        };
        fence.route.selector.update(cx, |selector, cx| {
            let _ = selector.finish_switch(
                fence.operation_id,
                fence.snapshot_generation,
                fence.branch_id,
                completion.snapshot,
                error,
                cx,
            );
        });
        // A worker may have attempted mutation even after its route became stale.
        // Queue all conservative workspace reconciliation before releasing authority.
        self.workspace_action_finished(&fence.route.project_id, cx);
        if self.trusted_actions.release(fence.lease) {
            fence
                .route
                .stream
                .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
        }
    }
}
