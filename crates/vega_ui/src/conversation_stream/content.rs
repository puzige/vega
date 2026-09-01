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
                HistoryEntry::UserText { content, .. } => {
                    let block_id = self.user_block_seq;
                    self.user_block_seq += 1;
                    hydrated.push(StreamEntry::User {
                        lines: user_message_lines(block_id, &content),
                    });
                }
                HistoryEntry::AssistantText { content, .. } => {
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
                        stream: Box::new(stream),
                        model,
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
                    cx.observe(&card, |this, card, cx| {
                        let index = this.entry_index_where(
                            |entry| matches!(entry, StreamEntry::Tool { card: owned } if owned == &card),
                        );
                        this.invalidate_item(index);
                        cx.notify();
                    })
                    .detach();
                    self.tool_cards.insert(call_id, card.clone());
                    hydrated.push(StreamEntry::Tool { card });
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
            if let Some((_, index)) = &mut self.active_agent_message {
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

    pub fn has_pending_plan_review(&self, cx: &App) -> bool {
        self.plan_cards
            .values()
            .any(|card| card.read(cx).status() == vega_conversation::types::PlanStatus::Pending)
    }

    pub fn set_trusted_action_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.trusted_action_busy = busy;
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
        self.remove_active_permission(cx);
    }

    pub(crate) fn install_pending_permission(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.permission_queue.take_pending() else {
            return;
        };
        if cx
            .try_global::<SettingsOpen>()
            .is_some_and(|settings| settings.0)
        {
            drop(pending);
            return;
        }
        let Some(request) = pending.request() else {
            drop(pending);
            return;
        };
        let call_id = request.call_id.clone();
        let identity_matches = self.tool_cards.get(&call_id).is_some_and(|card| {
            card.read(cx)
                .permission_identity()
                .is_some_and(|(tool, target)| {
                    tool == request.tool && target == request.display_target
                })
        });
        if !identity_matches {
            drop(pending);
            if let Some(card) = self.tool_cards.get(&call_id) {
                let card = card.clone();
                card.update(cx, ToolCard::fail_corrupt);
                self.invalidate_tool_card(&card);
            } else {
                self.push_corrupt_tool(call_id, cx);
            }
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
        // The prompt must be visible immediately: re-engage native tail
        // follow (which also scrolls to the end on the next layout).
        self.list.set_follow_mode(gpui::FollowMode::Tail);
        cx.notify();
    }

    pub(crate) fn remove_active_permission(&mut self, cx: &mut Context<Self>) {
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
                let entry_index = self.entries.len();
                self.entries.push(StreamEntry::Assistant {
                    stream: Box::new(MarkdownStream::new()),
                    model: StreamModel::default(),
                });
                self.list_append(entry_index);
                self.active_agent_message = Some((message_id, entry_index));
                cx.notify();
            }
            ConversationEvent::TextDelta { message_id, delta } => {
                let Some((active_id, entry_index)) = self.active_agent_message.as_ref() else {
                    return;
                };
                if active_id != &message_id {
                    return;
                }
                if let Some(StreamEntry::Assistant { stream, .. }) =
                    self.entries.get_mut(*entry_index)
                {
                    stream.append(&delta);
                    cx.notify();
                }
            }
            ConversationEvent::ThinkingDelta { .. } | ConversationEvent::UsageUpdated { .. } => {}
            ConversationEvent::ToolCallProposed { call } => {
                if let Some(existing) = self.tool_cards.get(&call.id) {
                    existing.update(cx, |card, cx| {
                        if !card.matches_call(&call) {
                            card.fail_corrupt(cx);
                        }
                    });
                    return;
                }
                let call_id = call.id.clone();
                let card = cx.new(|_| ToolCard::proposed(&call));
                cx.observe(&card, |this, card, cx| {
                    let index = this
                        .entry_index_where(|entry| matches!(entry, StreamEntry::Tool { card: owned } if owned == &card));
                    this.invalidate_item(index);
                    cx.notify();
                })
                .detach();
                let index = self.entries.len();
                self.entries.push(StreamEntry::Tool { card: card.clone() });
                self.list_append(index);
                self.tool_cards.insert(call_id, card);
                cx.notify();
            }
            ConversationEvent::ToolCallApproved { call_id, approval } => {
                if let Some(card) = self.tool_cards.get(&call_id) {
                    let card = card.clone();
                    card.update(cx, |card, cx| {
                        card.apply_approved(approval);
                        cx.notify();
                    });
                    self.invalidate_tool_card(&card);
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
                if self.active_permission.is_some() {
                    self.timeout_permission(cx);
                }
                if let Some(card) = self.tool_cards.get(&call_id) {
                    let card = card.clone();
                    card.update(cx, |card, cx| {
                        card.apply_finished(&result);
                        cx.notify();
                    });
                    self.invalidate_tool_card(&card);
                } else {
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
            ConversationEvent::MessageFinished { message_id, .. }
            | ConversationEvent::Interrupted { message_id } => {
                self.finish_agent_message(&message_id, cx);
            }
            ConversationEvent::Error { message_id, .. } => {
                if let Some(message_id) = message_id {
                    self.finish_agent_message(&message_id, cx);
                } else {
                    self.timeout_permission(cx);
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
        // finish() 丢弃 pending 并把尾块冻结为 committed（version bump）：
        // 这是从 mutable tail 摘除前的最后一次显式失效（C4 白名单），必须
        // 在本帧内完成最终物化——否则批量 ingress 末批 [delta…, Finished]
        // 的尾部 delta 永不上屏、终块永无 committed 高亮。
        if let Some(StreamEntry::Assistant { stream, model }) = self.entries.get_mut(*entry_index) {
            stream.finish();
            let snapshot = stream.snapshot();
            model.sync(&snapshot, &self.counters);
        }
        self.invalidate_item(Some(*entry_index));
        self.last_finished_agent_message = self.active_agent_message.take();
        self.timeout_permission(cx);
        cx.notify();
    }

    pub(crate) fn push_corrupt_tool(&mut self, call_id: String, cx: &mut Context<Self>) {
        self.push_tool_card(call_id, ToolCard::corrupt(), cx);
    }

    /// Marks an existing tool card item for re-measurement (status/approval/
    /// result/expansion changes may change its height; the C4 explicit
    /// invalidation whitelist).
    pub(crate) fn invalidate_tool_card(&mut self, card: &Entity<ToolCard>) {
        let index = self.entry_index_where(
            |entry| matches!(entry, StreamEntry::Tool { card: owned } if owned == card),
        );
        self.invalidate_item(index);
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
        cx.observe(&card, |this, card, cx| {
            let index = this.entry_index_where(
                |entry| matches!(entry, StreamEntry::Tool { card: owned } if owned == &card),
            );
            this.invalidate_item(index);
            cx.notify();
        })
        .detach();
        let index = self.entries.len();
        self.entries.push(StreamEntry::Tool { card: card.clone() });
        self.list_append(index);
        self.tool_cards.insert(call_id, card);
        cx.notify();
    }
}
