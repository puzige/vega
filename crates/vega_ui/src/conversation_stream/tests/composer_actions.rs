use super::*;

fn frame_has(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .is_some()
}
