use super::*;

pub(crate) fn insert_status(
    files: &mut BTreeMap<Vec<u8>, ParsedFile>,
    path: &[u8],
    previous_path: Option<&[u8]>,
    staged: WorkspaceChangeKind,
    unstaged: WorkspaceChangeKind,
    metadata_only: bool,
) -> Result<(), GitWorkspaceError> {
    validate_relative_path(path)?;
    if let Some(previous) = previous_path {
        validate_relative_path(previous)?;
    }
    if files.contains_key(path) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    files.insert(
        path.to_vec(),
        ParsedFile {
            path: path.to_vec(),
            previous_path: previous_path.map(<[u8]>::to_vec),
            staged,
            unstaged,
            additions: WorkspaceLineCount::Unknown,
            deletions: WorkspaceLineCount::Unknown,
            metadata_only,
        },
    );
    Ok(())
}

pub(crate) fn validate_relative_path(path: &[u8]) -> Result<(), GitWorkspaceError> {
    if path.is_empty()
        || path[0] == b'/'
        || path
            .split(|byte| *byte == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b".." || part == b".git")
    {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok(())
}

pub(crate) struct RawEntry {
    pub(crate) path: Vec<u8>,
    pub(crate) previous_path: Option<Vec<u8>>,
    pub(crate) kind: WorkspaceChangeKind,
    pub(crate) metadata_only: bool,
}

pub(crate) fn validate_raw(bytes: &[u8]) -> Result<Vec<RawEntry>, GitWorkspaceError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut unmerged_companions = BTreeSet::new();
    let mut oid_width = None;
    while let Some(header) = records.next() {
        if header.is_empty() && records.peek().is_none() {
            break;
        }
        let header = header
            .strip_prefix(b":")
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let mut pieces = header.splitn(5, |byte| *byte == b' ');
        let old_mode = pieces.next().ok_or_else(malformed)?;
        let new_mode = pieces.next().ok_or_else(malformed)?;
        let old_oid = pieces.next().ok_or_else(malformed)?;
        let new_oid = pieces.next().ok_or_else(malformed)?;
        if !valid_mode(old_mode)
            || !valid_mode(new_mode)
            || !valid_oid(old_oid)
            || !valid_oid(new_oid)
            || old_oid.len() != new_oid.len()
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        if oid_width
            .replace(old_oid.len())
            .is_some_and(|width| width != old_oid.len())
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let status = pieces
            .next()
            .filter(|piece| !piece.is_empty())
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        if !valid_raw_status(status) {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let path = records
            .next()
            .filter(|piece| !piece.is_empty())
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        validate_relative_path(path)?;
        let raw_metadata_only = matches!(old_mode, b"120000" | b"160000")
            || matches!(new_mode, b"120000" | b"160000")
            || status[0] == b'T';
        let mut previous_path = None;
        if matches!(status[0], b'R' | b'C') {
            let second = records
                .next()
                .filter(|piece| !piece.is_empty())
                .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
            validate_relative_path(second)?;
            previous_path = Some(path.to_vec());
            if !seen.insert(second.to_vec()) {
                return Err(error(GitWorkspaceErrorCode::MalformedOutput));
            }
            entries.push(RawEntry {
                path: second.to_vec(),
                previous_path,
                kind: if status[0] == b'R' {
                    WorkspaceChangeKind::Renamed
                } else {
                    WorkspaceChangeKind::Copied
                },
                metadata_only: raw_metadata_only,
            });
        } else {
            let kind = parse_change(status[0])?;
            let duplicate = !seen.insert(path.to_vec());
            if duplicate
                && !(kind == WorkspaceChangeKind::Modified
                    && !unmerged_companions.contains(path)
                    && entries.iter().any(|entry| {
                        entry.path == path
                            && entry.kind == WorkspaceChangeKind::Unmerged
                            && entry.previous_path.is_none()
                    }))
            {
                return Err(error(GitWorkspaceErrorCode::MalformedOutput));
            }
            if duplicate {
                unmerged_companions.insert(path.to_vec());
            }
            entries.push(RawEntry {
                path: path.to_vec(),
                previous_path,
                kind,
                metadata_only: raw_metadata_only,
            });
        }
    }
    Ok(entries)
}

pub(crate) fn cross_check_raw(
    files: &mut BTreeMap<Vec<u8>, ParsedFile>,
    entries: &[RawEntry],
    staged: bool,
) -> Result<(), GitWorkspaceError> {
    let expected: BTreeMap<Vec<u8>, (WorkspaceChangeKind, Option<Vec<u8>>)> = files
        .values()
        .filter_map(|file| {
            let kind = if staged { file.staged } else { file.unstaged };
            (kind != WorkspaceChangeKind::Unchanged && kind != WorkspaceChangeKind::Untracked)
                .then_some((file.path.clone(), (kind, file.previous_path.clone())))
        })
        .collect();
    let entry_paths: BTreeSet<&[u8]> = entries.iter().map(|entry| entry.path.as_slice()).collect();
    if entry_paths.len() > expected.len()
        || expected.iter().any(|(path, (kind, _))| {
            !entry_paths.contains(path.as_slice())
                && (staged || *kind != WorkspaceChangeKind::Modified)
        })
    {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    for entry in entries {
        let (kind, previous_path) = expected
            .get(entry.path.as_slice())
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let expected_previous = matches!(
            kind,
            WorkspaceChangeKind::Renamed | WorkspaceChangeKind::Copied
        )
        .then_some(previous_path.as_ref())
        .flatten();
        if (*kind != entry.kind
            && !(*kind == WorkspaceChangeKind::Unmerged
                && entry.kind == WorkspaceChangeKind::Modified))
            || expected_previous.map(Vec::as_slice) != entry.previous_path.as_deref()
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        if entry.metadata_only {
            files
                .get_mut(entry.path.as_slice())
                .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?
                .metadata_only = true;
        }
    }
    Ok(())
}

pub(crate) fn malformed() -> GitWorkspaceError {
    error(GitWorkspaceErrorCode::MalformedOutput)
}

pub(crate) fn valid_mode(mode: &[u8]) -> bool {
    matches!(
        mode,
        b"000000" | b"100644" | b"100755" | b"120000" | b"160000"
    )
}

pub(crate) fn special_modes(modes: &[&[u8]]) -> bool {
    modes
        .iter()
        .any(|mode| matches!(*mode, b"120000" | b"160000"))
}

pub(crate) fn valid_oid(oid: &[u8]) -> bool {
    matches!(oid.len(), 40 | 64)
        && oid
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub(crate) fn valid_raw_status(status: &[u8]) -> bool {
    match status.first() {
        Some(b'M' | b'A' | b'D' | b'T' | b'U') => status.len() == 1,
        Some(b'R' | b'C') => {
            (2..=4).contains(&status.len())
                && status[1..].iter().all(u8::is_ascii_digit)
                && std::str::from_utf8(&status[1..])
                    .ok()
                    .and_then(|value| value.parse::<u16>().ok())
                    .is_some_and(|score| score <= 100)
        }
        _ => false,
    }
}

pub(crate) fn merge_numstat(
    files: &mut BTreeMap<Vec<u8>, ParsedFile>,
    bytes: &[u8],
    staged: bool,
) -> Result<Vec<Vec<u8>>, GitWorkspaceError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    let mut paths = Vec::new();
    let mut seen = BTreeMap::<Vec<u8>, usize>::new();
    while let Some(record) = records.next() {
        if record.is_empty() && records.peek().is_none() {
            break;
        }
        let first_tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let second_relative = record[first_tab + 1..]
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let second_tab = first_tab + 1 + second_relative;
        let additions = parse_count(&record[..first_tab])?;
        let deletions = parse_count(&record[first_tab + 1..second_tab])?;
        if matches!(additions, WorkspaceLineCount::Binary)
            != matches!(deletions, WorkspaceLineCount::Binary)
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let inline_path = &record[second_tab + 1..];
        let mut rename_old = None;
        let path = if inline_path.is_empty() {
            let old = records
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
            let new = records
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
            validate_relative_path(old)?;
            rename_old = Some(old);
            new
        } else {
            inline_path
        };
        validate_relative_path(path)?;
        let file = files
            .get_mut(path)
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let layer_kind = if staged { file.staged } else { file.unstaged };
        let multiplicity = seen.entry(path.to_vec()).or_insert(0);
        *multiplicity = multiplicity
            .checked_add(1)
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        if *multiplicity > 1 && !(layer_kind == WorkspaceChangeKind::Unmerged && *multiplicity == 2)
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let expected_old = matches!(
            layer_kind,
            WorkspaceChangeKind::Renamed | WorkspaceChangeKind::Copied
        )
        .then_some(file.previous_path.as_deref())
        .flatten();
        if rename_old != expected_old {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        if layer_kind == WorkspaceChangeKind::Unmerged {
            file.additions = WorkspaceLineCount::Unknown;
            file.deletions = WorkspaceLineCount::Unknown;
        } else {
            file.additions = merge_count(file.additions, additions)?;
            file.deletions = merge_count(file.deletions, deletions)?;
        }
        paths.push(path.to_vec());
    }
    Ok(paths)
}

pub(crate) fn parse_count(bytes: &[u8]) -> Result<WorkspaceLineCount, GitWorkspaceError> {
    if bytes == b"-" {
        return Ok(WorkspaceLineCount::Binary);
    }
    let string =
        std::str::from_utf8(bytes).map_err(|_| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let value = string
        .parse::<u64>()
        .map_err(|_| error(GitWorkspaceErrorCode::MalformedOutput))?;
    Ok(WorkspaceLineCount::Known(value))
}

pub(crate) fn merge_count(
    a: WorkspaceLineCount,
    b: WorkspaceLineCount,
) -> Result<WorkspaceLineCount, GitWorkspaceError> {
    Ok(match (a, b) {
        (WorkspaceLineCount::Binary, _) | (_, WorkspaceLineCount::Binary) => {
            WorkspaceLineCount::Binary
        }
        (WorkspaceLineCount::Known(a), WorkspaceLineCount::Known(b)) => WorkspaceLineCount::Known(
            a.checked_add(b)
                .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?,
        ),
        (WorkspaceLineCount::Unknown, value) | (value, WorkspaceLineCount::Unknown) => value,
    })
}

pub(crate) fn fold_count(
    total: &mut Option<u64>,
    value: WorkspaceLineCount,
) -> Result<(), GitWorkspaceError> {
    match value {
        WorkspaceLineCount::Known(value) => {
            if let Some(total) = total {
                *total = total
                    .checked_add(value)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
            }
        }
        WorkspaceLineCount::Binary | WorkspaceLineCount::Unknown => *total = None,
    }
    Ok(())
}

pub(crate) fn build_projection(
    runner: &Runner,
    file: PrivateFile,
    cancel: &CancellationToken,
) -> Result<DiffTextProjection, GitWorkspaceError> {
    verify_snapshot_identity(runner, &file.snapshot_identity, cancel)?;
    if read_worktree_identity(&runner.root, file.path.as_bytes())? != file.worktree_identity {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    if file.binary || file.metadata_only || file.staged == WorkspaceChangeKind::Unmerged {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let needs_worktree_hash = file
        .worktree_identity
        .is_some_and(|identity| identity.kind == 0)
        && !matches!(
            file.unstaged,
            WorkspaceChangeKind::Unchanged
                | WorkspaceChangeKind::Deleted
                | WorkspaceChangeKind::Untracked
        );
    let worktree_hash = needs_worktree_hash
        .then(|| hash_worktree_no_filters(runner, &file.path, cancel))
        .transpose()?;
    let mut sections = Vec::new();
    let mut remaining_bytes = PATCH_LIMIT;
    let mut remaining_rows = PATCH_ROW_LIMIT;
    if file.unstaged == WorkspaceChangeKind::Untracked {
        sections.push(project_untracked(
            runner,
            &file,
            cancel,
            &mut remaining_bytes,
            &mut remaining_rows,
        )?);
    } else {
        for (layer, changed) in [
            (
                DiffLayer::Staged,
                file.staged != WorkspaceChangeKind::Unchanged,
            ),
            (
                DiffLayer::Unstaged,
                file.unstaged != WorkspaceChangeKind::Unchanged,
            ),
        ] {
            if !changed {
                continue;
            }
            let mut args = vec![
                OsString::from("--patch"),
                OsString::from("--find-renames"),
                OsString::from("--no-ext-diff"),
                OsString::from("--no-textconv"),
                OsString::from("--unified=3"),
            ];
            if layer == DiffLayer::Staged {
                args.insert(0, OsString::from("--cached"));
            }
            args.push(OsString::from("--"));
            if let Some(previous) = &file.previous_path {
                args.push(previous.clone());
            }
            args.push(file.path.clone());
            verify_filter_bytes_with_retained(
                runner,
                &file.snapshot_identity.filter_paths,
                &file.snapshot_identity.filter_attrs,
                snapshot_identity_retained(&file.snapshot_identity)?,
                cancel,
            )?;
            let output = runner.run("diff", &args, remaining_bytes, cancel)?;
            consume_projection_bytes(&mut remaining_bytes, output.stdout.len())?;
            sections.push(parse_patch(layer, &output.stdout, &mut remaining_rows)?);
        }
    }
    let projection = DiffTextProjection {
        file_id: file.id,
        language: file.language,
        sections,
    };
    verify_snapshot_identity(runner, &file.snapshot_identity, cancel)?;
    if worktree_hash.is_some()
        && worktree_hash.as_ref() != Some(&hash_worktree_no_filters(runner, &file.path, cancel)?)
    {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    if read_worktree_identity(&runner.root, file.path.as_bytes())? != file.worktree_identity {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    Ok(projection)
}

pub(crate) fn consume_projection_bytes(
    remaining: &mut usize,
    bytes: usize,
) -> Result<(), GitWorkspaceError> {
    *remaining = remaining
        .checked_sub(bytes)
        .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
    Ok(())
}
