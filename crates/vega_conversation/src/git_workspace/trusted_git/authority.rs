use super::*;

pub(crate) fn failed_prepare(
    code: CommitErrorCode,
    workspace: Option<WorkspaceSnapshot>,
) -> CommitPrepareCompletion {
    CommitPrepareCompletion {
        prepared: None,
        workspace,
        error: Some(code),
    }
}

pub(crate) fn map_git_error(error: GitWorkspaceError) -> CommitErrorCode {
    map_workspace_error(error)
}

pub(crate) fn map_workspace_error(error: GitWorkspaceError) -> CommitErrorCode {
    match error.code() {
        GitWorkspaceErrorCode::InvalidRoot => CommitErrorCode::InvalidRoot,
        GitWorkspaceErrorCode::NotRepository => CommitErrorCode::NotRepository,
        GitWorkspaceErrorCode::SpawnFailed => CommitErrorCode::SpawnFailed,
        GitWorkspaceErrorCode::TimedOut => CommitErrorCode::TimedOut,
        GitWorkspaceErrorCode::Cancelled => CommitErrorCode::Cancelled,
        GitWorkspaceErrorCode::OutputTooLarge => CommitErrorCode::OutputTooLarge,
        GitWorkspaceErrorCode::MalformedOutput => CommitErrorCode::MalformedOutput,
        GitWorkspaceErrorCode::ProcessControlFailed => CommitErrorCode::ProcessControlFailed,
        GitWorkspaceErrorCode::ChangedDuringRead | GitWorkspaceErrorCode::StaleGeneration => {
            CommitErrorCode::ChangedDuringRead
        }
        _ => CommitErrorCode::GitFailed,
    }
}

pub(crate) fn capture_authority(
    runner: &Runner,
    workspace_generation: u64,
    cancel: &CancellationToken,
) -> Result<IndexAuthority, CommitErrorCode> {
    let top = runner
        .run(
            "rev-parse",
            &[OsString::from("--show-toplevel")],
            STDOUT_LIMIT,
            cancel,
        )
        .map_err(map_workspace_error)?;
    if exact_line(&top.stdout)? != runner.root.as_os_str().as_bytes() {
        return Err(CommitErrorCode::InvalidRoot);
    }
    super::branch::reject_operation_markers(runner, cancel).map_err(map_workspace_error)?;
    let head = capture_head(runner, cancel)?;
    let status = runner
        .run("status", &status_args(), SNAPSHOT_LIMIT, cancel)
        .map_err(map_workspace_error)?;
    let stage = runner
        .run(
            "ls-files",
            &[OsString::from("--stage"), OsString::from("-z")],
            SNAPSHOT_LIMIT
                .checked_sub(status.stdout.len())
                .ok_or(CommitErrorCode::OutputTooLarge)?,
            cancel,
        )
        .map_err(map_workspace_error)?;
    let tree_raw = if head.unborn {
        Vec::new()
    } else {
        let tree = runner
            .run(
                "ls-tree",
                &[
                    OsString::from("-r"),
                    OsString::from("-z"),
                    OsString::from("--full-tree"),
                    OsString::from_vec(head.oid.clone()),
                ],
                SNAPSHOT_LIMIT
                    .checked_sub(status.stdout.len())
                    .and_then(|left| left.checked_sub(stage.stdout.len()))
                    .ok_or(CommitErrorCode::OutputTooLarge)?,
                cancel,
            )
            .map_err(map_workspace_error)?;
        tree.stdout
    };
    let authority = finalize_authority(
        head.clone(),
        status.stdout,
        stage.stdout,
        tree_raw,
        workspace_generation,
    )?;
    if capture_head(runner, cancel)? != head {
        return Err(CommitErrorCode::ChangedDuringRead);
    }
    Ok(authority)
}

pub(crate) fn finalize_authority(
    head: HeadAuthority,
    status_raw: Vec<u8>,
    stage_raw: Vec<u8>,
    tree_raw: Vec<u8>,
    workspace_generation: u64,
) -> Result<IndexAuthority, CommitErrorCode> {
    let records = parse_commit_status(&status_raw, &head)?;
    let stages = parse_stages(&stage_raw, head.oid.len())?;
    let tree = if head.unborn {
        if !tree_raw.is_empty() {
            return Err(CommitErrorCode::MalformedOutput);
        }
        Vec::new()
    } else {
        parse_tree(&tree_raw, head.oid.len())?
    };
    let retained = status_raw
        .len()
        .checked_add(stage_raw.len())
        .and_then(|bytes| bytes.checked_add(tree_raw.len()))
        .ok_or(CommitErrorCode::OutputTooLarge)?;
    if retained > SNAPSHOT_LIMIT || logical_path_count(&records, &stages, &tree)? > PATH_LIMIT {
        return Err(CommitErrorCode::OutputTooLarge);
    }
    cross_check_authority(&records, &stages, &tree)?;
    Ok(IndexAuthority {
        head,
        status_raw,
        stage_raw,
        tree_raw,
        records,
        stages,
        tree,
        workspace_generation,
    })
}

