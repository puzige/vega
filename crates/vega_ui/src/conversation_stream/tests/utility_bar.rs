//! R49 composer utility bar: one state, one production test.
//!
//! Every case mounts the real [`ConversationStream`] in a window and reads the
//! rendered frame's debug bounds — no test-only seam decides any branch. The
//! project rows come from the same `vega_store::projects` query the sidebar's
//! project block uses.

use super::menu_lists::branch_snapshot;
use super::*;
use gpui_kit::{Hsla, Modifiers, Quad, Rgba, VisualTestContext};

/// The `permission_thread()` fixture's durable project binding. The utility
/// bar's visibility predicate requires the shared selection to match it.
const PROJECT_BINDING: &str = "project-safe-id";

/// Owned store with the same project rows the sidebar renders, plus the
/// selection global the composer's project context resolves through.
fn install_utility_globals(
    cx: &mut TestAppContext,
    projects: &[(&str, &str)],
    selected: Option<&str>,
) -> Vec<String> {
    let store = vega_store::Store::open(":memory:").expect("owned utility store");
    store.migrate().expect("owned utility migrations");
    let ids = projects
        .iter()
        .map(|(path, name)| {
            vega_store::projects::create(store.conn(), path, name, None)
                .expect("owned utility project")
                .id
        })
        .collect();
    cx.update(|cx| {
        cx.set_global(crate::sidebar::VegaStore(Ok(store)));
        cx.set_global(crate::sidebar::SelectedProject(selected.map(str::to_owned)));
        cx.refresh_windows();
    });
    cx.run_until_parked();
    ids
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
    let bounds = bounds(window, selector, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

fn assert_close(actual: f32, expected: f32, label: &str) {
    assert!(
        (actual - expected).abs() <= 1.0,
        "{label}: expected {expected}±1px, got {actual}px"
    );
}

fn assert_literal(actual: f32, expected: f32, label: &str) {
    assert!(
        (actual - expected).abs() <= 0.1,
        "{label}: expected literal {expected}px, got {actual}px"
    );
}

#[gpui_kit::test]
async fn r49_new_task_page_renders_the_utility_bar_at_its_frozen_height(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r49-utility-bar");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let bar = bounds(window, "composer-utility-bar", cx);
    assert_close(
        f32::from(bar.size.height),
        Layout::COMPOSER_UTILITY_BAR_HEIGHT,
        "utility bar height",
    );
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-footer-branch-chip")
            .is_none(),
        "a new-task draft must not duplicate the utility-bar branch selector in the footer"
    );
}

#[gpui_kit::test]
async fn r49_session_page_with_a_message_renders_no_utility_bar(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "r49-session-page");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-bar")
            .is_some(),
        "the new-task page must start with the utility bar"
    );
    // The production commit path for a sent message: the durable start landed,
    // so the local echo enters the stream.
    stream.update(cx, |stream, cx| {
        stream.composer_submit_pending = true;
        stream.accept_composer_submission("first message", cx);
    });
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-bar")
            .is_none(),
        "a conversation with messages must not render the utility bar"
    );
}

