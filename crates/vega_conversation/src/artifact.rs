//! Route-owned artifact provenance, bounded preview, and fixed Open in service.
//!
//! Raw paths, call identifiers, provenance digests, and launcher argv remain
//! private. Public callers receive only safe metadata and bounded projections.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::git_workspace::{
    ArtifactEvidence, ArtifactOpenGuard, GitWorkspaceService, terminate_group,
};
use crate::types::{
    ArtifactCard, ArtifactCardId, ArtifactPreviewProjection, ArtifactSource, GitWorkspaceError,
    GitWorkspaceErrorCode, OpenInOutcome, OpenInTarget, ToolCall, ToolCallStatus, ToolResult,
    WorkspaceFileId,
};

const OPEN: &str = "/usr/bin/open";
const PREVIEW_BYTES: usize = 1024 * 1024;
const PREVIEW_LINES: usize = 10_000;
const PREVIEW_LINE_BYTES: usize = 64 * 1024;
const ROUTE_CARD_LIMIT: usize = 10_000;
const PROPOSAL_RETAINED_BYTES: usize = 64 * 1024;
const CALL_ID_BYTES: usize = 120;
const LOGICAL_PATH_BYTES: usize = 4096;
const TERMINAL_SUCCESS_BYTES: usize = 64 * 1024;
const CAPTURE_CANDIDATE_RETAINED_BYTES: usize = 8192;
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
static ARTIFACT_SERVICE_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, PartialEq, Eq)]
enum TerminalFingerprint {
    Write {
        path: String,
        input_fingerprint: String,
        bytes_written: u64,
    },
    Edit {
        path: String,
        input_fingerprint: String,
        bytes_written: u64,
        replacements: u64,
    },
}

/// Bounded, content-free capability produced by strict terminal validation.
///
/// The raw `ToolResult.output` is consumed during construction and is never
/// retained in this value. Its fields remain private so callers cannot forge a
/// capture after the trusted proposal/terminal pairing boundary.
#[derive(Clone)]
pub struct ArtifactCaptureCandidate {
    call_id: String,
    fingerprint: TerminalFingerprint,
}

impl std::fmt::Debug for ArtifactCaptureCandidate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArtifactCaptureCandidate")
            .field("metadata", &"[redacted]")
            .finish()
    }
}

impl TerminalFingerprint {
    fn path(&self) -> &str {
        match self {
            Self::Write { path, .. } | Self::Edit { path, .. } => path,
        }
    }

    fn bytes_written(&self) -> u64 {
        match self {
            Self::Write { bytes_written, .. } | Self::Edit { bytes_written, .. } => *bytes_written,
        }
    }
}

fn validate_candidate_retained(
    candidate: &ArtifactCaptureCandidate,
) -> Result<(), GitWorkspaceError> {
    let retained = std::mem::size_of::<ArtifactCaptureCandidate>()
        .checked_add(candidate.call_id.len())
        .and_then(|bytes| bytes.checked_add(candidate.fingerprint.path().len()))
        .and_then(|bytes| match &candidate.fingerprint {
            TerminalFingerprint::Write {
                input_fingerprint, ..
            }
            | TerminalFingerprint::Edit {
                input_fingerprint, ..
            } => bytes.checked_add(input_fingerprint.len()),
        })
        .ok_or_else(|| workspace_error(GitWorkspaceErrorCode::ArtifactLimit))?;
    if retained > CAPTURE_CANDIDATE_RETAINED_BYTES {
        return Err(workspace_error(GitWorkspaceErrorCode::ArtifactLimit));
    }
    Ok(())
}

struct ArtifactRecord {
    id: ArtifactCardId,
    fingerprint: TerminalFingerprint,
    path: OsString,
    label: String,
    source: ArtifactSource,
    evidence: Option<ArtifactEvidence>,
    current_file_id: Option<WorkspaceFileId>,
    stale_disabled: bool,
}

impl ArtifactRecord {
    fn projection(&self) -> ArtifactCard {
        ArtifactCard {
            id: self.id,
            label: self.label.clone(),
            source: self.source,
            current_file_id: self.current_file_id,
            preview_available: !self.stale_disabled
                && self.current_file_id.is_some()
                && text_preview_path_allowed(&self.path),
        }
    }
}

#[derive(Default)]
struct ArtifactState {
    by_call_id: HashMap<String, usize>,
    cards: Vec<ArtifactRecord>,
}

/// Headless route-owned artifact and fixed external handoff service.
pub struct ArtifactService {
    workspace: Arc<GitWorkspaceService>,
    project_id: String,
    thread_id: String,
    route_epoch: u64,
    instance_nonce: u64,
    state: Mutex<ArtifactState>,
    launcher: PathBuf,
    open_timeout: Duration,
    launch_attempts: Arc<AtomicU64>,
}

impl std::fmt::Debug for ArtifactService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cards = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .cards
            .len();
        formatter
            .debug_struct("ArtifactService")
            .field("route_epoch", &self.route_epoch)
            .field("cards", &cards)
            .field("workspace", &"[redacted]")
            .finish()
    }
}

