//! C1–C3: production Composer utility-menu coordination.
//!
//! These tests mount the real `ConversationStream`, drive the actual chip
//! mouse/key paths, and inspect the painted branch rows rather than inferring
//! state from the selector model alone.

use super::menu_lists::branch_snapshot;
use super::r64_popup_deferred::{PROJECT_BINDING, bounds, click, install_utility_globals};
use super::*;
use gpui_kit::{Hsla, Modifiers, Quad, Rgba, VisualTestContext};
use vega_conversation::types::{BranchId, BranchSnapshot};

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

fn branch_selector(
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) -> Entity<crate::branch_selector::BranchSelector> {
    stream.read_with(cx, |stream, _| stream.branch_selector())
}

fn leaked(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn set_theme(cx: &mut TestAppContext, dark: bool) {
    cx.update(|cx| {
        cx.set_global(if dark {
            vega_theme::Theme::dark()
        } else {
            vega_theme::Theme::light()
        });
        cx.refresh_windows();
    });
    cx.run_until_parked();
}

fn painted_quads(window: WindowHandle<StreamHarness>, cx: &mut TestAppContext) -> (Vec<Quad>, f32) {
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| {
            (window.painted_quads(), window.scale_factor())
        })
        .expect("utility-menu window must remain open while reading painted quads")
}

fn same_colour(actual: Hsla, expected: Rgba) -> bool {
    let actual = Rgba::from(actual);
    [
        (actual.r, expected.r),
        (actual.g, expected.g),
        (actual.b, expected.b),
        (actual.a, expected.a),
    ]
    .into_iter()
    .all(|(actual, expected)| (actual - expected).abs() <= 1.0 / 255.0)
}

fn quad_matches_bounds(
    quad: &Quad,
    target: gpui_kit::Bounds<gpui_kit::Pixels>,
    scale: f32,
) -> bool {
    let left = quad.bounds.left().as_f32() / scale;
    let top = quad.bounds.top().as_f32() / scale;
    let right = quad.bounds.right().as_f32() / scale;
    let bottom = quad.bounds.bottom().as_f32() / scale;
    let tolerance = 0.2;
    (left - target.left().as_f32()).abs() <= tolerance
        && (right - target.right().as_f32()).abs() <= tolerance
        && (top - target.top().as_f32()).abs() <= tolerance
        && (bottom - target.bottom().as_f32()).abs() <= tolerance
}

fn row_has_fill(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    expected: Rgba,
    cx: &mut TestAppContext,
) -> bool {
    let target = bounds(window, selector, cx);
    let (quads, scale) = painted_quads(window, cx);
    quads.iter().any(|quad| {
        quad.background.as_solid().is_some_and(|solid| {
            same_colour(solid, expected) && quad_matches_bounds(quad, target, scale)
        })
    })
}

fn row_has_any_solid_fill(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    let target = bounds(window, selector, cx);
    let (quads, scale) = painted_quads(window, cx);
    quads.iter().any(|quad| {
        quad.background.as_solid().is_some() && quad_matches_bounds(quad, target, scale)
    })
}

fn current_and_switchable_indices(snapshot: &BranchSnapshot) -> (usize, usize, usize) {
    let current = snapshot
        .branches
        .iter()
        .position(|branch| branch.current)
        .expect("branch fixture must have a current row");
    let mut switchable = snapshot
        .branches
        .iter()
        .enumerate()
        .filter(|(_, branch)| !branch.current)
        .map(|(index, _)| index);
    (
        current,
        switchable
            .next()
            .expect("branch fixture needs a switchable row"),
        switchable
            .next()
            .expect("branch fixture needs two switchable rows"),
    )
}

fn open_branch_with_snapshot(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    snapshot: BranchSnapshot,
    cx: &mut TestAppContext,
) -> Entity<crate::branch_selector::BranchSelector> {
    let selector = branch_selector(stream, cx);
    click(window, "composer-utility-branch-chip", cx);
    assert!(selector.read_with(cx, |selector, _| selector.is_open()));
    selector.update(cx, |selector, cx| {
        assert!(selector.apply_snapshot(snapshot, cx));
    });
    cx.run_until_parked();
    selector
}

