use std::{sync::Arc, time::Duration};

use gpui_kit::{App, Context, EventEmitter, FocusHandle, Focusable, Window, actions};
use vega_conversation::types::{
    DiffLanguage, DiffLayer, DiffRowKind, DiffTextProjection, GitWorkspaceErrorCode,
    WorkspaceChangeKind, WorkspaceFile, WorkspaceFileId, WorkspaceHead, WorkspaceLineCount,
    WorkspaceSnapshot,
};
use vega_markdown::HighlightKind;

actions!(
    vega_diff_view,
    [
        OpenWorkspaceDiff,
        CloseDiff,
        PreviousDiffHunk,
        NextDiffHunk,
        ToggleDiffLayout
    ]
);

pub const DIFF_REFRESH_INTERVAL: Duration = Duration::from_millis(750);
pub const DIFF_ROW_HEIGHT: f32 = 24.0;
pub const DIFF_CHANGE_BACKGROUND_OPACITY: f32 = 0.08;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffLayout {
    #[default]
    Unified,
    SideBySide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffProjectionRequested {
    pub thread_id: String,
    pub project_id: String,
    pub generation: u64,
    pub file_id: WorkspaceFileId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRetryRequested {
    pub thread_id: String,
    pub project_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffClosed {
    pub thread_id: String,
    pub project_id: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PreparedLine {
    kind: DiffRowKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    spans: Vec<PreparedSpan>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PreparedSpan {
    text: String,
    kind: Option<HighlightKind>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SidePair {
    left: Option<PreparedLine>,
    right: Option<PreparedLine>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum PreparedRow {
    File {
        id: WorkspaceFileId,
        label: String,
        summary: String,
        expanded: bool,
    },
    Section {
        label: &'static str,
    },
    Hunk {
        label: String,
    },
    Unified(PreparedLine),
    SideBySide(SidePair),
    ProjectionError {
        id: WorkspaceFileId,
        code: GitWorkspaceErrorCode,
    },
    Message {
        key: &'static str,
        danger: bool,
    },
}

pub(crate) struct PreparedProjection {
    file_id: WorkspaceFileId,
    sections: Vec<PreparedSection>,
}

pub(crate) struct PreparedSection {
    label: &'static str,
    hunks: Vec<PreparedHunk>,
}

pub(crate) struct PreparedHunk {
    label: String,
    lines: Vec<PreparedLine>,
    missing_trailing_newline: bool,
}

pub struct DiffView {
    thread_id: String,
    project_id: String,
    layout: DiffLayout,
    snapshot: Option<WorkspaceSnapshot>,
    expanded_file: Option<WorkspaceFileId>,
    prepared_projection: Option<PreparedProjection>,
    pending_projection: Option<WorkspaceFileId>,
    refresh_error: Option<GitWorkspaceErrorCode>,
    projection_error: Option<(WorkspaceFileId, GitWorkspaceErrorCode)>,
    refreshing: bool,
    show_refresh_progress: bool,
    rows: Arc<Vec<PreparedRow>>,
    hunk_indexes: Vec<usize>,
    current_hunk: Option<usize>,
    rendered_language: &'static str,
    focus: FocusHandle,
    presentation: Option<nsh_view::Presentation>,
    presentation_version: crate::nsh::ProjectionId,
    row_revision: u64,
    scroll_revision: u64,
    scroll_target: Option<usize>,
}

impl EventEmitter<DiffProjectionRequested> for DiffView {}
impl EventEmitter<DiffRetryRequested> for DiffView {}
impl EventEmitter<DiffClosed> for DiffView {}

impl Focusable for DiffView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

mod nsh_view;
mod render;
mod state;

pub(crate) use render::*;
