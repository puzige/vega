//! R68 §2/§3/§5: the composer's two utility-bar dropdowns dismiss on an
//! outside click, and the project popup is `MENU_MAX_WIDTH` wide instead of
//! being clamped to its chip.
//!
//! Contract source: `docs/vega-r68-popup-dismiss-and-padding.md` — §2 (R1–R6,
//! the dismissal mechanism and its capture-phase trap), §3 (R7–R12, the
//! **withdrawn** row-height change and the padding/fill trim) and §4 (R13–R15,
//! the width). §5's A1–A6 are the production-test evidence; A7–A10 are the
//! native pixel checks and are not reachable from this harness.
//!
//! ## What is being pinned, and why each case is not redundant
//!
//! R68's two defects interact, so the tests deliberately split:
//!
//! - **A1/A2** are the user's own report: a click anywhere outside the popup
//!   closes it. They fail if the out-handler is missing (or attached to the
//!   wrong element — the trap R68 §2 records, where wrapping the *trigger*
//!   instead makes clicks inside the popup count as outside).
//! - **A3** is the complementary half: the out-handler must not fire for
//!   clicks **inside** the popup, including on pieces that handle nothing
//!   themselves (the disabled action rows, the inert current branch row).
//! - **A4** is the capture-phase trap R68 §2 measured: the triggers toggle on
//!   mouse-**up** while the out-handler runs on mouse-**down**, so without
//!   `capture_any_mouse_down` the popup reopens on the way out and its own
//!   trigger can never close it. This test is the only one that fails when the
//!   trigger is reverted to the bubble phase.
//! - **A5** is the width, the spec's headline assertion (§3/§4): 350 logical
//!   px, independent of the chip's width. Two project-name lengths are used
//!   precisely so a `max_w_full()`-style clamp cannot pass by accident.
//! - **A6** pins the padding change **and** the withdrawn row-height change in
//!   one place: R68 R7 is withdrawn, so `MENU_ROW_HEIGHT` must still be 32.
//! - **A7** is the only case that needs painted pixels: the search row must
//!   paint no `bg_hover` fill (R68 R9).
//!
//! ## What these tests cannot pin
//!
//! The popup's *appearance* — radius, shadow, stacking, and whether the
//! project name is really no longer truncated on screen (A7–A9 of the spec).
//! `debug_bounds` proves layout, not rendering, and the two are not
//! substitutes; those claims belong to the native pixel scan, not to this
//! file.

use super::menu_lists::branch_snapshot;
use super::r64_popup_deferred::{PROJECT_BINDING, bounds, click, install_utility_globals};
use super::*;
use gpui_kit::{Bounds, Hsla, Modifiers, Point, Quad, Rgba, Size, VisualTestContext};

/// Whether `selector` is currently mounted. The popups are unmounted rather
/// than hidden, so this is exactly "the popup is open".
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

/// A window point that is outside every popup under test.
///
/// The popups are anchored to the chips, which sit in the utility bar at the
/// bottom of a centred 736px column; the window's top-left corner is far from
/// all of them. `guard` asserts that the caller really is testing an *outside*
/// click, so this helper cannot silently start clicking inside a popup.
fn outside_point(
    window: WindowHandle<StreamHarness>,
    guard: &'static str,
    cx: &mut TestAppContext,
) -> Point<Pixels> {
    let point = gpui_kit::point(px(4.), px(4.));
    let popup = bounds(window, guard, cx);
    assert!(
        !popup.contains(&point),
        "R68: `{guard}` must not cover the outside-click point, or this test \
         proves nothing; popup={popup:?} point={point:?}"
    );
    point
}

fn click_at(window: WindowHandle<StreamHarness>, point: Point<Pixels>, cx: &mut TestAppContext) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(point, Modifiers::default());
    visual.run_until_parked();
}

/// Opens the project popup through the production chip click.
fn open_project_popup(window: WindowHandle<StreamHarness>, cx: &mut TestAppContext) {
    click(window, "composer-utility-project-chip", cx);
    assert!(
        mounted(window, "composer-utility-project-menu", cx),
        "the chip click must open the project popup before the test continues"
    );
}

