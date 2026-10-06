use super::*;

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
            .gap_3()
            .child(
                div()
                    .text_size(px(Typography::BODY))
                    .text_color(colors.text_secondary)
                    .child(profile_status),
            )
            .child(self.codex_name_input.clone())
            .child(self.codex_executable_input.clone())
            .child(self.codex_args_input.clone())
            .child(
                div().flex().justify_end().child(
                    div()
                        .id("settings-codex-profile-save")
                        .debug_selector(|| "settings-codex-profile-save".into())
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(colors.accent)
                        .text_color(colors.bg_base)
                        .when(!self.codex_profile_saving, |button| {
                            button.cursor_pointer().on_mouse_up(
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
