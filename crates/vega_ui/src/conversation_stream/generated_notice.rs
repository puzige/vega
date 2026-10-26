use super::*;
use std::ops::Range;
use vega_conversation::types::GeneratedNoticeMetadata;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct GeneratedNoticeSpan {
    pub(crate) notice: GeneratedNoticeMetadata,
    pub(crate) display_start: usize,
    pub(crate) display_end: usize,
}

impl GeneratedNoticeSpan {
    pub(crate) fn is_valid_for(&self, text: &str) -> bool {
        self.notice
            .text_end_bytes
            .checked_sub(self.notice.text_start_bytes)
            == Some(self.notice.canonical_text().len())
            && GeneratedNoticeMetadata {
                text_start_bytes: self.display_start,
                text_end_bytes: self.display_end,
                ..self.notice
            }
            .is_valid_for(text)
    }

    pub(crate) fn fragments(&self, range: Range<usize>) -> Option<GeneratedNoticeFragment> {
        let start = self.display_start.max(range.start);
        let end = self.display_end.min(range.end);
        (start < end).then(|| GeneratedNoticeFragment {
            notice: self.notice,
            range: start - range.start..end - range.start,
            notice_offset: start - self.display_start,
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct GeneratedNoticeFragment {
    notice: GeneratedNoticeMetadata,
    range: Range<usize>,
    notice_offset: usize,
}

impl GeneratedNoticeFragment {
    pub(crate) fn shifted(mut self, offset: usize) -> Self {
        self.range.start += offset;
        self.range.end += offset;
        self
    }

    pub(crate) fn within(&self, range: Range<usize>) -> Option<Self> {
        let notice_start = self.range.start.checked_sub(self.notice_offset)?;
        let notice_end = notice_start.checked_add(self.notice.canonical_text().len())?;
        (range.start <= notice_start && notice_end <= range.end).then(|| Self {
            notice: self.notice,
            range: self.range.start - range.start..self.range.end - range.start,
            notice_offset: self.notice_offset,
        })
    }

    fn is_valid_for(&self, text: &str) -> bool {
        let canonical = self.notice.canonical_text();
        let expected = canonical.get(self.notice_offset..self.notice_offset + self.range.len());
        self.range.start < self.range.end
            && expected.is_some()
            && text.get(self.range.clone()) == expected
    }
}

pub(crate) fn project_text(text: &str, fragments: &[GeneratedNoticeFragment]) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    for fragment in fragments {
        if fragment.range.start < cursor || !fragment.is_valid_for(text) {
            return text.to_owned();
        }
        output.push_str(&text[cursor..fragment.range.start]);
        if fragment.notice_offset == 0 {
            output.push_str(&fragment.notice.message());
        }
        cursor = fragment.range.end;
    }
    output.push_str(&text[cursor..]);
    output
}

pub(crate) fn history_notices(
    entries: &[HistoryEntry],
) -> HashMap<(String, i64), Vec<GeneratedNoticeSpan>> {
    let mut texts = HashMap::<(String, i64), String>::new();
    for entry in entries {
        match entry {
            HistoryEntry::AssistantText {
                seq,
                message_id,
                content,
                ..
            } => texts
                .entry((message_id.clone(), *seq))
                .or_default()
                .push_str(content),
            HistoryEntry::Plan { seq, plan, .. } => {
                texts.insert((plan.id.clone(), *seq), plan.content.clone());
            }
            _ => {}
        }
    }
    let mut notices = HashMap::<(String, i64), Vec<GeneratedNoticeSpan>>::new();
    for entry in entries {
        if let HistoryEntry::GeneratedNotice {
            seq,
            message_id,
            notice,
            display_text_start_bytes,
            display_text_end_bytes,
        } = entry
        {
            let owner = (message_id.clone(), *seq);
            let span = GeneratedNoticeSpan {
                notice: *notice,
                display_start: *display_text_start_bytes,
                display_end: *display_text_end_bytes,
            };
            if texts
                .get(&owner)
                .is_some_and(|text| span.is_valid_for(text))
            {
                notices.entry(owner).or_default().push(span);
            }
        }
    }
    for spans in notices.values_mut() {
        spans.sort_by_key(|span| span.display_start);
        if spans
            .windows(2)
            .any(|pair| pair[0].display_end > pair[1].display_start)
        {
            spans.clear();
        }
    }
    notices
}

impl ConversationStream {
    pub(crate) fn apply_generated_notice(
        &mut self,
        message_id: &str,
        notice: GeneratedNoticeMetadata,
        cx: &mut Context<Self>,
    ) {
        if !self
            .active_agent_message
            .as_ref()
            .is_some_and(|(id, _)| id == message_id)
        {
            return;
        }
        let text = self
            .entries
            .iter()
            .zip(&self.entry_identities)
            .filter_map(|(entry, identity)| {
                if identity.message_id.as_deref() == Some(message_id)
                    && let StreamEntry::Assistant { copy, .. } = entry
                {
                    Some(copy.original_text())
                } else {
                    None
                }
            })
            .collect::<String>();
        if !notice.is_valid_for(&text) {
            return;
        }
        let spans = self
            .generated_notices
            .entry(message_id.to_owned())
            .or_default();
        if spans.iter().any(|span| {
            span.notice == notice
                || (notice.text_start_bytes < span.display_end
                    && span.display_start < notice.text_end_bytes)
        }) {
            return;
        }
        spans.push(GeneratedNoticeSpan {
            notice,
            display_start: notice.text_start_bytes,
            display_end: notice.text_end_bytes,
        });
        spans.sort_by_key(|span| span.display_start);
        let mut cursor = 0;
        let mut changed = Vec::new();
        for (index, (entry, identity)) in self
            .entries
            .iter_mut()
            .zip(&self.entry_identities)
            .enumerate()
        {
            if identity.message_id.as_deref() != Some(message_id) {
                continue;
            }
            if let StreamEntry::Assistant {
                copy,
                stream,
                model,
                ..
            } = entry
            {
                let end = cursor + copy.original_text().len();
                copy.set_generated_notices(
                    spans
                        .iter()
                        .filter_map(|span| span.fragments(cursor..end))
                        .collect(),
                );
                if copy.sync_model(stream, model, &self.counters) {
                    changed.push(index);
                }
                cursor = end;
            }
        }
        if let Some(responses) = self.run_responses.get_mut(message_id) {
            responses.refresh_generated_copy(message_id, &self.entries, &self.entry_identities);
        }
        for index in changed {
            self.invalidate_item(Some(index));
        }
        cx.notify();
    }
}
