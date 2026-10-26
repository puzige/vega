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
            self.error = Some(SettingsText::key("settings.config.path_unavailable"));
            cx.notify();
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
            if base.providers != candidate.providers {
                return Err(SettingsText::key(
                    "settings.preferences.provider_form_required",
                ));
            }
            let mut edit = config::begin_edit(&path)
                .map_err(|_| SettingsText::key("settings.config.read_failed"))?;
            macro_rules! patch {
                ($($field:ident).+) => {
                    if base.$($field).+ != candidate.$($field).+ {
                        if edit.config.$($field).+ != base.$($field).+ {
                            return Err(SettingsText::key("settings.config.conflict"));
                        }
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
            patch!(ui.language);
            patch!(ui.sidebar_collapsed);
            patch!(ui.sidebar_width);
            patch!(ui.projects_collapsed);
            patch!(ui.sessions_collapsed);
            edit.save()
                .map_err(|_| SettingsText::key("settings.config.save_failed"))?;
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
                        let language_changed = vega_i18n::language() != config.ui.language;
                        vega_i18n::set_language(&config.ui.language);
                        this.config = config;
                        this.error = None;
                        if language_changed {
                            this.refresh_localized_inputs(cx);
                            cx.refresh_windows();
                        }
                        cx.emit(SettingsSaved {
                            replay_identity_changed,
                        });
                        if desktop_enabled {
                            if cx.try_global::<SettingsOpen>().is_some_and(|open| open.0) {
                                this.acquire_preparation(cx);
                            }
                            cx.emit(DesktopPreparationRequested { pane: None });
                        }
                    }
                    Err(error) => this.error = Some(error),
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
