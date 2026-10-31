use super::context_usage::ContextUsageDisplay;
use super::*;
use crate::interaction::InteractionExt;
use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade, ScrollFadeTopGuards};
use gpui_kit::component::Sizable as _;
use gpui_kit::{
    Anchor, AnchoredPositionMode, Focusable, PathBuilder, Role, anchored, canvas, point,
};
// Re-exported so the permission picker's copy is testable from the suite the
// same way `PICKER_EFFORT_LABEL`/`PICKER_LIST_HEADING` are.
use super::composer_actions::{
    PERMISSION_ORDER, permission_description, permission_icon, permission_is_warning,
    permission_label,
};
pub(crate) use super::composer_actions::{PERMISSION_PICKER_LEARN_MORE, PERMISSION_PICKER_TITLE};

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

    fn focus_reasoning_retry_neighbor(
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

    fn copy_selected_text(&mut self, _: &CopySelectedText, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(copy) = self.entries.iter().find_map(|entry| {
            let copy = match entry {
                StreamEntry::User { copy, .. } | StreamEntry::Assistant { copy, .. } => copy,
                _ => return None,
            };
            copy.has_selected_text().then_some(copy)
        }) {
            copy.copy_selected_text(cx);
        }
    }

    fn on_resume_tail(&mut self, _: &ResumeTail, _: &mut Window, cx: &mut Context<Self>) {
        self.resume_tail(cx);
    }

    fn on_resume_tail_clicked(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.resume_tail(cx);
    }

    fn render_conversation_foreground(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let stream = cx.entity().downgrade();
        let fade = gpui_kit::linear_gradient(
            180.,
            gpui_kit::linear_color_stop(colors.bg_base.opacity(0.), 0.),
            gpui_kit::linear_color_stop(colors.bg_base, 1.),
        );
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
                    .bg(fade),
            )
            .child(self.render_resume_tail(cx))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .bg(colors.bg_base)
                    .occlude()
                    .child(
                        div().px(px(Layout::CONVERSATION_PADDING)).child(
                            div()
                                .w_full()
                                .max_w(px(Layout::CONVERSATION_MAX_WIDTH))
                                .mx_auto()
                                .children(self.message_location_status_element(cx))
                                .children(self.render_computer_use_notice(cx)),
                        ),
                    )
                    .when(self.privacy.opened, |column| {
                        column.child(self.render_privacy_control(cx))
                    })
                    .child(self.render_composer(window, cx))
                    .when(!self.thread.is_standalone(), |column| {
                        column.child(self.commit_panel.clone())
                    }),
            )
            .into_any_element()
    }

    fn render_resume_tail(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .h(px(Layout::READING_NAVIGATION_HEIGHT))
            .flex_shrink_0()
            .py(px(Layout::READING_NAVIGATION_PADDING))
            .flex()
            .justify_center()
            .when(!self.following_tail(), |row| {
                row.child(
                    div()
                        .id("scroll-to-bottom")
                        .aria_label(vega_i18n::t("ui.back_to_bottom"))
                        .track_focus(&self.resume_tail_focus)
                        .tab_stop(true)
                        .key_context("ResumeTailButton")
                        .on_action(cx.listener(Self::on_resume_tail))
                        .flex_shrink_0()
                        .size(px(Layout::SCROLL_TO_BOTTOM_SIZE))
                        .tooltip(|_, cx| {
                            crate::icons::tooltip(vega_i18n::t("ui.back_to_bottom"), cx)
                        })
                        .rounded(px(Layout::SCROLL_TO_BOTTOM_SIZE / 2.0))
                        .border_1()
                        .border_color(colors.border_subtle)
                        .bg(colors.bg_elevated)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(Typography::BODY))
                        .text_color(colors.text_secondary)
                        .cursor_pointer()
                        .interaction_surface(
                            colors.bg_elevated,
                            colors.bg_hover,
                            colors.bg_active,
                            true,
                        )
                        .focus_visible(move |style| style.border_color(colors.accent))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::on_resume_tail_clicked))
                        .child(crate::icons::icon(
                            crate::icons::Icon::ArrowDown,
                            colors.text_secondary,
                        )),
                )
            })
            .into_any_element()
    }

    fn render_composer(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if self.automation_permission.busy() {
            return self.render_automation_permission(window, cx);
        }
        let colors = theme(cx).colors;
        let file_retry_visible = self.file_selector_wanted && self.file_index_failure.is_some();
        let file_selector_active =
            self.file_selector.is_open() || self.file_selector_wanted || self.file_index_loading;
        let resume = self.shows_composer_resume(cx);
        let can_send = if resume {
            self.can_resume_composer(cx)
        } else {
            self.can_submit_message(cx)
        };
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
                    }),
            )
            .children(
                self.reasoning_default_failures
                    .iter()
                    .enumerate()
                    .map(|(index, failure)| {
                        let model = failure.request.defaults.model.clone();
                        let key_model = model.clone();
                        let click_model = model.clone();
                        div()
                            .w_full()
                            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                            .mx_auto()
                            .mt_1()
                            .mb_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(render_rows::plain_error_notice(
                                        vega_i18n::t_args(
                                            "ui.reasoning_default_not_saved",
                                            &[("model", &model)],
                                        ),
                                        colors.danger,
                                    )),
                            )
                            .when(failure.retry_pending, |row| {
                                row.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_color(colors.text_secondary)
                                        .child(vega_i18n::t("ui.saving")),
                                )
                            })
                            .children(self.reasoning_default_retry_focuses.get(&model).map(
                                |focus| {
                                    div()
                                        .id(format!("reasoning-default-retry-{model}"))
                                        .role(Role::Button)
                                        .aria_label(vega_i18n::t_args(
                                            "ui.reasoning_default_retry",
                                            &[("model", &model)],
                                        ))
                                        .focusable()
                                        .track_focus(focus)
                                        .tab_stop(true)
                                        .focus_visible(|style| {
                                            style
                                                .bg(colors.bg_active)
                                                .text_color(colors.text_primary)
                                        })
                                        .flex_shrink_0()
                                        .px_2()
                                        .h(px(Layout::TITLEBAR_CONTROL_SIZE))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(Layout::DISCLOSURE_RADIUS))
                                        .text_color(colors.text_primary)
                                        .bg(colors.bg_hover)
                                        .cursor_pointer()
                                        .on_key_down(cx.listener(
                                            move |this,
                                                  event: &gpui_kit::KeyDownEvent,
                                                  window,
                                                  cx| {
                                                if matches!(
                                                    event.keystroke.key.as_str(),
                                                    "enter" | "space" | " "
                                                ) {
                                                    this.request_reasoning_default_retry(
                                                        &key_model, cx,
                                                    );
                                                    window.focus(
                                                        &this.input.read(cx).focus_handle(cx),
                                                        cx,
                                                    );
                                                    cx.stop_propagation();
                                                }
                                            },
                                        ))
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                                this.request_reasoning_default_retry(
                                                    &click_model,
                                                    cx,
                                                );
                                            }),
                                        )
                                        .on_action(cx.listener(
                                            move |this, _: &NextComposerControl, window, cx| {
                                                this.focus_reasoning_retry_neighbor(
                                                    index, false, window, cx,
                                                );
                                            },
                                        ))
                                        .on_action(cx.listener(
                                            move |this, _: &PreviousComposerControl, window, cx| {
                                                this.focus_reasoning_retry_neighbor(
                                                    index, true, window, cx,
                                                );
                                            },
                                        ))
                                        .child(vega_i18n::t("ui.retry"))
                                },
                            ))
                    }),
            )
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
                    .bg(colors.bg_elevated)
                    .border_1()
                    .border_color(colors.border_subtle)
                    .rounded(px(Layout::COMPOSER_RADIUS))
                    .p_3()
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
                            .when(self.actions.visible(), |row| {
                                row.key_context("ComposerActions")
                            })
                            .child(self.render_composer_actions(cx))
                            .child(self.render_file_dropdown(cx))
                            .min_h(px(40.))
                            .child(self.input.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .when(self.compact_workspace, |row| row.gap_1())
                            .child(
                                crate::icons::localized_icon_button(
                                    crate::icons::Icon::Plus,
                                    "ui.add_context_or_change_mode",
                                    colors,
                                    cx.listener(|this, _, window, cx| {
                                        this.open_composer_actions(window, cx)
                                    }),
                                )
                                .size(px(Layout::TITLEBAR_CONTROL_SIZE))
                                .track_focus(&self.action_focus[0]),
                            )
                            .when(self.thread.backend == TaskBackend::Native, |row| {
                                row.child(self.render_permission_status(cx))
                            })
                            .when(self.thread.backend == TaskBackend::Codex, |row| {
                                row.child(self.render_backend_status(cx))
                            })
                            .child(div().min_w_0().flex_1())
                            .when(self.thread.backend == TaskBackend::Native, |row| {
                                row.child(self.render_model_controls(window, cx))
                            })
                            .when(
                                !self.composer_resume_pending && self.shows_running_send(cx),
                                |row| row.child(self.render_running_send(can_send, cx)),
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
                                |row| {
                                    row.child(
                                        div()
                                            .id("composer-resume-pending")
                                            .role(Role::Button)
                                            .aria_label(vega_i18n::t("ui.composer_resume"))
                                            .track_focus(&self.action_focus[1])
                                            .tab_stop(true)
                                            .focus_visible(move |style| {
                                                style
                                                    .border_2()
                                                    .border_color(colors.brand_primary_strong)
                                            })
                                            .size(px(Layout::COMPOSER_SEND_SIZE))
                                            .flex_shrink_0()
                                            .rounded_full()
                                            .bg(colors.bg_hover)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                gpui_kit::component::spinner::Spinner::new()
                                                    .ease(gpui_kit::linear)
                                                    .with_size(px(Layout::ICON_SIZE))
                                                    .icon(
                                                        gpui_kit::component::IconName::LoaderCircle,
                                                    )
                                                    .color(colors.text_secondary.into()),
                                            ),
                                    )
                                },
                            )
                            .when(
                                !self.actions.running
                                    && !self.composer_submit_pending
                                    && !self.composer_resume_pending
                                    && !self.ordinary_pending_before_run()
                                    && !self.has_cancellable_queue_wait(),
                                |row| {
                                    row.child(
                                        div()
                                            .id("composer-send")
                                            .role(Role::Button)
                                            .aria_label(vega_i18n::t(if resume {
                                                "ui.composer_resume"
                                            } else {
                                                "ui.send"
                                            }))
                                            .when(resume, |button| {
                                                button.tooltip(|_, cx| {
                                                    crate::icons::tooltip(
                                                        vega_i18n::t("ui.composer_resume"),
                                                        cx,
                                                    )
                                                })
                                            })
                                            .track_focus(&self.action_focus[1])
                                            .tab_stop(true)
                                            .focus_visible(move |style| {
                                                style
                                                    .border_2()
                                                    .border_color(colors.brand_primary_strong)
                                            })
                                            .on_key_down(cx.listener(
                                                |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                                    if matches!(
                                                        event.keystroke.key.as_str(),
                                                        "enter" | "space" | " "
                                                    ) {
                                                        this.activate_composer_primary(cx);
                                                        cx.stop_propagation();
                                                    }
                                                },
                                            ))
                                            .flex_shrink_0()
                                            .size(px(Layout::COMPOSER_SEND_SIZE))
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(can_send, |button| {
                                                button
                                                    .bg(colors.accent)
                                                    .text_color(colors.brand_on_accent)
                                                    .cursor_pointer()
                                                    .on_mouse_up(
                                                        MouseButton::Left,
                                                        cx.listener(Self::on_send_clicked),
                                                    )
                                            })
                                            .when(!can_send, |button| {
                                                button
                                                    .bg(colors.bg_hover)
                                                    .text_color(colors.text_tertiary)
                                            })
                                            .interaction_surface(
                                                if can_send {
                                                    colors.accent
                                                } else {
                                                    colors.bg_hover
                                                },
                                                colors.brand_primary_strong,
                                                colors.accent,
                                                can_send,
                                            )
                                            .child(crate::icons::icon(
                                                if resume {
                                                    crate::icons::Icon::Play
                                                } else {
                                                    crate::icons::Icon::ArrowUp
                                                },
                                                if can_send {
                                                    colors.brand_on_accent
                                                } else {
                                                    colors.text_tertiary
                                                },
                                            )),
                                    )
                                },
                            ),
                    ),
            )
            .child(self.render_active_skills(cx))
            .into_any_element()
    }

    fn render_permission_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mode = self.thread.permission_mode;
        let progress_label = self.permission_editor.progress_label();
        let saving = progress_label.is_some();
        let label = permission_label(mode);
        let enabled = !self.permission_change_pending();

        let glyph = permission_icon(mode);

        let ink = if permission_is_warning(mode) {
            colors.warning
        } else {
            colors.text_secondary
        };
        div()
            .relative()
            .flex()
            .flex_shrink_0()
            .child(
                div()
                    .id("composer-permission-status")
                    .role(gpui_kit::Role::Button)
                    .aria_label(progress_label.unwrap_or(vega_i18n::t("ui.change_permission_mode")))
                    .track_focus(&self.permission_editor.trigger)
                    .tab_stop(enabled)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px(px(Layout::COMPOSER_TEXT_TRIGGER_PADDING_X))
                    .h(px(Layout::COMPOSER_TEXT_TRIGGER_HEIGHT))
                    .min_w(px(Layout::COMPOSER_TEXT_TRIGGER_HEIGHT))
                    .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
                    .rounded(px(Layout::COMPOSER_TEXT_TRIGGER_RADIUS))
                    .text_size(px(Typography::BODY))
                    .text_color(ink)
                    .cursor_pointer()
                    .interactive(colors, enabled, self.permission_picker_open)
                    .focus_visible(move |style| style.border_1().border_color(colors.accent))
                    .tooltip(move |_, cx| {
                        crate::icons::tooltip(vega_i18n::t("ui.change_permission_mode"), cx)
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(Self::toggle_permission_picker),
                    )
                    .on_key_down(
                        cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                && !this.permission_change_pending()
                            {
                                this.open_permission_picker(window, cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .when(saving, |slot| {
                                slot.child(
                                    gpui_kit::component::spinner::Spinner::new()
                                        .ease(gpui_kit::linear)
                                        .with_size(px(Layout::ICON_SIZE))
                                        .icon(gpui_kit::component::IconName::LoaderCircle),
                                )
                            })
                            .when(!saving, |slot| slot.child(crate::icons::icon(glyph, ink))),
                    )
                    .child(label),
            )
            .when(self.permission_picker_open, |chip| {
                chip.child(self.render_permission_picker(cx))
            })
            .into_any_element()
    }

    fn render_backend_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let label = if self.codex_mode_confirmed {
            "Codex ACP · workspace-write · on-request"
        } else {
            "Codex ACP"
        };
        div()
            .id("composer-backend-status")
            .aria_label(label)
            .min_w_0()
            .flex_shrink_1()
            .h(px(Layout::TITLEBAR_CONTROL_SIZE))
            .flex()
            .items_center()
            .text_size(px(Typography::BODY))
            .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
            .text_color(colors.text_secondary)
            .tooltip(move |_, cx| crate::icons::tooltip(label, cx))
            .child(div().min_w_0().truncate().child(label))
            .into_any_element()
    }

    fn render_permission_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let current = self.thread.permission_mode;
        let active_mode = self
            .permission_editor
            .active_mode
            .filter(|mode| *mode != current);
        let rows = PERMISSION_ORDER
            .into_iter()
            .enumerate()
            .map(|(index, mode)| {
                let selected = mode == current;
                let warning = permission_is_warning(mode);
                let ink = if warning {
                    colors.warning
                } else {
                    colors.text_primary
                };
                let icon_ink = if warning {
                    colors.warning
                } else {
                    colors.text_secondary
                };
                div()
                    .id(("composer-permission-option", index))
                    .role(gpui_kit::Role::Button)
                    .aria_label(permission_label(mode))
                    .track_focus(&self.permission_editor.options[index])
                    .tab_stop(true)
                    .focus_visible(move |style| style.border_1().border_color(colors.accent))
                    .min_h(px(44.))
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .when(selected, |row| {
                        row.bg(crate::menu_list::selected_row_bg(colors))
                    })
                    .cursor_pointer()
                    .hover(move |row| row.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseUpEvent, window, cx| {
                            this.select_permission_mode(mode, window, cx);
                        }),
                    )
                    .on_key_down(cx.listener(
                        move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.select_permission_mode(mode, window, cx);
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .child(crate::icons::icon(permission_icon(mode), icon_ink))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(Typography::SIDEBAR))
                                    .text_color(ink)
                                    .child(permission_label(mode)),
                            )
                            .child(
                                div()
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.text_tertiary)
                                    .child(permission_description(mode)),
                            ),
                    )
                    // Fixed marker column: the selected row carries the check, and
                    // an unselected row reserves the same width so labels stay on
                    // one x axis.
                    .child(
                        div()
                            .w(px(Typography::SIDEBAR))
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .when(selected, |marker| {
                                marker.child(crate::icons::icon(
                                    crate::icons::Icon::Check,
                                    colors.brand_primary,
                                ))
                            }),
                    )
            });

        gpui_kit::deferred(
            div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .absolute()
                .bottom(gpui_kit::relative(1.0))
                .mb(px(Layout::COMPOSER_PICKER_TRIGGER_GAP))
                .left_0()
                .w(px(Layout::MENU_MAX_WIDTH))
                .occlude()
                .flex()
                .flex_col()
                .rounded(px(Layout::MENU_RADIUS))
                .border_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_elevated)
                .text_color(colors.text_primary)
                .shadow_sm()
                .child(
                    div()
                        .flex_shrink_0()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .flex()
                        .items_start()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(Typography::METADATA))
                                .text_color(colors.text_secondary)
                                .child(vega_i18n::t(if self.actions.running {
                                    "ui.permission_picker_next_run"
                                } else {
                                    PERMISSION_PICKER_TITLE
                                })),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(Typography::METADATA))
                                .text_color(colors.text_tertiary)
                                .child(vega_i18n::t(PERMISSION_PICKER_LEARN_MORE)),
                        ),
                )
                .when_some(active_mode, |menu, mode| {
                    menu.child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .child(vega_i18n::t_args(
                                "ui.permission_current_run",
                                &[("mode", permission_label(mode))],
                            )),
                    )
                })
                .child(
                    div()
                        .flex_shrink_0()
                        .mx_2()
                        .h(px(1.))
                        .bg(colors.border_subtle),
                )
                .child(div().flex_shrink_0().p_1().flex().flex_col().children(rows)),
        )
        .with_priority(2)
        .into_any_element()
    }

    pub(crate) fn toggle_permission_picker(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.permission_change_pending() {
            return;
        }
        let open = !self.permission_picker_open;
        self.close_composer_popovers(cx);
        if open {
            self.open_permission_picker(window, cx);
        } else {
            window.focus(&self.permission_editor.trigger, cx);
        }
        cx.notify();
    }

    fn open_permission_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_composer_popovers(cx);
        self.permission_picker_open = true;
        let index = PERMISSION_ORDER
            .iter()
            .position(|mode| *mode == self.thread.permission_mode)
            .unwrap_or(1);
        window.focus(&self.permission_editor.options[index], cx);
        cx.notify();
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
        let colors = theme(cx).colors;
        if !self.file_selector.is_open() && !self.file_selector_wanted && !self.file_index_loading {
            return div().into_any_element();
        }
        let highlighted = self.file_selector.highlighted();
        let content = if self.file_index_loading {
            div()
                .px_2()
                .py_2()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_secondary)
                .child(vega_i18n::t("ui.indexing_files"))
                .into_any_element()
        } else if let Some(code) = self.file_index_failure {
            div()
                .flex()
                .flex_row()
                .items_start()
                .justify_between()
                .gap_2()
                .p(px(Layout::STATUS_PADDING))
                .rounded(px(Layout::STATUS_RADIUS))
                .border_1()
                .border_color(colors.border_subtle)
                .text_size(px(Typography::BODY))
                .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
                .child(
                    div()
                        .mt(px((Typography::BODY * Typography::BODY_LINE_HEIGHT
                            - Layout::ICON_SIZE)
                            / 2.0))
                        .size(px(Layout::ICON_SIZE))
                        .flex_shrink_0()
                        .child(crate::icons::icon(
                            crate::icons::Icon::Warning,
                            colors.danger,
                        )),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(colors.danger)
                        .child(code.message()),
                )
                .child(
                    div()
                        .id("file-index-retry")
                        .flex_shrink_0()
                        .px_2()
                        .h(px(Layout::TITLEBAR_CONTROL_SIZE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(Layout::DISCLOSURE_RADIUS))
                        .text_color(colors.text_primary)
                        .bg(colors.bg_hover)
                        .aria_label(vega_i18n::t("ui.retry_file_indexing"))
                        .focusable()
                        .track_focus(&self.file_retry_focus)
                        .tab_stop(true)
                        .focus_visible(|style| {
                            style.bg(colors.bg_active).text_color(colors.text_primary)
                        })
                        .cursor_pointer()
                        .key_context("FileSelectRetryButton")
                        .on_action(cx.listener(Self::on_selector_retry_action))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::on_selector_retry))
                        .child(vega_i18n::t("ui.retry")),
                )
                .into_any_element()
        } else if !self.file_selector.is_open() {
            div()
                .px_2()
                .py_2()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_secondary)
                .child(vega_i18n::t("ui.no_matching_files"))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .children(
                    self.file_selector
                        .candidates()
                        .iter()
                        .take(FILE_SUGGESTION_LIMIT)
                        .enumerate()
                        .map(|(index, entry)| {
                            let selected = index == highlighted;
                            let entry = entry.clone();
                            div()
                                .px_2()
                                .py_1()
                                .text_size(px(Typography::SIDEBAR))
                                .truncate()
                                .cursor_pointer()
                                .when(selected, |row| row.bg(colors.bg_active))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                        this.on_selector_click(index, cx)
                                    }),
                                )
                                .text_color(if selected {
                                    colors.brand_primary
                                } else {
                                    colors.text_secondary
                                })
                                .child(entry)
                        }),
                )
                .into_any_element()
        };
        // The containing block is the whole input row. Anchor the popup's
        // bottom to its top, rather than covering the draft at bottom: 0.
        // Defer paint/hit-testing so the transcript cannot cover candidates.
        gpui_kit::deferred(
            div()
                .absolute()
                .bottom(gpui_kit::relative(1.0))
                .mb_2()
                .left_0()
                .w(px(Layout::MENU_MAX_WIDTH))
                .max_w_full()
                .occlude()
                .flex()
                .flex_col()
                .rounded(px(Layout::MENU_RADIUS))
                .border_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_elevated)
                .text_color(colors.text_primary)
                .shadow_sm()
                .child(content),
        )
        .with_priority(2)
        .into_any_element()
    }

    fn render_model_controls(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let display = self.context_usage_display();
        let colors = theme(cx).colors;
        div()
            .min_w_0()
            .flex_shrink_1()
            .flex()
            .items_center()
            .gap_1()
            .children(
                self.context_usage_visible()
                    .then(|| self.render_context_usage_indicator(&display, colors, window, cx)),
            )
            .child(self.render_model_selector(window, cx))
            .into_any_element()
    }

    fn render_context_usage_indicator(
        &self,
        display: &ContextUsageDisplay,
        colors: ThemeColors,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("composer-context-usage-hover-target")
            .relative()
            .flex_shrink_0()
            .size(px(Layout::CONTEXT_USAGE_SIZE))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.context_usage_trigger_hovered = *hovered;
                cx.notify();
            }))
            .child(
                div()
                    .id("composer-context-usage")
                    .aria_label(display.accessibility_label())
                    .focusable()
                    .track_focus(&self.context_usage_focus)
                    .tab_stop(true)
                    .size_full()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(move |style| style.bg(colors.bg_hover))
                    .focus_visible(move |style| style.bg(colors.bg_hover))
                    .child(
                        div()
                            .size(px(Layout::ICON_SIZE))
                            .flex_shrink_0()
                            .child(Self::context_usage_ring(display, colors)),
                    ),
            )
            .children(self.context_usage_trigger_hovered.then(|| {
                let lines = [
                    vega_i18n::t("ui.context_window").to_string(),
                    display.tooltip_status(),
                    display.tooltip_usage(),
                ];
                let text_width = lines
                    .iter()
                    .map(|line| {
                        window
                            .text_system()
                            .shape_line(
                                line.clone().into(),
                                px(Typography::BODY),
                                &[window.text_style().to_run(line.len())],
                                None,
                            )
                            .width()
                    })
                    .max()
                    .unwrap_or_default();
                let width = (text_width
                    + px(2.0 * (Layout::CONTEXT_USAGE_PADDING + Layout::CONTEXT_USAGE_BORDER)))
                .ceil()
                .max(px(Layout::CONTEXT_USAGE_MIN_WIDTH));
                div().absolute().top_0().left_0().size(px(0.0)).child(
                    div().relative().child(
                        anchored()
                            .anchor(Anchor::BottomLeft)
                            .position_mode(AnchoredPositionMode::Local)
                            .position(point(
                                (px(Layout::CONTEXT_USAGE_SIZE) - width) / 2.0,
                                px(-Layout::CONTEXT_USAGE_GAP),
                            ))
                            .snap_to_window()
                            .child(
                                gpui_kit::deferred(Self::context_usage_tooltip_element(
                                    &lines, width, colors,
                                ))
                                .with_priority(2),
                            ),
                    ),
                )
            }))
            .into_any_element()
    }

    fn context_usage_ring(display: &ContextUsageDisplay, colors: ThemeColors) -> AnyElement {
        let fill_fraction = display.fill_fraction;
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let center = bounds.center();
                let mut track = PathBuilder::stroke(px(2.0));
                for step in 0..=64 {
                    let angle =
                        -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * step as f32 / 64.0;
                    let position = center + point(px(angle.cos() * 6.0), px(angle.sin() * 6.0));
                    if step == 0 {
                        track.move_to(position);
                    } else {
                        track.line_to(position);
                    }
                }
                if let Ok(path) = track.build() {
                    window.paint_path(path, colors.border_subtle);
                }
                if fill_fraction <= 0.0 {
                    return;
                }
                let mut progress = PathBuilder::stroke(px(2.0));
                for step in 0..=64 {
                    let angle = -std::f32::consts::FRAC_PI_2
                        + std::f32::consts::TAU * fill_fraction * step as f32 / 64.0;
                    let position = center + point(px(angle.cos() * 6.0), px(angle.sin() * 6.0));
                    if step == 0 {
                        progress.move_to(position);
                    } else {
                        progress.line_to(position);
                    }
                }
                if let Ok(path) = progress.build() {
                    window.paint_path(path, colors.accent);
                }
            },
        )
        .size(px(16.0))
        .into_any_element()
    }

    fn context_usage_tooltip_element(
        lines: &[String; 3],
        width: gpui_kit::Pixels,
        colors: ThemeColors,
    ) -> impl IntoElement {
        div()
            .id("context-usage-tooltip")
            .w(width)
            .flex_shrink_0()
            .whitespace_nowrap()
            .px(px(Layout::CONTEXT_USAGE_PADDING))
            .py(px(Layout::CONTEXT_USAGE_VERTICAL_PADDING))
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .rounded(px(Layout::CONTEXT_USAGE_RADIUS))
            .border(px(Layout::CONTEXT_USAGE_BORDER))
            .border_color(colors.context_tooltip_border)
            .bg(colors.context_tooltip_bg)
            .shadow_md()
            .text_size(px(Typography::BODY))
            .line_height(px(Layout::CONTEXT_USAGE_LINE_HEIGHT))
            .text_color(colors.context_tooltip_secondary)
            .child(div().child(lines[0].clone()))
            .children(lines[1..].iter().map(|line| {
                div()
                    .text_color(colors.context_tooltip_primary)
                    .child(line.clone())
            }))
    }

    fn render_model_selector(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let enabled = !self.trusted_action_busy
            && !self.model_selection_save_busy()
            && self.model_selection_pending.is_none();

        let model = self.selector_model();
        let current = model_trigger_label(
            model,
            self.model_picker_level,
            self.model_selection_pending.is_some(),
        );
        let label_width = [
            current,
            model_trigger_label(model, ModelPickerLevel::Closed, false),
        ]
        .into_iter()
        .chain(
            self.thinking_slider
                .read(cx)
                .has_tiers()
                .then(|| model_trigger_label(model, ModelPickerLevel::Slider, false)),
        )
        .flat_map(str::lines)
        .map(|label| {
            window
                .text_system()
                .shape_line(
                    label.to_owned().into(),
                    px(Typography::SIDEBAR),
                    &[window.text_style().to_run(label.len())],
                    None,
                )
                .width()
        })
        .max()
        .unwrap_or_default()
        .ceil();
        div()
            .relative()
            .flex()
            .min_w_0()
            .flex_shrink_1()
            .child(
                div()
                    .id("composer-model")
                    .aria_label(current.to_owned())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .track_focus(&self.model_focus)
                    .key_context("ModelSelector")
                    .on_action(cx.listener(Self::on_activate_model))
                    .on_action(cx.listener(Self::on_model_previous))
                    .on_action(cx.listener(Self::on_model_next))
                    .on_action(cx.listener(Self::on_model_close))
                    .flex()
                    .items_center()
                    .justify_center()
                    .min_w(px(Layout::COMPOSER_TEXT_TRIGGER_HEIGHT))
                    .flex_shrink_1()
                    .truncate()
                    .px(px(Layout::COMPOSER_TEXT_TRIGGER_PADDING_X))
                    .h(px(Layout::COMPOSER_TEXT_TRIGGER_HEIGHT))
                    .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
                    .rounded(px(Layout::COMPOSER_TEXT_TRIGGER_RADIUS))
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if enabled {
                        colors.text_secondary
                    } else {
                        colors.text_tertiary
                    })
                    .when(enabled, |trigger| trigger.cursor_pointer())
                    .interactive(colors, enabled, self.model_picker_level.is_open())
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, window, cx| {
                            this.on_activate_model(&ActivateModel, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .w(label_width)
                                    .min_w_0()
                                    .flex_shrink_1()
                                    .truncate()
                                    .text_center()
                                    .child(current.to_owned()),
                            )
                            .child(crate::icons::icon(
                                crate::icons::Icon::ChevronDown,
                                colors.text_tertiary,
                            )),
                    ),
            )
            .child(self.render_model_picker_layers(cx))
            .into_any_element()
    }

    fn render_model_picker_layers(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.model_picker_level {
            ModelPickerLevel::Closed => div().into_any_element(),
            ModelPickerLevel::Slider => self.render_picker_slider_layer(cx),
            ModelPickerLevel::List => self.render_picker_list_layer(cx),
        }
    }

    fn picker_max_height(&self) -> Pixels {
        px(Layout::COMPOSER_PICKER_MAX_HEIGHT)
    }

    fn render_picker_slider_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.thinking_slider.read(cx).has_tiers() {
            return div().into_any_element();
        }

        gpui_kit::deferred(
            div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .absolute()
                .bottom(gpui_kit::relative(1.0))
                .mb(px(Layout::COMPOSER_PICKER_TRIGGER_GAP))
                .right_0()
                .max_h(self.picker_max_height())
                .occlude()
                .flex()
                .flex_col()
                .child(self.thinking_slider.clone()),
        )
        .with_priority(2)
        .into_any_element()
    }

    fn render_picker_list_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;

        gpui_kit::deferred(
            div()
                .id("composer-model-menu")
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .absolute()
                .bottom(gpui_kit::relative(1.0))
                .mb(px(Layout::COMPOSER_PICKER_TRIGGER_GAP))
                .right_0()
                .w(px(Layout::MENU_MAX_WIDTH))
                .max_h(self.picker_max_height())
                .occlude()
                .flex()
                .flex_col()
                .rounded(px(Layout::MENU_RADIUS))
                .border_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_elevated)
                .text_color(colors.text_primary)
                .shadow_sm()
                .child(
                    div()
                        .flex_shrink_0()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .child(vega_i18n::t(PICKER_LIST_HEADING)),
                )
                // Only the rows scroll: the heading stays visible so the layer never
                // becomes an unbounded column of model names.
                .child(
                    div()
                        .id("composer-model-rows")
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.model_menu_scroll)
                        .children(self.model_picker_rows(colors, cx))
                        .tracked_scroll_edge_fade(
                            self.model_menu_scroll.clone(),
                            colors.bg_elevated,
                            ScrollFade::compact(),
                        ),
                ),
        )
        .with_priority(2)
        .into_any_element()
    }

    fn model_picker_rows(&self, colors: ThemeColors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        for (index, option) in self.model_options.iter().enumerate() {
            if model_picker_starts_group(&self.model_options, index) {
                let first_group = index == 0;
                rows.push(
                    div()
                        .h(px(Layout::COMPOSER_PICKER_GROUP_HEIGHT))
                        .flex_shrink_0()
                        .px_2()
                        .flex()
                        .items_center()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .when(!first_group, |group| {
                            group.mt(px(Layout::COMPOSER_PICKER_GROUP_GAP))
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(option.provider.clone()),
                        )
                        .into_any_element(),
                );
            }
            let selected = index == self.model_selector_highlight;
            let current_model = option.model == self.selector_model();
            let model = option.model.clone();
            let label = option.model.clone();
            rows.push(
                div()
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex_shrink_0()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(Typography::SIDEBAR))
                    .when(selected, |row| row.bg(colors.bg_active))
                    .text_color(if current_model {
                        colors.brand_primary
                    } else if selected {
                        colors.text_primary
                    } else {
                        colors.text_secondary
                    })
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseUpEvent, _: &mut Window, cx| {
                            this.select_model_option(&model, cx);
                        }),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .child(crate::menu_list::selection_marker(current_model, colors))
                    .into_any_element(),
            );
        }
        rows
    }

    fn message_location_status_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
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
        Some(
            div()
                .id("message-anchor-status")
                .aria_label(label)
                .w_full()
                .px_2()
                .py_1()
                .text_size(px(Typography::METADATA))
                .text_color(color)
                .child(label)
                .into_any_element(),
        )
    }
}

