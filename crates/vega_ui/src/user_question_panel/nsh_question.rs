use super::*;
use crate::nsh::{OrderedCommands, ProjectionId, Surface};
use tokio::sync::mpsc;
use vega_conversation::types::UserQuestion;
use vega_theme::ThemeColors;

#[derive(Clone)]
pub(super) struct Snapshot {
    id: ProjectionId,
    page: usize,
    total: usize,
    question: UserQuestion,
    selected: Option<usize>,
    note_visible: bool,
    note: String,
    enabled: bool,
    can_advance: bool,
    error: Option<String>,
    colors: ThemeColors,
    other: String,
    recommended: String,
    note_label: String,
    placeholder: String,
    back: String,
    skip: String,
    primary: String,
    add_note: String,
    failed: String,
}

pub(super) struct Command {
    id: ProjectionId,
    page: usize,
    action: Action,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyboardTarget {
    None,
    Option,
    Control(PanelAction),
}

enum Action {
    Select(usize),
    Note(String),
    Compose(bool),
    Control(PanelAction),
    Escape,
}

impl UserQuestionPanel {
    fn question_snapshot(&self, cx: &App) -> Snapshot {
        let question = &self.input.questions[self.page];
        let draft = &self.drafts[self.page];
        let enabled = !self.submitted && !self.pending.is_resolved();
        Snapshot {
            id: self.projection_id.clone(),
            page: self.page,
            total: self.input.questions.len(),
            question: question.clone(),
            selected: draft.selected,
            note_visible: draft.note_visible,
            note: draft.note.read(cx).text().to_owned(),
            enabled,
            can_advance: enabled && !self.note_composing && self.answer(self.page, cx).is_some(),
            error: self.error.as_ref().map(|copy| copy.render()),
            colors: theme(cx).colors,
            other: vega_i18n::t("ui.other").into(),
            recommended: vega_i18n::t("ui.recommended").into(),
            note_label: vega_i18n::t(if draft.selected == Some(question.options.len()) {
                "ui.your_answer"
            } else {
                "ui.note"
            })
            .into(),
            placeholder: vega_i18n::t("ui.your_answer_or_note").into(),
            back: vega_i18n::t("ui.back_en").into(),
            skip: vega_i18n::t("ui.skip").into(),
            primary: vega_i18n::t(if self.page + 1 == self.input.questions.len() {
                "ui.submit"
            } else {
                "ui.next"
            })
            .into(),
            add_note: vega_i18n::t("ui.add_note").into(),
            failed: vega_i18n::t("ui.command_failed").into(),
        }
    }

    fn question_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if !self.projection_id.matches(&command.id)
            || command.page != self.page
            || self.submitted
            || self.pending.is_resolved()
        {
            return;
        }
        match command.action {
            Action::Select(index) if !self.note_composing => self.select(index, cx),
            Action::Select(_) => {}
            Action::Note(value) => {
                self.drafts[self.page]
                    .note
                    .update(cx, |note, cx| note.set_text(&value, cx));
                self.error = None;
            }
            Action::Compose(active) => self.note_composing = active,
            Action::Control(action) => self.act(action, window, cx),
            Action::Escape if !self.note_composing => self.composer_focus.focus(window, cx),
            Action::Escape => {}
        }
        if let Some(surface) = &self.surface {
            surface.publish(self.question_snapshot(cx));
        }
        cx.notify();
    }

