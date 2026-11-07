use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, EventEmitter, Window, div, px};
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{Plan, PlanReviewAction, PlanStatus};
use vega_theme::{ThemeColors, Typography, theme};

use crate::conversation_stream::{MONOFONT, ROW_HEIGHT};
use crate::nsh::{ProjectionId, Surface};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanReviewRequested {
    pub thread_id: String,
    pub plan_id: String,
    pub action: PlanReviewAction,
}

pub struct PlanCard {
    projection_id: ProjectionId,
    plan: Plan,
    generated_notices: Vec<crate::conversation_stream::generated_notice::GeneratedNoticeSpan>,
    submitted: bool,
    feedback: Option<crate::localized_copy::LocalizedCopy>,
    presentation: Option<Presentation>,
    read_only_presentation: Option<Presentation>,
}

struct Presentation {
    surface: Surface<PlanSnapshot, PlanCommand>,
    height: f32,
}

impl EventEmitter<PlanReviewRequested> for PlanCard {}

#[derive(Clone)]
pub(crate) struct PlanSnapshot {
    pub id: ProjectionId,
    pub content: String,
    pub status_label: String,
    pub review_note: Option<String>,
    pub feedback: Option<String>,
    pub pending: bool,
    pub can_review: bool,
    pub colors: ThemeColors,
    pub approve: String,
    pub changes: String,
    pub abandon: String,
    pub failed: String,
}

pub(crate) enum PlanCommand {
    Review {
        id: ProjectionId,
        action: PlanReviewAction,
        reply: oneshot::Sender<bool>,
    },
    Layout {
        height: f32,
    },
}

impl PlanCard {
    fn nsh_snapshot(&self, read_only: bool, cx: &App) -> PlanSnapshot {
        PlanSnapshot {
            id: self.projection_id.clone(),
            content: self.display_content(),
            status_label: vega_i18n::t_args(
                "ui.plan_status",
                &[("status", status_label(self.plan.status))],
            ),
            review_note: self.plan.review_note.clone(),
            feedback: self.feedback.as_ref().map(|copy| copy.render()),
            pending: !read_only && self.plan.status == PlanStatus::Pending,
            can_review: !read_only && self.plan.status == PlanStatus::Pending && !self.submitted,
            colors: theme(cx).colors,
            approve: vega_i18n::t("ui.approve_and_run").into(),
            changes: vega_i18n::t("ui.request_changes").into(),
            abandon: vega_i18n::t("ui.abandon").into(),
            failed: vega_i18n::t("ui.command_failed").into(),
        }
    }

    fn publish(&self, cx: &App) {
        if let Some(presentation) = &self.presentation {
            presentation.surface.publish(self.nsh_snapshot(false, cx));
        }
        if let Some(presentation) = &self.read_only_presentation {
            presentation.surface.publish(self.nsh_snapshot(true, cx));
        }
    }

    fn apply_nsh_command(&mut self, command: PlanCommand, read_only: bool, cx: &mut Context<Self>) {
        match command {
            PlanCommand::Review { id, action, reply } => {
                let accepted = !read_only
                    && self.projection_id.matches(&id)
                    && self.plan.status == PlanStatus::Pending
                    && !self.submitted;
                if accepted {
                    self.request(action, cx);
                }
                let _ = reply.send(accepted);
                self.publish(cx);
            }
            PlanCommand::Layout { height } if height.is_finite() && height > 0. => {
                let presentation = if read_only {
                    &mut self.read_only_presentation
                } else {
                    &mut self.presentation
                };
                if let Some(presentation) = presentation
                    && (presentation.height - height).abs() > 0.5
                {
                    presentation.height = height;
                    cx.notify();
                }
            }
            PlanCommand::Layout { .. } => {}
        }
    }

