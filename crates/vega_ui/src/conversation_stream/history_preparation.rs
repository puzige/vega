use super::*;
use generated_notice::{GeneratedNoticeFragment, GeneratedNoticeSpan};

pub(super) struct PreparedAssistantPart {
    pub(super) key: String,
    pub(super) starts_timestamp_boundary: bool,
    pub(super) body: model::PreparedMessage,
}

pub(super) struct PreparedHistoryEntries {
    pub(super) entries: Vec<HistoryEntry>,
    pub(super) assistants: HashMap<usize, Vec<PreparedAssistantPart>>,
    pub(super) generated_notices: HashMap<(String, i64), Vec<GeneratedNoticeSpan>>,
}

impl PreparedHistoryEntries {
    pub(super) fn new(entries: Vec<HistoryEntry>, counters: &StreamCounters) -> Self {
        let mut response_boundaries = phase::history_response_boundaries(&entries);
        let mut timestamp_boundaries: HashMap<String, HashSet<usize>> = HashMap::new();
        for entry in &entries {
            if let HistoryEntry::TimestampBoundary {
                message_id,
                text_start_bytes,
            } = entry
            {
                timestamp_boundaries
                    .entry(message_id.clone())
                    .or_default()
                    .insert(*text_start_bytes);
                response_boundaries
                    .entry(message_id.clone())
                    .or_default()
                    .push(*text_start_bytes);
            }
        }
        let generated_notices = generated_notice::history_notices(&entries);
        let mut assistant_segments = HashMap::<String, usize>::new();
        let mut assistant_offsets = HashMap::<String, usize>::new();
        let mut assistants = HashMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let HistoryEntry::AssistantText {
                seq,
                message_id,
                content,
                status,
                execution_duration_ms,
            } = entry
            else {
                continue;
            };
            if *status == vega_conversation::history::AssistantStatus::Done
                && content.is_empty()
                && execution_duration_ms.is_some()
            {
                continue;
            }
            let segment = assistant_segments.entry(message_id.clone()).or_default();
            let segment_ordinal = *segment;
            *segment += 1;
            let offset = assistant_offsets.entry(message_id.clone()).or_default();
            let start = *offset;
            *offset += content.len();
            let mut boundaries = response_boundaries
                .get(message_id)
                .into_iter()
                .flatten()
                .copied()
                .filter(|boundary| {
                    *boundary > start
                        && *boundary < *offset
                        && content.is_char_boundary(*boundary - start)
                })
                .map(|boundary| boundary - start)
                .collect::<Vec<_>>();
            boundaries.push(content.len());
            boundaries.sort_unstable();
            boundaries.dedup();
            let mut part_start = 0;
            let mut parts = Vec::new();
            for part_end in boundaries {
                let notices: Vec<GeneratedNoticeFragment> = generated_notices
                    .get(&(message_id.clone(), *seq))
                    .into_iter()
                    .flatten()
                    .filter_map(|span| span.fragments(start + part_start..start + part_end))
                    .collect();
                let key = if part_start == 0 {
                    format!("assistant-text-{segment_ordinal}")
                } else {
                    format!(
                        "assistant-text-{segment_ordinal}-response-{}",
                        start + part_start
                    )
                };
                parts.push(PreparedAssistantPart {
                    key,
                    starts_timestamp_boundary: timestamp_boundaries
                        .get(message_id)
                        .is_some_and(|offsets| offsets.contains(&(start + part_start))),
                    body: model::PreparedMessage::new(
                        &content[part_start..part_end],
                        notices,
                        counters,
                    ),
                });
                part_start = part_end;
            }
            assistants.insert(index, parts);
        }
        Self {
            entries,
            assistants,
            generated_notices,
        }
    }
}

impl ConversationStream {
    pub fn prepare_conversation(
        initial: vega_conversation::types::ConversationLoad,
    ) -> (
        Option<String>,
        impl FnOnce(&mut Self, &mut Context<Self>) + Send + 'static,
    ) {
        let vega_conversation::types::ConversationLoad {
            mut history,
            plans,
            composer_history,
            approved_not_started,
            usage,
            summary,
            pending_model,
            reasoning_preferences,
        } = initial;
        let counters = StreamCounters::default();
        let prepared = PreparedHistoryEntries::new(std::mem::take(&mut history.entries), &counters);
        (pending_model, move |stream, cx| {
            let started = Instant::now();
            stream.thread.reasoning_preferences = Some(reasoning_preferences);
            stream.counters.committed_materializations.fetch_add(
                counters.committed_materializations.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            stream.counters.frozen_rematerializations.fetch_add(
                counters.frozen_rematerializations.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            stream.counters.pending_materializations.fetch_add(
                counters.pending_materializations.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            stream.apply_prepared_history_page(history, prepared, cx);
            for plan in plans {
                stream.apply_plan(plan, cx);
            }
            let thread_id = stream.thread.id.clone();
            stream.apply_composer_history(&thread_id, composer_history, cx);
            if let Some(summary) = summary {
                stream.apply_task_summary(summary, cx);
            }
            if approved_not_started {
                stream.apply_approved_not_started(cx);
            }
            stream.restore_meter(usage, cx);
            tracing::debug!(
                elapsed_ms = started.elapsed().as_millis(),
                "conversation UI hydration"
            );
        })
    }
}
