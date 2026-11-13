use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, FocusHandle, Render, Window, actions};
use vega_conversation::agent::PendingUserQuestion;
use vega_conversation::types::{UserQuestionAnswer, UserQuestionAnswers, UserQuestionInput};
use vega_theme::{Layout, Typography, theme};

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelAction {
    Back,
    Skip,
    Advance,
    Note,
}

mod nsh_question;

pub(crate) struct UserQuestionPanel {
    projection_id: crate::nsh::ProjectionId,
    surface: Option<crate::nsh::Surface<nsh_question::Snapshot, nsh_question::Command>>,
    note_composing: bool,
    pending: PendingUserQuestion,
    input: UserQuestionInput,
    drafts: Vec<QuestionDraft>,
    page: usize,
    submitted: bool,
    error: Option<crate::localized_copy::LocalizedCopy>,
    composer_focus: FocusHandle,
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
        for _ in &input.questions {
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
        }
        Self {
            projection_id: crate::nsh::ProjectionId::new(),
            surface: None,
            note_composing: false,
            pending,
            input,
            drafts,
            page: 0,
            submitted: false,
            error: None,
            composer_focus,
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
        if self.submitted || self.pending.is_resolved() || self.note_composing {
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
            self.error = None;
            cx.notify();
            return;
        }
        let mut answers = UserQuestionAnswers::default();
        for (index, question) in self.input.questions.iter().enumerate() {
            let Some(answer) = self.answer(index, cx) else {
                self.page = index;
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
        if self.submitted || self.pending.is_resolved() || self.note_composing {
            return;
        }
        match action {
            PanelAction::Back => {
                self.page = self.page.saturating_sub(1);
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
            }
        }
        cx.notify();
    }
}

impl Render for UserQuestionPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_nsh_question(window, cx)
    }
}
