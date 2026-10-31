use super::timestamp_format::{TimestampDisplay, format_timestamp};
use super::*;
use gpui_kit::Role;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct TimestampState {
    pub(super) times: HashMap<String, Option<i64>>,
    pub(super) dirty: bool,
    pub(super) accepted_user: Option<(String, u64)>,
    pub(super) break_before: HashSet<String>,
    pub(super) boundary_after: HashSet<String>,
    pub(super) boundary_before_entries: HashSet<String>,
    pub(super) running_assistants: HashSet<String>,
    prefixes: HashMap<String, i64>,
    messages: HashMap<String, i64>,
    hidden_messages: HashSet<String>,
    focuses: HashMap<String, FocusHandle>,
    subscriptions: HashMap<String, Vec<gpui_kit::Subscription>>,
    pub(super) now_ms: i64,
    next_change_ms: Option<i64>,
}

#[derive(Default)]
struct TimestampAdjacency {
    previous: Option<(bool, Option<i64>)>,
    saw_user: bool,
}

impl TimestampAdjacency {
    fn reset(&mut self) {
        self.previous = None;
        self.saw_user = true;
    }

    fn push(&mut self, user: bool, sent_at_ms: Option<i64>, now_ms: i64) -> bool {
        let separate = if user && !self.saw_user {
            sent_at_ms.is_some_and(|sent| now_ms.saturating_sub(sent) > 3_600_000)
        } else {
            self.previous.is_some_and(|(assistant, previous)| {
                assistant
                    && previous.zip(sent_at_ms).is_some_and(|(before, after)| {
                        after
                            .checked_sub(before)
                            .is_some_and(|gap| gap > if user { 3_600_000 } else { 600_000 })
                    })
            })
        };
        self.saw_user |= user || sent_at_ms.is_none();
        self.previous = Some((!user, sent_at_ms));
        separate
    }
}

impl TimestampState {
    pub(super) fn start_assistant(&mut self, message_id: &str) {
        if self.running_assistants.insert(message_id.to_owned()) {
            self.dirty = true;
        }
    }

    pub(super) fn finish_run(&mut self) {
        if !self.running_assistants.is_empty() {
            self.running_assistants.clear();
            self.dirty = true;
        }
    }

    fn message_time(&self, owner: &str, time: Option<i64>) -> Option<i64> {
        time.filter(|_| !self.running_assistants.contains(owner))
    }

    pub(super) fn has_message_time(
        &self,
        index: usize,
        identities: &[StreamEntryIdentity],
    ) -> bool {
        identities.get(index).is_some_and(|identity| {
            self.messages.contains_key(&identity.key)
                || self.hidden_messages.contains(&identity.key)
        })
    }
}

