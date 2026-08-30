//! Canonical two-stage commit assistance.
//!
//! This module is the only T34 boundary allowed to stage or commit. Raw Git
//! paths, refs, object ids, status/tree bytes, patch summaries and messages
//! never leave the service as debuggable values.

use super::*;
use crate::types::{
    CommitChecklist, CommitCompletion, CommitDraft, CommitErrorCode, CommitOutcome,
    CommitPrepareCompletion, CommitSelection, CommitSelectionKind, IndexSnapshotId, PreparedCommit,
    PreparedCommitId,
};
use futures::StreamExt as _;
use std::collections::HashSet;
use vega_runtime::{ChatMessage, ChatRequest, ChatRole, Provider, ProviderEvent, StopReason};

const SUMMARY_LIMIT: usize = 256 * 1024;
const SUMMARY_MARKER: &[u8] = b"\n[vega-summary truncated=true]\n";
const MESSAGE_LIMIT: usize = 32 * 1024;
const DRAFT_TIMEOUT: Duration = Duration::from_secs(60);
const SYSTEM_PROMPT: &str = "Generate one concise Git commit message for the exact staged diff. Return only the commit message text. Do not call tools.";
const USER_PREFIX: &str = "Generate the commit message for the staged diff below.\ntruncated=";

#[derive(Clone, PartialEq, Eq)]
struct HeadAuthority {
    unborn: bool,
    oid: Vec<u8>,
    short: Vec<u8>,
    full_ref: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StageEntry {
    mode: Vec<u8>,
    oid: Vec<u8>,
    path: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TreeEntry {
    mode: Vec<u8>,
    object_type: Vec<u8>,
    oid: Vec<u8>,
    path: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StatusShape {
    Ordinary,
    Rename,
    Copy,
    Untracked,
}

#[derive(Clone, PartialEq, Eq)]
struct StatusRecord {
    shape: StatusShape,
    x: u8,
    y: u8,
    sub: Vec<u8>,
    head_mode: Vec<u8>,
    index_mode: Vec<u8>,
    worktree_mode: Vec<u8>,
    head_oid: Vec<u8>,
    index_oid: Vec<u8>,
    path: Vec<u8>,
    previous: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Eq)]
struct IndexAuthority {
    head: HeadAuthority,
    status_raw: Vec<u8>,
    stage_raw: Vec<u8>,
    tree_raw: Vec<u8>,
    records: Vec<StatusRecord>,
    stages: Vec<StageEntry>,
    tree: Vec<TreeEntry>,
    workspace_generation: u64,
}

#[derive(Clone)]
struct ChecklistRow {
    public: CommitSelection,
    closure: Vec<Vec<u8>>,
    record: StatusRecord,
    optional_kind: CommitSelectionKind,
    worktree_mode: Option<Vec<u8>>,
}

#[derive(Clone)]
struct StoredChecklist {
    id: IndexSnapshotId,
    authority: IndexAuthority,
    optional: Vec<ChecklistRow>,
}

struct StoredPrepared {
    id: PreparedCommitId,
    authority: IndexAuthority,
    summary: String,
    summary_truncated: bool,
}

#[derive(Default)]
struct CommitState {
    next_generation: u64,
    next_slot: u64,
    checklist: Option<StoredChecklist>,
    prepared: Option<StoredPrepared>,
    mutation_active: bool,
}

/// Route-owned headless commit service. Controller routing and the shared
/// trusted-action token remain app responsibilities; this service enforces
/// repository and single-use Git authority.
pub struct TrustedGitService {
    root: PathBuf,
    root_identity: RootIdentity,
    instance_nonce: u64,
    workspace: Arc<GitWorkspaceService>,
    state: Arc<Mutex<CommitState>>,
    #[cfg(test)]
    mutation_executable: Option<PathBuf>,
    #[cfg(test)]
    mutation_timeout: Duration,
    #[cfg(test)]
    read_executable: Option<PathBuf>,
}

impl std::fmt::Debug for TrustedGitService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        formatter
            .debug_struct("TrustedGitService")
            .field("generation", &state.next_generation)
            .field("has_checklist", &state.checklist.is_some())
            .field("has_prepared", &state.prepared.is_some())
            .field("mutation_active", &state.mutation_active)
            .finish()
    }
}

impl TrustedGitService {
    pub fn new(
        root: impl AsRef<Path>,
        workspace: Arc<GitWorkspaceService>,
    ) -> Result<Self, CommitErrorCode> {
        let root = fs::canonicalize(root).map_err(|_| CommitErrorCode::InvalidRoot)?;
        let metadata = fs::metadata(&root).map_err(|_| CommitErrorCode::InvalidRoot)?;
        if !metadata.is_dir() || workspace.root != root {
            return Err(CommitErrorCode::InvalidRoot);
        }
        let instance_nonce = SERVICE_NONCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| CommitErrorCode::OutputTooLarge)?;
        Ok(Self {
            root,
            root_identity: RootIdentity {
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
            instance_nonce,
            workspace,
            state: Arc::new(Mutex::new(CommitState::default())),
            #[cfg(test)]
            mutation_executable: None,
            #[cfg(test)]
            mutation_timeout: MUTATION_TIMEOUT,
            #[cfg(test)]
            read_executable: None,
        })
    }

    #[cfg(test)]
    fn new_with_mutation_for_test(
        root: impl AsRef<Path>,
        workspace: Arc<GitWorkspaceService>,
        executable: PathBuf,
    ) -> Result<Self, CommitErrorCode> {
        let mut service = Self::new(root, workspace)?;
        service.mutation_executable = Some(executable);
        Ok(service)
    }

    #[cfg(test)]
    fn new_with_mutation_timeout_for_test(
        root: impl AsRef<Path>,
        workspace: Arc<GitWorkspaceService>,
        executable: PathBuf,
        timeout: Duration,
    ) -> Result<Self, CommitErrorCode> {
        let mut service = Self::new_with_mutation_for_test(root, workspace, executable)?;
        service.mutation_timeout = timeout;
        Ok(service)
    }

    #[cfg(test)]
    fn new_with_executables_for_test(
        root: impl AsRef<Path>,
        workspace: Arc<GitWorkspaceService>,
        mutation_executable: PathBuf,
        read_executable: PathBuf,
    ) -> Result<Self, CommitErrorCode> {
        let mut service = Self::new(root, workspace)?;
        service.mutation_executable = Some(mutation_executable);
        service.read_executable = Some(read_executable);
        Ok(service)
    }

    /// Captures displayed A from three canonical Git truth sources.
    pub async fn open_checklist(
        &self,
        cancel: CancellationToken,
    ) -> Result<CommitChecklist, CommitErrorCode> {
        let workspace_generation = self.current_workspace_generation()?;
        let authority = self.capture(workspace_generation, cancel).await?;
        let (staged, optional) = self.project_rows(&authority)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.mutation_active {
            return Err(CommitErrorCode::StaleAuthority);
        }
        let generation = state
            .next_generation
            .checked_add(1)
            .ok_or(CommitErrorCode::OutputTooLarge)?;
        let slot = state
            .next_slot
            .checked_add(1)
            .ok_or(CommitErrorCode::OutputTooLarge)?;
        let id = IndexSnapshotId {
            generation,
            slot,
            seal: seal(
                self.root_identity,
                self.instance_nonce,
                generation,
                u32::try_from(slot).map_err(|_| CommitErrorCode::OutputTooLarge)?,
                b"commit-checklist",
            ),
        };
        let public = CommitChecklist {
            id,
            workspace_generation,
            staged: staged.iter().map(|row| row.public.clone()).collect(),
            optional: optional.iter().map(|row| row.public.clone()).collect(),
        };
        state.next_generation = generation;
        state.next_slot = slot;
        state.prepared = None;
        state.checklist = Some(StoredChecklist {
            id,
            authority,
            optional,
        });
        Ok(public)
    }

    /// Performs one uncancelled authoritative metadata reconciliation after
    /// an owner terminal. App workers call this only when the owner completion
    /// could not carry its own snapshot; no provider or UI data is accepted.
    pub async fn reconcile_workspace(&self) -> Result<WorkspaceSnapshot, CommitErrorCode> {
        self.workspace
            .refresh(CancellationToken::new())
            .await
            .map_err(map_git_error)
    }

    /// Recovers a disconnected mutation owner before the app may release its
    /// trusted-action lease. Any durable owner capability is consumed first;
    /// C/ABA falls back to a fresh ordinary authoritative snapshot. The
    /// abandoned checklist/prepared capability is never revived.
    pub async fn recover_disconnected_mutation(
        &self,
    ) -> Result<WorkspaceSnapshot, CommitErrorCode> {
        let workspace = match self.workspace.active_owned_refresh() {
            Some(owner) => match self.owner_refresh(owner).await {
                Ok(snapshot) => Ok(snapshot),
                Err(_) => self.reconcile_workspace().await,
            },
            None => self.reconcile_workspace().await,
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.mutation_active = false;
        state.checklist = None;
        state.prepared = None;
        workspace
    }

    /// Returns the exact workspace authority owned by this trusted service.
    /// The handle exposes only the existing safe snapshot/projection API; raw
    /// paths and Git command material remain private to the headless crate.
    pub fn workspace_service(&self) -> Arc<GitWorkspaceService> {
        self.workspace.clone()
    }

    /// Performs zero or one exact staging mutation and returns owned B.
    pub async fn prepare(
        &self,
        id: IndexSnapshotId,
        selected: Vec<WorkspaceFileId>,
        cancel: CancellationToken,
    ) -> CommitPrepareCompletion {
        let checklist = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let valid = state
                .checklist
                .as_ref()
                .is_some_and(|checklist| checklist.id == id)
                && !state.mutation_active;
            if !valid {
                return failed_prepare(CommitErrorCode::StaleAuthority, None);
            }
            state.mutation_active = true;
            match state.checklist.take() {
                Some(checklist) => checklist,
                None => {
                    state.mutation_active = false;
                    return failed_prepare(CommitErrorCode::StaleAuthority, None);
                }
            }
        };
        let result = self.prepare_owned(&checklist, selected, cancel).await;
        let workspace = match &result {
            Ok((_, workspace)) => Some(workspace.clone()),
            Err(_) => self.reconcile_workspace().await.ok(),
        };
        let mut completion = match result {
            Ok((mut prepared, owned_workspace)) => {
                let expected_generation = prepared.authority.workspace_generation;
                if owned_workspace.generation != expected_generation {
                    failed_prepare(CommitErrorCode::ChangedDuringRead, workspace.clone())
                } else {
                    let mut state = self
                        .state
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    let generation = match state.next_generation.checked_add(1) {
                        Some(generation) => generation,
                        None => {
                            state.mutation_active = false;
                            return failed_prepare(CommitErrorCode::OutputTooLarge, workspace);
                        }
                    };
                    let slot = match state.next_slot.checked_add(1) {
                        Some(slot) => slot,
                        None => {
                            state.mutation_active = false;
                            return failed_prepare(CommitErrorCode::OutputTooLarge, workspace);
                        }
                    };
                    let slot_u32 = match u32::try_from(slot) {
                        Ok(slot) => slot,
                        Err(_) => {
                            state.mutation_active = false;
                            return failed_prepare(CommitErrorCode::OutputTooLarge, workspace);
                        }
                    };
                    let prepared_id = PreparedCommitId {
                        generation,
                        slot,
                        seal: seal(
                            self.root_identity,
                            self.instance_nonce,
                            generation,
                            slot_u32,
                            b"prepared-commit",
                        ),
                    };
                    prepared.id = prepared_id;
                    let staged_file_count = match u32::try_from(prepared.authority.stages.len()) {
                        Ok(count) => count,
                        Err(_) => {
                            state.mutation_active = false;
                            return failed_prepare(CommitErrorCode::OutputTooLarge, workspace);
                        }
                    };
                    let public = PreparedCommit {
                        id: prepared_id,
                        workspace_generation: expected_generation,
                        staged_file_count,
                        summary_truncated: prepared.summary_truncated,
                    };
                    state.next_generation = generation;
                    state.next_slot = slot;
                    state.prepared = Some(prepared);
                    CommitPrepareCompletion {
                        prepared: Some(public),
                        workspace: workspace.clone(),
                        error: None,
                    }
                }
            }
            Err(code) => failed_prepare(code, workspace.clone()),
        };
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .mutation_active = false;
        if completion.workspace.is_none() {
            completion.error = Some(CommitErrorCode::ChangedDuringRead);
            completion.prepared = None;
        }
        completion
    }

