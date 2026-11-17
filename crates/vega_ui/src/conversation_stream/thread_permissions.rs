use super::*;
use vega_conversation::types::ThreadPermissionError;

mod native;

pub(crate) struct PermissionEditor {
    operation: u64,
    source: Option<Thread>,
    phase: Phase,
    pub(super) active_mode: Option<PermissionMode>,
    pub(super) trigger: FocusHandle,
    pub(super) options: [FocusHandle; 4],
    reload: FocusHandle,
    focus_pending: bool,
}

#[derive(Clone, PartialEq, Eq)]
enum Phase {
    Idle,
    Saving(u64),
    Reloading(u64),
    Failed(ThreadPermissionError),
}

impl PermissionEditor {
    pub(super) fn new(cx: &mut Context<ConversationStream>) -> Self {
        Self {
            operation: 0,
            source: None,
            phase: Phase::Idle,
            active_mode: None,
            trigger: cx.focus_handle().tab_stop(true),
            options: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            reload: cx.focus_handle().tab_stop(true),
            focus_pending: false,
        }
    }

    pub(super) fn busy(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }

    pub(super) fn progress_label(&self) -> Option<&'static str> {
        match self.phase {
            Phase::Saving(_) => Some(vega_i18n::t("ui.permission_saving")),
            Phase::Reloading(_) => Some(vega_i18n::t("ui.permission_reading")),
            _ => None,
        }
    }

    fn owns(&self, operation: u64) -> bool {
        matches!(self.phase, Phase::Saving(id) | Phase::Reloading(id) if id == operation)
    }
}

impl ConversationStream {
    pub fn apply_permission_reset(
        &mut self,
        thread: Thread,
        pending: PendingQueueProjection,
        automation: vega_conversation::types::AutomationProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.thread.id != thread.id || self.thread.project_id != thread.project_id {
            return false;
        }
        self.permission_editor.operation = self.permission_editor.operation.saturating_add(1);
        self.permission_editor.phase = Phase::Idle;
        self.permission_editor.source = None;
        self.permission_editor.active_mode = None;
        self.permission_picker_open = false;
        self.interrupted_tail = None;
        self.composer_resume_pending = false;
        self.permissions_revoked = true;
        self.invalidate_automation_permission(cx);
        self.timeout_permission(cx);
        self.cancel_user_questions(cx);
        self.invalidate_composer_prediction(cx);
        self.apply_thread(thread, cx);
        self.apply_pending_queue(pending, cx);
        self.apply_automation(automation, cx);
        self.refresh_ordinary_submissions(cx);
        true
    }

    pub fn begin_thread_permission_change(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(u64, Thread)> {
        if self.permission_change_pending()
            || matches!(self.permission_editor.phase, Phase::Failed(_))
            || self.model_selection_pending.is_some()
            || self.trusted_action_busy
            || self.context_operation_busy()
            || self.composer_submit_pending
            || self.thread.backend != TaskBackend::Native
        {
            return None;
        }
        let operation = self.permission_editor.operation.checked_add(1)?;
        self.permission_editor.operation = operation;
        self.permission_editor.source = Some(self.thread.clone());
        self.permission_editor.phase = Phase::Saving(operation);
        self.permission_editor.focus_pending = false;
        self.permission_picker_open = false;
        cx.notify();
        Some((operation, self.thread.clone()))
    }

    pub fn finish_thread_permission_change(
        &mut self,
        operation: u64,
        outcome: Result<Thread, ThreadPermissionError>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.permission_editor.owns(operation) {
            return false;
        }
        if self
            .permission_editor
            .source
            .as_ref()
            .is_some_and(|source| !self.permission_source_matches(source))
        {
            self.permission_editor.phase = Phase::Failed(ThreadPermissionError::Conflict);
            self.permission_editor.focus_pending = true;
            cx.notify();
            return false;
        }
        match outcome {
            Ok(saved)
                if saved.id == self.thread.id && saved.project_id == self.thread.project_id =>
            {
                let mut next = self.thread.clone();
                next.permission_mode = saved.permission_mode;
                next.permission_revision = saved.permission_revision;
                next.draft_authority_epoch = saved.draft_authority_epoch;
                next.full_access_grant = saved.full_access_grant;
                next.default_permission = saved.default_permission;
                next.created_at = saved.created_at;
                next.updated_at = next.updated_at.max(saved.updated_at);
                self.apply_thread(next, cx);
                self.permission_editor.phase = Phase::Idle;
                self.permission_editor.source = None;
                self.permission_editor.focus_pending = false;
                cx.notify();
                true
            }
            Ok(_) => {
                self.permission_editor.phase = Phase::Failed(ThreadPermissionError::Conflict);
                self.permission_editor.focus_pending = true;
                cx.notify();
                false
            }
            Err(error) => {
                self.permission_editor.phase = Phase::Failed(error);
                self.permission_editor.focus_pending = true;
                cx.notify();
                false
            }
        }
    }

    pub fn owns_thread_permission_operation(&self, operation: u64) -> bool {
        self.permission_editor.owns(operation)
    }

    fn permission_source_matches(&self, source: &Thread) -> bool {
        source.id == self.thread.id
            && source.project_id == self.thread.project_id
            && source.mode == self.thread.mode
            && source.model == self.thread.model
            && source.permission_mode == self.thread.permission_mode
            && source.permission_revision == self.thread.permission_revision
            && source.draft_authority_epoch == self.thread.draft_authority_epoch
    }

    fn reload_thread_permissions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.permission_editor.phase, Phase::Failed(_)) {
            return;
        }
        let Some(operation) = self.permission_editor.operation.checked_add(1) else {
            return;
        };
        self.permission_editor.operation = operation;
        self.permission_editor.source = Some(self.thread.clone());
        self.permission_editor.phase = Phase::Reloading(operation);
        self.permission_editor.focus_pending = false;
        window.focus(&self.permission_editor.trigger, cx);
        cx.emit(ThreadPermissionReloadRequested {
            operation,
            source: self.thread.clone(),
        });
        cx.notify();
    }

    pub(crate) fn prepare_thread_permission_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if std::mem::take(&mut self.permission_editor.focus_pending)
            && matches!(self.permission_editor.phase, Phase::Failed(_))
        {
            window.focus(&self.permission_editor.reload, cx);
        }
    }

    pub(crate) fn render_thread_permission_feedback(&self, cx: &mut Context<Self>) -> AnyElement {
        native::render(self, cx)
    }
}
