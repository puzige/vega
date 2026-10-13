use super::*;

pub(crate) struct ResponseCheckpoint {
    message_id: String,
    text_start_bytes: usize,
    entries: HashSet<String>,
    segments: usize,
    thinking_bytes: usize,
    thinking_blocks: usize,
    segment_ordinal: usize,
}

impl ConversationStream {
    pub(crate) fn checkpoint_response(
        &mut self,
        message_id: &str,
        text_start_bytes: usize,
        cx: &mut Context<Self>,
    ) {
        let Some((active_id, group)) = self.active_run_activity.clone() else {
            return;
        };
        if active_id != message_id {
            return;
        }
        self.close_active_segment_at_response(text_start_bytes);
        self.active_thinking = None;
        self.active_run_activity_segment = None;
        self.network_retry = None;
        self.response_checkpoint = Some(ResponseCheckpoint {
            message_id: message_id.into(),
            text_start_bytes,
            entries: self
                .entry_identities
                .iter()
                .filter(|identity| identity.message_id.as_deref() == Some(message_id))
                .map(|identity| identity.key.clone())
                .collect(),
            segments: group.update(cx, |group, cx| group.response_checkpoint(cx)),
            thinking_bytes: self.thinking_bytes,
            thinking_blocks: self.thinking_blocks,
            segment_ordinal: self.active_segment_ordinal,
        });
    }

    pub(crate) fn retry_response(
        &mut self,
        message_id: &str,
        text_start_bytes: usize,
        attempt: u32,
        max_retries: u32,
        cx: &mut Context<Self>,
    ) {
        let Some(checkpoint) = &self.response_checkpoint else {
            return;
        };
        if checkpoint.message_id != message_id
            || checkpoint.text_start_bytes != text_start_bytes
            || attempt == 0
            || attempt > max_retries
            || !self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active, _)| active == message_id)
        {
            return;
        }
        let removed = self
            .entry_identities
            .iter()
            .enumerate()
            .filter_map(|(index, identity)| {
                (identity.message_id.as_deref() == Some(message_id)
                    && !checkpoint.entries.contains(&identity.key))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        let segments = checkpoint.segments;
        self.thinking_bytes = checkpoint.thinking_bytes;
        self.thinking_blocks = checkpoint.thinking_blocks;
        self.active_segment_ordinal = checkpoint.segment_ordinal;
        let anchor = self.scroll_anchor_snapshot();
        for index in removed.into_iter().rev() {
            self.entries.remove(index);
            self.list_remove(index);
        }
        if let Some((_, group)) = &self.active_run_activity {
            group.update(cx, |group, cx| group.rollback_response(segments, cx));
        }
        self.active_agent_message = Some((message_id.into(), usize::MAX));
        self.active_response_start = Some(text_start_bytes);
        self.active_run_activity_segment = None;
        self.active_segment_has_text = false;
        self.active_thinking = None;
        self.network_retry = Some((message_id.into(), attempt, max_retries));
        self.refresh_run_notices(cx);
        self.restore_scroll_anchor(&anchor);
        cx.notify();
    }

    pub(crate) fn clear_recovered_retry(&mut self, event: &ConversationEvent) {
        let recovered_id = match event {
            ConversationEvent::TextDelta { message_id, delta }
            | ConversationEvent::ThinkingDelta { message_id, delta }
            | ConversationEvent::SummaryDelta { message_id, delta }
                if !delta.is_empty() =>
            {
                Some(message_id.as_str())
            }
            ConversationEvent::AssistantResponseCompleted { message_id, .. }
            | ConversationEvent::MessageFinished { message_id, .. }
            | ConversationEvent::Interrupted { message_id, .. } => Some(message_id.as_str()),
            ConversationEvent::Error {
                message_id: Some(message_id),
                ..
            } => Some(message_id.as_str()),
            ConversationEvent::ToolCallProposed { .. } => self
                .active_agent_message
                .as_ref()
                .map(|(id, _)| id.as_str()),
            _ => None,
        };
        if self
            .network_retry
            .as_ref()
            .is_some_and(|(id, _, _)| Some(id.as_str()) == recovered_id)
        {
            self.network_retry = None;
        }
    }
}