impl ConversationStream {
    pub(super) fn refresh_timestamps(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let previous_now = self.timestamps.now_ms;
        self.timestamps.now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_millis()).ok())
            .unwrap_or_default();
        if self.timestamps.now_ms < previous_now {
            self.timestamps.dirty = true;
        }
        if self
            .timestamps
            .next_change_ms
            .is_some_and(|at| self.timestamps.now_ms >= at)
        {
            self.timestamps.dirty = true;
        }
        if !std::mem::take(&mut self.timestamps.dirty) {
            return;
        }
        let mut adjacency = TimestampAdjacency::default();
        let mut seen = HashSet::new();
        let mut segment_owner = None;
        let mut next_change_ms = None;
        let mut prefixes = HashMap::new();
        let mut messages = HashMap::new();
        let mut hidden_messages = HashSet::new();
        let mut first_visible = HashMap::new();

        let mut last_body = HashMap::new();
        let mut valid_times = HashMap::new();
        let mut user_owners = HashSet::new();
        let continuation_starts = self
            .run_continuations
            .values()
            .flat_map(|relation| [&relation.user_message_id, &relation.next_assistant_id])
            .collect::<HashSet<_>>();
        for (index, entry) in self.entries.iter().enumerate() {
            let Some(identity) = self.entry_identities.get(index) else {
                continue;
            };
            let Some(owner) = identity
                .message_id
                .as_ref()
                .or(identity.pending_id.as_ref())
            else {
                continue;
            };
            if matches!(
                entry,
                StreamEntry::User { .. } | StreamEntry::UserImages { .. }
            ) {
                user_owners.insert(owner.clone());
            }
            if matches!(
                entry,
                StreamEntry::Subagents { .. }
                    | StreamEntry::ContextCompaction { .. }
                    | StreamEntry::Summary { .. }
                    | StreamEntry::SkillActivation { .. }
            ) {
                continue;
            }
            valid_times.entry(owner.clone()).or_insert_with(|| {
                self.timestamps
                    .times
                    .get(owner)
                    .copied()
                    .flatten()
                    .filter(|time| format_timestamp(*time, self.timestamps.now_ms).is_some())
            });
            let visible = entry.row_count(cx) > 0
                && !self.process_entry_hidden(index, cx)
                && !matches!(entry, StreamEntry::User { copy, .. } if !copy.has_text());
            if !visible {
                continue;
            }
            first_visible
                .entry(owner.clone())
                .or_insert_with(|| identity.key.clone());
            if matches!(
                entry,
                StreamEntry::User { .. }
                    | StreamEntry::UserImages { .. }
                    | StreamEntry::Assistant { .. }
                    | StreamEntry::Plan { .. }
            ) || (matches!(entry, StreamEntry::ArtifactGroup { .. })
                && self.copy_source_for_entry(index).is_some())
            {
                let target = last_body
                    .entry(owner.clone())
                    .or_insert_with(|| identity.key.clone());
                if !matches!(entry, StreamEntry::UserImages { .. }) {
                    *target = identity.key.clone();
                }
            }
        }
        for (owner, key) in last_body {
            let time = valid_times.get(&owner).copied().flatten();
            if let Some(time) = self.timestamps.message_time(&owner, time) {
                messages.insert(key, time);
            } else if time.is_some() && self.timestamps.running_assistants.contains(&owner) {
                hidden_messages.insert(key);
            }
        }
        for (index, entry) in self.entries.iter().enumerate() {
            let Some(identity) = self.entry_identities.get(index) else {
                continue;
            };
            let owner = identity
                .message_id
                .as_ref()
                .or(identity.pending_id.as_ref());
            let boundary = matches!(
                entry,
                StreamEntry::Subagents { .. }
                    | StreamEntry::ContextCompaction { .. }
                    | StreamEntry::Summary { .. }
                    | StreamEntry::SkillActivation { .. }
            );
            if boundary || owner.is_none() {
                segment_owner = None;
                adjacency.reset();
                continue;
            }
            let owner = owner.unwrap_or(&identity.key);
            if self
                .timestamps
                .boundary_before_entries
                .contains(&identity.key)
            {
                adjacency.reset();
                segment_owner = None;
            }
            let user = user_owners.contains(owner);
            let time = valid_times.get(owner).copied().flatten();
            if !seen.insert(owner.clone()) {
                if segment_owner.as_ref() != Some(owner) {
                    adjacency.reset();
                }
                continue;
            }
            segment_owner = Some(owner.clone());
            if continuation_starts.contains(owner) || self.timestamps.break_before.contains(owner) {
                adjacency.reset();
            }
            if user && !adjacency.saw_user {
                next_change_ms = time
                    .and_then(|time| time.checked_add(3_600_001))
                    .filter(|at| *at > self.timestamps.now_ms);
            }
            let separate = adjacency.push(user, time, self.timestamps.now_ms);
            if separate
                && let Some(time) = time
                && let Some(key) = first_visible.get(owner)
            {
                prefixes.insert(key.clone(), time);
            }
            if self.timestamps.boundary_after.contains(owner) {
                adjacency.reset();
            }
        }
        let changed = self
            .entry_identities
            .iter()
            .enumerate()
            .filter_map(|(index, identity)| {
                (prefixes.get(&identity.key) != self.timestamps.prefixes.get(&identity.key)
                    || messages.get(&identity.key) != self.timestamps.messages.get(&identity.key))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        self.timestamps.next_change_ms = next_change_ms;
        self.timestamps.prefixes = prefixes;
        self.timestamps.messages = messages;
        self.timestamps.hidden_messages = hidden_messages;
        for index in changed {
            self.invalidate_item(Some(index));
        }
        self.timestamps
            .focuses
            .retain(|key, _| self.timestamps.messages.contains_key(key));
        self.timestamps
            .subscriptions
            .retain(|key, _| self.timestamps.messages.contains_key(key));
        let keys = self.timestamps.messages.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            let focus = self
                .timestamps
                .focuses
                .entry(key.clone())
                .or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            self.timestamps.subscriptions.entry(key).or_insert_with(|| {
                vec![
                    cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
                    cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
                ]
            });
        }
    }

    pub(super) fn entry_timestamp(&self, index: usize) -> Option<TimestampDisplay> {
        let key = &self.entry_identities.get(index)?.key;
        format_timestamp(*self.timestamps.messages.get(key)?, self.timestamps.now_ms)
    }

    pub(super) fn timestamp_focus(&self, index: usize) -> Option<&FocusHandle> {
        self.timestamps
            .focuses
            .get(&self.entry_identities.get(index)?.key)
    }

    pub(super) fn timestamp_extra_height(&self, index: usize) -> f32 {
        let Some(identity) = self.entry_identities.get(index) else {
            return 0.;
        };
        let prefix = if self.timestamps.prefixes.contains_key(&identity.key) {
            Layout::CONVERSATION_STAGE_TOP * 2. + Typography::BODY * Typography::BODY_LINE_HEIGHT
        } else {
            0.
        };
        let footer = if self.timestamps.messages.contains_key(&identity.key)
            && self.copy_source_for_entry(index).is_none()
            && !matches!(self.entries.get(index), Some(StreamEntry::User { .. }))
        {
            Layout::MESSAGE_COPY_FOOTER
        } else {
            0.
        };
        prefix + footer
    }

    pub(super) fn timestamp_prefix(&self, index: usize, colors: ThemeColors) -> Option<AnyElement> {
        let key = &self.entry_identities.get(index)?.key;
        let display =
            format_timestamp(*self.timestamps.prefixes.get(key)?, self.timestamps.now_ms)?;
        let suffix = display
            .separator
            .strip_prefix(&display.day)
            .unwrap_or(&display.time)
            .to_owned();
        Some(
            div()
                .id(("timestamp-separator", index))
                .role(Role::Label)
                .aria_label(display.accessible)
                .w_full()
                .flex()
                .justify_center()
                .items_center()
                .py(px(Layout::CONVERSATION_STAGE_TOP))
                .text_size(px(Typography::BODY))
                .text_color(colors.text_tertiary)
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(display.day.clone()),
                )
                .child(suffix)
                .into_any_element(),
        )
    }
}
