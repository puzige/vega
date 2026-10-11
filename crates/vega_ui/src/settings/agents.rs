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
        settings_section()
            .child(
                div()
                    .text_size(px(Typography::BODY))
                    .text_color(colors.text_secondary)
                    .child(profile_status),
            )
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .debug_selector(|| "settings-agent-name-label".into())
                            .font_weight(Typography::HEADING_CARD_WEIGHT)
                            .child("名称"),
                    )
                    .child(
                        div()
                            .debug_selector(|| "settings-agent-name-input".into())
                            .child(self.codex_name_input.clone()),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .debug_selector(|| "settings-agent-executable-label".into())
                            .font_weight(Typography::HEADING_CARD_WEIGHT)
                            .child("可执行文件"),
                    )
                    .child(
                        div()
                            .debug_selector(|| "settings-agent-executable-input".into())
                            .child(self.codex_executable_input.clone()),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .debug_selector(|| "settings-agent-args-label".into())
                            .font_weight(Typography::HEADING_CARD_WEIGHT)
                            .child("参数（每行一个 argv）"),
                    )
                    .child(
                        div()
                            .debug_selector(|| "settings-agent-args-input".into())
                            .child(self.codex_args_input.clone()),
                    ),
            )
            .child(
                div().flex().justify_end().child(
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
                                        if matches!(event.keystroke.key.as_str(), "enter" | "space")
                                        {
                                            this.save_codex_profile(cx);
                                            cx.stop_propagation();
                                        }
                                    },
                                ))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.save_codex_profile(cx)),
                                )
                        })
                        .child(if self.codex_profile_saving {
                            "保存中…"
                        } else {
                            "保存 Codex profile"
                        }),
                ),
            )
            .into_any_element()
    }
}
