use super::*;
use vega_ui::interaction::InteractionExt;

fn merge_permission_refresh(current: &Thread, saved: &Thread) -> Thread {
    let mut refreshed = current.clone();
    refreshed.permission_mode = saved.permission_mode;
    refreshed.permission_revision = saved.permission_revision;
    refreshed.draft_authority_epoch = saved.draft_authority_epoch.clone();
    refreshed.full_access_grant = saved.full_access_grant.clone();
    refreshed.default_permission = saved.default_permission.clone();
    refreshed
}

fn reset_retained_draft_permissions(thread: &Thread) -> Thread {
    let mut draft = thread.clone();
    if draft.permission_mode == PermissionMode::FullAccess {
        draft.permission_mode = PermissionMode::Confirm;
    }
    draft.draft_authority_epoch = None;
    draft.full_access_grant = None;
    draft.default_permission = None;
    draft
}

impl VegaWindow {
    pub(in crate::window) fn project_retained_draft_permission_reset(thread: &Thread) -> Thread {
        reset_retained_draft_permissions(thread)
    }

    pub(super) fn permission_reset_streams(&self) -> Vec<Entity<ConversationStream>> {
        let mut streams: Vec<_> = self
            .retained_drafts
            .values()
            .map(|(_, stream)| stream.clone())
            .collect();
        if let Some((_, stream)) = &self.stream_view {
            streams.push(stream.clone());
        }
        streams.extend(
            self.agent_controller
                .active
                .values()
                .map(|active| active.stream.clone()),
        );
        streams.extend(self.agent_controller.preparation_stream.iter().cloned());
        streams.extend(self.agent_controller.queue_streams.values().cloned());
        streams.extend(
            self.agent_controller
                .queue_drain_waits
                .values()
                .map(|wait| wait.stream.clone()),
        );
        streams.extend(
            self.agent_controller
                .pending_review
                .values()
                .map(|review| review.stream.clone()),
        );
        streams.extend(self.automation_controller.streams.values().cloned());
        streams.extend(self.subagent_panel.cached_stream());
        let mut unique = Vec::new();
        for stream in streams {
            if !unique.contains(&stream) {
                unique.push(stream);
            }
        }
        unique
    }

    pub(super) fn apply_permission_reset_refresh(
        &mut self,
        refresh: PermissionResetRefresh,
        cx: &mut Context<Self>,
    ) -> bool {
        if !cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .is_ok_and(|store| store.database_identity() == refresh.database)
        {
            return false;
        }
        let Some((_, path)) = self.permission_reset.paths.as_ref() else {
            return false;
        };
        if self.composer_config_path().as_ref() != Some(path) {
            return false;
        }
        let mut defaults = NewTaskDefaults::from_config(Some(path), &refresh.config.defaults);
        defaults.reasoning_preferences = cx
            .try_global::<NewTaskDefaults>()
            .and_then(|previous| previous.reasoning_preferences.clone());
        let streams = self.permission_reset_streams();
        let project = |thread: &Thread| -> Option<Thread> {
            if let Some(saved) = refresh
                .threads
                .iter()
                .find(|saved| saved.thread.id == thread.id)
            {
                return (saved.thread.project_id == thread.project_id)
                    .then(|| merge_permission_refresh(thread, &saved.thread));
            }
            self.draft_for_route(&thread.id)
                .filter(|draft| draft.project_id == thread.project_id)
                .map(|_| Self::project_retained_draft_permission_reset(thread))
        };
        let mut projected = Vec::new();
        for stream in streams {
            let source = stream.read(cx).thread_snapshot();
            let Some(thread) = project(&source) else {
                return false;
            };
            projected.push((stream, thread));
        }
        let opened = match &cx.global::<OpenedThread>().0 {
            Some(thread) => {
                let Some(thread) = project(thread) else {
                    return false;
                };
                Some(thread)
            }
            None => None,
        };
        let draft = match &self.draft {
            Some(thread) => {
                let Some(thread) = project(thread) else {
                    return false;
                };
                Some(thread)
            }
            None => None,
        };
        let mut retained_drafts = Vec::new();
        for (_, thread) in &projected {
            if let Some(draft) = self.draft_for_route(&thread.id)
                && !retained_drafts
                    .iter()
                    .any(|existing: &Thread| existing.id == draft.id)
            {
                let Some(draft) = project(&draft) else {
                    return false;
                };
                retained_drafts.push(draft);
            }
        }
        for (stream, thread) in projected {
            let saved = refresh
                .threads
                .iter()
                .find(|saved| saved.thread.id == thread.id);
            let messages = saved.map_or_else(Vec::new, |saved| saved.pending.clone());
            let pending = PendingQueueProjection {
                paused: !messages.is_empty(),
                error: (!messages.is_empty())
                    .then(|| vega_i18n::t("ui.permission_reset_queue").into()),
                messages,
                ..Default::default()
            };
            let automation = saved.map_or_else(AutomationProjection::default, |saved| {
                saved.automation.clone()
            });
            if !stream.update(cx, |stream, cx| {
                stream.apply_permission_reset(thread, pending, automation, cx)
            }) {
                return false;
            }
        }
        cx.set_global(defaults);
        self.draft = draft;
        for draft in retained_drafts {
            self.mirror_draft(&draft);
        }
        cx.set_global(OpenedThread(opened));
        if let Some(view) = &self.settings_view {
            let values = DefaultPermissionValues::from(&refresh.config.defaults);
            view.update(cx, |view, cx| {
                view.apply_permission_reset_defaults(values, cx)
            });
        }
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.reload_sessions(cx));
        true
    }

    pub(in crate::window) fn render_permission_reset_notice(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.permission_reset.projection.authority_blocked() {
            return None;
        }
        let colors = theme(cx).colors;
        let focus = self
            .permission_reset
            .notice_focus
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let key = if self.permission_reset.session.is_some() {
            "ui.permission_reset_paused_notice"
        } else {
            "ui.permission_reset_failed_notice"
        };
        Some(
            div()
                .px_3()
                .py_1()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(Typography::METADATA))
                .text_color(colors.warning)
                .child(vega_i18n::t(key))
                .child(
                    div()
                        .id("permission-reset-return-settings")
                        .role(gpui_kit::Role::Button)
                        .aria_label(vega_i18n::t("ui.permission_reset_open_settings"))
                        .track_focus(&focus)
                        .tab_stop(true)
                        .focus_visible(move |style| style.border_1().border_color(colors.accent))
                        .cursor_pointer()
                        .rounded(px(4.0))
                        .px_2()
                        .py_1()
                        .interaction_surface(
                            colors.bg_base,
                            colors.bg_hover,
                            colors.bg_active,
                            true,
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.open_permission_reset_settings(cx)),
                        )
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.open_permission_reset_settings(cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(vega_i18n::t("ui.permission_reset_open_settings")),
                )
                .into_any_element(),
        )
    }

    fn open_permission_reset_settings(&mut self, cx: &mut Context<Self>) {
        cx.set_global(SettingsOpen(true));
        cx.set_global(PricingSettingsRequested(false));
        if let Some(view) = &self.settings_view {
            view.update(cx, |view, cx| view.show_general(cx));
        }
        cx.notify();
    }
}

