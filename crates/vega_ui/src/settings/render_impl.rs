use super::*;
use crate::settings::state::{
    PROVIDER_MODELS_FRAME_INSET, PROVIDER_MODELS_MAX_ROWS, PROVIDER_MODELS_MIN_ROWS,
};

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
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .flex_1()
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
        div()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .rounded(px(Layout::PANEL_RADIUS))
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
            (colors.accent, colors.bg_base)
        } else {
            (colors.bg_hover, colors.text_tertiary)
        };

        div()
            .key_context("ProviderSettings")
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("添加或更新 Provider", colors.text_primary))
            .child(self.name_input.clone())
            .child(self.base_url_input.clone())
            .child(
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
                    .child(self.models_input.clone()),
            )
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
            .child(self.key_input.clone())
            .child(
                div()
                    .text_color(colors.text_tertiary)
                    .text_size(px(Typography::BODY))
                    .child("更新已有 Provider 时可留空，以保留现有密钥"),
            )
            .child(
                div()
                    .key_context("ProviderAction")
                    .track_focus(&self.provider_save_focus)
                    .tab_stop(true)
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
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("外观与布局", colors.text_primary))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(field_label("主题", colors.text_secondary))
                    .children(
                        [("light", "浅色"), ("dark", "深色"), ("system", "跟随系统")]
                            .into_iter()
                            .map(|(value, label)| {
                                div()
                                    .id(label)
                                    .focusable()
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(if self.config.ui.theme == value {
                                        colors.bg_active
                                    } else {
                                        colors.bg_elevated
                                    })
                                    .hover(move |style| style.bg(colors.bg_hover))
                                    .child(label)
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.set_appearance(value, cx)
                                        }),
                                    )
                                    .on_key_down(cx.listener(
                                        move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                this.set_appearance(value, cx);
                                                cx.stop_propagation();
                                            }
                                        },
                                    ))
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(field_label("侧栏", colors.text_primary))
                            .child(
                                div()
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.text_secondary)
                                    .child("在主窗口中显示侧栏"),
                            ),
                    )
                    .child(
                        div()
                            .id("settings-sidebar-visibility")
                            .debug_selector(|| "settings-sidebar-switch".into())
                            .aria_label(if sidebar_visible {
                                "隐藏侧栏"
                            } else {
                                "显示侧栏"
                            })
                            .focusable()
                            .tab_stop(true)
                            .w(px(Layout::SETTINGS_SWITCH_WIDTH))
                            .h(px(Layout::SETTINGS_SWITCH_HEIGHT))
                            .p(px(2.))
                            .rounded_full()
                            .border_1()
                            .border_color(if sidebar_visible {
                                colors.brand_primary
                            } else {
                                colors.border_subtle
                            })
                            .bg(if sidebar_visible {
                                colors.brand_primary
                            } else {
                                colors.bg_hover
                            })
                            .flex()
                            .items_center()
                            .when(sidebar_visible, |switch| switch.justify_end())
                            .when(!sidebar_visible, |switch| switch.justify_start())
                            .cursor_pointer()
                            .focus_visible(move |switch| {
                                switch.border_2().border_color(colors.brand_primary_strong)
                            })
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|_, _, window, cx| {
                                    window.dispatch_action(
                                        Box::new(crate::sidebar::ToggleSidebar),
                                        cx,
                                    )
                                }),
                            )
                            .on_key_down(cx.listener(
                                |_, event: &gpui_kit::KeyDownEvent, window, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        window.dispatch_action(
                                            Box::new(crate::sidebar::ToggleSidebar),
                                            cx,
                                        );
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .child(div().size(px(16.)).rounded_full().bg(colors.bg_elevated)),
                    ),
            )
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
            .child(self.render_updater(cx))
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
                    .bg(colors.bg_hover)
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
                        .debug_selector(move || format!("settings-permission-option-{mode}"))
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_size(px(Typography::BODY))
                        .cursor_pointer()
                        .when(selected, move |row| {
                            row.bg(colors.bg_active).text_color(colors.brand_primary)
                        })
                        .when(!selected, move |row| {
                            row.hover(move |s| s.bg(colors.bg_hover))
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
                    .bg(colors.bg_hover)
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
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(Typography::BODY))
                            .cursor_pointer()
                            .when(selected, move |row| {
                                row.bg(colors.bg_active).text_color(colors.brand_primary)
                            })
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if cx
            .try_global::<PricingSettingsRequested>()
            .is_some_and(|request| request.0)
        {
            self.section = 3;
            cx.set_global(PricingSettingsRequested(false));
        }
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
            0 => ("Providers", "settings-page-providers"),
            1 => ("General", "settings-page-general"),
            2 => ("Reasoning", "settings-page-reasoning"),
            3 => ("Pricing", "settings-page-pricing"),
            4 => ("Usage", "settings-page-usage"),
            5 => ("MCP", "settings-page-mcp"),
            6 => ("Skills", "settings-page-skills"),
            _ => ("Usage", "settings-page-usage"),
        };
        let navigation = [
            (1, "General", "settings-nav-general"),
            (0, "Providers", "settings-nav-providers"),
            (2, "Reasoning", "settings-nav-reasoning"),
            (3, "Pricing", "settings-nav-pricing"),
            (4, "Usage", "settings-nav-usage"),
            (5, "MCP", "settings-nav-mcp"),
            (6, "Skills", "settings-nav-skills"),
        ];
        div()
            .id("settings-page")
            .key_context(if self.section == 6 {
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
                    .w(px(crate::sidebar::width(cx)))
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
                            .debug_selector(|| "settings-back".into())
                            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .rounded_md()
                            .focusable()
                            .tab_stop(true)
                            .cursor_pointer()
                            .hover(move |s| s.bg(colors.bg_hover))
                            .on_key_down(cx.listener(
                                |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.cancel_provider_operation(cx);
                                        this.cancel_mcp_oauth(cx);
                                        this.close_skills_session();
                                        window.dispatch_action(Box::new(CloseSettings), cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_back))
                            .child(crate::icons::icon(
                                crate::icons::Icon::ArrowLeft,
                                colors.text_primary,
                            ))
                            .child("Back to app"),
                    )
                    .child(div().h(px(24.)).flex_shrink_0())
                    .children(navigation.into_iter().map(|(index, label, selector)| {
                        div()
                            .id(("settings-section", index))
                            .debug_selector(move || selector.into())
                            .track_focus(&self.section_focuses[index])
                            .tab_stop(true)
                            .focus_visible(move |style| {
                                style.border_1().border_color(colors.brand_primary)
                            })
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
                                        this.skills_section_changed(index, cx);
                                        this.section = index;
                                        cx.stop_propagation();
                                        cx.notify();
                                    }
                                },
                            ))
                            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                            .px_2()
                            .flex()
                            .items_center()
                            .rounded_md()
                            .when(self.section == index, |row| row.bg(colors.bg_hover))
                            .cursor_pointer()
                            .hover(move |row| row.bg(colors.bg_hover))
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
                                    this.skills_section_changed(index, cx);
                                    this.section = index;
                                    cx.notify();
                                }),
                            )
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
                    .pt(px(if window.viewport_size().height > px(700.) {
                        68.
                    } else {
                        48.
                    }))
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
                            .gap_4()
                            .child(
                                div()
                                    .debug_selector(move || page_selector.into())
                                    .text_size(px(Typography::SETTINGS_TITLE))
                                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                                    .child(page_title),
                            )
                            .children(
                                self.error
                                    .clone()
                                    .map(|message| div().text_color(colors.danger).child(message)),
                            )
                            .child(
                                div()
                                    .id("settings-section-content")
                                    .debug_selector(|| "settings-section-content".into())
                                    .flex_1()
                                    .min_h_0()
                                    .min_w_0()
                                    .when(self.section != 0, |body| body.overflow_y_scroll())
                                    .when(self.section == 0, |body| body.overflow_hidden())
                                    .flex()
                                    .flex_col()
                                    .gap_4()
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
                                    }),
                            ),
                    ),
            )
    }
}