/// The branch selector entity mounted in the utility bar.
fn branch_selector(
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) -> Entity<crate::branch_selector::BranchSelector> {
    stream.read_with(cx, |stream, _| stream.branch_selector())
}

/// Opens the branch popup through the production open path — `request_open`
/// plus the typed snapshot the app's worker would deliver.
fn open_branch_popup(
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) -> Entity<crate::branch_selector::BranchSelector> {
    let selector = branch_selector(stream, cx);
    selector.update(cx, |selector, cx| {
        assert!(selector.request_open(cx));
        assert!(selector.apply_snapshot(branch_snapshot(&["main", "feature"]), cx));
    });
    cx.run_until_parked();
    assert!(
        selector.read_with(cx, |selector, _| selector.is_open()),
        "the branch popup must be open before the test continues"
    );
    selector
}

// ---------------------------------------------------------------- A1 / A2

/// R68 A1 / R1 / R2: clicking **outside** the project popup closes it, through
/// the popup's own existing close path (`utility_projects_open = false` plus
/// `cx.notify()`), which unmounts it.
#[gpui_kit::test]
async fn r68_a1_clicking_outside_closes_the_project_popup(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r68-a1-project");
    install_utility_globals(
        cx,
        &[("/tmp/r68-a1-alpha", "alpha"), ("/tmp/r68-a1-beta", "beta")],
        Some(PROJECT_BINDING),
    );
    open_project_popup(window, cx);

    let point = outside_point(window, "composer-utility-project-menu", cx);
    click_at(window, point, cx);

    assert!(
        !mounted(window, "composer-utility-project-menu", cx),
        "R68 R1/A1: a click outside the project popup must close it"
    );
    // The rest of the composer must be untouched: closing this popup is not a
    // general "close everything" (R5).
    assert!(
        mounted(window, "composer-utility-project-chip", cx),
        "the chip must survive its own popup closing"
    );
}

/// R68 A2 / R1 / R2 / R5: clicking **outside** the branch popup closes it via
/// [`BranchSelector::request_close`], and that path emits exactly one
/// `BranchSelectorClosed` — the event whose pending cleanup belongs to the
/// controller. A handler that mutated `model.status` directly would close the
/// popup but emit nothing, so the event count is what pins R2.
#[gpui_kit::test]
async fn r68_a2_clicking_outside_closes_the_branch_popup(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "r68-a2-branch");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let selector = open_branch_popup(&stream, cx);

    let closed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured = closed.clone();
    cx.update(|cx| {
        cx.subscribe(
            &selector,
            move |_, _: &crate::branch_selector::BranchSelectorClosed, _| {
                captured.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            },
        )
        .detach();
    });
    cx.run_until_parked();

    let point = outside_point(window, "branch-selector-search", cx);
    click_at(window, point, cx);

    assert!(
        !selector.read_with(cx, |selector, _| selector.is_open()),
        "R68 R1/A2: a click outside the branch popup must close it"
    );
    assert_eq!(
        closed.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "R68 R2: the close must go through `request_close`, which emits \
         exactly one `BranchSelectorClosed`; a direct `model.status` mutation \
         would emit none"
    );
    // R5: the project popup has its own state and must not have been touched.
    assert!(
        !mounted(window, "composer-utility-project-menu", cx),
        "R5: closing the branch popup must not open the project popup"
    );
}

// ---------------------------------------------------------------- A3