pub(crate) fn capture_head(
    runner: &Runner,
    cancel: &CancellationToken,
) -> Result<HeadAuthority, CommitErrorCode> {
    let status = runner
        .run("status", &status_args(), STDOUT_LIMIT, cancel)
        .map_err(map_workspace_error)?;
    let (oid, short) = parse_branch_headers(&status.stdout)?;
    let object_format = runner
        .run(
            "rev-parse",
            &[OsString::from("--show-object-format")],
            STDOUT_LIMIT,
            cancel,
        )
        .map_err(map_workspace_error)?;
    let width = match exact_line(&object_format.stdout)? {
        b"sha1" => 40,
        b"sha256" => 64,
        _ => return Err(CommitErrorCode::MalformedOutput),
    };
    let unborn = oid == b"(initial)";
    if short == b"(detached)" || short.is_empty() {
        return Err(CommitErrorCode::UnsafeRepository);
    }
    validate_ref_short(&short)?;
    let mut full_ref = b"refs/heads/".to_vec();
    full_ref.extend_from_slice(&short);
    if unborn {
        return Ok(HeadAuthority {
            unborn: true,
            oid: vec![b'0'; width],
            short,
            full_ref,
        });
    }
    if !valid_nonzero_oid(&oid, width) {
        return Err(CommitErrorCode::MalformedOutput);
    }
    let refs = runner
        .run(
            "for-each-ref",
            &[
                OsString::from("--sort=refname"),
                OsString::from("--format=%(objectname)%00%(refname)%00"),
                OsString::from("refs/heads/"),
            ],
            STDOUT_LIMIT,
            cancel,
        )
        .map_err(map_workspace_error)?;
    let found = parse_ref_target(&refs.stdout, &full_ref, oid.len())?;
    if found != oid {
        return Err(CommitErrorCode::ChangedDuringRead);
    }
    Ok(HeadAuthority {
        unborn: false,
        oid,
        short,
        full_ref,
    })
}

pub(crate) fn parse_branch_headers(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CommitErrorCode> {
    let mut oid = None;
    let mut head = None;
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            if oid.replace(value.to_vec()).is_some() {
                return Err(CommitErrorCode::MalformedOutput);
            }
        } else if let Some(value) = record.strip_prefix(b"# branch.head ") {
            if head.replace(value.to_vec()).is_some() {
                return Err(CommitErrorCode::MalformedOutput);
            }
        } else if record.starts_with(b"# ") {
            return Err(CommitErrorCode::MalformedOutput);
        }
    }
    Ok((
        oid.ok_or(CommitErrorCode::MalformedOutput)?,
        head.ok_or(CommitErrorCode::MalformedOutput)?,
    ))
}

pub(crate) fn parse_ref_target(
    bytes: &[u8],
    wanted: &[u8],
    width: usize,
) -> Result<Vec<u8>, CommitErrorCode> {
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(CommitErrorCode::MalformedOutput);
    }
    let mut found = None;
    let mut seen = BTreeSet::new();
    for record in bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
    {
        let fields: Vec<&[u8]> = record.split(|byte| *byte == 0).collect();
        if fields.len() != 3
            || !fields[2].is_empty()
            || !valid_nonzero_oid(fields[0], width)
            || !seen.insert(fields[1].to_vec())
        {
            return Err(CommitErrorCode::MalformedOutput);
        }
        if fields[1] == wanted && found.replace(fields[0].to_vec()).is_some() {
            return Err(CommitErrorCode::MalformedOutput);
        }
    }
    found.ok_or(CommitErrorCode::ChangedDuringRead)
}

