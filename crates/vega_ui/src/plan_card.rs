//! Typed Plan review card. It emits commands upward and never reads SQLite.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, MouseButton, Window, actions, div,
    px,
};
use vega_conversation::types::{Plan, PlanReviewAction, PlanStatus};
use vega_theme::{Typography, theme};

use crate::conversation_stream::{MONOFONT, ROW_HEIGHT};

actions!(vega_plan_card, [PlanActivate, PlanNext, PlanPrevious]);

/// One user request emitted by a pending card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanReviewRequested {
    pub thread_id: String,
    pub plan_id: String,
    pub action: PlanReviewAction,
}

/// Persisted Plan projection plus local duplicate-submit suppression.
pub struct PlanCard {
    plan: Plan,
    generated_notices: Vec<crate::conversation_stream::generated_notice::GeneratedNoticeSpan>,
    submitted: bool,
    feedback: Option<crate::localized_copy::LocalizedCopy>,
    focus: [FocusHandle; 3],
    focus_index: usize,
}

impl EventEmitter<PlanReviewRequested> for PlanCard {}

impl PlanCard {
    pub fn new(plan: Plan, cx: &mut Context<Self>) -> Self {
        Self {
            plan,
            generated_notices: Vec::new(),
            submitted: false,
            feedback: None,
            focus: [
                cx.focus_handle().tab_index(1).tab_stop(true),
                cx.focus_handle().tab_index(2).tab_stop(true),
                cx.focus_handle().tab_index(3).tab_stop(true),
            ],
            focus_index: 0,
        }
    }

    pub fn status(&self) -> PlanStatus {
        self.plan.status
    }

    pub(crate) fn set_generated_notices(
        &mut self,
        notices: Vec<crate::conversation_stream::generated_notice::GeneratedNoticeSpan>,
        cx: &mut Context<Self>,
    ) {
        self.generated_notices = notices;
        cx.notify();
    }

    fn display_content(&self) -> String {
        let notices = self
            .generated_notices
            .iter()
            .filter(|span| span.is_valid_for(&self.plan.content))
            .filter_map(|span| span.fragments(0..self.plan.content.len()))
            .collect::<Vec<_>>();
        crate::conversation_stream::generated_notice::project_text(&self.plan.content, &notices)
    }

