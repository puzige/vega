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
struct PreparedLine {
    kind: DiffRowKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    spans: Vec<PreparedSpan>,
}

#[derive(Clone, PartialEq, Eq)]
struct PreparedSpan {
    text: String,
    kind: Option<HighlightKind>,
}

#[derive(Clone, PartialEq, Eq)]
struct SidePair {
    left: Option<PreparedLine>,
    right: Option<PreparedLine>,
}

#[derive(Clone, PartialEq, Eq)]
enum PreparedRow {
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

struct PreparedProjection {
    file_id: WorkspaceFileId,
    sections: Vec<PreparedSection>,
}

struct PreparedSection {
    label: &'static str,
    hunks: Vec<PreparedHunk>,
}

struct PreparedHunk {
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

impl DiffView {
    pub fn new(thread_id: String, project_id: String, cx: &mut Context<Self>) -> Self {
        Self {
            thread_id,
            project_id,
            layout: DiffLayout::Unified,
            snapshot: None,
            expanded_file: None,
            prepared_projection: None,
            pending_projection: None,
            refresh_error: None,
            projection_error: None,
            refreshing: false,
            rows: Vec::new(),
            hunk_indexes: Vec::new(),
            current_hunk: None,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
        }
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn generation(&self) -> Option<u64> {
        self.snapshot.as_ref().map(|snapshot| snapshot.generation)
    }

    pub fn layout(&self) -> DiffLayout {
        self.layout
    }

    pub fn expanded_file(&self) -> Option<WorkspaceFileId> {
        self.expanded_file
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn is_refreshing(&self) -> bool {
        self.refreshing
    }

    pub fn refresh_error(&self) -> Option<GitWorkspaceErrorCode> {
        self.refresh_error
    }

    pub fn projection_error(&self) -> Option<GitWorkspaceErrorCode> {
        self.projection_error.map(|(_, code)| code)
    }

    /// Reconciles a fresh safe snapshot and returns at most one lazy request.
    pub fn apply_snapshot(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        let old_generation = self.generation();
        let old_expanded = self.expanded_file;
        let ids: Vec<_> = snapshot.files.iter().map(|file| file.id).collect();
        let expanded = reconcile_expanded(old_expanded, &ids);
        let preserve =
            should_preserve_projection(old_generation, snapshot.generation, old_expanded, expanded);

        if !preserve {
            self.prepared_projection = None;
            self.pending_projection = None;
            self.projection_error = None;
            self.current_hunk = None;
        }
        self.snapshot = Some(snapshot);
        self.expanded_file = expanded;
        self.refresh_error = None;
        self.rebuild_rows();
        if let Some(request) = self.request_missing_projection() {
            cx.emit(request);
        }
        cx.notify();
    }

    /// Applies only the exact current expanded file projection.
    pub fn apply_projection(
        &mut self,
        projection: DiffTextProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        let file_id = projection.file_id();
        let is_current = self.snapshot.as_ref().is_some_and(|snapshot| {
            exact_current_file(
                self.expanded_file,
                snapshot.files.iter().map(|file| file.id),
                file_id,
            )
        });
        if !is_current {
            return false;
        }
        self.prepared_projection = Some(prepare_projection(&projection));
        self.pending_projection = None;
        self.projection_error = None;
        self.current_hunk = None;
        self.rebuild_rows();
        cx.notify();
        true
    }

    /// Invalidates every capability after a latest refresh failure.
    pub fn apply_refresh_error(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        self.snapshot = None;
        self.expanded_file = None;
        self.prepared_projection = None;
        self.pending_projection = None;
        self.projection_error = None;
        self.refresh_error = Some(code);
        self.rows.clear();
        self.hunk_indexes.clear();
        self.current_hunk = None;
        cx.notify();
    }

    /// Applies an inline error only to the exact current accordion body.
    pub fn apply_projection_error(
        &mut self,
        file_id: WorkspaceFileId,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.expanded_file != Some(file_id) {
            return false;
        }
        self.prepared_projection = None;
        self.pending_projection = None;
        self.projection_error = Some((file_id, code));
        self.rebuild_rows();
        cx.notify();
        true
    }

    pub fn set_refreshing(&mut self, refreshing: bool, cx: &mut Context<Self>) {
        self.refreshing = refreshing;
        cx.notify();
    }

    /// Enforces the single-open accordion invariant.
    fn toggle_file(&mut self, file_id: WorkspaceFileId, cx: &mut Context<Self>) {
        let current = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.files.iter().any(|file| file.id == file_id));
        if !current {
            return;
        }
        self.expanded_file = (self.expanded_file != Some(file_id)).then_some(file_id);
        self.prepared_projection = None;
        self.pending_projection = None;
        self.projection_error = None;
        self.current_hunk = None;
        self.rebuild_rows();
        if let Some(request) = self.request_missing_projection() {
            cx.emit(request);
        }
        cx.notify();
    }

    fn toggle_layout(&mut self, cx: &mut Context<Self>) {
        self.layout = match self.layout {
            DiffLayout::Unified => DiffLayout::SideBySide,
            DiffLayout::SideBySide => DiffLayout::Unified,
        };
        self.current_hunk = None;
        self.rebuild_rows();
        cx.notify();
    }

    fn next_hunk(&mut self, cx: &mut Context<Self>) -> Option<usize> {
        let row = navigate_hunk(&self.hunk_indexes, &mut self.current_hunk, true);
        if let Some(row) = row {
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
            cx.notify();
        }
        row
    }

    fn previous_hunk(&mut self, cx: &mut Context<Self>) -> Option<usize> {
        let row = navigate_hunk(&self.hunk_indexes, &mut self.current_hunk, false);
        if let Some(row) = row {
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
            cx.notify();
        }
        row
    }

    fn request_missing_projection(&mut self) -> Option<DiffProjectionRequested> {
        let snapshot = self.snapshot.as_ref()?;
        let file_id = self.expanded_file?;
        if self.prepared_projection.is_some()
            || self.projection_error.is_some()
            || self.pending_projection == Some(file_id)
        {
            return None;
        }
        self.pending_projection = Some(file_id);
        Some(DiffProjectionRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            generation: snapshot.generation,
            file_id,
        })
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        self.hunk_indexes.clear();
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        for file in &snapshot.files {
            let expanded = self.expanded_file == Some(file.id);
            self.rows.push(PreparedRow::File {
                id: file.id,
                label: file.label.clone(),
                summary: file_summary(file),
                expanded,
            });
            if !expanded {
                continue;
            }
            if let Some((error_file, code)) = self.projection_error
                && error_file == file.id
            {
                self.rows.push(PreparedRow::ProjectionError {
                    id: file.id,
                    text: error_label(code).to_owned(),
                });
                continue;
            }
            let Some(projection) = self
                .prepared_projection
                .as_ref()
                .filter(|item| item.file_id == file.id)
            else {
                self.rows.push(PreparedRow::Message {
                    text: "Loading diff…".to_owned(),
                    danger: false,
                });
                continue;
            };
            let base = self.rows.len();
            let prepared = layout_projection_rows(projection, self.layout);
            self.hunk_indexes
                .extend(prepared.hunk_indexes.into_iter().map(|index| base + index));
            self.rows.extend(prepared.rows);
        }
    }

