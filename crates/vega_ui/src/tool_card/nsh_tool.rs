mod layout;

use super::*;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{Context, Window};
use nsh_core::prelude::Signal;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    time::Instant,
};
use tokio::sync::{mpsc, oneshot};
use vega_theme::Typography;

pub(super) struct Presentation {
    id: ProjectionId,
    surface: Surface<Snapshot, Request>,
    height: f32,
    headline: Option<(String, Option<Icon>, Option<Instant>)>,
    observed: Snapshot,
    mount: Weak<RefCell<layout::Authority>>,
    measured: Option<(layout::Authority, [f32; 2])>,
}

#[derive(Clone, PartialEq)]
struct AgentTarget {
    parent: String,
    agent: String,
    message: String,
}

#[derive(Clone, PartialEq)]
struct ShellControl {
    operation: String,
    label: String,
    enabled: bool,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    layout: Option<layout::Authority>,
    title: String,
    label: String,
    icon: Option<String>,
    icon_color: gpui_kit::Rgba,
    notice: Option<String>,
    control_failed: bool,
    shell_control: Option<ShellControl>,
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
    auto_hide_scrollbars: bool,
}

impl Snapshot {
    fn same_content(&self, other: &Self) -> bool {
        self.title == other.title
            && self.label == other.label
            && self.icon == other.icon
            && self.icon_color == other.icon_color
            && self.notice == other.notice
            && self.control_failed == other.control_failed
            && self.shell_control == other.shell_control
            && self.status == other.status
            && self.expanded == other.expanded
            && self.expandable == other.expandable
            && self.target == other.target
            && self.detail == other.detail
            && self.activity == other.activity
            && self.epoch == other.epoch
            && self.copy_command == other.copy_command
            && self.copy_output == other.copy_output
            && self.failed == other.failed
            && self.auto_hide_scrollbars == other.auto_hide_scrollbars
            && self.colors.conversation_tool == other.colors.conversation_tool
            && self.colors.text_primary == other.colors.text_primary
            && self.colors.code_bg == other.colors.code_bg
            && self.colors.border_subtle == other.colors.border_subtle
            && self.colors.text_secondary == other.colors.text_secondary
    }
}

enum Request {
    ShellControl {
        observed: Box<Snapshot>,
        reply: oneshot::Sender<bool>,
    },
    Activate {
        id: ProjectionId,
        expanded: bool,
        target: Option<AgentTarget>,
        reply: oneshot::Sender<bool>,
    },
    Layout {
        observed: Box<Snapshot>,
        authority: layout::Authority,
        size: [f32; 2],
    },
}

impl ToolCard {
    pub(super) fn invalidate_layout(&mut self) {
        if let Some(page) = &mut self.presentation {
            page.id = ProjectionId::new();
            page.measured = None;
        }
    }

    fn nsh_snapshot(
        &self,
        id: ProjectionId,
        headline: Option<&(String, Option<Icon>, Option<Instant>)>,
        colors: ThemeColors,
        auto_hide_scrollbars: bool,
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
        let notice = if self.shell_control_failed {
            Some(vega_i18n::t("ui.shell_session.control_failed").to_owned())
        } else {
            (!invocation)
                .then(|| headline.and_then(|(title, _, _)| self.headline_notice(title)))
                .flatten()
        };
        let shell_control = self.shell_command.as_ref().and_then(|command| {
            (command.concurrent && (command.can_cancel || command.can_retry)).then(|| {
                ShellControl {
                    operation: command.operation_id.clone(),
                    label: vega_i18n::t(if self.shell_control_pending {
                        "ui.shell_session.stopping"
                    } else if command.can_retry {
                        "ui.shell_recovery.retry"
                    } else {
                        "ui.shell_session.cancel"
                    })
                    .to_owned(),
                    enabled: !self.shell_control_pending,
                }
            })
        });
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
            layout: self
                .presentation
                .as_ref()
                .and_then(|page| page.mount.upgrade())
                .map(|mount| mount.borrow().clone()),
            title,
            label,
            icon: icon.and_then(crate::icons::nsh_source),
            icon_color: if invocation {
                colors.text_secondary
            } else {
                self.leading_icon_color(&colors)
            },
            notice,
            control_failed: self.shell_control_failed,
            shell_control,
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
            auto_hide_scrollbars,
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
                let snapshot = this.nsh_snapshot(
                    id.clone(),
                    headline.as_ref(),
                    colors,
                    cx.should_auto_hide_scrollbars(),
                );
                let height = this.estimated_height();
                let (surface, mut commands) = Surface::new(snapshot.clone(), render, window, cx);
                this.presentation = Some(Presentation {
                    id,
                    surface,
                    height,
                    headline: headline.clone(),
                    observed: snapshot,
                    mount: Weak::new(),
                    measured: None,
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
            let authority = this
                .presentation
                .as_ref()
                .and_then(|page| page.mount.upgrade())
                .unwrap_or_else(|| Rc::new(RefCell::new(layout::Authority::default())));
            if let Some(page) = &mut this.presentation {
                page.mount = Rc::downgrade(&authority);
            }
            this.publish_nsh(cx);
            div()
                .w_full()
                .flex_shrink_0()
                .when(indented, |view| view.pl_4())
                .children(this.presentation.as_ref().map(|presentation| {
                    layout::Mounted {
                        child: div()
                            .debug_selector(|| "shell-command-native-host".to_owned())
                            .w_full()
                            .h(this
                                .measured_size(cx)
                                .map_or(px(presentation.height), |size| size.height))
                            .child(presentation.surface.entity())
                            .into_any_element(),
                        card: cx.weak_entity(),
                        entity_id: cx.entity_id(),
                        authority: authority.clone(),
                    }
                }))
                .into_any_element()
        })
    }

