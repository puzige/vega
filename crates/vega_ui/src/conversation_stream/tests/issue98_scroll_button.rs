//! Issue #98: the detached-tail recovery control is the reference
//! implementation's floating circular "back to bottom" button.
//!
//! Contract source: `docs/vega-issue-98-scroll-to-bottom.md` §3 (R1–R5) and
//! §5 (A1–A8). The control used to be a top-right text pill (`回到底部`); the
//! frozen spec replaces it with a 32px `ArrowDown` circle that floats
//! `SCROLL_TO_BOTTOM_GAP` above the transcript viewport, centred on the
//! content column, mounted only while the stream is detached from the tail.
//!
//! ## What each case pins, and why none is redundant
//!
//! - **A1** pins the shape: a 32×32 painted quad whose four corner radii are
//!   half the size (a true circle, `rounded_full()`).
//! - **A2** pins the surface tokens (fill = `bg_elevated`, border =
//!   `border_subtle`) in **both** palettes, so a hard-coded colour cannot pass
//!   in one theme only.
//! - **A3** pins the horizontal anchor: the button shares the content column's
//!   centre. This is the "floating, not top-right" claim that the issue is
//!   actually about.
//! - **A4** pins the visibility predicate: mounted only when `!following_tail()`.
//! - **A5** pins the production behaviour: a real click re-engages tail follow
//!   and the button unmounts.
//! - **A6** pins the keyboard path through the same `resume_tail` handler.
//! - **A7** pins the accessible name (the label moved off-screen but must not
//!   be lost).
//! - **A8** is the theme token freeze, asserted in `vega_theme`'s own suite.
//!
//! ## What these tests cannot pin
//!
//! The exact native pixel geometry above the composer (A10): `debug_bounds`
//! proves layout, not rendering, and the composer's dynamic height is not
//! observable here. That claim belongs to the native screenshot scan.

use super::r64_popup_deferred::{PROJECT_BINDING, install_utility_globals};
use super::*;
use gpui_kit::{Bounds, Hsla, Modifiers, Pixels, Quad, Rgba, VisualTestContext};

const BUTTON: &str = "scroll-to-bottom";
const COLUMN: &str = "conversation-column";

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
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

fn painted_quads(window: WindowHandle<StreamHarness>, cx: &mut TestAppContext) -> (Vec<Quad>, f32) {
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| {
            (window.painted_quads(), window.scale_factor())
        })
        .expect("issue98 window must remain open while reading painted quads")
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

fn quad_matches(quad: &Quad, target: Bounds<Pixels>, scale: f32) -> bool {
    let tolerance = 0.5;
    (quad.bounds.left().as_f32() / scale - target.left().as_f32()).abs() <= tolerance
        && (quad.bounds.right().as_f32() / scale - target.right().as_f32()).abs() <= tolerance
        && (quad.bounds.top().as_f32() / scale - target.top().as_f32()).abs() <= tolerance
        && (quad.bounds.bottom().as_f32() / scale - target.bottom().as_f32()).abs() <= tolerance
}

