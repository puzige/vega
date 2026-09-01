use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{App, Entity, Focusable, Window, div};
use vega_conversation::types::{
    BranchSnapshot, BranchSwitchCompletion, BranchSwitchOutcome, CommitErrorCode,
    CommitPrepareCompletion, ConversationEvent, GitWorkspaceErrorCode, OpenInOutcome, OpenInTarget,
    Plan, PricingDraftReason, PricingNotice, PricingSettingsErrorCode, Thread, ToolCall,
    WorkspaceSnapshot,
};
use vega_conversation::{
    ArtifactService, BranchWorkspaceService, GitWorkspaceService, PricingSettingsService,
    TrustedGitService,
};
use vega_store::Store;
use vega_theme::*;
use vega_ui::artifact_card::{ArtifactCard, ArtifactOpenRequested, ArtifactPreviewRequested};
use vega_ui::branch_selector::{BranchSelectorClosed, BranchSwitchRequested};
use vega_ui::commit_panel::{
    CommitDraftRequested, CommitPanel, CommitPanelClosed, CommitPrepareRequested, CommitRequested,
};
use vega_ui::conversation_stream::{
    ConversationStream, HistoryPageRequested, OpenCommitPanelRequested, OpenWorkspaceDiffRequested,
};
use vega_ui::diff_view::{DIFF_REFRESH_INTERVAL, DiffProjectionRequested, DiffView};
use vega_ui::plan_card::PlanReviewRequested;
use vega_ui::settings::{PricingMutationRequested, SettingsOpen};
use vega_ui::sidebar::{OpenedThread, PendingDeleteConfirm, SidebarCollapsed, VegaStore};

use vega_conversation::types::*;

// Controller facades: the test modules reach everything through `super::*`.
use crate::app_agent::*;
use crate::artifact_controller::*;
use crate::branch_controller::*;
use crate::commit_controller::*;
use crate::diff_controller::*;
use crate::thread_reload::*;
use crate::trusted_action::*;
use crate::window::*;

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::process::Command;
use tempfile::TempDir;
use vega_store::messages::*;

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
pub(crate) use diff::{
    diff_controller_repo, fixture_git_command, install_diff_window_globals, receive_refresh,
    run_fixture_git,
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
