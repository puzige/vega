use super::*;

pub(crate) fn validate_transition(
    a: &IndexAuthority,
    b: &IndexAuthority,
    selected: &[&ChecklistRow],
    _paths: &[Vec<u8>],
) -> Result<(), CommitErrorCode> {
    let selected_paths: BTreeSet<&[u8]> = selected
        .iter()
        .flat_map(|row| row.closure.iter().map(Vec::as_slice))
        .collect();
    // A selected destination edit on a staged copy/rename never owns the
    // source's independent worktree component. Freeze that outside-S source
    // record byte-exact even though it participates in structural topology.
    for row in selected.iter().filter(|row| {
        matches!(row.record.shape, StatusShape::Rename | StatusShape::Copy)
            && matches!(
                row.optional_kind,
                CommitSelectionKind::Modified | CommitSelectionKind::TypeChanged
            )
    }) {
        let previous = row
            .record
            .previous
            .as_deref()
            .ok_or(CommitErrorCode::ChangedDuringRead)?;
        let a_source = a.records.iter().find(|record| record.path == previous);
        let b_source = b.records.iter().find(|record| record.path == previous);
        let legal_rename_split = row.record.shape == StatusShape::Rename
            && a_source.is_none()
            && b_source.is_some_and(|record| {
                record.shape == StatusShape::Ordinary
                    && record.previous.is_none()
                    && record.x == b'D'
                    && record.y == b'.'
            });
        if a_source != b_source && !legal_rename_split {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
    }
    let mut owners: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (owner, row) in selected.iter().enumerate() {
        let structural_closure =
            if (matches!(row.record.shape, StatusShape::Rename | StatusShape::Copy)
                && matches!(
                    row.optional_kind,
                    CommitSelectionKind::Modified | CommitSelectionKind::TypeChanged
                ))
                || (row.record.shape == StatusShape::Rename
                    && row.optional_kind == CommitSelectionKind::Deleted)
            {
                record_closure(&row.record)
            } else {
                row.closure.clone()
            };
        for path in &structural_closure {
            let path_owners = owners.entry(path.clone()).or_default();
            if !path_owners.is_empty() {
                let shared_copy_source = row.record.shape == StatusShape::Copy
                    && row.record.previous.as_deref() == Some(path.as_slice())
                    && path_owners.iter().all(|existing| {
                        selected[*existing].record.shape == StatusShape::Copy
                            && selected[*existing].record.previous.as_deref()
                                == Some(path.as_slice())
                    });
                if !shared_copy_source {
                    return Err(CommitErrorCode::InvalidSelection);
                }
            }
            path_owners.push(owner);
        }
    }
    let a_stage: BTreeMap<&[u8], &StageEntry> = a
        .stages
        .iter()
        .map(|entry| (entry.path.as_slice(), entry))
        .collect();
    let b_stage: BTreeMap<&[u8], &StageEntry> = b
        .stages
        .iter()
        .map(|entry| (entry.path.as_slice(), entry))
        .collect();
    for (path, entry) in &a_stage {
        if !selected_paths.contains(path) && b_stage.get(path).copied() != Some(*entry) {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
    }
    for (path, entry) in &b_stage {
        if !selected_paths.contains(path) && a_stage.get(path).copied() != Some(*entry) {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
    }
    let mut stage_paths = BTreeSet::new();
    stage_paths.extend(a_stage.keys().copied());
    stage_paths.extend(b_stage.keys().copied());
    for path in stage_paths {
        if a_stage.get(path) != b_stage.get(path) && !owners.contains_key(path) {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
    }
    for record in &a.records {
        if !record_closure(record)
            .iter()
            .any(|path| selected_paths.contains(path.as_slice()))
            && !b.records.contains(record)
        {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
    }
    for (record, other) in a
        .records
        .iter()
        .map(|record| (record, b.records.as_slice()))
        .chain(
            b.records
                .iter()
                .map(|record| (record, a.records.as_slice())),
        )
    {
        if !other.contains(record) {
            let same_topology = other.iter().any(|candidate| {
                candidate.path == record.path
                    && candidate.previous == record.previous
                    && candidate.shape == record.shape
                    && candidate.x == record.x
            });
            let closure = if same_topology {
                component_closure(record, false)
            } else {
                record_closure(record)
            };
            let mut candidates: Option<BTreeSet<usize>> = None;
            for path in closure {
                let path_owners: BTreeSet<usize> = owners
                    .get(path.as_slice())
                    .ok_or(CommitErrorCode::ChangedDuringRead)?
                    .iter()
                    .copied()
                    .collect();
                candidates = Some(match candidates {
                    None => path_owners,
                    Some(current) => current.intersection(&path_owners).copied().collect(),
                });
            }
            let selected_delete_untracked_merge = record.shape == StatusShape::Rename
                && record.previous.as_ref().is_some_and(|previous| {
                    is_selected_delete_untracked_rename(
                        selected,
                        &b.records,
                        previous,
                        &record.path,
                    )
                });
            if candidates.is_none_or(|candidates| candidates.len() != 1)
                && !selected_delete_untracked_merge
            {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
        }
    }
    for row in selected {
        let a_record = &row.record;
        let b_record = b
            .records
            .iter()
            .find(|record| record.path == a_record.path && record.previous == a_record.previous);
        match row.optional_kind {
            CommitSelectionKind::Deleted => {
                let merged_rename = selected.iter().any(|candidate| {
                    candidate.optional_kind == CommitSelectionKind::Added
                        && candidate.record.shape == StatusShape::Untracked
                        && is_selected_delete_untracked_rename(
                            selected,
                            &b.records,
                            &a_record.path,
                            &candidate.record.path,
                        )
                });
                let canonical_delete = if a_record.shape == StatusShape::Rename {
                    a_record.previous.as_ref().is_some_and(|previous| {
                        let mut old_records =
                            b.records.iter().filter(|record| record.path == *previous);
                        old_records.next().is_some_and(|record| {
                            record.shape == StatusShape::Ordinary
                                && record.previous.is_none()
                                && record.x == b'D'
                                && record.y == b'.'
                        }) && old_records.next().is_none()
                            && !b.records.iter().any(|record| {
                                record.previous.as_deref() == Some(previous.as_slice())
                            })
                            && !b_stage.contains_key(previous.as_slice())
                            && !b.records.iter().any(|record| record.path == a_record.path)
                    })
                } else {
                    b_record.is_some_and(|record| record.x == b'D' && record.y == b'.')
                        || merged_rename
                };
                if b_stage.contains_key(a_record.path.as_slice()) || !canonical_delete {
                    return Err(CommitErrorCode::ChangedDuringRead);
                }
            }
            CommitSelectionKind::Renamed => {
                let Some(record) = b_record else {
                    return Err(CommitErrorCode::ChangedDuringRead);
                };
                let Some(previous) = &a_record.previous else {
                    return Err(CommitErrorCode::ChangedDuringRead);
                };
                let exact_mode = row.worktree_mode.as_ref().is_some_and(|mode| {
                    b_stage
                        .get(a_record.path.as_slice())
                        .is_some_and(|entry| entry.mode == *mode)
                });
                if !exact_mode
                    || record.shape != StatusShape::Rename
                    || record.previous.as_ref() != Some(previous)
                    || record.path != a_record.path
                    || record.y != b'.'
                    || b_stage.contains_key(previous.as_slice())
                    || !b_stage.contains_key(a_record.path.as_slice())
                {
                    return Err(CommitErrorCode::ChangedDuringRead);
                }
            }
            CommitSelectionKind::Copied => {
                let Some(previous) = &a_record.previous else {
                    return Err(CommitErrorCode::ChangedDuringRead);
                };
                let Some(record) = b_record else {
                    return Err(CommitErrorCode::ChangedDuringRead);
                };
                let exact_mode = row.worktree_mode.as_ref().is_some_and(|mode| {
                    b_stage
                        .get(a_record.path.as_slice())
                        .is_some_and(|entry| entry.mode == *mode)
                });
                if !exact_mode
                    || a_stage.get(previous.as_slice()) != b_stage.get(previous.as_slice())
                    || !b_stage.contains_key(a_record.path.as_slice())
                    || record.y != b'.'
                    || !matches!(record.x, b'A' | b'C')
                    || (record.x == b'C'
                        && (record.shape != StatusShape::Copy
                            || record.previous.as_ref() != Some(previous)))
                {
                    return Err(CommitErrorCode::ChangedDuringRead);
                }
            }
            CommitSelectionKind::Added => {
                let merged_rename = selected.iter().any(|candidate| {
                    candidate.optional_kind == CommitSelectionKind::Deleted
                        && is_selected_delete_untracked_rename(
                            selected,
                            &b.records,
                            &candidate.record.path,
                            &a_record.path,
                        )
                });
                let expected_mode = row.worktree_mode.as_ref().is_some_and(|mode| {
                    b_stage
                        .get(a_record.path.as_slice())
                        .is_some_and(|entry| entry.mode == *mode)
                });
                if !expected_mode
                    || (!merged_rename
                        && b_record.is_none_or(|record| record.x != b'A' || record.y != b'.'))
                {
                    return Err(CommitErrorCode::ChangedDuringRead);
                }
            }
            CommitSelectionKind::Modified | CommitSelectionKind::TypeChanged => {
                let normalized_noop = !has_real_delta(b)
                    && row.optional_kind == CommitSelectionKind::Modified
                    && a_record.shape == StatusShape::Ordinary
                    && a_record.x == b'.'
                    && a_record.y == b'M'
                    && b_record.is_none()
                    && a_stage.get(a_record.path.as_slice())
                        == b_stage.get(a_record.path.as_slice())
                    && b_stage.get(a_record.path.as_slice()).is_some_and(|entry| {
                        entry.mode == a_record.worktree_mode
                            && b.tree.iter().any(|tree| {
                                tree.path == entry.path
                                    && tree.mode == entry.mode
                                    && tree.oid == entry.oid
                            })
                    });
                if normalized_noop {
                    continue;
                }
                let Some(entry) = b_stage.get(a_record.path.as_slice()) else {
                    return Err(CommitErrorCode::ChangedDuringRead);
                };
                let exact_topology = b_record.is_some_and(|record| {
                    let expected_x = if a_record.x == b'.' {
                        matches!(record.x, b'M' | b'T')
                    } else {
                        record.x == a_record.x
                            && record.shape == a_record.shape
                            && record.previous == a_record.previous
                    };
                    record.y == b'.' && expected_x
                });
                let split_rename = if a_record.shape == StatusShape::Rename {
                    a_record.previous.as_ref().is_some_and(|previous| {
                        b.records.iter().any(|record| {
                            record.shape == StatusShape::Ordinary
                                && record.path == a_record.path
                                && record.previous.is_none()
                                && record.x == b'A'
                                && record.y == b'.'
                        }) && b.records.iter().any(|record| {
                            record.shape == StatusShape::Ordinary
                                && record.path == *previous
                                && record.previous.is_none()
                                && record.x == b'D'
                                && record.y == b'.'
                        }) && !b_stage.contains_key(previous.as_slice())
                    })
                } else {
                    false
                };
                let split_copy = if a_record.shape == StatusShape::Copy {
                    a_record.previous.as_ref().is_some_and(|previous| {
                        b.records.iter().any(|record| {
                            record.shape == StatusShape::Ordinary
                                && record.path == a_record.path
                                && record.previous.is_none()
                                && record.x == b'A'
                                && record.y == b'.'
                        }) && a_stage.get(previous.as_slice()) == b_stage.get(previous.as_slice())
                    })
                } else {
                    false
                };
                if (!exact_topology && !split_rename && !split_copy)
                    || entry.mode != a_record.worktree_mode
                {
                    return Err(CommitErrorCode::ChangedDuringRead);
                }
            }
        }
    }
    if b.records.iter().any(|record| {
        record.y != b'.'
            && record_closure(record)
                .iter()
                .any(|path| selected_paths.contains(path.as_slice()))
    }) {
        return Err(CommitErrorCode::ChangedDuringRead);
    }
    Ok(())
}

pub(crate) fn is_selected_delete_untracked_rename(
    selected: &[&ChecklistRow],
    b_records: &[StatusRecord],
    source: &[u8],
    destination: &[u8],
) -> bool {
    let source_selected = selected.iter().filter(|row| {
        row.optional_kind == CommitSelectionKind::Deleted
            && row.record.shape == StatusShape::Ordinary
            && row.record.x == b'.'
            && row.record.y == b'D'
            && row.record.previous.is_none()
            && row.record.path == source
    });
    let destination_selected = selected.iter().filter(|row| {
        row.optional_kind == CommitSelectionKind::Added
            && row.record.shape == StatusShape::Untracked
            && row.record.previous.is_none()
            && row.record.path == destination
    });
    if source_selected.count() != 1 || destination_selected.count() != 1 {
        return false;
    }
    let mut touching = b_records.iter().filter(|record| {
        record.path == source
            || record.path == destination
            || record
                .previous
                .as_deref()
                .is_some_and(|previous| previous == source || previous == destination)
    });
    let Some(merged) = touching.next() else {
        return false;
    };
    touching.next().is_none()
        && merged.shape == StatusShape::Rename
        && merged.x == b'R'
        && merged.y == b'.'
        && merged.path == destination
        && merged.previous.as_deref() == Some(source)
}

pub(crate) fn optional_kind(record: &StatusRecord) -> CommitSelectionKind {
    match record.y {
        b'D' => CommitSelectionKind::Deleted,
        b'T' => CommitSelectionKind::TypeChanged,
        b'?' | b'A' => CommitSelectionKind::Added,
        b'R' => CommitSelectionKind::Renamed,
        b'C' => CommitSelectionKind::Copied,
        _ => CommitSelectionKind::Modified,
    }
}

pub(crate) fn has_real_delta(authority: &IndexAuthority) -> bool {
    !stage_matches_tree(&authority.stages, &authority.tree)
}

pub(crate) fn stage_matches_tree(stages: &[StageEntry], tree: &[TreeEntry]) -> bool {
    stages.len() == tree.len()
        && stages.iter().zip(tree).all(|(stage, tree)| {
            stage.mode == tree.mode && stage.oid == tree.oid && stage.path == tree.path
        })
}

pub(crate) fn logical_path_count(
    records: &[StatusRecord],
    stages: &[StageEntry],
    tree: &[TreeEntry],
) -> Result<usize, CommitErrorCode> {
    let mut paths = BTreeSet::new();
    for record in records {
        paths.insert(record.path.as_slice());
        if let Some(previous) = &record.previous {
            paths.insert(previous.as_slice());
        }
    }
    paths.extend(stages.iter().map(|entry| entry.path.as_slice()));
    paths.extend(tree.iter().map(|entry| entry.path.as_slice()));
    Ok(paths.len())
}

pub(crate) fn parse_parent_lines(
    bytes: &[u8],
    width: usize,
) -> Result<Vec<Vec<u8>>, CommitErrorCode> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(b"\n") {
        return Err(CommitErrorCode::MalformedOutput);
    }
    bytes[..bytes.len() - 1]
        .split(|byte| *byte == b'\n')
        .map(|line| {
            valid_oid_width(line, width)
                .then(|| line.to_vec())
                .ok_or(CommitErrorCode::MalformedOutput)
        })
        .collect()
}

pub(crate) struct EscapedSummary {
    pub(crate) rendered: String,
    marker_cut: usize,
}

impl EscapedSummary {
    fn new() -> Self {
        Self {
            rendered: String::new(),
            marker_cut: 0,
        }
    }

    fn record_boundary(&mut self) {
        let marker_target = SUMMARY_LIMIT - SUMMARY_MARKER.len();
        if self.rendered.len() <= marker_target {
            self.marker_cut = self.rendered.len();
        }
    }

    fn push_literal(&mut self, character: char) {
        self.rendered.push(character);
        self.record_boundary();
    }

    fn push_generated_escape(&mut self, byte: u8) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        self.rendered.push('\\');
        self.rendered.push('x');
        self.rendered.push(char::from(HEX[usize::from(byte >> 4)]));
        self.rendered
            .push(char::from(HEX[usize::from(byte & 0x0f)]));
        self.record_boundary();
    }
}

pub(crate) fn escape_summary(raw: &[u8]) -> Result<EscapedSummary, CommitErrorCode> {
    let mut escaped = EscapedSummary::new();
    let mut index = 0;
    while index < raw.len() {
        match std::str::from_utf8(&raw[index..]) {
            Ok(valid) => {
                push_escaped_controls(&mut escaped, valid.as_bytes())?;
                break;
            }
            Err(error) => {
                let valid = &raw[index..index + error.valid_up_to()];
                push_escaped_controls(&mut escaped, valid)?;
                index += error.valid_up_to();
                let invalid = error.error_len().unwrap_or(1);
                for byte in &raw[index..index + invalid] {
                    escaped.push_generated_escape(*byte);
                }
                index += invalid;
            }
        }
    }
    Ok(escaped)
}

pub(crate) fn push_escaped_controls(
    target: &mut EscapedSummary,
    bytes: &[u8],
) -> Result<(), CommitErrorCode> {
    let value = std::str::from_utf8(bytes).map_err(|_| CommitErrorCode::MalformedOutput)?;
    for character in value.chars() {
        if character == '\n' || character == '\t' || !character.is_control() {
            target.push_literal(character);
        } else {
            for byte in character.to_string().as_bytes() {
                target.push_generated_escape(*byte);
            }
        }
    }
    Ok(())
}

pub(crate) fn truncate_summary(mut escaped: EscapedSummary, raw_overflow: bool) -> (String, bool) {
    if escaped.rendered.len() <= SUMMARY_LIMIT && !raw_overflow {
        return (escaped.rendered, false);
    }
    escaped.rendered.truncate(escaped.marker_cut);
    escaped
        .rendered
        .push_str(std::str::from_utf8(SUMMARY_MARKER).unwrap_or(""));
    (escaped.rendered, true)
}

pub(crate) async fn collect_draft(
    provider: Arc<dyn Provider>,
    request: ChatRequest,
    cancel: CancellationToken,
) -> Result<String, CommitErrorCode> {
    let mut stream = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(CommitErrorCode::DraftFailed),
        result = provider.chat_stream(request, cancel.clone()) => {
            result.map_err(|_| CommitErrorCode::DraftFailed)?
        }
    };
    let mut text = String::new();
    let mut usage_started = false;
    let mut done = false;
    loop {
        let item = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(CommitErrorCode::DraftFailed),
            item = stream.next() => item,
        };
        match item {
            Some(Ok(ProviderEvent::TextDelta(delta))) if !usage_started && !done => {
                checked_draft_len(text.len(), delta.len())?;
                if delta.as_bytes().contains(&0) {
                    return Err(CommitErrorCode::DraftFailed);
                }
                text.push_str(&delta);
            }
            Some(Ok(ProviderEvent::Usage { .. })) if !done => usage_started = true,
            Some(Ok(ProviderEvent::Done {
                stop_reason: StopReason::End,
            })) if !done => done = true,
            None if done && !text.is_empty() => return Ok(text),
            _ => return Err(CommitErrorCode::DraftFailed),
        }
    }
}

pub(crate) async fn collect_draft_with_deadline(
    provider: Arc<dyn Provider>,
    request: ChatRequest,
    cancel: CancellationToken,
    deadline: Duration,
) -> Result<String, CommitErrorCode> {
    tokio::time::timeout(deadline, collect_draft(provider, request, cancel))
        .await
        .map_err(|_| CommitErrorCode::DraftFailed)?
}

pub(crate) fn checked_draft_len(current: usize, delta: usize) -> Result<usize, CommitErrorCode> {
    let next = current
        .checked_add(delta)
        .ok_or(CommitErrorCode::DraftFailed)?;
    if next > MESSAGE_LIMIT {
        return Err(CommitErrorCode::DraftFailed);
    }
    Ok(next)
}

pub(crate) fn exact_line(bytes: &[u8]) -> Result<&[u8], CommitErrorCode> {
    bytes
        .strip_suffix(b"\n")
        .filter(|line| !line.is_empty() && !line.contains(&b'\n') && !line.contains(&0))
        .ok_or(CommitErrorCode::MalformedOutput)
}

pub(crate) fn valid_status_code(code: u8) -> bool {
    matches!(code, b'.' | b'M' | b'T' | b'A' | b'D' | b'R' | b'C' | b'U')
}

pub(crate) fn valid_mode_or_zero(mode: &[u8]) -> bool {
    is_zero_mode(mode) || valid_index_mode(mode)
}

pub(crate) fn valid_index_mode(mode: &[u8]) -> bool {
    matches!(mode, b"100644" | b"100755" | b"120000" | b"160000")
}

pub(crate) fn is_zero_mode(mode: &[u8]) -> bool {
    mode == b"000000"
}

pub(crate) fn valid_oid_or_zero(oid: &[u8], width: usize) -> bool {
    is_zero_oid(oid) || valid_oid_width(oid, width)
}

pub(crate) fn valid_nonzero_oid(oid: &[u8], width: usize) -> bool {
    valid_oid_width(oid, width) && !is_zero_oid(oid)
}

pub(crate) fn is_zero_oid(oid: &[u8]) -> bool {
    !oid.is_empty() && oid.iter().all(|byte| *byte == b'0')
}

pub(crate) fn valid_oid_width(oid: &[u8], width: usize) -> bool {
    oid.len() == width
        && matches!(width, 40 | 64)
        && oid
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub(crate) fn parse_score(score: &[u8]) -> Result<StatusShape, CommitErrorCode> {
    let (&kind, digits) = score
        .split_first()
        .ok_or(CommitErrorCode::MalformedOutput)?;
    let value = std::str::from_utf8(digits)
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|value| *value <= 100)
        .ok_or(CommitErrorCode::MalformedOutput)?;
    let _ = value;
    match kind {
        b'R' => Ok(StatusShape::Rename),
        b'C' => Ok(StatusShape::Copy),
        _ => Err(CommitErrorCode::MalformedOutput),
    }
}

pub(crate) fn validate_ref_short(short: &[u8]) -> Result<(), CommitErrorCode> {
    if short.is_empty()
        || short[0] == b'-'
        || short == b"HEAD"
        || short == b"@"
        || short.starts_with(b"/")
        || short.ends_with(b"/")
        || short.ends_with(b".")
        || short
            .windows(2)
            .any(|window| window == b".." || window == b"//" || window == b"@{")
        || short.split(|byte| *byte == b'/').any(|component| {
            component.is_empty() || component.starts_with(b".") || component.ends_with(b".lock")
        })
        || short.iter().any(|byte| {
            *byte < 0x20
                || *byte == 0x7f
                || matches!(
                    *byte,
                    b' ' | b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\'
                )
        })
    {
        return Err(CommitErrorCode::UnsafeRepository);
    }
    Ok(())
}