/// R68 A3 / R4: clicks **inside** the project popup leave it open.
///
/// Two kinds of interior are used on purpose. The search field is a live
/// input; the `新建项目` row is a disabled action row that handles nothing and
/// carries a tooltip. A naive "any mouse-down closes" implementation passes
/// neither, and the disabled row in particular is where an implementation that
/// relies on each child stopping propagation would leak.
#[gpui_kit::test]
async fn r68_a3_clicking_inside_leaves_the_project_popup_open(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r68-a3-project");
    install_utility_globals(
        cx,
        &[("/tmp/r68-a3-alpha", "alpha"), ("/tmp/r68-a3-beta", "beta")],
        Some(PROJECT_BINDING),
    );
    open_project_popup(window, cx);

    click(window, "composer-utility-project-search", cx);
    assert!(
        mounted(window, "composer-utility-project-menu", cx),
        "R68 R4: clicking the search field must not close the popup"
    );

    click(window, "composer-utility-project-new", cx);
    assert!(
        mounted(window, "composer-utility-project-menu", cx),
        "R68 R4: clicking the trailing (disabled) action row must not close \
         the popup"
    );
}

/// R68 A3 / R4: clicks **inside** the branch popup leave it open — the search
/// field, the current branch row (which carries no handler at all), and the
/// disabled trailing action row.
#[gpui_kit::test]
async fn r68_a3_clicking_inside_leaves_the_branch_popup_open(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "r68-a3-branch");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let selector = open_branch_popup(&stream, cx);

    click(window, "branch-selector-search", cx);
    assert!(
        selector.read_with(cx, |selector, _| selector.is_open()),
        "R68 R4: clicking the branch search field must not close the popup"
    );

    click(window, "branch-row-0", cx);
    assert!(
        selector.read_with(cx, |selector, _| selector.is_open()),
        "R68 R4: clicking the current (inert) branch row must not close the \
         popup"
    );

    click(window, "branch-selector-new", cx);
    assert!(
        selector.read_with(cx, |selector, _| selector.is_open()),
        "R68 R4: clicking the trailing (disabled) action row must not close \
         the popup"
    );
}

// ---------------------------------------------------------------- A4

/// A point on `trigger` that `cover` does not also cover.
///
/// The branch popup is mounted below the trigger and the composer sits at the
/// window's bottom edge, so the `anchored` layer's `snap_to_window_with_margin`
/// pulls the popup back **up over** the chip; the chip's top edge is then the
/// only part of it a click can still reach. That exposed band is computed here
/// rather than hard-coded, so the test follows the geometry instead of
/// asserting a pixel sliver — and it fails loudly, rather than silently
/// clicking the popup, if the chip ever becomes fully covered.
fn exposed_point(trigger: Bounds<Pixels>, cover: Bounds<Pixels>, label: &str) -> Point<Pixels> {
    if f32::from(cover.top()) > f32::from(trigger.top()) {
        let y = trigger.top() + (cover.top() - trigger.top()) / 2.0;
        let point = gpui_kit::point(trigger.center().x, y);
        if trigger.contains(&point) && !cover.contains(&point) {
            return point;
        }
    }
    if f32::from(cover.left()) > f32::from(trigger.left()) {
        let x = trigger.left() + (cover.left() - trigger.left()) / 2.0;
        let point = gpui_kit::point(x, trigger.center().y);
        if trigger.contains(&point) && !cover.contains(&point) {
            return point;
        }
    }
    panic!(
        "R68 A4: `{label}` is fully covered by {cover:?} while its popup is \
         open, so no click can reach the trigger — either the geometry moved \
         or the popup really does swallow its own trigger"
    );
}

// ---------------------------------------------------------------- A4

/// R68 A4 / R3: clicking the **trigger** while its popup is open closes it.
///
/// This is the trap R68 §2 measured and the reason the triggers use
/// `capture_any_mouse_down`. The trigger toggles on mouse-**up** while the
/// popup's out-handler runs on mouse-**down**; with the old bubble-phase
/// `on_mouse_down(stop_propagation)` the sequence is "down closes, up
/// reopens", so the popup looks stuck open. Only this test fails when the
/// trigger is reverted to the bubble phase.
#[gpui_kit::test]
async fn r68_a4_clicking_the_trigger_closes_the_project_popup(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r68-a4-project");
    install_utility_globals(
        cx,
        &[("/tmp/r68-a4-alpha", "alpha"), ("/tmp/r68-a4-beta", "beta")],
        Some(PROJECT_BINDING),
    );

    open_project_popup(window, cx);
    click(window, "composer-utility-project-chip", cx);
    assert!(
        !mounted(window, "composer-utility-project-menu", cx),
        "R68 R3/A4: the chip must be able to close its own popup — the \
         capture-phase claim is what stops the mouse-down close from being \
         undone by the mouse-up toggle"
    );

    // And the chip is still a real toggle: it opens again.
    click(window, "composer-utility-project-chip", cx);
    assert!(
        mounted(window, "composer-utility-project-menu", cx),
        "R68 R3/A4: the chip must still open the popup after closing it"
    );
}

