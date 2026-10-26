use super::*;

fn run_activity_status(status: vega_conversation::history::AssistantStatus) -> RunActivityStatus {
    match status {
        vega_conversation::history::AssistantStatus::Done => RunActivityStatus::Completed,
        vega_conversation::history::AssistantStatus::Failed => RunActivityStatus::Failed,
        vega_conversation::history::AssistantStatus::Interrupted => RunActivityStatus::Interrupted,
    }
}

impl ConversationStream {
    /// Shows a restart-safe, non-executing projection for a durable approved
    /// instruction. Merely opening a thread must never read Keychain or start
    /// a provider request.
    pub fn apply_approved_not_started(&mut self, cx: &mut Context<Self>) {
        self.approved_not_started = true;
        self.controller_runtime_error = None;
        self.controller_error = Some(crate::localized_copy::LocalizedCopy::new(
            "ui.the_approved_plan_has_not_run_yet_a",
        ));
        cx.notify();
    }

    /// Adds or refreshes a validated durable Plan without direct SQLite UI access.
    pub fn apply_plan(&mut self, plan: Plan, cx: &mut Context<Self>) {
        if plan.thread_id != self.thread.id {
            return;
        }
        if let Some(card) = self.plan_cards.get(&plan.id) {
            card.update(cx, |card, cx| card.apply_persisted(plan, cx));
            return;
        }
        let id = plan.id.clone();
        let notices = self.generated_notices.get(&id).cloned().unwrap_or_default();
        let card = cx.new(|cx| {
            let mut card = PlanCard::new(plan, cx);
            card.set_generated_notices(notices, cx);
            card
        });
        cx.subscribe(&card, |_, _, event: &PlanReviewRequested, cx| {
            cx.emit(event.clone());
        })
        .detach();
        cx.observe(&card, |this, card, cx| {
            let index = this.entry_index_where(
                |entry| matches!(entry, StreamEntry::Plan { card: owned } if owned == &card),
            );
            this.invalidate_item(index);
            cx.notify();
        })
        .detach();
        let replace_index = self
            .last_finished_agent_message
            .as_ref()
            .filter(|(message_id, _)| message_id == &id)
            .map(|(_, entry_index)| *entry_index);
        let sequence = replace_index
            .and_then(|entry_index| self.entry_identities.get(entry_index))
            .and_then(|identity| identity.sequence);
        if let Some(entry_index) = replace_index {
            self.last_finished_agent_message = None;
            // Replaces an existing entry in place: same index, content may
            // change height → explicit invalidation (C4 whitelist).
            if let Some(entry) = self.entries.get_mut(entry_index) {
                *entry = StreamEntry::Plan { card: card.clone() };
                self.invalidate_item(Some(entry_index));
                let identity = self.durable_entry_identity(&id, "plan", sequence);
                self.set_entry_identity(entry_index, identity);
            } else {
                let index = self.entries.len();
                self.entries.push(StreamEntry::Plan { card: card.clone() });
                self.list_append(index);
                let identity = self.durable_entry_identity(&id, "plan", None);
                self.set_entry_identity(index, identity);
            }
        } else {
            let index = self.entries.len();
            self.entries.push(StreamEntry::Plan { card: card.clone() });
            self.list_append(index);
            let identity = self.durable_entry_identity(&id, "plan", None);
            self.set_entry_identity(index, identity);
        }
        self.plan_cards.insert(id, card);
        cx.notify();
    }

    pub fn apply_task_summary(&mut self, summary: TaskCostSummary, cx: &mut Context<Self>) {
        if self.summary_cards.contains_key(&summary.message_id) {
            return;
        }
        let message_id = summary.message_id.clone();
        let card = cx.new(|_| SummaryCard::new(summary));
        let index = self.entries.len();
        self.entries
            .push(StreamEntry::Summary { card: card.clone() });
        self.list_append(index);
        let identity = self.durable_entry_identity(&message_id, "summary", None);
        self.set_entry_identity(index, identity);
        self.summary_cards.insert(message_id, card);
        cx.notify();
    }

    pub fn apply_history_page(&mut self, page: HistoryPage, cx: &mut Context<Self>) {
        let subagents = page.subagents.clone();
        let was_empty = self.entries.is_empty();
        self.ensure_entry_identities();
        let anchor = self.scroll_anchor_snapshot();
        let (mut hydrated, mut hydrated_identities) =
            self.hydrate_history_entries(page.entries, cx);
        let prepended_entries = hydrated.len();
        hydrated.append(&mut self.entries);
        self.entries = hydrated;
        hydrated_identities.append(&mut self.entry_identities);
        self.entry_identities = hydrated_identities;
        self.list_prepend(prepended_entries);
        if prepended_entries > 0 {
            self.grow_entry_indices(0, prepended_entries);
            self.restore_scroll_anchor(&anchor);
        }
        self.hydration.older_cursor = page.older_cursor;
        if was_empty {
            self.hydration.newer_cursor = page.newer_cursor;
        }
        self.hydration.loading = false;
        self.hydration.paused = false;
        self.hydration.paused_direction = None;
        for snapshot in subagents {
            self.apply_subagent(snapshot, cx);
        }
        self.restore_subagent_request_entries();
        self.order_ordinary_history_entries();
        if !anchor.following_tail {
            self.restore_scroll_anchor(&anchor);
        }
        cx.notify();
    }

    pub fn apply_newer_history_page(&mut self, page: HistoryPage, cx: &mut Context<Self>) {
        let subagents = page.subagents.clone();
        self.ensure_entry_identities();
        let anchor = self.scroll_anchor_snapshot();
        let previous_len = self.entries.len();
        let (mut hydrated, mut hydrated_identities) =
            self.hydrate_history_entries(page.entries, cx);
        let appended_entries = hydrated.len();
        self.entries.append(&mut hydrated);
        self.entry_identities.append(&mut hydrated_identities);
        self.list_append(previous_len);
        self.hydration.newer_cursor = page.newer_cursor;
        self.hydration.loading = false;
        self.hydration.paused = false;
        self.hydration.paused_direction = None;
        if appended_entries > 0 && !anchor.following_tail {
            self.restore_scroll_anchor(&anchor);
        }
        for snapshot in subagents {
            self.apply_subagent(snapshot, cx);
        }
        self.restore_subagent_request_entries();
        self.order_ordinary_history_entries();
        if !anchor.following_tail {
            self.restore_scroll_anchor(&anchor);
        }
        cx.notify();
    }

