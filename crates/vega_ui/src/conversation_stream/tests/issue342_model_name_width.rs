use super::*;
use gpui_kit::{Bounds, Modifiers, Pixels, VisualTestContext, size};

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing selector {selector}"))
}
