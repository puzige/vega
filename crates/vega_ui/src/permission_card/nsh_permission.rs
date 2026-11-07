use super::*;
use gpui_kit::{AnyElement as HostElement, Entity, Window, div, prelude::*, px as host_px};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_theme::{Layout, Typography};

pub(super) struct Presentation {
    surface: Surface<PermissionSnapshot, Request>,
    height: f32,
}

enum Request {
    Decide(PermissionCommand, oneshot::Sender<bool>),
    Layout(f32),
}

impl PermissionCard {
    pub(crate) fn render_card(
        card: Entity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> HostElement {
        card.update(cx, |this, cx| {
            this.sync_external_resolution(cx);
            let snapshot = this.nsh_snapshot(cx);
            if this.presentation.is_none() {
                let height = (snapshot.command.lines().count().max(1) + snapshot.details.len() + 8)
                    as f32
                    * crate::conversation_stream::ROW_HEIGHT;
                let (surface, mut commands) = Surface::new(snapshot.clone(), render, window, cx);
                this.presentation = Some(Presentation { surface, height });
                cx.spawn_in(window, async move |this, cx| {
                    while let Some(request) = commands.recv().await {
                        if this
                            .update_in(cx, |this, _, cx| match request {
                                Request::Decide(command, reply) => {
                                    let accepted = this.apply_nsh_command(command, cx);
                                    if let Some(presentation) = &this.presentation {
                                        presentation.surface.publish(this.nsh_snapshot(cx));
                                    }
                                    let _ = reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(presentation) = &mut this.presentation
                                        && (presentation.height - height).abs() > 0.5
                                    {
                                        presentation.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            let presentation = this.presentation.as_mut().map(|presentation| {
                presentation.surface.sync_environment(cx);
                presentation.surface.publish(snapshot);
                (presentation.surface.entity(), presentation.height)
            });
            div()
                .w_full()
                .flex_shrink_0()
                .pt_1()
                .pb_2()
                .children(
                    presentation
                        .map(|(view, height)| div().w_full().h(host_px(height)).child(view)),
                )
                .into_any_element()
        })
    }
}

#[derive(Clone, Copy)]
struct Decisions {
    state: Signal<PermissionSnapshot>,
    note: Signal<String>,
    composing: Signal<bool>,
    pending: Signal<bool>,
    failed: Signal<bool>,
}

impl Decisions {
    fn send(&self, commands: mpsc::Sender<Request>, action: PermissionAction, escape: bool) {
        let snapshot = self.state.get();
        if snapshot.resolved || self.composing.get_untracked() || self.pending.get_untracked() {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        let command = PermissionCommand {
            id: snapshot.id,
            action,
            note: self.note.get_untracked(),
            composing: false,
            escape,
        };
        let decisions = *self;
        spawn(async move {
            let (reply, response) = oneshot::channel();
            if commands
                .send(Request::Decide(command, reply))
                .await
                .is_err()
                || !matches!(response.await, Ok(true))
            {
                decisions.failed.set(true);
            }
            decisions.pending.set(false);
        });
    }
}

fn render(
    state: Signal<PermissionSnapshot>,
    commands: mpsc::Sender<Request>,
) -> nsh_core::prelude::AnyElement {
    let decisions = Decisions {
        state,
        note: signal(String::new()),
        composing: signal(false),
        pending: signal(false),
        failed: signal(false),
    };
    let root = element_ref();
    let within = signal(false);
    root.on_focus_in(move || within.set(true));
    root.on_focus_out(move || within.set(false));
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let escape = commands.clone();
    let always = commands.clone();
    let enter = commands.clone();
    let submit_note = commands.clone();
    let buttons = commands;
    view()
        .element_ref(&root)
        .w_full()
        .flex_shrink_0()
        .p_3()
        .gap_2()
        .rounded(px(Layout::STATUS_RADIUS))
        .border(px(1.))
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle).to_hsla())
        .text_color(move || crate::nsh::color(state.get().colors.text_primary).to_hsla())
        .text_size(px(Typography::SIDEBAR))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (measured.get_untracked() - height).abs() > 0.5
            {
                measured.set(height);
                let commands = layout.clone();
                spawn(async move {
                    if commands.send(Request::Layout(height)).await.is_err() {
                        decisions.failed.set(true);
                    }
                });
            }
        })
        .child(when(
            move || within.get() && !decisions.composing.get() && !state.get().resolved,
            move || {
                let commands = escape.clone();
                on_key_with(
                    "escape",
                    KeyOptions {
                        when: nsh_core::keymap::When::Always,
                        ..Default::default()
                    },
                    move |_| {
                        decisions.send(commands.clone(), PermissionAction::Deny, true);
                    },
                );
                view()
            },
        ))
        .child(when(
            move || {
                within.get()
                    && !decisions.composing.get()
                    && !state.get().resolved
                    && state
                        .get()
                        .decisions
                        .iter()
                        .any(|choice| choice.action == PermissionAction::Always)
            },
            move || {
                let commands = always.clone();
                on_key_with(
                    "mod+enter",
                    KeyOptions {
                        when: nsh_core::keymap::When::Always,
                        ..Default::default()
                    },
                    move |_| {
                        decisions.send(commands.clone(), PermissionAction::Always, false);
                    },
                );
                view()
            },
        ))
        .child(when(
            move || {
                within.get()
                    && !decisions.composing.get()
                    && !state.get().resolved
                    && state.get().enter_action.is_some()
            },
            move || {
                let commands = enter.clone();
                on_key_with(
                    "enter",
                    KeyOptions {
                        when: nsh_core::keymap::When::NotEditing,
                        ..Default::default()
                    },
                    move |_| {
                        if let Some(action) = state.get().enter_action {
                            decisions.send(commands.clone(), action, false);
                        }
                    },
                );
                view()
            },
        ))
        .child(
            text(move || state.get().title)
                .text_size(px(Typography::HEADING_CARD))
                .text_color(move || {
                    let snapshot = state.get();
                    crate::nsh::color(if snapshot.danger.is_some() {
                        snapshot.colors.danger
                    } else {
                        snapshot.colors.warning
                    })
                    .to_hsla()
                }),
        )
        .child(
            text(move || state.get().command)
                .font_family(crate::conversation_stream::MONOFONT)
                .text_size(px(Typography::CODE))
                .flex_shrink_0()
                .selectable(true),
        )
        .child(each(
            move || {
                state
                    .get()
                    .details
                    .into_iter()
                    .enumerate()
                    .collect::<Vec<_>>()
            },
            |(index, _)| *index,
            move |line, _| text(move || line.get().1).selectable(true),
        ))
        .child(when(
            move || state.get().danger.is_some(),
            move || {
                text(move || state.get().danger.unwrap_or_default())
                    .text_color(move || crate::nsh::color(state.get().colors.danger).to_hsla())
            },
        ))
        .child(when(
            move || state.get().persistent_warning.is_some(),
            move || text(move || state.get().persistent_warning.unwrap_or_default()),
        ))
        .child(when(
            move || state.get().note_allowed,
            move || {
                let submit_note = submit_note.clone();
                nsh_base::input()
                    .value(decisions.note)
                    .label(move || state.get().note_placeholder)
                    .placeholder(move || state.get().note_placeholder)
                    .readonly(move || state.get().resolved || decisions.pending.get())
                    .on_change(move |note: &str| decisions.note.set(note.to_owned()))
                    .on_composition_change(move |event: &Composition| {
                        decisions.composing.set(event.composing)
                    })
                    .on_submit(move |_: &str| {
                        if let Some(action) = state.get().enter_action {
                            decisions.send(submit_note.clone(), action, false);
                        }
                    })
            },
        ))
        .child(each(
            move || (0..state.get().decisions.len()).collect::<Vec<_>>(),
            |index| *index,
            move |index, _| {
                let index = index.get();
                let commands = buttons.clone();
                let target = element_ref();
                let mounted = signal(false);
                effect(move || {
                    if mounted.get() && index == state.get_untracked().initial_choice {
                        target.focus();
                    }
                });
                nsh_base::button()
                    .element_ref(&target)
                    .px_3()
                    .py_2()
                    .rounded(px(Layout::DISCLOSURE_RADIUS))
                    .border(px(1.))
                    .border_color(move || {
                        crate::nsh::color(state.get().colors.border_subtle).to_hsla()
                    })
                    .disabled(move || {
                        state.get().resolved || decisions.pending.get() || decisions.composing.get()
                    })
                    .on_layout(move |_: &Bounds<Points>| {
                        if !mounted.get_untracked() {
                            mounted.set(true);
                        }
                    })
                    .on_press(move || {
                        if let Some(choice) = state.get().decisions.get(index) {
                            decisions.send(commands.clone(), choice.action.clone(), false);
                        }
                    })
                    .child(text(move || {
                        state
                            .get()
                            .decisions
                            .get(index)
                            .map(|choice| choice.label.clone())
                            .unwrap_or_default()
                    }))
            },
        ))
        .child(when(
            move || decisions.failed.get(),
            move || {
                text(move || state.get().failed)
                    .text_color(move || crate::nsh::color(state.get().colors.danger).to_hsla())
            },
        ))
        .into_element()
}
