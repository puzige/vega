use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use super::*;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Entity, Focusable, KeyBinding, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, px, size,
};
use gpui_platform::application;
use vega_conversation::history::HistoryPage;
use vega_conversation::types::{
    ArtifactCard as ArtifactProjection, ArtifactCardId, ArtifactPreviewProjection, BranchId,
    BranchSnapshot, BranchSwitchCompletion, BranchSwitchOutcome, CommitChecklist, CommitCompletion,
    CommitErrorCode, CommitOutcome, CommitPrepareCompletion, ConversationEvent, DiffTextProjection,
    GitWorkspaceErrorCode, OpenInOutcome, OpenInTarget, Plan, PlanReviewOutcome,
    PricingDraftReason, PricingNotice, PricingSettingsErrorCode, PricingSettingsProjection, Thread,
    ToolCall, WorkspaceFileId, WorkspaceSnapshot,
};
use vega_conversation::{
    ArtifactCaptureCandidate, ArtifactService, BranchSwitchPermit, BranchWorkspaceService,
    GitWorkspaceService, PricingAuthority, PricingLoadOutcome, PricingSaveOutcome, PricingSavePlan,
    PricingSettingsService, TrustedGitService,
};
use vega_store::Store;
use vega_theme::{Theme, ThemeColors, Typography, theme};
use vega_ui::artifact_card::{
    ArtifactCard, ArtifactCleared, ArtifactOpenRequested, ArtifactPreviewRequested,
};
use vega_ui::branch_selector::{
    BranchListRequested, BranchOperationId, BranchSelector, BranchSelectorClosed,
    BranchSwitchRequested,
};
use vega_ui::commit_panel::{
    CommitDraftRequested, CommitOperationId, CommitPanel, CommitPanelClosed,
    CommitPrepareRequested, CommitRequested,
};
use vega_ui::conversation_stream::{
    ComposerDefaultsRequested, ComposerSubmitted, ConversationStream, HistoryPageRequested,
    OpenCommitPanelRequested, OpenWorkspaceDiffRequested, ThreadSettingsRequested,
    WorkspaceToolTerminal, bench as render_frame_bench,
};
use vega_ui::diff_view::{
    DIFF_REFRESH_INTERVAL, DiffClosed, DiffProjectionRequested, DiffRetryRequested, DiffView,
};
use vega_ui::plan_card::PlanReviewRequested;
use vega_ui::settings::{
    CloseSettings, OpenSettings, PricingDiscardRequested, PricingMutationRequested,
    PricingReloadRequested, PricingRetryRequested, SettingsOpen, SettingsView, all_models,
};
use vega_ui::sidebar::{
    AUTO_COLLAPSE_WIDTH, CONTENT_MAX_WIDTH, CONTENT_MIN_PADDING, NewThread, OpenedThread,
    PendingDeleteConfirm, Sidebar, SidebarCollapsed, ToggleSidebar, VegaStore, load_collapsed,
    render_delete_confirm_overlay, toggle_persisted,
};

use crate::app_agent::*;
use crate::artifact_controller::*;
use crate::branch_controller::*;
use crate::commit_controller::*;
use crate::diff_controller::*;
use crate::pricing_controller::*;
use crate::thread_reload::*;
use crate::trusted_action::*;

impl VegaWindow {
    pub(crate) fn commit_route_is_current(&self, identity: &CommitRouteIdentity, cx: &App) -> bool {
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
            && self
                .stream_view
                .as_ref()
                .is_some_and(|(thread_id, stream)| {
                    thread_id == &identity.thread_id
                        && stream == &identity.stream
                        && stream.read(cx).commit_panel() == identity.panel
                })
    }

    pub(crate) fn commit_guards_clear(
        &self,
        stream: &Entity<ConversationStream>,
        cx: &App,
    ) -> bool {
        !self.trusted_actions.is_busy()
            && self.agent_controller.active.is_none()
            && !stream.read(cx).has_active_agent()
            && !stream.read(cx).has_pending_permission()
            && !stream.read(cx).has_pending_plan_review(cx)
    }