    fn hydrate_history_entries(
        &mut self,
        history_entries: Vec<HistoryEntry>,
        cx: &mut Context<Self>,
    ) -> (Vec<StreamEntry>, Vec<StreamEntryIdentity>) {
        let mut hydrated: Vec<StreamEntry> = Vec::new();
        let mut hydrated_identities: Vec<StreamEntryIdentity> = Vec::new();
        let mut assistant_segments: HashMap<String, usize> = HashMap::new();
        for entry in &history_entries {
            if let HistoryEntry::RunContinuation {
                previous_assistant_id,
                user_message_id,
                next_assistant_id,
            } = entry
            {
                self.remember_run_continuation(
                    vega_conversation::types::RunContinuation {
                        previous_assistant_id: previous_assistant_id.clone(),
                        user_message_id: user_message_id.clone(),
                        next_assistant_id: next_assistant_id.clone(),
                    },
                    cx,
                );
            }
        }
        let continuation_runs = self
            .run_continuations
            .values()
            .flat_map(|relation| [&relation.previous_assistant_id, &relation.next_assistant_id])
            .collect::<HashSet<_>>();
        let response_boundaries = phase::history_response_boundaries(&history_entries);
        let generated_notices = generated_notice::history_notices(&history_entries);
        let mut assistant_offsets = HashMap::<String, usize>::new();
        let activity_runs: HashSet<_> = history_entries
            .iter()
            .filter_map(|entry| match entry {
                HistoryEntry::AssistantResponseMetadata { message_id, .. }
                | HistoryEntry::Tool { message_id, .. } => Some(message_id.clone()),
                _ => None,
            })
            .collect();
        let terminal_runs: HashMap<_, _> = history_entries
            .iter()
            .filter_map(|entry| match entry {
                HistoryEntry::AssistantText {
                    message_id,
                    status,
                    execution_duration_ms: duration,
                    ..
                } if duration.is_some()
                    || activity_runs.contains(message_id)
                    || continuation_runs.contains(message_id) =>
                {
                    Some((message_id.clone(), (*status, *duration)))
                }
                _ => None,
            })
            .collect();
        let mut restored_responses: HashMap<String, phase::RunResponses> = HashMap::new();
        for entry in history_entries {
            match entry {
                HistoryEntry::RunContinuation { .. } | HistoryEntry::GeneratedNotice { .. } => {}
                HistoryEntry::CredentialContinuation {
                    message_id,
                    failure,
                    ..
                } => {
                    self.remember_run_failure(&message_id, failure);
                }
                HistoryEntry::AssistantResponseMetadata {
                    message_id,
                    response,
                    ..
                } => {
                    restored_responses
                        .entry(message_id)
                        .or_default()
                        .push(response);
                }
                HistoryEntry::AssistantScreenshot {
                    seq,
                    message_id,
                    call_id,
                    image,
                } => {
                    hydrated.push(StreamEntry::Screenshot {
                        card: self.screenshot_card(image, cx),
                    });
                    hydrated_identities.push(self.durable_entry_identity(
                        &message_id,
                        &format!("screenshot-{call_id}"),
                        Some(seq),
                    ));
                }
                HistoryEntry::UserImages {
                    seq,
                    message_id,
                    images,
                } => {
                    let identity =
                        self.durable_entry_identity(&message_id, "user-images", Some(seq));
                    if let Some(index) = self
                        .entry_identities
                        .iter()
                        .position(|existing| existing.key == identity.key)
                    {
                        self.entry_identities[index].sequence = Some(seq);
                        let unchanged = matches!(&self.entries[index], StreamEntry::UserImages { images: current }
                            if current.iter().map(|image| &image.image).eq(images.iter()));
                        if !unchanged {
                            self.entries[index] = StreamEntry::UserImages {
                                images: self.history_image_previews(images, cx),
                            };
                            self.invalidate_item(Some(index));
                        }
                        continue;
                    }
                    hydrated.push(StreamEntry::UserImages {
                        images: self.history_image_previews(images, cx),
                    });
                    hydrated_identities.push(self.durable_entry_identity(
                        &message_id,
                        "user-images",
                        Some(seq),
                    ));
                }
                HistoryEntry::UserText {
                    seq,
                    message_id,
                    content,
                } => {
                    let identity = self.durable_entry_identity(&message_id, "user-text", Some(seq));
                    if let Some(index) = self
                        .entry_identities
                        .iter()
                        .position(|existing| existing.key == identity.key)
                    {
                        self.entry_identities[index].sequence = Some(seq);
                        if ordinary_submissions::update_user_history_text(
                            &mut self.entries[index],
                            &content,
                            &mut self.user_block_seq,
                        ) {
                            self.text_cache.get_mut().retire(&identity.key);
                            self.invalidate_item(Some(index));
                        }
                        continue;
                    }
                    let block_id = self.user_block_seq;
                    self.user_block_seq += 1;
                    hydrated.push(StreamEntry::User {
                        copy: MessageCopy::new(&content),
                        lines: user_message_lines(block_id, &content),
                    });
                    hydrated_identities.push(self.durable_entry_identity(
                        &message_id,
                        "user-text",
                        Some(seq),
                    ));
                }
                HistoryEntry::AssistantText {
                    seq,
                    message_id,
                    content,
                    status,
                    execution_duration_ms,
                } => {
                    if let Some((terminal_status, duration)) = terminal_runs.get(&message_id) {
                        self.ensure_hydrated_run_activity_group(
                            &message_id,
                            run_activity_status(*terminal_status),
                            *duration,
                            &mut hydrated,
                            &mut hydrated_identities,
                            cx,
                        );
                    }
                    if status == vega_conversation::history::AssistantStatus::Done
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
                        .get(&message_id)
                        .into_iter()
                        .flatten()
                        .copied()
                        .filter(|boundary| *boundary > start && *boundary < *offset)
                        .map(|boundary| boundary - start)
                        .collect::<Vec<_>>();
                    boundaries.push(content.len());
                    let mut part_start = 0;
                    for part_end in boundaries {
                        let part = &content[part_start..part_end];
                        let mut stream = MarkdownStream::new();
                        stream.append(part);
                        let copy = MessageCopy::new(part);
                        if let Some(notices) = generated_notices.get(&(message_id.clone(), seq)) {
                            copy.set_generated_notices(
                                notices
                                    .iter()
                                    .filter_map(|span| {
                                        span.fragments(start + part_start..start + part_end)
                                    })
                                    .collect(),
                            );
                        }
                        let mut model = StreamModel::default();
                        copy.finish_model(&mut stream, &mut model, &self.counters);
                        hydrated.push(StreamEntry::Assistant {
                            copy,
                            stream: Box::new(stream),
                            model,
                            failure: (status
                                == vega_conversation::history::AssistantStatus::Failed)
                                .then_some(RunFailureKind::Persisted),
                        });
                        let key = if part_start == 0 {
                            format!("assistant-text-{segment_ordinal}")
                        } else {
                            format!(
                                "assistant-text-{segment_ordinal}-response-{}",
                                start + part_start
                            )
                        };
                        hydrated_identities.push(self.durable_entry_identity(
                            &message_id,
                            &key,
                            Some(seq),
                        ));
                        part_start = part_end;
                    }
                }
                HistoryEntry::Plan {
                    seq,
                    plan,
                    execution_duration_ms,
                } => {
                    if plan.thread_id != self.thread.id || self.plan_cards.contains_key(&plan.id) {
                        continue;
                    }
                    let id = plan.id.clone();
                    if let Some(duration) = execution_duration_ms {
                        self.ensure_hydrated_run_activity_group(
                            &id,
                            RunActivityStatus::Completed,
                            Some(duration),
                            &mut hydrated,
                            &mut hydrated_identities,
                            cx,
                        );
                    }
                    let notices = generated_notices
                        .get(&(id.clone(), seq))
                        .cloned()
                        .unwrap_or_default();
                    let card = cx.new(|cx| {
                        let mut card = PlanCard::new(plan, cx);
                        card.set_generated_notices(notices, cx);
                        card
                    });
                    cx.subscribe(&card, |_, _, event: &PlanReviewRequested, cx| {
                        cx.emit(event.clone());
                    })
                    .detach();
                    cx.observe(&card, |this, card, cx| {
                        let index = this
                            .entry_index_where(|entry| matches!(entry, StreamEntry::Plan { card: owned } if owned == &card));
                        this.invalidate_item(index);
                        cx.notify();
                    })
                    .detach();
                    self.plan_cards.insert(id.clone(), card.clone());
                    hydrated.push(StreamEntry::Plan { card });
                    hydrated_identities.push(self.durable_entry_identity(&id, "plan", Some(seq)));
                }
                HistoryEntry::Summary { seq, summary } => {
                    if self.summary_cards.contains_key(&summary.message_id) {
                        continue;
                    }
                    let message_id = summary.message_id.clone();
                    let card = cx.new(|_| SummaryCard::new(summary));
                    self.summary_cards.insert(message_id.clone(), card.clone());
                    hydrated.push(StreamEntry::Summary { card });
                    hydrated_identities.push(self.durable_entry_identity(
                        &message_id,
                        "summary",
                        Some(seq),
                    ));
                }
                HistoryEntry::Tool {
                    seq,
                    message_id,
                    call_id,
                    input,
                    status,
                    approval,
                    result,
                    ..
                } => {
                    if self.tool_cards.contains_key(&call_id) {
                        continue;
                    }
                    let card = cx.new(|_| ToolCard::hydrated(input, status, approval, result));
                    self.observe_tool_card(&card, cx);
                    self.tool_cards.insert(call_id.clone(), card.clone());
                    self.refresh_subagent_card(&card, cx);
                    let run_details =
                        self.run_activity_groups
                            .get(&message_id)
                            .cloned()
                            .or_else(|| {
                                terminal_runs.get(&message_id).map(|(status, duration)| {
                                    self.ensure_hydrated_run_activity_group(
                                        &message_id,
                                        run_activity_status(*status),
                                        *duration,
                                        &mut hydrated,
                                        &mut hydrated_identities,
                                        cx,
                                    )
                                })
                            });
                    if card.read(cx).is_subagent_invocation() {
                        if let Some(group) = run_details {
                            group.update(cx, |group, cx| group.attach_tool(card.clone(), cx));
                        }
                        hydrated.push(StreamEntry::Tool { card });
                        hydrated_identities.push(self.tool_call_identity(
                            &call_id,
                            Some(&message_id),
                            Some(seq),
                        ));
                        continue;
                    }
                    if let Some(group) = run_details {
                        let segment = hydrated.last().and_then(|entry| match entry {
                            StreamEntry::RunActivitySegment {
                                group: previous,
                                segment,
                            } if previous == &group => Some(*segment),
                            _ => None,
                        });
                        let segment = segment.unwrap_or_else(|| {
                            let segment = group.update(cx, RunActivityGroup::start_segment);
                            hydrated.push(StreamEntry::RunActivitySegment {
                                group: group.clone(),
                                segment,
                            });
                            hydrated_identities.push(self.durable_entry_identity(
                                &message_id,
                                &format!("run-activity-segment-{segment}"),
                                Some(seq),
                            ));
                            segment
                        });
                        group.update(cx, |group, cx| {
                            group.append_hydrated_tool(segment, card, cx)
                        });
                        continue;
                    }
                    let previous_len = hydrated.len();
                    self.append_hydrated_tool(&mut hydrated, card, cx);
                    if hydrated.len() > previous_len {
                        hydrated_identities.push(self.tool_call_identity(
                            &call_id,
                            Some(&message_id),
                            Some(seq),
                        ));
                    }
                }
                HistoryEntry::SkillActivation { seq, activation } => {
                    let stable_id = format!(
                        "{}:{}:{}:{:?}:{:?}",
                        activation.run_id,
                        activation.name,
                        activation.content_sha256,
                        activation.origin,
                        activation.source_scope
                    );
                    hydrated.push(StreamEntry::SkillActivation { activation });
                    hydrated_identities.push(self.durable_entry_identity(
                        &stable_id,
                        "skill-activation",
                        Some(seq),
                    ));
                }
            }
        }
        for (message_id, mut responses) in restored_responses {
            if terminal_runs.get(&message_id).is_some_and(|(status, _)| {
                *status == vega_conversation::history::AssistantStatus::Done
            }) {
                responses.refresh_final_copy(&message_id, &hydrated, &hydrated_identities);
            }
            if let Some(group) = self.run_activity_groups.get(&message_id)
                && terminal_runs.get(&message_id).is_some_and(|(status, _)| {
                    *status == vega_conversation::history::AssistantStatus::Done
                })
                && let Some(copies) =
                    phase::process_copies(&message_id, &responses, &hydrated, &hydrated_identities)
            {
                group.update(cx, |group, cx| group.confirm_final(copies, false, cx));
            }
            self.run_responses.insert(message_id, responses);
        }
        (hydrated, hydrated_identities)
    }

