//! Private bounded Git workspace service.
//!
//! Raw repository roots, paths, stderr, and patch bytes never leave this
//! module. Public callers receive only safe projections from `types`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::types::{
    DiffHunk, DiffLanguage, DiffLayer, DiffRow, DiffRowKind, DiffSection, DiffTextProjection,
    GitWorkspaceError, GitWorkspaceErrorCode, WorkspaceChangeKind, WorkspaceFile, WorkspaceFileId,
    WorkspaceHead, WorkspaceLineCount, WorkspaceSnapshot, WorkspaceStats,
};

mod branch;
mod trusted_git;
pub use branch::{BranchSwitchPermit, BranchWorkspaceService};
pub use trusted_git::TrustedGitService;

const GIT: &str = "/usr/bin/git";
const KILL: &str = "/bin/kill";
const IO_CHUNK: usize = 16 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const MUTATION_TIMEOUT: Duration = Duration::from_secs(120);
const TERM_GRACE: Duration = Duration::from_millis(300);
const DRAIN_GRACE: Duration = Duration::from_millis(500);
const STDOUT_LIMIT: usize = 8 * 1024 * 1024;
const STDERR_LIMIT: usize = 64 * 1024;
const MUTATION_STDOUT_LIMIT: usize = 1024 * 1024;
const SNAPSHOT_LIMIT: usize = 8 * 1024 * 1024;
const PATH_LIMIT: usize = 10_000;
const PATCH_LIMIT: usize = 4 * 1024 * 1024;
const PATCH_ROW_LIMIT: usize = 20_000;
const PATCH_LINE_LIMIT: usize = 64 * 1024;
static SERVICE_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
struct RetainedBudget {
    retained: usize,
    cap: usize,
}

impl RetainedBudget {
    fn new(cap: usize) -> Self {
        Self { retained: 0, cap }
    }

    fn remaining(self) -> usize {
        self.cap - self.retained
    }

    fn charge(&mut self, bytes: usize) -> Result<(), GitWorkspaceError> {
        self.retained = self
            .retained
            .checked_add(bytes)
            .filter(|retained| *retained <= self.cap)
            .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        Ok(())
    }

    fn retained(self) -> usize {
        self.retained
    }
}

const PREFIX: &[&str] = &[
    "--no-pager",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "color.ui=false",
    "-c",
    "maintenance.auto=false",
    "-c",
    "maintenance.autoDetach=false",
    "-c",
    "gc.auto=0",
];

#[derive(Clone, Copy)]
struct RootIdentity {
    dev: u64,
    ino: u64,
}

#[derive(Clone)]
struct PrivateFile {
    id: WorkspaceFileId,
    path: OsString,
    previous_path: Option<OsString>,
    staged: WorkspaceChangeKind,
    unstaged: WorkspaceChangeKind,
    binary: bool,
    metadata_only: bool,
    language: DiffLanguage,
    snapshot_identity: Arc<SnapshotIdentity>,
    worktree_identity: Option<FileIdentity>,
}

/// Crate-private capability for one file in the current workspace snapshot.
/// Raw paths never cross the `vega_conversation` boundary.
#[derive(Clone)]
pub(crate) struct ArtifactWorkspaceFile {
    pub(crate) id: WorkspaceFileId,
    pub(crate) path: OsString,
    snapshot_identity: Arc<SnapshotIdentity>,
    worktree_identity: Option<FileIdentity>,
}

#[derive(Clone)]
pub(crate) struct ArtifactPathMatch {
    pub(crate) file: ArtifactWorkspaceFile,
    pub(crate) previous_path_match: bool,
}

impl ArtifactWorkspaceFile {
    pub(crate) fn is_regular_current(&self) -> bool {
        self.worktree_identity
            .is_some_and(|identity| identity.kind == 0)
    }
}

/// Private, generation-bound provenance evidence.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ArtifactEvidence {
    pub(crate) dev: u64,
    pub(crate) ino: u64,
    pub(crate) size: u64,
    pub(crate) mtime_ns: i128,
    pub(crate) digest: Vec<u8>,
}

