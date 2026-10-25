use super::*;
use std::collections::HashSet;
use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};

#[derive(Default)]
pub(crate) struct RunResponses {
    responses: Vec<AssistantResponseMetadata>,
    invalid: bool,
    final_copy: Option<(u64, MessageCopy)>,
}

impl RunResponses {
    pub(crate) fn push(&mut self, response: AssistantResponseMetadata) -> bool {
        if let Some(existing) = self
            .responses
            .iter()
            .find(|item| item.response_id == response.response_id)
        {
            self.invalid |= existing != &response;
            return false;
        }
        self.responses.push(response);
        true
    }

    fn validated(&self, text: &str) -> Option<&[AssistantResponseMetadata]> {
        if self.invalid {
            return None;
        }
        let mut end = 0;
        let mut final_seen = false;
        for response in &self.responses {
            if !response.is_valid_for(text) || response.text_start_bytes < end || final_seen {
                return None;
            }
            end = response.text_end_bytes;
            final_seen = response.kind == AssistantResponseKind::Final;
        }
        Some(&self.responses)
    }

    pub(crate) fn refresh_final_copy(
        &mut self,
        message_id: &str,
        entries: &[StreamEntry],
        identities: &[StreamEntryIdentity],
    ) {
        let mut text = String::new();
        let mut owner = None;
        let mut failed = false;
        let mut notices = Vec::new();
        for (entry, identity) in entries.iter().zip(identities) {
            if identity.message_id.as_deref() == Some(message_id)
                && let StreamEntry::Assistant {
                    copy,
                    model,
                    failure,
                    ..
                } = entry
            {
                failed |= failure.is_some();
                notices.extend(
                    copy.generated_notices()
                        .into_iter()
                        .map(|notice| notice.shifted(text.len())),
                );
                text.push_str(&copy.original_text());
                if model.row_count() > 0 {
                    owner = Some((copy.id, text.len()));
                }
            }
        }
        let projection = (!failed)
            .then(|| self.validated(&text))
            .flatten()
            .and_then(|responses| {
                responses
                    .iter()
                    .find(|response| response.kind == AssistantResponseKind::Final)
            })
            .and_then(|response| {
                let (owner, _) = owner.filter(|(_, end)| *end > response.text_start_bytes)?;
                Some((
                    owner,
                    text.get(response.text_start_bytes..response.text_end_bytes)?,
                    response.text_start_bytes..response.text_end_bytes,
                ))
            });
        self.final_copy = projection.map(|(owner, source, range)| {
            let copy = self
                .final_copy
                .as_ref()
                .filter(|(previous, copy)| *previous == owner && copy.original_text() == source)
                .map(|(_, copy)| copy.clone())
                .unwrap_or_else(|| MessageCopy::new(source));
            copy.set_generated_notices(
                notices
                    .iter()
                    .filter_map(|notice| notice.within(range.clone()))
                    .collect(),
            );
            (owner, copy)
        });
    }

    pub(crate) fn refresh_generated_copy(
        &mut self,
        message_id: &str,
        entries: &[StreamEntry],
        identities: &[StreamEntryIdentity],
    ) {
        if self.final_copy.is_some() {
            self.refresh_final_copy(message_id, entries, identities);
        }
    }

    fn copy_owner(&self) -> Option<u64> {
        self.final_copy.as_ref().map(|(owner, _)| *owner)
    }

    pub(crate) fn clear_final_copy(&mut self) -> Option<u64> {
        self.final_copy.take().map(|(owner, _)| owner)
    }

    pub(crate) fn rollback(&mut self, start: usize) {
        self.final_copy = None;
        self.responses
            .retain(|response| response.text_end_bytes <= start);
    }
}

pub(crate) fn history_response_boundaries(entries: &[HistoryEntry]) -> HashMap<String, Vec<usize>> {
    let mut texts = HashMap::<String, String>::new();
    let mut responses = HashMap::<String, RunResponses>::new();
    for entry in entries {
        match entry {
            HistoryEntry::AssistantText {
                message_id,
                content,
                ..
            } => texts
                .entry(message_id.clone())
                .or_default()
                .push_str(content),
            HistoryEntry::AssistantResponseMetadata {
                message_id,
                response,
                ..
            } => {
                responses
                    .entry(message_id.clone())
                    .or_default()
                    .push(response.clone());
            }
            _ => {}
        }
    }
    responses
        .into_iter()
        .filter_map(|(message_id, responses)| {
            let text = texts.get(&message_id)?;
            let mut boundaries = responses
                .validated(text)?
                .iter()
                .flat_map(|response| [response.text_start_bytes, response.text_end_bytes])
                .collect::<Vec<_>>();
            boundaries.sort_unstable();
            boundaries.dedup();
            Some((message_id, boundaries))
        })
        .collect()
}

