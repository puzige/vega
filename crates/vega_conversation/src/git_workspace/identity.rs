use super::*;

pub(crate) fn hash_worktree_no_filters(
    runner: &Runner,
    path: &OsStr,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, GitWorkspaceError> {
    let output = runner.run(
        "hash-object",
        &[
            OsString::from("--no-filters"),
            OsString::from("--"),
            path.to_owned(),
        ],
        65,
        cancel,
    )?;
    let digest = output
        .stdout
        .strip_suffix(b"\n")
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    if !valid_oid(digest) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok(digest.to_vec())
}

const ARTIFACT_PROVENANCE_LIMIT: usize = 1024 * 1024;

pub(crate) fn build_artifact_evidence(
    runner: &Runner,
    file: &ArtifactWorkspaceFile,
    cancel: &CancellationToken,
) -> Result<ArtifactEvidence, GitWorkspaceError> {
    let _safe_prefix = read_artifact_file(runner, file, ARTIFACT_PROVENANCE_LIMIT, cancel)?;
    let expected = file
        .worktree_identity
        .filter(|identity| identity.kind == 0)
        .ok_or_else(|| error(GitWorkspaceErrorCode::MetadataOnly))?;
    let digest = hash_worktree_no_filters(runner, &file.path, cancel)?;
    verify_snapshot_identity(runner, &file.snapshot_identity, cancel)?;
    if read_worktree_identity(&runner.root, file.path.as_bytes())? != Some(expected) {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    let mtime_ns = i128::from(expected.mtime)
        .checked_mul(1_000_000_000)
        .and_then(|seconds| seconds.checked_add(i128::from(expected.mtime_ns)))
        .ok_or_else(|| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    Ok(ArtifactEvidence {
        dev: expected.dev,
        ino: expected.ino,
        size: expected.size,
        mtime_ns,
        digest,
    })
}

pub(crate) fn read_artifact_file(
    runner: &Runner,
    file: &ArtifactWorkspaceFile,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, GitWorkspaceError> {
    let (target, mut opened, expected) = fence_artifact_file(runner, file, cancel)?;
    if expected.size > limit as u64 {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(expected.size).unwrap_or(limit).min(limit));
    let mut chunk = [0_u8; IO_CHUNK];
    loop {
        if cancel.is_cancelled() {
            return Err(error(GitWorkspaceErrorCode::Cancelled));
        }
        let read = opened
            .read(&mut chunk)
            .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
        if read == 0 {
            break;
        }
        if bytes
            .len()
            .checked_add(read)
            .is_none_or(|length| length > limit)
        {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    let opened_after = opened
        .metadata()
        .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    let path_after = fs::symlink_metadata(&target)
        .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if file_identity(&opened_after) != expected || file_identity(&path_after) != expected {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    verify_snapshot_identity(runner, &file.snapshot_identity, cancel)?;
    Ok(bytes)
}

pub(crate) fn build_artifact_open_guard(
    runner: &Runner,
    file: &ArtifactWorkspaceFile,
    cancel: &CancellationToken,
) -> Result<ArtifactOpenGuard, GitWorkspaceError> {
    let (target, opened, expected) = fence_artifact_file(runner, file, cancel)?;
    let opened_after = opened
        .metadata()
        .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    let path_after = fs::symlink_metadata(&target)
        .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if file_identity(&opened_after) != expected || file_identity(&path_after) != expected {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    verify_snapshot_identity(runner, &file.snapshot_identity, cancel)?;
    let parent = target
        .parent()
        .ok_or_else(|| error(GitWorkspaceErrorCode::ChangedDuringRead))?
        .to_path_buf();
    let root_fd =
        File::open(&runner.root).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    let parent_fd =
        File::open(&parent).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    let root_identity = file_identity(
        &root_fd
            .metadata()
            .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?,
    );
    let parent_identity = file_identity(
        &parent_fd
            .metadata()
            .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?,
    );
    Ok(ArtifactOpenGuard {
        root: runner.root.clone(),
        parent,
        target,
        root_fd,
        parent_fd,
        target_fd: opened,
        root_identity,
        parent_identity,
        target_identity: expected,
    })
}

pub(crate) fn fence_artifact_file(
    runner: &Runner,
    file: &ArtifactWorkspaceFile,
    cancel: &CancellationToken,
) -> Result<(PathBuf, File, FileIdentity), GitWorkspaceError> {
    runner.verify_root()?;
    verify_snapshot_identity(runner, &file.snapshot_identity, cancel)?;
    if file
        .path
        .as_bytes()
        .split(|byte| *byte == b'/')
        .any(|component| component == b".git")
    {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let lexical = runner.root.join(&file.path);
    let target =
        fs::canonicalize(&lexical).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if target != lexical || target == runner.root || !target.starts_with(&runner.root) {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    let git_dir_output = runner.run(
        "rev-parse",
        &[OsString::from("--absolute-git-dir")],
        PATH_LIMIT,
        cancel,
    )?;
    let git_dir_bytes = trim_one_newline(&git_dir_output.stdout);
    let git_dir_path = PathBuf::from(OsString::from_vec(git_dir_bytes.to_vec()));
    if !git_dir_path.is_absolute() {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let git_dir = fs::canonicalize(git_dir_path)
        .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if target == git_dir || target.starts_with(&git_dir) {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let metadata = fs::symlink_metadata(&target)
        .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let expected = file
        .worktree_identity
        .ok_or_else(|| error(GitWorkspaceErrorCode::MetadataOnly))?;
    if file_identity(&metadata) != expected {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    let opened =
        File::open(&target).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    let opened_metadata = opened
        .metadata()
        .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if file_identity(&opened_metadata) != expected {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    Ok((target, opened, expected))
}

pub(crate) fn project_untracked(
    runner: &Runner,
    file: &PrivateFile,
    cancel: &CancellationToken,
    remaining_bytes: &mut usize,
    remaining_rows: &mut usize,
) -> Result<DiffSection, GitWorkspaceError> {
    if cancel.is_cancelled() {
        return Err(error(GitWorkspaceErrorCode::Cancelled));
    }
    let path = runner.root.join(&file.path);
    let canonical =
        fs::canonicalize(&path).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if canonical != path || !canonical.starts_with(&runner.root) {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let before =
        fs::symlink_metadata(&path).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if !before.file_type().is_file() || before.nlink() > 1 {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    if before.size() > *remaining_bytes as u64 {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    let mut reader = File::open(&path).map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?;
    let opened = reader
        .metadata()
        .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if file_identity(&opened) != file_identity(&before) {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    let mut bytes = Vec::with_capacity((before.size() as usize).min(*remaining_bytes));
    let mut chunk = [0_u8; IO_CHUNK];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > *remaining_bytes {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        bytes.extend_from_slice(&chunk[..read]);
        if cancel.is_cancelled() {
            return Err(error(GitWorkspaceErrorCode::Cancelled));
        }
    }
    let after =
        fs::symlink_metadata(&path).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    let opened_after = reader
        .metadata()
        .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    let after_canonical =
        fs::canonicalize(&path).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if after_canonical != canonical
        || file_identity(&before) != file_identity(&after)
        || file_identity(&opened) != file_identity(&opened_after)
        || file_identity(&after) != file_identity(&opened_after)
    {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?;
    if text.as_bytes().contains(&0) {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let mut rows = Vec::new();
    for (index, line) in logical_lines(text).enumerate() {
        if line.len() > PATCH_LINE_LIMIT || rows.len() == *remaining_rows {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        rows.push(DiffRow {
            kind: DiffRowKind::Addition,
            old_line: None,
            new_line: Some(
                u32::try_from(index + 1)
                    .map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?,
            ),
            text: line.to_owned(),
        });
    }
    consume_projection_bytes(remaining_bytes, bytes.len())?;
    *remaining_rows = remaining_rows
        .checked_sub(rows.len())
        .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
    Ok(DiffSection {
        layer: DiffLayer::Untracked,
        hunks: vec![DiffHunk {
            old_start: 0,
            old_count: 0,
            new_start: if rows.is_empty() { 0 } else { 1 },
            new_count: u32::try_from(rows.len())
                .map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?,
            heading_suffix: None,
            missing_trailing_newline: !bytes.ends_with(b"\n") && !bytes.is_empty(),
            rows,
        }],
    })
}
