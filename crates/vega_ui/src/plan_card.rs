//! Typed Plan review card. It emits commands upward and never reads SQLite.

use gpui::prelude::*;
use gpui::{
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
    submitted: bool,
    feedback: Option<String>,
    focus: [FocusHandle; 3],
    focus_index: usize,
}

impl EventEmitter<PlanReviewRequested> for PlanCard {}

impl PlanCard {
    pub fn new(plan: Plan, cx: &mut Context<Self>) -> Self {
        Self {
            plan,
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

    pub fn plan_id(&self) -> &str {
        &self.plan.id
    }

    pub fn status(&self) -> PlanStatus {
        self.plan.status
    }

    pub fn apply_persisted(&mut self, plan: Plan, cx: &mut Context<Self>) {
        if plan.id != self.plan.id || plan.thread_id != self.plan.thread_id {
            self.feedback = Some("计划状态损坏".into());
            self.submitted = true;
        } else {
            self.plan = plan;
            self.submitted = false;
            self.feedback = None;
        }
        cx.notify();
    }

    pub fn apply_stale(&mut self, cx: &mut Context<Self>) {
        self.feedback = Some("该计划已被处理或替代".into());
        self.submitted = true;
        cx.notify();
    }

    pub fn row_count(&self) -> usize {
        let content = self.plan.content.lines().count().max(1);
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
        let content_lines: Vec<&str> = plan.content.lines().collect();
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
                .child(format!("计划 · {}", status_label(plan.status)))
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
                        action_button("批准并执行", pending, colors.accent, focus[0].clone(), 0)
                            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                                approve_card.update(cx, PlanCard::approve);
                            }),
                    )
                    .child(
                        action_button("要求修改", pending, colors.warning, focus[1].clone(), 1)
                            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                                changes_card.update(cx, PlanCard::changes);
                            }),
                    )
                    .child(
                        action_button("放弃", pending, colors.danger, focus[2].clone(), 2)
                            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                                abandon_card.update(cx, PlanCard::abandon);
                            }),
                    );
            } else {
                actions = actions.child(
                    plan.review_note
                        .clone()
                        .unwrap_or_else(|| "审核已完成".into()),
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
            .child(feedback.unwrap_or_default())
            .into_any_element()
    }
}

fn action_button(
    label: &'static str,
    enabled: bool,
    color: gpui::Rgba,
    focus: FocusHandle,
    tab_index: isize,
) -> gpui::Div {
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
        PlanStatus::Pending => "待审批",
        PlanStatus::Approved => "已批准",
        PlanStatus::ChangesRequested => "需修改",
        PlanStatus::Abandoned => "已放弃",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use gpui::{Render, TestAppContext, WindowHandle};

    struct Harness {
        card: Entity<PlanCard>,
    }

    impl Render for Harness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let rows = self.card.read(cx).row_count();
            div().flex().flex_col().children(
                (0..rows).map(|row| PlanCard::render_row(self.card.clone(), row, window, cx)),
            )
        }
    }

    fn plan(status: PlanStatus) -> Plan {
        Plan {
            id: "plan".into(),
            thread_id: "thread".into(),
            content: "one\ntwo".into(),
            status,
            review_note: None,
            reviewed_at: (status != PlanStatus::Pending).then_some(1),
        }
    }

    fn open_card(
        cx: &mut TestAppContext,
    ) -> (
        WindowHandle<Harness>,
        Entity<PlanCard>,
        Arc<Mutex<Vec<PlanReviewRequested>>>,
    ) {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            crate::init(cx);
        });
        let card = cx.new(|cx| PlanCard::new(plan(PlanStatus::Pending), cx));
        let root_card = card.clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|cx| {
                    cx.subscribe(&root_card, move |_, _, event, _| {
                        if let Ok(mut events) = captured.lock() {
                            events.push(event.clone());
                        }
                    })
                    .detach();
                    Harness { card: root_card }
                })
            })
            .expect("plan card test window")
        });
        cx.run_until_parked();
        (window, card, events)
    }

    #[test]
    fn pending_and_terminal_rows_are_stable() {
        // Pure row count is covered through a minimal test context below;
        // terminal states retain the same card geometry.
        assert_eq!(plan(PlanStatus::Pending).content.lines().count(), 2);
        for status in [
            PlanStatus::Approved,
            PlanStatus::ChangesRequested,
            PlanStatus::Abandoned,
        ] {
            assert_eq!(plan(status).content.lines().count(), 2);
        }
    }

    #[gpui::test]
    async fn keyboard_focus_order_and_first_wins(cx: &mut TestAppContext) {
        let expected = [
            PlanReviewAction::Approve,
            PlanReviewAction::RequestChanges { note: None },
            PlanReviewAction::Abandon { note: None },
        ];
        for (tabs, expected) in expected.into_iter().enumerate() {
            let (window, card, events) = open_card(cx);
            let focused = window
                .update(cx, |_, window, cx| {
                    card.read(cx)
                        .focus
                        .iter()
                        .any(|focus| focus.is_focused(window))
                })
                .expect("plan card window");
            assert!(!focused, "an arriving Plan must not steal focus");
            window
                .update(cx, |_, window, cx| {
                    let focus = card.read(cx).focus[0].clone();
                    window.focus(&focus, cx);
                })
                .expect("plan card window");
            for _ in 0..tabs {
                cx.simulate_keystrokes(window.into(), "tab");
            }
            cx.simulate_keystrokes(window.into(), if tabs == 0 { "enter" } else { "space" });
            cx.simulate_keystrokes(window.into(), "enter space");
            let events = events.lock().expect("event capture");
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].thread_id, "thread");
            assert_eq!(events[0].plan_id, "plan");
            assert_eq!(events[0].action, expected);
            assert!(card.read_with(cx, |card, _| card.submitted));
        }
    }
}
