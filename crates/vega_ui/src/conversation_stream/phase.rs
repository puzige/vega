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

pub(crate) enum RunDetail {
    Operation(RunActivityChild),
    Assistant(usize),
}

impl ConversationStream {
    pub(crate) fn run_details(
        &self,
        group: &Entity<RunActivityGroup>,
        cx: &App,
    ) -> Vec<(usize, RunDetail)> {
        let mut details = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            match entry {
                StreamEntry::RunActivitySegment {
                    group: owner,
                    segment,
                } if owner == group => {
                    for child in group.read(cx).segment_children(*segment) {
                        if !matches!(child, RunActivityChild::Artifact(_)) {
                            details.push((index, RunDetail::Operation(child)));
                        }
                    }
                }
                StreamEntry::Assistant { copy, .. } if group.read(cx).hides_copy(copy.id) => {
                    details.push((index, RunDetail::Assistant(index)));
                }
                _ => {}
            }
        }
        details
    }

    pub(crate) fn estimated_run_height(&self, group: &Entity<RunActivityGroup>, cx: &App) -> f32 {
        let header = group.read(cx).estimated_header_height();
        if !group.read(cx).expanded() {
            return header;
        }
        let width = self
            .workspace_width
            .map(|width| width - 2. * Layout::CONVERSATION_PADDING)
            .unwrap_or(Layout::CONTENT_MAX_WIDTH)
            .clamp(1., Layout::CONTENT_MAX_WIDTH)
            - 2. * Layout::PROCESS_PADDING_X;
        let details = self.run_details(group, cx);
        if details.is_empty() {
            return header;
        }
        let height = details
            .iter()
            .map(|(_, detail)| match detail {
                RunDetail::Operation(child) => {
                    RunActivityGroup::estimated_child_height(child, width, cx)
                }
                RunDetail::Assistant(index) => message_anchors::estimate_entry_height(
                    &self.entries[*index],
                    cx,
                    self.workspace_width,
                ),
            })
            .sum::<f32>()
            + Layout::PROCESS_ITEM_GAP * details.len().saturating_sub(1) as f32;
        header + Layout::PROCESS_ITEM_GAP + height.min(Layout::TOOL_GROUP_MAX_HEIGHT)
    }

    pub(crate) fn render_run_details(
        &self,
        group: Entity<RunActivityGroup>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let details = if group.read(cx).expanded() {
            self.run_details(&group, cx)
                .into_iter()
                .map(|(_, detail)| match detail {
                    RunDetail::Operation(child) => {
                        RunActivityGroup::render_child(child, window, cx)
                    }
                    RunDetail::Assistant(index) => {
                        let selector = match &self.entries[index] {
                            StreamEntry::Assistant { copy, .. } => {
                                format!("run-process-message-{}", copy.id)
                            }
                            _ => format!("run-process-entry-{index}"),
                        };
                        gpui_kit::div()
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .debug_selector(move || selector.clone())
                            .child(render_rows::render_entry_with_selection(
                                &self.entries[index],
                                window,
                                cx,
                                &self.selection_focus,
                            ))
                            .into_any_element()
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        RunActivityGroup::render_with_details(group, details, window, cx)
    }

    pub(crate) fn restore_process_anchor(
        &mut self,
        anchor: &ThreadScrollAnchor,
        cx: &mut Context<Self>,
    ) -> bool {
        if anchor.following_tail {
            return false;
        }
        let Some(index) = self
            .entry_identities
            .iter()
            .position(|identity| Some(identity.key.as_str()) == anchor.identity.as_deref())
        else {
            return false;
        };
        if let StreamEntry::RunActivitySegment { group, segment } = &self.entries[index]
            && group
                .read(cx)
                .segment_children(*segment)
                .iter()
                .any(|child| matches!(child, RunActivityChild::Artifact(_)))
        {
            return false;
        }
        let Some(group) = self.entry_identities[index]
            .message_id
            .as_ref()
            .and_then(|id| self.run_activity_groups.get(id))
            .cloned()
        else {
            return false;
        };
        let Some(detail_index) = self
            .run_details(&group, cx)
            .iter()
            .position(|(source, _)| *source == index)
        else {
            return false;
        };
        let Some(header_index) = self.entries.iter().position(
            |entry| matches!(entry, StreamEntry::RunActivity { group: owner } if owner == &group),
        ) else {
            return false;
        };
        group.update(cx, |group, cx| {
            group.reveal_detail(detail_index, anchor.offset_in_item_px, cx)
        });
        self.list.scroll_to(gpui_kit::ListOffset {
            item_ix: header_index,
            offset_in_item: px(0.),
        });
        true
    }

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
