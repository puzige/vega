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
    epoch: u64,
    focused: Option<Action>,
    _focus_events: Vec<gpui_kit::Subscription>,
    keyboard_modality: bool,
}

impl Presentation {
    pub(super) fn revoke(&mut self) {
        self.id = ProjectionId::new();
        self.epoch = self.epoch.wrapping_add(1);
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    epoch: u64,
    focused: Option<Action>,
    predictions: bool,
    running: String,
    enabled: bool,
    colors: ThemeColors,
}

#[derive(Clone, Copy, PartialEq, Eq)]
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
        && current.epoch == observed.epoch
        && match action {
            Action::Predictions => current.predictions == observed.predictions,
            Action::Running(_) => current.running == observed.running,
        }
}

impl SettingsView {
    fn input_preferences_snapshot(&self, cx: &App, id: ProjectionId) -> Snapshot {
        Snapshot {
            id,
            epoch: self
                .input_preferences_presentation
                .as_ref()
                .map_or(0, |p| p.epoch),
            focused: self
                .input_preferences_presentation
                .as_ref()
                .and_then(|p| p.focused.filter(|_| p.keyboard_modality)),
            predictions: self.config.agent.composer_predictions_enabled,
            running: self.config.agent.running_message_behavior.clone(),
            enabled: self.section == 1
                && !self.palette_open
                && self.pending_section.is_none()
                && !self.pending_settings_close
                && cx
                    .try_global::<super::SettingsOpen>()
                    .is_some_and(|open| open.0)
                && !self.provider_management.saving
                && !self.default_permission.io_pending(),
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
            let (surface, mut requests) = Surface::new_transparent(
                self.input_preferences_snapshot(cx, id.clone()),
                render,
                window,
                cx,
            );
            let mut focus_events = Vec::new();
            for action in [
                Action::Predictions,
                Action::Running(PendingMessageBehavior::Queue),
                Action::Running(PendingMessageBehavior::Steer),
            ] {
                use super::nsh_control::{FocusTarget, Semantics};
                let target = FocusTarget::new(cx);
                let owner = cx.weak_entity();
                target.register(
                    surface.entity().read(cx),
                    action.name(),
                    if action == Action::Predictions {
                        Semantics::Switch
                    } else {
                        Semantics::Selection
                    },
                    move |displayed, cx| {
                        let Some(owner) = owner.upgrade() else {
                            return false;
                        };
                        let settings = owner.read(cx);
                        let Some(page) = &settings.input_preferences_presentation else {
                            return false;
                        };
                        let current = settings.input_preferences_snapshot(cx, page.id.clone());
                        current.enabled && *displayed == stamp(&current)
                    },
                );
                focus_events.push(cx.on_focus(&target.handle, window, move |settings, _, cx| {
                    if let Some(page) = &mut settings.input_preferences_presentation {
                        page.focused = Some(action);
                        cx.notify();
                    }
                }));
                focus_events.push(cx.on_blur(&target.handle, window, move |settings, _, cx| {
                    if let Some(page) = &mut settings.input_preferences_presentation
                        && page.focused == Some(action)
                    {
                        page.focused = None;
                        cx.notify();
                    }
                }));
            }
            self.input_preferences_presentation = Some(Presentation {
                focused: None,
                epoch: 0,
                _focus_events: focus_events,
                keyboard_modality: window.last_input_was_keyboard(),
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
                                    let accepted = cx
                                        .try_global::<super::SettingsOpen>()
                                        .is_some_and(|open| open.0)
                                        && this.section == 1
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
                                        if let Some(page) = &mut this.input_preferences_presentation
                                        {
                                            page.revoke();
                                        }
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
        if let Some(page) = &mut self.input_preferences_presentation {
            page.keyboard_modality = window.last_input_was_keyboard();
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

fn stamp(s: &Snapshot) -> serde_json::Value {
    serde_json::json!([s.epoch, s.enabled, s.predictions, s.running])
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Predictions => "input-predictions",
            Self::Running(behavior) => behavior.as_str(),
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Predictions => "settings.defaults.predictions_hint",
            Self::Running(PendingMessageBehavior::Queue) => "settings.defaults.queue_hint",
            Self::Running(PendingMessageBehavior::Steer) => "settings.defaults.steer_hint",
        }
    }
}

fn target(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: Action,
    hovered: Option<Signal<Option<u64>>>,
) -> AnyElement {
    let state = actions.state;
    let hover_timer = std::rc::Rc::new(std::cell::RefCell::new(
        None::<nsh_core::element::Spawned<()>>,
    ));
    if let Some(hovered) = hovered {
        let timer = hover_timer.clone();
        effect(move || {
            if !state.get().enabled || actions.pending.get() {
                if let Some(task) = timer.take() {
                    task.cancel();
                }
                hovered.set(None);
            }
        });
    }
    native(action.name())
        .absolute()
        .inset_0()
        .prop("report-hover", serde_json::Value::from(hovered.is_some()))
        .prop("state", move || state.with(stamp))
        .prop("label", move || {
            serde_json::Value::from(state.with(|s| match action {
                Action::Predictions => vega_i18n::t("settings.defaults.predictions").to_owned(),
                Action::Running(behavior) if s.running == behavior.as_str() => vega_i18n::t_args(
                    "settings.selection.selected",
                    &[("label", behavior.label())],
                ),
                Action::Running(behavior) => behavior.label().to_owned(),
            }))
        })
        .prop("hint", move || {
            serde_json::Value::from(state.with(|_| vega_i18n::t(action.hint())))
        })
        .prop("selected", move || {
            serde_json::Value::from(state.with(|s| match action {
                Action::Predictions => s.predictions,
                Action::Running(behavior) => s.running == behavior.as_str(),
            }))
        })
        .prop("enabled", move || {
            serde_json::Value::from(state.get().enabled && !actions.pending.get())
        })
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "hover"
                && let Some(hovered) = hovered
            {
                let current = state.get_untracked();
                if event.data.get(0) != Some(&stamp(&current)) {
                    return;
                }
                if let Some(timer) = hover_timer.take() {
                    timer.cancel();
                }
                hovered.set(None);
                if event.data.get(1).and_then(serde_json::Value::as_bool) == Some(true)
                    && current.enabled
                    && !actions.pending.get_untracked()
                {
                    let epoch = current.epoch;
                    hover_timer.replace(Some(set_timeout(
                        nsh_core::overlay::HINT_DELAY,
                        move || {
                            let live = state.get_untracked();
                            if live.enabled
                                && live.epoch == epoch
                                && !actions.pending.get_untracked()
                            {
                                hovered.set(Some(epoch));
                            }
                        },
                    )));
                }
            }

            if &*event.name == "activate" && event.data == stamp(&state.get_untracked()) {
                actions.send(commands.clone(), action);
            }
        })
        .into_element()
}

fn control(state: Signal<Snapshot>, action: Action) -> nsh_core::element::View {
    view()
        .relative()
        .accessible(false)
        .border(px(1.))
        .border_color(move || {
            crate::nsh::color(if state.get().focused == Some(action) {
                state.get().colors.accent
            } else {
                state.get().colors.border_subtle
            })
        })
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
}

fn running_button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    behavior: PendingMessageBehavior,
) -> AnyElement {
    let action = Action::Running(behavior);
    let hint = action.hint();
    let hovered = signal(None);
    control(actions.state, action)
        .self_start()
        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
        .py_1()
        .tooltip(
            nsh_base::tooltip()
                .open(move || {
                    Some(
                        actions.state.get().enabled
                            && (actions.state.get().focused == Some(action)
                                || hovered.get() == Some(actions.state.get().epoch)),
                    )
                })
                .p_2()
                .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                .bg(move || crate::nsh::color(actions.state.get().colors.bg_elevated))
                .text_color(move || crate::nsh::color(actions.state.get().colors.text_primary))
                .child(text(move || actions.state.with(|_| vega_i18n::t(hint)))),
        )
        .bg(move || {
            crate::nsh::color(if actions.state.get().running == behavior.as_str() {
                actions.state.get().colors.bg_active
            } else {
                actions.state.get().colors.bg_elevated
            })
        })
        .flex_row()
        .items_center()
        .gap_2()
        .child(text(move || actions.state.with(|_| behavior.label())).accessible(false))
        .child(view().size(px(Layout::ICON_SIZE)).child(when(
            move || actions.state.get().running == behavior.as_str(),
            move || {
                svg(nsh_core::element::ImageSrc::svg_markup(
                    &crate::icons::nsh_source(crate::icons::Icon::Check).unwrap_or_default(),
                ))
                .accessible(false)
                .size(px(Layout::ICON_SIZE))
                .text_color(move || crate::nsh::color(actions.state.get().colors.text_primary))
            },
        )))
        .child(target(actions, commands, action, Some(hovered)))
        .into_element()
}

fn field_row(
    state: Signal<Snapshot>,
    key: &'static str,
    description: Option<&'static str>,
    control: AnyElement,
) -> AnyElement {
    super::nsh_form::field(
        move || state.get().colors,
        text(move || state.with(|_| vega_i18n::t(key))),
        description.map(|key| text(move || state.with(|_| vega_i18n::t(key))).into_element()),
        control,
    )
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
        .gap_0()
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .text_size(px(Typography::BODY))
        .line_height(px((Typography::BODY * Typography::BODY_LINE_HEIGHT).round()))
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
        .child(field_row(
            state,
            "settings.defaults.predictions",
            Some("settings.defaults.predictions_hint"),
            control(state, Action::Predictions)
                .self_start()
                .rounded(px(Layout::SETTINGS_SWITCH_HEIGHT / 2.))
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
                                        Layout::SETTINGS_SWITCH_WIDTH - Layout::ICON_SIZE - 2.
                                    } else {
                                        2.
                                    })
                                })
                                .size(px(Layout::ICON_SIZE))
                                .rounded(px(Layout::ICON_SIZE / 2.))
                                .bg(move || crate::nsh::color(state.get().colors.bg_elevated)),
                        ),
                )
                .child(target(actions, predictions, Action::Predictions, None))
                .into_element(),
        ))
        .child(field_row(
            state,
            "settings.defaults.running_message",
            None,
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
                ))
                .into_element(),
        ))
        .child(when(
            move || actions.failed.get(),
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}
