use std::collections::HashSet;
use std::ops::Range;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, Window, actions,
};
use vega_conversation::types::{
    CommitChecklist, CommitDraft, CommitErrorCode, CommitSelection, IndexSnapshotId,
    PreparedCommit, PreparedCommitId, WorkspaceFileId,
};
use vega_theme::theme;

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
mod draft;
mod host;
mod measurement;
mod model;
mod nsh_view;
mod panel;

pub(crate) use model::{
    COMMIT_ROW_HEIGHT, commit_row_is_focusable, commit_row_key, commit_row_status,
};
pub use model::{
    CommitChecklistRequested, CommitDraftRequested, CommitOperationId, CommitPanelClosed,
    CommitPanelFocus, CommitPanelModel, CommitPanelStage, CommitPrepareRequested, CommitRequested,
};
pub use panel::CommitPanel;
