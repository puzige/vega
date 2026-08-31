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
use crate::branch_controller::*;
use crate::commit_controller::*;
use crate::diff_controller::*;
use crate::pricing_controller::*;
use crate::thread_reload::*;
use crate::trusted_action::*;
use crate::window::*;

pub(crate) const ARTIFACT_ROUTE_CAP: usize = 10_000;
#[cfg(test)]
pub(crate) static ARTIFACT_OPEN_WORKER_STARTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
pub(crate) static ARTIFACT_PREVIEW_WORKER_STARTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ArtifactRouteIdentity {
    pub(crate) epoch: u64,
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
    pub(crate) stream: Entity<ConversationStream>,
}

pub(crate) struct ArtifactTerminalJob {
    pub(crate) sequence: u64,
    pub(crate) work: ArtifactTerminalWork,
}

pub(crate) struct ArtifactTerminalDispatch {
    pub(crate) identity: ArtifactRouteIdentity,
    pub(crate) workspace: Arc<GitWorkspaceService>,
    pub(crate) service: Arc<ArtifactService>,
    pub(crate) job: ArtifactTerminalJob,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
}

pub(crate) enum ArtifactTerminalWork {
    Refresh,
    Capture {
        call_id: String,
        candidate: ArtifactCaptureCandidate,
    },
}

pub(crate) struct ArtifactProposal {
    pub(crate) generation: u64,
    pub(crate) call: Option<ToolCall>,
}

pub(crate) struct ArtifactTerminalResult {
    pub(crate) captured: Option<(String, ArtifactProjection)>,
    pub(crate) cards: Vec<ArtifactProjection>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ArtifactPreviewFence {
    pub(crate) route: ArtifactRouteIdentity,
    pub(crate) sequence: u64,
    pub(crate) card_id: ArtifactCardId,
    pub(crate) file_id: WorkspaceFileId,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ArtifactOpenFence {
    pub(crate) route: ArtifactRouteIdentity,
    pub(crate) sequence: u64,
    pub(crate) card_id: ArtifactCardId,
    pub(crate) file_id: WorkspaceFileId,
    pub(crate) target: OpenInTarget,
    pub(crate) lease: TrustedActionToken,
}

pub(crate) struct ActiveArtifactRoute {
    pub(crate) identity: ArtifactRouteIdentity,
    pub(crate) workspace: Arc<GitWorkspaceService>,
    pub(crate) service: Arc<ArtifactService>,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
    pub(crate) agent_generation: Option<u64>,
    pub(crate) proposals: HashMap<String, ArtifactProposal>,
    pub(crate) terminal_sequence: u64,
    pub(crate) terminal_in_flight: Option<u64>,
    pub(crate) terminal_queue: VecDeque<ArtifactTerminalJob>,
    pub(crate) cards: HashMap<ArtifactCardId, Entity<ArtifactCard>>,
    pub(crate) preview_sequence: u64,
    pub(crate) preview_fence: Option<ArtifactPreviewFence>,
    pub(crate) preview_cancel: Option<tokio_util::sync::CancellationToken>,
    pub(crate) open_sequence: u64,
    pub(crate) open_fence: Option<ArtifactOpenFence>,
    pub(crate) open_cancel: Option<tokio_util::sync::CancellationToken>,
}

#[derive(Default)]
pub(crate) struct ArtifactController {
    pub(crate) next_route_epoch: u64,
    pub(crate) active: Option<ActiveArtifactRoute>,
}

impl ArtifactController {
    pub(crate) fn begin(
        &mut self,
        thread: &Thread,
        stream: Entity<ConversationStream>,
        root: PathBuf,
    ) -> Result<ArtifactRouteIdentity, GitWorkspaceErrorCode> {
        if self.active.is_some() {
            return Err(GitWorkspaceErrorCode::ArtifactConflict);
        }
        let epoch = self
            .next_route_epoch
            .checked_add(1)
            .ok_or(GitWorkspaceErrorCode::ArtifactLimit)?;
        let workspace = Arc::new(GitWorkspaceService::new(root).map_err(|failure| failure.code())?);
        let service = Arc::new(
            ArtifactService::new(
                workspace.clone(),
                thread.project_id.clone(),
                thread.id.clone(),
                epoch,
            )
            .map_err(|failure| failure.code())?,
        );
        self.next_route_epoch = epoch;
        let identity = ArtifactRouteIdentity {
            epoch,
            thread_id: thread.id.clone(),
            project_id: thread.project_id.clone(),
            stream,
        };
        self.active = Some(ActiveArtifactRoute {
            identity: identity.clone(),
            workspace,
            service,
            cancel: tokio_util::sync::CancellationToken::new(),
            agent_generation: None,
            proposals: HashMap::new(),
            terminal_sequence: 0,
            terminal_in_flight: None,
            terminal_queue: VecDeque::new(),
            cards: HashMap::new(),
            preview_sequence: 0,
            preview_fence: None,
            preview_cancel: None,
            open_sequence: 0,
            open_fence: None,
            open_cancel: None,
        });
        Ok(identity)
    }

    pub(crate) fn close(&mut self) -> Option<ActiveArtifactRoute> {
        let active = self.active.take();
        if let Some(active) = &active {
            active.cancel.cancel();
            if let Some(cancel) = &active.preview_cancel {
                cancel.cancel();
            }
            if let Some(cancel) = &active.open_cancel {
                cancel.cancel();
            }
        }
        active
    }

    pub(crate) fn matches(&self, identity: &ArtifactRouteIdentity) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.identity == *identity)
    }
}