pub(crate) fn parse_commit_status(
    bytes: &[u8],
    head: &HeadAuthority,
) -> Result<Vec<StatusRecord>, CommitErrorCode> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(CommitErrorCode::MalformedOutput);
    }
    let mut records = Vec::new();
    let (status_oid, status_head) = parse_branch_headers(bytes)?;
    if status_head != head.short
        || (head.unborn && status_oid != b"(initial)")
        || (!head.unborn && status_oid != head.oid)
    {
        return Err(CommitErrorCode::ChangedDuringRead);
    }
    let fields: Vec<&[u8]> = bytes.split(|byte| *byte == 0).collect();
    let mut index = 0;
    while index < fields.len() {
        let record = fields[index];
        if record.is_empty() {
            if index + 1 == fields.len() {
                break;
            }
            return Err(CommitErrorCode::MalformedOutput);
        }
        index += 1;
        if record.starts_with(b"# ") {
            if record.starts_with(b"# branch.oid ") || record.starts_with(b"# branch.head ") {
                continue;
            }
            return Err(CommitErrorCode::MalformedOutput);
        }
        if let Some(path) = record.strip_prefix(b"? ") {
            validate_relative_path(path).map_err(map_workspace_error)?;
            records.push(StatusRecord {
                shape: StatusShape::Untracked,
                x: b'?',
                y: b'?',
                sub: b"N...".to_vec(),
                head_mode: b"000000".to_vec(),
                index_mode: b"000000".to_vec(),
                worktree_mode: b"100644".to_vec(),
                head_oid: vec![b'0'; head.oid.len()],
                index_oid: vec![b'0'; head.oid.len()],
                path: path.to_vec(),
                previous: None,
            });
            continue;
        }
        if record.starts_with(b"u ") || record.starts_with(b"! ") {
            return Err(CommitErrorCode::UnsafeRepository);
        }
        let parts: Vec<&[u8]> = record.splitn(9, |byte| *byte == b' ').collect();
        if parts.len() != 9 || !matches!(parts[0], b"1" | b"2") {
            return Err(CommitErrorCode::MalformedOutput);
        }
        let xy = parts[1];
        if xy.len() != 2 || xy.iter().any(|value| !valid_status_code(*value)) {
            return Err(CommitErrorCode::MalformedOutput);
        }
        let mut shape = StatusShape::Ordinary;
        let mut path = parts[8];
        let mut previous = None;
        if parts[0] == b"2" {
            let score_split = path
                .iter()
                .position(|byte| *byte == b' ')
                .ok_or(CommitErrorCode::MalformedOutput)?;
            let score = &path[..score_split];
            path = &path[score_split + 1..];
            shape = parse_score(score)?;
            let old = fields.get(index).ok_or(CommitErrorCode::MalformedOutput)?;
            index += 1;
            validate_relative_path(old).map_err(map_workspace_error)?;
            previous = Some(old.to_vec());
        }
        if xy == b".A" {
            return Err(CommitErrorCode::IntentToAdd);
        }
        if !canonical_status_pair(shape, xy[0], xy[1]) {
            return Err(CommitErrorCode::MalformedOutput);
        }
        validate_relative_path(path).map_err(map_workspace_error)?;
        for mode in [parts[3], parts[4], parts[5]] {
            if !valid_mode_or_zero(mode) {
                return Err(CommitErrorCode::MalformedOutput);
            }
        }
        for oid in [parts[6], parts[7]] {
            if !valid_oid_or_zero(oid, head.oid.len()) {
                return Err(CommitErrorCode::MalformedOutput);
            }
        }
        if !canonical_status_modes(
            xy[0], xy[1], parts[3], parts[4], parts[5], parts[6], parts[7],
        ) {
            return Err(CommitErrorCode::MalformedOutput);
        }
        records.push(StatusRecord {
            shape,
            x: xy[0],
            y: xy[1],
            sub: parts[2].to_vec(),
            head_mode: parts[3].to_vec(),
            index_mode: parts[4].to_vec(),
            worktree_mode: parts[5].to_vec(),
            head_oid: parts[6].to_vec(),
            index_oid: parts[7].to_vec(),
            path: path.to_vec(),
            previous,
        });
    }
    records.sort_by(|left, right| left.path.cmp(&right.path));
    if records.windows(2).any(|pair| pair[0].path == pair[1].path) {
        return Err(CommitErrorCode::MalformedOutput);
    }
    Ok(records)
}

