use super::*;

impl ConversationStream {
    /// Shows a restart-safe, non-executing projection for a durable approved
    /// instruction. Merely opening a thread must never read Keychain or start
    /// a provider request.
    pub fn apply_approved_not_started(&mut self, cx: &mut Context<Self>) {
        self.approved_not_started = true;
        self.controller_error = Some("已批准计划尚未执行；恢复入口待补充".into());
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
        let card = cx.new(|cx| PlanCard::new(plan, cx));
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
        if let Some(entry_index) = replace_index {
            self.last_finished_agent_message = None;
            // Replaces an existing entry in place: same index, content may
            // change height → explicit invalidation (C4 whitelist).
            if let Some(entry) = self.entries.get_mut(entry_index) {
                *entry = StreamEntry::Plan { card: card.clone() };
                self.invalidate_item(Some(entry_index));
            } else {
                let index = self.entries.len();
                self.entries.push(StreamEntry::Plan { card: card.clone() });
                self.list_append(index);
            }
        } else {
            let index = self.entries.len();
            self.entries.push(StreamEntry::Plan { card: card.clone() });
            self.list_append(index);
        }
        self.plan_cards.insert(id, card);
        cx.notify();
    }

    /// Appends the read-only per-task cost summary card of one finished
    /// assistant message (S7-T40/C4). The typed projection arrives from the
    /// app layer via `vega_conversation::summary`; the stream never queries
    /// SQLite and never computes a cost formula. Applications are keyed by
    /// the assistant message id and first-wins: duplicates and later stale
    /// projections of the same task are ignored, and projections of foreign
    /// threads are dropped.
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
        self.summary_cards.insert(message_id, card);
        cx.notify();
    }

    /// Applies one typed history page (S8-T45/C7). The first page fills the
    /// empty route-open stream; later pages PREPEND above the loaded history
    /// when the user scrolls up. Pages are converted through the typed
    /// projections only — no SQLite ever runs in this crate — and duplicate
    /// durable cards (summary/plan/tool) reconcile first-wins. Prepends keep
    /// the viewport anchored at the page boundary (uniform row heights make
    /// the pixel adjustment exact), and in-flight entry indices shift so a
    /// streaming run keeps writing into its own turn.
    pub fn apply_history_page(&mut self, page: HistoryPage, cx: &mut Context<Self>) {
        let mut hydrated: Vec<StreamEntry> = Vec::new();
        for entry in page.entries {
            match entry {
                HistoryEntry::UserImages { images, .. } => {
                    hydrated.push(StreamEntry::UserImages {
                        images: self.history_image_previews(images, cx),
                    });
                }
                HistoryEntry::UserText { content, .. } => {
                    let block_id = self.user_block_seq;
                    self.user_block_seq += 1;
                    hydrated.push(StreamEntry::User {
                        copy: MessageCopy::new(&content),
                        lines: user_message_lines(block_id, &content),
                    });
                }
                HistoryEntry::AssistantText {
                    content, status, ..
                } => {
                    // Durable markdown is complete; one append + finish is the
                    // whole turn. Empty (killed-before-first-delta) turns
                    // materialize zero content, like an empty live stream. The
                    // model syncs immediately so the prepended item renders
                    // its full natural height on the first frame.
                    let mut stream = MarkdownStream::new();
                    stream.append(&content);
                    stream.finish();
                    let mut model = StreamModel::default();
                    model.sync(&stream.snapshot(), &self.counters);
                    hydrated.push(StreamEntry::Assistant {
                        copy: MessageCopy::new(&content),
                        stream: Box::new(stream),
                        model,
                        // The original provider body is intentionally not
                        // durable. A failed row still needs a visible,
                        // truthful fallback after route reopen/restart.
                        failure: (status == vega_conversation::history::AssistantStatus::Failed)
                            .then_some(RunFailureKind::Persisted),
                    });
                }
                HistoryEntry::Plan { plan, .. } => {
                    // Same foreign-thread fence as the live `apply_plan` path;
                    // duplicate durable plans reconcile first-wins.
                    if plan.thread_id != self.thread.id || self.plan_cards.contains_key(&plan.id) {
                        continue;
                    }
                    let id = plan.id.clone();
                    let card = cx.new(|cx| PlanCard::new(plan, cx));
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
                    self.plan_cards.insert(id, card.clone());
                    hydrated.push(StreamEntry::Plan { card });
                }
                HistoryEntry::Summary { summary, .. } => {
                    if self.summary_cards.contains_key(&summary.message_id) {
                        continue;
                    }
                    let message_id = summary.message_id.clone();
                    let card = cx.new(|_| SummaryCard::new(summary));
                    self.summary_cards.insert(message_id, card.clone());
                    hydrated.push(StreamEntry::Summary { card });
                }
                HistoryEntry::Tool {
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
                    self.tool_cards.insert(call_id, card.clone());
                    self.append_hydrated_tool(&mut hydrated, card, cx);
                }
                HistoryEntry::SkillActivation { activation, .. } => {
                    hydrated.push(StreamEntry::SkillActivation { activation });
                }
            }
        }
        let prepended_entries = hydrated.len();
        let mut entries = hydrated;
        entries.append(&mut self.entries);
        self.entries = entries;
        self.list_prepend(prepended_entries);
        if prepended_entries > 0 {
            // Entry indices booked before the prepend must follow their turns.
            if let Some((_, index)) = &mut self.active_agent_message
                && *index != usize::MAX
            {
                *index += prepended_entries;
            }
            if let Some((_, index)) = &mut self.last_finished_agent_message {
                *index += prepended_entries;
            }
        }
        self.hydration.older_cursor = page.older_cursor;
        self.hydration.loading = false;
        self.hydration.paused = false;
        // Page-boundary anchor: the `splice` inside `list_prepend` shifts the
        // logical scroll top by the prepended count while keeping the pixel
        // offset into the scroll-top item, so while detached the content the
        // user was reading stays put (<1px by construction). While pinned to
        // the tail, native Tail follow keeps the viewport at the bottom.
        cx.notify();
    }

    /// Releases the in-flight page slot after a failed load. Auto-retry waits
    /// until the viewport leaves the top edge again (failure pause), so a
    /// persistently broken store cannot turn a pinned scroll into a spin loop.
    pub fn apply_history_load_failed(&mut self, cx: &mut Context<Self>) {
        self.hydration.loading = false;
        self.hydration.paused = true;
        cx.notify();
    }

    /// The cursor to request for scroll-up hydration, or `None` when no page
    /// may be requested: exhausted history, a page already in flight, a
    /// paused failure, or the viewport away from the top edge.
    pub(crate) fn history_page_request(&self, at_top: bool) -> Option<i64> {
        hydration_request(self.hydration, at_top)
    }

    /// Whether the list is scrolled to (within epsilon of) its top edge
    /// (native top-of-list offset; item-index 0 with no in-item offset).
    pub(crate) fn scroll_at_top(&self) -> bool {
        let top = self.list.logical_scroll_top();
        top.item_ix == 0 && f32::from(top.offset_in_item) <= ANCHOR_EPSILON_PX
    }

    /// Whether the failure pause is still armed; leaving the top edge
    /// re-arms scroll-up hydration.
    pub(crate) fn hydration_pause_is_stale(&self, at_top: bool) -> bool {
        self.hydration.paused && !at_top
    }

    /// Hook passed to the conversation runner for this visible stream.
    pub fn permission_queue(&self) -> PermissionQueue {
        self.permission_queue.clone()
    }

    /// The authoritative run mode this composer projects. Read by the
    /// application acceptance harness to prove a `+`-menu selection landed on
    /// the durable thread rather than only in local UI state.
    #[doc(hidden)]
    pub fn thread_mode(&self) -> ThreadMode {
        self.thread.mode
    }

    /// The authoritative permission mode this composer projects. Same seam as
    /// [`Self::thread_mode`].
    #[doc(hidden)]
    pub fn thread_permission_mode(&self) -> PermissionMode {
        self.thread.permission_mode
    }

    /// The thinking choice this composer projects (`provider_default`,
    /// `disabled`, or a declared effort). R57 P3 acceptance reads it to prove
    /// a slider selection became the composer's authoritative state rather
    /// than only moving the knob.
    #[doc(hidden)]
    pub fn thinking_choice(&self) -> &str {
        &self.composer_defaults.thinking
    }

    /// The exact reasoning capability projection this composer currently
    /// carries, or `None` when the model has no profile. R57 P3 acceptance
    /// reads it to prove the slider's tier list is the model's own
    /// `ReasoningProfile.efforts` rather than a fixed ladder.
    #[doc(hidden)]
    pub fn reasoning_profile(&self) -> Option<&ReasoningProfileProjection> {
        self.composer_defaults.reasoning.as_ref()
    }

    /// The mounted tier slider (R57 P3). The application acceptance harness
    /// reads its live tier and dot count to prove the composer's projection
    /// reached the real component.
    #[doc(hidden)]
    pub fn thinking_slider(&self) -> Entity<ThinkingSlider> {
        self.thinking_slider.clone()
    }

    /// Which model-picker level is mounted, if any (R59). The application
    /// acceptance harness reads it to prove the two levels are mutually
    /// exclusive and that a tier-only round trip leaves the slider mounted.
    #[doc(hidden)]
    pub fn model_picker_level(&self) -> ModelPickerLevel {
        self.model_picker_level
    }

    /// Number of configured model options the selector offers (#60 R1). R57 P3
    /// acceptance waits on it because the popup (and therefore the slider it
    /// hosts) only opens once the app published the catalog projection.
    #[doc(hidden)]
    pub fn model_options_len(&self) -> usize {
        self.model_options.len()
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

    /// Number of hydrated durable entries the stream currently carries
    /// (S8-T45/C7 observability for the app-layer fence tests; the row
    /// layout itself stays crate-private).
    pub fn hydrated_entry_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    StreamEntry::User { .. }
                        | StreamEntry::Assistant { .. }
                        | StreamEntry::Tool { .. }
                        | StreamEntry::ToolGroup { .. }
                        | StreamEntry::Plan { .. }
                        | StreamEntry::Summary { .. }
                )
            })
            .count()
    }

    /// Whether a hydration page may still be requested for scroll-up
    /// (exposes the pure gate to the app layer's fence tests).
    pub fn hydration_cursor(&self) -> Option<i64> {
        self.hydration.older_cursor
    }

    pub fn has_pending_permission(&self) -> bool {
        self.permission_queue.has_pending()
    }

    /// Content-free presence check for the production app-entry acceptance
    /// harness. A queue latch may be pending before its matching proposal is
    /// ingressed, so callers that need to prove a visible card must check this
    /// separately.
    #[doc(hidden)]
    pub fn has_active_permission_card(&self) -> bool {
        self.active_permission.is_some()
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
        let Some(card) = self.tool_cards.get(&call_id).cloned() else {
            self.deferred_permission = Some(pending);
            return;
        };
        let identity_matches = card
            .read(cx)
            .permission_identity()
            .is_some_and(|(tool, target)| tool == request.tool && target == request.display_target);
        if !identity_matches {
            drop(pending);
            card.update(cx, ToolCard::fail_corrupt);
            self.invalidate_tool_card(&card, cx);
            return;
        }
        self.remove_active_permission(cx);
        let Some((request, lease)) = pending.into_parts() else {
            return;
        };
        let card = cx.new(|cx| PermissionCard::new(&request, lease, cx));
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
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::Permission { card } if card == &active))
        {
            self.entries.remove(index);
            self.list_remove(index);
            self.merge_tool_entries_around(index, cx);
        }
        cx.notify();
    }

    /// Total row count across all entries.
    pub(crate) fn total_rows(&self, cx: &App) -> usize {
        self.entries.iter().map(|entry| entry.row_count(cx)).sum()
    }

    /// Applies an already-durable shared lifecycle event. The UI never reads
    /// SQLite and never consumes runtime-local events.
    pub fn apply_event(&mut self, event: ConversationEvent, cx: &mut Context<Self>) {
        // S7-T39: the bounded meter projection consumes the same accepted
        // events (fence below) before ownership moves into the render path.
        self.feed_meter(&event, cx);
        match event {
            ConversationEvent::MessageStarted { message_id, .. } => {
                if self.active_agent_message.is_some() {
                    self.apply_controller_error(cx);
                    return;
                }
                self.last_finished_agent_message = None;
                self.active_thinking = None;
                self.active_skills.clear();
                let entry_index = self.entries.len();
                self.entries.push(StreamEntry::Assistant {
                    copy: MessageCopy::default(),
                    stream: Box::new(MarkdownStream::new()),
                    model: StreamModel::default(),
                    failure: None,
                });
                self.list_append(entry_index);
                self.active_agent_message = Some((message_id, entry_index));
                self.active_segment_has_text = false;
                cx.notify();
            }
            ConversationEvent::TextDelta { message_id, delta } => {
                let Some((active_id, entry_index)) = self.active_agent_message.as_ref() else {
                    return;
                };
                if active_id != &message_id || delta.is_empty() {
                    return;
                }
                // Ends the `active_agent_message` borrow so the collapse below
                // can take `&mut self`.
                let entry_index = *entry_index;
                // #151 R151-4: the first nonempty delta of a new text segment
                // is newer content, so the current activity unit steps down.
                // Only the segment's first delta pays this scan — never the
                // per-token path.
                if !self.active_segment_has_text {
                    self.collapse_current_activity(cx);
                }
                self.active_thinking = None;
                let entry_index = if entry_index == usize::MAX {
                    let index = self.entries.len();
                    self.entries.push(StreamEntry::Assistant {
                        copy: MessageCopy::default(),
                        stream: Box::new(MarkdownStream::new()),
                        model: StreamModel::default(),
                        failure: None,
                    });
                    self.list_append(index);
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
                    self.active_segment_has_text = true;
                    cx.notify();
                }
            }
            ConversationEvent::ThinkingDelta { message_id, delta } => {
                self.append_thinking(&message_id, &delta, cx);
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
                    cx.notify();
                }
            }
            ConversationEvent::UsageUpdated { .. }
            | ConversationEvent::ContextCompactionUsageUpdated { .. }
            | ConversationEvent::ContextCompactionStatus { .. }
            | ConversationEvent::ContextAccounting { .. } => {}
            ConversationEvent::ToolCallProposed { call } => {
                if let Some(existing) = self.tool_cards.get(&call.id) {
                    existing.update(cx, |card, cx| {
                        if !card.matches_call(&call) {
                            card.fail_corrupt(cx);
                        }
                    });
                    self.install_pending_permission(cx);
                    return;
                }
                self.close_active_segment_before_tool();
                let call_id = call.id.clone();
                let card = cx.new(|_| ToolCard::proposed(&call));
                self.observe_tool_card(&card, cx);
                self.append_live_tool(card.clone(), cx);
                self.tool_cards.insert(call_id, card);
                self.install_pending_permission(cx);
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
                    card.update(cx, ToolCard::apply_running);
                    self.invalidate_tool_card(&card, cx);
                } else {
                    self.push_corrupt_tool(call_id, cx);
                }
            }
            ConversationEvent::ToolCallOutput { .. } => {
                // T26 emits a post-commit bounded output immediately before
                // Finished. Ignore it here: write/edit chunks can contain the
                // strict success JSON (including the opaque checkpoint ref),
                // and terminal projection is the sole card decode boundary.
            }
            ConversationEvent::ToolCallFinished { call_id, result } => {
                self.timeout_permission_for_call(&call_id, cx);
                if let Some(card) = self.tool_cards.get(&call_id) {
                    let card = card.clone();
                    card.update(cx, |card, cx| {
                        card.apply_finished(&result);
                        cx.notify();
                    });
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
            ConversationEvent::MessageFinished { message_id, .. } => {
                self.record_composer_terminal(&message_id, false);
                self.finish_agent_message(&message_id, cx);
            }
            ConversationEvent::Interrupted { message_id } => {
                self.record_composer_terminal(&message_id, true);
                self.finish_agent_message(&message_id, cx);
            }
            ConversationEvent::Error { message_id, error } => {
                let failure = RunFailureKind::from_runtime(&error);
                if let Some(message_id) = message_id {
                    // A stale event must not annotate the currently active
                    // assistant turn (nor overwrite a newer composer error).
                    if let Some((active_id, entry_index)) = self.active_agent_message.as_ref()
                        && active_id == &message_id
                        && *entry_index == usize::MAX
                    {
                        self.append_empty_terminal_segment();
                    }
                    if let Some((active_id, entry_index)) = self.active_agent_message.as_ref()
                        && active_id == &message_id
                        && let Some(StreamEntry::Assistant {
                            failure: active_failure,
                            ..
                        }) = self.entries.get_mut(*entry_index)
                    {
                        *active_failure = Some(failure);
                        self.controller_error = Some(failure.message());
                    }
                    self.record_composer_terminal(&message_id, false);
                    self.finish_agent_message(&message_id, cx);
                } else {
                    self.controller_error = Some(failure.message());
                    if let Some((message_id, _)) = self.active_agent_message.clone() {
                        self.finish_agent_message(&message_id, cx);
                    }
                    self.timeout_permission(cx);
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn finish_agent_message(&mut self, message_id: &str, cx: &mut Context<Self>) {
        let Some((active_id, entry_index)) = self.active_agent_message.as_ref() else {
            return;
        };
        if active_id != message_id {
            return;
        }
        self.active_thinking = None;
        self.active_skills.clear();
        // finish() 丢弃 pending 并把尾块冻结为 committed（version bump）：
        // 这是从 mutable tail 摘除前的最后一次显式失效（C4 白名单），必须
        // 在本帧内完成最终物化——否则批量 ingress 末批 [delta…, Finished]
        // 的尾部 delta 永不上屏、终块永无 committed 高亮。
        if let Some(StreamEntry::Assistant { stream, model, .. }) =
            self.entries.get_mut(*entry_index)
        {
            stream.finish();
            let snapshot = stream.snapshot();
            model.sync(&snapshot, &self.counters);
        }
        self.invalidate_item(Some(*entry_index));
        self.last_finished_agent_message = self
            .active_agent_message
            .take()
            .filter(|(_, index)| *index != usize::MAX);
        self.active_segment_has_text = false;
        self.timeout_permission(cx);
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
        if index == usize::MAX {
            return;
        }
        if self.active_segment_has_text {
            if let Some(StreamEntry::Assistant { stream, model, .. }) = self.entries.get_mut(index)
            {
                stream.finish();
                model.sync(&stream.snapshot(), &self.counters);
            }
            self.invalidate_item(Some(index));
        } else {
            self.entries.remove(index);
            self.list_remove(index);
        }
        if let Some((_, active_index)) = &mut self.active_agent_message {
            *active_index = usize::MAX;
        }
        self.active_segment_has_text = false;
    }

    /// #151 R151-2: at most one auto-expanded activity unit exists at a time.
    /// The newest thinking block / tool row / tool group is the current unit;
    /// creating a newer one steps this one down. Manual expansion of *older*
    /// units is deliberately untouched — they are not the newest, so the user
    /// may keep them open alongside the current one.
    pub(crate) fn collapse_current_activity(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.entries.iter().rposition(|entry| {
            matches!(
                entry,
                StreamEntry::Thinking { .. }
                    | StreamEntry::Tool { .. }
                    | StreamEntry::ToolGroup { .. }
            )
        }) else {
            return;
        };
        match &self.entries[index] {
            StreamEntry::Thinking { card } => {
                card.update(cx, |card, cx| card.set_expanded(false, cx));
            }
            StreamEntry::Tool { card } => {
                card.update(cx, |card, cx| card.set_expanded(false, cx));
            }
            StreamEntry::ToolGroup { group } => {
                group.update(cx, |group, cx| group.set_expanded(false, cx));
            }
            _ => return,
        }
        self.invalidate_item(Some(index));
    }

    fn append_empty_terminal_segment(&mut self) {
        let index = self.entries.len();
        self.entries.push(StreamEntry::Assistant {
            copy: MessageCopy::default(),
            stream: Box::new(MarkdownStream::new()),
            model: StreamModel::default(),
            failure: None,
        });
        self.list_append(index);
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
            StreamEntry::Tool { card: owned } => owned == card,
            StreamEntry::ToolGroup { group } => group.read(cx).contains(card),
            _ => false,
        }
    }

    pub(crate) fn tool_entry_index(&self, card: &Entity<ToolCard>, cx: &App) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| Self::tool_entry_contains(entry, card, cx))
    }

    fn observe_tool_card(&mut self, card: &Entity<ToolCard>, cx: &mut Context<Self>) {
        cx.observe(card, |this, card, cx| {
            let index = this.tool_entry_index(&card, cx);
            this.invalidate_item(index);
            cx.notify();
        })
        .detach();
    }

    fn observe_tool_group(&mut self, group: &Entity<ToolActivityGroup>, cx: &mut Context<Self>) {
        cx.observe(group, |this, group, cx| {
            let index = this.entry_index_where(
                |entry| matches!(entry, StreamEntry::ToolGroup { group: owned } if owned == &group),
            );
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
            Some(StreamEntry::Tool { card }) => Tail::Single(card.clone()),
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

    fn append_live_tool(&mut self, card: Entity<ToolCard>, cx: &mut Context<Self>) {
        enum Tail {
            Single(Entity<ToolCard>),
            Group(Entity<ToolActivityGroup>),
            Boundary,
        }
        let index = self.entries.len().saturating_sub(1);
        let tail = match self.entries.last() {
            Some(StreamEntry::Tool { card }) => Tail::Single(card.clone()),
            Some(StreamEntry::ToolGroup { group }) => Tail::Group(group.clone()),
            _ => Tail::Boundary,
        };
        match tail {
            Tail::Single(first) => {
                // #151 R151-3: the newest unit becomes an expanded group. The
                // row it replaces was the previous current unit and has just
                // been superseded, so its own detail steps down into a compact
                // child row; the group itself is what opens.
                first.update(cx, |card, cx| card.set_expanded(false, cx));
                let group = self.new_tool_group(first, card, cx);
                group.update(cx, |group, cx| group.set_expanded(true, cx));
                if let Some(entry) = self.entries.last_mut() {
                    *entry = StreamEntry::ToolGroup { group };
                }
                self.invalidate_item(Some(index));
            }
            Tail::Group(group) => {
                // Appending to the current group keeps it the newest unit, so
                // it stays (or returns to) expanded; child detail untouched.
                group.update(cx, |group, cx| {
                    group.append(card, cx);
                    group.set_expanded(true, cx);
                });
                self.invalidate_item(Some(index));
            }
            Tail::Boundary => {
                // #151 R151-1/R151-2: a new single-call unit supersedes the
                // previous current unit and opens itself.
                self.collapse_current_activity(cx);
                let previous_len = self.entries.len();
                self.entries.push(StreamEntry::Tool { card: card.clone() });
                self.list_append(previous_len);
                card.update(cx, |card, cx| card.set_expanded(true, cx));
            }
        }
    }

    fn tool_entry_cards(entry: &StreamEntry, cx: &App) -> Option<(Vec<Entity<ToolCard>>, bool)> {
        match entry {
            StreamEntry::Tool { card } => Some((vec![card.clone()], false)),
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
        self.append_live_tool(card.clone(), cx);
        self.tool_cards.insert(call_id, card);
        cx.notify();
    }
}
