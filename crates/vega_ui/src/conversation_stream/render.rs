use super::*;
use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade, ScrollFadeTopGuards};
use gpui_kit::{Focusable, canvas};

pub(crate) const PICKER_EFFORT_LABEL: &str = "ui.select_effort";

pub(crate) const PICKER_LIST_HEADING: &str = "ui.select_model";

pub(crate) fn model_trigger_label(
    model: &str,
    level: ModelPickerLevel,
    save_pending: bool,
) -> &str {
    if save_pending {
        return vega_i18n::t("ui.saving");
    }
    if model.is_empty() {
        return vega_i18n::t("ui.model");
    }
    if level == ModelPickerLevel::Slider {
        return vega_i18n::t(PICKER_EFFORT_LABEL);
    }
    model
}

impl ConversationStream {
    pub(super) fn emit_open_diff(&mut self, cx: &mut Context<Self>) {
        if self.reading_preview || self.thread.is_standalone() {
            return;
        }
        cx.emit(OpenWorkspaceDiffRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
        });
    }

    fn open_diff_action(&mut self, _: &OpenWorkspaceDiff, _: &mut Window, cx: &mut Context<Self>) {
        self.emit_open_diff(cx);
    }

    pub(super) fn focus_reasoning_retry_neighbor(
        &self,
        index: usize,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let neighboring = if backwards {
            self.reasoning_default_failures[..index]
                .iter()
                .rev()
                .find_map(|failure| {
                    self.reasoning_default_retry_focuses
                        .get(&failure.request.defaults.model)
                })
        } else {
            self.reasoning_default_failures[index + 1..]
                .iter()
                .find_map(|failure| {
                    self.reasoning_default_retry_focuses
                        .get(&failure.request.defaults.model)
                })
        };
        if let Some(focus) = neighboring {
            window.focus(focus, cx);
        } else {
            window.focus(&self.input.read(cx).focus_handle(cx), cx);
        }
        cx.stop_propagation();
    }

    fn previous_composer_control_with_reasoning_retry(
        &mut self,
        action: &PreviousComposerControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.input.read(cx).is_composing()
            && self.input.read(cx).focus_handle(cx).is_focused(window)
            && let Some(focus) = self
                .reasoning_default_failures
                .iter()
                .rev()
                .find_map(|failure| {
                    self.reasoning_default_retry_focuses
                        .get(&failure.request.defaults.model)
                })
        {
            window.focus(focus, cx);
            cx.stop_propagation();
            return;
        }
        self.previous_composer_control(action, window, cx);
    }

    fn copy_selected_text(
        &mut self,
        _: &CopySelectedText,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reading = self.entries.iter().find_map(|entry| {
            let copy = match entry {
                StreamEntry::User { copy, .. } | StreamEntry::Assistant { copy, .. } => copy,
                _ => return None,
            };
            copy.nsh_reading
                .borrow()
                .as_ref()
                .filter(|binding| binding.selection_current(cx))
                .cloned()
        });
        if let Some(reading) = reading {
            reading.copy(window, cx);
            cx.stop_propagation();
        }
    }

    fn render_conversation_foreground(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let stream = cx.entity().downgrade();
        div()
            .absolute()
            .bottom_0()
            .left_0()
            .w_full()
            .flex()
            .flex_col()
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let height = bounds.size.height;
                        let changed = stream
                            .update(cx, |stream, _| stream.foreground_height != height)
                            .unwrap_or(false);
                        if changed {
                            cx.defer(move |cx| {
                                let _ = stream.update(cx, |stream, cx| {
                                    if stream.foreground_height != height {
                                        stream.foreground_height = height;
                                        cx.notify();
                                    }
                                });
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(
                div()
                    .absolute()
                    .top(px(
                        Layout::READING_NAVIGATION_HEIGHT - Layout::CONVERSATION_BOTTOM_FADE_HEIGHT
                    ))
                    .left_0()
                    .w_full()
                    .h(px(Layout::CONVERSATION_BOTTOM_FADE_HEIGHT))
                    .child(super::foreground_chrome::decoration(
                        super::foreground_chrome::Decoration::Fade,
                    )),
            )
            .child(self.render_resume_tail(cx))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .relative()
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(super::foreground_chrome::decoration(
                                super::foreground_chrome::Decoration::Base,
                            )),
                    )
                    .occlude()
                    .child(
                        div().px(px(Layout::CONVERSATION_PADDING)).child(
                            div()
                                .w_full()
                                .max_w(px(Layout::CONVERSATION_MAX_WIDTH))
                                .mx_auto()
                                .children(self.message_location_status_element(window, cx))
                                .children(self.render_computer_use_notice(cx)),
                        ),
                    )
                    .when(self.privacy.opened, |column| {
                        column.child(self.render_privacy_control(cx))
                    })
                    .child(self.render_composer(window, cx)),
            )
            .into_any_element()
    }

    fn render_resume_tail(&self, cx: &mut Context<Self>) -> AnyElement {
        let stream = cx.entity().downgrade();
        let ui = self.resume_tail_ui.clone();
        let authority = self.resume_tail_authority;
        let epoch = self.reading_navigation_epoch.clone();
        gpui_kit::container_query(move |_, window, cx| {
            let current = stream
                .upgrade()
                .is_some_and(|owner| owner.read(cx).reading_navigation_epoch.matches(&epoch));
            if !current {
                return div().size_full().into_any_element();
            }
            let mut ui = ui.borrow_mut();
            if let Some(authority) = authority {
                ui.element(stream.clone(), authority, window, cx)
            } else {
                ui.hide(cx);
                div().size_full().into_any_element()
            }
        })
        .w_full()
        .h(px(Layout::READING_NAVIGATION_HEIGHT))
        .flex_shrink_0()
        .into_any_element()
    }

    fn render_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.automation_permission.busy() {
            self.hide_shell_recovery_surface();
            return self.render_automation_permission(window, cx);
        }
        let colors = theme(cx).colors;
        let file_retry_visible = self.file_selector_wanted && self.file_index_failure.is_some();
        let file_selector_active =
            self.file_selector.is_open() || self.file_selector_wanted || self.file_index_loading;
        div()
            .px(px(Layout::CONVERSATION_PADDING))
            .pt(px(Layout::COMPOSER_PADDING_TOP))
            .pb(px(Layout::COMPOSER_PADDING_BOTTOM))
            .flex_shrink_0()
            .key_context("Composer")
            .on_action(cx.listener(Self::next_composer_control))
            .on_action(cx.listener(Self::previous_composer_control_with_reasoning_retry))
            .when(self.utility_bar_visible(cx), |column| {
                column.child(self.render_composer_utility_bar(window, cx))
            })
            .children(
                self.controller_error
                    .clone()
                    .filter(|_| !self.controller_error_is_run_notice())
                    .map(|error| {
                        div()
                            .w_full()
                            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                            .mx_auto()
                            .mt_1()
                            .mb_2()
                            .child(render_rows::plain_error_notice(
                                error.render(),
                                colors.danger,
                            ))
                            .when(!self.input.read(cx).skill_mentions().is_empty(), |row| {
                                row.child(
                                    super::privacy::PrivacyButton::new(
                                        "composer-skill-settings",
                                        colors,
                                    )
                                    .owner(self, cx)
                                    .label(vega_i18n::t("ui.open_skill_settings"))
                                    .on_click(|_, _, cx| {
                                        cx.set_global(crate::settings::SkillsSettingsRequested(
                                            true,
                                        ));
                                        cx.set_global(crate::settings::SettingsOpen(true));
                                        cx.refresh_windows();
                                    }),
                                )
                            })
                    }),
            )
            .child(self.render_reasoning_feedback(cx))
            .when(self.thread.backend == TaskBackend::Native, |column| {
                column.child(self.render_pending_status(cx))
            })
            .when(
                self.thread.backend == TaskBackend::Native
                    && !self.visible_pending_messages().is_empty(),
                |column| column.child(self.render_pending_queue(cx)),
            )
            .children(self.active_user_question.clone())
            .child(self.render_automation(cx))
            .child(self.render_thread_permission_feedback(cx))
            .child(self.render_shell_recovery(window, cx))
            .child(
                div()
                    .on_drop(cx.listener(Self::drop_images))
                    .w_full()
                    .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                    .min_h(px(Layout::COMPOSER_MIN_HEIGHT))
                    .mx_auto()
                    // Cmd+Enter 的按键上下文（绑定见 vega_ui::init）。
                    .key_context("Composer")
                    .when(self.permission_picker_open, |shell| {
                        shell.on_key_down(cx.listener(Self::on_permission_picker_key_down))
                    })
                    .on_action(cx.listener(Self::previous_composer_action))
                    .on_action(cx.listener(Self::next_composer_action))
                    .on_action(cx.listener(Self::accept_composer_action))
                    .on_action(cx.listener(Self::close_composer_actions))
                    .on_action(cx.listener(Self::on_send_action))
                    .on_action(cx.listener(Self::on_previous_message))
                    .on_action(cx.listener(Self::on_selector_previous))
                    .on_action(cx.listener(Self::on_selector_next))
                    .on_action(cx.listener(Self::on_selector_cancel))
                    .on_action(cx.listener(Self::on_selector_accept))
                    .when(file_retry_visible, |row| {
                        row.key_context("FileSelectRetry")
                            .on_action(cx.listener(Self::on_selector_retry_action))
                            .on_action(cx.listener(Self::on_selector_retry_focus))
                            .on_action(cx.listener(Self::on_selector_retry_focus_previous))
                    })
                    .on_action(cx.listener(Self::on_activate_model))
                    .on_action(cx.listener(Self::on_model_previous))
                    .on_action(cx.listener(Self::on_model_next))
                    .on_action(cx.listener(Self::on_model_close))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .relative()
                    .border_1()
                    .border_color(gpui_kit::transparent_black())
                    .rounded(px(Layout::COMPOSER_RADIUS))
                    .p_3()
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(self.render_composer_shell(window, cx)),
                    )
                    .when(
                        !self.attachments.is_empty()
                            || self.attachment_import_pending
                            || self.attachment_error.is_some(),
                        |el| el.child(self.render_attachments(cx)),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .when(file_selector_active && !file_retry_visible, |row| {
                                row.key_context("FileSelect")
                            })
                            .when(self.composer_actions_visible(cx), |row| {
                                row.key_context("ComposerActions")
                            })
                            .child(self.render_composer_actions(cx))
                            .child(self.render_file_dropdown(cx))
                            .min_h(px(40.))
                            .child(self.render_nsh_composer_input(window, cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .when(self.compact_workspace, |row| row.gap_1())
                            .when(self.context_menu_available(), |row| {
                                row.child(self.render_composer_trigger(cx))
                            })
                            .when(self.thread.backend == TaskBackend::Native, |row| {
                                row.child(self.render_permission_status(cx))
                            })
                            .when(self.thread.backend == TaskBackend::Codex, |row| {
                                row.child(self.render_backend_status(window, cx))
                            })
                            .child(div().min_w_0().flex_1())
                            .when(self.thread.backend == TaskBackend::Native, |row| {
                                row.child(self.render_model_controls(window, cx))
                            })
                            .when(
                                !self.composer_resume_pending && self.shows_running_send(cx),
                                |row| row.child(self.render_composer_running_send(cx)),
                            )
                            .when(
                                (self.actions.running
                                    || self.composer_submit_pending
                                    || self.ordinary_pending_before_run()
                                    || self.has_cancellable_queue_wait())
                                    && (self.composer_resume_pending
                                        || !self.shows_running_send(cx)),
                                |row| row.child(self.render_composer_stop(cx)),
                            )
                            .when(
                                self.composer_resume_pending
                                    && !self.actions.running
                                    && !self.composer_submit_pending
                                    && !self.ordinary_pending_before_run()
                                    && !self.has_cancellable_queue_wait(),
                                |row| row.child(self.render_composer_resume_pending(cx)),
                            )
                            .when(
                                !self.actions.running
                                    && !self.composer_submit_pending
                                    && !self.composer_resume_pending
                                    && !self.ordinary_pending_before_run()
                                    && !self.has_cancellable_queue_wait(),
                                |row| row.child(self.render_composer_primary(cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_permission_status(&self, cx: &mut Context<Self>) -> AnyElement {
        self.render_nsh_permission_selector(cx)
    }

    fn render_backend_status(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let label = if self.codex_mode_confirmed {
            "Codex ACP · workspace-write · on-request"
        } else {
            "Codex ACP"
        };
        super::conversation_status::backend(label.to_owned(), window, cx)
    }

    fn on_permission_picker_key_down(
        &mut self,
        event: &gpui_kit::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).is_composing() {
            return;
        }
        if matches!(event.keystroke.key.as_str(), "up" | "down" | "tab") {
            let current = self
                .permission_editor
                .options
                .iter()
                .position(|focus| focus.is_focused(window))
                .unwrap_or(0);
            let backwards = event.keystroke.key == "up"
                || (event.keystroke.key == "tab" && event.keystroke.modifiers.shift);
            let next = if backwards {
                (current + 3) % 4
            } else {
                (current + 1) % 4
            };
            window.focus(&self.permission_editor.options[next], cx);
            cx.stop_propagation();
            return;
        }
        if event.keystroke.key != "escape" {
            return;
        }
        self.permission_picker_open = false;
        window.focus(&self.permission_editor.trigger, cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn select_permission_mode(
        &mut self,
        mode: PermissionMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.permission_picker_open = false;
        self.request_permission_mode(mode, cx);
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn render_file_dropdown(&self, cx: &mut Context<Self>) -> AnyElement {
        self.render_nsh_file_dropdown(cx)
    }

    fn render_model_controls(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .min_w_0()
            .flex_shrink_1()
            .flex()
            .items_center()
            .gap_1()
            .children(
                self.context_usage_visible()
                    .then(|| super::foreground_chrome::indicator(cx.weak_entity())),
            )
            .child(self.render_model_selector(window, cx))
            .into_any_element()
    }

    fn render_model_selector(&self, _: &Window, cx: &mut Context<Self>) -> AnyElement {
        self.render_nsh_model_selector(cx)
    }

    fn message_location_status_element(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let status = self.message_location_status?;
        let colors = theme(cx).colors;
        let (label, color) = match status {
            MessageLocationStatus::Searching => {
                (vega_i18n::t("ui.locating_message"), colors.text_secondary)
            }
            MessageLocationStatus::Deferred => (
                vega_i18n::t("ui.the_message_will_be_located_after_the_task"),
                colors.text_secondary,
            ),
            MessageLocationStatus::NotFound => (
                vega_i18n::t("ui.this_message_is_not_in_the_current_conversation"),
                colors.warning,
            ),
            MessageLocationStatus::Failed => (
                vega_i18n::t("ui.could_not_locate_the_message_retry_or_choose"),
                colors.danger,
            ),
            MessageLocationStatus::Located => return None,
        };
        Some(super::conversation_status::location(
            label.into(),
            color,
            window,
            cx,
        ))
    }
}

impl Render for ConversationStream {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ui_presentations_suspended = false;
        if self.reading_preview {
            self.suspend_business_presentations(cx);
        } else if self.thread.is_standalone() {
            self.commit_panel
                .update(cx, |panel, cx| panel.suspend_presentation(cx));
        }
        if !self.reading_preview && self.entries.is_empty() {
            self.suspend_reading_navigation(cx);
        }
        if !self.reading_preview {
            self.prepare_nsh_composer(window, cx);
            self.reconcile_pending_focus(window, cx);
            self.prepare_thread_permission_focus(window, cx);
            self.prepare_automation_permission_focus(window, cx);
        }
        if !self.reading_preview && !self.selection_view_initialized {
            gpui_kit::base::TextSelection::clear(window, cx);
            self.selection_view_initialized = true;
        }
        if !self.reading_preview && self.actions.restore_focus {
            self.actions.restore_focus = false;
            self.focus_composer(window, cx);
        }
        if self.rendered_language != vega_i18n::language() {
            self.rendered_language = vega_i18n::language();
            self.ensure_entry_identities();
            let top = self.list.logical_scroll_top();
            let anchor = self.scroll_anchor_snapshot();
            for entry in &mut self.entries {
                if let StreamEntry::User { copy, lines, model } = entry
                    && copy.refresh_live_projection()
                {
                    let block = self.user_block_seq;
                    self.user_block_seq += 1;
                    let text = copy.original_text();
                    *lines = user_message_lines(block, &text);
                    *model = user_markdown_model(&text);
                }
                if let StreamEntry::Assistant {
                    copy,
                    stream,
                    model,
                    ..
                } = entry
                {
                    copy.sync_model(stream, model, &self.counters);
                }
            }
            self.text_cache.get_mut().clear();
            self.measured_entry_heights.clear();
            self.list.remeasure();
            self.restore_scroll_anchor_at(&anchor, top.item_ix);
        }
        self.reset_image_visibility(cx);
        for group in self.run_activity_groups.values() {
            for child in group.read(cx).children() {
                if let RunActivityChild::Thinking(card) = child {
                    card.read(cx).reset_body_visibility();
                }
            }
        }
        let render_t0 = Instant::now();
        let colors = theme(cx).colors;
        let counters = self.counters.clone();

        // 1) 差量同步：仅 mutable tail（流式中的 assistant 段）参与快照
        //    diff —— 冻结段内容在终结后不再变化，永不重物化（P3/C4 白名
        //    单）；尾项高度可能随内容变化 → 显式失效重测。user 回显等行数
        //    变化在各自的 apply 路径上已登记。
        if std::mem::take(&mut self.assistant_model_dirty)
            && let Some((_, index)) = self.active_agent_message.as_ref()
            && let Some(StreamEntry::Assistant {
                copy,
                stream,
                model,
                ..
            }) = self.entries.get_mut(*index)
            && copy.sync_model(stream, model, &self.counters)
        {
            if !self
                .timestamps
                .has_message_time(*index, &self.entry_identities)
            {
                self.timestamps.dirty = true;
            }
            self.invalidate_item(Some(*index));
        }
        self.settle_final_presentation(window, cx);
        self.refresh_run_notices(cx);
        self.refresh_timestamps(window, cx);
        let stage_top_insets = self.stage_top_insets(cx);
        self.prepare_text_cache();

        let at_top = self.scroll_at_top();
        let at_bottom = self.scroll_at_bottom();
        if self.hydration_pause_is_stale(at_top, at_bottom) {
            self.hydration.paused = false;
            self.hydration.paused_direction = None;
        }
        if let Some(before) = self.history_page_request(at_top) {
            self.hydration.loading = true;
            cx.emit(HistoryPageRequested {
                thread_id: self.thread.id.clone(),
                before,
            });
        } else if let Some(after) = self.newer_history_page_request(at_bottom) {
            self.hydration.loading = true;
            cx.emit(NewerHistoryPageRequested {
                thread_id: self.thread.id.clone(),
                after,
            });
        }

        let mut message_anchor_preview = None;
        let mut message_anchor_rail_overlay = None;
        let mut message_anchor_preview_in_lane = false;
        let body: AnyElement = if self.reading_preview && self.entries.is_empty() {
            div().into_any_element()
        } else if self.entries.is_empty() {
            div()
                .w_full()
                .flex()
                .flex_col()
                .child(super::conversation_status::empty(window, cx))
                .children(self.message_location_status_element(window, cx))
                .into_any_element()
        } else {
            let top_fade_guards = ScrollFadeTopGuards::default();
            let rendered_top_fade_guards = top_fade_guards.clone();
            let (message_anchor_rail, preview, preview_in_lane) =
                self.render_message_anchor_rail(&stage_top_insets, window, cx);
            message_anchor_rail_overlay = Some(message_anchor_rail);
            message_anchor_preview = preview;
            message_anchor_preview_in_lane = preview_in_lane;
            div()
                .id("conversation-scroll")
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .min_h_0()
                        .flex_1()
                        .flex()
                        .flex_row()
                        .relative()
                        .when(preview_in_lane, |row| {
                            row.children(message_anchor_preview.take())
                        })
                        .child(
                            div()
                                .id("conversation-message-list")
                                .relative()
                                .min_w_0()
                                .flex_1()
                                .h_full()
                                .overflow_hidden()
                                .child(
                                    selection_menu::mounted::ReadingList::new(
                                        cx.entity().downgrade(),
                                        turn_placement::TurnList::new(
                                        self.list.clone(),
                                        self.turn_placement.clone(),
                                        self.foreground_height,
                                        self.entries.len(),
                                        cx.processor(
                                            move |this: &mut ConversationStream,
                                                  index: usize,
                                                  window,
                                                  cx| {
                                                let row_t0 = Instant::now();
                                                let entry = this.entries.get(index);
                                                let mut pinned = None;
                                                let item = if this.reading_preview && let Some(StreamEntry::Permission { card }) = entry {
                                                    let card = card.read(cx);
                                                    let (tool, target) = card.identity();
                                                    let status = if card.is_resolved() { vega_i18n::t("ui.confirmation_handled") } else { vega_i18n::t("ui.awaiting_confirmation") };
                                                    super::conversation_status::read_only_permission(index as u64, format!("{status} · {tool} · {target}"), window, cx)
                                                } else if this.reading_preview && let Some(StreamEntry::Plan { card }) = entry {
                                                    PlanCard::render_read_only(card.clone(), window, cx)
                                                } else if this.process_entry_hidden(index, cx) || this.compaction_failure_is_represented(index) {
                                                    div().h_0().into_any_element()
                                                } else {
                                                    match entry {
                                                        Some(StreamEntry::RunActivity {
                                                            group,
                                                        }) => {
                                                            this.rebind_thinking_presentation(group, cx);
                                                            this.rebind_run_activity_presentation(group, cx);
                                                            pinned = group.read(cx).scroll_fade_pinned_indicator(None);
                                                            div()
                                                                .w_full()
                                                                .min_w_0()
                                                                .flex_shrink_0()
                                                                .pt(px(stage_top_insets.get(&index).copied().unwrap_or(0.)))
                                                                .child(this.render_run_details(group.clone(), window, cx))
                                                                .into_any_element()
                                                        },
                                                        Some(StreamEntry::RunActivitySegment { group, segment }) => {
                                                            this.rebind_thinking_presentation(group, cx);
                                                            this.rebind_run_activity_presentation(group, cx);
                                                            pinned = group.read(cx).scroll_fade_pinned_indicator(Some(*segment));
                                                            RunActivityGroup::render_segment_with_top(group.clone(), *segment, stage_top_insets.get(&index).copied().unwrap_or(Layout::CONVERSATION_STAGE_TOP), window, cx)
                                                        },
                                                        Some(entry) => {
                                                            this.prepare_partial_compaction_menu(index, cx);
                                                            this.prepare_fork_menu(index, cx);
                                                            let stream = cx.entity().downgrade();
                                                            let body = render_stream_entry(
                                                                entry,
                                                                this.entry_identities
                                                                    .get(index)
                                                                    .is_some_and(|identity| {
                                                                        identity.message_id.is_some()
                                                                    }),
                                                                stage_top_insets
                                                                    .get(&index)
                                                                    .copied()
                                                                    .unwrap_or(0.),
                                                                window,
                                                                cx,
                                                                render_rows::MessageInteraction {
                                                                    user_top: this.user_message_top(index),
                                                                    stream: Some(stream),
                                                                    entry_identity: this.entry_identities.get(index).cloned(),
                                                                    ..this.message_interaction()
                                                                },
                                                            );
                                                            let user = matches!(entry, StreamEntry::User { .. } | StreamEntry::UserImages { .. });
                                                            let user_bubble = matches!(entry, StreamEntry::User { .. });
                                                            let source = match entry {
                                                                StreamEntry::User { copy, .. } => Some(copy),
                                                                _ => this.action_copy_source_for_entry(index),
                                                            };
                                                            message_footer::message_with_copy(body, source, message_footer::MessageActions {
                                                                owner: cx.entity().downgrade(),
                                                                identity: this.entry_identities.get(index).cloned(),
                                                                id: this.entry_identities.get(index).map(|id| id.key.clone()).unwrap_or_else(|| format!("message-{index}")).into(),
                                                                user, user_bubble, colors: theme(cx).colors, enabled: this.message_actions_enabled(),
                                                                timestamp: this.entry_timestamp(index), focus: this.timestamp_focus(index),
                                                                fork: this.fork_request_for_entry(index).map(|request| (request, cx.entity().downgrade())),
                                                                fork_pending: this.fork_pending.as_deref(),
                                                                fork_enabled: this.fork_is_idle(cx),
                                                            }, window, cx)
                                                        },
                                                        None => div().into_any_element(),
                                                    }
                                                };
                                                let item = if let Some(controls) = this.ordinary_controls(index, cx) {
                                                    div().w_full().flex().flex_col().child(item).child(controls).into_any_element()
                                                } else { item };
                                                let item = if let Some(notice) =
                                                    this.run_notices.get(&index)
                                                {
                let colors = theme(cx).colors;
                                                    div()
                                                        .w_full()
                                                        .min_w_0()
                                                        .flex_shrink_0()
                                                        .flex()
                                                        .flex_col()
                                                        .child(item)
                                                        .child(notice.render(&colors, this.thread.id.clone(), this.reading_preview || this.permissions_revoked, cx))
                                                        .into_any_element()
                                                } else {
                                                    item
                                                };
                                                let item = if let Some(notice) = this.mcp_notices.get(&index) {
                                                    let colors = theme(cx).colors;
                                                    div()
                                                        .w_full()
                                                        .min_w_0()
                                                        .flex_shrink_0()
                                                        .flex()
                                                        .flex_col()
                                                        .child(item)
                                                        .child(notice.render(&colors, this.thread.id.clone(), this.reading_preview || this.permissions_revoked, cx))
                                                        .into_any_element()
                                                } else {
                                                    item
                                                };
                                                let item = if let Some(prefix) = this.timestamp_prefix(index, theme(cx).colors) {
                                                    div().w_full().min_w_0().flex().flex_col().child(prefix).child(item).into_any_element()
                                                } else { item };
                                                this.counters.record_row_callback(
                                                    row_t0.elapsed().as_nanos(),
                                                );
                                                if let Some(pinned) = pinned {
                                                    rendered_top_fade_guards.observe(item, pinned)
                                                } else {
                                                    item
                                                }
                                            },
                                        ),
                                    )
                                    .h_full()
                                    .w_full()
                                    .tracked_scroll_edge_fade(self.list.clone(), colors.bg_base, ScrollFade::reading().top_only().vertical_list())
                                    .preserve_pinned_headers(top_fade_guards),
                                    ),
                                ),
                        ),
                )
                .into_any_element()
        };

        let project_bound = !self.thread.is_standalone();
        let navigation_stream = cx.entity().downgrade();
        let navigation_epoch = self.reading_navigation_epoch.clone();
        let navigation_authority = self.resume_tail_authority;
        let element = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .relative()
            .text_color(colors.text_primary)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .child(super::foreground_chrome::decoration(
                        super::foreground_chrome::Decoration::ConversationBase,
                    )),
            )
            .key_context("ConversationStream")
            .track_focus(&self.selection_focus)
            .capture_action(
                cx.listener(|this, _: &crate::text_input::AcceptPrediction, _, cx| {
                    if this.model_picker_level.is_open() {
                        cx.stop_propagation();
                    }
                }),
            )
            .capture_action(cx.listener(
                |this, _: &crate::text_input::DismissPrediction, window, cx| {
                    if this.model_picker_level.is_open() {
                        this.on_model_close(&CloseModel, window, cx);
                        cx.stop_propagation();
                    }
                },
            ))
            .capture_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if this.model_picker_level.is_open()
                        && event.keystroke.key == "escape"
                        && !this.input.read(cx).is_composing()
                    {
                        this.on_model_close(&CloseModel, window, cx);
                        cx.stop_propagation();
                        return;
                    }
                    if this.shell_recovery.opened
                        && event.keystroke.key == "escape"
                        && !this.input.read(cx).is_composing()
                    {
                        this.dismiss_shell_recovery(window, cx);
                        cx.stop_propagation();
                        return;
                    }
                    if this.context_usage_trigger_hovered && event.keystroke.key == "escape" {
                        this.context_usage_trigger_hovered = false;
                        cx.notify();
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.model_picker_level.is_open() || this.permission_picker_open {
                        this.model_picker_level = ModelPickerLevel::Closed;
                        this.permission_picker_open = false;
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(Self::open_diff_action))
            .on_action(cx.listener(Self::copy_selected_text))
            .on_action(move |_: &ResumeTail, window, cx| {
                let current = navigation_stream.upgrade().is_some_and(|stream| {
                    stream
                        .read(cx)
                        .reading_navigation_epoch
                        .matches(&navigation_epoch)
                });
                if current
                    && window.is_window_active()
                    && navigation_authority
                        .is_some_and(|authority| authority(&navigation_stream, window, cx))
                {
                    let _ = navigation_stream.update(cx, |stream, cx| stream.resume_tail(cx));
                }
                cx.stop_propagation();
            })
            .when(!self.reading_preview, |root| {
                root.children(self.render_parent_navigation(window, cx))
            })
            .child(
                div()
                    .w_full()
                    .min_h_0()
                    .when(!self.entries.is_empty(), |body| {
                        body.flex_1().overflow_hidden()
                    })
                    .when(self.entries.is_empty(), |body| {
                        body.flex_1()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                    })
                    .px(px(Layout::CONVERSATION_PADDING))
                    .relative()
                    .child(
                        div()
                            .min_w_0()
                            .w_full()
                            .max_w(px(Layout::CONVERSATION_MAX_WIDTH))
                            .mx_auto()
                            .when(!self.entries.is_empty(), |body| body.h_full())
                            .child(body),
                    )
                    .children(message_anchor_rail_overlay)
                    .children(if message_anchor_preview_in_lane {
                        None
                    } else {
                        message_anchor_preview
                    }),
            )
            .when(!self.reading_preview && !self.entries.is_empty(), |root| {
                root.child(self.render_conversation_foreground(window, cx))
            })
            .when(!self.reading_preview && self.entries.is_empty(), |root| {
                root.when(self.privacy.opened, |view| {
                    view.child(self.render_privacy_control(cx))
                })
                .child(self.render_composer(window, cx))
            })
            .when(
                !self.reading_preview && !self.reading_navigation_blocked && project_bound,
                |root| root.child(self.commit_panel.clone()),
            )
            .when(self.reading_preview, |root| {
                root.child(self.render_resume_tail(cx))
            })
            .when(!self.reading_preview, |root| {
                root.children(self.render_pending_menu(cx))
                    .child(self.render_shell_recovery_overlay(window, cx))
            })
            .into_any_element();
        counters.record_render(render_t0);
        crate::link_preview::MarkdownReadingScope::new(
            format!("conversation-{}", self.thread.id),
            self.selection_focus.clone(),
        )
        .track_list(self.list.clone())
        .fill_height()
        .child(element)
    }
}
