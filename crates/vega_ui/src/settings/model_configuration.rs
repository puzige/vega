use super::*;
use crate::interaction::InteractionExt;

#[derive(Clone, Copy)]
pub(crate) enum SettingsDestination {
    Section(usize),
    ModelSection(usize),
    ProviderCommand,
}

impl SettingsView {
    pub(crate) fn guard_settings_close(
        &mut self,
        _: &CloseSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_help_open.take().is_some() {
            window.focus(&self.settings_help_focus, cx);
            cx.stop_propagation();
            cx.notify();
        } else if self.settings_have_draft(cx) {
            self.pending_settings_close = true;
            self.pending_section = Some(SettingsDestination::ModelSection(self.section));
            cx.stop_propagation();
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn navigation_section(&self) -> usize {
        if matches!(self.section, 2 | 3) {
            0
        } else {
            self.section
        }
    }

    pub(crate) fn request_settings_section(&mut self, destination: usize, cx: &mut Context<Self>) {
        self.request_settings_destination(SettingsDestination::Section(destination), cx);
    }

    fn request_settings_destination(
        &mut self,
        destination: SettingsDestination,
        cx: &mut Context<Self>,
    ) {
        if self.settings_have_draft(cx) {
            self.pending_section = Some(destination);
            cx.notify();
            return;
        }
        self.finish_settings_navigation(destination, cx);
    }

    fn finish_settings_navigation(
        &mut self,
        destination: SettingsDestination,
        cx: &mut Context<Self>,
    ) {
        let section = match destination {
            SettingsDestination::ProviderCommand => {
                self.pending_section = None;
                self.finish_provider_navigation(cx);
                return;
            }
            SettingsDestination::Section(section) => {
                self.close_model_configuration(cx);
                section
            }
            SettingsDestination::ModelSection(section) => section,
        };
        self.skills_section_changed(section, cx);
        self.section = section;
        self.pending_section = None;
        self.settings_help_open = None;
        cx.notify();
    }

    pub(crate) fn resume_settings_navigation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(destination) = self.pending_section
            && self.error.is_none()
            && !self.settings_have_draft(cx)
            && !matches!(self.pricing, PricingSettingsProjection::Saving { .. })
            && !matches!(self.reasoning, ReasoningSettingsProjection::Saving { .. })
        {
            self.finish_settings_navigation(destination, cx);
            if self.pending_settings_close {
                self.pending_settings_close = false;
                window.dispatch_action(Box::new(CloseSettings), cx);
            }
        }
    }

    pub(crate) fn settings_have_draft(&self, cx: &App) -> bool {
        self.provider_has_draft(cx)
            || self.pricing_editor.is_some()
            || matches!(
                self.pricing,
                PricingSettingsProjection::Ready {
                    draft_reason: Some(_),
                    ..
                }
            )
            || self.reasoning_draft.is_some()
            || self.model_capability_draft.is_some()
    }

    pub(crate) fn render_settings_navigation_guard(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let saving = self.provider_is_saving()
            || matches!(self.reasoning, ReasoningSettingsProjection::Saving { .. })
            || matches!(self.pricing, PricingSettingsProjection::Saving { .. })
            || matches!(
                self.model_capabilities,
                ModelCapabilitySettingsProjection::Saving { .. }
            );
        settings_section()
            .child(settings_status("当前修改尚未保存", colors.warning, colors))
            .child(
                settings_actions()
                    .child(self.settings_route_button(
                        "settings-save-before-leave",
                        "保存后继续",
                        !saving,
                        cx.listener(|this, _, _, cx| {
                            if this.section == 3 {
                                if let PricingSettingsProjection::Ready {
                                    generation,
                                    draft_reason: Some(_),
                                    ..
                                } = this.pricing
                                {
                                    cx.emit(PricingRetryRequested { generation });
                                } else {
                                    this.submit_pricing_editor(cx);
                                }
                            } else if this.model_capability_draft.is_some() {
                                if let Some(index) =
                                    this.model_capability_models.iter().position(|model| {
                                        this.model_capability_draft.as_ref().is_some_and(|draft| {
                                            draft.identity == model.resolved.identity
                                        })
                                    })
                                {
                                    this.save_model_capability(index, false, cx);
                                }
                            } else if this.section == 2 {
                                if let (
                                    Some(draft),
                                    ReasoningSettingsProjection::Ready { profiles, .. },
                                ) = (this.reasoning_draft.clone(), &this.reasoning)
                                    && let Some(index) = profiles.iter().position(|profile| {
                                        profile.provider == draft.provider
                                            && profile.model == draft.model
                                    })
                                {
                                    this.emit_reasoning_save(index, draft, cx);
                                }
                            } else {
                                this.save_provider_draft(cx);
                            }
                        }),
                        cx,
                    ))
                    .child(self.settings_route_button(
                        "settings-discard-before-leave",
                        "放弃修改",
                        !saving,
                        cx.listener(|this, _, window, cx| {
                            if let PricingSettingsProjection::Ready {
                                generation,
                                draft_reason: Some(_),
                                ..
                            } = this.pricing
                            {
                                cx.emit(PricingDiscardRequested { generation });
                            }
                            this.pricing_editor = None;
                            this.reasoning_draft = None;
                            this.model_capability_draft = None;
                            if let Some(destination) = this.pending_section {
                                this.discard_provider_draft(cx);
                                this.finish_settings_navigation(destination, cx);
                                if this.pending_settings_close {
                                    this.pending_settings_close = false;
                                    window.dispatch_action(Box::new(CloseSettings), cx);
                                }
                            }
                        }),
                        cx,
                    ))
                    .child(self.settings_route_button(
                        "settings-stay",
                        "留在本页",
                        true,
                        cx.listener(|this, _, _, cx| {
                            this.pending_section = None;
                            this.pending_settings_close = false;
                            cx.notify();
                        }),
                        cx,
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn settings_route_button(
        &self,
        id: &'static str,
        label: &'static str,
        enabled: bool,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let listener = std::rc::Rc::new(listener);
        let keyboard = listener.clone();
        settings_action_chrome(colors)
            .id(id)
            .interactive(colors, enabled, false)
            .when(enabled, |button| {
                button
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .on_mouse_up(MouseButton::Left, move |event, window, cx| {
                        listener(event, window, cx)
                    })
                    .on_key_down(move |event, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            keyboard(
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
                    })
            })
            .focus_visible(move |style| style.border_color(colors.accent))
            .when(!enabled, |button| button.text_color(colors.text_tertiary))
            .child(label)
            .into_any_element()
    }

    pub(crate) fn settings_help(
        &self,
        id: &'static str,
        label: &'static str,
        text: impl Into<gpui_kit::SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let text = text.into();
        let tooltip = text.clone();
        let open = self.settings_help_open.as_deref() == Some(id);
        div()
            .relative()
            .self_start()
            .child(
                div()
                    .id(id)
                    .aria_label(label)
                    .track_focus(&self.settings_help_focus)
                    .tab_stop(true)
                    .size(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .interactive(colors, true, open)
                    .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                    .cursor_pointer()
                    .tooltip(move |_, cx| crate::icons::tooltip(tooltip.clone(), cx))
                    .focus_visible(move |style| style.bg(colors.bg_active))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.settings_help_open = if open { None } else { Some(id.into()) };
                            window.focus(&this.settings_help_focus, cx);
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(
                        move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space" | "escape")
                            {
                                this.settings_help_open = if open || event.keystroke.key == "escape"
                                {
                                    None
                                } else {
                                    Some(id.into())
                                };
                                window.focus(&this.settings_help_focus, cx);
                                cx.stop_propagation();
                                cx.notify();
                            }
                        },
                    ))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Help,
                        colors.text_secondary,
                    )),
            )
            .when(open, |host| {
                host.child(
                    gpui_kit::deferred(
                        div()
                            .id("settings-help-popup")
                            .absolute()
                            .top(relative(1.))
                            .right_0()
                            .w(px(Layout::SETTINGS_LABEL_WIDTH))
                            .p_3()
                            .rounded(px(Layout::MENU_RADIUS))
                            .border_1()
                            .border_color(colors.border_subtle)
                            .bg(colors.bg_elevated)
                            .text_color(colors.text_secondary)
                            .shadow_sm()
                            .occlude()
                            .on_mouse_down_out(cx.listener(
                                |this, _: &gpui_kit::MouseDownEvent, _, cx| {
                                    this.settings_help_open = None;
                                    cx.notify();
                                },
                            ))
                            .child(text),
                    )
                    .with_priority(2),
                )
            })
            .into_any_element()
    }

    pub(crate) fn render_model_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut navigation = settings_section();
        if let Some((provider, model)) = &self.model_settings {
            let available =
                self.config.providers.iter().any(|candidate| {
                    candidate.name == *provider && candidate.models.contains(model)
                });
            navigation = navigation
                .child(
                    settings_actions()
                        .child(self.settings_route_button(
                            "model-settings-back",
                            "返回提供方",
                            true,
                            cx.listener(|this, _, _, cx| this.request_settings_section(0, cx)),
                            cx,
                        ))
                        .child(
                            div()
                                .min_w_0()
                                .text_color(colors.text_secondary)
                                .child(format!("{provider} / {model}")),
                        ),
                )
                .child(
                    settings_actions().children(
                        [
                            (0, "基本信息", "model-settings-basic"),
                            (2, "思考", "model-settings-reasoning"),
                            (3, "定价", "model-settings-pricing"),
                        ]
                        .into_iter()
                        .map(|(section, label, id)| {
                            let active = self.section == section;
                            div()
                                .child(self.settings_route_button(
                                    id,
                                    label,
                                    !active && (section != 0 || available),
                                    cx.listener(move |this, _, _, cx| {
                                        this.request_settings_destination(
                                            SettingsDestination::ModelSection(section),
                                            cx,
                                        )
                                    }),
                                    cx,
                                ))
                                .when(active, |view| view.border_b_2().border_color(colors.accent))
                        }),
                    ),
                );
        } else if self.section == 3 {
            navigation = navigation.child(
                settings_actions()
                    .child(self.settings_route_button(
                        "pricing-library-back",
                        "返回提供方",
                        true,
                        cx.listener(|this, _, _, cx| this.request_settings_section(0, cx)),
                        cx,
                    ))
                    .child("模型价格库 · 所有提供方共享"),
            );
        } else if self.section == 2 {
            navigation = navigation.child(
                settings_actions()
                    .child(self.settings_route_button(
                        "reasoning-models-back",
                        "返回提供方",
                        true,
                        cx.listener(|this, _, _, cx| this.request_settings_section(0, cx)),
                        cx,
                    ))
                    .child("选择模型配置思考能力"),
            );
        }
        navigation.into_any_element()
    }
}
