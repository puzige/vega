use super::*;
use crate::interaction::InteractionExt;
#[cfg(test)]
#[path = "shell_recovery_tests.rs"]
mod tests;
use vega_conversation::types::{
    ShellRecoveryConfirmation, ShellRecoveryError, ShellRecoveryProjection,
};

#[derive(Clone)]
pub enum ShellRecoveryAction {
    Read,
    Cancel(String),
    Check(ShellRecoveryProjection),
    Retry(ShellRecoveryProjection),
    Prepare(ShellRecoveryProjection),
    Resume(Box<ShellRecoveryConfirmation>),
}

#[derive(Clone)]
pub struct ShellRecoveryRequested {
    pub source: Thread,
    pub sequence: u64,
    pub draft: bool,
    pub action: ShellRecoveryAction,
}

impl ShellRecoveryRequested {
    fn matches_route(&self, thread: &Thread, draft: bool, sequence: u64) -> bool {
        self.source.id == thread.id
            && self.source.project_id == thread.project_id
            && self.draft == draft
            && self.sequence == sequence
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RecoveryPresentation {
    Hidden,
    Summary(&'static str),
    Details,
}

fn recovery_presentation(draft: bool, blocked: bool, unavailable: bool) -> RecoveryPresentation {
    if !draft {
        RecoveryPresentation::Details
    } else if unavailable {
        RecoveryPresentation::Summary("ui.shell_recovery.status_unavailable")
    } else if blocked {
        RecoveryPresentation::Summary("ui.shell_recovery.paused")
    } else {
        RecoveryPresentation::Hidden
    }
}

pub enum ShellRecoveryOutcome {
    Projection(ShellRecoveryProjection),
    Confirmation(Box<ShellRecoveryConfirmation>),
}

impl EventEmitter<ShellRecoveryRequested> for ConversationStream {}

pub(crate) struct ShellRecoveryUi {
    sequence: u64,
    pending: Option<&'static str>,
    pub(super) opened: bool,
    pub(super) projection: Option<ShellRecoveryProjection>,
    decision: RecoveryDecision<ShellRecoveryConfirmation>,
    error: Option<ShellRecoveryError>,
    refresh: Option<gpui_kit::Task<()>>,
    review_focus: FocusHandle,
    safe_focus: FocusHandle,
}

struct RecoveryDecision<T> {
    confirmation: Option<Box<T>>,
    submitted: bool,
}

impl<T> Default for RecoveryDecision<T> {
    fn default() -> Self {
        Self {
            confirmation: None,
            submitted: false,
        }
    }
}

impl<T> RecoveryDecision<T> {
    fn dismiss(&mut self) {
        if !self.submitted {
            self.confirmation = None;
        }
    }
}

impl ShellRecoveryUi {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            sequence: 0,
            pending: None,
            opened: false,
            projection: None,
            decision: RecoveryDecision::default(),
            error: None,
            refresh: None,
            review_focus: cx.focus_handle(),
            safe_focus: cx.focus_handle(),
        }
    }
}

impl ConversationStream {
    pub(super) fn reset_shell_recovery(&mut self, cx: &mut Context<Self>) {
        let sequence = self.shell_recovery.sequence.saturating_add(1);
        self.shell_recovery = ShellRecoveryUi::new(cx);
        self.shell_recovery.sequence = sequence;
        let stream = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = stream.update(cx, |this, cx| this.refresh_shell_recovery(cx));
        });
    }

    pub(super) fn dismiss_shell_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shell_recovery.opened = false;
        if self.shell_recovery.pending.is_none() {
            self.shell_recovery.decision.dismiss();
        }
        self.shell_recovery.review_focus.focus(window, cx);
        cx.notify();
    }

    pub fn refresh_shell_recovery(&mut self, cx: &mut Context<Self>) {
        if self.thread.backend == TaskBackend::Native
            && !self.thread.project_id.is_empty()
            && self.shell_recovery.pending.is_none()
            && self.shell_recovery.decision.confirmation.is_none()
        {
            self.request_shell_recovery(ShellRecoveryAction::Read, cx);
        }
    }

    fn request_shell_recovery(&mut self, action: ShellRecoveryAction, cx: &mut Context<Self>) {
        if self.shell_recovery.pending.is_some() {
            return;
        }
        let Some(sequence) = self.shell_recovery.sequence.checked_add(1) else {
            return;
        };
        let label = match &action {
            ShellRecoveryAction::Retry(_) => "ui.shell_recovery.retrying",
            ShellRecoveryAction::Resume(_) => "ui.shell_recovery.resuming",
            ShellRecoveryAction::Cancel(_) => "ui.shell_session.stopping",
            _ => "ui.shell_recovery.checking",
        };
        if matches!(&action, ShellRecoveryAction::Resume(_)) {
            self.shell_recovery.decision.submitted = true;
        }
        self.shell_recovery.sequence = sequence;
        self.shell_recovery.pending = Some(label);
        self.shell_recovery.error = None;
        cx.emit(ShellRecoveryRequested {
            source: self.thread.clone(),
            sequence,
            draft: self.draft_route,
            action,
        });
        cx.notify();
    }

    pub fn finish_shell_recovery(
        &mut self,
        request: &ShellRecoveryRequested,
        result: Result<ShellRecoveryOutcome, ShellRecoveryError>,
        cx: &mut Context<Self>,
    ) {
        if !request.matches_route(&self.thread, self.draft_route, self.shell_recovery.sequence) {
            return;
        }
        self.shell_recovery.pending = None;
        match result {
            Ok(ShellRecoveryOutcome::Projection(projection)) => {
                for command in &projection.commands {
                    if command.thread_id == self.thread.id
                        && let Some(card) = self.tool_cards.get(&command.call_id)
                    {
                        card.update(cx, |card, cx| {
                            if card.apply_shell_command(command) {
                                cx.notify();
                            }
                        });
                    }
                }
                self.shell_recovery.decision = RecoveryDecision::default();
                if projection.rows.is_empty() {
                    self.shell_recovery.opened = false;
                }
                self.shell_recovery.projection = Some(projection);
            }
            Ok(ShellRecoveryOutcome::Confirmation(confirmation)) => {
                self.shell_recovery.decision = RecoveryDecision {
                    confirmation: Some(confirmation),
                    submitted: false,
                };
            }
            Err(error) => {
                if error == ShellRecoveryError::Changed {
                    self.shell_recovery.decision = RecoveryDecision::default();
                }
                self.shell_recovery.error = Some(error);
            }
        }
        let refresh = self
            .shell_recovery
            .projection
            .as_ref()
            .is_some_and(|projection| {
                !projection.active.is_empty()
                    || projection.commands.iter().any(|command| {
                        command.thread_id == self.thread.id
                            && matches!(
                                command.state.as_str(),
                                "prepared" | "running" | "finishing" | "stopping"
                            )
                    })
            });
        if refresh && self.shell_recovery.refresh.is_none() {
            self.shell_recovery.refresh = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.shell_recovery.refresh = None;
                    this.refresh_shell_recovery(cx);
                });
            }));
        }
        cx.notify();
    }

    pub(super) fn render_shell_recovery(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.shell_recovery;
        let blocked = state
            .projection
            .as_ref()
            .is_some_and(|p| !p.rows.is_empty());
        let label = match recovery_presentation(self.draft_route, blocked, state.error.is_some()) {
            RecoveryPresentation::Hidden => return div().into_any_element(),
            RecoveryPresentation::Details => return self.render_shell_recovery_details(cx),
            RecoveryPresentation::Summary(label) => label,
        };
        let colors = theme(cx).colors;
        div()
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .px(px(Layout::CONVERSATION_PADDING))
            .flex()
            .items_center()
            .gap_2()
            .text_color(colors.text_secondary)
            .child(vega_i18n::t(label))
            .child(focused_button(
                "shell-draft-review",
                "ui.shell_recovery.review",
                Some(&state.review_focus),
                true,
                colors,
                cx.listener(|this, _, window, cx| {
                    this.shell_recovery.opened = true;
                    this.shell_recovery.safe_focus.focus(window, cx);
                    this.refresh_shell_recovery(cx);
                    cx.notify();
                }),
            ))
            .into_any_element()
    }

    pub(super) fn render_shell_recovery_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.draft_route || !self.shell_recovery.opened {
            return div().into_any_element();
        }
        let colors = theme(cx).colors;
        gpui_kit::deferred(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("draft-shell-recovery-details")
                        .track_focus(&self.shell_recovery.safe_focus)
                        .w_full()
                        .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                        .max_h(px(Layout::COMPOSER_PICKER_MAX_HEIGHT))
                        .overflow_y_scroll()
                        .rounded(px(Layout::MENU_RADIUS))
                        .border_1()
                        .border_color(colors.border_subtle)
                        .bg(colors.bg_elevated)
                        .shadow_sm()
                        .child(button(
                            "shell-draft-close",
                            "ui.shell_recovery.not_now",
                            true,
                            colors,
                            cx.listener(|this, _, window, cx| {
                                this.dismiss_shell_recovery(window, cx);
                            }),
                        ))
                        .child(self.render_shell_recovery_details(cx)),
                ),
        )
        .with_priority(3)
        .into_any_element()
    }

    fn render_shell_recovery_details(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.shell_recovery;
        let rows = state
            .projection
            .as_ref()
            .map(|p| p.rows.as_slice())
            .unwrap_or_default();
        let active = state
            .projection
            .as_ref()
            .map(|p| p.active.as_slice())
            .unwrap_or_default();
        if rows.is_empty() && active.is_empty() && state.error.is_none() {
            return div().into_any_element();
        }
        let colors = theme(cx).colors;
        let busy = state.pending.is_some();
        let mut panel = div()
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .px(px(Layout::CONVERSATION_PADDING))
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .text_color(colors.text_secondary);
        for command in active {
            let operation_id = command.operation_id.clone();
            let label = match command.state.as_str() {
                "stopping" => "ui.shell_session.stopping",
                "finishing" => "ui.shell_session.finishing",
                _ => "ui.shell_session.running",
            };
            panel = panel.child(div().child(vega_i18n::t(label)).child(button(
                "shell-cancel",
                "ui.shell_session.cancel",
                !busy && command.state == "running",
                colors,
                cx.listener(move |this, _, _, cx| {
                    this.request_shell_recovery(
                        ShellRecoveryAction::Cancel(operation_id.clone()),
                        cx,
                    )
                }),
            )));
        }
        if !rows.is_empty() {
            panel = panel.child(
                div()
                    .text_color(colors.danger)
                    .child(vega_i18n::t("ui.shell_recovery.paused")),
            );
        }
        if let Some(label) = state.pending {
            panel = panel.child(vega_i18n::t(label));
        }
        if let Some(error) = state.error {
            panel = panel.child(vega_i18n::t(match error {
                ShellRecoveryError::Changed => "ui.shell_recovery.changed",
                ShellRecoveryError::KnownProcessOrResource => "ui.shell_recovery.unsettled",
                ShellRecoveryError::Unavailable => "ui.shell_recovery.unavailable",
            }));
        }
        if rows.is_empty() {
            if state.error.is_some() {
                panel = panel.child(button(
                    "shell-read-again",
                    "ui.shell_recovery.check",
                    !busy,
                    colors,
                    cx.listener(|this, _, _, cx| {
                        this.request_shell_recovery(ShellRecoveryAction::Read, cx)
                    }),
                ));
            }
            return panel.into_any_element();
        }
        if !state.opened {
            return panel
                .child(focused_button(
                    "shell-review",
                    "ui.shell_recovery.review",
                    Some(&state.review_focus),
                    !busy,
                    colors,
                    cx.listener(|this, _, _, cx| {
                        this.shell_recovery.opened = true;
                        this.refresh_shell_recovery(cx);
                        cx.notify();
                    }),
                ))
                .into_any_element();
        }
        if let Some(projection) = &state.projection {
            panel = panel.child(div().min_w_0().child(projection.workspace.clone()));
            for (index, row) in rows.iter().enumerate() {
                let result = match row.exit_code {
                    Some(code) => {
                        vega_i18n::t_args("ui.shell_recovery.exit", &[("code", &code.to_string())])
                    }
                    None => vega_i18n::t("ui.shell_recovery.unknown").to_owned(),
                };
                panel = panel.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(if row.command.is_empty() {
                            vega_i18n::t_args(
                                "ui.shell_recovery.operation",
                                &[("number", &(index + 1).to_string())],
                            )
                        } else {
                            row.command.clone()
                        })
                        .child(result)
                        .child(
                            div()
                                .text_color(colors.text_secondary)
                                .child(recovery_reason(&row.reason)),
                        )
                        .when(row.output_incomplete && row.can_resume, |item| {
                            item.child(vega_i18n::t("ui.shell_recovery.output_closed"))
                        })
                        .when(!row.can_resume, |item| {
                            item.child(vega_i18n::t("ui.shell_recovery.unsettled"))
                        }),
                );
            }
        }
        if let Some(confirmation) = &state.decision.confirmation {
            let confirmation = confirmation.clone();
            panel = panel
                .child(vega_i18n::t("ui.shell_recovery.confirm_title"))
                .child(vega_i18n::t("ui.shell_recovery.confirm_body"));
            if rows.iter().any(|row| row.output_incomplete) {
                panel = panel.child(vega_i18n::t("ui.shell_recovery.confirm_output"));
            }
            panel = panel
                .child(button(
                    "shell-accept",
                    "ui.shell_recovery.accept",
                    !busy,
                    colors,
                    cx.listener(move |this, _, _, cx| {
                        this.request_shell_recovery(
                            ShellRecoveryAction::Resume(confirmation.clone()),
                            cx,
                        )
                    }),
                ))
                .child(focused_button(
                    "shell-keep-paused",
                    "ui.shell_recovery.keep_paused",
                    Some(&state.safe_focus),
                    !busy,
                    colors,
                    cx.listener(|this, _, window, cx| {
                        this.dismiss_shell_recovery(window, cx);
                    }),
                ));
        } else if let Some(projection) = &state.projection {
            let check = projection.clone();
            let retry = projection.clone();
            let prepare = projection.clone();
            panel = panel
                .child(button(
                    "shell-check",
                    "ui.shell_recovery.check",
                    !busy,
                    colors,
                    cx.listener(move |this, _, _, cx| {
                        this.request_shell_recovery(ShellRecoveryAction::Check(check.clone()), cx)
                    }),
                ))
                .child(button(
                    "shell-retry",
                    "ui.shell_recovery.retry",
                    !busy && rows.iter().any(|row| row.can_retry),
                    colors,
                    cx.listener(move |this, _, _, cx| {
                        this.request_shell_recovery(ShellRecoveryAction::Retry(retry.clone()), cx)
                    }),
                ))
                .child(button(
                    "shell-resume",
                    "ui.shell_recovery.resume",
                    !busy && !rows.is_empty() && rows.iter().all(|row| row.can_resume),
                    colors,
                    cx.listener(move |this, _, window, cx| {
                        this.shell_recovery.safe_focus.focus(window, cx);
                        this.request_shell_recovery(
                            ShellRecoveryAction::Prepare(prepare.clone()),
                            cx,
                        )
                    }),
                ));
        }
        panel
            .child(button(
                "shell-not-now",
                "ui.shell_recovery.not_now",
                !busy,
                colors,
                cx.listener(|this, _, window, cx| {
                    this.dismiss_shell_recovery(window, cx);
                }),
            ))
            .into_any_element()
    }
}

