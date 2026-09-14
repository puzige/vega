use super::composer_actions::permission_label;
use super::*;

/// R59 R4/R5: horizontal inset of a model-picker layer from the right edge of
/// the composer card, so the layer's right edge lands on the model trigger's
/// own right edge.
///
/// Derived from the frozen bottom row rather than measured: the trigger is the
/// last control before the send button, so the inset is the card's border (1)
/// plus its `p_3` (12) plus the send button ([`Layout::COMPOSER_SEND_SIZE`])
/// plus the row's `gap_2` (8). A structural test asserts the layer's right edge
/// equals the trigger's, so a change to any of those terms cannot silently
/// drift the popup.
const PICKER_RIGHT_INSET: f32 = 1.0 + 12.0 + Layout::COMPOSER_SEND_SIZE + 8.0;

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

    fn on_resume_tail(&mut self, _: &ResumeTail, _: &mut Window, cx: &mut Context<Self>) {
        self.resume_tail(cx);
    }

    fn on_resume_tail_clicked(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.resume_tail(cx);
    }

    /// Keeps the existing detached-tail recovery on the conversation surface
    /// after R19 moves route identity and project actions into the window
    /// header. This control is backed by the original scroll handler.
    fn render_resume_tail(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .absolute()
            .top_2()
            .right_3()
            .track_focus(&self.resume_tail_focus)
            .key_context("ResumeTailButton")
            .on_action(cx.listener(Self::on_resume_tail))
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .shadow_sm()
            .text_size(px(Typography::METADATA))
            .text_color(colors.text_secondary)
            .cursor_pointer()
            .hover(move |style| style.bg(colors.bg_hover))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_resume_tail_clicked))
            .child("回到底部")
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
    fn render_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let file_retry_visible = self.file_selector_wanted && self.file_index_failure.is_some();
        let file_selector_active =
            self.file_selector.is_open() || self.file_selector_wanted || self.file_index_loading;
        let can_send = !self.actions.running
            && self.actions.pending_mode.is_none()
            && !self.input.read(cx).text().is_empty()
            && !self.composer_submit_pending
            && !self.approved_not_started
            && !self.trusted_action_busy
            && self.model_selection_pending.is_none();
        div()
            .px(px(Layout::CONTENT_PADDING))
            .pt(px(Layout::COMPOSER_PADDING_TOP))
            .pb(px(Layout::COMPOSER_PADDING_BOTTOM))
            .flex_shrink_0()
            // R59 R4/R5: the picker's two layers are absolutely positioned
            // against this column, so it must be their containing block. They
            // anchor to the column's **top** edge (`bottom: 100%`), which is
            // the utility bar's top edge when the bar is mounted and the
            // composer card's top edge when it is not — so a layer can never
            // reach the bar or the card, on any window size.
            .relative()
            // R49: the utility bar is a real sibling above the card on the
            // new-task page (zero overlap, no negative margin). The session
            // page renders the card alone, exactly like Codex.
            .when(self.utility_bar_visible(cx), |column| {
                column.child(self.render_composer_utility_bar(cx))
            })
            .child(
                div()
                    .debug_selector(|| "composer-shell".into())
                    .w_full()
                    .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                    .min_h(px(Layout::COMPOSER_MIN_HEIGHT))
                    .mx_auto()
                    // Cmd+Enter 的按键上下文（绑定见 vega_ui::init）。
                    .key_context("Composer")
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
                    .shadow_sm()
                    .p_3()
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
                            .when(!self.compact_workspace, |row| row.flex_wrap())
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
                                .track_focus(&self.action_focus[0])
                                .debug_selector(|| "composer-add".into()),
                            )
                            .child(self.render_permission_status(cx))
                            .child(div().flex_1())
                            .child(self.render_model_selector(cx))
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
                                                    .hover(move |style| {
                                                        style.bg(colors.brand_primary_strong)
                                                    })
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
            .child(self.render_composer_run_status(cx))
            .when(self.composer_submit_pending, |composer| {
                composer.child(
                    div()
                        .id("composer-preparing-request")
                        .w_full()
                        .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                        .mx_auto()
                        .mt_2()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child("正在准备请求…"),
                )
            })
            .children(self.controller_error.clone().map(|error| {
                div()
                    .debug_selector(|| "conversation-controller-error".to_string())
                    .w_full()
                    .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                    .mx_auto()
                    .mt_1()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.danger)
                    .child(error)
            }))
            // R59 R4: the picker overlay is the column's last child, so the
            // layers paint above the bar and the card rather than inside either
            // one.
            .child(self.render_model_picker_overlay(cx))
            .into_any_element()
    }

    /// R57 P2b (spec §2.3 R2/R5): the permission mode as static status text.
    ///
    /// Warning-coloured label plus the warning glyph, never clickable — the
    /// reference implementation's `⚠ Full access` shape with Vega's existing
    /// Chinese labels. The label comes from the one projection the `+` menu
    /// also renders, so the two surfaces cannot drift. The change entry point
    /// is the `+` menu (P1), which owns the keyboard path this control no
    /// longer needs.
    fn render_permission_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let label = permission_label(self.thread.permission_mode);
        div()
            .id("composer-permission-status")
            .debug_selector(|| "composer-permission-status".into())
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .py_1()
            .text_size(px(Typography::METADATA))
            .text_color(colors.warning)
            .tooltip(move |_, cx| crate::icons::tooltip(label, cx))
            .child(crate::icons::icon(
                crate::icons::Icon::Warning,
                colors.warning,
            ))
            .child(label)
            .into_any_element()
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
                .items_center()
                .justify_between()
                .gap_2()
                .px_2()
                .py_2()
                .text_size(px(Typography::SIDEBAR))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(colors.text_secondary)
                        .child(code.message()),
                )
                .child(
                    div()
                        .id("file-index-retry")
                        .flex_shrink_0()
                        .px_2()
                        .py_1()
                        .rounded_md()
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
            .child(
                div()
                    .id("composer-model")
                    .debug_selector(|| "composer-model".into())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .track_focus(&self.model_focus)
                    .key_context("ModelSelector")
                    .on_action(cx.listener(Self::on_activate_model))
                    .on_action(cx.listener(Self::on_model_previous))
                    .on_action(cx.listener(Self::on_model_next))
                    .on_action(cx.listener(Self::on_model_close))
                    .min_w_0()
                    .max_w(px(if self.compact_workspace { 98. } else { 150. }))
                    .flex_shrink_0()
                    // Keep the trigger on one line with an ellipsis. The
                    // max-width alone leaves the text node's intrinsic width
                    // in the flex measure and can crowd the send action.
                    .truncate()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if enabled {
                        colors.text_secondary
                    } else {
                        colors.text_tertiary
                    })
                    .when(enabled, |trigger| {
                        trigger
                            .cursor_pointer()
                            .hover(move |style| style.bg(colors.bg_hover))
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, window, cx| {
                            this.on_activate_model(&ActivateModel, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(current.to_owned())
                            .child(crate::icons::icon(
                                crate::icons::Icon::ChevronDown,
                                colors.text_tertiary,
                            )),
                    ),
            )
            .into_any_element()
    }

    /// R59 R4: the picker's two floating layers.
    ///
    /// The single `model_picker_level` value decides which arm below runs, so
    /// at most one layer is in the element tree per frame and the slider can
    /// never be nested inside the list's container (R59 A3). Each layer carries
    /// its own border, background and shadow, which is what removes R57's
    /// card-inside-a-card look (D1).
    fn render_model_picker_layers(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.model_picker_level {
            ModelPickerLevel::Closed => div().into_any_element(),
            ModelPickerLevel::Slider => self.render_picker_slider_layer(cx),
            ModelPickerLevel::List => self.render_picker_list_layer(cx),
        }
    }

    /// R59 R4/R5: the zero-height overlay both picker layers hang from.
    ///
    /// It is pinned to the **top edge of the composer column** — the utility
    /// bar's top edge when the bar is mounted, the card's top edge when it is
    /// not — and its inner box reproduces the composer card's own horizontal
    /// geometry (`max_w` + `mx_auto`), so a layer's `right(px(..))` measures
    /// from the card's right edge rather than from the window's.
    ///
    /// Anchoring here is what makes R5 structural rather than arithmetic: the
    /// layers open upward from the column's top edge, so no window size, model
    /// count, or grown input row can bring them down over the bar. Anchoring to
    /// the model trigger was rejected by measurement — the gap between the
    /// trigger's top (1022) and the bar's bottom (961) is 61px, while the
    /// R57-frozen slider card needs 84px, so a trigger-anchored popup cannot
    /// show the card without covering the bar (the D2 defect).
    ///
    /// The layers are deliberately **not** wrapped in `gpui_kit::deferred`:
    /// deferred content is laid out in its own window-space paint layer, which
    /// breaks these composer-relative coordinates (measured: the card landed at
    /// x 1232.5 on a 1200-wide window instead of on the card's right edge). The
    /// composer column clips nothing, so no escape hatch is needed; each
    /// layer's own `occlude()` supplies the hit-testing priority instead.
    fn render_model_picker_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.model_picker_level.is_open() {
            return div().into_any_element();
        }
        div()
            .debug_selector(|| "composer-picker-anchor".into())
            .absolute()
            .bottom(gpui_kit::relative(1.0))
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .items_end()
            .child(
                div()
                    .debug_selector(|| "composer-picker-anchor-box".into())
                    .w_full()
                    .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(self.render_model_picker_layers(cx)),
            )
            .into_any_element()
    }

    /// R59 R5: the height bound shared by both layers.
    ///
    /// R57 shipped no bound at all, so the popup grew to 1409px with a 40-model
    /// catalog and covered the whole transcript. The bound is what makes the
    /// list scroll inside itself instead of growing the layer.
    fn picker_max_height(&self) -> Pixels {
        px(Layout::COMPOSER_PICKER_MAX_HEIGHT)
    }

    /// R59 R1: **level one** — the tier slider card, and nothing else.
    ///
    /// R57 mounted this card as the model menu's last child (D1); here it is a
    /// standalone floating layer with its own chrome (R4). A model that
    /// declares no tiers renders nothing at all (R57 R12), so the layer is not
    /// mounted in that case rather than leaving an empty padded card behind.
    fn render_picker_slider_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.thinking_slider.read(cx).has_tiers() {
            return div().into_any_element();
        }
        div()
            .debug_selector(|| "composer-thinking-slider".into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .bottom(gpui_kit::relative(1.0))
            .mb(px(Layout::COMPOSER_PICKER_ANCHOR_GAP))
            .right(px(PICKER_RIGHT_INSET))
            // R59 R4: the card's own surface. `ThinkingSlider` already draws the
            // measured 254.5px card with its own border, background and shadow,
            // so this layer adds only placement and the height bound.
            .max_h(self.picker_max_height())
            .occlude()
            .flex()
            .flex_col()
            .child(self.thinking_slider.clone())
            .into_any_element()
    }

    /// R59 R2/R3: **level two** — the model list, reached only from the
    /// slider's title row.
    ///
    /// The list is bounded by [`Self::picker_max_height`] and scrolls inside
    /// that bound, so a long catalog cannot grow the layer over the utility bar
    /// (R5). The heading is the reference implementation's
    /// `composer.modelPicker.modelList.heading` ("Select model"); Vega's
    /// existing Chinese wording for this surface is 选择模型.
    fn render_picker_list_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id("composer-model-menu")
            .debug_selector(|| "composer-model-menu".into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .bottom(gpui_kit::relative(1.0))
            .mb(px(Layout::COMPOSER_PICKER_ANCHOR_GAP))
            .right(px(PICKER_RIGHT_INSET))
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
                                cx.listener(move |this, _: &MouseUpEvent, _: &mut Window, cx| {
                                    this.select_model_option(&model, cx);
                                }),
                            )
                            .child(label)
                    })),
            )
            .into_any_element()
    }
}