/// R68 A4 / R3: the same trap on the branch chip — its `toggle` also runs on
/// mouse-up.
///
/// Unlike the project popup, the branch popup can cover its own trigger, so
/// the click point is the chip's exposed band (see [`exposed_point`]).
#[gpui_kit::test]
async fn r68_a4_clicking_the_trigger_closes_the_branch_popup(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "r68-a4-branch");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let selector = open_branch_popup(&stream, cx);

    let chip = bounds(window, "composer-utility-branch-chip", cx);
    let popup = bounds(window, "branch-selector-popup", cx);
    let trigger = exposed_point(chip, popup, "the branch chip");

    click_at(window, trigger, cx);
    assert!(
        !selector.read_with(cx, |selector, _| selector.is_open()),
        "R68 R3/A4: the branch chip must be able to close its own popup — the \
         capture-phase claim is what stops the mouse-down close from being \
         undone by the mouse-up toggle"
    );

    // Reopen and close again, so the assertion is about the toggle and not
    // about one lucky frame.
    let chip = bounds(window, "composer-utility-branch-chip", cx);
    click_at(window, chip.center(), cx);
    assert!(
        selector.read_with(cx, |selector, _| selector.is_open()),
        "R68 R3/A4: the branch chip must still open the popup after closing it"
    );
    let chip = bounds(window, "composer-utility-branch-chip", cx);
    let popup = bounds(window, "branch-selector-popup", cx);
    click_at(window, exposed_point(chip, popup, "the branch chip"), cx);
    assert!(
        !selector.read_with(cx, |selector, _| selector.is_open()),
        "R68 R3/A4: the branch chip must close its own popup every time"
    );
}

// ---------------------------------------------------------------- A5

/// R68 A5 / R13 / R14: the project popup is [`Layout::MENU_MAX_WIDTH`] wide and
/// its width does **not** follow the chip.
///
/// This is the spec's headline assertion (§3/§4). Before R68 the popup carried
/// `.w(350).max_w_full()` inside the chip's `relative()` container, so the
/// containing block was the chip and `max_w_full` clamped the popup to the
/// chip's width — the direct cause of every truncated project name. Two
/// different project-name lengths are measured so a clamp cannot pass by
/// accident: with a short name the chip is ~40px, with a long one it is ~200px
/// (the label is capped at 180), and both must still yield 350.
#[gpui_kit::test]
async fn r68_a5_the_project_popup_width_is_fixed_and_independent_of_the_chip(
    cx: &mut TestAppContext,
) {
    let (window, stream, _events) = open_controller_stream(cx, "r68-a5-width");
    install_utility_globals(
        cx,
        &[("/tmp/r68-a5-alpha", "alpha"), ("/tmp/r68-a5-beta", "beta")],
        Some(PROJECT_BINDING),
    );

    let mut widths = Vec::new();
    let mut chip_widths = Vec::new();
    for label in [
        "a",
        "r13-alpha-project-with-a-very-long-name-that-truncates",
    ] {
        stream.update(cx, |stream, cx| {
            stream.set_project_label(label.to_string(), cx);
        });
        cx.run_until_parked();

        open_project_popup(window, cx);
        let menu = bounds(window, "composer-utility-project-menu", cx);
        let chip = bounds(window, "composer-utility-project-chip", cx);
        widths.push(f32::from(menu.size.width));
        chip_widths.push(f32::from(chip.size.width));

        // Close before the next label so the reopen is a real open.
        click(window, "composer-utility-project-chip", cx);
    }

    assert!(
        chip_widths[0] < chip_widths[1],
        "this test only means something if the two project names give the chip \
         two different widths; got {chip_widths:?}"
    );
    for (index, width) in widths.iter().enumerate() {
        assert_eq!(
            *width,
            Layout::MENU_MAX_WIDTH,
            "R68 R13/A5: the project popup must be `MENU_MAX_WIDTH` wide, not \
             clamped to the chip (project name {index}, chip width {}). \
             Widths measured: {widths:?}",
            chip_widths[index]
        );
    }
}

