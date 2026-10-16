use super::*;
use crate::types::{PendingMessage, PendingMessageBehavior};

fn pending(id: &str, behavior: PendingMessageBehavior, content: &str) -> PendingMessage {
    PendingMessage {
        id: id.into(),
        thread_id: "thread-1".into(),
        generation: "run-1".into(),
        behavior,
        content: content.into(),
        images: Vec::new(),
    }
}

fn final_response(text: &str) -> Vec<ScriptStep> {
    vec![
        ScriptStep::text(text),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]
}