impl ArtifactService {
    /// Creates ephemeral artifact state owned by one route epoch.
    pub fn new(
        workspace: Arc<GitWorkspaceService>,
        project_id: String,
        thread_id: String,
        route_epoch: u64,
    ) -> Result<Self, GitWorkspaceError> {
        vega_tools::CheckpointIds::new(&project_id, &thread_id, "route-check")
            .map_err(|_| workspace_error(GitWorkspaceErrorCode::ArtifactConflict))?;
        let instance_nonce = ARTIFACT_SERVICE_NONCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| workspace_error(GitWorkspaceErrorCode::ArtifactLimit))?;
        Ok(Self {
            workspace,
            project_id,
            thread_id,
            route_epoch,
            instance_nonce,
            state: Mutex::new(ArtifactState::default()),
            launcher: PathBuf::from(OPEN),
            open_timeout: OPEN_TIMEOUT,
            launch_attempts: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Enforces the pre-clone retained proposal cap used by the app ingress.
    pub fn validate_proposal(call: &ToolCall) -> Result<(), GitWorkspaceError> {
        if call.id.len() > CALL_ID_BYTES {
            return Err(workspace_error(GitWorkspaceErrorCode::ArtifactLimit));
        }
        let retained = call
            .id
            .len()
            .checked_add(call.tool.len())
            .and_then(|bytes| bytes.checked_add(call.input_json.len()))
            .ok_or_else(|| workspace_error(GitWorkspaceErrorCode::ArtifactLimit))?;
        if retained > PROPOSAL_RETAINED_BYTES {
            return Err(workspace_error(GitWorkspaceErrorCode::ArtifactLimit));
        }
        Ok(())
    }

    /// Consumes a paired terminal immediately and returns only bounded,
    /// content-free capture metadata. Raw terminal output is never retained.
    pub fn prepare_capture(
        &self,
        call: &ToolCall,
        result: &ToolResult,
    ) -> Result<Option<ArtifactCaptureCandidate>, GitWorkspaceError> {
        Self::validate_proposal(call)?;
        if result.status == ToolCallStatus::Success
            && !result.reused
            && result.output.len() > TERMINAL_SUCCESS_BYTES
        {
            return Err(workspace_error(GitWorkspaceErrorCode::ArtifactLimit));
        }
        let call_id = call.id.as_str();
        let Some(fingerprint) = verified_terminal(&self.project_id, &self.thread_id, call, result)?
        else {
            if self.existing_call_id(call_id) {
                return Err(workspace_error(GitWorkspaceErrorCode::ArtifactConflict));
            }
            return Ok(None);
        };
        if fingerprint.path().len() > LOGICAL_PATH_BYTES {
            return Err(workspace_error(GitWorkspaceErrorCode::ArtifactLimit));
        }
        let candidate = ArtifactCaptureCandidate {
            call_id: call_id.to_owned(),
            fingerprint,
        };
        validate_candidate_retained(&candidate)?;
        let _ = self.existing_call(call_id, &candidate.fingerprint)?;
        Ok(Some(candidate))
    }

    /// Records one already-validated mutation capability after workspace
    /// refresh. Identical duplicates remain idempotent.
    pub async fn capture_candidate(
        &self,
        candidate: ArtifactCaptureCandidate,
        cancel: CancellationToken,
    ) -> Result<Option<ArtifactCard>, GitWorkspaceError> {
        let ArtifactCaptureCandidate {
            call_id,
            fingerprint,
        } = candidate;
        if let Some(existing) = self.existing_call(&call_id, &fingerprint)? {
            return Ok(Some(existing));
        }

        let path = OsString::from(fingerprint.path());
        let mut label = escape_label(path.as_os_str().as_bytes());
        let mut current_file_id = None;
        let mut evidence = None;
        if let Some(file) = self.workspace.artifact_file_for_path(&path) {
            label = escape_label(file.path.as_bytes());
            if file.is_regular_current() {
                current_file_id = Some(file.id);
                match self.workspace.artifact_evidence(file, cancel).await {
                    Ok(provenance) if provenance.size == fingerprint.bytes_written() => {
                        evidence = Some(provenance);
                    }
                    Ok(_) => {}
                    Err(failure) if failure.code() == GitWorkspaceErrorCode::Cancelled => {
                        return Err(failure);
                    }
                    Err(failure) if failure.code() == GitWorkspaceErrorCode::OutputTooLarge => {}
                    Err(_) => current_file_id = None,
                }
            }
        }
        let source = if evidence.is_some() {
            ArtifactSource::AgentArtifact
        } else {
            ArtifactSource::WorkspaceChange
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(slot) = state.by_call_id.get(&call_id).copied() {
            let existing = state
                .cards
                .get(slot)
                .ok_or_else(|| workspace_error(GitWorkspaceErrorCode::ArtifactConflict))?;
            if existing.fingerprint == fingerprint {
                return Ok(Some(existing.projection()));
            }
            return Err(workspace_error(GitWorkspaceErrorCode::ArtifactConflict));
        }
        if state.cards.len() >= ROUTE_CARD_LIMIT {
            return Err(workspace_error(GitWorkspaceErrorCode::ArtifactLimit));
        }
        let slot = state.cards.len();
        let slot_u32 = u32::try_from(slot)
            .map_err(|_| workspace_error(GitWorkspaceErrorCode::ArtifactLimit))?;
        let id = ArtifactCardId {
            route_epoch: self.route_epoch,
            slot: slot_u32,
            seal: card_seal(self.instance_nonce, self.route_epoch, slot_u32),
        };
        let card = ArtifactRecord {
            id,
            fingerprint,
            path,
            label,
            source,
            evidence,
            current_file_id,
            stale_disabled: current_file_id.is_none(),
        };
        let projection = card.projection();
        state.by_call_id.insert(call_id, slot);
        state.cards.push(card);
        Ok(Some(projection))
    }

    /// Convenience API for headless callers that do not need to queue the
    /// bounded candidate separately.
    pub async fn capture(
        &self,
        call: &ToolCall,
        result: &ToolResult,
        cancel: CancellationToken,
    ) -> Result<Option<ArtifactCard>, GitWorkspaceError> {
        let Some(candidate) = self.prepare_capture(call, result)? else {
            return Ok(None);
        };
        self.capture_candidate(candidate, cancel).await
    }

    /// Reconciles every card against the latest workspace snapshot. Agent
    /// provenance is monotonic: any uncertainty permanently downgrades it.
    pub async fn reconcile(
        &self,
        cancel: CancellationToken,
    ) -> Result<Vec<ArtifactCard>, GitWorkspaceError> {
        let candidates = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state
                .cards
                .iter()
                .map(|record| {
                    (
                        record.id,
                        record.path.clone(),
                        record.source,
                        record.evidence.clone(),
                        record.stale_disabled,
                    )
                })
                .collect::<Vec<_>>()
        };
        for (card_id, prior_path, source, prior_evidence, stale_disabled) in candidates {
            if cancel.is_cancelled() {
                return Err(workspace_error(GitWorkspaceErrorCode::Cancelled));
            }
            if stale_disabled {
                continue;
            }
            let mut matches = self.workspace.artifact_path_matches(&prior_path);
            if matches.len() != 1 {
                self.apply_reconcile(card_id, None, None, None, true)?;
                continue;
            }
            let matched = matches
                .pop()
                .ok_or_else(|| workspace_error(GitWorkspaceErrorCode::ChangedDuringRead))?;
            let renamed = matched.previous_path_match;
            let file = matched.file;
            let next_path = file.path.clone();
            let next_label = escape_label(next_path.as_bytes());
            if !file.is_regular_current() {
                self.apply_reconcile(card_id, Some(next_path), Some(next_label), None, true)?;
                continue;
            }
            let file_id = file.id;
            let (downgrade, evidence_valid) = if source == ArtifactSource::AgentArtifact {
                match self.workspace.artifact_evidence(file, cancel.clone()).await {
                    Ok(current) => {
                        let changed = prior_evidence.as_ref() != Some(&current);
                        (changed, !renamed || !changed)
                    }
                    Err(failure) if failure.code() == GitWorkspaceErrorCode::Cancelled => {
                        return Err(failure);
                    }
                    Err(_) => (true, false),
                }
            } else {
                (false, !renamed)
            };
            self.apply_reconcile(
                card_id,
                Some(next_path),
                Some(next_label),
                evidence_valid.then_some(file_id),
                downgrade,
            )?;
        }
        Ok(self.cards())
    }

    /// Returns safe card projections in route insertion order.
    pub fn cards(&self) -> Vec<ArtifactCard> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .cards
            .iter()
            .map(ArtifactRecord::projection)
            .collect()
    }

    /// Builds a bounded text preview only for the frozen safe path allowlist.
    pub async fn preview(
        &self,
        card_id: ArtifactCardId,
        cancel: CancellationToken,
    ) -> Result<ArtifactPreviewProjection, GitWorkspaceError> {
        let file_id = self.current_file_id(card_id)?;
        let file = self.workspace.artifact_file_by_id(file_id)?;
        if !text_preview_path_allowed(&file.path) {
            return Err(workspace_error(GitWorkspaceErrorCode::MetadataOnly));
        }
        let bytes = self
            .workspace
            .artifact_preview_bytes(file, PREVIEW_BYTES, cancel)
            .await?;
        if bytes.contains(&0) {
            return Err(workspace_error(GitWorkspaceErrorCode::MetadataOnly));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| workspace_error(GitWorkspaceErrorCode::MetadataOnly))?;
        validate_preview_lines(&text)?;
        Ok(ArtifactPreviewProjection {
            card_id,
            file_id,
            text,
        })
    }

    /// Performs exactly one fixed `/usr/bin/open` attempt after full preflight.
    pub async fn open_in(
        &self,
        card_id: ArtifactCardId,
        target: OpenInTarget,
        cancel: CancellationToken,
    ) -> Result<OpenInOutcome, GitWorkspaceError> {
        let file_id = self.current_file_id(card_id)?;
        let file = self.workspace.artifact_file_by_id(file_id)?;
        self.ensure_card_file_id(card_id, file_id)?;
        let launcher = self.launcher.clone();
        let timeout = self.open_timeout;
        let attempts = self.launch_attempts.clone();
        self.workspace
            .artifact_open_with(file, cancel, move |guard, cancel| {
                if cancel.is_cancelled() {
                    return Err(workspace_error(GitWorkspaceErrorCode::Cancelled));
                }
                attempts.fetch_add(1, Ordering::SeqCst);
                launch_open(&launcher, guard, target, timeout, cancel)
            })
            .await?;
        Ok(OpenInOutcome { card_id, target })
    }

    fn existing_call(
        &self,
        call_id: &str,
        fingerprint: &TerminalFingerprint,
    ) -> Result<Option<ArtifactCard>, GitWorkspaceError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(slot) = state.by_call_id.get(call_id).copied() else {
            return Ok(None);
        };
        let record = state
            .cards
            .get(slot)
            .ok_or_else(|| workspace_error(GitWorkspaceErrorCode::ArtifactConflict))?;
        if &record.fingerprint != fingerprint {
            return Err(workspace_error(GitWorkspaceErrorCode::ArtifactConflict));
        }
        Ok(Some(record.projection()))
    }