fn assert_popup_state(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    expected: Option<&'static str>,
    cx: &mut TestAppContext,
) {
    let project = mounted(window, "composer-utility-project-menu", cx);
    let branch = mounted(window, "branch-selector-popup", cx);
    assert!(
        !(project && branch),
        "at most one utility popup may be mounted"
    );
    match expected {
        Some("project") => assert!(project && !branch, "project popup must be the only popup"),
        Some("branch") => assert!(branch && !project, "branch popup must be the only popup"),
        None => assert!(!project && !branch, "both utility popups must be closed"),
        Some(other) => panic!("unknown expected popup {other}"),
    }
    let (project_open, branch_open) = stream.read_with(cx, |stream, cx| {
        (
            stream.utility_projects_open,
            stream
                .branch_selector
                .read_with(cx, |selector, _| selector.is_open()),
        )
    });
    assert_eq!(
        project, project_open,
        "project mounted state must match entity state"
    );
    assert_eq!(
        branch, branch_open,
        "branch mounted state must match entity state"
    );
}

async fn assert_initial_current_only_reopen_and_keyboard(cx: &mut TestAppContext, dark: bool) {
    let (window, stream, _events) = open_controller_stream(
        cx,
        if dark {
            "utility-coordination-dark"
        } else {
            "utility-coordination-light"
        },
    );
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    set_theme(cx, dark);

    let snapshot = branch_snapshot(&["main", "feature", "bugfix"]);
    let (current_index, first_switchable, second_switchable) =
        current_and_switchable_indices(&snapshot);
    let current_selector = leaked(format!("branch-row-{current_index}"));
    let first_selector = leaked(format!("branch-row-{first_switchable}"));
    let second_selector = leaked(format!("branch-row-{second_switchable}"));
    let selected = if dark {
        vega_theme::DARK.bg_active_alpha
    } else {
        vega_theme::LIGHT.bg_active_alpha
    };
    let keyboard = if dark {
        vega_theme::DARK.bg_hover
    } else {
        vega_theme::LIGHT.bg_hover
    };

    let selector = open_branch_with_snapshot(window, &stream, snapshot.clone(), cx);

    // C1 painted evidence: the logical first switchable candidate exists for
    // Enter, but the untouched menu paints only the current row's selection.
    assert!(
        row_has_fill(window, current_selector, selected, cx),
        "C1 {dark:?}: current branch must paint the selected surface initially"
    );
    assert!(
        !row_has_any_solid_fill(window, first_selector, cx),
        "C1 {dark:?}: automatic first switchable candidate must not paint focus initially"
    );
    assert!(
        !row_has_any_solid_fill(window, second_selector, cx),
        "C1 {dark:?}: untouched second switchable row must remain unfilled"
    );
    assert!(
        selector
            .read_with(cx, |selector, _| selector.focused_branch())
            .is_some(),
        "C1: the logical Enter candidate remains seeded for activation"
    );

    // First arrow reveals the seeded candidate; the next arrow advances to the
    // next visible switchable row. This proves intentional keyboard focus is
    // still visible without skipping the first target.
    cx.simulate_keystrokes(window.into(), "down");
    cx.run_until_parked();
    assert!(
        row_has_fill(window, first_selector, keyboard, cx),
        "C1 {dark:?}: first arrow must reveal the first switchable row"
    );
    cx.simulate_keystrokes(window.into(), "down");
    cx.run_until_parked();
    assert!(
        row_has_fill(window, second_selector, keyboard, cx),
        "C1 {dark:?}: second arrow must advance visual focus to the next row"
    );

    // Reopen reset: close the real trigger, reopen it, and deliver a fresh
    // snapshot. Visual keyboard intent must not leak across the close.
    click(window, "composer-utility-branch-chip", cx);
    assert_popup_state(window, &stream, None, cx);
    let _selector = open_branch_with_snapshot(window, &stream, snapshot.clone(), cx);
    assert!(
        row_has_fill(window, current_selector, selected, cx),
        "C1 {dark:?}: current branch must remain selected after reopen"
    );
    assert!(
        !row_has_any_solid_fill(window, first_selector, cx),
        "C1 {dark:?}: first switchable row must reset to no visual focus on reopen"
    );
    assert!(
        !row_has_any_solid_fill(window, second_selector, cx),
        "C1 {dark:?}: second switchable row must reset to no visual focus on reopen"
    );

    let activated = std::sync::Arc::new(std::sync::Mutex::new(Vec::<BranchId>::new()));
    let captured = activated.clone();
    cx.update(|cx| {
        cx.subscribe(
            &selector,
            move |_, event: &crate::branch_selector::BranchSwitchRequested, _| {
                if let Ok(mut ids) = captured.lock() {
                    ids.push(event.branch_id);
                }
            },
        )
        .detach();
    });
    // Enter keeps the logical candidate semantics; arrows intentionally move
    // to the second switchable target before Enter activates it.
    cx.simulate_keystrokes(window.into(), "down down enter");
    let ids = activated.lock().expect("branch activation capture");
    assert_eq!(ids.as_slice(), &[snapshot.branches[second_switchable].id]);
}