    fn retry_clicked(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.refreshing {
            return;
        }
        cx.emit(DiffRetryRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
    }

    fn file_clicked(&mut self, file_id: WorkspaceFileId, cx: &mut Context<Self>) {
        self.toggle_file(file_id, cx);
    }

    fn retry_projection(&mut self, file_id: WorkspaceFileId, cx: &mut Context<Self>) {
        if self.projection_error.map(|(id, _)| id) != Some(file_id)
            || self.expanded_file != Some(file_id)
        {
            return;
        }
        self.projection_error = None;
        self.rebuild_rows();
        if let Some(request) = self.request_missing_projection() {
            cx.emit(request);
        }
        cx.notify();
    }

    fn back_clicked(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DiffClosed {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
    }

    fn close_action(&mut self, _: &CloseDiff, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DiffClosed {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
    }

    fn previous_action(&mut self, _: &PreviousDiffHunk, _: &mut Window, cx: &mut Context<Self>) {
        self.previous_hunk(cx);
    }

    fn next_action(&mut self, _: &NextDiffHunk, _: &mut Window, cx: &mut Context<Self>) {
        self.next_hunk(cx);
    }

    fn toggle_layout_action(
        &mut self,
        _: &ToggleDiffLayout,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_layout(cx);
    }
}

impl Render for DiffView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let row_count = self.rows.len();
        let view = cx.entity();
        let header = self.snapshot.as_ref().map(|snapshot| {
            (
                head_label(&snapshot.head),
                format!(
                    "{} files  +{}  -{}",
                    snapshot.stats.file_count,
                    count_label(snapshot.stats.additions),
                    count_label(snapshot.stats.deletions)
                ),
            )
        });
        let layout_label = match self.layout {
            DiffLayout::Unified => "Unified",
            DiffLayout::SideBySide => "Side by side",
        };

        let body: AnyElement = if let Some(code) = self.refresh_error {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(colors.danger)
                .child(error_label(code))
                .child(
                    diff_button("Retry", colors.warning)
                        .when(!self.refreshing, |button| button.cursor_pointer())
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::retry_clicked)),
                )
                .into_any_element()
        } else if row_count == 0 {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(colors.text_tertiary)
                .child(if self.refreshing {
                    "Refreshing workspace diff…"
                } else {
                    "No workspace changes"
                })
                .into_any_element()
        } else {
            div()
                .id("workspace-diff-scroll")
                .size_full()
                .overflow_hidden()
                .child(
                    uniform_list(
                        "workspace-diff-rows",
                        row_count,
                        cx.processor(move |this: &mut DiffView, range: Range<usize>, _, _cx| {
                            range
                                .filter_map(|index| this.rows.get(index).cloned())
                                .map(|row| render_prepared_row(row, &colors, view.clone()))
                                .collect()
                        }),
                    )
                    .track_scroll(&self.scroll)
                    .size_full(),
                )
                .into_any_element()
        };

