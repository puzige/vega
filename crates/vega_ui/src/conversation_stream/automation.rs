use super::*;
use crate::automation_editor::format_elapsed;
use crate::icons::{Icon, sized_icon, tooltip};
use crate::interaction::InteractionExt;
use gpui_kit::Role;
use vega_conversation::types::{
    AutomationCommand, AutomationControl, AutomationKind, AutomationProjection, AutomationStatus,
};

#[derive(Clone)]
pub struct AutomationRequested {
    pub thread_id: String,
    pub command: AutomationCommand,
}
impl EventEmitter<AutomationRequested> for ConversationStream {}

#[derive(Default)]
pub(crate) struct AutomationUi {
    pub projection: AutomationProjection,
    pub editor_dirty: bool,
    pub timings: HashMap<String, AutomationTiming>,
    clocks: HashMap<String, Entity<AutomationClock>>,
    pub focus: HashMap<String, [FocusHandle; 3]>,
    pub wakeup_labels: HashMap<String, String>,
}
#[derive(Clone)]
pub struct AutomationEditorRequested {
    pub thread_id: String,
    pub identity: String,
}
impl EventEmitter<AutomationEditorRequested> for ConversationStream {}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct AutomationTiming {
    pub elapsed_ms: u64,
    pub running: bool,
}

struct AutomationClock(String);
impl Render for AutomationClock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .whitespace_nowrap()
            .text_size(px(Typography::METADATA))
            .text_color(theme(cx).colors.text_tertiary)
            .child(self.0.clone())
    }
}

impl ConversationStream {
    pub fn set_automation_editor_dirty(&mut self, dirty: bool, cx: &mut Context<Self>) {
        if self.automation.editor_dirty != dirty {
            self.automation.editor_dirty = dirty;
            cx.notify();
        }
    }
    pub fn apply_automation_timings(
        &mut self,
        timings: HashMap<String, AutomationTiming>,
        cx: &mut Context<Self>,
    ) {
        let running_changed = self.automation.projection.tasks.iter().any(|task| {
            self.automation
                .timings
                .get(&task.identity)
                .is_some_and(|time| time.running)
                != timings.get(&task.identity).is_some_and(|time| time.running)
        });
        self.automation.timings = timings;
        for task in &self.automation.projection.tasks {
            let elapsed = self
                .automation
                .timings
                .get(&task.identity)
                .map_or(task.elapsed_ms, |time| time.elapsed_ms);
            let label = format_elapsed(elapsed);
            if let Some(clock) = self.automation.clocks.get(&task.identity) {
                clock.update(cx, |clock, cx| {
                    if clock.0 != label {
                        clock.0 = label;
                        cx.notify();
                    }
                });
            }
        }
        if running_changed {
            cx.notify();
        }
    }

