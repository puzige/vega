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

#[gpui_kit::test]
async fn issue174_pending_preflight_keeps_composer_wrapper_height_and_stop_projection(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue174-pending-preflight");
    stream.update(cx, |stream, cx| {
        stream.input.update(cx, |input, cx| {
            input.set_text("keep this preflight draft", cx)
        });
    });

    let idle_height = bounds_height(window, "composer-wrapper", cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("composer-preparing-request").is_none());
    assert!(visual.debug_bounds("composer-send").is_some());
    assert!(visual.debug_bounds("composer-stop").is_none());

    stream.update(cx, |stream, cx| stream.submit_message(cx));
    cx.run_until_parked();

    let pending = stream.read_with(cx, |stream, _| stream.composer_submit_pending);
    assert!(pending);
    let pending_height = bounds_height(window, "composer-wrapper", cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        (idle_height - pending_height).abs() <= 1.0,
        "pending-only transition changed Composer wrapper height from {idle_height}px to {pending_height}px"
    );
    assert!(visual.debug_bounds("composer-preparing-request").is_none());
    assert!(visual.debug_bounds("composer-send").is_none());
    assert!(visual.debug_bounds("composer-stop").is_some());
}
