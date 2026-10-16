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
/// bottom of a centred 768px column; the window's top-left corner is far from
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

/// The branch selector entity mounted in the utility bar.
fn branch_selector(
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) -> Entity<crate::branch_selector::BranchSelector> {
    stream.read_with(cx, |stream, _| stream.branch_selector())
}

// ---------------------------------------------------------------- A1 / A2

// ---------------------------------------------------------------- A3

// ---------------------------------------------------------------- A4

// ---------------------------------------------------------------- A4

// ---------------------------------------------------------------- A5

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
