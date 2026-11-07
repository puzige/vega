use super::*;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{Context, Window};
use nsh_core::prelude::Signal;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};
use vega_theme::Typography;

pub(super) struct Presentation {
    id: ProjectionId,
    surface: Surface<Snapshot, Request>,
    height: f32,
    headline: Option<(String, Option<Icon>, Option<Instant>)>,
}

#[derive(Clone)]
struct AgentTarget {
    parent: String,
    agent: String,
    message: String,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    title: String,
    label: String,
    icon: Option<String>,
    icon_color: gpui_kit::Rgba,
    notice: Option<String>,
    status: Option<(String, bool)>,
    expanded: bool,
    expandable: bool,
    target: Option<AgentTarget>,
    detail: Option<ToolDetail>,
    colors: ThemeColors,
    activity: ToolActivityState,
    epoch: Option<Instant>,
    copy_command: String,
    copy_output: String,
    failed: String,
}

enum Request {
    Activate {
        id: ProjectionId,
        expanded: bool,
        target: Option<AgentTarget>,
        reply: oneshot::Sender<bool>,
    },
    Layout(f32),
}

impl ToolCard {
    fn nsh_snapshot(
        &self,
        id: ProjectionId,
        headline: Option<&(String, Option<Icon>, Option<Instant>)>,
        colors: ThemeColors,
    ) -> Snapshot {
        let invocation = self.is_subagent_invocation();
        let target = invocation
            .then(|| self.subagent())
            .flatten()
            .map(|agent| AgentTarget {
                parent: agent.parent_thread_id.clone(),
                agent: agent.agent_id.clone(),
                message: agent.message_id.clone(),
            });
        let invocation_title = || {
            self.description_title()
                .unwrap_or_else(|| match (&self.input, self.subagent()) {
                    (Some(ToolCardInputProjection::Subagent { tool, .. }), Some(agent)) => {
                        vega_i18n::t_args(
                            "ui.action_name",
                            &[
                                (
                                    "action",
                                    vega_i18n::t(if tool == "spawn_agent" {
                                        "ui.delegate"
                                    } else {
                                        "ui.continue"
                                    }),
                                ),
                                ("name", &agent.name),
                            ],
                        )
                    }
                    (Some(ToolCardInputProjection::Subagent { title, .. }), _) => title.clone(),
                    _ => vega_i18n::t("ui.delegate_task").into(),
                })
        };
        let title = headline.map_or_else(
            || {
                if invocation {
                    invocation_title()
                } else {
                    self.compact_title()
                }
            },
            |(title, _, _)| title.clone(),
        );
        let error_detail = invocation && target.is_none() && !self.output_rows.is_empty();
        let expandable = if invocation {
            error_detail
        } else {
            self.has_detail()
        };
        let expanded = self.expanded && expandable;
        let status = invocation.then(|| {
            let agent = self.subagent();
            let label = agent.map_or_else(
                || match self.status {
                    ToolCallStatus::PendingApproval => "ui.pending",
                    ToolCallStatus::Approved => "ui.ready",
                    ToolCallStatus::Running => "ui.running",
                    ToolCallStatus::Cancelled => "ui.interrupted",
                    ToolCallStatus::Rejected => "ui.rejected",
                    ToolCallStatus::Failed | ToolCallStatus::Success => "ui.failed",
                },
                |agent| match agent.state {
                    SubagentState::Running => "ui.running",
                    SubagentState::Cancelling => "ui.stopping",
                    SubagentState::Completed => "ui.completed",
                    SubagentState::Failed => "ui.failed",
                    SubagentState::Interrupted => "ui.interrupted",
                },
            );
            let failed = agent.map_or_else(
                || {
                    matches!(
                        self.status,
                        ToolCallStatus::Failed | ToolCallStatus::Success
                    )
                },
                |agent| agent.state == SubagentState::Failed,
            );
            (vega_i18n::t(label).to_owned(), failed)
        });
        let notice = (!invocation)
            .then(|| headline.and_then(|(title, _, _)| self.headline_notice(title)))
            .flatten();
        let label = if let Some((status, _)) = &status {
            vega_i18n::t_args("ui.title_status", &[("title", &title), ("status", status)])
        } else {
            let accessible = notice
                .as_ref()
                .map_or_else(|| title.clone(), |notice| format!("{title} · {notice}"));
            vega_i18n::t_args(
                "ui.tool_disclosure",
                &[
                    ("title", &accessible),
                    (
                        "action",
                        vega_i18n::t(if expanded { "ui.collapse" } else { "ui.expand" }),
                    ),
                ],
            )
        };
        let icon = if invocation {
            Some(Icon::Bot)
        } else {
            headline.map_or(Some(self.leading_icon()), |(_, icon, _)| *icon)
        };
        Snapshot {
            id,
            title,
            label,
            icon: icon.and_then(crate::icons::nsh_source),
            icon_color: if invocation {
                colors.text_secondary
            } else {
                self.leading_icon_color(&colors)
            },
            notice,
            status,
            expanded,
            expandable,
            target,
            detail: if error_detail && expanded {
                Some(ToolDetail {
                    title: None,
                    command: None,
                    output: self.output_rows.clone(),
                    output_copy: None,
                    footer: None,
                })
            } else if !invocation && expanded {
                self.detail()
            } else {
                None
            },
            activity: self.activity_state(),
            epoch: headline.and_then(|(_, _, epoch)| *epoch),
            colors,
            copy_command: vega_i18n::t("ui.copy_command").into(),
            copy_output: vega_i18n::t("ui.copy_output").into(),
            failed: vega_i18n::t("ui.command_failed").into(),
        }
    }

