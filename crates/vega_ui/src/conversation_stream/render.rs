use super::context_usage::ContextUsageDisplay;
use super::*;
use crate::interaction::InteractionExt;
use gpui_kit::{Anchor, AnchoredPositionMode, PathBuilder, anchored, canvas, point};
// Re-exported so the permission picker's copy is testable from the suite the
// same way `PICKER_EFFORT_LABEL`/`PICKER_LIST_HEADING` are.
use super::composer_actions::{
    PERMISSION_ORDER, permission_description, permission_icon, permission_icon_selector,
    permission_is_warning, permission_label,
};
pub(crate) use super::composer_actions::{PERMISSION_PICKER_LEARN_MORE, PERMISSION_PICKER_TITLE};

/// R59 R7: the trigger label while the tier slider (level one) is showing.
///
/// The reference implementation's `composer.modelPicker.selectEffort.label`
/// ("Select effort") is shown "while adjusting the reasoning effort of an
/// explicitly selected model". Vega's UI is Chinese and its existing wording
/// for this concept is 档位/强度 (`settings/reasoning_render.rs`, R57 §2.2),
/// so 选择强度 is the in-convention counterpart rather than a literal
/// translation.
pub(crate) const PICKER_EFFORT_LABEL: &str = "选择强度";

/// R59 R2: the level-two heading. The reference implementation's
/// `composer.modelPicker.modelList.heading` is "Select model".
pub(crate) const PICKER_LIST_HEADING: &str = "选择模型";

/// R59 R7: the model trigger's label for one composer state.
///
/// Split out of the view so the rule is directly testable, and so the four
/// states cannot drift from their documentation:
/// 1. a durable save in flight shows the bounded pending text;
/// 2. a model with no name yet shows the generic 模型 placeholder;
/// 3. **the slider level shows [`PICKER_EFFORT_LABEL`]** — this is the R59 R7
///    case, the one the reference implementation covers with
///    `composer.modelPicker.selectEffort.label`;
/// 4. every other state shows the selected model name, exactly as before.
pub(crate) fn model_trigger_label(
    model: &str,
    level: ModelPickerLevel,
    save_pending: bool,
) -> &str {
    if save_pending {
        return "保存中…";
    }
    if model.is_empty() {
        return "模型";
    }
    if level == ModelPickerLevel::Slider {
        return PICKER_EFFORT_LABEL;
    }
    model
}