impl Render for ConversationStream {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.reading_preview {
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
                if let StreamEntry::User { copy, lines } = entry
                    && copy.refresh_live_projection()
                {
                    let block = self.user_block_seq;
                    self.user_block_seq += 1;
                    *lines = user_message_lines(block, &copy.original_text());
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
                .h(px(72.))
                .mb(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(Typography::EMPTY_STATE_TITLE))
                        .font_weight(Typography::EMPTY_STATE_TITLE_WEIGHT)
                        .text_color(colors.text_primary)
                        .child(vega_i18n::t("ui.what_would_you_like_to_do_today")),
                )
                .child(
                    div()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(vega_i18n::t("ui.enter_a_task_to_start_or_use_to")),
                )
                .children(self.message_location_status_element(cx))
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
                                    list(
                                        self.list.clone(),
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
                                                    div().text_size(px(Typography::BODY)).text_color(theme(cx).colors.text_secondary).child(format!("{status} · {tool} · {target}")).into_any_element()
                                                } else if this.reading_preview && let Some(StreamEntry::Plan { card }) = entry {
                                                    PlanCard::render_read_only(card.clone(), window, cx)
                                                } else if this.process_entry_hidden(index, cx) || this.compaction_failure_is_represented(index) {
                                                    div().h_0().into_any_element()
                                                } else {
                                                    match entry {
                                                        Some(StreamEntry::RunActivity {
                                                            group,
                                                        }) => {
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
                                                            pinned = group.read(cx).scroll_fade_pinned_indicator(Some(*segment));
                                                            RunActivityGroup::render_segment_with_top(group.clone(), *segment, stage_top_insets.get(&index).copied().unwrap_or(Layout::CONVERSATION_STAGE_TOP), window, cx)
                                                        },
                                                        Some(entry) => {
                                                            this.prepare_partial_compaction_menu(index, cx);
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
                                                                this.message_interaction(),
                                                            );
                                                            let user = matches!(entry, StreamEntry::User { .. });
                                                            let source = match entry {
                                                                StreamEntry::User { copy, .. } => Some(copy),
                                                                _ => this.action_copy_source_for_entry(index),
                                                            };
                                                            render_rows::message_with_copy(body, source, render_rows::MessageActions {
                                                                id: this.entry_identities.get(index).map(|id| id.key.clone()).unwrap_or_else(|| format!("message-{index}")).into(),
                                                                user, colors: theme(cx).colors, enabled: this.message_actions_enabled(),
                                                                timestamp: this.entry_timestamp(index), focus: this.timestamp_focus(index),
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
                                    .pb(self.foreground_height)
                                    .tracked_scroll_edge_fade(self.list.clone(), colors.bg_base, ScrollFade::reading().top_only().vertical_list())
                                    .preserve_pinned_headers(top_fade_guards),
                                ),
                        ),
                )
                .into_any_element()
        };

        let project_bound = !self.thread.is_standalone();
        let element = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .relative()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .key_context("ConversationStream")
            .track_focus(&self.selection_focus)
            .capture_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                if this.context_usage_trigger_hovered && event.keystroke.key == "escape" {
                    this.context_usage_trigger_hovered = false;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
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
            .when(!self.reading_preview, |root| {
                root.children(self.render_subagent_parent(cx))
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
                .when(project_bound, |root| root.child(self.commit_panel.clone()))
            })
            .when(self.reading_preview, |root| {
                root.child(self.render_resume_tail(cx))
            })
            .when(!self.reading_preview, |root| {
                root.children(self.render_pending_menu(cx))
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
