#[allow(unused_imports)]
use std::collections::{HashMap, VecDeque};
#[allow(unused_imports)]
use std::path::PathBuf;
#[allow(unused_imports)]
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
#[allow(unused_imports)]
use std::time::{Duration, Instant};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use gpui::prelude::*;
#[allow(unused_imports)]
use gpui::{
    AnyElement, App, Bounds, Entity, Focusable, KeyBinding, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, px, size,
};
#[allow(unused_imports)]
use gpui_platform::application;
#[allow(unused_imports)]
use vega_conversation::history::HistoryPage;
#[allow(unused_imports)]
use vega_conversation::types::{
    ArtifactCard as ArtifactProjection, ArtifactCardId, ArtifactPreviewProjection, BranchId,
    BranchSnapshot, BranchSwitchCompletion, BranchSwitchOutcome, CommitChecklist, CommitCompletion,
    CommitErrorCode, CommitOutcome, CommitPrepareCompletion, ConversationEvent, DiffTextProjection,
    GitWorkspaceErrorCode, OpenInOutcome, OpenInTarget, Plan, PlanReviewOutcome,
    PricingDraftReason, PricingNotice, PricingSettingsErrorCode, PricingSettingsProjection, Thread,
    ToolCall, WorkspaceFileId, WorkspaceSnapshot,
};
#[allow(unused_imports)]
use vega_conversation::{
    ArtifactCaptureCandidate, ArtifactService, BranchSwitchPermit, BranchWorkspaceService,
    GitWorkspaceService, PricingAuthority, PricingLoadOutcome, PricingSaveOutcome, PricingSavePlan,
    PricingSettingsService, TrustedGitService,
};
#[allow(unused_imports)]
use vega_store::Store;
#[allow(unused_imports)]
use vega_theme::{Theme, ThemeColors, Typography, theme};
#[allow(unused_imports)]
use vega_ui::artifact_card::{
    ArtifactCard, ArtifactCleared, ArtifactOpenRequested, ArtifactPreviewRequested,
};
#[allow(unused_imports)]
use vega_ui::branch_selector::{
    BranchListRequested, BranchOperationId, BranchSelector, BranchSelectorClosed,
    BranchSwitchRequested,
};
#[allow(unused_imports)]
use vega_ui::commit_panel::{
    CommitDraftRequested, CommitOperationId, CommitPanel, CommitPanelClosed,
    CommitPrepareRequested, CommitRequested,
};
#[allow(unused_imports)]
use vega_ui::conversation_stream::{
    ComposerDefaultsRequested, ComposerSubmitted, ConversationStream, HistoryPageRequested,
    OpenCommitPanelRequested, OpenWorkspaceDiffRequested, ThreadSettingsRequested,
    WorkspaceToolTerminal, bench as render_frame_bench,
};
#[allow(unused_imports)]
use vega_ui::diff_view::{
    DIFF_REFRESH_INTERVAL, DiffClosed, DiffProjectionRequested, DiffRetryRequested, DiffView,
};
#[allow(unused_imports)]
use vega_ui::plan_card::PlanReviewRequested;
#[allow(unused_imports)]
use vega_ui::settings::{
    CloseSettings, OpenSettings, PricingDiscardRequested, PricingMutationRequested,
    PricingReloadRequested, PricingRetryRequested, SettingsOpen, SettingsView, all_models,
};
#[allow(unused_imports)]
use vega_ui::sidebar::{
    AUTO_COLLAPSE_WIDTH, CONTENT_MAX_WIDTH, CONTENT_MIN_PADDING, NewThread, OpenedThread,
    PendingDeleteConfirm, Sidebar, SidebarCollapsed, ToggleSidebar, VegaStore, load_collapsed,
    render_delete_confirm_overlay, toggle_persisted,
};

