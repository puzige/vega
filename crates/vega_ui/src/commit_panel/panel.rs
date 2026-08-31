use super::*;

pub struct CommitPanel {
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
    pub(crate) model: CommitPanelModel,
    pub(crate) message: Entity<TextInput>,
    pub(crate) focus: FocusHandle,
    pub(crate) cancel_focus: FocusHandle,
    pub(crate) draft_focus: FocusHandle,
    pub(crate) confirm_focus: FocusHandle,
    pub(crate) scroll: UniformListScrollHandle,
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
        self.cancel_focus.clone()
    }
}

impl CommitPanel {
    pub fn new(thread_id: String, project_id: String, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| {
            TextInput::new_multiline(cx, "Commit message… (Enter newline · Cmd+Enter commit)", 4)
        });
        cx.observe(&message, |this, _, cx| {
            match this.editor_revision.checked_add(1) {
                Some(revision) => this.editor_revision = revision,
                None => this.editor_revision_overflow = true,
            }
            cx.notify();
        })
        .detach();
        Self {
            thread_id,
            project_id,
            model: CommitPanelModel::default(),
            message,
            focus: cx.focus_handle().tab_stop(true),
            cancel_focus: cx.focus_handle().tab_stop(true),
            draft_focus: cx.focus_handle().tab_stop(true),
            confirm_focus: cx.focus_handle().tab_stop(true),
            scroll: UniformListScrollHandle::new(),
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

    pub fn is_open(&self) -> bool {
        self.model.is_open()
    }

    pub fn stage(&self) -> CommitPanelStage {
        self.model.stage()
    }

    /// Returns the safe control focus projection used by controller/UI tests.
    pub fn focused_control(&self) -> CommitPanelFocus {
        self.model.focus()
    }

    /// Returns the bounded editable commit message projection.
    pub fn commit_message(&self, cx: &App) -> String {
        self.message.read(cx).text().to_owned()
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    pub fn request_open(&mut self, cx: &mut Context<Self>) -> bool {
        if self.disabled || !self.model.open() {
            return false;
        }
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
        let text = draft.as_ref().ok().map(|draft| draft.text().to_string());
        let unchanged = self.draft_revision_is_current(operation);
        self.draft_revision = None;
        let result = if unchanged {
            draft.map(|_| ())
        } else {
            Err(CommitErrorCode::ChangedDuringRead)
        };
        let accepted = self.model.finish_draft(operation, result);
        if accepted
            && unchanged
            && let Some(text) = text
        {
            self.message
                .update(cx, |message, cx| message.set_text(&text, cx));
        }
        if accepted {
            self.focus_cancel_pending = matches!(self.model.stage(), CommitPanelStage::Failed(_));
            cx.notify();
        }
        accepted
    }

    pub(crate) fn draft_revision_is_current(&self, operation: CommitOperationId) -> bool {
        !self.editor_revision_overflow
            && self.draft_revision == Some((operation, self.editor_revision))
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
        cx.emit(CommitPanelClosed {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
        cx.notify();
        true
    }

    fn confirm(&mut self, _: &ConfirmCommitStage, _: &mut Window, cx: &mut Context<Self>) {
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
                let message = self.message.read(cx).text().to_string();
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

    fn draft(&mut self, _: &RequestCommitDraft, _: &mut Window, cx: &mut Context<Self>) {
        if self.editor_revision_overflow {
            self.model.stage = CommitPanelStage::Failed(CommitErrorCode::OutputTooLarge);
            self.model.focus = CommitPanelFocus::Cancel;
            self.focus_cancel_pending = true;
            cx.notify();
            return;
        }
        if let Some((prepared_id, operation_id)) = self.model.begin_draft() {
            self.draft_revision = Some((operation_id, self.editor_revision));
            cx.emit(CommitDraftRequested {
                thread_id: self.thread_id.clone(),
                project_id: self.project_id.clone(),
                prepared_id,
                operation_id,
            });
            cx.notify();
        }
    }

    fn close(&mut self, _: &CloseCommitPanel, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.request_close(cx);
    }

    fn toggle_action(&mut self, _: &ToggleCommitSelection, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.model.focused_optional() {
            self.toggle_row(id, cx);
        }
    }

    fn activate_enter(
        &mut self,
        _: &ActivateCommitEnter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.model.focus() {
            CommitPanelFocus::Draft => {
                window.dispatch_action(Box::new(crate::text_input::InsertNewline), cx)
            }
            CommitPanelFocus::Generate => self.draft(&RequestCommitDraft, window, cx),
            _ => {}
        }
    }

    fn activate_space(
        &mut self,
        _: &ActivateCommitSpace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.focus() == CommitPanelFocus::Generate {
            self.draft(&RequestCommitDraft, window, cx);
        } else {
            self.toggle_action(&ToggleCommitSelection, window, cx);
        }
    }

    fn focus_current(&self, window: &mut Window, cx: &mut App) {
        match self.model.focus() {
            CommitPanelFocus::Cancel => self.cancel_focus.focus(window, cx),
            CommitPanelFocus::Draft => self.message.read(cx).focus_handle(cx).focus(window, cx),
            CommitPanelFocus::Generate => self.draft_focus.focus(window, cx),
            CommitPanelFocus::Confirm => self.confirm_focus.focus(window, cx),
            CommitPanelFocus::Optional(_) => self.focus.focus(window, cx),
        }
    }

    fn next_focus(&mut self, _: &NextCommitFocus, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.move_focus(false) {
            self.focus_current(window, cx);
        } else {
            window.focus_next(cx);
        }
        cx.notify();
    }

    fn previous_focus(
        &mut self,
        _: &PreviousCommitFocus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.move_focus(true) {
            self.focus_current(window, cx);
        } else {
            window.focus_prev(cx);
        }
        cx.notify();
    }

    fn toggle_row(&mut self, id: WorkspaceFileId, cx: &mut Context<Self>) {
        if self.model.toggle(id) {
            cx.notify();
        }
    }
}

impl Render for CommitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        if !self.model.is_open() {
            return div().hidden().into_any_element();
        }
        if self.focus_cancel_pending {
            self.cancel_focus.focus(window, cx);
            self.focus_cancel_pending = false;
        }
        let rows = self.model.checklist.as_ref().map_or(0, |checklist| {
            checklist.staged.len() + checklist.optional.len()
        });
        let staged_len = self
            .model
            .checklist
            .as_ref()
            .map_or(0, |checklist| checklist.staged.len());
        let workspace_generation = self
            .model
            .checklist
            .as_ref()
            .map_or(0, |checklist| checklist.workspace_generation);
        let view = cx.entity().clone();
        let ready = self.model.stage() == CommitPanelStage::CommitReady;
        let actionable = matches!(
            self.model.stage(),
            CommitPanelStage::Checklist | CommitPanelStage::CommitReady
        );
        let inline_error = match self.model.stage() {
            CommitPanelStage::Failed(code) => Some(code.as_str()),
            _ => None,
        };
        div()
            .key_context("CommitPanel")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::draft))
            .on_action(cx.listener(Self::activate_enter))
            .on_action(cx.listener(Self::activate_space))
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::toggle_action))
            .on_action(cx.listener(Self::next_focus))
            .on_action(cx.listener(Self::previous_focus))
            .absolute()
            .inset_0()
            .bg(colors.bg_base)
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(Typography::HEADING_BLOCK))
                            .text_color(colors.text_primary)
                            .child("Commit changes"),
                    )
                    .child(
                        div()
                            .id("commit-cancel")
                            .track_focus(&self.cancel_focus)
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border_subtle)
                            .when(self.model.focus == CommitPanelFocus::Cancel, |button| {
                                button.bg(colors.bg_hover)
                            })
                            .cursor_pointer()
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let _ = this.request_close(cx);
                                }),
                            )
                            .child("Cancel"),
                    ),
            )
            .when_some(inline_error, |panel, code| {
                panel.child(
                    div()
                        .text_size(px(Typography::BODY))
                        .text_color(colors.danger)
                        .child(format!("Commit unavailable: {code}")),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .rounded_md()
                    .overflow_hidden()
                    .child(
                        uniform_list(
                            "commit-checklist",
                            rows,
                            cx.processor(move |this: &mut CommitPanel, range, _, _cx| {
                                this.model
                                    .rows(range)
                                    .into_iter()
                                    .map(|(index, row, selected)| {
                                        let row_id = row.file_id;
                                        let view = view.clone();
                                        let key =
                                            commit_row_key(workspace_generation, index, staged_len);
                                        div()
                                            .id(key)
                                            .h(px(COMMIT_ROW_HEIGHT))
                                            .px_2()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .text_size(px(Typography::CODE))
                                            .text_color(if row.forced {
                                                colors.text_tertiary
                                            } else {
                                                colors.text_primary
                                            })
                                            .when(
                                                this.model.focus
                                                    == CommitPanelFocus::Optional(
                                                        index.saturating_sub(staged_len),
                                                    )
                                                    && !row.forced,
                                                |element| element.bg(colors.bg_hover),
                                            )
                                            .when(commit_row_is_focusable(row.forced), |element| {
                                                element.cursor_pointer().on_mouse_up(
                                                    MouseButton::Left,
                                                    move |_, _, cx| {
                                                        view.update(cx, |this, cx| {
                                                            this.toggle_row(row_id, cx)
                                                        });
                                                    },
                                                )
                                            })
                                            .child(if row.forced || selected {
                                                "●"
                                            } else {
                                                "○"
                                            })
                                            .child(
                                                div()
                                                    .text_color(colors.text_tertiary)
                                                    .child(commit_row_status(row.forced, selected)),
                                            )
                                            .child(div().min_w_0().truncate().child(row.label))
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .h_full(),
                    ),
            )
            .when(ready, |panel| {
                panel
                    .child(
                        div()
                            .h(px(112.0))
                            .border_1()
                            .border_color(colors.border_subtle)
                            .when(self.model.focus == CommitPanelFocus::Draft, |button| {
                                button.bg(colors.bg_hover)
                            })
                            .rounded_md()
                            .p_2()
                            .child(self.message.clone()),
                    )
                    .child(
                        div()
                            .id("commit-draft")
                            .track_focus(&self.draft_focus)
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border_subtle)
                            .when(self.model.focus == CommitPanelFocus::Generate, |button| {
                                button.bg(colors.bg_hover)
                            })
                            .cursor_pointer()
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.draft(&RequestCommitDraft, window, cx)
                                }),
                            )
                            .child("Generate message"),
                    )
            })
            .child(
                div()
                    .id("commit-confirm")
                    .track_focus(&self.confirm_focus)
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .when(self.model.focus == CommitPanelFocus::Confirm, |button| {
                        button.bg(colors.bg_hover)
                    })
                    .bg(if actionable {
                        colors.bg_active
                    } else {
                        colors.bg_elevated
                    })
                    .text_color(colors.text_primary)
                    .when(actionable, |button| {
                        button.cursor_pointer().on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.confirm(&ConfirmCommitStage, window, cx)
                            }),
                        )
                    })
                    .child(if ready {
                        "Commit (⌘↵)"
                    } else {
                        "Prepare (⌘↵)"
                    }),
            )
            .into_any_element()
    }
}
