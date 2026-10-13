use super::*;
use crate::interaction::InteractionExt;
use crate::settings::state::{
    PROVIDER_MODELS_FRAME_INSET, PROVIDER_MODELS_MAX_ROWS, PROVIDER_MODELS_MIN_ROWS,
};

impl SettingsView {
    pub(crate) fn render_pricing(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let selected_model = self.model_settings.clone();
        let header = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
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

        let mut column = settings_section()
            .child(if self.model_settings.is_some() {
                "此模型的共享价格 · 所有提供方共用"
            } else {
                "模型价格库 · 所有提供方共享"
            })
            .child(header);
        match &self.pricing {
            PricingSettingsProjection::Loading | PricingSettingsProjection::Reloading => {
                column = column.child(settings_status(
                    "正在加载定价…",
                    colors.text_secondary,
                    colors,
                ));
            }
            PricingSettingsProjection::Invalid(code) => {
                column = column.child(settings_status(
                    pricing_error_label(*code),
                    colors.danger,
                    colors,
                ));
            }
            PricingSettingsProjection::Ready {
                generation,
                entries,
                notice,
                draft_reason,
                error,
                ..
            } => {
                let entries = if self.pricing_editor.is_some() {
                    Vec::new()
                } else {
                    entries.clone()
                };
                let generation = *generation;
                let notice = *notice;
                let draft_reason = *draft_reason;
                let error = *error;
                if let Some(notice) = notice {
                    column = column.child(settings_status(
                        pricing_notice_label(notice),
                        colors.warning,
                        colors,
                    ));
                }
                if let Some(draft_reason) = draft_reason {
                    column = column.child(settings_status(
                        match draft_reason {
                            PricingDraftReason::RetryPending => "保存未提交；原草稿可重试或放弃",
                            PricingDraftReason::ExternalConflict => {
                                "已采用外部有效版本；当前编辑草稿仍有冲突"
                            }
                        },
                        colors.warning,
                        colors,
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
                    column = column.child(settings_status(
                        pricing_error_label(error),
                        colors.danger,
                        colors,
                    ));
                }
                for (index, entry) in entries.into_iter().enumerate().filter(|(_, entry)| {
                    selected_model
                        .as_ref()
                        .is_none_or(|(_, model)| entry.model == *model)
                }) {
                    column = column.child(self.render_pricing_entry(index, entry, cx));
                }
            }
            PricingSettingsProjection::Saving { entries, .. } => {
                let entries = entries.clone();
                column = column.child(settings_status(
                    "正在保存并复验…",
                    colors.text_secondary,
                    colors,
                ));
                for (index, entry) in entries.into_iter().enumerate().filter(|(_, entry)| {
                    selected_model
                        .as_ref()
                        .is_none_or(|(_, model)| entry.model == *model)
                }) {
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
        settings_section()
            .py_3()
            .border_b_1()
            .border_color(colors.border_subtle)
            .flex_row()
            .items_center()
            .gap(px(Layout::SETTINGS_FIELD_GAP))
            .flex_wrap()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .debug_selector(move || format!("pricing-model-name-{index}"))
                            .min_w_0()
                            .child(entry.model),
                    )
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
                    .flex_shrink_0()
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
        let expanded = self.pricing_advanced;
        settings_group(colors)
            .when_some(editor.model.clone(), |form, model| {
                form.child(
                    div()
                        .debug_selector(|| "pricing-editor-model".into())
                        .min_w_0()
                        .mb_3()
                        .font_weight(Typography::HEADING_CARD_WEIGHT)
                        .child(model),
                )
            })
            .when(editor.kind == PricingEditorKind::AddCustom, |form| {
                form.child(settings_field(
                    "pricing-model",
                    "模型 ID",
                    None,
                    self.pricing_model_input.clone(),
                    colors,
                ))
            })
            .children(
                self.pricing_rate_inputs
                    .iter()
                    .enumerate()
                    .take(if show_peak && expanded { 8 } else { 4 })
                    .map(|(index, input)| {
                        let label = [
                            "基础输入",
                            "基础输出",
                            "基础缓存读取",
                            "基础缓存写入",
                            "高峰输入",
                            "高峰输出",
                            "高峰缓存读取",
                            "高峰缓存写入",
                        ][index];
                        settings_field(
                            format!("pricing-rate-{index}"),
                            label,
                            Some("USD / 1M tokens".into()),
                            input.clone(),
                            colors,
                        )
                    }),
            )
            .when(show_peak, |form| {
                form.child(self.settings_route_button(
                    "pricing-advanced",
                    if expanded {
                        "收起高峰费率"
                    } else {
                        "高峰费率"
                    },
                    true,
                    cx.listener(|this, _, _, cx| {
                        this.pricing_advanced = !this.pricing_advanced;
                        cx.notify();
                    }),
                    cx,
                ))
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
        if self.settings_have_draft(cx) {
            self.pending_settings_close = true;
            self.pending_section = Some(SettingsDestination::ModelSection(self.section));
            cx.notify();
            return;
        }
        self.cancel_provider_operation(cx);
        self.cancel_mcp_oauth(cx);
        self.close_skills_session();
        window.dispatch_action(Box::new(CloseSettings), cx);
    }

    pub(crate) fn on_submit(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_provider(cx);
    }

    pub(crate) fn select_mode(&mut self, mode: &'static str, cx: &mut Context<Self>) {
        self.mode_open = false;
        let mut candidate = self.config.clone();
        if select_permission_mode(&mut candidate, mode).is_ok() {
            self.save_preferences(candidate, cx);
        }
        cx.notify();
    }

    pub(crate) fn select_model(&mut self, model: &str, cx: &mut Context<Self>) {
        self.model_open = false;
        let mut candidate = self.config.clone();
        set_default_model(&mut candidate, model);
        self.save_preferences(candidate, cx);
        cx.notify();
    }

    pub(crate) fn render_add_form(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        // 渲染期读取输入内容：既驱动提交按钮的有效态，也让每次键入
        // 重新渲染本视图（GPUI 的渲染期读取即依赖注册）。
        let name = self.name_input.read(cx).text().to_string();
        let base_url = self.base_url_input.read(cx).text().to_string();
        let models_rows = self
            .models_input
            .read(cx)
            .visible_rows()
            .clamp(PROVIDER_MODELS_MIN_ROWS, PROVIDER_MODELS_MAX_ROWS);
        let models_height = px(models_rows as f32
            * Typography::BODY
            * Typography::BODY_LINE_HEIGHT
            + PROVIDER_MODELS_FRAME_INSET);
        let key = self.key_input.read(cx).text().to_string();
        let existing = self.provider_management.form_base.is_some()
            || (!self.provider_management.form
                && self
                    .config
                    .providers
                    .iter()
                    .any(|provider| provider.name == name.trim()));
        let submittable = !self.provider_management.saving
            && provider_form_is_submittable(&name, &base_url, &key, existing);

        let (button_bg, button_text) = if submittable {
            (colors.brand_primary, colors.brand_on_accent)
        } else {
            (colors.bg_hover, colors.text_tertiary)
        };

        settings_group(colors)
            .key_context("ProviderSettings")
            .child(section_title("添加或更新 Provider", colors.text_primary))
            .child(
                settings_actions().children(
                    [
                        (
                            vega_conversation::types::ProviderApi::ChatCompletions,
                            "Chat Completions",
                            "provider-api-chat",
                        ),
                        (
                            vega_conversation::types::ProviderApi::Responses,
                            "Responses",
                            "provider-api-responses",
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (api, label, id))| {
                        settings_action_chrome(colors)
                            .id(id)
                            .interaction_surface(if self.provider_management.form_api == api { colors.bg_active } else { colors.bg_elevated }, colors.bg_hover, colors.bg_active, !self.provider_management.saving)
                            .debug_selector(move || id.into())
                            .cursor_pointer()
                            .track_focus(&self.provider_api_focuses[index])
                            .tab_stop(true)
                            .aria_label(label)
                            .focus_visible(move |row| row.border_color(colors.accent))
                            .on_key_down(cx.listener(
                                move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                    if !this.provider_management.saving
                                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                                    {
                                        this.provider_management.form_api = api;
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            ))
                            .bg(if self.provider_management.form_api == api {
                                colors.bg_hover
                            } else {
                                colors.bg_elevated
                            })
                            .text_color(colors.text_primary)
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    if !this.provider_management.saving {
                                        this.provider_management.form_api = api;
                                        cx.notify();
                                    }
                                }),
                            )
                            .child(label)
                    }),
                ),
            )
            .child(settings_field("provider-name", "名称", None, self.name_input.clone(), colors))
            .child(settings_field("provider-url", "服务地址", None, self.base_url_input.clone(), colors))
            .child(settings_field("provider-models", "模型 ID", None,
                div()
                    .id("provider-models-input-frame")
                    .debug_selector(|| "provider-models-input-frame".to_string())
                    .w_full()
                    .h(models_height)
                    .flex_shrink_0()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .p_2()
                    .child(self.models_input.clone()), colors))
            .child(
                div()
                    .debug_selector(|| "provider-models-input-help".to_string())
                    .text_color(colors.text_tertiary)
                    .text_size(px(Typography::BODY))
                    .child(format!(
                        "每行填写一个模型 ID；保留大小写和 / - .；最多 {} 个",
                        PROVIDER_MODEL_COUNT_MAX
                    )),
            )
            .child(div().text_color(colors.text_tertiary).text_size(px(Typography::BODY))
                .child("Responses 请求摘要；需要兼容的 reasoning 配置，不支持原始思考回传或 Zhipu thinking 开关。"))
            .child(settings_field("provider-key", "API Key", Some("更新时留空以保留现有密钥".into()), self.key_input.clone(), colors))

            .child(
                settings_action_chrome(colors)
                    .id("provider-save")
                    .interaction_surface(button_bg, colors.brand_primary_strong, colors.brand_primary_strong, submittable)
                    .debug_selector(|| "provider-save".into())
                    .key_context("ProviderAction")
                    .track_focus(&self.provider_save_focus)
                    .tab_stop(true)
                    .focus_visible(move |style| style.border_color(colors.accent))
                    .bg(button_bg)
                    .text_color(button_text)
                    .text_size(px(Typography::SIDEBAR))
                    .when(submittable, |button| {
                        button
                            .cursor_pointer()
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_submit))
                    })
                    .child("保存 Provider"),
            )
            .into_any_element()
    }

    fn set_appearance(&mut self, value: &str, cx: &mut Context<Self>) {
        let mut candidate = self.config.clone();
        candidate.ui.theme = value.to_string();
        if let Some(sidebar) = cx.try_global::<crate::sidebar::SidebarCollapsed>() {
            candidate.ui.sidebar_collapsed = sidebar.0;
        }
        candidate.ui.sidebar_width = crate::sidebar::width(cx);
        self.save_preferences(candidate, cx);
    }

    pub(crate) fn render_defaults(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let sidebar_visible = !cx
            .try_global::<crate::sidebar::SidebarCollapsed>()
            .is_some_and(|sidebar| sidebar.0);
        let themes = settings_actions().children(
            [("light", "浅色"), ("dark", "深色"), ("system", "跟随系统")]
                .into_iter()
                .map(|(value, label)| {
                    settings_action_chrome(colors)
                        .id(label)
                        .interaction_surface(
                            if self.config.ui.theme == value {
                                colors.bg_active
                            } else {
                                colors.bg_elevated
                            },
                            if self.config.ui.theme == value {
                                colors.bg_active
                            } else {
                                colors.bg_hover
                            },
                            colors.bg_active,
                            !self.provider_management.saving,
                        )
                        .focusable()
                        .tab_stop(true)
                        .focus_visible(move |style| style.border_color(colors.accent))
                        .cursor_pointer()
                        .child(label)
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| this.set_appearance(value, cx)),
                        )
                        .on_key_down(cx.listener(
                            move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.set_appearance(value, cx);
                                    cx.stop_propagation();
                                }
                            },
                        ))
                }),
        );
        let sidebar = settings_switch(
            "settings-sidebar-switch",
            if sidebar_visible {
                "隐藏侧栏"
            } else {
                "显示侧栏"
            },
            sidebar_visible,
            true,
            colors,
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|_, _, window, cx| {
                window.dispatch_action(Box::new(crate::sidebar::ToggleSidebar), cx)
            }),
        )
        .on_key_down(
            cx.listener(|_, event: &gpui_kit::KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    window.dispatch_action(Box::new(crate::sidebar::ToggleSidebar), cx);
                    cx.stop_propagation();
                }
            }),
        );
        settings_section()
            .gap(px(Layout::SETTINGS_SECTION_GAP))
            .child(
                settings_group(colors)
                    .debug_selector(|| "settings-general-appearance".into())
                    .child(section_title("外观与布局", colors.text_primary))
                    .child(settings_field(
                        "settings-theme",
                        "主题",
                        None,
                        themes,
                        colors,
                    ))
                    .child(settings_field(
                        "settings-sidebar",
                        "侧栏",
                        None,
                        sidebar,
                        colors,
                    )),
            )
            .child(
                settings_group(colors)
                    .debug_selector(|| "settings-general-defaults".into())
                    .child(section_title("默认项", colors.text_primary))
                    .child(settings_field(
                        "settings-permission",
                        "权限模式",
                        None,
                        self.render_mode_selector(cx),
                        colors,
                    ))
                    .child(settings_field(
                        "settings-model",
                        "模型",
                        None,
                        self.render_model_selector(cx),
                        colors,
                    ))
                    .child(settings_field(
                        "settings-running-message",
                        "运行中发送消息",
                        None,
                        self.render_running_message_behavior(cx),
                        colors,
                    )),
            )
            .child(self.render_updater(cx))
            .into_any_element()
    }

    fn render_running_message_behavior(&self, cx: &mut Context<Self>) -> AnyElement {
        use vega_conversation::types::PendingMessageBehavior;
        let colors = theme(cx).colors;
        let enabled = !self.provider_management.saving;
        settings_actions()
            .children(
                [PendingMessageBehavior::Queue, PendingMessageBehavior::Steer]
                    .into_iter()
                    .map(|behavior| {
                        let selected =
                            self.config.agent.running_message_behavior == behavior.as_str();
                        let label = behavior.label();
                        let description = match behavior {
                            PendingMessageBehavior::Queue => "当前任务结束后发送",
                            PendingMessageBehavior::Steer => "补充要求或调整当前任务方向",
                        };
                        let id = format!("settings-running-message-{}", behavior.as_str());
                        let selector = id.clone();
                        settings_action_chrome(colors)
                            .id(id)
                            .debug_selector(move || selector.clone())
                            .aria_label(if selected {
                                format!("{label}，已选中")
                            } else {
                                label.to_string()
                            })
                            .aria_description(description)
                            .interaction_surface(
                                if selected {
                                    colors.bg_active
                                } else {
                                    colors.bg_elevated
                                },
                                if selected {
                                    colors.bg_active
                                } else {
                                    colors.bg_hover
                                },
                                colors.bg_active,
                                enabled,
                            )
                            .focusable()
                            .tab_stop(true)
                            .focus_visible(move |style| style.border_color(colors.accent))
                            .when(enabled, |element| element.cursor_pointer())
                            .tooltip(move |_, cx| crate::icons::tooltip(description, cx))
                            .child(label)
                            .child(div().size(px(Layout::ICON_SIZE)).when(selected, |element| {
                                element
                                    .debug_selector(move || {
                                        format!(
                                            "settings-running-message-{}-selected",
                                            behavior.as_str()
                                        )
                                    })
                                    .child(crate::icons::icon(
                                        crate::icons::Icon::Check,
                                        colors.text_primary,
                                    ))
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    if enabled {
                                        this.select_running_message_behavior(behavior, cx);
                                    }
                                }),
                            )
                            .on_key_down(cx.listener(
                                move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                    if enabled
                                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                                    {
                                        this.select_running_message_behavior(behavior, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                    }),
            )
            .into_any_element()
    }

    /// In-place expandable picker for the permission mode (minimal equivalent
    /// of a dropdown: click to expand, click an option to choose and collapse).
    pub(crate) fn render_mode_selector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let current = self.config.defaults.permission_mode.clone();
        div()
            .min_w_0()
            .max_w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                settings_action_chrome(colors)
                    .id("settings-mode-picker")
                    .interaction_surface(
                        colors.bg_elevated,
                        colors.bg_hover,
                        colors.bg_active,
                        true,
                    )
                    .debug_selector(|| "settings-mode-picker".into())
                    .focusable()
                    .tab_stop(true)
                    .focus_visible(move |style| style.border_color(colors.accent))
                    .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.mode_open = !this.mode_open;
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                            this.mode_open = !this.mode_open;
                            cx.notify();
                        }),
                    )
                    .child(if current == "full_access" {
                        "完全访问".to_string()
                    } else {
                        current.clone()
                    })
                    .child(crate::icons::icon(
                        if self.mode_open {
                            crate::icons::Icon::ChevronDown
                        } else {
                            crate::icons::Icon::ChevronRight
                        },
                        colors.text_tertiary,
                    )),
            )
            .when(self.mode_open, |column| {
                column.children(PERMISSION_MODES.iter().map(|mode| {
                    let selected = *mode == current;
                    div()
                        .id(gpui_kit::SharedString::from(format!(
                            "settings-permission-option-{mode}"
                        )))
                        .interaction_surface(
                            if selected {
                                colors.bg_active
                            } else {
                                colors.bg_elevated
                            },
                            if selected {
                                colors.bg_active
                            } else {
                                colors.bg_hover
                            },
                            colors.bg_active,
                            true,
                        )
                        .debug_selector(move || format!("settings-permission-option-{mode}"))
                        .focusable()
                        .tab_stop(true)
                        .border_1()
                        .border_color(gpui_kit::transparent_black())
                        .focus_visible(move |style| style.border_color(colors.accent))
                        .on_key_down(cx.listener(
                            move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.select_mode(mode, cx);
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
                        .py_1()
                        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                        .text_size(px(Typography::BODY))
                        .cursor_pointer()
                        .when(selected, move |row| {
                            row.bg(colors.bg_active).text_color(colors.brand_primary)
                        })
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                this.select_mode(mode, cx);
                            }),
                        )
                        .when(*mode == "full_access", |row| {
                            row.flex()
                                .items_center()
                                .gap_2()
                                .text_color(colors.warning)
                                .child(crate::icons::icon(
                                    crate::icons::Icon::Warning,
                                    colors.warning,
                                ))
                        })
                        .child(if *mode == "full_access" {
                            "完全访问"
                        } else {
                            *mode
                        })
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
            .min_w_0()
            .max_w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                settings_action_chrome(colors)
                    .id("settings-model-picker")
                    .interaction_surface(
                        colors.bg_elevated,
                        colors.bg_hover,
                        colors.bg_active,
                        true,
                    )
                    .debug_selector(|| "settings-model-picker".into())
                    .focusable()
                    .tab_stop(true)
                    .focus_visible(move |style| style.border_color(colors.accent))
                    .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.model_open = !this.model_open;
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                            this.model_open = !this.model_open;
                            cx.notify();
                        }),
                    )
                    .child(div().min_w_0().child(trigger_label))
                    .child(crate::icons::icon(
                        if self.model_open {
                            crate::icons::Icon::ChevronDown
                        } else {
                            crate::icons::Icon::ChevronRight
                        },
                        colors.text_tertiary,
                    )),
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
                        let keyboard_model = model.clone();
                        div()
                            .id(format!("settings-model-option-{row_model}"))
                            .interaction_surface(
                                if selected {
                                    colors.bg_active
                                } else {
                                    colors.bg_elevated
                                },
                                if selected {
                                    colors.bg_active
                                } else {
                                    colors.bg_hover
                                },
                                colors.bg_active,
                                true,
                            )
                            .focusable()
                            .tab_stop(true)
                            .border_1()
                            .border_color(gpui_kit::transparent_black())
                            .focus_visible(move |style| style.border_color(colors.accent))
                            .on_key_down(cx.listener(
                                move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.select_model(&keyboard_model, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
                            .py_1()
                            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                            .text_size(px(Typography::BODY))
                            .cursor_pointer()
                            .when(selected, move |row| {
                                row.bg(colors.bg_active).text_color(colors.brand_primary)
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if cx
            .try_global::<PricingSettingsRequested>()
            .is_some_and(|request| request.0)
        {
            self.request_settings_section(3, cx);
            cx.set_global(PricingSettingsRequested(false));
        }
        self.resume_settings_navigation(window, cx);
        self.ensure_model_catalog(cx);
        let current_theme = theme(cx);
        self.config.ui.theme = if current_theme.follow_system {
            "system"
        } else {
            match current_theme.appearance {
                vega_theme::Appearance::Light => "light",
                vega_theme::Appearance::Dark => "dark",
            }
        }
        .into();
        if let Some(sidebar) = cx.try_global::<crate::sidebar::SidebarCollapsed>() {
            self.config.ui.sidebar_collapsed = sidebar.0;
        }
        self.config.ui.sidebar_width = crate::sidebar::width(cx);
        if let Some(projects) = cx.try_global::<crate::sidebar::ProjectsCollapsed>() {
            self.config.ui.projects_collapsed = projects.0;
        }
        if let Some(sessions) = cx.try_global::<crate::sidebar::SessionsCollapsed>() {
            self.config.ui.sessions_collapsed = sessions.0;
        }
        let colors = theme(cx).colors;
        let (page_title, page_selector) = match self.section {
            0 => ("服务提供方", "settings-page-providers"),
            1 => ("通用", "settings-page-general"),
            2 => ("服务提供方", "settings-page-reasoning"),
            3 => ("服务提供方", "settings-page-pricing"),
            4 => ("用量", "settings-page-usage"),
            5 => ("MCP", "settings-page-mcp"),
            6 => ("Skills", "settings-page-skills"),
            7 => ("Agents", "settings-page-agents"),
            _ => ("用量", "settings-page-usage"),
        };
        let navigation = [
            (1, "通用", "settings-nav-general"),
            (0, "服务提供方", "settings-nav-providers"),
            (5, "MCP", "settings-nav-mcp"),
            (7, "Agents", "settings-nav-agents"),
            (6, "Skills", "settings-nav-skills"),
            (4, "用量", "settings-nav-usage"),
        ];
        div()
            .id("settings-page")
            .on_action(cx.listener(Self::guard_settings_close))
            .on_key_down(|event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            })
            .key_context(if self.section == 5 {
                "McpSettings"
            } else if self.section == 6 {
                "SkillsSettings"
            } else {
                "PricingSettings"
            })
            .on_action(cx.listener(Self::activate_provider_action))
            .on_action(cx.listener(Self::next_provider_action))
            .on_action(cx.listener(Self::previous_provider_action))
            .on_action(cx.listener(Self::activate_pricing_action))
            .on_action(cx.listener(Self::next_pricing_action))
            .on_action(cx.listener(Self::previous_pricing_action))
            .on_action(cx.listener(Self::next_skills_action))
            .on_action(cx.listener(Self::previous_skills_action))
            .on_action(cx.listener(Self::next_mcp_action))
            .on_action(cx.listener(Self::previous_mcp_action))
            .size_full()
            .flex()
            .flex_row()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .line_height(relative(Typography::BODY_LINE_HEIGHT))
            .child(
                div()
                    .id("settings-navigation")
                    .debug_selector(|| "settings-navigation".into())
                    .w(px(Layout::SETTINGS_NAV_WIDTH))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .pt(px(Layout::MAIN_HEADER_HEIGHT))
                    .px(px(Layout::SIDEBAR_PADDING))
                    .border_r_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_sidebar)
                    .child(
                        div()
                            .id("settings-back")
                            .interactive(colors, true, false)
                            .debug_selector(|| "settings-back".into())
                            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                            .border_1()
                            .border_color(gpui_kit::transparent_black())
                            .focus_visible(move |style| style.border_color(colors.accent))
                            .focusable()
                            .tab_stop(true)
                            .cursor_pointer()
                            .on_key_down(cx.listener(
                                |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.on_back(
                                            &MouseUpEvent {
                                                button: MouseButton::Left,
                                                position: gpui_kit::point(px(0.), px(0.)),
                                                modifiers: event.keystroke.modifiers,
                                                click_count: 1,
                                            },
                                            window,
                                            cx,
                                        );
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_back))
                            .child(crate::icons::icon(
                                crate::icons::Icon::ArrowLeft,
                                colors.text_primary,
                            ))
                            .child("返回应用"),
                    )
                    .child(div().h(px(24.)).flex_shrink_0())
                    .children(navigation.into_iter().map(|(index, label, selector)| {
                        div()
                            .id(("settings-section", index))
                            .interactive(colors, true, self.navigation_section() == index)
                            .debug_selector(move || selector.into())
                            .track_focus(&self.section_focuses[index])
                            .tab_stop(true)
                            .focus_visible(move |style| style.border_color(colors.accent))
                            .on_key_down(cx.listener(
                                move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.cancel_provider_operation(cx);
                                        if this.section == 5
                                            && index != 5
                                            && (this.mcp.oauth_discovery.is_some()
                                                || this.mcp.oauth_flow_id.is_some())
                                        {
                                            this.cancel_mcp_oauth(cx);
                                        }
                                        this.request_settings_section(index, cx);
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            ))
                            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                            .border_1()
                            .border_color(gpui_kit::transparent_black())
                            .when(self.navigation_section() == index, |row| {
                                row.bg(colors.bg_active)
                            })
                            .cursor_pointer()
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.cancel_provider_operation(cx);
                                    if this.section == 5
                                        && index != 5
                                        && (this.mcp.oauth_discovery.is_some()
                                            || this.mcp.oauth_flow_id.is_some())
                                    {
                                        this.cancel_mcp_oauth(cx);
                                    }
                                    this.request_settings_section(index, cx);
                                    cx.notify();
                                }),
                            )
                            .gap_2()
                            .child(crate::icons::icon(
                                match index {
                                    0 => crate::icons::Icon::Connection,
                                    1 => crate::icons::Icon::Settings,
                                    2 => crate::icons::Icon::TextSelect,
                                    3 => crate::icons::Icon::ArrowUpDown,
                                    4 => crate::icons::Icon::Chart,
                                    5 => crate::icons::Icon::Plug,
                                    6 => crate::icons::Icon::Blocks,
                                    _ => crate::icons::Icon::Bot,
                                },
                                colors.text_secondary,
                            ))
                            .child(label)
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(colors.bg_base)
                    .px(px(24.))
                    .pt(px(Layout::SETTINGS_SECTION_GAP))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("settings-content-column")
                            .debug_selector(|| "settings-content-column".into())
                            .w_full()
                            .max_w(px(Layout::SETTINGS_CONTENT_MAX_WIDTH))
                            .h_full()
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap(px(Layout::SETTINGS_TITLE_GAP))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .debug_selector(move || page_selector.into())
                                    .text_size(px(Typography::SETTINGS_TITLE))
                                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                                    .child(page_title),
                            )
                            .children(
                                self.error
                                    .clone()
                                    .map(|message| settings_status(message, colors.danger, colors)),
                            )
                            .children(
                                self.pending_section
                                    .map(|_| self.render_settings_navigation_guard(cx)),
                            )
                            .when(
                                (self.section == 0 && self.model_settings.is_some())
                                    || matches!(self.section, 2 | 3),
                                |column| column.child(self.render_model_navigation(cx)),
                            )
                            .child(
                                div()
                                    .id(("settings-section-content", self.section))
                                    .debug_selector(|| "settings-section-content".into())
                                    .when_some(
                                        self.section_scrolls.get(self.section),
                                        |body, scroll| body.track_scroll(scroll),
                                    )
                                    .flex_1()
                                    .min_h_0()
                                    .min_w_0()
                                    .when(self.section != 0, |body| body.overflow_y_scroll())
                                    .when(self.section == 0, |body| body.overflow_hidden())
                                    .flex()
                                    .flex_col()
                                    .gap(px(Layout::SETTINGS_SECTION_GAP))
                                    .when(self.section == 0, |body| {
                                        body.child(self.render_provider_management(window, cx))
                                    })
                                    .when(self.section == 1, |body| {
                                        body.child(self.render_defaults(cx))
                                    })
                                    .when(self.section == 2, |body| {
                                        body.child(self.render_reasoning(cx))
                                    })
                                    .when(self.section == 3, |body| {
                                        body.child(self.render_pricing(cx))
                                    })
                                    .when(self.section == 4, |body| {
                                        body.child(self.render_usage(cx))
                                    })
                                    .when(self.section == 5, |body| body.child(self.render_mcp(cx)))
                                    .when(self.section == 6, |body| {
                                        body.child(self.render_skills(cx))
                                    })
                                    .when(self.section == 7, |body| {
                                        body.child(self.render_agents(cx))
                                    }),
                            ),
                    ),
            )
    }
}