#[gpui_kit::test]
async fn c1_light_branch_menu_paints_current_only_then_keyboard_focus(cx: &mut TestAppContext) {
    assert_initial_current_only_reopen_and_keyboard(cx, false).await;
}

#[gpui_kit::test]
async fn c1_dark_branch_menu_paints_current_only_then_keyboard_focus(cx: &mut TestAppContext) {
    assert_initial_current_only_reopen_and_keyboard(cx, true).await;
}

#[gpui_kit::test]
async fn c2_project_branch_project_trigger_sequence_is_mutually_exclusive(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "utility-project-branch-project");
    install_utility_globals(cx, &[("/tmp/coord-alpha", "alpha")], Some(PROJECT_BINDING));

    click(window, "composer-utility-project-chip", cx);
    assert_popup_state(window, &stream, Some("project"), cx);

    // Branch opens in one click and closes its sibling at the
    // `BranchListRequested` boundary.
    click(window, "composer-utility-branch-chip", cx);
    assert_popup_state(window, &stream, Some("branch"), cx);

    // Project opening closes the branch through request_close before it opens.
    click(window, "composer-utility-project-chip", cx);
    assert_popup_state(window, &stream, Some("project"), cx);

    // Repeated trigger toggling closes the one open popup and never leaves a
    // hidden sibling mounted behind it.
    click(window, "composer-utility-project-chip", cx);
    assert_popup_state(window, &stream, None, cx);
    click(window, "composer-utility-project-chip", cx);
    assert_popup_state(window, &stream, Some("project"), cx);

    let outside = gpui_kit::point(gpui_kit::px(4.0), gpui_kit::px(4.0));
    VisualTestContext::from_window(window.into(), cx).simulate_click(outside, Modifiers::default());
    cx.run_until_parked();
    assert_popup_state(window, &stream, None, cx);
}

#[gpui_kit::test]
async fn c2_branch_project_branch_sequence_keeps_one_popup_and_escape_closes(
    cx: &mut TestAppContext,
) {
    let (window, stream, _events) = open_controller_stream(cx, "utility-branch-project-branch");
    install_utility_globals(cx, &[("/tmp/coord-beta", "beta")], Some(PROJECT_BINDING));

    click(window, "composer-utility-branch-chip", cx);
    assert_popup_state(window, &stream, Some("branch"), cx);
    click(window, "composer-utility-project-chip", cx);
    assert_popup_state(window, &stream, Some("project"), cx);
    click(window, "composer-utility-branch-chip", cx);
    assert_popup_state(window, &stream, Some("branch"), cx);

    // Escape follows the selector's production key context and its normal
    // BranchSelectorClosed path.
    cx.simulate_keystrokes(window.into(), "escape");
    assert_popup_state(window, &stream, None, cx);

    // The branch trigger can be reopened after Escape and toggled closed with
    // one more click, proving no stale capture/ghost menu remains.
    click(window, "composer-utility-branch-chip", cx);
    assert_popup_state(window, &stream, Some("branch"), cx);
    click(window, "composer-utility-branch-chip", cx);
    assert_popup_state(window, &stream, None, cx);
}