/// Seeds enough entries that the transcript can scroll, then detaches the
/// native tail follow through a real upward scroll (the production detach path,
/// not a test seam).
fn seed_and_detach(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) {
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    stream.update(cx, |stream, cx| {
        let entries: Vec<StreamEntry> = (0..40)
            .map(|index| StreamEntry::User {
                lines: user_message_lines(index as u64, &format!("滚动消息 {index}")),
            })
            .collect();
        let count = entries.len();
        stream.entries = entries;
        stream.list_prepend(count);
        cx.notify();
    });
    cx.run_until_parked();
    // A real upward scroll of more than a viewport detaches the tail anchor.
    stream.update(cx, |stream, cx| {
        stream.list.scroll_by(px(-600.));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        !stream.read_with(cx, |stream, _| stream.following_tail()),
        "the upward scroll must detach the tail before the button can be tested"
    );
    assert!(
        mounted(window, BUTTON, cx),
        "the detached-tail control must mount once detached"
    );
}

/// A4: while the stream follows the tail the control is not mounted; once
/// detached it is. This is the predicate the whole card hangs on.
#[gpui_kit::test]
async fn issue98_a4_button_mounts_only_when_detached(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "issue98-a4");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    stream.update(cx, |stream, cx| {
        let entries: Vec<StreamEntry> = (0..40)
            .map(|index| StreamEntry::User {
                lines: user_message_lines(index as u64, &format!("滚动消息 {index}")),
            })
            .collect();
        let count = entries.len();
        stream.entries = entries;
        stream.list_prepend(count);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        stream.read_with(cx, |stream, _| stream.following_tail()),
        "a fresh seeded stream follows the tail"
    );
    assert!(
        !mounted(window, BUTTON, cx),
        "a tail-following stream must not mount the back-to-bottom control"
    );

    stream.update(cx, |stream, cx| {
        stream.list.scroll_by(px(-600.));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(!stream.read_with(cx, |stream, _| stream.following_tail()));
    assert!(
        mounted(window, BUTTON, cx),
        "detaching the tail must mount the back-to-bottom control"
    );
}

/// A1 + A2: the control paints a 32×32 circle (`rounded_full()` → radius 16)
/// filled with `bg_elevated` and bordered `border_subtle`, in both palettes.
async fn assert_button_surface(cx: &mut TestAppContext, dark: bool) {
    let (window, stream, _events) = open_controller_stream(
        cx,
        if dark {
            "issue98-a12-dark"
        } else {
            "issue98-a12-light"
        },
    );
    if dark {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::dark());
            cx.refresh_windows();
        });
        cx.run_until_parked();
    }
    seed_and_detach(window, &stream, cx);

    let target = bounds(window, BUTTON, cx);
    assert!(
        (f32::from(target.size.width) - Layout::SCROLL_TO_BOTTOM_SIZE).abs() <= 0.5
            && (f32::from(target.size.height) - Layout::SCROLL_TO_BOTTOM_SIZE).abs() <= 0.5,
        "the control must be a {}px square, got {}×{}",
        Layout::SCROLL_TO_BOTTOM_SIZE,
        f32::from(target.size.width),
        f32::from(target.size.height),
    );

    let palette = if dark {
        vega_theme::DARK
    } else {
        vega_theme::LIGHT
    };
    let (quads, scale) = painted_quads(window, cx);
    let at_bounds: Vec<&Quad> = quads
        .iter()
        .filter(|quad| quad_matches(quad, target, scale))
        .collect();
    let circle = at_bounds
        .iter()
        .copied()
        .find(|quad| {
            quad.background
                .as_solid()
                .is_some_and(|solid| same_colour(solid, palette.bg_elevated))
        })
        .unwrap_or_else(|| {
            panic!(
                "no bg_elevated quad at the control's bounds (dark={dark}); quads there: {:?}",
                at_bounds
                    .iter()
                    .map(|quad| quad.background)
                    .collect::<Vec<_>>()
            )
        });

    let half = (Layout::SCROLL_TO_BOTTOM_SIZE / 2.0) * scale;
    for (corner, actual) in [
        ("top_left", circle.corner_radii.top_left),
        ("top_right", circle.corner_radii.top_right),
        ("bottom_right", circle.corner_radii.bottom_right),
        ("bottom_left", circle.corner_radii.bottom_left),
    ] {
        assert!(
            (actual.as_f32() - half).abs() <= 0.75,
            "the control must be a full circle: {corner} radius {} ≠ half-size {half}",
            actual.as_f32(),
        );
    }

    // GPUI paints a bordered div's border on the same quad as its fill, but a
    // `.shadow_sm()` surface can also emit a bounds-matching quad, so accept the
    // border on any quad at the control's exact bounds.
    let bordered = at_bounds.iter().copied().find(|quad| {
        same_colour(quad.border_color, palette.border_subtle)
            && quad.border_widths.top.as_f32() >= 1.0 * scale - 0.5
    });
    assert!(
        bordered.is_some(),
        "the control must carry a 1px border_subtle border (dark={dark}); border quads there: {:?}",
        at_bounds
            .iter()
            .map(|quad| (quad.border_color, quad.border_widths))
            .collect::<Vec<_>>()
    );
}