    pub(crate) fn poll_commit_worker(
        &mut self,
        fence: CommitFence,
        receiver: mpsc::Receiver<CommitWorkerResult>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |this, cx| {
                            this.recover_disconnected_commit_worker(fence, cx)
                        });
                        return;
                    }
                };
                let _ = this.update(cx, |this, cx| this.finish_commit_worker(fence, result, cx));
                break;
            }
        })
        .detach();
    }

    pub(crate) fn recover_disconnected_commit_worker(
        &mut self,
        fence: CommitFence,
        cx: &mut Context<Self>,
    ) {
        if matches!(fence.phase, CommitPhase::Checklist | CommitPhase::Drafting) {
            let result = if fence.phase == CommitPhase::Checklist {
                CommitWorkerResult::Checklist(Err(CommitErrorCode::SpawnFailed))
            } else {
                CommitWorkerResult::Draft(Err(CommitErrorCode::DraftFailed))
            };
            self.finish_commit_worker(fence, result, cx);
            return;
        }
        let route = self
            .commit_controller
            .active
            .as_mut()
            .filter(|active| active.pending.as_ref() == Some(&fence))
            .or_else(|| {
                self.commit_controller
                    .retiring
                    .as_mut()
                    .filter(|active| active.pending.as_ref() == Some(&fence))
            });
        let Some(active) = route else {
            return;
        };
        let service = active.service.clone();
        let lease = active.lease;
        let terminal_done = Arc::new(AtomicBool::new(false));
        active.terminal_done = Some(terminal_done.clone());
        let branch = self
            .branch_controller
            .active
            .as_ref()
            .filter(|branch| branch.identity.project_id == fence.route.project_id)
            .map(|branch| branch.service.clone());
        let artifacts = self
            .artifact_controller
            .active
            .as_ref()
            .filter(|artifacts| {
                artifacts.identity.project_id == fence.route.project_id
                    && Arc::ptr_eq(&artifacts.workspace, &service.workspace_service())
            })
            .map(|artifacts| artifacts.service.clone());
        let (sender, receiver) = mpsc::sync_channel(1);
        let window_alive = self.window_alive.clone();
        let actions = self.trusted_actions.clone();
        #[cfg(test)]
        let probe = self.commit_test_probe.clone();
        let recovery_phase = fence.phase;
        cx.background_executor()
            .spawn(async move {
                let result = run_commit_recovery_worker(
                    service,
                    branch,
                    artifacts,
                    #[cfg(test)]
                    probe,
                );
                mark_commit_worker_terminal_if_authoritative(
                    recovery_phase,
                    &result,
                    terminal_done,
                    window_alive,
                    actions,
                    lease,
                );
                let _ = sender.send(result);
            })
            .detach();
        self.poll_commit_worker(fence, receiver, cx);
    }

    pub(crate) fn finish_commit_worker(
        &mut self,
        fence: CommitFence,
        result: CommitWorkerResult,
        cx: &mut Context<Self>,
    ) {
        let has_authoritative_workspace =
            commit_result_has_authoritative_workspace(fence.phase, &result);
        let reconciliation = commit_result_reconciliation(&result);
        match self.commit_controller.claim(&fence) {
            CommitClaim::Stale => return,
            CommitClaim::Retiring(mut active) => {
                if !has_authoritative_workspace {
                    active.pending = Some(fence.clone());
                    self.commit_controller.retiring = Some(*active);
                    self.recover_disconnected_commit_worker(fence, cx);
                    return;
                }
                if let Some(reconciled) = reconciliation {
                    self.apply_commit_workspace_reconciliation(&fence.route, reconciled, cx);
                }
                let terminal_applied = if let Some(operation) = fence.operation {
                    active
                        .identity
                        .panel
                        .update(cx, |panel, cx| panel.clear_pending(operation, cx))
                } else {
                    false
                };
                if terminal_applied {
                    self.record_commit_terminal_application(true);
                }
                if self.trusted_actions.release(active.lease) {
                    active
                        .identity
                        .stream
                        .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                    self.record_commit_probe("lease_release");
                }
                return;
            }
            CommitClaim::Active => {}
        }
        if !has_authoritative_workspace {
            if let Some(mut active) = self.commit_controller.active.take() {
                let _ = active
                    .identity
                    .panel
                    .update(cx, |panel, cx| panel.request_close(cx));
                active.pending = Some(fence.clone());
                self.commit_controller.retiring = Some(active);
                self.recover_disconnected_commit_worker(fence, cx);
            }
            return;
        }
        if let Some(reconciled) = reconciliation {
            self.apply_commit_workspace_reconciliation(&fence.route, reconciled, cx);
        }
        if !self.commit_route_is_current(&fence.route, cx) {
            self.close_commit_route(cx);
            return;
        }
        match (fence.phase, result) {
            (CommitPhase::Checklist, CommitWorkerResult::Checklist(result)) => {
                let accepted = match result {
                    Ok(checklist) => {
                        let snapshot_id = checklist.id;
                        let accepted = fence
                            .route
                            .panel
                            .update(cx, |panel, cx| panel.apply_checklist(checklist, cx));
                        if accepted && let Some(active) = self.commit_controller.active.as_mut() {
                            active.phase = CommitPhase::Checklist;
                            active.snapshot = Some(snapshot_id);
                            active.prepared = None;
                        }
                        accepted
                    }
                    Err(code) => fence.route.panel.update(cx, |panel, cx| {
                        panel.apply_error(
                            vega_ui::commit_panel::CommitPanelStage::Loading,
                            code,
                            cx,
                        )
                    }),
                };
                if !accepted {
                    let _ = fence.route.panel.update(cx, |panel, cx| {
                        panel.apply_error(
                            vega_ui::commit_panel::CommitPanelStage::Loading,
                            CommitErrorCode::MalformedOutput,
                            cx,
                        )
                    });
                    self.close_commit_route(cx);
                } else {
                    self.record_commit_terminal_application(false);
                }
            }
            (CommitPhase::Preparing, CommitWorkerResult::Prepare(completion, _)) => {
                let success = completion.prepared.is_some()
                    && completion.workspace.is_some()
                    && completion.error.is_none();
                let prepared_id = completion.prepared.as_ref().map(|prepared| prepared.id);
                if let Some(active) = self.commit_controller.active.as_mut() {
                    active.phase = if success {
                        CommitPhase::CommitReady
                    } else {
                        CommitPhase::Checklist
                    };
                    active.prepared = success.then_some(prepared_id).flatten();
                }
                if let Some(operation) = fence.operation {
                    let applied = fence.route.panel.update(cx, |panel, cx| {
                        let value = completion.prepared.ok_or(
                            completion
                                .error
                                .unwrap_or(CommitErrorCode::ChangedDuringRead),
                        );
                        panel.finish_prepare(operation, value, cx)
                    });
                    if applied {
                        self.record_commit_terminal_application(true);
                    }
                }
            }
            (CommitPhase::Drafting, CommitWorkerResult::Draft(result)) => {
                if let Some(active) = self.commit_controller.active.as_mut() {
                    active.phase = CommitPhase::CommitReady;
                }
                if let Some(operation) = fence.operation {
                    let applied = fence
                        .route
                        .panel
                        .update(cx, |panel, cx| panel.finish_draft(operation, result, cx));
                    if applied {
                        self.record_commit_terminal_application(false);
                    }
                }
            }
            (CommitPhase::Committing, CommitWorkerResult::Commit(completion, _)) => {
                let error = match completion.outcome {
                    CommitOutcome::Committed if completion.workspace.is_some() => None,
                    CommitOutcome::Committed => Some(CommitErrorCode::ChangedDuringRead),
                    CommitOutcome::Failed(code) => Some(code),
                };
                if let Some(operation) = fence.operation {
                    let applied = fence
                        .route
                        .panel
                        .update(cx, |panel, cx| panel.finish_commit(operation, error, cx));
                    if applied {
                        self.record_commit_terminal_application(true);
                    }
                }
                if let Some(active) = self.commit_controller.active.take()
                    && self.trusted_actions.release(active.lease)
                {
                    active
                        .identity
                        .stream
                        .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                    self.record_commit_probe("lease_release");
                }
            }
            (CommitPhase::Preparing, CommitWorkerResult::Recovered(code, _)) => {
                if let Some(active) = self.commit_controller.active.as_mut() {
                    active.phase = CommitPhase::Checklist;
                    active.prepared = None;
                }
                if let Some(operation) = fence.operation {
                    let applied = fence.route.panel.update(cx, |panel, cx| {
                        panel.finish_prepare(operation, Err(code), cx)
                    });
                    if applied {
                        self.record_commit_terminal_application(false);
                    }
                }
            }
            (CommitPhase::Committing, CommitWorkerResult::Recovered(code, _)) => {
                if let Some(operation) = fence.operation {
                    let applied = fence.route.panel.update(cx, |panel, cx| {
                        panel.finish_commit(operation, Some(code), cx)
                    });
                    if applied {
                        self.record_commit_terminal_application(false);
                    }
                }
                if let Some(active) = self.commit_controller.active.take()
                    && self.trusted_actions.release(active.lease)
                {
                    active
                        .identity
                        .stream
                        .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                    self.record_commit_probe("lease_release");
                }
            }
            (CommitPhase::Preparing, CommitWorkerResult::RuntimeUnavailable(code)) => {
                if let Some(operation) = fence.operation {
                    let applied = fence.route.panel.update(cx, |panel, cx| {
                        panel.finish_prepare(operation, Err(code), cx)
                    });
                    if applied {
                        self.record_commit_terminal_application(false);
                    }
                }
                if let Some(active) = self.commit_controller.active.take()
                    && self.trusted_actions.release(active.lease)
                {
                    active
                        .identity
                        .stream
                        .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                }
            }
            (CommitPhase::Committing, CommitWorkerResult::RuntimeUnavailable(code)) => {
                if let Some(operation) = fence.operation {
                    let applied = fence.route.panel.update(cx, |panel, cx| {
                        panel.finish_commit(operation, Some(code), cx)
                    });
                    if applied {
                        self.record_commit_terminal_application(false);
                    }
                }
                if let Some(active) = self.commit_controller.active.take()
                    && self.trusted_actions.release(active.lease)
                {
                    active
                        .identity
                        .stream
                        .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                }
            }
            _ => self.close_commit_route(cx),
        }
    }

    pub(crate) fn open_commit_panel(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &OpenCommitPanelRequested,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = cx.global::<OpenedThread>().0.clone().filter(|thread| {
            thread.id == request.thread_id && thread.project_id == request.project_id
        }) else {
            return;
        };
        if self
            .stream_view
            .as_ref()
            .is_none_or(|(thread_id, current)| thread_id != &thread.id || current != &stream)
        {
            return;
        }
        if !self.commit_guards_clear(&stream, cx) || self.commit_controller.is_open() {
            return;
        }
        let Ok(root) = Self::artifact_project_root(&thread, cx) else {
            return;
        };
        let workspace = self
            .artifact_controller
            .active
            .as_ref()
            .filter(|active| {
                active.identity.project_id == thread.project_id
                    && active.identity.thread_id == thread.id
                    && active.identity.stream == stream
            })
            .map(|active| active.workspace.clone())
            .or_else(|| GitWorkspaceService::new(&root).ok().map(Arc::new));
        let Some(workspace) = workspace else {
            return;
        };
        let Ok(service) = TrustedGitService::new(&root, workspace.clone()).map(Arc::new) else {
            return;
        };
        let Some(epoch) = self.commit_controller.next_epoch.checked_add(1) else {
            return;
        };
        let Some(lease) = self
            .trusted_actions
            .acquire(TrustedActionKind::Commit, epoch, 1)
        else {
            return;
        };
        let panel = stream.read(cx).commit_panel();
        if !panel.update(cx, |panel, cx| panel.request_open(cx)) {
            let _ = self.trusted_actions.release(lease);
            return;
        }
        let identity = CommitRouteIdentity {
            epoch,
            thread_id: thread.id,
            project_id: thread.project_id,
            stream: stream.clone(),
            panel: panel.clone(),
        };
        let mut active = ActiveCommitRoute {
            identity,
            service: service.clone(),
            lease,
            next_sequence: 0,
            phase: CommitPhase::Checklist,
            snapshot: None,
            prepared: None,
            focus_pending: true,
            pending: None,
            cancel: None,
            terminal_done: None,
        };
        let Some((fence, cancel, terminal_done)) = CommitController::begin_fence(
            &mut active,
            CommitPhase::Checklist,
            None,
            CommitFenceAuthority::None,
        ) else {
            let _ = panel.update(cx, |panel, cx| {
                panel.apply_error(
                    vega_ui::commit_panel::CommitPanelStage::Loading,
                    CommitErrorCode::OutputTooLarge,
                    cx,
                )
            });
            let _ = self.trusted_actions.release(lease);
            return;
        };
        self.commit_controller.next_epoch = epoch;
        self.commit_controller.active = Some(active);
        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        let (sender, receiver) = mpsc::sync_channel(1);
        let window_alive = self.window_alive.clone();
        let actions = self.trusted_actions.clone();
        #[cfg(test)]
        let probe = self.commit_test_probe.clone();
        cx.background_executor()
            .spawn(async move {
                let result = run_commit_checklist_worker(
                    workspace,
                    service,
                    cancel,
                    #[cfg(test)]
                    probe,
                );
                mark_commit_worker_terminal_if_authoritative(
                    CommitPhase::Checklist,
                    &result,
                    terminal_done,
                    window_alive,
                    actions,
                    lease,
                );
                let _ = sender.send(result);
            })
            .detach();
        self.poll_commit_worker(fence, receiver, cx);
    }

    pub(crate) fn close_commit_route(&mut self, cx: &mut Context<Self>) {
        if let Some((lease, stream)) = self.commit_controller.retire_or_close()
            && self.trusted_actions.release(lease)
        {
            stream.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
        }
    }

    pub(crate) fn fail_commit_request_before_worker(
        &mut self,
        panel: &Entity<CommitPanel>,
        operation: CommitOperationId,
        cx: &mut Context<Self>,
    ) {
        let _ = panel.update(cx, |panel, cx| {
            panel.fail_pending(operation, CommitErrorCode::OutputTooLarge, cx)
        });
        self.close_commit_route(cx);
    }

    pub(crate) fn close_commit_if_route_stale(&mut self, cx: &mut Context<Self>) {
        let stale = self
            .commit_controller
            .active
            .as_ref()
            .is_some_and(|active| !self.commit_route_is_current(&active.identity, cx));
        if stale {
            if let Some(active) = self.commit_controller.active.as_ref() {
                active.identity.panel.update(cx, |panel, cx| {
                    let _ = panel.request_close(cx);
                });
            }
            self.close_commit_route(cx);
        }
    }

    pub(crate) fn request_commit_prepare(
        &mut self,
        panel: Entity<CommitPanel>,
        request: &CommitPrepareRequested,
        cx: &mut Context<Self>,
    ) {
        let route_current = self
            .commit_controller
            .active
            .as_ref()
            .is_some_and(|active| self.commit_route_is_current(&active.identity, cx));
        let Some(active) = self.commit_controller.active.as_mut() else {
            return;
        };
        if active.identity.panel != panel
            || active.identity.thread_id != request.thread_id
            || active.identity.project_id != request.project_id
            || active.phase != CommitPhase::Checklist
            || active.snapshot != Some(request.snapshot_id)
            || active.pending.is_some()
            || !panel.read(cx).owns_pending(request.operation_id)
            || !route_current
        {
            return;
        }
        let Some((fence, cancel, terminal_done)) = CommitController::begin_fence(
            active,
            CommitPhase::Preparing,
            Some(request.operation_id),
            CommitFenceAuthority::Snapshot(request.snapshot_id),
        ) else {
            self.fail_commit_request_before_worker(&panel, request.operation_id, cx);
            return;
        };
        let service = active.service.clone();
        let lease = active.lease;
        let snapshot_id = request.snapshot_id;
        let selected = request.selected.clone();
        let branch = self
            .branch_controller
            .active
            .as_ref()
            .filter(|branch| branch.identity.project_id == request.project_id)
            .map(|branch| branch.service.clone());
        let artifacts = self
            .artifact_controller
            .active
            .as_ref()
            .filter(|artifacts| {
                artifacts.identity.project_id == request.project_id
                    && Arc::ptr_eq(&artifacts.workspace, &service.workspace_service())
            })
            .map(|artifacts| artifacts.service.clone());
        let (sender, receiver) = mpsc::sync_channel(1);
        let window_alive = self.window_alive.clone();
        let actions = self.trusted_actions.clone();
        #[cfg(test)]
        let probe = self.commit_test_probe.clone();
        cx.background_executor()
            .spawn(async move {
                let result = run_commit_prepare_worker(
                    service,
                    snapshot_id,
                    selected,
                    cancel,
                    branch,
                    artifacts,
                    #[cfg(test)]
                    probe,
                );
                mark_commit_worker_terminal_if_authoritative(
                    CommitPhase::Preparing,
                    &result,
                    terminal_done,
                    window_alive,
                    actions,
                    lease,
                );
                let _ = sender.send(result);
            })
            .detach();
        self.poll_commit_worker(fence, receiver, cx);
    }

    pub(crate) fn request_commit_draft(
        &mut self,
        panel: Entity<CommitPanel>,
        request: &CommitDraftRequested,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = cx.global::<OpenedThread>().0.clone() else {
            return;
        };
        let route_current = self
            .commit_controller
            .active
            .as_ref()
            .is_some_and(|active| self.commit_route_is_current(&active.identity, cx));
        let Some(active) = self.commit_controller.active.as_mut() else {
            return;
        };
        if active.identity.panel != panel
            || active.identity.thread_id != request.thread_id
            || active.identity.project_id != request.project_id
            || active.phase != CommitPhase::CommitReady
            || active.prepared != Some(request.prepared_id)
            || active.pending.is_some()
            || !panel.read(cx).owns_pending(request.operation_id)
            || !route_current
        {
            return;
        }
        let Some((fence, cancel, terminal_done)) = CommitController::begin_fence(
            active,
            CommitPhase::Drafting,
            Some(request.operation_id),
            CommitFenceAuthority::Prepared(request.prepared_id),
        ) else {
            self.fail_commit_request_before_worker(&panel, request.operation_id, cx);
            return;
        };
        let service = active.service.clone();
        let lease = active.lease;
        let prepared_id = request.prepared_id;
        #[cfg(test)]
        let provider_override = self.commit_provider_override.clone();
        #[cfg(not(test))]
        let provider_override = None;
        let (sender, receiver) = mpsc::sync_channel(1);
        let window_alive = self.window_alive.clone();
        let actions = self.trusted_actions.clone();
        #[cfg(test)]
        let probe = self.commit_test_probe.clone();
        cx.background_executor()
            .spawn(async move {
                let result = run_commit_draft_worker(
                    service,
                    prepared_id,
                    thread,
                    cancel,
                    provider_override,
                    #[cfg(test)]
                    probe,
                );
                mark_commit_worker_terminal_if_authoritative(
                    CommitPhase::Drafting,
                    &result,
                    terminal_done,
                    window_alive,
                    actions,
                    lease,
                );
                let _ = sender.send(result);
            })
            .detach();
        self.poll_commit_worker(fence, receiver, cx);
    }

    pub(crate) fn request_commit_execute(
        &mut self,
        panel: Entity<CommitPanel>,
        request: &CommitRequested,
        cx: &mut Context<Self>,
    ) {
        let route_current = self
            .commit_controller
            .active
            .as_ref()
            .is_some_and(|active| self.commit_route_is_current(&active.identity, cx));
        let Some(active) = self.commit_controller.active.as_mut() else {
            return;
        };
        if active.identity.panel != panel
            || active.identity.thread_id != request.thread_id
            || active.identity.project_id != request.project_id
            || active.phase != CommitPhase::CommitReady
            || active.prepared != Some(request.prepared_id)
            || active.pending.is_some()
            || !panel.read(cx).owns_pending(request.operation_id)
            || !route_current
        {
            return;
        }
        let Some((fence, cancel, terminal_done)) = CommitController::begin_fence(
            active,
            CommitPhase::Committing,
            Some(request.operation_id),
            CommitFenceAuthority::Prepared(request.prepared_id),
        ) else {
            self.fail_commit_request_before_worker(&panel, request.operation_id, cx);
            return;
        };
        let service = active.service.clone();
        let lease = active.lease;
        let prepared_id = request.prepared_id;
        let message = request.message.clone();
        let branch = self
            .branch_controller
            .active
            .as_ref()
            .filter(|branch| branch.identity.project_id == request.project_id)
            .map(|branch| branch.service.clone());
        let artifacts = self
            .artifact_controller
            .active
            .as_ref()
            .filter(|artifacts| {
                artifacts.identity.project_id == request.project_id
                    && Arc::ptr_eq(&artifacts.workspace, &service.workspace_service())
            })
            .map(|artifacts| artifacts.service.clone());
        let (sender, receiver) = mpsc::sync_channel(1);
        let window_alive = self.window_alive.clone();
        let actions = self.trusted_actions.clone();
        #[cfg(test)]
        let probe = self.commit_test_probe.clone();
        cx.background_executor()
            .spawn(async move {
                #[cfg(test)]
                let disconnect_probe = probe.clone();
                let result = run_commit_execute_worker(
                    service,
                    prepared_id,
                    message,
                    cancel,
                    branch,
                    artifacts,
                    #[cfg(test)]
                    probe,
                );
                #[cfg(test)]
                if disconnect_probe
                    .as_ref()
                    .is_some_and(|probe| probe.drop_commit_sender.swap(false, Ordering::SeqCst))
                {
                    return;
                }
                mark_commit_worker_terminal_if_authoritative(
                    CommitPhase::Committing,
                    &result,
                    terminal_done,
                    window_alive,
                    actions,
                    lease,
                );
                let _ = sender.send(result);
            })
            .detach();
        self.poll_commit_worker(fence, receiver, cx);
    }

    pub(crate) fn commit_panel_closed(
        &mut self,
        panel: Entity<CommitPanel>,
        request: &CommitPanelClosed,
        cx: &mut Context<Self>,
    ) {
        if self
            .commit_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.identity.panel == panel
                    && active.identity.thread_id == request.thread_id
                    && active.identity.project_id == request.project_id
            })
        {
            self.close_commit_route(cx);
        }
    }

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