#[gpui_kit::test]
async fn issue191_persisted_project_conversation_opens_the_footer_branch_selector(
    cx: &mut TestAppContext,
) {
    let (window, stream, _events) = open_controller_stream(cx, "issue191-branch-entry");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    stream.update(cx, |stream, cx| {
        stream.composer_submit_pending = true;
        stream.accept_composer_submission("first message", cx);
    });

    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-bar")
            .is_none(),
        "a persisted conversation must keep the R49 utility bar absent"
    );
    let add = bounds(window, "composer-add", cx);
    let footer_branch = bounds(window, "composer-footer-branch-chip", cx);
    assert!(
        add.right() <= footer_branch.left(),
        "the persisted conversation branch entry must follow the add-context control"
    );
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let requests = Arc::new(Mutex::new(
        Vec::<crate::branch_selector::BranchListRequested>::new(),
    ));
    let captured = requests.clone();
    cx.update(|cx| {
        cx.subscribe(
            &selector,
            move |_, request: &crate::branch_selector::BranchListRequested, _| {
                if let Ok(mut requests) = captured.lock() {
                    requests.push(request.clone());
                }
            },
        )
        .detach();
    });

    click(window, "composer-footer-branch-chip", cx);
    assert_eq!(
        requests
            .lock()
            .expect("branch list request capture")
            .as_slice(),
        &[crate::branch_selector::BranchListRequested {
            thread_id: "issue191-branch-entry".into(),
            project_id: PROJECT_BINDING.into(),
        }]
    );

    let snapshot = branch_snapshot(&["main", "feature"]);
    selector.update(cx, |selector, cx| {
        assert!(selector.apply_snapshot(snapshot, cx));
    });
    bounds(window, "branch-selector-popup", cx);
    bounds(window, "branch-row-0", cx);

    let switches = Arc::new(Mutex::new(Vec::<
        crate::branch_selector::BranchSwitchRequested,
    >::new()));
    let captured = switches.clone();
    cx.update(|cx| {
        cx.subscribe(
            &selector,
            move |_, request: &crate::branch_selector::BranchSwitchRequested, _| {
                if let Ok(mut requests) = captured.lock() {
                    requests.push(request.clone());
                }
            },
        )
        .detach();
    });
    click(window, "branch-row-0", cx);
    let request = switches
        .lock()
        .expect("branch switch request capture")
        .first()
        .cloned()
        .expect("branch switch request");
    assert_eq!(request.thread_id, "issue191-branch-entry");
    assert_eq!(request.project_id, PROJECT_BINDING);
    assert!(selector.read_with(cx, |selector, _| selector.is_pending()));

    selector.update(cx, |selector, cx| {
        assert!(selector.finish_switch(
            request.operation_id,
            request.snapshot_generation,
            request.branch_id,
            Some(branch_snapshot(&["feature", "main"])),
            None,
            cx,
        ));
    });
    assert!(!selector.read_with(cx, |selector, _| selector.is_pending()));
    assert!(!selector.read_with(cx, |selector, _| selector.is_open()));
}

#[gpui_kit::test]
async fn issue191_standalone_conversation_has_no_footer_branch_entry(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "issue191-standalone");
    install_utility_globals(cx, &[], None);
    stream.update(cx, |stream, cx| {
        stream.thread.project_id.clear();
        stream.composer_submit_pending = true;
        stream.accept_composer_submission("first message", cx);
    });
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-footer-branch-chip")
            .is_none(),
        "standalone conversations must not render the branch entry"
    );
}

#[gpui_kit::test]
async fn r49_utility_bar_keeps_the_frozen_inset_and_chip_ladder(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r49-utility-geometry");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let card = bounds(window, "composer-shell", cx);
    let bar = bounds(window, "composer-utility-bar", cx);
    assert_close(
        f32::from(bar.left() - card.left()),
        Layout::COMPOSER_UTILITY_BAR_INSET,
        "utility bar left inset from the card",
    );
    assert_close(
        f32::from(card.right() - bar.right()),
        Layout::COMPOSER_UTILITY_BAR_INSET,
        "utility bar right inset from the card",
    );
    assert_close(
        f32::from(bar.center().x),
        f32::from(card.center().x),
        "utility bar shares the card's axis",
    );
    assert!(
        f32::from(bar.bottom() - card.top()).abs() <= 1.0,
        "utility bar bottom must meet the card top with zero overlap"
    );

    let folder = bounds(window, "composer-utility-project-chip", cx);
    let branch = bounds(window, "composer-utility-branch-chip", cx);
    let folder_icon = bounds(window, "composer-utility-project-icon", cx);
    let branch_icon = bounds(window, "composer-utility-branch-icon", cx);
    assert_close(
        f32::from(folder.left() - bar.left()),
        Layout::COMPOSER_UTILITY_CHIP_INSET,
        "first chip inset from the bar's left edge",
    );
    assert_literal(f32::from(folder.size.height), 28.0, "folder chip height");
    assert_literal(f32::from(branch.size.height), 28.0, "branch chip height");
    assert_literal(
        f32::from(folder_icon.left() - folder.left()),
        8.0,
        "folder chip horizontal padding",
    );
    assert_literal(
        f32::from(branch_icon.left() - branch.left()),
        8.0,
        "branch chip horizontal padding",
    );
    assert_literal(f32::from(branch.left() - folder.right()), 8.0, "chip gap");
}

