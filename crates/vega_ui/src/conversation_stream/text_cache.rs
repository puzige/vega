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
    #[cfg(test)]
    height_scans: usize,
    #[cfg(test)]
    preview_builds: usize,
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
        #[cfg(test)]
        {
            self.height_scans += 1;
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
        #[cfg(test)]
        {
            self.preview_builds += 1;
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
        StreamEntry::User { lines, .. } => {
            Some(message_anchors::estimate_user_height(lines, width))
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

    pub(crate) fn estimated_entry_height(&self, index: usize, cx: &App, owned: bool) -> f32 {
        let entry = &self.entries[index];
        if let Some(identity) = self.entry_identities.get(index)
            && let Some(raw) = self.text_cache.borrow_mut().height(
                index,
                &identity.key,
                entry,
                self.workspace_width,
            )
        {
            return compose_text_height(entry, raw, owned);
        }
        if owned && matches!(entry, StreamEntry::Summary { .. }) {
            return 0.;
        }
        message_anchors::estimate_entry_height(entry, cx, self.workspace_width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(key: String) -> StreamEntryIdentity {
        StreamEntryIdentity {
            key,
            message_id: Some("message".into()),
            pending_id: None,
            echo_kind: None,
            sequence: None,
        }
    }

    fn user(text: &str) -> StreamEntry {
        StreamEntry::User {
            lines: user_message_lines(USER_BLOCK_BASE, text),
            copy: MessageCopy::new(text),
        }
    }

    fn assistant(text: &str, finish: bool) -> StreamEntry {
        let mut stream = MarkdownStream::new();
        stream.append(text);
        if finish {
            stream.finish();
        }
        let mut model = StreamModel::default();
        model.sync(&stream.snapshot(), &StreamCounters::default());
        StreamEntry::Assistant {
            copy: MessageCopy::new(text),
            stream: Box::new(stream),
            model,
            failure: None,
        }
    }

    fn warm(cache: &mut TextCache, entries: &[StreamEntry], ids: &[StreamEntryIdentity]) {
        for (index, (entry, id)) in entries.iter().zip(ids).enumerate() {
            assert!(cache.height(index, &id.key, entry, None).is_some());
            assert!(cache.preview(index, &id.key, entry).is_some());
        }
    }

    #[test]
    fn warm_text_reuses_estimates_and_sanitized_previews_across_width_changes() {
        let entries = [
            user(&"word ".repeat(400)),
            assistant("answer Bearer syntheticcredential", true),
        ];
        let ids = [identity("user".into()), identity("assistant".into())];
        let mut cache = TextCache::default();
        cache.prepare(&entries, &ids, 0);
        warm(&mut cache, &entries, &ids);
        let cold_height = cache.height(0, "user", &entries[0], None).unwrap();
        assert_eq!(
            cache.preview(1, "assistant", &entries[1]).as_deref(),
            Some("answer [凭据已隐藏]")
        );
        warm(&mut cache, &entries, &ids);
        assert_eq!((cache.height_scans, cache.preview_builds), (2, 2));
        let confirmed = Layout::CONVERSATION_MAX_WIDTH + 2. * Layout::CONVERSATION_PADDING;
        assert_eq!(
            cache.height(0, "user", &entries[0], Some(confirmed)),
            Some(cold_height)
        );
        assert_eq!(cache.height_scans, 3);
        assert!(cache.height(0, "user", &entries[0], Some(240.)).unwrap() > cold_height);
        assert_eq!(cache.height_scans, 4);
        assert_eq!(
            cache.preview(0, "user", &entries[0]),
            message_anchors::text_preview(&entries[0])
        );
        assert_eq!(cache.preview_builds, 2);
        assert_eq!(
            cache.height(0, "user", &entries[0], None),
            Some(cold_height)
        );
        assert_eq!(cache.height_scans, 5);
        assert_eq!(cache.records.len(), 2);
    }

    #[test]
    fn revisions_and_incarnations_replace_the_current_source_without_stale_hits() {
        let mut entries = [assistant("alpha", true)];
        let ids = [identity("same-key".into())];
        let mut cache = TextCache::default();
        cache.prepare(&entries, &ids, 0);
        warm(&mut cache, &entries, &ids);
        if let StreamEntry::Assistant { model, .. } = &mut entries[0] {
            let nodes = [RenderNode::Paragraph {
                spans: vec![Inline::Text("bravo".into())],
            }];
            let old = model.content_revision();
            let snapshot = StreamSnapshot {
                blocks: vec![BlockView {
                    block_id: model.committed_ids[0],
                    version: model.committed_versions[0] + 1,
                    nodes: &nodes,
                }],
                pending: None,
            };
            assert!(model.sync(&snapshot, &StreamCounters::default()));
            assert!(model.content_revision() > old);
            assert!(!model.sync(&snapshot, &StreamCounters::default()));
            assert_eq!(model.content_revision(), old + 1);
        }
        assert_eq!(
            cache.preview(0, "same-key", &entries[0]).as_deref(),
            Some("bravo")
        );
        assert!(cache.height(0, "same-key", &entries[0], None).is_some());
        assert_eq!(cache.height_scans, 2);
        entries[0] = assistant("fresh", true);
        cache.prepare(&entries, &ids, 0);
        assert_eq!(
            cache.preview(0, "same-key", &entries[0]).as_deref(),
            Some("fresh")
        );
        assert_eq!(cache.records.len(), 1);
        assert_eq!(cache.preview_builds, 3);
        entries[0] = user("human");
        cache.prepare(&entries, &ids, 0);
        assert_eq!(
            cache.preview(0, "same-key", &entries[0]).as_deref(),
            Some("human")
        );
        if let StreamEntry::User { copy, lines } = &mut entries[0] {
            copy.replace_text("other");
            *lines = user_message_lines(USER_BLOCK_BASE, "other");
        }
        assert_eq!(
            cache.preview(0, "same-key", &entries[0]).as_deref(),
            Some("other")
        );
    }

    #[test]
    fn append_and_finish_invalidate_text_even_without_copy_generation_changes() {
        let mut entries = [assistant("pending", false)];
        let ids = [identity("stream".into())];
        let mut cache = TextCache::default();
        cache.prepare(&entries, &ids, 0);
        warm(&mut cache, &entries, &ids);
        let initial = TextSource::for_entry(&entries[0]).unwrap();
        if let StreamEntry::Assistant { stream, model, .. } = &mut entries[0] {
            stream.append(" tail");
            assert!(model.sync(&stream.snapshot(), &StreamCounters::default()));
        }
        assert_eq!(
            cache.preview(0, "stream", &entries[0]).as_deref(),
            Some("pending tail")
        );
        let appended = TextSource::for_entry(&entries[0]).unwrap();
        assert_eq!(initial.owner, appended.owner);
        assert!(appended.revision > initial.revision);
        if let StreamEntry::Assistant { stream, model, .. } = &mut entries[0] {
            let revision = model.content_revision();
            stream.finish();
            assert!(model.sync(&stream.snapshot(), &StreamCounters::default()));
            assert!(model.content_revision() > revision);
            stream.finish();
            assert!(!model.sync(&stream.snapshot(), &StreamCounters::default()));
        }
        assert_eq!(
            cache.preview(0, "stream", &entries[0]).as_deref(),
            Some("pending tail")
        );
        assert_eq!(cache.preview_builds, 3);
    }

    #[test]
    fn changed_text_does_not_rescan_unchanged_resident_history() {
        let mut entries = [user("history"), assistant("stream", false)];
        let ids = [identity("history".into()), identity("active".into())];
        let mut cache = TextCache::default();
        cache.prepare(&entries, &ids, 0);
        warm(&mut cache, &entries, &ids);
        if let StreamEntry::Assistant { stream, model, .. } = &mut entries[1] {
            stream.append(" grows");
            assert!(model.sync(&stream.snapshot(), &StreamCounters::default()));
        }
        cache.prepare(&entries, &ids, 0);
        warm(&mut cache, &entries, &ids);
        assert_eq!((cache.height_scans, cache.preview_builds), (3, 3));
        assert_eq!(
            cache.preview(0, "history", &entries[0]).as_deref(),
            Some("history")
        );
        assert_eq!(
            cache.preview(1, "active", &entries[1]).as_deref(),
            Some("stream grows")
        );
    }

    #[test]
    fn precise_widths_on_opposite_sides_of_a_wrap_boundary_do_not_share_estimates() {
        let entries = [user("abcdefghijklm")];
        let ids = [identity("wrap".into())];
        let mut cache = TextCache::default();
        cache.prepare(&entries, &ids, 0);
        let boundary = (13. * Typography::CONVERSATION * 0.56
            + 2. * Layout::CONVERSATION_USER_PADDING)
            / Layout::CONVERSATION_USER_RATIO
            + 2. * Layout::CONVERSATION_PADDING;
        let narrow = cache
            .height(0, "wrap", &entries[0], Some(boundary - 0.01))
            .unwrap();
        let wide = cache
            .height(0, "wrap", &entries[0], Some(boundary + 0.01))
            .unwrap();
        assert_eq!(
            narrow - wide,
            Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT
        );
        assert_eq!(cache.height_scans, 2);
    }

    #[test]
    fn model_revision_covers_pending_replacement_removal_append_and_reset() {
        let counters = StreamCounters::default();
        let mut model = StreamModel::default();
        let first = [RenderNode::Paragraph {
            spans: vec![Inline::Text("first".into())],
        }];
        let other = [RenderNode::Paragraph {
            spans: vec![Inline::Text("other".into())],
        }];
        fn pending(version: u64, nodes: &[RenderNode]) -> StreamSnapshot<'_> {
            StreamSnapshot {
                blocks: vec![],
                pending: Some(vega_markdown::PendingView {
                    block_id: 1,
                    version,
                    nodes,
                }),
            }
        }
        assert!(!model.sync(
            &StreamSnapshot {
                blocks: vec![],
                pending: None
            },
            &counters
        ));
        assert_eq!(model.content_revision(), 0);
        assert!(model.sync(&pending(1, &first), &counters));
        assert_eq!(model.content_revision(), 1);
        assert!(model.sync(&pending(2, &other), &counters));
        assert_eq!(model.content_revision(), 2);
        assert!(model.sync(
            &StreamSnapshot {
                blocks: vec![],
                pending: None
            },
            &counters
        ));
        assert_eq!(model.content_revision(), 3);
        for (block_id, revision) in [(1, 4), (9, 5)] {
            assert!(model.sync(
                &StreamSnapshot {
                    blocks: vec![BlockView {
                        block_id,
                        version: 1,
                        nodes: &first
                    }],
                    pending: None
                },
                &counters
            ));
            assert_eq!(model.content_revision(), revision);
        }
        assert!(model.sync(
            &StreamSnapshot {
                blocks: vec![
                    BlockView {
                        block_id: 9,
                        version: 1,
                        nodes: &first
                    },
                    BlockView {
                        block_id: 10,
                        version: 1,
                        nodes: &other
                    }
                ],
                pending: None
            },
            &counters
        ));
        assert_eq!(model.content_revision(), 6);
        assert_eq!(
            message_anchors::text_preview(&StreamEntry::Assistant {
                copy: MessageCopy::default(),
                stream: Box::default(),
                model,
                failure: None
            })
            .as_deref(),
            Some("first other")
        );
    }

    #[test]
    fn whole_history_sweeps_preserve_reading_and_tail_admission() {
        let entries = (0..3000).map(|_| user("text")).collect::<Vec<_>>();
        let ids = (0..3000)
            .map(|index| identity(format!("entry-{index}")))
            .collect::<Vec<_>>();
        let mut cache = TextCache::default();
        cache.prepare(&entries, &ids, 1500);
        warm(&mut cache, &entries, &ids);
        assert_eq!(cache.records.len(), 2048);
        assert!(cache.records.contains_key("entry-1500"));
        assert!(cache.records.contains_key("entry-2999"));
        assert!(!cache.records.contains_key("entry-0"));
        cache.prepare(&entries, &ids, 1500);
        warm(&mut cache, &entries, &ids);
        assert_eq!(cache.height_scans, 3000 + 952);
        assert_eq!(cache.preview_builds, 3000 + 952);
        cache.prepare(&entries[..2000], &ids[..2000], 1999);
        warm(&mut cache, &entries[..2000], &ids[..2000]);
        assert_eq!(cache.records.len(), 2000);
        assert!(cache.records.contains_key("entry-0"));
    }

    #[test]
    fn byte_budgets_and_oversized_keys_return_correct_uncached_results() {
        let entries = (0..2048)
            .map(|_| user(&"界".repeat(120)))
            .collect::<Vec<_>>();
        let ids = (0..2048)
            .map(|index| identity(format!("{index}-{}", "k".repeat(3000))))
            .collect::<Vec<_>>();
        let mut cache = TextCache::default();
        cache.prepare(&entries, &ids, 0);
        warm(&mut cache, &entries, &ids);
        assert!(cache.records.len() < 2048);
        assert!(cache.retained_bytes <= 2 * 1024 * 1024);
        assert_eq!(
            cache.retained_bytes,
            cache
                .records
                .iter()
                .map(|(key, record)| record_payload(key.capacity(), record))
                .sum::<usize>()
        );
        assert!(
            cache
                .records
                .iter()
                .all(|(key, record)| record_payload(key.capacity(), record) <= 4096)
        );
        let oversized = "x".repeat(4097);
        let expected = message_anchors::text_preview(&entries[0]);
        assert_eq!(cache.preview(0, &oversized, &entries[0]), expected);
        assert!(!cache.records.contains_key(&oversized));
        cache.clear();
        cache.prepare(&entries, &ids, 0);
        let near_limit = "x".repeat(4000);
        assert_eq!(cache.preview(0, &near_limit, &entries[0]), expected);
        assert!(!cache.records.contains_key(&near_limit));
        cache.clear();
        assert_eq!(cache.retained_bytes, 0);
        assert!(cache.records.is_empty());
    }

    #[test]
    fn removal_and_order_changes_retain_only_current_stable_identities() {
        let mut entries = vec![user("first"), user("second"), user("third")];
        let mut ids = vec![
            identity("one".into()),
            identity("two".into()),
            identity("three".into()),
        ];
        let mut cache = TextCache::default();
        cache.prepare(&entries, &ids, 0);
        warm(&mut cache, &entries, &ids);
        entries.swap(0, 2);
        ids.swap(0, 2);
        cache.prepare(&entries, &ids, 0);
        warm(&mut cache, &entries, &ids);
        assert_eq!((cache.height_scans, cache.preview_builds), (3, 3));
        entries.remove(1);
        ids.remove(1);
        cache.prepare(&entries, &ids, 0);
        assert!(!cache.records.contains_key("two"));
        cache.retire("one");
        assert!(!cache.records.contains_key("one"));
        assert_eq!(
            cache.preview(0, "three", &entries[0]).as_deref(),
            Some("third")
        );
        cache.prepare(&[], &[], 0);
        assert_eq!(cache.retained_bytes, 0);
        assert!(cache.records.is_empty());
    }

    #[test]
    fn failure_and_ownership_are_composed_live_without_rescanning_text() {
        let mut entries = [assistant("answer", true)];
        let ids = [identity("answer".into())];
        let mut cache = TextCache::default();
        cache.prepare(&entries, &ids, 0);
        let raw = cache.height(0, "answer", &entries[0], None).unwrap();
        let normal = compose_text_height(&entries[0], raw, false);
        if let StreamEntry::Assistant { failure, .. } = &mut entries[0] {
            *failure = Some(RunFailureKind::Persisted);
        }
        assert_eq!(cache.height(0, "answer", &entries[0], None), Some(raw));
        assert_eq!(compose_text_height(&entries[0], raw, true), normal);
        assert_eq!(
            compose_text_height(&entries[0], raw, false),
            normal + Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT
        );
        assert_eq!(cache.height_scans, 1);
    }

    #[test]
    fn markdown_tables_and_code_have_identical_cold_and_warm_estimates() {
        let entries = [assistant(
            "| A | B |\n|---|---|\n| long cell | 界界界 |\n\n```rust\nlet value = 42;\n```",
            true,
        )];
        let ids = [identity("markdown".into())];
        let mut cache = TextCache::default();
        cache.prepare(&entries, &ids, 0);
        let width = Some(360.);
        let expected = raw_text_height(&entries[0], TextWidth::new(width).effective());
        assert!(expected.unwrap() > 0.);
        assert_eq!(cache.height(0, "markdown", &entries[0], width), expected);
        assert_eq!(cache.height(0, "markdown", &entries[0], width), expected);
        assert_eq!(cache.height_scans, 1);
    }
}
