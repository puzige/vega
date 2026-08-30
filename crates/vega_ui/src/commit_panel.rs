//! IO-free canonical two-stage commit panel.

use std::collections::HashSet;
use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton, Render,
    UniformListScrollHandle, Window, actions, div, px, uniform_list,
};
use vega_conversation::types::{
    CommitChecklist, CommitDraft, CommitErrorCode, CommitSelection, IndexSnapshotId,
    PreparedCommit, PreparedCommitId, WorkspaceFileId,
};
use vega_theme::{Typography, theme};

use crate::text_input::TextInput;

actions!(
    vega_commit_panel,
    [
        ConfirmCommitStage,
        CloseCommitPanel,
        ToggleCommitSelection,
        RequestCommitDraft,
        ActivateCommitEnter,
        ActivateCommitSpace,
        NextCommitFocus,
        PreviousCommitFocus
    ]
);

pub const COMMIT_ROW_HEIGHT: f32 = 24.0;
pub const COMMIT_PATH_LIMIT: usize = 10_000;

fn checklist_count_is_bounded(staged: usize, optional: usize) -> bool {
    staged
        .checked_add(optional)
        .is_some_and(|count| count <= COMMIT_PATH_LIMIT)
}

fn commit_row_key(generation: u64, index: usize, staged: usize) -> String {
    if index < staged {
        format!("commit-row-{generation}-staged-{index}")
    } else {
        format!("commit-row-{generation}-optional-{}", index - staged)
    }
}

fn commit_row_status(forced: bool, selected: bool) -> &'static str {
    match (forced, selected) {
        (true, _) => "Included · staged",
        (false, true) => "Selected · worktree",
        (false, false) => "Optional · worktree",
    }
}

