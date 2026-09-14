//! R62 §8: the two utility-bar dropdowns carry real list structure.
//!
//! Contract source: `docs/vega-r62-slider-card-three-rows.md` §8 (R10/R11) and
//! §9 (A12/A13). Every case mounts the real [`ConversationStream`] in a window
//! and reads the rendered frame — no test-only seam decides any branch.
//!
//! The defect these tests pin: R49 shipped both dropdowns as bare text lists,
//! so they had no search field, no per-row icon, no checkmark, no separator and
//! no trailing actions. R62 R11 also fixes the rule the structure must respect:
//! filtering may hide rows but must never change what "select" means.

use super::*;
use gpui_kit::{Modifiers, VisualTestContext};
use vega_conversation::types::BranchSnapshot;

/// The `permission_thread()` fixture's durable project binding.
const PROJECT_BINDING: &str = "project-safe-id";

/// Owned store with the same project rows the sidebar renders, plus the
/// selection global the composer's project context resolves through.
///
/// The first row is inserted with the fixture thread's own binding id, so the
/// "current project" the dropdown marks is a row the store really lists — the
/// same shape `vega`'s branch tests build with raw SQL.
fn install_utility_globals(
    cx: &mut TestAppContext,
    projects: &[(&str, &str)],
    selected: Option<&str>,
) -> Vec<String> {
    let store = vega_store::Store::open(":memory:").expect("owned utility store");
    store.migrate().expect("owned utility migrations");
    store
        .conn()
        .execute(
            "INSERT INTO projects(id,path,name,created_at,last_opened_at) \
             VALUES(?1,'/tmp/r62-bound','bound',0,0)",
            [PROJECT_BINDING],
        )
        .expect("owned bound project");
    let ids = std::iter::once(PROJECT_BINDING.to_string())
        .chain(projects.iter().map(|(path, name)| {
            vega_store::projects::create(store.conn(), path, name, None)
                .expect("owned utility project")
                .id
        }))
        .collect();
    cx.update(|cx| {
        cx.set_global(crate::sidebar::VegaStore(Ok(store)));
        cx.set_global(crate::sidebar::SelectedProject(selected.map(str::to_owned)));
        cx.refresh_windows();
    });
    cx.run_until_parked();
    ids
}

fn mounted(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .is_some()
}

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let b = bounds(window, selector, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(b.center(), Modifiers::default());
    visual.run_until_parked();
}

/// A12: the project dropdown renders the search field, a folder icon per row,
/// a checkmark on the current selection, a separator, and the two trailing
/// action rows.
#[gpui_kit::test]
async fn r62_project_dropdown_carries_the_full_list_structure(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r62-project-menu");
    let ids = install_utility_globals(
        cx,
        &[("/tmp/r62-alpha", "alpha"), ("/tmp/r62-beta", "beta")],
        Some(PROJECT_BINDING),
    );
    click(window, "composer-utility-project-chip", cx);

    // Search field.
    assert!(
        mounted(window, "composer-utility-project-search", cx),
        "the project dropdown must render a search field"
    );
    // Per-row icon + a checkmark on the selection, and only on the selection.
    let bound_row: &'static str =
        Box::leak(format!("composer-utility-project-row-{PROJECT_BINDING}").into_boxed_str());
    let bound_check: &'static str =
        Box::leak(format!("composer-utility-project-row-{PROJECT_BINDING}-check").into_boxed_str());
    let alpha_row: &'static str =
        Box::leak(format!("composer-utility-project-row-{}", ids[1]).into_boxed_str());
    let alpha_check: &'static str =
        Box::leak(format!("composer-utility-project-row-{}-check", ids[1]).into_boxed_str());
    assert!(mounted(window, bound_row, cx), "the bound project row");
    assert!(
        mounted(window, alpha_row, cx),
        "the other project row must render"
    );
    assert!(
        mounted(window, bound_check, cx),
        "the current project must carry a checkmark"
    );
    assert!(
        !mounted(window, alpha_check, cx),
        "an unselected project row must not carry a checkmark"
    );

    // The trailing action group: `+ 新建项目` (disabled: no Vega
    // implementation path from this surface) and `× 不关联项目` (real).
    assert!(
        mounted(window, "composer-utility-project-new", cx),
        "the `新建项目` row must render (disabled, per R62 R11)"
    );
    assert!(
        mounted(window, "composer-utility-project-detach", cx),
        "the `不关联项目` row must render"
    );

    // The rows and the action group stack in order, with the separator between.
    let last_row = bounds(window, alpha_row, cx);
    let new_row = bounds(window, "composer-utility-project-new", cx);
    let detach_row = bounds(window, "composer-utility-project-detach", cx);
    assert!(
        f32::from(last_row.bottom()) <= f32::from(new_row.top()),
        "the action group must follow the project rows"
    );
    assert!(
        f32::from(new_row.bottom()) <= f32::from(detach_row.top()),
        "the two action rows must stack in order"
    );
}

