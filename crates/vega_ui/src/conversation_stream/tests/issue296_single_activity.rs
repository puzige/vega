use super::*;

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "single".into(),
            seq: 1,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::RunStarted {
            message_id: "single".into(),
            started_at: Instant::now() - Duration::from_secs(12),
        },
        cx,
    );
}