pub(crate) struct ArtifactOpenGuard {
    root: PathBuf,
    parent: PathBuf,
    target: PathBuf,
    root_fd: File,
    parent_fd: File,
    target_fd: File,
    root_identity: FileIdentity,
    parent_identity: FileIdentity,
    target_identity: FileIdentity,
}

impl ArtifactOpenGuard {
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn target(&self) -> &Path {
        &self.target
    }

    pub(crate) fn revalidate(&self) -> Result<(), GitWorkspaceError> {
        for (path, fd, expected) in [
            (&self.root, &self.root_fd, self.root_identity),
            (&self.parent, &self.parent_fd, self.parent_identity),
            (&self.target, &self.target_fd, self.target_identity),
        ] {
            let canonical = fs::canonicalize(path)
                .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
            let path_metadata = fs::symlink_metadata(path)
                .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
            let fd_metadata = fd
                .metadata()
                .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
            if canonical != *path
                || file_identity(&path_metadata) != expected
                || file_identity(&fd_metadata) != expected
            {
                return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
            }
        }
        if !self.target.starts_with(&self.root) || !self.parent.starts_with(&self.root) {
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    dev: u64,
    ino: u64,
    kind: u8,
    mode: u32,
    size: u64,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
}

#[derive(PartialEq, Eq)]
struct SnapshotIdentity {
    filter_paths: Arc<[u8]>,
    filter_attrs: Vec<u8>,
    status: Vec<u8>,
    staged_raw: Vec<u8>,
    unstaged_raw: Vec<u8>,
    staged_numstat: Vec<u8>,
    unstaged_numstat: Vec<u8>,
}

#[derive(Default)]
struct ServiceState {
    next_request: u64,
    latest_request: u64,
    next_generation: u64,
    content_revision: u64,
    next_mutation_owner: u64,
    active_mutation_owner: Option<WorkspaceMutationOwner>,
    generation: u64,
    identity: Option<Arc<SnapshotIdentity>>,
    snapshot: Option<WorkspaceSnapshot>,
    files: Vec<PrivateFile>,
}

/// Single-use capability for the authoritative snapshot handoff following a
/// trusted mutation. It is minted before mutation begins so failed captures
/// and concurrent ordinary polls cannot silently transfer owner authority.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorkspaceMutationOwner {
    sequence: u64,
    parent_generation: u64,
    parent_revision: u64,
    seal: u64,
}

/// Headless, ephemeral Git snapshot and lazy-diff service.
pub struct GitWorkspaceService {
    root: PathBuf,
    identity: RootIdentity,
    instance_nonce: u64,
    state: Arc<Mutex<ServiceState>>,
    #[cfg(test)]
    executable: Option<PathBuf>,
}

impl std::fmt::Debug for GitWorkspaceService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let generation = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .generation;
        formatter
            .debug_struct("GitWorkspaceService")
            .field("root", &"[redacted]")
            .field("generation", &generation)
            .finish()
    }
}