        div()
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .key_context("DiffView")
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .on_action(cx.listener(Self::close_action))
            .on_action(cx.listener(Self::previous_action))
            .on_action(cx.listener(Self::next_action))
            .on_action(cx.listener(Self::toggle_layout_action))
            .child(
                div()
                    .h(px(DIFF_ROW_HEIGHT * 2.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(colors.border_subtle)
                    .child(
                        diff_button("← Back", colors.text_secondary)
                            .cursor_pointer()
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::back_clicked)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .when_some(header, |column, (head, stats)| {
                                column
                                    .child(
                                        div()
                                            .overflow_hidden()
                                            .font_weight(Typography::HEADING_CARD_WEIGHT)
                                            .child(head),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(Typography::SIDEBAR))
                                            .text_color(colors.text_secondary)
                                            .child(stats),
                                    )
                            }),
                    )
                    .when(self.refreshing, |header| {
                        header.child(
                            div()
                                .text_size(px(Typography::SIDEBAR))
                                .text_color(colors.text_tertiary)
                                .child("Refreshing…"),
                        )
                    })
                    .child(
                        diff_button(layout_label, colors.accent)
                            .cursor_pointer()
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.toggle_layout(cx);
                                }),
                            ),
                    )
                    .child(
                        diff_button("↑ [", colors.text_secondary)
                            .cursor_pointer()
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.previous_hunk(cx);
                                }),
                            ),
                    )
                    .child(
                        diff_button("↓ ]", colors.text_secondary)
                            .cursor_pointer()
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.next_hunk(cx);
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .min_h_0()
                    .flex_1()
                    .overflow_hidden()
                    .child(body),
            )
    }
}

fn diff_button(label: &'static str, color: gpui::Rgba) -> gpui::Div {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(color)
        .text_color(color)
        .text_size(px(Typography::SIDEBAR))
        .child(label)
}