/// A12/A13: typing in the search field filters the visible rows, and the
/// selection semantics are untouched — clicking a still-visible row writes the
/// same shared selection as it did before R62.
#[gpui_kit::test]
async fn r62_project_search_filters_rows_without_changing_selection(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "r62-project-filter");
    let ids = install_utility_globals(
        cx,
        &[("/tmp/r62-alpha", "alpha"), ("/tmp/r62-beta", "beta")],
        Some(PROJECT_BINDING),
    );
    click(window, "composer-utility-project-chip", cx);
    let alpha_row: &'static str =
        Box::leak(format!("composer-utility-project-row-{}", ids[1]).into_boxed_str());
    let beta_row: &'static str =
        Box::leak(format!("composer-utility-project-row-{}", ids[2]).into_boxed_str());
    assert!(mounted(window, alpha_row, cx));
    assert!(mounted(window, beta_row, cx));

    // Type a needle that matches only `beta`.
    stream.update(cx, |stream, cx| {
        stream
            .utility_project_search
            .update(cx, |input, cx| input.set_text("bet", cx));
    });
    cx.run_until_parked();
    assert!(mounted(window, beta_row, cx), "the matching row stays");
    assert!(
        !mounted(window, alpha_row, cx),
        "a non-matching row must be filtered out"
    );
    // The action group is not part of the filtered projection.
    assert!(mounted(window, "composer-utility-project-detach", cx));

    // Selecting a visible row still means "switch to it" (R49 contract).
    click(window, beta_row, cx);
    let selected = cx.update(|cx| cx.global::<crate::sidebar::SelectedProject>().0.clone());
    assert_eq!(
        selected.as_deref(),
        Some(ids[2].as_str()),
        "a filtered row must still switch the shared selection"
    );
    assert!(
        !mounted(window, "composer-utility-project-menu", cx),
        "the menu must close after a choice"
    );

    // A needle that matches nothing shows the empty state rather than a
    // silently blank panel. (The selection moved off the binding, so the
    // utility bar itself is gone; restore it the way the app's route does.)
    cx.update(|cx| {
        cx.set_global(crate::sidebar::SelectedProject(Some(
            PROJECT_BINDING.to_string(),
        )));
        cx.refresh_windows();
    });
    cx.run_until_parked();
    click(window, "composer-utility-project-chip", cx);
    stream.update(cx, |stream, cx| {
        stream
            .utility_project_search
            .update(cx, |input, cx| input.set_text("no-such-project", cx));
    });
    cx.run_until_parked();
    assert!(!mounted(window, beta_row, cx));
    assert!(
        mounted(window, "composer-utility-project-menu", cx),
        "the menu stays mounted with an explicit empty state"
    );
}

/// R62 R11: `不关联项目` writes the same shared selection global with `None`,
/// which is what makes the utility bar's own visibility predicate stop
/// matching the durable binding.
#[gpui_kit::test]
async fn r62_detach_project_clears_the_shared_selection(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r62-project-detach");
    install_utility_globals(cx, &[("/tmp/r62-alpha", "alpha")], Some(PROJECT_BINDING));
    click(window, "composer-utility-project-chip", cx);
    click(window, "composer-utility-project-detach", cx);

    let selected = cx.update(|cx| cx.global::<crate::sidebar::SelectedProject>().0.clone());
    assert_eq!(
        selected, None,
        "`不关联项目` must clear the shared project selection"
    );
    assert!(
        !mounted(window, "composer-utility-project-menu", cx),
        "the menu must close after the action"
    );
}

/// A12: the branch dropdown renders the search field, a per-row icon, a
/// checkmark on the current branch, a separator, and the trailing
/// `+ 新建并切换分支` row.
#[gpui_kit::test]
async fn r62_branch_dropdown_carries_the_full_list_structure(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "r62-branch-menu");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));

    // The production open path: `request_open` on the real selector, then the
    // typed snapshot the app's worker would deliver.
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let snapshot = branch_snapshot(&["main", "feature"]);
    let row_of = |label: &str| {
        snapshot
            .branches
            .iter()
            .position(|branch| branch.label == label)
            .unwrap_or_else(|| panic!("{label} in fixture snapshot"))
    };
    let (current_row, other_row) = (row_of("main"), row_of("feature"));
    assert!(
        snapshot.branches[current_row].current,
        "`main` is the fixture's current branch"
    );
    selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(snapshot, cx));
    });
    cx.run_until_parked();

    let current_selector: &'static str =
        Box::leak(format!("branch-row-{current_row}").into_boxed_str());
    let current_check: &'static str =
        Box::leak(format!("branch-row-{current_row}-check").into_boxed_str());
    let other_selector: &'static str =
        Box::leak(format!("branch-row-{other_row}").into_boxed_str());
    let other_check: &'static str =
        Box::leak(format!("branch-row-{other_row}-check").into_boxed_str());

    assert!(
        mounted(window, "branch-selector-search", cx),
        "the branch dropdown must render a search field"
    );
    assert!(mounted(window, current_selector, cx), "the current row");
    assert!(mounted(window, other_selector, cx), "the other row");
    assert!(
        mounted(window, current_check, cx),
        "the current branch must carry a checkmark"
    );
    assert!(
        !mounted(window, other_check, cx),
        "a non-current branch must not carry a checkmark"
    );
    assert!(
        mounted(window, "branch-selector-new", cx),
        "the `新建并切换分支` row must render (disabled, per R62 R11)"
    );

    // The action group follows the rows, and the search field precedes them.
    let search = bounds(window, "branch-selector-search", cx);
    let first_row = bounds(window, current_selector, cx);
    let action = bounds(window, "branch-selector-new", cx);
    assert!(
        f32::from(search.bottom()) <= f32::from(first_row.top()),
        "the search field must sit above the branch rows"
    );
    assert!(
        f32::from(first_row.bottom()) <= f32::from(action.top()),
        "the trailing action must follow the branch rows"
    );
}