pub(crate) fn parse_stages(bytes: &[u8], width: usize) -> Result<Vec<StageEntry>, CommitErrorCode> {
    let mut entries = parse_nul_records(bytes, |record| {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or(CommitErrorCode::MalformedOutput)?;
        let fields: Vec<&[u8]> = record[..tab].split(|byte| *byte == b' ').collect();
        if fields.len() != 3
            || !valid_index_mode(fields[0])
            || !valid_nonzero_oid(fields[1], width)
            || fields[2] != b"0"
        {
            return Err(CommitErrorCode::MalformedOutput);
        }
        let path = &record[tab + 1..];
        validate_relative_path(path).map_err(map_workspace_error)?;
        Ok(StageEntry {
            mode: fields[0].to_vec(),
            oid: fields[1].to_vec(),
            path: path.to_vec(),
        })
    })?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    if entries.windows(2).any(|pair| pair[0].path == pair[1].path) {
        return Err(CommitErrorCode::MalformedOutput);
    }
    Ok(entries)
}

pub(crate) fn parse_tree(bytes: &[u8], width: usize) -> Result<Vec<TreeEntry>, CommitErrorCode> {
    let mut entries = parse_nul_records(bytes, |record| {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or(CommitErrorCode::MalformedOutput)?;
        let fields: Vec<&[u8]> = record[..tab].split(|byte| *byte == b' ').collect();
        if fields.len() != 3 || !valid_nonzero_oid(fields[2], width) {
            return Err(CommitErrorCode::MalformedOutput);
        }
        let valid_type = matches!(
            (fields[0], fields[1]),
            (b"100644" | b"100755" | b"120000", b"blob") | (b"160000", b"commit")
        );
        if !valid_type {
            return Err(CommitErrorCode::MalformedOutput);
        }
        let path = &record[tab + 1..];
        validate_relative_path(path).map_err(map_workspace_error)?;
        Ok(TreeEntry {
            mode: fields[0].to_vec(),
            object_type: fields[1].to_vec(),
            oid: fields[2].to_vec(),
            path: path.to_vec(),
        })
    })?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    if entries.windows(2).any(|pair| pair[0].path == pair[1].path) {
        return Err(CommitErrorCode::MalformedOutput);
    }
    Ok(entries)
}

pub(crate) fn parse_nul_records<T>(
    bytes: &[u8],
    mut parse: impl FnMut(&[u8]) -> Result<T, CommitErrorCode>,
) -> Result<Vec<T>, CommitErrorCode>
where
    T: Ord,
{
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(&[0]) {
        return Err(CommitErrorCode::MalformedOutput);
    }
    let mut values = Vec::new();
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    while let Some(record) = records.next() {
        if record.is_empty() {
            if records.peek().is_none() {
                break;
            }
            return Err(CommitErrorCode::MalformedOutput);
        }
        values.push(parse(record)?);
    }
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(CommitErrorCode::MalformedOutput);
    }
    Ok(values)
}

pub(crate) fn canonical_status_pair(shape: StatusShape, x: u8, y: u8) -> bool {
    match shape {
        StatusShape::Ordinary => match x {
            b'.' => matches!(y, b'M' | b'T' | b'D'),
            b'M' | b'T' | b'A' => matches!(y, b'.' | b'M' | b'T' | b'D'),
            b'D' => y == b'.',
            _ => false,
        },
        StatusShape::Rename => x == b'R' && matches!(y, b'.' | b'M' | b'T' | b'D'),
        StatusShape::Copy => x == b'C' && matches!(y, b'.' | b'M' | b'T' | b'D'),
        StatusShape::Untracked => x == b'?' && y == b'?',
    }
}

pub(crate) fn canonical_status_modes(
    x: u8,
    y: u8,
    head_mode: &[u8],
    index_mode: &[u8],
    worktree_mode: &[u8],
    head_oid: &[u8],
    index_oid: &[u8],
) -> bool {
    let head_present = !is_zero_mode(head_mode) && !is_zero_oid(head_oid);
    let index_present = !is_zero_mode(index_mode) && !is_zero_oid(index_oid);
    let x_valid = match x {
        b'.' => head_present && index_present && head_mode == index_mode && head_oid == index_oid,
        b'M' => {
            head_present
                && index_present
                && same_mode_kind(head_mode, index_mode)
                && (head_mode != index_mode || head_oid != index_oid)
        }
        b'T' => head_present && index_present && !same_mode_kind(head_mode, index_mode),
        b'A' => !head_present && index_present,
        b'D' => head_present && !index_present,
        b'R' | b'C' => head_present && index_present,
        _ => false,
    };
    let worktree_valid = match y {
        b'.' => {
            if index_present {
                worktree_mode == index_mode
            } else {
                is_zero_mode(worktree_mode)
            }
        }
        b'M' => index_present && same_mode_kind(index_mode, worktree_mode),
        b'T' => {
            index_present
                && !is_zero_mode(worktree_mode)
                && !same_mode_kind(index_mode, worktree_mode)
        }
        b'D' => index_present && is_zero_mode(worktree_mode),
        _ => false,
    };
    x_valid && worktree_valid
}

