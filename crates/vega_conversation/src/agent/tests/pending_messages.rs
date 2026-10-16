use super::*;
use crate::types::{PendingMessage, PendingMessageBehavior};

fn queued(id: &str, behavior: PendingMessageBehavior, content: &str) -> PendingMessage {
    PendingMessage {
        id: id.into(),
        thread_id: "thread-1".into(),
        generation: "run-1".into(),
        behavior,
        content: content.into(),
        images: Vec::new(),
    }
}
