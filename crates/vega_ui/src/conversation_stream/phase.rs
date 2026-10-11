use super::*;
use std::collections::HashSet;
use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};

#[derive(Default)]
pub(crate) struct RunResponses {
    responses: Vec<AssistantResponseMetadata>,
    invalid: bool,
}

impl RunResponses {
    pub(crate) fn push(&mut self, response: AssistantResponseMetadata) {
        if let Some(existing) = self
            .responses
            .iter()
            .find(|item| item.response_id == response.response_id)
        {
            self.invalid |= existing != &response;
            return;
        }
        self.responses.push(response);
    }
}

pub(crate) fn process_copies(
    message_id: &str,
    responses: &RunResponses,
    entries: &[StreamEntry],
    identities: &[StreamEntryIdentity],
) -> Option<HashSet<u64>> {
    if responses.invalid {
        return None;
    }
    let mut text = String::new();
    let mut copies = Vec::new();
    for (entry, identity) in entries.iter().zip(identities) {
        if identity.message_id.as_deref() == Some(message_id)
            && let StreamEntry::Assistant { copy, failure, .. } = entry
        {
            if failure.is_some() {
                return None;
            }
            let start = text.len();
            text.push_str(&copy.original_text());
            copies.push((copy.id, start, text.len()));
        }
    }
    let mut end = 0;
    let mut final_start = None;
    for response in &responses.responses {
        if !response.is_valid_for(&text) || response.text_start_bytes < end || final_start.is_some()
        {
            return None;
        }
        end = response.text_end_bytes;
        if response.kind == AssistantResponseKind::Final {
            final_start = Some(response.text_start_bytes);
        }
    }
    let final_start = final_start?;
    if copies
        .iter()
        .any(|(_, start, end)| *start < final_start && *end > final_start)
    {
        return None;
    }
    Some(
        copies
            .into_iter()
            .filter_map(|(id, start, end)| {
                (end <= final_start
                    && responses.responses.iter().any(|response| {
                        response.kind == AssistantResponseKind::Commentary
                            && response.text_start_bytes <= start
                            && response.text_end_bytes >= end
                    }))
                .then_some(id)
            })
            .collect(),
    )
}

impl ConversationStream {
    pub(crate) fn apply_response_metadata(
        &mut self,
        message_id: &str,
        response: AssistantResponseMetadata,
        cx: &mut Context<Self>,
    ) {
        if !self
            .active_agent_message
            .as_ref()
            .is_some_and(|(id, _)| id == message_id)
        {
            return;
        }
        let responses = self.run_responses.entry(message_id.to_owned()).or_default();
        responses.push(response);
        let copies = process_copies(message_id, responses, &self.entries, &self.entry_identities);
        if let Some(group) = self.run_activity_groups.get(message_id) {
            group.update(cx, |group, cx| {
                if let Some(copies) = copies {
                    group.confirm_final(copies, true, cx);
                } else if responses.invalid
                    || responses
                        .responses
                        .iter()
                        .any(|response| response.kind == AssistantResponseKind::Final)
                {
                    group.invalidate_final(cx);
                }
            });
        }
    }

    pub(crate) fn process_entry_hidden(&self, index: usize, cx: &App) -> bool {
        let Some(StreamEntry::Assistant {
            copy,
            failure: None,
            ..
        }) = self.entries.get(index)
        else {
            return false;
        };
        self.entry_identities
            .get(index)
            .and_then(|identity| identity.message_id.as_ref())
            .and_then(|id| self.run_activity_groups.get(id))
            .is_some_and(|group| group.read(cx).hides_copy(copy.id))
    }

    pub(crate) fn settle_final_presentation(&mut self, window: &Window, cx: &mut Context<Self>) {
        let groups = self
            .run_activity_groups
            .values()
            .filter(|group| group.read(cx).collapse_pending())
            .cloned()
            .collect::<Vec<_>>();
        for group in groups {
            let reading = !self.list.is_following_tail() || self.selection_focus.is_focused(window)
                || group.read(cx).reading(window, cx)
                || self.entries.iter().any(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.has_selected_text()));
            let anchor = self.scroll_anchor_snapshot();
            group.update(cx, |group, cx| group.settle_final(reading, cx));
            for index in 0..self.entries.len() {
                if self.process_entry_hidden(index, cx) {
                    self.invalidate_item(Some(index));
                }
            }
            if reading {
                self.restore_scroll_anchor(&anchor);
            }
        }
    }
}
