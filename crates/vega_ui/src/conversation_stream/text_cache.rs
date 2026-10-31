use super::*;
use std::ops::Range;

const RECORD_LIMIT: usize = 2048;
const PAYLOAD_LIMIT: usize = 2 * 1024 * 1024;
const RECORD_PAYLOAD_LIMIT: usize = 4 * 1024;
const READING_LIMIT: usize = 1792;
const TAIL_LIMIT: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TextVariant {
    User,
    Assistant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextOwner {
    variant: TextVariant,
    incarnation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextSource {
    owner: TextOwner,
    revision: u64,
}

impl TextSource {
    fn for_entry(entry: &StreamEntry) -> Option<Self> {
        let (variant, copy, revision) = match entry {
            StreamEntry::User { copy, .. } => (TextVariant::User, copy, copy.content_generation()),
            StreamEntry::Assistant { copy, model, .. } => {
                (TextVariant::Assistant, copy, model.content_revision())
            }
            _ => return None,
        };
        Some(Self {
            owner: TextOwner {
                variant,
                incarnation: copy.id,
            },
            revision,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TextWidth(Option<f32>);

impl TextWidth {
    fn new(workspace_width: Option<f32>) -> Self {
        Self(workspace_width.map(|width| {
            (width - 2. * Layout::CONVERSATION_PADDING).clamp(1., Layout::CONVERSATION_MAX_WIDTH)
        }))
    }

    fn effective(self) -> f32 {
        self.0.unwrap_or(Layout::CONVERSATION_MAX_WIDTH)
    }
}

struct TextRecord {
    source: TextSource,
    estimate: Option<(TextWidth, f32)>,
    preview: Option<String>,
}

#[derive(Default)]
pub(super) struct TextCache {
    records: HashMap<String, TextRecord>,
    admitted: HashSet<usize>,
    retained_bytes: usize,
}

impl TextCache {
    pub(super) fn clear(&mut self) {
        self.records.clear();
        self.admitted.clear();
        self.retained_bytes = 0;
    }

    pub(super) fn retire(&mut self, key: &str) {
        if let Some((key, record)) = self.records.remove_entry(key) {
            self.retained_bytes -= record_payload(key.capacity(), &record);
        }
    }

    pub(super) fn prepare(
        &mut self,
        entries: &[StreamEntry],
        identities: &[StreamEntryIdentity],
        reading_index: usize,
    ) {
        let sources =
            entries
                .iter()
                .zip(identities)
                .enumerate()
                .filter_map(|(index, (entry, identity))| {
                    TextSource::for_entry(entry)
                        .map(|source| (index, identity.key.as_str(), source))
                });
        let total = sources.clone().count();
        let before_reading = sources
            .clone()
            .take_while(|(index, _, _)| *index < reading_index)
            .count();
        let reading = reading_range(total, before_reading);
        let tail_start = total.saturating_sub(TAIL_LIMIT);
        let selected = sources
            .enumerate()
            .filter(|(ordinal, _)| reading.contains(ordinal) || *ordinal >= tail_start)
            .map(|(_, (index, key, source))| (key, (index, source)))
            .collect::<HashMap<_, _>>();
        self.admitted.clear();
        self.admitted
            .extend(selected.values().map(|(index, _)| *index));
        self.records.retain(|key, record| {
            selected
                .get(key.as_str())
                .is_some_and(|(_, source)| *source == record.source)
        });
        self.retained_bytes = self
            .records
            .iter()
            .map(|(key, record)| record_payload(key.capacity(), record))
            .sum();
    }

    fn record(&mut self, index: usize, key: &str, source: TextSource) -> Option<&mut TextRecord> {
        if self
            .records
            .get(key)
            .is_some_and(|record| record.source != source)
        {
            self.retire(key);
        }
        if !self.records.contains_key(key) {
            if !self.admitted.contains(&index)
                || self.records.len() >= RECORD_LIMIT
                || key.len() > RECORD_PAYLOAD_LIMIT
                || self.retained_bytes + key.len() > PAYLOAD_LIMIT
            {
                return None;
            }
            let key = key.to_string();
            if key.capacity() > RECORD_PAYLOAD_LIMIT
                || self.retained_bytes + key.capacity() > PAYLOAD_LIMIT
            {
                return None;
            }
            self.retained_bytes += key.capacity();
            self.records.insert(
                key,
                TextRecord {
                    source,
                    estimate: None,
                    preview: None,
                },
            );
        }
        self.records.get_mut(key)
    }

    pub(super) fn height(
        &mut self,
        index: usize,
        key: &str,
        entry: &StreamEntry,
        workspace_width: Option<f32>,
    ) -> Option<f32> {
        let source = TextSource::for_entry(entry)?;
        let width = TextWidth::new(workspace_width);
        if let Some(record) = self.records.get(key)
            && record.source == source
            && let Some((cached_width, height)) = record.estimate
            && cached_width == width
        {
            return Some(height);
        }
        let height = raw_text_height(entry, width.effective())?;
        if let Some(record) = self.record(index, key, source) {
            record.estimate = Some((width, height));
        }
        Some(height)
    }

    pub(super) fn preview(
        &mut self,
        index: usize,
        key: &str,
        entry: &StreamEntry,
    ) -> Option<String> {
        let source = TextSource::for_entry(entry)?;
        if let Some(record) = self.records.get(key)
            && record.source == source
            && let Some(preview) = &record.preview
        {
            return Some(preview.clone());
        }
        let preview = message_anchors::text_preview(entry)?;
        if self.record(index, key, source).is_some() {
            self.retain_preview(key, &preview);
        }
        Some(preview)
    }

    fn retain_preview(&mut self, key: &str, preview: &str) {
        let Some((stored_key, record)) = self.records.get_key_value(key) else {
            return;
        };
        let retained = preview.to_string();
        let capacity = retained.capacity();
        if stored_key.capacity() + capacity > RECORD_PAYLOAD_LIMIT {
            self.retire(key);
            return;
        }
        let old_capacity = record.preview.as_ref().map_or(0, String::capacity);
        if self.retained_bytes - old_capacity + capacity > PAYLOAD_LIMIT {
            return;
        }
        if let Some(record) = self.records.get_mut(key) {
            record.preview = Some(retained);
            self.retained_bytes = self.retained_bytes - old_capacity + capacity;
        }
    }
}

fn reading_range(total: usize, before_reading: usize) -> Range<usize> {
    if total <= RECORD_LIMIT {
        return 0..total;
    }
    let start = before_reading
        .saturating_sub(READING_LIMIT / 2)
        .min(total.saturating_sub(READING_LIMIT));
    start..(start + READING_LIMIT).min(total)
}

fn record_payload(key_capacity: usize, record: &TextRecord) -> usize {
    key_capacity + record.preview.as_ref().map_or(0, String::capacity)
}

pub(super) fn raw_text_height(entry: &StreamEntry, width: f32) -> Option<f32> {
    match entry {
        StreamEntry::User { lines, copy } => {
            Some(message_anchors::estimate_user_height(copy, lines, width))
        }
        StreamEntry::Assistant { model, .. } => {
            Some(message_anchors::estimate_markdown_height(model, width))
        }
        _ => None,
    }
}

pub(super) fn compose_text_height(entry: &StreamEntry, raw: f32, owned: bool) -> f32 {
    match entry {
        StreamEntry::Assistant { failure, .. } => {
            raw + Layout::CONVERSATION_BODY_TOP
                + if failure.is_some() && !owned {
                    Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT
                } else {
                    0.
                }
        }
        _ => raw,
    }
}

impl ConversationStream {
    pub(crate) fn prepare_text_cache(&mut self) {
        self.ensure_entry_identities();
        self.text_cache.get_mut().prepare(
            &self.entries,
            &self.entry_identities,
            self.list.logical_scroll_top().item_ix,
        );
    }

    pub(crate) fn user_message_top(&self, index: usize) -> f32 {
        if matches!(self.entries.get(index), Some(StreamEntry::User { .. }))
            && index.checked_sub(1).is_some_and(|previous| {
                matches!(
                    self.entries.get(previous),
                    Some(StreamEntry::UserImages { .. })
                ) && self
                    .entry_identities
                    .get(previous)
                    .zip(self.entry_identities.get(index))
                    .is_some_and(|(images, text)| {
                        images.message_id == text.message_id && images.pending_id == text.pending_id
                    })
            })
        {
            Layout::USER_ATTACHMENT_TEXT_GAP
        } else {
            Layout::USER_MESSAGE_PADDING_TOP
        }
    }

    pub(crate) fn estimated_entry_height(&self, index: usize, cx: &App, owned: bool) -> f32 {
        let entry = &self.entries[index];
        let user_top_adjustment = match entry {
            StreamEntry::User { copy, lines } if user_message_is_visible(copy, lines) => {
                self.user_message_top(index) - Layout::USER_MESSAGE_PADDING_TOP
            }
            _ => 0.,
        };
        if let Some(identity) = self.entry_identities.get(index)
            && let Some(raw) = self.text_cache.borrow_mut().height(
                index,
                &identity.key,
                entry,
                self.workspace_width,
            )
        {
            return compose_text_height(entry, raw, owned)
                + user_top_adjustment
                + self.timestamp_extra_height(index);
        }
        if owned && matches!(entry, StreamEntry::Summary { .. }) {
            return 0.;
        }
        message_anchors::estimate_entry_height(entry, cx, self.workspace_width)
            + user_top_adjustment
            + self.timestamp_extra_height(index)
    }
}