    pub fn apply_automation_thread_status(&mut self, thread: &Thread, cx: &mut Context<Self>) {
        if thread.id == self.thread.id
            && thread.project_id == self.thread.project_id
            && thread.status != self.thread.status
        {
            self.thread.status = thread.status;
            cx.notify();
        }
    }
    pub fn automation_projection(&self) -> &AutomationProjection {
        &self.automation.projection
    }
    pub fn apply_automation(&mut self, projection: AutomationProjection, cx: &mut Context<Self>) {
        for task in &projection.tasks {
            self.automation
                .clocks
                .entry(task.identity.clone())
                .or_insert_with(|| cx.new(|_| AutomationClock(format_elapsed(task.elapsed_ms))));
            self.automation
                .focus
                .entry(task.identity.clone())
                .or_insert_with(|| std::array::from_fn(|_| cx.focus_handle().tab_stop(true)));
        }
        self.automation
            .focus
            .retain(|id, _| projection.tasks.iter().any(|task| task.identity == *id));
        self.automation
            .clocks
            .retain(|id, _| projection.tasks.iter().any(|task| task.identity == *id));
        let now = vega_conversation::automation::now_ms();
        let wakeup_labels: HashMap<_, _> = projection
            .tasks
            .iter()
            .filter(|task| {
                task.kind == AutomationKind::Loop && task.status == AutomationStatus::Active
            })
            .map(|task| {
                let remaining = task.next_due_ms.saturating_sub(now).max(0);
                let label = if remaining == 0 {
                    "等待执行".to_string()
                } else if remaining <= 60_000 {
                    "<1m".to_string()
                } else {
                    format!("{}m 后", remaining.saturating_add(59_999) / 60_000)
                };
                (task.identity.clone(), label)
            })
            .collect();
        if self.automation.projection != projection
            || self.automation.wakeup_labels != wakeup_labels
        {
            self.automation.projection = projection;
            self.automation.wakeup_labels = wakeup_labels;
            cx.notify();
        }
    }
    pub fn automation_can_run(&self, cx: &App) -> bool {
        self.thread.backend == TaskBackend::Native
            && self.thread.status == vega_conversation::types::ThreadStatus::Active
            && !self.actions.running
            && !self.composer_submit_pending
            && self.pending_draft_is_empty(cx)
            && !self.input.read(cx).is_composing()
            && !self.pending_queue.busy
            && self.pending_queue.messages.is_empty()
            && !self.permission_queue.has_pending()
            && !self.has_pending_user_question()
            && !self.approved_not_started
            && !self.trusted_action_busy
            && !self.context_operation_busy()
            && !self.has_pending_model_selection()
            && !self.skill_mutation_pending
            && !self.privacy.pending
            && !self.automation.projection.busy
            && !self.automation.editor_dirty
            && !self
                .run_notices
                .values()
                .any(|notice| notice.can_resume_credentials())
    }
    pub fn accept_automation_command(&mut self, original: &str, cx: &mut Context<Self>) {
        if self.input.read(cx).text() == original {
            self.input.update(cx, TextInput::clear);
        }
        self.reject_composer_submission(cx);
        cx.notify();
    }
    pub(crate) fn automation_focuses(&self) -> Vec<FocusHandle> {
        self.automation
            .projection
            .tasks
            .iter()
            .flat_map(|task| {
                self.automation
                    .focus
                    .get(&task.identity)
                    .into_iter()
                    .flat_map(move |focus| {
                        [2, 1, 0]
                            .into_iter()
                            .filter(move |index| {
                                (*index != 1 || task.can_control())
                                    && (*index == 0 || !self.automation.projection.busy)
                            })
                            .map(|index| focus[index].clone())
                    })
            })
            .collect()
    }
    fn automation_button(
        &self,
        id: String,
        action: (String, Icon),
        focus: &FocusHandle,
        enabled: bool,
        cx: &mut Context<Self>,
        activate: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let (label, glyph) = action;
        let colors = theme(cx).colors;
        let hint = label.clone();
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label.clone())
            .track_focus(focus)
            .tab_stop(enabled)
            .size(px(Layout::AUTOMATION_ACTION_SIZE))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
            .text_size(px(Typography::BODY))
            .text_color(colors.text_secondary)
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .interaction_surface(
                colors.bg_elevated,
                colors.bg_hover,
                colors.bg_active,
                enabled,
            )
            .focus_visible(move |style| style.border_1().border_color(colors.accent))
            .when(enabled, |button| button.cursor_pointer())
            .on_click(cx.listener(move |this, _: &gpui_kit::ClickEvent, _, cx| {
                if enabled {
                    cx.stop_propagation();
                    activate(this, cx);
                }
            }))
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                    if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        keyboard(this, cx);
                    }
                }),
            )
            .tooltip(move |_, cx| tooltip(hint.clone(), cx))
            .child(sized_icon(
                glyph,
                colors.text_secondary,
                Layout::AUTOMATION_ICON_SIZE,
            ))
            .into_any_element()
    }
    pub(crate) fn automation_compact(&self) -> bool {
        self.workspace_width.is_some_and(|width| {
            width - 2. * Layout::CONVERSATION_PADDING < Layout::AUTOMATION_COMPACT_WIDTH
        })
    }
    pub(crate) fn render_automation(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let compact = self.automation_compact();
        let mut view = div()
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .flex()
            .flex_col()
            .key_context("Composer")
            .on_action(cx.listener(Self::next_composer_control))
            .on_action(cx.listener(Self::previous_composer_control));
        if let Some(error) = &self.automation.projection.error {
            view = view.child(
                div()
                    .mb_2()
                    .text_size(px(Typography::BODY))
                    .text_color(colors.danger)
                    .child(error.clone()),
            );
        }
        if self.thread.backend != TaskBackend::Native {
            return view.into_any_element();
        }
        for task in &self.automation.projection.tasks {
            if !matches!(
                task.status,
                AutomationStatus::Active | AutomationStatus::Complete
            ) && let Some(reason) = &task.reason
                && !(task.status == AutomationStatus::Paused && reason == "已由你暂停")
            {
                view = view.child(
                    div()
                        .px(px(Layout::AUTOMATION_INSET))
                        .mb_2()
                        .text_size(px(Typography::BODY))
                        .text_color(colors.text_secondary)
                        .child(reason.clone()),
                );
            }
        }
        for task in &self.automation.projection.tasks {
            let Some(focus) = self.automation.focus.get(&task.identity) else {
                continue;
            };
            let kind = task.kind;
            let identity = task.identity.clone();
            let expand = identity.clone();
            let summary = task
                .instruction
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let label = if compact {
                if task.status == AutomationStatus::BudgetLimited {
                    "预算用尽".to_string()
                } else {
                    task.status.label().to_string()
                }
            } else if task.status == AutomationStatus::Active {
                kind.label().to_string()
            } else {
                format!(
                    "{} · {}",
                    if kind == AutomationKind::Goal {
                        "目标"
                    } else {
                        "循环"
                    },
                    task.status.label()
                )
            };
            let mut row = div()
                .id(format!("automation-{}-strip", kind.key()))
                .h(px(Layout::AUTOMATION_HEIGHT))
                .role(Role::Group)
                .aria_label(kind.label())
                .mx(px(if compact {
                    Layout::AUTOMATION_COMPACT_INSET
                } else {
                    Layout::AUTOMATION_INSET
                }))
                .px(px(if compact {
                    Layout::AUTOMATION_COMPACT_PADDING
                } else {
                    Layout::AUTOMATION_PADDING
                }))
                .flex()
                .items_center()
                .gap(px(Layout::AUTOMATION_GAP))
                .min_w_0()
                .bg(colors.bg_elevated)
                .border_t_1()
                .border_l_1()
                .border_r_1()
                .border_color(colors.border_subtle)
                .rounded_t(px(Layout::AUTOMATION_RADIUS))
                .child(sized_icon(
                    if kind == AutomationKind::Goal {
                        Icon::Goal
                    } else {
                        Icon::Refresh
                    },
                    colors.text_secondary,
                    Layout::AUTOMATION_ICON_SIZE,
                ))
                .child(
                    div()
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_primary)
                        .child(label),
                )
                .child(
                    div()
                        .id(format!("automation-{}-summary", kind.key()))
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(px(Typography::BODY))
                        .text_color(colors.text_secondary)
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _: &gpui_kit::ClickEvent, _, cx| {
                            cx.emit(AutomationEditorRequested {
                                thread_id: this.thread.id.clone(),
                                identity: identity.clone(),
                            });
                        }))
                        .child(summary),
                );
            if !compact
                && kind == AutomationKind::Loop
                && !self.actions.running
                && !self
                    .automation
                    .timings
                    .get(&task.identity)
                    .is_some_and(|time| time.running)
                && let Some(wait) = self.automation.wakeup_labels.get(&task.identity)
            {
                row = row.child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .child(wait.clone()),
                );
            } else if !compact && let Some(clock) = self.automation.clocks.get(&task.identity) {
                row = row.child(clock.clone());
            }
            row = row.child(self.automation_button(
                format!("automation-{}-clear", kind.key()),
                (
                    format!(
                        "清除{}",
                        if kind == AutomationKind::Goal {
                            "目标"
                        } else {
                            "循环"
                        }
                    ),
                    Icon::Trash,
                ),
                &focus[2],
                !self.automation.projection.busy,
                cx,
                move |this, cx| {
                    cx.emit(AutomationRequested {
                        thread_id: this.thread.id.clone(),
                        command: AutomationCommand::Control(kind, AutomationControl::Clear),
                    });
                },
            ));
            if task.can_control() {
                let active = task.status == AutomationStatus::Active;
                row = row.child(self.automation_button(
                    format!("automation-{}-control", kind.key()),
                    (
                        format!(
                            "{}{}",
                            if active { "暂停" } else { "恢复" },
                            if kind == AutomationKind::Goal {
                                "目标"
                            } else {
                                "循环"
                            }
                        ),
                        if active { Icon::Pause } else { Icon::Play },
                    ),
                    &focus[1],
                    !self.automation.projection.busy,
                    cx,
                    move |this, cx| {
                        cx.emit(AutomationRequested {
                            thread_id: this.thread.id.clone(),
                            command: AutomationCommand::Control(
                                kind,
                                if active {
                                    AutomationControl::Pause
                                } else {
                                    AutomationControl::Resume
                                },
                            ),
                        });
                    },
                ));
            } else {
                row = row.child(
                    div()
                        .size(px(Layout::AUTOMATION_ACTION_SIZE))
                        .flex_shrink_0(),
                );
            }
            row = row.child(
                self.automation_button(
                    format!("automation-{}-details", kind.key()),
                    (
                        if kind == AutomationKind::Goal {
                            "编辑目标"
                        } else {
                            "编辑循环"
                        }
                        .into(),
                        Icon::Maximize,
                    ),
                    &focus[0],
                    true,
                    cx,
                    move |this, cx| {
                        cx.emit(AutomationEditorRequested {
                            thread_id: this.thread.id.clone(),
                            identity: expand.clone(),
                        });
                    },
                ),
            );
            view = view.child(row);
        }
        view.into_any_element()
    }
}
