use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, KeyDownEvent, MouseButton, Render,
    Role, ScrollHandle, Window, actions, div, px,
};
use vega_conversation::agent::PendingUserQuestion;
use vega_conversation::types::{UserQuestionAnswer, UserQuestionAnswers, UserQuestionInput};
use vega_theme::{Layout, Typography, theme};

use crate::icons::{Icon, icon};
use crate::interaction::InteractionExt;
use crate::text_input::TextInput;

actions!(
    vega_user_question,
    [
        QuestionAdvance,
        QuestionActivate,
        QuestionEscape,
        QuestionNoteNewline,
        QuestionNextFocus,
        QuestionPreviousFocus
    ]
);

struct QuestionDraft {
    selected: Option<usize>,
    skipped: bool,
    note_visible: bool,
    note: Entity<TextInput>,
}

#[derive(Clone, Copy)]
enum PanelAction {
    Back,
    Skip,
    Advance,
    Note,
}

pub(crate) struct UserQuestionPanel {
    pending: PendingUserQuestion,
    input: UserQuestionInput,
    drafts: Vec<QuestionDraft>,
    page: usize,
    submitted: bool,
    error: Option<crate::localized_copy::LocalizedCopy>,
    option_focus: Vec<Vec<FocusHandle>>,
    controls: [FocusHandle; 4],
    composer_focus: FocusHandle,
    scroll: ScrollHandle,
}

impl UserQuestionPanel {
    pub(crate) fn resolution(&self) -> vega_conversation::types::UserQuestionResolution {
        self.pending.resolution()
    }

    pub(crate) fn new(
        pending: PendingUserQuestion,
        composer_focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = pending.request().input.clone();
        let mut drafts = Vec::new();
        let mut option_focus = Vec::new();
        for question in &input.questions {
            let note = cx.new(|cx| {
                TextInput::new_multiline(cx, "", 2)
                    .with_placeholder_key("ui.your_answer_or_note")
                    .with_tab_stop(true)
            });
            cx.observe(&note, |this, _, cx| {
                this.error = None;
                cx.notify();
            })
            .detach();
            drafts.push(QuestionDraft {
                selected: None,
                skipped: false,
                note_visible: false,
                note,
            });
            option_focus.push(
                (0..=question.options.len())
                    .map(|_| cx.focus_handle().tab_stop(true))
                    .collect(),
            );
        }
        Self {
            pending,
            input,
            drafts,
            page: 0,
            submitted: false,
            error: None,
            option_focus,
            controls: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            composer_focus,
            scroll: ScrollHandle::new(),
        }
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.submitted
            || self.pending.is_resolved()
            || index > self.input.questions[self.page].options.len()
        {
            return;
        }
        let draft = &mut self.drafts[self.page];
        draft.selected = Some(index);
        draft.skipped = false;
        if index == self.input.questions[self.page].options.len() {
            draft.note_visible = true;
        }
        self.error = None;
        cx.notify();
    }

