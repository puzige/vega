//! Safe, IO-free workspace diff state and row preparation.

use std::{ops::Range, time::Duration};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    MouseUpEvent, Render, ScrollStrategy, UniformListScrollHandle, Window, actions, div, px,
    uniform_list,
};
use vega_conversation::types::{
    DiffLanguage, DiffLayer, DiffRowKind, DiffTextProjection, GitWorkspaceErrorCode,
    WorkspaceChangeKind, WorkspaceFile, WorkspaceFileId, WorkspaceHead, WorkspaceLineCount,
    WorkspaceSnapshot,
};
use vega_markdown::HighlightKind;
use vega_theme::{ThemeColors, Typography, theme};

use crate::conversation_stream::{MONOFONT, code_token_style};

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
pub const DIFF_MIN_WINDOW_WIDTH: f32 = 960.0;
pub const DIFF_MIN_WINDOW_HEIGHT: f32 = 600.0;
pub const DIFF_CHANGE_BACKGROUND_OPACITY: f32 = 0.08;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffLayout {
    #[default]
    Unified,
    SideBySide,
}

/// Requests the sole expanded accordion body's bounded projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffProjectionRequested {
    pub thread_id: String,
    pub project_id: String,
    pub generation: u64,
    pub file_id: WorkspaceFileId,
}

/// Requests a content-free workspace refresh retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRetryRequested {
    pub thread_id: String,
    pub project_id: String,
}

/// Closes the current safe diff route.
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
        text: String,
    },
    Message {
        text: String,
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

/// IO-free state behind the diff panel.
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
    rows: Vec<PreparedRow>,
    hunk_indexes: Vec<usize>,
    current_hunk: Option<usize>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
}

impl EventEmitter<DiffProjectionRequested> for DiffView {}
impl EventEmitter<DiffRetryRequested> for DiffView {}
impl EventEmitter<DiffClosed> for DiffView {}

impl Focusable for DiffView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

mod render;
mod state;

pub(crate) use render::*;

#[cfg(test)]
mod tests;