pub(crate) fn process_copies(
    message_id: &str,
    responses: &RunResponses,
    entries: &[StreamEntry],
    identities: &[StreamEntryIdentity],
) -> Option<HashSet<u64>> {
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
    let responses = responses.validated(&text)?;
    let final_start = responses
        .iter()
        .find(|response| response.kind == AssistantResponseKind::Final)?
        .text_start_bytes;
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
                    && responses
                        .iter()
                        .filter(|response| response.kind == AssistantResponseKind::Commentary)
                        .fold(start, |covered, response| {
                            if response.text_start_bytes <= covered
                                && response.text_end_bytes > covered
                            {
                                response.text_end_bytes
                            } else {
                                covered
                            }
                        })
                        >= end)
                    .then_some(id)
            })
            .collect(),
    )
}

pub(crate) enum RunDetail {
    Operation(RunActivityChild),
    Assistant(usize),
}

fn operation_top_inset(index: usize, previous_assistant: bool) -> f32 {
    if index == 0 {
        Layout::CONVERSATION_BODY_TOP
    } else if previous_assistant {
        Layout::CONVERSATION_STAGE_TOP
    } else {
        0.
    }
}

impl ConversationStream {
    pub(crate) fn final_copy_for_entry(&self, index: usize) -> Option<&MessageCopy> {
        let StreamEntry::Assistant {
            copy,
            failure: None,
            ..
        } = self.entries.get(index)?
        else {
            return None;
        };
        let message_id = self.entry_identities.get(index)?.message_id.as_ref()?;
        self.run_responses
            .get(message_id)?
            .final_copy
            .as_ref()
            .filter(|(owner, _)| *owner == copy.id)
            .map(|(_, source)| source)
    }

