use super::*;
use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};

impl SettingsView {
    pub(crate) fn on_back(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 与 Esc 同效：派发同一动作，由 app 级处理器统一收口。
        if self.guard_default_permission_close(window, cx) {
            return;
        }
        if self.settings_have_draft(cx) {
            self.invalidate_settings_navigation_intent();
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

    pub(crate) fn select_model(&mut self, model: &str, cx: &mut Context<Self>) {
        self.model_open = false;
        let mut candidate = self.config.clone();
        set_default_model(&mut candidate, model);
        self.save_preferences(candidate, cx);
        cx.notify();
    }

    pub(super) fn set_appearance(&mut self, value: &str, cx: &mut Context<Self>) {
        let mut candidate = self.config.clone();
        candidate.ui.theme = value.to_string();
        if let Some(sidebar) = cx.try_global::<crate::sidebar::SidebarCollapsed>() {
            candidate.ui.sidebar_collapsed = sidebar.0;
        }
        candidate.ui.sidebar_width = crate::sidebar::width(cx);
        self.save_preferences(candidate, cx);
    }

    pub(super) fn set_ui_language(&mut self, value: &str, cx: &mut Context<Self>) {
        if self.provider_management.saving || !matches!(value, "en" | "zh-CN") {
            return;
        }
        let mut candidate = self.config.clone();
        candidate.ui.language = value.to_string();
        self.save_preferences(candidate, cx);
    }

    pub(crate) fn render_defaults(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        settings_section()
            .gap(px(Layout::SETTINGS_SECTION_GAP))
            .child(self.render_appearance(window, cx))
            .child(
                settings_section()
                    .gap_2()
                    .child(section_title(
                        t("settings.defaults.title"),
                        colors.text_primary,
                        window,
                        cx,
                    ))
                    .child(
                        settings_group(colors, window, cx)
                            .child(settings_field(
                                "settings-defaults-permission-field",
                                t("settings.defaults.permission_mode"),
                                None,
                                self.render_permission_picker(window, cx),
                                colors,
                                window,
                                cx,
                            ))
                            .child(settings_field(
                                "settings-defaults-model-field",
                                t("settings.defaults.model"),
                                None,
                                self.render_default_model(window, cx),
                                colors,
                                window,
                                cx,
                            ))
                            .child(self.render_input_preferences(window, cx)),
                    ),
            )
            .child(self.render_updater(window, cx))
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.config.ui.language != vega_i18n::language() {
            self.config.ui.language = vega_i18n::language().to_string();
            self.refresh_localized_inputs(cx);
        }
        if cx
            .try_global::<PricingSettingsRequested>()
            .is_some_and(|request| request.0)
        {
            self.request_settings_section(3, cx);
            cx.set_global(PricingSettingsRequested(false));
        }
        if cx
            .try_global::<NativePreparationSettingsRequested>()
            .is_some_and(|request| request.0)
        {
            self.request_settings_section(7, cx);
            cx.set_global(NativePreparationSettingsRequested(false));
        }
        self.resume_settings_navigation(window, cx);
        self.prepare_provider_presentation(window);
        self.sync_provider_presentation(cx);
        self.sync_settings_navigation_presentations(cx);
        self.sync_default_model(cx);
        self.sync_usage_presentation(cx);
        self.sync_pricing_presentation(cx);
        self.sync_agents_presentation(cx);
        self.sync_skills_presentation(cx);
        self.sync_mcp_presentation(cx);
        self.sync_capability_presentations(cx);
        self.sync_permission_presentation(cx);
        self.prepare_default_permission_focus(window, cx);
        self.prepare_permission_reset_focus(window, cx);
        self.project_settings_motion(window, cx);
        self.ensure_model_catalog(cx);
        self.sync_catalog_presentation(cx);
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
        self.sync_shell_presentations(cx);
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
            .key_context(if self.section == 1 {
                "GeneralSettings"
            } else if self.section == 5 {
                "McpSettings"
            } else if self.section == 6 {
                "SkillsSettings"
            } else {
                "PricingSettings"
            })
            .on_action(cx.listener(Self::activate_pricing_action))
            .on_action(cx.listener(Self::next_pricing_action))
            .on_action(cx.listener(Self::previous_pricing_action))
            .size_full()
            .flex()
            .flex_row()
            .relative()
            .child(super::nsh_tail::background(
                "settings-page-background",
                colors.bg_base,
                window,
                cx,
            ))
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .line_height(relative(Typography::BODY_LINE_HEIGHT))
            .child(
                div()
                    .w(px(Layout::SETTINGS_NAV_WIDTH))
                    .h_full()
                    .flex_shrink_0()
                    .child(self.render_shell_navigation(window, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .relative()
                    .child(super::nsh_tail::background(
                        "settings-content-background",
                        colors.bg_base,
                        window,
                        cx,
                    ))
                    .px(px(24.))
                    .pt(px(Layout::SETTINGS_SECTION_GAP))
                    .pb(px(24.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("settings-content-column")
                            .w_full()
                            .max_w(px(Layout::SETTINGS_CONTENT_MAX_WIDTH))
                            .h_full()
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap(px(Layout::SETTINGS_SECTION_GAP))
                            .child(self.render_shell_heading(window, cx))
                            .children(
                                self.pending_section
                                    .map(|_| self.render_settings_navigation_guard(window, cx)),
                            )
                            .when(
                                (self.section == 0 && self.model_settings.is_some())
                                    || matches!(self.section, 2 | 3),
                                |column| column.child(self.render_model_navigation(window, cx)),
                            )
                            .child(
                                div()
                                    .id(("settings-section-content", self.section))
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
                                        body.child(self.render_defaults(window, cx))
                                    })
                                    .when(self.section == 2, |body| {
                                        body.child(self.render_reasoning(window, cx))
                                    })
                                    .when(self.section == 3, |body| {
                                        body.child(self.render_pricing(window, cx))
                                    })
                                    .when(self.section == 4, |body| {
                                        body.child(self.render_usage(window, cx))
                                    })
                                    .when(self.section == 5, |body| {
                                        body.child(self.render_mcp(window, cx))
                                    })
                                    .when(self.section == 6, |body| {
                                        body.child(self.render_skills(window, cx))
                                    })
                                    .when(self.section == 7, |body| {
                                        body.child(self.render_agents(window, cx))
                                    })
                                    .tracked_scroll_edge_fade(
                                        self.section_scrolls[self.section].clone(),
                                        colors.bg_base,
                                        ScrollFade::reading().enabled(self.section != 0),
                                    ),
                            ),
                    ),
            )
    }
}