/// R68 R14: the popup never overflows the window's right edge. In a viewport
/// narrower than the chip's left inset plus the popup width, it shrinks to the
/// room that is left rather than running off the edge.
#[gpui_kit::test]
async fn r68_r14_the_project_popup_stays_inside_a_narrow_window(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r68-r14-narrow");
    install_utility_globals(
        cx,
        &[("/tmp/r68-r14-alpha", "alpha")],
        Some(PROJECT_BINDING),
    );

    // Narrow the window before opening so the popup measures the new viewport.
    window
        .update(cx, |_, window, _| {
            window.resize(gpui_kit::size(px(320.), px(720.)));
        })
        .expect("resize the test window");
    cx.run_until_parked();

    open_project_popup(window, cx);
    let menu = bounds(window, "composer-utility-project-menu", cx);
    let viewport = window
        .update(cx, |_, window, _| window.viewport_size())
        .expect("viewport size");
    assert!(
        f32::from(menu.size.width) <= Layout::MENU_MAX_WIDTH,
        "R68 R14: a narrow window must not widen the popup; got {}",
        f32::from(menu.size.width)
    );
    assert!(
        f32::from(menu.right()) <= f32::from(viewport.width),
        "R68 R14: the popup must not overflow the window's right edge; \
         menu={menu:?} viewport={viewport:?}"
    );
    assert!(
        f32::from(menu.size.width) > 0.0,
        "R68 R14: the popup must still have a positive width"
    );
}

/// Leaks a formatted selector so it can be passed to `debug_bounds`, which
/// takes `&'static str`. Test-only and bounded (each call is one small string),
/// so the leak is the price of not threading a selector type through the
/// harness.
fn selector(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// Asserts one row's trailing content sits `MENU_ROW_PADDING_X` from the row's
/// right edge, and returns the measured padding.
///
/// The row's selection marker is the last child, so the marker's right edge is
/// the row's content box's right edge; the distance between the two is exactly
/// the row's horizontal padding. Both popups name the marker by appending
/// `-check` to the row's own selector, so the caller passes the row selector
/// and this helper derives the rest.
fn row_padding(
    window: WindowHandle<StreamHarness>,
    row_selector: &'static str,
    cx: &mut TestAppContext,
) -> f32 {
    let marker_selector = selector(format!("{row_selector}-check"));
    let row = bounds(window, row_selector, cx);
    let marker = bounds(window, marker_selector, cx);
    assert_eq!(
        f32::from(row.size.height),
        crate::menu_list::MENU_ROW_HEIGHT,
        "R68 A6: the row's own height must be `MENU_ROW_HEIGHT`"
    );
    f32::from(row.right() - marker.right())
}

/// The current row's selector in the branch popup: the one row whose `-check`
/// marker is mounted. `visible_rows` supplies the snapshot indices the
/// selectors are built from (R62 R10's `render_branch_row`), so this reads the
/// real projection rather than assuming `main` is row 0.
fn current_branch_row(
    branch: &Entity<crate::branch_selector::BranchSelector>,
    cx: &mut TestAppContext,
) -> &'static str {
    let current = branch.read_with(cx, |branch, _| {
        branch
            .visible_rows(0..branch.visible_count())
            .into_iter()
            .find(|(_, row)| row.current)
            .map(|(index, _)| index)
    });
    let index = current.expect("the fixture snapshot always has a current branch");
    selector(format!("branch-row-{index}"))
}

// ---------------------------------------------------------------- A6