    fn publish_nsh(&mut self, cx: &App) {
        let Some(page) = &self.presentation else {
            return;
        };
        let mut snapshot = self.nsh_snapshot(
            page.id.clone(),
            page.headline.as_ref(),
            theme(cx).colors,
            cx.should_auto_hide_scrollbars(),
        );
        let Some(page) = &mut self.presentation else {
            return;
        };
        if !snapshot.same_content(&page.observed) {
            page.id = ProjectionId::new();
            page.measured = None;
            snapshot.id = page.id.clone();
        }
        let same_layout = match (&snapshot.layout, &page.observed.layout) {
            (Some(current), Some(previous)) => current.matches(previous),
            (None, None) => true,
            _ => false,
        };
        if snapshot.id.matches(&page.observed.id)
            && snapshot.same_content(&page.observed)
            && same_layout
        {
            return;
        }
        page.observed = snapshot.clone();
        page.surface.publish(snapshot);
    }

    pub(crate) fn measured_size(&self, cx: &App) -> Option<gpui_kit::Size<gpui_kit::Pixels>> {
        let page = self.presentation.as_ref()?;
        let (authority, size) = page.measured.as_ref()?;
        let mount = page.mount.upgrade()?;
        let current = self.nsh_snapshot(
            page.id.clone(),
            page.headline.as_ref(),
            theme(cx).colors,
            cx.should_auto_hide_scrollbars(),
        );
        (authority.matches(&mount.borrow())
            && current.same_content(&page.observed)
            && page.surface.errors().borrow().is_none())
        .then(|| gpui_kit::size(px(size[0]), px(size[1])))
    }

