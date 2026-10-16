use super::*;
use gpui_kit::{Bounds, Modifiers, VisualTestContext};
use vega_conversation::types::PendingMessage;

fn message(id: &str, content: &str) -> PendingMessage {
    PendingMessage {
        id: id.into(),
        thread_id: "issue299".into(),
        generation: "run-one".into(),
        behavior: PendingMessageBehavior::Queue,
        content: content.into(),
        images: Vec::new(),
    }
}

fn project(
    stream: &Entity<ConversationStream>,
    messages: Vec<PendingMessage>,
    cx: &mut TestAppContext,
) {
    stream.update(cx, |stream, cx| {
        stream.apply_pending_queue(
            PendingQueueProjection {
                messages,
                ..Default::default()
            },
            cx,
        );
    });
    cx.run_until_parked();
}

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

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let target = bounds(window, selector, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(target.center(), Modifiers::default());
    visual.run_until_parked();
}
