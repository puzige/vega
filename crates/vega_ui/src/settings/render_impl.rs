use super::*;

impl SettingsView {
    pub(crate) fn render_pricing(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(section_title(
                "模型定价（USD / 1M tokens）",
                colors.text_primary,
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(action_button(
                        "重新加载",
                        colors,
                        self.pricing_focus(&PricingFocusTarget::Reload),
                        cx.listener(|this, _, _, cx| {
                            if !matches!(this.pricing, PricingSettingsProjection::Saving { .. }) {
                                cx.emit(PricingReloadRequested);
                            }
                        }),
                    ))
                    .child(action_button(
                        "添加自定义",
                        colors,
                        self.pricing_focus(&PricingFocusTarget::Add),
                        cx.listener(|this, _, _, cx| {
                            this.begin_add_custom(cx);
                        }),
                    )),
            );

        let mut column = div().flex().flex_col().gap_2().child(header);
        match &self.pricing {
            PricingSettingsProjection::Loading | PricingSettingsProjection::Reloading => {
                column = column.child(pricing_status("正在加载定价…", colors.text_secondary));
            }
            PricingSettingsProjection::Invalid(code) => {
                column = column.child(pricing_status(pricing_error_label(*code), colors.danger));
            }
            PricingSettingsProjection::Ready {
                generation,
                entries,
                notice,
                draft_reason,
                error,
                ..
            } => {
                let entries = entries.clone();
                let generation = *generation;
                let notice = *notice;
                let draft_reason = *draft_reason;
                let error = *error;
                if let Some(notice) = notice {
                    column =
                        column.child(pricing_status(pricing_notice_label(notice), colors.warning));
                }
                if let Some(draft_reason) = draft_reason {
                    column = column.child(pricing_status(
                        match draft_reason {
                            PricingDraftReason::RetryPending => "保存未提交；原草稿可重试或放弃",
                            PricingDraftReason::ExternalConflict => {
                                "已采用外部有效版本；当前编辑草稿仍有冲突"
                            }
                        },
                        colors.warning,
                    ));
                    column = column.child(
                        div()
                            .flex()
                            .gap_2()
                            .child(action_button(
                                "重试原草稿",
                                colors,
                                self.pricing_focus(&PricingFocusTarget::Retry),
                                cx.listener(move |_, _, _, cx| {
                                    cx.emit(PricingRetryRequested { generation });
                                }),
                            ))
                            .child(action_button(
                                "放弃草稿",
                                colors,
                                self.pricing_focus(&PricingFocusTarget::Discard),
                                cx.listener(move |_, _, _, cx| {
                                    cx.emit(PricingDiscardRequested { generation });
                                }),
                            )),
                    );
                }
                if let Some(error) = error {
                    column =
                        column.child(pricing_status(pricing_error_label(error), colors.danger));
                }
                for (index, entry) in entries.into_iter().enumerate() {
                    column = column.child(self.render_pricing_entry(index, entry, cx));
                }
            }
            PricingSettingsProjection::Saving { entries, .. } => {
                let entries = entries.clone();
                column = column.child(pricing_status("正在保存并复验…", colors.text_secondary));
                for (index, entry) in entries.into_iter().enumerate() {
                    column = column.child(self.render_pricing_entry(index, entry, cx));
                }
            }
        }
        if let Some(editor) = self.pricing_editor.clone() {
            column = column.child(self.render_pricing_editor(editor, cx));
        }
        column.into_any_element()
    }

    pub(crate) fn render_pricing_entry(
        &mut self,
        index: usize,
        entry: PricingEntryProjection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let summary = if entry.kind == PricingEntryKind::BuiltInScheduled {
            "内置 · Base 4 项 + Peak 4 项 · UTC 时段锁定"
        } else if entry.kind == PricingEntryKind::CustomStatic {
            "自定义 · Static 4 项"
        } else {
            "内置 · Base 4 项 · Profile metadata 锁定"
        };
        let edit_entry = entry.clone();
        let model_for_reset = entry.model.clone();
        let model_for_delete = entry.model.clone();
        div()
            .flex()
            .items_center()
            .justify_between()
            .min_w_0()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().truncate().child(entry.model))
                    .child(
                        div()
                            .text_size(px(Typography::BODY))
                            .text_color(colors.text_secondary)
                            .child(summary),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(action_button(
                        "编辑",
                        colors,
                        self.pricing_focus(&PricingFocusTarget::Edit(index)),
                        cx.listener(move |this, _, _, cx| {
                            this.begin_edit_pricing(edit_entry.clone(), cx);
                        }),
                    ))
                    .when(entry.kind != PricingEntryKind::CustomStatic, |row| {
                        row.child(action_button(
                            "重置",
                            colors,
                            self.pricing_focus(&PricingFocusTarget::Secondary(index)),
                            cx.listener(move |this, _, _, cx| {
                                this.emit_pricing_mutation(
                                    PricingMutation::ResetBuiltin {
                                        model: model_for_reset.clone(),
                                    },
                                    cx,
                                );
                            }),
                        ))
                    })
                    .when(entry.kind == PricingEntryKind::CustomStatic, |row| {
                        row.child(action_button(
                            "删除",
                            colors,
                            self.pricing_focus(&PricingFocusTarget::Secondary(index)),
                            cx.listener(move |this, _, _, cx| {
                                this.emit_pricing_mutation(
                                    PricingMutation::DeleteCustom {
                                        model: model_for_delete.clone(),
                                    },
                                    cx,
                                );
                            }),
                        ))
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn render_pricing_editor(
        &mut self,
        editor: PricingEditor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let show_peak = editor.kind == PricingEditorKind::UpdateDeepSeek;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .rounded_lg()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .when(editor.kind == PricingEditorKind::AddCustom, |form| {
                form.child(self.pricing_model_input.clone())
            })
            .children(self.pricing_rate_inputs[..4].iter().cloned())
            .when(show_peak, |form| {
                form.children(self.pricing_rate_inputs[4..].iter().cloned())
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(action_button(
                        "保存定价",
                        colors,
                        self.pricing_focus(&PricingFocusTarget::Save),
                        cx.listener(|this, _, _, cx| {
                            this.submit_pricing_editor(cx);
                        }),
                    ))
                    .child(action_button(
                        "取消",
                        colors,
                        self.pricing_focus(&PricingFocusTarget::Cancel),
                        cx.listener(|this, _, _, cx| {
                            this.pricing_editor = None;
                            this.rebuild_pricing_focuses(cx);
                            cx.notify();
                        }),
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn on_back(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 与 Esc 同效：派发同一动作，由 app 级处理器统一收口。
        window.dispatch_action(Box::new(CloseSettings), cx);
    }

    pub(crate) fn on_submit(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.name_input.read(cx).text().trim().to_string();
        let base_url = self.base_url_input.read(cx).text().trim().to_string();
        let key = self.key_input.read(cx).text().to_string();
        if !form_is_submittable(&name, &base_url, &key) {
            // 空字段时按钮本应无效；这里的守卫保证即便触发也无副作用。
            return;
        }

        // 凭据只进 Keychain（安全红线）；key_ref 约定为 provider 名称。
        if let Err(error) = keystore::set_key(&name, &key) {
            self.error = Some(format!("Keychain 写入失败：{error}"));
            cx.notify();
            return;
        }

        upsert_provider(
            &mut self.config.providers,
            ProviderConfig {
                name: name.clone(),
                base_url,
                // 表单不编辑 models：新增时为空，同名更新时保留原值。
                models: Vec::new(),
                key_ref: name,
            },
        );

        // 立即落盘，满足"保存 → config.toml 更新；重启后配置恢复"。
        if let Err(error) = self.config.save() {
            self.error = Some(format!("配置保存失败：{error}"));
            cx.notify();
            return;
        }

        self.name_input.update(cx, TextInput::clear);
        self.base_url_input.update(cx, TextInput::clear);
        self.key_input.update(cx, TextInput::clear);
        self.error = None;
        cx.notify();
    }

    pub(crate) fn select_mode(&mut self, mode: &'static str, cx: &mut Context<Self>) {
        self.mode_open = false;
        if select_permission_mode(&mut self.config, mode).is_ok() {
            // 改动即保存。
            if let Err(error) = self.config.save() {
                self.error = Some(format!("配置保存失败：{error}"));
            }
        }
        cx.notify();
    }

    pub(crate) fn select_model(&mut self, model: &str, cx: &mut Context<Self>) {
        self.model_open = false;
        set_default_model(&mut self.config, model);
        // 改动即保存。
        if let Err(error) = self.config.save() {
            self.error = Some(format!("配置保存失败：{error}"));
        }
        cx.notify();
    }

    pub(crate) fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::SIDEBAR))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_back))
                    .child("返回"),
            )
            .child(
                div()
                    .text_size(px(Typography::HEADING_PAGE))
                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                    .child("设置"),
            )
            .into_any_element()
    }

    pub(crate) fn render_provider_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("Provider", colors.text_primary))
            .children(self.config.providers.is_empty().then(|| {
                div()
                    .text_color(colors.text_tertiary)
                    .text_size(px(Typography::BODY))
                    .child("暂无 Provider，使用下方表单添加")
                    .into_any_element()
            }))
            .children(self.config.providers.iter().map(|provider| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(Typography::HEADING_CARD))
                                    .font_weight(Typography::HEADING_CARD_WEIGHT)
                                    .child(provider.name.clone()),
                            )
                            // 凡 key_ref 非空即显示已存储占位，永不回显 key 值。
                            .children((!provider.key_ref.is_empty()).then(|| {
                                div()
                                    .text_color(colors.text_secondary)
                                    .text_size(px(Typography::BODY))
                                    .child(KEY_STORED_PLACEHOLDER)
                            })),
                    )
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .text_size(px(Typography::BODY))
                            .child(provider.base_url.clone()),
                    )
                    .children((!provider.models.is_empty()).then(|| {
                        div()
                            .text_color(colors.text_tertiary)
                            .text_size(px(Typography::BODY))
                            .child(provider.models.join(", "))
                    }))
            }))
            .into_any_element()
    }

    pub(crate) fn render_add_form(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        // 渲染期读取输入内容：既驱动提交按钮的有效态，也让每次键入
        // 重新渲染本视图（GPUI 的渲染期读取即依赖注册）。
        let name = self.name_input.read(cx).text().to_string();
        let base_url = self.base_url_input.read(cx).text().to_string();
        let key = self.key_input.read(cx).text().to_string();
        let submittable = form_is_submittable(&name, &base_url, &key);

        let (button_bg, button_text) = if submittable {
            (colors.accent, colors.bg_base)
        } else {
            (colors.bg_hover, colors.text_tertiary)
        };

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("添加 Provider", colors.text_primary))
            .child(self.name_input.clone())
            .child(self.base_url_input.clone())
            .child(self.key_input.clone())
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .self_start()
                    .bg(button_bg)
                    .text_color(button_text)
                    .text_size(px(Typography::SIDEBAR))
                    .when(submittable, |button| {
                        button
                            .cursor_pointer()
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_submit))
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn render_defaults(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("默认项", colors.text_primary))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(field_label("权限模式", colors.text_secondary))
                    .child(self.render_mode_selector(cx)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(field_label("模型", colors.text_secondary))
                    .child(self.render_model_selector(cx)),
            )
            .into_any_element()
    }

    /// In-place expandable picker for the permission mode (minimal equivalent
    /// of a dropdown: click to expand, click an option to choose and collapse).
    pub(crate) fn render_mode_selector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let current = self.config.defaults.permission_mode.clone();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::BODY))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                            this.mode_open = !this.mode_open;
                            cx.notify();
                        }),
                    )
                    .child(current.clone())
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .child(if self.mode_open { "▾" } else { "▸" }),
                    ),
            )
            .when(self.mode_open, |column| {
                column.children(PERMISSION_MODES.iter().map(|mode| {
                    let selected = *mode == current;
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_size(px(Typography::BODY))
                        .cursor_pointer()
                        .when(selected, move |row| row.bg(colors.bg_active))
                        .when(!selected, move |row| {
                            row.hover(move |s| s.bg(colors.bg_hover))
                        })
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                this.select_mode(mode, cx);
                            }),
                        )
                        .child(*mode)
                }))
            })
            .into_any_element()
    }

    /// In-place expandable picker for the default model; options are the
    /// union of all providers' models, with an empty-state hint when none.
    pub(crate) fn render_model_selector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let models = all_models(&self.config.providers);
        let current = self.config.defaults.model.clone();
        let trigger_label = if current.is_empty() {
            "未选择".to_string()
        } else {
            current.clone()
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::BODY))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                            this.model_open = !this.model_open;
                            cx.notify();
                        }),
                    )
                    .child(trigger_label)
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .child(if self.model_open { "▾" } else { "▸" }),
                    ),
            )
            .when(self.model_open, |column| {
                column
                    .children(models.is_empty().then(|| {
                        div()
                            .text_color(colors.text_tertiary)
                            .text_size(px(Typography::BODY))
                            .child("暂无可选模型，先在下方添加 Provider")
                    }))
                    .children(models.iter().map(|model| {
                        let model = model.clone();
                        let selected = model == current;
                        let row_model = model.clone();
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(Typography::BODY))
                            .cursor_pointer()
                            .when(selected, move |row| row.bg(colors.bg_active))
                            .when(!selected, move |row| {
                                row.hover(move |s| s.bg(colors.bg_hover))
                            })
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                    this.select_model(&row_model, cx);
                                }),
                            )
                            .child(model)
                    }))
            })
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .id("settings-page")
            .key_context("PricingSettings")
            .on_action(cx.listener(Self::activate_pricing_action))
            .on_action(cx.listener(Self::next_pricing_action))
            .on_action(cx.listener(Self::previous_pricing_action))
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .line_height(relative(Typography::BODY_LINE_HEIGHT))
            .overflow_y_scroll()
            .child(
                // Content column per UI spec §1: max 820px, centered, ≥24px
                // side padding.
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w_full()
                    .max_w(px(820.))
                    .mx_auto()
                    .px(px(24.))
                    .py(px(24.))
                    .child(self.render_header(cx))
                    .children(self.error.clone().map(|message| {
                        div()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(colors.bg_elevated)
                            .border_1()
                            .border_color(colors.danger)
                            .text_color(colors.danger)
                            .text_size(px(Typography::BODY))
                            .child(message)
                    }))
                    .child(self.render_provider_list(cx))
                    .child(self.render_add_form(cx))
                    .child(self.render_defaults(cx))
                    .child(self.render_pricing(cx)),
            )
    }
}