#[cfg(test)]
mod reasoning_permission_development_tests {
    use super::{merge_permission_refresh, reset_retained_draft_permissions};
    use vega_conversation::types::{PermissionMode, SessionReasoningPreference};

    #[test]
    fn delayed_permission_refresh_preserves_confirmed_reasoning() {
        let mut current = vega_conversation::threads::draft_thread(None, "m", "confirm");
        current.reasoning_preferences = Some(vec![SessionReasoningPreference {
            provider: "p".into(),
            model: "m".into(),
            preference: "high".into(),
        }]);
        let mut old_snapshot = current.clone();
        old_snapshot.reasoning_preferences.as_mut().unwrap()[0].preference = "low".into();
        old_snapshot.permission_mode = PermissionMode::ReadOnly;
        old_snapshot.permission_revision = 42;
        let merged = merge_permission_refresh(&current, &old_snapshot);
        assert_eq!(merged.reasoning_preferences, current.reasoning_preferences);
        assert_eq!(merged.permission_mode, PermissionMode::ReadOnly);
        assert_eq!(merged.permission_revision, 42);
    }

    #[test]
    fn each_retained_draft_loses_full_access_without_losing_its_reasoning() {
        let mut a = vega_conversation::threads::draft_thread(None, "m", "full_access");
        a.reasoning_preferences = Some(vec![SessionReasoningPreference {
            provider: "p".into(),
            model: "m".into(),
            preference: "high".into(),
        }]);
        let mut b = vega_conversation::threads::draft_thread(None, "m", "readonly");
        b.reasoning_preferences = Some(vec![SessionReasoningPreference {
            provider: "p".into(),
            model: "m".into(),
            preference: "low".into(),
        }]);
        let reset_a = reset_retained_draft_permissions(&a);
        let reset_b = reset_retained_draft_permissions(&b);
        assert_ne!(reset_a.id, reset_b.id);
        assert_eq!(reset_a.permission_mode, PermissionMode::Confirm);
        assert_eq!(reset_b.permission_mode, PermissionMode::ReadOnly);
        assert_eq!(reset_a.reasoning_preferences, a.reasoning_preferences);
        assert_eq!(reset_b.reasoning_preferences, b.reasoning_preferences);
        assert!(reset_a.draft_authority_epoch.is_none());
        assert!(reset_a.full_access_grant.is_none());
        assert!(reset_a.default_permission.is_none());
        assert!(reset_b.draft_authority_epoch.is_none());
        assert!(reset_b.full_access_grant.is_none());
        assert!(reset_b.default_permission.is_none());
    }
}
