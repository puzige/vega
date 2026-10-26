use super::*;
use crate::cua_onboarding::PermissionPane;
use crate::cua_onboarding::controller::CheckState;
use crate::interaction::InteractionExt;
use vega_conversation::types::{ComponentState, PermissionState};

impl SettingsView {
    pub(crate) fn save_codex_profile(&mut self, cx: &mut Context<Self>) {
        if self.codex_profile_saving {
            return;
        }
        let name = self.codex_name_input.read(cx).text().trim().to_string();
        let executable = self
            .codex_executable_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let args = self
            .codex_args_input
            .read(cx)
            .text()
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let profile = vega_store::config::CodexAcpProfileConfig {
            display_name: name,
            executable,
            args,
        };
        if profile.validate().is_err()
            || vega_conversation::types::CodexAdapterArgument::from_argv(&profile.args).is_err()
        {
            self.error = Some(SettingsText::key("settings.agents.codex.invalid"));
            cx.notify();
            return;
        }
        let Some(path) = self.config_path.clone() else {
            self.error = Some(SettingsText::key("settings.config.path_unavailable"));
            cx.notify();
            return;
        };
        let base = self.config.agent.codex_acp_profile.clone();
        self.codex_profile_saving = true;
        let worker = cx.background_executor().spawn(async move {
            let mut edit = config::begin_edit(&path)
                .map_err(|_| SettingsText::key("settings.config.read_failed"))?;
            if edit.config.agent.codex_acp_profile != base {
                return Err(SettingsText::key("settings.agents.codex.conflict"));
            }
            edit.config.agent.codex_acp_profile = Some(profile);
            edit.save()
                .map_err(|_| SettingsText::key("settings.config.save_failed"))?;
            Ok(edit.config.clone())
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.codex_profile_saving = false;
                match result {
                    Ok(config) => {
                        let replay_identity_changed = this.config.agent != config.agent;
                        this.config = config;
                        this.error = None;
                        cx.emit(SettingsSaved {
                            replay_identity_changed,
                        });
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn render_agents(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let profile_status = if self.config.agent.codex_acp_profile.is_some() {
            t("settings.agents.codex.configured")
        } else {
            t("settings.agents.codex.setup")
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.render_native_agent(cx))
            .child(
                settings_group(colors)
                    .child(section_title("Codex", colors.text_primary))
                    .child(
                        div()
                            .mt_2()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .child(profile_status),
                    )
                    .child(settings_field(
                        t("settings.field.name"),
                        None,
                        div().child(self.codex_name_input.clone()),
                        colors,
                    ))
                    .child(settings_field(
                        t("settings.field.executable"),
                        None,
                        div().child(self.codex_executable_input.clone()),
                        colors,
                    ))
                    .child(settings_field(
                        t("settings.field.arguments"),
                        Some(t("settings.agents.codex.arguments_hint").into()),
                        div()
                            .bg(colors.bg_elevated)
                            .border_1()
                            .border_color(colors.border_subtle)
                            .rounded_lg()
                            .px_2()
                            .py_1()
                            .child(self.codex_args_input.clone()),
                        colors,
                    ))
                    .child(
                        div().mt_3().flex().child(
                            settings_action_chrome(colors)
                                .id("settings-codex-profile-save")
                                .interaction_surface(
                                    if self.codex_profile_saving {
                                        colors.bg_elevated
                                    } else {
                                        colors.brand_primary
                                    },
                                    colors.brand_primary_strong,
                                    colors.brand_primary_strong,
                                    !self.codex_profile_saving,
                                )
                                .focusable()
                                .tab_stop(true)
                                .focus_visible(move |style| style.border_color(colors.accent))
                                .bg(if self.codex_profile_saving {
                                    colors.bg_elevated
                                } else {
                                    colors.brand_primary
                                })
                                .text_color(if self.codex_profile_saving {
                                    colors.text_tertiary
                                } else {
                                    colors.brand_on_accent
                                })
                                .when(!self.codex_profile_saving, |button| {
                                    button
                                        .cursor_pointer()
                                        .on_key_down(cx.listener(
                                            |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                                if matches!(
                                                    event.keystroke.key.as_str(),
                                                    "enter" | "space"
                                                ) {
                                                    this.save_codex_profile(cx);
                                                    cx.stop_propagation();
                                                }
                                            },
                                        ))
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                this.save_codex_profile(cx)
                                            }),
                                        )
                                })
                                .child(if self.codex_profile_saving {
                                    t("settings.saving")
                                } else {
                                    t("settings.agents.codex.save")
                                }),
                        ),
                    ),
            )
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
enum NativeAgentAction {
    Desktop,
    Browser,
    Check,
    Prepare,
    Accessibility,
    ScreenRecording,
    Extension,
}

impl SettingsView {
    pub(crate) fn acquire_preparation(&mut self, cx: &mut Context<Self>) {
        if self.preparation_owner.is_none() {
            self.preparation_owner = Some(
                self.preparation
                    .update(cx, |model, cx| model.acquire(false, cx)),
            );
        }
    }

    fn native_agent_action(&mut self, action: NativeAgentAction, cx: &mut Context<Self>) {
        match action {
            NativeAgentAction::Desktop | NativeAgentAction::Browser => {
                let mut config = self.config.clone();
                match action {
                    NativeAgentAction::Desktop => {
                        config.agent.desktop_use_enabled = !config.agent.desktop_use_enabled
                    }
                    _ => config.agent.browser_use_enabled = !config.agent.browser_use_enabled,
                }
                self.save_preferences(config, cx);
            }
            NativeAgentAction::Prepare => {
                self.acquire_preparation(cx);
                cx.emit(DesktopPreparationRequested { pane: None });
            }
            NativeAgentAction::Accessibility => {
                self.acquire_preparation(cx);
                cx.emit(DesktopPreparationRequested { pane: Some(PermissionPane::Accessibility) });
            }
            NativeAgentAction::Check => {
                self.acquire_preparation(cx);
                self.preparation.update(cx, |model, cx| model.recheck(cx));
            }
            NativeAgentAction::ScreenRecording => {
                self.acquire_preparation(cx);
                cx.emit(DesktopPreparationRequested { pane: Some(PermissionPane::ScreenRecording) });
            }
            NativeAgentAction::Extension => {
                cx.open_url("https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/extension/README.md")
            }
        }
        cx.notify();
    }

    fn native_agent_button(
        &self,
        id: &'static str,
        label: &str,
        action: NativeAgentAction,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        settings_action_chrome(colors)
            .id(id)
            .role(gpui_kit::Role::Button)
            .aria_label(label.to_string())
            .interaction_surface(
                colors.bg_elevated,
                colors.bg_hover,
                colors.bg_hover,
                !disabled,
            )
            .text_color(if disabled {
                colors.text_tertiary
            } else {
                colors.text_primary
            })
            .when(!disabled, |button| {
                button
                    .focusable()
                    .tab_stop(true)
                    .focus_visible(move |style| style.border_color(colors.accent))
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.native_agent_action(action, cx)),
                    )
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.native_agent_action(action, cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
            })
            .child(label.to_string())
            .into_any_element()
    }

    fn render_native_agent(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let snapshot = self.preparation.read(cx).snapshot();
        let view = &snapshot.view;
        let permission = |state| match state {
            PermissionState::Granted => t("settings.state.on"),
            PermissionState::Denied => t("settings.state.denied"),
            PermissionState::Unknown
                if view.check_state == CheckState::Idle && !snapshot.retained =>
            {
                t("settings.state.unchecked")
            }
            PermissionState::Unknown => t("settings.state.unknown"),
        };
        let component = match view.component {
            Some(ComponentState::Available) => t("settings.state.available"),
            Some(ComponentState::Missing) => t("settings.state.not_installed"),
            Some(ComponentState::Incompatible) => t("settings.state.incompatible"),
            Some(ComponentState::UnsupportedPlatform) => t("settings.state.unsupported_platform"),
            None if view.check_state == CheckState::Idle && !snapshot.retained => {
                t("settings.state.unchecked")
            }
            None => t("settings.state.unknown"),
        };
        settings_group(colors)
            .child(section_title(
                t("settings.agents.native.title"),
                colors.text_primary,
            ))
            .child(
                div()
                    .mt_2()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(t("settings.agents.native.description")),
            )
            .child(settings_field(
                t("settings.agents.desktop.title"),
                Some(t("settings.agents.desktop.description").into()),
                self.native_agent_button(
                    "settings-desktop-use-toggle",
                    if self.config.agent.desktop_use_enabled {
                        t("settings.state.on")
                    } else {
                        t("settings.state.off")
                    },
                    NativeAgentAction::Desktop,
                    self.provider_management.saving,
                    cx,
                ),
                colors,
            ))
            .child(settings_field(
                t("settings.agents.browser.title"),
                Some(t("settings.agents.browser.description").into()),
                self.native_agent_button(
                    "settings-browser-use-toggle",
                    if self.config.agent.browser_use_enabled {
                        t("settings.state.on")
                    } else {
                        t("settings.state.off")
                    },
                    NativeAgentAction::Browser,
                    self.provider_management.saving,
                    cx,
                ),
                colors,
            ))
            .child(
                div()
                    .mt_3()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.native_agent_button(
                        "settings-desktop-use-prepare",
                        t("settings.agents.desktop.prepare"),
                        NativeAgentAction::Prepare,
                        false,
                        cx,
                    ))
                    .child(self.native_agent_button(
                        "settings-computer-use-check",
                        if view.can_recheck {
                            t("settings.action.recheck")
                        } else {
                            t("settings.state.checking")
                        },
                        NativeAgentAction::Check,
                        !view.can_recheck,
                        cx,
                    )),
            )
            .children(snapshot.feedback.iter().map(|feedback| {
                div()
                    .mt_2()
                    .text_size(px(Typography::METADATA))
                    .text_color(if feedback.warning {
                        colors.warning
                    } else {
                        colors.text_secondary
                    })
                    .child(feedback.message.clone())
            }))
            .child(settings_field(
                t("settings.agents.desktop.component"),
                None,
                div().child(component).into_any_element(),
                colors,
            ))
            .child(settings_field(
                t("settings.agents.accessibility"),
                None,
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(permission(view.accessibility))
                    .child(self.native_agent_button(
                        "settings-computer-use-accessibility",
                        t("settings.agents.accessibility.settings"),
                        NativeAgentAction::Accessibility,
                        false,
                        cx,
                    ))
                    .into_any_element(),
                colors,
            ))
            .child(settings_field(
                t("settings.agents.screen_recording"),
                None,
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(permission(view.screen_recording))
                    .child(self.native_agent_button(
                        "settings-computer-use-screen-recording",
                        t("settings.agents.screen_recording.settings"),
                        NativeAgentAction::ScreenRecording,
                        false,
                        cx,
                    ))
                    .into_any_element(),
                colors,
            ))
            .child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(t("settings.agents.screen_recording.restart")),
            )
            .child(settings_field(
                t("settings.agents.capture"),
                None,
                div()
                    .child(t("settings.state.unverified"))
                    .into_any_element(),
                colors,
            ))
            .child(div().mt_3().child(self.native_agent_button(
                "settings-computer-use-extension",
                t("settings.agents.browser.install_extension"),
                NativeAgentAction::Extension,
                false,
                cx,
            )))
            .into_any_element()
    }
}
