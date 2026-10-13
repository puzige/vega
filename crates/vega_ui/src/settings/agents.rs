use super::*;
use crate::interaction::InteractionExt;

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
            self.error = Some("Codex profile 无效；请检查绝对路径和安全参数".into());
            cx.notify();
            return;
        }
        let Some(path) = self.config_path.clone() else {
            self.error = Some("配置路径不可用".into());
            cx.notify();
            return;
        };
        let base = self.config.agent.codex_acp_profile.clone();
        self.codex_profile_saving = true;
        let worker = cx.background_executor().spawn(async move {
            let mut edit = config::begin_edit(&path).map_err(|_| "配置读取失败")?;
            if edit.config.agent.codex_acp_profile != base {
                return Err("Codex profile 已在其他窗口更改，请重新读取");
            }
            edit.config.agent.codex_acp_profile = Some(profile);
            edit.save().map_err(|_| "配置保存失败")?;
            Ok(edit.config.clone())
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.codex_profile_saving = false;
                match result {
                    Ok(config) => {
                        this.config = config;
                        this.error = None;
                        cx.emit(SettingsSaved);
                    }
                    Err(error) => this.error = Some(error.into()),
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
            "已配置的 profile 只保存名称、绝对 executable 和 argv。Vega 不读取或复制 Codex 登录凭据。"
        } else {
            "配置一个本地 Codex ACP executable 后，New Task 才能选择 Codex。"
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
                        "settings-agent-name",
                        "名称",
                        None,
                        div()
                            .debug_selector(|| "settings-agent-name-input".into())
                            .child(self.codex_name_input.clone()),
                        colors,
                    ))
                    .child(settings_field(
                        "settings-agent-executable",
                        "可执行文件",
                        None,
                        div()
                            .debug_selector(|| "settings-agent-executable-input".into())
                            .child(self.codex_executable_input.clone()),
                        colors,
                    ))
                    .child(settings_field(
                        "settings-agent-args",
                        "参数",
                        Some("每行一个 argv".into()),
                        div()
                            .debug_selector(|| "settings-agent-args-input".into())
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
                                .debug_selector(|| "settings-codex-profile-save".into())
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
                                    "保存中…"
                                } else {
                                    "保存 Codex profile"
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
    Accessibility,
    ScreenRecording,
    Extension,
}

impl SettingsView {
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
            NativeAgentAction::Check => {
                if self.computer_use_check_pending {
                    return;
                }
                self.computer_use_check_pending = true;
                let worker = cx.background_executor().spawn(async {
                    vega_conversation::types::ManagedComponents::detect()
                        .map(|_| "随包组件已就绪。系统权限和浏览器连接将在使用时检查。".to_string())
                        .unwrap_or_else(|error| error.to_string())
                });
                cx.spawn(async move |this, cx| {
                    let result = worker.await;
                    let _ = this.update(cx, |this, cx| {
                        this.computer_use_check = Some(result);
                        this.computer_use_check_pending = false;
                        cx.notify();
                    });
                })
                .detach();
            }
            NativeAgentAction::Accessibility => cx.open_url(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            ),
            NativeAgentAction::ScreenRecording => cx.open_url(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
            ),
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
            .debug_selector(move || id.into())
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
        settings_group(colors)
            .child(section_title("Native · 电脑与浏览器", colors.text_primary))
            .child(div().mt_2().text_size(px(Typography::METADATA)).text_color(colors.text_secondary)
                .child("开启后用于新运行。电脑操控可发现并切换所有可访问窗口；浏览器仍需选择并授权标签页。可随时接管或停止。需要 macOS 14 或更新版本。"))
            .child(settings_field("settings-desktop-use", "电脑操控", Some("所有可访问桌面窗口，后台输入受系统能力限制".into()),
                self.native_agent_button("settings-desktop-use-toggle", if self.config.agent.desktop_use_enabled { "已开启" } else { "已关闭" }, NativeAgentAction::Desktop, self.provider_management.saving, cx), colors))
            .child(settings_field("settings-browser-use", "浏览器操控", Some("使用已登录的 Chrome 标签页".into()),
                self.native_agent_button("settings-browser-use-toggle", if self.config.agent.browser_use_enabled { "已开启" } else { "已关闭" }, NativeAgentAction::Browser, self.provider_management.saving, cx), colors))
            .child(div().mt_3().flex().flex_wrap().gap_2()
                .child(self.native_agent_button("settings-computer-use-check", if self.computer_use_check_pending { "检测中…" } else { "检测随包组件" }, NativeAgentAction::Check, self.computer_use_check_pending, cx))
                .child(self.native_agent_button("settings-computer-use-accessibility", "辅助功能设置", NativeAgentAction::Accessibility, false, cx))
                .child(self.native_agent_button("settings-computer-use-screen-recording", "屏幕录制设置", NativeAgentAction::ScreenRecording, false, cx))
                .child(self.native_agent_button("settings-computer-use-extension", "安装浏览器扩展", NativeAgentAction::Extension, false, cx)))
            .child(div().mt_2().text_size(px(Typography::METADATA)).text_color(colors.text_secondary)
                .child(self.computer_use_check.clone().unwrap_or_else(|| "组件随 Vega 提供。查看窗口和截图需要屏幕录制权限；读取控件和输入还需要辅助功能权限。浏览器扩展需由你安装并选择标签页。".into())))
            .into_any_element()
    }
}
