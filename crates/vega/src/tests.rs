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
    ConversationStream, HistoryPageRequested, OpenCommitPanelRequested, OpenWorkspaceDiffRequested,
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
mod title_fixture;
use title_fixture::with_auxiliary_title_fixture;
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
mod r69;
mod reasoning;

pub(crate) use artifact_terminal::{
    artifact_capture_work, artifact_controller_repo, artifact_write_call, artifact_write_result,
    receive_artifact_terminal,
};
pub(crate) use diff::{
    diff_controller_repo, install_diff_window_globals, receive_refresh, run_fixture_git,
};
pub(crate) use pricing::CommitPanelHarness;

#[test]
fn r21_default_window_geometry_is_frozen() {
    assert_eq!(crate::WINDOW_INITIAL_WIDTH, 1403.0);
    assert_eq!(crate::WINDOW_INITIAL_HEIGHT, 860.0);
    assert_eq!(crate::WINDOW_MIN_WIDTH, 960.0);
    assert_eq!(crate::WINDOW_MIN_HEIGHT, 600.0);
}

// ---------------------------------------------------------------------------
// R52 (docs/vega-r52-test-gate.md §2.5): the load-sensitive disabled set is
// frozen. Rust has no runtime reflection over test attributes, so this gate
// scans the workspace sources instead: the tree may contain exactly the ten
// `ignore` attributes below, each must carry a reason starting with
// `load-sensitive:`, and each must sit on one of the ten named tests. Any
// silent growth of the disabled set fails here instead of quietly weakening
// the parallel gate.
// ---------------------------------------------------------------------------

/// The ten tests R52 disables, each asserting a wall-clock budget that is
/// only reliable when the test binary runs without parallel CPU contention.
///
/// `issue73_connection_deadline_covers_probe_and_catalog_together` was added
/// to this set on 2026-09-22 (issue #123): it asserts a shared-deadline
/// wall-clock budget (probe <280ms, total <700ms) and was observed flaking
/// under CI parallel load, blocking PR check. The test body and its
/// assertions are unchanged; it runs via `--ignored`.
const R52_LOAD_SENSITIVE_TESTS: [&str; 10] = [
    "lone_text_delta_flushes_during_provider_stall_within_sixteen_ms",
    "cancellation_is_persisted_as_interrupted_under_one_second",
    "duplicate_stop_races_converge_to_exactly_one_terminal_event",
    "one_hundred_case_delay_matrix_converges_with_p99_under_one_second",
    "cancellation_stops_a_delayed_provider_under_one_second",
    "retry_429_honors_retry_after_header",
    "cancel_during_backoff_aborts_without_another_request",
    "cancel_mid_stream_stops_immediately_with_no_further_events",
    "draft_deadline_covers_setup_pre_done_and_post_done_stalls",
    "issue73_connection_deadline_covers_probe_and_catalog_together",
];

/// Resolve source paths from nextest's remapped runtime manifest directory.
/// Cargo test uses the compile-time manifest directory when no remap is present.
fn r52_workspace_root() -> PathBuf {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    manifest_dir
        .parent()
        .and_then(|crates| crates.parent())
        .expect("vega manifest lives under <workspace>/crates/vega")
        .to_path_buf()
}

fn r52_collect_rust_sources(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read source dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            r52_collect_rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn r52_load_sensitive_ignores_are_frozen() {
    let root = r52_workspace_root();
    let mut sources = Vec::new();
    for member in ["crates", "xtask"] {
        r52_collect_rust_sources(&root.join(member), &mut sources);
    }
    assert!(!sources.is_empty(), "workspace scan found no Rust sources");

    // Assembled at runtime so this test's own source cannot match the needle.
    let needle = concat!("#[", "ignore");
    let reason_prefix = "load-sensitive:";
    let mut disabled = Vec::new();
    for path in &sources {
        let text = fs::read_to_string(path).expect("read source");
        for (offset, _) in text.match_indices(needle) {
            let line = text[..offset].matches('\n').count() + 1;
            let rest = text[offset + needle.len()..].trim_start();
            let rest = rest.strip_prefix('=').unwrap_or_else(|| {
                panic!("{}:{line}: {needle} must carry a reason", path.display())
            });
            let reason = rest.trim_start().strip_prefix('"').unwrap_or_else(|| {
                panic!(
                    "{}:{line}: {needle} reason must be a string literal",
                    path.display()
                )
            });
            assert!(
                reason.starts_with(reason_prefix),
                "{}:{line}: {needle} reason must start with `{reason_prefix}`; the disabled set \
                 may only hold load-sensitive tests",
                path.display()
            );
            let name = text[offset..]
                .lines()
                .map(str::trim_start)
                .find(|candidate| {
                    candidate.starts_with("async fn ") || candidate.starts_with("fn ")
                })
                .and_then(|signature| {
                    let word = if signature.starts_with("async") { 2 } else { 1 };
                    signature.split_whitespace().nth(word)
                })
                .and_then(|signature| signature.split('(').next())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    panic!(
                        "{}:{line}: {needle} is not attached to a fn",
                        path.display()
                    )
                });
            disabled.push(name);
        }
    }

    disabled.sort();
    let mut expected = R52_LOAD_SENSITIVE_TESTS.map(str::to_owned).to_vec();
    expected.sort();
    assert_eq!(
        disabled,
        expected,
        "the disabled test set changed; R52 froze exactly these {} load-sensitive tests",
        expected.len()
    );
}

#[track_caller]
fn pump_test_app(
    cx: &mut gpui_kit::TestAppContext,
    mut ready: impl FnMut(&mut gpui_kit::TestAppContext) -> bool,
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

mod controller_git_fixture;
pub(crate) use controller_git_fixture::{ControllerRepo, fixture_git_command};
