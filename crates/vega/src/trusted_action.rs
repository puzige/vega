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
use crate::window::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Commit is the frozen T34 seam sharing this coordinator.
pub(crate) enum TrustedActionKind {
    BranchSwitch,
    ArtifactOpen,
    Commit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrustedActionToken {
    pub(crate) generation: u64,
    pub(crate) kind: TrustedActionKind,
    pub(crate) owner_epoch: u64,
    pub(crate) request_sequence: u64,
}

#[derive(Default)]
pub(crate) struct TrustedActionState {
    pub(crate) next_generation: u64,
    pub(crate) active: Option<TrustedActionToken>,
}

#[derive(Clone, Default)]
pub(crate) struct TrustedActionCoordinator {
    pub(crate) state: Arc<Mutex<TrustedActionState>>,
}

impl TrustedActionCoordinator {
    pub(crate) fn acquire(
        &self,
        kind: TrustedActionKind,
        owner_epoch: u64,
        request_sequence: u64,
    ) -> Option<TrustedActionToken> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active.is_some() {
            return None;
        }
        let generation = state.next_generation.checked_add(1)?;
        state.next_generation = generation;
        let token = TrustedActionToken {
            generation,
            kind,
            owner_epoch,
            request_sequence,
        };
        state.active = Some(token);
        Some(token)
    }

    pub(crate) fn release(&self, token: TrustedActionToken) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active != Some(token) {
            return false;
        }
        state.active = None;
        true
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active
            .is_some()
    }

    #[cfg(test)]
    pub(crate) fn active_token(&self) -> Option<TrustedActionToken> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active
    }
}
