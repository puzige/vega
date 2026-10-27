use super::*;
use crate::interaction::InteractionExt;
use vega_conversation::types::{ThreadPermissionChange, ThreadPermissionError};

pub(crate) struct PermissionEditor {
    operation: u64,
    source: Option<Thread>,
    phase: Phase,
    pub(super) active_mode: Option<PermissionMode>,
    pub(super) trigger: FocusHandle,
    pub(super) options: [FocusHandle; 4],
    cancel: FocusHandle,
    confirm: FocusHandle,
    reload: FocusHandle,
    focus_pending: bool,
}

enum Phase {
    Idle,
    Preparing(u64),
    Confirming(u64, Box<ThreadPermissionChange>),
    Saving(u64),
    Reloading(u64),
    Failed(ThreadPermissionError),
}

#[derive(Clone, Copy)]
enum Action {
    Cancel,
    Confirm,
    Reload,
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
            cancel: cx.focus_handle().tab_stop(true),
            confirm: cx.focus_handle().tab_stop(true),
            reload: cx.focus_handle().tab_stop(true),
            focus_pending: false,
        }
    }

    pub(super) fn busy(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }

    fn owns(&self, operation: u64) -> bool {
        matches!(self.phase, Phase::Preparing(id) | Phase::Saving(id) | Phase::Reloading(id) | Phase::Confirming(id, _) if id == operation)
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
        mode: PermissionMode,
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
        self.permission_editor.phase = if mode == PermissionMode::FullAccess {
            Phase::Preparing(operation)
        } else {
            Phase::Saving(operation)
        };
        self.permission_editor.focus_pending = mode == PermissionMode::FullAccess;
        self.permission_picker_open = false;
        cx.notify();
        Some((operation, self.thread.clone()))
    }

    pub fn show_thread_permission_confirmation(
        &mut self,
        operation: u64,
        change: ThreadPermissionChange,
        cx: &mut Context<Self>,
    ) {
        if !self.permission_editor.owns(operation) {
            return;
        }
        if !self.permission_source_matches(change.source()) {
            self.finish_thread_permission_change(
                operation,
                Err(ThreadPermissionError::Conflict),
                cx,
            );
            return;
        }
        self.permission_editor.phase = Phase::Confirming(operation, Box::new(change));
        self.permission_editor.focus_pending = true;
        cx.notify();
    }

    pub fn begin_thread_permission_save(
        &mut self,
        submitted: &ThreadPermissionConfirmed,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(&self.permission_editor.phase, Phase::Confirming(id, change) if *id == submitted.operation && **change == submitted.change)
            || !self.permission_source_matches(submitted.change.source())
        {
            return false;
        }
        self.permission_editor.phase = Phase::Saving(submitted.operation);
        cx.notify();
        true
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
                self.permission_editor.focus_pending = true;
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

    pub fn cancel_thread_permission_change(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.permission_editor.phase,
            Phase::Preparing(_) | Phase::Confirming(_, _)
        ) {
            self.permission_editor.phase = Phase::Idle;
            self.permission_editor.source = None;
            self.permission_editor.focus_pending = true;
            cx.notify();
        }
    }

    fn permission_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::Cancel => self.cancel_thread_permission_change(cx),
            Action::Confirm => {
                if let Phase::Confirming(operation, change) = &self.permission_editor.phase {
                    cx.emit(ThreadPermissionConfirmed {
                        operation: *operation,
                        change: (**change).clone(),
                    });
                }
            }
            Action::Reload => {
                let Some(operation) = self.permission_editor.operation.checked_add(1) else {
                    return;
                };
                self.permission_editor.operation = operation;
                self.permission_editor.source = Some(self.thread.clone());
                self.permission_editor.phase = Phase::Reloading(operation);
                cx.emit(ThreadPermissionReloadRequested {
                    operation,
                    source: self.thread.clone(),
                });
                cx.notify();
            }
        }
        if matches!(self.permission_editor.phase, Phase::Idle) {
            window.focus(&self.permission_editor.trigger, cx);
        }
    }

    pub(crate) fn prepare_thread_permission_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !std::mem::take(&mut self.permission_editor.focus_pending) {
            return;
        }
        let focus = match self.permission_editor.phase {
            Phase::Preparing(_) | Phase::Confirming(_, _) => &self.permission_editor.cancel,
            Phase::Failed(_) => &self.permission_editor.reload,
            Phase::Idle => &self.permission_editor.trigger,
            _ => return,
        };
        window.focus(focus, cx);
    }

    fn thread_permission_button(
        &self,
        action: Action,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let (id, focus) = match action {
            Action::Cancel => ("thread-permission-cancel", &self.permission_editor.cancel),
            Action::Confirm => ("thread-permission-confirm", &self.permission_editor.confirm),
            Action::Reload => ("thread-permission-reload", &self.permission_editor.reload),
        };
        div()
            .id(id)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .track_focus(focus)
            .tab_stop(true)
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, true)
            .focus_visible(move |style| style.border_color(colors.accent))
            .cursor_pointer()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.permission_action(action, window, cx)),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        this.permission_action(action, window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .child(label)
            .into_any_element()
    }

    pub(crate) fn render_thread_permission_feedback(&self, cx: &mut Context<Self>) -> AnyElement {
        if matches!(self.permission_editor.phase, Phase::Idle) {
            return div().into_any_element();
        }
        let colors = theme(cx).colors;
        let panel = div()
            .id("thread-permission-feedback")
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .mb_2()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .border_1()
            .rounded(px(Layout::PANEL_RADIUS))
            .border_color(colors.border_subtle)
            .text_size(px(Typography::BODY))
            .text_color(colors.text_primary)
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "tab"
                        && matches!(this.permission_editor.phase, Phase::Confirming(_, _))
                    {
                        let focus = if this.permission_editor.cancel.is_focused(window) {
                            &this.permission_editor.confirm
                        } else {
                            &this.permission_editor.cancel
                        };
                        window.focus(focus, cx);
                        cx.stop_propagation();
                    } else if event.keystroke.key == "escape" {
                        this.permission_action(Action::Cancel, window, cx);
                        cx.stop_propagation();
                    }
                }),
            );
        match self.permission_editor.phase {
            Phase::Confirming(_, _) => panel
                .child(vega_i18n::t("ui.permission_confirm_thread"))
                .child(vega_i18n::t("ui.permission_thread_scope"))
                .child(
                    div()
                        .text_color(colors.warning)
                        .child(vega_i18n::t("settings.permission.full_access_risk")),
                )
                .child(vega_i18n::t("settings.permission.persistent_effects"))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(self.thread_permission_button(
                            Action::Cancel,
                            vega_i18n::t("ui.cancel"),
                            cx,
                        ))
                        .child(self.thread_permission_button(
                            Action::Confirm,
                            vega_i18n::t("ui.permission_enable_thread"),
                            cx,
                        )),
                )
                .into_any_element(),
            Phase::Preparing(_) => panel
                .child(vega_i18n::t("ui.permission_reading"))
                .child(self.thread_permission_button(Action::Cancel, vega_i18n::t("ui.cancel"), cx))
                .into_any_element(),
            Phase::Saving(_) => panel
                .child(vega_i18n::t("ui.permission_saving"))
                .into_any_element(),
            Phase::Reloading(_) => panel
                .child(vega_i18n::t("ui.permission_reading"))
                .into_any_element(),
            Phase::Failed(error) => {
                let key = match error {
                    ThreadPermissionError::Conflict => "ui.permission_error_conflict",
                    ThreadPermissionError::ResetPending => "ui.permission_error_reset",
                    ThreadPermissionError::OutcomeUnknown => "ui.permission_error_unknown",
                    ThreadPermissionError::ConfirmationRequired => {
                        "ui.permission_error_confirmation"
                    }
                    ThreadPermissionError::Unavailable => "ui.permission_error_unavailable",
                };
                panel
                    .child(div().text_color(colors.danger).child(vega_i18n::t(key)))
                    .child(self.thread_permission_button(
                        Action::Reload,
                        vega_i18n::t("ui.permission_reload"),
                        cx,
                    ))
                    .into_any_element()
            }
            Phase::Idle => panel.into_any_element(),
        }
    }
}