    fn nsh_request(&mut self, request: Request, cx: &mut Context<Self>) {
        match request {
            Request::ShellControl { observed, reply } => {
                let valid = self.presentation.as_ref().is_some_and(|page| {
                    observed.id.matches(&page.id)
                        && observed.same_content(&self.nsh_snapshot(
                            page.id.clone(),
                            page.headline.as_ref(),
                            theme(cx).colors,
                            cx.should_auto_hide_scrollbars(),
                        ))
                        && observed.layout.as_ref().is_some_and(|authority| {
                            page.mount
                                .upgrade()
                                .is_some_and(|mount| authority.matches(&mount.borrow()))
                        })
                        && page.surface.errors().borrow().is_none()
                });
                let operation = observed.shell_control.as_ref().filter(|control| {
                    valid
                        && control.enabled
                        && !self.shell_control_pending
                        && matches!(&self.input, Some(ToolCardInputProjection::Bash { .. }))
                        && matches!(&self.result, Some(ToolCardResultProjection::Bash { operation_id: Some(operation), .. }) if operation.as_ref() == control.operation.as_str())
                        && self.shell_command.as_ref().is_some_and(|command| {
                            command.operation_id == control.operation
                                && command.concurrent
                                && (command.can_cancel || command.can_retry)
                        })
                });
                let accepted = if let Some(control) = operation {
                    self.shell_control_pending = true;
                    self.shell_control_failed = false;
                    cx.emit(ShellControlRequested {
                        operation_id: control.operation.clone(),
                    });
                    cx.notify();
                    true
                } else {
                    false
                };
                self.publish_nsh(cx);
                let _ = reply.send(accepted);
            }
            Request::Layout {
                observed,
                authority,
                size,
            } => {
                let valid = self.presentation.as_ref().is_some_and(|page| {
                    observed.id.matches(&page.id)
                        && observed.same_content(&self.nsh_snapshot(
                            page.id.clone(),
                            page.headline.as_ref(),
                            theme(cx).colors,
                            cx.should_auto_hide_scrollbars(),
                        ))
                        && page
                            .mount
                            .upgrade()
                            .is_some_and(|mount| authority.matches(&mount.borrow()))
                        && page.surface.errors().borrow().is_none()
                        && authority.accepts_size(size)
                });
                if valid && let Some(page) = &mut self.presentation {
                    let changed = (page.height - size[1]).abs() > 0.5
                        || page.measured.as_ref().is_none_or(|(previous, measured)| {
                            !previous.matches(&authority) || *measured != size
                        });
                    page.height = size[1];
                    page.measured = Some((authority, size));
                    if changed {
                        cx.notify();
                    }
                }
            }
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
    let scrollbar_presentation = stored(None);
    effect(move || {
        let snapshot = state.get();
        let presentation = (snapshot.colors.text_primary, snapshot.auto_hide_scrollbars);
        if scrollbar_presentation.get() == Some(presentation) {
            return;
        }
        scrollbar_presentation.replace(Some(presentation));
        let mut look = nsh_base::ScrollbarLook::default();
        look.class = Some("absolute top-0 right-0 bottom-0 w-4 p-1".into());
        let tint = crate::nsh::color(snapshot.colors.text_primary);
        let idle = u32::from(tint.opacity(0.35));
        let engaged = u32::from(tint.opacity(0.55));
        look.thumb_class = Some(
            format!(
                "w-full rounded-full bg-[#{idle:08x}] hover:bg-[#{engaged:08x}] active:bg-[#{engaged:08x}]"
            )
            .into(),
        );
        let mut scrollbar = nsh_base::ScrollbarTheme::default();
        scrollbar.mode = if snapshot.auto_hide_scrollbars {
            nsh_base::ScrollbarMode::Hover
        } else {
            nsh_base::ScrollbarMode::Always
        };
        scrollbar.vertical = Some(look);
        nsh_base::set_scrollbar_theme(Some(scrollbar));
    });
    let pending = signal(false);
    let failed = signal(false);
    let root = element_ref();
    let measured = signal_with(None::<(ProjectionId, ProjectionId)>, SignalEquals::Never);
    let scroll_offset = signal(point(px(0.), px(0.)));
    let layout = crate::nsh::OrderedCommands::new(commands.clone());
    let layout_event = layout.clone();
    effect(move || {
        let observed = state.get();
        let Some(authority) = observed.layout.clone().filter(|layout| layout.width > 0.) else {
            return;
        };
        if measured.with_untracked(|previous| {
            previous
                .as_ref()
                .is_some_and(|(id, mount)| id.matches(&observed.id) && mount.matches(&authority.id))
        }) {
            return;
        }
        measured.set(Some((observed.id.clone(), authority.id.clone())));
        let layout = layout.clone();
        spawn(async move {
            if let Some(bounds) = root.measure().await {
                layout.send(Request::Layout {
                    observed: Box::new(observed),
                    authority,
                    size: [bounds.frame.size.width.0, bounds.frame.size.height.0],
                });
            }
        });
    });
    let shell_commands = commands.clone();
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
        .element_ref(&root)
        .flex_shrink_0()
        .text_size(px(Typography::CONVERSATION))
        .text_color(move || crate::nsh::color(state.get().colors.conversation_tool).to_hsla())
        .on_layout(move |_: &Bounds<Points>| {
            let observed = state.get_untracked();
            if let Some(authority) = observed.layout.clone() {
                let layout = layout_event.clone();
                spawn(async move {
                    if let Some(measurement) = root.measure().await {
                        layout.send(Request::Layout {
                            observed: Box::new(observed),
                            authority,
                            size: [
                                measurement.frame.size.width.0,
                                measurement.frame.size.height.0,
                            ],
                        });
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
                    crate::nsh::color(if snapshot.control_failed {
                        snapshot.colors.text_secondary
                    } else {
                        snapshot.activity.terminal_color(&snapshot.colors)
                    })
                    .to_hsla()
                })
            },
        ))
        .child(when(
            move || state.get().shell_control.is_some(),
            move || {
                let commands = shell_commands.clone();
                let pending = signal(false);
                view()
                    .role(AccessibilityRole::Button)
                    .optional_label(move || {
                        state
                            .get()
                            .shell_control
                            .map(|control| control.label.into())
                    })
                    .disabled(move || {
                        pending.get()
                            || state
                                .get()
                                .shell_control
                                .is_none_or(|control| !control.enabled)
                    })
                    .accessibility_state(move || AccessibilityState {
                        disabled: pending.get()
                            || state
                                .get()
                                .shell_control
                                .is_none_or(|control| !control.enabled),
                        ..Default::default()
                    })
                    .px_2()
                    .py_1()
                    .rounded(px(Layout::PANEL_RADIUS))
                    .text_color(move || {
                        crate::nsh::color(state.get().colors.text_secondary).to_hsla()
                    })
                    .hover(move |style| style.bg(crate::nsh::color(state.get().colors.bg_hover)))
                    .on_press(move |_: &nsh_core::PressEvent| {
                        let observed = state.get_untracked();
                        if pending.get_untracked()
                            || observed
                                .shell_control
                                .as_ref()
                                .is_none_or(|control| !control.enabled)
                        {
                            return;
                        }
                        pending.set(true);
                        failed.set(false);
                        let commands = commands.clone();
                        spawn(async move {
                            let (reply, response) = oneshot::channel();
                            if commands
                                .send(Request::ShellControl {
                                    observed: Box::new(observed),
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
                    .child(text(move || {
                        state
                            .get()
                            .shell_control
                            .map(|control| control.label)
                            .unwrap_or_default()
                    }))
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
