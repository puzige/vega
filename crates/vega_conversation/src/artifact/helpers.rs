use super::*;

pub(crate) fn verified_terminal(
    project_id: &str,
    thread_id: &str,
    call: &ToolCall,
    result: &ToolResult,
) -> Result<Option<TerminalFingerprint>, GitWorkspaceError> {
    if !matches!(call.tool.as_str(), "write" | "edit") {
        return Ok(None);
    }
    if result.status != ToolCallStatus::Success || result.reused {
        return Ok(None);
    }
    if result.exit_code.is_some()
        || result.duration_ms.is_some()
        || result.truncated != Some(false)
        || result.invalid.is_some()
    {
        return Err(workspace_error(GitWorkspaceErrorCode::ArtifactConflict));
    }
    let ids = vega_tools::CheckpointIds::new(project_id, thread_id, &call.id)
        .map_err(|_| workspace_error(GitWorkspaceErrorCode::ArtifactConflict))?;
    let expected_checkpoint = ids.checkpoint_ref();
    let audit = vega_tools::WriteEditAudit::from_json(&call.input_json)
        .map_err(|_| workspace_error(GitWorkspaceErrorCode::ArtifactConflict))?;
    if audit.tool().as_str() != call.tool {
        return Err(workspace_error(GitWorkspaceErrorCode::ArtifactConflict));
    }
    match audit {
        vega_tools::WriteEditAudit::Write {
            path,
            content_bytes,
            fingerprint_v1,
        } => {
            let success = vega_tools::WriteSuccessOutput::from_json(&result.output)
                .map_err(|_| workspace_error(GitWorkspaceErrorCode::ArtifactConflict))?;
            if success.path != path
                || success.bytes_written != content_bytes
                || success.checkpoint_ref != expected_checkpoint
            {
                return Err(workspace_error(GitWorkspaceErrorCode::ArtifactConflict));
            }
            Ok(Some(TerminalFingerprint::Write {
                path,
                input_fingerprint: fingerprint_v1,
                bytes_written: success.bytes_written,
            }))
        }
        vega_tools::WriteEditAudit::Edit {
            path,
            fingerprint_v1,
            ..
        } => {
            let success = vega_tools::EditSuccessOutput::from_json(&result.output)
                .map_err(|_| workspace_error(GitWorkspaceErrorCode::ArtifactConflict))?;
            if success.path != path
                || success.replacements != 1
                || success.checkpoint_ref != expected_checkpoint
            {
                return Err(workspace_error(GitWorkspaceErrorCode::ArtifactConflict));
            }
            Ok(Some(TerminalFingerprint::Edit {
                path,
                input_fingerprint: fingerprint_v1,
                bytes_written: success.bytes_written,
                replacements: success.replacements,
            }))
        }
    }
}

pub(crate) fn record(
    state: &ArtifactState,
    route_epoch: u64,
    card_id: ArtifactCardId,
) -> Result<&ArtifactRecord, GitWorkspaceError> {
    if card_id.route_epoch != route_epoch {
        return Err(workspace_error(GitWorkspaceErrorCode::StaleGeneration));
    }
    let slot = usize::try_from(card_id.slot)
        .map_err(|_| workspace_error(GitWorkspaceErrorCode::UnknownFile))?;
    state
        .cards
        .get(slot)
        .filter(|record| record.id == card_id)
        .ok_or_else(|| workspace_error(GitWorkspaceErrorCode::UnknownFile))
}

pub(crate) fn record_mut(
    state: &mut ArtifactState,
    route_epoch: u64,
    card_id: ArtifactCardId,
) -> Result<&mut ArtifactRecord, GitWorkspaceError> {
    if card_id.route_epoch != route_epoch {
        return Err(workspace_error(GitWorkspaceErrorCode::StaleGeneration));
    }
    let slot = usize::try_from(card_id.slot)
        .map_err(|_| workspace_error(GitWorkspaceErrorCode::UnknownFile))?;
    state
        .cards
        .get_mut(slot)
        .filter(|record| record.id == card_id)
        .ok_or_else(|| workspace_error(GitWorkspaceErrorCode::UnknownFile))
}