#[allow(unused_imports)]
use crate::app_agent::*;
#[allow(unused_imports)]
use crate::artifact_controller::*;
#[allow(unused_imports)]
use crate::branch_controller::*;
#[allow(unused_imports)]
use crate::commit_controller::*;
#[allow(unused_imports)]
use crate::diff_controller::*;
#[allow(unused_imports)]
use crate::pricing_controller::*;
#[allow(unused_imports)]
use crate::thread_reload::*;
#[allow(unused_imports)]
use crate::trusted_action::*;

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
        if let Some(active) = self.artifact_controller.close() {
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
        let result = Self::artifact_project_root(thread, cx).and_then(|root| {
            self.artifact_controller
                .begin(thread, stream, root)
                .map(|_| ())
        });
        if result.is_err() {
            self.close_artifact_route(GitWorkspaceErrorCode::InvalidRoot, cx);
        }
    }

    pub(crate) fn begin_artifact_agent_generation(
        &mut self,
        generation: u64,
        stream: &Entity<ConversationStream>,
    ) {
        if let Some(active) = self.artifact_controller.active.as_mut()
            && active.identity.stream == *stream
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
        if let Some(active) = self.artifact_controller.active.as_mut()
            && active.identity.stream == *stream
            && active.agent_generation == Some(generation)
        {
            active.proposals.clear();
            active.agent_generation = None;
        }
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
        for event in batch.events {
            self.observe_artifact_event(generation, stream, &event, cx);
            if matches!(event, ConversationEvent::MessageStarted { .. })
                && let Some(content) = self
                    .agent_controller
                    .accept_durable_start(generation, thread_id, stream)
            {
                stream.update(cx, |stream, cx| {
                    stream.accept_composer_submission(&content, cx)
                });
            }
            self.agent_controller
                .observe_terminal_message(generation, thread_id, stream, &event);
            stream.update(cx, |stream, cx| stream.apply_event(event, cx));
        }
        let Some(success) = batch.finished else {
            return AgentBatchIngress::Running;
        };
        self.poison_artifact_agent_generation(generation, stream);
        let Some(run) = self.agent_controller.finish(generation, thread_id, stream) else {
            return AgentBatchIngress::Stale;
        };
        // S7-T40/C4: the run's durable terminal message becomes a read-only
        // per-task cost summary card. The projection reads only the persisted
        // audits (a non-terminal/corrupt row fails closed → no card); the
        // duration is the live in-memory wall-clock measurement and degrades
        // to `—` after a restart.
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
        AgentBatchIngress::Finished { success, run }
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
        let current = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.identity.stream == *stream
                    && active.agent_generation == Some(generation)
                    && Self::artifact_route_is_current(&active.identity, cx)
            });
        if !current {
            return;
        }
        let Some(active) = self.artifact_controller.active.as_mut() else {
            return;
        };
        match event {
            ConversationEvent::ToolCallProposed { call }
                if matches!(call.tool.as_str(), "write" | "edit") =>
            {
                if let Err(failure) = ArtifactService::validate_proposal(call) {
                    self.close_artifact_route(failure.code(), cx);
                    return;
                }
                if let Some(existing) = active.proposals.get_mut(&call.id) {
                    if existing.generation != generation || existing.call.as_ref() != Some(call) {
                        existing.call = None;
                    }
                } else if active.proposals.len() < ARTIFACT_ROUTE_CAP {
                    active.proposals.insert(
                        call.id.clone(),
                        ArtifactProposal {
                            generation,
                            call: Some(call.clone()),
                        },
                    );
                } else {
                    self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
                }
            }
            ConversationEvent::ToolCallFinished { call_id, result } => {
                Self::cancel_artifact_interactions(active, cx);
                let call = active
                    .proposals
                    .remove(call_id)
                    .filter(|proposal| proposal.generation == generation)
                    .and_then(|proposal| proposal.call);
                let work = if let Some(call) = call {
                    match active.service.prepare_capture(&call, result) {
                        Ok(Some(candidate)) => ArtifactTerminalWork::Capture {
                            call_id: call_id.clone(),
                            candidate,
                        },
                        Ok(None) => ArtifactTerminalWork::Refresh,
                        Err(failure) => {
                            let code = failure.code();
                            self.close_artifact_route(code, cx);
                            return;
                        }
                    }
                } else {
                    ArtifactTerminalWork::Refresh
                };
                let Some(sequence) = active.terminal_sequence.checked_add(1) else {
                    self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
                    return;
                };
                if active.terminal_queue.len() >= ARTIFACT_ROUTE_CAP {
                    self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
                    return;
                }
                active.terminal_sequence = sequence;
                active
                    .terminal_queue
                    .push_back(ArtifactTerminalJob { sequence, work });
                self.launch_next_artifact_terminal(cx);
            }
            _ => {}
        }
    }

    pub(crate) fn launch_next_artifact_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(dispatch) = self.take_next_artifact_terminal() else {
            return;
        };
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

    pub(crate) fn take_next_artifact_terminal(&mut self) -> Option<ArtifactTerminalDispatch> {
        let active = self.artifact_controller.active.as_mut()?;
        if active.terminal_in_flight.is_some() {
            return None;
        }
        let job = active.terminal_queue.pop_front()?;
        active.terminal_in_flight = Some(job.sequence);
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
        if !Self::artifact_route_is_current(identity, cx) {
            if self.artifact_controller.matches(identity) {
                self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
            }
            return;
        }
        let expected = self
            .artifact_controller
            .active
            .as_ref()
            .filter(|active| active.identity == *identity)
            .and_then(|active| active.terminal_in_flight);
        let sequence = result.as_ref().ok().map(|(sequence, _)| *sequence);
        if sequence.is_some() && sequence != expected {
            return;
        }
        if let Err(
            code @ (GitWorkspaceErrorCode::ArtifactConflict | GitWorkspaceErrorCode::ArtifactLimit),
        ) = &result
        {
            self.close_artifact_route(*code, cx);
            return;
        }
        let stream = {
            let Some(active) = self.artifact_controller.active.as_mut() else {
                return;
            };
            if active.identity != *identity {
                return;
            }
            active.terminal_in_flight = None;
            active.identity.stream.clone()
        };
        if let Ok((_, result)) = result {
            for projection in result.cards {
                let card = self
                    .artifact_controller
                    .active
                    .as_ref()
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
                    .active
                    .as_ref()
                    .and_then(|active| active.cards.get(&projection.id).cloned());
                if let Some(card) = existing {
                    card.update(cx, |card, cx| {
                        let _ = card.apply_metadata(projection, cx);
                    });
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
                    }) && let Some(active) = self.artifact_controller.active.as_mut()
                    {
                        active.cards.insert(projection.id, card);
                    }
                }
            }
        }
        self.launch_next_artifact_terminal(cx);
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
        let (sender, receiver) = mpsc::sync_channel(1);
        #[cfg(test)]
        ARTIFACT_PREVIEW_WORKER_STARTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
        #[cfg(test)]
        ARTIFACT_OPEN_WORKER_STARTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