pub(crate) fn run_artifact_terminal_worker(
    workspace: Arc<GitWorkspaceService>,
    service: Arc<ArtifactService>,
    job: ArtifactTerminalJob,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<Result<(u64, ArtifactTerminalResult), GitWorkspaceErrorCode>>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime.block_on(async {
                let refreshed = workspace.refresh(cancel.child_token()).await;
                service
                    .reconcile(cancel.child_token())
                    .await
                    .map_err(|failure| failure.code())?;
                if let Err(failure) = refreshed {
                    if failure.code() == GitWorkspaceErrorCode::Cancelled {
                        return Err(failure.code());
                    }
                    return Ok((
                        job.sequence,
                        ArtifactTerminalResult {
                            captured: None,
                            cards: service.cards(),
                        },
                    ));
                }
                let captured = match job.work {
                    ArtifactTerminalWork::Capture { call_id, candidate } => service
                        .capture_candidate(candidate, cancel.child_token())
                        .await
                        .map_err(|failure| failure.code())?
                        .map(|card| (call_id, card)),
                    ArtifactTerminalWork::Refresh => None,
                };
                let cards = service
                    .reconcile(cancel)
                    .await
                    .map_err(|failure| failure.code())?;
                Ok((job.sequence, ArtifactTerminalResult { captured, cards }))
            })
        });
    let _ = sender.send(result);
}

pub(crate) fn run_artifact_preview_worker(
    service: Arc<ArtifactService>,
    fence: ArtifactPreviewFence,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<(
        ArtifactPreviewFence,
        Result<ArtifactPreviewProjection, GitWorkspaceErrorCode>,
    )>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime
                .block_on(service.preview(fence.card_id, cancel))
                .map_err(|failure| failure.code())
        });
    let _ = sender.send((fence, result));
}

pub(crate) fn run_artifact_open_worker(
    service: Arc<ArtifactService>,
    fence: ArtifactOpenFence,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<(
        ArtifactOpenFence,
        Result<OpenInOutcome, GitWorkspaceErrorCode>,
    )>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime
                .block_on(service.open_in(fence.card_id, fence.target, cancel))
                .map_err(|failure| failure.code())
        });
    let _ = sender.send((fence, result));
}
