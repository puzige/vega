use super::*;

impl ConversationStream {
    fn emit_open_diff(&mut self, cx: &mut Context<Self>) {
        cx.emit(OpenWorkspaceDiffRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
        });
    }

    fn open_diff_clicked(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.emit_open_diff(cx);
    }

    fn open_diff_action(&mut self, _: &OpenWorkspaceDiff, _: &mut Window, cx: &mut Context<Self>) {
        self.emit_open_diff(cx);
    }

    fn open_commit_clicked(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.trusted_action_busy {
            return;
        }
        cx.emit(OpenCommitPanelRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
        });
    }

    /// Renders the thread header: title + anchor status + demo button.
    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let title = if self.thread.title.is_empty() {
            "未命名任务".to_string()
        } else {
            self.thread.title.clone()
        };
        let following = self.following_tail();
        let (injected, total) = self
            .injecting
            .as_ref()
            .map(|injection| (injection.replay.injected(), injection.replay.total()))
            .unwrap_or((0, 0));
        div()
            .px(px(CONTENT_MIN_PADDING))
            .py(px(12.))
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(Typography::HEADING_PAGE))
                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                    .child(title),
            )
            .child(
                // 锚定状态指示（P4 走查辅助）。
                div()
                    .flex_shrink_0()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_tertiary)
                    .child(if following {
                        "跟随中"
                    } else {
                        "已脱离 · 回到底部恢复"
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_tertiary)
                    .child("S3 演示"),
            )
            .child(
                // 演示注入按钮（驱动 vega_markdown::MockReplay 公共回放器）。
                div()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_secondary)
                    .when(!self.trusted_action_busy, |button| button.cursor_pointer())
                    .hover(move |style| style.bg(colors.bg_hover))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::start_demo_injection))
                    .child(if injected > 0 {
                        format!("演示注入中 {injected}/{total} δ")
                    } else {
                        "演示注入".to_string()
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_secondary)
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.bg_hover))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::open_diff_clicked))
                    .child("Diff"),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if self.trusted_action_busy {
                        colors.text_tertiary
                    } else {
                        colors.text_secondary
                    })
                    .when(!self.trusted_action_busy, |button| {
                        button
                            .cursor_pointer()
                            .hover(move |style| style.bg(colors.bg_hover))
                    })
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::open_commit_clicked))
                    .child("Commit"),
            )
            .into_any_element()
    }

    /// Renders the Composer (ui-spec §4.4)：底部固定、圆角 12px
    /// （rounded_xl）、1px border_subtle、bg_elevated；1~8 行自适应多行输入
    /// （超出内滚，S8-T47 P0-3 已由 [`TextInput`] 自适应视口承担）+
    /// [发送] 按钮（空输入禁用）+ `@file` 选择器（A2-12）+ 模型选择器与
    /// thinking 档位（A2-14）。命令面板仍为 Composer 完全体后续范围。
    fn render_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let can_send = !self.input.read(cx).text().is_empty()
            && !self.composer_submit_pending
            && !self.approved_not_started
            && !self.trusted_action_busy;
        div()
            .px(px(CONTENT_MIN_PADDING))
            .pt(px(8.))
            .pb(px(12.))
            .border_t_1()
            .border_color(colors.border_subtle)
            .child(
                div()
                    .w_full()
                    // Cmd+Enter 的按键上下文（绑定见 vega_ui::init）。
                    .key_context("Composer")
                    .on_action(cx.listener(Self::on_send_action))
                    .on_action(cx.listener(Self::on_previous_message))
                    .on_action(cx.listener(Self::on_selector_previous))
                    .on_action(cx.listener(Self::on_selector_next))
                    .on_action(cx.listener(Self::on_selector_cancel))
                    .on_action(cx.listener(Self::on_selector_accept))
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
                    .rounded_xl()
                    .p_2()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(self.render_mode_controls(cx))
                            .child(self.render_permission_controls(cx))
                            .child(self.branch_selector.clone())
                            .child(self.render_model_selector(cx))
                            .child(self.render_thinking_control(cx)),
                    )
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_row()
                            .items_end()
                            .gap_2()
                            // 选择器打开时的按键作用域（A2-12）：仅当下拉
                            // 打开才挂 FileSelect 上下文，Up/Down/Enter/Tab/
                            // Esc 先到选择器（first-wins），关闭时回落到
                            // Composer 既有绑定（Enter=换行、Up=历史召回）。
                            .when(self.file_selector.is_open(), |row| {
                                row.key_context("FileSelect")
                            })
                            .child(self.render_file_dropdown(cx))
                            .child(self.input.clone())
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .text_size(px(Typography::SIDEBAR))
                                    .when(can_send, |button| {
                                        button
                                            .bg(colors.accent)
                                            .text_color(colors.bg_base)
                                            .cursor_pointer()
                                            .on_mouse_up(
                                                MouseButton::Left,
                                                cx.listener(Self::on_send_clicked),
                                            )
                                    })
                                    .when(!can_send, |button| {
                                        button.bg(colors.bg_hover).text_color(colors.text_tertiary)
                                    })
                                    .child("发送"),
                            ),
                    )
                    // ui-spec §4.4 token 计数器：右下角常驻 compact counter
                    // （S7-T39/C4）。数据只来自 conversation meter 投影；
                    // 更新路径零 IO（checked 整数运算），数字宽度变化只影响
                    // 本行文本，不触碰已冻结会话区（P3 不回退）。
                    .child(
                        div()
                            .flex()
                            .w_full()
                            .justify_end()
                            .text_size(px(Typography::SIDEBAR))
                            .text_color(colors.text_tertiary)
                            .child(self.meter.snapshot().display()),
                    ),
            )
            .children(self.controller_error.clone().map(|error| {
                div()
                    .mt_1()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.danger)
                    .child(error)
            }))
            .into_any_element()
    }

    fn render_mode_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_row()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .child(
                segment(
                    "Ask",
                    self.thread.mode == ThreadMode::Ask,
                    colors,
                    self.setting_focus[0].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_ask))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_ask)),
            )
            .child(
                segment(
                    "Plan",
                    self.thread.mode == ThreadMode::Plan,
                    colors,
                    self.setting_focus[1].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_plan))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_plan)),
            )
            .child(
                segment(
                    "Execute",
                    self.thread.mode == ThreadMode::Execute,
                    colors,
                    self.setting_focus[2].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_execute))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_execute)),
            )
            .into_any_element()
    }

    fn render_permission_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_row()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .child(
                segment(
                    "ReadOnly",
                    self.thread.permission_mode == PermissionMode::ReadOnly,
                    colors,
                    self.setting_focus[3].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_readonly))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_readonly)),
            )
            .child(
                segment(
                    "Confirm",
                    self.thread.permission_mode == PermissionMode::Confirm,
                    colors,
                    self.setting_focus[4].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_confirm))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_confirm)),
            )
            .child(
                segment(
                    "Auto",
                    self.thread.permission_mode == PermissionMode::Auto,
                    colors,
                    self.setting_focus[5].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_auto))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_auto)),
            )
            .into_any_element()
    }

    /// The `@file` suggestion dropdown (A2-12): rendered above the input row
    /// while an `@token` is being completed. Bounded candidate list, mouse
    /// and keyboard parity; zero filesystem access from this view.
    fn render_file_dropdown(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        if !self.file_selector.is_open() {
            return div().into_any_element();
        }
        let highlighted = self.file_selector.highlighted();
        div()
            .absolute()
            .bottom(px(0.))
            .left_0()
            .w(px(360.))
            .max_w_full()
            .flex()
            .flex_col()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .text_color(colors.text_primary)
            .shadow_md()
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
                            .when(selected, |row| row.bg(colors.bg_active))
                            .text_color(if selected {
                                colors.text_primary
                            } else {
                                colors.text_secondary
                            })
                            .child(entry)
                    }),
            )
            .into_any_element()
    }

    /// The model selector (A2-14): trigger shows the current selection;
    /// options are the priced catalog projection installed by the app layer
    /// (zero file IO). Keyboard: Enter/Space open, Up/Down move, Enter
    /// accept (first-wins), Esc close.
    fn render_model_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let current = if self.composer_defaults.model.is_empty() {
            "模型"
        } else {
            self.composer_defaults.model.as_str()
        };
        div()
            .relative()
            .child(
                div()
                    .track_focus(&self.model_focus)
                    .key_context("ModelSelector")
                    .on_action(cx.listener(Self::on_activate_model))
                    .on_action(cx.listener(Self::on_model_previous))
                    .on_action(cx.listener(Self::on_model_next))
                    .on_action(cx.listener(Self::on_model_close))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_secondary)
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, window, cx| {
                            this.on_activate_model(&ActivateModel, window, cx);
                        }),
                    )
                    .child(format!("{current} ▾")),
            )
            .when(self.model_selector_open, |root| {
                root.child(
                    div()
                        .absolute()
                        .bottom(px(28.))
                        .left_0()
                        .w(px(320.))
                        .max_w_full()
                        .flex()
                        .flex_col()
                        .rounded_md()
                        .border_1()
                        .border_color(colors.border_subtle)
                        .bg(colors.bg_elevated)
                        .text_color(colors.text_primary)
                        .shadow_md()
                        .children(self.model_options.iter().enumerate().map(|(index, model)| {
                            let selected = index == self.model_selector_highlight;
                            let current_model = *model == self.composer_defaults.model;
                            let model = model.clone();
                            let label = model.clone();
                            div()
                                .px_2()
                                .py_1()
                                .text_size(px(Typography::SIDEBAR))
                                .truncate()
                                .when(selected, |row| row.bg(colors.bg_active))
                                .text_color(if current_model {
                                    colors.success
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

    /// The thinking-level control (A2-14): one segmented chip cycling
    /// off → low → medium → high on click/Enter (mouse + keyboard parity,
    /// ui-spec §6).
    fn render_thinking_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let level = if self.composer_defaults.thinking.is_empty() {
            "off"
        } else {
            self.composer_defaults.thinking.as_str()
        };
        div()
            .key_context("ThinkingLevel")
            .on_action(cx.listener(Self::on_cycle_thinking))
            .track_focus(&self.setting_focus[6].clone())
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .text_size(px(Typography::SIDEBAR))
            .text_color(if level == "off" {
                colors.text_tertiary
            } else {
                colors.success
            })
            .cursor_pointer()
            .hover(move |style| style.bg(colors.bg_hover))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::cycle_thinking_clicked))
            .child(format!("思考:{level}"))
            .into_any_element()
    }
}

fn segment(
    label: &'static str,
    selected: bool,
    colors: ThemeColors,
    focus: FocusHandle,
) -> gpui::Div {
    div()
        .track_focus(&focus)
        .px_2()
        .py_1()
        .text_size(px(Typography::SIDEBAR))
        .cursor_pointer()
        .when(selected, |item| {
            item.bg(colors.bg_active).text_color(colors.text_primary)
        })
        .when(!selected, |item| item.text_color(colors.text_secondary))
        .child(label)
}

impl Render for ConversationStream {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_t0 = Instant::now();
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
            // §4.6 空态：内存态会话从演示注入或 Composer 开始。
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(colors.text_tertiary)
                .text_size(px(Typography::BODY))
                .child("会话内容为空：点右上「演示注入」以 ~500 δ/s 流式生成，或在下方输入后发送（S3 内存态）")
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
            .on_action(cx.listener(Self::open_diff_action))
            // tech-spec §5.4 动效禁令：流式期间节点无任何入场 opacity/动画
            // （本管线自 T17 起即不引入入场动画，T18 维持）。
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .child(body),
            )
            .child(self.render_composer(cx))
            .child(self.commit_panel.clone())
            .into_any_element();
        counters.record_render(render_t0);
        element
    }
}
