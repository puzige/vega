use super::*;

impl VegaWindow {
    pub(crate) fn apply_commit_workspace_reconciliation(
        &mut self,
        route: &CommitRouteIdentity,
        reconciled: &CommitWorkspaceReconciliation,
        cx: &mut Context<Self>,
    ) {
        let exact_stream = self
            .stream_view
            .as_ref()
            .is_some_and(|(thread_id, stream)| {
                thread_id == &route.thread_id
                    && stream == &route.stream
                    && stream.read(cx).commit_panel() == route.panel
            });
        if !exact_stream {
            return;
        }
        if let Some(active) = self.diff_controller.active.as_mut()
            && active.identity.project_id == route.project_id
            && active.identity.thread_id == route.thread_id
        {
            active.cancel.cancel();
            if let Some(cancel) = active.projection_cancel.take() {
                cancel.cancel();
            }
            active.cancel = tokio_util::sync::CancellationToken::new();
            active.service = Some(reconciled.workspace_service.clone());
            active.refresh_in_flight = None;
            active.queued_refresh_seq = None;
            active.requested_file = None;
            active.pending_projection = None;
            active.snapshot_generation = reconciled
                .workspace
                .as_ref()
                .ok()
                .map(|workspace| workspace.generation);
            active
                .view
                .update(cx, |view, cx| match &reconciled.workspace {
                    Ok(workspace) => {
                        view.set_refreshing(false, cx);
                        view.apply_snapshot(workspace.clone(), cx);
                    }
                    Err(code) => {
                        view.set_refreshing(false, cx);
                        view.apply_refresh_error(map_commit_reconcile_error(*code), cx);
                    }
                });
            self.record_commit_probe("ui_diff");
        }
        if let Some((service, snapshot)) = &reconciled.branch
            && let Some(active) = self.branch_controller.active.as_mut()
            && active.identity.project_id == route.project_id
            && active.identity.thread_id == route.thread_id
            && active.identity.stream == route.stream
            && Arc::ptr_eq(&active.service, service)
        {
            if let Some(cancel) = active.list_cancel.take() {
                cancel.cancel();
            }
            active.list_fence = None;
            active
                .identity
                .selector
                .update(cx, |selector, cx| match snapshot {
                    Ok(snapshot) => {
                        let _ = selector.apply_snapshot(snapshot.clone(), cx);
                    }
                    Err(code) => selector.apply_error(*code, cx),
                });
            self.record_commit_probe("ui_branch");
        }
        let mut artifact_failure = None;
        if let Some((service, cards)) = &reconciled.artifacts
            && let Some(active) = self.artifact_controller.active.as_mut()
            && active.identity.project_id == route.project_id
            && active.identity.thread_id == route.thread_id
            && active.identity.stream == route.stream
            && Arc::ptr_eq(&active.service, service)
        {
            Self::cancel_artifact_interactions(active, cx);
            match cards {
                Ok(cards) => {
                    for projection in cards {
                        if let Some(card) = active.cards.get(&projection.id) {
                            card.update(cx, |card, cx| {
                                let _ = card.apply_metadata(projection.clone(), cx);
                            });
                        }
                    }
                }
                Err(code) => artifact_failure = Some(*code),
            }
            self.record_commit_probe("ui_artifact");
        }
        if let Some(code) = artifact_failure {
            self.close_artifact_route(code, cx);
        }
    }
}
