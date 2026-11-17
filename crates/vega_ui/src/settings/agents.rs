use super::*;
use crate::cua_onboarding::PermissionPane;

pub(super) mod nsh_agents;

impl SettingsView {
    pub(crate) fn save_codex_profile(&mut self, cx: &mut Context<Self>) {
        if self.codex_profile_saving || self.default_permission.io_pending() {
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
                this.release_agent_draft();
                match result {
                    Ok(config) => {
                        let replay_identity_changed = this.config.agent != config.agent;
                        this.apply_unrelated_config(config);
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
}