/// Reads the actual production scene rather than inferring paint from debug
/// bounds. GPUI stores quad bounds in device pixels, so the helper converts
/// them back to logical pixels using the window's scale factor.
fn painted_quads(window: WindowHandle<StreamHarness>, cx: &mut TestAppContext) -> (Vec<Quad>, f32) {
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| {
            (window.painted_quads(), window.scale_factor())
        })
        .expect("utility-bar window must remain open while reading painted quads")
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

fn has_overlay_fill(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    dark: bool,
    cx: &mut TestAppContext,
) -> bool {
    let target = bounds(window, selector, cx);
    let expected = if dark {
        vega_theme::DARK.bg_utility_chip_overlay
    } else {
        vega_theme::LIGHT.bg_utility_chip_overlay
    };
    let (quads, scale) = painted_quads(window, cx);
    quads.iter().any(|quad| {
        let Some(solid) = quad.background.as_solid() else {
            return false;
        };
        if !same_colour(solid, expected) {
            return false;
        }
        quad_matches_chip_bounds(quad, target, scale)
            && quad_has_radius(quad, scale, Layout::COMPOSER_UTILITY_CHIP_RADIUS)
    })
}

/// A fill is only the chip's fill when the painted quad is the chip's own
/// bounds.  Ancestor surfaces are intentionally not accepted: a test that
/// only checks containment would let a broad utility-bar background conceal a
/// missing chip overlay.
fn quad_matches_chip_bounds(
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

fn quad_has_radius(quad: &Quad, scale: f32, expected: f32) -> bool {
    let expected = expected * scale;
    let tolerance = 0.75;
    [
        quad.corner_radii.top_left.as_f32(),
        quad.corner_radii.top_right.as_f32(),
        quad.corner_radii.bottom_right.as_f32(),
        quad.corner_radii.bottom_left.as_f32(),
    ]
    .into_iter()
    .all(|actual| (actual - expected).abs() <= tolerance)
}

/// Finds any opaque/solid quad at exactly the chip's bounds.  The rest state
/// must have none, regardless of its color, so replacing the shared overlay
/// with an unrelated theme color cannot evade the state test.
fn has_any_chip_fill(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    let target = bounds(window, selector, cx);
    let (quads, scale) = painted_quads(window, cx);
    quads.iter().any(|quad| {
        quad.background.as_solid().is_some() && quad_matches_chip_bounds(quad, target, scale)
    })
}

fn move_pointer(
    window: WindowHandle<StreamHarness>,
    point: gpui_kit::Point<gpui_kit::Pixels>,
    cx: &mut TestAppContext,
) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_mouse_move(point, None, Modifiers::default());
    // The test platform compares the incoming position with the previous
    // event; dispatching the same move twice also lets any hover invalidation
    // scheduled by the first event settle before the scene is read.
    visual.simulate_mouse_move(point, None, Modifiers::default());
    visual.run_until_parked();
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

async fn assert_chip_paint_states(cx: &mut TestAppContext, dark: bool) {
    let (window, stream, _events) = open_controller_stream(
        cx,
        if dark {
            "r2-utility-chip-dark"
        } else {
            "r2-utility-chip-light"
        },
    );
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    set_theme(cx, dark);

    let project = bounds(window, "composer-utility-project-chip", cx);
    let branch = bounds(window, "composer-utility-branch-chip", cx);
    let away = gpui_kit::point(gpui_kit::px(600.0), gpui_kit::px(600.0));
    // R2: rest is transparent for both production chips.
    move_pointer(
        window,
        gpui_kit::point(gpui_kit::px(700.0), gpui_kit::px(700.0)),
        cx,
    );
    move_pointer(window, away, cx);
    assert!(
        !has_any_chip_fill(window, "composer-utility-project-chip", cx),
        "project chip rest must not paint a solid quad at its own bounds"
    );
    assert!(
        !has_any_chip_fill(window, "composer-utility-branch-chip", cx),
        "branch chip rest must not paint a solid quad at its own bounds"
    );

    // R2: a real pointer transition paints the shared overlay on each chip.
    move_pointer(window, project.center(), cx);
    assert!(
        has_overlay_fill(window, "composer-utility-project-chip", dark, cx),
        "project chip pointer hover must paint the shared overlay"
    );
    move_pointer(window, away, cx);
    assert!(
        !has_any_chip_fill(window, "composer-utility-project-chip", cx),
        "project chip must clear its hover fill after pointer exit"
    );
    move_pointer(window, branch.center(), cx);
    assert!(
        has_overlay_fill(window, "composer-utility-branch-chip", dark, cx),
        "branch chip pointer hover must paint the shared overlay"
    );

    // R2: opening the project menu keeps the fill after the pointer leaves.
    click(window, "composer-utility-project-chip", cx);
    move_pointer(window, away, cx);
    assert!(
        has_overlay_fill(window, "composer-utility-project-chip", dark, cx),
        "open project chip must keep its overlay after pointer exit"
    );
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(away, Modifiers::default());
    visual.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-project-menu")
            .is_none()
    );
    assert!(
        !has_any_chip_fill(window, "composer-utility-project-chip", cx),
        "closed project chip away from pointer must be transparent"
    );

    // The branch selector opens through its real entity path. No snapshot is
    // needed for the trigger-state contract: Loading is already an open menu.
    click(window, "composer-utility-branch-chip", cx);
    assert!(stream.read_with(cx, |stream, _| {
        stream
            .branch_selector()
            .read_with(cx, |selector, _| selector.is_open())
    }));
    move_pointer(window, away, cx);
    assert!(
        has_overlay_fill(window, "composer-utility-branch-chip", dark, cx),
        "open branch chip must keep its overlay after pointer exit"
    );
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    selector.update(cx, |selector, cx| {
        assert!(selector.request_close(cx));
    });
    cx.run_until_parked();
    move_pointer(window, away, cx);
    assert!(
        !has_any_chip_fill(window, "composer-utility-branch-chip", cx),
        "closed branch chip away from pointer must be transparent"
    );
}