    fn ensure_hydrated_run_activity_group(
        &mut self,
        message_id: &str,
        status: RunActivityStatus,
        execution_duration_ms: Option<u64>,
        entries: &mut Vec<StreamEntry>,
        identities: &mut Vec<StreamEntryIdentity>,
        cx: &mut Context<Self>,
    ) -> Entity<RunActivityGroup> {
        if let Some(group) = self.run_activity_groups.get(message_id) {
            return group.clone();
        }
        let continued = self.run_continuations.contains_key(message_id);
        let continuation_successor = self
            .run_continuations
            .values()
            .any(|relation| relation.next_assistant_id == message_id);
        let group = cx.new(|cx| {
            let mut group = RunActivityGroup::new(status, execution_duration_ms, false, cx);
            if continuation_successor {
                group.mark_continuation_successor(cx);
            }
            if continued {
                group.continue_run(cx);
            }
            group
        });
        self.observe_run_activity_group(&group, cx);
        entries.push(StreamEntry::RunActivity {
            group: group.clone(),
        });
        identities.push(self.durable_entry_identity(message_id, "run-activity", None));
        self.run_activity_groups
            .insert(message_id.to_string(), group.clone());
        group
    }

    fn ensure_live_run_activity_header(
        &mut self,
        message_id: &str,
        group: &Entity<RunActivityGroup>,
        sequence: Option<i64>,
        before_assistant: bool,
    ) {
        if self.entries.iter().any(
            |entry| matches!(entry, StreamEntry::RunActivity { group: owned } if owned == group),
        ) {
            return;
        }
        let index = if before_assistant {
            self.entry_identities.iter().position(|identity| {
                identity.message_id.as_deref() == Some(message_id)
                    && identity
                        .key
                        .rsplit_once(":kind:")
                        .is_some_and(|(_, kind)| kind.starts_with("assistant-text-"))
            })
        } else {
            None
        }
        .unwrap_or(self.entries.len());
        self.entries.insert(
            index,
            StreamEntry::RunActivity {
                group: group.clone(),
            },
        );
        self.list_insert(index);
        let identity = self.durable_entry_identity(message_id, "run-activity", sequence);
        self.set_entry_identity(index, identity);
        self.grow_entry_indices(index, 1);
    }

