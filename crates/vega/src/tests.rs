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
use crate::trusted_action::*;
#[allow(unused_imports)]
use crate::window::*;

#[allow(unused_imports)]
use std::ffi::OsString;
#[allow(unused_imports)]
use std::fs;
#[allow(unused_imports)]
use std::os::unix::ffi::OsStrExt;
#[allow(unused_imports)]
use std::process::Command;
#[allow(unused_imports)]
use tempfile::TempDir;
#[allow(unused_imports)]
use vega_conversation::types::{
    PermissionMode, PlanReviewAction, PlanStatus, ThreadMode, ThreadStatus, ToolResult,
};
#[allow(unused_imports)]
use vega_store::messages::{MessageRow, complete_plan, insert};

mod agent;
mod artifact_preview;
mod artifact_terminal;
mod branch;
mod commit_controller;
mod commit_panel;
mod diff;
mod history;
mod plan;
mod pricing;

pub(crate) use artifact_terminal::{
    artifact_capture_work, artifact_controller_repo, artifact_write_call, artifact_write_result,
    receive_artifact_terminal,
};
#[allow(unused_imports)]
pub(crate) use diff::{
    configure_fixture_git_environment, diff_controller_repo, fixture_git_command,
    install_diff_window_globals, receive_refresh, run_fixture_git, scrub_fixture_git_environment,
};
pub(crate) use pricing::CommitPanelHarness;

fn pump_test_app(
    cx: &mut gpui::TestAppContext,
    mut ready: impl FnMut(&mut gpui::TestAppContext) -> bool,
) {
    for _ in 0..400 {
        cx.executor().advance_clock(DIFF_RESULT_POLL);
        cx.run_until_parked();
        if ready(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("test app did not reach the expected terminal state");
}

fn pending_plan() -> (Store, String) {
    let store = Store::open(":memory:").expect("memory store");
    store.migrate().expect("migrations");
    let project = vega_store::projects::create(
        store.conn(),
        "/tmp/vega-controller-plan-test",
        "controller",
        None,
    )
    .expect("project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("thread");
    vega_conversation::threads::set_thread_mode(&store, &thread.id, ThreadMode::Plan)
        .expect("plan mode");
    insert(
        store.conn(),
        &MessageRow {
            id: "plan".into(),
            thread_id: thread.id.clone(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .expect("streaming plan");
    complete_plan(
        store.conn(),
        &thread.id,
        "plan",
        "1. inspect\n2. execute",
        2,
    )
    .expect("complete plan");
    (store, thread.id)
}
