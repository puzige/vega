use super::*;

#[derive(Clone)]
pub(crate) struct ParsedFile {
    pub(crate) path: Vec<u8>,
    pub(crate) previous_path: Option<Vec<u8>>,
    pub(crate) staged: WorkspaceChangeKind,
    pub(crate) unstaged: WorkspaceChangeKind,
    pub(crate) additions: WorkspaceLineCount,
    pub(crate) deletions: WorkspaceLineCount,
    pub(crate) metadata_only: bool,
}

pub(crate) struct ParsedStatus {
    pub(crate) head: WorkspaceHead,
    pub(crate) files: BTreeMap<Vec<u8>, ParsedFile>,
}

pub(crate) fn build_snapshot(
    runner: &Runner,
    generation: u64,
    instance_nonce: u64,
    cancel: &CancellationToken,
) -> Result<(WorkspaceSnapshot, Vec<PrivateFile>, Arc<SnapshotIdentity>), GitWorkspaceError> {
    let top = runner.run(
        "rev-parse",
        &[OsString::from("--show-toplevel")],
        STDOUT_LIMIT,
        cancel,
    )?;
    let top_bytes = trim_one_newline(&top.stdout);
    let top_path = fs::canonicalize(PathBuf::from(OsString::from_vec(top_bytes.to_vec())))
        .map_err(|_| error(GitWorkspaceErrorCode::NotRepository))?;
    if top_path != runner.root {
        return Err(error(GitWorkspaceErrorCode::InvalidRoot));
    }
    let filter_identity = capture_filter_identity(runner, cancel, SNAPSHOT_LIMIT)?;
    let mut budget = RetainedBudget::new(SNAPSHOT_LIMIT);
    budget.charge(filter_identity.paths.len())?;
    budget.charge(filter_identity.attrs.len())?;
    verify_filter_bytes_with_retained(
        runner,
        &filter_identity.paths,
        &filter_identity.attrs,
        budget.retained(),
        cancel,
    )?;
    let status_output = runner.run("status", &status_args(), budget.remaining(), cancel)?;
    let mut parsed = parse_status(&status_output.stdout)?;
    let worktree_before = capture_worktree_identities(&runner.root, &parsed.files)?;
    budget.charge(status_output.stdout.len())?;
    let mut raw_outputs = Vec::with_capacity(2);
    let mut numstat_outputs = Vec::with_capacity(2);
    for cached in [true, false] {
        verify_filter_bytes_with_retained(
            runner,
            &filter_identity.paths,
            &filter_identity.attrs,
            budget.retained(),
            cancel,
        )?;
        let raw_args = raw_args(cached);
        let raw = runner.run("diff", &raw_args, budget.remaining(), cancel)?;
        let raw_entries = validate_raw(&raw.stdout)?;
        cross_check_raw(&mut parsed.files, &raw_entries, cached)?;
        budget.charge(raw.stdout.len())?;
        raw_outputs.push(raw.stdout);

        let numstat_args = numstat_args(cached);
        verify_filter_bytes_with_retained(
            runner,
            &filter_identity.paths,
            &filter_identity.attrs,
            budget.retained(),
            cancel,
        )?;
        let numstat = runner.run("diff", &numstat_args, budget.remaining(), cancel)?;
        let numstat_paths = merge_numstat(&mut parsed.files, &numstat.stdout, cached)?;
        let raw_paths = path_multiplicity(raw_entries.iter().map(|entry| entry.path.as_slice()));
        let numstat_paths = path_multiplicity(numstat_paths.iter().map(Vec::as_slice));
        if raw_paths != numstat_paths {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        budget.charge(numstat.stdout.len())?;
        numstat_outputs.push(numstat.stdout);
    }

    if parsed.files.len() > PATH_LIMIT {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    let snapshot_identity = Arc::new(SnapshotIdentity {
        filter_paths: filter_identity.paths,
        filter_attrs: filter_identity.attrs,
        status: status_output.stdout,
        staged_raw: raw_outputs.remove(0),
        unstaged_raw: raw_outputs.remove(0),
        staged_numstat: numstat_outputs.remove(0),
        unstaged_numstat: numstat_outputs.remove(0),
    });
    verify_snapshot_identity(runner, &snapshot_identity, cancel)?;
    let mut worktree_after = capture_worktree_identities(&runner.root, &parsed.files)?;
    if worktree_before != worktree_after {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    let mut public_files = Vec::with_capacity(parsed.files.len());
    let mut private_files = Vec::with_capacity(parsed.files.len());
    let mut aggregate_add = Some(0_u64);
    let mut aggregate_delete = Some(0_u64);
    for (slot, parsed_file) in parsed.files.into_values().enumerate() {
        let slot = u32::try_from(slot).map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        let seal = seal(
            runner.identity,
            instance_nonce,
            generation,
            slot,
            &parsed_file.path,
        );
        let id = WorkspaceFileId {
            generation,
            slot,
            seal,
        };
        let language = language_for(&parsed_file.path);
        let binary = matches!(parsed_file.additions, WorkspaceLineCount::Binary)
            || matches!(parsed_file.deletions, WorkspaceLineCount::Binary);
        let worktree_identity = worktree_after
            .remove(&parsed_file.path)
            .ok_or_else(|| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
        fold_count(&mut aggregate_add, parsed_file.additions)?;
        fold_count(&mut aggregate_delete, parsed_file.deletions)?;
        public_files.push(WorkspaceFile {
            id,
            label: escape_path(&parsed_file.path),
            previous_label: parsed_file.previous_path.as_deref().map(escape_path),
            staged: parsed_file.staged,
            unstaged: parsed_file.unstaged,
            additions: parsed_file.additions,
            deletions: parsed_file.deletions,
            language,
        });
        private_files.push(PrivateFile {
            id,
            path: OsString::from_vec(parsed_file.path),
            previous_path: parsed_file.previous_path.map(OsString::from_vec),
            staged: parsed_file.staged,
            unstaged: parsed_file.unstaged,
            binary,
            metadata_only: parsed_file.metadata_only,
            language,
            snapshot_identity: snapshot_identity.clone(),
            worktree_identity,
        });
    }
    let additions = aggregate_add.map_or(WorkspaceLineCount::Unknown, WorkspaceLineCount::Known);
    let deletions = aggregate_delete.map_or(WorkspaceLineCount::Unknown, WorkspaceLineCount::Known);
    let snapshot = WorkspaceSnapshot {
        generation,
        head: parsed.head,
        stats: WorkspaceStats {
            file_count: u32::try_from(public_files.len())
                .map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?,
            additions,
            deletions,
        },
        files: public_files,
    };
    ensure_candidate_retained(
        &snapshot_identity,
        &snapshot,
        &private_files,
        SNAPSHOT_LIMIT,
    )?;
    Ok((snapshot, private_files, snapshot_identity))
}

pub(crate) fn path_multiplicity<'a>(
    paths: impl Iterator<Item = &'a [u8]>,
) -> BTreeMap<Vec<u8>, usize> {
    let mut counts = BTreeMap::new();
    for path in paths {
        *counts.entry(path.to_vec()).or_insert(0) += 1;
    }
    counts
}

pub(crate) fn status_args() -> Vec<OsString> {
    vec![
        OsString::from("--porcelain=v2"),
        OsString::from("-z"),
        OsString::from("--branch"),
        OsString::from("--renames"),
        OsString::from("--untracked-files=all"),
    ]
}

pub(crate) struct FilterIdentity {
    pub(crate) paths: Arc<[u8]>,
    pub(crate) attrs: Vec<u8>,
}

pub(crate) fn capture_filter_identity(
    runner: &Runner,
    cancel: &CancellationToken,
    limit: usize,
) -> Result<FilterIdentity, GitWorkspaceError> {
    let paths = runner.run(
        "ls-files",
        &[
            OsString::from("-z"),
            OsString::from("--cached"),
            OsString::from("--deduplicate"),
        ],
        limit,
        cancel,
    )?;
    let path_bytes: Arc<[u8]> = paths.stdout.into();
    let parsed_paths = parse_nul_paths(&path_bytes)?;
    let remaining = limit
        .checked_sub(path_bytes.len())
        .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
    let attrs = runner.run_with_input(
        "check-attr",
        &[
            OsString::from("-z"),
            OsString::from("--stdin"),
            OsString::from("--all"),
        ],
        path_bytes.clone(),
        remaining,
        cancel,
    )?;
    validate_filter_attrs(&parsed_paths, &attrs.stdout)?;
    Ok(FilterIdentity {
        paths: path_bytes,
        attrs: attrs.stdout,
    })
}

pub(crate) fn parse_nul_paths(bytes: &[u8]) -> Result<Vec<Vec<u8>>, GitWorkspaceError> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    let mut fields = bytes.split(|byte| *byte == 0).peekable();
    while let Some(path) = fields.next() {
        if path.is_empty() {
            if fields.peek().is_none() {
                break;
            }
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        validate_relative_path(path)?;
        if paths.len() == PATH_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        if !seen.insert(path.to_vec()) {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        paths.push(path.to_vec());
    }
    Ok(paths)
}

pub(crate) fn validate_filter_attrs(
    paths: &[Vec<u8>],
    bytes: &[u8],
) -> Result<(), GitWorkspaceError> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let fields: Vec<&[u8]> = bytes.split(|byte| *byte == 0).collect();
    if fields.last().is_some_and(|field| field.is_empty()) {
        // trailing terminator is structural and excluded below
    } else if !fields.is_empty() {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let fields = if fields.last().is_some_and(|field| field.is_empty()) {
        &fields[..fields.len() - 1]
    } else {
        &fields[..]
    };
    let (triples, remainder) = fields.as_chunks::<3>();
    if !remainder.is_empty() {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let allowed: BTreeSet<&[u8]> = paths.iter().map(Vec::as_slice).collect();
    let mut seen = BTreeSet::new();
    for triple in triples {
        if !allowed.contains(triple[0])
            || triple[1].is_empty()
            || triple[2].is_empty()
            || !seen.insert((triple[0].to_vec(), triple[1].to_vec()))
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        if triple[1] == b"filter" {
            return Err(error(GitWorkspaceErrorCode::GitFailed));
        }
    }
    Ok(())
}

pub(crate) fn verify_filter_bytes_with_retained(
    runner: &Runner,
    expected_paths: &[u8],
    expected_attrs: &[u8],
    retained: usize,
    cancel: &CancellationToken,
) -> Result<(), GitWorkspaceError> {
    let remaining = SNAPSHOT_LIMIT
        .checked_sub(retained)
        .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
    let current = capture_filter_identity(runner, cancel, remaining)?;
    if current.paths.as_ref() != expected_paths || current.attrs != expected_attrs {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    Ok(())
}

pub(crate) fn raw_args(cached: bool) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("--raw"),
        OsString::from("-z"),
        OsString::from("--abbrev=64"),
        OsString::from("--find-renames"),
        OsString::from("--no-ext-diff"),
        OsString::from("--no-textconv"),
    ];
    if cached {
        args.insert(0, OsString::from("--cached"));
    }
    args
}

pub(crate) fn numstat_args(cached: bool) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("--numstat"),
        OsString::from("-z"),
        OsString::from("--find-renames"),
        OsString::from("--no-ext-diff"),
        OsString::from("--no-textconv"),
    ];
    if cached {
        args.insert(0, OsString::from("--cached"));
    }
    args
}

pub(crate) fn verify_snapshot_identity(
    runner: &Runner,
    expected: &SnapshotIdentity,
    cancel: &CancellationToken,
) -> Result<(), GitWorkspaceError> {
    let retained = snapshot_identity_retained(expected)?;
    let remaining = SNAPSHOT_LIMIT
        .checked_sub(retained)
        .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
    verify_filter_bytes_with_retained(
        runner,
        &expected.filter_paths,
        &expected.filter_attrs,
        retained,
        cancel,
    )?;
    {
        let status = runner.run("status", &status_args(), remaining, cancel)?;
        parse_status(&status.stdout)?;
        if status.stdout != expected.status {
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
    }
    verify_filter_bytes_with_retained(
        runner,
        &expected.filter_paths,
        &expected.filter_attrs,
        retained,
        cancel,
    )?;
    {
        let staged = runner.run("diff", &raw_args(true), remaining, cancel)?;
        validate_raw(&staged.stdout)?;
        if staged.stdout != expected.staged_raw {
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
    }
    verify_filter_bytes_with_retained(
        runner,
        &expected.filter_paths,
        &expected.filter_attrs,
        retained,
        cancel,
    )?;
    {
        let unstaged = runner.run("diff", &raw_args(false), remaining, cancel)?;
        validate_raw(&unstaged.stdout)?;
        if unstaged.stdout != expected.unstaged_raw {
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
    }
    for (cached, expected_numstat) in [
        (true, &expected.staged_numstat),
        (false, &expected.unstaged_numstat),
    ] {
        verify_filter_bytes_with_retained(
            runner,
            &expected.filter_paths,
            &expected.filter_attrs,
            retained,
            cancel,
        )?;
        let numstat = runner.run("diff", &numstat_args(cached), remaining, cancel)?;
        if numstat.stdout != *expected_numstat {
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
    }
    verify_filter_bytes_with_retained(
        runner,
        &expected.filter_paths,
        &expected.filter_attrs,
        retained,
        cancel,
    )?;
    Ok(())
}

pub(crate) fn snapshot_identity_retained(
    expected: &SnapshotIdentity,
) -> Result<usize, GitWorkspaceError> {
    let mut retained = 0_usize;
    charge_logical(&mut retained, std::mem::size_of::<SnapshotIdentity>())?;
    // Both Arc allocations retain two counters outside their pointed-to
    // value: the snapshot identity and its tracked-path slice.
    charge_logical(
        &mut retained,
        4_usize
            .checked_mul(std::mem::size_of::<usize>())
            .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?,
    )?;
    for bytes in [
        expected.filter_paths.len(),
        expected.filter_attrs.len(),
        expected.status.len(),
        expected.staged_raw.len(),
        expected.unstaged_raw.len(),
        expected.staged_numstat.len(),
        expected.unstaged_numstat.len(),
    ] {
        charge_logical(&mut retained, bytes)?;
    }
    Ok(retained)
}

pub(crate) fn ensure_candidate_retained(
    identity: &SnapshotIdentity,
    snapshot: &WorkspaceSnapshot,
    private_files: &[PrivateFile],
    cap: usize,
) -> Result<usize, GitWorkspaceError> {
    let mut retained = snapshot_identity_retained(identity)?;
    // The committed authority is one ServiceState allocation. Its fixed size
    // already includes counters plus the identity, snapshot and Vec handles;
    // candidate-local handles are deliberately not charged again.
    charge_logical(&mut retained, std::mem::size_of::<ServiceState>())?;
    let head_label = match &snapshot.head {
        WorkspaceHead::Branch { label } => label.len(),
        WorkspaceHead::Unborn { label } => label.as_ref().map_or(0, String::len),
        WorkspaceHead::Detached => 0,
    };
    charge_logical(&mut retained, head_label)?;
    for file in &snapshot.files {
        charge_logical(&mut retained, std::mem::size_of::<WorkspaceFile>())?;
        charge_logical(&mut retained, file.label.len())?;
        charge_logical(
            &mut retained,
            file.previous_label.as_ref().map_or(0, String::len),
        )?;
    }
    for file in private_files {
        charge_logical(&mut retained, std::mem::size_of::<PrivateFile>())?;
        charge_logical(&mut retained, file.path.as_bytes().len())?;
        charge_logical(
            &mut retained,
            file.previous_path
                .as_ref()
                .map_or(0, |path| path.as_bytes().len()),
        )?;
    }
    if retained > cap {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    Ok(retained)
}

pub(crate) fn charge_logical(retained: &mut usize, bytes: usize) -> Result<(), GitWorkspaceError> {
    *retained = retained
        .checked_add(bytes)
        .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
    Ok(())
}

pub(crate) fn capture_worktree_identities(
    root: &Path,
    files: &BTreeMap<Vec<u8>, ParsedFile>,
) -> Result<HashMap<Vec<u8>, Option<FileIdentity>>, GitWorkspaceError> {
    let mut identities = HashMap::with_capacity(files.len());
    for path in files.keys() {
        identities.insert(path.clone(), read_worktree_identity(root, path)?);
    }
    Ok(identities)
}

pub(crate) fn read_worktree_identity(
    root: &Path,
    path: &[u8],
) -> Result<Option<FileIdentity>, GitWorkspaceError> {
    match fs::symlink_metadata(root.join(OsString::from_vec(path.to_vec()))) {
        Ok(metadata) => Ok(Some(file_identity(&metadata))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(error(GitWorkspaceErrorCode::ChangedDuringRead)),
    }
}

pub(crate) fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
    let file_type = metadata.file_type();
    let kind = if file_type.is_file() {
        0
    } else if file_type.is_dir() {
        1
    } else if file_type.is_symlink() {
        2
    } else if file_type.is_block_device() {
        3
    } else if file_type.is_char_device() {
        4
    } else if file_type.is_fifo() {
        5
    } else if file_type.is_socket() {
        6
    } else {
        7
    };
    FileIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
        kind,
        mode: metadata.mode(),
        size: metadata.size(),
        mtime: metadata.mtime(),
        mtime_ns: metadata.mtime_nsec(),
        ctime: metadata.ctime(),
        ctime_ns: metadata.ctime_nsec(),
    }
}

pub(crate) fn parse_status(bytes: &[u8]) -> Result<ParsedStatus, GitWorkspaceError> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    let mut oid = None;
    let mut branch = None;
    let mut files = BTreeMap::new();
    while let Some(record) = records.next() {
        if record.is_empty() {
            if records.peek().is_none() {
                break;
            }
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            if oid.is_some()
                || (value != b"(initial)"
                    && !(matches!(value.len(), 40 | 64)
                        && value
                            .iter()
                            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))))
            {
                return Err(error(GitWorkspaceErrorCode::MalformedOutput));
            }
            oid = Some(value.to_vec());
            continue;
        }
        if let Some(value) = record.strip_prefix(b"# branch.head ") {
            if branch.is_some() || value.is_empty() {
                return Err(error(GitWorkspaceErrorCode::MalformedOutput));
            }
            branch = Some(value.to_vec());
            continue;
        }
        if record.starts_with(b"# branch.upstream ") || record.starts_with(b"# branch.ab ") {
            continue;
        }
        match record[0] {
            b'1' => {
                let fields = split_prefix_fields(record, 8)?;
                validate_ordinary_fields(&fields, false)?;
                let (staged, unstaged) = parse_xy(fields[1])?;
                insert_status(
                    &mut files,
                    fields[8],
                    None,
                    staged,
                    unstaged,
                    special_modes(&fields[3..=5])
                        || matches!(staged, WorkspaceChangeKind::TypeChanged)
                        || matches!(unstaged, WorkspaceChangeKind::TypeChanged),
                )?;
            }
            b'2' => {
                let fields = split_prefix_fields(record, 9)?;
                validate_ordinary_fields(&fields, true)?;
                let old = records
                    .next()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                let (staged, unstaged) = parse_xy(fields[1])?;
                insert_status(
                    &mut files,
                    fields[9],
                    Some(old),
                    staged,
                    unstaged,
                    special_modes(&fields[3..=5])
                        || matches!(staged, WorkspaceChangeKind::TypeChanged)
                        || matches!(unstaged, WorkspaceChangeKind::TypeChanged),
                )?;
            }
            b'u' => {
                let fields = split_prefix_fields(record, 10)?;
                if fields[0] != b"u"
                    || !valid_sub(fields[2])
                    || !fields[3..=6].iter().all(|field| valid_mode(field))
                    || !consistent_oids(&fields[7..=9])
                {
                    return Err(error(GitWorkspaceErrorCode::MalformedOutput));
                }
                insert_status(
                    &mut files,
                    fields[10],
                    None,
                    WorkspaceChangeKind::Unmerged,
                    WorkspaceChangeKind::Unmerged,
                    special_modes(&fields[3..=6]),
                )?;
            }
            b'?' => {
                let path = record
                    .strip_prefix(b"? ")
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                insert_status(
                    &mut files,
                    path,
                    None,
                    WorkspaceChangeKind::Unchanged,
                    WorkspaceChangeKind::Untracked,
                    false,
                )?;
            }
            _ => return Err(error(GitWorkspaceErrorCode::MalformedOutput)),
        }
        if files.len() > PATH_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
    }
    let oid = oid.ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let branch = branch.ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let branch_label = if branch == b"(detached)" {
        None
    } else {
        Some(escape_ref(&branch))
    };
    let head = if oid == b"(initial)" {
        WorkspaceHead::Unborn {
            label: branch_label,
        }
    } else if branch == b"(detached)" {
        WorkspaceHead::Detached
    } else {
        WorkspaceHead::Branch {
            label: branch_label.ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?,
        }
    };
    Ok(ParsedStatus { head, files })
}

pub(crate) fn split_prefix_fields(
    record: &[u8],
    spaces: usize,
) -> Result<Vec<&[u8]>, GitWorkspaceError> {
    let mut fields = Vec::with_capacity(spaces + 1);
    let mut start = 0;
    for _ in 0..spaces {
        let relative = record[start..]
            .iter()
            .position(|byte| *byte == b' ')
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let end = start + relative;
        fields.push(&record[start..end]);
        start = end + 1;
    }
    if start >= record.len() {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    fields.push(&record[start..]);
    Ok(fields)
}

pub(crate) fn validate_ordinary_fields(
    fields: &[&[u8]],
    renamed: bool,
) -> Result<(), GitWorkspaceError> {
    if fields[0] != if renamed { b"2" } else { b"1" }
        || !valid_sub(fields[2])
        || !fields[3..=5].iter().all(|field| valid_mode(field))
        || !consistent_oids(&fields[6..=7])
    {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    if renamed {
        let score = fields[8];
        if !matches!(score.first(), Some(b'R' | b'C'))
            || !(2..=4).contains(&score.len())
            || !score[1..].iter().all(u8::is_ascii_digit)
            || std::str::from_utf8(&score[1..])
                .ok()
                .and_then(|value| value.parse::<u16>().ok())
                .is_none_or(|value| value > 100)
            || !fields[1].contains(&score[0])
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
    }
    Ok(())
}

pub(crate) fn valid_sub(value: &[u8]) -> bool {
    value == b"N..."
        || (value.len() == 4
            && value[0] == b'S'
            && matches!(value[1], b'.' | b'C')
            && matches!(value[2], b'.' | b'M')
            && matches!(value[3], b'.' | b'U'))
}

pub(crate) fn consistent_oids(values: &[&[u8]]) -> bool {
    let Some(width) = values.first().map(|value| value.len()) else {
        return false;
    };
    matches!(width, 40 | 64)
        && values.iter().all(|value| {
            value.len() == width
                && value
                    .iter()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        })
}

pub(crate) fn parse_xy(
    value: &[u8],
) -> Result<(WorkspaceChangeKind, WorkspaceChangeKind), GitWorkspaceError> {
    if value.len() != 2 {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok((parse_change(value[0])?, parse_change(value[1])?))
}

pub(crate) fn parse_change(value: u8) -> Result<WorkspaceChangeKind, GitWorkspaceError> {
    match value {
        b'.' => Ok(WorkspaceChangeKind::Unchanged),
        b'A' => Ok(WorkspaceChangeKind::Added),
        b'M' => Ok(WorkspaceChangeKind::Modified),
        b'D' => Ok(WorkspaceChangeKind::Deleted),
        b'R' => Ok(WorkspaceChangeKind::Renamed),
        b'C' => Ok(WorkspaceChangeKind::Copied),
        b'T' => Ok(WorkspaceChangeKind::TypeChanged),
        b'U' => Ok(WorkspaceChangeKind::Unmerged),
        _ => Err(error(GitWorkspaceErrorCode::MalformedOutput)),
    }
}