    pub(crate) fn ensure_live_run_activity_segment(
        &mut self,
        message_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<(Entity<RunActivityGroup>, usize)> {
        let (active_id, group) = self.active_run_activity.clone()?;
        if active_id != message_id {
            return None;
        }
        if let Some(segment) = self.active_run_activity_segment {
            return Some((group, segment));
        }
        self.ensure_live_run_activity_header(message_id, &group, self.active_agent_sequence, false);
        let segment = group.update(cx, |group, cx| group.start_segment(cx));
        let index = self.entries.len();
        self.entries.push(StreamEntry::RunActivitySegment {
            group: group.clone(),
            segment,
        });
        self.list_append(index);
        let identity = self.durable_entry_identity(
            message_id,
            &format!("run-activity-segment-{segment}"),
            self.active_agent_sequence,
        );
        self.set_entry_identity(index, identity);
        self.active_run_activity_segment = Some(segment);
        Some((group, segment))
    }

    pub fn request_message_location(&mut self, message_id: &str, cx: &mut Context<Self>) {
        self.request_message_location_with_anchor(message_id, None, cx);
    }

    pub fn request_message_location_with_anchor(
        &mut self,
        message_id: &str,
        restore_anchor: Option<ThreadScrollAnchor>,
        cx: &mut Context<Self>,
    ) {
        self.message_location_status = Some(MessageLocationStatus::Searching);
        cx.emit(MessageLocationRequested {
            thread_id: self.thread.id.clone(),
            message_id: message_id.to_string(),
            restore_anchor,
        });
        cx.notify();
    }

    pub fn reveal_loaded_message(&mut self, message_id: &str, cx: &mut Context<Self>) -> bool {
        self.ensure_entry_identities();
        let Some(index) = self
            .entry_identities
            .iter()
            .position(|identity| identity.message_id.as_deref() == Some(message_id))
        else {
            return false;
        };
        if let Some(group) = self.run_activity_groups.get(message_id)
            && group.read(cx).process_in_details()
        {
            group.update(cx, |group, cx| group.reveal_detail(0, 0., cx));
        }
        self.list.pause_following_tail();
        self.list.scroll_to(gpui_kit::ListOffset {
            item_ix: index,
            offset_in_item: px(0.),
        });
        self.message_location_status = Some(MessageLocationStatus::Located);
        cx.notify();
        true
    }

    pub fn replace_history_window(
        &mut self,
        page: HistoryPage,
        message_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.replace_history_window_with_anchor(page, message_id, None, cx)
    }

    pub fn replace_history_window_with_anchor(
        &mut self,
        page: HistoryPage,
        message_id: &str,
        restore_anchor: Option<&ThreadScrollAnchor>,
        cx: &mut Context<Self>,
    ) -> bool {
        let contains_target = page.entries.iter().any(|entry| match entry {
            HistoryEntry::CredentialContinuation { message_id: id, .. }
            | HistoryEntry::UserText { message_id: id, .. }
            | HistoryEntry::UserImages { message_id: id, .. }
            | HistoryEntry::AssistantText { message_id: id, .. }
            | HistoryEntry::AssistantScreenshot { message_id: id, .. }
            | HistoryEntry::Tool { message_id: id, .. }
            | HistoryEntry::AssistantResponseMetadata { message_id: id, .. }
            | HistoryEntry::GeneratedNotice { message_id: id, .. } => id == message_id,
            HistoryEntry::Plan { plan, .. } => plan.id == message_id,
            HistoryEntry::Summary { summary, .. } => summary.message_id == message_id,
            HistoryEntry::SkillActivation { .. } | HistoryEntry::RunContinuation { .. } => false,
        });
        if !contains_target {
            self.message_location_status = Some(MessageLocationStatus::NotFound);
            cx.notify();
            return false;
        }
        let ordinary_entries = self.take_ordinary_history_entries();
        self.assistant_role_rows.clear();
        self.run_notices.clear();
        self.response_checkpoint = None;
        self.network_retry = None;
        self.controller_runtime_error = None;
        self.entry_identities.clear();
        self.measured_entry_heights.clear();
        self.text_cache.get_mut().clear();
        self.message_anchor_hovered = None;
        self.message_anchor_preview_hovered = None;
        self.message_anchor_keyboard_id = None;
        self.list.reset(0);
        self.tool_cards.clear();
        self.preserve_subagent_requests(cx);
        self.subagent_executions.clear();
        self.run_activity_groups.clear();
        self.run_continuations.clear();
        self.run_responses.clear();
        self.generated_notices.clear();
        self.artifact_cards.clear();
        self.artifact_call_owners.clear();
        self.artifact_history_generation = self.artifact_history_generation.wrapping_add(1);
        self.artifact_groups.clear();
        self.finished_artifact_turns.clear();
        self.plan_cards.clear();
        self.summary_cards.clear();
        self.active_agent_message = None;
        self.active_agent_sequence = None;
        self.active_run_activity = None;
        self.active_run_activity_segment = None;
        self.active_segment_ordinal = 0;
        self.active_response_start = None;
        self.active_response_id = None;
        self.response_reasoning_completed = false;
        self.active_segment_has_text = false;
        self.active_segment_has_visible_text = false;
        self.active_thinking = None;
        self.thinking_bytes = 0;
        self.thinking_blocks = 0;
        self.retained_thinking.clear();
        self.last_finished_agent_message = None;
        self.active_permission = None;
        self.active_permission_call_id = None;
        self.deferred_permission = None;
        self.hydration = HistoryHydration::default();
        self.apply_history_page(page, cx);
        self.restore_ordinary_history_entries(ordinary_entries, cx);
        if let Some(anchor) = restore_anchor
            && (self.restore_process_anchor(anchor, cx) || self.restore_scroll_anchor(anchor))
        {
            self.message_location_status = Some(MessageLocationStatus::Located);
            cx.notify();
            return true;
        }
        self.reveal_loaded_message(message_id, cx)
    }

    pub fn apply_message_location_status(
        &mut self,
        status: MessageLocationStatus,
        cx: &mut Context<Self>,
    ) {
        self.message_location_status = Some(status);
        cx.notify();
    }

    /// Releases the in-flight page slot after a failed load. Auto-retry waits
    /// until the viewport leaves the top edge again (failure pause), so a
    /// persistently broken store cannot turn a pinned scroll into a spin loop.
    pub fn apply_history_load_failed(&mut self, cx: &mut Context<Self>) {
        self.hydration.loading = false;
        self.hydration.paused = true;
        self.hydration.paused_direction = Some(HistoryPageDirection::Older);
        cx.notify();
    }

    pub fn apply_newer_history_load_failed(&mut self, cx: &mut Context<Self>) {
        self.hydration.loading = false;
        self.hydration.paused = true;
        self.hydration.paused_direction = Some(HistoryPageDirection::Newer);
        cx.notify();
    }

    /// The cursor to request for scroll-up hydration, or `None` when no page
    /// may be requested: exhausted history, a page already in flight, a
    /// paused failure, or the viewport away from the top edge.
    pub(crate) fn history_page_request(&self, at_top: bool) -> Option<i64> {
        hydration_request(self.hydration, at_top)
    }

    pub(crate) fn newer_history_page_request(&self, at_bottom: bool) -> Option<i64> {
        if !at_bottom || self.hydration.loading || self.hydration.paused {
            return None;
        }
        self.hydration.newer_cursor
    }

    pub fn newer_history_cursor(&self) -> Option<i64> {
        self.hydration.newer_cursor
    }

    pub(crate) fn scroll_at_bottom(&self) -> bool {
        self.list.is_following_tail() || self.list.is_scrolled_to_end() == Some(true)
    }

    /// Whether the list is scrolled to (within epsilon of) its top edge
    /// (native top-of-list offset; item-index 0 with no in-item offset).
    pub(crate) fn scroll_at_top(&self) -> bool {
        let top = self.list.logical_scroll_top();
        top.item_ix == 0 && f32::from(top.offset_in_item) <= ANCHOR_EPSILON_PX
    }

    /// Whether the failure pause is still armed; leaving the top edge
    /// re-arms scroll-up hydration.
    pub(crate) fn hydration_pause_is_stale(&self, at_top: bool, at_bottom: bool) -> bool {
        self.hydration.paused
            && match self.hydration.paused_direction.unwrap_or_default() {
                HistoryPageDirection::Older => !at_top,
                HistoryPageDirection::Newer => !at_bottom,
            }
    }

    /// Hook passed to the conversation runner for this visible stream.
    pub fn permission_queue(&self) -> PermissionQueue {
        self.permission_queue.clone()
    }

    pub fn branch_selector(&self) -> Entity<BranchSelector> {
        self.branch_selector.clone()
    }

    pub fn commit_panel(&self) -> Entity<CommitPanel> {
        self.commit_panel.clone()
    }

    /// Content-free app-controller guards for trusted workspace actions.
    pub fn has_active_agent(&self) -> bool {
        self.active_agent_message.is_some()
    }

    pub fn has_pending_permission(&self) -> bool {
        self.permission_queue.has_pending()
    }

    pub fn has_pending_plan_review(&self, cx: &App) -> bool {
        self.plan_cards
            .values()
            .any(|card| card.read(cx).status() == vega_conversation::types::PlanStatus::Pending)
    }

    /// Returns whether a model-selection request would race another
    /// thread-owned operation. The caller supplies the app context so the
    /// durable plan-card projection can be checked without touching SQLite.
    pub fn model_selection_blocked(&self, cx: &App) -> bool {
        self.actions.running
            || self.context_operation_busy()
            || self.composer_submit_pending
            || self.approved_not_started
            || self.trusted_action_busy
            || self.active_permission.is_some()
            || self.permission_queue.has_pending()
            || self.has_pending_user_question()
            || self.has_pending_plan_review(cx)
    }

    /// Returns whether the exact-owner model-selection save is still pending.
    pub fn has_pending_model_selection(&self) -> bool {
        self.model_selection_pending.is_some()
    }

    /// Whether a callback still belongs to this stream's exact selection
    /// owner. This is intentionally separate from the route fence in the app
    /// window: stale streams still need to clear their own pending state.
    pub fn owns_model_selection_request(&self, request_id: u64) -> bool {
        self.model_selection_pending
            .as_ref()
            .is_some_and(|(owner_id, _)| *owner_id == request_id)
    }

    /// Whether this stream has already installed the exact app-level busy
    /// owner for its pending model request. Generic trusted-action busy is
    /// intentionally excluded, so duplicate delivery is distinguishable from
    /// a model request rejected behind another trusted action.
    pub fn model_selection_save_busy(&self) -> bool {
        self.model_selection_save_owner.is_some()
    }

    pub fn set_trusted_action_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.trusted_action_busy = busy;
        if busy {
            self.model_picker_level = ModelPickerLevel::Closed;
        }
        self.branch_selector
            .update(cx, |selector, cx| selector.set_disabled(busy, cx));
        self.commit_panel.update(cx, |panel, cx| {
            panel.set_disabled(busy && !panel.is_open(), cx)
        });
        cx.notify();
    }

    /// Fails the visible/pending prompt closed before Settings, thread switch,
    /// or window teardown hides the card.
    pub fn timeout_permission(&mut self, cx: &mut Context<Self>) {
        self.permission_queue.timeout_active();
        drop(self.deferred_permission.take());
        self.remove_active_permission(cx);
    }

    pub(crate) fn install_pending_permission(&mut self, cx: &mut Context<Self>) {
        if cx
            .try_global::<SettingsOpen>()
            .is_some_and(|settings| settings.0)
        {
            self.timeout_permission(cx);
            return;
        }
        if self
            .deferred_permission
            .as_ref()
            .is_some_and(PendingPermission::is_resolved)
        {
            drop(self.deferred_permission.take());
        }
        let Some(pending) = self
            .deferred_permission
            .take()
            .or_else(|| self.permission_queue.take_pending())
        else {
            return;
        };
        let Some(request) = pending.request() else {
            drop(pending);
            return;
        };
        let call_id = request.call_id.clone();
        let card = self.tool_cards.get(&call_id).cloned();
        let identity = card
            .as_ref()
            .and_then(|card| card.read(cx).permission_identity());
        match pending.presentation(identity) {
            vega_conversation::types::PermissionPresentation::WaitingForTool => {
                self.deferred_permission = Some(pending);
                return;
            }
            vega_conversation::types::PermissionPresentation::Mismatch => {
                drop(pending);
                if let Some(card) = card {
                    card.update(cx, ToolCard::fail_corrupt);
                    self.invalidate_tool_card(&card, cx);
                }
                return;
            }
            vega_conversation::types::PermissionPresentation::Ready => {}
        }
        let allow_persistent = pending.allows_persistent_permission();
        let codex_identity = card
            .as_ref()
            .and_then(|card| card.read(cx).codex_permission_display());
        self.remove_active_permission(cx);
        let Some((request, lease)) = pending.into_parts() else {
            return;
        };
        let card =
            cx.new(|cx| PermissionCard::new(&request, lease, allow_persistent, codex_identity, cx));
        cx.subscribe(&card, |this, card, _: &PermissionCardResolved, cx| {
            if this
                .active_permission
                .as_ref()
                .is_some_and(|active| active == &card)
            {
                this.remove_active_permission(cx);
            }
        })
        .detach();
        self.entries
            .push(StreamEntry::Permission { card: card.clone() });
        self.list_append(self.entries.len() - 1);
        self.active_permission = Some(card);
        if let Some((_, group)) = &self.active_run_activity {
            group.update(cx, |group, cx| group.set_waiting(true, cx));
        }
        self.active_permission_call_id = Some(request.call_id);
        // The prompt must be visible immediately: re-engage native tail
        // follow (which also scrolls to the end on the next layout).
        self.list.set_follow_mode(gpui_kit::FollowMode::Tail);
        cx.notify();
    }