fn render_prepared_row(
    row: PreparedRow,
    colors: &ThemeColors,
    view: Entity<DiffView>,
) -> AnyElement {
    let base = div()
        .h(px(DIFF_ROW_HEIGHT))
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .items_center();
    match row {
        PreparedRow::File {
            id,
            label,
            summary,
            expanded,
        } => base
            .px_3()
            .gap_2()
            .cursor_pointer()
            .bg(if expanded {
                colors.bg_active
            } else {
                colors.bg_base
            })
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(if expanded { "▾" } else { "▸" })
            .child(div().min_w_0().flex_1().overflow_hidden().child(label))
            .child(
                div()
                    .flex_shrink_0()
                    .font_family(MONOFONT.to_string())
                    .text_size(px(Typography::CODE))
                    .text_color(colors.text_secondary)
                    .child(summary),
            )
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                view.update(cx, |this, cx| this.file_clicked(id, cx));
            })
            .into_any_element(),
        PreparedRow::Section { label } => base
            .px_3()
            .bg(colors.bg_elevated)
            .text_color(colors.text_secondary)
            .font_weight(Typography::HEADING_CARD_WEIGHT)
            .child(label)
            .into_any_element(),
        PreparedRow::Hunk { label } => base
            .px_3()
            .bg(colors.code_bg)
            .font_family(MONOFONT.to_string())
            .text_size(px(Typography::CODE))
            .text_color(colors.text_secondary)
            .child(label)
            .into_any_element(),
        PreparedRow::Unified(line) => render_diff_line(line, colors, LineSide::Unified),
        PreparedRow::SideBySide(pair) => base
            .child(render_diff_half(pair.left, colors, LineSide::Old))
            .child(render_diff_half(pair.right, colors, LineSide::New))
            .into_any_element(),
        PreparedRow::ProjectionError { id, text } => base
            .px_3()
            .gap_2()
            .text_color(colors.danger)
            .child(div().min_w_0().flex_1().overflow_hidden().child(text))
            .child(
                diff_button("Retry", colors.warning)
                    .cursor_pointer()
                    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                        view.update(cx, |this, cx| this.retry_projection(id, cx));
                    }),
            )
            .into_any_element(),
        PreparedRow::Message { text, danger } => base
            .px_3()
            .text_color(if danger {
                colors.danger
            } else {
                colors.text_tertiary
            })
            .child(text)
            .into_any_element(),
    }
}

#[derive(Clone, Copy)]
enum LineSide {
    Unified,
    Old,
    New,
}

fn render_diff_half(line: Option<PreparedLine>, colors: &ThemeColors, side: LineSide) -> gpui::Div {
    match line {
        Some(line) => render_diff_line_div(line, colors, side)
            .w_1_2()
            .border_r_1()
            .border_color(colors.border_subtle),
        None => div()
            .h(px(DIFF_ROW_HEIGHT))
            .w_1_2()
            .flex_shrink_0()
            .border_r_1()
            .border_color(colors.border_subtle),
    }
}

fn render_diff_line(line: PreparedLine, colors: &ThemeColors, side: LineSide) -> AnyElement {
    render_diff_line_div(line, colors, side).into_any_element()
}

fn render_diff_line_div(line: PreparedLine, colors: &ThemeColors, side: LineSide) -> gpui::Div {
    let number = match side {
        LineSide::Unified => format!(
            "{:>4} {:>4}",
            line.old_line
                .map_or(String::new(), |value| value.to_string()),
            line.new_line
                .map_or(String::new(), |value| value.to_string())
        ),
        LineSide::Old => line
            .old_line
            .map_or(String::new(), |value| value.to_string()),
        LineSide::New => line
            .new_line
            .map_or(String::new(), |value| value.to_string()),
    };
    let marker = match line.kind {
        DiffRowKind::Addition => "+",
        DiffRowKind::Deletion => "-",
        DiffRowKind::Context => " ",
    };
    let mut text = div().min_w_0().flex_1().overflow_hidden().flex();
    for span in line.spans {
        let mut element = div().child(span.text);
        if let Some(kind) = span.kind {
            let style = code_token_style(kind, colors);
            element = element.text_color(style.color).font_weight(style.weight);
            if style.italic {
                element = element.italic();
            }
        }
        text = text.child(element);
    }
    div()
        .h(px(DIFF_ROW_HEIGHT))
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .font_family(MONOFONT.to_string())
        .text_size(px(Typography::CODE))
        .when(line.kind == DiffRowKind::Addition, |row| {
            row.bg(colors.success.opacity(DIFF_CHANGE_BACKGROUND_OPACITY))
        })
        .when(line.kind == DiffRowKind::Deletion, |row| {
            row.bg(colors.danger.opacity(DIFF_CHANGE_BACKGROUND_OPACITY))
        })
        .child(
            div()
                .w(px(if matches!(side, LineSide::Unified) {
                    DIFF_ROW_HEIGHT * 4.0
                } else {
                    DIFF_ROW_HEIGHT * 2.0
                }))
                .flex_shrink_0()
                .pr_2()
                .text_color(colors.text_tertiary)
                .child(number),
        )
        .child(
            div()
                .w(px(DIFF_ROW_HEIGHT))
                .flex_shrink_0()
                .text_color(match line.kind {
                    DiffRowKind::Addition => colors.success,
                    DiffRowKind::Deletion => colors.danger,
                    DiffRowKind::Context => colors.text_tertiary,
                })
                .child(marker),
        )
        .child(text)
}

