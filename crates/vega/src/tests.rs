use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{App, Entity, Focusable, Window, div};
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
    ContextUsageProjection, ConversationStream, HistoryPageRequested, MessageLocationRequested,
    NewerHistoryPageRequested, OpenCommitPanelRequested, OpenWorkspaceDiffRequested,
    ThreadSettingsRequested,
};
use vega_ui::diff_view::{
    DIFF_REFRESH_INTERVAL, DiffProjectionRequested, DiffRetryRequested, DiffView,
};
use vega_ui::plan_card::PlanReviewRequested;
use vega_ui::settings::{
    PricingMutationRequested, ReasoningProfileSaveRequested, SettingsOpen, SettingsSaved,
    SettingsView,
};
use vega_ui::sidebar::{
    OpenedThread, PendingDeleteConfirm, SidebarCollapsed, SidebarWidth, VegaStore,
};

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

use std::fs;
use tempfile::TempDir;
use vega_store::messages::*;

mod agent;
mod artifact_preview;
mod artifact_terminal;
mod automatic_titles;
mod computer_use;
mod project_context;
mod title_fixture;
use title_fixture::with_auxiliary_title_fixture;
mod automatic_reasoning_replay_app;
mod branch;
mod commit_controller;
mod commit_panel;
mod composer_actions;
mod diff;
mod history;

mod model_selection;
mod palette;
mod plan;
mod pricing;

mod reasoning;
mod subagents;
mod updater;
mod user_questions;

pub(crate) use artifact_terminal::{
    artifact_capture_work, artifact_controller_repo, artifact_write_call, artifact_write_result,
    receive_artifact_terminal,
};
pub(crate) use diff::{
    diff_controller_repo, install_diff_window_globals, receive_refresh, run_fixture_git,
};
pub(crate) use pricing::CommitPanelHarness;

#[track_caller]
fn pump_test_app(
    cx: &mut gpui_kit::TestAppContext,
    ready: impl FnMut(&mut gpui_kit::TestAppContext) -> bool,
) {
    pump_test_app_with_diagnostics(cx, ready, |_| String::new());
}

#[track_caller]
fn pump_test_app_with_diagnostics(
    cx: &mut gpui_kit::TestAppContext,
    mut ready: impl FnMut(&mut gpui_kit::TestAppContext) -> bool,
    diagnostics: impl FnOnce(&mut gpui_kit::TestAppContext) -> String,
) {
    for _ in 0..400 {
        cx.executor().advance_clock(DIFF_RESULT_POLL);
        cx.run_until_parked();
        if ready(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "test app did not reach the expected terminal state: {}",
        diagnostics(cx)
    );
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
        None,
    )
    .expect("complete plan");
    (store, thread.id)
}

mod controller_git_fixture;
pub(crate) use controller_git_fixture::{ControllerRepo, fixture_git_command};
