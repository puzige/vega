use super::*;

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

pub(crate) fn diff_button(label: &'static str, color: gpui::Rgba) -> gpui::Div {
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

pub(crate) fn render_prepared_row(
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

pub(crate) fn render_diff_half(
    line: Option<PreparedLine>,
    colors: &ThemeColors,
    side: LineSide,
) -> gpui::Div {
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

pub(crate) fn render_diff_line(
    line: PreparedLine,
    colors: &ThemeColors,
    side: LineSide,
) -> AnyElement {
    render_diff_line_div(line, colors, side).into_any_element()
}

pub(crate) fn render_diff_line_div(
    line: PreparedLine,
    colors: &ThemeColors,
    side: LineSide,
) -> gpui::Div {
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

pub(crate) struct PreparedProjectionRows {
    pub(crate) rows: Vec<PreparedRow>,
    pub(crate) hunk_indexes: Vec<usize>,
}

pub(crate) fn prepare_projection(projection: &DiffTextProjection) -> PreparedProjection {
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

pub(crate) fn layout_projection_rows(
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

pub(crate) fn reconcile_expanded<T: Copy + Eq>(old: Option<T>, current: &[T]) -> Option<T> {
    old.filter(|id| current.contains(id))
}

pub(crate) fn should_preserve_projection<T: Eq>(
    old_generation: Option<u64>,
    new_generation: u64,
    old_expanded: Option<T>,
    new_expanded: Option<T>,
) -> bool {
    old_generation == Some(new_generation) && old_expanded == new_expanded
}

pub(crate) fn exact_current_file<T: Copy + Eq>(
    expanded: Option<T>,
    current: impl IntoIterator<Item = T>,
    candidate: T,
) -> bool {
    expanded == Some(candidate) && current.into_iter().any(|id| id == candidate)
}

pub(crate) fn pair_side_by_side(lines: &[PreparedLine]) -> Vec<SidePair> {
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

pub(crate) fn navigate_hunk(
    indexes: &[usize],
    current: &mut Option<usize>,
    forward: bool,
) -> Option<usize> {
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

pub(crate) fn prepare_spans(text: &str, language: DiffLanguage) -> Vec<PreparedSpan> {
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

pub(crate) fn file_summary(file: &WorkspaceFile) -> String {
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

pub(crate) fn change_label(kind: WorkspaceChangeKind) -> &'static str {
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

pub(crate) fn count_label(count: WorkspaceLineCount) -> String {
    match count {
        WorkspaceLineCount::Known(value) => value.to_string(),
        WorkspaceLineCount::Binary => "binary".to_owned(),
        WorkspaceLineCount::Unknown => "?".to_owned(),
    }
}

pub(crate) fn error_label(code: GitWorkspaceErrorCode) -> &'static str {
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
        | GitWorkspaceErrorCode::ArtifactLimit
        | GitWorkspaceErrorCode::BranchDirty
        | GitWorkspaceErrorCode::BranchOperationInProgress
        | GitWorkspaceErrorCode::BranchDetached
        | GitWorkspaceErrorCode::BranchUnborn
        | GitWorkspaceErrorCode::BranchUnsafeFilter
        | GitWorkspaceErrorCode::BranchAlreadyCurrent => "Git diff could not be loaded safely.",
    }
}

pub(crate) fn head_label(head: &WorkspaceHead) -> String {
    match head {
        WorkspaceHead::Branch { label } => label.clone(),
        WorkspaceHead::Detached => "Detached HEAD".to_owned(),
        WorkspaceHead::Unborn { label: Some(label) } => format!("{label} (unborn)"),
        WorkspaceHead::Unborn { label: None } => "Unborn HEAD".to_owned(),
    }
}

pub(crate) fn layer_label(layer: DiffLayer) -> &'static str {
    match layer {
        DiffLayer::Staged => "Staged",
        DiffLayer::Unstaged => "Unstaged",
        DiffLayer::Untracked => "Untracked",
    }
}

pub(crate) fn hunk_label(
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