struct PreparedProjectionRows {
    rows: Vec<PreparedRow>,
    hunk_indexes: Vec<usize>,
}

fn prepare_projection(projection: &DiffTextProjection) -> PreparedProjection {
    let sections = projection
        .sections()
        .iter()
        .map(|section| PreparedSection {
            label: layer_label(section.layer()),
            hunks: section
                .hunks()
                .iter()
                .map(|hunk| PreparedHunk {
                    label: hunk_label(
                        hunk.old_start(),
                        hunk.old_count(),
                        hunk.new_start(),
                        hunk.new_count(),
                        hunk.heading_suffix(),
                    ),
                    lines: hunk
                        .rows()
                        .iter()
                        .map(|row| PreparedLine {
                            kind: row.kind(),
                            old_line: row.old_line(),
                            new_line: row.new_line(),
                            spans: prepare_spans(row.text(), projection.language()),
                        })
                        .collect(),
                    missing_trailing_newline: hunk.missing_trailing_newline(),
                })
                .collect(),
        })
        .collect();
    PreparedProjection {
        file_id: projection.file_id(),
        sections,
    }
}

fn layout_projection_rows(
    projection: &PreparedProjection,
    layout: DiffLayout,
) -> PreparedProjectionRows {
    let mut rows = Vec::new();
    let mut hunk_indexes = Vec::new();
    for section in &projection.sections {
        rows.push(PreparedRow::Section {
            label: section.label,
        });
        for hunk in &section.hunks {
            hunk_indexes.push(rows.len());
            rows.push(PreparedRow::Hunk {
                label: hunk.label.clone(),
            });
            match layout {
                DiffLayout::Unified => {
                    rows.extend(hunk.lines.iter().cloned().map(PreparedRow::Unified));
                }
                DiffLayout::SideBySide => rows.extend(
                    pair_side_by_side(&hunk.lines)
                        .into_iter()
                        .map(PreparedRow::SideBySide),
                ),
            }
            if hunk.missing_trailing_newline {
                rows.push(PreparedRow::Message {
                    text: "No newline at end of file".to_owned(),
                    danger: false,
                });
            }
        }
    }
    PreparedProjectionRows { rows, hunk_indexes }
}

fn reconcile_expanded<T: Copy + Eq>(old: Option<T>, current: &[T]) -> Option<T> {
    old.filter(|id| current.contains(id))
}

fn should_preserve_projection<T: Eq>(
    old_generation: Option<u64>,
    new_generation: u64,
    old_expanded: Option<T>,
    new_expanded: Option<T>,
) -> bool {
    old_generation == Some(new_generation) && old_expanded == new_expanded
}

fn exact_current_file<T: Copy + Eq>(
    expanded: Option<T>,
    current: impl IntoIterator<Item = T>,
    candidate: T,
) -> bool {
    expanded == Some(candidate) && current.into_iter().any(|id| id == candidate)
}

fn pair_side_by_side(lines: &[PreparedLine]) -> Vec<SidePair> {
    let mut pairs = Vec::new();
    let mut cursor = 0;
    while cursor < lines.len() {
        if lines[cursor].kind == DiffRowKind::Context {
            pairs.push(SidePair {
                left: Some(lines[cursor].clone()),
                right: Some(lines[cursor].clone()),
            });
            cursor += 1;
            continue;
        }

        let deletion_start = cursor;
        while cursor < lines.len() && lines[cursor].kind == DiffRowKind::Deletion {
            cursor += 1;
        }
        let addition_start = cursor;
        while cursor < lines.len() && lines[cursor].kind == DiffRowKind::Addition {
            cursor += 1;
        }

        if deletion_start == addition_start && addition_start == cursor {
            cursor += 1;
            continue;
        }
        let deletions = &lines[deletion_start..addition_start];
        let additions = &lines[addition_start..cursor];
        let pair_count = deletions.len().max(additions.len());
        pairs.extend((0..pair_count).map(|ordinal| SidePair {
            left: deletions.get(ordinal).cloned(),
            right: additions.get(ordinal).cloned(),
        }));
    }
    pairs
}

fn navigate_hunk(indexes: &[usize], current: &mut Option<usize>, forward: bool) -> Option<usize> {
    if indexes.is_empty() {
        *current = None;
        return None;
    }
    let next = match (*current, forward) {
        (None, true) => 0,
        (None, false) => indexes.len() - 1,
        (Some(index), true) => (index + 1).min(indexes.len() - 1),
        (Some(0), false) => 0,
        (Some(index), false) => index - 1,
    };
    *current = Some(next);
    Some(indexes[next])
}

