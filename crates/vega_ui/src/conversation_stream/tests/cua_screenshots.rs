use super::*;
use gpui_kit::{VisualTestContext, point, size};

fn picture(name: &str) -> ImageAttachment {
    let bytes: &[u8] = match name {
        "landscape" => include_bytes!("fixtures/screenshot-landscape.png"),
        "portrait" => include_bytes!("fixtures/screenshot-portrait.png"),
        "strip" => include_bytes!("fixtures/screenshot-strip.png"),
        "column" => include_bytes!("fixtures/screenshot-column.png"),
        "tiny" => include_bytes!("fixtures/screenshot-tiny.png"),
        _ => unreachable!(),
    };
    ImageAttachment::from_bytes(bytes.to_vec()).unwrap()
}

fn deliver(stream: &Entity<ConversationStream>, image: ImageAttachment, cx: &mut TestAppContext) {
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "shot-message".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ScreenshotDelivered {
                message_id: "shot-message".into(),
                call_id: "shot".into(),
                image,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                message_id: "shot-message".into(),
                execution_duration_ms: Some(100),
            },
            cx,
        );
    });
    cx.run_until_parked();
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Default::default());
    visual.run_until_parked();
}
