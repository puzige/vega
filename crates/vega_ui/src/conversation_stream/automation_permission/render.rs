use super::*;
use crate::interaction::InteractionExt;

impl ConversationStream {
    fn automation_permission_action_button(
        &self,
        action: Action,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let (id, focus) = match action {
            Action::Cancel => (
                "automation-permission-cancel",
                &self.automation_permission.cancel,
            ),
            Action::Confirm => (
                "automation-permission-confirm",
                &self.automation_permission.confirm,
            ),
            Action::Reload => (
                "automation-permission-reload",
                &self.automation_permission.reload,
            ),
        };
        div()
            .id(id)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .track_focus(focus)
            .tab_stop(true)
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, true)
            .focus_visible(move |style| style.border_color(colors.accent))
            .cursor_pointer()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.automation_permission_action(action, cx)),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.automation_permission_action(action, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .child(label)
            .into_any_element()
    }

    pub(crate) fn render_automation_permission(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.automation_permission.busy() {
            return div().into_any_element();
        }
        let colors = theme(cx).colors;
        let panel = div()
            .id("automation-permission-feedback")
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .mb_2()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .border_1()
            .rounded_md()
            .border_color(colors.border_subtle)
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.cancel_automation_permission(cx);
                        cx.stop_propagation();
                    } else if event.keystroke.key == "tab"
                        && matches!(this.automation_permission.phase, Phase::Confirming(_))
                    {
                        let focuses = [
                            &this.automation_permission.cancel,
                            &this.automation_permission.confirm,
                            &this.automation_permission.details,
                        ];
                        let current = focuses
                            .iter()
                            .position(|focus| focus.is_focused(window))
                            .unwrap_or(0);
                        let next = (current
                            + if event.keystroke.modifiers.shift {
                                2
                            } else {
                                1
                            })
                            % 3;
                        window.focus(focuses[next], cx);
                        cx.stop_propagation();
                    }
                }),
            );
        match &self.automation_permission.phase {
            Phase::Preparing(_) => panel
                .child(vega_i18n::t("ui.permission_reading"))
                .child(self.automation_permission_action_button(
                    Action::Cancel,
                    vega_i18n::t("ui.cancel"),
                    cx,
                ))
                .into_any_element(),
            Phase::Confirming(change) => {
                let task = change.task();
                let schedule = if let Some(interval) = task.interval_seconds {
                    vega_i18n::t_args(
                        "ui.permission_automation_interval",
                        &[("seconds", &interval.to_string())],
                    )
                } else {
                    vega_i18n::t("ui.permission_automation_goal").to_string()
                };
                let details = div()
                    .id("automation-permission-details")
                    .role(gpui_kit::Role::Group)
                    .aria_label(vega_i18n::t("ui.permission_automation_details"))
                    .track_focus(&self.automation_permission.details)
                    .tab_stop(true)
                    .min_h_0()
                    .max_h(px(
                        (f32::from(window.viewport_size().height) - 240.0).clamp(96.0, 360.0)
                    ))
                    .overflow_y_scroll()
                    .track_scroll(&self.automation_permission.details_scroll)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_1()
                    .border_color(gpui_kit::transparent_black())
                    .focus_visible(move |style| style.border_color(colors.accent))
                    .on_scroll_wheel(crate::tool_card::contain_disclosure_scroll(
                        &self.automation_permission.details_scroll,
                    ))
                    .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                        let scroll = &this.automation_permission.details_scroll;
                        let mut offset = scroll.offset();
                        let limit = scroll.max_offset().y.max(px(0.));
                        offset.y = match event.keystroke.key.as_str() {
                            "up" => offset.y + px(32.),
                            "down" => offset.y - px(32.),
                            "pageup" => offset.y + px(180.),
                            "pagedown" => offset.y - px(180.),
                            "space" if event.keystroke.modifiers.shift => offset.y + px(180.),
                            "space" => offset.y - px(180.),
                            "home" => px(0.),
                            "end" => -limit,
                            _ => return,
                        }
                        .clamp(-limit, px(0.));
                        scroll.set_offset(offset);
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_color(colors.warning)
                            .child(vega_i18n::t("settings.permission.full_access_risk")),
                    )
                    .child(vega_i18n::t("settings.permission.persistent_effects"))
                    .child(vega_i18n::t("ui.permission_automation_scope"))
                    .child(schedule)
                    .when_some(task.token_budget, |details, budget| {
                        details.child(vega_i18n::t_args(
                            "ui.permission_automation_budget",
                            &[("tokens", &budget.to_string())],
                        ))
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink_0()
                            .whitespace_normal()
                            .child(task.instruction.clone()),
                    );
                panel
                    .child(vega_i18n::t_args(
                        "ui.permission_automation_title",
                        &[("kind", task.kind.label())],
                    ))
                    .child(details)
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(self.automation_permission_action_button(
                                Action::Cancel,
                                vega_i18n::t("ui.cancel"),
                                cx,
                            ))
                            .child(self.automation_permission_action_button(
                                Action::Confirm,
                                vega_i18n::t(if change.resumes_task() {
                                    "ui.permission_automation_enable_resume"
                                } else {
                                    "ui.permission_automation_enable"
                                }),
                                cx,
                            )),
                    )
                    .into_any_element()
            }
            Phase::Saving => panel
                .child(vega_i18n::t("ui.permission_saving"))
                .into_any_element(),
            Phase::Reloading => panel
                .child(vega_i18n::t("ui.permission_reading"))
                .into_any_element(),
            Phase::Failed(error) => {
                let key = match error {
                    Error::Conflict => "ui.permission_error_conflict",
                    Error::ResetPending => "ui.permission_error_reset",
                    Error::OutcomeUnknown => "ui.permission_error_unknown",
                    Error::ConfirmationRequired => "ui.permission_error_confirmation",
                    Error::Unavailable => "ui.permission_error_unavailable",
                };
                panel
                    .child(div().text_color(colors.danger).child(vega_i18n::t(key)))
                    .child(self.automation_permission_action_button(
                        Action::Reload,
                        vega_i18n::t("ui.permission_reload"),
                        cx,
                    ))
                    .into_any_element()
            }
            Phase::Idle => panel.into_any_element(),
        }
    }

    pub(crate) fn render_automation_permission_trigger(
        &self,
        task: &AutomationTask,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let task = task.clone();
        let resume = task.status != AutomationStatus::Active;
        let enabled = !self.permission_change_pending()
            && !self.actions.running
            && !self.composer_submit_pending
            && !self.trusted_action_busy
            && !self.automation.projection.busy;
        let key = if resume {
            "ui.permission_automation_resume_action"
        } else {
            "ui.permission_automation_action"
        };
        let keyboard_task = task.clone();
        div()
            .id(format!("automation-{}-authorize", task.kind.key()))
            .role(gpui_kit::Role::Button)
            .aria_label(vega_i18n::t(key))
            .track_focus(focus)
            .tab_stop(enabled)
            .px_2()
            .py_1()
            .rounded_md()
            .text_size(px(Typography::METADATA))
            .whitespace_nowrap()
            .border_1()
            .border_color(colors.border_subtle)
            .interaction_surface(
                colors.bg_elevated,
                colors.bg_hover,
                colors.bg_active,
                enabled,
            )
            .focus_visible(move |style| style.border_color(colors.accent))
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.request_automation_permission(task.clone(), resume, None, cx)
                        }),
                    )
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.request_automation_permission(
                                    keyboard_task.clone(),
                                    resume,
                                    None,
                                    cx,
                                );
                                cx.stop_propagation();
                            }
                        }),
                    )
            })
            .when(!enabled, |button| button.text_color(colors.text_tertiary))
            .child(vega_i18n::t(key))
            .into_any_element()
    }
}