impl GitWorkspaceService {
    /// Creates a service fenced to one canonical repository root.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, GitWorkspaceError> {
        Self::new_inner(root.as_ref(), None)
    }

    fn new_inner(
        root: &Path,
        #[allow(unused_variables)] executable: Option<PathBuf>,
    ) -> Result<Self, GitWorkspaceError> {
        let root = fs::canonicalize(root).map_err(|_| error(GitWorkspaceErrorCode::InvalidRoot))?;
        let metadata =
            fs::metadata(&root).map_err(|_| error(GitWorkspaceErrorCode::InvalidRoot))?;
        if !metadata.is_dir() {
            return Err(error(GitWorkspaceErrorCode::InvalidRoot));
        }
        let instance_nonce = SERVICE_NONCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        Ok(Self {
            root,
            identity: RootIdentity {
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
            instance_nonce,
            state: Arc::new(Mutex::new(ServiceState::default())),
            #[cfg(test)]
            executable,
        })
    }

    /// Refreshes the complete metadata snapshot. A newer request invalidates
    /// an older in-flight result, while byte-identical content retains its
    /// generation and opaque file identifiers.
    pub async fn refresh(
        &self,
        cancel: CancellationToken,
    ) -> Result<WorkspaceSnapshot, GitWorkspaceError> {
        let request = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let request = state
                .next_request
                .checked_add(1)
                .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
            state.next_request = request;
            state.latest_request = request;
            request
        };
        let root = self.root.clone();
        let identity = self.identity;
        let instance_nonce = self.instance_nonce;
        #[cfg(test)]
        let executable = self.executable.clone();
        let result = tokio::task::spawn_blocking(move || {
            let runner = Runner::new(
                root,
                identity,
                #[cfg(test)]
                executable,
            );
            build_snapshot(&runner, 0, instance_nonce, &cancel)
        })
        .await
        .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))
        .and_then(|result| result);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.latest_request != request {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        let (mut snapshot, mut files, identity) = match result {
            Ok(result) => result,
            Err(failure) => {
                invalidate_current(&mut state);
                return Err(failure);
            }
        };
        if same_snapshot_content(&state, &identity, &snapshot, &files) {
            return state
                .snapshot
                .clone()
                .ok_or_else(|| error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
        let Some(generation) = state.next_generation.checked_add(1) else {
            invalidate_current(&mut state);
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        };
        if let Err(failure) = assign_generation(
            &mut snapshot,
            &mut files,
            generation,
            self.identity,
            self.instance_nonce,
        ) {
            invalidate_current(&mut state);
            return Err(failure);
        }
        let Some(content_revision) = state.content_revision.checked_add(1) else {
            invalidate_current(&mut state);
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        };
        state.next_generation = generation;
        state.content_revision = content_revision;
        state.generation = generation;
        state.identity = Some(identity);
        state.files = files;
        state.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }

    /// Reserves the exact A -> terminal handoff before a trusted mutation.
    /// Only this capability can retry a failed owner capture or publish into
    /// an invalidated state that has not observed another successful content
    /// generation.
    pub(crate) fn begin_owned_refresh(
        &self,
        parent_generation: u64,
    ) -> Result<WorkspaceMutationOwner, GitWorkspaceError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active_mutation_owner.is_some()
            || state.generation != parent_generation
            || state.snapshot.as_ref().map(|snapshot| snapshot.generation)
                != Some(parent_generation)
        {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        let sequence = state
            .next_mutation_owner
            .checked_add(1)
            .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        let slot =
            u32::try_from(sequence).map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        let owner = WorkspaceMutationOwner {
            sequence,
            parent_generation,
            parent_revision: state.content_revision,
            seal: seal(
                self.identity,
                self.instance_nonce,
                parent_generation,
                slot,
                b"workspace-mutation-owner",
            ),
        };
        state.next_mutation_owner = sequence;
        state.active_mutation_owner = Some(owner);
        Ok(owner)
    }

    pub(crate) fn active_owned_refresh(&self) -> Option<WorkspaceMutationOwner> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_mutation_owner
    }

    /// Publishes the authoritative snapshot following one trusted mutation.
    ///
    /// Unlike an ordinary poll, this capture owns the A -> B handoff. Its
    /// commit linearization fences every poll registered before it, accepts a
    /// byte-exact B already published by such a poll, and rejects C/ABA rather
    /// than making success depend on callback order.
    pub(crate) async fn refresh_owned_after_mutation(
        &self,
        owner: WorkspaceMutationOwner,
        cancel: CancellationToken,
    ) -> Result<WorkspaceSnapshot, GitWorkspaceError> {
        let root = self.root.clone();
        let identity = self.identity;
        let instance_nonce = self.instance_nonce;
        #[cfg(test)]
        let executable = self.executable.clone();
        let result = tokio::task::spawn_blocking(move || {
            let runner = Runner::new(
                root,
                identity,
                #[cfg(test)]
                executable,
            );
            build_snapshot(&runner, 0, instance_nonce, &cancel)
        })
        .await
        .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))
        .and_then(|result| result);

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active_mutation_owner != Some(owner)
            || owner.seal
                != seal(
                    self.identity,
                    self.instance_nonce,
                    owner.parent_generation,
                    u32::try_from(owner.sequence)
                        .map_err(|_| error(GitWorkspaceErrorCode::StaleGeneration))?,
                    b"workspace-mutation-owner",
                )
        {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        let (mut snapshot, mut files, identity) = match result {
            Ok(result) => result,
            // The exact owner retains retry authority after a failed capture.
            // In particular, do not destroy its parent/revision evidence here.
            Err(failure) => return Err(failure),
        };

        let revision_delta = state
            .content_revision
            .checked_sub(owner.parent_revision)
            .ok_or_else(|| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
        if same_snapshot_content(&state, &identity, &snapshot, &files) {
            // One changed generation may be the exact B published by an
            // ordinary poll. Two or more changes are C/ABA and must never
            // revive the owner's earlier capability.
            if revision_delta <= 1 {
                let request = state
                    .next_request
                    .checked_add(1)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
                state.next_request = request;
                state.latest_request = request;
                state.active_mutation_owner = None;
                return state
                    .snapshot
                    .clone()
                    .ok_or_else(|| error(GitWorkspaceErrorCode::ChangedDuringRead));
            }
            state.active_mutation_owner = None;
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }

        let parent_is_current = state.generation == owner.parent_generation && revision_delta == 0;
        let only_failed_poll_invalidated =
            state.generation == 0 && state.snapshot.is_none() && revision_delta == 0;
        if !parent_is_current && !only_failed_poll_invalidated {
            state.active_mutation_owner = None;
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
        let Some(generation) = state.next_generation.checked_add(1) else {
            state.active_mutation_owner = None;
            invalidate_current(&mut state);
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        };
        if let Err(failure) = assign_generation(
            &mut snapshot,
            &mut files,
            generation,
            self.identity,
            self.instance_nonce,
        ) {
            state.active_mutation_owner = None;
            invalidate_current(&mut state);
            return Err(failure);
        }
        let Some(content_revision) = state.content_revision.checked_add(1) else {
            state.active_mutation_owner = None;
            invalidate_current(&mut state);
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        };
        state.next_generation = generation;
        state.content_revision = content_revision;
        state.generation = generation;
        state.identity = Some(identity);
        state.files = files;
        state.snapshot = Some(snapshot.clone());
        let Some(request) = state.next_request.checked_add(1) else {
            state.active_mutation_owner = None;
            invalidate_current(&mut state);
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        };
        state.next_request = request;
        state.latest_request = request;
        state.active_mutation_owner = None;
        Ok(snapshot)
    }

    /// Lazily loads one structured patch for the current snapshot.
    pub async fn diff(
        &self,
        file_id: WorkspaceFileId,
        cancel: CancellationToken,
    ) -> Result<DiffTextProjection, GitWorkspaceError> {
        let private = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if state.generation != file_id.generation {
                return Err(error(GitWorkspaceErrorCode::StaleGeneration));
            }
            let slot = usize::try_from(file_id.slot)
                .map_err(|_| error(GitWorkspaceErrorCode::UnknownFile))?;
            state
                .files
                .get(slot)
                .filter(|file| file.id == file_id)
                .cloned()
                .ok_or_else(|| error(GitWorkspaceErrorCode::UnknownFile))?
        };
        let root = self.root.clone();
        let identity = self.identity;
        #[cfg(test)]
        let executable = self.executable.clone();
        let result = tokio::task::spawn_blocking(move || {
            let runner = Runner::new(
                root,
                identity,
                #[cfg(test)]
                executable,
            );
            build_projection(&runner, private, &cancel)
        })
        .await
        .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))??;
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let current = usize::try_from(file_id.slot)
            .ok()
            .and_then(|slot| state.files.get(slot))
            .is_some_and(|file| file.id == file_id);
        if state.generation != file_id.generation || !current {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        Ok(result)
    }

    pub(crate) fn artifact_file_for_path(&self, path: &OsStr) -> Option<ArtifactWorkspaceFile> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let file = unique_file(
            state
                .files
                .iter()
                .filter(|file| file.path.as_bytes() == path.as_bytes()),
        )?;
        Some(artifact_workspace_file(file))
    }

    pub(crate) fn artifact_path_matches(&self, path: &OsStr) -> Vec<ArtifactPathMatch> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state
            .files
            .iter()
            .filter_map(|file| {
                let exact = file.path.as_bytes() == path.as_bytes();
                let previous = file
                    .previous_path
                    .as_ref()
                    .is_some_and(|previous| previous.as_bytes() == path.as_bytes());
                (exact || previous).then(|| ArtifactPathMatch {
                    file: artifact_workspace_file(file),
                    previous_path_match: previous,
                })
            })
            .collect()
    }

    pub(crate) fn artifact_file_by_id(
        &self,
        file_id: WorkspaceFileId,
    ) -> Result<ArtifactWorkspaceFile, GitWorkspaceError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.generation != file_id.generation {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        let slot =
            usize::try_from(file_id.slot).map_err(|_| error(GitWorkspaceErrorCode::UnknownFile))?;
        let file = state
            .files
            .get(slot)
            .filter(|file| file.id == file_id)
            .ok_or_else(|| error(GitWorkspaceErrorCode::UnknownFile))?;
        Ok(artifact_workspace_file(file))
    }

    pub(crate) async fn artifact_evidence(
        &self,
        file: ArtifactWorkspaceFile,
        cancel: CancellationToken,
    ) -> Result<ArtifactEvidence, GitWorkspaceError> {
        let file_id = file.id;
        let runner = self.artifact_runner();
        let result =
            tokio::task::spawn_blocking(move || build_artifact_evidence(&runner, &file, &cancel))
                .await
                .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))??;
        self.ensure_artifact_current(file_id)?;
        Ok(result)
    }

    pub(crate) async fn artifact_preview_bytes(
        &self,
        file: ArtifactWorkspaceFile,
        limit: usize,
        cancel: CancellationToken,
    ) -> Result<Vec<u8>, GitWorkspaceError> {
        let file_id = file.id;
        let runner = self.artifact_runner();
        let result =
            tokio::task::spawn_blocking(move || read_artifact_file(&runner, &file, limit, &cancel))
                .await
                .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))??;
        self.ensure_artifact_current(file_id)?;
        Ok(result)
    }

    pub(crate) async fn artifact_open_with<T, F>(
        &self,
        file: ArtifactWorkspaceFile,
        cancel: CancellationToken,
        operation: F,
    ) -> Result<T, GitWorkspaceError>
    where
        T: Send + 'static,
        F: FnOnce(&ArtifactOpenGuard, &CancellationToken) -> Result<T, GitWorkspaceError>
            + Send
            + 'static,
    {
        let file_id = file.id;
        let runner = self.artifact_runner();
        let result = tokio::task::spawn_blocking(move || {
            let guard = build_artifact_open_guard(&runner, &file, &cancel)?;
            guard.revalidate()?;
            if cancel.is_cancelled() {
                return Err(error(GitWorkspaceErrorCode::Cancelled));
            }
            let result = operation(&guard, &cancel);
            let postflight = guard.revalidate();
            match (result, postflight) {
                (_, Err(failure)) => Err(failure),
                (result, Ok(())) => result,
            }
        })
        .await
        .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))??;
        self.ensure_artifact_current(file_id)?;
        Ok(result)
    }

    fn ensure_artifact_current(&self, file_id: WorkspaceFileId) -> Result<(), GitWorkspaceError> {
        self.artifact_file_by_id(file_id).map(|_| ())
    }

    fn artifact_runner(&self) -> Runner {
        Runner::new(
            self.root.clone(),
            self.identity,
            #[cfg(test)]
            self.executable.clone(),
        )
    }

    #[cfg(test)]
    fn new_for_test(root: &Path, executable: PathBuf) -> Result<Self, GitWorkspaceError> {
        Self::new_inner(root, Some(executable))
    }
}