fn recovery_reason(reason: &str) -> String {
    let mut keys = Vec::new();
    for reason in reason.split(", ") {
        let key = match reason {
            "control_continuity_lost" | "session_expired" => "ui.shell_recovery.reason_restart",
            "process_identity_changed" | "group_continuity_lost" => {
                "ui.shell_recovery.reason_identity"
            }
            "cleanup_budget_exhausted" => "ui.shell_recovery.reason_budget",
            "initial_identity_unavailable"
            | "descendant_observation_failed"
            | "process_observation_failed"
            | "process_identity_capacity" => "ui.shell_recovery.reason_observation",
            "group_signal_failed"
            | "descendant_signal_failed"
            | "child_wait_failed"
            | "process_cleanup_unresolved"
            | "termination_failed" => "ui.shell_recovery.reason_process",
            "credential_cleanup_failed" => "ui.shell_recovery.reason_credentials",
            "cleanup_commit_unconfirmed" => "ui.shell_recovery.reason_storage",
            "output_incomplete" | "output_failed" | "output_pipes_unavailable" => {
                "ui.shell_recovery.reason_output"
            }
            _ => "ui.shell_recovery.reason_resource",
        };
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys.into_iter()
        .map(vega_i18n::t)
        .collect::<Vec<_>>()
        .join(" ")
}

fn button(
    id: &'static str,
    key: &'static str,
    enabled: bool,
    colors: vega_theme::ThemeColors,
    activate: impl Fn(&(), &mut Window, &mut App) + 'static,
) -> AnyElement {
    focused_button(id, key, None, enabled, colors, activate)
}

fn focused_button(
    id: &'static str,
    key: &'static str,
    focus: Option<&FocusHandle>,
    enabled: bool,
    colors: vega_theme::ThemeColors,
    activate: impl Fn(&(), &mut Window, &mut App) + 'static,
) -> AnyElement {
    let activate = std::rc::Rc::new(activate);
    let keyboard = activate.clone();
    div()
        .id(id)
        .aria_label(vega_i18n::t(key))
        .focusable()
        .when_some(focus, |button, focus| button.track_focus(focus))
        .tab_stop(enabled)
        .px_2()
        .py_1()
        .rounded_md()
        .text_color(if enabled {
            colors.text_secondary
        } else {
            colors.text_tertiary
        })
        .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, enabled)
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .focus_visible(move |style| style.border_1().border_color(colors.accent))
        })
        .on_click(move |_, window, cx| {
            if enabled {
                cx.stop_propagation();
                activate(&(), window, cx);
            }
        })
        .on_key_down(move |event, window, cx| {
            if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                keyboard(&(), window, cx);
            }
        })
        .child(vega_i18n::t(key))
        .into_any_element()
}
