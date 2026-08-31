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
mod model;
mod panel;

#[cfg(test)]
mod tests;

pub(crate) use model::{
    COMMIT_PATH_LIMIT, COMMIT_ROW_HEIGHT, checklist_count_is_bounded, commit_row_is_focusable,
    commit_row_key, commit_row_status,
};
pub use model::{
    CommitChecklistRequested, CommitDraftRequested, CommitOperationId, CommitPanelClosed,
    CommitPanelFocus, CommitPanelModel, CommitPanelStage, CommitPrepareRequested, CommitRequested,
};
pub use panel::CommitPanel;
