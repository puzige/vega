use super::*;
use gpui_kit::{ListOffset, Modifiers, VisualTestContext};

fn fixture(
    cx: &mut TestAppContext,
    final_state: bool,
) -> (WindowHandle<StreamHarness>, Entity<ConversationStream>) {
    let (window, stream, _) = open_controller_stream(cx, "sticky-disclosure");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "sticky".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "sticky".into(),
                delta: "Synthetic process".into(),
            },
            cx,
        );
        for index in 0..20 {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(&format!("tool-{index}"), "pwd"),
                },
                cx,
            );
        }
        if final_state {
            confirm_test_final(stream, "sticky", cx);
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "sticky".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                    execution_duration_ms: Some(1000),
                },
                cx,
            );
            stream.composer_submit_pending = true;
            stream.accept_composer_submission(&"Following visible content.\n\n".repeat(80), cx);
        } else {
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "sticky".into(),
                    delta: "Following visible content.\n\n".repeat(80),
                },
                cx,
            );
        }
    });
    cx.run_until_parked();
    (window, stream)
}

fn position(
    stream: &Entity<ConversationStream>,
    index: usize,
    offset: f32,
    visual: &mut VisualTestContext,
) {
    stream.update(visual, |stream, cx| {
        stream.list.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: px(offset),
        });
        cx.notify();
    });
    visual.run_until_parked();
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual.debug_bounds(selector).expect(selector);
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

fn wheel(visual: &mut VisualTestContext, position: gpui_kit::Point<Pixels>, delta: f32) {
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position,
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(delta))),
        modifiers: Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    visual.run_until_parked();
}

fn focus_header(visual: &mut VisualTestContext, selector: &'static str) {
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| window.blur(cx));
    for _ in 0..40 {
        visual.update(|window, cx| window.focus_next(cx));
        visual.run_until_parked();
        let bounds = visual.debug_bounds(selector).unwrap();
        if visual.update(|window, cx| {
            window.painted_quads().iter().any(|quad| {
                quad.bounds == bounds.scale(window.scale_factor())
                    && quad.background.as_solid() == Some(theme(cx).colors.bg_hover.into())
            })
        }) {
            return;
        }
    }
    panic!("header keyboard focus");
}
