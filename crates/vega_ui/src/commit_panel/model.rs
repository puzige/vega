use super::*;

pub const COMMIT_ROW_HEIGHT: f32 = 24.0;
pub const COMMIT_PATH_LIMIT: usize = 10_000;

pub(crate) fn checklist_count_is_bounded(staged: usize, optional: usize) -> bool {
    staged
        .checked_add(optional)
        .is_some_and(|count| count <= COMMIT_PATH_LIMIT)
}

pub(crate) fn commit_row_key(generation: u64, index: usize, staged: usize) -> String {
    if index < staged {
        format!("commit-row-{generation}-staged-{index}")
    } else {
        format!("commit-row-{generation}-optional-{}", index - staged)
    }
}

pub(crate) fn commit_row_status(forced: bool, selected: bool) -> &'static str {
    match (forced, selected) {
        (true, _) => "Included · staged",
        (false, true) => "Selected · worktree",
        (false, false) => "Optional · worktree",
    }
}

pub(crate) fn commit_row_is_focusable(forced: bool) -> bool {
    !forced
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CommitOperationId(pub(crate) u64);

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
    pub(crate) stage: CommitPanelStage,
    pub(crate) checklist: Option<CommitChecklist>,
    pub(crate) prepared: Option<PreparedCommit>,
    pub(crate) selected: HashSet<WorkspaceFileId>,
    pub(crate) next_operation: u64,
    pub(crate) pending: Option<CommitOperationId>,
    pub(crate) focus: CommitPanelFocus,
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

    pub(crate) fn next_operation(&mut self) -> Option<CommitOperationId> {
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
