use super::*;

impl ConversationStream {
    /// Inserts one artifact immediately after the exact tool entry. Identical
    /// duplicates reconcile in place; conflicting ids fail the existing card
    /// closed and never insert an unrelated entry.
    pub fn apply_artifact_card(
        &mut self,
        call_id: &str,
        card: Entity<ArtifactCard>,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(existing) = self.artifact_cards.get(call_id) {
            let incoming = card.read(cx).projection().clone();
            if existing.read(cx).id() == incoming.id {
                existing.update(cx, |card, cx| {
                    let _ = card.apply_metadata(incoming, cx);
                });
                return true;
            }
            existing.update(cx, ArtifactCard::fail_corrupt);
            return false;
        }
        let Some(tool) = self.tool_cards.get(call_id) else {
            return false;
        };
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::Tool { card } if card == tool))
        else {
            return false;
        };
        if matches!(
            self.entries.get(index + 1),
            Some(StreamEntry::Artifact { .. })
        ) {
            return false;
        }
        cx.observe(&card, |this, card, cx| {
            let index = this.entry_index_where(
                |entry| matches!(entry, StreamEntry::Artifact { card: owned } if owned == &card),
            );
            this.invalidate_item(index);
            cx.notify();
        })
        .detach();
        self.entries
            .insert(index + 1, StreamEntry::Artifact { card: card.clone() });
        self.list_insert(index + 1);
        self.artifact_cards.insert(call_id.to_owned(), card);
        cx.notify();
        true
    }

    /// Content-free structural check used by the application integration
    /// harness to prove exact Tool -> Artifact adjacency.
    #[doc(hidden)]
    pub fn artifact_card_is_adjacent(&self, call_id: &str) -> bool {
        let Some(tool) = self.tool_cards.get(call_id) else {
            return false;
        };
        let Some(artifact) = self.artifact_cards.get(call_id) else {
            return false;
        };
        self.entries.windows(2).any(|entries| {
            matches!(&entries[0], StreamEntry::Tool { card } if card == tool)
                && matches!(&entries[1], StreamEntry::Artifact { card } if card == artifact)
        })
    }

    /// Content-free virtual-row count for integration tests of dynamic cards.
    #[doc(hidden)]
    pub fn virtual_row_count(&self, cx: &App) -> usize {
        self.total_rows(cx)
    }

    /// Starts the demo injection (标题头旁按钮)：drives the built-in
    /// ~200-block sample through the public mock replayer
    /// ([`vega_markdown::MockReplay`], 位于 vega_markdown) at ~500 δ/s into a
    /// fresh assistant entry, then finishes that stream (tech-spec §5.4 final
    /// 终结语义：作废 pending 补全残留).
    pub fn start_demo_injection(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.injecting.is_some() {
            return;
        }
        let entry_index = self.entries.len();
        self.entries.push(StreamEntry::Assistant {
            stream: Box::new(MarkdownStream::new()),
            model: StreamModel::default(),
        });
        self.list_append(entry_index);
        self.injecting = Some(InjectionState {
            entry_index,
            replay: MockReplay::new(&sample_document(200), INJECT_RATE, 0x5EED),
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(INJECT_TICK).await;
                let alive = this
                    .update(cx, |this, cx| {
                        let Some(injection) = this.injecting.as_mut() else {
                            return false;
                        };
                        // 回放器按「速率 × 已流时间」自校正出批（16ms tick
                        // 只负责轮询，主线程抖动不累积漂移）。
                        let batch = injection.replay.take_due();
                        let entry = this.entries.get_mut(injection.entry_index);
                        if let Some(StreamEntry::Assistant { stream, .. }) = entry {
                            for delta in &batch {
                                stream.append(delta);
                            }
                        }
                        if injection.replay.is_finished() {
                            // 回放结束 → final 终结语义（tech-spec §5.4）。
                            if let Some(StreamEntry::Assistant { stream, .. }) =
                                this.entries.get_mut(injection.entry_index)
                            {
                                stream.finish();
                            }
                            this.injecting = None;
                            cx.notify();
                            return false;
                        }
                        if !batch.is_empty() {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    /// Requests a durable turn. Draft/history/user echo remain untouched until
    /// the controller observes durable `MessageStarted` for this exact run.
    pub(crate) fn submit_message(&mut self, cx: &mut Context<Self>) {
        if self.composer_submit_pending || self.approved_not_started || self.trusted_action_busy {
            return;
        }
        // 提交即收起 `@file` 建议下拉（不拦截发送路径）。
        self.file_selector.close();
        let text = self.input.read(cx).text().to_string();
        if text.is_empty() {
            return;
        }
        self.composer_submit_pending = true;
        cx.emit(ComposerSubmitted {
            thread_id: self.thread.id.clone(),
            content: text,
        });
        cx.notify();
    }

    /// Commits the local echo only after conversation persistence emitted the
    /// matching durable assistant start. Edits made while waiting are kept as
    /// the next draft rather than being cleared accidentally.
    pub fn accept_composer_submission(&mut self, content: &str, cx: &mut Context<Self>) {
        if !self.composer_submit_pending {
            return;
        }
        self.composer_submit_pending = false;
        if self.composer_history.last().map(String::as_str) != Some(content) {
            self.composer_history.push(content.to_string());
        }
        self.history_cursor = None;
        self.history_draft = None;
        if self.input.read(cx).text() == content {
            self.input.update(cx, TextInput::clear);
        }
        let block_id = self.user_block_seq;
        self.user_block_seq += 1;
        let index = self.entries.len();
        self.entries.push(StreamEntry::User {
            lines: user_message_lines(block_id, content),
        });
        self.list_append(index);
        cx.notify();
    }

    /// Re-arms submit after preparation failed before any durable message.
    pub fn reject_composer_submission(&mut self, cx: &mut Context<Self>) {
        if self.composer_submit_pending {
            self.composer_submit_pending = false;
            cx.notify();
        }
    }

    /// Cmd+Enter in the Composer context ([`SendMessage`] binding).
    pub(crate) fn on_send_action(
        &mut self,
        _: &SendMessage,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_message(cx);
    }

    /// [发送] button click — same submit path as Cmd+Enter.
    pub(crate) fn on_send_clicked(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_message(cx);
    }

    /// Recalls older submitted drafts only from the first logical line; Up
    /// inside a multi-line draft remains a caret/navigation concern.
    pub(crate) fn on_previous_message(
        &mut self,
        _: &PreviousMessage,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer_history.is_empty()
            || (self.history_cursor.is_none() && !self.input.read(cx).cursor_allows_history())
        {
            return;
        }
        let current = self.input.read(cx).text().to_string();
        if let Some(index) = self.history_cursor
            && self.composer_history.get(index) != Some(&current)
        {
            self.history_cursor = None;
            self.history_draft = Some(current.clone());
        }
        let index = match self.history_cursor {
            Some(index) => index.saturating_sub(1),
            None => {
                if self.history_draft.is_none() {
                    self.history_draft = Some(current);
                }
                self.composer_history.len() - 1
            }
        };
        self.history_cursor = Some(index);
        let recalled = self.composer_history[index].clone();
        self.input
            .update(cx, |input, cx| input.set_text(&recalled, cx));
    }

    pub(crate) fn request_mode(&mut self, mode: ThreadMode, cx: &mut Context<Self>) {
        if mode != self.thread.mode {
            cx.emit(ThreadSettingsRequested {
                thread_id: self.thread.id.clone(),
                mode: Some(mode),
                permission_mode: None,
            });
        }
    }

    pub(crate) fn select_ask(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.request_mode(ThreadMode::Ask, cx);
    }

    pub(crate) fn activate_ask(
        &mut self,
        _: &ActivateThreadSetting,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_mode(ThreadMode::Ask, cx);
    }

    pub(crate) fn select_plan(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.request_mode(ThreadMode::Plan, cx);
    }

    pub(crate) fn activate_plan(
        &mut self,
        _: &ActivateThreadSetting,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_mode(ThreadMode::Plan, cx);
    }

    pub(crate) fn select_execute(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_mode(ThreadMode::Execute, cx);
    }

    pub(crate) fn activate_execute(
        &mut self,
        _: &ActivateThreadSetting,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_mode(ThreadMode::Execute, cx);
    }

    pub(crate) fn request_permission_mode(&mut self, mode: PermissionMode, cx: &mut Context<Self>) {
        if mode != self.thread.permission_mode {
            cx.emit(ThreadSettingsRequested {
                thread_id: self.thread.id.clone(),
                mode: None,
                permission_mode: Some(mode),
            });
        }
    }

    pub(crate) fn select_readonly(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_permission_mode(PermissionMode::ReadOnly, cx);
    }

    pub(crate) fn activate_readonly(
        &mut self,
        _: &ActivateThreadSetting,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_permission_mode(PermissionMode::ReadOnly, cx);
    }

    pub(crate) fn select_confirm(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_permission_mode(PermissionMode::Confirm, cx);
    }

    pub(crate) fn activate_confirm(
        &mut self,
        _: &ActivateThreadSetting,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_permission_mode(PermissionMode::Confirm, cx);
    }

    pub(crate) fn select_auto(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.request_permission_mode(PermissionMode::Auto, cx);
    }

    pub(crate) fn activate_auto(
        &mut self,
        _: &ActivateThreadSetting,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_permission_mode(PermissionMode::Auto, cx);
    }
}