    fn existing_call_id(&self, call_id: &str) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.by_call_id.contains_key(call_id)
    }

    fn apply_reconcile(
        &self,
        card_id: ArtifactCardId,
        path: Option<OsString>,
        label: Option<String>,
        current_file_id: Option<WorkspaceFileId>,
        downgrade: bool,
    ) -> Result<(), GitWorkspaceError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let record = record_mut(&mut state, self.route_epoch, card_id)?;
        if let Some(path) = path {
            record.path = path;
        }
        if let Some(label) = label {
            record.label = label;
        }
        record.current_file_id = current_file_id;
        record.stale_disabled |= current_file_id.is_none();
        if downgrade {
            record.source = ArtifactSource::WorkspaceChange;
            record.evidence = None;
        }
        Ok(())
    }

    fn current_file_id(
        &self,
        card_id: ArtifactCardId,
    ) -> Result<WorkspaceFileId, GitWorkspaceError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let record = record(&state, self.route_epoch, card_id)?;
        record
            .current_file_id
            .ok_or_else(|| workspace_error(GitWorkspaceErrorCode::StaleGeneration))
    }

    fn ensure_card_file_id(
        &self,
        card_id: ArtifactCardId,
        file_id: WorkspaceFileId,
    ) -> Result<(), GitWorkspaceError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let record = record(&state, self.route_epoch, card_id)?;
        if record.current_file_id != Some(file_id) || record.stale_disabled {
            return Err(workspace_error(GitWorkspaceErrorCode::StaleGeneration));
        }
        Ok(())
    }

    #[cfg(test)]
    fn new_for_test(
        workspace: Arc<GitWorkspaceService>,
        project_id: String,
        thread_id: String,
        route_epoch: u64,
        launcher: PathBuf,
        timeout: Duration,
    ) -> Result<Self, GitWorkspaceError> {
        let mut service = Self::new(workspace, project_id, thread_id, route_epoch)?;
        service.launcher = launcher;
        service.open_timeout = timeout;
        Ok(service)
    }

    #[cfg(test)]
    fn launch_attempts(&self) -> u64 {
        self.launch_attempts.load(Ordering::SeqCst)
    }
}

