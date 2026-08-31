use super::*;

pub(crate) fn commit_branch_identity(
    state: &mut BranchState,
    mut identity: BranchIdentity,
    root_identity: RootIdentity,
    instance_nonce: u64,
) -> Result<BranchSnapshot, GitWorkspaceError> {
    if state
        .identity
        .as_deref()
        .is_some_and(|current| same_branch_identity(current, &identity))
    {
        return state
            .snapshot
            .clone()
            .ok_or_else(|| error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    let generation = state
        .next_generation
        .checked_add(1)
        .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
    assign_branch_ids(
        &mut identity.branches,
        generation,
        root_identity,
        instance_nonce,
    )?;
    ensure_retained(&identity)?;
    let snapshot = project_snapshot(generation, &identity.branches);
    state.next_generation = generation;
    state.generation = generation;
    state.branches = identity.branches.clone();
    state.identity = Some(Arc::new(identity));
    state.snapshot = Some(snapshot.clone());
    Ok(snapshot)
}

pub(crate) fn build_branch_identity(
    runner: &Runner,
    cancel: &CancellationToken,
) -> Result<BranchIdentity, GitWorkspaceError> {
    let top = runner.run(
        "rev-parse",
        &[OsString::from("--show-toplevel")],
        STDOUT_LIMIT,
        cancel,
    )?;
    if exact_single_line(&top.stdout)? != runner.root.as_os_str().as_bytes() {
        return Err(error(GitWorkspaceErrorCode::InvalidRoot));
    }
    reject_operation_markers(runner, cancel)?;
    let refs = runner.run(
        "for-each-ref",
        &[
            OsString::from("--sort=refname"),
            OsString::from("--format=%(objectname)%00%(refname)%00"),
            OsString::from("refs/heads/"),
        ],
        STDOUT_LIMIT,
        cancel,
    )?;
    let filter_identity = capture_branch_filter_identity(runner, cancel)?;
    let status = runner.run("status", &status_args(), STDOUT_LIMIT, cancel)?;
    let parsed = parse_status(&status.stdout)?;
    if !parsed.files.is_empty() {
        return Err(error(GitWorkspaceErrorCode::BranchDirty));
    }
    let (current_raw, current_oid_raw) = parse_clean_head(&status.stdout, &parsed.head)?;
    let mut branches = parse_refs(&refs.stdout, &current_raw, &current_oid_raw)?;
    if !branches.iter().any(|branch| branch.current) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    branches.sort_by(|left, right| left.short.as_bytes().cmp(right.short.as_bytes()));
    ensure_retained_parts(
        &filter_identity.paths,
        &filter_identity.attrs,
        &status.stdout,
        &refs.stdout,
        &branches,
    )?;
    Ok(BranchIdentity {
        filter_paths: filter_identity.paths,
        filter_attrs: filter_identity.attrs,
        status: status.stdout,
        refs: refs.stdout,
        branches,
    })
}

pub(crate) fn capture_branch_filter_identity(
    runner: &Runner,
    cancel: &CancellationToken,
) -> Result<FilterIdentity, GitWorkspaceError> {
    let paths = runner.run(
        "ls-files",
        &[
            OsString::from("-z"),
            OsString::from("--cached"),
            OsString::from("--deduplicate"),
        ],
        BRANCH_RETAINED_LIMIT,
        cancel,
    )?;
    let path_bytes: Arc<[u8]> = paths.stdout.into();
    let parsed_paths = parse_nul_paths(&path_bytes)?;
    let remaining = BRANCH_RETAINED_LIMIT
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
    validate_branch_attrs(&parsed_paths, &attrs.stdout)?;
    Ok(FilterIdentity {
        paths: path_bytes,
        attrs: attrs.stdout,
    })
}

pub(crate) fn parse_clean_head(
    bytes: &[u8],
    head: &WorkspaceHead,
) -> Result<(Vec<u8>, Vec<u8>), GitWorkspaceError> {
    match head {
        WorkspaceHead::Detached => return Err(error(GitWorkspaceErrorCode::BranchDetached)),
        WorkspaceHead::Unborn { .. } => return Err(error(GitWorkspaceErrorCode::BranchUnborn)),
        WorkspaceHead::Branch { .. } => {}
    }
    let mut branch = None;
    let mut oid = None;
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        if let Some(value) = record.strip_prefix(b"# branch.head ") {
            branch = Some(value.to_vec());
        } else if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            oid = Some(value.to_vec());
        }
    }
    let branch = branch
        .filter(|value| value != b"(detached)")
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    validate_branch_short(&branch)?;
    let oid = oid
        .filter(|value| valid_oid(value))
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    Ok((branch, oid))
}