fn unique_file<'a>(mut files: impl Iterator<Item = &'a PrivateFile>) -> Option<&'a PrivateFile> {
    let candidate = files.next()?;
    files.next().is_none().then_some(candidate)
}

fn artifact_workspace_file(file: &PrivateFile) -> ArtifactWorkspaceFile {
    ArtifactWorkspaceFile {
        id: file.id,
        path: file.path.clone(),
        snapshot_identity: file.snapshot_identity.clone(),
        worktree_identity: file.worktree_identity,
    }
}

fn invalidate_current(state: &mut ServiceState) {
    state.generation = 0;
    state.identity = None;
    state.snapshot = None;
    state.files.clear();
}

fn same_snapshot_content(
    state: &ServiceState,
    candidate_identity: &SnapshotIdentity,
    candidate: &WorkspaceSnapshot,
    candidate_files: &[PrivateFile],
) -> bool {
    let Some(current) = state.snapshot.as_ref() else {
        return false;
    };
    if state.identity.as_deref() != Some(candidate_identity)
        || current.head != candidate.head
        || current.stats != candidate.stats
        || current.files.len() != candidate.files.len()
        || current.files.len() != candidate_files.len()
        || current.files.len() != state.files.len()
    {
        return false;
    }
    current
        .files
        .iter()
        .zip(&candidate.files)
        .zip(&state.files)
        .zip(candidate_files)
        .all(
            |(((current_public, candidate_public), current_private), candidate_private)| {
                current_public.label == candidate_public.label
                    && current_public.previous_label == candidate_public.previous_label
                    && current_public.staged == candidate_public.staged
                    && current_public.unstaged == candidate_public.unstaged
                    && current_public.additions == candidate_public.additions
                    && current_public.deletions == candidate_public.deletions
                    && current_public.language == candidate_public.language
                    && current_private.id == current_public.id
                    && current_private.path.as_bytes() == candidate_private.path.as_bytes()
                    && current_private
                        .previous_path
                        .as_ref()
                        .map(|path| path.as_bytes())
                        == candidate_private
                            .previous_path
                            .as_ref()
                            .map(|path| path.as_bytes())
                    && current_private.staged == candidate_private.staged
                    && current_private.unstaged == candidate_private.unstaged
                    && current_private.binary == candidate_private.binary
                    && current_private.metadata_only == candidate_private.metadata_only
                    && current_private.language == candidate_private.language
                    && current_private.worktree_identity == candidate_private.worktree_identity
                    && current_private.snapshot_identity.as_ref()
                        == candidate_private.snapshot_identity.as_ref()
            },
        )
}

fn assign_generation(
    snapshot: &mut WorkspaceSnapshot,
    files: &mut [PrivateFile],
    generation: u64,
    identity: RootIdentity,
    instance_nonce: u64,
) -> Result<(), GitWorkspaceError> {
    if snapshot.files.len() != files.len() {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    snapshot.generation = generation;
    for (slot, (public, private)) in snapshot.files.iter_mut().zip(files).enumerate() {
        let slot = u32::try_from(slot).map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        let id = WorkspaceFileId {
            generation,
            slot,
            seal: seal(
                identity,
                instance_nonce,
                generation,
                slot,
                private.path.as_bytes(),
            ),
        };
        public.id = id;
        private.id = id;
    }
    Ok(())
}

mod diff;
mod identity;
mod projection;
mod runner;
mod snapshot;

#[cfg(test)]
mod tests;

pub(crate) use diff::*;
pub(crate) use identity::*;
pub(crate) use projection::*;
pub(crate) use runner::*;
pub(crate) use snapshot::*;