/// Runs only the project trigger's state machine.  Keeping this independent
/// from the branch assertions makes a mutation in either production path fail
/// its own named test instead of being masked by the first assertion in the
/// other path.
async fn assert_project_chip_paint_states(cx: &mut TestAppContext, dark: bool) {
    let (window, _stream, _events) = open_controller_stream(
        cx,
        if dark {
            "r2-project-chip-dark"
        } else {
            "r2-project-chip-light"
        },
    );
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    set_theme(cx, dark);
    let chip = bounds(window, "composer-utility-project-chip", cx);
    let away = gpui_kit::point(gpui_kit::px(600.0), gpui_kit::px(600.0));

    move_pointer(window, away, cx);
    assert!(
        !has_any_chip_fill(window, "composer-utility-project-chip", cx),
        "project-only rest state must be transparent"
    );
    move_pointer(window, chip.center(), cx);
    assert!(
        has_overlay_fill(window, "composer-utility-project-chip", dark, cx),
        "project-only hover state must paint the overlay"
    );
    move_pointer(window, away, cx);
    assert!(
        !has_any_chip_fill(window, "composer-utility-project-chip", cx),
        "project-only closed-away state must clear hover"
    );

    move_pointer(window, chip.center(), cx);
    click(window, "composer-utility-project-chip", cx);
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-project-menu")
            .is_some(),
        "project-only click must open the real project menu"
    );
    move_pointer(window, away, cx);
    assert!(
        has_overlay_fill(window, "composer-utility-project-chip", dark, cx),
        "project-only open-away state must keep the overlay"
    );
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(away, Modifiers::default());
    visual.run_until_parked();
    assert!(
        !has_any_chip_fill(window, "composer-utility-project-chip", cx),
        "project-only outside dismissal must clear the overlay"
    );
}

