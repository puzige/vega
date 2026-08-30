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
pub use branch::{BranchSwitchPermit, BranchWorkspaceService};

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
    generation: u64,
    identity: Option<Arc<SnapshotIdentity>>,
    snapshot: Option<WorkspaceSnapshot>,
    files: Vec<PrivateFile>,
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
        state.next_generation = generation;
        state.generation = generation;
        state.identity = Some(identity);
        state.files = files;
        state.snapshot = Some(snapshot.clone());
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

struct Runner {
    root: PathBuf,
    identity: RootIdentity,
    #[cfg(test)]
    executable: Option<PathBuf>,
}

struct Output {
    stdout: Vec<u8>,
}

impl Runner {
    fn new(
        root: PathBuf,
        identity: RootIdentity,
        #[cfg(test)] executable: Option<PathBuf>,
    ) -> Self {
        Self {
            root,
            identity,
            #[cfg(test)]
            executable,
        }
    }

    fn run(
        &self,
        verb: &'static str,
        args: &[OsString],
        stdout_limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Output, GitWorkspaceError> {
        self.run_inner(verb, args, None, stdout_limit, cancel)
    }

    fn run_with_input(
        &self,
        verb: &'static str,
        args: &[OsString],
        input: Arc<[u8]>,
        stdout_limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Output, GitWorkspaceError> {
        self.run_inner(verb, args, Some(input), stdout_limit, cancel)
    }

    fn run_inner(
        &self,
        verb: &'static str,
        args: &[OsString],
        input: Option<Arc<[u8]>>,
        stdout_limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Output, GitWorkspaceError> {
        if !matches!(
            verb,
            "status"
                | "diff"
                | "rev-parse"
                | "for-each-ref"
                | "check-attr"
                | "ls-files"
                | "ls-tree"
                | "hash-object"
        ) {
            return Err(error(GitWorkspaceErrorCode::GitFailed));
        }
        self.verify_root()?;
        if cancel.is_cancelled() {
            return Err(error(GitWorkspaceErrorCode::Cancelled));
        }
        #[cfg(test)]
        let executable = self.executable.as_deref().unwrap_or_else(|| Path::new(GIT));
        #[cfg(not(test))]
        let executable = Path::new(GIT);
        let mut command = Command::new(executable);
        command.current_dir(&self.root);
        command.args(PREFIX).arg(verb).args(args);
        scrub_git_environment(&mut command);
        command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command
            .spawn()
            .map_err(|_| error(GitWorkspaceErrorCode::SpawnFailed))?;
        collect_child(
            &mut child,
            input,
            stdout_limit,
            STDERR_LIMIT,
            READ_TIMEOUT,
            cancel,
        )
    }

    fn run_trusted_switch(
        &self,
        branch: &OsStr,
        cancel: &CancellationToken,
    ) -> Result<Output, GitWorkspaceError> {
        #[cfg(test)]
        let executable = self.executable.as_deref().unwrap_or_else(|| Path::new(GIT));
        #[cfg(not(test))]
        let executable = Path::new(GIT);
        self.run_trusted_switch_with_executable(branch, cancel, executable)
    }

    fn run_trusted_switch_with_executable(
        &self,
        branch: &OsStr,
        cancel: &CancellationToken,
        executable: &Path,
    ) -> Result<Output, GitWorkspaceError> {
        self.verify_root()?;
        if cancel.is_cancelled() {
            return Err(error(GitWorkspaceErrorCode::Cancelled));
        }
        let mut command = Command::new(executable);
        command.current_dir(&self.root);
        command
            .args(PREFIX)
            .args(["-c", "core.hooksPath=/dev/null", "switch"])
            .args([
                OsStr::new("--no-guess"),
                OsStr::new("--no-overwrite-ignore"),
                OsStr::new("--no-recurse-submodules"),
            ])
            .arg(branch);
        scrub_git_environment(&mut command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command
            .spawn()
            .map_err(|_| error(GitWorkspaceErrorCode::SpawnFailed))?;
        collect_child(
            &mut child,
            None,
            MUTATION_STDOUT_LIMIT,
            STDERR_LIMIT,
            MUTATION_TIMEOUT,
            cancel,
        )
    }

    fn verify_root(&self) -> Result<(), GitWorkspaceError> {
        let canonical = fs::canonicalize(&self.root)
            .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
        let metadata = fs::metadata(&canonical)
            .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
        if canonical != self.root
            || metadata.dev() != self.identity.dev
            || metadata.ino() != self.identity.ino
        {
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
        Ok(())
    }
}

fn scrub_git_environment(command: &mut Command) {
    let explicit_git_keys: Vec<OsString> = command
        .get_envs()
        .filter(|(key, _)| key.as_bytes().starts_with(b"GIT_"))
        .map(|(key, _)| key.to_owned())
        .collect();
    for key in explicit_git_keys {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.as_os_str().as_bytes().starts_with(b"GIT_") {
            command.env_remove(key);
        }
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("LC_ALL", "C");
}

struct ReaderResult {
    stream: Stream,
    bytes: Vec<u8>,
    overflow: bool,
    failed: bool,
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

fn collect_child(
    child: &mut Child,
    input: Option<Arc<[u8]>>,
    stdout_limit: usize,
    stderr_limit: usize,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<Output, GitWorkspaceError> {
    let pgid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdin = input.as_ref().and_then(|_| child.stdin.take());
    let (stdout, stderr, stdin) = match (stdout, stderr, stdin, input.is_some()) {
        (Some(stdout), Some(stderr), Some(stdin), true) => (stdout, stderr, Some(stdin)),
        (Some(stdout), Some(stderr), None, false) => (stdout, stderr, None),
        (stdout, stderr, stdin, _) => {
            cleanup_partial_child(child, pgid, stdout, stderr, stdin);
            return Err(error(GitWorkspaceErrorCode::ProcessControlFailed));
        }
    };
    let overflowed = Arc::new(AtomicBool::new(false));
    let writer_done = Arc::new(AtomicBool::new(input.is_none()));
    let writer_failed = Arc::new(AtomicBool::new(false));
    if let Some(input) = input {
        let Some(mut stdin) = stdin else {
            cleanup_partial_child(child, pgid, None, None, None);
            return Err(error(GitWorkspaceErrorCode::ProcessControlFailed));
        };
        let done = writer_done.clone();
        let failed = writer_failed.clone();
        thread::spawn(move || {
            for chunk in input.chunks(IO_CHUNK) {
                if stdin.write_all(chunk).is_err() {
                    failed.store(true, Ordering::SeqCst);
                    break;
                }
            }
            drop(stdin);
            done.store(true, Ordering::SeqCst);
        });
    }
    let (sender, receiver) = mpsc::channel();
    spawn_reader(
        stdout,
        Stream::Stdout,
        stdout_limit,
        overflowed.clone(),
        sender.clone(),
    );
    spawn_reader(
        stderr,
        Stream::Stderr,
        stderr_limit,
        overflowed.clone(),
        sender,
    );

    let started = Instant::now();
    let mut status = None;
    let mut stop_code = None;
    while status.is_none() {
        if cancel.is_cancelled() {
            stop_code = Some(GitWorkspaceErrorCode::Cancelled);
            break;
        }
        if overflowed.load(Ordering::SeqCst) {
            stop_code = Some(GitWorkspaceErrorCode::OutputTooLarge);
            break;
        }
        if writer_failed.load(Ordering::SeqCst) {
            stop_code = Some(GitWorkspaceErrorCode::GitFailed);
            break;
        }
        if started.elapsed() >= timeout {
            stop_code = Some(GitWorkspaceErrorCode::TimedOut);
            break;
        }
        match child.try_wait() {
            Ok(current) => status = current,
            Err(_) => {
                stop_code = Some(GitWorkspaceErrorCode::ProcessControlFailed);
                break;
            }
        }
        if status.is_none() {
            thread::sleep(Duration::from_millis(5));
        }
    }
    let mut cleanup_failed = false;
    if stop_code.is_some() && terminate_group(child, pgid).is_err() {
        cleanup_failed = true;
    }

    let drain_started = Instant::now();
    let mut outputs = Vec::with_capacity(2);
    while outputs.len() < 2 && drain_started.elapsed() < DRAIN_GRACE {
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(output) => outputs.push(output),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if outputs.len() < 2 {
        stop_code.get_or_insert(GitWorkspaceErrorCode::ProcessControlFailed);
        if terminate_group(child, pgid).is_err() {
            cleanup_failed = true;
        }
        while outputs.len() < 2 {
            match receiver.recv_timeout(DRAIN_GRACE) {
                Ok(output) => outputs.push(output),
                Err(_) => {
                    cleanup_failed = true;
                    break;
                }
            }
        }
    }
    if status.is_none() {
        let deadline = Instant::now();
        while status.is_none() && deadline.elapsed() < DRAIN_GRACE {
            status = match child.try_wait() {
                Ok(status) => status,
                Err(_) => {
                    cleanup_failed = true;
                    let _ = terminate_group(child, pgid);
                    break;
                }
            };
            if status.is_none() {
                thread::sleep(Duration::from_millis(5));
            }
        }
        if status.is_none() {
            cleanup_failed = true;
            let _ = terminate_group(child, pgid);
        }
    }
    let writer_started = Instant::now();
    while !writer_done.load(Ordering::SeqCst) && writer_started.elapsed() < DRAIN_GRACE {
        thread::sleep(Duration::from_millis(5));
    }
    if !writer_done.load(Ordering::SeqCst) || writer_failed.load(Ordering::SeqCst) {
        stop_code.get_or_insert(GitWorkspaceErrorCode::GitFailed);
        if terminate_group(child, pgid).is_err() {
            cleanup_failed = true;
        }
    }
    if cleanup_failed {
        return Err(error(GitWorkspaceErrorCode::ProcessControlFailed));
    }
    if let Some(code) = stop_code {
        return Err(error(code));
    }
    if outputs.iter().any(|output| output.overflow) {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    if outputs.iter().any(|output| output.failed) {
        return Err(error(GitWorkspaceErrorCode::GitFailed));
    }
    let status = status.ok_or_else(|| error(GitWorkspaceErrorCode::ProcessControlFailed))?;
    if !status.success() {
        return Err(classify_git_failure(status, &outputs));
    }
    let stdout = outputs
        .into_iter()
        .find(|output| matches!(output.stream, Stream::Stdout))
        .map(|output| output.bytes)
        .ok_or_else(|| error(GitWorkspaceErrorCode::GitFailed))?;
    Ok(Output { stdout })
}

fn cleanup_partial_child(
    child: &mut Child,
    pgid: u32,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    stdin: Option<ChildStdin>,
) {
    drop(stdin);
    let overflowed = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    let mut readers = 0;
    if let Some(stdout) = stdout {
        readers += 1;
        spawn_reader(
            stdout,
            Stream::Stdout,
            0,
            overflowed.clone(),
            sender.clone(),
        );
    }
    if let Some(stderr) = stderr {
        readers += 1;
        spawn_reader(stderr, Stream::Stderr, 0, overflowed, sender.clone());
    }
    drop(sender);
    let _ = terminate_group(child, pgid);
    for _ in 0..readers {
        if receiver.recv_timeout(DRAIN_GRACE).is_err() {
            break;
        }
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    stream: Stream,
    limit: usize,
    overflowed: Arc<AtomicBool>,
    sender: mpsc::Sender<ReaderResult>,
) {
    thread::spawn(move || {
        let mut retained = Vec::with_capacity(limit.min(IO_CHUNK));
        let mut chunk = [0_u8; IO_CHUNK];
        let mut overflow = false;
        let mut failed = false;
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    if retained.len().saturating_add(read) <= limit {
                        retained.extend_from_slice(&chunk[..read]);
                    } else {
                        overflow = true;
                        overflowed.store(true, Ordering::SeqCst);
                    }
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        let _ = sender.send(ReaderResult {
            stream,
            bytes: retained,
            overflow,
            failed,
        });
    });
}

pub(crate) fn terminate_group(child: &mut Child, pgid: u32) -> Result<(), GitWorkspaceError> {
    let mut control_failed = !signal_group_checked(pgid, "-TERM");
    thread::sleep(TERM_GRACE);
    if !signal_group_checked(pgid, "-KILL") {
        control_failed = true;
        if child.kill().is_err() {
            control_failed = true;
        }
    }
    let mut reaped = bounded_reap(child, DRAIN_GRACE);
    if !reaped {
        control_failed = true;
        let _ = child.kill();
        reaped = bounded_reap(child, DRAIN_GRACE);
    }
    if control_failed || !reaped {
        Err(error(GitWorkspaceErrorCode::ProcessControlFailed))
    } else {
        Ok(())
    }
}

fn signal_group(pgid: u32, signal: &str) -> std::io::Result<ExitStatus> {
    Command::new(KILL)
        .args([signal, "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}

fn signal_group_checked(pgid: u32, signal: &str) -> bool {
    match signal_group(pgid, signal) {
        Ok(status) if status.success() => true,
        Ok(_) => !group_exists(pgid),
        Err(_) => false,
    }
}

fn group_exists(pgid: u32) -> bool {
    Command::new(KILL)
        .args(["-0", "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn bounded_reap(child: &mut Child, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => return false,
        }
    }
    false
}

fn classify_git_failure(status: ExitStatus, outputs: &[ReaderResult]) -> GitWorkspaceError {
    let not_repository = status.code() == Some(128)
        && outputs.iter().any(|output| {
            matches!(output.stream, Stream::Stderr)
                && output
                    .bytes
                    .windows(b"not a git repository".len())
                    .any(|window| window == b"not a git repository")
        });
    error(if not_repository {
        GitWorkspaceErrorCode::NotRepository
    } else {
        GitWorkspaceErrorCode::GitFailed
    })
}

fn error(code: GitWorkspaceErrorCode) -> GitWorkspaceError {
    GitWorkspaceError::new(code)
}

#[derive(Clone)]
struct ParsedFile {
    path: Vec<u8>,
    previous_path: Option<Vec<u8>>,
    staged: WorkspaceChangeKind,
    unstaged: WorkspaceChangeKind,
    additions: WorkspaceLineCount,
    deletions: WorkspaceLineCount,
    metadata_only: bool,
}

struct ParsedStatus {
    head: WorkspaceHead,
    files: BTreeMap<Vec<u8>, ParsedFile>,
}

fn build_snapshot(
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

fn path_multiplicity<'a>(paths: impl Iterator<Item = &'a [u8]>) -> BTreeMap<Vec<u8>, usize> {
    let mut counts = BTreeMap::new();
    for path in paths {
        *counts.entry(path.to_vec()).or_insert(0) += 1;
    }
    counts
}

fn status_args() -> Vec<OsString> {
    vec![
        OsString::from("--porcelain=v2"),
        OsString::from("-z"),
        OsString::from("--branch"),
        OsString::from("--renames"),
        OsString::from("--untracked-files=all"),
    ]
}

struct FilterIdentity {
    paths: Arc<[u8]>,
    attrs: Vec<u8>,
}

fn capture_filter_identity(
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

fn parse_nul_paths(bytes: &[u8]) -> Result<Vec<Vec<u8>>, GitWorkspaceError> {
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

fn validate_filter_attrs(paths: &[Vec<u8>], bytes: &[u8]) -> Result<(), GitWorkspaceError> {
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

fn verify_filter_bytes_with_retained(
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

fn raw_args(cached: bool) -> Vec<OsString> {
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

fn numstat_args(cached: bool) -> Vec<OsString> {
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

fn verify_snapshot_identity(
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

fn snapshot_identity_retained(expected: &SnapshotIdentity) -> Result<usize, GitWorkspaceError> {
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

fn ensure_candidate_retained(
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

fn charge_logical(retained: &mut usize, bytes: usize) -> Result<(), GitWorkspaceError> {
    *retained = retained
        .checked_add(bytes)
        .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
    Ok(())
}

fn capture_worktree_identities(
    root: &Path,
    files: &BTreeMap<Vec<u8>, ParsedFile>,
) -> Result<HashMap<Vec<u8>, Option<FileIdentity>>, GitWorkspaceError> {
    let mut identities = HashMap::with_capacity(files.len());
    for path in files.keys() {
        identities.insert(path.clone(), read_worktree_identity(root, path)?);
    }
    Ok(identities)
}

fn read_worktree_identity(
    root: &Path,
    path: &[u8],
) -> Result<Option<FileIdentity>, GitWorkspaceError> {
    match fs::symlink_metadata(root.join(OsString::from_vec(path.to_vec()))) {
        Ok(metadata) => Ok(Some(file_identity(&metadata))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(error(GitWorkspaceErrorCode::ChangedDuringRead)),
    }
}

fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
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
        size: metadata.size(),
        mtime: metadata.mtime(),
        mtime_ns: metadata.mtime_nsec(),
        ctime: metadata.ctime(),
        ctime_ns: metadata.ctime_nsec(),
    }
}

fn parse_status(bytes: &[u8]) -> Result<ParsedStatus, GitWorkspaceError> {
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

fn split_prefix_fields(record: &[u8], spaces: usize) -> Result<Vec<&[u8]>, GitWorkspaceError> {
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

fn validate_ordinary_fields(fields: &[&[u8]], renamed: bool) -> Result<(), GitWorkspaceError> {
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

fn valid_sub(value: &[u8]) -> bool {
    value == b"N..."
        || (value.len() == 4
            && value[0] == b'S'
            && matches!(value[1], b'.' | b'C')
            && matches!(value[2], b'.' | b'M')
            && matches!(value[3], b'.' | b'U'))
}

fn consistent_oids(values: &[&[u8]]) -> bool {
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

fn parse_xy(value: &[u8]) -> Result<(WorkspaceChangeKind, WorkspaceChangeKind), GitWorkspaceError> {
    if value.len() != 2 {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok((parse_change(value[0])?, parse_change(value[1])?))
}

fn parse_change(value: u8) -> Result<WorkspaceChangeKind, GitWorkspaceError> {
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

fn insert_status(
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

fn validate_relative_path(path: &[u8]) -> Result<(), GitWorkspaceError> {
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

struct RawEntry {
    path: Vec<u8>,
    previous_path: Option<Vec<u8>>,
    kind: WorkspaceChangeKind,
    metadata_only: bool,
}

fn validate_raw(bytes: &[u8]) -> Result<Vec<RawEntry>, GitWorkspaceError> {
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

fn cross_check_raw(
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
    if entry_paths.len() != expected.len() {
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

fn malformed() -> GitWorkspaceError {
    error(GitWorkspaceErrorCode::MalformedOutput)
}

fn valid_mode(mode: &[u8]) -> bool {
    matches!(
        mode,
        b"000000" | b"100644" | b"100755" | b"120000" | b"160000"
    )
}

fn special_modes(modes: &[&[u8]]) -> bool {
    modes
        .iter()
        .any(|mode| matches!(*mode, b"120000" | b"160000"))
}

fn valid_oid(oid: &[u8]) -> bool {
    matches!(oid.len(), 40 | 64)
        && oid
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_raw_status(status: &[u8]) -> bool {
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

fn merge_numstat(
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

fn parse_count(bytes: &[u8]) -> Result<WorkspaceLineCount, GitWorkspaceError> {
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

fn merge_count(
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

fn fold_count(total: &mut Option<u64>, value: WorkspaceLineCount) -> Result<(), GitWorkspaceError> {
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

fn build_projection(
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

fn consume_projection_bytes(remaining: &mut usize, bytes: usize) -> Result<(), GitWorkspaceError> {
    *remaining = remaining
        .checked_sub(bytes)
        .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
    Ok(())
}

fn hash_worktree_no_filters(
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

fn build_artifact_evidence(
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

fn read_artifact_file(
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

fn build_artifact_open_guard(
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

fn fence_artifact_file(
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

fn project_untracked(
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

fn parse_patch(
    layer: DiffLayer,
    bytes: &[u8],
    remaining_rows: &mut usize,
) -> Result<DiffSection, GitWorkspaceError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?;
    if text.contains("Binary files ") || text.contains("GIT binary patch") {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let mut hunks = Vec::new();
    let mut current: Option<DiffHunk> = None;
    let mut old_line = 0_u32;
    let mut new_line = 0_u32;
    for line in logical_lines(text) {
        if line.starts_with("@@ ") {
            if line.len() > PATCH_LINE_LIMIT {
                return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
            }
            if let Some(hunk) = current.take() {
                validate_hunk(&hunk)?;
                hunks.push(hunk);
            }
            let (old_start, old_count, new_start, new_count, heading_suffix) =
                parse_hunk_header(line)?;
            old_line = old_start;
            new_line = new_start;
            current = Some(DiffHunk {
                old_start,
                old_count,
                new_start,
                new_count,
                heading_suffix,
                missing_trailing_newline: false,
                rows: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = current.as_mut() else {
            continue;
        };
        let Some((&prefix, body)) = line.as_bytes().split_first() else {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        };
        if body.len() > PATCH_LINE_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        if prefix == b'\\' {
            if line != "\\ No newline at end of file" {
                return Err(error(GitWorkspaceErrorCode::MalformedOutput));
            }
            hunk.missing_trailing_newline = true;
            continue;
        }
        if *remaining_rows == 0 {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        *remaining_rows -= 1;
        let body = std::str::from_utf8(body)
            .map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?
            .to_owned();
        let row = match prefix {
            b' ' => {
                let row = DiffRow {
                    kind: DiffRowKind::Context,
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                    text: body,
                };
                old_line = old_line
                    .checked_add(1)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                new_line = new_line
                    .checked_add(1)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                row
            }
            b'-' => {
                let row = DiffRow {
                    kind: DiffRowKind::Deletion,
                    old_line: Some(old_line),
                    new_line: None,
                    text: body,
                };
                old_line = old_line
                    .checked_add(1)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                row
            }
            b'+' => {
                let row = DiffRow {
                    kind: DiffRowKind::Addition,
                    old_line: None,
                    new_line: Some(new_line),
                    text: body,
                };
                new_line = new_line
                    .checked_add(1)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                row
            }
            _ => return Err(error(GitWorkspaceErrorCode::MalformedOutput)),
        };
        hunk.rows.push(row);
    }
    if let Some(hunk) = current {
        validate_hunk(&hunk)?;
        hunks.push(hunk);
    }
    Ok(DiffSection { layer, hunks })
}

fn parse_hunk_header(
    line: &str,
) -> Result<(u32, u32, u32, u32, Option<String>), GitWorkspaceError> {
    let end = line[3..]
        .find(" @@")
        .map(|index| index + 3)
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let mut ranges = line[3..end].split(' ');
    let old = ranges
        .next()
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let new = ranges
        .next()
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    if ranges.next().is_some() {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let (old_start, old_count) = parse_range(old, '-')?;
    let (new_start, new_count) = parse_range(new, '+')?;
    let suffix = line[end + 3..]
        .strip_prefix(' ')
        .unwrap_or(&line[end + 3..]);
    let suffix = (!suffix.is_empty()).then(|| suffix.to_owned());
    Ok((old_start, old_count, new_start, new_count, suffix))
}

fn parse_range(value: &str, prefix: char) -> Result<(u32, u32), GitWorkspaceError> {
    let value = value
        .strip_prefix(prefix)
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    let start = start
        .parse()
        .map_err(|_| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let count = count
        .parse()
        .map_err(|_| error(GitWorkspaceErrorCode::MalformedOutput))?;
    Ok((start, count))
}

fn validate_hunk(hunk: &DiffHunk) -> Result<(), GitWorkspaceError> {
    let old = hunk
        .rows
        .iter()
        .filter(|row| row.kind != DiffRowKind::Addition)
        .count();
    let new = hunk
        .rows
        .iter()
        .filter(|row| row.kind != DiffRowKind::Deletion)
        .count();
    if old != hunk.old_count as usize || new != hunk.new_count as usize {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok(())
}

fn logical_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_terminator('\n')
}

fn language_for(path: &[u8]) -> DiffLanguage {
    let name = path.rsplit(|byte| *byte == b'/').next().unwrap_or(path);
    let extension = name
        .iter()
        .rposition(|byte| *byte == b'.')
        .map(|position| &name[position + 1..]);
    match extension {
        Some(b"rs") => DiffLanguage::Rust,
        Some(b"ts") => DiffLanguage::TypeScript,
        Some(b"tsx") => DiffLanguage::Tsx,
        Some(b"js" | b"jsx" | b"mjs" | b"cjs") => DiffLanguage::JavaScript,
        Some(b"py") => DiffLanguage::Python,
        _ => DiffLanguage::Plain,
    }
}

fn escape_path(path: &[u8]) -> String {
    escape_bytes(path)
}

fn escape_ref(reference: &[u8]) -> String {
    escape_bytes(reference)
}

fn escape_bytes(bytes: &[u8]) -> String {
    if let Ok(value) = std::str::from_utf8(bytes) {
        let mut escaped = String::new();
        for character in value.chars() {
            if character.is_control() || is_bidi_control(character) {
                for byte in character.to_string().bytes() {
                    escaped.push_str(&format!("\\x{byte:02x}"));
                }
            } else if character == '\\' {
                escaped.push_str("\\\\");
            } else {
                escaped.push(character);
            }
        }
        return escaped;
    }
    let mut escaped = String::new();
    for byte in bytes {
        if (0x20..=0x7e).contains(byte) && *byte != b'\\' {
            escaped.push(char::from(*byte));
        } else if *byte == b'\\' {
            escaped.push_str("\\\\");
        } else {
            escaped.push_str(&format!("\\x{byte:02x}"));
        }
    }
    escaped
}

fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

fn seal(
    identity: RootIdentity,
    instance_nonce: u64,
    generation: u64,
    slot: u32,
    path: &[u8],
) -> u64 {
    let mut value = identity.dev
        ^ identity.ino.rotate_left(17)
        ^ instance_nonce.rotate_left(23)
        ^ generation.rotate_left(31);
    value ^= u64::from(slot).rotate_left(7);
    for byte in path {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100_0000_01b3);
    }
    value
}

fn trim_one_newline(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use tempfile::{TempDir, tempdir};

    struct Repo {
        dir: TempDir,
    }

    impl Repo {
        fn new() -> Self {
            let dir = tempdir().unwrap();
            git(dir.path(), &["init", "-q", "--initial-branch=main"]);
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
            let path = self.path().join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, body).unwrap();
        }

        fn commit_all(&self) {
            git(self.path(), &["add", "-A"]);
            git(self.path(), &["commit", "-q", "-m", "fixture"]);
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let status = git_command(root, args).status().unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn git_command(root: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(GIT);
        command
            .current_dir(root)
            .args(args)
            // Exercise the same repository-targeting variables inherited from
            // Git hooks on every fixture command. The scrub below must win.
            .env("GIT_DIR", root.join(".vega-poison-git-dir"))
            .env("GIT_WORK_TREE", root.join(".vega-poison-work-tree"))
            .env("GIT_INDEX_FILE", root.join(".vega-poison-index"));
        scrub_git_environment(&mut command);
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        command
    }

    #[test]
    fn git_workspace_fixture_git_scrubs_repository_targeting_environment() {
        let repo = Repo::new();
        repo.write("isolated.txt", b"isolated\n");
        repo.commit_all();

        assert!(repo.path().join(".git").is_dir());
        for path in [
            ".vega-poison-git-dir",
            ".vega-poison-work-tree",
            ".vega-poison-index",
        ] {
            assert!(!repo.path().join(path).exists(), "poison target {path}");
        }
    }

    #[tokio::test]
    async fn git_workspace_clean_staged_unstaged_untracked_and_structured_projection() {
        let repo = Repo::new();
        repo.write("src/lib.rs", b"one\ntwo\n");
        repo.commit_all();
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let clean = service.refresh(CancellationToken::new()).await.unwrap();
        assert!(clean.files.is_empty());

        repo.write("src/lib.rs", b"ONE\ntwo\n");
        git(repo.path(), &["add", "src/lib.rs"]);
        repo.write("src/lib.rs", b"ONE\nTWO\n");
        repo.write("new.ts", b"export const value = 1;\n");
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        assert_eq!(snapshot.files.len(), 2);
        let tracked = snapshot
            .files
            .iter()
            .find(|file| file.label == "src/lib.rs")
            .unwrap();
        assert_eq!(tracked.staged, WorkspaceChangeKind::Modified);
        assert_eq!(tracked.unstaged, WorkspaceChangeKind::Modified);
        assert_eq!(tracked.language, DiffLanguage::Rust);
        let projection = service
            .diff(tracked.id, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(projection.sections.len(), 2);
        assert!(
            projection
                .sections
                .iter()
                .all(|section| !section.hunks.is_empty())
        );

        let untracked = snapshot
            .files
            .iter()
            .find(|file| file.label == "new.ts")
            .unwrap();
        assert_eq!(untracked.unstaged, WorkspaceChangeKind::Untracked);
        assert_eq!(untracked.additions, WorkspaceLineCount::Unknown);
        let projection = service
            .diff(untracked.id, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(projection.sections[0].layer, DiffLayer::Untracked);
        assert_eq!(projection.sections[0].hunks[0].rows.len(), 1);
    }

    #[tokio::test]
    async fn git_workspace_staged_and_unstaged_sections_share_row_budget() {
        let repo = Repo::new();
        repo.write("large.txt", "a\n".repeat(6_000).as_bytes());
        repo.commit_all();
        repo.write("large.txt", "b\n".repeat(6_000).as_bytes());
        git(repo.path(), &["add", "large.txt"]);
        repo.write("large.txt", "c\n".repeat(6_000).as_bytes());
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        assert_eq!(
            service
                .diff(snapshot.files[0].id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::OutputTooLarge
        );
    }

    #[tokio::test]
    async fn git_workspace_delete_rename_space_and_literal_magic_names() {
        let repo = Repo::new();
        repo.write("delete.txt", b"delete-only\n");
        for path in ["old name.txt", ":(glob)**", ":!safe"] {
            repo.write(path, b"body\n");
        }
        repo.commit_all();
        fs::remove_file(repo.path().join("delete.txt")).unwrap();
        fs::rename(
            repo.path().join("old name.txt"),
            repo.path().join("new name.txt"),
        )
        .unwrap();
        repo.write(":(glob)**", b"changed glob\n");
        repo.write(":!safe", b"changed exclude\n");
        git(repo.path(), &["add", "-A"]);
        repo.write("new name.txt", b"body\nafter-rename\n");
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        assert!(
            snapshot
                .files
                .iter()
                .any(|file| file.staged == WorkspaceChangeKind::Deleted)
        );
        let renamed = snapshot
            .files
            .iter()
            .find(|file| file.label == "new name.txt")
            .unwrap();
        assert_eq!(renamed.previous_label.as_deref(), Some("old name.txt"));
        assert_eq!(renamed.staged, WorkspaceChangeKind::Renamed);
        assert_eq!(renamed.unstaged, WorkspaceChangeKind::Modified);
        assert_eq!(
            service
                .diff(renamed.id, CancellationToken::new())
                .await
                .unwrap()
                .sections
                .len(),
            2
        );
        for name in [":(glob)**", ":!safe"] {
            let file = snapshot
                .files
                .iter()
                .find(|file| file.label == name)
                .unwrap();
            let projection = service
                .diff(file.id, CancellationToken::new())
                .await
                .unwrap();
            assert!(!projection.sections[0].hunks.is_empty());
        }
    }

    #[test]
    fn git_workspace_unstaged_type_two_record_is_strictly_parsed() {
        let oid = "a".repeat(40);
        let status = format!(
            "# branch.oid {oid}\0# branch.head main\0\
             2 .R N... 100644 100644 100644 {oid} {oid} R100 after.txt\0before.txt\0"
        );
        let parsed = parse_status(status.as_bytes()).unwrap();
        let renamed = parsed.files.get(b"after.txt".as_slice()).unwrap();
        assert_eq!(renamed.staged, WorkspaceChangeKind::Unchanged);
        assert_eq!(renamed.unstaged, WorkspaceChangeKind::Renamed);
        assert_eq!(
            renamed.previous_path.as_deref(),
            Some(b"before.txt".as_slice())
        );
    }

    #[tokio::test]
    async fn git_workspace_binary_symlink_and_special_are_metadata_only() {
        let repo = Repo::new();
        repo.write("binary.bin", b"a\0b");
        symlink("binary.bin", repo.path().join("link")).unwrap();
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        for label in ["binary.bin", "link"] {
            let file = snapshot
                .files
                .iter()
                .find(|file| file.label == label)
                .unwrap();
            assert_eq!(
                service
                    .diff(file.id, CancellationToken::new())
                    .await
                    .unwrap_err()
                    .code(),
                GitWorkspaceErrorCode::MetadataOnly
            );
        }
    }

    #[tokio::test]
    async fn git_workspace_tracked_staged_and_unstaged_symlinks_are_metadata_only() {
        let repo = Repo::new();
        repo.write("target.txt", b"target\n");
        repo.write("staged-link", b"regular staged\n");
        repo.write("unstaged-link", b"regular unstaged\n");
        repo.commit_all();
        fs::remove_file(repo.path().join("staged-link")).unwrap();
        fs::remove_file(repo.path().join("unstaged-link")).unwrap();
        symlink("target.txt", repo.path().join("staged-link")).unwrap();
        symlink("target.txt", repo.path().join("unstaged-link")).unwrap();
        git(repo.path(), &["add", "staged-link"]);

        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        for label in ["staged-link", "unstaged-link"] {
            let file = snapshot
                .files
                .iter()
                .find(|file| file.label == label)
                .unwrap();
            assert_eq!(
                service
                    .diff(file.id, CancellationToken::new())
                    .await
                    .unwrap_err()
                    .code(),
                GitWorkspaceErrorCode::MetadataOnly
            );
        }
    }

    #[tokio::test]
    async fn git_workspace_real_conflict_is_unmerged_metadata_only_without_filter_execution() {
        let repo = Repo::new();
        repo.write("conflict.txt", b"base\n");
        repo.commit_all();
        git(repo.path(), &["branch", "side"]);
        git(repo.path(), &["checkout", "-q", "side"]);
        repo.write("conflict.txt", b"side\n");
        repo.commit_all();
        git(repo.path(), &["checkout", "-q", "main"]);
        repo.write("conflict.txt", b"main\n");
        repo.commit_all();
        let merge = git_command(repo.path(), &["merge", "--no-edit", "side"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(!merge.success());
        let marker = repo.path().join("filter-ran");
        git(
            repo.path(),
            &[
                "config",
                "filter.unused.clean",
                &format!("printf ran > '{}'; cat", marker.display()),
            ],
        );
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        let conflicted = snapshot
            .files
            .iter()
            .find(|file| file.label == "conflict.txt")
            .unwrap();
        assert_eq!(conflicted.staged, WorkspaceChangeKind::Unmerged);
        assert_eq!(conflicted.unstaged, WorkspaceChangeKind::Unmerged);
        assert_eq!(conflicted.additions, WorkspaceLineCount::Unknown);
        assert_eq!(conflicted.deletions, WorkspaceLineCount::Unknown);
        assert_eq!(
            service
                .diff(conflicted.id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::MetadataOnly
        );
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn git_workspace_unborn_detached_nonrepo_and_stale_ids_are_typed() {
        let repo = Repo::new();
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let unborn = service.refresh(CancellationToken::new()).await.unwrap();
        assert!(matches!(unborn.head, WorkspaceHead::Unborn { .. }));
        repo.write("a.py", b"print(1)\n");
        let first = service.refresh(CancellationToken::new()).await.unwrap();
        let stale = first.files[0].id;
        let other_service = GitWorkspaceService::new(repo.path()).unwrap();
        other_service
            .refresh(CancellationToken::new())
            .await
            .unwrap();
        repo.write("other.txt", b"other\n");
        other_service
            .refresh(CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            other_service
                .diff(stale, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::UnknownFile
        );
        fs::remove_file(repo.path().join("other.txt")).unwrap();
        repo.write("b.txt", b"b\n");
        service.refresh(CancellationToken::new()).await.unwrap();
        assert_eq!(
            service
                .diff(stale, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
        repo.commit_all();
        git(repo.path(), &["checkout", "--detach", "-q"]);
        assert!(matches!(
            service
                .refresh(CancellationToken::new())
                .await
                .unwrap()
                .head,
            WorkspaceHead::Detached
        ));

        let nonrepo = tempdir().unwrap();
        let service = GitWorkspaceService::new(nonrepo.path()).unwrap();
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::NotRepository
        );
    }

    #[tokio::test]
    async fn git_workspace_identical_refresh_retains_generation_and_opaque_ids() {
        let repo = Repo::new();
        repo.write("stable.rs", b"fn stable() {}\n");
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let first = service.refresh(CancellationToken::new()).await.unwrap();
        let first_id = first.files[0].id;

        let second = service.refresh(CancellationToken::new()).await.unwrap();
        assert_eq!(second.generation, first.generation);
        assert_eq!(second.files[0].id, first_id);
        assert_eq!(second, first);
        assert_eq!(
            service
                .diff(first_id, CancellationToken::new())
                .await
                .unwrap()
                .file_id(),
            first_id
        );

        repo.write("stable.rs", b"fn changed() {}\n");
        let changed = service.refresh(CancellationToken::new()).await.unwrap();
        assert_ne!(changed.generation, first.generation);
        assert_ne!(changed.files[0].id, first_id);
        assert_eq!(
            service
                .diff(first_id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
    }

    #[tokio::test]
    async fn git_workspace_canonical_vec_slot_and_seal_are_lookup_authority() {
        let repo = Repo::new();
        repo.write("z-last.txt", b"z\n");
        repo.write("a-first.txt", b"a\n");
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        assert_eq!(snapshot.files.len(), 2);
        {
            let state = service
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            assert_eq!(state.files.len(), snapshot.files.len());
            for (slot, (public, private)) in snapshot.files.iter().zip(&state.files).enumerate() {
                assert_eq!(usize::try_from(public.id.slot).unwrap(), slot);
                assert_eq!(private.id, public.id);
                assert_eq!(escape_path(private.path.as_bytes()), public.label);
            }
        }
        let valid = snapshot.files[0].id;
        assert_eq!(
            service
                .diff(valid, CancellationToken::new())
                .await
                .unwrap()
                .file_id(),
            valid
        );
        let forged = WorkspaceFileId {
            generation: valid.generation,
            slot: valid.slot,
            seal: valid.seal ^ 1,
        };
        assert_eq!(
            service
                .diff(forged, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::UnknownFile
        );
    }

    #[tokio::test]
    async fn git_workspace_clean_unchanged_refresh_retains_generation() {
        let repo = Repo::new();
        repo.write("clean.txt", b"clean\n");
        repo.commit_all();
        let service = GitWorkspaceService::new(repo.path()).unwrap();

        let first = service.refresh(CancellationToken::new()).await.unwrap();
        let second = service.refresh(CancellationToken::new()).await.unwrap();

        assert!(first.files.is_empty());
        assert_eq!(second, first);
    }

    #[tokio::test]
    async fn git_workspace_clean_head_only_change_rotates_generation() {
        let repo = Repo::new();
        repo.write("clean.txt", b"clean\n");
        repo.commit_all();
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let before = service.refresh(CancellationToken::new()).await.unwrap();

        git(
            repo.path(),
            &["commit", "-q", "--allow-empty", "-m", "head"],
        );
        let after = service.refresh(CancellationToken::new()).await.unwrap();

        assert!(before.files.is_empty());
        assert!(after.files.is_empty());
        assert_eq!(after.head, before.head);
        assert_eq!(after.stats, before.stats);
        assert_ne!(after.generation, before.generation);
    }

    #[tokio::test]
    async fn git_workspace_clean_info_attributes_change_rotates_generation() {
        let repo = Repo::new();
        repo.write("clean.txt", b"clean\n");
        repo.commit_all();
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let before = service.refresh(CancellationToken::new()).await.unwrap();

        fs::write(
            repo.path().join(".git/info/attributes"),
            b"clean.txt linguist-language=Rust\n",
        )
        .unwrap();
        let after = service.refresh(CancellationToken::new()).await.unwrap();

        assert!(before.files.is_empty());
        assert!(after.files.is_empty());
        assert_eq!(after.head, before.head);
        assert_eq!(after.stats, before.stats);
        assert_ne!(after.generation, before.generation);
    }

    #[tokio::test]
    async fn git_workspace_private_content_head_and_raw_rename_rotate_ids() {
        let repo = Repo::new();
        repo.write("tracked.txt", b"base\n");
        repo.commit_all();
        repo.write("tracked.txt", b"aaaa\n");
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let content_a = service.refresh(CancellationToken::new()).await.unwrap();
        let content_a_id = content_a.files[0].id;

        // Same path, size, classification and line statistics: ctime/private
        // file identity is still part of the equality authority.
        repo.write("tracked.txt", b"bbbb\n");
        let content_b = service.refresh(CancellationToken::new()).await.unwrap();
        assert_ne!(content_b.generation, content_a.generation);
        assert_eq!(
            service
                .diff(content_a_id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );

        // An empty commit changes only the captured HEAD while the safe file
        // projection remains equal.
        let before_head = content_b;
        git(
            repo.path(),
            &["commit", "-q", "--allow-empty", "-m", "head"],
        );
        let after_head = service.refresh(CancellationToken::new()).await.unwrap();
        assert_ne!(after_head.generation, before_head.generation);
        assert_eq!(after_head.files[0].label, before_head.files[0].label);
        assert_eq!(after_head.files[0].staged, before_head.files[0].staged);
        assert_eq!(after_head.files[0].unstaged, before_head.files[0].unstaged);
        assert_eq!(after_head.stats, before_head.stats);

        let old_path_id = after_head.files[0].id;
        fs::rename(
            repo.path().join("tracked.txt"),
            repo.path().join("renamed.txt"),
        )
        .unwrap();
        let renamed = service.refresh(CancellationToken::new()).await.unwrap();
        assert_ne!(renamed.generation, after_head.generation);
        assert!(renamed.files.iter().any(|file| file.label == "renamed.txt"));
        assert_eq!(
            service
                .diff(old_path_id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
    }

    #[tokio::test]
    async fn git_workspace_aba_allocates_fresh_generation_without_id_revival() {
        let repo = Repo::new();
        repo.write("aba.txt", b"state-a\n");
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let first_a = service.refresh(CancellationToken::new()).await.unwrap();
        let first_id = first_a.files[0].id;

        repo.write("aba.txt", b"state-b\n");
        let state_b = service.refresh(CancellationToken::new()).await.unwrap();
        repo.write("aba.txt", b"state-a\n");
        let second_a = service.refresh(CancellationToken::new()).await.unwrap();

        assert_ne!(state_b.generation, first_a.generation);
        assert_ne!(second_a.generation, state_b.generation);
        assert_ne!(second_a.generation, first_a.generation);
        assert_ne!(second_a.files[0].id, first_id);
        assert_eq!(
            service
                .diff(first_id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
    }

    #[tokio::test]
    async fn git_workspace_latest_failure_invalidates_ids_and_next_success_reseals() {
        let repo = Repo::new();
        repo.write("failure.txt", b"stable\n");
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let before = service.refresh(CancellationToken::new()).await.unwrap();
        let old_id = before.files[0].id;

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            service.refresh(cancelled).await.unwrap_err().code(),
            GitWorkspaceErrorCode::Cancelled
        );
        assert_eq!(
            service
                .diff(old_id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );

        let after = service.refresh(CancellationToken::new()).await.unwrap();
        assert_ne!(after.generation, before.generation);
        assert_ne!(after.files[0].id, old_id);
    }

    #[tokio::test]
    async fn git_workspace_generation_allocation_failure_invalidates_current() {
        let repo = Repo::new();
        repo.write("overflow.txt", b"before\n");
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let before = service.refresh(CancellationToken::new()).await.unwrap();
        let old_id = before.files[0].id;
        service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .next_generation = u64::MAX;
        repo.write("overflow.txt", b"after!\n");

        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::OutputTooLarge
        );
        assert_eq!(
            service
                .diff(old_id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
    }

    #[tokio::test]
    async fn git_workspace_escapes_control_bidi_and_non_utf8_paths_without_round_trip() {
        let repo = Repo::new();
        for bytes in [
            b"tab\tname.txt".to_vec(),
            b"line\nname.txt".to_vec(),
            "bidi\u{202e}name.txt".as_bytes().to_vec(),
        ] {
            fs::write(repo.path().join(OsString::from_vec(bytes)), b"body\n").unwrap();
        }
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        assert_eq!(snapshot.files.len(), 3);
        let labels: Vec<&str> = snapshot
            .files
            .iter()
            .map(|file| file.label.as_str())
            .collect();
        assert!(labels.iter().any(|label| label.contains("\\x09")));
        assert!(labels.iter().any(|label| label.contains("\\x0a")));
        assert!(labels.iter().any(|label| label.contains("\\xe2\\x80\\xae")));
        assert_eq!(escape_path(b"invalid-\xff.rs"), "invalid-\\xff.rs");
        assert!(
            labels
                .iter()
                .all(|label| !label.contains('\n') && !label.contains('\t'))
        );
    }

    #[test]
    fn git_workspace_parsers_fail_closed_and_extension_map_is_frozen() {
        for malformed in [
            b"# branch.oid (initial)\0# branch.head main".as_slice(),
            b"# branch.oid (initial)\0# branch.head main\0x unknown\0".as_slice(),
            b"# branch.oid (initial)\0# branch.head main\0? ../escape\0".as_slice(),
        ] {
            let error = match parse_status(malformed) {
                Ok(_) => panic!("malformed status was accepted"),
                Err(error) => error,
            };
            assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
        }
        let error = match validate_raw(b":100644 100644 a b M file") {
            Ok(_) => panic!("malformed raw output was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
        let mut files = BTreeMap::new();
        assert_eq!(
            merge_numstat(&mut files, b"1\t2\tfile", true)
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::MalformedOutput
        );
        files.insert(
            b"file".to_vec(),
            ParsedFile {
                path: b"file".to_vec(),
                previous_path: None,
                staged: WorkspaceChangeKind::Modified,
                unstaged: WorkspaceChangeKind::Unchanged,
                additions: WorkspaceLineCount::Unknown,
                deletions: WorkspaceLineCount::Unknown,
                metadata_only: false,
            },
        );
        assert_eq!(
            merge_numstat(&mut files, b"-\t1\tfile\0", true)
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::MalformedOutput
        );
        assert_eq!(
            merge_numstat(&mut files, b"1\t1\tfile\x001\t1\tfile\0", true)
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::MalformedOutput
        );
        let oid = "a".repeat(40);
        let raw = format!(
            ":100644 100644 {oid} {oid} M\0file\0\
             :100644 100644 {oid} {oid} M\0file\0"
        );
        let duplicate_error = match validate_raw(raw.as_bytes()) {
            Ok(_) => panic!("duplicate raw path was accepted"),
            Err(error) => error,
        };
        assert_eq!(
            duplicate_error.code(),
            GitWorkspaceErrorCode::MalformedOutput
        );

        let mut unmerged_files = BTreeMap::from([(
            b"conflict".to_vec(),
            ParsedFile {
                path: b"conflict".to_vec(),
                previous_path: None,
                staged: WorkspaceChangeKind::Unchanged,
                unstaged: WorkspaceChangeKind::Unmerged,
                additions: WorkspaceLineCount::Unknown,
                deletions: WorkspaceLineCount::Unknown,
                metadata_only: false,
            },
        )]);
        let conflict_paths = merge_numstat(
            &mut unmerged_files,
            b"0\t0\tconflict\x004\t0\tconflict\0",
            false,
        )
        .unwrap();
        assert_eq!(conflict_paths, [b"conflict".to_vec(), b"conflict".to_vec()]);
        assert_eq!(
            unmerged_files[b"conflict".as_slice()].additions,
            WorkspaceLineCount::Unknown
        );
        assert_eq!(
            merge_numstat(
                &mut unmerged_files,
                b"0\t0\tconflict\x004\t0\tconflict\x001\t0\tconflict\0",
                false,
            )
            .unwrap_err()
            .code(),
            GitWorkspaceErrorCode::MalformedOutput
        );

        let conflict_raw = format!(
            ":100644 100644 {oid} {oid} U\0conflict\0\
             :100644 100644 {oid} {oid} M\0conflict\0"
        );
        assert_eq!(validate_raw(conflict_raw.as_bytes()).unwrap().len(), 2);
        let third_raw = format!("{conflict_raw}:100644 100644 {oid} {oid} M\0conflict\0");
        let third_error = match validate_raw(third_raw.as_bytes()) {
            Ok(_) => panic!("third unmerged raw record was accepted"),
            Err(error) => error,
        };
        assert_eq!(third_error.code(), GitWorkspaceErrorCode::MalformedOutput);
        for (path, expected) in [
            (b"a.rs".as_slice(), DiffLanguage::Rust),
            (b"a.ts", DiffLanguage::TypeScript),
            (b"a.tsx", DiffLanguage::Tsx),
            (b"a.js", DiffLanguage::JavaScript),
            (b"a.jsx", DiffLanguage::JavaScript),
            (b"a.mjs", DiffLanguage::JavaScript),
            (b"a.cjs", DiffLanguage::JavaScript),
            (b"a.py", DiffLanguage::Python),
            (b"a.go", DiffLanguage::Plain),
        ] {
            assert_eq!(language_for(path), expected);
        }
    }

    #[test]
    fn git_workspace_retained_budget_and_path_caps_are_inclusive() {
        let mut budget = RetainedBudget::new(10);
        budget.charge(3).unwrap();
        assert_eq!(budget.remaining(), 7);
        budget.charge(7).unwrap();
        assert_eq!(budget.remaining(), 0);
        assert_eq!(budget.retained(), 10);
        assert_eq!(
            budget.charge(1).unwrap_err().code(),
            GitWorkspaceErrorCode::OutputTooLarge
        );

        assert_eq!(
            parse_nul_paths(b"one\0\0two\0").unwrap_err().code(),
            GitWorkspaceErrorCode::MalformedOutput
        );
        let mut paths = Vec::new();
        for index in 0..=PATH_LIMIT {
            paths.extend_from_slice(format!("path-{index}\0").as_bytes());
        }
        assert_eq!(
            parse_nul_paths(&paths).unwrap_err().code(),
            GitWorkspaceErrorCode::OutputTooLarge
        );
    }

    #[test]
    fn git_workspace_candidate_logical_retained_private_paths_are_exactly_bounded() {
        let identity = Arc::new(SnapshotIdentity {
            filter_paths: Arc::from([]),
            filter_attrs: Vec::new(),
            status: Vec::new(),
            staged_raw: Vec::new(),
            unstaged_raw: Vec::new(),
            staged_numstat: Vec::new(),
            unstaged_numstat: Vec::new(),
        });
        let id = WorkspaceFileId {
            generation: 0,
            slot: 0,
            seal: 0,
        };
        let snapshot = WorkspaceSnapshot {
            generation: 0,
            head: WorkspaceHead::Detached,
            files: vec![WorkspaceFile {
                id,
                label: String::new(),
                previous_label: None,
                staged: WorkspaceChangeKind::Modified,
                unstaged: WorkspaceChangeKind::Unchanged,
                additions: WorkspaceLineCount::Unknown,
                deletions: WorkspaceLineCount::Unknown,
                language: DiffLanguage::Plain,
            }],
            stats: WorkspaceStats {
                file_count: 1,
                additions: WorkspaceLineCount::Unknown,
                deletions: WorkspaceLineCount::Unknown,
            },
        };
        let make_private = |current_len: usize, previous_len: usize| PrivateFile {
            id,
            path: OsString::from_vec(vec![b'p'; current_len]),
            previous_path: Some(OsString::from_vec(vec![b'o'; previous_len])),
            staged: WorkspaceChangeKind::Modified,
            unstaged: WorkspaceChangeKind::Unchanged,
            binary: false,
            metadata_only: false,
            language: DiffLanguage::Plain,
            snapshot_identity: identity.clone(),
            worktree_identity: None,
        };
        let base_private = [make_private(0, 1)];
        let base =
            ensure_candidate_retained(&identity, &snapshot, &base_private, usize::MAX).unwrap();
        let current_len = SNAPSHOT_LIMIT.checked_sub(base).unwrap();
        let exact_private = [make_private(current_len, 1)];
        assert_eq!(
            ensure_candidate_retained(&identity, &snapshot, &exact_private, SNAPSHOT_LIMIT)
                .unwrap(),
            SNAPSHOT_LIMIT
        );
        let plus_one_private = [make_private(current_len, 2)];
        assert_eq!(
            ensure_candidate_retained(&identity, &snapshot, &plus_one_private, SNAPSHOT_LIMIT)
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::OutputTooLarge
        );
    }

    #[test]
    fn git_workspace_projection_redaction_and_service_debug_are_safe() {
        let id = WorkspaceFileId {
            generation: 1,
            slot: 2,
            seal: 3,
        };
        let projection = DiffTextProjection {
            file_id: id,
            language: DiffLanguage::Plain,
            sections: vec![DiffSection {
                layer: DiffLayer::Untracked,
                hunks: vec![DiffHunk {
                    old_start: 0,
                    old_count: 0,
                    new_start: 1,
                    new_count: 1,
                    heading_suffix: None,
                    missing_trailing_newline: false,
                    rows: vec![DiffRow {
                        kind: DiffRowKind::Addition,
                        old_line: None,
                        new_line: Some(1),
                        text: "LEAK_SENTINEL".into(),
                    }],
                }],
            }],
        };
        let debug = format!("{projection:?}");
        assert!(!debug.contains("LEAK_SENTINEL"));
        assert!(debug.contains("redacted"));
        let repo = Repo::new();
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        assert!(!format!("{service:?}").contains(&repo.path().to_string_lossy().to_string()));
    }

    #[test]
    fn git_workspace_hunk_suffix_no_newline_and_line_cap_are_preserved() {
        let exact_line = "x".repeat(PATCH_LINE_LIMIT);
        let patch =
            format!("@@ -0,0 +1,1 @@ fn name\n+{exact_line}\n\\ No newline at end of file\n");
        let mut rows = PATCH_ROW_LIMIT;
        let section = parse_patch(DiffLayer::Unstaged, patch.as_bytes(), &mut rows).unwrap();
        let hunk = &section.hunks[0];
        assert_eq!(hunk.heading_suffix.as_deref(), Some("fn name"));
        assert!(hunk.missing_trailing_newline);
        assert_eq!(hunk.rows[0].text.len(), PATCH_LINE_LIMIT);
        let too_long = format!("@@ -0,0 +1,1 @@\n+{}\n", "x".repeat(PATCH_LINE_LIMIT + 1));
        let error = match parse_patch(DiffLayer::Unstaged, too_long.as_bytes(), &mut rows) {
            Ok(_) => panic!("oversized line was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code(), GitWorkspaceErrorCode::OutputTooLarge);
        let mut rows = PATCH_ROW_LIMIT;
        let bad_marker = b"@@ -1,1 +1,1 @@\n same\n\\ unexpected marker\n";
        let error = match parse_patch(DiffLayer::Unstaged, bad_marker, &mut rows) {
            Ok(_) => panic!("unknown backslash marker was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
        let mut rows = PATCH_ROW_LIMIT;
        let overflow = b"@@ -4294967295,1 +1,1 @@\n same\n";
        let error = match parse_patch(DiffLayer::Unstaged, overflow, &mut rows) {
            Ok(_) => panic!("overflowing line coordinate was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
    }

    #[test]
    fn git_workspace_combined_patch_byte_and_row_caps_are_inclusive() {
        let mut bytes = PATCH_LIMIT;
        consume_projection_bytes(&mut bytes, PATCH_LIMIT / 2).unwrap();
        consume_projection_bytes(&mut bytes, PATCH_LIMIT - PATCH_LIMIT / 2).unwrap();
        assert_eq!(bytes, 0);
        assert_eq!(
            consume_projection_bytes(&mut bytes, 1).unwrap_err().code(),
            GitWorkspaceErrorCode::OutputTooLarge
        );

        let patch = |rows: usize| {
            let mut body = format!("@@ -0,0 +1,{rows} @@\n");
            for _ in 0..rows {
                body.push_str("+x\n");
            }
            body
        };
        let mut rows = PATCH_ROW_LIMIT;
        let staged = parse_patch(
            DiffLayer::Staged,
            patch(PATCH_ROW_LIMIT / 2).as_bytes(),
            &mut rows,
        )
        .unwrap();
        let unstaged = parse_patch(
            DiffLayer::Unstaged,
            patch(PATCH_ROW_LIMIT / 2).as_bytes(),
            &mut rows,
        )
        .unwrap();
        assert_eq!(rows, 0);
        assert_eq!(staged.hunks[0].rows.len(), PATCH_ROW_LIMIT / 2);
        assert_eq!(unstaged.hunks[0].rows.len(), PATCH_ROW_LIMIT / 2);
        let mut rows = PATCH_ROW_LIMIT;
        parse_patch(
            DiffLayer::Staged,
            patch(PATCH_ROW_LIMIT / 2).as_bytes(),
            &mut rows,
        )
        .unwrap();
        let row_error = match parse_patch(
            DiffLayer::Unstaged,
            patch(PATCH_ROW_LIMIT / 2 + 1).as_bytes(),
            &mut rows,
        ) {
            Ok(_) => panic!("combined row cap +1 was accepted"),
            Err(error) => error,
        };
        assert_eq!(row_error.code(), GitWorkspaceErrorCode::OutputTooLarge);
    }

    #[test]
    fn git_workspace_environment_scrub_is_exact() {
        let mut command = Command::new("/usr/bin/true");
        command
            .env("GIT_DIR", "/private/leak")
            .env("GIT_CONFIG_COUNT", "9")
            .env("VEGA_KEEP", "yes");
        scrub_git_environment(&mut command);
        let env: HashMap<_, _> = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
            .collect();
        assert_eq!(env.get(OsStr::new("GIT_DIR")), Some(&None));
        assert_eq!(env.get(OsStr::new("GIT_CONFIG_COUNT")), Some(&None));
        assert_eq!(
            env.get(OsStr::new("GIT_LITERAL_PATHSPECS"))
                .and_then(|value| value.as_deref()),
            Some(OsStr::new("1"))
        );
        assert_eq!(
            env.get(OsStr::new("GIT_NO_LAZY_FETCH"))
                .and_then(|value| value.as_deref()),
            Some(OsStr::new("1"))
        );
        assert_eq!(
            env.get(OsStr::new("VEGA_KEEP"))
                .and_then(|value| value.as_deref()),
            Some(OsStr::new("yes"))
        );
    }

    #[tokio::test]
    async fn git_workspace_runner_scrubs_git_environment_and_bounds_output() {
        let repo = Repo::new();
        let script = repo.path().join("fixture-git");
        fs::write(
            &script,
            "#!/bin/sh\nif env | grep '^GIT_CONFIG_COUNT=' >/dev/null; then exit 90; fi\nif [ \"$GIT_LITERAL_PATHSPECS\" != 1 ] || [ \"$GIT_NO_LAZY_FETCH\" != 1 ]; then exit 91; fi\npython3 -c 'import sys; sys.stdout.write(\"x\" * (8 * 1024 * 1024 + 1))'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        let service = GitWorkspaceService::new_for_test(repo.path(), script).unwrap();
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::OutputTooLarge
        );
    }

    #[tokio::test]
    async fn git_workspace_explicit_filter_attribute_rejects_before_driver_execution() {
        let repo = Repo::new();
        repo.write("victim.txt", b"base\n");
        repo.commit_all();
        let marker = repo.path().join("filter-ran");
        let driver = repo.path().join("filter-driver");
        fs::write(
            &driver,
            format!("#!/bin/sh\nprintf ran > '{}'\ncat\n", marker.display()),
        )
        .unwrap();
        let mut permissions = fs::metadata(&driver).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&driver, permissions).unwrap();
        git(
            repo.path(),
            &["config", "filter.evil.clean", &driver.to_string_lossy()],
        );
        repo.write(".gitattributes", b"*.txt filter=evil\n");
        repo.write("victim.txt", b"changed\n");

        let service = GitWorkspaceService::new(repo.path()).unwrap();
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::GitFailed
        );
        assert!(!marker.exists(), "filter driver executed during preflight");
        assert_eq!(
            validate_filter_attrs(&[b"victim.txt".to_vec()], b"victim.txt\0filter\0unset\0")
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::GitFailed
        );
    }

    #[test]
    fn git_workspace_bounded_stdin_stdout_stderr_progress_concurrently() {
        let repo = Repo::new();
        let script = repo.path().join("fixture-git");
        fs::write(
            &script,
            "#!/bin/sh\nif [ \"${12}\" != check-attr ] || [ \"${13}\" != -z ] || [ \"${14}\" != --stdin ] || [ \"${15}\" != --all ]; then exit 91; fi\npython3 -c 'import sys; sys.stdout.write(\"o\" * 65536); sys.stdout.flush(); data=sys.stdin.buffer.read(); sys.stderr.write(\"e\" * 32768); sys.stderr.flush(); sys.stdout.write(str(len(data)))'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
        let runner = Runner::new(service.root.clone(), service.identity, Some(script));
        let input = vec![b'i'; 128 * 1024];
        let output = runner
            .run_with_input(
                "check-attr",
                &[
                    OsString::from("-z"),
                    OsString::from("--stdin"),
                    OsString::from("--all"),
                ],
                Arc::from(input),
                128 * 1024,
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(&output.stdout[..65_536], vec![b'o'; 65_536]);
        assert!(output.stdout.ends_with(b"131072"));
    }

    #[test]
    fn git_workspace_stderr_cap_is_inclusive_and_plus_one_fails() {
        let repo = Repo::new();
        let script = repo.path().join("fixture-git");
        let write_fixture = |size: usize| {
            fs::write(
                &script,
                format!("#!/bin/sh\npython3 -c 'import sys; sys.stderr.write(\"e\" * {size})'\n"),
            )
            .unwrap();
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&script, permissions).unwrap();
        };
        let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
        let runner = Runner::new(service.root.clone(), service.identity, Some(script.clone()));
        write_fixture(STDERR_LIMIT);
        runner
            .run(
                "rev-parse",
                &[OsString::from("--show-toplevel")],
                1,
                &CancellationToken::new(),
            )
            .unwrap();
        write_fixture(STDERR_LIMIT + 1);
        let stderr_error = match runner.run(
            "rev-parse",
            &[OsString::from("--show-toplevel")],
            1,
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("stderr cap +1 was accepted"),
            Err(error) => error,
        };
        assert_eq!(stderr_error.code(), GitWorkspaceErrorCode::OutputTooLarge);
    }

    #[test]
    fn git_workspace_read_timeout_is_typed_and_bounded() {
        let repo = Repo::new();
        let script = repo.path().join("fixture-git");
        let pid_file = repo.path().join("timeout-descendant.pid");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
        let runner = Runner::new(service.root.clone(), service.identity, Some(script));
        let started = Instant::now();
        let timeout_error = match runner.run(
            "rev-parse",
            &[OsString::from("--show-toplevel")],
            1,
            &CancellationToken::new(),
        ) {
            Ok(_) => panic!("read timeout was not enforced"),
            Err(error) => error,
        };
        assert_eq!(timeout_error.code(), GitWorkspaceErrorCode::TimedOut);
        assert!(started.elapsed() >= READ_TIMEOUT);
        assert!(started.elapsed() < READ_TIMEOUT + Duration::from_secs(3));
        let pid = fs::read_to_string(pid_file).unwrap();
        assert!(
            !Command::new(KILL)
                .args(["-0", &pid])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "timeout descendant survived cleanup"
        );
    }

    #[tokio::test]
    async fn git_workspace_latest_refresh_wins_without_stale_overwrite() {
        let repo = Repo::new();
        repo.write("latest.txt", b"latest\n");
        let script = repo.path().join("fixture-git");
        let gate = tempdir().unwrap();
        let lock = gate.path().join("first.lock");
        let ready = gate.path().join("first.ready");
        let release = gate.path().join("first.release");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nif mkdir '{}' 2>/dev/null; then : > '{}'; while [ ! -e '{}' ]; do sleep 0.01; done; fi\nexec /usr/bin/git \"$@\"\n",
                lock.display(),
                ready.display(),
                release.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        let service = Arc::new(GitWorkspaceService::new_for_test(repo.path(), script).unwrap());
        let first = tokio::spawn({
            let service = service.clone();
            async move { service.refresh(CancellationToken::new()).await }
        });
        for _ in 0..500 {
            if ready.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(ready.exists(), "first refresh did not enter fixture delay");
        let latest = service.refresh(CancellationToken::new()).await.unwrap();
        fs::write(&release, b"release\n").unwrap();
        assert_eq!(
            first.await.unwrap().unwrap_err().code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
        let file = latest
            .files
            .iter()
            .find(|file| file.label == "latest.txt")
            .unwrap();
        assert_eq!(
            service
                .diff(file.id, CancellationToken::new())
                .await
                .unwrap()
                .file_id(),
            file.id
        );
    }

    #[tokio::test]
    async fn git_workspace_obsolete_failure_does_not_invalidate_newer_snapshot() {
        let repo = Repo::new();
        repo.write("newer.txt", b"newer\n");
        let script = repo.path().join("fixture-git");
        let gate = tempdir().unwrap();
        let lock = gate.path().join("first.lock");
        let ready = gate.path().join("first.ready");
        let release = gate.path().join("first.release");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nif mkdir '{}' 2>/dev/null; then : > '{}'; while [ ! -e '{}' ]; do sleep 0.01; done; exit 91; fi\nexec /usr/bin/git \"$@\"\n",
                lock.display(),
                ready.display(),
                release.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        let service = Arc::new(GitWorkspaceService::new_for_test(repo.path(), script).unwrap());
        let obsolete = tokio::spawn({
            let service = service.clone();
            async move { service.refresh(CancellationToken::new()).await }
        });
        for _ in 0..500 {
            if ready.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            ready.exists(),
            "obsolete refresh did not enter fixture delay"
        );
        let latest = service.refresh(CancellationToken::new()).await.unwrap();
        fs::write(&release, b"release\n").unwrap();
        assert_eq!(
            obsolete.await.unwrap().unwrap_err().code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
        let file = latest
            .files
            .iter()
            .find(|file| file.label == "newer.txt")
            .unwrap();
        assert_eq!(
            service
                .diff(file.id, CancellationToken::new())
                .await
                .unwrap()
                .file_id(),
            file.id
        );
    }

    #[tokio::test]
    async fn git_workspace_ctime_detects_equal_size_edit_with_restored_mtime() {
        let repo = Repo::new();
        repo.write("tracked.txt", b"base\n");
        repo.commit_all();
        repo.write("tracked.txt", b"left\n");
        let reference = repo.path().join("mtime-reference");
        let tracked = repo.path().join("tracked.txt");
        assert!(
            Command::new("/bin/cp")
                .args([OsStr::new("-p"), tracked.as_os_str(), reference.as_os_str()])
                .status()
                .unwrap()
                .success()
        );
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        let file = snapshot
            .files
            .iter()
            .find(|file| file.label == "tracked.txt")
            .unwrap();
        let before = file_identity(&fs::metadata(&tracked).unwrap());
        repo.write("tracked.txt", b"rght\n");
        assert!(
            Command::new("/usr/bin/touch")
                .args([OsStr::new("-r"), reference.as_os_str(), tracked.as_os_str()])
                .status()
                .unwrap()
                .success()
        );
        let after = file_identity(&fs::metadata(&tracked).unwrap());
        assert_eq!(before.size, after.size);
        assert_eq!(
            (before.mtime, before.mtime_ns),
            (after.mtime, after.mtime_ns)
        );
        assert_ne!(
            (before.ctime, before.ctime_ns),
            (after.ctime, after.ctime_ns)
        );
        assert_eq!(
            service
                .diff(file.id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::ChangedDuringRead
        );
    }

    #[test]
    fn git_workspace_metadata_remaining_cap_is_inclusive_and_plus_one_fails() {
        let repo = Repo::new();
        let script = repo.path().join("fixture-git");
        let write_fixture = |size: usize| {
            fs::write(
                &script,
                format!("#!/bin/sh\npython3 -c 'import sys; sys.stdout.write(\"x\" * {size})'\n"),
            )
            .unwrap();
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&script, permissions).unwrap();
        };
        write_fixture(1024);
        let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
        let runner = Runner::new(service.root.clone(), service.identity, Some(script.clone()));
        assert_eq!(
            verify_filter_bytes_with_retained(
                &runner,
                &[],
                &[],
                SNAPSHOT_LIMIT - 1024,
                &CancellationToken::new(),
            )
            .unwrap_err()
            .code(),
            GitWorkspaceErrorCode::MalformedOutput
        );
        write_fixture(1025);
        assert_eq!(
            verify_filter_bytes_with_retained(
                &runner,
                &[],
                &[],
                SNAPSHOT_LIMIT - 1024,
                &CancellationToken::new(),
            )
            .unwrap_err()
            .code(),
            GitWorkspaceErrorCode::OutputTooLarge
        );
    }

    #[tokio::test]
    async fn git_workspace_cancel_is_typed_and_reaps_fixture_group() {
        let repo = Repo::new();
        let script = repo.path().join("fixture-git");
        let pid_file = repo.path().join("descendant.pid");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\nchild=$!\nprintf '%s' \"$child\" > '{}'\nwait\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        let service = Arc::new(GitWorkspaceService::new_for_test(repo.path(), script).unwrap());
        let cancel = CancellationToken::new();
        let task = tokio::spawn({
            let service = service.clone();
            let cancel = cancel.clone();
            async move { service.refresh(cancel).await }
        });
        for _ in 0..500 {
            if pid_file.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(pid_file.exists(), "fixture descendant was not started");
        cancel.cancel();
        assert_eq!(
            task.await.unwrap().unwrap_err().code(),
            GitWorkspaceErrorCode::Cancelled
        );
        let pid = fs::read_to_string(pid_file).unwrap();
        let mut gone = false;
        for _ in 0..50 {
            let status = Command::new(KILL)
                .args(["-0", &pid])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            if !status.success() {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(gone, "descendant process survived cancellation");
    }

    #[tokio::test]
    async fn git_workspace_early_parent_exit_with_inherited_pipes_fails_and_reaps_group() {
        let repo = Repo::new();
        let script = repo.path().join("fixture-git");
        let pid_file = repo.path().join("early-descendant.pid");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nexit 0\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        let service = GitWorkspaceService::new_for_test(repo.path(), script).unwrap();
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::ProcessControlFailed
        );
        let pid = fs::read_to_string(pid_file).unwrap();
        assert!(
            !Command::new(KILL)
                .args(["-0", &pid])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "inherited-pipe descendant survived cleanup"
        );
    }
}
