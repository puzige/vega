use super::*;
use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "run".into(),
            seq: 1,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::RunStarted {
            message_id: "run".into(),
            started_at: Instant::now() - Duration::from_secs(22),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: "run".into(),
            delta: "检查。".into(),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::AssistantResponseCompleted {
            message_id: "run".into(),
            response: AssistantResponseMetadata {
                response_id: "commentary".into(),
                text_start_bytes: 0,
                text_end_bytes: "检查。".len(),
                kind: AssistantResponseKind::Commentary,
            },
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::SummaryDelta {
            message_id: "run".into(),
            delta: "Reviewing changes".into(),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: "run".into(),
            delta: "答案。".into(),
        },
        cx,
    );
}

fn terminal() -> ConversationEvent {
    ConversationEvent::MessageFinished {
        message_id: "run".into(),
        stop_reason: vega_conversation::types::ConversationStopReason::End,
        execution_duration_ms: Some(23_100),
    }
}

struct PhaseVisibilityHarness {
    stream: Entity<ConversationStream>,
    visible: bool,
}

impl Render for PhaseVisibilityHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .when(self.visible, |view| view.child(self.stream.clone()))
    }
}

struct ShimmerBoundsHarness;

impl Render for ShimmerBoundsHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "long-shimmer-row".into())
            .w(px(100.))
            .h(px(Layout::DISCLOSURE_HEADER_HEIGHT))
            .child(shimmer::text(
                "正在检查与整理很长的真实摘要，内容需要省略并保持行高稳定".into(),
                Instant::now(),
                window,
                cx,
            ))
    }
}