    /// Generates one bounded editable draft under the exact strict stream
    /// grammar. The provider never receives mutation capabilities.
    pub async fn draft(
        &self,
        id: PreparedCommitId,
        model: String,
        provider: Arc<dyn Provider>,
        cancel: CancellationToken,
    ) -> Result<CommitDraft, CommitErrorCode> {
        let (authority, summary, truncated) = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let prepared = state
                .prepared
                .as_ref()
                .filter(|prepared| prepared.id == id)
                .ok_or(CommitErrorCode::StaleAuthority)?;
            (
                prepared.authority.clone(),
                prepared.summary.clone(),
                prepared.summary_truncated,
            )
        };
        self.require_exact_authority(&authority, cancel.clone())
            .await?;
        let request = ChatRequest {
            model,
            messages: vec![
                ChatMessage::new(ChatRole::System, SYSTEM_PROMPT),
                ChatMessage::new(
                    ChatRole::User,
                    format!(
                        "{USER_PREFIX}{}\n--- staged diff ---\n{summary}",
                        if truncated { "true" } else { "false" }
                    ),
                ),
            ],
            tools: Vec::new(),
            max_tokens: Some(256),
        };
        let draft =
            collect_draft_with_deadline(provider, request, cancel.clone(), DRAFT_TIMEOUT).await?;
        self.require_exact_authority(&authority, cancel).await?;
        Ok(CommitDraft::new(draft))
    }

    /// Consumes B exactly once, commits through in-memory stdin, then proves
    /// immutable parent/tree/ref topology and refreshes on every exit.
    pub async fn commit(
        &self,
        id: PreparedCommitId,
        message: String,
        cancel: CancellationToken,
    ) -> CommitCompletion {
        if message.is_empty() || message.len() > MESSAGE_LIMIT || message.as_bytes().contains(&0) {
            return CommitCompletion {
                outcome: CommitOutcome::Failed(CommitErrorCode::InvalidMessage),
                workspace: None,
            };
        }
        let prepared = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if state.mutation_active
                || state
                    .prepared
                    .as_ref()
                    .is_none_or(|prepared| prepared.id != id)
            {
                return CommitCompletion {
                    outcome: CommitOutcome::Failed(CommitErrorCode::StaleAuthority),
                    workspace: None,
                };
            }
            state.mutation_active = true;
            match state.prepared.take() {
                Some(prepared) => prepared,
                None => {
                    state.mutation_active = false;
                    return CommitCompletion {
                        outcome: CommitOutcome::Failed(CommitErrorCode::StaleAuthority),
                        workspace: None,
                    };
                }
            }
        };
        let parent_generation = prepared.authority.workspace_generation;
        let owner = self.workspace.begin_owned_refresh(parent_generation);
        let (result, workspace) = match owner {
            Ok(owner) => {
                let result = self.commit_owned(&prepared, message, cancel).await;
                let workspace = self.owner_refresh(owner).await.ok();
                (result, workspace)
            }
            Err(failure) => (Err(map_git_error(failure)), None),
        };
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .mutation_active = false;
        CommitCompletion {
            outcome: match result {
                Ok(()) if workspace.is_some() => CommitOutcome::Committed,
                Ok(()) => CommitOutcome::Failed(CommitErrorCode::ChangedDuringRead),
                Err(code) => CommitOutcome::Failed(code),
            },
            workspace,
        }
    }

    fn current_workspace_generation(&self) -> Result<u64, CommitErrorCode> {
        self.workspace
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.generation)
            .ok_or(CommitErrorCode::StaleAuthority)
    }

    async fn owner_refresh(
        &self,
        owner: WorkspaceMutationOwner,
    ) -> Result<WorkspaceSnapshot, GitWorkspaceError> {
        let mut backoff = Duration::from_millis(25);
        loop {
            match self
                .workspace
                .refresh_owned_after_mutation(owner, CancellationToken::new())
                .await
            {
                Ok(snapshot) => return Ok(snapshot),
                Err(failure)
                    if matches!(
                        failure.code(),
                        GitWorkspaceErrorCode::ChangedDuringRead
                            | GitWorkspaceErrorCode::StaleGeneration
                    ) =>
                {
                    return Err(failure);
                }
                Err(_) => {
                    tokio::time::sleep(backoff).await;
                    backoff = backoff.saturating_mul(2).min(Duration::from_secs(1));
                }
            }
        }
    }

    async fn capture(
        &self,
        workspace_generation: u64,
        cancel: CancellationToken,
    ) -> Result<IndexAuthority, CommitErrorCode> {
        let root = self.root.clone();
        let identity = self.root_identity;
        #[cfg(test)]
        let read_executable = self.read_executable.clone();
        tokio::task::spawn_blocking(move || {
            capture_authority(
                &Runner::new(
                    root,
                    identity,
                    #[cfg(test)]
                    read_executable,
                ),
                workspace_generation,
                &cancel,
            )
        })
        .await
        .map_err(|_| CommitErrorCode::GitFailed)?
    }

    async fn require_exact_authority(
        &self,
        authority: &IndexAuthority,
        cancel: CancellationToken,
    ) -> Result<(), CommitErrorCode> {
        if self.current_workspace_generation()? != authority.workspace_generation {
            return Err(CommitErrorCode::StaleAuthority);
        }
        let current = self.capture(authority.workspace_generation, cancel).await?;
        if current != *authority {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
        Ok(())
    }

    fn project_rows(
        &self,
        authority: &IndexAuthority,
    ) -> Result<(Vec<ChecklistRow>, Vec<ChecklistRow>), CommitErrorCode> {
        let workspace = self
            .workspace
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if workspace.generation != authority.workspace_generation {
            return Err(CommitErrorCode::StaleAuthority);
        }
        if workspace
            .identity
            .as_ref()
            .is_none_or(|identity| identity.status != authority.status_raw)
        {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
        let public_snapshot = workspace
            .snapshot
            .as_ref()
            .ok_or(CommitErrorCode::StaleAuthority)?;
        let mut staged = Vec::new();
        let mut optional = Vec::new();
        let mut matched_files = HashSet::new();
        for record in &authority.records {
            let mut candidates = workspace.files.iter().filter(|file| {
                file.path.as_bytes() == record.path
                    && file.previous_path.as_ref().map(|path| path.as_bytes())
                        == record.previous.as_deref()
            });
            let private = candidates
                .next()
                .ok_or(CommitErrorCode::ChangedDuringRead)?;
            if candidates.next().is_some() || !matched_files.insert(private.id) {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
            let public = public_snapshot
                .files
                .iter()
                .find(|file| file.id == private.id)
                .ok_or(CommitErrorCode::ChangedDuringRead)?;
            let staged_closure = component_closure(record, true);
            let optional_closure = component_closure(record, false);
            if record.shape != StatusShape::Untracked && record.x != b'.' {
                staged.push(ChecklistRow {
                    public: project_selection(public, record, true)?,
                    closure: staged_closure,
                    record: record.clone(),
                    optional_kind: optional_kind(record),
                    worktree_mode: private.worktree_identity.and_then(|identity| {
                        match identity.kind {
                            0 => Some(if identity.mode & 0o111 == 0 {
                                b"100644".to_vec()
                            } else {
                                b"100755".to_vec()
                            }),
                            2 => Some(b"120000".to_vec()),
                            _ => None,
                        }
                    }),
                });
            }
            if record.y != b'.' || record.shape == StatusShape::Untracked {
                optional.push(ChecklistRow {
                    public: project_selection(public, record, false)?,
                    closure: optional_closure,
                    record: record.clone(),
                    optional_kind: optional_kind(record),
                    worktree_mode: private.worktree_identity.and_then(|identity| {
                        match identity.kind {
                            0 => Some(if identity.mode & 0o111 == 0 {
                                b"100644".to_vec()
                            } else {
                                b"100755".to_vec()
                            }),
                            2 => Some(b"120000".to_vec()),
                            _ => None,
                        }
                    }),
                });
            }
        }
        Ok((staged, optional))
    }

    async fn prepare_owned(
        &self,
        checklist: &StoredChecklist,
        selected: Vec<WorkspaceFileId>,
        cancel: CancellationToken,
    ) -> Result<(StoredPrepared, WorkspaceSnapshot), CommitErrorCode> {
        self.require_exact_authority(&checklist.authority, cancel.clone())
            .await?;
        let selected_rows = resolve_selected(checklist, &selected)?;
        let paths = selected_paths(&selected_rows)?;
        if paths.is_empty() && !has_real_delta(&checklist.authority) {
            return Err(CommitErrorCode::NoStagedChanges);
        }
        let filter_before = if paths.is_empty() {
            Vec::new()
        } else {
            self.capture_attrs(&paths, cancel.clone()).await?
        };
        if !paths.is_empty() {
            let immediate = self.capture_attrs(&paths, cancel.clone()).await?;
            if immediate != filter_before {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
            self.workspace
                .refresh(cancel.clone())
                .await
                .map_err(map_git_error)?;
            self.require_exact_authority(&checklist.authority, cancel.clone())
                .await?;
            let final_attrs = self.capture_attrs(&paths, cancel.clone()).await?;
            if final_attrs != filter_before {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
        }
        let owner = self
            .workspace
            .begin_owned_refresh(checklist.authority.workspace_generation)
            .map_err(map_git_error)?;
        let mutation = if paths.is_empty() {
            Ok(())
        } else {
            self.run_add(&paths, cancel.clone()).await
        };
        let workspace = self.owner_refresh(owner).await.map_err(map_git_error)?;
        mutation?;
        let b = self.capture(workspace.generation, cancel.clone()).await?;
        if b.head != checklist.authority.head {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
        if paths.is_empty() {
            if b.status_raw != checklist.authority.status_raw
                || b.stage_raw != checklist.authority.stage_raw
                || b.tree_raw != checklist.authority.tree_raw
            {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
        } else {
            let after = self.capture_attrs(&paths, cancel.clone()).await?;
            if after != filter_before {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
            validate_transition(&checklist.authority, &b, &selected_rows, &paths)?;
        }
        if !has_real_delta(&b) {
            return Err(CommitErrorCode::NoStagedChanges);
        }
        let (summary, truncated) = self.capture_summary(&b, cancel).await?;
        Ok((
            StoredPrepared {
                id: PreparedCommitId {
                    generation: 0,
                    slot: 0,
                    seal: 0,
                },
                authority: b,
                summary,
                summary_truncated: truncated,
            },
            workspace,
        ))
    }

    async fn capture_attrs(
        &self,
        paths: &[Vec<u8>],
        cancel: CancellationToken,
    ) -> Result<Vec<u8>, CommitErrorCode> {
        let paths = paths.to_vec();
        let mut input = Vec::new();
        for path in &paths {
            input
                .len()
                .checked_add(path.len() + 1)
                .filter(|size| *size <= SNAPSHOT_LIMIT)
                .ok_or(CommitErrorCode::OutputTooLarge)?;
            input.extend_from_slice(path);
            input.push(0);
        }
        let root = self.root.clone();
        let identity = self.root_identity;
        #[cfg(test)]
        let read_executable = self.read_executable.clone();
        tokio::task::spawn_blocking(move || {
            let runner = Runner::new(
                root,
                identity,
                #[cfg(test)]
                read_executable,
            );
            let output = runner
                .run_with_input(
                    "check-attr",
                    &[
                        OsString::from("-z"),
                        OsString::from("--stdin"),
                        OsString::from("--all"),
                    ],
                    Arc::from(input),
                    SNAPSHOT_LIMIT,
                    &cancel,
                )
                .map_err(map_workspace_error)?;
            validate_filter_attrs(&paths, &output.stdout).map_err(|error| {
                if error.code() == GitWorkspaceErrorCode::GitFailed {
                    CommitErrorCode::UnsafeFilter
                } else {
                    map_workspace_error(error)
                }
            })?;
            Ok(output.stdout)
        })
        .await
        .map_err(|_| CommitErrorCode::GitFailed)?
    }

    async fn run_add(
        &self,
        paths: &[Vec<u8>],
        cancel: CancellationToken,
    ) -> Result<(), CommitErrorCode> {
        let mut input = Vec::new();
        for path in paths {
            input.extend_from_slice(path);
            input.push(0);
        }
        let root = self.root.clone();
        let identity = self.root_identity;
        #[cfg(test)]
        let mutation_executable = self.mutation_executable.clone();
        #[cfg(test)]
        let mutation_timeout = self.mutation_timeout;
        tokio::task::spawn_blocking(move || {
            let runner = Runner::new(
                root,
                identity,
                #[cfg(test)]
                None,
            );
            #[cfg(test)]
            if let Some(executable) = mutation_executable {
                return runner
                    .run_trusted_mutation_with_executable_and_timeout(
                        "add",
                        &[
                            OsString::from("-A"),
                            OsString::from("--pathspec-from-file=-"),
                            OsString::from("--pathspec-file-nul"),
                        ],
                        Arc::from(input),
                        &cancel,
                        &executable,
                        mutation_timeout,
                    )
                    .map(|_| ())
                    .map_err(map_workspace_error);
            }
            runner
                .run_trusted_mutation(
                    "add",
                    &[
                        OsString::from("-A"),
                        OsString::from("--pathspec-from-file=-"),
                        OsString::from("--pathspec-file-nul"),
                    ],
                    Arc::from(input),
                    &cancel,
                )
                .map(|_| ())
                .map_err(map_workspace_error)
        })
        .await
        .map_err(|_| CommitErrorCode::GitFailed)?
    }

    async fn capture_summary(
        &self,
        authority: &IndexAuthority,
        cancel: CancellationToken,
    ) -> Result<(String, bool), CommitErrorCode> {
        self.require_exact_authority(authority, cancel.clone())
            .await?;
        let root = self.root.clone();
        let identity = self.root_identity;
        #[cfg(test)]
        let read_executable = self.read_executable.clone();
        let output = tokio::task::spawn_blocking(move || {
            Runner::new(
                root,
                identity,
                #[cfg(test)]
                read_executable,
            )
            .run_commit_summary(SUMMARY_LIMIT, &cancel)
            .map_err(map_workspace_error)
        })
        .await
        .map_err(|_| CommitErrorCode::GitFailed)??;
        let rendered = escape_summary(&output.stdout)?;
        let (rendered, truncated) = truncate_summary(rendered, output.overflow);
        self.require_exact_authority(authority, CancellationToken::new())
            .await?;
        Ok((rendered, truncated))
    }

    async fn commit_owned(
        &self,
        prepared: &StoredPrepared,
        message: String,
        cancel: CancellationToken,
    ) -> Result<(), CommitErrorCode> {
        self.require_exact_authority(&prepared.authority, cancel.clone())
            .await?;
        let before = prepared.authority.head.clone();
        let root = self.root.clone();
        let identity = self.root_identity;
        #[cfg(test)]
        let mutation_executable = self.mutation_executable.clone();
        #[cfg(test)]
        let mutation_timeout = self.mutation_timeout;
        tokio::task::spawn_blocking(move || {
            let runner = Runner::new(
                root,
                identity,
                #[cfg(test)]
                None,
            );
            let input: Arc<[u8]> = Arc::from(message.into_bytes());
            let args = [
                OsString::from("--no-gpg-sign"),
                OsString::from("--file=-"),
                OsString::from("--cleanup=verbatim"),
            ];
            #[cfg(test)]
            if let Some(executable) = mutation_executable {
                return runner
                    .run_trusted_mutation_with_executable_and_timeout(
                        "commit",
                        &args,
                        input,
                        &cancel,
                        &executable,
                        mutation_timeout,
                    )
                    .map(|_| ())
                    .map_err(map_workspace_error);
            }
            runner
                .run_trusted_mutation("commit", &args, input, &cancel)
                .map(|_| ())
                .map_err(map_workspace_error)
        })
        .await
        .map_err(|_| CommitErrorCode::GitFailed)??;
        self.prove_commit(&before, &prepared.authority).await
    }

    async fn prove_commit(
        &self,
        before: &HeadAuthority,
        prepared: &IndexAuthority,
    ) -> Result<(), CommitErrorCode> {
        let root = self.root.clone();
        let identity = self.root_identity;
        let before = before.clone();
        let expected_tree = prepared.stages.clone();
        #[cfg(test)]
        let read_executable = self.read_executable.clone();
        tokio::task::spawn_blocking(move || {
            let cancel = CancellationToken::new();
            let runner = Runner::new(
                root,
                identity,
                #[cfg(test)]
                read_executable,
            );
            let head = capture_head(&runner, &cancel)?;
            if head.unborn || head.full_ref != before.full_ref || head.oid == before.oid {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
            let mut parent_arg = head.oid.clone();
            parent_arg.extend_from_slice(b"^@");
            let parents = runner
                .run(
                    "rev-parse",
                    &[OsString::from_vec(parent_arg)],
                    STDOUT_LIMIT,
                    &cancel,
                )
                .map_err(map_workspace_error)?;
            let parsed_parents = parse_parent_lines(&parents.stdout, head.oid.len())?;
            if (before.unborn && !parsed_parents.is_empty())
                || (!before.unborn
                    && (parsed_parents.len() != 1 || parsed_parents[0] != before.oid))
            {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
            let tree = runner
                .run(
                    "ls-tree",
                    &[
                        OsString::from("-r"),
                        OsString::from("-z"),
                        OsString::from("--full-tree"),
                        OsString::from_vec(head.oid.clone()),
                    ],
                    SNAPSHOT_LIMIT,
                    &cancel,
                )
                .map_err(map_workspace_error)?;
            let parsed_tree = parse_tree(&tree.stdout, head.oid.len())?;
            if !stage_matches_tree(&expected_tree, &parsed_tree) {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
            let final_head = capture_head(&runner, &cancel)?;
            if final_head != head {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
            Ok(())
        })
        .await
        .map_err(|_| CommitErrorCode::GitFailed)?
    }
}

fn failed_prepare(
    code: CommitErrorCode,
    workspace: Option<WorkspaceSnapshot>,
) -> CommitPrepareCompletion {
    CommitPrepareCompletion {
        prepared: None,
        workspace,
        error: Some(code),
    }
}

fn map_git_error(error: GitWorkspaceError) -> CommitErrorCode {
    map_workspace_error(error)
}

fn map_workspace_error(error: GitWorkspaceError) -> CommitErrorCode {
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

fn capture_authority(
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

fn finalize_authority(
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

fn capture_head(
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

fn parse_branch_headers(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CommitErrorCode> {
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

fn parse_ref_target(bytes: &[u8], wanted: &[u8], width: usize) -> Result<Vec<u8>, CommitErrorCode> {
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

fn parse_commit_status(
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

fn parse_stages(bytes: &[u8], width: usize) -> Result<Vec<StageEntry>, CommitErrorCode> {
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

fn parse_tree(bytes: &[u8], width: usize) -> Result<Vec<TreeEntry>, CommitErrorCode> {
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

fn parse_nul_records<T>(
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

fn canonical_status_pair(shape: StatusShape, x: u8, y: u8) -> bool {
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

fn canonical_status_modes(
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

fn same_mode_kind(left: &[u8], right: &[u8]) -> bool {
    matches!(
        (left, right),
        (b"100644" | b"100755", b"100644" | b"100755")
    ) || left == right
}

fn cross_check_authority(
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

fn project_selection(
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

fn resolve_selected<'a>(
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

fn selected_paths(rows: &[&ChecklistRow]) -> Result<Vec<Vec<u8>>, CommitErrorCode> {
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

fn record_closure(record: &StatusRecord) -> Vec<Vec<u8>> {
    let mut paths = vec![record.path.clone()];
    if let Some(previous) = &record.previous {
        paths.push(previous.clone());
    }
    paths.sort();
    paths.dedup();
    paths
}

fn component_closure(record: &StatusRecord, forced: bool) -> Vec<Vec<u8>> {
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

fn is_gitattributes(path: &[u8]) -> bool {
    path.rsplit(|byte| *byte == b'/').next() == Some(b".gitattributes")
}

fn validate_transition(
    a: &IndexAuthority,
    b: &IndexAuthority,
    selected: &[&ChecklistRow],
    _paths: &[Vec<u8>],
) -> Result<(), CommitErrorCode> {
    let selected_paths: BTreeSet<&[u8]> = selected
        .iter()
        .flat_map(|row| row.closure.iter().map(Vec::as_slice))
        .collect();
    // A selected destination edit on a staged copy/rename never owns the
    // source's independent worktree component. Freeze that outside-S source
    // record byte-exact even though it participates in structural topology.
    for row in selected.iter().filter(|row| {
        matches!(row.record.shape, StatusShape::Rename | StatusShape::Copy)
            && matches!(
                row.optional_kind,
                CommitSelectionKind::Modified | CommitSelectionKind::TypeChanged
            )
    }) {
        let previous = row
            .record
            .previous
            .as_deref()
            .ok_or(CommitErrorCode::ChangedDuringRead)?;
        let a_source = a.records.iter().find(|record| record.path == previous);
        let b_source = b.records.iter().find(|record| record.path == previous);
        let legal_rename_split = row.record.shape == StatusShape::Rename
            && a_source.is_none()
            && b_source.is_some_and(|record| {
                record.shape == StatusShape::Ordinary
                    && record.previous.is_none()
                    && record.x == b'D'
                    && record.y == b'.'
            });
        if a_source != b_source && !legal_rename_split {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
    }
    let mut owners: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (owner, row) in selected.iter().enumerate() {
        let structural_closure =
            if (matches!(row.record.shape, StatusShape::Rename | StatusShape::Copy)
                && matches!(
                    row.optional_kind,
                    CommitSelectionKind::Modified | CommitSelectionKind::TypeChanged
                ))
                || (row.record.shape == StatusShape::Rename
                    && row.optional_kind == CommitSelectionKind::Deleted)
            {
                record_closure(&row.record)
            } else {
                row.closure.clone()
            };
        for path in &structural_closure {
            let path_owners = owners.entry(path.clone()).or_default();
            if !path_owners.is_empty() {
                let shared_copy_source = row.record.shape == StatusShape::Copy
                    && row.record.previous.as_deref() == Some(path.as_slice())
                    && path_owners.iter().all(|existing| {
                        selected[*existing].record.shape == StatusShape::Copy
                            && selected[*existing].record.previous.as_deref()
                                == Some(path.as_slice())
                    });
                if !shared_copy_source {
                    return Err(CommitErrorCode::InvalidSelection);
                }
            }
            path_owners.push(owner);
        }
    }
    let a_stage: BTreeMap<&[u8], &StageEntry> = a
        .stages
        .iter()
        .map(|entry| (entry.path.as_slice(), entry))
        .collect();
    let b_stage: BTreeMap<&[u8], &StageEntry> = b
        .stages
        .iter()
        .map(|entry| (entry.path.as_slice(), entry))
        .collect();
    for (path, entry) in &a_stage {
        if !selected_paths.contains(path) && b_stage.get(path).copied() != Some(*entry) {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
    }
    for (path, entry) in &b_stage {
        if !selected_paths.contains(path) && a_stage.get(path).copied() != Some(*entry) {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
    }
    let mut stage_paths = BTreeSet::new();
    stage_paths.extend(a_stage.keys().copied());
    stage_paths.extend(b_stage.keys().copied());
    for path in stage_paths {
        if a_stage.get(path) != b_stage.get(path) && !owners.contains_key(path) {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
    }
    for record in &a.records {
        if !record_closure(record)
            .iter()
            .any(|path| selected_paths.contains(path.as_slice()))
            && !b.records.contains(record)
        {
            return Err(CommitErrorCode::ChangedDuringRead);
        }
    }
    for (record, other) in a
        .records
        .iter()
        .map(|record| (record, b.records.as_slice()))
        .chain(
            b.records
                .iter()
                .map(|record| (record, a.records.as_slice())),
        )
    {
        if !other.contains(record) {
            let same_topology = other.iter().any(|candidate| {
                candidate.path == record.path
                    && candidate.previous == record.previous
                    && candidate.shape == record.shape
                    && candidate.x == record.x
            });
            let closure = if same_topology {
                component_closure(record, false)
            } else {
                record_closure(record)
            };
            let mut candidates: Option<BTreeSet<usize>> = None;
            for path in closure {
                let path_owners: BTreeSet<usize> = owners
                    .get(path.as_slice())
                    .ok_or(CommitErrorCode::ChangedDuringRead)?
                    .iter()
                    .copied()
                    .collect();
                candidates = Some(match candidates {
                    None => path_owners,
                    Some(current) => current.intersection(&path_owners).copied().collect(),
                });
            }
            let selected_delete_untracked_merge = record.shape == StatusShape::Rename
                && record.previous.as_ref().is_some_and(|previous| {
                    is_selected_delete_untracked_rename(
                        selected,
                        &b.records,
                        previous,
                        &record.path,
                    )
                });
            if candidates.is_none_or(|candidates| candidates.len() != 1)
                && !selected_delete_untracked_merge
            {
                return Err(CommitErrorCode::ChangedDuringRead);
            }
        }
    }
    for row in selected {
        let a_record = &row.record;
        let b_record = b
            .records
            .iter()
            .find(|record| record.path == a_record.path && record.previous == a_record.previous);
        match row.optional_kind {
            CommitSelectionKind::Deleted => {
                let merged_rename = selected.iter().any(|candidate| {
                    candidate.optional_kind == CommitSelectionKind::Added
                        && candidate.record.shape == StatusShape::Untracked
                        && is_selected_delete_untracked_rename(
                            selected,
                            &b.records,
                            &a_record.path,
                            &candidate.record.path,
                        )
                });
                let canonical_delete = if a_record.shape == StatusShape::Rename {
                    a_record.previous.as_ref().is_some_and(|previous| {
                        let mut old_records =
                            b.records.iter().filter(|record| record.path == *previous);
                        old_records.next().is_some_and(|record| {
                            record.shape == StatusShape::Ordinary
                                && record.previous.is_none()
                                && record.x == b'D'
                                && record.y == b'.'
                        }) && old_records.next().is_none()
                            && !b.records.iter().any(|record| {
                                record.previous.as_deref() == Some(previous.as_slice())
                            })
                            && !b_stage.contains_key(previous.as_slice())
                            && !b.records.iter().any(|record| record.path == a_record.path)
                    })
                } else {
                    b_record.is_some_and(|record| record.x == b'D' && record.y == b'.')
                        || merged_rename
                };
                if b_stage.contains_key(a_record.path.as_slice()) || !canonical_delete {
                    return Err(CommitErrorCode::ChangedDuringRead);
                }
            }
            CommitSelectionKind::Renamed => {
                let Some(record) = b_record else {
                    return Err(CommitErrorCode::ChangedDuringRead);
                };
                let Some(previous) = &a_record.previous else {
                    return Err(CommitErrorCode::ChangedDuringRead);
                };
                let exact_mode = row.worktree_mode.as_ref().is_some_and(|mode| {
                    b_stage
                        .get(a_record.path.as_slice())
                        .is_some_and(|entry| entry.mode == *mode)
                });
                if !exact_mode
                    || record.shape != StatusShape::Rename
                    || record.previous.as_ref() != Some(previous)
                    || record.path != a_record.path
                    || record.y != b'.'
                    || b_stage.contains_key(previous.as_slice())
                    || !b_stage.contains_key(a_record.path.as_slice())
                {
                    return Err(CommitErrorCode::ChangedDuringRead);
                }
            }
            CommitSelectionKind::Copied => {
                let Some(previous) = &a_record.previous else {
                    return Err(CommitErrorCode::ChangedDuringRead);
                };
                let Some(record) = b_record else {
                    return Err(CommitErrorCode::ChangedDuringRead);
                };
                let exact_mode = row.worktree_mode.as_ref().is_some_and(|mode| {
                    b_stage
                        .get(a_record.path.as_slice())
                        .is_some_and(|entry| entry.mode == *mode)
                });
                if !exact_mode
                    || a_stage.get(previous.as_slice()) != b_stage.get(previous.as_slice())
                    || !b_stage.contains_key(a_record.path.as_slice())
                    || record.y != b'.'
                    || !matches!(record.x, b'A' | b'C')
                    || (record.x == b'C'
                        && (record.shape != StatusShape::Copy
                            || record.previous.as_ref() != Some(previous)))
                {
                    return Err(CommitErrorCode::ChangedDuringRead);
                }
            }
            CommitSelectionKind::Added => {
                let merged_rename = selected.iter().any(|candidate| {
                    candidate.optional_kind == CommitSelectionKind::Deleted
                        && is_selected_delete_untracked_rename(
                            selected,
                            &b.records,
                            &candidate.record.path,
                            &a_record.path,
                        )
                });
                let expected_mode = row.worktree_mode.as_ref().is_some_and(|mode| {
                    b_stage
                        .get(a_record.path.as_slice())
                        .is_some_and(|entry| entry.mode == *mode)
                });
                if !expected_mode
                    || (!merged_rename
                        && b_record.is_none_or(|record| record.x != b'A' || record.y != b'.'))
                {
                    return Err(CommitErrorCode::ChangedDuringRead);
                }
            }
            CommitSelectionKind::Modified | CommitSelectionKind::TypeChanged => {
                let normalized_noop = !has_real_delta(b)
                    && row.optional_kind == CommitSelectionKind::Modified
                    && a_record.shape == StatusShape::Ordinary
                    && a_record.x == b'.'
                    && a_record.y == b'M'
                    && b_record.is_none()
                    && a_stage.get(a_record.path.as_slice())
                        == b_stage.get(a_record.path.as_slice())
                    && b_stage.get(a_record.path.as_slice()).is_some_and(|entry| {
                        entry.mode == a_record.worktree_mode
                            && b.tree.iter().any(|tree| {
                                tree.path == entry.path
                                    && tree.mode == entry.mode
                                    && tree.oid == entry.oid
                            })
                    });
                if normalized_noop {
                    continue;
                }
                let Some(entry) = b_stage.get(a_record.path.as_slice()) else {
                    return Err(CommitErrorCode::ChangedDuringRead);
                };
                let exact_topology = b_record.is_some_and(|record| {
                    let expected_x = if a_record.x == b'.' {
                        matches!(record.x, b'M' | b'T')
                    } else {
                        record.x == a_record.x
                            && record.shape == a_record.shape
                            && record.previous == a_record.previous
                    };
                    record.y == b'.' && expected_x
                });
                let split_rename = if a_record.shape == StatusShape::Rename {
                    a_record.previous.as_ref().is_some_and(|previous| {
                        b.records.iter().any(|record| {
                            record.shape == StatusShape::Ordinary
                                && record.path == a_record.path
                                && record.previous.is_none()
                                && record.x == b'A'
                                && record.y == b'.'
                        }) && b.records.iter().any(|record| {
                            record.shape == StatusShape::Ordinary
                                && record.path == *previous
                                && record.previous.is_none()
                                && record.x == b'D'
                                && record.y == b'.'
                        }) && !b_stage.contains_key(previous.as_slice())
                    })
                } else {
                    false
                };
                let split_copy = if a_record.shape == StatusShape::Copy {
                    a_record.previous.as_ref().is_some_and(|previous| {
                        b.records.iter().any(|record| {
                            record.shape == StatusShape::Ordinary
                                && record.path == a_record.path
                                && record.previous.is_none()
                                && record.x == b'A'
                                && record.y == b'.'
                        }) && a_stage.get(previous.as_slice()) == b_stage.get(previous.as_slice())
                    })
                } else {
                    false
                };
                if (!exact_topology && !split_rename && !split_copy)
                    || entry.mode != a_record.worktree_mode
                {
                    return Err(CommitErrorCode::ChangedDuringRead);
                }
            }
        }
    }
    if b.records.iter().any(|record| {
        record.y != b'.'
            && record_closure(record)
                .iter()
                .any(|path| selected_paths.contains(path.as_slice()))
    }) {
        return Err(CommitErrorCode::ChangedDuringRead);
    }
    Ok(())
}

fn is_selected_delete_untracked_rename(
    selected: &[&ChecklistRow],
    b_records: &[StatusRecord],
    source: &[u8],
    destination: &[u8],
) -> bool {
    let source_selected = selected.iter().filter(|row| {
        row.optional_kind == CommitSelectionKind::Deleted
            && row.record.shape == StatusShape::Ordinary
            && row.record.x == b'.'
            && row.record.y == b'D'
            && row.record.previous.is_none()
            && row.record.path == source
    });
    let destination_selected = selected.iter().filter(|row| {
        row.optional_kind == CommitSelectionKind::Added
            && row.record.shape == StatusShape::Untracked
            && row.record.previous.is_none()
            && row.record.path == destination
    });
    if source_selected.count() != 1 || destination_selected.count() != 1 {
        return false;
    }
    let mut touching = b_records.iter().filter(|record| {
        record.path == source
            || record.path == destination
            || record
                .previous
                .as_deref()
                .is_some_and(|previous| previous == source || previous == destination)
    });
    let Some(merged) = touching.next() else {
        return false;
    };
    touching.next().is_none()
        && merged.shape == StatusShape::Rename
        && merged.x == b'R'
        && merged.y == b'.'
        && merged.path == destination
        && merged.previous.as_deref() == Some(source)
}

fn optional_kind(record: &StatusRecord) -> CommitSelectionKind {
    match record.y {
        b'D' => CommitSelectionKind::Deleted,
        b'T' => CommitSelectionKind::TypeChanged,
        b'?' | b'A' => CommitSelectionKind::Added,
        b'R' => CommitSelectionKind::Renamed,
        b'C' => CommitSelectionKind::Copied,
        _ => CommitSelectionKind::Modified,
    }
}

fn has_real_delta(authority: &IndexAuthority) -> bool {
    !stage_matches_tree(&authority.stages, &authority.tree)
}

fn stage_matches_tree(stages: &[StageEntry], tree: &[TreeEntry]) -> bool {
    stages.len() == tree.len()
        && stages.iter().zip(tree).all(|(stage, tree)| {
            stage.mode == tree.mode && stage.oid == tree.oid && stage.path == tree.path
        })
}

fn logical_path_count(
    records: &[StatusRecord],
    stages: &[StageEntry],
    tree: &[TreeEntry],
) -> Result<usize, CommitErrorCode> {
    let mut paths = BTreeSet::new();
    for record in records {
        paths.insert(record.path.as_slice());
        if let Some(previous) = &record.previous {
            paths.insert(previous.as_slice());
        }
    }
    paths.extend(stages.iter().map(|entry| entry.path.as_slice()));
    paths.extend(tree.iter().map(|entry| entry.path.as_slice()));
    Ok(paths.len())
}

fn parse_parent_lines(bytes: &[u8], width: usize) -> Result<Vec<Vec<u8>>, CommitErrorCode> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(b"\n") {
        return Err(CommitErrorCode::MalformedOutput);
    }
    bytes[..bytes.len() - 1]
        .split(|byte| *byte == b'\n')
        .map(|line| {
            valid_oid_width(line, width)
                .then(|| line.to_vec())
                .ok_or(CommitErrorCode::MalformedOutput)
        })
        .collect()
}

struct EscapedSummary {
    rendered: String,
    marker_cut: usize,
}

impl EscapedSummary {
    fn new() -> Self {
        Self {
            rendered: String::new(),
            marker_cut: 0,
        }
    }

    fn record_boundary(&mut self) {
        let marker_target = SUMMARY_LIMIT - SUMMARY_MARKER.len();
        if self.rendered.len() <= marker_target {
            self.marker_cut = self.rendered.len();
        }
    }

    fn push_literal(&mut self, character: char) {
        self.rendered.push(character);
        self.record_boundary();
    }

    fn push_generated_escape(&mut self, byte: u8) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        self.rendered.push('\\');
        self.rendered.push('x');
        self.rendered.push(char::from(HEX[usize::from(byte >> 4)]));
        self.rendered
            .push(char::from(HEX[usize::from(byte & 0x0f)]));
        self.record_boundary();
    }
}

fn escape_summary(raw: &[u8]) -> Result<EscapedSummary, CommitErrorCode> {
    let mut escaped = EscapedSummary::new();
    let mut index = 0;
    while index < raw.len() {
        match std::str::from_utf8(&raw[index..]) {
            Ok(valid) => {
                push_escaped_controls(&mut escaped, valid.as_bytes())?;
                break;
            }
            Err(error) => {
                let valid = &raw[index..index + error.valid_up_to()];
                push_escaped_controls(&mut escaped, valid)?;
                index += error.valid_up_to();
                let invalid = error.error_len().unwrap_or(1);
                for byte in &raw[index..index + invalid] {
                    escaped.push_generated_escape(*byte);
                }
                index += invalid;
            }
        }
    }
    Ok(escaped)
}

fn push_escaped_controls(target: &mut EscapedSummary, bytes: &[u8]) -> Result<(), CommitErrorCode> {
    let value = std::str::from_utf8(bytes).map_err(|_| CommitErrorCode::MalformedOutput)?;
    for character in value.chars() {
        if character == '\n' || character == '\t' || !character.is_control() {
            target.push_literal(character);
        } else {
            for byte in character.to_string().as_bytes() {
                target.push_generated_escape(*byte);
            }
        }
    }
    Ok(())
}

fn truncate_summary(mut escaped: EscapedSummary, raw_overflow: bool) -> (String, bool) {
    if escaped.rendered.len() <= SUMMARY_LIMIT && !raw_overflow {
        return (escaped.rendered, false);
    }
    escaped.rendered.truncate(escaped.marker_cut);
    escaped
        .rendered
        .push_str(std::str::from_utf8(SUMMARY_MARKER).unwrap_or(""));
    (escaped.rendered, true)
}

async fn collect_draft(
    provider: Arc<dyn Provider>,
    request: ChatRequest,
    cancel: CancellationToken,
) -> Result<String, CommitErrorCode> {
    let mut stream = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(CommitErrorCode::DraftFailed),
        result = provider.chat_stream(request, cancel.clone()) => {
            result.map_err(|_| CommitErrorCode::DraftFailed)?
        }
    };
    let mut text = String::new();
    let mut usage_started = false;
    let mut done = false;
    loop {
        let item = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(CommitErrorCode::DraftFailed),
            item = stream.next() => item,
        };
        match item {
            Some(Ok(ProviderEvent::TextDelta(delta))) if !usage_started && !done => {
                checked_draft_len(text.len(), delta.len())?;
                if delta.as_bytes().contains(&0) {
                    return Err(CommitErrorCode::DraftFailed);
                }
                text.push_str(&delta);
            }
            Some(Ok(ProviderEvent::Usage { .. })) if !done => usage_started = true,
            Some(Ok(ProviderEvent::Done {
                stop_reason: StopReason::End,
            })) if !done => done = true,
            None if done && !text.is_empty() => return Ok(text),
            _ => return Err(CommitErrorCode::DraftFailed),
        }
    }
}

async fn collect_draft_with_deadline(
    provider: Arc<dyn Provider>,
    request: ChatRequest,
    cancel: CancellationToken,
    deadline: Duration,
) -> Result<String, CommitErrorCode> {
    tokio::time::timeout(deadline, collect_draft(provider, request, cancel))
        .await
        .map_err(|_| CommitErrorCode::DraftFailed)?
}

fn checked_draft_len(current: usize, delta: usize) -> Result<usize, CommitErrorCode> {
    let next = current
        .checked_add(delta)
        .ok_or(CommitErrorCode::DraftFailed)?;
    if next > MESSAGE_LIMIT {
        return Err(CommitErrorCode::DraftFailed);
    }
    Ok(next)
}

fn exact_line(bytes: &[u8]) -> Result<&[u8], CommitErrorCode> {
    bytes
        .strip_suffix(b"\n")
        .filter(|line| !line.is_empty() && !line.contains(&b'\n') && !line.contains(&0))
        .ok_or(CommitErrorCode::MalformedOutput)
}

fn valid_status_code(code: u8) -> bool {
    matches!(code, b'.' | b'M' | b'T' | b'A' | b'D' | b'R' | b'C' | b'U')
}

fn valid_mode_or_zero(mode: &[u8]) -> bool {
    is_zero_mode(mode) || valid_index_mode(mode)
}

fn valid_index_mode(mode: &[u8]) -> bool {
    matches!(mode, b"100644" | b"100755" | b"120000" | b"160000")
}

fn is_zero_mode(mode: &[u8]) -> bool {
    mode == b"000000"
}

fn valid_oid_or_zero(oid: &[u8], width: usize) -> bool {
    is_zero_oid(oid) || valid_oid_width(oid, width)
}

fn valid_nonzero_oid(oid: &[u8], width: usize) -> bool {
    valid_oid_width(oid, width) && !is_zero_oid(oid)
}

fn is_zero_oid(oid: &[u8]) -> bool {
    !oid.is_empty() && oid.iter().all(|byte| *byte == b'0')
}

fn valid_oid_width(oid: &[u8], width: usize) -> bool {
    oid.len() == width
        && matches!(width, 40 | 64)
        && oid
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn parse_score(score: &[u8]) -> Result<StatusShape, CommitErrorCode> {
    let (&kind, digits) = score
        .split_first()
        .ok_or(CommitErrorCode::MalformedOutput)?;
    let value = std::str::from_utf8(digits)
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|value| *value <= 100)
        .ok_or(CommitErrorCode::MalformedOutput)?;
    let _ = value;
    match kind {
        b'R' => Ok(StatusShape::Rename),
        b'C' => Ok(StatusShape::Copy),
        _ => Err(CommitErrorCode::MalformedOutput),
    }
}

fn validate_ref_short(short: &[u8]) -> Result<(), CommitErrorCode> {
    if short.is_empty()
        || short[0] == b'-'
        || short == b"HEAD"
        || short == b"@"
        || short.starts_with(b"/")
        || short.ends_with(b"/")
        || short.ends_with(b".")
        || short
            .windows(2)
            .any(|window| window == b".." || window == b"//" || window == b"@{")
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
        return Err(CommitErrorCode::UnsafeRepository);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;
    use std::process::Command;
    use std::sync::atomic::AtomicUsize;

    struct Repo {
        dir: tempfile::TempDir,
    }

    impl Repo {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp repo");
            run_git(dir.path(), &["init", "-q"]);
            run_git(dir.path(), &["config", "user.name", "Vega Test"]);
            run_git(
                dir.path(),
                &["config", "user.email", "vega@example.invalid"],
            );
            fs::write(dir.path().join("tracked.txt"), "base\n").expect("write fixture");
            run_git(dir.path(), &["add", "tracked.txt"]);
            run_git(dir.path(), &["commit", "-qm", "base"]);
            Self { dir }
        }

        fn unborn() -> Self {
            let dir = tempfile::tempdir().expect("temp unborn repo");
            run_git(dir.path(), &["init", "-q"]);
            run_git(dir.path(), &["config", "user.name", "Vega Test"]);
            run_git(
                dir.path(),
                &["config", "user.email", "vega@example.invalid"],
            );
            Self { dir }
        }

        fn try_sha256() -> Result<Self, String> {
            let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
            let mut init = Command::new(GIT);
            init.current_dir(dir.path())
                .args(["init", "--object-format=sha256", "-q"]);
            scrub_git_environment(&mut init);
            let output = init.output().map_err(|error| error.to_string())?;
            if !output.status.success() {
                return Err(format!(
                    "git init --object-format=sha256 unsupported: status={:?}, stderr={}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            run_git(dir.path(), &["config", "user.name", "Vega Test"]);
            run_git(
                dir.path(),
                &["config", "user.email", "vega@example.invalid"],
            );
            fs::write(dir.path().join("tracked.txt"), "base\n")
                .map_err(|error| error.to_string())?;
            run_git(dir.path(), &["add", "tracked.txt"]);
            run_git(dir.path(), &["commit", "-qm", "base"]);
            Ok(Self { dir })
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        async fn services(&self) -> (Arc<GitWorkspaceService>, TrustedGitService) {
            let workspace = Arc::new(GitWorkspaceService::new(self.path()).expect("workspace"));
            workspace
                .refresh(CancellationToken::new())
                .await
                .expect("workspace refresh");
            let trusted = TrustedGitService::new(self.path(), workspace.clone()).expect("trusted");
            (workspace, trusted)
        }
    }

    fn run_git(root: &Path, args: &[&str]) {
        let mut command = Command::new(GIT);
        command.current_dir(root).args(args);
        scrub_git_environment(&mut command);
        let status = command.status().expect("git fixture");
        assert!(status.success(), "git fixture failed: {args:?}");
    }

    fn run_git_output(root: &Path, args: &[&str]) -> Vec<u8> {
        let mut command = Command::new(GIT);
        command.current_dir(root).args(args);
        scrub_git_environment(&mut command);
        let output = command.output().expect("git output fixture");
        assert!(output.status.success(), "git output failed: {args:?}");
        output.stdout
    }

    fn test_head(unborn: bool, width: usize) -> HeadAuthority {
        HeadAuthority {
            unborn,
            oid: vec![if unborn { b'0' } else { b'a' }; width],
            short: b"master".to_vec(),
            full_ref: b"refs/heads/master".to_vec(),
        }
    }

    fn status_prefix(head: &HeadAuthority) -> Vec<u8> {
        let mut bytes = b"# branch.oid ".to_vec();
        if head.unborn {
            bytes.extend_from_slice(b"(initial)");
        } else {
            bytes.extend_from_slice(&head.oid);
        }
        bytes.extend_from_slice(b"\0# branch.head ");
        bytes.extend_from_slice(&head.short);
        bytes.push(0);
        bytes
    }

    fn stage_record(mode: &[u8], oid: &[u8], path: &[u8]) -> Vec<u8> {
        let mut bytes = mode.to_vec();
        bytes.push(b' ');
        bytes.extend_from_slice(oid);
        bytes.extend_from_slice(b" 0\t");
        bytes.extend_from_slice(path);
        bytes.push(0);
        bytes
    }

    fn tree_record(mode: &[u8], object_type: &[u8], oid: &[u8], path: &[u8]) -> Vec<u8> {
        let mut bytes = mode.to_vec();
        bytes.push(b' ');
        bytes.extend_from_slice(object_type);
        bytes.push(b' ');
        bytes.extend_from_slice(oid);
        bytes.push(b'\t');
        bytes.extend_from_slice(path);
        bytes.push(0);
        bytes
    }

    fn status_rc_record(
        kind: u8,
        head_oid: &[u8],
        index_oid: &[u8],
        current: &[u8],
        previous: &[u8],
    ) -> Vec<u8> {
        let mut bytes = b"2 ".to_vec();
        bytes.push(kind);
        bytes.extend_from_slice(b". N... 100644 100644 100644 ");
        bytes.extend_from_slice(head_oid);
        bytes.push(b' ');
        bytes.extend_from_slice(index_oid);
        bytes.push(b' ');
        bytes.push(kind);
        bytes.extend_from_slice(b"100 ");
        bytes.extend_from_slice(current);
        bytes.push(0);
        bytes.extend_from_slice(previous);
        bytes.push(0);
        bytes
    }

    fn mutation_recorder() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("recorder tempdir");
        let script = dir.path().join("mutation-recorder.sh");
        let argv = dir.path().join("mutation-argv.bin");
        let input = dir.path().join("mutation-input.bin");
        let attempts = dir.path().join("mutation-attempts");
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\nprintf x >> '{}'\n: > '{}'\nfor arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{}'; done\n/usr/bin/tee '{}' | /usr/bin/git \"$@\"\n",
                quote(&attempts),
                quote(&argv),
                quote(&argv),
                quote(&input),
            ),
        )
        .expect("recorder script");
        let mut permissions = fs::metadata(&script)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("script executable");
        (dir, script, argv, input)
    }

    fn blocking_mutation() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("blocking mutation tempdir");
        let script = dir.path().join("blocking-mutation.sh");
        let ready = dir.path().join("ready");
        let release = dir.path().join("release");
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\n/usr/bin/git \"$@\"\n: > '{}'\nwhile [ ! -e '{}' ]; do /bin/sleep 0.01; done\n",
                quote(&ready),
                quote(&release),
            ),
        )
        .expect("blocking script");
        let mut permissions = fs::metadata(&script)
            .expect("blocking metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("blocking executable");
        (dir, script, ready, release)
    }

    fn blocking_before_mutation() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("pre-mutation tempdir");
        let script = dir.path().join("pre-mutation.sh");
        let ready = dir.path().join("ready");
        let release = dir.path().join("release");
        let attempts = dir.path().join("mutation-attempts");
        let argv = dir.path().join("mutation-argv.bin");
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\nprintf x >> '{}'\n: > '{}'\nfor arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{}'; done\n: > '{}'\nwhile [ ! -e '{}' ]; do /bin/sleep 0.01; done\nexec /usr/bin/git \"$@\"\n",
                quote(&attempts),
                quote(&argv),
                quote(&argv),
                quote(&ready),
                quote(&release),
            ),
        )
        .expect("pre-mutation script");
        let mut permissions = fs::metadata(&script)
            .expect("pre-mutation metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("pre-mutation executable");
        (dir, script, ready, release)
    }

    fn fail_first_status_after_trigger(trigger: &Path) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("read fault tempdir");
        let script = dir.path().join("read-fault.sh");
        let failed = dir.path().join("failed-once");
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\nis_status=0\nfor arg in \"$@\"; do [ \"$arg\" = status ] && is_status=1 || true; done\nif [ \"$is_status\" = 1 ] && [ -e '{}' ] && [ ! -e '{}' ]; then : > '{}'; exit 7; fi\nexec /usr/bin/git \"$@\"\n",
                quote(trigger),
                quote(&failed),
                quote(&failed),
            ),
        )
        .expect("read fault script");
        let mut permissions = fs::metadata(&script)
            .expect("read fault metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("read fault executable");
        (dir, script, failed)
    }

    fn scripted_mutation(body: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("scripted mutation tempdir");
        let script = dir.path().join("mutation.sh");
        let attempts = dir.path().join("attempts");
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\nprintf x >> '{}'\n{}\n",
                quote(&attempts),
                body
            ),
        )
        .expect("scripted mutation");
        let mut permissions = fs::metadata(&script)
            .expect("scripted mutation metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("scripted mutation executable");
        (dir, script, attempts)
    }

    fn before_git_mutation(body: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("before-git fixture");
        let script = dir.path().join("before-git.sh");
        let attempts = dir.path().join("attempts");
        let argv = dir.path().join("argv");
        let input = dir.path().join("input");
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\nprintf x >> '{}'\n: > '{}'\nfor arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{}'; done\n/usr/bin/tee '{}' >/dev/null\n{}\n",
                quote(&attempts),
                quote(&argv),
                quote(&argv),
                quote(&input),
                body
            ),
        )
        .expect("before-git script");
        let mut permissions = fs::metadata(&script)
            .expect("before-git metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("before-git executable");
        (dir, script, attempts, argv, input)
    }

    fn after_git_mutation(plan: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("after-git fixture");
        let script = dir.path().join("after-git.sh");
        let attempts = dir.path().join("attempts");
        let argv = dir.path().join("argv");
        let input = dir.path().join("input");
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        let tail = match plan {
            "nonzero" => "exit 17".to_string(),
            "stdout-exact" => format!(
                "/usr/bin/python3 -c 'import sys; sys.stdout.buffer.write(b\"x\" * {})'",
                MUTATION_STDOUT_LIMIT
            ),
            "stdout-overflow" => format!(
                "/usr/bin/python3 -c 'import sys; sys.stdout.buffer.write(b\"x\" * {})'",
                MUTATION_STDOUT_LIMIT + 1
            ),
            "stderr-exact" => format!(
                "/usr/bin/python3 -c 'import sys; sys.stderr.buffer.write(b\"x\" * {})'",
                STDERR_LIMIT
            ),
            "stderr-overflow" => format!(
                "/usr/bin/python3 -c 'import sys; sys.stderr.buffer.write(b\"x\" * {})'",
                STDERR_LIMIT + 1
            ),
            "wait" => "/bin/sleep 30".to_string(),
            "inherited-pipe" => "/bin/sleep 30 & exit 0".to_string(),
            _ => panic!("unknown after-git plan"),
        };
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\nprintf x >> '{}'\n: > '{}'\nfor arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{}'; done\n/usr/bin/tee '{}' | /usr/bin/git \"$@\" >/dev/null\n{}\n",
                quote(&attempts),
                quote(&argv),
                quote(&argv),
                quote(&input),
                tail
            ),
        )
        .expect("after-git script");
        let mut permissions = fs::metadata(&script)
            .expect("after-git metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("after-git executable");
        (dir, script, attempts, argv, input)
    }

    fn proof_read_recorder(
        root: &Path,
        base_oid: &[u8],
        plan: &str,
    ) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("proof recorder tempdir");
        let script = dir.path().join("read-recorder.sh");
        let log = dir.path().join("read-argv.bin");
        let base = dir.path().join("base-oid");
        let attached_ref_file = dir.path().join("attached-ref");
        let status_count = dir.path().join("post-status-count");
        let root_backup = dir.path().join("root-backup");
        fs::write(&base, base_oid).expect("base oid");
        let attached_ref = run_git_output(root, &["symbolic-ref", "HEAD"]);
        fs::write(
            &attached_ref_file,
            attached_ref
                .strip_suffix(b"\n")
                .expect("attached ref newline"),
        )
        .expect("attached ref");
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &script,
            format!(
                r#"#!/bin/sh
set -eu
base=$(/bin/cat '{base}')
attached_ref=$(/bin/cat '{attached_ref_file}')
current=$(/usr/bin/git rev-parse --verify HEAD 2>/dev/null || true)
phase=pre
[ "$current" != "$base" ] && phase=post
printf '%s\0' "$phase" >> '{log}'
verb=
parent_arg=
for arg in "$@"; do
  printf '%s\0' "$arg" >> '{log}'
  case "$arg" in status|rev-parse|ls-tree|for-each-ref) verb="$arg" ;; esac
  case "$arg" in *'^@') parent_arg="$arg" ;; esac
done
printf '\n' >> '{log}'
if [ "$phase" = post ] && [ -n "$parent_arg" ]; then
  case '{plan}' in
    zero-parent) exit 0 ;;
    wrong-parent) printf '%s\n' "$current"; exit 0 ;;
    two-parent) printf '%s\n%s\n' "$base" "$base"; exit 0 ;;
    malformed-parent) printf 'not-an-oid\n'; exit 0 ;;
    short-parent) printf '0123456789abcdef\n'; exit 0 ;;
    mixed-parent) printf '%064d\n' 0; exit 0 ;;
    object-missing)
      prefix=$(printf '%s' "$current" | /usr/bin/cut -c1-2)
      suffix=$(printf '%s' "$current" | /usr/bin/cut -c3-)
      object=$(/usr/bin/git rev-parse --git-path "objects/$prefix/$suffix")
      /bin/mv "$object" "$object.vega-test"
      status=0
      /usr/bin/git "$@" >/dev/null 2>&1 || status=$?
      /bin/mv "$object.vega-test" "$object"
      exit "$status"
      ;;
  esac
fi
if [ "$phase" = post ] && [ "$verb" = ls-tree ] && [ '{plan}' = tree-diff ]; then
  exec /usr/bin/git ls-tree -r -z --full-tree "$base"
fi
if [ "$phase" = post ] && [ "$verb" = status ]; then
  count=0
  [ -e '{status_count}' ] && count=$(/bin/cat '{status_count}')
  count=$((count + 1))
  printf '%s' "$count" > '{status_count}'
  if [ "$count" = 1 ] && [ '{plan}' = root-swap ]; then
    /bin/mv '{root}' '{root_backup}'
    /bin/mkdir '{root}'
  fi
  if [ "$count" = 2 ]; then
    case '{plan}' in
      ref-moved) /usr/bin/git update-ref "$attached_ref" "$base" ;;
      ref-deleted) /usr/bin/git update-ref -d "$attached_ref" ;;
      ref-renamed) /usr/bin/git branch -m renamed-after-proof ;;
    esac
  fi
fi
exec /usr/bin/git "$@"
"#,
                base = quote(&base),
                attached_ref_file = quote(&attached_ref_file),
                log = quote(&log),
                status_count = quote(&status_count),
                plan = plan,
                root = quote(root),
                root_backup = quote(&root_backup),
            ),
        )
        .expect("proof recorder script");
        let mut permissions = fs::metadata(&script)
            .expect("proof recorder metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("proof recorder executable");
        (dir, script, log)
    }

    fn blocking_summary_reader() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("summary reader tempdir");
        let script = dir.path().join("summary-reader.sh");
        let ready = dir.path().join("summary-drained");
        let release = dir.path().join("summary-release");
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\nis_summary=false\nfor arg in \"$@\"; do [ \"$arg\" = --patch ] && is_summary=true; done\n/usr/bin/git \"$@\"\nstatus=$?\nif [ \"$is_summary\" = true ]; then : > '{}'; while [ ! -e '{}' ]; do /bin/sleep 0.01; done; fi\nexit \"$status\"\n",
                quote(&ready),
                quote(&release),
            ),
        )
        .expect("summary reader script");
        let mut permissions = fs::metadata(&script)
            .expect("summary reader metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("summary reader executable");
        (dir, script, ready, release)
    }

    fn read_invocations(path: &Path) -> Vec<Vec<Vec<u8>>> {
        fs::read(path)
            .expect("read invocation log")
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| {
                line.split(|byte| *byte == 0)
                    .filter(|field| !field.is_empty())
                    .map(<[u8]>::to_vec)
                    .collect()
            })
            .collect()
    }

    fn test_runner(root: &Path) -> Runner {
        let canonical = fs::canonicalize(root).expect("canonical test root");
        let metadata = fs::metadata(&canonical).expect("test root metadata");
        Runner::new(
            canonical,
            RootIdentity {
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
            None,
        )
    }

    fn run_fake_mutation(
        runner: &Runner,
        verb: &'static str,
        executable: &Path,
        input: Arc<[u8]>,
        cancel: &CancellationToken,
        timeout: Duration,
    ) -> Result<Output, GitWorkspaceError> {
        runner.run_trusted_mutation_with_executable_and_timeout(
            verb,
            &[],
            input,
            cancel,
            executable,
            timeout,
        )
    }

    fn mutation_error_code(result: Result<Output, GitWorkspaceError>) -> GitWorkspaceErrorCode {
        match result {
            Ok(_) => panic!("mutation unexpectedly succeeded"),
            Err(error) => error.code(),
        }
    }

    async fn wait_for_path(path: &Path) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !path.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("barrier ready");
    }

    fn expected_mutation_argv(verb: &[u8], tail: &[&[u8]]) -> Vec<u8> {
        let mut expected = Vec::new();
        for argument in PREFIX
            .iter()
            .map(|value| value.as_bytes())
            .chain([b"-c".as_slice(), b"core.hooksPath=/dev/null", verb])
            .chain(tail.iter().copied())
        {
            expected.extend_from_slice(argument);
            expected.push(0);
        }
        expected
    }

    fn assert_terminal_workspace(trusted: &TrustedGitService, terminal: &WorkspaceSnapshot) {
        let workspace = trusted
            .workspace
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(workspace.snapshot.as_ref(), Some(terminal));
        assert!(workspace.active_mutation_owner.is_none());
        drop(workspace);
        let state = trusted
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert!(!state.mutation_active);
    }

    async fn staged_service_with_recorder() -> (
        Repo,
        tempfile::TempDir,
        TrustedGitService,
        PreparedCommit,
        PathBuf,
        PathBuf,
    ) {
        let repo = Repo::new();
        fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged fixture");
        run_git(repo.path(), &["add", "staged.txt"]);
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("staged refresh");
        let (recorder, script, argv, input) = mutation_recorder();
        let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
            .expect("trusted recorder");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("staged checklist");
        let prepared = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await
            .prepared
            .expect("staged prepared");
        assert!(!argv.exists(), "empty selection spawned add");
        (repo, recorder, trusted, prepared, argv, input)
    }

    async fn prepared_with_proof_plan(
        unborn: bool,
        plan: &str,
    ) -> (
        Repo,
        tempfile::TempDir,
        tempfile::TempDir,
        TrustedGitService,
        PreparedCommit,
        PathBuf,
        PathBuf,
        Vec<u8>,
    ) {
        let repo = if unborn { Repo::unborn() } else { Repo::new() };
        if unborn {
            fs::write(repo.path().join("first.txt"), "first\n").expect("unborn fixture");
        } else {
            fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged fixture");
            run_git(repo.path(), &["add", "staged.txt"]);
        }
        let base = if unborn {
            Vec::new()
        } else {
            run_git_output(repo.path(), &["rev-parse", "HEAD"])
                .strip_suffix(b"\n")
                .expect("base newline")
                .to_vec()
        };
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("proof workspace refresh");
        let (read_dir, read, read_log) = proof_read_recorder(repo.path(), &base, plan);
        let (mutation_dir, mutation, mutation_argv, _mutation_input) = mutation_recorder();
        let trusted = TrustedGitService::new_with_executables_for_test(
            repo.path(),
            workspace,
            mutation,
            read,
        )
        .expect("trusted proof fixture");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("proof checklist");
        let selected = if unborn {
            vec![checklist.optional[0].file_id]
        } else {
            Vec::new()
        };
        let prepared = trusted
            .prepare(checklist.id, selected, CancellationToken::new())
            .await
            .prepared
            .expect("proof prepared");
        (
            repo,
            read_dir,
            mutation_dir,
            trusted,
            prepared,
            read_log,
            mutation_argv,
            base,
        )
    }

    #[tokio::test]
    async fn trusted_git_empty_selection_commits_existing_staged_delta() {
        let repo = Repo::new();
        fs::write(repo.path().join("staged.txt"), "staged\n").expect("write staged");
        run_git(repo.path(), &["add", "staged.txt"]);
        let (workspace, trusted) = repo.services().await;
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("refresh staged");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        assert_eq!(checklist.staged.len(), 1);
        let prepared = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await;
        assert!(prepared.error.is_none());
        let prepared = prepared.prepared.expect("prepared");
        let completion = trusted
            .commit(
                prepared.id,
                "test: staged only".into(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.outcome, CommitOutcome::Committed);
    }

    #[tokio::test]
    async fn e2e_owned_repo_checklist_prepare_mock_draft_commit() {
        let repo = Repo::new();
        let base = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
        let base = base.strip_suffix(b"\n").expect("base newline").to_vec();
        fs::write(repo.path().join("tracked.txt"), "changed\n").expect("modify");
        let (workspace, trusted) = repo.services().await;
        let snapshot = workspace
            .refresh(CancellationToken::new())
            .await
            .expect("refresh modified");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        assert_eq!(checklist.optional.len(), 1);
        assert_eq!(checklist.optional[0].file_id, snapshot.files[0].id);
        let prepared = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(prepared.error, None);
        let prepared = prepared.prepared.expect("prepared");
        let provider = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::text("test: production headless e2e"),
            vega_runtime::ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ]));
        let draft = trusted
            .draft(
                prepared.id,
                "mock-e2e".into(),
                provider.clone(),
                CancellationToken::new(),
            )
            .await
            .expect("mock draft");
        assert_eq!(provider.requests().len(), 1);
        assert!(provider.requests()[0].tools.is_empty());
        assert_eq!(provider.requests()[0].max_tokens, Some(256));
        let completion = trusted
            .commit(
                prepared.id,
                draft.text().to_owned(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.outcome, CommitOutcome::Committed);
        assert_terminal_workspace(
            &trusted,
            completion.workspace.as_ref().expect("terminal workspace"),
        );
        assert!(run_git_output(repo.path(), &["status", "--porcelain=v2", "-z"]).is_empty());
        let parents = run_git_output(repo.path(), &["rev-list", "--parents", "-n", "1", "HEAD"]);
        let parents = parents
            .strip_suffix(b"\n")
            .expect("parent newline")
            .split(|byte| *byte == b' ')
            .collect::<Vec<_>>();
        assert_eq!(parents.len(), 2);
        assert_eq!(parents[1], base);
        let tree = run_git_output(repo.path(), &["ls-tree", "-rz", "--full-tree", "HEAD"]);
        assert!(tree.ends_with(b"\ttracked.txt\0"));
    }

    #[tokio::test]
    async fn owner_refresh_prepare_first_capture_failure_retries_exact_owner() {
        let repo = Repo::new();
        fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
        let (_mutation_dir, mutation, mutation_argv, _input) = mutation_recorder();
        let (_read_dir, read, failed) = fail_first_status_after_trigger(&mutation_argv);
        let workspace = Arc::new(
            GitWorkspaceService::new_for_test(repo.path(), read).expect("fault workspace"),
        );
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("A refresh");
        let trusted =
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace.clone(), mutation)
                .expect("trusted");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        let completion = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert!(failed.exists(), "first owner status was faulted");
        assert!(completion.error.is_none());
        let terminal = completion.workspace.expect("authoritative B");
        assert!(terminal.generation > checklist.workspace_generation);
        assert!(completion.prepared.is_some());
    }

    #[tokio::test]
    async fn owner_refresh_commit_first_capture_failure_recovers_new_head_once() {
        let repo = Repo::new();
        fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
        let (_mutation_dir, mutation, mutation_argv, _input) = mutation_recorder();
        let (_read_dir, read, failed) = fail_first_status_after_trigger(&mutation_argv);
        let workspace = Arc::new(
            GitWorkspaceService::new_for_test(repo.path(), read).expect("fault workspace"),
        );
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("A refresh");
        let trusted =
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
                .expect("trusted");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        let prepared = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await
            .prepared
            .expect("prepared");
        fs::remove_file(&mutation_argv).expect("re-arm commit trigger");
        let before = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
        let completion = trusted
            .commit(
                prepared.id,
                "test: owner retry".into(),
                CancellationToken::new(),
            )
            .await;
        assert!(failed.exists(), "first post-commit status was faulted");
        assert_eq!(completion.outcome, CommitOutcome::Committed);
        assert!(completion.workspace.is_some());
        let after = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
        assert_ne!(before, after);
    }

    #[tokio::test]
    async fn disconnected_recovery_consumes_zombie_owner_before_future_checklist() {
        let repo = Repo::new();
        let (workspace, trusted) = repo.services().await;
        let parent = workspace
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .generation;
        let _owner = workspace
            .begin_owned_refresh(parent)
            .expect("mutation owner");
        trusted
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .mutation_active = true;
        fs::write(repo.path().join("tracked.txt"), "terminal state\n").expect("mutate");
        let recovered = trusted
            .recover_disconnected_mutation()
            .await
            .expect("authoritative recovery");
        assert!(recovered.generation > parent);
        assert!(workspace.active_owned_refresh().is_none());
        assert!(
            !trusted
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .mutation_active
        );
        trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("fresh checklist after recovery");
    }

    #[tokio::test]
    async fn trusted_git_selected_am_component_preserves_forced_add_topology() {
        let repo = Repo::new();
        fs::write(repo.path().join("added.txt"), "first\n").expect("new file");
        run_git(repo.path(), &["add", "added.txt"]);
        fs::write(repo.path().join("added.txt"), "second\n").expect("unstaged edit");
        let (workspace, trusted) = repo.services().await;
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("refresh AM");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("AM checklist");
        assert_eq!(checklist.staged.len(), 1);
        assert_eq!(checklist.optional.len(), 1);
        assert_eq!(checklist.staged[0].file_id, checklist.optional[0].file_id);
        let completion = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.error, None);
        assert!(completion.prepared.is_some());
        let status = run_git_output(repo.path(), &["status", "--porcelain"]);
        assert_eq!(status, b"A  added.txt\n");
    }

    #[tokio::test]
    async fn untracked_entry_is_optional_only_and_prepares_as_added() {
        let repo = Repo::new();
        fs::write(repo.path().join("untracked.txt"), "new\n").expect("new file");
        let (workspace, trusted) = repo.services().await;
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("refresh untracked");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("untracked checklist");
        assert!(checklist.staged.is_empty());
        assert_eq!(checklist.optional.len(), 1);
        assert_eq!(checklist.optional[0].kind, CommitSelectionKind::Added);
        assert!(!checklist.optional[0].forced);
        let completion = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.error, None);
        assert!(completion.prepared.is_some());
        assert_eq!(
            run_git_output(repo.path(), &["status", "--porcelain"]),
            b"A  untracked.txt\n"
        );
    }

    #[tokio::test]
    async fn selected_delete_and_untracked_destination_may_canonicalize_to_staged_rename() {
        let repo = Repo::new();
        fs::rename(
            repo.path().join("tracked.txt"),
            repo.path().join("renamed.txt"),
        )
        .expect("rename fixture");
        let (workspace, trusted) = repo.services().await;
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("refresh delete and untracked");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("rename checklist");
        assert!(checklist.staged.is_empty());
        assert_eq!(checklist.optional.len(), 2);
        assert!(
            checklist
                .optional
                .iter()
                .any(|row| row.kind == CommitSelectionKind::Deleted)
        );
        assert!(
            checklist
                .optional
                .iter()
                .any(|row| row.kind == CommitSelectionKind::Added)
        );
        let selected = checklist.optional.iter().map(|row| row.file_id).collect();
        let completion = trusted
            .prepare(checklist.id, selected, CancellationToken::new())
            .await;
        assert_eq!(completion.error, None);
        assert_eq!(
            completion
                .prepared
                .as_ref()
                .map(|prepared| prepared.staged_file_count),
            Some(1)
        );
        assert_eq!(
            run_git_output(repo.path(), &["status", "--porcelain"]),
            b"R  tracked.txt -> renamed.txt\n"
        );
    }

    #[test]
    fn delete_untracked_joint_rename_rejects_any_extra_touching_b_record() {
        let oid = |byte: u8| vec![byte; 40];
        let source_record = StatusRecord {
            shape: StatusShape::Ordinary,
            x: b'.',
            y: b'D',
            sub: b"N...".to_vec(),
            head_mode: b"100644".to_vec(),
            index_mode: b"100644".to_vec(),
            worktree_mode: b"000000".to_vec(),
            head_oid: oid(b'1'),
            index_oid: oid(b'1'),
            path: b"source.txt".to_vec(),
            previous: None,
        };
        let destination_record = StatusRecord {
            shape: StatusShape::Untracked,
            x: b'?',
            y: b'?',
            sub: b"N...".to_vec(),
            head_mode: b"000000".to_vec(),
            index_mode: b"000000".to_vec(),
            worktree_mode: b"100644".to_vec(),
            head_oid: oid(b'0'),
            index_oid: oid(b'0'),
            path: b"destination.txt".to_vec(),
            previous: None,
        };
        let row = |slot: u32,
                   record: StatusRecord,
                   kind: CommitSelectionKind,
                   mode: Option<Vec<u8>>| ChecklistRow {
            public: CommitSelection {
                file_id: WorkspaceFileId {
                    generation: 1,
                    slot,
                    seal: u64::from(slot),
                },
                label: String::new(),
                previous_label: None,
                kind,
                forced: false,
            },
            closure: vec![record.path.clone()],
            record,
            optional_kind: kind,
            worktree_mode: mode,
        };
        let rows = [
            row(1, source_record.clone(), CommitSelectionKind::Deleted, None),
            row(
                2,
                destination_record,
                CommitSelectionKind::Added,
                Some(b"100644".to_vec()),
            ),
        ];
        let selected = vec![&rows[0], &rows[1]];
        let merged = StatusRecord {
            shape: StatusShape::Rename,
            x: b'R',
            y: b'.',
            sub: b"N...".to_vec(),
            head_mode: b"100644".to_vec(),
            index_mode: b"100644".to_vec(),
            worktree_mode: b"100644".to_vec(),
            head_oid: oid(b'1'),
            index_oid: oid(b'1'),
            path: b"destination.txt".to_vec(),
            previous: Some(b"source.txt".to_vec()),
        };
        assert!(is_selected_delete_untracked_rename(
            &selected,
            std::slice::from_ref(&merged),
            b"source.txt",
            b"destination.txt"
        ));
        assert!(!is_selected_delete_untracked_rename(
            &selected,
            &[merged, source_record],
            b"source.txt",
            b"destination.txt"
        ));
    }

    #[tokio::test]
    async fn trusted_git_selected_staged_rename_with_unstaged_edit_proves_structural_split() {
        let repo = Repo::new();
        run_git(repo.path(), &["mv", "tracked.txt", "renamed.txt"]);
        fs::write(repo.path().join("renamed.txt"), "renamed and edited\n").expect("edit rename");
        let (workspace, trusted) = repo.services().await;
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("refresh RM");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("RM checklist");
        assert_eq!(checklist.staged.len(), 1);
        assert_eq!(checklist.optional.len(), 1);
        assert_eq!(checklist.staged[0].kind, CommitSelectionKind::Renamed);
        assert_eq!(checklist.optional[0].kind, CommitSelectionKind::Modified);
        let completion = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.error, None);
        let status = run_git_output(repo.path(), &["status", "--porcelain"]);
        assert_eq!(status, b"A  renamed.txt\nD  tracked.txt\n");
    }

    #[tokio::test]
    async fn staged_rename_destination_mode_flip_is_rejected_after_one_add() {
        let repo = Repo::new();
        run_git(repo.path(), &["mv", "tracked.txt", "renamed.txt"]);
        fs::write(repo.path().join("renamed.txt"), "renamed and edited\n").expect("edit rename");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("A refresh");
        let (_gate, mutation, ready, release) = blocking_before_mutation();
        let trusted = Arc::new(
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
                .expect("trusted"),
        );
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("rename checklist");
        let selected = checklist.optional[0].file_id;
        let worker = tokio::spawn({
            let trusted = trusted.clone();
            async move {
                trusted
                    .prepare(checklist.id, vec![selected], CancellationToken::new())
                    .await
            }
        });
        wait_for_path(&ready).await;
        let path = repo.path().join("renamed.txt");
        let mut mode = fs::metadata(&path).expect("rename metadata").permissions();
        mode.set_mode(0o755);
        fs::set_permissions(&path, mode).expect("flip executable mode");
        fs::write(&release, b"release").expect("release add");
        let completion = worker.await.expect("prepare worker");
        assert_eq!(completion.error, Some(CommitErrorCode::ChangedDuringRead));
        assert!(completion.prepared.is_none());
    }

    #[tokio::test]
    async fn staged_rename_source_recreation_is_not_owned_by_destination_edit() {
        let repo = Repo::new();
        run_git(repo.path(), &["mv", "tracked.txt", "renamed.txt"]);
        fs::write(repo.path().join("renamed.txt"), "renamed and edited\n").expect("edit rename");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("A refresh");
        let (_gate, mutation, ready, release) = blocking_before_mutation();
        let trusted = Arc::new(
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
                .expect("trusted"),
        );
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("rename checklist");
        let selected = checklist.optional[0].file_id;
        let worker = tokio::spawn({
            let trusted = trusted.clone();
            async move {
                trusted
                    .prepare(checklist.id, vec![selected], CancellationToken::new())
                    .await
            }
        });
        wait_for_path(&ready).await;
        fs::write(repo.path().join("tracked.txt"), "outside S\n").expect("recreate source");
        fs::write(&release, b"release").expect("release add");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            !worker.is_finished(),
            "unsafe source recreation must not publish a terminal owner snapshot"
        );
        fs::remove_file(repo.path().join("tracked.txt")).expect("restore safe source absence");
        let completion = worker.await.expect("prepare worker");
        assert!(completion.workspace.is_some());
    }

    #[tokio::test]
    async fn staged_rename_destination_delete_claims_only_canonical_old_deletion() {
        let repo = Repo::new();
        run_git(repo.path(), &["mv", "tracked.txt", "renamed.txt"]);
        fs::remove_file(repo.path().join("renamed.txt")).expect("delete rename destination");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("RD refresh");
        let (_recorder, mutation, argv, input) = mutation_recorder();
        let trusted =
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
                .expect("trusted");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("RD checklist");
        let selected = checklist
            .optional
            .iter()
            .find(|row| row.kind == CommitSelectionKind::Deleted)
            .expect("RD optional delete")
            .file_id;
        let completion = trusted
            .prepare(checklist.id, vec![selected], CancellationToken::new())
            .await;
        assert_eq!(completion.error, None);
        let prepared = completion.prepared.expect("prepared RD");
        assert_eq!(fs::read(&input).expect("RD add stdin"), b"renamed.txt\0");
        assert_eq!(
            fs::read(&argv).expect("RD add argv"),
            expected_mutation_argv(
                b"add",
                &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul",],
            )
        );
        {
            let state = trusted
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let authority = &state.prepared.as_ref().expect("stored RD").authority;
            assert!(authority.records.iter().any(|record| {
                record.shape == StatusShape::Ordinary
                    && record.path == b"tracked.txt"
                    && record.previous.is_none()
                    && record.x == b'D'
                    && record.y == b'.'
            }));
            assert!(
                !authority
                    .stages
                    .iter()
                    .any(|entry| { entry.path == b"tracked.txt" || entry.path == b"renamed.txt" })
            );
        }
        let committed = trusted
            .commit(
                prepared.id,
                "test: delete renamed file".into(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(committed.outcome, CommitOutcome::Committed);
        assert!(!repo.path().join("tracked.txt").exists());
        assert!(!repo.path().join("renamed.txt").exists());
    }

    #[tokio::test]
    async fn trusted_git_selected_regular_to_symlink_binds_type_change() {
        let repo = Repo::new();
        fs::remove_file(repo.path().join("tracked.txt")).expect("remove regular");
        std::os::unix::fs::symlink("missing-target", repo.path().join("tracked.txt"))
            .expect("symlink");
        let (workspace, trusted) = repo.services().await;
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("refresh type change");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("type checklist");
        assert_eq!(checklist.optional[0].kind, CommitSelectionKind::TypeChanged);
        let completion = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.error, None);
        let index = run_git_output(repo.path(), &["ls-files", "--stage", "--", "tracked.txt"]);
        assert!(index.starts_with(b"120000 "));
    }

    #[tokio::test]
    async fn trusted_git_selected_executable_add_binds_exact_worktree_mode() {
        let repo = Repo::new();
        let path = repo.path().join("run.sh");
        fs::write(&path, "#!/bin/sh\nexit 0\n").expect("script");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("chmod");
        let (workspace, trusted) = repo.services().await;
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("refresh executable");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("executable checklist");
        let completion = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.error, None);
        let index = run_git_output(repo.path(), &["ls-files", "--stage", "--", "run.sh"]);
        assert!(index.starts_with(b"100755 "));
    }

    #[tokio::test]
    async fn trusted_git_mutations_use_exact_argv_and_in_memory_stdin() {
        let repo = Repo::new();
        fs::write(repo.path().join("tracked.txt"), "changed\n").expect("modify");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let (_recorder, script, argv, input) = mutation_recorder();
        let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
            .expect("trusted fake");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        let prepared = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await
            .prepared
            .expect("prepared");
        assert_eq!(fs::read(&input).expect("add stdin"), b"tracked.txt\0");
        assert_eq!(
            fs::read(&argv).expect("add argv"),
            expected_mutation_argv(
                b"add",
                &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul",],
            )
        );
        let message = "feat: exact stdin";
        let completion = trusted
            .commit(prepared.id, message.into(), CancellationToken::new())
            .await;
        assert_eq!(completion.outcome, CommitOutcome::Committed);
        assert_eq!(fs::read(&input).expect("commit stdin"), message.as_bytes());
        assert_eq!(
            fs::read(&argv).expect("commit argv"),
            expected_mutation_argv(
                b"commit",
                &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"],
            )
        );
    }

    #[test]
    fn trusted_mutation_runner_enforces_spawn_cancel_exit_and_output_caps_for_add_and_commit() {
        let repo = Repo::new();
        let runner = test_runner(repo.path());
        for verb in ["add", "commit"] {
            let missing = repo.path().join(format!("missing-{verb}"));
            assert_eq!(
                mutation_error_code(run_fake_mutation(
                    &runner,
                    verb,
                    &missing,
                    Arc::from([]),
                    &CancellationToken::new(),
                    Duration::from_secs(1),
                )),
                GitWorkspaceErrorCode::SpawnFailed
            );

            let (_fixture, script, attempts) = scripted_mutation("exit 7");
            let cancelled = CancellationToken::new();
            cancelled.cancel();
            assert_eq!(
                mutation_error_code(run_fake_mutation(
                    &runner,
                    verb,
                    &script,
                    Arc::from([]),
                    &cancelled,
                    Duration::from_secs(1),
                )),
                GitWorkspaceErrorCode::Cancelled
            );
            assert!(!attempts.exists(), "pre-cancel spawned {verb}");
            assert_eq!(
                mutation_error_code(run_fake_mutation(
                    &runner,
                    verb,
                    &script,
                    Arc::from([]),
                    &CancellationToken::new(),
                    Duration::from_secs(1),
                )),
                GitWorkspaceErrorCode::GitFailed
            );
            assert_eq!(fs::read(&attempts).expect("one attempt"), b"x");

            for (stream, limit) in [("stdout", MUTATION_STDOUT_LIMIT), ("stderr", STDERR_LIMIT)] {
                let body = format!(
                    "/usr/bin/python3 -c 'import sys; sys.{}.buffer.write(b\"x\" * {})'",
                    stream, limit
                );
                let (_fixture, script, attempts) = scripted_mutation(&body);
                run_fake_mutation(
                    &runner,
                    verb,
                    &script,
                    Arc::from([]),
                    &CancellationToken::new(),
                    Duration::from_secs(3),
                )
                .expect("inclusive output cap");
                assert_eq!(fs::read(&attempts).expect("inclusive attempt"), b"x");

                let body = format!(
                    "/usr/bin/python3 -c 'import sys; sys.{}.buffer.write(b\"x\" * {})'",
                    stream,
                    limit + 1
                );
                let (_fixture, script, attempts) = scripted_mutation(&body);
                assert_eq!(
                    mutation_error_code(run_fake_mutation(
                        &runner,
                        verb,
                        &script,
                        Arc::from([]),
                        &CancellationToken::new(),
                        Duration::from_secs(3),
                    )),
                    GitWorkspaceErrorCode::OutputTooLarge
                );
                assert_eq!(fs::read(&attempts).expect("overflow attempt"), b"x");
            }
        }
    }

    #[test]
    fn trusted_mutation_runner_times_out_cancels_and_reaps_process_groups() {
        let repo = Repo::new();
        let runner = test_runner(repo.path());
        for verb in ["add", "commit"] {
            let timeout_dir = tempfile::tempdir().expect("timeout fixture");
            let pid_file = timeout_dir.path().join("pid");
            let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
            let body = format!(
                "trap '' TERM\n/bin/sleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait",
                quote(&pid_file)
            );
            let (_fixture, script, attempts) = scripted_mutation(&body);
            assert_eq!(
                mutation_error_code(run_fake_mutation(
                    &runner,
                    verb,
                    &script,
                    Arc::from(vec![b'i'; 2 * 1024 * 1024]),
                    &CancellationToken::new(),
                    Duration::from_millis(500),
                )),
                GitWorkspaceErrorCode::TimedOut
            );
            assert_eq!(fs::read(&attempts).expect("timeout attempt"), b"x");
            let pid = fs::read_to_string(&pid_file).expect("descendant pid");
            assert!(
                !Command::new(KILL)
                    .args(["-0", &pid])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .expect("kill probe")
                    .success(),
                "timeout descendant survived"
            );

            let cancel_dir = tempfile::tempdir().expect("cancel fixture");
            let ready = cancel_dir.path().join("ready");
            let body = format!(": > '{}'\n/bin/sleep 30", quote(&ready));
            let (_fixture, script, attempts) = scripted_mutation(&body);
            let cancel = CancellationToken::new();
            let trigger = cancel.clone();
            let ready_clone = ready.clone();
            let canceller = thread::spawn(move || {
                let started = Instant::now();
                while !ready_clone.exists() && started.elapsed() < Duration::from_secs(3) {
                    thread::sleep(Duration::from_millis(5));
                }
                trigger.cancel();
            });
            assert_eq!(
                mutation_error_code(run_fake_mutation(
                    &runner,
                    verb,
                    &script,
                    Arc::from(vec![b'i'; 4 * 1024 * 1024]),
                    &cancel,
                    Duration::from_secs(5),
                )),
                GitWorkspaceErrorCode::Cancelled
            );
            canceller.join().expect("canceller");
            assert_eq!(fs::read(&attempts).expect("cancel attempt"), b"x");
        }
    }

    #[test]
    fn trusted_mutation_runner_drains_floods_while_writing_large_stdin() {
        let repo = Repo::new();
        let runner = test_runner(repo.path());
        let body = "/usr/bin/python3 -c 'import sys; sys.stdout.buffer.write(b\"o\" * 524288); sys.stdout.flush(); sys.stderr.buffer.write(b\"e\" * 32768); sys.stderr.flush(); data=sys.stdin.buffer.read(); raise SystemExit(0 if len(data)==4194304 else 9)'";
        for verb in ["add", "commit"] {
            let (_fixture, script, attempts) = scripted_mutation(body);
            let output = run_fake_mutation(
                &runner,
                verb,
                &script,
                Arc::from(vec![b'i'; 4 * 1024 * 1024]),
                &CancellationToken::new(),
                Duration::from_secs(5),
            )
            .expect("concurrent stdin/stdout/stderr");
            assert_eq!(output.stdout.len(), 512 * 1024);
            assert_eq!(fs::read(&attempts).expect("flood attempt"), b"x");
        }
    }

    #[tokio::test]
    async fn service_reports_authoritative_state_after_add_and_commit_process_failures() {
        for (plan, expected) in [
            ("nonzero", CommitErrorCode::GitFailed),
            ("stdout-overflow", CommitErrorCode::OutputTooLarge),
            ("wait", CommitErrorCode::TimedOut),
            ("inherited-pipe", CommitErrorCode::ProcessControlFailed),
        ] {
            let repo = Repo::new();
            fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
            let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
            workspace
                .refresh(CancellationToken::new())
                .await
                .expect("add A");
            let (_fixture, script, attempts, _argv, _input) = after_git_mutation(plan);
            let trusted = TrustedGitService::new_with_mutation_timeout_for_test(
                repo.path(),
                workspace,
                script,
                if plan == "wait" {
                    Duration::from_millis(500)
                } else {
                    Duration::from_secs(3)
                },
            )
            .expect("trusted add fault");
            let checklist = trusted
                .open_checklist(CancellationToken::new())
                .await
                .expect("add checklist");
            let completion = trusted
                .prepare(
                    checklist.id,
                    vec![checklist.optional[0].file_id],
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(completion.error, Some(expected), "add plan {plan}");
            assert!(completion.prepared.is_none(), "add plan {plan}");
            assert!(completion.workspace.is_some(), "add plan {plan}");
            assert_eq!(
                fs::read(attempts)
                    .unwrap_or_else(|error| panic!("add plan {plan} marker: {error}")),
                b"x"
            );
            assert!(
                !run_git_output(repo.path(), &["diff", "--cached", "--name-only"]).is_empty(),
                "add plan {plan} lost real index mutation"
            );

            let repo = Repo::new();
            fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged fixture");
            run_git(repo.path(), &["add", "staged.txt"]);
            let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
            workspace
                .refresh(CancellationToken::new())
                .await
                .expect("commit B refresh");
            // Recreate an exact prepared capability under the faulting service.
            let (_fixture, script, attempts, _argv, _input) = after_git_mutation(plan);
            let trusted = TrustedGitService::new_with_mutation_timeout_for_test(
                repo.path(),
                workspace,
                script,
                if plan == "wait" {
                    Duration::from_millis(500)
                } else {
                    Duration::from_secs(3)
                },
            )
            .expect("trusted commit fault");
            let checklist = trusted
                .open_checklist(CancellationToken::new())
                .await
                .expect("commit checklist");
            let prepared = trusted
                .prepare(checklist.id, Vec::new(), CancellationToken::new())
                .await
                .prepared
                .expect("commit prepared");
            let before = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
            let completion = trusted
                .commit(
                    prepared.id,
                    "test: process fault".into(),
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(
                completion.outcome,
                CommitOutcome::Failed(expected),
                "{plan}"
            );
            assert!(completion.workspace.is_some(), "commit plan {plan}");
            assert_eq!(fs::read(attempts).expect("one commit"), b"x");
            let after = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
            assert_ne!(before, after, "commit plan {plan} lost real commit");
            let duplicate = trusted
                .commit(
                    prepared.id,
                    "test: no retry".into(),
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(
                duplicate.outcome,
                CommitOutcome::Failed(CommitErrorCode::StaleAuthority)
            );
        }

        for phase in ["add", "commit"] {
            let repo = Repo::new();
            let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
            if phase == "add" {
                fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
            } else {
                fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
                run_git(repo.path(), &["add", "staged.txt"]);
            }
            workspace
                .refresh(CancellationToken::new())
                .await
                .expect("exact output A");
            let (_fixture, script, attempts, _argv, _input) = after_git_mutation("stdout-exact");
            let trusted = TrustedGitService::new_with_mutation_timeout_for_test(
                repo.path(),
                workspace,
                script,
                Duration::from_secs(3),
            )
            .expect("trusted exact output");
            let checklist = trusted
                .open_checklist(CancellationToken::new())
                .await
                .expect("exact output checklist");
            let selected = if phase == "add" {
                vec![checklist.optional[0].file_id]
            } else {
                Vec::new()
            };
            let prepared = trusted
                .prepare(checklist.id, selected, CancellationToken::new())
                .await
                .prepared
                .expect("inclusive stdout prepared");
            if phase == "commit" {
                let completion = trusted
                    .commit(
                        prepared.id,
                        "test: inclusive output".into(),
                        CancellationToken::new(),
                    )
                    .await;
                assert_eq!(completion.outcome, CommitOutcome::Committed);
            }
            assert_eq!(fs::read(attempts).expect("inclusive attempt"), b"x");
        }
    }

    #[tokio::test]
    async fn service_entry_pre_mutation_failures_are_authoritative_and_single_use() {
        for phase in ["add", "commit"] {
            for (case, expected, expected_attempts) in [
                ("missing", CommitErrorCode::SpawnFailed, 0_usize),
                ("pre-cancel", CommitErrorCode::Cancelled, 0),
                ("nonzero-before", CommitErrorCode::GitFailed, 1),
            ] {
                let repo = Repo::new();
                if phase == "add" {
                    fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
                } else {
                    fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
                    run_git(repo.path(), &["add", "staged.txt"]);
                }
                let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
                workspace
                    .refresh(CancellationToken::new())
                    .await
                    .expect("entry A");
                let (fixture, executable, attempts, argv) = if case == "missing" {
                    let fixture = tempfile::tempdir().expect("missing fixture");
                    let executable = fixture.path().join("missing-executable");
                    let attempts = fixture.path().join("attempts");
                    let argv = fixture.path().join("argv");
                    (fixture, executable, attempts, argv)
                } else {
                    let (fixture, executable, attempts, argv, _input) =
                        before_git_mutation("exit 17");
                    (fixture, executable, attempts, argv)
                };
                let trusted = TrustedGitService::new_with_mutation_for_test(
                    repo.path(),
                    workspace,
                    executable,
                )
                .expect("trusted pre-mutation fault");
                let checklist = trusted
                    .open_checklist(CancellationToken::new())
                    .await
                    .expect("entry checklist");
                let cancel = CancellationToken::new();
                if case == "pre-cancel" {
                    cancel.cancel();
                }
                if phase == "add" {
                    let completion = trusted
                        .prepare(checklist.id, vec![checklist.optional[0].file_id], cancel)
                        .await;
                    assert_eq!(completion.error, Some(expected), "add {case}");
                    assert!(completion.prepared.is_none(), "add {case}");
                    let terminal = completion.workspace.as_ref().expect("add terminal");
                    assert_terminal_workspace(&trusted, terminal);
                    assert!(
                        run_git_output(repo.path(), &["diff", "--cached", "--name-only"])
                            .is_empty(),
                        "add {case} changed index"
                    );
                    let duplicate = trusted
                        .prepare(checklist.id, Vec::new(), CancellationToken::new())
                        .await;
                    assert_eq!(duplicate.error, Some(CommitErrorCode::StaleAuthority));
                } else {
                    let prepared = trusted
                        .prepare(checklist.id, Vec::new(), CancellationToken::new())
                        .await
                        .prepared
                        .expect("entry prepared");
                    let invalid = trusted
                        .commit(prepared.id, String::new(), cancel.clone())
                        .await;
                    assert_eq!(
                        invalid.outcome,
                        CommitOutcome::Failed(CommitErrorCode::InvalidMessage)
                    );
                    assert!(invalid.workspace.is_none());
                    assert!(!attempts.exists(), "invalid message spawned {case}");
                    let before = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
                    let completion = trusted
                        .commit(prepared.id, "test: entry failure".into(), cancel)
                        .await;
                    assert_eq!(
                        completion.outcome,
                        CommitOutcome::Failed(expected),
                        "{case}"
                    );
                    let terminal = completion.workspace.as_ref().expect("commit terminal");
                    assert_terminal_workspace(&trusted, terminal);
                    assert_eq!(before, run_git_output(repo.path(), &["rev-parse", "HEAD"]));
                    let duplicate = trusted
                        .commit(
                            prepared.id,
                            "test: no retry".into(),
                            CancellationToken::new(),
                        )
                        .await;
                    assert_eq!(
                        duplicate.outcome,
                        CommitOutcome::Failed(CommitErrorCode::StaleAuthority)
                    );
                }
                let actual_attempts = fs::read(&attempts).map_or(0, |bytes| bytes.len());
                assert_eq!(actual_attempts, expected_attempts, "{phase} {case}");
                if expected_attempts == 1 {
                    let expected_argv = if phase == "add" {
                        expected_mutation_argv(
                            b"add",
                            &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"],
                        )
                    } else {
                        expected_mutation_argv(
                            b"commit",
                            &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"],
                        )
                    };
                    assert_eq!(fs::read(&argv).expect("exact safe argv"), expected_argv);
                } else {
                    assert!(!argv.exists(), "zero-spawn {phase} {case} wrote argv");
                }
                drop(fixture);
            }
        }
    }

    #[tokio::test]
    async fn service_entry_stderr_caps_are_exact_for_add_and_commit() {
        for phase in ["add", "commit"] {
            for (plan, expected) in [
                ("stderr-exact", None),
                ("stderr-overflow", Some(CommitErrorCode::OutputTooLarge)),
            ] {
                let repo = Repo::new();
                if phase == "add" {
                    fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
                } else {
                    fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
                    run_git(repo.path(), &["add", "staged.txt"]);
                }
                let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
                workspace
                    .refresh(CancellationToken::new())
                    .await
                    .expect("stderr A");
                let (_fixture, script, attempts, argv, _input) = after_git_mutation(plan);
                let trusted =
                    TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
                        .expect("trusted stderr cap");
                let checklist = trusted
                    .open_checklist(CancellationToken::new())
                    .await
                    .expect("stderr checklist");
                if phase == "add" {
                    let completion = trusted
                        .prepare(
                            checklist.id,
                            vec![checklist.optional[0].file_id],
                            CancellationToken::new(),
                        )
                        .await;
                    assert_eq!(completion.error, expected, "add {plan}");
                    assert_eq!(completion.prepared.is_some(), expected.is_none());
                    let terminal = completion.workspace.as_ref().expect("add stderr terminal");
                    assert_terminal_workspace(&trusted, terminal);
                    assert!(
                        !run_git_output(repo.path(), &["diff", "--cached", "--name-only"])
                            .is_empty()
                    );
                } else {
                    let prepared = trusted
                        .prepare(checklist.id, Vec::new(), CancellationToken::new())
                        .await
                        .prepared
                        .expect("stderr prepared");
                    let before = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
                    let completion = trusted
                        .commit(
                            prepared.id,
                            "test: stderr cap".into(),
                            CancellationToken::new(),
                        )
                        .await;
                    let expected_outcome =
                        expected.map_or(CommitOutcome::Committed, CommitOutcome::Failed);
                    assert_eq!(completion.outcome, expected_outcome, "commit {plan}");
                    let terminal = completion
                        .workspace
                        .as_ref()
                        .expect("commit stderr terminal");
                    assert_terminal_workspace(&trusted, terminal);
                    assert_ne!(before, run_git_output(repo.path(), &["rev-parse", "HEAD"]));
                    let duplicate = trusted
                        .commit(
                            prepared.id,
                            "test: no retry".into(),
                            CancellationToken::new(),
                        )
                        .await;
                    assert_eq!(
                        duplicate.outcome,
                        CommitOutcome::Failed(CommitErrorCode::StaleAuthority)
                    );
                }
                assert_eq!(fs::read(attempts).expect("one stderr attempt"), b"x");
                let expected_argv = if phase == "add" {
                    expected_mutation_argv(
                        b"add",
                        &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"],
                    )
                } else {
                    expected_mutation_argv(
                        b"commit",
                        &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"],
                    )
                };
                assert_eq!(fs::read(argv).expect("stderr safe argv"), expected_argv);
            }
        }
    }

    #[tokio::test]
    async fn service_cancel_after_real_add_or_commit_returns_authoritative_state_once() {
        for phase in ["add", "commit"] {
            let repo = Repo::new();
            if phase == "add" {
                fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
            } else {
                fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
                run_git(repo.path(), &["add", "staged.txt"]);
            }
            let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
            workspace
                .refresh(CancellationToken::new())
                .await
                .expect("cancel A");
            let (_fixture, script, ready, _release) = blocking_mutation();
            let trusted = Arc::new(
                TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
                    .expect("trusted cancel"),
            );
            let checklist = trusted
                .open_checklist(CancellationToken::new())
                .await
                .expect("cancel checklist");
            let selected = if phase == "add" {
                vec![checklist.optional[0].file_id]
            } else {
                Vec::new()
            };
            let cancel = CancellationToken::new();
            if phase == "add" {
                let worker = tokio::spawn({
                    let trusted = trusted.clone();
                    let cancel = cancel.clone();
                    async move { trusted.prepare(checklist.id, selected, cancel).await }
                });
                wait_for_path(&ready).await;
                cancel.cancel();
                let completion = worker.await.expect("cancel add worker");
                assert_eq!(completion.error, Some(CommitErrorCode::Cancelled));
                assert!(completion.workspace.is_some());
                assert!(completion.prepared.is_none());
            } else {
                let prepared = trusted
                    .prepare(checklist.id, selected, CancellationToken::new())
                    .await
                    .prepared
                    .expect("cancel commit prepared");
                let before = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
                let worker = tokio::spawn({
                    let trusted = trusted.clone();
                    let cancel = cancel.clone();
                    async move {
                        trusted
                            .commit(prepared.id, "test: cancel".into(), cancel)
                            .await
                    }
                });
                wait_for_path(&ready).await;
                cancel.cancel();
                let completion = worker.await.expect("cancel commit worker");
                assert_eq!(
                    completion.outcome,
                    CommitOutcome::Failed(CommitErrorCode::Cancelled)
                );
                assert!(completion.workspace.is_some());
                assert_ne!(before, run_git_output(repo.path(), &["rev-parse", "HEAD"]));
            }
        }
    }

    #[tokio::test]
    async fn trusted_git_empty_selection_spawns_zero_add() {
        let repo = Repo::new();
        fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
        run_git(repo.path(), &["add", "staged.txt"]);
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let (_recorder, script, argv, _input) = mutation_recorder();
        let _ = fs::remove_file(&argv);
        let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
            .expect("trusted fake");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        let prepared = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await;
        assert!(prepared.prepared.is_some());
        assert!(!argv.exists(), "empty S must not spawn add");
    }

    #[tokio::test]
    async fn empty_selection_never_spawns_add_for_each_real_staged_delta() {
        for kind in ["add", "modify", "mode", "delete", "rename"] {
            let repo = Repo::new();
            match kind {
                "add" => {
                    fs::write(repo.path().join("added.txt"), "added\n").expect("add fixture");
                    run_git(repo.path(), &["add", "added.txt"]);
                }
                "modify" => {
                    fs::write(repo.path().join("tracked.txt"), "modified\n")
                        .expect("modify fixture");
                    run_git(repo.path(), &["add", "tracked.txt"]);
                }
                "mode" => {
                    let path = repo.path().join("tracked.txt");
                    let mut permissions = fs::metadata(&path).expect("mode metadata").permissions();
                    permissions.set_mode(0o755);
                    fs::set_permissions(path, permissions).expect("mode fixture");
                    run_git(repo.path(), &["add", "tracked.txt"]);
                }
                "delete" => run_git(repo.path(), &["rm", "-q", "tracked.txt"]),
                "rename" => run_git(repo.path(), &["mv", "tracked.txt", "renamed.txt"]),
                _ => unreachable!(),
            }
            let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
            workspace
                .refresh(CancellationToken::new())
                .await
                .expect("staged refresh");
            let (_recorder, script, argv, _input) = mutation_recorder();
            let trusted =
                TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
                    .expect("trusted recorder");
            let checklist = trusted
                .open_checklist(CancellationToken::new())
                .await
                .unwrap_or_else(|error| panic!("{kind} staged checklist: {error:?}"));
            let completion = trusted
                .prepare(checklist.id, Vec::new(), CancellationToken::new())
                .await;
            assert!(completion.prepared.is_some(), "{kind} staged delta");
            assert_eq!(completion.error, None, "{kind} staged delta");
            assert!(!argv.exists(), "{kind} empty selection spawned add");
        }
    }

    #[tokio::test]
    async fn clean_and_normalized_noop_are_no_staged_changes_without_commit() {
        let repo = Repo::new();
        let (workspace, trusted) = repo.services().await;
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("clean checklist");
        let clean = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await;
        assert_eq!(clean.error, Some(CommitErrorCode::NoStagedChanges));
        assert!(clean.prepared.is_none());

        run_git(repo.path(), &["config", "core.filemode", "false"]);
        let path = repo.path().join("tracked.txt");
        let mut permissions = fs::metadata(&path).expect("mode metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("ignored mode change");
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("ignored mode refresh");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("ignored mode checklist");
        assert!(checklist.staged.is_empty() && checklist.optional.is_empty());
        let ignored = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await;
        assert_eq!(ignored.error, Some(CommitErrorCode::NoStagedChanges));
        assert!(ignored.prepared.is_none());

        let repo = Repo::new();
        fs::write(repo.path().join(".gitattributes"), "* text eol=lf\n").expect("eol attributes");
        run_git(repo.path(), &["add", ".gitattributes"]);
        run_git(repo.path(), &["commit", "-qm", "eol policy"]);
        fs::write(repo.path().join("tracked.txt"), b"base\r\n").expect("crlf worktree");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("eol refresh");
        let (_recorder, script, argv, _input) = mutation_recorder();
        let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
            .expect("trusted eol");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("eol checklist");
        assert_eq!(checklist.optional.len(), 1);
        let normalized = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(normalized.error, Some(CommitErrorCode::NoStagedChanges));
        assert!(normalized.prepared.is_none());
        assert_eq!(
            fs::read(argv).expect("one normalization add"),
            expected_mutation_argv(
                b"add",
                &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
            )
        );

        let repo = Repo::new();
        fs::write(repo.path().join(".gitattributes"), "* text eol=lf\n")
            .expect("drift eol attributes");
        fs::write(repo.path().join("other.txt"), "other\n").expect("other fixture");
        run_git(repo.path(), &["add", ".gitattributes", "other.txt"]);
        run_git(repo.path(), &["commit", "-qm", "eol drift policy"]);
        fs::write(repo.path().join("tracked.txt"), b"base\r\n").expect("selected crlf");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("drift A");
        let (fixture, script, ready, release) = blocking_before_mutation();
        let trusted = Arc::new(
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
                .expect("trusted drift"),
        );
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("drift checklist");
        let selected = checklist.optional[0].file_id;
        let worker = tokio::spawn({
            let trusted = trusted.clone();
            async move {
                trusted
                    .prepare(checklist.id, vec![selected], CancellationToken::new())
                    .await
            }
        });
        wait_for_path(&ready).await;
        fs::write(repo.path().join("other.txt"), b"other\r\n").expect("outside-S drift");
        fs::write(release, b"release").expect("release normalized add");
        let completion = worker.await.expect("normalized drift worker");
        assert_eq!(completion.error, Some(CommitErrorCode::ChangedDuringRead));
        assert!(completion.prepared.is_none());
        let terminal = completion.workspace.as_ref().expect("terminal workspace");
        assert_terminal_workspace(&trusted, terminal);
        assert_eq!(
            fs::read(fixture.path().join("mutation-attempts")).expect("one add attempt"),
            b"x"
        );
        assert_eq!(
            fs::read(fixture.path().join("mutation-argv.bin")).expect("only add argv"),
            expected_mutation_argv(
                b"add",
                &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
            )
        );
        let state = trusted
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert!(!state.mutation_active);
        assert!(state.prepared.is_none());
    }

    #[tokio::test]
    async fn selected_awkward_raw_paths_use_one_sorted_nul_stdin_and_no_path_argv() {
        let repo = Repo::new();
        let mut paths = vec![
            b"space name.txt".to_vec(),
            b"tab\tname.txt".to_vec(),
            b"line\nname.txt".to_vec(),
            b"-leading.txt".to_vec(),
        ];
        for raw in &paths {
            fs::write(
                repo.path().join(OsString::from_vec(raw.clone())),
                b"awkward\n",
            )
            .expect("awkward fixture");
        }
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("awkward refresh");
        let (_recorder, script, argv, input) = mutation_recorder();
        let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, script)
            .expect("trusted recorder");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("awkward checklist");
        assert_eq!(checklist.optional.len(), paths.len());
        let selected = checklist.optional.iter().map(|row| row.file_id).collect();
        let completion = trusted
            .prepare(checklist.id, selected, CancellationToken::new())
            .await;
        assert_eq!(completion.error, None);
        paths.sort();
        let mut expected_input = Vec::new();
        for path in paths {
            expected_input.extend_from_slice(&path);
            expected_input.push(0);
        }
        assert_eq!(fs::read(input).expect("awkward add stdin"), expected_input);
        assert_eq!(
            fs::read(argv).expect("awkward add argv"),
            expected_mutation_argv(
                b"add",
                &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
            )
        );

        // Darwin/APFS may reject non-UTF-8 leaf creation with EILSEQ. Exercise
        // the same production mutation pipe directly so raw bytes still have
        // byte-exact evidence without claiming an unavailable filesystem E2E.
        let raw = b"nonutf8-\xff.txt\0".to_vec();
        let (_recorder, script, argv, input) = mutation_recorder();
        let runner = test_runner(repo.path());
        let result = runner.run_trusted_mutation_with_executable_and_timeout(
            "add",
            &[
                OsString::from("-A"),
                OsString::from("--pathspec-from-file=-"),
                OsString::from("--pathspec-file-nul"),
            ],
            Arc::from(raw.clone()),
            &CancellationToken::new(),
            &script,
            Duration::from_secs(3),
        );
        assert!(result.is_err(), "missing raw fixture unexpectedly staged");
        assert_eq!(fs::read(input).expect("raw byte stdin"), raw);
        let recorded = fs::read(argv).expect("raw byte argv");
        assert_eq!(
            recorded,
            expected_mutation_argv(
                b"add",
                &[b"-A", b"--pathspec-from-file=-", b"--pathspec-file-nul"]
            )
        );
        assert!(!recorded.windows(2).any(|window| window == [0xff, 0]));
    }

    #[tokio::test]
    async fn commit_proof_uses_explicit_new_oid_for_born_and_unborn_commits() {
        for unborn in [false, true] {
            let (repo, _read_dir, _mutation_dir, trusted, prepared, read_log, mutation_argv, base) =
                prepared_with_proof_plan(unborn, "pass").await;
            let completion = trusted
                .commit(
                    prepared.id,
                    "test: explicit immutable proof".into(),
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(completion.outcome, CommitOutcome::Committed);
            assert!(completion.workspace.is_some());
            let new_oid = run_git_output(repo.path(), &["rev-parse", "HEAD"])
                .strip_suffix(b"\n")
                .expect("new oid newline")
                .to_vec();
            assert_ne!(new_oid, base);
            let invocations = read_invocations(&read_log);
            if unborn {
                assert!(
                    !invocations.iter().any(|invocation| {
                        invocation.first().is_some_and(|phase| phase == b"pre")
                            && invocation.iter().any(|arg| arg == b"ls-tree")
                    }),
                    "unborn A performed a HEAD tree read"
                );
            }
            let mut parent_arg = new_oid.clone();
            parent_arg.extend_from_slice(b"^@");
            assert!(invocations.iter().any(|invocation| {
                invocation.first().is_some_and(|phase| phase == b"post")
                    && invocation.iter().any(|arg| arg == b"rev-parse")
                    && invocation.iter().any(|arg| arg == &parent_arg)
            }));
            assert!(invocations.iter().any(|invocation| {
                invocation.first().is_some_and(|phase| phase == b"post")
                    && invocation.iter().any(|arg| arg == b"ls-tree")
                    && invocation.iter().any(|arg| arg == &new_oid)
                    && !invocation.iter().any(|arg| arg == b"HEAD")
            }));
            assert_eq!(
                fs::read(mutation_argv).expect("commit argv"),
                expected_mutation_argv(
                    b"commit",
                    &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
                )
            );
            let parents =
                run_git_output(repo.path(), &["rev-list", "--parents", "-n", "1", "HEAD"]);
            let fields: Vec<_> = parents
                .split(|byte| byte.is_ascii_whitespace())
                .filter(|field| !field.is_empty())
                .collect();
            assert_eq!(fields.len(), if unborn { 1 } else { 2 });
            if !unborn {
                assert_eq!(fields[1], base);
            }
        }
    }

    #[tokio::test]
    async fn commit_proof_rejects_parent_tree_and_final_ref_faults_after_one_commit() {
        for (plan, expected) in [
            ("zero-parent", CommitErrorCode::ChangedDuringRead),
            ("wrong-parent", CommitErrorCode::ChangedDuringRead),
            ("two-parent", CommitErrorCode::ChangedDuringRead),
            ("tree-diff", CommitErrorCode::ChangedDuringRead),
            ("malformed-parent", CommitErrorCode::MalformedOutput),
            ("short-parent", CommitErrorCode::MalformedOutput),
            ("mixed-parent", CommitErrorCode::MalformedOutput),
            ("object-missing", CommitErrorCode::GitFailed),
            ("ref-moved", CommitErrorCode::ChangedDuringRead),
            ("ref-deleted", CommitErrorCode::ChangedDuringRead),
            ("ref-renamed", CommitErrorCode::ChangedDuringRead),
        ] {
            let (
                _repo,
                _read_dir,
                mutation_dir,
                trusted,
                prepared,
                _read_log,
                mutation_argv,
                _base,
            ) = prepared_with_proof_plan(false, plan).await;
            let completion = trusted
                .commit(
                    prepared.id,
                    "test: proof must fail closed".into(),
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(
                completion.outcome,
                CommitOutcome::Failed(expected),
                "proof plan {plan}"
            );
            assert!(completion.workspace.is_some(), "{plan} terminal refresh");
            assert_eq!(
                fs::read(&mutation_argv).expect("one commit argv"),
                expected_mutation_argv(
                    b"commit",
                    &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
                ),
                "proof plan {plan}"
            );
            assert_eq!(
                fs::read(mutation_dir.path().join("mutation-attempts"))
                    .expect("one commit process attempt"),
                b"x",
                "proof plan {plan}"
            );
            let duplicate = trusted
                .commit(
                    prepared.id,
                    "test: duplicate proof".into(),
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(
                duplicate.outcome,
                CommitOutcome::Failed(CommitErrorCode::StaleAuthority),
                "proof plan {plan}"
            );
            assert_eq!(
                fs::read(mutation_argv).expect("still one commit"),
                expected_mutation_argv(
                    b"commit",
                    &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
                )
            );
        }
    }

    #[tokio::test]
    async fn commit_proof_rejects_root_identity_swap_after_exactly_one_commit() {
        let (repo, read_dir, mutation_dir, trusted, prepared, _read_log, mutation_argv, _base) =
            prepared_with_proof_plan(false, "root-swap").await;
        let completion = trusted
            .commit(
                prepared.id,
                "test: root swap".into(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(
            completion.outcome,
            CommitOutcome::Failed(CommitErrorCode::ChangedDuringRead)
        );
        assert!(completion.workspace.is_none());
        assert_eq!(
            fs::read(mutation_argv).expect("one root-swap commit"),
            expected_mutation_argv(
                b"commit",
                &[b"--no-gpg-sign", b"--file=-", b"--cleanup=verbatim"]
            )
        );
        assert_eq!(
            fs::read(mutation_dir.path().join("mutation-attempts"))
                .expect("one root-swap process attempt"),
            b"x"
        );

        let root = repo.path().to_path_buf();
        let backup = read_dir.path().join("root-backup");
        fs::remove_dir(&root).expect("remove exact empty replacement root");
        fs::rename(backup, root).expect("restore exact fixture root");
    }

    #[tokio::test]
    async fn commit_third_capture_mismatch_consumes_prepared_and_spawns_zero_commit() {
        for change in ["status", "index", "ref", "operation"] {
            let (repo, _recorder, trusted, prepared, argv, _input) =
                staged_service_with_recorder().await;
            match change {
                "status" => {
                    fs::write(repo.path().join("tracked.txt"), "changed after B\n")
                        .expect("status drift");
                }
                "index" => {
                    fs::write(repo.path().join("other.txt"), "index drift\n").expect("index drift");
                    run_git(repo.path(), &["add", "other.txt"]);
                }
                "ref" => run_git(
                    repo.path(),
                    &["commit", "--allow-empty", "-qm", "ref drift"],
                ),
                "operation" => {
                    let oid = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
                    fs::write(repo.path().join(".git/MERGE_HEAD"), oid).expect("operation marker");
                }
                _ => unreachable!(),
            }
            let completion = trusted
                .commit(
                    prepared.id,
                    "test: must not execute".into(),
                    CancellationToken::new(),
                )
                .await;
            assert!(
                matches!(completion.outcome, CommitOutcome::Failed(_)),
                "{change} drift"
            );
            assert!(completion.workspace.is_some(), "{change} terminal refresh");
            assert!(!argv.exists(), "{change} drift spawned commit");
            let stale = trusted
                .commit(
                    prepared.id,
                    "test: duplicate".into(),
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(
                stale.outcome,
                CommitOutcome::Failed(CommitErrorCode::StaleAuthority)
            );
            assert!(!argv.exists(), "duplicate spawned commit");
        }
    }

    #[tokio::test]
    async fn commit_message_byte_bounds_and_exact_stdin_are_enforced() {
        let (_repo, _recorder, trusted, prepared, argv, input) =
            staged_service_with_recorder().await;
        for invalid in [
            String::new(),
            "nul\0body".into(),
            "x".repeat(MESSAGE_LIMIT + 1),
        ] {
            let completion = trusted
                .commit(prepared.id, invalid, CancellationToken::new())
                .await;
            assert_eq!(
                completion.outcome,
                CommitOutcome::Failed(CommitErrorCode::InvalidMessage)
            );
            assert!(!argv.exists(), "invalid message spawned commit");
        }
        let exact = "x".repeat(MESSAGE_LIMIT);
        let completion = trusted
            .commit(prepared.id, exact.clone(), CancellationToken::new())
            .await;
        assert_eq!(completion.outcome, CommitOutcome::Committed);
        assert_eq!(
            fs::read(input).expect("exact message stdin"),
            exact.as_bytes()
        );

        let (_repo, _recorder, trusted, prepared, argv, input) =
            staged_service_with_recorder().await;
        let multibyte_exact = "é".repeat(MESSAGE_LIMIT / 2);
        let multibyte_plus_one = format!("{multibyte_exact}x");
        assert_eq!(multibyte_exact.len(), MESSAGE_LIMIT);
        assert_eq!(multibyte_plus_one.len(), MESSAGE_LIMIT + 1);
        let rejected = trusted
            .commit(prepared.id, multibyte_plus_one, CancellationToken::new())
            .await;
        assert_eq!(
            rejected.outcome,
            CommitOutcome::Failed(CommitErrorCode::InvalidMessage)
        );
        assert!(!argv.exists(), "multibyte +1 spawned commit");
        let committed = trusted
            .commit(
                prepared.id,
                multibyte_exact.clone(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(committed.outcome, CommitOutcome::Committed);
        assert_eq!(
            fs::read(input).expect("multibyte stdin"),
            multibyte_exact.as_bytes()
        );

        let (_repo, _recorder, trusted, prepared, _argv, input) =
            staged_service_with_recorder().await;
        let newline_message = "subject\n\nbody\n";
        let committed = trusted
            .commit(
                prepared.id,
                newline_message.into(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(committed.outcome, CommitOutcome::Committed);
        assert_eq!(
            fs::read(input).expect("newline stdin"),
            newline_message.as_bytes()
        );
    }

    #[tokio::test]
    async fn owned_prepare_accepts_exact_b_published_by_ordinary_poll() {
        let repo = Repo::new();
        fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("A refresh");
        let (_barrier, script, ready, release) = blocking_mutation();
        let trusted = Arc::new(
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace.clone(), script)
                .expect("trusted"),
        );
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        let selected = vec![checklist.optional[0].file_id];
        let worker = {
            let trusted = trusted.clone();
            tokio::spawn(async move {
                trusted
                    .prepare(checklist.id, selected, CancellationToken::new())
                    .await
            })
        };
        wait_for_path(&ready).await;
        let observed_b = workspace
            .refresh(CancellationToken::new())
            .await
            .expect("ordinary B poll");
        fs::write(&release, b"release").expect("release");
        let completion = worker.await.expect("prepare task");
        assert!(completion.prepared.is_some());
        assert_eq!(
            completion
                .workspace
                .as_ref()
                .map(|snapshot| snapshot.generation),
            Some(observed_b.generation)
        );
        let after = workspace
            .refresh(CancellationToken::new())
            .await
            .expect("post completion poll");
        assert_eq!(after.generation, observed_b.generation);
    }

    #[tokio::test]
    async fn owned_prepare_rejects_a_to_b_to_a_without_capability() {
        let repo = Repo::new();
        fs::write(repo.path().join("tracked.txt"), "selected\n").expect("modify");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        let a = workspace
            .refresh(CancellationToken::new())
            .await
            .expect("A refresh");
        let (_barrier, script, ready, release) = blocking_mutation();
        let trusted = Arc::new(
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace.clone(), script)
                .expect("trusted"),
        );
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        let selected = vec![checklist.optional[0].file_id];
        let worker = {
            let trusted = trusted.clone();
            tokio::spawn(async move {
                trusted
                    .prepare(checklist.id, selected, CancellationToken::new())
                    .await
            })
        };
        wait_for_path(&ready).await;
        let b = workspace
            .refresh(CancellationToken::new())
            .await
            .expect("B poll");
        assert_ne!(b.generation, a.generation);
        run_git(repo.path(), &["reset", "-q", "HEAD", "--", "tracked.txt"]);
        let aba = workspace
            .refresh(CancellationToken::new())
            .await
            .expect("ABA poll");
        assert_ne!(aba.generation, b.generation);
        fs::write(&release, b"release").expect("release");
        let completion = worker.await.expect("prepare task");
        assert!(completion.prepared.is_none());
        assert_eq!(completion.error, Some(CommitErrorCode::ChangedDuringRead));
    }

    #[tokio::test]
    async fn trusted_git_rejects_intent_to_add_and_hidden_delete_form() {
        let repo = Repo::new();
        fs::write(repo.path().join("intent.txt"), "intent\n").expect("intent");
        run_git(repo.path(), &["add", "-N", "intent.txt"]);
        let (workspace, trusted) = repo.services().await;
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("workspace intent");
        assert_eq!(
            trusted.open_checklist(CancellationToken::new()).await,
            Err(CommitErrorCode::IntentToAdd)
        );
        fs::remove_file(repo.path().join("intent.txt")).expect("remove intent");
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("workspace hidden intent");
        assert_eq!(
            trusted.open_checklist(CancellationToken::new()).await,
            Err(CommitErrorCode::IntentToAdd)
        );
    }

    #[tokio::test]
    async fn trusted_git_rejects_detached_and_operation_state() {
        let repo = Repo::new();
        run_git(repo.path(), &["checkout", "--detach", "-q"]);
        let (_workspace, trusted) = repo.services().await;
        assert_eq!(
            trusted.open_checklist(CancellationToken::new()).await,
            Err(CommitErrorCode::UnsafeRepository)
        );
    }

    #[test]
    fn summary_escape_is_chunk_independent_and_bounded() {
        assert_eq!(
            escape_summary(b"a\xff\0b").expect("escape").rendered,
            "a\\xFF\\x00b"
        );
        assert_eq!(
            escape_summary(b"a\0\xff\r\n\tb")
                .expect("binary escape")
                .rendered,
            "a\\x00\\xFF\\x0D\n\tb"
        );
        for (raw, raw_overflow, expected_truncated) in [
            (vec![b'a'; SUMMARY_LIMIT], false, false),
            (vec![b'a'; SUMMARY_LIMIT + 1], false, true),
            (vec![b'a'; SUMMARY_LIMIT], true, true),
            (vec![0xff; SUMMARY_LIMIT / 2], false, true),
        ] {
            let (summary, truncated) =
                truncate_summary(escape_summary(&raw).expect("summary"), raw_overflow);
            assert_eq!(truncated, expected_truncated);
            assert!(summary.len() <= SUMMARY_LIMIT);
            if truncated {
                assert!(summary.ends_with(std::str::from_utf8(SUMMARY_MARKER).expect("marker")));
                assert_eq!(summary.matches("[vega-summary truncated=true]").count(), 1);
            }
        }
    }

    #[test]
    fn summary_rendered_exact_plus_one_escape_and_multibyte_boundaries() {
        let marker = std::str::from_utf8(SUMMARY_MARKER).expect("marker");
        for length in [SUMMARY_LIMIT - 1, SUMMARY_LIMIT, SUMMARY_LIMIT + 1] {
            let raw = "a".repeat(length);
            let (summary, truncated) = truncate_summary(
                escape_summary(raw.as_bytes()).expect("ASCII summary"),
                false,
            );
            assert_eq!(truncated, length > SUMMARY_LIMIT);
            assert!(summary.len() <= SUMMARY_LIMIT);
            if truncated {
                let payload = summary.strip_suffix(marker).expect("plus-one marker");
                assert_eq!(payload.len(), SUMMARY_LIMIT - SUMMARY_MARKER.len());
            } else {
                assert_eq!(summary, raw);
            }
        }

        let literal_slash = format!("{}\\", "s".repeat(SUMMARY_LIMIT - 1));
        let (literal_slash, truncated) = truncate_summary(
            escape_summary(literal_slash.as_bytes()).expect("literal slash summary"),
            false,
        );
        assert!(!truncated);
        assert_eq!(literal_slash.len(), SUMMARY_LIMIT);
        assert!(literal_slash.ends_with('\\'));

        let escaped_exact = escape_summary(&vec![0xff; SUMMARY_LIMIT / 4]).expect("escaped exact");
        assert_eq!(escaped_exact.rendered.len(), SUMMARY_LIMIT);
        let (escaped_exact, truncated) = truncate_summary(escaped_exact, false);
        assert!(!truncated);
        assert_eq!(escaped_exact.len(), SUMMARY_LIMIT);

        let mut escaped_plus_one_raw = vec![0xff; SUMMARY_LIMIT / 4];
        escaped_plus_one_raw.push(b'a');
        let escaped_plus_one = escape_summary(&escaped_plus_one_raw).expect("escaped plus one");
        assert_eq!(escaped_plus_one.rendered.len(), SUMMARY_LIMIT + 1);
        let (escaped_plus_one, truncated) = truncate_summary(escaped_plus_one, false);
        assert!(truncated);
        let escaped_payload = escaped_plus_one
            .strip_suffix(marker)
            .expect("escaped marker");
        assert_eq!(escaped_payload.len() % 4, 0);
        let (escape_chunks, escape_remainder) = escaped_payload.as_bytes().as_chunks::<4>();
        assert!(escape_remainder.is_empty());
        assert!(
            escape_chunks.iter().all(|chunk| chunk == b"\\xFF"),
            "partial escape retained before marker"
        );

        let target = SUMMARY_LIMIT - SUMMARY_MARKER.len();
        for literal_suffix in ["\\", "\\x", "\\xF"] {
            let mut raw = "l".repeat(target - literal_suffix.len());
            raw.push_str(literal_suffix);
            raw.push_str(&"tail".repeat(10));
            let (summary, truncated) = truncate_summary(
                escape_summary(raw.as_bytes()).expect("literal suffix"),
                false,
            );
            assert!(truncated);
            let payload = summary.strip_suffix(marker).expect("literal marker");
            assert_eq!(payload.len(), target);
            assert!(payload.ends_with(literal_suffix));
            assert_eq!(summary.matches(marker).count(), 1);
            assert_eq!(summary.len(), SUMMARY_LIMIT);
        }

        for generated_cut in 1..=3 {
            let prefix_len = target - generated_cut;
            let mut raw = vec![b'g'; prefix_len];
            raw.push(0xff);
            raw.extend_from_slice(&[b't'; 40]);
            let (summary, truncated) =
                truncate_summary(escape_summary(&raw).expect("generated escape"), false);
            assert!(truncated);
            let payload = summary.strip_suffix(marker).expect("generated marker");
            assert_eq!(payload.len(), prefix_len);
            assert!(payload.bytes().all(|byte| byte == b'g'));
            assert_eq!(summary.matches(marker).count(), 1);
            assert!(summary.len() <= SUMMARY_LIMIT);
        }

        let mut generated_exact_target = vec![b'x'];
        generated_exact_target.extend(std::iter::repeat_n(0xff, (target - 1) / 4));
        generated_exact_target.extend_from_slice(&[b't'; 40]);
        let (generated_exact_target, truncated) = truncate_summary(
            escape_summary(&generated_exact_target).expect("generated exact target"),
            false,
        );
        assert!(truncated);
        let generated_exact_payload = generated_exact_target
            .strip_suffix(marker)
            .expect("generated exact marker");
        assert_eq!(generated_exact_payload.len(), target);
        assert!(generated_exact_payload.ends_with("\\xFF"));
        assert_eq!(generated_exact_target.len(), SUMMARY_LIMIT);

        let mut multibyte_raw = "m".repeat(target - 1);
        multibyte_raw.push('é');
        multibyte_raw.push_str(&"tail".repeat(10));
        let (multibyte, truncated) = truncate_summary(
            escape_summary(multibyte_raw.as_bytes()).expect("multibyte summary"),
            false,
        );
        assert!(truncated);
        let multibyte_payload = multibyte.strip_suffix(marker).expect("multibyte marker");
        assert_eq!(multibyte_payload.len(), target - 1);
        assert!(multibyte_payload.chars().all(|character| character == 'm'));

        let literal_at_marker_cut = format!("{}\\{}", "q".repeat(target - 1), "tail".repeat(10));
        let (literal_at_marker_cut, truncated) = truncate_summary(
            escape_summary(literal_at_marker_cut.as_bytes()).expect("literal at marker cut"),
            false,
        );
        assert!(truncated);
        let payload = literal_at_marker_cut
            .strip_suffix(marker)
            .expect("literal marker cut marker");
        assert_eq!(payload.len(), target);
        assert!(payload.ends_with('\\'));
        assert_eq!(literal_at_marker_cut.len(), SUMMARY_LIMIT);
        assert_eq!(literal_at_marker_cut.matches(marker).count(), 1);
    }

    #[test]
    fn commit_summary_binary_bytes_are_deterministically_escaped() {
        let repo = Repo::new();
        let fixture = tempfile::tempdir().expect("binary summary fixture");
        let script = fixture.path().join("summary-git");
        fs::write(
            &script,
            "#!/bin/sh\nexec python3 -c 'import sys; sys.stdout.buffer.write(b\"a\\x00\\xff\\r\\n\\tb\")'\n",
        )
        .expect("binary summary script");
        let mut permissions = fs::metadata(&script)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("script executable");
        let runner = test_runner(repo.path());
        let runner = Runner::new(runner.root, runner.identity, Some(script));
        let output = runner
            .run_commit_summary(SUMMARY_LIMIT, &CancellationToken::new())
            .expect("binary summary");
        assert_eq!(
            escape_summary(&output.stdout)
                .expect("escaped binary summary")
                .rendered,
            "a\\x00\\xFF\\x0D\n\tb"
        );
    }

    #[tokio::test]
    async fn provider_draft_uses_strict_done_eof_grammar_and_redacted_output() {
        let provider = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::text("feat: safe"),
            vega_runtime::ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ]));
        let draft = collect_draft(
            provider.clone(),
            ChatRequest {
                model: "mock".into(),
                messages: Vec::new(),
                tools: Vec::new(),
                max_tokens: Some(256),
            },
            CancellationToken::new(),
        )
        .await
        .expect("draft");
        assert_eq!(draft, "feat: safe");
        assert_eq!(provider.requests().len(), 1);
        let invalid = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::text("partial"),
        ]));
        assert_eq!(
            collect_draft(invalid, ChatRequest::default(), CancellationToken::new()).await,
            Err(CommitErrorCode::DraftFailed)
        );
    }

    #[tokio::test]
    async fn provider_draft_grammar_table_is_closed_and_usage_star_is_accepted() {
        use vega_runtime::ScriptStep;

        let success = Arc::new(vega_runtime::MockProvider::new(vec![
            ScriptStep::text("x".repeat(MESSAGE_LIMIT)),
            ScriptStep::events(vec![
                ProviderEvent::Usage {
                    input: 1,
                    output: 2,
                    cache_read: 3,
                    cache_write: 4,
                },
                ProviderEvent::Usage {
                    input: 5,
                    output: 6,
                    cache_read: 7,
                    cache_write: 8,
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]),
        ]));
        assert_eq!(
            collect_draft(success, ChatRequest::default(), CancellationToken::new())
                .await
                .expect("Usage* grammar"),
            "x".repeat(MESSAGE_LIMIT)
        );

        let invalid_scripts = vec![
            vec![],
            vec![ScriptStep::text("partial")],
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("text".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("text".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
                ProviderEvent::TextDelta("after done".into()),
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("text".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
                ProviderEvent::Usage {
                    input: 1,
                    output: 1,
                    cache_read: 0,
                    cache_write: 0,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("text".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::Length,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("text".into()),
                ProviderEvent::Usage {
                    input: 1,
                    output: 1,
                    cache_read: 0,
                    cache_write: 0,
                },
                ProviderEvent::TextDelta("late".into()),
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::ThinkingDelta("secret".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "id".into(),
                    name: "tool".into(),
                    input_json: "{}".into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
            vec![
                ScriptStep::text("partial"),
                ScriptStep::Error {
                    status: Some(500),
                    message: "provider payload".into(),
                    retryable: false,
                },
            ],
            vec![ScriptStep::Error {
                status: Some(500),
                message: "provider setup payload".into(),
                retryable: false,
            }],
            vec![
                ScriptStep::events(vec![
                    ProviderEvent::TextDelta("text".into()),
                    ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    },
                ]),
                ScriptStep::Error {
                    status: Some(500),
                    message: "provider after done payload".into(),
                    retryable: false,
                },
            ],
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("nul\0text".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])],
            vec![
                ScriptStep::text("x".repeat(MESSAGE_LIMIT + 1)),
                ScriptStep::events(vec![ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }]),
            ],
        ];
        for (case, script) in invalid_scripts.into_iter().enumerate() {
            let provider = Arc::new(vega_runtime::MockProvider::new(script));
            assert_eq!(
                collect_draft(provider, ChatRequest::default(), CancellationToken::new()).await,
                Err(CommitErrorCode::DraftFailed),
                "invalid grammar case {case}"
            );
        }
        assert_eq!(
            checked_draft_len(usize::MAX, 1),
            Err(CommitErrorCode::DraftFailed)
        );
    }

    #[tokio::test]
    async fn draft_deadline_covers_setup_pre_done_and_post_done_stalls() {
        #[derive(Clone, Copy)]
        enum Phase {
            Setup,
            PreDone,
            PostDone,
        }
        struct StallingProvider(Phase);
        impl Provider for StallingProvider {
            fn chat_stream(
                &self,
                _request: ChatRequest,
                _cancel: CancellationToken,
            ) -> futures::future::BoxFuture<
                'static,
                Result<vega_runtime::EventStream, vega_runtime::VegaError>,
            > {
                match self.0 {
                    Phase::Setup => Box::pin(std::future::pending()),
                    Phase::PreDone => Box::pin(async {
                        let stream = futures::stream::iter(vec![Ok(ProviderEvent::TextDelta(
                            "partial".into(),
                        ))])
                        .chain(futures::stream::pending());
                        Ok(Box::pin(stream) as vega_runtime::EventStream)
                    }),
                    Phase::PostDone => Box::pin(async {
                        let stream = futures::stream::iter(vec![
                            Ok(ProviderEvent::TextDelta("complete".into())),
                            Ok(ProviderEvent::Done {
                                stop_reason: StopReason::End,
                            }),
                        ])
                        .chain(futures::stream::pending());
                        Ok(Box::pin(stream) as vega_runtime::EventStream)
                    }),
                }
            }
        }
        for phase in [Phase::Setup, Phase::PreDone, Phase::PostDone] {
            let started = Instant::now();
            assert_eq!(
                collect_draft_with_deadline(
                    Arc::new(StallingProvider(phase)),
                    ChatRequest::default(),
                    CancellationToken::new(),
                    Duration::from_millis(25),
                )
                .await,
                Err(CommitErrorCode::DraftFailed)
            );
            assert!(started.elapsed() >= Duration::from_millis(20));
            assert!(started.elapsed() < Duration::from_secs(1));
        }
    }

    #[tokio::test]
    async fn draft_cancel_is_biased_at_setup_event_and_post_done_stalls() {
        #[derive(Clone, Copy)]
        enum Phase {
            Setup,
            PreDone,
            PostDone,
        }
        struct CancelStallProvider {
            phase: Phase,
            ready: Arc<tokio::sync::Notify>,
        }
        impl Provider for CancelStallProvider {
            fn chat_stream(
                &self,
                _request: ChatRequest,
                _cancel: CancellationToken,
            ) -> futures::future::BoxFuture<
                'static,
                Result<vega_runtime::EventStream, vega_runtime::VegaError>,
            > {
                let ready = self.ready.clone();
                match self.phase {
                    Phase::Setup => Box::pin(async move {
                        ready.notify_one();
                        std::future::pending().await
                    }),
                    phase => Box::pin(async move {
                        let prefix = match phase {
                            Phase::PreDone => vec![Ok(ProviderEvent::TextDelta("partial".into()))],
                            Phase::PostDone => vec![
                                Ok(ProviderEvent::TextDelta("complete".into())),
                                Ok(ProviderEvent::Done {
                                    stop_reason: StopReason::End,
                                }),
                            ],
                            Phase::Setup => unreachable!(),
                        };
                        let tail = futures::stream::once(async move {
                            ready.notify_one();
                            std::future::pending::<Result<ProviderEvent, vega_runtime::VegaError>>()
                                .await
                        });
                        Ok(Box::pin(futures::stream::iter(prefix).chain(tail))
                            as vega_runtime::EventStream)
                    }),
                }
            }
        }
        for phase in [Phase::Setup, Phase::PreDone, Phase::PostDone] {
            let ready = Arc::new(tokio::sync::Notify::new());
            let cancel = CancellationToken::new();
            let worker = tokio::spawn(collect_draft_with_deadline(
                Arc::new(CancelStallProvider {
                    phase,
                    ready: ready.clone(),
                }),
                ChatRequest::default(),
                cancel.clone(),
                Duration::from_secs(1),
            ));
            tokio::time::timeout(Duration::from_secs(1), ready.notified())
                .await
                .expect("stall reached");
            cancel.cancel();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), worker)
                    .await
                    .expect("cancel bounded")
                    .expect("draft task"),
                Err(CommitErrorCode::DraftFailed)
            );
        }
    }

    #[tokio::test]
    async fn draft_cancel_wins_when_cancel_and_provider_branch_are_both_ready() {
        #[derive(Clone, Copy)]
        enum Phase {
            Setup,
            PreDone,
            PostDone,
        }
        struct GatedProvider {
            phase: Phase,
            ready: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Notify>,
            provider_branch_selected: Arc<AtomicUsize>,
        }
        impl Provider for GatedProvider {
            fn chat_stream(
                &self,
                _request: ChatRequest,
                _cancel: CancellationToken,
            ) -> futures::future::BoxFuture<
                'static,
                Result<vega_runtime::EventStream, vega_runtime::VegaError>,
            > {
                let ready = self.ready.clone();
                let release = self.release.clone();
                let selected = self.provider_branch_selected.clone();
                match self.phase {
                    Phase::Setup => Box::pin(async move {
                        ready.notify_one();
                        release.notified().await;
                        selected.fetch_add(1, Ordering::SeqCst);
                        Ok(Box::pin(futures::stream::empty()) as vega_runtime::EventStream)
                    }),
                    Phase::PreDone => Box::pin(async move {
                        let gated_done = futures::stream::once(async move {
                            ready.notify_one();
                            release.notified().await;
                            selected.fetch_add(1, Ordering::SeqCst);
                            Ok(ProviderEvent::Done {
                                stop_reason: StopReason::End,
                            })
                        });
                        Ok(Box::pin(
                            futures::stream::iter(vec![Ok(ProviderEvent::TextDelta(
                                "partial".into(),
                            ))])
                            .chain(gated_done),
                        ) as vega_runtime::EventStream)
                    }),
                    Phase::PostDone => Box::pin(async move {
                        let gated_eof = futures::stream::unfold((), move |()| {
                            let ready = ready.clone();
                            let release = release.clone();
                            let selected = selected.clone();
                            async move {
                                ready.notify_one();
                                release.notified().await;
                                selected.fetch_add(1, Ordering::SeqCst);
                                None::<(Result<ProviderEvent, vega_runtime::VegaError>, ())>
                            }
                        });
                        Ok(Box::pin(
                            futures::stream::iter(vec![
                                Ok(ProviderEvent::TextDelta("complete".into())),
                                Ok(ProviderEvent::Done {
                                    stop_reason: StopReason::End,
                                }),
                            ])
                            .chain(gated_eof),
                        ) as vega_runtime::EventStream)
                    }),
                }
            }
        }
        for phase in [Phase::Setup, Phase::PreDone, Phase::PostDone] {
            let ready = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let selected = Arc::new(AtomicUsize::new(0));
            let cancel = CancellationToken::new();
            let worker = tokio::spawn(collect_draft_with_deadline(
                Arc::new(GatedProvider {
                    phase,
                    ready: ready.clone(),
                    release: release.clone(),
                    provider_branch_selected: selected.clone(),
                }),
                ChatRequest::default(),
                cancel.clone(),
                Duration::from_secs(1),
            ));
            tokio::time::timeout(Duration::from_secs(1), ready.notified())
                .await
                .expect("provider branch reached gate");
            // There is deliberately no await between these operations. The
            // worker's next poll observes both select branches as ready.
            cancel.cancel();
            release.notify_one();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), worker)
                    .await
                    .expect("biased cancel bounded")
                    .expect("draft task"),
                Err(CommitErrorCode::DraftFailed)
            );
            assert_eq!(
                selected.load(Ordering::SeqCst),
                0,
                "provider branch won a simultaneous-ready race"
            );
        }
    }

    #[tokio::test]
    async fn commit_draft_request_matches_frozen_literals_for_both_truncation_flags() {
        const FIXTURE_SUMMARY: &str = "fixture staged summary";
        const EXPECTED_SYSTEM: &str = "Generate one concise Git commit message for the exact staged diff. Return only the commit message text. Do not call tools.";
        for truncated in [false, true] {
            let (_repo, _recorder, trusted, prepared, _argv, _input) =
                staged_service_with_recorder().await;
            {
                let mut state = trusted
                    .state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                let stored = state.prepared.as_mut().expect("stored prepared");
                stored.summary = FIXTURE_SUMMARY.into();
                stored.summary_truncated = truncated;
            }
            let provider = Arc::new(vega_runtime::MockProvider::new(vec![
                vega_runtime::ScriptStep::text("feat: exact request"),
                vega_runtime::ScriptStep::events(vec![ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }]),
            ]));
            let draft = trusted
                .draft(
                    prepared.id,
                    "commit-model-sentinel".into(),
                    provider.clone(),
                    CancellationToken::new(),
                )
                .await
                .expect("draft");
            assert!(draft.text() == "feat: exact request", "draft mismatch");
            let requests = provider.requests();
            assert_eq!(requests.len(), 1);
            let request = &requests[0];
            assert!(request.model == "commit-model-sentinel", "model mismatch");
            assert!(request.tools.is_empty(), "commit request advertised tools");
            assert_eq!(request.max_tokens, Some(256));
            assert_eq!(request.messages.len(), 2);
            assert!(
                request.messages[0] == ChatMessage::new(ChatRole::System, EXPECTED_SYSTEM),
                "system prompt mismatch"
            );
            let expected_user = format!(
                "Generate the commit message for the staged diff below.\ntruncated={}\n--- staged diff ---\n{FIXTURE_SUMMARY}",
                if truncated { "true" } else { "false" }
            );
            assert!(
                request.messages[1] == ChatMessage::new(ChatRole::User, expected_user),
                "user prompt mismatch"
            );
        }
    }

    #[tokio::test]
    async fn failed_draft_keeps_prepared_authority_usable() {
        let (_repo, _recorder, trusted, prepared, argv, input) =
            staged_service_with_recorder().await;
        assert!(
            !argv.exists() && !input.exists(),
            "draft fixture mutated before provider"
        );
        let invalid = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::text("partial"),
        ]));
        assert_eq!(
            trusted
                .draft(
                    prepared.id,
                    "model".into(),
                    invalid,
                    CancellationToken::new(),
                )
                .await,
            Err(CommitErrorCode::DraftFailed)
        );
        let valid = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::text("feat: recovered draft"),
            vega_runtime::ScriptStep::events(vec![
                ProviderEvent::Usage {
                    input: 1,
                    output: 1,
                    cache_read: 0,
                    cache_write: 0,
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]),
        ]));
        assert_eq!(
            trusted
                .draft(prepared.id, "model".into(), valid, CancellationToken::new(),)
                .await
                .expect("recovered draft")
                .text(),
            "feat: recovered draft"
        );
        let state = trusted
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(
            state.prepared.as_ref().map(|stored| stored.id),
            Some(prepared.id)
        );
        assert!(!state.mutation_active);
        assert!(
            !argv.exists() && !input.exists(),
            "draft path started a Git mutation"
        );
    }

    #[tokio::test]
    async fn summary_authority_change_after_capture_fails_before_provider() {
        let repo = Repo::new();
        fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged file");
        fs::write(repo.path().join("outside.txt"), "base\n").expect("outside file");
        run_git(repo.path(), &["add", "staged.txt", "outside.txt"]);
        run_git(repo.path(), &["commit", "-qm", "summary base"]);
        fs::write(repo.path().join("staged.txt"), "staged changed\n").expect("staged change");
        run_git(repo.path(), &["add", "staged.txt"]);
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("workspace A");
        let (_gate, read, ready, release) = blocking_summary_reader();
        let trusted = Arc::new(
            TrustedGitService::new_with_executables_for_test(
                repo.path(),
                workspace,
                PathBuf::from(GIT),
                read,
            )
            .expect("trusted summary barrier"),
        );
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
        let worker = tokio::spawn({
            let trusted = trusted.clone();
            async move {
                trusted
                    .prepare(checklist.id, Vec::new(), CancellationToken::new())
                    .await
            }
        });
        wait_for_path(&ready).await;
        fs::write(repo.path().join("outside.txt"), "outside drift\n").expect("outside drift");
        run_git(repo.path(), &["add", "outside.txt"]);
        fs::write(release, b"release").expect("release summary");
        let completion = worker.await.expect("prepare worker");
        assert_eq!(completion.error, Some(CommitErrorCode::ChangedDuringRead));
        assert!(completion.prepared.is_none());
        assert!(completion.workspace.is_some());
        assert_eq!(
            provider.requests().len(),
            0,
            "provider must remain uncalled"
        );
    }

    #[test]
    fn commit_redaction_all_public_provider_carriers_hide_sentinels() {
        const SENTINEL: &str = "VEGA_T34_SECRET_SENTINEL";
        let call = vega_runtime::ChatToolCall {
            id: SENTINEL.into(),
            name: SENTINEL.into(),
            input_json: SENTINEL.into(),
        };
        let request = ChatRequest {
            model: SENTINEL.into(),
            messages: vec![ChatMessage::assistant_with_tools(SENTINEL, vec![call])],
            tools: vec![vega_runtime::ToolDefinition {
                name: SENTINEL.into(),
                description: SENTINEL.into(),
                input_schema: serde_json::json!({"sentinel": SENTINEL}),
            }],
            max_tokens: Some(256),
        };
        let event = ProviderEvent::ToolUse {
            id: SENTINEL.into(),
            name: SENTINEL.into(),
            input_json: SENTINEL.into(),
        };
        let step = vega_runtime::ScriptStep::Error {
            status: Some(500),
            message: SENTINEL.into(),
            retryable: false,
        };
        let mock = vega_runtime::MockProvider::new(vec![step.clone()]);
        let draft = CommitDraft::new(SENTINEL.into());
        let runtime_result = vega_runtime::RuntimeToolResult {
            call_id: SENTINEL.into(),
            output: SENTINEL.into(),
            status: vega_runtime::RuntimeToolStatus::Failed,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            approval: None,
            remember_rule: None,
        };
        let runtime_call = vega_runtime::RuntimeToolCall {
            id: SENTINEL.into(),
            name: SENTINEL.into(),
            input_json: SENTINEL.into(),
        };
        let runtime_event =
            vega_runtime::RuntimeEvent::Error(Arc::new(vega_runtime::VegaError::Provider {
                status: Some(500),
                message: SENTINEL.into(),
                retryable: false,
            }));
        let agent_request = vega_runtime::AgentRequest {
            model: SENTINEL.into(),
            system_prompt: SENTINEL.into(),
            history: vec![ChatMessage::new(ChatRole::User, SENTINEL)],
            max_tokens: Some(256),
            completed_tool_results: std::collections::HashMap::from([(
                SENTINEL.into(),
                vega_runtime::CompletedToolCall {
                    tool: SENTINEL.into(),
                    input_json: SENTINEL.into(),
                    result: runtime_result.clone(),
                },
            )]),
            tool_config: vega_runtime::RuntimeToolConfig::new(
                vega_runtime::RuntimeRunMode::Execute,
                vega_runtime::RuntimePermissionMode::Confirm,
                SENTINEL.into(),
                SENTINEL.into(),
                PathBuf::from(SENTINEL),
                vec![vega_runtime::RuntimeExactRule {
                    tool: vega_runtime::RuntimeMutatingTool::Write,
                    pattern: SENTINEL.into(),
                }],
            ),
        };
        let agent_outcome = vega_runtime::AgentOutcome {
            events: vec![runtime_event.clone()],
            messages: vec![ChatMessage::new(ChatRole::Assistant, SENTINEL)],
            final_text: SENTINEL.into(),
            tool_call_count: 1,
            executed_tool_call_count: 0,
            interrupted: false,
            failed: true,
        };
        let conversation_result = crate::types::ToolResult {
            status: crate::types::ToolCallStatus::Failed,
            output: SENTINEL.into(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            invalid: None,
        };
        let conversation_event = crate::types::ConversationEvent::ToolCallFinished {
            call_id: SENTINEL.into(),
            result: conversation_result.clone(),
        };
        let conversation_error = crate::types::ConversationEvent::Error {
            message_id: Some(SENTINEL.into()),
            error: Arc::new(vega_runtime::VegaError::Provider {
                status: None,
                message: SENTINEL.into(),
                retryable: false,
            }),
        };
        let conversation_run = crate::agent::ConversationRun {
            user_message_id: SENTINEL.into(),
            assistant_message_id: SENTINEL.into(),
            events: vec![conversation_event.clone(), conversation_error.clone()],
            content: SENTINEL.into(),
            interrupted: false,
            failed: true,
        };
        for debug in [
            format!("{request:?}"),
            format!("{event:?}"),
            format!("{step:?}"),
            format!("{mock:?}"),
            format!("{draft:?}"),
            format!("{runtime_result:?}"),
            format!("{runtime_call:?}"),
            format!("{runtime_event:?}"),
            format!("{agent_request:?}"),
            format!("{agent_outcome:?}"),
            format!("{conversation_result:?}"),
            format!("{conversation_event:?}"),
            format!("{conversation_error:?}"),
            format!("{conversation_run:?}"),
        ] {
            assert!(!debug.contains(SENTINEL), "debug leaked sentinel");
        }
    }

    #[test]
    fn three_source_parsers_reject_conflicting_duplicate_paths() {
        let oid_a = b"1111111111111111111111111111111111111111";
        let oid_b = b"2222222222222222222222222222222222222222";
        let mut stages = Vec::new();
        stages.extend_from_slice(b"100644 ");
        stages.extend_from_slice(oid_a);
        stages.extend_from_slice(b" 0\tduplicate.txt\x00100755 ");
        stages.extend_from_slice(oid_b);
        stages.extend_from_slice(b" 0\tduplicate.txt\0");
        assert!(matches!(
            parse_stages(&stages, 40),
            Err(CommitErrorCode::MalformedOutput)
        ));

        let mut tree = Vec::new();
        tree.extend_from_slice(b"100644 blob ");
        tree.extend_from_slice(oid_a);
        tree.extend_from_slice(b"\tduplicate.txt\x00100755 blob ");
        tree.extend_from_slice(oid_b);
        tree.extend_from_slice(b"\tduplicate.txt\0");
        assert!(matches!(
            parse_tree(&tree, 40),
            Err(CommitErrorCode::MalformedOutput)
        ));
    }

    #[test]
    fn stage_and_tree_codecs_reject_noncanonical_nul_framing() {
        let oid = b"1111111111111111111111111111111111111111";
        let mut stage = b"100644 ".to_vec();
        stage.extend_from_slice(oid);
        stage.extend_from_slice(b" 0\tfile.txt");
        let mut tree = b"100644 blob ".to_vec();
        tree.extend_from_slice(oid);
        tree.extend_from_slice(b"\tfile.txt");

        assert!(matches!(parse_stages(b"", 40), Ok(entries) if entries.is_empty()));
        assert!(matches!(parse_tree(b"", 40), Ok(entries) if entries.is_empty()));
        for record in [stage, tree] {
            let is_tree = record.starts_with(b"100644 blob");
            let parser = |bytes: &[u8]| {
                if is_tree {
                    parse_tree(bytes, 40).map(|entries| entries.len())
                } else {
                    parse_stages(bytes, 40).map(|entries| entries.len())
                }
            };
            assert_eq!(parser(b"\0"), Err(CommitErrorCode::MalformedOutput));
            let mut leading = vec![0];
            leading.extend_from_slice(&record);
            leading.push(0);
            assert_eq!(parser(&leading), Err(CommitErrorCode::MalformedOutput));
            let mut doubled = record.clone();
            doubled.extend_from_slice(b"\0\0");
            assert_eq!(parser(&doubled), Err(CommitErrorCode::MalformedOutput));
            assert_eq!(parser(&record), Err(CommitErrorCode::MalformedOutput));
            let mut canonical = record;
            canonical.push(0);
            assert_eq!(parser(&canonical), Ok(1));
        }
    }

    #[test]
    fn status_codec_uses_closed_xy_shape_and_header_whitelists() {
        for &x in b".MTADRCU?" {
            for &y in b".MTADRCU?" {
                let ordinary = match x {
                    b'.' => matches!(y, b'M' | b'T' | b'D'),
                    b'M' | b'T' | b'A' => matches!(y, b'.' | b'M' | b'T' | b'D'),
                    b'D' => y == b'.',
                    _ => false,
                };
                assert_eq!(
                    canonical_status_pair(StatusShape::Ordinary, x, y),
                    ordinary,
                    "ordinary {x:?}{y:?}"
                );
                let rename = x == b'R' && matches!(y, b'.' | b'M' | b'T' | b'D');
                assert_eq!(
                    canonical_status_pair(StatusShape::Rename, x, y),
                    rename,
                    "rename {x:?}{y:?}"
                );
                let copy = x == b'C' && matches!(y, b'.' | b'M' | b'T' | b'D');
                assert_eq!(
                    canonical_status_pair(StatusShape::Copy, x, y),
                    copy,
                    "copy {x:?}{y:?}"
                );
            }
        }

        let head = HeadAuthority {
            unborn: false,
            oid: b"1111111111111111111111111111111111111111".to_vec(),
            short: b"main".to_vec(),
            full_ref: b"refs/heads/main".to_vec(),
        };
        let mut unknown =
            b"# branch.oid 1111111111111111111111111111111111111111\0# branch.head main\0".to_vec();
        unknown.extend_from_slice(b"# branch.future value\0");
        assert!(matches!(
            parse_commit_status(&unknown, &head),
            Err(CommitErrorCode::MalformedOutput)
        ));
    }

    #[test]
    fn special_selected_components_are_rejected_before_any_mutation() {
        let file_id = WorkspaceFileId {
            generation: 1,
            slot: 0,
            seal: 7,
        };
        for kind in [
            CommitSelectionKind::Added,
            CommitSelectionKind::Modified,
            CommitSelectionKind::TypeChanged,
            CommitSelectionKind::Renamed,
            CommitSelectionKind::Copied,
        ] {
            let row = ChecklistRow {
                public: CommitSelection {
                    file_id,
                    label: "special".into(),
                    previous_label: None,
                    kind,
                    forced: false,
                },
                closure: vec![b"special".to_vec()],
                record: StatusRecord {
                    shape: StatusShape::Untracked,
                    x: b'?',
                    y: b'?',
                    sub: b"N...".to_vec(),
                    head_mode: b"000000".to_vec(),
                    index_mode: b"000000".to_vec(),
                    worktree_mode: b"000000".to_vec(),
                    head_oid: vec![b'0'; 40],
                    index_oid: vec![b'0'; 40],
                    path: b"special".to_vec(),
                    previous: None,
                },
                optional_kind: kind,
                worktree_mode: None,
            };
            let checklist = StoredChecklist {
                id: IndexSnapshotId {
                    generation: 0,
                    slot: 0,
                    seal: 0,
                },
                authority: IndexAuthority {
                    head: HeadAuthority {
                        unborn: false,
                        oid: vec![b'1'; 40],
                        short: b"main".to_vec(),
                        full_ref: b"refs/heads/main".to_vec(),
                    },
                    status_raw: Vec::new(),
                    stage_raw: Vec::new(),
                    tree_raw: Vec::new(),
                    records: Vec::new(),
                    stages: Vec::new(),
                    tree: Vec::new(),
                    workspace_generation: 1,
                },
                optional: vec![row],
            };
            assert!(matches!(
                resolve_selected(&checklist, &[file_id]),
                Err(CommitErrorCode::InvalidSelection)
            ));
        }
    }

    #[test]
    fn selected_copy_components_share_only_the_source_invariant() {
        let oid = |byte: u8| vec![byte; 40];
        let head = HeadAuthority {
            unborn: false,
            oid: oid(b'a'),
            short: b"master".to_vec(),
            full_ref: b"refs/heads/master".to_vec(),
        };
        let source = StageEntry {
            mode: b"100644".to_vec(),
            oid: oid(b'1'),
            path: b"source.txt".to_vec(),
        };
        let staged_copy = |path: &[u8], object: u8| StageEntry {
            mode: b"100644".to_vec(),
            oid: oid(object),
            path: path.to_vec(),
        };
        let copy_record = |path: &[u8]| StatusRecord {
            shape: StatusShape::Copy,
            x: b'C',
            y: b'M',
            sub: b"N...".to_vec(),
            head_mode: b"100644".to_vec(),
            index_mode: b"100644".to_vec(),
            worktree_mode: b"100644".to_vec(),
            head_oid: oid(b'1'),
            index_oid: oid(b'1'),
            path: path.to_vec(),
            previous: Some(b"source.txt".to_vec()),
        };
        let final_record = |path: &[u8]| StatusRecord {
            shape: StatusShape::Ordinary,
            x: b'A',
            y: b'.',
            sub: b"N...".to_vec(),
            head_mode: b"000000".to_vec(),
            index_mode: b"100644".to_vec(),
            worktree_mode: b"100644".to_vec(),
            head_oid: oid(b'0'),
            index_oid: oid(b'2'),
            path: path.to_vec(),
            previous: None,
        };
        let make_row = |slot: u32, path: &[u8]| ChecklistRow {
            public: CommitSelection {
                file_id: WorkspaceFileId {
                    generation: 1,
                    slot,
                    seal: u64::from(slot),
                },
                label: String::new(),
                previous_label: None,
                kind: CommitSelectionKind::Modified,
                forced: false,
            },
            closure: vec![path.to_vec()],
            record: copy_record(path),
            optional_kind: CommitSelectionKind::Modified,
            worktree_mode: Some(b"100644".to_vec()),
        };
        let rows = [make_row(1, b"copy-one.txt"), make_row(2, b"copy-two.txt")];
        let selected = vec![&rows[0], &rows[1]];
        let a = IndexAuthority {
            head: head.clone(),
            status_raw: Vec::new(),
            stage_raw: Vec::new(),
            tree_raw: Vec::new(),
            records: vec![copy_record(b"copy-one.txt"), copy_record(b"copy-two.txt")],
            stages: vec![
                staged_copy(b"copy-one.txt", b'1'),
                staged_copy(b"copy-two.txt", b'1'),
                source.clone(),
            ],
            tree: Vec::new(),
            workspace_generation: 1,
        };
        let b = IndexAuthority {
            head,
            status_raw: Vec::new(),
            stage_raw: Vec::new(),
            tree_raw: Vec::new(),
            records: vec![final_record(b"copy-one.txt"), final_record(b"copy-two.txt")],
            stages: vec![
                staged_copy(b"copy-one.txt", b'2'),
                staged_copy(b"copy-two.txt", b'2'),
                source,
            ],
            tree: Vec::new(),
            workspace_generation: 2,
        };
        let paths = vec![b"copy-one.txt".to_vec(), b"copy-two.txt".to_vec()];
        assert_eq!(validate_transition(&a, &b, &selected, &paths), Ok(()));

        let mut source_drift = b.clone();
        source_drift.records.push(StatusRecord {
            shape: StatusShape::Ordinary,
            x: b'.',
            y: b'M',
            sub: b"N...".to_vec(),
            head_mode: b"100644".to_vec(),
            index_mode: b"100644".to_vec(),
            worktree_mode: b"100644".to_vec(),
            head_oid: oid(b'1'),
            index_oid: oid(b'1'),
            path: b"source.txt".to_vec(),
            previous: None,
        });
        assert_eq!(
            validate_transition(&a, &source_drift, &selected, &paths),
            Err(CommitErrorCode::ChangedDuringRead)
        );

        let mut mode_flip = b.clone();
        mode_flip
            .stages
            .iter_mut()
            .find(|entry| entry.path == b"copy-one.txt")
            .expect("copy destination")
            .mode = b"100755".to_vec();
        assert_eq!(
            validate_transition(&a, &mode_flip, &selected, &paths),
            Err(CommitErrorCode::ChangedDuringRead)
        );

        let overlap = [make_row(3, b"copy-one.txt"), make_row(4, b"copy-one.txt")];
        assert_eq!(
            validate_transition(&a, &b, &[&overlap[0], &overlap[1]], &paths),
            Err(CommitErrorCode::InvalidSelection)
        );
    }

    #[tokio::test]
    async fn sha256_repository_completes_checklist_prepare_and_commit() {
        let repo = match Repo::try_sha256() {
            Ok(repo) => repo,
            Err(reason) => {
                eprintln!("SKIP sha256 repository E2E: {reason}");
                return;
            }
        };
        assert_eq!(
            run_git_output(repo.path(), &["rev-parse", "--show-object-format"]),
            b"sha256\n"
        );
        fs::write(repo.path().join("tracked.txt"), "sha256 change\n")
            .expect("sha256 worktree change");
        let base = run_git_output(repo.path(), &["rev-parse", "HEAD"])
            .strip_suffix(b"\n")
            .expect("sha256 base newline")
            .to_vec();
        assert!(valid_nonzero_oid(&base, 64));
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("sha256 workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("sha256 workspace refresh");
        let (_read_dir, read, read_log) = proof_read_recorder(repo.path(), &base, "ok");
        let (mutation_dir, mutation, _mutation_argv, _mutation_input) = mutation_recorder();
        let trusted = TrustedGitService::new_with_executables_for_test(
            repo.path(),
            workspace,
            mutation,
            read,
        )
        .expect("sha256 trusted service");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("sha256 checklist");
        assert_eq!(checklist.optional.len(), 1);
        let prepared = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await
            .prepared
            .expect("sha256 prepared");
        {
            let state = trusted
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let stored = state.prepared.as_ref().expect("stored sha256 B");
            assert!(valid_nonzero_oid(&stored.authority.head.oid, 64));
            assert!(
                stored
                    .authority
                    .records
                    .iter()
                    .all(|record| { record.head_oid.len() == 64 && record.index_oid.len() == 64 })
            );
            assert!(
                stored
                    .authority
                    .stages
                    .iter()
                    .all(|stage| valid_nonzero_oid(&stage.oid, 64))
            );
            assert!(
                stored
                    .authority
                    .tree
                    .iter()
                    .all(|entry| valid_nonzero_oid(&entry.oid, 64))
            );
        }
        let provider = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::text("test: sha256 commit"),
            vega_runtime::ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ]));
        let draft = trusted
            .draft(
                prepared.id,
                "mock-sha256".into(),
                provider.clone(),
                CancellationToken::new(),
            )
            .await
            .expect("sha256 mock draft");
        assert_eq!(provider.requests().len(), 1);
        assert!(provider.requests()[0].tools.is_empty());
        assert_eq!(provider.requests()[0].max_tokens, Some(256));
        let completion = trusted
            .commit(
                prepared.id,
                draft.text().to_owned(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.outcome, CommitOutcome::Committed);
        let oid = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
        let oid = oid.strip_suffix(b"\n").expect("sha256 oid newline");
        assert!(valid_nonzero_oid(oid, 64));
        assert_eq!(
            fs::read(mutation_dir.path().join("mutation-attempts"))
                .expect("sha256 add and commit attempts"),
            b"xx"
        );
        let reads = read_invocations(&read_log);
        assert!(reads.iter().flatten().any(|argument| {
            argument.len() == 66
                && argument.ends_with(b"^@")
                && valid_nonzero_oid(&argument[..64], 64)
        }));
        assert!(reads.iter().any(|invocation| {
            invocation.iter().any(|argument| argument == b"ls-tree")
                && invocation.iter().any(|argument| argument == oid)
        }));
        assert_terminal_workspace(
            &trusted,
            completion.workspace.as_ref().expect("sha256 workspace"),
        );
    }

    #[test]
    fn head_and_ref_oid_codecs_reject_mixed_width_uppercase_and_zero() {
        let valid_40 = vec![b'a'; 40];
        let valid_64 = vec![b'b'; 64];
        assert!(valid_nonzero_oid(&valid_40, 40));
        assert!(valid_nonzero_oid(&valid_64, 64));
        for (value, width) in [
            (vec![b'a'; 39], 40),
            (vec![b'a'; 41], 40),
            (vec![b'a'; 40], 64),
            (vec![b'a'; 64], 40),
            (vec![b'A'; 40], 40),
            (vec![b'0'; 40], 40),
            (vec![b'0'; 64], 64),
        ] {
            assert!(!valid_nonzero_oid(&value, width));
            let mut refs = value;
            refs.extend_from_slice(b"\0refs/heads/master\0\n");
            assert_eq!(
                parse_ref_target(&refs, b"refs/heads/master", width),
                Err(CommitErrorCode::MalformedOutput)
            );
        }
    }

    #[tokio::test]
    async fn capture_head_service_rejects_bad_born_oids_before_any_mutation() {
        for bad_oid in [
            "0".repeat(40),
            "0".repeat(64),
            "A".repeat(40),
            "a".repeat(39),
            "a".repeat(64),
        ] {
            let repo = Repo::new();
            fs::write(repo.path().join("tracked.txt"), "candidate\n").expect("candidate");
            let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
            let terminal = workspace
                .refresh(CancellationToken::new())
                .await
                .expect("baseline workspace");
            let read_dir = tempfile::tempdir().expect("bad head read fixture");
            let read = read_dir.path().join("git-read.sh");
            fs::write(
                &read,
                format!(
                    "#!/bin/sh\nset -eu\nfor arg in \"$@\"; do if [ \"$arg\" = status ]; then printf '# branch.oid {bad_oid}\\0# branch.head master\\0'; exit 0; fi; done\nexec /usr/bin/git \"$@\"\n"
                ),
            )
            .expect("bad head read script");
            let mut permissions = fs::metadata(&read)
                .expect("bad head read metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&read, permissions).expect("bad head read executable");
            let (mutation_dir, mutation, _argv, _input) = mutation_recorder();
            let trusted = TrustedGitService::new_with_executables_for_test(
                repo.path(),
                workspace,
                mutation,
                read,
            )
            .expect("bad head trusted service");
            assert!(
                matches!(
                    trusted.open_checklist(CancellationToken::new()).await,
                    Err(CommitErrorCode::MalformedOutput)
                ),
                "bad born oid length={} prefix={:?}",
                bad_oid.len(),
                bad_oid.as_bytes().first()
            );
            assert!(!mutation_dir.path().join("mutation-attempts").exists());
            assert_terminal_workspace(&trusted, &terminal);
        }
    }

    #[test]
    fn mode_codecs_and_gitlink_union_are_closed() {
        let oid = vec![b'1'; 40];
        for mode in [b"100644", b"100755", b"120000", b"160000"] {
            assert_eq!(
                parse_stages(&stage_record(mode, &oid, b"path"), 40).map(|v| v.len()),
                Ok(1)
            );
        }
        for (mode, kind) in [
            (b"100644".as_slice(), b"blob".as_slice()),
            (b"100755".as_slice(), b"blob".as_slice()),
            (b"120000".as_slice(), b"blob".as_slice()),
            (b"160000".as_slice(), b"commit".as_slice()),
        ] {
            assert_eq!(
                parse_tree(&tree_record(mode, kind, &oid, b"path"), 40).map(|v| v.len()),
                Ok(1)
            );
        }
        for (mode, kind) in [
            (b"040000".as_slice(), b"tree".as_slice()),
            (b"100600".as_slice(), b"blob".as_slice()),
            (b"160000".as_slice(), b"blob".as_slice()),
            (b"100644".as_slice(), b"commit".as_slice()),
        ] {
            assert!(matches!(
                parse_tree(&tree_record(mode, kind, &oid, b"path"), 40),
                Err(CommitErrorCode::MalformedOutput)
            ));
        }
        for stage in [b"1", b"2", b"3"] {
            let mut record = b"100644 ".to_vec();
            record.extend_from_slice(&oid);
            record.push(b' ');
            record.extend_from_slice(stage);
            record.extend_from_slice(b"\tpath\0");
            assert!(matches!(
                parse_stages(&record, 40),
                Err(CommitErrorCode::MalformedOutput)
            ));
        }
        assert!(matches!(
            parse_tree(&tree_record(b"100644", b"blob", &[b'0'; 40], b"path"), 40),
            Err(CommitErrorCode::MalformedOutput)
        ));

        let stage = StageEntry {
            mode: b"160000".to_vec(),
            oid: oid.clone(),
            path: b"module".to_vec(),
        };
        let tree = TreeEntry {
            mode: b"160000".to_vec(),
            object_type: b"commit".to_vec(),
            oid: oid.clone(),
            path: b"module".to_vec(),
        };
        assert_eq!(
            cross_check_authority(
                &[],
                std::slice::from_ref(&stage),
                std::slice::from_ref(&tree),
            ),
            Ok(())
        );
        assert_eq!(
            cross_check_authority(&[], std::slice::from_ref(&stage), &[]),
            Err(CommitErrorCode::UnsafeRepository)
        );
        assert_eq!(
            cross_check_authority(&[], &[], std::slice::from_ref(&tree)),
            Err(CommitErrorCode::UnsafeRepository)
        );
        let mut changed_stage = stage.clone();
        changed_stage.oid = vec![b'2'; 40];
        assert_eq!(
            cross_check_authority(&[], &[changed_stage], std::slice::from_ref(&tree)),
            Err(CommitErrorCode::UnsafeRepository)
        );
        let record = StatusRecord {
            shape: StatusShape::Ordinary,
            x: b'.',
            y: b'M',
            sub: b"N...".to_vec(),
            head_mode: b"160000".to_vec(),
            index_mode: b"160000".to_vec(),
            worktree_mode: b"160000".to_vec(),
            head_oid: oid.clone(),
            index_oid: oid,
            path: b"module".to_vec(),
            previous: None,
        };
        assert_eq!(
            cross_check_authority(&[record], &[stage], &[tree]),
            Err(CommitErrorCode::UnsafeRepository)
        );
    }

    #[test]
    fn raw_rename_copy_topology_is_exact_and_fail_closed() {
        let head = test_head(false, 40);
        let source_oid = vec![b'1'; 40];
        let other_oid = vec![b'2'; 40];
        let source_tree = tree_record(b"100644", b"blob", &source_oid, b"source.txt");
        let source_stage = stage_record(b"100644", &source_oid, b"source.txt");
        let destination = |path: &[u8]| stage_record(b"100644", &source_oid, path);
        let authority = |status: Vec<u8>, stage: Vec<u8>, tree: Vec<u8>| {
            finalize_authority(head.clone(), status, stage, tree, 1)
        };

        let mut shared_copy = status_prefix(&head);
        shared_copy.extend_from_slice(&status_rc_record(
            b'C',
            &source_oid,
            &source_oid,
            b"copy-a.txt",
            b"source.txt",
        ));
        shared_copy.extend_from_slice(&status_rc_record(
            b'C',
            &source_oid,
            &source_oid,
            b"copy-b.txt",
            b"source.txt",
        ));
        let mut shared_copy_stage = source_stage.clone();
        shared_copy_stage.extend_from_slice(&destination(b"copy-a.txt"));
        shared_copy_stage.extend_from_slice(&destination(b"copy-b.txt"));
        assert!(authority(shared_copy, shared_copy_stage, source_tree.clone()).is_ok());

        let mut rename = status_prefix(&head);
        rename.extend_from_slice(&status_rc_record(
            b'R',
            &source_oid,
            &source_oid,
            b"renamed.txt",
            b"source.txt",
        ));
        let mut retained_source = source_stage.clone();
        retained_source.extend_from_slice(&destination(b"renamed.txt"));
        assert!(matches!(
            authority(rename.clone(), retained_source, source_tree.clone()),
            Err(CommitErrorCode::MalformedOutput)
        ));

        let mut copy = status_prefix(&head);
        copy.extend_from_slice(&status_rc_record(
            b'C',
            &source_oid,
            &source_oid,
            b"copied.txt",
            b"source.txt",
        ));
        assert!(matches!(
            authority(copy, destination(b"copied.txt"), source_tree.clone()),
            Err(CommitErrorCode::MalformedOutput)
        ));

        let mut destination_exists_tree = source_tree.clone();
        destination_exists_tree.extend_from_slice(&tree_record(
            b"100644",
            b"blob",
            &other_oid,
            b"renamed.txt",
        ));
        assert!(matches!(
            authority(
                rename.clone(),
                destination(b"renamed.txt"),
                destination_exists_tree,
            ),
            Err(CommitErrorCode::MalformedOutput)
        ));

        let mut duplicate_rename = rename;
        duplicate_rename.extend_from_slice(&status_rc_record(
            b'R',
            &source_oid,
            &source_oid,
            b"renamed-again.txt",
            b"source.txt",
        ));
        let mut duplicate_destinations = destination(b"renamed.txt");
        duplicate_destinations.extend_from_slice(&destination(b"renamed-again.txt"));
        assert!(matches!(
            authority(
                duplicate_rename,
                duplicate_destinations,
                source_tree.clone(),
            ),
            Err(CommitErrorCode::MalformedOutput)
        ));

        let mut same_path = status_prefix(&head);
        same_path.extend_from_slice(&status_rc_record(
            b'R',
            &source_oid,
            &source_oid,
            b"source.txt",
            b"source.txt",
        ));
        assert!(matches!(
            authority(same_path, source_stage, source_tree),
            Err(CommitErrorCode::MalformedOutput)
        ));
    }

    #[test]
    fn authority_combined_bytes_and_logical_paths_are_exactly_bounded() {
        let head = test_head(true, 40);
        let prefix = status_prefix(&head);
        let path_len = SNAPSHOT_LIMIT - prefix.len() - b"? \0".len();
        let mut exact = prefix.clone();
        exact.extend_from_slice(b"? ");
        exact.extend(std::iter::repeat_n(b'p', path_len));
        exact.push(0);
        let authority = finalize_authority(head.clone(), exact, Vec::new(), Vec::new(), 1)
            .expect("exact retained authority");
        assert_eq!(authority.status_raw.len(), SNAPSHOT_LIMIT);

        let mut plus_one = prefix.clone();
        plus_one.extend_from_slice(b"? ");
        plus_one.extend(std::iter::repeat_n(b'p', path_len + 1));
        plus_one.push(0);
        assert!(matches!(
            finalize_authority(head.clone(), plus_one, Vec::new(), Vec::new(), 1),
            Err(CommitErrorCode::OutputTooLarge)
        ));

        let build_paths = |count: usize| {
            let mut status = status_prefix(&head);
            for index in 0..count {
                status.extend_from_slice(format!("? path-{index:05}").as_bytes());
                status.push(0);
            }
            status
        };
        let exact_paths = finalize_authority(
            head.clone(),
            build_paths(PATH_LIMIT),
            Vec::new(),
            Vec::new(),
            1,
        )
        .expect("exact logical path authority");
        assert_eq!(
            logical_path_count(&exact_paths.records, &exact_paths.stages, &exact_paths.tree),
            Ok(PATH_LIMIT)
        );
        assert!(matches!(
            finalize_authority(
                head.clone(),
                build_paths(PATH_LIMIT + 1),
                Vec::new(),
                Vec::new(),
                1,
            ),
            Err(CommitErrorCode::OutputTooLarge)
        ));
    }

    #[test]
    fn explicit_filter_values_are_typed_unsafe_filter() {
        let paths = vec![b"tracked.txt".to_vec()];
        for value in [b"set".as_slice(), b"unset", b"unspecified", b"driver"] {
            let mut attrs = b"tracked.txt\0filter\0".to_vec();
            attrs.extend_from_slice(value);
            attrs.push(0);
            let error = validate_filter_attrs(&paths, &attrs).expect_err("explicit filter");
            assert_eq!(error.code(), GitWorkspaceErrorCode::GitFailed);
            let mapped = if error.code() == GitWorkspaceErrorCode::GitFailed {
                CommitErrorCode::UnsafeFilter
            } else {
                map_workspace_error(error)
            };
            assert_eq!(mapped, CommitErrorCode::UnsafeFilter);
        }
    }

    #[tokio::test]
    async fn prepare_maps_every_explicit_filter_value_to_unsafe_filter_before_add() {
        for value in ["set", "unset", "unspecified", "driver"] {
            let repo = Repo::new();
            fs::write(repo.path().join("tracked.txt"), "filter candidate\n")
                .expect("filter candidate");
            let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
            workspace
                .refresh(CancellationToken::new())
                .await
                .expect("filter baseline workspace");
            let read_dir = tempfile::tempdir().expect("filter read fixture");
            let read = read_dir.path().join("git-read.sh");
            fs::write(
                &read,
                format!(
                    "#!/bin/sh\nset -eu\nfor arg in \"$@\"; do if [ \"$arg\" = check-attr ]; then printf 'tracked.txt\\0filter\\0{value}\\0'; exit 0; fi; done\nexec /usr/bin/git \"$@\"\n"
                ),
            )
            .expect("filter read script");
            let mut permissions = fs::metadata(&read)
                .expect("filter read metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&read, permissions).expect("filter read executable");
            let (mutation_dir, mutation, _argv, _input) = mutation_recorder();
            let attempts = mutation_dir.path().join("mutation-attempts");
            let trusted = TrustedGitService::new_with_executables_for_test(
                repo.path(),
                workspace,
                mutation,
                read,
            )
            .expect("filter trusted service");
            let checklist = trusted
                .open_checklist(CancellationToken::new())
                .await
                .expect("filter checklist");
            let completion = trusted
                .prepare(
                    checklist.id,
                    vec![checklist.optional[0].file_id],
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(completion.error, Some(CommitErrorCode::UnsafeFilter));
            assert!(completion.prepared.is_none());
            assert_terminal_workspace(
                &trusted,
                completion
                    .workspace
                    .as_ref()
                    .expect("filter terminal workspace"),
            );
            assert!(!attempts.exists(), "explicit filter spawned add: {value}");
        }
    }

    #[tokio::test]
    async fn selected_current_or_rename_old_gitattributes_is_zero_add_unsafe_filter() {
        let repo = Repo::new();
        fs::write(repo.path().join(".gitattributes"), "# candidate\n")
            .expect("current attributes candidate");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("current attributes workspace");
        let (mutation_dir, mutation, _argv, _input) = mutation_recorder();
        let attempts = mutation_dir.path().join("mutation-attempts");
        let trusted =
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
                .expect("current attributes service");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("current attributes checklist");
        let selected = checklist
            .optional
            .iter()
            .find(|row| row.label == ".gitattributes")
            .expect("current attributes row")
            .file_id;
        let completion = trusted
            .prepare(checklist.id, vec![selected], CancellationToken::new())
            .await;
        assert_eq!(completion.error, Some(CommitErrorCode::UnsafeFilter));
        assert_terminal_workspace(
            &trusted,
            completion
                .workspace
                .as_ref()
                .expect("current attributes terminal"),
        );
        assert!(!attempts.exists());

        let repo = Repo::new();
        fs::write(repo.path().join(".gitattributes"), "# base\n").expect("old attributes base");
        run_git(repo.path(), &["add", ".gitattributes"]);
        run_git(repo.path(), &["commit", "-qm", "attributes base"]);
        run_git(repo.path(), &["mv", ".gitattributes", "attributes.txt"]);
        fs::write(repo.path().join("attributes.txt"), "# base\n# worktree\n")
            .expect("rename destination edit");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("old attributes workspace");
        let (mutation_dir, mutation, _argv, _input) = mutation_recorder();
        let attempts = mutation_dir.path().join("mutation-attempts");
        let trusted =
            TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
                .expect("old attributes service");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("old attributes checklist");
        let selected = checklist
            .optional
            .iter()
            .find(|row| row.previous_label.as_deref() == Some(".gitattributes"))
            .expect("rename old attributes row")
            .file_id;
        let completion = trusted
            .prepare(checklist.id, vec![selected], CancellationToken::new())
            .await;
        assert_eq!(completion.error, Some(CommitErrorCode::UnsafeFilter));
        assert_terminal_workspace(
            &trusted,
            completion
                .workspace
                .as_ref()
                .expect("old attributes terminal"),
        );
        assert!(!attempts.exists());
    }

    #[tokio::test]
    async fn attrs_drift_at_immediate_final_and_post_add_barriers_has_zero_zero_one_add() {
        for drift_call in [2_u8, 3, 4] {
            let repo = Repo::new();
            fs::write(repo.path().join("tracked.txt"), "attrs candidate\n")
                .expect("attrs candidate");
            let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
            workspace
                .refresh(CancellationToken::new())
                .await
                .expect("attrs A workspace");
            let read_dir = tempfile::tempdir().expect("attrs read fixture");
            let read = read_dir.path().join("git-read.sh");
            let count = read_dir.path().join("attr-count");
            let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
            fs::write(
                &read,
                format!(
                    "#!/bin/sh\nset -eu\nfor arg in \"$@\"; do if [ \"$arg\" = check-attr ]; then count=0; [ -e '{count}' ] && count=$(/bin/cat '{count}'); count=$((count + 1)); printf '%s' \"$count\" > '{count}'; if [ \"$count\" -eq {drift_call} ]; then printf 'tracked.txt\\0text\\0set\\0'; fi; exit 0; fi; done\nexec /usr/bin/git \"$@\"\n",
                    count = quote(&count),
                ),
            )
            .expect("attrs read script");
            let mut permissions = fs::metadata(&read)
                .expect("attrs read metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&read, permissions).expect("attrs read executable");
            let (mutation_dir, mutation, _argv, _input) = mutation_recorder();
            let attempts = mutation_dir.path().join("mutation-attempts");
            let trusted = TrustedGitService::new_with_executables_for_test(
                repo.path(),
                workspace,
                mutation,
                read,
            )
            .expect("attrs trusted service");
            let checklist = trusted
                .open_checklist(CancellationToken::new())
                .await
                .expect("attrs checklist");
            let completion = trusted
                .prepare(
                    checklist.id,
                    vec![checklist.optional[0].file_id],
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(
                completion.error,
                Some(CommitErrorCode::ChangedDuringRead),
                "attrs barrier {drift_call}"
            );
            assert!(completion.prepared.is_none());
            let terminal = completion
                .workspace
                .as_ref()
                .expect("attrs terminal workspace");
            assert_terminal_workspace(&trusted, terminal);
            if drift_call == 4 {
                assert_eq!(fs::read(&attempts).expect("post-add attempt"), b"x");
            } else {
                assert!(!attempts.exists(), "pre-add attrs drift spawned add");
            }
        }
    }

    #[tokio::test]
    async fn real_gitlink_is_allowed_only_as_exact_clean_unchanged_union_entry() {
        fn install_gitlink(repo: &Repo) -> Vec<u8> {
            let target = run_git_output(repo.path(), &["rev-parse", "HEAD"])
                .strip_suffix(b"\n")
                .expect("gitlink target newline")
                .to_vec();
            let target_text = std::str::from_utf8(&target).expect("fixture oid");
            let cache = format!("160000,{target_text},module");
            run_git(
                repo.path(),
                &["update-index", "--add", "--cacheinfo", &cache],
            );
            run_git(repo.path(), &["commit", "-qm", "add gitlink"]);
            let module = repo.path().join("module");
            let mut clone = Command::new(GIT);
            clone
                .current_dir(repo.path())
                .args(["clone", "-q"])
                .arg(repo.path())
                .arg(&module);
            scrub_git_environment(&mut clone);
            assert!(clone.status().expect("clone gitlink worktree").success());
            run_git(&module, &["checkout", "-q", target_text]);
            target
        }

        let unchanged = Repo::new();
        install_gitlink(&unchanged);
        fs::write(unchanged.path().join("tracked.txt"), "ordinary change\n")
            .expect("ordinary alongside gitlink");
        let (_workspace, trusted) = unchanged.services().await;
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("unchanged gitlink checklist");
        assert_eq!(checklist.optional.len(), 1);
        assert!(!checklist.optional[0].label.contains("module"));
        let prepared = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await
            .prepared
            .expect("unchanged gitlink prepared");
        assert_eq!(prepared.staged_file_count, 2);

        let deleted = Repo::new();
        install_gitlink(&deleted);
        let (workspace, _clean_service) = deleted.services().await;
        run_git(
            deleted.path(),
            &["update-index", "--force-remove", "module"],
        );
        fs::remove_dir_all(deleted.path().join("module")).expect("remove deleted gitlink worktree");
        let trusted = TrustedGitService::new(deleted.path(), workspace).expect("deleted service");
        assert_eq!(
            trusted.open_checklist(CancellationToken::new()).await,
            Err(CommitErrorCode::UnsafeRepository)
        );

        let updated = Repo::new();
        install_gitlink(&updated);
        let (workspace, _clean_service) = updated.services().await;
        let other = run_git_output(updated.path(), &["rev-parse", "HEAD"]);
        let other = std::str::from_utf8(other.strip_suffix(b"\n").expect("updated oid newline"))
            .expect("updated oid");
        let cache = format!("160000,{other},module");
        run_git(
            updated.path(),
            &["update-index", "--add", "--cacheinfo", &cache],
        );
        let trusted = TrustedGitService::new(updated.path(), workspace).expect("updated service");
        assert_eq!(
            trusted.open_checklist(CancellationToken::new()).await,
            Err(CommitErrorCode::UnsafeRepository)
        );

        for mode in ["100644", "120000"] {
            let changed = Repo::new();
            install_gitlink(&changed);
            let (workspace, _clean_service) = changed.services().await;
            let blob = run_git_output(changed.path(), &["rev-parse", "HEAD:tracked.txt"]);
            let blob = std::str::from_utf8(blob.strip_suffix(b"\n").expect("blob newline"))
                .expect("blob oid");
            let cache = format!("{mode},{blob},module");
            fs::remove_dir_all(changed.path().join("module"))
                .expect("remove type-changed gitlink worktree");
            run_git(
                changed.path(),
                &["update-index", "--add", "--cacheinfo", &cache],
            );
            let trusted =
                TrustedGitService::new(changed.path(), workspace).expect("type change service");
            assert_eq!(
                trusted.open_checklist(CancellationToken::new()).await,
                Err(CommitErrorCode::UnsafeRepository),
                "gitlink type change mode {mode}"
            );
        }

        let unborn = Repo::new();
        let target = install_gitlink(&unborn);
        let (workspace, _clean_service) = unborn.services().await;
        let target = std::str::from_utf8(&target).expect("unborn target oid");
        run_git(
            unborn.path(),
            &["symbolic-ref", "HEAD", "refs/heads/unborn"],
        );
        run_git(unborn.path(), &["read-tree", "--empty"]);
        fs::remove_dir_all(unborn.path().join("module")).expect("remove unborn gitlink worktree");
        let cache = format!("160000,{target},module");
        run_git(
            unborn.path(),
            &["update-index", "--add", "--cacheinfo", &cache],
        );
        let trusted = TrustedGitService::new(unborn.path(), workspace).expect("unborn service");
        assert_eq!(
            trusted.open_checklist(CancellationToken::new()).await,
            Err(CommitErrorCode::UnsafeRepository)
        );
    }

    #[tokio::test]
    async fn empty_blob_add_worktree_delete_and_staged_empty_delete_remain_distinct() {
        let added = Repo::new();
        fs::write(added.path().join("empty.txt"), b"").expect("empty add");
        run_git(added.path(), &["add", "empty.txt"]);
        let (_workspace, trusted) = added.services().await;
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("empty add checklist");
        assert_eq!(checklist.staged.len(), 1);
        assert!(checklist.optional.is_empty());
        let prepared = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await
            .prepared
            .expect("empty add prepared");
        assert_eq!(
            trusted
                .commit(
                    prepared.id,
                    "test: add empty blob".into(),
                    CancellationToken::new(),
                )
                .await
                .outcome,
            CommitOutcome::Committed
        );

        let deleted = Repo::new();
        fs::write(deleted.path().join("empty.txt"), b"").expect("tracked empty");
        run_git(deleted.path(), &["add", "empty.txt"]);
        run_git(deleted.path(), &["commit", "-qm", "empty base"]);
        fs::remove_file(deleted.path().join("empty.txt")).expect("worktree empty delete");
        let (_workspace, trusted) = deleted.services().await;
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("worktree empty delete checklist");
        assert!(checklist.staged.is_empty());
        assert_eq!(checklist.optional.len(), 1);
        assert_eq!(checklist.optional[0].kind, CommitSelectionKind::Deleted);

        for select_delete in [false, true] {
            let repo = Repo::new();
            fs::write(repo.path().join("tracked.txt"), b"").expect("stage empty blob");
            run_git(repo.path(), &["add", "tracked.txt"]);
            fs::remove_file(repo.path().join("tracked.txt")).expect("delete after staged empty");
            let (_workspace, trusted) = repo.services().await;
            let checklist = trusted
                .open_checklist(CancellationToken::new())
                .await
                .expect("staged empty plus delete checklist");
            assert_eq!(checklist.staged.len(), 1);
            assert_eq!(checklist.staged[0].kind, CommitSelectionKind::Modified);
            assert_eq!(checklist.optional.len(), 1);
            assert_eq!(checklist.optional[0].kind, CommitSelectionKind::Deleted);
            let selected = if select_delete {
                vec![checklist.optional[0].file_id]
            } else {
                Vec::new()
            };
            let completion = trusted
                .prepare(checklist.id, selected, CancellationToken::new())
                .await;
            let prepared = completion.prepared.expect("staged-empty prepared");
            let committed = trusted
                .commit(
                    prepared.id,
                    format!("test: staged empty delete selected={select_delete}"),
                    CancellationToken::new(),
                )
                .await;
            assert_eq!(committed.outcome, CommitOutcome::Committed);
            let indexed = run_git_output(repo.path(), &["ls-files", "--stage", "tracked.txt"]);
            assert_eq!(indexed.is_empty(), select_delete);
        }
    }

    #[test]
    fn logical_rename_sides_and_raw_path_codec_are_closed() {
        let synthetic = |index: usize| StatusRecord {
            shape: StatusShape::Rename,
            x: b'R',
            y: b'.',
            sub: b"N...".to_vec(),
            head_mode: b"100644".to_vec(),
            index_mode: b"100644".to_vec(),
            worktree_mode: b"100644".to_vec(),
            head_oid: vec![b'1'; 40],
            index_oid: vec![b'2'; 40],
            path: format!("new-{index:05}").into_bytes(),
            previous: Some(format!("old-{index:05}").into_bytes()),
        };
        let exact: Vec<_> = (0..PATH_LIMIT / 2).map(synthetic).collect();
        assert_eq!(logical_path_count(&exact, &[], &[]), Ok(PATH_LIMIT));
        let plus_one: Vec<_> = (0..=PATH_LIMIT / 2).map(synthetic).collect();
        assert!(logical_path_count(&plus_one, &[], &[]).expect("logical count") > PATH_LIMIT);

        for unsafe_path in [
            b"".as_slice(),
            b"/absolute",
            b"a//b",
            b"a/./b",
            b"a/../b",
            b".git/config",
            b"a/.git/config",
        ] {
            assert!(validate_relative_path(unsafe_path).is_err());
        }
        let head = test_head(true, 40);
        let mut status = status_prefix(&head);
        for path in [
            b"space name".as_slice(),
            b"tab\tname",
            b"line\nname",
            b"-leading",
            b"\xff-nonutf8",
        ] {
            status.extend_from_slice(b"? ");
            status.extend_from_slice(path);
            status.push(0);
        }
        let authority = finalize_authority(head, status, Vec::new(), Vec::new(), 1)
            .expect("awkward raw authority");
        assert!(
            authority
                .records
                .windows(2)
                .all(|pair| pair[0].path < pair[1].path)
        );
        assert!(
            authority
                .records
                .iter()
                .any(|record| record.path == b"\xff-nonutf8")
        );
    }

    #[test]
    fn unborn_three_source_add_and_conflict_table_is_fail_closed() {
        let head = test_head(true, 40);
        let blob = vec![b'1'; 40];
        let mut status = status_prefix(&head);
        status.extend_from_slice(b"1 A. N... 000000 100644 100644 ");
        status.extend(std::iter::repeat_n(b'0', 40));
        status.push(b' ');
        status.extend_from_slice(&blob);
        status.extend_from_slice(b" added.txt\0");
        let stage = stage_record(b"100644", &blob, b"added.txt");
        let authority =
            finalize_authority(head.clone(), status.clone(), stage.clone(), Vec::new(), 1)
                .expect("canonical unborn add");
        assert_eq!(authority.records.len(), 1);
        assert_eq!(authority.stages.len(), 1);
        assert!(authority.tree.is_empty());

        assert!(matches!(
            finalize_authority(head.clone(), status.clone(), Vec::new(), Vec::new(), 1),
            Err(CommitErrorCode::MalformedOutput)
        ));
        assert!(matches!(
            finalize_authority(head.clone(), status_prefix(&head), stage, Vec::new(), 1,),
            Err(CommitErrorCode::MalformedOutput)
        ));
        assert!(matches!(
            finalize_authority(
                head,
                status,
                stage_record(b"100755", &blob, b"added.txt"),
                tree_record(b"100644", b"blob", &blob, b"fabricated.txt"),
                1,
            ),
            Err(CommitErrorCode::MalformedOutput)
        ));
    }
}
