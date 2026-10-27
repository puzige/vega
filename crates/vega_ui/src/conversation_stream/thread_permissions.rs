use super::*;
use crate::interaction::InteractionExt;
use vega_conversation::types::{
    FullAccessGrant, FullAccessScope, ThreadPermissionError, ThreadStatus,
};

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
    pub(super) fn full_access_selection_needs_save(&self, cx: &App) -> bool {
        if self.thread.permission_mode != PermissionMode::FullAccess {
            return false;
        }
        if let Some(grant) = &self.thread.full_access_grant {
            return self.thread.default_permission.is_some()
                || !grant.is_scoped_to(&FullAccessScope::Thread {
                    thread_id: self.thread.id.clone(),
                });
        }
        let Some(snapshot) = &self.thread.default_permission else {
            return true;
        };
        if self.thread.backend != TaskBackend::Native
            || self.thread.status != ThreadStatus::Active
            || self.thread.parent_thread_id.is_some()
            || self.thread.forked_from_thread_id.is_some()
            || self.thread.forked_from_message_id.is_some()
            || self.thread.permission_revision != 0
            || snapshot.config_path.is_none()
            || snapshot.permission_revision < 0
            || snapshot.permission_mode != self.thread.permission_mode.as_str()
            || cx
                .try_global::<crate::sidebar::NewTaskDefaults>()
                .is_none_or(|defaults| &defaults.permission != snapshot)
        {
            return true;
        }
        let Some(configuration_id) = &snapshot.configuration_id else {
            return true;
        };
        snapshot
            .full_access_grant
            .as_deref()
            .and_then(|value| serde_json::from_str::<FullAccessGrant>(value).ok())
            .is_none_or(|grant| {
                !grant.is_scoped_to(&FullAccessScope::FutureThreads {
                    configuration_id: configuration_id.clone(),
                })
            })
    }

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
            && source.full_access_grant == self.thread.full_access_grant
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
        let Phase::Failed(error) = self.permission_editor.phase else {
            return div().into_any_element();
        };
        let key = match error {
            ThreadPermissionError::Conflict => "ui.permission_error_conflict",
            ThreadPermissionError::ResetPending => "ui.permission_error_reset",
            ThreadPermissionError::OutcomeUnknown => "ui.permission_error_unknown",
            ThreadPermissionError::ConfirmationRequired => "ui.permission_error_confirmation",
            ThreadPermissionError::Unavailable => "ui.permission_error_unavailable",
        };
        let colors = theme(cx).colors;
        let reload_label = vega_i18n::t("ui.permission_reload");
        div()
            .id("thread-permission-feedback")
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .mb_2()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(Typography::METADATA))
            .child(
                div()
                    .flex_1()
                    .text_color(colors.danger)
                    .child(vega_i18n::t(key)),
            )
            .child(
                div()
                    .id("thread-permission-reload")
                    .role(gpui_kit::Role::Button)
                    .aria_label(reload_label)
                    .track_focus(&self.permission_editor.reload)
                    .tab_stop(true)
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_color(colors.text_primary)
                    .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, true)
                    .focus_visible(move |style| style.border_1().border_color(colors.accent))
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.reload_thread_permissions(window, cx)
                        }),
                    )
                    .on_key_down(
                        cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.reload_thread_permissions(window, cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .child(reload_label),
            )
            .into_any_element()
    }
}
