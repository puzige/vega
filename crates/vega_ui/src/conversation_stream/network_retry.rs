use super::*;

pub(crate) struct ResponseCheckpoint {
    message_id: String,
    response_id: String,
    text_start_bytes: usize,
    entries: HashSet<String>,
    activity: run_activity::RunActivityCheckpoint,
    thinking_bytes: usize,
    thinking_blocks: usize,
    segment_ordinal: usize,
}

impl ConversationStream {
    pub(crate) fn checkpoint_response(
        &mut self,
        message_id: &str,
        response_id: &str,
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
        self.begin_context_response(response_id);
        self.active_response_id = Some(response_id.into());
        self.response_reasoning_completed = false;
        self.active_thinking = None;
        self.network_retry = None;
        let active_segment = self.active_run_activity_segment;
        self.response_checkpoint = Some(ResponseCheckpoint {
            message_id: message_id.into(),
            response_id: response_id.into(),
            text_start_bytes,
            entries: self
                .entry_identities
                .iter()
                .filter(|identity| identity.message_id.as_deref() == Some(message_id))
                .map(|identity| identity.key.clone())
                .collect(),
            activity: group.update(cx, |group, cx| {
                group.set_phase(run_activity::RunPhase::Requesting, cx);
                group.response_checkpoint(active_segment, cx)
            }),
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
            || self.active_response_id.as_deref() != Some(checkpoint.response_id.as_str())
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
        let activity = checkpoint.activity.clone();
        self.thinking_bytes = checkpoint.thinking_bytes;
        self.thinking_blocks = checkpoint.thinking_blocks;
        self.active_segment_ordinal = checkpoint.segment_ordinal;
        self.clear_final_copy(message_id);
        if let Some(responses) = self.run_responses.get_mut(message_id) {
            responses.rollback(text_start_bytes);
        }
        if let Some(notices) = self.generated_notices.get_mut(message_id) {
            notices.retain(|span| span.display_end <= text_start_bytes);
        }
        let anchor = self.scroll_anchor_snapshot();
        for index in removed.into_iter().rev() {
            self.entries.remove(index);
            self.list_remove(index);
            self.shift_entry_indices(index, 1);
        }
        if let Some((_, group)) = &self.active_run_activity {
            group.update(cx, |group, cx| {
                group.rollback_response(&activity, cx);
                group.set_phase(run_activity::RunPhase::Requesting, cx);
            });
        }
        self.active_agent_message = Some((message_id.into(), usize::MAX));
        self.active_response_start = Some(text_start_bytes);
        self.response_reasoning_completed = false;
        self.active_run_activity_segment = activity.active_segment();
        self.active_segment_has_text = false;
        self.active_segment_has_visible_text = false;
        self.active_thinking = None;
        self.network_retry = Some((message_id.into(), attempt, max_retries));
        self.refresh_run_notices(cx);
        self.restore_scroll_anchor(&anchor);
        cx.notify();
    }

    pub(crate) fn matches_response(&self, message_id: &str, response_id: &str) -> bool {
        self.active_agent_message
            .as_ref()
            .is_some_and(|(active, _)| active == message_id)
            && self.active_response_id.as_deref() == Some(response_id)
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
            ConversationEvent::ReasoningCompleted {
                message_id,
                response_id,
            } if self.matches_response(message_id, response_id) => Some(message_id.as_str()),
            ConversationEvent::AssistantResponseCompleted {
                message_id,
                response,
            } if self.matches_response(message_id, &response.response_id) => {
                Some(message_id.as_str())
            }
            ConversationEvent::MessageFinished { message_id, .. }
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