/// R68 A6 / R8 / R10 / R11 / **R7 (withdrawn)**: the shared row padding is 10,
/// the row height is still 32, and the padding really reaches both popups.
///
/// R68 v1 asked for a 36px row from the reference's *browser* CSS token; §3
/// measured the Codex desktop screenshot at 28.5 and **withdrew** that (R7),
/// so the row height assertion here is a guard against the withdrawn change
/// coming back, not a restatement of it.
///
/// The padding is asserted twice, and both halves are needed: the constant
/// pins the shared source of truth (R10 — one change must reach both popups),
/// and the geometry pins that the row really spends it.
#[gpui_kit::test]
async fn r68_a6_the_shared_row_padding_is_ten_and_the_row_height_is_still_32(
    cx: &mut TestAppContext,
) {
    assert_eq!(
        crate::menu_list::MENU_ROW_PADDING_X,
        10.0,
        "R68 R8: the shared row padding is 10 (reference measured 9.5, \
         reference token 10)"
    );
    assert_eq!(
        crate::menu_list::MENU_ROW_HEIGHT,
        32.0,
        "R68 R7 (withdrawn) / R11: the row height must stay 32 — the \
         reference is 28.5, so raising it to 36 is a reverse optimisation"
    );
    assert_eq!(
        crate::menu_list::MENU_ROW_HEIGHT,
        Typography::SIDEBAR_LINE_HEIGHT,
        "the row height is the sidebar line height by construction"
    );

    // The branch popup first: R68 does not change its width, so its rows are
    // the cleanest place to measure the shared inset.
    let (window, stream, _events) = open_controller_stream(cx, "r68-a6-padding");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let branch = open_branch_popup(&stream, cx);
    let row = current_branch_row(&branch, cx);
    let padding = row_padding(window, row, cx);
    assert!(
        (padding - crate::menu_list::MENU_ROW_PADDING_X).abs() <= 0.5,
        "R68 R8/A6: the branch row's trailing marker must sit exactly \
         `MENU_ROW_PADDING_X` from the row's right edge; measured {padding}px"
    );

    // The project popup draws the same shared rows (R10), so the same inset
    // must hold there too.
    let (window, _stream, _events) = open_controller_stream(cx, "r68-a6-project-padding");
    install_utility_globals(
        cx,
        &[("/tmp/r68-a6-alpha", "alpha"), ("/tmp/r68-a6-beta", "beta")],
        Some(PROJECT_BINDING),
    );
    open_project_popup(window, cx);
    // The bound project is the selected row, so it is the one with a marker.
    let project_row = selector(format!("composer-utility-project-row-{PROJECT_BINDING}"));
    let project_padding = row_padding(window, project_row, cx);
    assert!(
        (project_padding - crate::menu_list::MENU_ROW_PADDING_X).abs() <= 0.5,
        "R68 R10/A6: both popups must take the padding from the shared \
         `row_container`; the project row measured {project_padding}px"
    );
}

// ---------------------------------------------------------------- A7

/// The painted scene plus the window's scale factor.
///
/// `Window::painted_quads()` reads `rendered_frame.scene.quads` directly and
/// needs no `HeadlessRenderer`, so it works in this harness. The quads'
/// bounds are in **scaled** pixels (2x on this machine), unlike
/// `debug_bounds`, which is logical; the scale factor brings the two into the
/// same space.
fn painted_scene(
    visual: &mut VisualTestContext,
    window: WindowHandle<StreamHarness>,
) -> (Vec<Quad>, f32) {
    visual
        .update_window(window.into(), |_, window, _| {
            (window.painted_quads(), window.scale_factor())
        })
        .expect("the window must still be open to read its painted quads")
}