    pub(crate) fn copy_source_for_entry(&self, index: usize) -> Option<&MessageCopy> {
        if let Some(source) = self.final_copy_for_entry(index) {
            return Some(source);
        }
        if self.thread.backend != TaskBackend::Codex {
            return None;
        }
        let StreamEntry::Assistant {
            copy,
            failure: None,
            ..
        } = self.entries.get(index)?
        else {
            return None;
        };
        let message_id = self
            .entry_identities
            .get(index)
            .and_then(|identity| identity.message_id.as_deref());
        if message_id.is_some_and(|id| self.run_failures.contains_key(id))
            || self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active_id, active_index)| {
                    *active_index == index || message_id == Some(active_id.as_str())
                })
        {
            return None;
        }
        copy.has_text().then_some(copy)
    }

    fn invalidate_copy_owner(&mut self, owner: Option<u64>) {
        if let Some(owner) = owner {
            let index = self.entries.iter().position(
                |entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.id == owner),
            );
            self.invalidate_item(index);
        }
    }

    pub(crate) fn clear_final_copy(&mut self, message_id: &str) -> bool {
        let owner = self
            .run_responses
            .get_mut(message_id)
            .and_then(RunResponses::clear_final_copy);
        self.invalidate_copy_owner(owner);
        owner.is_some()
    }

    pub fn completion_response(&self, message_ids: &[String], cx: &App) -> Option<String> {
        for message_id in message_ids.iter().rev() {
            let copies = self
                .entries
                .iter()
                .zip(&self.entry_identities)
                .filter_map(|(entry, identity)| {
                    if identity.message_id.as_ref() != Some(message_id) {
                        return None;
                    }
                    match entry {
                        StreamEntry::Assistant { copy, .. } => Some(copy),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>();
            if let Some(responses) = self.run_responses.get(message_id) {
                let text = copies
                    .iter()
                    .map(|copy| copy.original_text())
                    .collect::<String>();
                let Some(responses) = responses.validated(&text) else {
                    continue;
                };
                if let Some(response) = responses
                    .iter()
                    .rev()
                    .find(|response| response.kind == AssistantResponseKind::Final)
                    && let Some(text) = text.get(response.text_start_bytes..response.text_end_bytes)
                    && !text.trim().is_empty()
                {
                    let mut offset = 0;
                    let notices = copies
                        .iter()
                        .flat_map(|copy| {
                            let notices = copy
                                .generated_notices()
                                .into_iter()
                                .map(|notice| notice.shifted(offset))
                                .collect::<Vec<_>>();
                            offset += copy.original_text().len();
                            notices
                        })
                        .filter_map(|notice| {
                            notice.within(response.text_start_bytes..response.text_end_bytes)
                        })
                        .collect::<Vec<_>>();
                    return Some(generated_notice::project_text(text, &notices));
                }
                continue;
            }
            for copy in copies.into_iter().rev() {
                if self
                    .run_activity_groups
                    .get(message_id)
                    .is_some_and(|group| group.read(cx).hides_copy(copy.id))
                {
                    continue;
                }
                let text = copy.display_text();
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
        }
        None
    }

    pub(crate) fn run_details(
        &self,
        group: &Entity<RunActivityGroup>,
        cx: &App,
    ) -> Vec<(usize, RunDetail)> {
        if !group.read(cx).process_in_details() {
            return Vec::new();
        }
        let mut details = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            match entry {
                StreamEntry::RunActivitySegment {
                    group: owner,
                    segment,
                } if owner == group => {
                    for child in RunActivityGroup::compact_children(
                        group.read(cx).segment_children(*segment),
                        cx,
                    ) {
                        details.push((index, RunDetail::Operation(child)));
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

    pub(crate) fn stage_top_insets(&self, cx: &App) -> HashMap<usize, f32> {
        let mut previous_clock = HashMap::new();
        let mut previous_visible = None;
        let mut insets = HashMap::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if self.process_entry_hidden(index, cx) {
                continue;
            }
            if matches!(
                entry,
                StreamEntry::Tool { .. }
                    | StreamEntry::ToolGroup { .. }
                    | StreamEntry::ContextCompaction { .. }
            ) {
                let top = match previous_visible.and_then(|previous| self.entries.get(previous)) {
                    Some(
                        StreamEntry::RunActivity { .. }
                        | StreamEntry::User { .. }
                        | StreamEntry::UserImages { .. },
                    ) => Layout::CONVERSATION_BODY_TOP,
                    Some(StreamEntry::Assistant { .. }) => Layout::CONVERSATION_STAGE_TOP,
                    _ => 0.,
                };
                if top > 0. {
                    insets.insert(index, top);
                }
                previous_visible = Some(index);
                continue;
            }
            let Some(owner) = self
                .entry_identities
                .get(index)
                .and_then(|identity| identity.message_id.as_ref())
            else {
                previous_visible = Some(index);
                continue;
            };
            match entry {
                StreamEntry::RunActivity { .. } => {
                    previous_clock.insert(owner, true);
                }
                StreamEntry::RunActivitySegment { group, segment } => {
                    if group.read(cx).segment_row_count(*segment, cx) == 0 {
                        continue;
                    }
                    insets.insert(
                        index,
                        if previous_clock.get(owner) == Some(&true)
                            || matches!(
                                previous_visible.and_then(|previous| self.entries.get(previous)),
                                Some(StreamEntry::User { .. } | StreamEntry::UserImages { .. })
                            )
                        {
                            Layout::CONVERSATION_BODY_TOP
                        } else {
                            Layout::CONVERSATION_STAGE_TOP
                        },
                    );
                    previous_clock.insert(owner, false);
                }
                StreamEntry::Assistant { model, .. } if model.row_count() == 0 => continue,
                StreamEntry::Summary { .. } => continue,
                _ => {
                    previous_clock.insert(owner, false);
                }
            }
            previous_visible = Some(index);
        }
        insets
    }

    pub(crate) fn estimated_run_height(&self, group: &Entity<RunActivityGroup>, cx: &App) -> f32 {
        let header = group.read(cx).estimated_header_height();
        if !group.read(cx).expanded() {
            return header;
        }
        let width = self
            .workspace_width
            .map(|width| width - 2. * Layout::CONVERSATION_PADDING)
            .unwrap_or(Layout::CONVERSATION_MAX_WIDTH)
            .clamp(1., Layout::CONVERSATION_MAX_WIDTH)
            - 2. * Layout::PROCESS_PADDING_X;
        let details = self.run_details(group, cx);
        if details.is_empty() {
            return header;
        }
        let mut height = 0.;
        let mut previous_assistant = false;
        for (index, (_, detail)) in details.iter().enumerate() {
            match detail {
                RunDetail::Operation(child) => {
                    height += operation_top_inset(index, previous_assistant)
                        + RunActivityGroup::estimated_child_height(child, width, cx);
                    previous_assistant = false;
                }
                RunDetail::Assistant(entry_index) => {
                    height += self.estimated_entry_height(*entry_index, cx, false);
                    previous_assistant = true;
                }
            }
        }
        header + height.min(Layout::TOOL_GROUP_MAX_HEIGHT)
    }

    pub(crate) fn render_run_details(
        &self,
        group: Entity<RunActivityGroup>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let details = if group.read(cx).expanded() {
            let mut previous_assistant = false;
            self.run_details(&group, cx)
                .into_iter()
                .enumerate()
                .map(|(detail_index, (_, detail))| match detail {
                    RunDetail::Operation(child) => {
                        let top = operation_top_inset(detail_index, previous_assistant);
                        previous_assistant = false;
                        gpui_kit::div()
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .pt(px(top))
                            .child(RunActivityGroup::render_child(child, window, cx))
                            .into_any_element()
                    }
                    RunDetail::Assistant(index) => {
                        previous_assistant = true;
                        gpui_kit::div()
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
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
            && !group.read(cx).process_in_details()
            && group.update(cx, |group, cx| group.reveal_segment(*segment, cx))
        {
            self.list.remeasure_items(index..index + 1);
            self.list.scroll_to(gpui_kit::ListOffset {
                item_ix: index,
                offset_in_item: px(anchor.offset_in_item_px),
            });
            return true;
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
        if response.kind == AssistantResponseKind::Final
            && let Some((_, index)) = &self.active_agent_message
        {
            let index = *index;
            if let Some(StreamEntry::Assistant {
                copy,
                stream,
                model,
                ..
            }) = self.entries.get_mut(index)
                && copy.sync_model(stream, model, &self.counters)
            {
                self.invalidate_item(Some(index));
            }
        }
        let responses = self.run_responses.entry(message_id.to_owned()).or_default();
        let previous_copy = responses.copy_owner();
        let closes_response = response.kind == AssistantResponseKind::Commentary;
        let inserted = responses.push(response);
        responses.refresh_final_copy(message_id, &self.entries, &self.entry_identities);
        let next_copy = responses.copy_owner();
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
        if inserted && closes_response && !responses.invalid {
            let text = self
                .entries
                .iter()
                .zip(&self.entry_identities)
                .filter_map(|(entry, identity)| {
                    if identity.message_id.as_deref() != Some(message_id) {
                        return None;
                    }
                    match entry {
                        StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
                        _ => None,
                    }
                })
                .collect::<String>();
            if responses.validated(&text).is_some()
                && responses
                    .responses
                    .last()
                    .is_some_and(|response| response.text_end_bytes == text.len())
            {
                self.close_active_segment_at_response(text.len());
            }
        }
        if previous_copy != next_copy {
            self.invalidate_copy_owner(previous_copy);
            self.invalidate_copy_owner(next_copy);
            cx.notify();
        }
    }

    pub(crate) fn process_entry_hidden(&self, index: usize, cx: &App) -> bool {
        if self.compaction_failure_resolved(index) {
            return true;
        }
        if let Some(StreamEntry::RunActivity { group }) = self.entries.get(index) {
            return group.read(cx).header_hidden();
        }
        if matches!(self.entries.get(index), Some(StreamEntry::ContextCompaction { record, .. })
            if matches!(record.status, vega_conversation::types::ContextCompactionStatus::Succeeded | vega_conversation::types::ContextCompactionStatus::Cancelled))
        {
            return true;
        }
        let Some(StreamEntry::Assistant {
            copy,
            model,
            failure: None,
            ..
        }) = self.entries.get(index)
        else {
            return false;
        };
        if model.row_count() == 0 {
            return true;
        }
        self.entry_identities
            .get(index)
            .and_then(|identity| identity.message_id.as_ref())
            .and_then(|id| self.run_activity_groups.get(id))
            .is_some_and(|group| group.read(cx).hides_copy(copy.id))
    }

    pub(crate) fn settle_final_presentation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let groups = self
            .run_activity_groups
            .values()
            .filter(|group| group.read(cx).collapse_pending())
            .cloned()
            .collect::<Vec<_>>();
        for group in groups {
            let copies = self
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    StreamEntry::Assistant { copy, .. }
                        if group.read(cx).owns_process_copy(copy.id) =>
                    {
                        Some(copy.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            let focus_process_selection = self.selection_focus.is_focused(window)
                && copies.iter().any(|copy| copy.selection_has_cursor(cx));
            for copy in copies {
                copy.clear_hidden_selection(window, cx);
            }
            group.update(cx, |group, cx| {
                group.settle_final(focus_process_selection, window, cx)
            });
            for index in 0..self.entries.len() {
                if self.process_entry_hidden(index, cx) {
                    self.invalidate_item(Some(index));
                }
            }
        }
    }
}
