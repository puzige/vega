use super::*;
use crate::interaction::InteractionExt;
use vega_conversation::types::{
    PermissionResetAction as Action, PermissionResetError, PermissionResetProjection as Projection,
    PermissionResetRequested,
};

pub(crate) struct ResetState {
    pub(crate) projection: Projection,
    focuses: [FocusHandle; 5],
    focus_pending: bool,
}

impl ResetState {
    pub(super) fn new(cx: &mut Context<SettingsView>) -> Self {
        Self {
            projection: Projection::Idle,
            focuses: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            focus_pending: false,
        }
    }
}

impl SettingsView {
    pub fn apply_permission_reset_projection(
        &mut self,
        projection: Projection,
        cx: &mut Context<Self>,
    ) {
        if self.permission_reset.projection != projection {
            self.permission_reset.focus_pending = matches!(
                projection,
                Projection::Confirming | Projection::Blocked { .. } | Projection::Completed
            );
            self.permission_reset.projection = projection;
            cx.notify();
        }
    }

    fn permission_reset_action(&mut self, action: Action, cx: &mut Context<Self>) {
        if matches!(action, Action::Prepare) {
            if self.default_permission.io_pending()
                || self.provider_management.saving
                || self.codex_profile_saving
            {
                return;
            }
            self.close_default_permission_session();
        }
        cx.emit(PermissionResetRequested(action));
    }

    pub(crate) fn prepare_permission_reset_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.section != 1 || !std::mem::take(&mut self.permission_reset.focus_pending) {
            return;
        }
        let index = match self.permission_reset.projection {
            Projection::Confirming => 1,
            Projection::Blocked { .. } => 3,
            _ => 0,
        };
        window.focus(&self.permission_reset.focuses[index], cx);
    }

    fn permission_reset_button(
        &self,
        action: Action,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let (index, id, key) = match action {
            Action::Prepare => (0, "permission-reset-prepare", "ui.permission_reset_action"),
            Action::Cancel => (1, "permission-reset-cancel", "ui.permission_reset_cancel"),
            Action::Confirm => (2, "permission-reset-confirm", "ui.permission_reset_confirm"),
            Action::Retry => (3, "permission-reset-retry", "ui.permission_reset_retry"),
            Action::Inspect => (4, "permission-reset-inspect", "ui.permission_reset_inspect"),
        };
        let label = vega_i18n::t(key);
        settings_action_chrome(colors)
            .id(id)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .track_focus(&self.permission_reset.focuses[index])
            .tab_stop(enabled)
            .focus_visible(move |style| style.border_color(colors.accent))
            .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, enabled)
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.permission_reset_action(action, cx)),
                    )
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.permission_reset_action(action, cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
            })
            .when(!enabled, |button| button.text_color(colors.text_tertiary))
            .child(label)
            .into_any_element()
    }

    pub(crate) fn render_permission_reset(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let panel = settings_section().on_key_down(cx.listener(
            |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                if matches!(this.permission_reset.projection, Projection::Confirming)
                    && event.keystroke.key == "tab"
                {
                    let index = if this.permission_reset.focuses[1].is_focused(window) {
                        2
                    } else {
                        1
                    };
                    window.focus(&this.permission_reset.focuses[index], cx);
                    cx.stop_propagation();
                } else if matches!(
                    this.permission_reset.projection,
                    Projection::Preparing | Projection::Confirming
                ) && event.keystroke.key == "escape"
                {
                    this.permission_reset_action(Action::Cancel, cx);
                    cx.stop_propagation();
                }
            },
        ));
        match &self.permission_reset.projection {
            Projection::Idle | Projection::Completed => panel
                .child(self.permission_reset_button(
                    Action::Prepare,
                    !self.default_permission.io_pending()
                        && !self.provider_management.saving
                        && !self.codex_profile_saving,
                    cx,
                ))
                .into_any_element(),
            Projection::Preparing => panel
                .child(settings_status(
                    vega_i18n::t("ui.permission_reset_preparing"),
                    colors.text_secondary,
                    colors,
                ))
                .child(self.permission_reset_button(Action::Cancel, true, cx))
                .into_any_element(),
            Projection::Confirming => panel
                .p_3()
                .border_1()
                .rounded(px(Layout::PANEL_RADIUS))
                .border_color(colors.warning)
                .child(section_title(
                    vega_i18n::t("ui.permission_reset_title"),
                    colors.text_primary,
                ))
                .child(vega_i18n::t("ui.permission_reset_scope"))
                .child(
                    div()
                        .text_color(colors.warning)
                        .child(vega_i18n::t("ui.permission_reset_effects")),
                )
                .child(
                    settings_actions()
                        .child(self.permission_reset_button(Action::Cancel, true, cx))
                        .child(self.permission_reset_button(Action::Confirm, true, cx)),
                )
                .into_any_element(),
            Projection::Pausing | Projection::Stopping => panel
                .child(settings_status(
                    vega_i18n::t("ui.permission_reset_stopping"),
                    colors.text_secondary,
                    colors,
                ))
                .into_any_element(),
            Projection::Refreshing => panel
                .child(settings_status(
                    vega_i18n::t("ui.permission_reset_refreshing"),
                    colors.text_secondary,
                    colors,
                ))
                .into_any_element(),
            Projection::Blocked {
                error,
                can_cancel,
                thread_id,
            } => {
                let key = match error {
                    PermissionResetError::Busy => "ui.permission_reset_busy",
                    PermissionResetError::UnsettledWork => "ui.permission_reset_unsettled",
                    PermissionResetError::ConfigurationConflict => "ui.permission_reset_conflict",
                    PermissionResetError::ConfigurationUnavailable => {
                        "ui.permission_reset_configuration"
                    }
                    PermissionResetError::OutcomeUnknown => "ui.permission_reset_unknown",
                    PermissionResetError::Unavailable => "ui.permission_reset_unavailable",
                };
                panel
                    .child(settings_status(vega_i18n::t(key), colors.danger, colors))
                    .child(
                        settings_actions()
                            .child(self.permission_reset_button(Action::Retry, true, cx))
                            .when(*can_cancel, |actions| {
                                actions.child(self.permission_reset_button(
                                    Action::Cancel,
                                    true,
                                    cx,
                                ))
                            })
                            .when(thread_id.is_some(), |actions| {
                                actions.child(self.permission_reset_button(
                                    Action::Inspect,
                                    true,
                                    cx,
                                ))
                            }),
                    )
                    .into_any_element()
            }
        }
    }
}