pub(crate) fn card_seal(instance_nonce: u64, route_epoch: u64, slot: u32) -> u64 {
    let mut value = instance_nonce ^ route_epoch.rotate_left(19) ^ u64::from(slot);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

pub(crate) fn text_preview_path_allowed(path: &OsStr) -> bool {
    let name = path
        .as_bytes()
        .rsplit(|byte| *byte == b'/')
        .next()
        .unwrap_or_default();
    const BASENAMES: &[&[u8]] = &[
        b"README",
        b"LICENSE",
        b"NOTICE",
        b"CHANGELOG",
        b"Makefile",
        b"Dockerfile",
        b".gitignore",
        b".gitattributes",
        b".editorconfig",
    ];
    if BASENAMES.contains(&name) {
        return true;
    }
    let Some(dot) = name.iter().rposition(|byte| *byte == b'.') else {
        return false;
    };
    let extension = &name[dot + 1..];
    const EXTENSIONS: &[&[u8]] = &[
        b"txt",
        b"md",
        b"markdown",
        b"rst",
        b"adoc",
        b"csv",
        b"tsv",
        b"json",
        b"jsonl",
        b"yaml",
        b"yml",
        b"toml",
        b"xml",
        b"html",
        b"htm",
        b"css",
        b"scss",
        b"sass",
        b"less",
        b"js",
        b"jsx",
        b"mjs",
        b"cjs",
        b"ts",
        b"tsx",
        b"rs",
        b"py",
        b"rb",
        b"go",
        b"java",
        b"kt",
        b"kts",
        b"swift",
        b"c",
        b"h",
        b"cc",
        b"cpp",
        b"cxx",
        b"hpp",
        b"hxx",
        b"m",
        b"mm",
        b"sh",
        b"bash",
        b"zsh",
        b"fish",
        b"sql",
        b"graphql",
        b"gql",
        b"proto",
        b"diff",
        b"patch",
        b"log",
    ];
    EXTENSIONS
        .iter()
        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
}

pub(crate) fn validate_preview_lines(text: &str) -> Result<(), GitWorkspaceError> {
    let mut lines = 0_usize;
    for line in text.split_inclusive('\n') {
        lines = lines
            .checked_add(1)
            .ok_or_else(|| workspace_error(GitWorkspaceErrorCode::OutputTooLarge))?;
        if lines > PREVIEW_LINES {
            return Err(workspace_error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        let content = line.strip_suffix('\n').unwrap_or(line);
        if content.len() > PREVIEW_LINE_BYTES {
            return Err(workspace_error(GitWorkspaceErrorCode::OutputTooLarge));
        }
    }
    Ok(())
}

pub(crate) fn launch_open(
    launcher: &Path,
    guard: &ArtifactOpenGuard,
    target: OpenInTarget,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<(), GitWorkspaceError> {
    if cancel.is_cancelled() {
        return Err(workspace_error(GitWorkspaceErrorCode::Cancelled));
    }
    let mut command = Command::new(launcher);
    command.args(open_arguments(guard.root(), guard.target(), target));
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| workspace_error(GitWorkspaceErrorCode::SpawnFailed))?;
    let pgid = child.id();
    if let Err(failure) = guard.revalidate() {
        terminate_group(&mut child, pgid)?;
        return Err(failure);
    }
    let started = Instant::now();
    loop {
        if cancel.is_cancelled() {
            terminate_group(&mut child, pgid)?;
            return Err(workspace_error(GitWorkspaceErrorCode::Cancelled));
        }
        if started.elapsed() >= timeout {
            terminate_group(&mut child, pgid)?;
            return Err(workspace_error(GitWorkspaceErrorCode::TimedOut));
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(workspace_error(GitWorkspaceErrorCode::GitFailed)),
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                terminate_group(&mut child, pgid)?;
                return Err(workspace_error(GitWorkspaceErrorCode::GitFailed));
            }
        }
    }
}

pub(crate) fn open_arguments(
    root: &Path,
    target_path: &Path,
    target: OpenInTarget,
) -> Vec<OsString> {
    match target {
        OpenInTarget::VisualStudioCode => vec![
            OsString::from("-a"),
            OsString::from("Visual Studio Code"),
            target_path.as_os_str().to_owned(),
        ],
        OpenInTarget::Cursor => vec![
            OsString::from("-a"),
            OsString::from("Cursor"),
            target_path.as_os_str().to_owned(),
        ],
        OpenInTarget::Zed => vec![
            OsString::from("-a"),
            OsString::from("Zed"),
            target_path.as_os_str().to_owned(),
        ],
        OpenInTarget::Terminal => vec![
            OsString::from("-a"),
            OsString::from("Terminal"),
            root.as_os_str().to_owned(),
        ],
        OpenInTarget::DefaultApplication => vec![target_path.as_os_str().to_owned()],
        OpenInTarget::RevealInFinder => {
            vec![OsString::from("-R"), target_path.as_os_str().to_owned()]
        }
    }
}

pub(crate) fn escape_label(bytes: &[u8]) -> String {
    let mut escaped = String::new();
    for byte in bytes {
        match byte {
            b'\\' => escaped.push_str("\\\\"),
            0x20..=0x7e => escaped.push(char::from(*byte)),
            _ => escaped.push_str(&format!("\\x{byte:02x}")),
        }
    }
    escaped
}

pub(crate) fn workspace_error(code: GitWorkspaceErrorCode) -> GitWorkspaceError {
    GitWorkspaceError::new(code)
}