    fn answer(&self, page: usize, cx: &App) -> Option<UserQuestionAnswer> {
        let draft = &self.drafts[page];
        if draft.skipped {
            return Some(UserQuestionAnswer::default());
        }
        let selected = draft.selected?;
        let note = draft.note.read(cx).text();
        let mut answers = Vec::new();
        if let Some(option) = self.input.questions[page].options.get(selected) {
            answers.push(option.label.clone());
        } else if note.trim().is_empty() {
            return None;
        }
        if draft.note_visible && !note.trim().is_empty() {
            answers.push(format!("user_note: {note}"));
        }
        Some(UserQuestionAnswer { answers })
    }

    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitted || self.pending.is_resolved() {
            return;
        }
        if self.answer(self.page, cx).is_none() {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.choose_an_option_or_enter_an_answer",
            ));
            cx.notify();
            return;
        }
        if self.page + 1 < self.input.questions.len() {
            self.page += 1;
            self.focus_current_option(window, cx);
            self.error = None;
            self.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
            cx.notify();
            return;
        }
        let mut answers = UserQuestionAnswers::default();
        for (index, question) in self.input.questions.iter().enumerate() {
            let Some(answer) = self.answer(index, cx) else {
                self.page = index;
                self.focus_current_option(window, cx);
                self.error = Some(crate::localized_copy::LocalizedCopy::new(
                    "ui.answer_or_skip_this_question_before_submitting",
                ));
                cx.notify();
                return;
            };
            answers.answers.insert(question.id.clone(), answer);
        }
        match self.pending.respond(answers) {
            Ok(true) => {
                self.submitted = true;
                self.error = None;
                self.composer_focus.focus(window, cx);
            }
            Ok(false) => {
                self.submitted = true;
                self.error = Some(crate::localized_copy::LocalizedCopy::new(
                    "ui.question_interrupted",
                ));
            }
            Err(_) => {
                self.error = Some(crate::localized_copy::LocalizedCopy::new(
                    "ui.answer_could_not_be_submitted_check_its_length",
                ))
            }
        }
        cx.notify();
    }

    fn act(&mut self, action: PanelAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitted || self.pending.is_resolved() {
            return;
        }
        match action {
            PanelAction::Back => {
                self.page = self.page.saturating_sub(1);
                self.focus_current_option(window, cx);
                self.error = None;
            }
            PanelAction::Skip => {
                self.drafts[self.page].skipped = true;
                self.drafts[self.page].selected = None;
                self.drafts[self.page].note_visible = false;
                self.advance(window, cx);
            }
            PanelAction::Advance => self.advance(window, cx),
            PanelAction::Note => {
                self.drafts[self.page].note_visible = true;
                self.drafts[self.page]
                    .note
                    .read(cx)
                    .focus_handle(cx)
                    .focus(window, cx);
            }
        }
        cx.notify();
    }

    fn focus_current_option(&self, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.drafts[self.page].selected.unwrap_or(0);
        self.option_focus[self.page][index].focus(window, cx);
    }

    fn option_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitted
            || self.pending.is_resolved()
            || event.keystroke.modifiers != gpui_kit::Modifiers::default()
        {
            return;
        }
        let focuses = &self.option_focus[self.page];
        let Some(current) = focuses.iter().position(|focus| focus.is_focused(window)) else {
            return;
        };
        let key = event.keystroke.key.as_str();
        match key {
            "up" | "left" | "down" | "right" => {
                let next = if matches!(key, "up" | "left") {
                    (current + focuses.len() - 1) % focuses.len()
                } else {
                    (current + 1) % focuses.len()
                };
                focuses[next].focus(window, cx);
                cx.stop_propagation();
            }
            "1" | "2" | "3" | "4" | "5" => {
                if let Ok(number) = key.parse::<usize>()
                    && number <= focuses.len()
                {
                    focuses[number - 1].focus(window, cx);
                    self.select(number - 1, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn button(
        &self,
        label: &'static str,
        action: PanelAction,
        index: usize,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let button = div()
            .id(("question-action", index))
            .role(Role::Button)
            .aria_label(label)
            .track_focus(&self.controls[index])
            .key_context("UserQuestionControl")
            .h(px(Layout::TITLEBAR_CONTROL_SIZE))
            .min_w(px(Layout::TITLEBAR_CONTROL_SIZE))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(Typography::SIDEBAR))
            .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
            .px_3()
            .rounded(px(Layout::TITLEBAR_CONTROL_SIZE / 2.0))
            .border_1()
            .border_color(colors.bg_elevated)
            .text_color(if enabled {
                colors.text_primary
            } else {
                colors.text_tertiary
            })
            .interactive(colors, enabled, false)
            .focus_visible(|style| style.border_color(colors.accent))
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| this.act(action, window, cx)),
                    )
                    .on_action(cx.listener(move |this, _: &QuestionActivate, window, cx| {
                        this.act(action, window, cx)
                    }))
            })
            .child(label);
        button.into_any_element()
    }
}