/// Branch counterpart to [`assert_project_chip_paint_states`].
async fn assert_branch_chip_paint_states(cx: &mut TestAppContext, dark: bool) {
    let (window, stream, _events) = open_controller_stream(
        cx,
        if dark {
            "r2-branch-chip-dark"
        } else {
            "r2-branch-chip-light"
        },
    );
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    set_theme(cx, dark);
    let chip = bounds(window, "composer-utility-branch-chip", cx);
    let away = gpui_kit::point(gpui_kit::px(600.0), gpui_kit::px(600.0));

    move_pointer(window, away, cx);
    assert!(
        !has_any_chip_fill(window, "composer-utility-branch-chip", cx),
        "branch-only rest state must be transparent"
    );
    move_pointer(window, chip.center(), cx);
    assert!(
        has_overlay_fill(window, "composer-utility-branch-chip", dark, cx),
        "branch-only hover state must paint the overlay"
    );
    move_pointer(window, away, cx);
    assert!(
        !has_any_chip_fill(window, "composer-utility-branch-chip", cx),
        "branch-only closed-away state must clear hover"
    );

    move_pointer(window, chip.center(), cx);
    click(window, "composer-utility-branch-chip", cx);
    assert!(stream.read_with(cx, |stream, _| {
        stream
            .branch_selector()
            .read_with(cx, |selector, _| selector.is_open())
    }));
    move_pointer(window, away, cx);
    assert!(
        has_overlay_fill(window, "composer-utility-branch-chip", dark, cx),
        "branch-only open-away state must keep the overlay"
    );
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    selector.update(cx, |selector, cx| {
        assert!(selector.request_close(cx));
    });
    cx.run_until_parked();
    move_pointer(window, away, cx);
    assert!(
        !has_any_chip_fill(window, "composer-utility-branch-chip", cx),
        "branch-only close-away state must clear the overlay"
    );
}

#[gpui_kit::test]
async fn r2_light_utility_chips_paint_rest_hover_open_and_closed_states(cx: &mut TestAppContext) {
    assert_chip_paint_states(cx, false).await;
}

#[gpui_kit::test]
async fn r2_dark_utility_chips_paint_rest_hover_open_and_closed_states(cx: &mut TestAppContext) {
    assert_chip_paint_states(cx, true).await;
}

#[gpui_kit::test]
async fn r2_light_project_chip_paint_states_are_independently_verified(cx: &mut TestAppContext) {
    assert_project_chip_paint_states(cx, false).await;
}

#[gpui_kit::test]
async fn r2_dark_project_chip_paint_states_are_independently_verified(cx: &mut TestAppContext) {
    assert_project_chip_paint_states(cx, true).await;
}

#[gpui_kit::test]
async fn r2_light_branch_chip_paint_states_are_independently_verified(cx: &mut TestAppContext) {
    assert_branch_chip_paint_states(cx, false).await;
}

#[gpui_kit::test]
async fn r2_dark_branch_chip_paint_states_are_independently_verified(cx: &mut TestAppContext) {
    assert_branch_chip_paint_states(cx, true).await;
}

