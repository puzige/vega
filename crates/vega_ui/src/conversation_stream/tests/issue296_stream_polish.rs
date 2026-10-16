use super::*;
use gpui_kit::{Modifiers, VisualTestContext};

fn start(stream: &mut ConversationStream, id: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: id.into(),
            seq: 1,
        },
        cx,
    );
}

fn thinking(stream: &mut ConversationStream, id: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::ThinkingDelta {
            message_id: id.into(),
            delta: "Synthetic phase".into(),
        },
        cx,
    );
}

fn tool(
    stream: &mut ConversationStream,
    id: &str,
    status: ToolCallStatus,
    cx: &mut Context<ConversationStream>,
) {
    stream.apply_event(
        ConversationEvent::ToolCallProposed {
            call: bash_call(id, "pwd"),
        },
        cx,
    );
    if status != ToolCallStatus::Rejected {
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: id.into(),
                approval: Approval::Once,
            },
            cx,
        );
    }
    stream.apply_event(
        ConversationEvent::ToolCallFinished {
            call_id: id.into(),
            result: ToolResult {
                status,
                output: "synthetic output".into(),
                reused: false,
                exit_code: (status == ToolCallStatus::Success).then_some(0),
                duration_ms: (status == ToolCallStatus::Success).then_some(10),
                truncated: (status == ToolCallStatus::Success).then_some(false),
                invalid: None,
            },
        },
        cx,
    );
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let row = visual.debug_bounds(selector).expect(selector);
    visual.simulate_click(row.center(), Modifiers::default());
    visual.run_until_parked();
}

fn leave(visual: &mut VisualTestContext) {
    visual.simulate_mouse_move(gpui_kit::point(px(2.), px(2.)), None, Modifiers::default());
    visual.run_until_parked();
}

fn history_body(
    id: &str,
    seq: i64,
    content: &str,
    status: vega_conversation::history::AssistantStatus,
) -> HistoryEntry {
    HistoryEntry::AssistantText {
        seq,
        message_id: id.into(),
        content: content.into(),
        status,
        execution_duration_ms: Some(100),
    }
}

pub(super) fn focus_row(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual.debug_bounds(selector).unwrap();
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| window.blur(cx));
    for _ in 0..40 {
        visual.update(|window, cx| window.focus_next(cx));
        visual.run_until_parked();
        if visual.update(|window, cx| {
            let colors = theme(cx).colors;
            window.painted_quads().iter().any(|quad| {
                quad.bounds == bounds.scale(window.scale_factor())
                    && (quad.background.as_solid() == Some(colors.bg_hover.into())
                        || quad.border_color == colors.accent.into())
            })
        }) {
            return;
        }
    }
    panic!("disclosure did not receive keyboard focus: {selector}");
}

struct DisabledDisclosureHarness {
    expanded: bool,
    focus: FocusHandle,
}
impl Render for DisabledDisclosureHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .id("disabled-disclosure")
            .group("disabled-disclosure")
            .debug_selector(|| "disabled-disclosure".into())
            .h(px(25.))
            .w(px(200.))
            .flex()
            .track_focus(&self.focus)
            .children([
                crate::disclosure::chevron(
                    "disabled-disclosure".into(),
                    "disabled-legacy".into(),
                    &self.focus,
                    self.expanded,
                    false,
                    colors,
                    window,
                ),
                crate::disclosure::inline_chevron(
                    "disabled-disclosure".into(),
                    "disabled-inline".into(),
                    &self.focus,
                    self.expanded,
                    false,
                    colors,
                    window,
                ),
            ])
    }
}
