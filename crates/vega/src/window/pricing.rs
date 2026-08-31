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
    pub(crate) fn start_pricing_load(&mut self, cx: &mut Context<Self>) {
        let Some(service) = self.pricing_controller.service.clone() else {
            return;
        };
        let Some(operation) = self.pricing_controller.begin_operation() else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::LimitExceeded);
            return;
        };
        self.pricing_controller.state = PricingControllerState::Loading;
        self.spawn_pricing_worker(
            operation,
            PricingWorkerKind::Authority,
            move || PricingWorkerResult::Authority(service.load_or_seed()),
            cx,
        );
    }

    pub(crate) fn request_pricing_reload(
        &mut self,
        view: Entity<SettingsView>,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.pricing_controller.active_operation.is_some()
        {
            return;
        }
        let Some(service) = self.pricing_controller.service.clone() else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::Io);
            self.push_pricing_projection(cx);
            return;
        };
        let Some(operation) = self.pricing_controller.begin_operation() else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::LimitExceeded);
            self.push_pricing_projection(cx);
            return;
        };
        self.pricing_controller.state = PricingControllerState::Reloading;
        self.push_pricing_projection(cx);
        self.spawn_pricing_worker(
            operation,
            PricingWorkerKind::Authority,
            move || PricingWorkerResult::Authority(service.reload()),
            cx,
        );
    }

    pub(crate) fn request_pricing_mutation(
        &mut self,
        view: Entity<SettingsView>,
        request: &PricingMutationRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.pricing_controller.active_operation.is_some()
        {
            return;
        }
        let (previous, generation, notice, draft, draft_reason) =
            match &self.pricing_controller.state {
                PricingControllerState::Ready {
                    authority,
                    generation,
                    notice,
                    draft,
                    draft_reason,
                    ..
                } => (
                    authority.clone(),
                    *generation,
                    *notice,
                    draft.clone(),
                    *draft_reason,
                ),
                _ => return,
            };
        if generation != request.generation {
            return;
        }
        if draft.is_some() {
            self.pricing_controller.state = PricingControllerState::Ready {
                authority: previous,
                generation,
                notice,
                draft,
                draft_reason,
                error: Some(PricingSettingsErrorCode::Busy),
            };
            self.push_pricing_projection(cx);
            return;
        }
        let mutation = match &request.mutation {
            Ok(mutation) => mutation.clone(),
            Err(code) => {
                self.pricing_controller.state = PricingControllerState::Ready {
                    authority: previous,
                    generation,
                    notice,
                    draft: None,
                    draft_reason: None,
                    error: Some(*code),
                };
                self.push_pricing_projection(cx);
                return;
            }
        };
        let Some(service) = self.pricing_controller.service.clone() else {
            return;
        };
        let plan = match service.prepare_save(&previous, mutation) {
            Ok(plan) => plan,
            Err(code) => {
                self.pricing_controller.state = PricingControllerState::Ready {
                    authority: previous,
                    generation,
                    notice,
                    draft: None,
                    draft_reason: None,
                    error: Some(code),
                };
                self.push_pricing_projection(cx);
                return;
            }
        };
        self.begin_pricing_save(previous, notice, generation, plan, service, cx);
    }

    pub(crate) fn begin_pricing_save(
        &mut self,
        previous: PricingAuthority,
        previous_notice: Option<PricingNotice>,
        generation: u64,
        plan: PricingSavePlan,
        service: Arc<PricingSettingsService>,
        cx: &mut Context<Self>,
    ) {
        let Some(operation) = self.pricing_controller.begin_operation() else {
            self.pricing_controller.state = pricing_retry_ready(
                previous,
                generation,
                previous_notice,
                plan,
                PricingSettingsErrorCode::LimitExceeded,
            );
            self.push_pricing_projection(cx);
            return;
        };
        self.pricing_controller.state = PricingControllerState::Saving {
            previous,
            previous_notice,
            generation,
            plan: plan.clone(),
        };
        self.push_pricing_projection(cx);
        self.spawn_pricing_worker(
            operation,
            PricingWorkerKind::Save,
            move || PricingWorkerResult::Save(service.save(&plan)),
            cx,
        );
    }

    pub(crate) fn request_pricing_retry(
        &mut self,
        view: Entity<SettingsView>,
        request: &PricingRetryRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.pricing_controller.active_operation.is_some()
        {
            return;
        }
        let (authority, notice, generation, plan) = match &self.pricing_controller.state {
            PricingControllerState::Ready {
                authority,
                notice,
                generation,
                draft: Some(plan),
                ..
            } if *generation == request.generation => {
                (authority.clone(), *notice, *generation, plan.clone())
            }
            _ => return,
        };
        let Some(service) = self.pricing_controller.service.clone() else {
            return;
        };
        self.begin_pricing_save(authority, notice, generation, plan, service, cx);
    }

    pub(crate) fn request_pricing_discard(
        &mut self,
        view: Entity<SettingsView>,
        request: &PricingDiscardRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.pricing_controller.active_operation.is_some()
        {
            return;
        }
        if !discard_pricing_draft(&mut self.pricing_controller.state, request.generation) {
            return;
        }
        self.push_pricing_projection(cx);
    }

    pub(crate) fn spawn_pricing_worker(
        &mut self,
        operation: u64,
        kind: PricingWorkerKind,
        worker: impl FnOnce() -> PricingWorkerResult + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let (sender, receiver) = mpsc::sync_channel(1);
        #[cfg(test)]
        let drop_result = std::mem::replace(&mut self.pricing_drop_next_worker_result, false);
        #[cfg(test)]
        let worker_gate = self.pricing_next_worker_gate.take();
        #[cfg(not(test))]
        let drop_result = false;
        let spawned = std::thread::Builder::new()
            .name("vega-pricing".to_string())
            .spawn(move || {
                #[cfg(test)]
                if let Some(gate) = worker_gate {
                    gate.wait();
                }
                let result = worker();
                if !drop_result {
                    let _ = sender.send(result);
                }
            });
        if spawned.is_err() {
            self.apply_pricing_worker_not_started(operation, kind, cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                match receiver.try_recv() {
                    Ok(result) => {
                        let _ = this.update(cx, |this, cx| {
                            this.apply_pricing_worker_result(operation, result, cx);
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor().timer(PRICING_RESULT_POLL).await;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |this, cx| {
                            this.apply_pricing_worker_disconnected(operation, kind, cx);
                        });
                        break;
                    }
                }
            }
        })
        .detach();
    }

    pub(crate) fn apply_pricing_worker_result(
        &mut self,
        operation: u64,
        result: PricingWorkerResult,
        cx: &mut Context<Self>,
    ) {
        if !self.pricing_controller.claim_completion(operation) {
            return;
        }
        match result {
            PricingWorkerResult::Authority(Ok(outcome)) => {
                let Some(generation) = self.pricing_controller.next_generation() else {
                    self.pricing_controller.state =
                        PricingControllerState::Invalid(PricingSettingsErrorCode::LimitExceeded);
                    self.push_pricing_projection(cx);
                    return;
                };
                self.pricing_controller.state = PricingControllerState::Ready {
                    authority: outcome.authority,
                    generation,
                    notice: outcome.notice,
                    draft: None,
                    draft_reason: None,
                    error: None,
                };
            }
            PricingWorkerResult::Authority(Err(code)) => {
                self.pricing_controller.state = PricingControllerState::Invalid(code);
            }
            PricingWorkerResult::Save(outcome) => {
                let old = std::mem::replace(
                    &mut self.pricing_controller.state,
                    PricingControllerState::Invalid(PricingSettingsErrorCode::Io),
                );
                let PricingControllerState::Saving {
                    previous,
                    previous_notice,
                    generation,
                    plan,
                } = old
                else {
                    return;
                };
                match outcome {
                    PricingSaveOutcome::Ready {
                        authority,
                        notice,
                        dirty_conflict,
                    } => {
                        let Some(new_generation) = self.pricing_controller.next_generation() else {
                            self.pricing_controller.state = PricingControllerState::Invalid(
                                PricingSettingsErrorCode::LimitExceeded,
                            );
                            self.push_pricing_projection(cx);
                            return;
                        };
                        self.pricing_controller.state = PricingControllerState::Ready {
                            authority,
                            generation: new_generation,
                            notice,
                            draft: dirty_conflict.then_some(plan),
                            draft_reason: dirty_conflict
                                .then_some(PricingDraftReason::ExternalConflict),
                            error: None,
                        };
                    }
                    PricingSaveOutcome::PreCommitFailure(code) => {
                        self.pricing_controller.state =
                            pricing_retry_ready(previous, generation, previous_notice, plan, code);
                    }
                    PricingSaveOutcome::RecoveryRequired => {
                        self.pricing_controller.state = PricingControllerState::Invalid(
                            PricingSettingsErrorCode::RecoveryRequired,
                        );
                    }
                }
            }
        }
        self.push_pricing_projection(cx);
    }

    pub(crate) fn apply_pricing_worker_not_started(
        &mut self,
        operation: u64,
        kind: PricingWorkerKind,
        cx: &mut Context<Self>,
    ) {
        if !self.pricing_controller.claim_completion(operation) {
            return;
        }
        let code = match kind {
            PricingWorkerKind::Recovery => PricingSettingsErrorCode::RecoveryRequired,
            PricingWorkerKind::Authority | PricingWorkerKind::Save => PricingSettingsErrorCode::Io,
        };
        let old = std::mem::replace(
            &mut self.pricing_controller.state,
            PricingControllerState::Invalid(code),
        );
        if matches!(kind, PricingWorkerKind::Save)
            && let PricingControllerState::Saving {
                previous,
                previous_notice,
                generation,
                plan,
            } = old
        {
            self.pricing_controller.state =
                pricing_retry_ready(previous, generation, previous_notice, plan, code);
        }
        self.push_pricing_projection(cx);
    }

    pub(crate) fn apply_pricing_worker_disconnected(
        &mut self,
        operation: u64,
        kind: PricingWorkerKind,
        cx: &mut Context<Self>,
    ) {
        if !self.pricing_controller.claim_completion(operation) {
            return;
        }
        if !matches!(kind, PricingWorkerKind::Save) {
            self.pricing_controller.state = PricingControllerState::Invalid(match kind {
                PricingWorkerKind::Recovery => PricingSettingsErrorCode::RecoveryRequired,
                PricingWorkerKind::Authority | PricingWorkerKind::Save => {
                    PricingSettingsErrorCode::Io
                }
            });
            self.push_pricing_projection(cx);
            return;
        }
        let PricingControllerState::Saving { plan, .. } = &self.pricing_controller.state else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::RecoveryRequired);
            self.push_pricing_projection(cx);
            return;
        };
        let plan = plan.clone();
        let Some(service) = self.pricing_controller.service.clone() else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::RecoveryRequired);
            self.push_pricing_projection(cx);
            return;
        };
        let Some(recovery_operation) = self.pricing_controller.begin_operation() else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::RecoveryRequired);
            self.push_pricing_projection(cx);
            return;
        };
        self.spawn_pricing_worker(
            recovery_operation,
            PricingWorkerKind::Recovery,
            move || PricingWorkerResult::Save(service.recover_started_save(&plan)),
            cx,
        );
    }

    pub(crate) fn push_pricing_projection(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = &self.settings_view {
            let projection = self.pricing_controller.projection();
            view.update(cx, |view, cx| {
                view.apply_pricing_projection(projection, cx);
            });
        }
        cx.notify();
    }
}