    pub(super) fn render_nsh_question(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        if self.surface.is_none() {
            let (surface, mut commands) =
                Surface::new(self.question_snapshot(cx), render, window, cx);
            self.surface = Some(surface);
            cx.spawn_in(window, async move |this, cx| {
                while let Some(command) = commands.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            this.question_command(command, window, cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let snapshot = self.question_snapshot(cx);
        if let Some(surface) = &mut self.surface {
            surface.sync_environment(cx);
            surface.publish(snapshot);
            surface.entity().into_any_element()
        } else {
            div().into_any_element()
        }
    }
}

fn render(
    state: nsh_core::prelude::Signal<Snapshot>,
    sender: mpsc::Sender<Command>,
) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;
    let commands = OrderedCommands::new(sender);
    let failed = commands.failed;
    let composing = signal(false);
    let escape = commands.clone();
    let keyboard_target = signal(KeyboardTarget::None);
    let enter = commands.clone();
    view()
        .w_full()
        .max_w(px(Layout::COMPOSER_MAX_WIDTH))
        .p_3()
        .gap_2()
        .rounded(px(Layout::PANEL_RADIUS))
        .border(px(1.))
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle).to_hsla())
        .bg(move || crate::nsh::color(state.get().colors.bg_elevated).to_hsla())
        .text_color(move || crate::nsh::color(state.get().colors.text_primary).to_hsla())
        .text_size(px(Typography::SIDEBAR))
        .child(when(
            move || !composing.get(),
            move || {
                let escape = escape.clone();
                on_key_with(
                    "escape",
                    KeyOptions {
                        when: nsh_core::keymap::When::Always,
                        ..Default::default()
                    },
                    move |_| {
                        let snapshot = state.get();
                        escape.send(Command {
                            id: snapshot.id,
                            page: snapshot.page,
                            action: Action::Escape,
                        });
                    },
                );
                view()
            },
        ))
        .child(when(
            move || {
                !composing.get()
                    && state.get().enabled
                    && keyboard_target.get() != KeyboardTarget::None
            },
            move || {
                let enter = enter.clone();
                on_key_with(
                    "enter",
                    KeyOptions {
                        when: nsh_core::keymap::When::NotEditing,
                        ..Default::default()
                    },
                    move |_| {
                        let action = match keyboard_target.get() {
                            KeyboardTarget::Option => PanelAction::Advance,
                            KeyboardTarget::Control(action) => action,
                            KeyboardTarget::None => return,
                        };
                        let snapshot = state.get();
                        enter.send(Command {
                            id: snapshot.id,
                            page: snapshot.page,
                            action: Action::Control(action),
                        });
                    },
                );
                view()
            },
        ))
        .child(
            view()
                .flex_row()
                .justify_between()
                .child(text(move || state.get().question.header))
                .child(text(move || {
                    format!("{} / {}", state.get().page + 1, state.get().total)
                })),
        )
        .child(each(
            move || vec![state.get().page],
            |page| *page,
            move |page, _| {
                let page_index = page.get();
                let first = state.get();
                let page_id = first.id.clone();
                let page_commands = commands.clone();
                let options_commands = commands.clone();
                let note_commands = commands.clone();
                let back_commands = commands.clone();
                let skip_commands = commands.clone();
                let next_commands = commands.clone();
                let ids = (0..=first.question.options.len()).collect::<Vec<_>>();
                let focused = signal(first.selected.unwrap_or(0));
                for (key, delta) in [("up", -1isize), ("left", -1), ("down", 1), ("right", 1)] {
                    let count = ids.len();
                    on_key_with(
                        key,
                        KeyOptions {
                            when: nsh_core::keymap::When::NotEditing,
                            ..Default::default()
                        },
                        move |_| {
                            if keyboard_target.get() == KeyboardTarget::Option && !composing.get() {
                                focused.set(
                                    (focused.get() as isize + delta).rem_euclid(count as isize)
                                        as usize,
                                );
                            }
                        },
                    );
                }
                for (index, key) in ["1", "2", "3", "4", "5"]
                    .into_iter()
                    .enumerate()
                    .take(ids.len())
                {
                    let sender = commands.clone();
                    let id = page_id.clone();
                    on_key_with(
                        key,
                        KeyOptions {
                            when: nsh_core::keymap::When::NotEditing,
                            ..Default::default()
                        },
                        move |_| {
                            if !composing.get() && keyboard_target.get() == KeyboardTarget::Option {
                                focused.set(index);
                                sender.send(Command {
                                    id: id.clone(),
                                    page: page_index,
                                    action: Action::Select(index),
                                });
                            }
                        },
                    );
                }
                view()
                    .gap_2()
                    .child(
                        scroll()
                            .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
                            .child(
                                view()
                                    .gap_2()
                                    .child(
                                        text(move || state.get().question.question)
                                            .selectable(true),
                                    )
                                    .child(each(
                                        ids,
                                        |index| *index,
                                        move |index, _| {
                                            let index = index.get();
                                            let target = element_ref();
                                            let mounted = signal(false);
                                            let sender = options_commands.clone();
                                            let id = state.get().id;
                                            effect(move || {
                                                if mounted.get() && focused.get() == index {
                                                    target.focus();
                                                }
                                            });
                                            nsh_base::button()
                                                .element_ref(&target)
                                                .on_layout(move |_: &Bounds<Points>| {
                                                    mounted.set(true)
                                                })
                                                .on_focus(move |_: &()| {
                                                    keyboard_target.set(KeyboardTarget::Option);
                                                    focused.set(index);
                                                })
                                                .on_blur(move |_: &()| {
                                                    keyboard_target.set(KeyboardTarget::None)
                                                })
                                                .disabled(move || !state.get().enabled)
                                                .accessibility_state(move || AccessibilityState {
                                                    selected: state.get().selected == Some(index),
                                                    ..Default::default()
                                                })
                                                .px_3()
                                                .py_2()
                                                .rounded(px(Layout::DISCLOSURE_RADIUS))
                                                .border(px(1.))
                                                .border_color(move || {
                                                    crate::nsh::color(
                                                        if state.get().selected == Some(index) {
                                                            state.get().colors.accent
                                                        } else {
                                                            state.get().colors.border_subtle
                                                        },
                                                    )
                                                    .to_hsla()
                                                })
                                                .on_press(move || {
                                                    sender.send(Command {
                                                        id: id.clone(),
                                                        page: page_index,
                                                        action: Action::Select(index),
                                                    })
                                                })
                                                .child(text(move || {
                                                    let snapshot = state.get();
                                                    let label = snapshot
                                                        .question
                                                        .options
                                                        .get(index)
                                                        .map_or(
                                                            snapshot.other.as_str(),
                                                            |option| option.label.as_str(),
                                                        );
                                                    label
                                                        .strip_suffix(" (Recommended)")
                                                        .unwrap_or(label)
                                                        .to_owned()
                                                }))
                                                .child(when(
                                                    move || {
                                                        state
                                                            .get()
                                                            .question
                                                            .options
                                                            .get(index)
                                                            .is_some_and(|option| {
                                                                option
                                                                    .label
                                                                    .ends_with(" (Recommended)")
                                                            })
                                                    },
                                                    move || text(move || state.get().recommended),
                                                ))
                                                .child(text(move || {
                                                    state
                                                        .get()
                                                        .question
                                                        .options
                                                        .get(index)
                                                        .map(|option| option.description.clone())
                                                        .unwrap_or_default()
                                                }))
                                        },
                                    ))
                                    .child(when(
                                        move || state.get().note_visible,
                                        move || {
                                            let value = signal(state.get().note);
                                            let change = note_commands.clone();
                                            let compose = note_commands.clone();
                                            let id = state.get().id;
                                            let compose_id = id.clone();
                                            view()
                                                .gap_1()
                                                .child(text(move || state.get().note_label))
                                                .child(
                                                    input()
                                                        .value(value)
                                                        .on_focus(move |_: &()| {
                                                            keyboard_target
                                                                .set(KeyboardTarget::None)
                                                        })
                                                        .multiline(true)
                                                        .auto_focus(true)
                                                        .label(move || state.get().note_label)
                                                        .placeholder(move || {
                                                            state.get().placeholder
                                                        })
                                                        .readonly(move || !state.get().enabled)
                                                        .submit_behavior(
                                                            nsh_core::SubmitBehavior::Newline,
                                                        )
                                                        .on_change(move |text: &str| {
                                                            value.set(text.to_owned());
                                                            change.send(Command {
                                                                id: id.clone(),
                                                                page: page_index,
                                                                action: Action::Note(
                                                                    text.to_owned(),
                                                                ),
                                                            });
                                                        })
                                                        .on_composition_change(
                                                            move |event: &Composition| {
                                                                composing.set(event.composing);
                                                                compose.send(Command {
                                                                    id: compose_id.clone(),
                                                                    page: page_index,
                                                                    action: Action::Compose(
                                                                        event.composing,
                                                                    ),
                                                                });
                                                            },
                                                        ),
                                                )
                                        },
                                    ))
                                    .child(when(
                                        move || {
                                            !state.get().note_visible
                                                && state.get().selected.is_some()
                                        },
                                        move || {
                                            let sender = page_commands.clone();
                                            let id = state.get().id;
                                            nsh_base::button()
                                                .on_focus(move |_: &()| {
                                                    keyboard_target.set(KeyboardTarget::Control(
                                                        PanelAction::Note,
                                                    ))
                                                })
                                                .on_blur(move |_: &()| {
                                                    keyboard_target.set(KeyboardTarget::None)
                                                })
                                                .disabled(move || !state.get().enabled)
                                                .on_press(move || {
                                                    sender.send(Command {
                                                        id: id.clone(),
                                                        page: page_index,
                                                        action: Action::Control(PanelAction::Note),
                                                    })
                                                })
                                                .child(text(move || state.get().add_note))
                                        },
                                    )),
                            ),
                    )
                    .child(when(
                        move || state.get().error.is_some(),
                        move || {
                            text(move || state.get().error.unwrap_or_default()).text_color(
                                move || crate::nsh::color(state.get().colors.danger).to_hsla(),
                            )
                        },
                    ))
                    .child(
                        view()
                            .flex_row()
                            .justify_between()
                            .gap_2()
                            .child(
                                nsh_base::button()
                                    .on_focus(move |_: &()| {
                                        keyboard_target
                                            .set(KeyboardTarget::Control(PanelAction::Back))
                                    })
                                    .on_blur(move |_: &()| {
                                        keyboard_target.set(KeyboardTarget::None)
                                    })
                                    .disabled(move || !state.get().enabled || page_index == 0)
                                    .on_press(move || {
                                        let snapshot = state.get();
                                        back_commands.send(Command {
                                            id: snapshot.id,
                                            page: page_index,
                                            action: Action::Control(PanelAction::Back),
                                        });
                                    })
                                    .child(text(move || state.get().back)),
                            )
                            .child(
                                nsh_base::button()
                                    .on_focus(move |_: &()| {
                                        keyboard_target
                                            .set(KeyboardTarget::Control(PanelAction::Skip))
                                    })
                                    .on_blur(move |_: &()| {
                                        keyboard_target.set(KeyboardTarget::None)
                                    })
                                    .disabled(move || !state.get().enabled)
                                    .on_press(move || {
                                        let snapshot = state.get();
                                        skip_commands.send(Command {
                                            id: snapshot.id,
                                            page: page_index,
                                            action: Action::Control(PanelAction::Skip),
                                        });
                                    })
                                    .child(text(move || state.get().skip)),
                            )
                            .child(
                                nsh_base::button()
                                    .on_focus(move |_: &()| {
                                        keyboard_target
                                            .set(KeyboardTarget::Control(PanelAction::Advance))
                                    })
                                    .on_blur(move |_: &()| {
                                        keyboard_target.set(KeyboardTarget::None)
                                    })
                                    .disabled(move || !state.get().can_advance || composing.get())
                                    .on_press(move || {
                                        let snapshot = state.get();
                                        next_commands.send(Command {
                                            id: snapshot.id,
                                            page: page_index,
                                            action: Action::Control(PanelAction::Advance),
                                        });
                                    })
                                    .child(text(move || state.get().primary)),
                            ),
                    )
            },
        ))
        .child(when(
            move || failed.get(),
            move || text(move || state.get().failed),
        ))
        .into_element()
}
