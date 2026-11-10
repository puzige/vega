use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_theme::{Layout, ThemeColors, Typography};

use super::{SettingsView, helpers::all_models};
use crate::nsh::{ProjectionId, Surface};

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    current: String,
    models: Vec<String>,
    open: bool,
    visible: bool,
    enabled: bool,
    colors: ThemeColors,
}

#[derive(Clone)]
enum Action {
    Toggle,
    Close,
    Select(String),
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

fn accepts(current: &Snapshot, observed: &Snapshot, action: &Action) -> bool {
    current.visible
        && current.open == observed.open
        && match action {
            Action::Close => current.open,
            Action::Toggle => current.enabled,
            Action::Select(model) => {
                current.enabled
                    && current.open
                    && current.current == observed.current
                    && current.models == observed.models
                    && current.models.contains(model)
            }
        }
}

impl SettingsView {
    fn default_model_snapshot(&self, cx: &App, id: ProjectionId) -> Snapshot {
        Snapshot {
            id,
            current: self.config.defaults.model.clone(),
            models: all_models(&self.config.providers),
            open: self.model_open,
            visible: self.section == 1
                && cx
                    .try_global::<super::SettingsOpen>()
                    .is_some_and(|open| open.0),
            enabled: !self.provider_management.saving && !self.default_permission.io_pending(),
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(super) fn sync_default_model(&self, cx: &App) {
        if let Some(page) = &self.default_model_presentation {
            page.surface
                .publish(self.default_model_snapshot(cx, page.id.clone()));
        }
    }

    pub(super) fn render_default_model(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.default_model_presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) = Surface::new(
                self.default_model_snapshot(cx, id.clone()),
                render,
                window,
                cx,
            );
            self.default_model_presentation = Some(Presentation {
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
                                        && this.default_model_presentation.as_ref().is_some_and(
                                            |page| {
                                                page.id.matches(&command.observed.id)
                                                    && accepts(
                                                        &this.default_model_snapshot(
                                                            cx,
                                                            page.id.clone(),
                                                        ),
                                                        &command.observed,
                                                        &command.action,
                                                    )
                                            },
                                        );
                                    if accepted {
                                        match command.action {
                                            Action::Toggle => this.model_open = !this.model_open,
                                            Action::Close => this.model_open = false,
                                            Action::Select(model) => this.select_model(&model, cx),
                                        }
                                        cx.notify();
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.default_model_presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            if let Some(page) = &this.default_model_presentation {
                                page.surface
                                    .publish(this.default_model_snapshot(cx, page.id.clone()));
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
        let Some(page) = &self.default_model_presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.default_model_snapshot(cx, page.id.clone());
        let Some(page) = &mut self.default_model_presentation else {
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
    trigger: ElementRef,
    restore_focus: Signal<bool>,
}

impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, action: Action) {
        let observed = self.state.get_untracked();
        if self.pending.get_untracked() || !accepts(&observed, &observed, &action) {
            return;
        }
        let restore_focus = !matches!(action, Action::Toggle);
        self.pending.set(true);
        self.failed.set(false);
        spawn(async move {
            let (reply, response) = oneshot::channel();
            let accepted = commands
                .send(Request::Decide(Box::new(Decision {
                    observed,
                    action,
                    reply,
                })))
                .await
                .is_ok()
                && matches!(response.await, Ok(true));
            self.failed.set(!accepted);
            self.pending.set(false);
            if accepted && restore_focus {
                self.restore_focus.set(true);
            }
        });
    }
}

fn control(state: Signal<Snapshot>, target: ElementRef) -> nsh_core::element::View {
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .px_3()
        .py_2()
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                state.get().colors.accent
            } else {
                state.get().colors.border_subtle
            })
        })
        .bg(move || crate::nsh::color(state.get().colors.bg_elevated))
}

fn icon(kind: crate::icons::Icon, state: Signal<Snapshot>) -> AnyElement {
    let source = crate::icons::nsh_source(kind).unwrap_or_default();
    svg(nsh_core::element::ImageSrc::svg_markup(&source))
        .size(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .tint_color(move || crate::nsh::color(state.get().colors.text_secondary).to_hsla())
        .into_element()
}

fn model_label(snapshot: &Snapshot) -> String {
    if snapshot.current.is_empty() {
        vega_i18n::t("settings.model.not_selected").to_string()
    } else {
        snapshot.current.clone()
    }
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
        trigger: element_ref(),
        restore_focus: signal(false),
    };
    effect(move || {
        let snapshot = state.get();
        if !snapshot.visible {
            actions.restore_focus.set(false);
        } else if actions.restore_focus.get()
            && !actions.pending.get()
            && snapshot.enabled
            && !snapshot.open
        {
            actions.restore_focus.set(false);
            actions.trigger.focus();
        }
    });
    let root = element_ref();
    let within = signal(false);
    root.on_focus_in(move || within.set(true));
    root.on_focus_out(move || within.set(false));
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let toggle = commands.clone();
    let escape = commands.clone();
    view()
        .element_ref(&root)
        .w_full()
        .flex_shrink_0()
        .gap_2()
        .text_size(px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
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
        .child(when(
            move || within.get() && state.get().open,
            move || {
                let commands = escape.clone();
                on_key_with(
                    "escape",
                    KeyOptions {
                        when: nsh_core::keymap::When::NotEditing,
                        ..Default::default()
                    },
                    move |_| actions.send(commands.clone(), Action::Close),
                );
                view()
            },
        ))
        .child(
            control(state, actions.trigger)
                .label(move || {
                    state.with(|snapshot| {
                        format!(
                            "{}: {}",
                            vega_i18n::t("settings.defaults.model"),
                            model_label(snapshot)
                        )
                    })
                })
                .accessibility_state(move || AccessibilityState {
                    expanded: Some(state.get().open),
                    ..Default::default()
                })
                .disabled(move || actions.pending.get() || !state.get().enabled)
                .flex_row()
                .justify_between()
                .gap_2()
                .on_press(move |_: &PressEvent| actions.send(toggle.clone(), Action::Toggle))
                .child(text(move || state.with(model_label)))
                .child(dynamic(move || {
                    icon(
                        if state.get().open {
                            crate::icons::Icon::ChevronDown
                        } else {
                            crate::icons::Icon::ChevronRight
                        },
                        state,
                    )
                })),
        )
        .child(when(
            move || state.get().open && state.get().enabled,
            move || {
                let commands = commands.clone();
                view()
                    .gap_1()
                    .child(when(
                        move || state.get().models.is_empty(),
                        move || {
                            text(move || {
                                state.with(|_| vega_i18n::t("settings.model.none_available"))
                            })
                            .text_color(move || {
                                crate::nsh::color(state.get().colors.text_secondary)
                            })
                        },
                    ))
                    .child(each(
                        move || state.get().models,
                        |model| model.clone(),
                        move |model, _| {
                            let commands = commands.clone();
                            control(state, element_ref())
                                .label(move || model.get())
                                .accessibility_state(move || AccessibilityState {
                                    selected: state.get().current == model.get(),
                                    ..Default::default()
                                })
                                .disabled(move || actions.pending.get() || !state.get().enabled)
                                .flex_row()
                                .justify_between()
                                .gap_2()
                                .bg(move || {
                                    crate::nsh::color(if state.get().current == model.get() {
                                        state.get().colors.bg_active
                                    } else {
                                        state.get().colors.bg_elevated
                                    })
                                })
                                .on_press(move |_: &PressEvent| {
                                    actions.send(commands.clone(), Action::Select(model.get()))
                                })
                                .child(text(move || model.get()))
                                .child(when(
                                    move || state.get().current == model.get(),
                                    move || icon(crate::icons::Icon::Check, state),
                                ))
                        },
                    ))
            },
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