/// The `bg_hover`-coloured quads, in **logical** pixels.
///
/// `Background::Solid` is `pub(crate)` in GPUI, so the fill is read through
/// the public `Background::as_solid()` accessor. The comparison goes through
/// 8-bit RGBA with a one-step tolerance rather than `Hsla`'s own `PartialEq`
/// (the R67 pattern): the strict equality does hold here, but the tolerance
/// keeps the assertion meaningful if a future theme or GPUI version
/// re-introduces a rounding difference, while still pinning the token —
/// `bg_hover` is `0xF3F3F3` in the light theme and no neighbouring token is
/// within one 8-bit step of it.
fn hover_fills(quads: &[Quad], scale: f32) -> Vec<Bounds<f32>> {
    let expected = Hsla::from(vega_theme::Theme::light().colors.bg_hover);
    let same = |actual: Hsla| {
        let (actual, expected) = (Rgba::from(actual), Rgba::from(expected));
        [
            (actual.r, expected.r),
            (actual.g, expected.g),
            (actual.b, expected.b),
            (actual.a, expected.a),
        ]
        .into_iter()
        .all(|(actual, expected)| (actual - expected).abs() <= 1.0 / 255.0)
    };
    quads
        .iter()
        .filter_map(|quad| quad.background.as_solid().map(|solid| (quad, solid)))
        .filter(|(_, solid)| same(*solid))
        .map(|(quad, _)| Bounds {
            origin: Point {
                x: quad.bounds.left().as_f32() / scale,
                y: quad.bounds.top().as_f32() / scale,
            },
            size: Size {
                width: quad.bounds.size.width.as_f32() / scale,
                height: quad.bounds.size.height.as_f32() / scale,
            },
        })
        .collect()
}

/// Whether `outer` (logical) covers `inner` (logical), allowing for the
/// device-pixel snapping `painted_quads` applies.
fn covers(outer: &Bounds<f32>, inner: &Bounds<Pixels>) -> bool {
    const TOLERANCE: f32 = 1.0;
    outer.left() <= f32::from(inner.left()) + TOLERANCE
        && outer.right() >= f32::from(inner.right()) - TOLERANCE
        && outer.top() <= f32::from(inner.top()) + TOLERANCE
        && outer.bottom() >= f32::from(inner.bottom()) - TOLERANCE
}

/// R68 A7 / R9: the search row paints **no** `bg_hover` fill.
///
/// Vega drew a grey pill behind the magnifier; the reference has just the
/// magnifier and the placeholder sitting on the card's own surface, which is
/// why removing the fill was one of the most visible parts of §3. Both popups
/// share `menu_list::search_field` (R10), so both are checked.
#[gpui_kit::test]
async fn r68_a7_the_search_row_paints_no_hover_fill(cx: &mut TestAppContext) {
    let (window, _stream, _events) = open_controller_stream(cx, "r68-a7-fill");
    install_utility_globals(
        cx,
        &[("/tmp/r68-a7-alpha", "alpha"), ("/tmp/r68-a7-beta", "beta")],
        Some(PROJECT_BINDING),
    );
    open_project_popup(window, cx);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let search = visual
        .debug_bounds("composer-utility-project-search")
        .expect("mounted project search row");
    let (quads, scale) = painted_scene(&mut visual, window);
    assert!(
        !quads.is_empty(),
        "the harness must be painting quads, or this test proves nothing"
    );
    let fills = hover_fills(&quads, scale);
    assert!(
        fills.iter().all(|fill| !covers(fill, &search)),
        "R68 R9/A7: the project search row must paint no `bg_hover` fill; \
         found {fills:?} over the row {search:?} ({} quads painted)",
        quads.len()
    );

    // The branch popup shares the same helper, so it must be fill-free too.
    let (window, stream, _events) = open_controller_stream(cx, "r68-a7-branch-fill");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    open_branch_popup(&stream, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let search = visual
        .debug_bounds("branch-selector-search")
        .expect("mounted branch search row");
    let (quads, scale) = painted_scene(&mut visual, window);
    assert!(!quads.is_empty(), "the branch popup must be painting quads");
    let fills = hover_fills(&quads, scale);
    assert!(
        fills.iter().all(|fill| !covers(fill, &search)),
        "R68 R9/R10/A7: the branch search row must paint no `bg_hover` fill; \
         found {fills:?} over the row {search:?}"
    );
}