pub(crate) fn same_mode_kind(left: &[u8], right: &[u8]) -> bool {
    matches!(
        (left, right),
        (b"100644" | b"100755", b"100644" | b"100755")
    ) || left == right
}

pub(crate) fn cross_check_authority(
    records: &[StatusRecord],
    stages: &[StageEntry],
    tree: &[TreeEntry],
) -> Result<(), CommitErrorCode> {
    let stage_map: BTreeMap<&[u8], &StageEntry> = stages
        .iter()
        .map(|entry| (entry.path.as_slice(), entry))
        .collect();
    let tree_map: BTreeMap<&[u8], &TreeEntry> = tree
        .iter()
        .map(|entry| (entry.path.as_slice(), entry))
        .collect();
    let mut renamed_sources = BTreeSet::new();
    let mut copied_sources = BTreeSet::new();
    for record in records {
        if record.shape == StatusShape::Untracked {
            if stage_map.contains_key(record.path.as_slice())
                || tree_map.contains_key(record.path.as_slice())
            {
                return Err(CommitErrorCode::MalformedOutput);
            }
            continue;
        }
        if record.sub != b"N..." || record.x == b'U' || record.y == b'U' {
            return Err(CommitErrorCode::UnsafeRepository);
        }
        let head_path = record.previous.as_deref().unwrap_or(&record.path);
        match tree_map.get(head_path) {
            Some(entry) => {
                if entry.mode != record.head_mode || entry.oid != record.head_oid {
                    return Err(CommitErrorCode::MalformedOutput);
                }
            }
            None => {
                if !is_zero_mode(&record.head_mode)
                    || !is_zero_oid(&record.head_oid)
                    || record.x == b'.'
                {
                    return Err(if record.x == b'.' {
                        CommitErrorCode::IntentToAdd
                    } else {
                        CommitErrorCode::MalformedOutput
                    });
                }
            }
        }
        match stage_map.get(record.path.as_slice()) {
            Some(entry) => {
                if entry.mode != record.index_mode || entry.oid != record.index_oid {
                    return Err(CommitErrorCode::MalformedOutput);
                }
            }
            None => {
                if !is_zero_mode(&record.index_mode) || !is_zero_oid(&record.index_oid) {
                    return Err(CommitErrorCode::MalformedOutput);
                }
            }
        }
        if matches!(record.shape, StatusShape::Rename | StatusShape::Copy) {
            let previous = record
                .previous
                .as_deref()
                .ok_or(CommitErrorCode::MalformedOutput)?;
            if previous == record.path || tree_map.contains_key(record.path.as_slice()) {
                return Err(CommitErrorCode::MalformedOutput);
            }
            match record.shape {
                StatusShape::Rename => {
                    if stage_map.contains_key(previous)
                        || !renamed_sources.insert(previous)
                        || copied_sources.contains(previous)
                    {
                        return Err(CommitErrorCode::MalformedOutput);
                    }
                }
                StatusShape::Copy => {
                    let source_stage = stage_map
                        .get(previous)
                        .ok_or(CommitErrorCode::MalformedOutput)?;
                    let source_tree = tree_map
                        .get(previous)
                        .ok_or(CommitErrorCode::MalformedOutput)?;
                    if source_stage.mode != source_tree.mode
                        || source_stage.oid != source_tree.oid
                        || renamed_sources.contains(previous)
                    {
                        return Err(CommitErrorCode::MalformedOutput);
                    }
                    copied_sources.insert(previous);
                }
                StatusShape::Ordinary | StatusShape::Untracked => {
                    return Err(CommitErrorCode::MalformedOutput);
                }
            }
        }
    }
    let mut gitlink_paths = BTreeSet::new();
    gitlink_paths.extend(
        stages
            .iter()
            .filter(|entry| entry.mode == b"160000")
            .map(|entry| entry.path.as_slice()),
    );
    gitlink_paths.extend(
        tree.iter()
            .filter(|entry| entry.mode == b"160000")
            .map(|entry| entry.path.as_slice()),
    );
    for path in gitlink_paths {
        let unchanged = stage_map.get(path).is_some_and(|stage| {
            stage.mode == b"160000"
                && tree_map.get(path).is_some_and(|tree| {
                    tree.mode == b"160000" && tree.object_type == b"commit" && tree.oid == stage.oid
                })
        });
        let changed = records.iter().any(|record| {
            record.path.as_slice() == path || record.previous.as_deref() == Some(path)
        });
        if !unchanged || changed {
            return Err(CommitErrorCode::UnsafeRepository);
        }
    }
    let mut delta_paths = BTreeSet::new();
    delta_paths.extend(stage_map.keys().copied());
    delta_paths.extend(tree_map.keys().copied());
    for path in delta_paths {
        if stage_map.get(path).is_some_and(|stage| {
            tree_map
                .get(path)
                .is_some_and(|tree| stage.mode == tree.mode && stage.oid == tree.oid)
        }) {
            continue;
        }
        let explained = records.iter().any(|record| {
            record.x != b'.'
                && (record.path.as_slice() == path
                    || (record.shape == StatusShape::Rename
                        && record.previous.as_deref() == Some(path)))
        });
        if !explained {
            return Err(CommitErrorCode::MalformedOutput);
        }
    }
    Ok(())
}

