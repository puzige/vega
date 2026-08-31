use super::*;

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
    pub(crate) fn new_with_mutation_for_test(
        root: impl AsRef<Path>,
        workspace: Arc<GitWorkspaceService>,
        executable: PathBuf,
    ) -> Result<Self, CommitErrorCode> {
        let mut service = Self::new(root, workspace)?;
        service.mutation_executable = Some(executable);
        Ok(service)
    }

    #[cfg(test)]
    pub(crate) fn new_with_mutation_timeout_for_test(
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
    pub(crate) fn new_with_executables_for_test(
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
