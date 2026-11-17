use super::automation_permission::native::{Intent, control};
use super::*;
use crate::automation_editor::format_elapsed;
use vega_conversation::types::{
    AutomationCommand, AutomationKind, AutomationProjection, AutomationStatus,
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
    pub error: Option<crate::localized_copy::LocalizedCopy>,
    pub editor_dirty: bool,
    pub timings: HashMap<String, AutomationTiming>,
    pub focus: HashMap<String, [FocusHandle; 4]>,
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
        if self.automation.timings != timings {
            self.automation.timings = timings;
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
        let error = projection.error.as_ref().map(|error| error.clone().into());
        self.apply_automation_localized(projection, error, cx);
    }
    pub fn apply_automation_localized(
        &mut self,
        projection: AutomationProjection,
        error: Option<crate::localized_copy::LocalizedCopy>,
        cx: &mut Context<Self>,
    ) {
        self.reconcile_automation_permission(&projection);
        let error_changed = self.automation.error.as_ref().map(|error| error.render())
            != error.as_ref().map(|error| error.render());
        self.automation.error = error;
        for task in &projection.tasks {
            self.automation
                .focus
                .entry(task.identity.clone())
                .or_insert_with(|| std::array::from_fn(|_| cx.focus_handle().tab_stop(true)));
        }
        self.automation
            .focus
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
                    vega_i18n::t("ui.waiting_to_run").to_string()
                } else if remaining <= 60_000 {
                    "<1m".to_string()
                } else {
                    vega_i18n::t_args(
                        "ui.in_minutes_m",
                        &[(
                            "minutes",
                            &(remaining.saturating_add(59_999) / 60_000).to_string(),
                        )],
                    )
                };
                (task.identity.clone(), label)
            })
            .collect();
        if error_changed
            || self.automation.projection != projection
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
            && !self.permission_change_pending()
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
                        [3, 2, 1, 0]
                            .into_iter()
                            .filter(move |index| {
                                (*index != 3
                                    || (self.thread.permission_mode == PermissionMode::FullAccess
                                        && self.thread.mode == ThreadMode::Execute
                                        && !task.status.replaceable()))
                                    && (*index != 1 || task.can_control())
                                    && (*index == 0
                                        || (!self.automation.projection.busy
                                            && !self.permission_change_pending()))
                            })
                            .map(|index| focus[index].clone())
                    })
            })
            .collect()
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
            .flex_col();
        if let Some(error) = &self.automation.error {
            let mut error = control(
                self,
                "automation-error".to_owned(),
                error.render(),
                Intent::Read,
                None,
                None,
                cx,
            );
            error.foreground = colors.danger;
            view = view.child(error);
        }
        if self.thread.backend != TaskBackend::Native {
            return view.into_any_element();
        }
        for task in &self.automation.projection.tasks {
            if !matches!(
                task.status,
                AutomationStatus::Active | AutomationStatus::Complete
            ) && let Some(reason) = task.reason_message()
                && !(task.status == AutomationStatus::Paused && task.is_user_paused())
            {
                view = view.child(control(
                    self,
                    format!("automation-{}-reason", task.identity),
                    reason.to_owned(),
                    Intent::Read,
                    Some(task),
                    None,
                    cx,
                ));
            }
            let Some(focus) = self.automation.focus.get(&task.identity) else {
                continue;
            };
            let kind = task.kind;
            let label = if compact {
                if task.status == AutomationStatus::BudgetLimited {
                    vega_i18n::t("ui.budget_exhausted").to_owned()
                } else {
                    task.status.label().to_owned()
                }
            } else if task.status == AutomationStatus::Active {
                kind.label().to_owned()
            } else {
                vega_i18n::t_args(
                    "ui.automation_status",
                    &[("kind", kind.label()), ("status", task.status.label())],
                )
            };
            let summary = task
                .instruction
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let mut row = div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(Layout::AUTOMATION_GAP));
            row = row.child(
                div()
                    .flex_1()
                    .min_w(px(Layout::AUTOMATION_ACTION_SIZE))
                    .child(control(
                        self,
                        format!("automation-{}-summary", task.identity),
                        format!("{label} · {summary}"),
                        Intent::Edit,
                        Some(task),
                        Some(focus[0].clone()),
                        cx,
                    )),
            );
            if !compact {
                let time = if kind == AutomationKind::Loop
                    && !self.actions.running
                    && !self
                        .automation
                        .timings
                        .get(&task.identity)
                        .is_some_and(|time| time.running)
                {
                    self.automation.wakeup_labels.get(&task.identity).cloned()
                } else {
                    None
                };
                let time = time.unwrap_or_else(|| {
                    format_elapsed(
                        self.automation
                            .timings
                            .get(&task.identity)
                            .map_or(task.elapsed_ms, |time| time.elapsed_ms),
                    )
                });
                let mut clock = control(
                    self,
                    format!("automation-{}-clock", task.identity),
                    time,
                    Intent::Read,
                    Some(task),
                    None,
                    cx,
                );
                clock.foreground = colors.text_tertiary;
                row = row.child(
                    div()
                        .w(px(Layout::AUTOMATION_ACTION_SIZE * 2.))
                        .flex_shrink_0()
                        .child(clock),
                );
            }
            let mut actions = div().flex().flex_wrap().gap(px(Layout::AUTOMATION_GAP));
            if self.thread.permission_mode == PermissionMode::FullAccess
                && self.thread.mode == ThreadMode::Execute
                && !task.status.replaceable()
            {
                actions = actions.child(
                    div()
                        .w(px(Layout::AUTOMATION_ACTION_SIZE * 6.))
                        .child(self.render_automation_permission_trigger(task, &focus[3], cx)),
                );
            }
            actions = actions.child(div().w(px(Layout::AUTOMATION_ACTION_SIZE)).child(control(
                self,
                format!("automation-{}-clear", task.identity),
                vega_i18n::t_args("ui.clear_kind", &[("kind", kind.label())]),
                Intent::Clear,
                Some(task),
                Some(focus[2].clone()),
                cx,
            )));
            if task.can_control() {
                actions =
                    actions.child(div().w(px(Layout::AUTOMATION_ACTION_SIZE)).child(control(
                        self,
                        format!("automation-{}-control", task.identity),
                        vega_i18n::t_args(
                            if task.status == AutomationStatus::Active {
                                "ui.pause_kind"
                            } else {
                                "ui.resume_kind"
                            },
                            &[("kind", kind.label())],
                        ),
                        Intent::Control,
                        Some(task),
                        Some(focus[1].clone()),
                        cx,
                    )));
            }
            view = view.child(row.child(actions));
        }
        view.into_any_element()
    }
}