fn verified_terminal(
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

fn record(
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

fn record_mut(
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

fn card_seal(instance_nonce: u64, route_epoch: u64, slot: u32) -> u64 {
    let mut value = instance_nonce ^ route_epoch.rotate_left(19) ^ u64::from(slot);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn text_preview_path_allowed(path: &OsStr) -> bool {
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

fn validate_preview_lines(text: &str) -> Result<(), GitWorkspaceError> {
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

fn launch_open(
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

fn open_arguments(root: &Path, target_path: &Path, target: OpenInTarget) -> Vec<OsString> {
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

fn escape_label(bytes: &[u8]) -> String {
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

fn workspace_error(code: GitWorkspaceErrorCode) -> GitWorkspaceError {
    GitWorkspaceError::new(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    const PROJECT_ID: &str = "project";
    const THREAD_ID: &str = "thread";

    struct Repo {
        dir: TempDir,
    }

    impl Repo {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            git(dir.path(), &["init", "-q"]);
            git(dir.path(), &["config", "user.name", "Vega Test"]);
            git(
                dir.path(),
                &["config", "user.email", "vega@example.invalid"],
            );
            Self { dir }
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        fn write(&self, path: &str, body: &[u8]) {
            let target = self.path().join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(target, body).unwrap();
        }

        fn commit_all(&self) {
            git(self.path(), &["add", "-A"]);
            git(self.path(), &["commit", "-q", "-m", "fixture"]);
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let mut command = Command::new("/usr/bin/git");
        command
            .current_dir(root)
            .args(args)
            .env("GIT_DIR", root.join(".poison-git-dir"))
            .env("GIT_WORK_TREE", root.join(".poison-work-tree"))
            .env("GIT_INDEX_FILE", root.join(".poison-index"));
        scrub_all_git_environment(&mut command);
        let status = command.status().unwrap();
        assert!(status.success(), "git {args:?}");
        for poison in [".poison-git-dir", ".poison-work-tree", ".poison-index"] {
            assert!(!root.join(poison).exists(), "poison target {poison}");
        }
    }

    fn scrub_all_git_environment(command: &mut Command) {
        let explicit = command
            .get_envs()
            .filter(|(key, _)| key.as_bytes().starts_with(b"GIT_"))
            .map(|(key, _)| key.to_owned())
            .collect::<Vec<_>>();
        for key in explicit {
            command.env_remove(key);
        }
        for (key, _) in std::env::vars_os() {
            if key.as_os_str().as_bytes().starts_with(b"GIT_") {
                command.env_remove(key);
            }
        }
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C");
    }

    fn write_call(call_id: &str, path: &str, bytes: u64) -> ToolCall {
        write_call_with_fingerprint(call_id, path, bytes, 'a')
    }

    fn write_call_with_fingerprint(
        call_id: &str,
        path: &str,
        bytes: u64,
        fingerprint: char,
    ) -> ToolCall {
        ToolCall {
            id: call_id.to_owned(),
            tool: "write".to_owned(),
            input_json: format!(
                r#"{{"audit_version":"write_edit_v1","tool":"write","path":"{path}","content_bytes":{bytes},"fingerprint_v1":"{}"}}"#,
                fingerprint.to_string().repeat(64)
            ),
        }
    }

    fn edit_call(call_id: &str, path: &str) -> ToolCall {
        ToolCall {
            id: call_id.to_owned(),
            tool: "edit".to_owned(),
            input_json: format!(
                r#"{{"audit_version":"write_edit_v1","tool":"edit","path":"{path}","old_string_bytes":1,"new_string_bytes":1,"fingerprint_v1":"{}"}}"#,
                "b".repeat(64)
            ),
        }
    }

    fn write_result(call_id: &str, path: &str, bytes: u64, reused: bool) -> ToolResult {
        write_result_for_scope(PROJECT_ID, THREAD_ID, call_id, path, bytes, reused)
    }

    fn write_result_for_scope(
        project_id: &str,
        thread_id: &str,
        call_id: &str,
        path: &str,
        bytes: u64,
        reused: bool,
    ) -> ToolResult {
        let checkpoint_ref = vega_tools::CheckpointIds::new(project_id, thread_id, call_id)
            .unwrap()
            .checkpoint_ref();
        ToolResult {
            status: ToolCallStatus::Success,
            output: vega_tools::WriteSuccessOutput {
                path: path.to_owned(),
                bytes_written: bytes,
                checkpoint_ref,
            }
            .to_json()
            .unwrap(),
            reused,
            exit_code: None,
            duration_ms: None,
            truncated: (!reused).then_some(false),
            invalid: None,
        }
    }

    fn failed_result() -> ToolResult {
        ToolResult {
            status: ToolCallStatus::Failed,
            output: "Tool error: write failed".to_owned(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            invalid: None,
        }
    }

    fn rejected_or_cancelled_result(status: ToolCallStatus) -> ToolResult {
        let output = match status {
            ToolCallStatus::Rejected => "Tool error: permission denied",
            ToolCallStatus::Cancelled => vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT,
            _ => panic!("test helper accepts rejected/cancelled only"),
        };
        ToolResult {
            status,
            output: output.to_owned(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            invalid: None,
        }
    }

    fn edit_result(call_id: &str, path: &str, bytes: u64) -> ToolResult {
        let checkpoint_ref = vega_tools::CheckpointIds::new(PROJECT_ID, THREAD_ID, call_id)
            .unwrap()
            .checkpoint_ref();
        ToolResult {
            status: ToolCallStatus::Success,
            output: vega_tools::EditSuccessOutput {
                path: path.to_owned(),
                bytes_written: bytes,
                replacements: 1,
                checkpoint_ref,
            }
            .to_json()
            .unwrap(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: Some(false),
            invalid: None,
        }
    }

    #[test]
    fn artifact_retained_caps_are_inclusive_and_plus_one_fails_closed() {
        let exact_id = "i".repeat(CALL_ID_BYTES);
        let exact_total_input = "x".repeat(PROPOSAL_RETAINED_BYTES - exact_id.len() - 5);
        let exact_proposal = ToolCall {
            id: exact_id.clone(),
            tool: "write".into(),
            input_json: exact_total_input,
        };
        assert!(ArtifactService::validate_proposal(&exact_proposal).is_ok());
        let mut plus_one_total = exact_proposal.clone();
        plus_one_total.input_json.push('x');
        assert_eq!(
            ArtifactService::validate_proposal(&plus_one_total).map_err(|failure| failure.code()),
            Err(GitWorkspaceErrorCode::ArtifactLimit)
        );
        let mut plus_one_id = exact_proposal;
        plus_one_id.id.push('i');
        plus_one_id.input_json.clear();
        assert_eq!(
            ArtifactService::validate_proposal(&plus_one_id).map_err(|failure| failure.code()),
            Err(GitWorkspaceErrorCode::ArtifactLimit)
        );

        let repo = Repo::new();
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).unwrap());
        let service =
            ArtifactService::new(workspace, PROJECT_ID.into(), THREAD_ID.into(), 909).unwrap();
        let call = write_call("cap", "artifact.txt", 1);
        let mut exact_envelope = write_result("cap", "artifact.txt", 1, false);
        exact_envelope.output = "x".repeat(TERMINAL_SUCCESS_BYTES);
        assert!(matches!(
            service
                .prepare_capture(&call, &exact_envelope)
                .map_err(|failure| failure.code()),
            Err(code) if code != GitWorkspaceErrorCode::ArtifactLimit
        ));
        exact_envelope.output.push('x');
        assert!(matches!(
            service
                .prepare_capture(&call, &exact_envelope)
                .map_err(|failure| failure.code()),
            Err(GitWorkspaceErrorCode::ArtifactLimit)
        ));

        let exact_path = "p".repeat(LOGICAL_PATH_BYTES);
        assert!(
            service
                .prepare_capture(
                    &write_call("path-cap", &exact_path, 1),
                    &write_result("path-cap", &exact_path, 1, false),
                )
                .is_ok()
        );
        let plus_one_path = "p".repeat(LOGICAL_PATH_BYTES + 1);
        assert!(matches!(
            service
                .prepare_capture(
                    &write_call("path-over", &plus_one_path, 1),
                    &write_result("path-over", &plus_one_path, 1, false),
                )
                .map_err(|failure| failure.code()),
            Err(GitWorkspaceErrorCode::ArtifactLimit)
        ));

        let fixed = std::mem::size_of::<ArtifactCaptureCandidate>() + 1 + 64;
        let exact_candidate = ArtifactCaptureCandidate {
            call_id: "c".into(),
            fingerprint: TerminalFingerprint::Write {
                path: "p".repeat(CAPTURE_CANDIDATE_RETAINED_BYTES - fixed),
                input_fingerprint: "a".repeat(64),
                bytes_written: 1,
            },
        };
        assert!(validate_candidate_retained(&exact_candidate).is_ok());
        let mut plus_one_candidate = exact_candidate;
        if let TerminalFingerprint::Write { path, .. } = &mut plus_one_candidate.fingerprint {
            path.push('p');
        }
        assert_eq!(
            validate_candidate_retained(&plus_one_candidate).map_err(|failure| failure.code()),
            Err(GitWorkspaceErrorCode::ArtifactLimit)
        );
    }

    async fn refreshed_workspace(repo: &Repo) -> Arc<GitWorkspaceService> {
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).unwrap());
        workspace.refresh(CancellationToken::new()).await.unwrap();
        workspace
    }

    async fn captured_text_artifact(
        repo: &Repo,
        route_epoch: u64,
    ) -> (Arc<GitWorkspaceService>, ArtifactService, ArtifactCard) {
        captured_artifact_at(repo, "artifact.txt", route_epoch).await
    }

    async fn captured_artifact_at(
        repo: &Repo,
        path: &str,
        route_epoch: u64,
    ) -> (Arc<GitWorkspaceService>, ArtifactService, ArtifactCard) {
        let workspace = refreshed_workspace(repo).await;
        let service = ArtifactService::new(
            workspace.clone(),
            PROJECT_ID.to_owned(),
            THREAD_ID.to_owned(),
            route_epoch,
        )
        .unwrap();
        let bytes = fs::metadata(repo.path().join(path)).unwrap().len();
        let call_id = "call-1";
        let card = service
            .capture(
                &write_call(call_id, path, bytes),
                &write_result(call_id, path, bytes, false),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        (workspace, service, card)
    }

    #[test]
    fn text_preview_allowlist_is_exact_and_case_insensitive_only_for_extensions() {
        for extension in [
            "txt", "md", "markdown", "rst", "adoc", "csv", "tsv", "json", "jsonl", "yaml", "yml",
            "toml", "xml", "html", "htm", "css", "scss", "sass", "less", "js", "jsx", "mjs", "cjs",
            "ts", "tsx", "rs", "py", "rb", "go", "java", "kt", "kts", "swift", "c", "h", "cc",
            "cpp", "cxx", "hpp", "hxx", "m", "mm", "sh", "bash", "zsh", "fish", "sql", "graphql",
            "gql", "proto", "diff", "patch", "log",
        ] {
            let accepted = format!("nested/file.{}", extension.to_ascii_uppercase());
            assert!(
                text_preview_path_allowed(OsStr::new(&accepted)),
                "{accepted}"
            );
        }
        for basename in [
            "README",
            "LICENSE",
            "NOTICE",
            "CHANGELOG",
            "Makefile",
            "Dockerfile",
            ".gitignore",
            ".gitattributes",
            ".editorconfig",
        ] {
            assert!(
                text_preview_path_allowed(OsStr::new(basename)),
                "{basename}"
            );
        }
        for rejected in [
            ".env",
            ".npmrc",
            "README.md.bak",
            "readme",
            "image.svg",
            "image.png",
            "unknown.bin",
        ] {
            assert!(
                !text_preview_path_allowed(OsStr::new(rejected)),
                "{rejected}"
            );
        }
    }

    #[test]
    fn preview_line_caps_are_inclusive() {
        let exact_lines = "x\n".repeat(PREVIEW_LINES);
        assert!(validate_preview_lines(&exact_lines).is_ok());
        let too_many = format!("{exact_lines}x");
        assert_eq!(
            validate_preview_lines(&too_many).map_err(|failure| failure.code()),
            Err(GitWorkspaceErrorCode::OutputTooLarge)
        );
        assert!(validate_preview_lines(&"x".repeat(PREVIEW_LINE_BYTES)).is_ok());
        assert_eq!(
            validate_preview_lines(&"x".repeat(PREVIEW_LINE_BYTES + 1))
                .map_err(|failure| failure.code()),
            Err(GitWorkspaceErrorCode::OutputTooLarge)
        );
    }

    #[tokio::test]
    async fn artifact_strict_success_duplicate_and_non_candidates() {
        let repo = Repo::new();
        repo.write("artifact.txt", b"agent\n");
        let (_workspace, service, card) = captured_text_artifact(&repo, 7).await;
        assert_eq!(card.source, ArtifactSource::AgentArtifact);
        assert!(card.current_file_id.is_some());

        let duplicate = service
            .capture(
                &write_call("call-1", "artifact.txt", 6),
                &write_result("call-1", "artifact.txt", 6, false),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(duplicate.id, card.id);
        assert_eq!(service.cards().len(), 1);

        let conflict = service
            .capture(
                &write_call("call-1", "other.txt", 1),
                &write_result("call-1", "other.txt", 1, false),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(conflict.code(), GitWorkspaceErrorCode::ArtifactConflict);
        assert_eq!(service.cards().len(), 1);

        let same_length_different_body = service
            .capture(
                &write_call_with_fingerprint("call-1", "artifact.txt", 6, 'b'),
                &write_result("call-1", "artifact.txt", 6, false),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            same_length_different_body.code(),
            GitWorkspaceErrorCode::ArtifactConflict
        );

        for (call, result) in [
            (
                write_call("checkpoint-call", "artifact.txt", 6),
                write_result("other-call", "artifact.txt", 6, false),
            ),
            (
                write_call("project-call", "artifact.txt", 6),
                write_result_for_scope(
                    "other-project",
                    THREAD_ID,
                    "project-call",
                    "artifact.txt",
                    6,
                    false,
                ),
            ),
            (
                write_call("thread-call", "artifact.txt", 6),
                write_result_for_scope(
                    PROJECT_ID,
                    "other-thread",
                    "thread-call",
                    "artifact.txt",
                    6,
                    false,
                ),
            ),
        ] {
            assert_eq!(
                service
                    .capture(&call, &result, CancellationToken::new())
                    .await
                    .unwrap_err()
                    .code(),
                GitWorkspaceErrorCode::ArtifactConflict
            );
        }

        assert!(
            service
                .capture(
                    &write_call("failed", "artifact.txt", 6),
                    &failed_result(),
                    CancellationToken::new(),
                )
                .await
                .unwrap()
                .is_none()
        );
        for (call_id, status) in [
            ("rejected", ToolCallStatus::Rejected),
            ("cancelled", ToolCallStatus::Cancelled),
        ] {
            assert!(
                service
                    .capture(
                        &write_call(call_id, "artifact.txt", 6),
                        &rejected_or_cancelled_result(status),
                        CancellationToken::new(),
                    )
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            service
                .capture(
                    &write_call("reused", "artifact.txt", 6),
                    &write_result("reused", "artifact.txt", 6, true),
                    CancellationToken::new(),
                )
                .await
                .unwrap()
                .is_none()
        );
        let edit = service
            .capture(
                &edit_call("edit-call", "artifact.txt"),
                &edit_result("edit-call", "artifact.txt", 6),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(edit.source, ArtifactSource::AgentArtifact);
        assert_eq!(service.cards().len(), 2);
        let bash_call = ToolCall {
            id: "bash".to_owned(),
            tool: "bash".to_owned(),
            input_json: r#"{"command":"true"}"#.to_owned(),
        };
        let bash_result = ToolResult {
            status: ToolCallStatus::Success,
            output: String::new(),
            reused: false,
            exit_code: Some(0),
            duration_ms: Some(1),
            truncated: Some(false),
            invalid: None,
        };
        assert!(
            service
                .capture(&bash_call, &bash_result, CancellationToken::new())
                .await
                .unwrap()
                .is_none()
        );
        let read_call = ToolCall {
            id: "read".to_owned(),
            tool: "read".to_owned(),
            input_json: r#"{"path":"artifact.txt"}"#.to_owned(),
        };
        assert!(
            service
                .capture(&read_call, &bash_result, CancellationToken::new())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn artifact_provenance_downgrades_once_and_aba_does_not_upgrade() {
        let repo = Repo::new();
        repo.write("artifact.txt", b"AAAA\n");
        let (workspace, service, card) = captured_text_artifact(&repo, 8).await;
        assert_eq!(card.source, ArtifactSource::AgentArtifact);

        repo.write("artifact.txt", b"BBBB\n");
        workspace.refresh(CancellationToken::new()).await.unwrap();
        let changed = service.reconcile(CancellationToken::new()).await.unwrap();
        assert_eq!(changed[0].source, ArtifactSource::WorkspaceChange);

        repo.write("artifact.txt", b"AAAA\n");
        workspace.refresh(CancellationToken::new()).await.unwrap();
        let restored = service.reconcile(CancellationToken::new()).await.unwrap();
        assert_eq!(restored[0].source, ArtifactSource::WorkspaceChange);
        assert!(restored[0].current_file_id.is_some());
    }

    #[tokio::test]
    async fn artifact_rename_tracks_raw_path_and_delete_disables_actions() {
        let repo = Repo::new();
        repo.write(
            "artifact.txt",
            b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nshared-6\nshared-7\nshared-8\nbase\n",
        );
        repo.commit_all();
        repo.write(
            "artifact.txt",
            b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nshared-6\nshared-7\nshared-8\nagent\n",
        );
        let (workspace, service, card) = captured_text_artifact(&repo, 9).await;
        assert_eq!(card.source, ArtifactSource::AgentArtifact);

        fs::rename(
            repo.path().join("artifact.txt"),
            repo.path().join("renamed.txt"),
        )
        .unwrap();
        git(repo.path(), &["add", "-A"]);
        workspace.refresh(CancellationToken::new()).await.unwrap();
        let renamed = service.reconcile(CancellationToken::new()).await.unwrap();
        assert_eq!(renamed[0].label, "renamed.txt");
        assert_eq!(renamed[0].source, ArtifactSource::AgentArtifact);
        assert!(renamed[0].current_file_id.is_some());

        fs::remove_file(repo.path().join("renamed.txt")).unwrap();
        workspace.refresh(CancellationToken::new()).await.unwrap();
        let deleted = service.reconcile(CancellationToken::new()).await.unwrap();
        assert_eq!(deleted[0].label, "renamed.txt");
        assert!(deleted[0].current_file_id.is_none());
        assert_eq!(deleted[0].source, ArtifactSource::WorkspaceChange);
        assert_eq!(
            service
                .preview(card.id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );

        repo.write("renamed.txt", b"replacement\n");
        workspace.refresh(CancellationToken::new()).await.unwrap();
        let recreated = service.reconcile(CancellationToken::new()).await.unwrap();
        assert!(recreated[0].current_file_id.is_none());
        assert_eq!(recreated[0].source, ArtifactSource::WorkspaceChange);
    }

    #[tokio::test]
    async fn artifact_rename_old_path_collision_never_binds_replacement() {
        let repo = Repo::new();
        repo.write(
            "artifact.txt",
            b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nbase\n",
        );
        repo.commit_all();
        repo.write(
            "artifact.txt",
            b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nagent\n",
        );
        let (workspace, service, card) = captured_text_artifact(&repo, 91).await;
        assert_eq!(card.source, ArtifactSource::AgentArtifact);

        git(repo.path(), &["mv", "artifact.txt", "renamed.txt"]);
        repo.write("artifact.txt", b"unrelated replacement\n");
        workspace.refresh(CancellationToken::new()).await.unwrap();
        let reconciled = service.reconcile(CancellationToken::new()).await.unwrap();
        assert_eq!(reconciled[0].label, "artifact.txt");
        assert_eq!(reconciled[0].source, ArtifactSource::WorkspaceChange);
        assert!(reconciled[0].current_file_id.is_none());
        assert_eq!(
            service
                .open_in(
                    card.id,
                    OpenInTarget::DefaultApplication,
                    CancellationToken::new(),
                )
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
        assert_eq!(service.launch_attempts(), 0);
    }

    #[tokio::test]
    async fn artifact_preview_is_bounded_utf8_no_nul_and_path_classified() {
        let repo = Repo::new();
        repo.write("artifact.txt", b"safe preview\n");
        let (workspace, service, card) = captured_text_artifact(&repo, 10).await;
        let preview = service
            .preview(card.id, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(preview.text(), "safe preview\n");
        assert!(!format!("{preview:?}").contains("safe preview"));

        repo.write("artifact.txt", b"secret\0tail\n");
        workspace.refresh(CancellationToken::new()).await.unwrap();
        let cards = service.reconcile(CancellationToken::new()).await.unwrap();
        assert_eq!(
            service
                .preview(cards[0].id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::MetadataOnly
        );

        let oversized = vec![b'x'; PREVIEW_BYTES + 1];
        repo.write("large.txt", &oversized);
        workspace.refresh(CancellationToken::new()).await.unwrap();
        let large = service
            .capture(
                &write_call("large", "large.txt", (PREVIEW_BYTES + 1) as u64),
                &write_result("large", "large.txt", (PREVIEW_BYTES + 1) as u64, false),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(large.source, ArtifactSource::WorkspaceChange);
        assert!(large.current_file_id.is_some());
        assert_eq!(
            service
                .preview(large.id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::OutputTooLarge
        );

        repo.write("unknown.svg", b"<svg>secret</svg>\n");
        workspace.refresh(CancellationToken::new()).await.unwrap();
        let svg = service
            .capture(
                &write_call("svg", "unknown.svg", 18),
                &write_result("svg", "unknown.svg", 18, false),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            service
                .preview(svg.id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::MetadataOnly
        );
    }

    #[tokio::test]
    async fn artifact_preview_public_api_exact_and_plus_one_boundaries() {
        let repo = Repo::new();
        let exact_bytes = format!("{}\n", "x".repeat(127)).repeat(8192);
        assert_eq!(exact_bytes.len(), PREVIEW_BYTES);
        let too_many_bytes = format!("{exact_bytes}x");
        let exact_lines = "line\n".repeat(PREVIEW_LINES);
        let too_many_lines = format!("{exact_lines}line");
        let exact_line = "z".repeat(PREVIEW_LINE_BYTES);
        let too_long_line = "z".repeat(PREVIEW_LINE_BYTES + 1);
        for (path, bytes) in [
            ("exact-bytes.txt", exact_bytes.as_bytes()),
            ("too-many-bytes.txt", too_many_bytes.as_bytes()),
            ("exact-lines.txt", exact_lines.as_bytes()),
            ("too-many-lines.txt", too_many_lines.as_bytes()),
            ("exact-line.txt", exact_line.as_bytes()),
            ("too-long-line.txt", too_long_line.as_bytes()),
            ("invalid-utf8.txt", b"sentinel-\xff"),
        ] {
            repo.write(path, bytes);
        }
        let workspace = refreshed_workspace(&repo).await;
        let service =
            ArtifactService::new(workspace, PROJECT_ID.to_owned(), THREAD_ID.to_owned(), 101)
                .unwrap();
        for (index, (path, expected)) in [
            ("exact-bytes.txt", None),
            (
                "too-many-bytes.txt",
                Some(GitWorkspaceErrorCode::OutputTooLarge),
            ),
            ("exact-lines.txt", None),
            (
                "too-many-lines.txt",
                Some(GitWorkspaceErrorCode::OutputTooLarge),
            ),
            ("exact-line.txt", None),
            (
                "too-long-line.txt",
                Some(GitWorkspaceErrorCode::OutputTooLarge),
            ),
            (
                "invalid-utf8.txt",
                Some(GitWorkspaceErrorCode::MetadataOnly),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let call_id = format!("preview-{index}");
            let bytes = fs::metadata(repo.path().join(path)).unwrap().len();
            let card = service
                .capture(
                    &write_call(&call_id, path, bytes),
                    &write_result(&call_id, path, bytes, false),
                    CancellationToken::new(),
                )
                .await
                .unwrap()
                .unwrap();
            match expected {
                None => {
                    let projection = service
                        .preview(card.id, CancellationToken::new())
                        .await
                        .unwrap();
                    assert_eq!(projection.text().len() as u64, bytes);
                    assert!(!format!("{projection:?}").contains("sentinel"));
                }
                Some(code) => assert_eq!(
                    service
                        .preview(card.id, CancellationToken::new())
                        .await
                        .unwrap_err()
                        .code(),
                    code
                ),
            }
        }
    }

    #[tokio::test]
    async fn artifact_route_card_limit_is_inclusive() {
        let repo = Repo::new();
        let workspace = refreshed_workspace(&repo).await;
        let service =
            ArtifactService::new(workspace, PROJECT_ID.to_owned(), THREAD_ID.to_owned(), 11)
                .unwrap();
        for slot in 0..ROUTE_CARD_LIMIT {
            let call_id = format!("missing-{slot}");
            let card = service
                .capture(
                    &write_call(&call_id, "missing.txt", 1),
                    &write_result(&call_id, "missing.txt", 1, false),
                    CancellationToken::new(),
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(card.source, ArtifactSource::WorkspaceChange);
            assert!(card.current_file_id.is_none());
        }
        assert_eq!(service.cards().len(), ROUTE_CARD_LIMIT);
        let failure = service
            .capture(
                &write_call("missing-overflow", "missing.txt", 1),
                &write_result("missing-overflow", "missing.txt", 1, false),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(failure.code(), GitWorkspaceErrorCode::ArtifactLimit);
        assert_eq!(service.cards().len(), ROUTE_CARD_LIMIT);
    }

    fn launcher_script(root: &Path, body: &str) -> PathBuf {
        let script = root.join("fake-open");
        fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        script
    }

    fn raw_argv(path: &Path) -> Vec<Vec<u8>> {
        let bytes = fs::read(path).unwrap();
        let payload = bytes.strip_suffix(&[0]).unwrap_or(&bytes);
        payload
            .split(|byte| *byte == 0)
            .map(<[u8]>::to_vec)
            .collect()
    }

    fn pid_is_alive(pid: u32) -> bool {
        for _ in 0..100 {
            let alive = Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !alive {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
        true
    }

    #[tokio::test]
    async fn open_in_uses_six_exact_raw_argv_forms() {
        let repo = Repo::new();
        repo.write(
            "artifact.txt",
            b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nshared-6\nshared-7\nshared-8\nbase\n",
        );
        repo.commit_all();
        repo.write(
            "artifact.txt",
            b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nshared-6\nshared-7\nshared-8\nagent\n",
        );
        let (workspace, base_service, card) = captured_text_artifact(&repo, 12).await;

        let raw_name = OsString::from("-awkward name\tline\n.txt");
        fs::rename(
            repo.path().join("artifact.txt"),
            repo.path().join(&raw_name),
        )
        .unwrap();
        git(repo.path(), &["add", "-A"]);
        workspace.refresh(CancellationToken::new()).await.unwrap();
        let launcher_dir = tempfile::tempdir().unwrap();
        let recording = launcher_dir.path().join("argv.bin");
        let script_body = format!(
            ": > '{}'; for arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{}'; done; exit 0",
            recording.display(),
            recording.display()
        );
        let launcher = launcher_script(launcher_dir.path(), &script_body);
        let service = ArtifactService::new_for_test(
            workspace,
            PROJECT_ID.to_owned(),
            THREAD_ID.to_owned(),
            12,
            launcher,
            Duration::from_secs(1),
        )
        .unwrap();
        {
            let mut target = service.state.lock().unwrap();
            let source = base_service.state.lock().unwrap();
            target.by_call_id.insert("call-1".to_owned(), 0);
            let original = &source.cards[0];
            target.cards.push(ArtifactRecord {
                id: ArtifactCardId {
                    route_epoch: 12,
                    slot: 0,
                    seal: card_seal(service.instance_nonce, 12, 0),
                },
                fingerprint: original.fingerprint.clone(),
                path: original.path.clone(),
                label: original.label.clone(),
                source: original.source,
                evidence: original.evidence.clone(),
                current_file_id: original.current_file_id,
                stale_disabled: original.stale_disabled,
            });
        }
        let card_id = service.reconcile(CancellationToken::new()).await.unwrap()[0].id;
        let canonical_root = fs::canonicalize(repo.path()).unwrap();
        let absolute_target = canonical_root.join(&raw_name);
        let cases = [
            (
                OpenInTarget::VisualStudioCode,
                vec![
                    b"-a".to_vec(),
                    b"Visual Studio Code".to_vec(),
                    absolute_target.as_os_str().as_bytes().to_vec(),
                ],
            ),
            (
                OpenInTarget::Cursor,
                vec![
                    b"-a".to_vec(),
                    b"Cursor".to_vec(),
                    absolute_target.as_os_str().as_bytes().to_vec(),
                ],
            ),
            (
                OpenInTarget::Zed,
                vec![
                    b"-a".to_vec(),
                    b"Zed".to_vec(),
                    absolute_target.as_os_str().as_bytes().to_vec(),
                ],
            ),
            (
                OpenInTarget::Terminal,
                vec![
                    b"-a".to_vec(),
                    b"Terminal".to_vec(),
                    canonical_root.as_os_str().as_bytes().to_vec(),
                ],
            ),
            (
                OpenInTarget::DefaultApplication,
                vec![absolute_target.as_os_str().as_bytes().to_vec()],
            ),
            (
                OpenInTarget::RevealInFinder,
                vec![
                    b"-R".to_vec(),
                    absolute_target.as_os_str().as_bytes().to_vec(),
                ],
            ),
        ];
        for (target, expected) in cases {
            service
                .open_in(card_id, target, CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(raw_argv(&recording), expected);
        }
        assert_eq!(service.launch_attempts(), 6);

        let non_utf8_target = canonical_root.join(OsString::from_vec(b"raw-\xff.txt".to_vec()));
        let status = Command::new(&service.launcher)
            .args(open_arguments(
                &canonical_root,
                &non_utf8_target,
                OpenInTarget::DefaultApplication,
            ))
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            raw_argv(&recording),
            vec![non_utf8_target.as_os_str().as_bytes().to_vec()]
        );
        assert_eq!(card.id.route_epoch, 12);
    }

    #[tokio::test]
    async fn open_in_preflight_is_zero_attempt_and_failures_are_one_attempt() {
        let repo = Repo::new();
        repo.write("artifact.txt", b"agent\n");
        let (workspace, _base, card) = captured_text_artifact(&repo, 13).await;
        let launcher_dir = tempfile::tempdir().unwrap();
        let success_launcher = launcher_script(launcher_dir.path(), "exit 0");
        let service = ArtifactService::new_for_test(
            workspace.clone(),
            PROJECT_ID.to_owned(),
            THREAD_ID.to_owned(),
            13,
            success_launcher,
            Duration::from_secs(1),
        )
        .unwrap();
        let imported = service
            .capture(
                &write_call("call-1", "artifact.txt", 6),
                &write_result("call-1", "artifact.txt", 6, false),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        repo.write("artifact.txt", b"changed\n");
        workspace.refresh(CancellationToken::new()).await.unwrap();
        assert!(
            service
                .open_in(
                    imported.id,
                    OpenInTarget::DefaultApplication,
                    CancellationToken::new(),
                )
                .await
                .is_err()
        );
        assert_eq!(service.launch_attempts(), 0);

        let current = service.reconcile(CancellationToken::new()).await.unwrap()[0].id;
        let missing = ArtifactService::new_for_test(
            workspace.clone(),
            PROJECT_ID.to_owned(),
            THREAD_ID.to_owned(),
            14,
            launcher_dir.path().join("missing-open"),
            Duration::from_secs(1),
        )
        .unwrap();
        let missing_card = missing
            .capture(
                &write_call("missing-call", "artifact.txt", 8),
                &write_result("missing-call", "artifact.txt", 8, false),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            missing
                .open_in(
                    missing_card.id,
                    OpenInTarget::DefaultApplication,
                    CancellationToken::new(),
                )
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::SpawnFailed
        );
        assert_eq!(missing.launch_attempts(), 1);

        let nonzero_launcher = launcher_script(launcher_dir.path(), "exit 7");
        let nonzero = ArtifactService::new_for_test(
            workspace.clone(),
            PROJECT_ID.to_owned(),
            THREAD_ID.to_owned(),
            15,
            nonzero_launcher,
            Duration::from_secs(1),
        )
        .unwrap();
        let nonzero_card = nonzero
            .capture(
                &write_call("nonzero-call", "artifact.txt", 8),
                &write_result("nonzero-call", "artifact.txt", 8, false),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            nonzero
                .open_in(
                    nonzero_card.id,
                    OpenInTarget::DefaultApplication,
                    CancellationToken::new(),
                )
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::GitFailed
        );
        assert_eq!(nonzero.launch_attempts(), 1);

        let timeout_pids = launcher_dir.path().join("timeout-pids");
        let timeout_launcher = launcher_script(
            launcher_dir.path(),
            &format!(
                "sleep 5 & child=$!; printf '%s\\n%s\\n' \"$$\" \"$child\" > '{}'; wait \"$child\"",
                timeout_pids.display()
            ),
        );
        let timeout = ArtifactService::new_for_test(
            workspace,
            PROJECT_ID.to_owned(),
            THREAD_ID.to_owned(),
            16,
            timeout_launcher,
            Duration::from_millis(20),
        )
        .unwrap();
        let timeout_card = timeout
            .capture(
                &write_call("timeout-call", "artifact.txt", 8),
                &write_result("timeout-call", "artifact.txt", 8, false),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            timeout
                .open_in(
                    timeout_card.id,
                    OpenInTarget::DefaultApplication,
                    CancellationToken::new(),
                )
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::TimedOut
        );
        assert_eq!(timeout.launch_attempts(), 1);
        let timeout_processes = fs::read_to_string(&timeout_pids)
            .unwrap()
            .lines()
            .map(|line| line.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(timeout_processes.len(), 2);
        for pid in timeout_processes {
            assert!(
                !pid_is_alive(pid),
                "timed-out launcher descendant {pid} survived"
            );
        }
        assert_eq!(current.route_epoch, 13);
        assert_eq!(card.id.route_epoch, 13);
    }

    #[tokio::test]
    async fn open_in_symlink_segment_hardlink_special_and_root_swap_are_zero_attempt() {
        use std::os::unix::fs::symlink;

        let launcher_dir = tempfile::tempdir().unwrap();
        let launcher = launcher_script(launcher_dir.path(), "exit 0");

        let symlink_repo = Repo::new();
        symlink_repo.write("nested/artifact.txt", b"agent\n");
        let (_workspace, mut symlink_service, symlink_card) =
            captured_artifact_at(&symlink_repo, "nested/artifact.txt", 21).await;
        symlink_service.launcher = launcher.clone();
        let external = tempfile::tempdir().unwrap();
        fs::rename(
            symlink_repo.path().join("nested"),
            external.path().join("nested"),
        )
        .unwrap();
        symlink(
            external.path().join("nested"),
            symlink_repo.path().join("nested"),
        )
        .unwrap();
        assert!(
            symlink_service
                .open_in(
                    symlink_card.id,
                    OpenInTarget::DefaultApplication,
                    CancellationToken::new(),
                )
                .await
                .is_err()
        );
        assert_eq!(symlink_service.launch_attempts(), 0);

        let hardlink_repo = Repo::new();
        hardlink_repo.write("artifact.txt", b"agent\n");
        let (_workspace, mut hardlink_service, hardlink_card) =
            captured_text_artifact(&hardlink_repo, 22).await;
        hardlink_service.launcher = launcher.clone();
        let hardlink_dir = tempfile::tempdir().unwrap();
        fs::hard_link(
            hardlink_repo.path().join("artifact.txt"),
            hardlink_dir.path().join("alias.txt"),
        )
        .unwrap();
        assert!(
            hardlink_service
                .open_in(
                    hardlink_card.id,
                    OpenInTarget::DefaultApplication,
                    CancellationToken::new(),
                )
                .await
                .is_err()
        );
        assert_eq!(hardlink_service.launch_attempts(), 0);

        let special_repo = Repo::new();
        special_repo.write("artifact.txt", b"agent\n");
        let (_workspace, mut special_service, special_card) =
            captured_text_artifact(&special_repo, 23).await;
        special_service.launcher = launcher.clone();
        fs::remove_file(special_repo.path().join("artifact.txt")).unwrap();
        let status = Command::new("/usr/bin/mkfifo")
            .arg(special_repo.path().join("artifact.txt"))
            .status()
            .unwrap();
        assert!(status.success());
        assert!(
            special_service
                .open_in(
                    special_card.id,
                    OpenInTarget::DefaultApplication,
                    CancellationToken::new(),
                )
                .await
                .is_err()
        );
        assert_eq!(special_service.launch_attempts(), 0);

        let root_repo = Repo::new();
        root_repo.write("artifact.txt", b"agent\n");
        let (_workspace, mut root_service, root_card) =
            captured_text_artifact(&root_repo, 24).await;
        root_service.launcher = launcher;
        let original_root = root_repo.path().to_path_buf();
        let moved_root = original_root.with_extension("moved-root");
        fs::rename(&original_root, &moved_root).unwrap();
        fs::create_dir(&original_root).unwrap();
        assert!(
            root_service
                .open_in(
                    root_card.id,
                    OpenInTarget::DefaultApplication,
                    CancellationToken::new(),
                )
                .await
                .is_err()
        );
        assert_eq!(root_service.launch_attempts(), 0);
        fs::remove_dir(&original_root).unwrap();
        fs::rename(&moved_root, &original_root).unwrap();
    }
}
