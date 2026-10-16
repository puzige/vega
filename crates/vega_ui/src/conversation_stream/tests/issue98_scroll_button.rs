use super::*;
use gpui_kit::{Bounds, Hsla, Modifiers, Pixels, Quad, Rgba, VisualTestContext};

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