    pub fn apply_persisted(&mut self, plan: Plan, cx: &mut Context<Self>) {
        if plan.id != self.plan.id || plan.thread_id != self.plan.thread_id {
            self.feedback = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.plan_state_corrupted",
            ));
            self.submitted = true;
        } else {
            self.plan = plan;
            self.submitted = false;
            self.feedback = None;
        }
        cx.notify();
    }

    pub fn apply_stale(&mut self, cx: &mut Context<Self>) {
        self.feedback = Some(crate::localized_copy::LocalizedCopy::new(
            "ui.this_plan_has_already_been_handled_or_replaced",
        ));
        self.submitted = true;
        cx.notify();
    }

    pub fn row_count(&self) -> usize {
        let content = self.display_content().lines().count().max(1);
        1 + content + 1 + usize::from(self.feedback.is_some())
    }

    fn request(&mut self, action: PlanReviewAction, cx: &mut Context<Self>) {
        if self.plan.status != PlanStatus::Pending || self.submitted {
            return;
        }
        self.submitted = true;
        cx.emit(PlanReviewRequested {
            thread_id: self.plan.thread_id.clone(),
            plan_id: self.plan.id.clone(),
            action,
        });
        cx.notify();
    }

    fn approve(&mut self, cx: &mut Context<Self>) {
        self.request(PlanReviewAction::Approve, cx);
    }

    fn changes(&mut self, cx: &mut Context<Self>) {
        self.request(PlanReviewAction::RequestChanges { note: None }, cx);
    }

    fn abandon(&mut self, cx: &mut Context<Self>) {
        self.request(PlanReviewAction::Abandon { note: None }, cx);
    }

    fn activate(&mut self, _: &PlanActivate, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self
            .focus
            .iter()
            .position(|handle| handle.is_focused(window))
            .unwrap_or(self.focus_index);
        match focused {
            0 => self.approve(cx),
            1 => self.changes(cx),
            _ => self.abandon(cx),
        }
    }

    fn next(&mut self, _: &PlanNext, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_index = (self.focus_index + 1) % self.focus.len();
        window.focus(&self.focus[self.focus_index], cx);
    }

    fn previous(&mut self, _: &PlanPrevious, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_index = (self.focus_index + self.focus.len() - 1) % self.focus.len();
        window.focus(&self.focus[self.focus_index], cx);
    }

    pub fn render_read_only(card: Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        let rows = card.read(cx).display_content().lines().count().max(1) + 1;
        let feedback = card.read(cx).feedback.clone();
        let review_note = card.read(cx).plan.review_note.clone();
        let colors = theme(cx).colors;
        div()
            .w_full()
            .flex()
            .flex_col()
            .children((0..rows).map(|row| Self::render_row(card.clone(), row, window, cx)))
            .children(
                review_note.map(|note| div().px_3().text_color(colors.text_secondary).child(note)),
            )
            .children(feedback.map(|feedback| {
                div()
                    .px_3()
                    .text_color(colors.danger)
                    .child(feedback.render())
            }))
            .into_any_element()
    }

    pub fn render_row(
        card: Entity<Self>,
        row: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let (plan, submitted, feedback, focus) = {
            let snapshot = card.read(cx);
            (
                snapshot.plan.clone(),
                snapshot.submitted,
                snapshot.feedback.clone(),
                snapshot.focus.clone(),
            )
        };
        let content = card.read(cx).display_content();
        let content_lines: Vec<&str> = content.lines().collect();
        let content_count = content_lines.len().max(1);
        let base = div()
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .px_3()
            .bg(colors.bg_elevated)
            .border_color(colors.border_subtle)
            .border_l_1()
            .border_r_1();
        if row == 0 {
            return base
                .border_t_1()
                .rounded_tl_lg()
                .rounded_tr_lg()
                .text_size(px(Typography::HEADING_CARD))
                .child(vega_i18n::t_args(
                    "ui.plan_status",
                    &[("status", &(status_label(plan.status)).to_string())],
                ))
                .into_any_element();
        }
        if row <= content_count {
            return base
                .font_family(MONOFONT.to_string())
                .text_size(px(Typography::CODE))
                .child(
                    content_lines
                        .get(row - 1)
                        .copied()
                        .unwrap_or_default()
                        .to_string(),
                )
                .into_any_element();
        }
        if row == content_count + 1 {
            let pending = plan.status == PlanStatus::Pending && !submitted;
            let mut actions = div().flex().flex_row().gap_2();
            if plan.status == PlanStatus::Pending {
                let approve_card = card.clone();
                let changes_card = card.clone();
                let abandon_card = card.clone();
                let action_card = card.clone();
                let next_card = card.clone();
                let previous_card = card.clone();
                actions = actions
                    .key_context("PlanCard")
                    .on_action(move |action: &PlanActivate, window, cx| {
                        action_card.update(cx, |card, cx| card.activate(action, window, cx));
                    })
                    .on_action(move |action: &PlanNext, window, cx| {
                        next_card.update(cx, |card, cx| card.next(action, window, cx));
                    })
                    .on_action(move |action: &PlanPrevious, window, cx| {
                        previous_card.update(cx, |card, cx| card.previous(action, window, cx));
                    })
                    .child(
                        action_button(
                            vega_i18n::t("ui.approve_and_run"),
                            pending,
                            colors.accent,
                            focus[0].clone(),
                            0,
                        )
                        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                            approve_card.update(cx, PlanCard::approve);
                        }),
                    )
                    .child(
                        action_button(
                            vega_i18n::t("ui.request_changes"),
                            pending,
                            colors.warning,
                            focus[1].clone(),
                            1,
                        )
                        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                            changes_card.update(cx, PlanCard::changes);
                        }),
                    )
                    .child(
                        action_button(
                            vega_i18n::t("ui.abandon"),
                            pending,
                            colors.danger,
                            focus[2].clone(),
                            2,
                        )
                        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                            abandon_card.update(cx, PlanCard::abandon);
                        }),
                    );
            } else {
                actions = actions.child(
                    plan.review_note
                        .clone()
                        .unwrap_or_else(|| vega_i18n::t("ui.review_completed").into()),
                );
            }
            return base
                .border_b_1()
                .rounded_bl_lg()
                .rounded_br_lg()
                .child(actions)
                .into_any_element();
        }
        base.text_color(colors.danger)
            .child(feedback.map(|copy| copy.render()).unwrap_or_default())
            .into_any_element()
    }
}

fn action_button(
    label: &'static str,
    enabled: bool,
    color: gpui_kit::Rgba,
    focus: FocusHandle,
    tab_index: isize,
) -> gpui_kit::Div {
    div()
        .track_focus(&focus)
        .tab_index(tab_index)
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(color)
        .text_color(color)
        .when(enabled, |button| button.cursor_pointer())
        .child(label)
}

fn status_label(status: PlanStatus) -> &'static str {
    match status {
        PlanStatus::Pending => vega_i18n::t("ui.pending_approval"),
        PlanStatus::Approved => vega_i18n::t("ui.approved"),
        PlanStatus::ChangesRequested => vega_i18n::t("ui.changes_requested"),
        PlanStatus::Abandoned => vega_i18n::t("ui.abandoned"),
    }
}