pub(crate) fn parse_refs(
    bytes: &[u8],
    current: &[u8],
    current_oid: &[u8],
) -> Result<Vec<PrivateBranch>, GitWorkspaceError> {
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let mut branches = Vec::new();
    let mut seen_short = BTreeSet::new();
    let mut seen_full = BTreeSet::new();
    let records: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    for (index, record) in records.iter().enumerate() {
        if record.is_empty() {
            if index + 1 == records.len() {
                continue;
            }
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let fields: Vec<&[u8]> = record.split(|byte| *byte == 0).collect();
        if fields.len() != 3
            || !fields[2].is_empty()
            || !valid_oid_width(fields[0], current_oid.len())
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let short = fields[1]
            .strip_prefix(b"refs/heads/")
            .filter(|short| !short.is_empty())
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        validate_branch_short(short)?;
        if !seen_short.insert(short.to_vec())
            || !seen_full.insert(fields[1].to_vec())
            || branches.len() == BRANCH_LIMIT
        {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        let is_current = short == current;
        if is_current && fields[0] != current_oid {
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
        branches.push(PrivateBranch {
            id: BranchId {
                generation: 0,
                slot: 0,
                seal: 0,
            },
            short: OsString::from_vec(short.to_vec()),
            full: fields[1].to_vec(),
            oid: fields[0].to_vec(),
            current: is_current,
        });
    }
    if branches.is_empty() {
        return Err(error(GitWorkspaceErrorCode::BranchUnborn));
    }
    Ok(branches)
}

pub(crate) fn validate_branch_short(short: &[u8]) -> Result<(), GitWorkspaceError> {
    if short.is_empty()
        || short[0] == b'-'
        || short == b"HEAD"
        || short == b"@"
        || short.starts_with(b"/")
        || short.ends_with(b"/")
        || short.ends_with(b".")
        || short
            .windows(2)
            .any(|window| window == b".." || window == b"//")
        || short.windows(2).any(|window| window == b"@{")
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
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok(())
}

pub(crate) fn valid_oid_width(oid: &[u8], width: usize) -> bool {
    oid.len() == width
        && matches!(width, 40 | 64)
        && oid
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub(crate) fn valid_oid(oid: &[u8]) -> bool {
    matches!(oid.len(), 40 | 64)
        && oid
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub(crate) fn reject_operation_markers(
    runner: &Runner,
    cancel: &CancellationToken,
) -> Result<(), GitWorkspaceError> {
    let git_dir = canonical_git_dir(runner, "--absolute-git-dir", cancel)?;
    let common_dir = canonical_git_dir(runner, "--git-common-dir", cancel)?;
    for marker in OPERATION_MARKERS {
        let output = runner.run(
            "rev-parse",
            &[OsString::from("--git-path"), OsString::from(marker)],
            STDOUT_LIMIT,
            cancel,
        )?;
        let raw = exact_single_line(&output.stdout)?;
        if raw.is_empty() || raw.contains(&0) {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let path = PathBuf::from(OsString::from_vec(raw.to_vec()));
        let path = if path.is_absolute() {
            path
        } else {
            runner.root.join(path)
        };
        let parent = path
            .parent()
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let parent = fs::canonicalize(parent)
            .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
        if !parent.starts_with(&git_dir) && !parent.starts_with(&common_dir) {
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
        match fs::symlink_metadata(path) {
            Ok(_) => return Err(error(GitWorkspaceErrorCode::BranchOperationInProgress)),
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(error(GitWorkspaceErrorCode::ChangedDuringRead)),
        }
    }
    Ok(())
}

pub(crate) fn canonical_git_dir(
    runner: &Runner,
    flag: &str,
    cancel: &CancellationToken,
) -> Result<PathBuf, GitWorkspaceError> {
    let output = runner.run("rev-parse", &[OsString::from(flag)], STDOUT_LIMIT, cancel)?;
    let raw = exact_single_line(&output.stdout)?;
    if raw.is_empty() || raw.contains(&0) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let path = PathBuf::from(OsString::from_vec(raw.to_vec()));
    let path = if path.is_absolute() {
        path
    } else {
        runner.root.join(path)
    };
    fs::canonicalize(path).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))
}

pub(crate) fn validate_target_changes(
    runner: &Runner,
    current_oid: &[u8],
    target_oid: &[u8],
    preflight_retained: usize,
    permit_payload: usize,
    cancel: &CancellationToken,
) -> Result<SwitchAuthority, GitWorkspaceError> {
    let mut budget = RetainedBudget::new(BRANCH_RETAINED_LIMIT);
    budget.charge(preflight_retained)?;
    budget.charge(permit_payload)?;
    for fixed in [
        std::mem::size_of::<BranchSwitchPermit>(),
        std::mem::size_of::<SwitchAuthority>(),
    ] {
        budget.charge(fixed)?;
    }
    let output = runner.run(
        "diff",
        &[
            OsString::from("--name-status"),
            OsString::from("-z"),
            OsString::from("--diff-filter=ACMRT"),
            OsString::from("-M"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            OsString::from_vec(current_oid.to_vec()),
            OsString::from_vec(target_oid.to_vec()),
        ],
        budget.remaining(),
        cancel,
    )?;
    budget.charge(output.stdout.len())?;
    let parsed_paths = parse_target_paths(&output.stdout)?;
    let paths = parsed_paths.materialized;
    let mut authority_paths = parsed_paths.authority;
    charge_paths(&mut budget, &paths)?;
    charge_paths(&mut budget, &authority_paths)?;
    let deletes = runner.run(
        "diff",
        &[
            OsString::from("--name-status"),
            OsString::from("-z"),
            OsString::from("--diff-filter=D"),
            OsString::from("-M"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            OsString::from_vec(current_oid.to_vec()),
            OsString::from_vec(target_oid.to_vec()),
        ],
        budget.remaining(),
        cancel,
    )?;
    budget.charge(deletes.stdout.len())?;
    let deleted_paths = parse_delete_paths(&deletes.stdout)?;
    charge_paths(&mut budget, &deleted_paths)?;
    authority_paths.extend(deleted_paths);
    authority_paths.sort();
    authority_paths.dedup();
    if authority_paths.len() > PATH_LIMIT {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    if authority_paths.iter().any(|path| is_gitattributes(path)) {
        return Err(error(GitWorkspaceErrorCode::BranchUnsafeFilter));
    }
    let mut input = Vec::new();
    for path in &paths {
        input.extend_from_slice(path);
        input.push(0);
    }
    budget.charge(input.len())?;
    budget.charge(2 * std::mem::size_of::<usize>())?;
    let attrs = runner.run_with_input(
        "check-attr",
        &[
            {
                let mut source = b"--source=".to_vec();
                source.extend_from_slice(target_oid);
                OsString::from_vec(source)
            },
            OsString::from("-z"),
            OsString::from("--stdin"),
            OsString::from("--all"),
        ],
        Arc::from(input),
        budget.remaining(),
        cancel,
    )?;
    budget.charge(attrs.stdout.len())?;
    validate_branch_attrs(&paths, &attrs.stdout)?;
    Ok(SwitchAuthority {
        acmrt_raw: output.stdout,
        delete_raw: deletes.stdout,
        materialized_paths: paths,
        authority_paths,
        attrs: attrs.stdout,
    })
}

pub(crate) fn charge_paths(
    budget: &mut RetainedBudget,
    paths: &[Vec<u8>],
) -> Result<(), GitWorkspaceError> {
    budget.charge(
        paths
            .len()
            .checked_mul(std::mem::size_of::<Vec<u8>>())
            .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?,
    )?;
    for path in paths {
        budget.charge(path.len())?;
    }
    Ok(())
}

pub(crate) fn exact_single_line(bytes: &[u8]) -> Result<&[u8], GitWorkspaceError> {
    let line = bytes
        .strip_suffix(b"\n")
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    if line.is_empty() || line.contains(&0) || line.contains(&b'\n') || line.contains(&b'\r') {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok(line)
}

pub(crate) fn parse_target_paths(bytes: &[u8]) -> Result<ParsedTargetPaths, GitWorkspaceError> {
    if bytes.is_empty() {
        return Ok(ParsedTargetPaths {
            materialized: Vec::new(),
            authority: Vec::new(),
        });
    }
    if !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let mut fields = bytes[..bytes.len() - 1].split(|byte| *byte == 0);
    let mut paths = BTreeSet::new();
    let mut authority = BTreeSet::new();
    while let Some(status) = fields.next() {
        let kind = parse_acmrt_status(status)?;
        if !matches!(kind, b'A' | b'C' | b'M' | b'R' | b'T') {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let first = fields
            .next()
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let target = if matches!(kind, b'R' | b'C') {
            validate_relative_path(first)?;
            authority.insert(first.to_vec());
            fields
                .next()
                .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?
        } else {
            first
        };
        validate_relative_path(target)?;
        authority.insert(target.to_vec());
        if authority.len() > PATH_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        if !paths.insert(target.to_vec()) || paths.len() > PATH_LIMIT {
            return Err(error(if paths.len() > PATH_LIMIT {
                GitWorkspaceErrorCode::OutputTooLarge
            } else {
                GitWorkspaceErrorCode::MalformedOutput
            }));
        }
    }
    Ok(ParsedTargetPaths {
        materialized: paths.into_iter().collect(),
        authority: authority.into_iter().collect(),
    })
}

pub(crate) fn parse_acmrt_status(status: &[u8]) -> Result<u8, GitWorkspaceError> {
    match status {
        [kind @ (b'A' | b'M' | b'T')] => Ok(*kind),
        [kind @ (b'R' | b'C'), score @ ..]
            if !score.is_empty()
                && score.len() <= 3
                && score.iter().all(u8::is_ascii_digit)
                && std::str::from_utf8(score)
                    .ok()
                    .and_then(|score| score.parse::<u16>().ok())
                    .is_some_and(|score| score <= 100) =>
        {
            Ok(*kind)
        }
        _ => Err(error(GitWorkspaceErrorCode::MalformedOutput)),
    }
}

pub(crate) fn parse_delete_paths(bytes: &[u8]) -> Result<Vec<Vec<u8>>, GitWorkspaceError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let fields: Vec<&[u8]> = bytes[..bytes.len() - 1].split(|byte| *byte == 0).collect();
    let (pairs, remainder) = fields.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let mut paths = BTreeSet::new();
    for [status, path] in pairs {
        if *status != b"D" {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        validate_relative_path(path)?;
        if !paths.insert(path.to_vec()) || paths.len() > PATH_LIMIT {
            return Err(error(if paths.len() > PATH_LIMIT {
                GitWorkspaceErrorCode::OutputTooLarge
            } else {
                GitWorkspaceErrorCode::MalformedOutput
            }));
        }
    }
    Ok(paths.into_iter().collect())
}

pub(crate) fn is_gitattributes(path: &[u8]) -> bool {
    path.rsplit(|byte| *byte == b'/').next() == Some(b".gitattributes")
}

pub(crate) fn validate_branch_attrs(
    paths: &[Vec<u8>],
    bytes: &[u8],
) -> Result<(), GitWorkspaceError> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let fields: Vec<&[u8]> = bytes.split(|byte| *byte == 0).collect();
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
        if !allowed.contains(triple[0]) || triple[1].is_empty() {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        if triple[1] == b"filter" {
            return Err(error(GitWorkspaceErrorCode::BranchUnsafeFilter));
        }
        if triple[2].is_empty() || !seen.insert((triple[0].to_vec(), triple[1].to_vec())) {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
    }
    Ok(())
}

pub(crate) fn current_oid(identity: &BranchIdentity) -> Result<&[u8], GitWorkspaceError> {
    identity
        .branches
        .iter()
        .find(|branch| branch.current)
        .map(|branch| branch.oid.as_slice())
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))
}

pub(crate) fn verify_target_matches(
    identity: &BranchIdentity,
    expected: &PrivateBranch,
) -> Result<(), GitWorkspaceError> {
    identity
        .branches
        .iter()
        .find(|branch| branch.short == expected.short)
        .filter(|branch| branch.oid == expected.oid && !branch.current)
        .map(|_| ())
        .ok_or_else(|| error(GitWorkspaceErrorCode::ChangedDuringRead))
}

pub(crate) fn same_branch_identity(left: &BranchIdentity, right: &BranchIdentity) -> bool {
    left.filter_paths == right.filter_paths
        && left.filter_attrs == right.filter_attrs
        && left.status == right.status
        && left.refs == right.refs
        && left.branches.len() == right.branches.len()
        && left
            .branches
            .iter()
            .zip(&right.branches)
            .all(|(left, right)| {
                left.short == right.short
                    && left.full == right.full
                    && left.oid == right.oid
                    && left.current == right.current
            })
}

pub(crate) fn assign_branch_ids(
    branches: &mut [PrivateBranch],
    generation: u64,
    root_identity: RootIdentity,
    instance_nonce: u64,
) -> Result<(), GitWorkspaceError> {
    for (slot, branch) in branches.iter_mut().enumerate() {
        let slot = u32::try_from(slot).map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        branch.id = BranchId {
            generation,
            slot,
            seal: seal(
                root_identity,
                instance_nonce,
                generation,
                slot,
                &branch.full,
            ),
        };
    }
    Ok(())
}

pub(crate) fn project_snapshot(generation: u64, branches: &[PrivateBranch]) -> BranchSnapshot {
    BranchSnapshot {
        generation,
        branches: branches
            .iter()
            .map(|branch| BranchItem {
                id: branch.id,
                label: escape_ref(branch.short.as_bytes()),
                current: branch.current,
            })
            .collect(),
    }
}

pub(crate) fn ensure_retained(identity: &BranchIdentity) -> Result<(), GitWorkspaceError> {
    ensure_retained_parts(
        &identity.filter_paths,
        &identity.filter_attrs,
        &identity.status,
        &identity.refs,
        &identity.branches,
    )
}

pub(crate) fn branch_identity_retained(
    identity: &BranchIdentity,
) -> Result<usize, GitWorkspaceError> {
    retained_size(
        &identity.filter_paths,
        &identity.filter_attrs,
        &identity.status,
        &identity.refs,
        &identity.branches,
    )
}

pub(crate) fn ensure_retained_parts(
    filter_paths: &[u8],
    filter_attrs: &[u8],
    status: &[u8],
    refs: &[u8],
    branches: &[PrivateBranch],
) -> Result<(), GitWorkspaceError> {
    let retained = retained_size(filter_paths, filter_attrs, status, refs, branches)?;
    if retained > BRANCH_RETAINED_LIMIT {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    Ok(())
}

pub(crate) fn retained_size(
    filter_paths: &[u8],
    filter_attrs: &[u8],
    status: &[u8],
    refs: &[u8],
    branches: &[PrivateBranch],
) -> Result<usize, GitWorkspaceError> {
    let mut retained = std::mem::size_of::<BranchState>();
    for amount in [
        std::mem::size_of::<BranchIdentity>(),
        filter_paths.len(),
        filter_attrs.len(),
        status.len(),
        refs.len(),
        branches
            .len()
            .checked_mul(std::mem::size_of::<PrivateBranch>())
            .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?,
    ] {
        retained = retained
            .checked_add(amount)
            .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
    }
    for branch in branches {
        for amount in [
            branch.short.as_bytes().len(),
            branch.full.len(),
            branch.oid.len(),
            std::mem::size_of::<BranchItem>(),
            escape_ref(branch.short.as_bytes()).len(),
        ] {
            retained = retained
                .checked_add(amount)
                .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        }
    }
    Ok(retained)
}

pub(crate) fn invalidate_branch_state(state: &mut BranchState) {
    state.generation = 0;
    state.identity = None;
    state.snapshot = None;
    state.branches.clear();
    state.issued_permits.clear();
}