#[gpui_kit::test]
async fn r19_legacy_branch_trigger_keeps_32px_when_chip_chrome_is_disabled(
    cx: &mut TestAppContext,
) {
    let (window, stream, _events) = open_controller_stream(cx, "r19-legacy-branch-trigger");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    selector.update(cx, |selector, cx| {
        selector.set_chip_chrome(false);
        cx.notify();
    });
    cx.run_until_parked();
    let trigger = bounds(window, "composer-utility-branch-chip", cx);
    assert_literal(
        f32::from(trigger.size.height),
        32.0,
        "R19 legacy branch trigger height",
    );
}

#[gpui_kit::test]
async fn r4_branch_uniform_rows_fill_the_popup_content_and_keep_the_marker_trailing(
    cx: &mut TestAppContext,
) {
    let (window, stream, _events) = open_controller_stream(cx, "r4-branch-row-width");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let snapshot = branch_snapshot(&["main", "feature"]);
    let current_row = snapshot
        .branches
        .iter()
        .position(|branch| branch.current)
        .expect("owned branch fixture has a current branch");
    selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(snapshot, cx));
    });
    cx.run_until_parked();

    let popup = bounds(window, "branch-selector-popup", cx);
    let row_selector: &'static str =
        Box::leak(format!("branch-row-{current_row}").into_boxed_str());
    let marker_selector: &'static str =
        Box::leak(format!("branch-row-{current_row}-check").into_boxed_str());
    let row = bounds(window, row_selector, cx);
    let marker = bounds(window, marker_selector, cx);
    assert_close(
        f32::from(row.left() - popup.left()),
        4.0,
        "branch row left edge gutter",
    );
    assert_close(
        f32::from(popup.right() - row.right()),
        4.0,
        "branch row right edge gutter",
    );
    assert!(
        marker.right() <= row.right(),
        "the selection marker must remain inside the full-width row: row={row:?} marker={marker:?}"
    );
    assert_close(
        f32::from(row.right() - marker.right()),
        10.0,
        "trailing marker column follows the row's existing horizontal padding",
    );
}

#[gpui_kit::test]
async fn r49_folder_chip_selects_another_project_from_the_shared_rows(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r49-project-chip");
    let ids = install_utility_globals(
        cx,
        &[("/tmp/r49-alpha", "alpha"), ("/tmp/r49-beta", "beta")],
        Some(PROJECT_BINDING),
    );
    click(window, "composer-utility-project-chip", cx);
    let menu = bounds(window, "composer-utility-project-menu", cx);
    assert!(
        f32::from(menu.size.height) > 0.0,
        "the project menu must render real rows"
    );
    let row_selector: &'static str =
        Box::leak(format!("composer-utility-project-row-{}", ids[1]).into_boxed_str());
    click(window, row_selector, cx);
    let selected = cx.update(|cx| cx.global::<crate::sidebar::SelectedProject>().0.clone());
    assert_eq!(
        selected.as_deref(),
        Some(ids[1].as_str()),
        "choosing a row must rewrite the shared SelectedProject global"
    );
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-project-menu")
            .is_none(),
        "the menu must close after a choice"
    );
}

#[gpui_kit::test]
async fn r49_committed_empty_session_keeps_its_project_fence_but_a8_draft_can_choose(
    cx: &mut TestAppContext,
) {
    let (window, stream, _events) = open_controller_stream(cx, "r49-no-project");
    install_utility_globals(cx, &[], None);
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-bar")
            .is_none(),
        "a committed task whose selection mismatches stays behind the R49 fence"
    );
    stream.update(cx, |stream, cx| {
        stream.set_draft_route(true, cx);
        let mut draft = stream.thread.clone();
        draft.project_id.clear();
        assert!(stream.rebind_draft_project(draft, String::new(), cx));
    });
    bounds(window, "composer-utility-bar", cx);
    bounds(window, "composer-utility-project-chip", cx);
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-utility-branch-chip")
            .is_none(),
        "an unbound draft has only the project chip"
    );
}
