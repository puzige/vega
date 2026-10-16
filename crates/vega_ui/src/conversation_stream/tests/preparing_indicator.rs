use super::*;
use gpui_kit::VisualTestContext;

fn bounds_height(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> f32 {
    cx.run_until_parked();
    let bounds = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    f32::from(bounds.size.height)
}
