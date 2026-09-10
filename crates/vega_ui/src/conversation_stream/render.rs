use super::*;

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
    /// with the existing context, mode, permission, model, thinking and
    /// send/stop handlers. Project identity and branch controls live in the
    /// window shell, so this surface never duplicates them or invents state.
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
            .pt(px(12.))
            .pb(px(16.))
            .flex_shrink_0()
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
                    .on_action(cx.listener(Self::on_cycle_thinking))
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
                            .child(self.render_compact_settings(false, cx))
                            .child(self.render_compact_settings(true, cx))
                            .child(div().flex_1())
                            .child(self.render_model_selector(cx))
                            .child(self.render_thinking_control(cx))
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
            .into_any_element()
    }

    fn render_compact_settings(&self, permissions: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let trigger_selector = if permissions {
            "composer-permission"
        } else {
            "composer-mode"
        };
        let menu_selector = if permissions {
            "composer-permission-menu"
        } else {
            "composer-mode-menu"
        };
        let (label, open, index) = if permissions {
            (
                match self.thread.permission_mode {
                    PermissionMode::ReadOnly => "只读",
                    PermissionMode::Confirm => "确认",
                    PermissionMode::Auto => "自动",
                },
                self.permission_menu_open,
                1,
            )
        } else {
            (
                match self.thread.mode {
                    ThreadMode::Ask => "Ask",
                    ThreadMode::Plan => "Plan",
                    ThreadMode::Execute => "Execute",
                },
                self.mode_menu_open,
                0,
            )
        };
        div()
            .relative()
            .child(
                div()
                    .id(("composer-settings", index))
                    .debug_selector(move || trigger_selector.into())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .track_focus(&self.compact_focus[index])
                    .tab_stop(true)
                    .px_1()
                    .py_1()
                    .rounded_md()
                    .text_size(px(Typography::METADATA))
                    .text_color(if permissions {
                        colors.warning
                    } else {
                        colors.brand_primary
                    })
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.compact_focus[index].focus(window, cx);
                            let open = if permissions {
                                !this.permission_menu_open
                            } else {
                                !this.mode_menu_open
                            };
                            this.close_composer_popovers(cx);
                            if permissions {
                                this.permission_menu_open = open;
                            } else {
                                this.mode_menu_open = open;
                            }
                            cx.notify();
                        }),
                    )
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                let open = if permissions {
                                    !this.permission_menu_open
                                } else {
                                    !this.mode_menu_open
                                };
                                this.close_composer_popovers(cx);
                                if permissions {
                                    this.permission_menu_open = open;
                                } else {
                                    this.mode_menu_open = open;
                                }
                                cx.stop_propagation();
                                cx.notify();
                            }
                        }),
                    )
                    .tooltip(move |_, cx| crate::icons::tooltip(label, cx))
                    .when(self.compact_workspace, |trigger| {
                        trigger
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(crate::icons::icon(
                                if permissions {
                                    crate::icons::Icon::Shield
                                } else {
                                    crate::icons::Icon::Mode
                                },
                                if permissions {
                                    colors.warning
                                } else {
                                    colors.brand_primary
                                },
                            ))
                    })
                    .when(!self.compact_workspace, |trigger| {
                        trigger.flex().items_center().gap_1().child(label).child(
                            crate::icons::icon(
                                crate::icons::Icon::ChevronDown,
                                colors.text_tertiary,
                            ),
                        )
                    }),
            )
            .when(open, |root| {
                root.child(
                    div()
                        .debug_selector(move || menu_selector.into())
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .absolute()
                        .bottom(px(32.))
                        .left_0()
                        .w(px(180.).min(px(Layout::MENU_MAX_WIDTH)))
                        .p_1()
                        .rounded(px(Layout::MENU_RADIUS))
                        .border_1()
                        .border_color(colors.border_subtle)
                        .bg(colors.bg_elevated)
                        .shadow_sm()
                        .child(if permissions {
                            self.render_permission_controls(cx)
                        } else {
                            self.render_mode_controls(cx)
                        }),
                )
            })
            .into_any_element()
    }

    fn render_mode_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let enabled = self.model_selection_pending.is_none();
        div()
            .flex()
            .flex_col()
            .child(
                segment(
                    "Ask",
                    self.thread.mode == ThreadMode::Ask,
                    colors,
                    self.setting_focus[0].clone(),
                    enabled,
                )
                .key_context("ThreadSettings")
                .when(enabled, |segment| {
                    segment
                        .on_action(cx.listener(Self::activate_ask))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::select_ask))
                }),
            )
            .child(
                segment(
                    "Plan",
                    self.thread.mode == ThreadMode::Plan,
                    colors,
                    self.setting_focus[1].clone(),
                    enabled,
                )
                .key_context("ThreadSettings")
                .when(enabled, |segment| {
                    segment
                        .on_action(cx.listener(Self::activate_plan))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::select_plan))
                }),
            )
            .child(
                segment(
                    "Execute",
                    self.thread.mode == ThreadMode::Execute,
                    colors,
                    self.setting_focus[2].clone(),
                    enabled,
                )
                .key_context("ThreadSettings")
                .when(enabled, |segment| {
                    segment
                        .on_action(cx.listener(Self::activate_execute))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::select_execute))
                }),
            )
            .into_any_element()
    }

    fn render_permission_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let enabled = self.model_selection_pending.is_none();
        div()
            .flex()
            .flex_col()
            .child(
                segment(
                    "只读",
                    self.thread.permission_mode == PermissionMode::ReadOnly,
                    colors,
                    self.setting_focus[3].clone(),
                    enabled,
                )
                .key_context("ThreadSettings")
                .when(enabled, |segment| {
                    segment
                        .on_action(cx.listener(Self::activate_readonly))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::select_readonly))
                }),
            )
            .child(
                segment(
                    "确认",
                    self.thread.permission_mode == PermissionMode::Confirm,
                    colors,
                    self.setting_focus[4].clone(),
                    enabled,
                )
                .key_context("ThreadSettings")
                .when(enabled, |segment| {
                    segment
                        .on_action(cx.listener(Self::activate_confirm))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::select_confirm))
                }),
            )
            .child(
                segment(
                    "自动",
                    self.thread.permission_mode == PermissionMode::Auto,
                    colors,
                    self.setting_focus[5].clone(),
                    enabled,
                )
                .when(
                    self.thread.permission_mode == PermissionMode::Auto,
                    |item| item.text_color(colors.warning),
                )
                .key_context("ThreadSettings")
                .when(enabled, |segment| {
                    segment
                        .on_action(cx.listener(Self::activate_auto))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::select_auto))
                }),
            )
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
    fn render_model_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let enabled = !self.trusted_action_busy
            && !self.model_selection_save_busy()
            && self.model_selection_pending.is_none();
        // R1: while the durable save is in flight the chip shows a bounded
        // pending state instead of claiming the new selection already.
        let current = if self.model_selection_pending.is_some() {
            "保存中…"
        } else if self.composer_defaults.model.is_empty() {
            "模型"
        } else {
            self.composer_defaults.model.as_str()
        };
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
            .when(self.model_selector_open, |root| {
                root.child(
                    div()
                        .debug_selector(|| "composer-model-menu".into())
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .absolute()
                        .bottom(px(28.))
                        .right_0()
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
                        .children(self.model_options.iter().enumerate().map(|(index, model)| {
                            let selected = index == self.model_selector_highlight;
                            let current_model = *model == self.composer_defaults.model;
                            let model = model.clone();
                            let label = model.clone();
                            div()
                                .h(px(Typography::SIDEBAR_LINE_HEIGHT))
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
                )
            })
            .into_any_element()
    }

    /// The thinking-level control (R2): one chip cycling only through the
    /// exact choices declared by the current provider/model profile.
    fn render_thinking_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let level = if self.composer_defaults.thinking.is_empty() {
            "provider_default"
        } else {
            self.composer_defaults.thinking.as_str()
        };
        let label = match level {
            "provider_default" => "提供方默认",
            "disabled" => "关闭",
            effort => effort,
        };
        div()
            .id("thinking-control")
            .key_context("ThinkingLevel")
            .on_action(cx.listener(Self::on_cycle_thinking))
            .track_focus(&self.setting_focus[6].clone())
            .px_2()
            .py_1()
            .rounded_md()
            .text_size(px(Typography::SIDEBAR))
            .text_color(if matches!(level, "provider_default" | "disabled") {
                colors.text_tertiary
            } else {
                colors.brand_primary
            })
            .cursor_pointer()
            .hover(move |style| style.bg(colors.bg_hover))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::cycle_thinking_clicked))
            .tooltip({
                let label = format!("推理深度：{label}");
                move |_, cx| crate::icons::tooltip(label.clone(), cx)
            })
            .when(self.compact_workspace, |trigger| {
                trigger.px_1().child(crate::icons::icon(
                    crate::icons::Icon::Thinking,
                    if matches!(level, "provider_default" | "disabled") {
                        colors.text_secondary
                    } else {
                        colors.brand_primary
                    },
                ))
            })
            .when(!self.compact_workspace, |trigger| {
                trigger
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(label.to_owned())
                    .child(crate::icons::icon(
                        crate::icons::Icon::ChevronDown,
                        colors.text_tertiary,
                    ))
            })
            .into_any_element()
    }
}

fn segment(
    label: &'static str,
    selected: bool,
    colors: ThemeColors,
    focus: FocusHandle,
    enabled: bool,
) -> gpui_kit::Div {
    div()
        .track_focus(&focus)
        .h(px(Typography::SIDEBAR_LINE_HEIGHT))
        .w_full()
        .px_2()
        .flex()
        .items_center()
        .rounded_md()
        .text_size(px(Typography::SIDEBAR))
        .when(enabled, |item| item.cursor_pointer())
        .when(selected, |item| {
            item.bg(colors.bg_active).text_color(colors.brand_primary)
        })
        .when(!selected && enabled, |item| {
            item.text_color(colors.text_secondary)
        })
        .when(!enabled, |item| item.text_color(colors.text_tertiary))
        .child(label)
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
            .when(self.mode_menu_open || self.permission_menu_open, |root| {
                root.key_context("CompactComposerSettings")
            })
            .on_action(cx.listener(Self::close_compact_settings))
            .on_key_down(cx.listener(Self::on_settings_menu_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.mode_menu_open || this.permission_menu_open || this.model_selector_open
                    {
                        this.mode_menu_open = false;
                        this.permission_menu_open = false;
                        this.model_selector_open = false;
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
