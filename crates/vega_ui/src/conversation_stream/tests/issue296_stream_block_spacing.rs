use super::*;
use gpui_kit::VisualTestContext;

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "spacing".into(),
            seq: 1,
        },
        cx,
    );
}

fn prose(stream: &mut ConversationStream, text: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: "spacing".into(),
            delta: text.into(),
        },
        cx,
    );
}

fn tool(stream: &mut ConversationStream, id: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::ToolCallProposed {
            call: bash_call(id, "pwd"),
        },
        cx,
    );
}

fn response(
    stream: &mut ConversationStream,
    id: &str,
    start: usize,
    end: usize,
    kind: vega_conversation::types::AssistantResponseKind,
    cx: &mut Context<ConversationStream>,
) {
    stream.apply_event(
        ConversationEvent::AssistantResponseCompleted {
            message_id: "spacing".into(),
            response: vega_conversation::types::AssistantResponseMetadata {
                response_id: id.into(),
                text_start_bytes: start,
                text_end_bytes: end,
                kind,
            },
        },
        cx,
    );
}