    pub fn new(plan: Plan, _cx: &mut Context<Self>) -> Self {
        Self {
            projection_id: ProjectionId::new(),
            plan,
            generated_notices: Vec::new(),
            submitted: false,
            feedback: None,
            presentation: None,
            read_only_presentation: None,
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
        self.publish(cx);
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
        self.projection_id = ProjectionId::new();
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
        self.publish(cx);
        cx.notify();
    }

    pub fn apply_stale(&mut self, cx: &mut Context<Self>) {
        self.projection_id = ProjectionId::new();
        self.feedback = Some(crate::localized_copy::LocalizedCopy::new(
            "ui.this_plan_has_already_been_handled_or_replaced",
        ));
        self.submitted = true;
        self.publish(cx);
        cx.notify();
    }

    fn request(&mut self, action: PlanReviewAction, cx: &mut Context<Self>) {
        if self.plan.status != PlanStatus::Pending || self.submitted {
            return;
        }
        self.submitted = true;
        self.publish(cx);
        cx.emit(PlanReviewRequested {
            thread_id: self.plan.thread_id.clone(),
            plan_id: self.plan.id.clone(),
            action,
        });
        cx.notify();
    }

    pub fn render_card(
        card: Entity<Self>,
        read_only: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        card.update(cx, |this, cx| {
            let snapshot = this.nsh_snapshot(read_only, cx);
            let slot = if read_only {
                &mut this.read_only_presentation
            } else {
                &mut this.presentation
            };
            if slot.is_none() {
                let height = (snapshot.content.lines().count().max(1) + 4) as f32 * ROW_HEIGHT;
                let (surface, mut commands) =
                    Surface::new(snapshot.clone(), render_plan, window, cx);
                *slot = Some(Presentation { surface, height });
                cx.spawn_in(window, async move |this, cx| {
                    while let Some(command) = commands.recv().await {
                        if this
                            .update_in(cx, |this, _, cx| {
                                this.apply_nsh_command(command, read_only, cx)
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            let presentation = slot.as_mut().map(|presentation| {
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
                    presentation.map(|(view, height)| div().w_full().h(px(height)).child(view)),
                )
                .into_any_element()
        })
    }

    pub fn render_read_only(card: Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        Self::render_card(card, true, window, cx)
    }
}

fn render_plan(
    state: nsh_core::prelude::Signal<PlanSnapshot>,
    commands: mpsc::Sender<PlanCommand>,
) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;

    let pending = signal(false);
    let failed = signal(false);
    let measured = signal(0.0f32);
    let layout_commands = commands.clone();
    let review_commands = commands;
    view()
        .w_full()
        .flex_shrink_0()
        .p_3()
        .gap_2()
        .bg(move || crate::nsh::color(state.get().colors.bg_elevated).to_hsla())
        .text_color(move || crate::nsh::color(state.get().colors.text_primary).to_hsla())
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (measured.get_untracked() - height).abs() > 0.5
            {
                measured.set(height);
                let commands = layout_commands.clone();
                spawn(async move {
                    if commands.send(PlanCommand::Layout { height }).await.is_err() {
                        failed.set(true);
                    }
                });
            }
        })
        .child(text(move || state.get().status_label).text_size(px(Typography::HEADING_CARD)))
        .child(
            text(move || state.get().content)
                .font_family(MONOFONT)
                .text_size(px(Typography::CODE))
                .flex_shrink_0()
                .selectable(true),
        )
        .child(when(
            move || state.get().review_note.is_some(),
            move || {
                text(move || state.get().review_note.unwrap_or_default())
                    .selectable(true)
                    .text_size(px(Typography::BODY))
            },
        ))
        .child(when(
            move || state.get().pending,
            move || {
                let mut actions = view().flex_row().flex_wrap().gap_2();
                for index in 0..3 {
                    let commands = review_commands.clone();
                    actions = actions.child(
                        nsh_base::button()
                            .disabled(move || !state.get().can_review || pending.get())
                            .on_press(move || {
                                let snapshot = state.get();
                                if !snapshot.can_review || pending.get_untracked() {
                                    return;
                                }
                                pending.set(true);
                                failed.set(false);
                                let commands = commands.clone();
                                spawn(async move {
                                    let (reply, outcome) = oneshot::channel();
                                    let action = match index {
                                        0 => PlanReviewAction::Approve,
                                        1 => PlanReviewAction::RequestChanges { note: None },
                                        _ => PlanReviewAction::Abandon { note: None },
                                    };
                                    if commands
                                        .send(PlanCommand::Review {
                                            id: snapshot.id,
                                            action,
                                            reply,
                                        })
                                        .await
                                        .is_err()
                                        || !matches!(outcome.await, Ok(true))
                                    {
                                        failed.set(true);
                                    }
                                    pending.set(false);
                                });
                            })
                            .child(text(move || {
                                let snapshot = state.get();
                                match index {
                                    0 => snapshot.approve,
                                    1 => snapshot.changes,
                                    _ => snapshot.abandon,
                                }
                            })),
                    );
                }
                actions
            },
        ))
        .child(when(
            move || state.get().feedback.is_some(),
            move || {
                text(move || state.get().feedback.unwrap_or_default())
                    .text_color(move || crate::nsh::color(state.get().colors.danger).to_hsla())
            },
        ))
        .child(when(
            move || failed.get(),
            move || {
                text(move || state.get().failed)
                    .text_color(move || crate::nsh::color(state.get().colors.danger).to_hsla())
            },
        ))
        .into_element()
}

fn status_label(status: PlanStatus) -> &'static str {
    match status {
        PlanStatus::Pending => vega_i18n::t("ui.pending_approval"),
        PlanStatus::Approved => vega_i18n::t("ui.approved"),
        PlanStatus::ChangesRequested => vega_i18n::t("ui.changes_requested"),
        PlanStatus::Abandoned => vega_i18n::t("ui.abandoned"),
    }
}