fn prepare_spans(text: &str, language: DiffLanguage) -> Vec<PreparedSpan> {
    let Some(highlights) = vega_markdown::highlight(text, language_tag(language)) else {
        return vec![PreparedSpan {
            text: text.to_owned(),
            kind: None,
        }];
    };
    let mut prepared = Vec::with_capacity(highlights.len().saturating_mul(2).saturating_add(1));
    let mut cursor = 0;
    for span in highlights {
        if span.start_byte > cursor {
            prepared.push(PreparedSpan {
                text: text[cursor..span.start_byte].to_owned(),
                kind: None,
            });
        }
        prepared.push(PreparedSpan {
            text: text[span.start_byte..span.end_byte].to_owned(),
            kind: Some(span.kind),
        });
        cursor = span.end_byte;
    }
    if cursor < text.len() {
        prepared.push(PreparedSpan {
            text: text[cursor..].to_owned(),
            kind: None,
        });
    }
    if prepared.is_empty() {
        prepared.push(PreparedSpan {
            text: text.to_owned(),
            kind: None,
        });
    }
    prepared
}

fn file_summary(file: &WorkspaceFile) -> String {
    let mut statuses = Vec::with_capacity(2);
    if file.staged != WorkspaceChangeKind::Unchanged {
        statuses.push(format!("staged {}", change_label(file.staged)));
    }
    if file.unstaged != WorkspaceChangeKind::Unchanged {
        statuses.push(format!("worktree {}", change_label(file.unstaged)));
    }
    if statuses.is_empty() {
        statuses.push("unchanged".to_owned());
    }
    let rename = file
        .previous_label
        .as_ref()
        .map(|previous| format!("{previous} → {} · ", file.label))
        .unwrap_or_default();
    format!(
        "{rename}{}  +{}  -{}",
        statuses.join(" · "),
        count_label(file.additions),
        count_label(file.deletions)
    )
}

fn change_label(kind: WorkspaceChangeKind) -> &'static str {
    match kind {
        WorkspaceChangeKind::Unchanged => "unchanged",
        WorkspaceChangeKind::Added => "added",
        WorkspaceChangeKind::Modified => "modified",
        WorkspaceChangeKind::Deleted => "deleted",
        WorkspaceChangeKind::Renamed => "renamed",
        WorkspaceChangeKind::Copied => "copied",
        WorkspaceChangeKind::TypeChanged => "type changed",
        WorkspaceChangeKind::Unmerged => "unmerged",
        WorkspaceChangeKind::Untracked => "untracked",
    }
}

fn count_label(count: WorkspaceLineCount) -> String {
    match count {
        WorkspaceLineCount::Known(value) => value.to_string(),
        WorkspaceLineCount::Binary => "binary".to_owned(),
        WorkspaceLineCount::Unknown => "?".to_owned(),
    }
}

fn error_label(code: GitWorkspaceErrorCode) -> &'static str {
    match code {
        GitWorkspaceErrorCode::InvalidRoot => "Project root is unavailable.",
        GitWorkspaceErrorCode::NotRepository => "This project is not a Git repository.",
        GitWorkspaceErrorCode::TimedOut => "Git diff timed out. Retry when the repository is idle.",
        GitWorkspaceErrorCode::Cancelled => "Git diff refresh was cancelled.",
        GitWorkspaceErrorCode::OutputTooLarge => "Diff exceeds the safe display limit.",
        GitWorkspaceErrorCode::MetadataOnly => "This file can only be shown as metadata.",
        GitWorkspaceErrorCode::ChangedDuringRead => "The file changed while its diff was loading.",
        GitWorkspaceErrorCode::StaleGeneration | GitWorkspaceErrorCode::UnknownFile => {
            "This diff is stale. Refresh and try again."
        }
        GitWorkspaceErrorCode::SpawnFailed
        | GitWorkspaceErrorCode::GitFailed
        | GitWorkspaceErrorCode::MalformedOutput
        | GitWorkspaceErrorCode::ProcessControlFailed
        | GitWorkspaceErrorCode::ArtifactConflict
        | GitWorkspaceErrorCode::ArtifactLimit => "Git diff could not be loaded safely.",
    }
}

