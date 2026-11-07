use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::PendingMessageBehavior;
use vega_theme::{Layout, ThemeColors, Typography};

use super::SettingsView;
use crate::nsh::{ProjectionId, Surface};

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    predictions: bool,
    running: String,
    enabled: bool,
    colors: ThemeColors,
}

#[derive(Clone, Copy)]
enum Action {
    Predictions,
    Running(PendingMessageBehavior),
}

struct Decision {
    observed: Snapshot,
    action: Action,
    reply: oneshot::Sender<bool>,
}

enum Request {
    Decide(Box<Decision>),
    Layout(f32),
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: Action) -> bool {
    current.enabled
        && match action {
            Action::Predictions => current.predictions == observed.predictions,
            Action::Running(_) => current.running == observed.running,
        }
}

impl SettingsView {
    fn input_preferences_snapshot(&self, cx: &App, id: ProjectionId) -> Snapshot {
        Snapshot {
            id,
            predictions: self.config.agent.composer_predictions_enabled,
            running: self.config.agent.running_message_behavior.clone(),
            enabled: !self.provider_management.saving && !self.default_permission.io_pending(),
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(super) fn render_input_preferences(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.input_preferences_presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) = Surface::new(
                self.input_preferences_snapshot(cx, id.clone()),
                render,
                window,
                cx,
            );
            self.input_preferences_presentation = Some(Presentation {
                surface,
                id,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted = this.section == 1
                                        && this
                                            .input_preferences_presentation
                                            .as_ref()
                                            .is_some_and(|page| {
                                                page.id.matches(&command.observed.id)
                                                    && accepts(
                                                        &this.input_preferences_snapshot(
                                                            cx,
                                                            page.id.clone(),
                                                        ),
                                                        &command.observed,
                                                        command.action,
                                                    )
                                            });
                                    if accepted {
                                        match command.action {
                                            Action::Predictions => {
                                                this.toggle_composer_predictions(cx)
                                            }
                                            Action::Running(value) => {
                                                this.select_running_message_behavior(value, cx)
                                            }
                                        }
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.input_preferences_presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            if let Some(page) = &this.input_preferences_presentation {
                                page.surface
                                    .publish(this.input_preferences_snapshot(cx, page.id.clone()));
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = &self.input_preferences_presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.input_preferences_snapshot(cx, page.id.clone());
        let Some(page) = &mut self.input_preferences_presentation else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
        page.surface.publish(snapshot);
        gpui_kit::div()
            .w_full()
            .flex_shrink_0()
            .h(gpui_kit::px(page.height))
            .child(page.surface.entity())
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
struct Actions {
    state: Signal<Snapshot>,
    pending: Signal<bool>,
    failed: Signal<bool>,
}

impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, action: Action) {
        let observed = self.state.get_untracked();
        if self.pending.get_untracked() || !observed.enabled {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        spawn(async move {
            let (reply, response) = oneshot::channel();
            if commands
                .send(Request::Decide(Box::new(Decision {
                    observed,
                    action,
                    reply,
                })))
                .await
                .is_err()
                || !matches!(response.await, Ok(true))
            {
                self.failed.set(true);
            }
            self.pending.set(false);
        });
    }
}

fn control(state: Signal<Snapshot>) -> nsh_core::element::View {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .border(px(1.))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                state.get().colors.accent
            } else {
                state.get().colors.border_subtle
            })
        })
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .px_3()
        .py_2()
}

fn running_button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    behavior: PendingMessageBehavior,
) -> AnyElement {
    let hint = match behavior {
        PendingMessageBehavior::Queue => "settings.defaults.queue_hint",
        PendingMessageBehavior::Steer => "settings.defaults.steer_hint",
    };
    let focused = signal(false);
    let hovered = signal(false);
    control(actions.state)
        .role(AccessibilityRole::Button)
        .on_focus(move |_: &()| focused.set(true))
        .on_blur(move |_: &()| focused.set(false))
        .tooltip(
            nsh_base::tooltip()
                .open(move || Some(focused.get() || hovered.get()))
                .on_open_change(move |event: &nsh_base::TooltipEvent| {
                    if let nsh_base::TooltipEvent::OpenChange(open, _) = event {
                        hovered.set(*open);
                    }
                })
                .p_2()
                .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                .bg(move || crate::nsh::color(actions.state.get().colors.bg_elevated))
                .text_color(move || crate::nsh::color(actions.state.get().colors.text_primary))
                .child(text(move || actions.state.with(|_| vega_i18n::t(hint)))),
        )
        .label(move || actions.state.with(|_| behavior.label()))
        .hint(move || actions.state.with(|_| vega_i18n::t(hint)))
        .accessibility_state(move || nsh_core::AccessibilityState {
            selected: actions.state.get().running == behavior.as_str(),
            ..Default::default()
        })
        .disabled(move || actions.pending.get() || !actions.state.get().enabled)
        .bg(move || {
            crate::nsh::color(if actions.state.get().running == behavior.as_str() {
                actions.state.get().colors.bg_active
            } else {
                actions.state.get().colors.bg_elevated
            })
        })
        .on_press(move |_: &nsh_core::PressEvent| {
            actions.send(commands.clone(), Action::Running(behavior))
        })
        .child(text(move || actions.state.with(|_| behavior.label())))
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
    };
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let predictions = commands.clone();
    view()
        .w_full()
        .flex_shrink_0()
        .gap_3()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .border(px(1.))
        .rounded(px(Layout::PANEL_RADIUS))
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
        .bg(move || crate::nsh::color(state.get().colors.bg_subtle))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .text_size(px(Typography::BODY))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (measured.get_untracked() - height).abs() > 0.5
            {
                measured.set(height);
                let commands = layout.clone();
                spawn(async move {
                    if commands.send(Request::Layout(height)).await.is_err() {
                        actions.failed.set(true);
                    }
                });
            }
        })
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    view()
                        .gap_2()
                        .child(text(move || {
                            state.with(|_| vega_i18n::t("settings.defaults.predictions"))
                        }))
                        .child(
                            text(move || {
                                state.with(|_| vega_i18n::t("settings.defaults.predictions_hint"))
                            })
                            .text_color(move || {
                                crate::nsh::color(state.get().colors.text_secondary)
                            }),
                        ),
                )
                .child(
                    control(state)
                        .role(AccessibilityRole::Switch)
                        .label(move || {
                            state.with(|_| vega_i18n::t("settings.defaults.predictions"))
                        })
                        .accessibility_state(move || nsh_core::AccessibilityState {
                            checked: Some(if state.get().predictions {
                                nsh_core::Checked::True
                            } else {
                                nsh_core::Checked::False
                            }),
                            ..Default::default()
                        })
                        .disabled(move || actions.pending.get() || !state.get().enabled)
                        .on_press(move |_: &nsh_core::PressEvent| {
                            actions.send(predictions.clone(), Action::Predictions)
                        })
                        .child(
                            view()
                                .relative()
                                .w(px(Layout::SETTINGS_SWITCH_WIDTH))
                                .h(px(Layout::SETTINGS_SWITCH_HEIGHT))
                                .rounded(px(Layout::SETTINGS_SWITCH_HEIGHT / 2.))
                                .bg(move || {
                                    crate::nsh::color(if state.get().predictions {
                                        state.get().colors.brand_primary
                                    } else {
                                        state.get().colors.bg_hover
                                    })
                                })
                                .child(
                                    view()
                                        .absolute()
                                        .top(px(2.))
                                        .left(move || {
                                            px(if state.get().predictions {
                                                Layout::SETTINGS_SWITCH_WIDTH
                                                    - Layout::ICON_SIZE
                                                    - 2.
                                            } else {
                                                2.
                                            })
                                        })
                                        .size(px(Layout::ICON_SIZE))
                                        .rounded(px(Layout::ICON_SIZE / 2.))
                                        .bg(move || {
                                            crate::nsh::color(state.get().colors.bg_elevated)
                                        }),
                                ),
                        ),
                ),
        )
        .child(text(move || {
            state.with(|_| vega_i18n::t("settings.defaults.running_message"))
        }))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(running_button(
                    actions,
                    commands.clone(),
                    PendingMessageBehavior::Queue,
                ))
                .child(running_button(
                    actions,
                    commands,
                    PendingMessageBehavior::Steer,
                )),
        )
        .child(when(
            move || actions.failed.get(),
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}

#[cfg(test)]
mod tests {
    use super::{Action, Snapshot, accepts};
    use vega_conversation::types::PendingMessageBehavior;

    fn snapshot() -> Snapshot {
        Snapshot {
            id: crate::nsh::ProjectionId::new(),
            predictions: true,
            running: "queue".into(),
            enabled: true,
            colors: vega_theme::Theme::light().colors,
        }
    }

    #[test]
    fn stale_prediction_toggle_cannot_flip_new_authoritative_preference() {
        let observed = snapshot();
        let mut current = observed.clone();
        current.predictions = false;
        assert!(!accepts(&current, &observed, Action::Predictions));
        assert!(accepts(&current, &current, Action::Predictions));
        current.enabled = false;
        assert!(!accepts(&current, &current, Action::Predictions));
    }

    #[test]
    fn running_message_choice_rechecks_current_value_and_save_availability() {
        let observed = snapshot();
        let mut current = observed.clone();
        current.running = "steer".into();
        assert!(!accepts(
            &current,
            &observed,
            Action::Running(PendingMessageBehavior::Queue)
        ));
        assert!(accepts(
            &current,
            &current,
            Action::Running(PendingMessageBehavior::Queue)
        ));
        current.enabled = false;
        assert!(!accepts(
            &current,
            &current,
            Action::Running(PendingMessageBehavior::Queue)
        ));
    }
}
