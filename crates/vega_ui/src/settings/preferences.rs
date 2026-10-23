//! Preference writes share provider serialization and preserve unrelated disk fields.
use super::*;
use crate::conversation_stream::RunningMessageDefault;
use vega_conversation::types::PendingMessageBehavior;

impl SettingsView {
    pub(crate) fn save_preferences(&mut self, candidate: AppConfig, cx: &mut Context<Self>) {
        if self.provider_management.saving {
            return;
        }
        let Some(path) = self.config_path.clone() else {
            return;
        };
        let base = self.config.clone();
        let theme_changed = base.ui.theme != candidate.ui.theme;
        let desktop_enable_requested =
            !base.agent.desktop_use_enabled && candidate.agent.desktop_use_enabled;
        let replay_identity_changed = base.providers != candidate.providers
            || base.agent != candidate.agent
            || base.defaults.permission_mode != candidate.defaults.permission_mode;
        self.provider_management.saving = true;
        let worker = cx.background_executor().spawn(async move {
            if base.providers != candidate.providers { return Err("供应商编辑必须使用专用表单"); }
            let mut edit = config::begin_edit(&path).map_err(|_| "配置读取失败")?;
            macro_rules! patch {
                ($($field:ident).+) => {
                    if base.$($field).+ != candidate.$($field).+ {
                        if edit.config.$($field).+ != base.$($field).+ { return Err("配置已更改，请重新读取"); }
                        edit.config.$($field).+ = candidate.$($field).+.clone();
                    }
                };
            }
            patch!(agent.composer_predictions_enabled);
            patch!(agent.desktop_use_enabled);
            patch!(agent.browser_use_enabled);
            patch!(agent.running_message_behavior);
            patch!(defaults.model);
            patch!(defaults.permission_mode);
            patch!(ui.theme);
            patch!(ui.sidebar_collapsed);
            patch!(ui.sidebar_width);
            patch!(ui.projects_collapsed);
            patch!(ui.sessions_collapsed);
            edit.save().map_err(|_| "配置保存失败")?;
            Ok(edit.config.clone())
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_management.saving = false;
                match result {
                    Ok(config) => {
                        let desktop_enabled =
                            desktop_enable_requested && config.agent.desktop_use_enabled;
                        crate::conversation_stream::set_composer_predictions_enabled(
                            config.agent.composer_predictions_enabled,
                            cx,
                        );
                        cx.set_global(RunningMessageDefault(
                            if config.agent.running_message_behavior == "steer" {
                                PendingMessageBehavior::Steer
                            } else {
                                PendingMessageBehavior::Queue
                            },
                        ));
                        if theme_changed {
                            cx.set_global(match config.ui.theme.as_str() {
                                "light" => vega_theme::Theme::light(),
                                "dark" => vega_theme::Theme::dark(),
                                _ => vega_theme::Theme::system(cx),
                            });
                        }
                        this.config = config;
                        this.error = None;
                        cx.emit(SettingsSaved {
                            replay_identity_changed,
                        });
                        if desktop_enabled {
                            cx.emit(DesktopPreparationRequested);
                        }
                    }
                    Err(error) => this.error = Some(error.into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn select_running_message_behavior(
        &mut self,
        behavior: PendingMessageBehavior,
        cx: &mut Context<Self>,
    ) {
        let mut candidate = self.config.clone();
        candidate.agent.running_message_behavior = behavior.as_str().to_string();
        self.save_preferences(candidate, cx);
    }
    pub(crate) fn toggle_composer_predictions(&mut self, cx: &mut Context<Self>) {
        let mut candidate = self.config.clone();
        candidate.agent.composer_predictions_enabled =
            !candidate.agent.composer_predictions_enabled;
        self.save_preferences(candidate, cx);
    }
}