fn head_label(head: &WorkspaceHead) -> String {
    match head {
        WorkspaceHead::Branch { label } => label.clone(),
        WorkspaceHead::Detached => "Detached HEAD".to_owned(),
        WorkspaceHead::Unborn { label: Some(label) } => format!("{label} (unborn)"),
        WorkspaceHead::Unborn { label: None } => "Unborn HEAD".to_owned(),
    }
}

fn layer_label(layer: DiffLayer) -> &'static str {
    match layer {
        DiffLayer::Staged => "Staged",
        DiffLayer::Unstaged => "Unstaged",
        DiffLayer::Untracked => "Untracked",
    }
}

fn hunk_label(
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
    suffix: Option<&str>,
) -> String {
    let suffix = suffix.map(|value| format!(" {value}")).unwrap_or_default();
    format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@{suffix}")
}

/// Exact frozen syntax tag passed to `vega_markdown` by the renderer.
pub const fn language_tag(language: DiffLanguage) -> &'static str {
    match language {
        DiffLanguage::Rust => "rs",
        DiffLanguage::TypeScript => "ts",
        DiffLanguage::Tsx => "tsx",
        DiffLanguage::JavaScript => "js",
        DiffLanguage::Python => "py",
        DiffLanguage::Plain => "",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use gpui::{TestAppContext, WindowHandle};

    struct Harness {
        view: Entity<DiffView>,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.view.clone())
        }
    }

    #[test]
    fn default_layout_is_unified() {
        assert_eq!(DiffLayout::default(), DiffLayout::Unified);
    }

    fn line(kind: DiffRowKind, text: &str) -> PreparedLine {
        PreparedLine {
            kind,
            old_line: None,
            new_line: None,
            spans: vec![PreparedSpan {
                text: text.to_owned(),
                kind: None,
            }],
        }
    }

    fn prepared_text(line: &PreparedLine) -> String {
        line.spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn exact_language_tags_are_frozen() {
        assert_eq!(language_tag(DiffLanguage::Rust), "rs");
        assert_eq!(language_tag(DiffLanguage::TypeScript), "ts");
        assert_eq!(language_tag(DiffLanguage::Tsx), "tsx");
        assert_eq!(language_tag(DiffLanguage::JavaScript), "js");
        assert_eq!(language_tag(DiffLanguage::Python), "py");
        assert_eq!(language_tag(DiffLanguage::Plain), "");
    }

    #[test]
    fn context_is_mirrored() {
        let pairs = pair_side_by_side(&[line(DiffRowKind::Context, "same")]);
        assert_eq!(pairs.len(), 1);
        assert_eq!(
            pairs[0].left.as_ref().map(prepared_text),
            Some("same".to_owned())
        );
        assert_eq!(
            pairs[0].right.as_ref().map(prepared_text),
            Some("same".to_owned())
        );
    }

    #[test]
    fn consecutive_delete_add_runs_pair_by_ordinal() {
        let pairs = pair_side_by_side(&[
            line(DiffRowKind::Deletion, "old-1"),
            line(DiffRowKind::Deletion, "old-2"),
            line(DiffRowKind::Addition, "new-1"),
            line(DiffRowKind::Addition, "new-2"),
        ]);
        assert_eq!(pairs.len(), 2);
        assert_eq!(
            pairs[1].left.as_ref().map(prepared_text),
            Some("old-2".to_owned())
        );
        assert_eq!(
            pairs[1].right.as_ref().map(prepared_text),
            Some("new-2".to_owned())
        );
    }

    #[test]
    fn shorter_addition_side_is_blank() {
        let pairs = pair_side_by_side(&[
            line(DiffRowKind::Deletion, "old-1"),
            line(DiffRowKind::Deletion, "old-2"),
            line(DiffRowKind::Addition, "new-1"),
        ]);
        assert_eq!(pairs.len(), 2);
        assert!(pairs[1].right.is_none());
    }

    #[test]
    fn shorter_deletion_side_is_blank() {
        let pairs = pair_side_by_side(&[
            line(DiffRowKind::Deletion, "old-1"),
            line(DiffRowKind::Addition, "new-1"),
            line(DiffRowKind::Addition, "new-2"),
        ]);
        assert_eq!(pairs.len(), 2);
        assert!(pairs[1].left.is_none());
    }

    #[test]
    fn context_breaks_pairing_runs() {
        let pairs = pair_side_by_side(&[
            line(DiffRowKind::Deletion, "old"),
            line(DiffRowKind::Context, "same"),
            line(DiffRowKind::Addition, "new"),
        ]);
        assert_eq!(pairs.len(), 3);
        assert!(pairs[0].right.is_none());
        assert!(pairs[2].left.is_none());
    }

    #[test]
    fn expanded_identity_is_preserved_or_closed() {
        assert_eq!(reconcile_expanded(Some(2_u64), &[1, 2, 3]), Some(2));
        assert_eq!(reconcile_expanded(Some(4_u64), &[1, 2, 3]), None);
        assert_eq!(reconcile_expanded(Some(4_u64), &[]), None);
        assert_eq!(reconcile_expanded(None::<u64>, &[1, 2, 3]), None);
    }

    #[test]
    fn projection_preservation_requires_same_generation_and_expansion() {
        assert!(should_preserve_projection(Some(7), 7, Some(2_u64), Some(2)));
        assert!(!should_preserve_projection(
            Some(7),
            8,
            Some(2_u64),
            Some(2)
        ));
        assert!(!should_preserve_projection(
            Some(7),
            7,
            Some(2_u64),
            Some(3)
        ));
    }

    #[test]
    fn stale_candidate_requires_exact_expanded_current_id() {
        assert!(exact_current_file(Some(2_u64), [1, 2, 3], 2));
        assert!(!exact_current_file(Some(2_u64), [1, 3, 4], 2));
        assert!(!exact_current_file(Some(3_u64), [1, 2, 3], 2));
    }

    #[test]
    fn hunk_navigation_stops_at_both_boundaries() {
        let mut current = None;
        assert_eq!(navigate_hunk(&[3, 8], &mut current, true), Some(3));
        assert_eq!(navigate_hunk(&[3, 8], &mut current, true), Some(8));
        assert_eq!(navigate_hunk(&[3, 8], &mut current, true), Some(8));
        assert_eq!(navigate_hunk(&[3, 8], &mut current, false), Some(3));
        assert_eq!(navigate_hunk(&[3, 8], &mut current, false), Some(3));
    }

    #[test]
    fn empty_hunk_navigation_is_inert() {
        let mut current = Some(9);
        assert_eq!(navigate_hunk(&[], &mut current, true), None);
        assert_eq!(current, None);
    }

    #[test]
    fn hunk_heading_is_structured_not_raw_patch() {
        assert_eq!(
            hunk_label(2, 3, 5, 7, Some("fn demo")),
            "@@ -2,3 +5,7 @@ fn demo"
        );
    }

    #[test]
    fn prepared_line_preserves_long_text_as_one_row() {
        let long = "x".repeat(64 * 1024);
        let pairs = pair_side_by_side(&[line(DiffRowKind::Addition, &long)]);
        assert_eq!(pairs.len(), 1);
        assert_eq!(
            pairs[0]
                .right
                .as_ref()
                .map(|line| line.spans.iter().map(|span| span.text.len()).sum()),
            Some(long.len())
        );
    }

    #[test]
    fn frozen_layout_constants_are_exact() {
        assert_eq!(DIFF_REFRESH_INTERVAL, Duration::from_millis(750));
        assert_eq!(DIFF_ROW_HEIGHT, 24.0);
        assert_eq!(DIFF_MIN_WINDOW_WIDTH, 960.0);
        assert_eq!(DIFF_MIN_WINDOW_HEIGHT, 600.0);
        assert_eq!(DIFF_CHANGE_BACKGROUND_OPACITY, 0.08);
    }

    #[gpui::test]
    async fn focused_escape_closes_the_exact_diff_route(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            crate::init(cx);
        });
        let view = cx.new(|cx| DiffView::new("thread".into(), "project".into(), cx));
        let root_view = view.clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let window: WindowHandle<Harness> = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|cx| {
                    cx.subscribe(&root_view, move |_, _, event: &DiffClosed, _| {
                        if let Ok(mut events) = captured.lock() {
                            events.push(event.clone());
                        }
                    })
                    .detach();
                    Harness { view: root_view }
                })
            })
            .expect("diff test window")
        });
        window
            .update(cx, |_, window, cx| {
                let focus = view.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            })
            .expect("diff focus window");
        cx.simulate_keystrokes(window.into(), "] [ escape");
        let events = events.lock().expect("diff close events");
        assert_eq!(
            events.as_slice(),
            &[DiffClosed {
                thread_id: "thread".into(),
                project_id: "project".into(),
            }]
        );
    }
}