fn commit_row_is_focusable(forced: bool) -> bool {
    !forced
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CommitOperationId(u64);

impl std::fmt::Debug for CommitOperationId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CommitOperationId([opaque])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CommitChecklistRequested {
    pub thread_id: String,
    pub project_id: String,
}

impl std::fmt::Debug for CommitChecklistRequested {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitChecklistRequested")
            .field("thread_id_bytes", &self.thread_id.len())
            .field("project_id_bytes", &self.project_id.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CommitPrepareRequested {
    pub thread_id: String,
    pub project_id: String,
    pub snapshot_id: IndexSnapshotId,
    pub selected: Vec<WorkspaceFileId>,
    pub operation_id: CommitOperationId,
}

impl std::fmt::Debug for CommitPrepareRequested {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitPrepareRequested")
            .field("thread_id_bytes", &self.thread_id.len())
            .field("project_id_bytes", &self.project_id.len())
            .field("snapshot_id", &self.snapshot_id)
            .field("selected_count", &self.selected.len())
            .field("operation_id", &self.operation_id)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CommitDraftRequested {
    pub thread_id: String,
    pub project_id: String,
    pub prepared_id: PreparedCommitId,
    pub operation_id: CommitOperationId,
}

impl std::fmt::Debug for CommitDraftRequested {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitDraftRequested")
            .field("thread_id_bytes", &self.thread_id.len())
            .field("project_id_bytes", &self.project_id.len())
            .field("prepared_id", &self.prepared_id)
            .field("operation_id", &self.operation_id)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CommitRequested {
    pub thread_id: String,
    pub project_id: String,
    pub prepared_id: PreparedCommitId,
    pub operation_id: CommitOperationId,
    pub message: String,
}

impl std::fmt::Debug for CommitRequested {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitRequested")
            .field("thread_id_bytes", &self.thread_id.len())
            .field("project_id_bytes", &self.project_id.len())
            .field("prepared_id", &self.prepared_id)
            .field("operation_id", &self.operation_id)
            .field("message_bytes", &self.message.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CommitPanelClosed {
    pub thread_id: String,
    pub project_id: String,
}

impl std::fmt::Debug for CommitPanelClosed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitPanelClosed")
            .field("thread_id_bytes", &self.thread_id.len())
            .field("project_id_bytes", &self.project_id.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitPanelStage {
    Closed,
    Loading,
    Checklist,
    Preparing,
    CommitReady,
    Drafting,
    Committing,
    Failed(CommitErrorCode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitPanelFocus {
    Cancel,
    Optional(usize),
    Draft,
    Generate,
    Confirm,
}

pub struct CommitPanelModel {
    stage: CommitPanelStage,
    checklist: Option<CommitChecklist>,
    prepared: Option<PreparedCommit>,
    selected: HashSet<WorkspaceFileId>,
    next_operation: u64,
    pending: Option<CommitOperationId>,
    focus: CommitPanelFocus,
}

impl Default for CommitPanelModel {
    fn default() -> Self {
        Self {
            stage: CommitPanelStage::Closed,
            checklist: None,
            prepared: None,
            selected: HashSet::new(),
            next_operation: 0,
            pending: None,
            focus: CommitPanelFocus::Cancel,
        }
    }
}

impl CommitPanelModel {
    pub fn stage(&self) -> CommitPanelStage {
        self.stage
    }

    pub fn is_open(&self) -> bool {
        self.stage != CommitPanelStage::Closed
    }

    pub fn focus(&self) -> CommitPanelFocus {
        self.focus
    }

    pub fn open(&mut self) -> bool {
        if self.is_open() || self.pending.is_some() {
            return false;
        }
        self.stage = CommitPanelStage::Loading;
        self.checklist = None;
        self.prepared = None;
        self.selected.clear();
        self.focus = CommitPanelFocus::Cancel;
        true
    }

    pub fn apply_checklist(&mut self, checklist: CommitChecklist) -> bool {
        if self.stage != CommitPanelStage::Loading
            || !checklist_count_is_bounded(checklist.staged.len(), checklist.optional.len())
        {
            return false;
        }
        let Some(row_count) = checklist.staged.len().checked_add(checklist.optional.len()) else {
            return false;
        };
        let mut identities = std::collections::HashMap::with_capacity(row_count);
        let structurally_valid = checklist
            .staged
            .iter()
            .all(|row| row.forced && !row.label.is_empty())
            && checklist
                .optional
                .iter()
                .all(|row| !row.forced && !row.label.is_empty())
            && checklist
                .staged
                .iter()
                .chain(&checklist.optional)
                .all(|row| {
                    if row.previous_label.as_deref() == Some(row.label.as_str()) {
                        return false;
                    }
                    let labels =
                        std::iter::once(row.label.as_str()).chain(row.previous_label.as_deref());
                    labels.into_iter().all(|label| {
                        !label.is_empty()
                            && identities
                                .get(label)
                                .is_none_or(|existing| *existing == row.file_id)
                            && {
                                identities.insert(label, row.file_id);
                                true
                            }
                    })
                });
        if !structurally_valid {
            return false;
        }
        self.checklist = Some(checklist);
        self.stage = CommitPanelStage::Checklist;
        self.focus = CommitPanelFocus::Cancel;
        true
    }

    pub fn toggle(&mut self, id: WorkspaceFileId) -> bool {
        let allowed = self.stage == CommitPanelStage::Checklist
            && self
                .checklist
                .as_ref()
                .is_some_and(|checklist| checklist.optional.iter().any(|row| row.file_id == id));
        if !allowed {
            return false;
        }
        if !self.selected.remove(&id) {
            self.selected.insert(id);
        }
        true
    }

    pub fn begin_prepare(
        &mut self,
    ) -> Option<(IndexSnapshotId, Vec<WorkspaceFileId>, CommitOperationId)> {
        if self.stage != CommitPanelStage::Checklist || self.pending.is_some() {
            return None;
        }
        let checklist = self.checklist.as_ref()?;
        if checklist.staged.is_empty() && self.selected.is_empty() {
            return None;
        }
        let selected: Vec<_> = checklist
            .optional
            .iter()
            .filter(|row| self.selected.contains(&row.file_id))
            .map(|row| row.file_id)
            .collect();
        let snapshot_id = checklist.id;
        let operation = self.next_operation()?;
        self.stage = CommitPanelStage::Preparing;
        self.pending = Some(operation);
        Some((snapshot_id, selected, operation))
    }

    pub fn finish_prepare(
        &mut self,
        operation: CommitOperationId,
        prepared: Result<PreparedCommit, CommitErrorCode>,
    ) -> bool {
        if self.pending != Some(operation) || self.stage != CommitPanelStage::Preparing {
            return false;
        }
        self.pending = None;
        match prepared {
            Ok(prepared) => {
                self.prepared = Some(prepared);
                self.stage = CommitPanelStage::CommitReady;
                self.focus = CommitPanelFocus::Cancel;
            }
            Err(code) => self.stage = CommitPanelStage::Failed(code),
        }
        if matches!(self.stage, CommitPanelStage::Failed(_)) {
            self.focus = CommitPanelFocus::Cancel;
        }
        true
    }

    pub fn begin_draft(&mut self) -> Option<(PreparedCommitId, CommitOperationId)> {
        if self.stage != CommitPanelStage::CommitReady || self.pending.is_some() {
            return None;
        }
        let prepared = self.prepared.as_ref()?.id;
        let operation = self.next_operation()?;
        self.pending = Some(operation);
        self.stage = CommitPanelStage::Drafting;
        Some((prepared, operation))
    }

    pub fn finish_draft(
        &mut self,
        operation: CommitOperationId,
        result: Result<(), CommitErrorCode>,
    ) -> bool {
        if self.pending != Some(operation) || self.stage != CommitPanelStage::Drafting {
            return false;
        }
        self.pending = None;
        self.stage = match result {
            Ok(()) => CommitPanelStage::CommitReady,
            Err(code) => CommitPanelStage::Failed(code),
        };
        if matches!(self.stage, CommitPanelStage::Failed(_)) {
            self.focus = CommitPanelFocus::Cancel;
        }
        true
    }

    pub fn begin_commit(
        &mut self,
        message: String,
    ) -> Option<(PreparedCommitId, CommitOperationId, String)> {
        if self.stage != CommitPanelStage::CommitReady || self.pending.is_some() {
            return None;
        }
        if message.is_empty() || message.len() > 32 * 1024 || message.as_bytes().contains(&0) {
            self.stage = CommitPanelStage::Failed(CommitErrorCode::InvalidMessage);
            self.focus = CommitPanelFocus::Cancel;
            return None;
        }
        let prepared = self.prepared.as_ref()?.id;
        let operation = self.next_operation()?;
        self.pending = Some(operation);
        self.stage = CommitPanelStage::Committing;
        Some((prepared, operation, message))
    }

    pub fn finish_commit(
        &mut self,
        operation: CommitOperationId,
        error: Option<CommitErrorCode>,
    ) -> bool {
        if self.pending != Some(operation) || self.stage != CommitPanelStage::Committing {
            return false;
        }
        self.pending = None;
        self.stage = error.map_or(CommitPanelStage::Closed, CommitPanelStage::Failed);
        if error.is_some() {
            self.focus = CommitPanelFocus::Cancel;
        }
        true
    }

    pub fn close_visible(&mut self) -> bool {
        if !self.is_open() {
            return false;
        }
        self.stage = CommitPanelStage::Closed;
        self.checklist = None;
        self.prepared = None;
        self.selected.clear();
        self.focus = CommitPanelFocus::Cancel;
        true
    }

    pub fn owns_pending(&self, operation: CommitOperationId) -> bool {
        self.pending == Some(operation)
    }

    pub fn clear_pending(&mut self, operation: CommitOperationId) -> bool {
        if self.pending != Some(operation) {
            return false;
        }
        self.pending = None;
        true
    }

    pub fn fail_pending(&mut self, operation: CommitOperationId, code: CommitErrorCode) -> bool {
        if self.pending != Some(operation) {
            return false;
        }
        self.pending = None;
        self.stage = CommitPanelStage::Failed(code);
        self.focus = CommitPanelFocus::Cancel;
        true
    }

    pub fn rows(&self, range: Range<usize>) -> Vec<(usize, CommitSelection, bool)> {
        self.checklist.as_ref().map_or_else(Vec::new, |checklist| {
            let staged_len = checklist.staged.len();
            range
                .filter_map(|index| {
                    if index < staged_len {
                        checklist
                            .staged
                            .get(index)
                            .cloned()
                            .map(|row| (index, row, true))
                    } else {
                        checklist
                            .optional
                            .get(index - staged_len)
                            .cloned()
                            .map(|row| {
                                let selected = self.selected.contains(&row.file_id);
                                (index, row, selected)
                            })
                    }
                })
                .collect()
        })
    }

    pub fn move_focus(&mut self, backwards: bool) -> bool {
        let previous = self.focus;
        let optional = self
            .checklist
            .as_ref()
            .map_or(0, |checklist| checklist.optional.len());
        self.focus = match (self.stage, self.focus, backwards) {
            (_, CommitPanelFocus::Cancel, true) => return false,
            (CommitPanelStage::Checklist, CommitPanelFocus::Cancel, false) if optional > 0 => {
                CommitPanelFocus::Optional(0)
            }
            (CommitPanelStage::Checklist, CommitPanelFocus::Cancel, false) => {
                CommitPanelFocus::Confirm
            }
            (CommitPanelStage::Checklist, CommitPanelFocus::Optional(index), true) => {
                if index == 0 {
                    CommitPanelFocus::Cancel
                } else {
                    CommitPanelFocus::Optional(index - 1)
                }
            }
            (CommitPanelStage::Checklist, CommitPanelFocus::Optional(index), false) => {
                if index + 1 < optional {
                    CommitPanelFocus::Optional(index + 1)
                } else {
                    CommitPanelFocus::Confirm
                }
            }
            (CommitPanelStage::CommitReady, CommitPanelFocus::Cancel, false) => {
                CommitPanelFocus::Draft
            }
            (CommitPanelStage::CommitReady, CommitPanelFocus::Draft, true) => {
                CommitPanelFocus::Cancel
            }
            (CommitPanelStage::CommitReady, CommitPanelFocus::Draft, false) => {
                CommitPanelFocus::Generate
            }
            (CommitPanelStage::CommitReady, CommitPanelFocus::Generate, true) => {
                CommitPanelFocus::Draft
            }
            (CommitPanelStage::CommitReady, CommitPanelFocus::Generate, false) => {
                CommitPanelFocus::Confirm
            }
            (_, CommitPanelFocus::Confirm, true) if self.stage == CommitPanelStage::CommitReady => {
                CommitPanelFocus::Generate
            }
            (_, CommitPanelFocus::Confirm, true) if optional > 0 => {
                CommitPanelFocus::Optional(optional - 1)
            }
            (_, CommitPanelFocus::Confirm, true) => CommitPanelFocus::Cancel,
            (_, CommitPanelFocus::Confirm, false) => return false,
            (_, focus, false) => focus,
            (_, focus, true) => focus,
        };
        self.focus != previous
    }

    pub fn focused_optional(&self) -> Option<WorkspaceFileId> {
        let CommitPanelFocus::Optional(index) = self.focus else {
            return None;
        };
        self.checklist
            .as_ref()?
            .optional
            .get(index)
            .map(|row| row.file_id)
    }

    fn next_operation(&mut self) -> Option<CommitOperationId> {
        let Some(sequence) = self.next_operation.checked_add(1) else {
            self.pending = None;
            self.stage = CommitPanelStage::Failed(CommitErrorCode::OutputTooLarge);
            self.focus = CommitPanelFocus::Cancel;
            return None;
        };
        self.next_operation = sequence;
        Some(CommitOperationId(sequence))
    }
}

pub struct CommitPanel {
    thread_id: String,
    project_id: String,
    model: CommitPanelModel,
    message: Entity<TextInput>,
    focus: FocusHandle,
    cancel_focus: FocusHandle,
    draft_focus: FocusHandle,
    confirm_focus: FocusHandle,
    scroll: UniformListScrollHandle,
    disabled: bool,
    editor_revision: u64,
    editor_revision_overflow: bool,
    draft_revision: Option<(CommitOperationId, u64)>,
    focus_cancel_pending: bool,
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

    fn draft_revision_is_current(&self, operation: CommitOperationId) -> bool {
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use gpui::{TestAppContext, WindowHandle};

    struct Harness {
        panel: Entity<CommitPanel>,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.panel.clone())
        }
    }

    #[test]
    fn commit_panel_open_close_is_first_wins_and_cancel_visible() {
        let mut model = CommitPanelModel::default();
        assert!(model.open());
        assert!(!model.open());
        assert_eq!(model.stage(), CommitPanelStage::Loading);
        assert_eq!(model.focus(), CommitPanelFocus::Cancel);
        assert!(model.close_visible());
        assert!(!model.close_visible());
        assert_eq!(model.stage(), CommitPanelStage::Closed);
    }

    #[test]
    fn commit_panel_fixed_geometry_and_limit_are_exact() {
        assert_eq!(COMMIT_ROW_HEIGHT, 24.0);
        assert_eq!(COMMIT_PATH_LIMIT, 10_000);
        assert!(checklist_count_is_bounded(10_000, 0));
        assert!(checklist_count_is_bounded(4_000, 6_000));
        assert!(!checklist_count_is_bounded(10_000, 1));
        assert!(!checklist_count_is_bounded(usize::MAX, 1));
        assert_ne!(
            commit_row_key(7, 0, 1),
            commit_row_key(7, 1, 1),
            "mixed staged/optional rows require distinct stable GPUI ids"
        );
        assert_eq!(commit_row_key(7, 1, 1), "commit-row-7-optional-0");
        assert_eq!(commit_row_status(true, true), "Included · staged");
        assert_eq!(commit_row_status(false, true), "Selected · worktree");
        assert_eq!(commit_row_status(false, false), "Optional · worktree");
        assert!(!commit_row_is_focusable(true));
        assert!(commit_row_is_focusable(false));
        assert_ne!(
            commit_row_status(true, true),
            commit_row_status(false, true)
        );
    }

    #[test]
    fn commit_panel_message_and_events_are_debug_redacted() {
        let thread_sentinel = "VEGA_COMMIT_UI_THREAD_SECRET";
        let project_sentinel = "VEGA_COMMIT_UI_PROJECT_SECRET";
        let request = CommitChecklistRequested {
            thread_id: thread_sentinel.into(),
            project_id: project_sentinel.into(),
        };
        let closed = CommitPanelClosed {
            thread_id: thread_sentinel.into(),
            project_id: project_sentinel.into(),
        };
        for rendered in [format!("{request:?}"), format!("{closed:?}")] {
            assert!(!rendered.contains(thread_sentinel));
            assert!(!rendered.contains(project_sentinel));
        }
    }

    #[test]
    fn commit_panel_failures_restore_cancel_and_clear_exact_pending() {
        let operation = CommitOperationId(7);
        let mut model = CommitPanelModel {
            stage: CommitPanelStage::Preparing,
            pending: Some(operation),
            focus: CommitPanelFocus::Confirm,
            ..CommitPanelModel::default()
        };
        assert!(!model.fail_pending(CommitOperationId(8), CommitErrorCode::SpawnFailed));
        assert!(model.owns_pending(operation));
        assert!(model.fail_pending(operation, CommitErrorCode::SpawnFailed));
        assert_eq!(
            model.stage(),
            CommitPanelStage::Failed(CommitErrorCode::SpawnFailed)
        );
        assert_eq!(model.focus(), CommitPanelFocus::Cancel);
        assert!(!model.owns_pending(operation));
    }

    #[test]
    fn commit_panel_checked_operation_overflow_fails_closed() {
        let mut model = CommitPanelModel {
            stage: CommitPanelStage::CommitReady,
            next_operation: u64::MAX,
            focus: CommitPanelFocus::Confirm,
            ..CommitPanelModel::default()
        };
        assert_eq!(model.next_operation(), None);
        assert_eq!(
            model.stage(),
            CommitPanelStage::Failed(CommitErrorCode::OutputTooLarge)
        );
        assert_eq!(model.focus(), CommitPanelFocus::Cancel);
    }

    #[test]
    fn commit_panel_invalid_messages_are_typed_before_any_event() {
        for message in [String::new(), "\0".into(), "x".repeat(32 * 1024 + 1)] {
            let mut model = CommitPanelModel {
                stage: CommitPanelStage::CommitReady,
                focus: CommitPanelFocus::Confirm,
                ..CommitPanelModel::default()
            };
            assert_eq!(model.begin_commit(message), None);
            assert_eq!(
                model.stage(),
                CommitPanelStage::Failed(CommitErrorCode::InvalidMessage)
            );
            assert_eq!(model.focus(), CommitPanelFocus::Cancel);
            assert!(model.pending.is_none());
        }
    }

    #[test]
    fn commit_panel_focus_boundaries_escape_without_wrapping() {
        let mut model = CommitPanelModel {
            stage: CommitPanelStage::Checklist,
            ..CommitPanelModel::default()
        };
        assert!(!model.move_focus(true), "Shift+Tab at Cancel must escape");
        assert!(model.move_focus(false));
        assert_eq!(model.focus(), CommitPanelFocus::Confirm);
        assert!(!model.move_focus(false), "Tab at Confirm must escape");
        assert!(model.move_focus(true));
        assert_eq!(model.focus(), CommitPanelFocus::Cancel);

        model.stage = CommitPanelStage::CommitReady;
        assert!(model.move_focus(false));
        assert_eq!(model.focus(), CommitPanelFocus::Draft);
        assert!(model.move_focus(false));
        assert_eq!(model.focus(), CommitPanelFocus::Generate);
        assert!(model.move_focus(false));
        assert_eq!(model.focus(), CommitPanelFocus::Confirm);
        assert!(!model.move_focus(false));
        assert!(model.move_focus(true));
        assert_eq!(model.focus(), CommitPanelFocus::Generate);
    }

    #[gpui::test]
    async fn commit_panel_draft_revision_overflow_never_accepts_equal_revision(
        cx: &mut TestAppContext,
    ) {
        let operation = CommitOperationId(9);
        // The entity-level fence must reject an apparently equal revision
        // after checked revision arithmetic has overflowed.
        let entity = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
        entity.update(cx, |panel, _| {
            panel.editor_revision = u64::MAX;
            panel.editor_revision_overflow = true;
            panel.draft_revision = Some((operation, u64::MAX));
            assert!(!panel.draft_revision_is_current(operation));
        });
    }

    #[gpui::test]
    async fn commit_panel_scoped_keys_focus_cancel_and_escape_first_wins(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            crate::init(cx);
        });
        let panel = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
        let captured = Arc::new(Mutex::new(Vec::new()));
        let events = captured.clone();
        let root = panel.clone();
        let window: WindowHandle<Harness> = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|cx| {
                    cx.subscribe(&root, move |_, _, event: &CommitPanelClosed, _| {
                        events.lock().expect("events").push(event.clone());
                    })
                    .detach();
                    Harness { panel: root }
                })
            })
            .expect("commit panel window")
        });
        window
            .update(cx, |_, window, cx| {
                assert!(panel.update(cx, |panel, cx| panel.request_open(cx)));
                let focus = panel.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
                assert!(focus.is_focused(window));
            })
            .expect("focus commit panel");
        cx.simulate_keystrokes(window.into(), "enter space cmd-enter");
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.stage()),
            CommitPanelStage::Loading
        );
        cx.simulate_keystrokes(window.into(), "escape escape");
        assert_eq!(captured.lock().expect("events").len(), 1);
        assert!(!panel.read_with(cx, |panel, _| panel.is_open()));
    }

    #[gpui::test]
    async fn commit_panel_ready_tab_chain_reaches_editor_generate_and_confirm(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            crate::init(cx);
        });
        let panel = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
        let root = panel.clone();
        let window: WindowHandle<Harness> = cx
            .update(|cx| {
                cx.open_window(Default::default(), move |_, cx| {
                    cx.new(|_| Harness { panel: root })
                })
            })
            .expect("commit ready focus window");
        window
            .update(cx, |_, window, cx| {
                panel.update(cx, |panel, _| {
                    panel.model.stage = CommitPanelStage::CommitReady;
                    panel.model.focus = CommitPanelFocus::Cancel;
                });
                let focus = panel.read(cx).cancel_focus.clone();
                focus.focus(window, cx);
            })
            .expect("focus cancel");
        cx.simulate_keystrokes(window.into(), "tab");
        assert!(
            window
                .update(cx, |_, window, cx| {
                    panel
                        .read(cx)
                        .message
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                })
                .expect("editor focus")
        );
        cx.simulate_keystrokes(window.into(), "tab");
        assert!(
            window
                .update(cx, |_, window, cx| {
                    let focus = panel.read(cx).draft_focus.clone();
                    focus.is_focused(window)
                })
                .expect("generate focus")
        );
        cx.simulate_keystrokes(window.into(), "tab");
        assert!(
            window
                .update(cx, |_, window, cx| {
                    let focus = panel.read(cx).confirm_focus.clone();
                    focus.is_focused(window)
                })
                .expect("confirm focus")
        );
    }
}