pub(crate) fn project_selection(
    file: &WorkspaceFile,
    record: &StatusRecord,
    forced: bool,
) -> Result<CommitSelection, CommitErrorCode> {
    let code = if forced { record.x } else { record.y };
    let kind = if forced && matches!(record.shape, StatusShape::Rename) {
        CommitSelectionKind::Renamed
    } else if forced && matches!(record.shape, StatusShape::Copy) {
        CommitSelectionKind::Copied
    } else {
        match code {
            b'A' | b'?' => CommitSelectionKind::Added,
            b'M' => CommitSelectionKind::Modified,
            b'D' => CommitSelectionKind::Deleted,
            b'T' => CommitSelectionKind::TypeChanged,
            b'R' => CommitSelectionKind::Renamed,
            b'C' => CommitSelectionKind::Copied,
            _ => return Err(CommitErrorCode::MalformedOutput),
        }
    };
    Ok(CommitSelection {
        file_id: file.id,
        label: file.label.clone(),
        previous_label: file.previous_label.clone(),
        kind,
        forced,
    })
}

pub(crate) fn resolve_selected<'a>(
    checklist: &'a StoredChecklist,
    selected: &[WorkspaceFileId],
) -> Result<Vec<&'a ChecklistRow>, CommitErrorCode> {
    let mut unique = HashSet::new();
    let mut rows = Vec::new();
    for id in selected {
        if !unique.insert(*id) {
            return Err(CommitErrorCode::InvalidSelection);
        }
        let row = checklist
            .optional
            .iter()
            .find(|row| row.public.file_id == *id)
            .ok_or(CommitErrorCode::InvalidSelection)?;
        if row.optional_kind != CommitSelectionKind::Deleted && row.worktree_mode.is_none() {
            return Err(CommitErrorCode::InvalidSelection);
        }
        rows.push(row);
    }
    Ok(rows)
}

pub(crate) fn selected_paths(rows: &[&ChecklistRow]) -> Result<Vec<Vec<u8>>, CommitErrorCode> {
    let mut paths = BTreeSet::new();
    for row in rows {
        if record_closure(&row.record)
            .iter()
            .any(|path| is_gitattributes(path))
        {
            return Err(CommitErrorCode::UnsafeFilter);
        }
        for path in &row.closure {
            paths.insert(path.clone());
        }
    }
    if paths.len() > PATH_LIMIT {
        return Err(CommitErrorCode::OutputTooLarge);
    }
    Ok(paths.into_iter().collect())
}

pub(crate) fn record_closure(record: &StatusRecord) -> Vec<Vec<u8>> {
    let mut paths = vec![record.path.clone()];
    if let Some(previous) = &record.previous {
        paths.push(previous.clone());
    }
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn component_closure(record: &StatusRecord, forced: bool) -> Vec<Vec<u8>> {
    let includes_previous = if forced {
        matches!(record.shape, StatusShape::Rename | StatusShape::Copy)
    } else {
        matches!(record.y, b'R' | b'C')
    };
    let mut paths = vec![record.path.clone()];
    if includes_previous && let Some(previous) = &record.previous {
        paths.push(previous.clone());
    }
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn is_gitattributes(path: &[u8]) -> bool {
    path.rsplit(|byte| *byte == b'/').next() == Some(b".gitattributes")
}
