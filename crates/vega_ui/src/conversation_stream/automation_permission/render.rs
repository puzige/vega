use super::*;
use native::{Intent, control};

impl ConversationStream {
    fn automation_permission_action_button(
        &self,
        action: Action,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, focus) = match action {
            Action::Cancel => ("cancel", &self.automation_permission.cancel),
            Action::Confirm => ("confirm", &self.automation_permission.confirm),
            Action::Reload => ("reload", &self.automation_permission.reload),
        };
        div()
            .flex_1()
            .min_w_0()
            .child(control(
                self,
                format!("automation-permission-{id}"),
                label.to_owned(),
                Intent::Permission(action),
                None,
                Some(focus.clone()),
                cx,
            ))
            .into_any_element()
    }

    pub(crate) fn render_automation_permission(
        &self,
        _: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.automation_permission.busy() {
            return div().into_any_element();
        }
        let text = |id: &str, text: String, cx: &Context<Self>| {
            control(
                self,
                format!("automation-permission-{id}"),
                text,
                Intent::Read,
                None,
                None,
                cx,
            )
        };
        let panel = div()
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .mb_2()
            .flex()
            .flex_col()
            .gap_2();
        match &self.automation_permission.phase {
            Phase::Preparing(_) => panel
                .child(text(
                    "preparing",
                    vega_i18n::t("ui.permission_reading").to_owned(),
                    cx,
                ))
                .child(self.automation_permission_action_button(
                    Action::Cancel,
                    vega_i18n::t("ui.cancel"),
                    cx,
                ))
                .into_any_element(),
            Phase::Confirming(change) => {
                let task = change.task();
                let schedule = task.interval_seconds.map_or_else(
                    || vega_i18n::t("ui.permission_automation_goal").to_owned(),
                    |interval| {
                        vega_i18n::t_args(
                            "ui.permission_automation_interval",
                            &[("seconds", &interval.to_string())],
                        )
                    },
                );
                let mut details = vec![
                    vega_i18n::t("settings.permission.persistent_effects").to_owned(),
                    vega_i18n::t("ui.permission_automation_scope").to_owned(),
                    schedule,
                ];
                if let Some(budget) = task.token_budget {
                    details.push(vega_i18n::t_args(
                        "ui.permission_automation_budget",
                        &[("tokens", &budget.to_string())],
                    ));
                }
                details.push(task.instruction.clone());
                let mut risk = text(
                    "risk",
                    vega_i18n::t("settings.permission.full_access_risk").to_owned(),
                    cx,
                );
                risk.foreground = theme(cx).colors.warning;
                panel
                    .child(text(
                        "title",
                        vega_i18n::t_args(
                            "ui.permission_automation_title",
                            &[("kind", task.kind.label())],
                        ),
                        cx,
                    ))
                    .child(risk)
                    .child(control(
                        self,
                        "automation-permission-details".to_owned(),
                        details.join("\n\n"),
                        Intent::Details,
                        Some(task),
                        Some(self.automation_permission.details.clone()),
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
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
                .child(text(
                    "saving",
                    vega_i18n::t("ui.permission_saving").to_owned(),
                    cx,
                ))
                .into_any_element(),
            Phase::Reloading => panel
                .child(text(
                    "reloading",
                    vega_i18n::t("ui.permission_reading").to_owned(),
                    cx,
                ))
                .into_any_element(),
            Phase::Failed(error) => {
                let key = match error {
                    Error::Conflict => "ui.permission_error_conflict",
                    Error::ResetPending => "ui.permission_error_reset",
                    Error::OutcomeUnknown => "ui.permission_error_unknown",
                    Error::ConfirmationRequired => "ui.permission_error_confirmation",
                    Error::Unavailable => "ui.permission_error_unavailable",
                };
                let mut error = text("error", vega_i18n::t(key).to_owned(), cx);
                error.foreground = theme(cx).colors.danger;
                panel
                    .child(error)
                    .child(self.automation_permission_action_button(
                        Action::Reload,
                        vega_i18n::t("ui.permission_reload"),
                        cx,
                    ))
                    .into_any_element()
            }
            Phase::Idle => div().into_any_element(),
        }
    }

    pub(crate) fn render_automation_permission_trigger(
        &self,
        task: &AutomationTask,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        control(
            self,
            format!("automation-{}-authorize", task.identity),
            vega_i18n::t(if task.status != AutomationStatus::Active {
                "ui.permission_automation_resume_action"
            } else {
                "ui.permission_automation_action"
            })
            .to_owned(),
            Intent::Authorize,
            Some(task),
            Some(focus.clone()),
            cx,
        )
        .into_any_element()
    }
}
