use super::host::State;
use super::*;

pub struct CommitPanel {
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
    pub(crate) model: CommitPanelModel,
    pub(crate) message: Entity<TextInput>,
    pub(crate) focus: FocusHandle,
    pub(crate) cancel_focus: FocusHandle,
    pub(super) presentation: State,
    pub(crate) disabled: bool,
    pub(crate) editor_revision: u64,
    pub(crate) editor_revision_overflow: bool,
    pub(crate) draft_revision: Option<(CommitOperationId, u64)>,
    pub(crate) focus_cancel_pending: bool,
}

impl EventEmitter<CommitChecklistRequested> for CommitPanel {}
impl EventEmitter<CommitPrepareRequested> for CommitPanel {}
impl EventEmitter<CommitDraftRequested> for CommitPanel {}
impl EventEmitter<CommitRequested> for CommitPanel {}
impl EventEmitter<CommitPanelClosed> for CommitPanel {}

impl Focusable for CommitPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl CommitPanel {
    pub fn new(thread_id: String, project_id: String, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| {
            TextInput::new_multiline(cx, "", 4)
                .with_placeholder_key("ui.commit_message_enter_newline_cmd_enter_commit")
                .with_scroll_fade_on_base()
        });
        cx.observe(&message, |this, _, cx| {
            this.reconcile_carrier(cx);
            cx.notify();
        })
        .detach();
        let cancel_focus = cx.focus_handle().tab_stop(true);
        Self {
            thread_id,
            project_id,
            model: CommitPanelModel::default(),
            message,
            focus: cancel_focus.clone(),
            cancel_focus,
            presentation: State::default(),
            disabled: false,
            editor_revision: 0,
            editor_revision_overflow: false,
            draft_revision: None,
            focus_cancel_pending: false,
        }
    }

    pub fn route(&self) -> (&str, &str) {
        (&self.thread_id, &self.project_id)
    }

    pub fn rebind_draft_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if self.project_id == project_id {
            return;
        }
        self.project_id = project_id.to_owned();
        self.presentation.unmount();
        self.model = CommitPanelModel::default();
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.model.is_open()
    }

    pub fn stage(&self) -> CommitPanelStage {
        self.model.stage()
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        if let Ok(mut draft) = self.presentation.draft.0.lock() {
            draft.editable = !disabled
                && matches!(
                    self.model.stage(),
                    CommitPanelStage::CommitReady | CommitPanelStage::Drafting
                );
        }
        cx.notify();
    }

    pub fn request_open(&mut self, cx: &mut Context<Self>) -> bool {
        if self.disabled || !self.model.open() {
            return false;
        }
        self.presentation.unmount();
        cx.emit(CommitChecklistRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
        cx.notify();
        true
    }

    pub fn apply_checklist(&mut self, checklist: CommitChecklist, cx: &mut Context<Self>) -> bool {
        let accepted = self.model.apply_checklist(checklist);
        if accepted {
            cx.notify();
        }
        accepted
    }

    pub fn apply_error(
        &mut self,
        expected: CommitPanelStage,
        code: CommitErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.model.stage != expected || self.model.pending.is_some() {
            return false;
        }
        self.model.stage = CommitPanelStage::Failed(code);
        self.model.focus = CommitPanelFocus::Cancel;
        self.focus_cancel_pending = true;
        cx.notify();
        true
    }

    pub fn finish_prepare(
        &mut self,
        operation: CommitOperationId,
        prepared: Result<PreparedCommit, CommitErrorCode>,
        cx: &mut Context<Self>,
    ) -> bool {
        let accepted = self.model.finish_prepare(operation, prepared);
        if accepted {
            self.focus_cancel_pending = matches!(
                self.model.stage(),
                CommitPanelStage::CommitReady | CommitPanelStage::Failed(_)
            );
            cx.notify();
        }
        accepted
    }

    pub fn finish_draft(
        &mut self,
        operation: CommitOperationId,
        draft: Result<CommitDraft, CommitErrorCode>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.model.owns_pending(operation) || self.model.stage() != CommitPanelStage::Drafting {
            return false;
        }
        self.reconcile_carrier(cx);
        let revision_current = self.draft_revision_is_current(operation);
        let shared = self.presentation.draft.clone();
        let Ok(mut value) = shared.0.lock() else {
            return false;
        };
        let accepted = super::draft::finish_generated(
            &mut self.model,
            &mut self.draft_revision,
            &mut value,
            operation,
            revision_current,
            draft.map(|draft| draft.text().to_owned()),
        );
        self.editor_revision = value.revision;
        self.editor_revision_overflow = value.overflow;
        let text = value.text.clone();
        drop(value);
        self.update_carrier(&text, cx);
        if accepted {
            self.focus_cancel_pending = matches!(self.model.stage(), CommitPanelStage::Failed(_));
            cx.notify();
        }
        accepted
    }

    pub(crate) fn draft_revision_is_current(&self, operation: CommitOperationId) -> bool {
        !self.editor_revision_overflow
            && self.presentation.draft.0.lock().is_ok_and(|value| {
                !value.overflow
                    && !value.composing
                    && value.revision >= self.editor_revision
                    && self.draft_revision == Some((operation, value.revision))
            })
    }

    pub fn finish_commit(
        &mut self,
        operation: CommitOperationId,
        error: Option<CommitErrorCode>,
        cx: &mut Context<Self>,
    ) -> bool {
        let accepted = self.model.finish_commit(operation, error);
        if accepted {
            self.focus_cancel_pending = matches!(self.model.stage(), CommitPanelStage::Failed(_));
            cx.notify();
        }
        accepted
    }

    pub fn owns_pending(&self, operation: CommitOperationId) -> bool {
        self.model.owns_pending(operation)
    }

    pub fn clear_pending(&mut self, operation: CommitOperationId, cx: &mut Context<Self>) -> bool {
        let cleared = self.model.clear_pending(operation);
        if cleared {
            cx.notify();
        }
        cleared
    }

    pub fn fail_pending(
        &mut self,
        operation: CommitOperationId,
        code: CommitErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        let failed = self.model.fail_pending(operation, code);
        if failed {
            self.draft_revision = None;
            self.focus_cancel_pending = true;
            cx.notify();
        }
        failed
    }

    pub fn request_close(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.model.close_visible() {
            return false;
        }
        self.presentation.unmount();
        cx.emit(CommitPanelClosed {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
        cx.notify();
        true
    }

    pub fn suspend_presentation(&mut self, cx: &mut Context<Self>) {
        self.presentation.unmount();
        cx.notify();
    }

    pub(super) fn confirm_message(&mut self, message: String, cx: &mut Context<Self>) {
        match self.model.stage() {
            CommitPanelStage::Checklist => {
                if let Some((snapshot_id, selected, operation_id)) = self.model.begin_prepare() {
                    cx.emit(CommitPrepareRequested {
                        thread_id: self.thread_id.clone(),
                        project_id: self.project_id.clone(),
                        snapshot_id,
                        selected,
                        operation_id,
                    });
                }
            }
            CommitPanelStage::CommitReady => {
                if let Some((prepared_id, operation_id, message)) = self.model.begin_commit(message)
                {
                    cx.emit(CommitRequested {
                        thread_id: self.thread_id.clone(),
                        project_id: self.project_id.clone(),
                        prepared_id,
                        operation_id,
                        message,
                    });
                } else if matches!(self.model.stage(), CommitPanelStage::Failed(_)) {
                    self.focus_cancel_pending = true;
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn draft_message(&mut self, revision: u64, overflow: bool, cx: &mut Context<Self>) {
        if overflow {
            self.model.stage = CommitPanelStage::Failed(CommitErrorCode::OutputTooLarge);
            self.model.focus = CommitPanelFocus::Cancel;
            self.focus_cancel_pending = true;
            cx.notify();
            return;
        }
        if let Some((prepared_id, operation_id)) = self.model.begin_draft() {
            self.draft_revision = Some((operation_id, revision));
            cx.emit(CommitDraftRequested {
                thread_id: self.thread_id.clone(),
                project_id: self.project_id.clone(),
                prepared_id,
                operation_id,
            });
            cx.notify();
        }
    }

    pub(super) fn toggle_row(&mut self, id: WorkspaceFileId, cx: &mut Context<Self>) {
        if self.model.toggle(id) {
            cx.notify();
        }
    }
}

impl Render for CommitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_nsh(window, cx)
    }
}

pub(super) fn commit_error_label(code: CommitErrorCode) -> &'static str {
    match code {
        CommitErrorCode::GitUnavailable => {
            vega_i18n::t("ui.git_2_40_was_not_found_install_homebrew")
        }
        CommitErrorCode::GitUnsupported => vega_i18n::t("ui.git_2_40_is_required_upgrade_git_and"),
        CommitErrorCode::GitExecutableChanged => {
            vega_i18n::t("ui.git_changed_while_vega_was_running_restart_vega")
        }
        _ => code.as_str(),
    }
}
