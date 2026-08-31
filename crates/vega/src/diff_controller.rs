use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

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
use crate::pricing_controller::*;
use crate::thread_reload::*;
use crate::trusted_action::*;
use crate::window::*;

pub(crate) const DIFF_RESULT_POLL: Duration = Duration::from_millis(4);

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct DiffRouteIdentity {
    pub(crate) epoch: u64,
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct DiffProjectionFence {
    pub(crate) route: DiffRouteIdentity,
    pub(crate) refresh_request_seq: u64,
    pub(crate) snapshot_generation: u64,
    pub(crate) file_request_seq: u64,
    pub(crate) file_id: WorkspaceFileId,
}

pub(crate) struct PendingDiffProjection {
    pub(crate) fence: DiffProjectionFence,
    pub(crate) result: Result<DiffTextProjection, GitWorkspaceErrorCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffProjectionDisposition {
    Apply,
    Defer,
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffRefreshDecision {
    Start(u64),
    Coalesced,
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffRefreshCompletion {
    Latest,
    Superseded(Option<u64>),
}

pub(crate) struct ActiveDiffRoute {
    pub(crate) identity: DiffRouteIdentity,
    pub(crate) view: Entity<DiffView>,
    pub(crate) service: Option<Arc<GitWorkspaceService>>,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
    pub(crate) refresh_request_seq: u64,
    pub(crate) refresh_in_flight: Option<u64>,
    pub(crate) queued_refresh_seq: Option<u64>,
    pub(crate) snapshot_generation: Option<u64>,
    pub(crate) file_request_seq: u64,
    pub(crate) requested_file: Option<WorkspaceFileId>,
    pub(crate) projection_cancel: Option<tokio_util::sync::CancellationToken>,
    pub(crate) pending_projection: Option<PendingDiffProjection>,
    pub(crate) focus_pending: bool,
}

impl ActiveDiffRoute {
    pub(crate) fn request_refresh(&mut self) -> DiffRefreshDecision {
        let Some(next) = self.refresh_request_seq.checked_add(1) else {
            return DiffRefreshDecision::Overflow;
        };
        self.refresh_request_seq = next;
        if self.refresh_in_flight.is_some() {
            self.queued_refresh_seq = Some(next);
            return DiffRefreshDecision::Coalesced;
        }
        self.refresh_in_flight = Some(next);
        DiffRefreshDecision::Start(next)
    }

    pub(crate) fn complete_refresh(&mut self, request_seq: u64) -> Option<DiffRefreshCompletion> {
        if self.refresh_in_flight != Some(request_seq) {
            return None;
        }
        self.refresh_in_flight = None;
        let queued = self.queued_refresh_seq.take();
        if request_seq == self.refresh_request_seq {
            Some(DiffRefreshCompletion::Latest)
        } else {
            if let Some(next) = queued {
                self.refresh_in_flight = Some(next);
            }
            Some(DiffRefreshCompletion::Superseded(queued))
        }
    }

    pub(crate) fn next_projection_fence(
        &mut self,
        generation: u64,
        file_id: WorkspaceFileId,
    ) -> Option<DiffProjectionFence> {
        if self.snapshot_generation != Some(generation) {
            return None;
        }
        let next = self.file_request_seq.checked_add(1)?;
        self.file_request_seq = next;
        self.requested_file = Some(file_id);
        Some(DiffProjectionFence {
            route: self.identity.clone(),
            refresh_request_seq: self.refresh_request_seq,
            snapshot_generation: generation,
            file_request_seq: next,
            file_id,
        })
    }

    pub(crate) fn projection_disposition(
        &self,
        fence: &DiffProjectionFence,
    ) -> DiffProjectionDisposition {
        let current = self.identity == fence.route
            && self.snapshot_generation == Some(fence.snapshot_generation)
            && self.file_request_seq == fence.file_request_seq
            && self.requested_file == Some(fence.file_id)
            && fence.refresh_request_seq <= self.refresh_request_seq;
        if !current {
            DiffProjectionDisposition::Drop
        } else if self.refresh_in_flight.is_some() {
            DiffProjectionDisposition::Defer
        } else {
            DiffProjectionDisposition::Apply
        }
    }
}

#[derive(Default)]
pub(crate) struct DiffController {
    pub(crate) next_route_epoch: u64,
    pub(crate) active: Option<ActiveDiffRoute>,
}

impl DiffController {
    pub(crate) fn begin(
        &mut self,
        thread_id: String,
        project_id: String,
        view: Entity<DiffView>,
    ) -> Option<DiffRouteIdentity> {
        self.close();
        let epoch = self.next_route_epoch.checked_add(1)?;
        self.next_route_epoch = epoch;
        let identity = DiffRouteIdentity {
            epoch,
            thread_id,
            project_id,
        };
        self.active = Some(ActiveDiffRoute {
            identity: identity.clone(),
            view,
            service: None,
            cancel: tokio_util::sync::CancellationToken::new(),
            refresh_request_seq: 0,
            refresh_in_flight: None,
            queued_refresh_seq: None,
            snapshot_generation: None,
            file_request_seq: 0,
            requested_file: None,
            projection_cancel: None,
            pending_projection: None,
            focus_pending: true,
        });
        Some(identity)
    }

    pub(crate) fn close(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancel.cancel();
            if let Some(cancel) = active.projection_cancel {
                cancel.cancel();
            }
        }
    }

    pub(crate) fn matches(&self, identity: &DiffRouteIdentity) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.identity == *identity)
    }

    pub(crate) fn visible_view(&self, thread: &Thread) -> Option<Entity<DiffView>> {
        self.active.as_ref().and_then(|active| {
            (active.identity.thread_id == thread.id
                && active.identity.project_id == thread.project_id)
                .then(|| active.view.clone())
        })
    }
}

pub(crate) enum DiffRefreshWorkerResult {
    Ready {
        service: Arc<GitWorkspaceService>,
        snapshot: WorkspaceSnapshot,
    },
    Failed(GitWorkspaceErrorCode),
}

pub(crate) fn run_diff_refresh_worker(
    service: Option<Arc<GitWorkspaceService>>,
    root: Option<PathBuf>,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<DiffRefreshWorkerResult>,
) {
    let result = (|| {
        let service = match service {
            Some(service) => service,
            None => Arc::new(
                GitWorkspaceService::new(root.ok_or(GitWorkspaceErrorCode::InvalidRoot)?)
                    .map_err(|error| error.code())?,
            ),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)?;
        let snapshot = runtime
            .block_on(service.refresh(cancel))
            .map_err(|error| error.code())?;
        Ok::<_, GitWorkspaceErrorCode>((service, snapshot))
    })();
    let output = match result {
        Ok((service, snapshot)) => DiffRefreshWorkerResult::Ready { service, snapshot },
        Err(code) => DiffRefreshWorkerResult::Failed(code),
    };
    let _ = sender.send(output);
}

pub(crate) fn run_diff_projection_worker(
    service: Arc<GitWorkspaceService>,
    file_id: WorkspaceFileId,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<Result<DiffTextProjection, GitWorkspaceErrorCode>>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime
                .block_on(service.diff(file_id, cancel))
                .map_err(|error| error.code())
        });
    let _ = sender.send(result);
}
