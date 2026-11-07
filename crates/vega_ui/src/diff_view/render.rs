use super::*;

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
                    key: "ui.no_newline_at_end_of_file",
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
        statuses.push(vega_i18n::t_args(
            "ui.diff_staged_status",
            &[("status", change_label(file.staged))],
        ));
    }
    if file.unstaged != WorkspaceChangeKind::Unchanged {
        statuses.push(vega_i18n::t_args(
            "ui.diff_worktree_status",
            &[("status", change_label(file.unstaged))],
        ));
    }
    if statuses.is_empty() {
        statuses.push(vega_i18n::t("ui.unchanged").to_owned());
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
        WorkspaceChangeKind::Unchanged => vega_i18n::t("ui.unchanged"),
        WorkspaceChangeKind::Added => vega_i18n::t("ui.added"),
        WorkspaceChangeKind::Modified => vega_i18n::t("ui.modified"),
        WorkspaceChangeKind::Deleted => vega_i18n::t("ui.deleted"),
        WorkspaceChangeKind::Renamed => vega_i18n::t("ui.renamed"),
        WorkspaceChangeKind::Copied => vega_i18n::t("ui.copied"),
        WorkspaceChangeKind::TypeChanged => vega_i18n::t("ui.type_changed"),
        WorkspaceChangeKind::Unmerged => vega_i18n::t("ui.unmerged"),
        WorkspaceChangeKind::Untracked => vega_i18n::t("ui.untracked"),
    }
}

pub(crate) fn count_label(count: WorkspaceLineCount) -> String {
    match count {
        WorkspaceLineCount::Known(value) => value.to_string(),
        WorkspaceLineCount::Binary => vega_i18n::t("ui.binary").to_owned(),
        WorkspaceLineCount::Unknown => "?".to_owned(),
    }
}

pub(crate) fn error_label(code: GitWorkspaceErrorCode) -> &'static str {
    match code {
        GitWorkspaceErrorCode::InvalidRoot => vega_i18n::t("ui.project_root_is_unavailable"),
        GitWorkspaceErrorCode::NotRepository => {
            vega_i18n::t("ui.this_project_is_not_a_git_repository")
        }
        GitWorkspaceErrorCode::GitUnavailable => {
            vega_i18n::t("ui.git_2_40_was_not_found_install_homebrew")
        }
        GitWorkspaceErrorCode::GitUnsupported => {
            vega_i18n::t("ui.git_2_40_is_required_upgrade_git_and")
        }
        GitWorkspaceErrorCode::GitExecutableChanged => {
            vega_i18n::t("ui.git_changed_while_vega_was_running_restart_vega")
        }
        GitWorkspaceErrorCode::TimedOut => {
            vega_i18n::t("ui.git_diff_timed_out_retry_when_the_repository")
        }
        GitWorkspaceErrorCode::Cancelled => vega_i18n::t("ui.git_diff_refresh_was_cancelled"),
        GitWorkspaceErrorCode::OutputTooLarge => {
            vega_i18n::t("ui.diff_exceeds_the_safe_display_limit")
        }
        GitWorkspaceErrorCode::MetadataOnly => {
            vega_i18n::t("ui.this_file_can_only_be_shown_as_metadata")
        }
        GitWorkspaceErrorCode::ChangedDuringRead => {
            vega_i18n::t("ui.the_file_changed_while_its_diff_was_loading")
        }
        GitWorkspaceErrorCode::StaleGeneration | GitWorkspaceErrorCode::UnknownFile => {
            vega_i18n::t("ui.this_diff_is_stale_refresh_and_try_again")
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
        | GitWorkspaceErrorCode::BranchAlreadyCurrent
        | GitWorkspaceErrorCode::BranchPathLimit
        | GitWorkspaceErrorCode::BranchCountLimit => {
            vega_i18n::t("ui.git_diff_could_not_be_loaded_safely")
        }
    }
}

pub(crate) fn head_label(head: &WorkspaceHead) -> String {
    match head {
        WorkspaceHead::Branch { label } => label.clone(),
        WorkspaceHead::Detached => vega_i18n::t("ui.detached_head").to_owned(),
        WorkspaceHead::Unborn { label: Some(label) } => {
            vega_i18n::t_args("ui.label_unborn", &[("label", label)])
        }
        WorkspaceHead::Unborn { label: None } => vega_i18n::t("ui.unborn_head").to_owned(),
    }
}

pub(crate) fn layer_label(layer: DiffLayer) -> &'static str {
    match layer {
        DiffLayer::Staged => "ui.staged",
        DiffLayer::Unstaged => "ui.unstaged",
        DiffLayer::Untracked => "ui.untracked_en",
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