impl ConversationStream {
    fn emit_open_diff(&mut self, cx: &mut Context<Self>) {
        if self.thread.is_standalone() {
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
            .debug_selector(|| "conversation-foreground".into())
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
                    .debug_selector(|| "conversation-bottom-fade".into())
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
                    .debug_selector(|| "conversation-foreground-surface".into())
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
            .debug_selector(|| "conversation-reading-navigation".into())
            .h(px(Layout::READING_NAVIGATION_HEIGHT))
            .flex_shrink_0()
            .py(px(Layout::READING_NAVIGATION_PADDING))
            .flex()
            .justify_center()
            .when(!self.following_tail(), |row| {
                row.child(
                    div()
                        .id("scroll-to-bottom")
                        .debug_selector(|| "scroll-to-bottom".into())
                        .aria_label("回到底部")
                        .track_focus(&self.resume_tail_focus)
                        .tab_stop(true)
                        .key_context("ResumeTailButton")
                        .on_action(cx.listener(Self::on_resume_tail))
                        .flex_shrink_0()
                        .size(px(Layout::SCROLL_TO_BOTTOM_SIZE))
                        .tooltip(|_, cx| crate::icons::tooltip("回到底部", cx))
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

    /// Renders the R19 composer: one growing input followed by one action row
    /// with the existing context, permission status, model and send/stop
    /// handlers. Project identity and branch controls live in the window
    /// shell, so this surface never duplicates them or invents state.
    ///
    /// R57 P2b (spec §2.3 R1–R5) freezes the bottom row to
    /// `+` | permission status | spacer | model | send/stop: the `Execute`
    /// dropdown and the thinking chip are gone, and the permission control is
    /// static text (not clickable). Thread mode and permission mode stay
    /// reachable through the `+` menu (`composer_actions.rs`).
    fn render_composer(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let file_retry_visible = self.file_selector_wanted && self.file_index_failure.is_some();
        let file_selector_active =
            self.file_selector.is_open() || self.file_selector_wanted || self.file_index_loading;
        let can_send = self.can_submit_message(cx);
        div()
            .debug_selector(|| "composer-wrapper".into())
            .px(px(Layout::CONVERSATION_PADDING))
            .pt(px(Layout::COMPOSER_PADDING_TOP))
            .pb(px(Layout::COMPOSER_PADDING_BOTTOM))
            .flex_shrink_0()
            // R61 R2: the composer column no longer hosts the picker's
            // floating layers — each layer is mounted inside the model
            // trigger's own `.relative()` wrapper (`render_model_selector`),
            // which is what makes the card hug the trigger. R59 pinned them
            // here instead, to the column's top edge, to keep the R49 utility
            // bar clear; the user ruled that trade-off out (R61 §2), so the
            // column keeps no picker-related positioning role.
            // R49: the utility bar is a real sibling above the card on the
            // new-task page (zero overlap, no negative margin). The session
            // page renders the card alone, exactly like Codex.
            .when(self.utility_bar_visible(cx), |column| {
                column.child(self.render_composer_utility_bar(window, cx))
            })
            .children(
                self.controller_error
                    .clone()
                    .filter(|_| !self.controller_error_is_run_notice())
                    .map(|error| {
                        div()
                            .debug_selector(|| "conversation-controller-error".to_string())
                            .w_full()
                            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                            .mx_auto()
                            .mt_1()
                            .mb_2()
                            .child(render_rows::plain_error_notice(error, colors.danger))
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
            .child(
                div()
                    .debug_selector(|| "composer-shell".into())
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
                    .on_action(cx.listener(Self::next_composer_control))
                    .on_action(cx.listener(Self::previous_composer_control))
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
                            // 选择器打开时的按键作用域（A2-12）：Failed
                            // 使用独立 Retry 上下文，避免隐藏后仍吞掉
                            // Composer 的按键；其余可见状态由 FileSelect
                            // 先处理 Up/Down/Enter/Tab/Esc。
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
                                crate::icons::icon_button(
                                    crate::icons::Icon::Plus,
                                    "添加上下文或切换模式",
                                    colors,
                                    cx.listener(|this, _, window, cx| {
                                        this.open_composer_actions(window, cx)
                                    }),
                                )
                                .size(px(Layout::TITLEBAR_CONTROL_SIZE))
                                .track_focus(&self.action_focus[0])
                                .debug_selector(|| "composer-add".into()),
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
                                self.actions.running
                                    && self.thread.backend == TaskBackend::Native
                                    && (!self.input.read(cx).text().is_empty()
                                        || !self.attachments.is_empty()),
                                |row| row.child(self.render_running_send(can_send, cx)),
                            )
                            .when(
                                self.actions.running || self.composer_submit_pending,
                                |row| row.child(self.render_composer_stop(cx)),
                            )
                            .when(
                                !self.actions.running && !self.composer_submit_pending,
                                |row| {
                                    row.child(
                                        div()
                                            .id("composer-send")
                                            .debug_selector(|| "composer-send".into())
                                            .aria_label("发送")
                                            .track_focus(&self.action_focus[1])
                                            .tab_stop(true)
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
                                                crate::icons::Icon::ArrowUp,
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
            .children(self.mcp_warning.clone().map(|warning| {
                div()
                    .debug_selector(|| "conversation-mcp-warning".to_string())
                    .w_full()
                    .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                    .mx_auto()
                    .mt_1()
                    .child(render_rows::status_notice(warning, colors.warning, &colors))
            }))
            .into_any_element()
    }

    /// R57 P2b (spec §2.3 R2/R5) / R62 R7 / R63: the permission mode as a
    /// clickable status chip.
    ///
    /// Glyph, ink and label all come from the same projections the picker's
    /// rows read (`permission_icon`, `permission_is_warning`,
    /// `permission_label`), so the two surfaces can never contradict each
    /// other: 只读 shows the hand, 确认 the shield, and only 自动 — the
    /// unrestricted mode — carries the warning triangle and the warning ink.
    /// R63 fixes the one place that had drifted: this chip hard-coded
    /// `Icon::Warning` plus `colors.warning`, so it showed a warning triangle
    /// in every mode while the picker's rows already showed the right glyph.
    ///
    /// Every non-warning mode reads in `text_secondary`, the ink the `+`
    /// button and the model trigger beside it in this row use, so the chip
    /// stays one of the row's neutral controls instead of carrying a status
    /// colour of its own.
    ///
    /// R62 R7 reverses R57 P2b's "static text" decision: the user's screenshot
    /// proves the reference implementation's `Full access` label is a real
    /// affordance that opens a three-option picker (R62 §7 records the wrong
    /// reasoning chain behind P2b). The chip therefore keeps its exact frozen
    /// geometry and gains the interaction, and the picker layer hangs from a
    /// `.relative()` wrapper exactly like the model trigger's layer does
    /// (`render_model_selector`), so the frozen bottom row's shape is
    /// unchanged: an absolutely positioned child contributes nothing to the
    /// wrapper's measured size.
    fn render_permission_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mode = self.thread.permission_mode;
        let label = permission_label(mode);
        // R63: the glyph is chosen once, and both the painted icon and the
        // harness-visible tag below are that same value — so the tag can never
        // name one icon while the chip paints another.
        let glyph = permission_icon(mode);
        // R63: one rule decides both inks, and it is the picker's own rule.
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
                    .debug_selector(|| "composer-permission-status".into())
                    // R62 R7: the same guard the model trigger carries. The
                    // conversation root's own mouse-down closes any open
                    // picker, so without this the chip's mouse-up toggle would
                    // reopen the layer the same click had just closed.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .h(px(Layout::TITLEBAR_CONTROL_SIZE))
                    .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
                    .rounded(px(Layout::DISCLOSURE_RADIUS))
                    .text_size(px(Typography::BODY))
                    .text_color(ink)
                    .cursor_pointer()
                    .interactive(colors, true, self.permission_picker_open)
                    .tooltip(move |_, cx| crate::icons::tooltip("切换权限模式", cx))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(Self::toggle_permission_picker),
                    )
                    // R63: the wrapper exists only so the test harness can read
                    // *which* glyph the chip painted — an `AnyElement` icon
                    // carries no selector of its own. It is a transparent flex
                    // box hugging the 16px icon (the same technique the
                    // slider's chevron uses), so the chip's frozen geometry is
                    // unchanged, and the tag is derived from `glyph` itself
                    // rather than from a second match on the mode.
                    .child(
                        div()
                            .debug_selector(move || permission_icon_selector(glyph).to_string())
                            .flex()
                            .flex_shrink_0()
                            .child(crate::icons::icon(glyph, ink)),
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
            .debug_selector(|| "composer-backend-status".into())
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

    /// R62 R8: the permission picker — the reference implementation's
    /// three-option approvals dropdown.
    ///
    /// Structure (2026-09-14 screenshot):
    ///
    /// ```text
    /// How should ChatGPT actions be approved?          Learn more
    ///   <icon> 只读 / 每次都询问
    ///   <icon> 确认 / 仅对潜在不安全操作询问
    ///   <icon> 自动 / 自动批准，使用沙箱
    ///   <icon> 完全访问 / 不使用沙箱，危险操作仍需确认          ✓
    /// ```
    ///
    /// Every row's icon, label and description come from the shared
    /// `permission_*` projections in `composer_actions.rs`, and the rows are
    /// ordered by the same [`PERMISSION_ORDER`] the `+` menu uses, so the two
    /// entries can never disagree about what a mode is called or which one is
    /// current (R62 R9).
    ///
    /// The layer is anchored to the chip's own box (`bottom: 100%` plus the
    /// frozen [`Layout::COMPOSER_PICKER_TRIGGER_GAP`], `left_0()`), the same
    /// technique R61 settled on for the model card. It opens **upward**,
    /// because the composer sits at the window's bottom edge.
    ///
    /// The title row's `了解更多` is plain secondary text: the reference
    /// implementation opens its docs page, and Vega has no such route, so the
    /// row renders the label without inventing a target (R62 R11's rule for
    /// actions with no implementation path — the *picker's own* actions are
    /// all real; only this link has no Vega counterpart).
    fn render_permission_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let current = self.thread.permission_mode;
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
                    .debug_selector(move || format!("composer-permission-option-{}", mode.as_str()))
                    .min_h(px(44.))
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    // R50 contract A: on the picker's white `bg_elevated`
                    // surface the selection uses the derived 5%-ink fill, not
                    // the sidebar's pre-composited `bg_active` constant.
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
                                marker
                                    .debug_selector(move || {
                                        format!(
                                            "composer-permission-option-{}-check",
                                            mode.as_str()
                                        )
                                    })
                                    .child(crate::icons::icon(
                                        crate::icons::Icon::Check,
                                        colors.brand_primary,
                                    ))
                            }),
                    )
            });
        // R64 R1/R3: deferred paint, priority 2 — see `render_file_dropdown`.
        gpui_kit::deferred(
            div()
                .debug_selector(|| "composer-permission-picker".into())
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
                        .debug_selector(|| "composer-permission-picker-title".into())
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
                                .child(PERMISSION_PICKER_TITLE),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(Typography::METADATA))
                                .text_color(colors.text_tertiary)
                                .child(PERMISSION_PICKER_LEARN_MORE),
                        ),
                )
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

    /// R62 R7: opens/closes the permission picker. Closing every other
    /// composer popover first is what keeps the picker, the `+` menu, the
    /// file dropdown and the model picker mutually exclusive.
    pub(crate) fn toggle_permission_picker(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = !self.permission_picker_open;
        self.close_composer_popovers(cx);
        self.permission_picker_open = open;
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn on_permission_picker_key_down(
        &mut self,
        event: &gpui_kit::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key != "escape" || self.input.read(cx).is_composing() {
            return;
        }
        self.permission_picker_open = false;
        self.focus_composer(window, cx);
        cx.stop_propagation();
        cx.notify();
    }

    /// R62 R8/A10: applies a picker choice. The picker closes first (a mode
    /// change is a completed round trip, exactly like the model picker's
    /// R59 R3 rule), then the request goes out on the one existing
    /// `ThreadSettingsRequested` path — the same one the `+` menu uses, so
    /// persistence semantics cannot differ between the two entries (R62 R9).
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

    /// The `@file` suggestion dropdown (A2-12): rendered above the input row
    /// while an `@token` is being completed. Bounded candidate list, mouse
    /// and keyboard parity; zero filesystem access from this view.
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
                .child("正在索引文件…")
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
                        .aria_label("重试文件索引")
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
                        .child("重试"),
                )
                .into_any_element()
        } else if !self.file_selector.is_open() {
            div()
                .px_2()
                .py_2()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_secondary)
                .child("没有匹配文件")
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
        let (estimated_tokens, context_limit) = self.context_usage_source();
        let display = ContextUsageDisplay::new(estimated_tokens, context_limit);
        let colors = theme(cx).colors;
        let focused = self.context_usage_focus.contains_focused(window, cx);
        let hovered = self.context_usage_trigger_hovered || self.context_usage_tooltip_hovered;
        let shown =
            self.context_usage_open || (!self.context_usage_dismissed && (focused || hovered));
        div()
            .relative()
            .min_w_0()
            .flex_shrink_1()
            .flex()
            .items_center()
            .gap_1()
            .child(self.render_context_usage_indicator(&display, colors, cx))
            .child(self.render_model_selector(cx))
            .children(shown.then(|| {
                let tooltip_hover = cx.listener(|this, hovered: &bool, _, cx| {
                    this.context_usage_tooltip_hovered = *hovered;
                    cx.notify();
                });
                div().absolute().top_0().right_0().size(px(0.0)).child(
                    div().relative().child(
                        anchored()
                            .anchor(Anchor::BottomRight)
                            .position_mode(AnchoredPositionMode::Local)
                            .position(point(px(0.0), px(0.0)))
                            .snap_to_window()
                            .child(
                                gpui_kit::deferred(
                                    div()
                                        .id("context-usage-tooltip-popover")
                                        .debug_selector(|| "context-usage-tooltip-popover".into())
                                        .on_hover(tooltip_hover)
                                        .on_mouse_down_out(cx.listener(
                                            |this, _: &MouseDownEvent, window, cx| {
                                                if this.context_usage_open {
                                                    cx.stop_propagation();
                                                }
                                                this.dismiss_context_usage(window, cx);
                                            },
                                        ))
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .child(
                                            self.context_usage_tooltip_element(
                                                &display, colors, cx,
                                            ),
                                        ),
                                )
                                .with_priority(2),
                            ),
                    ),
                )
            }))
            .into_any_element()
    }

    fn render_context_usage_indicator(
        &self,
        display: &ContextUsageDisplay,
        colors: ThemeColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let accessible_label = display.accessibility_label();
        let control = div()
            .id("composer-context-usage-hover-target")
            .debug_selector(|| "composer-context-usage-hover-target".into())
            .flex_shrink_0()
            .min_h(px(Layout::TITLEBAR_CONTROL_SIZE))
            .flex()
            .items_center()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.context_usage_trigger_hovered = *hovered;
                if *hovered {
                    this.context_usage_dismissed = false;
                }
                cx.notify();
            }))
            .child(
                div()
                    .id("composer-context-usage")
                    .debug_selector(|| "composer-context-usage".into())
                    .aria_label(accessible_label)
                    .focusable()
                    .track_focus(&self.context_usage_focus)
                    .tab_stop(true)
                    .capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            if this.context_usage_open {
                                this.dismiss_context_usage(window, cx);
                            } else {
                                this.close_composer_popovers(cx);
                                this.context_usage_open = true;
                                this.context_usage_dismissed = false;
                                this.context_usage_focus.focus(window, cx);
                                cx.notify();
                            }
                        }),
                    )
                    .on_key_down(
                        cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                            match event.keystroke.key.as_str() {
                                "escape" => this.dismiss_context_usage(window, cx),
                                "enter" | "space" => {
                                    if this.context_usage_open {
                                        this.dismiss_context_usage(window, cx);
                                    } else {
                                        this.close_composer_popovers(cx);
                                        this.context_usage_open = true;
                                        this.context_usage_dismissed = false;
                                        cx.notify();
                                    }
                                }
                                _ => return,
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .size(px(Layout::CONTEXT_USAGE_SIZE))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(move |style| style.bg(colors.bg_hover))
                    .focus_visible(move |style| style.bg(colors.bg_hover))
                    .child(
                        div()
                            .debug_selector(|| "composer-context-usage-ring".into())
                            .size(px(16.0))
                            .flex_shrink_0()
                            .child(Self::context_usage_ring(display, colors)),
                    ),
            );
        div().child(control).into_any_element()
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

    fn dismiss_context_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.context_usage_open = false;
        self.context_usage_dismissed = true;
        self.context_usage_trigger_hovered = false;
        self.context_usage_tooltip_hovered = false;
        self.context_usage_focus.focus(window, cx);
        cx.notify();
    }

    fn context_usage_tooltip_element(
        &self,
        display: &ContextUsageDisplay,
        colors: ThemeColors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let over_limit = display
            .estimated_tokens
            .zip(display.model_input_limit)
            .is_some_and(|(used, limit)| used > limit);
        let status_color = if over_limit {
            colors.warning
        } else {
            colors.text_primary
        };
        let help = format!("{}；{}", display.estimate_label(), display.capacity_label());
        div()
            .id("context-usage-tooltip")
            .debug_selector(|| "context-usage-tooltip".into())
            .w(px(Layout::CONTEXT_USAGE_WIDTH))
            .p(px(Layout::CONTEXT_USAGE_PADDING))
            .flex()
            .flex_col()
            .gap_3()
            .rounded(px(Layout::TOOL_DETAIL_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .shadow_sm()
            .text_size(px(Typography::METADATA))
            .text_color(colors.text_secondary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .debug_selector(|| "context-usage-tooltip-title".into())
                            .flex_1()
                            .text_size(px(Typography::BODY))
                            .text_color(colors.text_primary)
                            .font_weight(FontWeight::MEDIUM)
                            .child("上下文用量"),
                    )
                    .child(
                        div()
                            .id("context-usage-help")
                            .debug_selector(|| "context-usage-help".into())
                            .size(px(Layout::ICON_SIZE))
                            .tooltip(move |_, cx| crate::icons::tooltip(help.clone(), cx))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Help,
                                colors.text_secondary,
                            )),
                    )
                    .child(crate::icons::icon_button(
                        crate::icons::Icon::Close,
                        "关闭上下文用量",
                        colors,
                        cx.listener(|this, _, window, cx| this.dismiss_context_usage(window, cx)),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .children(display.percentage_value().map(|percentage| {
                        div()
                            .debug_selector(|| "context-usage-tooltip-percentage".into())
                            .text_size(px(Typography::EMPTY_STATE_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(status_color)
                            .child(percentage)
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .debug_selector(|| "context-usage-tooltip-estimate".into())
                                    .child("估算已用 / 配置输入上限"),
                            )
                            .child(
                                div()
                                    .debug_selector(|| "context-usage-tooltip-compact".into())
                                    .text_color(colors.text_primary)
                                    .child(display.compact_usage_label()),
                            ),
                    ),
            )
            .children(display.percentage_value().map(|_| {
                div()
                    .debug_selector(|| "context-usage-bar".into())
                    .h(px(6.))
                    .w_full()
                    .rounded_full()
                    .overflow_hidden()
                    .bg(colors.border_subtle)
                    .child(
                        div()
                            .h_full()
                            .w(gpui_kit::relative(display.fill_fraction))
                            .rounded_full()
                            .bg(if over_limit {
                                colors.warning
                            } else {
                                colors.accent
                            }),
                    )
            }))
            .children(
                (display.model_input_limit.is_none() || over_limit).then(|| {
                    div()
                        .debug_selector(|| "context-usage-tooltip-capacity".into())
                        .text_color(if over_limit {
                            colors.warning
                        } else {
                            colors.text_secondary
                        })
                        .child(if over_limit {
                            "已超过配置输入上限"
                        } else {
                            "配置输入上限未设置"
                        })
                }),
            )
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.dismiss_context_usage(window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
    }

    /// The model selector (A2-14): trigger shows the current selection;
    /// options are the priced catalog projection installed by the app layer
    /// (zero file IO). Keyboard: Enter/Space open, Up/Down move, Enter
    /// accept (first-wins), Esc close.
    ///
    /// R59 R7: while the tier slider (level one) is showing, the trigger reads
    /// `选择强度` — the Chinese counterpart of the reference implementation's
    /// `Select effort`, which is what its `selectEffort.label` says the trigger
    /// shows "while adjusting the reasoning effort of an explicitly selected
    /// model". Vega's existing wording for this concept is 档位/强度 (see
    /// `settings/reasoning_render.rs` and R57 §2.2), so 选择强度 is the
    /// in-convention translation. Outside that state the trigger keeps the
    /// existing model-name display.
    ///
    /// R61 R1/R2: this `.relative()` wrapper is the picker's containing block.
    /// Both levels are its absolutely positioned children and hang off its
    /// **top** edge (`bottom: 100%` plus the gap), right-aligned to its
    /// **right** edge (`right: 0`), so the card hugs the model button instead
    /// of the composer column's top edge. That means the card overlaps the R49
    /// utility bar by design (R61 §2: the user chose the overlap), and the
    /// bar's chips may be covered while a layer is open — the layers
    /// `stop_propagation` on mouse-down, so an outside click still closes the
    /// picker and restores the bar (R61 R6).
    fn render_model_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let enabled = !self.trusted_action_busy
            && !self.model_selection_save_busy()
            && self.model_selection_pending.is_none();
        // R1: while the durable save is in flight the chip shows a bounded
        // pending state instead of claiming the new selection already.
        // R59 R7: which model name (if any) the trigger shows is decided by
        // `model_trigger_label`, so the rule is directly testable.
        let current = model_trigger_label(
            &self.composer_defaults.model,
            self.model_picker_level,
            self.model_selection_pending.is_some(),
        );
        div()
            .relative()
            .flex()
            .min_w_0()
            .flex_shrink_1()
            .child(
                div()
                    .id("composer-model")
                    .debug_selector(|| "composer-model".into())
                    .aria_label(current.to_owned())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .track_focus(&self.model_focus)
                    .key_context("ModelSelector")
                    .on_action(cx.listener(Self::on_activate_model))
                    .on_action(cx.listener(Self::on_model_previous))
                    .on_action(cx.listener(Self::on_model_next))
                    .on_action(cx.listener(Self::on_model_close))
                    .min_w_0()
                    .flex_shrink_1()
                    .truncate()
                    .px_2()
                    .h(px(Layout::TITLEBAR_CONTROL_SIZE))
                    .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
                    .rounded(px(Layout::DISCLOSURE_RADIUS))
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
                            .h_full()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .debug_selector(|| "composer-model-label".into())
                                    .min_w_0()
                                    .flex_shrink_1()
                                    .truncate()
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

    /// R59 R4 / R61 R1: the picker's two floating layers.
    ///
    /// The single `model_picker_level` value decides which arm below runs, so
    /// at most one layer is in the element tree per frame and the slider can
    /// never be nested inside the list's container (R59 A3). Each layer carries
    /// its own border, background and shadow, which is what removes R57's
    /// card-inside-a-card look (D1).
    ///
    /// R61 R2 removed the R59 `render_model_picker_overlay` wrapper that pinned
    /// this call to the composer **column's** top edge with a
    /// `max_w(COMPOSER_MAX_WIDTH) + mx_auto` inner box. That wrapper existed to
    /// keep the R49 utility bar clear; the user ruled the trade-off out
    /// (R61 §2), so the layers now hang from the model trigger's own
    /// `.relative()` wrapper (see [`Self::render_model_selector`]) and each
    /// layer right-aligns with `right_0()`.
    ///
    /// R64 R1: both layers are wrapped in `gpui_kit::deferred(...)` at
    /// priority 2. GPUI paints an element's border **after** its children
    /// (`Style::paint`), so the composer card's own 1px border crossed every
    /// layer mounted inside it. `deferred` delays only the painting until after
    /// the ancestors, keeping layout in the current tree, which is why the
    /// trigger anchoring above is untouched.
    ///
    /// R59 recorded that a deferred card landed at x 1232.5 on a 1200-wide
    /// window. R64 §3 established that this came from wrapping the R59
    /// `max_w(COMPOSER_MAX_WIDTH) + mx_auto` **outer box** — the structure R61
    /// R2 deleted — not from `deferred` itself, which records and replays the
    /// element's own `absolute_offset` (`window.rs`'s `defer_draw`). The layers
    /// below are measured to keep the exact bounds they had before the wrap.
    ///
    /// `occlude()` is kept on each layer: it supplies the mouse hit-testing
    /// priority (`HitboxBehavior::BlockMouse`) and was never a paint-order
    /// mechanism.
    fn render_model_picker_layers(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.model_picker_level {
            ModelPickerLevel::Closed => div().into_any_element(),
            ModelPickerLevel::Slider => self.render_picker_slider_layer(cx),
            ModelPickerLevel::List => self.render_picker_list_layer(cx),
        }
    }

    /// R59 R5: the height bound shared by both layers.
    ///
    /// R57 shipped no bound at all, so the popup grew to 1409px with a 40-model
    /// catalog and covered the whole transcript. The bound is what makes the
    /// list scroll inside itself instead of growing the layer. R61 R3 keeps it
    /// unchanged.
    fn picker_max_height(&self) -> Pixels {
        px(Layout::COMPOSER_PICKER_MAX_HEIGHT)
    }

    /// R59 R1 / R61 R1: the tier slider card, and nothing else.
    ///
    /// R57 mounted this card as the model menu's last child (D1); here it is a
    /// standalone floating layer with its own chrome (R4). A model that
    /// declares no tiers opens the list directly (I68), so the slider layer
    /// is not mounted rather than leaving an empty padded card behind (R57 R12).
    ///
    /// R61 R1: `bottom: 100%` + [`Layout::COMPOSER_PICKER_TRIGGER_GAP`] places
    /// the card's bottom edge that far above the **trigger's** top edge, and
    /// `right_0()` aligns its right edge with the trigger's — no arithmetic
    /// inset, because the containing block *is* the trigger's wrapper. R61
    /// accepts the resulting overlap with the R49 utility bar (§2).
    fn render_picker_slider_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.thinking_slider.read(cx).has_tiers() {
            return div().into_any_element();
        }
        // R64 R1/R3: deferred paint, priority 2 — see `render_file_dropdown`.
        gpui_kit::deferred(
            div()
                .debug_selector(|| "composer-thinking-slider".into())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .absolute()
                .bottom(gpui_kit::relative(1.0))
                .mb(px(Layout::COMPOSER_PICKER_TRIGGER_GAP))
                .right_0()
                // R59 R4: the card's own surface. `ThinkingSlider` already draws the
                // measured 254.5px card with its own border, background and shadow,
                // so this layer adds only placement and the height bound.
                .max_h(self.picker_max_height())
                .occlude()
                .flex()
                .flex_col()
                .child(self.thinking_slider.clone()),
        )
        .with_priority(2)
        .into_any_element()
    }

    /// R59 R2/R3 / I68: the model list, reached from the slider title when
    /// tiers exist or directly from the trigger when they do not.
    ///
    /// The list is bounded by [`Self::picker_max_height`] and scrolls inside
    /// that bound, so a long catalog cannot grow the layer without limit (R61
    /// A4). The heading is the reference implementation's
    /// `composer.modelPicker.modelList.heading` ("Select model"); Vega's
    /// existing Chinese wording for this surface is 选择模型.
    ///
    /// R61 R1: same trigger anchoring as the slider layer — `bottom: 100%`
    /// minus the gap, `right_0()`. The list keeps its own frozen
    /// [`Layout::MENU_MAX_WIDTH`], so it extends to the **left** of the trigger
    /// rather than to its right.
    fn render_picker_list_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        // R64 R1/R3: deferred paint, priority 2 — see `render_file_dropdown`.
        gpui_kit::deferred(
            div()
                .id("composer-model-menu")
                .debug_selector(|| "composer-model-menu".into())
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
                        .debug_selector(|| "composer-model-heading".into())
                        .flex_shrink_0()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .child(PICKER_LIST_HEADING),
                )
                // Only the rows scroll: the heading stays visible so the layer never
                // becomes an unbounded column of model names.
                .child(
                    div()
                        .id("composer-model-rows")
                        .debug_selector(|| "composer-model-rows".into())
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.model_menu_scroll)
                        .children(self.model_options.iter().enumerate().map(|(index, model)| {
                            let selected = index == self.model_selector_highlight;
                            let current_model = *model == self.composer_defaults.model;
                            let model = model.clone();
                            let label = model.clone();
                            div()
                                .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                                .flex_shrink_0()
                                .px_2()
                                .flex()
                                .items_center()
                                .text_size(px(Typography::SIDEBAR))
                                .truncate()
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
                                    cx.listener(
                                        move |this, _: &MouseUpEvent, _: &mut Window, cx| {
                                            this.select_model_option(&model, cx);
                                        },
                                    ),
                                )
                                .child(label)
                        })),
                ),
        )
        .with_priority(2)
        .into_any_element()
    }

    fn message_location_status_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let status = self.message_location_status?;
        let colors = theme(cx).colors;
        let (label, color) = match status {
            MessageLocationStatus::Searching => ("正在定位消息…", colors.text_secondary),
            MessageLocationStatus::Deferred => ("任务完成后定位消息…", colors.text_secondary),
            MessageLocationStatus::NotFound => {
                ("当前会话中没有这条消息，可选择其他消息。", colors.warning)
            }
            MessageLocationStatus::Failed => {
                ("定位消息失败，可重试或选择其他消息。", colors.danger)
            }
            MessageLocationStatus::Located => return None,
        };
        Some(
            div()
                .id("message-anchor-status")
                .debug_selector(|| "message-anchor-status".into())
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
        self.reconcile_pending_focus(window, cx);
        if !self.context_usage_focus.contains_focused(window, cx) {
            self.context_usage_dismissed = false;
        }
        if self.context_usage_focus_subscriptions.is_none() {
            let focus = self.context_usage_focus.clone();
            let focus_in = cx.on_focus_in(&focus, window, |_, _, cx| cx.notify());
            let focus_out = cx.on_focus_out(&focus, window, |this, _, _, cx| {
                this.context_usage_dismissed = false;
                cx.notify();
            });
            self.context_usage_focus_subscriptions = Some((focus_in, focus_out));
        }
        if !self.selection_view_initialized {
            gpui_kit::base::TextSelection::clear(window, cx);
            self.selection_view_initialized = true;
        }
        if self.actions.restore_focus {
            self.actions.restore_focus = false;
            self.focus_composer(window, cx);
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
            && let Some(StreamEntry::Assistant { stream, model, .. }) = self.entries.get_mut(*index)
        {
            let snapshot = stream.snapshot();
            if model.sync(&snapshot, &self.counters) {
                self.invalidate_item(Some(*index));
            }
        }
        self.settle_final_presentation(window, cx);
        self.refresh_run_notices(cx);
        let stage_top_insets = self.stage_top_insets(cx);

        // 2) 顶部水合请求（S8-T45/C7）：视口到达顶部且仍存在更早历史时，
        //    向 app 层请求上一页（typed 投影返回后 splice 前插，页边界保
        //    anchor）。本 crate 零 SQLite；一页在飞，失败暂停直到离开顶部。
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
        let body: AnyElement = if self.entries.is_empty() {
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
                        .child("今天想做些什么？"),
                )
                .child(
                    div()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child("输入任务开始，或用 @ 引用文件"),
                )
                .children(self.message_location_status_element(cx))
                .into_any_element()
        } else {
            let (message_anchor_rail, preview, preview_in_lane) =
                self.render_message_anchor_rail(window, cx);
            message_anchor_rail_overlay = Some(message_anchor_rail);
            message_anchor_preview = preview;
            message_anchor_preview_in_lane = preview_in_lane;
            div()
                .id("conversation-scroll")
                .debug_selector(|| "conversation-scroll".into())
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
                                .debug_selector(|| "conversation-message-list".into())
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
                                                let item = if this.process_entry_hidden(index, cx) || this.compaction_failure_is_represented(index) {
                                                    div().h_0().into_any_element()
                                                } else {
                                                    match entry {
                                                        Some(StreamEntry::RunActivity {
                                                            group,
                                                        }) => this.render_run_details(
                                                            group.clone(),
                                                            window,
                                                            cx,
                                                        ),
                                                        Some(StreamEntry::RunActivitySegment { group, segment }) => RunActivityGroup::render_segment_with_top(group.clone(), *segment, stage_top_insets.get(&index).copied().unwrap_or(Layout::CONVERSATION_STAGE_TOP), window, cx),
                                                        Some(entry) => render_stream_entry(
                                                            entry,
                                                            index,
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
                                                            &this.selection_focus,
                                                        ),
                                                        None => div().into_any_element(),
                                                    }
                                                };
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
                                                        .child(notice.render(&colors, this.thread.id.clone(), cx))
                                                        .into_any_element()
                                                } else {
                                                    item
                                                };
                                                this.counters.record_row_callback(
                                                    row_t0.elapsed().as_nanos(),
                                                );
                                                item
                                            },
                                        ),
                                    )
                                    .h_full()
                                    .w_full()
                                    .pb(self.foreground_height),
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    // R59/R62: a click anywhere outside a picker closes
                    // whichever level is mounted. The layers themselves stop
                    // propagation, so this only sees genuine outside clicks.
                    if this.model_picker_level.is_open() || this.permission_picker_open {
                        this.model_picker_level = ModelPickerLevel::Closed;
                        this.permission_picker_open = false;
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(Self::open_diff_action))
            .on_action(cx.listener(Self::copy_selected_text))
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
                            .debug_selector(|| "conversation-column".into())
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
            .when(!self.entries.is_empty(), |root| {
                root.child(self.render_conversation_foreground(window, cx))
            })
            .when(self.entries.is_empty(), |root| {
                root.when(self.privacy.opened, |view| {
                    view.child(self.render_privacy_control(cx))
                })
                .child(self.render_composer(window, cx))
                .when(project_bound, |root| root.child(self.commit_panel.clone()))
            })
            .children(self.render_pending_menu(cx))
            .into_any_element();
        counters.record_render(render_t0);
        element
    }
}
