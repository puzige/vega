use super::*;

mod render;
use vega_conversation::types::{
    AutomationPermissionChange, AutomationPermissionConfirmed, AutomationPermissionReloadRequested,
    AutomationPermissionRequested, AutomationProjection, AutomationStatus, AutomationTask,
    ThreadPermissionError as Error,
};

pub(crate) struct AutomationPermissionUi {
    operation: u64,
    task_id: Option<String>,
    original_input: Option<String>,
    phase: Phase,
    cancel: FocusHandle,
    confirm: FocusHandle,
    reload: FocusHandle,
    details: FocusHandle,
    details_scroll: ScrollHandle,
    focus_pending: bool,
}

enum Phase {
    Idle,
    Preparing(Box<AutomationPermissionRequested>),
    Confirming(Box<AutomationPermissionChange>),
    Saving,
    Reloading,
    Failed(Error),
}

#[derive(Clone, Copy)]
enum Action {
    Cancel,
    Confirm,
    Reload,
}

impl AutomationPermissionUi {
    pub(super) fn new(cx: &mut Context<ConversationStream>) -> Self {
        Self {
            operation: 0,
            task_id: None,
            original_input: None,
            phase: Phase::Idle,
            cancel: cx.focus_handle().tab_stop(true),
            confirm: cx.focus_handle().tab_stop(true),
            reload: cx.focus_handle().tab_stop(true),
            details: cx.focus_handle().tab_stop(true),
            details_scroll: ScrollHandle::new(),
            focus_pending: false,
        }
    }
    pub(super) fn busy(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }
}

impl EventEmitter<AutomationPermissionRequested> for ConversationStream {}
impl EventEmitter<AutomationPermissionConfirmed> for ConversationStream {}
impl EventEmitter<AutomationPermissionReloadRequested> for ConversationStream {}

impl ConversationStream {
    pub fn permission_change_pending(&self) -> bool {
        self.permission_editor.busy() || self.automation_permission.busy()
    }

    pub fn request_automation_permission(
        &mut self,
        task: AutomationTask,
        resume: bool,
        original_input: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.permission_change_pending()
            || self.reading_preview
            || self.thread.backend != TaskBackend::Native
            || self.actions.running
            || self.composer_submit_pending
            || self.trusted_action_busy
            || self.model_selection_pending.is_some()
            || self.context_operation_busy()
            || self.automation.projection.busy
            || self.thread.permission_mode != PermissionMode::FullAccess
            || self.thread.mode != ThreadMode::Execute
            || task.status.replaceable()
            || (resume && task.status == AutomationStatus::Active)
        {
            return;
        }
        let Some(operation) = self.automation_permission.operation.checked_add(1) else {
            return;
        };
        let request = AutomationPermissionRequested {
            operation,
            source: self.thread.clone(),
            task: task.clone(),
            resume,
        };
        self.automation_permission.operation = operation;
        self.automation_permission.task_id = Some(task.identity);
        self.automation_permission.original_input = original_input;
        self.automation_permission
            .details_scroll
            .set_offset(gpui_kit::point(px(0.), px(0.)));
        self.automation_permission.phase = Phase::Preparing(Box::new(request.clone()));
        self.automation_permission.focus_pending = true;
        self.permission_picker_open = false;
        cx.emit(request);
        cx.notify();
    }

    pub fn owns_automation_permission_operation(&self, operation: u64) -> bool {
        self.automation_permission.operation == operation
            && matches!(
                self.automation_permission.phase,
                Phase::Preparing(_) | Phase::Confirming(_) | Phase::Saving | Phase::Reloading
            )
    }

    pub fn show_automation_permission_confirmation(
        &mut self,
        operation: u64,
        change: AutomationPermissionChange,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_automation_permission_operation(operation) {
            return;
        }
        let valid = matches!(&self.automation_permission.phase, Phase::Preparing(request) if request.task == *change.task() && request.resume == change.resumes_task() && request.source.permission_revision == self.thread.permission_revision && request.source.full_access_grant == self.thread.full_access_grant)
            && self
                .automation
                .projection
                .tasks
                .iter()
                .any(|task| task == change.task());
        if !valid {
            self.fail_automation_permission(operation, Error::Conflict, cx);
            return;
        }
        self.automation_permission.phase = Phase::Confirming(Box::new(change));
        self.automation_permission.focus_pending = true;
        cx.notify();
    }

    pub fn begin_automation_permission_save(
        &mut self,
        request: &AutomationPermissionConfirmed,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.automation_permission.operation != request.operation
            || !matches!(&self.automation_permission.phase, Phase::Confirming(change) if **change == request.change)
        {
            return false;
        }
        self.automation_permission.phase = Phase::Saving;
        cx.notify();
        true
    }