impl Render for ConversationStream {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.actions.restore_focus {
            self.actions.restore_focus = false;
            self.focus_composer(window, cx);
        }
        let render_t0 = Instant::now();
        self.branch_selector
            .update(cx, |selector, _| selector.set_menu_below(true));
        let colors = theme(cx).colors;
        let counters = self.counters.clone();

        // 1) 差量同步：仅 mutable tail（流式中的 assistant 段）参与快照
        //    diff —— 冻结段内容在终结后不再变化，永不重物化（P3/C4 白名
        //    单）；尾项高度可能随内容变化 → 显式失效重测。user 回显等行数
        //    变化在各自的 apply 路径上已登记。
        if let Some((_, index)) = self.active_agent_message.as_ref()
            && let Some(StreamEntry::Assistant { stream, model }) = self.entries.get_mut(*index)
        {
            let snapshot = stream.snapshot();
            model.sync(&snapshot, &self.counters);
            self.invalidate_item(Some(*index));
        }

        // 2) 顶部水合请求（S8-T45/C7）：视口到达顶部且仍存在更早历史时，
        //    向 app 层请求上一页（typed 投影返回后 splice 前插，页边界保
        //    anchor）。本 crate 零 SQLite；一页在飞，失败暂停直到离开顶部。
        let at_top = self.scroll_at_top();
        if self.hydration_pause_is_stale(at_top) {
            self.hydration.paused = false;
        }
        if let Some(before) = self.history_page_request(at_top) {
            self.hydration.loading = true;
            cx.emit(HistoryPageRequested {
                thread_id: self.thread.id.clone(),
                before,
            });
        }

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
                .into_any_element()
        } else {
            div()
                .id("conversation-scroll")
                .size_full()
                .overflow_hidden()
                .child(
                    list(
                        self.list.clone(),
                        cx.processor(
                            move |this: &mut ConversationStream, index: usize, window, cx| {
                                let entry = this.entries.get(index);
                                match entry {
                                    Some(entry) => {
                                        let row_t0 = Instant::now();
                                        let item = render_entry(entry, &this.counters, window, cx);
                                        if let Ok(mut samples) = this.counters.row_build_ns.lock() {
                                            samples.push(row_t0.elapsed().as_nanos());
                                        }
                                        item
                                    }
                                    None => div().into_any_element(),
                                }
                            },
                        ),
                    )
                    .h_full()
                    .w_full(),
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    // R59: a click anywhere outside the picker closes whichever
                    // level is mounted. The layers themselves stop propagation,
                    // so this only sees genuine outside clicks.
                    if this.model_picker_level.is_open() {
                        this.model_picker_level = ModelPickerLevel::Closed;
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(Self::open_diff_action))
            // tech-spec §5.4 动效禁令：流式期间节点无任何入场 opacity/动画
            // （本管线自 T17 起即不引入入场动画，T18 维持）。
            .when(!self.following_tail(), |root| {
                root.child(self.render_resume_tail(cx))
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
                    .px(px(Layout::CONTENT_PADDING))
                    .child(
                        div()
                            .debug_selector(|| "conversation-column".into())
                            .min_w_0()
                            .w_full()
                            .max_w(px(Layout::CONTENT_MAX_WIDTH))
                            .mx_auto()
                            .when(!self.entries.is_empty(), |body| {
                                body.h_full().overflow_hidden()
                            })
                            .child(body),
                    ),
            )
            .child(self.render_composer(cx))
            .when(project_bound, |root| root.child(self.commit_panel.clone()))
            .into_any_element();
        counters.record_render(render_t0);
        element
    }
}