    pub(crate) fn render(
        card: Entity<Self>,
        indented: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        Self::render_with_title(card, indented, None, window, cx)
    }

    pub(crate) fn render_with_title(
        card: Entity<Self>,
        indented: bool,
        headline: Option<(String, Option<Icon>, Option<Instant>)>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        card.update(cx, |this, cx| {
            let colors = theme(cx).colors;
            if this.presentation.is_none() {
                let id = ProjectionId::new();
                let snapshot = this.nsh_snapshot(id.clone(), headline.as_ref(), colors);
                let height = this.estimated_height();
                let (surface, mut commands) = Surface::new(snapshot, render, window, cx);
                this.presentation = Some(Presentation {
                    id,
                    surface,
                    height,
                    headline: headline.clone(),
                });
                cx.spawn_in(window, async move |this, cx| {
                    while let Some(request) = commands.recv().await {
                        if this
                            .update_in(cx, |this, _, cx| this.nsh_request(request, cx))
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            if let Some(presentation) = &mut this.presentation {
                presentation.headline = headline;
                presentation.surface.sync_environment(cx);
            }
            this.publish_nsh(cx);
            div()
                .w_full()
                .flex_shrink_0()
                .when(indented, |view| view.pl_4())
                .children(this.presentation.as_ref().map(|presentation| {
                    div()
                        .w_full()
                        .h(px(presentation.height))
                        .child(presentation.surface.entity())
                }))
                .into_any_element()
        })
    }

    fn publish_nsh(&self, cx: &App) {
        if let Some(presentation) = &self.presentation {
            presentation.surface.publish(self.nsh_snapshot(
                presentation.id.clone(),
                presentation.headline.as_ref(),
                theme(cx).colors,
            ));
        }
    }

    fn nsh_request(&mut self, request: Request, cx: &mut Context<Self>) {
        match request {
            Request::Layout(height) if height.is_finite() && height > 0. => {
                if let Some(presentation) = &mut self.presentation
                    && (presentation.height - height).abs() > 0.5
                {
                    presentation.height = height;
                    cx.notify();
                }
            }
            Request::Layout(_) => {}
            Request::Activate {
                id,
                expanded,
                target,
                reply,
            } => {
                let valid = self
                    .presentation
                    .as_ref()
                    .is_some_and(|presentation| presentation.id.matches(&id));
                let accepted = valid
                    && if let Some(target) = target {
                        let valid = self.is_subagent_invocation()
                            && self.subagent().is_some_and(|agent| {
                                agent.parent_thread_id == target.parent
                                    && agent.agent_id == target.agent
                                    && agent.message_id == target.message
                            });
                        if valid {
                            cx.emit(SubagentOpenRequested {
                                source_thread_id: target.parent,
                                thread_id: target.agent,
                            });
                        }
                        valid
                    } else {
                        let expandable = if self.is_subagent_invocation() {
                            self.subagent().is_none() && !self.output_rows.is_empty()
                        } else {
                            self.has_detail()
                        };
                        if expandable && self.expanded == expanded {
                            self.set_expanded(!expanded, cx);
                            true
                        } else {
                            false
                        }
                    };
                self.publish_nsh(cx);
                let _ = reply.send(accepted);
            }
        }
    }
}

fn icon(source: String, tint: gpui_kit::Rgba) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;
    svg(nsh_core::element::ImageSrc::svg_markup(&source))
        .size(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .text_color(crate::nsh::color(tint).to_hsla())
        .tint_color(crate::nsh::color(tint).to_hsla())
        .into_element()
}

fn render(
    state: Signal<Snapshot>,
    commands: mpsc::Sender<Request>,
) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;
    let pending = signal(false);
    let failed = signal(false);
    let height = signal(0.0f32);
    let scroll_offset = signal(point(px(0.), px(0.)));
    let layout = commands.clone();
    let activate = commands;
    let phase = signal(0.0f32);
    let title = text("").child(each(
        move || state.with(|snapshot| snapshot.title.chars().enumerate().collect::<Vec<_>>()),
        |(index, _)| *index,
        move |part, _| {
            text(move || part.get().1.to_string()).text_color(move || {
                state.with(|snapshot| {
                    let (index, _) = part.get();
                    let strength = if snapshot.epoch.is_some() && !reduce_motion() {
                        let center = phase.get().fract() * 1.6 - 0.3;
                        let x = index as f32 / snapshot.title.chars().count().max(1) as f32;
                        (1. - (x - center).abs() / 0.24).max(0.)
                    } else {
                        0.
                    };
                    let base = snapshot.colors.conversation_tool;
                    let highlight = snapshot.colors.text_primary;
                    crate::nsh::color(gpui_kit::Rgba {
                        r: base.r + (highlight.r - base.r) * strength,
                        g: base.g + (highlight.g - base.g) * strength,
                        b: base.b + (highlight.b - base.b) * strength,
                        a: base.a,
                    })
                    .to_hsla()
                })
            })
        },
    ));
    view()
        .w_full()
        .flex_shrink_0()
        .text_size(px(Typography::CONVERSATION))
        .text_color(move || crate::nsh::color(state.get().colors.conversation_tool).to_hsla())
        .on_layout(move |bounds: &Bounds<Points>| {
            let next = bounds.size.height.0;
            if next.is_finite() && next > 0. && (height.get_untracked() - next).abs() > 0.5 {
                height.set(next);
                let commands = layout.clone();
                spawn(async move {
                    if commands.send(Request::Layout(next)).await.is_err() {
                        failed.set(true);
                    }
                });
            }
        })
        .child(when(
            move || state.get().epoch.is_some() && !reduce_motion(),
            move || {
                spawn(async move {
                    loop {
                        next_frame().await;
                        if let Some(epoch) = state.with_untracked(|snapshot| snapshot.epoch) {
                            phase.set(
                                epoch.elapsed().as_secs_f32() * 1000.
                                    / vega_theme::Motion::THINKING_SHIMMER_MS as f32,
                            );
                        }
                    }
                });
                view()
            },
        ))
        .child(
            view()
                .role(AccessibilityRole::Button)
                .w_full()
                .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
                .flex_row()
                .items_center()
                .gap(px(Layout::CONVERSATION_INLINE_GAP))
                .optional_label(move || Some(state.get().label.into()))
                .accessibility_state(move || nsh_core::AccessibilityState {
                    expanded: state
                        .with(|snapshot| snapshot.expandable.then_some(snapshot.expanded)),
                    ..Default::default()
                })
                .disabled(move || {
                    pending.get() || !(state.get().expandable || state.get().target.is_some())
                })
                .on_press(move |_: &nsh_core::PressEvent| {
                    if pending.get_untracked() {
                        return;
                    }
                    let snapshot = state.get();
                    if !(snapshot.expandable || snapshot.target.is_some()) {
                        return;
                    }
                    pending.set(true);
                    failed.set(false);
                    let commands = activate.clone();
                    spawn(async move {
                        let (reply, response) = oneshot::channel();
                        if commands
                            .send(Request::Activate {
                                id: snapshot.id,
                                expanded: snapshot.expanded,
                                target: snapshot.target,
                                reply,
                            })
                            .await
                            .is_err()
                            || !matches!(response.await, Ok(true))
                        {
                            failed.set(true);
                        }
                        pending.set(false);
                    });
                })
                .child(when(
                    move || state.get().icon.is_some(),
                    move || {
                        dynamic(move || {
                            icon(state.get().icon.unwrap_or_default(), state.get().icon_color)
                        })
                    },
                ))
                .child(
                    title
                        .flex_1()
                        .min_w(px(0.))
                        .text_overflow(nsh_core::EllipsizeMode::Tail),
                )
                .child(when(
                    move || state.get().status.is_some(),
                    move || {
                        text(move || {
                            state
                                .get()
                                .status
                                .map(|(label, _)| label)
                                .unwrap_or_default()
                        })
                        .text_color(move || {
                            let snapshot = state.get();
                            crate::nsh::color(
                                if snapshot.status.is_some_and(|(_, failed)| failed) {
                                    snapshot.colors.danger
                                } else {
                                    snapshot.colors.text_secondary
                                },
                            )
                            .to_hsla()
                        })
                    },
                ))
                .child(when(
                    move || state.get().expandable,
                    move || {
                        dynamic(move || {
                            let snapshot = state.get();
                            let source = crate::icons::nsh_source(if snapshot.expanded {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            })
                            .unwrap_or_default();
                            icon(source, snapshot.colors.text_secondary)
                        })
                    },
                )),
        )
        .child(when(
            move || state.get().notice.is_some(),
            move || {
                text(move || state.get().notice.unwrap_or_default()).text_color(move || {
                    let snapshot = state.get();
                    crate::nsh::color(snapshot.activity.terminal_color(&snapshot.colors)).to_hsla()
                })
            },
        ))
        .child(when(
            move || state.get().detail.is_some(),
            move || render_detail(state, scroll_offset),
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

fn render_detail(
    state: Signal<Snapshot>,
    offset: Signal<nsh_core::Point<nsh_core::Points>>,
) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;
    scroll()
        .w_full()
        .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
        .mt(px(Layout::PROCESS_ITEM_GAP))
        .rounded(px(Layout::TOOL_DETAIL_RADIUS))
        .bg(move || crate::nsh::color(state.get().colors.code_bg).to_hsla())
        .border(px(1.))
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle).to_hsla())
        .content_offset(offset.get_untracked())
        .on_scroll(move |event: &ScrollEvent| offset.set(event.offset))
        .child(
            view()
                .p(px(Layout::TOOL_DETAIL_PADDING))
                .gap_1()
                .flex_shrink_0()
                .child(when(
                    move || {
                        state.with(|snapshot| {
                            snapshot
                                .detail
                                .as_ref()
                                .is_some_and(|detail| detail.title.is_some())
                        })
                    },
                    move || {
                        text(move || {
                            state.with(|snapshot| {
                                snapshot
                                    .detail
                                    .as_ref()
                                    .and_then(|detail| detail.title.clone())
                                    .unwrap_or_default()
                            })
                        })
                        .selectable(true)
                    },
                ))
                .child(when(
                    move || {
                        state.with(|snapshot| {
                            snapshot
                                .detail
                                .as_ref()
                                .is_some_and(|detail| detail.command.is_some())
                        })
                    },
                    move || {
                        view()
                            .gap_1()
                            .child(
                                text(move || {
                                    format!(
                                        "$ {}",
                                        state.with(|snapshot| snapshot
                                            .detail
                                            .as_ref()
                                            .and_then(|detail| detail.command.clone())
                                            .unwrap_or_default())
                                    )
                                })
                                .font_family(MONOFONT)
                                .text_size(px(Typography::CODE))
                                .selectable(true),
                            )
                            .child(
                                nsh_base::button()
                                    .on_press(move || {
                                        if let Some(command) = state
                                            .with(|snapshot| {
                                                snapshot
                                                    .detail
                                                    .as_ref()
                                                    .and_then(|detail| detail.command.clone())
                                            })
                                            .filter(|command| !command.is_empty())
                                        {
                                            set_clipboard(command);
                                        }
                                    })
                                    .child(text(move || {
                                        state.with(|snapshot| snapshot.copy_command.clone())
                                    })),
                            )
                    },
                ))
                .child(each(
                    move || {
                        state.with(|snapshot| {
                            snapshot
                                .detail
                                .as_ref()
                                .map(|detail| {
                                    detail
                                        .output
                                        .iter()
                                        .cloned()
                                        .enumerate()
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default()
                        })
                    },
                    |(index, _)| *index,
                    move |line, _| {
                        text(move || line.get().1)
                            .font_family(MONOFONT)
                            .text_size(px(Typography::CODE))
                            .flex_shrink_0()
                            .selectable(true)
                    },
                ))
                .child(when(
                    move || {
                        state.with(|snapshot| {
                            snapshot
                                .detail
                                .as_ref()
                                .is_some_and(|detail| detail.output_copy.is_some())
                        })
                    },
                    move || {
                        nsh_base::button()
                            .on_press(move || {
                                if let Some(output) = state.with(|snapshot| {
                                    snapshot
                                        .detail
                                        .as_ref()
                                        .and_then(|detail| detail.output_copy.clone())
                                }) {
                                    set_clipboard(output);
                                }
                            })
                            .child(text(move || {
                                state.with(|snapshot| snapshot.copy_output.clone())
                            }))
                    },
                ))
                .child(when(
                    move || {
                        state.with(|snapshot| {
                            snapshot
                                .detail
                                .as_ref()
                                .is_some_and(|detail| detail.footer.is_some())
                        })
                    },
                    move || {
                        text(move || {
                            state.with(|snapshot| {
                                snapshot
                                    .detail
                                    .as_ref()
                                    .and_then(|detail| detail.footer.as_ref())
                                    .map(|(label, _)| label.clone())
                                    .unwrap_or_default()
                            })
                        })
                        .text_color(move || {
                            state.with(|snapshot| {
                                let activity = snapshot
                                    .detail
                                    .as_ref()
                                    .and_then(|detail| detail.footer.as_ref())
                                    .map(|(_, activity)| *activity)
                                    .unwrap_or(snapshot.activity);
                                crate::nsh::color(if activity == ToolActivityState::Success {
                                    snapshot.colors.text_secondary
                                } else {
                                    activity.terminal_color(&snapshot.colors)
                                })
                                .to_hsla()
                            })
                        })
                    },
                )),
        )
        .into_element()
}
