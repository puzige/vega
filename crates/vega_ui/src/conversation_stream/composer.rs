use super::*;
use gpui_kit::Focusable;

static NEXT_SUBMISSION_REQUEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl ConversationStream {
    pub fn register_artifact_call(&mut self, call_id: &str) -> bool {
        let Some((message_id, _)) = &self.active_agent_message else {
            return false;
        };
        match self.artifact_call_owners.entry(call_id.to_owned()) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry.get().as_deref() != Some(message_id.as_str()) {
                    entry.insert(None);
                    return false;
                }
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(message_id.clone()));
            }
        }
        true
    }

    pub fn artifact_capture_owner(&self, call_id: &str) -> Option<(String, u64)> {
        self.artifact_owner(call_id)
            .map(|message_id| (message_id, self.artifact_history_generation))
    }

    pub fn matches_artifact_capture_owner(&self, call_id: &str, owner: &(String, u64)) -> bool {
        owner.1 == self.artifact_history_generation
            && self.artifact_owner(call_id).as_ref() == Some(&owner.0)
    }

    fn artifact_owner(&self, call_id: &str) -> Option<String> {
        let message_id = self.artifact_call_owners.get(call_id)?.as_ref()?;
        let active = self
            .active_agent_message
            .as_ref()
            .is_some_and(|(id, _)| id == message_id);
        ((active || self.finished_artifact_turns.contains(message_id))
            && self
                .entry_identities
                .iter()
                .any(|identity| identity.message_id.as_ref() == Some(message_id)))
        .then(|| message_id.clone())
    }

    fn artifact_group(
        &mut self,
        message_id: &str,
        cx: &mut Context<Self>,
    ) -> Entity<ArtifactGroup> {
        if let Some(group) = self.artifact_groups.get(message_id) {
            return group.clone();
        }
        let group = cx.new(|_| {
            ArtifactGroup::new(
                message_id.to_owned(),
                OpenWorkspaceDiffRequested {
                    thread_id: self.thread.id.clone(),
                    project_id: self.thread.project_id.clone(),
                },
                self.reading_preview,
                self.thread.is_standalone(),
            )
        });
        cx.subscribe(&group, |this, _, _: &OpenWorkspaceDiffRequested, cx| {
            this.emit_open_diff(cx);
        })
        .detach();
        cx.observe(&group, |this, group, cx| {
            let index = this.entry_index_where(|entry| {
                matches!(entry, StreamEntry::ArtifactGroup { group: owned } if owned == &group)
            });
            this.invalidate_item(index);
            cx.notify();
        })
        .detach();
        self.artifact_groups
            .insert(message_id.to_owned(), group.clone());
        group
    }

    pub fn apply_artifact_card(
        &mut self,
        call_id: &str,
        card: Entity<ArtifactCard>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(message_id) = self.artifact_owner(call_id) else {
            return false;
        };
        if let Some(existing) = self.artifact_cards.get(call_id).cloned() {
            let incoming = card.read(cx).projection().clone();
            if existing.read(cx).id() != incoming.id {
                existing.update(cx, ArtifactCard::fail_corrupt);
                return false;
            }
            existing.update(cx, |card, cx| {
                card.apply_metadata(incoming, cx);
            });
            if let Some(group) = self.artifact_groups.get(&message_id) {
                group.update(cx, |group, cx| group.resolve_capture_error(call_id, cx));
            }
            return true;
        }
        card.update(cx, |card, cx| {
            card.set_reading_preview(self.reading_preview, cx)
        });
        let group = self.artifact_group(&message_id, cx);
        group.update(cx, |group, cx| group.capture(call_id, card.clone(), cx));
        self.artifact_cards.insert(call_id.to_owned(), card);
        self.flush_artifact_group(&message_id, cx);
        true
    }

    pub fn apply_artifact_capture_error(
        &mut self,
        call_id: &str,
        code: vega_conversation::types::GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(message_id) = self.artifact_owner(call_id) else {
            return false;
        };
        if code == vega_conversation::types::GitWorkspaceErrorCode::Cancelled {
            return true;
        }
        let group = self.artifact_group(&message_id, cx);
        group.update(cx, |group, cx| group.capture_error(call_id, code, cx));
        self.flush_artifact_group(&message_id, cx);
        true
    }

    pub(crate) fn finish_artifact_turn(&mut self, message_id: &str, cx: &mut Context<Self>) {
        self.finished_artifact_turns.insert(message_id.to_owned());
        let mut finished_messages = self
            .finished_artifact_turns
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        finished_messages.sort_unstable();
        for message_id in finished_messages {
            self.flush_artifact_group(&message_id, cx);
        }
    }

    fn artifact_turn_display_eligible(&self, message_id: &str) -> bool {
        let mut current = message_id;
        for _ in 0..=self.run_continuations.len() {
            if !self.finished_artifact_turns.contains(current) {
                return false;
            }
            let Some(relation) = self.run_continuations.get(current) else {
                return true;
            };
            current = &relation.next_assistant_id;
        }
        false
    }

    fn flush_artifact_group(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if !self.artifact_turn_display_eligible(message_id) {
            return;
        }
        let Some(group) = self.artifact_groups.get(message_id).cloned() else {
            return;
        };
        if group.read(cx).row_count() == 0
            || self.entries.iter().any(|entry| {
                matches!(entry, StreamEntry::ArtifactGroup { group: owned } if owned == &group)
            })
        {
            return;
        }
        let Some(index) = self
            .entries
            .iter()
            .zip(&self.entry_identities)
            .rposition(|(entry, identity)| {
                identity.message_id.as_deref() == Some(message_id)
                    && !matches!(entry, StreamEntry::Summary { .. })
            })
            .map(|index| index + 1)
        else {
            return;
        };
        let index = self.entries.iter().position(|entry| {
            matches!(entry, StreamEntry::Summary { card } if card.read(cx).summary().message_id == message_id)
        }).map_or(index, |summary_index| index.min(summary_index));
        self.entries
            .insert(index, StreamEntry::ArtifactGroup { group });
        self.list_insert(index);
        self.grow_entry_indices(index, 1);
        let identity = self.durable_entry_identity(message_id, "artifacts", None);
        self.set_entry_identity(index, identity);
        cx.notify();
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
            copy: MessageCopy::default(),
            stream: Box::new(MarkdownStream::new()),
            model: StreamModel::default(),
            failure: None,
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
                        if let Some(StreamEntry::Assistant { stream, copy, .. }) = entry {
                            for delta in &batch {
                                stream.append(delta);
                                copy.append(delta);
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

    pub(crate) fn can_submit_message(&self, cx: &App) -> bool {
        !self.reading_preview
            && (!self.actions.running || self.thread.backend == TaskBackend::Native)
            && !self.pending_queue.busy
            && !self.context_operation_busy()
            && self.actions.pending_mode.is_none()
            && (!self.input.read(cx).text().is_empty() || !self.attachments.is_empty())
            && !self.attachment_import_pending
            && !self.composer_submit_pending
            && !self.ordinary_pending_before_run()
            && !self.approved_not_started
            && !self.trusted_action_busy
            && !self.skill_mutation_pending
            && self.model_selection_pending.is_none()
    }

    pub(crate) fn submit_message(&mut self, cx: &mut Context<Self>) {
        if !self.can_submit_message(cx) {
            return;
        }
        // 提交即收起 `@file` 建议下拉并取消当前索引 owner；解析阶段
        // 仍由 app 在 provider 构造前执行。
        self.invalidate_composer_prediction(cx);
        self.close_file_selector_and_cancel(cx);
        let text = self.input.read(cx).text().to_string();
        self.composer_submit_pending = true;
        self.pending_submission_revision = self
            .actions
            .running
            .then(|| self.input.read(cx).content_revision());
        self.submitted_attachments = self.attachments.clone();
        cx.emit(ComposerSubmitted {
            request_id: NEXT_SUBMISSION_REQUEST.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            draft_revision: self.input.read(cx).content_revision(),
            attachment_ids: self.attachments.iter().map(|(id, _)| *id).collect(),
            pending_behavior: self
                .actions
                .running
                .then(|| self.running_message_behavior(cx)),
            images: self
                .attachments
                .iter()
                .map(|(_, image)| image.image.clone())
                .collect(),
            thread_id: self.thread.id.clone(),
            content: text,
            skill_intent: self.skill_intent.clone(),
            reasoning: self.frozen_reasoning_for_submit(),
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
        self.pending_submission_revision = None;
        self.pending_behavior_override = None;
        let copy = MessageCopy::from_live_text(content);
        let projected = copy.original_text();
        if self.composer_history.last().map(String::as_str) != Some(content) {
            self.composer_history.push(projected.clone());
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
            copy,
            lines: user_message_lines(block_id, &projected),
        });
        self.list_append(index);
        let submitted = std::mem::take(&mut self.submitted_attachments);
        self.attachments
            .retain(|(id, _)| !submitted.iter().any(|(sent, _)| sent == id));
        if !submitted.is_empty() {
            let index = self.entries.len();
            let images = self.history_image_previews(
                submitted
                    .into_iter()
                    .map(|(_, image)| image.image)
                    .collect(),
                cx,
            );
            self.entries.push(StreamEntry::UserImages { images });
            self.list_append(index);
        }
        cx.notify();
    }

    /// Re-arms submit after preparation failed before any durable message.
    pub fn reject_composer_submission(&mut self, cx: &mut Context<Self>) {
        self.pending_submission_revision = None;
        self.submitted_attachments.clear();
        if self.composer_submit_pending {
            self.composer_submit_pending = false;
            cx.notify();
        }
    }

    pub(crate) fn on_send_action(
        &mut self,
        _: &SendMessage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.input.read(cx).focus_handle(cx).is_focused(window) {
            cx.propagate();
            return;
        }

        if self.input.read(cx).is_composing() {
            return;
        }
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).is_composing() {
            return;
        }
        let current = self.input.read(cx).text().to_string();
        let recalling_history = self
            .history_cursor
            .and_then(|index| self.composer_history.get(index))
            == Some(&current);
        if self.composer_history.is_empty()
            || (!recalling_history && !self.input.read(cx).cursor_allows_history())
        {
            self.input
                .update(cx, |input, cx| input.up(&crate::text_input::Up, window, cx));
            return;
        }
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
        if self.model_selection_pending.is_some() {
            return;
        }
        if mode != self.thread.mode {
            cx.emit(ThreadSettingsRequested {
                thread_id: self.thread.id.clone(),
                mode: Some(mode),
                permission_mode: None,
            });
        }
    }

    pub(crate) fn request_permission_mode(&mut self, mode: PermissionMode, cx: &mut Context<Self>) {
        if self.model_selection_pending.is_some() {
            return;
        }
        if mode != self.thread.permission_mode {
            cx.emit(ThreadSettingsRequested {
                thread_id: self.thread.id.clone(),
                mode: None,
                permission_mode: Some(mode),
            });
        }
    }
}
