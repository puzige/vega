use super::*;
use crate::interaction::InteractionExt;
use gpui_kit::{Div, Role, Stateful};
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
    pub details: HashSet<String>,
    pub focus: HashMap<String, [FocusHandle; 3]>,
    pub wakeup_labels: HashMap<String, String>,
}
impl ConversationStream {
    pub fn automation_projection(&self) -> &AutomationProjection {
        &self.automation.projection
    }
    pub fn apply_automation(&mut self, projection: AutomationProjection, cx: &mut Context<Self>) {
        for task in &projection.tasks {
            self.automation
                .focus
                .entry(task.identity.clone())
                .or_insert_with(|| std::array::from_fn(|_| cx.focus_handle().tab_stop(true)));
        }
        self.automation
            .focus
            .retain(|id, _| projection.tasks.iter().any(|task| task.identity == *id));
        self.automation
            .details
            .retain(|id| projection.tasks.iter().any(|task| task.identity == *id));
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
                    "下次执行：1 分钟内".to_string()
                } else {
                    format!(
                        "下次执行：约 {} 分钟后",
                        remaining.saturating_add(59_999) / 60_000
                    )
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
            && !self.approved_not_started
            && !self.trusted_action_busy
            && !self.context_operation_busy()
            && !self.has_pending_model_selection()
            && !self.skill_mutation_pending
            && !self.privacy.pending
            && !self.automation.projection.busy
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
                        focus
                            .iter()
                            .enumerate()
                            .filter(move |(index, _)| {
                                (*index != 1 || task.can_control())
                                    && (*index == 0 || !self.automation.projection.busy)
                            })
                            .map(|(_, focus)| focus.clone())
                    })
            })
            .collect()
    }
    fn automation_button(
        &self,
        id: String,
        label: String,
        focus: &FocusHandle,
        enabled: bool,
        cx: &mut Context<Self>,
        activate: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let colors = theme(cx).colors;
        let selector = id.clone();
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        div()
            .id(id)
            .debug_selector(move || selector.clone())
            .role(Role::Button)
            .aria_label(label.clone())
            .track_focus(focus)
            .tab_stop(enabled)
            .px_2()
            .py_1()
            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
            .text_size(px(Typography::BODY))
            .text_color(colors.text_secondary)
            .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, enabled)
            .focus_visible(move |style| style.border_1().border_color(colors.accent))
            .when(enabled, |button| button.cursor_pointer())
            .on_click(cx.listener(move |this, _, _, cx| {
                if enabled {
                    cx.stop_propagation();
                    activate(this, cx);
                }
            }))
            .on_key_down(cx.listener(move |this, event, _, cx| {
                if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    keyboard(this, cx);
                }
            }))
            .child(label)
    }
    pub(crate) fn render_automation(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut view = div()
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .flex()
            .flex_col()
            .gap_1()
            .key_context("Composer")
            .on_action(cx.listener(Self::next_composer_control))
            .on_action(cx.listener(Self::previous_composer_control));
        for task in &self.automation.projection.tasks {
            let Some(focus) = self.automation.focus.get(&task.identity) else {
                continue;
            };
            let identity = task.identity.clone();
            let expanded = self.automation.details.contains(&identity);
            let label = format!("{} · {}", task.kind.label(), task.status.label());
            let mut row =
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(self.automation_button(
                        format!("automation-{}-details", task.kind.key()),
                        label,
                        &focus[0],
                        true,
                        cx,
                        move |this, cx| {
                            if !this.automation.details.remove(&identity) {
                                this.automation.details.insert(identity.clone());
                            }
                            cx.notify();
                        },
                    ));
            if task.can_control() {
                let kind = task.kind;
                let (control, label) = if task.status == AutomationStatus::Active {
                    (AutomationControl::Pause, "暂停")
                } else {
                    (AutomationControl::Resume, "恢复")
                };
                row = row.child(self.automation_button(
                    format!("automation-{}-control", kind.key()),
                    label.into(),
                    &focus[1],
                    !self.automation.projection.busy,
                    cx,
                    move |this, cx| {
                        cx.emit(AutomationRequested {
                            thread_id: this.thread.id.clone(),
                            command: AutomationCommand::Control(kind, control),
                        });
                    },
                ));
            }
            let kind = task.kind;
            row = row.child(self.automation_button(
                format!("automation-{}-clear", kind.key()),
                "清除".into(),
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
            if task.kind == AutomationKind::Goal {
                let used = if task.usage_complete {
                    task.used_tokens.to_string()
                } else {
                    format!("用量未知（已记录 {}）", task.used_tokens)
                };
                let usage = match task.token_budget {
                    Some(budget) => format!("{used} / {budget} tokens"),
                    None => format!("{used} tokens"),
                };
                row = row.child(
                    div()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(usage),
                );
            } else if !self.actions.running
                && let Some(label) = self.automation.wakeup_labels.get(&task.identity)
            {
                row = row.child(
                    div()
                        .debug_selector(|| "automation-loop-wakeup".into())
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(label.clone()),
                );
            }
            view = view.child(row);
            if !matches!(
                task.status,
                AutomationStatus::Active | AutomationStatus::Complete
            ) && let Some(reason) = &task.reason
            {
                view = view.child(
                    div()
                        .px_2()
                        .text_size(px(Typography::BODY))
                        .text_color(colors.text_secondary)
                        .child(reason.clone()),
                );
            }
            if expanded {
                view = view.child(
                    div()
                        .id(format!("automation-{}-instruction", task.kind.key()))
                        .debug_selector(move || format!("automation-{}-instruction", kind.key()))
                        .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
                        .overflow_y_scroll()
                        .px_2()
                        .text_size(px(Typography::BODY))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().child(task.instruction.clone()))
                        .children(
                            task.reason
                                .clone()
                                .filter(|_| {
                                    matches!(
                                        task.status,
                                        AutomationStatus::Active | AutomationStatus::Complete
                                    )
                                })
                                .map(|reason| div().child(reason)),
                        ),
                );
            }
        }
        if let Some(error) = &self.automation.projection.error {
            view = view.child(
                div()
                    .debug_selector(|| "automation-error".into())
                    .text_size(px(Typography::BODY))
                    .text_color(colors.danger)
                    .child(error.clone()),
            );
        }
        view.into_any_element()
    }
}