#[gpui_kit::test]
async fn issue98_a1_a2_button_surface_light(cx: &mut TestAppContext) {
    assert_button_surface(cx, false).await;
}

#[gpui_kit::test]
async fn issue98_a1_a2_button_surface_dark(cx: &mut TestAppContext) {
    assert_button_surface(cx, true).await;
}

/// A3: the button floats on the content column's centre — this is the
/// "floating, not top-right" contract the issue is about.
#[gpui_kit::test]
async fn issue98_a3_button_is_centred_on_the_content_column(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "issue98-a3");
    seed_and_detach(window, &stream, cx);

    let button = bounds(window, BUTTON, cx);
    let column = bounds(window, COLUMN, cx);
    assert!(
        (f32::from(button.center().x) - f32::from(column.center().x)).abs() <= 1.0,
        "the button must share the content column's axis: button={} column={}",
        f32::from(button.center().x),
        f32::from(column.center().x),
    );
    assert!(
        f32::from(button.bottom()) <= f32::from(column.bottom()) + 0.5,
        "the button must sit inside the transcript viewport"
    );
}

/// A5: a real click re-engages tail follow through `resume_tail`, and the
/// button unmounts because the predicate flips.
#[gpui_kit::test]
async fn issue98_a5_click_resumes_tail_and_unmounts(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "issue98-a5");
    seed_and_detach(window, &stream, cx);

    let center = bounds(window, BUTTON, cx).center();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(center, Modifiers::default());
    visual.run_until_parked();

    assert!(
        stream.read_with(cx, |stream, _| stream.following_tail()),
        "clicking the control must re-engage tail follow"
    );
    assert!(
        !mounted(window, BUTTON, cx),
        "after resuming the tail the control must unmount"
    );
}

/// A6: the keyboard path — focusing the control and pressing Enter runs the
/// same `ResumeTail` action.
#[gpui_kit::test]
async fn issue98_a6_enter_resumes_tail(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "issue98-a6");
    seed_and_detach(window, &stream, cx);

    window
        .update(cx, |_, window, cx| {
            let focus = stream.read_with(cx, |stream, _| stream.resume_tail_focus.clone());
            window.focus(&focus, cx);
        })
        .expect("issue98 window");
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();

    assert!(
        stream.read_with(cx, |stream, _| stream.following_tail()),
        "Enter on the focused control must resume the tail"
    );
}

/// A7: the accessible name survives even though the visible label is gone.
///
/// The test platform exposes no accessibility-tree query
/// (`Window::debug_a11y_tree_json` exists, but no test in this repo can drive
/// accesskit activation, so it returns `None`), so the accessible name cannot
/// be asserted here. This case pins the only observable proxy: the control is
/// mounted and its focus handle is wired (the same handle A6 activates). The
/// `aria_label("回到底部")` itself is pinned by the production source
/// (`render_resume_tail`) and the native inspection; recorded as SKIP in the
/// delivery matrix rather than faked.
#[gpui_kit::test]
async fn issue98_a7_button_is_mounted_and_focusable(cx: &mut TestAppContext) {
    let (window, stream, _events) = open_controller_stream(cx, "issue98-a7");
    seed_and_detach(window, &stream, cx);

    assert!(mounted(window, BUTTON, cx), "the control must be mounted");
    window
        .update(cx, |_, window, cx| {
            let focus = stream.read_with(cx, |stream, _| stream.resume_tail_focus.clone());
            window.focus(&focus, cx);
        })
        .expect("issue98 window");
    let focused = window
        .update(cx, |_, window, cx| {
            let focus = stream.read_with(cx, |stream, _| stream.resume_tail_focus.clone());
            focus.contains_focused(window, cx)
        })
        .expect("issue98 window");
    assert!(
        focused,
        "the control's focus handle must be reachable on the mounted control"
    );
}
