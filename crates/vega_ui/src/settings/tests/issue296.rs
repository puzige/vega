use super::*;

fn reveal_settings_control(
    window: WindowHandle<SettingsHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<gpui_kit::Pixels> {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let control = visual.debug_bounds(selector).expect("Settings control");
    let viewport = visual
        .debug_bounds("settings-section-content")
        .expect("Settings viewport");
    let delta = if control.bottom() > viewport.bottom() {
        viewport.bottom() - control.bottom() - px(8.)
    } else if control.top() < viewport.top() {
        viewport.top() - control.top() + px(8.)
    } else {
        px(0.)
    };
    if delta != px(0.) {
        visual.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), delta)),
            modifiers: Default::default(),
            touch_phase: gpui_kit::TouchPhase::Moved,
        });
        visual.run_until_parked();
    }
    let control = visual
        .debug_bounds(selector)
        .expect("visible Settings control");
    assert!(control.top() >= viewport.top() && control.bottom() <= viewport.bottom());
    assert!(control.left() >= viewport.left() && control.right() <= viewport.right());
    control
}