impl Render for UserQuestionPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let question = &self.input.questions[self.page];
        let draft = &self.drafts[self.page];
        let enabled = !self.submitted && !self.pending.is_resolved();
        let mut options = div().flex().flex_col().gap_1();
        for index in 0..=question.options.len() {
            let option = question.options.get(index);
            let source_label =
                option.map_or(vega_i18n::t("ui.other"), |option| option.label.as_str());
            let recommended = source_label.ends_with(" (Recommended)");
            let label = source_label
                .strip_suffix(" (Recommended)")
                .unwrap_or(source_label)
                .to_string();
            let selected = draft.selected == Some(index);
            let row = div()
                .id(("question-option", index))
                .role(Role::Button)
                .aria_label(if selected {
                    vega_i18n::t_args(
                        "ui.selected_source_label",
                        &[("source_label", source_label)],
                    )
                } else {
                    source_label.to_string()
                })
                .track_focus(&self.option_focus[self.page][index])
                .key_context("UserQuestionOptions")
                .w_full()
                .min_w_0()
                .px_3()
                .py_2()
                .rounded(px(Layout::DISCLOSURE_RADIUS))
                .border_1()
                .border_color(colors.bg_elevated)
                .interactive(colors, enabled, selected)
                .focus_visible(|style| style.border_color(colors.accent))
                .when(enabled, |row| {
                    row.cursor_pointer()
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.option_focus[this.page][index].focus(window, cx);
                                this.select(index, cx);
                            }),
                        )
                        .on_key_down(cx.listener(Self::option_key))
                        .on_action(cx.listener(move |this, _: &QuestionActivate, _, cx| {
                            this.select(index, cx)
                        }))
                })
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .items_start()
                        .child(
                            div()
                                .w(px(Layout::ICON_SIZE))
                                .flex_shrink_0()
                                .when(selected, |marker| {
                                    marker.child(icon(Icon::Check, colors.accent))
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().flex().flex_wrap().gap_2().child(label).when(
                                    recommended,
                                    |row| {
                                        row.child(
                                            div()
                                                .text_size(px(Typography::METADATA))
                                                .text_color(colors.text_secondary)
                                                .child(vega_i18n::t("ui.recommended")),
                                        )
                                    },
                                ))
                                .when_some(
                                    option.filter(|option| !option.description.is_empty()),
                                    |row, option| {
                                        row.child(
                                            div()
                                                .text_color(colors.text_secondary)
                                                .child(option.description.clone()),
                                        )
                                    },
                                ),
                        ),
                );
            options = options.child(row);
        }
        let note_visible = draft.note_visible;
        let note = draft.note.clone();
        let note_text = note.read(cx).text().to_string();
        let custom = draft.selected == Some(question.options.len());
        let can_advance = enabled && self.answer(self.page, cx).is_some();
        let primary = if self.page + 1 == self.input.questions.len() {
            vega_i18n::t("ui.submit")
        } else {
            vega_i18n::t("ui.next")
        };
        let content = div()
            .id("question-content")
            .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(crate::tool_card::contain_disclosure_scroll(&self.scroll))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(Typography::BODY))
                    .child(question.question.clone()),
            )
            .child(options)
            .when(note_visible, |content| {
                content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .key_context("UserQuestionNote")
                        .child(if custom {
                            vega_i18n::t("ui.your_answer")
                        } else {
                            vega_i18n::t("ui.note")
                        })
                        .when(enabled, |content| content.child(note))
                        .when(!enabled, |content| content.child(note_text)),
                )
            })
            .when(!note_visible && draft.selected.is_some(), |content| {
                content.child(self.button(
                    vega_i18n::t("ui.add_note"),
                    PanelAction::Note,
                    3,
                    enabled,
                    cx,
                ))
            })
            .tracked_scroll_edge_fade(
                self.scroll.clone(),
                colors.bg_elevated,
                ScrollFade::compact().vertical_gutter(px(Layout::DISCLOSURE_SCROLLBAR_GUTTER)),
            );
        div()
            .id("user-question-panel")
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .mb_2()
            .p_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.text_primary)
            .key_context("UserQuestionPanel")
            .flex()
            .flex_col()
            .gap_2()
            .on_action(
                cx.listener(|this, _: &QuestionAdvance, window, cx| this.advance(window, cx)),
            )
            .on_action(cx.listener(|this, _: &QuestionEscape, window, cx| {
                if !this.drafts[this.page].note.read(cx).is_composing() {
                    this.composer_focus.focus(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &QuestionNoteNewline, window, cx| {
                let note = &this.drafts[this.page].note;
                if !this.submitted && !this.pending.is_resolved() && !note.read(cx).is_composing() {
                    note.update(cx, |note, cx| {
                        note.insert_newline(&crate::text_input::InsertNewline, window, cx)
                    });
                }
            }))
            .on_action(|_: &QuestionNextFocus, window, cx| window.focus_next(cx))
            .on_action(|_: &QuestionPreviousFocus, window, cx| window.focus_prev(cx))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .gap_2()
                    .child(question.header.clone())
                    .child(div().text_color(colors.text_secondary).child(format!(
                        "{} / {}",
                        self.page + 1,
                        self.input.questions.len()
                    ))),
            )
            .child(content)
            .children(
                self.error
                    .as_ref()
                    .map(|error| div().text_color(colors.danger).child(error.render())),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(self.button(
                        vega_i18n::t("ui.back_en"),
                        PanelAction::Back,
                        0,
                        enabled && self.page > 0,
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.button(
                                vega_i18n::t("ui.skip"),
                                PanelAction::Skip,
                                1,
                                enabled,
                                cx,
                            ))
                            .child(self.button(primary, PanelAction::Advance, 2, can_advance, cx)),
                    ),
            )
    }
}
