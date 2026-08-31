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

mod helpers;

#[cfg(test)]
mod tests;

pub(crate) use helpers::*;
