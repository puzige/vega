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
    /// Whether the viewport is narrower than the auto-collapse threshold
    /// (ui-spec §1). Reads the live viewport size: every platform resize is
    /// delivered as an event (`Window::bounds_changed` → redraw), so each
    /// render sees the current size and no polling is involved.
    pub(crate) fn auto_collapsed(&self, window: &Window) -> bool {
        window.viewport_size().width < px(AUTO_COLLAPSE_WIDTH)
    }

    /// Cmd+N entry point: creates a thread in the selected project and opens
    /// it (the sidebar [新建任务] button shares this handler).
    pub(crate) fn open_new_thread(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.update(cx, Sidebar::create_thread);
    }

    /// A2-14: persists the composer's model selection as the app-level
    /// default model (config file, no DDL, no thread-row write).
    pub(crate) fn persist_composer_defaults(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerDefaultsRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let model = request.defaults.model.clone();
        let _ = vega_store::config::load().map(|mut config| {
            config.defaults.model = model;
            let _ = config.save();
        });
        stream.update(cx, |stream, cx| {
            stream.apply_composer_defaults(request.defaults.clone(), cx)
        });
    }

    pub(crate) fn persist_thread_settings(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ThreadSettingsRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let thread_id = request.thread_id.clone();
        let result = match &cx.global::<VegaStore>().0 {
            Ok(store) => (|| {
                if let Some(mode) = request.mode {
                    vega_conversation::threads::set_thread_mode(store, &thread_id, mode)?;
                }
                if let Some(permission_mode) = request.permission_mode {
                    vega_conversation::threads::set_thread_permission_mode(
                        store,
                        &thread_id,
                        permission_mode,
                    )?;
                }
                vega_conversation::threads::open_thread(store, &thread_id)
            })()
            .map_err(|error| error.to_string()),
            Err(error) => Err(error.clone()),
        };
        match result {
            Ok(thread) => {
                cx.set_global(OpenedThread(Some(thread.clone())));
                stream.update(cx, |stream, cx| stream.apply_thread(thread, cx));
            }
            Err(_) => stream.update(cx, ConversationStream::apply_controller_error),
        }
    }

    pub(crate) fn review_plan(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &PlanReviewRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        if self.trusted_actions.is_busy() {
            stream.update(cx, ConversationStream::apply_controller_error);
            return;
        }
        if self.agent_controller.active.is_some() {
            if self.agent_controller.queue_review(&stream, request) {
                if let Some(active) = self.agent_controller.active.as_ref() {
                    self.poison_artifact_agent_generation(active.generation, &stream);
                }
                stream.update(cx, |stream, cx| stream.timeout_permission(cx));
            } else {
                stream.update(cx, ConversationStream::apply_controller_error);
            }
            return;
        }
        let result = match &cx.global::<VegaStore>().0 {
            Ok(store) => persist_review(store, request),
            Err(error) => Err(error.clone()),
        };
        match result {
            Ok(refresh) => {
                let approved_instruction_id = refresh.approved_instruction_id.clone();
                Self::apply_refresh(&stream, refresh.thread, refresh.plans, cx);
                if let Some(instruction_message_id) = approved_instruction_id {
                    self.start_agent_run(
                        stream,
                        &request.thread_id,
                        PendingAgentRun::ApprovedPlan(instruction_message_id),
                        cx,
                    );
                }
            }
            Err(_) => {
                // A SQLite error may be commit-ambiguous. Reload authoritative
                // state before deciding whether the card may be re-armed.
                let reload = match &cx.global::<VegaStore>().0 {
                    Ok(store) => reload_thread_and_plans(store, &request.thread_id),
                    Err(error) => Err(error.clone()),
                };
                if let Ok((thread, plans)) = reload {
                    Self::apply_refresh(&stream, thread, plans, cx);
                }
                stream.update(cx, ConversationStream::apply_controller_error);
            }
        }
    }
}