    pub(crate) fn remove_active_permission(&mut self, cx: &mut Context<Self>) {
        self.active_permission_call_id = None;
        let Some(active) = self.active_permission.take() else {
            return;
        };
        if let Some((_, group)) = &self.active_run_activity {
            group.update(cx, |group, cx| group.set_waiting(false, cx));
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::Permission { card } if card == &active))
        {
            self.entries.remove(index);
            self.list_remove(index);
            self.shift_entry_indices(index, 1);
            self.merge_tool_entries_around(index, cx);
        }
        cx.notify();
    }

    fn observe_run_activity_group(
        &mut self,
        group: &Entity<RunActivityGroup>,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(
            group,
            |this, group, change: &run_activity::RunActivityDisplayChanged, cx| {
                if change.geometry {
                    let indexes = this
                        .entries
                        .iter()
                        .enumerate()
                        .filter_map(|(index, entry)| match (entry, change.segment) {
                            (StreamEntry::RunActivity { group: owned }, None)
                                if owned == &group =>
                            {
                                Some(index)
                            }
                            (
                                StreamEntry::RunActivitySegment {
                                    group: owned,
                                    segment,
                                },
                                Some(changed),
                            ) if owned == &group && *segment == changed => Some(index),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    for index in indexes {
                        this.invalidate_item(Some(index));
                    }
                }
                cx.notify();
            },
        )
        .detach();
        cx.observe(group, |this, group, cx| {
            let indexes = this
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| match entry {
                    StreamEntry::RunActivity { group: owned }
                    | StreamEntry::RunActivitySegment { group: owned, .. }
                        if owned == &group =>
                    {
                        Some(index)
                    }
                    StreamEntry::Assistant { copy, .. }
                        if group.read(cx).owns_process_copy(copy.id) =>
                    {
                        Some(index)
                    }
                    StreamEntry::Tool { card }
                        if card.read(cx).is_subagent_invocation()
                            && this.entry_owner_group(index) == Some(&group) =>
                    {
                        Some(index)
                    }
                    StreamEntry::ContextCompaction { .. }
                        if this
                            .entry_identities
                            .get(index)
                            .and_then(|identity| identity.message_id.as_ref())
                            .and_then(|owner| this.run_activity_groups.get(owner))
                            == Some(&group) =>
                    {
                        Some(index)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            for index in indexes {
                this.invalidate_item(Some(index));
            }
            if let Some(segment) = group.update(cx, |group, _| group.take_sticky_collapse())
                && let Some(identity) =
                    this.entries
                        .iter()
                        .zip(&this.entry_identities)
                        .find_map(|(entry, identity)| match (entry, &segment) {
                            (
                                StreamEntry::RunActivity { group: owned },
                                run_activity::StickyCollapse::Run,
                            ) if owned == &group => Some(identity.key.clone()),
                            (
                                StreamEntry::RunActivitySegment {
                                    group: owned,
                                    segment: owned_segment,
                                },
                                run_activity::StickyCollapse::Segment(segment),
                            ) if owned == &group && owned_segment == segment => {
                                Some(identity.key.clone())
                            }
                            _ => None,
                        })
                && let Some(index) = this
                    .entry_identities
                    .iter()
                    .position(|entry| entry.key == identity)
            {
                this.list.scroll_to(gpui_kit::ListOffset {
                    item_ix: index,
                    offset_in_item: px(0.),
                });
            }
            cx.notify();
        })
        .detach();
    }

    fn remember_run_continuation(
        &mut self,
        relation: vega_conversation::types::RunContinuation,
        cx: &mut Context<Self>,
    ) -> bool {
        let previous = &relation.previous_assistant_id;
        let next = &relation.next_assistant_id;
        if let Some(known) = self.run_continuations.get(previous) {
            return known == &relation;
        }
        if previous == next
            || previous == &relation.user_message_id
            || next == &relation.user_message_id
            || self.run_continuations.values().any(|known| {
                known.next_assistant_id == *next
                    || known.user_message_id == relation.user_message_id
            })
        {
            return false;
        }
        let mut successor = next.as_str();
        while let Some(known) = self.run_continuations.get(successor) {
            successor = &known.next_assistant_id;
            if successor == previous {
                return false;
            }
        }
        let anchor = self.scroll_anchor_snapshot();
        if let Some(group) = self.run_activity_groups.get(previous) {
            group.update(cx, |group, cx| group.continue_run(cx));
        }
        if let Some(group) = self.run_activity_groups.get(next) {
            group.update(cx, |group, cx| group.mark_continuation_successor(cx));
        }
        let indexes = self
            .entry_identities
            .iter()
            .enumerate()
            .filter_map(|(index, identity)| {
                identity
                    .message_id
                    .as_ref()
                    .is_some_and(|id| id == previous || id == next)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        self.run_continuations.insert(previous.clone(), relation);
        for index in indexes {
            self.invalidate_item(Some(index));
            self.invalidate_item((index + 1 < self.entries.len()).then_some(index + 1));
        }
        self.restore_scroll_anchor(&anchor);
        cx.notify();
        true
    }

    fn invalidate_codex_copy_entries(&mut self, message_id: &str) {
        if self.thread.backend != TaskBackend::Codex {
            return;
        }
        let indexes = self
            .entries
            .iter()
            .zip(&self.entry_identities)
            .enumerate()
            .filter_map(|(index, (entry, identity))| {
                (matches!(entry, StreamEntry::Assistant { .. })
                    && identity.message_id.as_deref() == Some(message_id))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        for index in indexes {
            self.invalidate_item(Some(index));
        }
    }

    fn finish_run_activity(
        &mut self,
        message_id: &str,
        status: RunActivityStatus,
        execution_duration_ms: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if status != RunActivityStatus::Completed {
            self.clear_final_copy(message_id);
        }
        if let Some(group) = self.run_activity_groups.get(message_id).cloned() {
            if self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active, _)| active == message_id)
            {
                self.ensure_live_run_activity_header(
                    message_id,
                    &group,
                    self.active_agent_sequence,
                    true,
                );
            }
            group.update(cx, |group, cx| {
                group.complete(status, execution_duration_ms, cx)
            });
        }
        self.invalidate_codex_copy_entries(message_id);
    }

    /// Applies an already-durable shared lifecycle event. The UI never reads
    /// SQLite and never consumes runtime-local events.
    pub fn apply_event(&mut self, event: ConversationEvent, cx: &mut Context<Self>) {
        let terminal_id = match &event {
            ConversationEvent::MessageFinished { message_id, .. }
            | ConversationEvent::Interrupted { message_id, .. }
            | ConversationEvent::Error {
                message_id: Some(message_id),
                ..
            } => Some(message_id),
            _ => None,
        };
        if terminal_id.is_some_and(|id| self.run_continuations.contains_key(id)) {
            return;
        }
        self.feed_meter(&event, cx);
        self.clear_recovered_retry(&event);
        match event {
            ConversationEvent::RunContinued {
                previous_assistant_id,
                user_message_id,
                next_assistant_id,
            } => {
                if self
                    .active_agent_message
                    .as_ref()
                    .is_some_and(|(active, _)| active == &previous_assistant_id)
                    && self.remember_run_continuation(
                        vega_conversation::types::RunContinuation {
                            previous_assistant_id: previous_assistant_id.clone(),
                            user_message_id,
                            next_assistant_id,
                        },
                        cx,
                    )
                {
                    self.finish_agent_message(&previous_assistant_id, cx);
                }
            }
            ConversationEvent::AssistantResponseStarted {
                message_id,
                response_id,
                text_start_bytes,
            } => {
                self.checkpoint_response(&message_id, &response_id, text_start_bytes, cx);
            }
            ConversationEvent::ReasoningCompleted {
                message_id,
                response_id,
            } => {
                if self.matches_response(&message_id, &response_id)
                    && !self.response_reasoning_completed
                {
                    self.response_reasoning_completed = true;
                    self.finish_live_thinking(cx);
                }
            }
            ConversationEvent::ResponseRetrying {
                message_id,
                text_start_bytes,
                attempt,
                max_retries,
            } => {
                self.retry_response(&message_id, text_start_bytes, attempt, max_retries, cx);
            }
            ConversationEvent::PendingMessageDelivered {
                pending_id,
                message_id,
                seq,
                content,
                images,
            } => {
                self.append_pending_user(&pending_id, &message_id, seq, &content, images, cx);
            }
            ConversationEvent::MessageStarted { message_id, seq } => {
                if self.finished_artifact_turns.contains(&message_id) {
                    return;
                }
                if self.active_agent_message.is_some() {
                    self.apply_controller_error(cx);
                    return;
                }
                self.reset_live_context_usage();
                self.last_finished_agent_message = None;
                let continuation_successor = self
                    .run_continuations
                    .values()
                    .any(|relation| relation.next_assistant_id == message_id);
                if !continuation_successor {
                    self.begin_computer_use_task(cx);
                }
                self.active_thinking = None;
                self.active_skills.clear();
                self.active_skill_focuses.clear();
                let group = cx.new(|cx| {
                    let mut group =
                        RunActivityGroup::new(RunActivityStatus::Running, None, false, cx);
                    if continuation_successor {
                        group.mark_continuation_successor(cx);
                    }
                    group
                });
                self.observe_run_activity_group(&group, cx);
                self.run_activity_groups
                    .insert(message_id.clone(), group.clone());
                self.active_run_activity = Some((message_id.clone(), group.clone()));
                self.active_segment_ordinal = 0;
                self.active_response_start = None;
                self.active_response_id = None;
                self.response_reasoning_completed = false;
                self.active_agent_sequence = i64::try_from(seq).ok();
                self.active_run_activity_segment = None;
                self.ensure_live_run_activity_header(
                    &message_id,
                    &group,
                    self.active_agent_sequence,
                    false,
                );
                self.generated_notices.remove(&message_id);
                self.active_agent_message = Some((message_id, usize::MAX));
                self.active_segment_has_text = false;
                self.active_segment_has_visible_text = false;
                self.sync_retained_thinking_actions(cx);
                cx.notify();
            }
            ConversationEvent::RunStarted {
                message_id,
                started_at,
                work_clock,
            } => {
                if let Some((active_id, group)) = &self.active_run_activity
                    && active_id == &message_id
                {
                    group.update(cx, |group, cx| {
                        group.start_clock(started_at, work_clock, cx)
                    });
                }
            }
            ConversationEvent::AssistantResponseCompleted {
                message_id,
                response,
            } => {
                if self.matches_response(&message_id, &response.response_id) {
                    let text = self
                        .entries
                        .iter()
                        .zip(&self.entry_identities)
                        .filter_map(|(entry, identity)| {
                            if identity.message_id.as_deref() != Some(message_id.as_str()) {
                                return None;
                            }
                            match entry {
                                StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
                                _ => None,
                            }
                        })
                        .collect::<String>();
                    if response.is_valid_for(&text) {
                        self.finish_live_thinking(cx);
                    }
                }
                self.apply_response_metadata(&message_id, response, cx);
            }
            ConversationEvent::GeneratedNotice { message_id, notice } => {
                self.apply_generated_notice(&message_id, notice, cx);
            }
            ConversationEvent::TextDelta { message_id, delta } => {
                let Some((active_id, entry_index)) = self.active_agent_message.as_ref() else {
                    return;
                };
                if active_id != &message_id || delta.is_empty() {
                    return;
                }
                let entry_index = *entry_index;
                if self.clear_final_copy(&message_id)
                    && let Some(group) = self.run_activity_groups.get(&message_id)
                {
                    group.update(cx, |group, cx| group.invalidate_final(cx));
                }
                let starts_visible_text = !self.active_segment_has_visible_text
                    && delta.chars().any(|character| !character.is_whitespace());
                if delta.chars().any(|character| !character.is_whitespace()) {
                    self.finish_live_thinking(cx);
                    if let Some((_, group)) = &self.active_run_activity {
                        group.update(cx, |group, cx| {
                            group.set_phase(run_activity::RunPhase::Responding, cx);
                        });
                    }
                    self.active_thinking = None;
                }
                if starts_visible_text {
                    self.active_run_activity_segment = None;
                }
                let entry_index = if entry_index == usize::MAX {
                    let index = self.entries.len();
                    self.entries.push(StreamEntry::Assistant {
                        copy: MessageCopy::default(),
                        stream: Box::new(MarkdownStream::new()),
                        model: StreamModel::default(),
                        failure: None,
                    });
                    self.list_append(index);
                    let identity = self.durable_entry_identity(
                        &message_id,
                        &self
                            .active_response_start
                            .map(|start| {
                                format!(
                                    "assistant-text-{}-response-{start}",
                                    self.active_segment_ordinal
                                )
                            })
                            .unwrap_or_else(|| {
                                format!("assistant-text-{}", self.active_segment_ordinal)
                            }),
                        if self.active_segment_ordinal == 0 {
                            self.active_agent_sequence
                        } else {
                            None
                        },
                    );
                    self.set_entry_identity(index, identity);
                    if let Some((_, active_index)) = &mut self.active_agent_message {
                        *active_index = index;
                    }
                    index
                } else {
                    entry_index
                };
                if let Some(StreamEntry::Assistant { stream, copy, .. }) =
                    self.entries.get_mut(entry_index)
                {
                    stream.append(&delta);
                    copy.append(&delta);
                    self.assistant_model_dirty = true;
                    self.active_segment_has_text = true;
                    self.active_segment_has_visible_text |= starts_visible_text;
                    cx.notify();
                }
            }
            ConversationEvent::SummaryDelta { message_id, delta } => {
                self.append_reasoning(&message_id, &delta, true, cx);
            }
            ConversationEvent::ThinkingDelta { message_id, delta } => {
                self.append_thinking(&message_id, &delta, cx);
            }
            ConversationEvent::ScreenshotDelivered {
                message_id,
                call_id,
                image,
            } => {
                if self
                    .active_agent_message
                    .as_ref()
                    .is_some_and(|(active, _)| active == &message_id)
                {
                    let identity = self.durable_entry_identity(
                        &message_id,
                        &format!("screenshot-{call_id}"),
                        self.active_agent_sequence,
                    );
                    if !self
                        .entry_identities
                        .iter()
                        .any(|existing| existing == &identity)
                    {
                        let index = self.entries.len();
                        let card = self.screenshot_card(image, cx);
                        self.entries.push(StreamEntry::Screenshot { card });
                        self.list_append(index);
                        self.set_entry_identity(index, identity);
                        self.invalidate_item(Some(index));
                        cx.notify();
                    }
                }
            }
            ConversationEvent::SkillActivated { message_id, skill } => {
                if self
                    .active_agent_message
                    .as_ref()
                    .is_some_and(|(active, _)| active == &message_id)
                    && !self
                        .active_skills
                        .iter()
                        .any(|item| item.name == skill.name)
                {
                    self.active_skills.push(skill);
                    self.active_skill_focuses.push([
                        cx.focus_handle().tab_stop(true),
                        cx.focus_handle().tab_stop(true),
                    ]);
                    cx.notify();
                }
            }
            ConversationEvent::UsageUpdated { .. }
            | ConversationEvent::ContextCompactionUsageUpdated { .. }
            | ConversationEvent::ContextCompactionStatus { .. }
            | ConversationEvent::ContextAccounting { .. } => {}
            ConversationEvent::ToolCallProposed { call } => {
                self.register_artifact_call(&call.id);
                if let Some(existing) = self.tool_cards.get(&call.id) {
                    existing.update(cx, |card, cx| {
                        if !card.matches_call(&call) {
                            card.fail_corrupt(cx);
                        }
                    });
                    self.install_pending_permission(cx);
                    self.install_pending_user_question(cx);
                    return;
                }
                self.finish_live_thinking(cx);
                self.close_active_segment_before_tool();
                let call_id = call.id.clone();
                let card = cx.new(|_| ToolCard::proposed(&call));
                self.observe_tool_card(&card, cx);
                self.append_live_tool(card.clone(), true, cx);
                if card.read(cx).is_subagent_invocation()
                    && let Some(index) = self.tool_entry_index(&card, cx)
                {
                    let identity = self.tool_call_identity(
                        &call_id,
                        self.active_agent_message
                            .as_ref()
                            .map(|(id, _)| id.as_str()),
                        self.active_agent_sequence,
                    );
                    self.set_entry_identity(index, identity);
                }
                self.tool_cards.insert(call_id, card);
                self.install_pending_permission(cx);
                self.install_pending_user_question(cx);
                cx.notify();
            }
            ConversationEvent::ToolCallApproved { call_id, approval } => {
                if let Some(card) = self.tool_cards.get(&call_id) {
                    let card = card.clone();
                    card.update(cx, |card, cx| {
                        card.apply_approved(approval);
                        cx.notify();
                    });
                    self.invalidate_tool_card(&card, cx);
                } else {
                    self.push_corrupt_tool(call_id, cx);
                }
            }
            ConversationEvent::ToolCallRunning { call_id } => {
                if let Some(card) = self.tool_cards.get(&call_id) {
                    let card = card.clone();
                    let started = card.update(cx, |card, cx| {
                        let was_running = card.is_running();
                        card.apply_running(cx);
                        !was_running && card.is_running()
                    });
                    if started && let Some((_, group)) = &self.active_run_activity {
                        group.update(cx, |group, cx| group.tool_started(&card, cx));
                    }
                    self.invalidate_tool_card(&card, cx);
                } else {
                    self.push_corrupt_tool(call_id, cx);
                }
            }
            ConversationEvent::ToolCallOutput { .. } => {}
            ConversationEvent::ToolCallFinished { call_id, result } => {
                self.timeout_permission_for_call(&call_id, cx);
                self.finish_user_question(&call_id, cx);
                if let Some(card) = self.tool_cards.get(&call_id) {
                    let card = card.clone();
                    card.update(cx, |card, cx| {
                        card.apply_finished(&result);
                        cx.notify();
                    });
                    self.refresh_subagent_card(&card, cx);
                    self.invalidate_tool_card(&card, cx);
                } else {
                    // Validation rejection/conflict can be terminal without
                    // a prior proposal event. Its first visible card is
                    // still a Markdown boundary in the accepted timeline.
                    self.close_active_segment_before_tool();
                    let card = if result.invalid.is_some() {
                        ToolCard::invalid_terminal(&result)
                    } else {
                        ToolCard::corrupt()
                    };
                    self.push_tool_card(call_id, card, cx);
                }
                cx.emit(WorkspaceToolTerminal {
                    thread_id: self.thread.id.clone(),
                    project_id: self.thread.project_id.clone(),
                });
            }
            ConversationEvent::MessageFinished {
                message_id,
                execution_duration_ms,
                ..
            } => {
                self.finish_run_activity(
                    &message_id,
                    RunActivityStatus::Completed,
                    execution_duration_ms,
                    cx,
                );
                self.record_composer_terminal(&message_id, false);
                self.finish_agent_message(&message_id, cx);
            }
            ConversationEvent::Interrupted {
                message_id,
                execution_duration_ms,
            } => {
                self.finish_run_activity(
                    &message_id,
                    RunActivityStatus::Interrupted,
                    execution_duration_ms,
                    cx,
                );
                self.record_composer_terminal(&message_id, true);
                self.finish_agent_message(&message_id, cx);
            }
            ConversationEvent::Error {
                message_id,
                error,
                execution_duration_ms,
            } => {
                let failure = RunFailureKind::from_runtime(&error);
                if let Some(message_id) = message_id {
                    let failure =
                        if self
                            .run_activity_groups
                            .get(&message_id)
                            .is_some_and(|group| {
                                group.read(cx).status() != RunActivityStatus::Completed
                            })
                            && self.entry_identities.iter().any(|identity| {
                                identity.message_id.as_deref() == Some(message_id.as_str())
                            })
                        {
                            self.remember_run_failure(&message_id, failure)
                        } else {
                            failure
                        };
                    self.finish_run_activity(
                        &message_id,
                        RunActivityStatus::Failed,
                        execution_duration_ms,
                        cx,
                    );
                    // A stale event must not annotate the currently active
                    // assistant turn (nor overwrite a newer composer error).
                    if let Some((active_id, entry_index)) = self.active_agent_message.as_ref()
                        && active_id == &message_id
                        && *entry_index == usize::MAX
                    {
                        self.append_empty_terminal_segment();
                    }
                    let active_failure_index =
                        self.active_agent_message
                            .as_ref()
                            .and_then(|(active_id, index)| {
                                (active_id == &message_id && *index != usize::MAX).then_some(*index)
                            });
                    if let Some(entry_index) = active_failure_index {
                        let failure = self.remember_run_failure(&message_id, failure);
                        if let Some(StreamEntry::Assistant {
                            failure: active_failure,
                            ..
                        }) = self.entries.get_mut(entry_index)
                        {
                            *active_failure = Some(failure);
                            self.controller_runtime_error = Some((message_id.clone(), failure));
                            self.controller_error =
                                Some(crate::localized_copy::LocalizedCopy::Run(failure));
                        }
                    }
                    self.record_composer_terminal(&message_id, false);
                    self.finish_agent_message(&message_id, cx);
                } else {
                    self.controller_runtime_error = None;
                    self.controller_error =
                        Some(crate::localized_copy::LocalizedCopy::Run(failure));
                    if let Some((message_id, _)) = self.active_agent_message.clone() {
                        self.finish_agent_message(&message_id, cx);
                    }
                    self.timeout_permission(cx);
                    self.cancel_user_questions(cx);
                    cx.notify();
                }
            }
        }
    }

    fn finish_live_thinking(&mut self, cx: &mut Context<Self>) {
        if let Some(card) = &self.active_thinking {
            card.update(cx, |card, _| card.progress_finished = true);
        }
        if let Some((_, group)) = &self.active_run_activity {
            group.update(cx, |group, cx| group.finish_thinking(cx));
        }
    }

    pub(crate) fn finish_agent_message(&mut self, message_id: &str, cx: &mut Context<Self>) {
        let Some((active_id, entry_index)) = self.active_agent_message.as_ref() else {
            return;
        };
        if active_id != message_id {
            return;
        }
        let entry_index = *entry_index;
        if let Some((active_id, group)) = &self.active_run_activity
            && active_id == message_id
        {
            group.update(cx, |group, cx| group.release_presentation(cx));
        }
        self.clear_live_context_accounting();
        self.response_checkpoint = None;
        self.network_retry = None;
        self.active_thinking = None;
        self.active_skills.clear();
        self.active_skill_focuses.clear();
        // finish() 丢弃 pending 并把尾块冻结为 committed（version bump）：
        // 这是从 mutable tail 摘除前的最后一次显式失效（C4 白名单），必须
        // 在本帧内完成最终物化——否则批量 ingress 末批 [delta…, Finished]
        // 的尾部 delta 永不上屏、终块永无 committed 高亮。
        if entry_index != usize::MAX {
            if let Some(StreamEntry::Assistant {
                copy,
                stream,
                model,
                ..
            }) = self.entries.get_mut(entry_index)
            {
                copy.finish_model(stream, model, &self.counters);
            }
            self.invalidate_item(Some(entry_index));
        }
        self.last_finished_agent_message = self
            .active_agent_message
            .take()
            .filter(|(_, index)| *index != usize::MAX);
        self.invalidate_codex_copy_entries(message_id);
        self.active_segment_has_text = false;
        self.active_segment_has_visible_text = false;
        self.active_segment_ordinal = 0;
        self.active_response_start = None;
        self.active_response_id = None;
        self.response_reasoning_completed = false;
        self.active_agent_sequence = None;
        self.active_run_activity_segment = None;
        if self
            .active_run_activity
            .as_ref()
            .is_some_and(|(active, _)| active == message_id)
        {
            self.active_run_activity = None;
        }
        self.finish_artifact_turn(message_id, cx);
        self.timeout_permission(cx);
        self.cancel_user_questions(cx);
        self.sync_retained_thinking_actions(cx);
        cx.notify();
    }

    /// Freeze the Markdown parser at a tool boundary. A later provider round
    /// gets a fresh parser so unfinished fences/tables cannot absorb it.
    pub(crate) fn close_active_segment_before_tool(&mut self) {
        self.active_thinking = None;
        let Some((_, index)) = self.active_agent_message.as_ref() else {
            return;
        };
        let index = *index;
        let response_closed = self.active_response_start.take().is_some();
        if index == usize::MAX {
            if response_closed {
                self.active_segment_ordinal = self.active_segment_ordinal.saturating_add(1);
            }
            return;
        }
        if self.active_segment_has_text {
            if let Some(StreamEntry::Assistant {
                copy,
                stream,
                model,
                ..
            }) = self.entries.get_mut(index)
            {
                copy.finish_model(stream, model, &self.counters);
            }
            self.invalidate_item(Some(index));
            self.active_segment_ordinal = self.active_segment_ordinal.saturating_add(1);
        } else {
            self.entries.remove(index);
            self.list_remove(index);
            self.shift_entry_indices(index, 1);
        }
        if let Some((_, active_index)) = &mut self.active_agent_message {
            *active_index = usize::MAX;
        }
        self.active_segment_has_text = false;
        self.active_segment_has_visible_text = false;
    }

    pub(crate) fn close_active_segment_at_response(&mut self, end: usize) {
        let ordinal = self.active_segment_ordinal;
        self.close_active_segment_before_tool();
        self.active_segment_ordinal = ordinal;
        self.active_response_start = Some(end);
    }

    fn append_empty_terminal_segment(&mut self) {
        let index = self.entries.len();
        let message_id = self
            .active_agent_message
            .as_ref()
            .map(|(message_id, _)| message_id.clone());
        self.entries.push(StreamEntry::Assistant {
            copy: MessageCopy::default(),
            stream: Box::new(MarkdownStream::new()),
            model: StreamModel::default(),
            failure: None,
        });
        self.list_append(index);
        if let Some(message_id) = message_id {
            let identity = self.durable_entry_identity(
                &message_id,
                &format!("assistant-text-{}", self.active_segment_ordinal),
                None,
            );
            self.set_entry_identity(index, identity);
        }
        if let Some((_, active_index)) = &mut self.active_agent_message {
            *active_index = index;
        }
    }

    /// Fails only the permission request bound to this terminal call. A late
    /// or unrelated terminal event must not consume another call's pending
    /// latch or remove its visible card.
    fn timeout_permission_for_call(&mut self, call_id: &str, cx: &mut Context<Self>) {
        let deferred_matches = self
            .deferred_permission
            .as_ref()
            .and_then(PendingPermission::request)
            .is_some_and(|request| request.call_id == call_id);
        if deferred_matches {
            drop(self.deferred_permission.take());
        }
        if self.active_permission_call_id.as_deref() == Some(call_id) {
            if let Some(active) = self.active_permission.clone() {
                active.update(cx, PermissionCard::timeout);
            }
            self.remove_active_permission(cx);
        }
    }

    pub(crate) fn push_corrupt_tool(&mut self, call_id: String, cx: &mut Context<Self>) {
        self.push_tool_card(call_id, ToolCard::corrupt(), cx);
    }

    /// Marks an existing tool card item for re-measurement (status/approval/
    /// result/expansion changes may change its height; the C4 explicit
    /// invalidation whitelist).
    pub(crate) fn invalidate_tool_card(&mut self, card: &Entity<ToolCard>, cx: &App) {
        let index = self.tool_entry_index(card, cx);
        self.invalidate_item(index);
    }

    pub(crate) fn tool_entry_contains(
        entry: &StreamEntry,
        card: &Entity<ToolCard>,
        cx: &App,
    ) -> bool {
        match entry {
            StreamEntry::RunActivity { group } => {
                group.read(cx).process_in_details() && group.read(cx).contains_tool(card, cx)
            }
            StreamEntry::RunActivitySegment { group, segment } => {
                !group.read(cx).process_in_details()
                    && group
                        .read(cx)
                        .segment_children(*segment)
                        .iter()
                        .any(|child| match child {
                            RunActivityChild::Tool(owned) => owned == card,
                            RunActivityChild::ToolGroup(group) => group.read(cx).contains(card),
                            _ => false,
                        })
            }
            StreamEntry::Tool { card: owned } => owned == card,
            StreamEntry::ToolGroup { group } => group.read(cx).contains(card),
            _ => false,
        }
    }

    pub(crate) fn tool_entry_index(&self, card: &Entity<ToolCard>, cx: &App) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| {
                matches!(entry, StreamEntry::RunActivity { .. })
                    && Self::tool_entry_contains(entry, card, cx)
            })
            .or_else(|| {
                self.entries
                    .iter()
                    .position(|entry| Self::tool_entry_contains(entry, card, cx))
            })
    }

    fn observe_tool_card(&mut self, card: &Entity<ToolCard>, cx: &mut Context<Self>) {
        cx.subscribe(
            card,
            |this, _, event: &vega_conversation::types::SubagentOpenRequested, cx| {
                if event.source_thread_id == this.thread.id {
                    cx.emit(event.clone());
                }
            },
        )
        .detach();
        cx.observe(card, |this, card, cx| {
            let index = this.tool_entry_index(&card, cx);
            this.invalidate_item(index);
            cx.notify();
        })
        .detach();
    }

    fn observe_tool_group(&mut self, group: &Entity<ToolActivityGroup>, cx: &mut Context<Self>) {
        cx.observe(group, |this, group, cx| {
            let index = this.entry_index_where(|entry| match entry {
                StreamEntry::ToolGroup { group: owned } => owned == &group,
                StreamEntry::RunActivity { group: activity } => {
                    activity.read(cx).contains_tool_group(&group)
                }
                _ => false,
            });
            this.invalidate_item(index);
            cx.notify();
        })
        .detach();
    }

    fn new_tool_group(
        &mut self,
        first: Entity<ToolCard>,
        second: Entity<ToolCard>,
        cx: &mut Context<Self>,
    ) -> Entity<ToolActivityGroup> {
        let group = cx.new(|_| ToolActivityGroup::new(first, second));
        self.observe_tool_group(&group, cx);
        group
    }

    fn append_hydrated_tool(
        &mut self,
        entries: &mut Vec<StreamEntry>,
        card: Entity<ToolCard>,
        cx: &mut Context<Self>,
    ) {
        enum Tail {
            Single(Entity<ToolCard>),
            Group(Entity<ToolActivityGroup>),
            Boundary,
        }
        let tail = match entries.last() {
            Some(StreamEntry::Tool { card }) if !card.read(cx).is_subagent_invocation() => {
                Tail::Single(card.clone())
            }
            Some(StreamEntry::ToolGroup { group }) => Tail::Group(group.clone()),
            _ => Tail::Boundary,
        };
        match tail {
            Tail::Single(first) => {
                let group = self.new_tool_group(first, card, cx);
                if let Some(entry) = entries.last_mut() {
                    *entry = StreamEntry::ToolGroup { group };
                }
            }
            Tail::Group(group) => group.update(cx, |group, cx| group.append(card, cx)),
            Tail::Boundary => entries.push(StreamEntry::Tool { card }),
        }
    }

    fn append_live_tool(&mut self, card: Entity<ToolCard>, proposed: bool, cx: &mut Context<Self>) {
        if card.read(cx).is_subagent_invocation() {
            if let Some((_, group)) = self.active_run_activity.clone() {
                group.update(cx, |group, cx| group.attach_tool(card.clone(), cx));
            }
            self.active_run_activity_segment = None;
            let index = self.entries.len();
            self.entries.push(StreamEntry::Tool { card });
            self.list_append(index);
            return;
        }
        let active_message_id = self
            .active_agent_message
            .as_ref()
            .map(|(message_id, _)| message_id.clone());
        if let Some(message_id) = active_message_id
            && let Some((group, segment)) = self.ensure_live_run_activity_segment(&message_id, cx)
        {
            group.update(cx, |group, cx| {
                group.append_tool(segment, card, proposed, cx)
            });
            return;
        }
        enum Tail {
            Single(Entity<ToolCard>),
            Group(Entity<ToolActivityGroup>),
            Boundary,
        }
        let index = self.entries.len().saturating_sub(1);
        let tail = match self.entries.last() {
            Some(StreamEntry::Tool { card }) if !card.read(cx).is_subagent_invocation() => {
                Tail::Single(card.clone())
            }
            Some(StreamEntry::ToolGroup { group }) => Tail::Group(group.clone()),
            _ => Tail::Boundary,
        };
        match tail {
            Tail::Single(first) => {
                let expanded = first.read(cx).is_expanded();
                let group = self.new_tool_group(first, card, cx);
                group.update(cx, |group, cx| group.set_expanded(expanded, cx));
                if let Some(entry) = self.entries.last_mut() {
                    *entry = StreamEntry::ToolGroup { group };
                }
                self.invalidate_item(Some(index));
            }
            Tail::Group(group) => {
                group.update(cx, |group, cx| group.append(card, cx));
                self.invalidate_item(Some(index));
            }
            Tail::Boundary => {
                let previous_len = self.entries.len();
                self.entries.push(StreamEntry::Tool { card: card.clone() });
                self.list_append(previous_len);
                card.update(cx, |card, cx| card.set_expanded(false, cx));
            }
        }
    }

    fn tool_entry_cards(entry: &StreamEntry, cx: &App) -> Option<(Vec<Entity<ToolCard>>, bool)> {
        match entry {
            StreamEntry::Tool { card } if !card.read(cx).is_subagent_invocation() => {
                Some((vec![card.clone()], false))
            }
            StreamEntry::ToolGroup { group } => {
                let group = group.read(cx);
                Some((group.children(), group.expanded()))
            }
            _ => None,
        }
    }

    /// A permission card is transient. If calls landed on both sides while it
    /// was visible, removing it restores the same adjacency as if it had
    /// never been a durable timeline boundary.
    fn merge_tool_entries_around(&mut self, right_index: usize, cx: &mut Context<Self>) {
        let Some(left_index) = right_index.checked_sub(1) else {
            return;
        };
        let Some((left_cards, left_expanded)) = self
            .entries
            .get(left_index)
            .and_then(|entry| Self::tool_entry_cards(entry, cx))
        else {
            return;
        };
        let Some((right_cards, right_expanded)) = self
            .entries
            .get(right_index)
            .and_then(|entry| Self::tool_entry_cards(entry, cx))
        else {
            return;
        };

        let existing_left_group = match self.entries.get(left_index) {
            Some(StreamEntry::ToolGroup { group }) => Some(group.clone()),
            _ => None,
        };
        if let Some(group) = existing_left_group {
            group.update(cx, |group, cx| {
                group.extend(right_cards, right_expanded, cx)
            });
        } else {
            let mut children = left_cards;
            children.extend(right_cards);
            let group = cx.new(|_| {
                ToolActivityGroup::from_children(children, left_expanded || right_expanded)
            });
            self.observe_tool_group(&group, cx);
            self.entries[left_index] = StreamEntry::ToolGroup { group };
        }
        self.entries.remove(right_index);
        self.list_remove(right_index);
        self.shift_entry_indices(right_index, 1);
        self.invalidate_item(Some(left_index));
    }

    pub(crate) fn push_tool_card(
        &mut self,
        call_id: String,
        card: ToolCard,
        cx: &mut Context<Self>,
    ) {
        if self.tool_cards.contains_key(&call_id) {
            return;
        }
        let card = cx.new(|_| card);
        self.observe_tool_card(&card, cx);
        self.append_live_tool(card.clone(), false, cx);
        self.tool_cards.insert(call_id, card);
        cx.notify();
    }
}