    pub fn finish_automation_permission_save(
        &mut self,
        operation: u64,
        result: Result<Vec<AutomationTask>, Error>,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_automation_permission_operation(operation) {
            return;
        }
        match result {
            Ok(tasks) => {
                self.automation_permission.phase = Phase::Idle;
                self.automation_permission.focus_pending = true;
                if let Some(original) = self.automation_permission.original_input.take() {
                    self.accept_automation_command(&original, cx);
                }
                self.apply_automation(
                    AutomationProjection {
                        tasks,
                        busy: false,
                        error: None,
                    },
                    cx,
                );
            }
            Err(error) => self.fail_automation_permission(operation, error, cx),
        }
        cx.notify();
    }

    pub fn finish_automation_permission_reload(
        &mut self,
        operation: u64,
        result: Result<(Thread, Vec<AutomationTask>), Error>,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_automation_permission_operation(operation) {
            return;
        }
        match result {
            Ok((thread, tasks))
                if thread.id == self.thread.id && thread.project_id == self.thread.project_id =>
            {
                self.automation_permission.phase = Phase::Idle;
                self.automation_permission.focus_pending = true;
                self.apply_thread(thread, cx);
                self.apply_automation(
                    AutomationProjection {
                        tasks,
                        busy: false,
                        error: None,
                    },
                    cx,
                );
            }
            Ok(_) => self.fail_automation_permission(operation, Error::Conflict, cx),
            Err(error) => self.fail_automation_permission(operation, error, cx),
        }
        cx.notify();
    }

    pub fn fail_automation_permission(
        &mut self,
        operation: u64,
        error: Error,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_automation_permission_operation(operation) {
            return;
        }
        self.automation_permission.phase = Phase::Failed(error);
        self.automation_permission.focus_pending = true;
        cx.notify();
    }

    pub fn cancel_automation_permission(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.automation_permission.phase,
            Phase::Preparing(_) | Phase::Confirming(_)
        ) {
            self.invalidate_automation_permission(cx);
        }
    }

    pub(crate) fn invalidate_automation_permission(&mut self, cx: &mut Context<Self>) {
        self.automation_permission.operation =
            self.automation_permission.operation.saturating_add(1);
        self.automation_permission.phase = Phase::Idle;
        self.automation_permission.original_input = None;
        self.automation_permission.focus_pending = true;
        cx.notify();
    }

    pub(crate) fn reconcile_automation_permission(&mut self, projection: &AutomationProjection) {
        let expected = match &self.automation_permission.phase {
            Phase::Preparing(request) => Some(&request.task),
            Phase::Confirming(change) => Some(change.task()),
            _ => None,
        };
        if expected.is_some_and(|expected| !projection.tasks.iter().any(|task| task == expected)) {
            self.automation_permission.phase = Phase::Failed(Error::Conflict);
            self.automation_permission.focus_pending = true;
        }
    }

    pub(crate) fn prepare_automation_permission_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !std::mem::take(&mut self.automation_permission.focus_pending) {
            return;
        }
        let focus = match self.automation_permission.phase {
            Phase::Preparing(_) | Phase::Confirming(_) => Some(&self.automation_permission.cancel),
            Phase::Failed(_) => Some(&self.automation_permission.reload),
            Phase::Idle => self
                .automation_permission
                .task_id
                .as_ref()
                .and_then(|id| self.automation.focus.get(id))
                .map(|focus| &focus[3]),
            _ => None,
        };
        if let Some(focus) = focus {
            window.focus(focus, cx);
        }
    }

    fn automation_permission_action(&mut self, action: Action, cx: &mut Context<Self>) {
        match action {
            Action::Cancel => self.cancel_automation_permission(cx),
            Action::Confirm => {
                if let Phase::Confirming(change) = &self.automation_permission.phase {
                    cx.emit(AutomationPermissionConfirmed {
                        operation: self.automation_permission.operation,
                        change: (**change).clone(),
                    });
                }
            }
            Action::Reload if matches!(self.automation_permission.phase, Phase::Failed(_)) => {
                let Some(operation) = self.automation_permission.operation.checked_add(1) else {
                    return;
                };
                self.automation_permission.operation = operation;
                self.automation_permission.phase = Phase::Reloading;
                cx.emit(AutomationPermissionReloadRequested {
                    operation,
                    thread_id: self.thread.id.clone(),
                });
                cx.notify();
            }
            _ => {}
        }
    }
}
