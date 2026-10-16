//! Production context subscriptions and workers; only the provider is mocked.
use super::*;

fn reply(text: &str) -> Vec<vega_runtime::ScriptStep> {
    vec![vega_runtime::ScriptStep::events(vec![
        vega_runtime::ProviderEvent::TextDelta(text.into()),
        vega_runtime::ProviderEvent::Done {
            stop_reason: vega_runtime::StopReason::End,
        },
    ])]
}

fn click_context_control(f: &Fixture, selector: &'static str, cx: &mut gpui_kit::TestAppContext) {
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    let viewport = visual
        .debug_bounds("provider-model-configuration")
        .expect("model configuration viewport");
    let control = visual
        .debug_bounds(selector)
        .expect("model context control");
    let delta = if control.bottom() > viewport.bottom() {
        viewport.bottom() - control.bottom() - px(8.)
    } else if control.top() < viewport.top() {
        viewport.top() - control.top() + px(8.)
    } else {
        px(0.)
    };
    if delta != px(0.) {
        visual.simulate_mouse_move(viewport.center(), None, Default::default());
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
        .expect("visible model context control");
    assert!(control.top() >= viewport.top() && control.bottom() <= viewport.bottom());
    assert!(control.left() >= viewport.left() && control.right() <= viewport.right());
    visual.simulate_click(control.center(), Default::default());
    visual.run_until_parked();
}

fn input_context(
    f: &Fixture,
    selector: &'static str,
    value: &str,
    cx: &mut gpui_kit::TestAppContext,
) {
    click_context_control(f, selector, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-a");
    let keys = value
        .chars()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    cx.simulate_keystrokes(f.window.into(), &keys);
}