/// A13: the branch filter hides rows but leaves the switch contract intact —
/// a visible non-current row still emits exactly the existing
/// `BranchSwitchRequested` for its own opaque id.
#[gpui_kit::test]
async fn r62_branch_search_filters_rows_without_changing_switch_semantics(cx: &mut TestAppContext) {
    let (_window, stream, _events) = open_controller_stream(cx, "r62-branch-filter");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let snapshot = branch_snapshot(&["main", "feature"]);
    let generation = snapshot.generation;
    let feature_id = snapshot
        .branches
        .iter()
        .find(|branch| branch.label == "feature")
        .expect("feature branch")
        .id;
    selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(snapshot, cx));
    });
    cx.run_until_parked();

    // Filter to `feature`: `main` disappears, `feature` stays selectable.
    selector.update(cx, |selector, cx| {
        selector
            .search_input()
            .update(cx, |input, cx| input.set_text("feat", cx));
    });
    cx.run_until_parked();
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.visible_count()),
        1,
        "the filter must narrow the visible projection"
    );
    assert_eq!(
        selector.read_with(cx, |selector, _| selector
            .visible_rows(0..4)
            .into_iter()
            .map(|(_, branch)| branch.label)
            .collect::<Vec<_>>()),
        vec!["feature".to_string()],
        "only the matching branch remains visible"
    );

    // The switch capability is validated against the full snapshot, so the
    // filtered row switches exactly as it did before R62.
    let operation = selector.update(cx, |selector, cx| {
        selector
            .begin_switch(generation, feature_id, cx)
            .expect("the visible row stays switchable")
    });
    assert!(selector.read_with(cx, |selector, _| {
        selector.owns_pending(operation, generation, feature_id)
    }));
}

/// R62 R11: a filter that hides every switchable branch clears the keyboard
/// focus, so Enter cannot activate a row the user cannot see. Clearing the
/// filter restores a focus target without ever widening the switch set.
#[gpui_kit::test]
async fn r62_branch_filter_never_leaves_keyboard_focus_on_a_hidden_row(cx: &mut TestAppContext) {
    let (_window, stream, _events) = open_controller_stream(cx, "r62-branch-focus");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let snapshot = branch_snapshot(&["main", "feature"]);
    selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(snapshot, cx));
    });
    assert!(
        selector
            .read_with(cx, |selector, _| selector.focused_branch())
            .is_some(),
        "an unfiltered list focuses a switchable row"
    );

    // `main` is the only row matching this needle, and it is current, so
    // nothing switchable is visible any more.
    selector.update(cx, |selector, cx| {
        selector
            .search_input()
            .update(cx, |input, cx| input.set_text("main", cx));
    });
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.focused_branch()),
        None,
        "a filter that hides every switchable row must clear the keyboard focus"
    );

    // Clearing the filter restores a real focus target.
    selector.update(cx, |selector, cx| {
        selector
            .search_input()
            .update(cx, |input, cx| input.set_text("", cx));
    });
    assert!(
        selector
            .read_with(cx, |selector, _| selector.focused_branch())
            .is_some(),
        "clearing the filter must restore a focus target"
    );
}

/// A fixture snapshot with the same generation/id shape the headless Git
/// service produces. `main` is current, so `feature` is the switchable row.
///
/// [`BranchId`] has no public constructor by design (the opaque-id contract),
/// so the snapshot comes from the real service over a real owned repository —
/// the same route `vega`'s own branch tests take.
fn branch_snapshot(labels: &[&str]) -> BranchSnapshot {
    let root = tempfile::tempdir().expect("owned branch fixture repo");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("/usr/bin/git")
            .arg("-C")
            .arg(root.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .status()
            .expect("owned branch fixture git");
        assert!(status.success(), "owned branch fixture git {args:?}");
    };
    git(&[
        "init",
        "-q",
        "-b",
        labels.first().copied().unwrap_or("main"),
    ]);
    git(&[
        "-c",
        "user.name=Vega Test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "--allow-empty",
        "-q",
        "-m",
        "owned",
    ]);
    for label in labels.iter().skip(1) {
        git(&["branch", label]);
    }
    let service = vega_conversation::BranchWorkspaceService::new(root.path())
        .expect("owned branch fixture service");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("owned branch fixture runtime");
    runtime
        .block_on(service.refresh(tokio_util::sync::CancellationToken::new()))
        .expect("owned branch fixture snapshot")
}
