//! Bounded, headless local-branch enumeration and trusted switch service.
//!
//! Raw refs, object ids, changed paths, Git output and repository paths are
//! confined to this module. Public projections contain only escaped labels
//! and opaque generation-bound identifiers.

use super::*;
use crate::types::{
    BranchId, BranchItem, BranchSnapshot, BranchSwitchCompletion, BranchSwitchOutcome,
};

const BRANCH_LIMIT: usize = 10_000;
const BRANCH_RETAINED_LIMIT: usize = 8 * 1024 * 1024;
const OPERATION_MARKERS: &[&str] = &[
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "BISECT_START",
    "BISECT_LOG",
    "rebase-merge",
    "rebase-apply",
    "sequencer",
];

#[derive(Clone, PartialEq, Eq)]
struct PrivateBranch {
    id: BranchId,
    short: OsString,
    full: Vec<u8>,
    oid: Vec<u8>,
    current: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct BranchIdentity {
    filter_paths: Arc<[u8]>,
    filter_attrs: Vec<u8>,
    status: Vec<u8>,
    refs: Vec<u8>,
    branches: Vec<PrivateBranch>,
}

#[derive(PartialEq, Eq)]
struct SwitchAuthority {
    acmrt_raw: Vec<u8>,
    delete_raw: Vec<u8>,
    materialized_paths: Vec<Vec<u8>>,
    authority_paths: Vec<Vec<u8>>,
    attrs: Vec<u8>,
}

struct ParsedTargetPaths {
    materialized: Vec<Vec<u8>>,
    authority: Vec<Vec<u8>>,
}

#[derive(Default)]
struct BranchState {
    next_request: u64,
    latest_request: u64,
    next_generation: u64,
    generation: u64,
    identity: Option<Arc<BranchIdentity>>,
    snapshot: Option<BranchSnapshot>,
    branches: Vec<PrivateBranch>,
    next_permit: u64,
    issued_permits: BTreeSet<u64>,
    active_mutation: Option<u64>,
}

/// Single-use, service-bound capability produced by a successful preflight.
/// It deliberately has no `Clone` implementation and its debug output is
/// redacted.
pub struct BranchSwitchPermit {
    service_nonce: u64,
    permit_sequence: u64,
    target_id: BranchId,
    target_short: OsString,
    target_oid: Vec<u8>,
    preflight: BranchIdentity,
    authority: SwitchAuthority,
}

impl std::fmt::Debug for BranchSwitchPermit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BranchSwitchPermit([opaque])")
    }
}

/// Headless, ephemeral service for local branch listing and guarded switching.
pub struct BranchWorkspaceService {
    root: PathBuf,
    root_identity: RootIdentity,
    instance_nonce: u64,
    state: Arc<Mutex<BranchState>>,
    #[cfg(test)]
    executable: Option<PathBuf>,
    #[cfg(test)]
    mutation_executable: Option<PathBuf>,
}

impl std::fmt::Debug for BranchWorkspaceService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let generation = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .generation;
        formatter
            .debug_struct("BranchWorkspaceService")
            .field("root", &"[redacted]")
            .field("generation", &generation)
            .finish()
    }
}

impl BranchWorkspaceService {
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
            root_identity: RootIdentity {
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
            instance_nonce,
            state: Arc::new(Mutex::new(BranchState::default())),
            #[cfg(test)]
            executable,
            #[cfg(test)]
            mutation_executable: None,
        })
    }

    /// Refreshes the complete local branch list. Byte-identical state keeps
    /// its generation and opaque ids; newer requests always win.
    pub async fn refresh(
        &self,
        cancel: CancellationToken,
    ) -> Result<BranchSnapshot, GitWorkspaceError> {
        let request = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if state.active_mutation.is_some() {
                return Err(error(GitWorkspaceErrorCode::StaleGeneration));
            }
            let request = state
                .next_request
                .checked_add(1)
                .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
            state.next_request = request;
            state.latest_request = request;
            request
        };
        let result = self.capture(cancel).await;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active_mutation.is_some() {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        if state.latest_request != request {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        let identity = match result {
            Ok(identity) => identity,
            Err(failure) => {
                invalidate_branch_state(&mut state);
                return Err(failure);
            }
        };
        commit_branch_identity(
            &mut state,
            identity,
            self.root_identity,
            self.instance_nonce,
        )
    }

    /// Produces a single-use switch capability only after two byte-exact,
    /// clean preflight captures and target-tree filter validation.
    pub async fn prepare_switch(
        &self,
        branch_id: BranchId,
        cancel: CancellationToken,
    ) -> Result<BranchSwitchPermit, GitWorkspaceError> {
        let target = self.current_branch(branch_id)?;
        if target.current {
            return Err(error(GitWorkspaceErrorCode::BranchAlreadyCurrent));
        }
        let first = self.capture(cancel.clone()).await?;
        verify_target_matches(&first, &target)?;
        let root = self.root.clone();
        let root_identity = self.root_identity;
        let target_oid = target.oid.clone();
        let current_oid = current_oid(&first)?.to_vec();
        let preflight_retained = branch_identity_retained(&first)?;
        let permit_payload = target
            .short
            .as_bytes()
            .len()
            .checked_add(target.oid.len())
            .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        #[cfg(test)]
        let executable = self.executable.clone();
        let cancel_for_check = cancel.clone();
        let authority = tokio::task::spawn_blocking(move || {
            let runner = Runner::new(
                root,
                root_identity,
                #[cfg(test)]
                executable,
            );
            validate_target_changes(
                &runner,
                &current_oid,
                &target_oid,
                preflight_retained,
                permit_payload,
                &cancel_for_check,
            )
        })
        .await
        .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))??;
        let second = self.capture(cancel).await?;
        if first != second {
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
        let permit_sequence = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let current = state
                .branches
                .get(
                    usize::try_from(branch_id.slot)
                        .map_err(|_| error(GitWorkspaceErrorCode::UnknownFile))?,
                )
                .filter(|branch| branch.id == branch_id && branch.oid == target.oid)
                .ok_or_else(|| error(GitWorkspaceErrorCode::StaleGeneration))?;
            if state.generation != branch_id.generation || current.current {
                return Err(error(GitWorkspaceErrorCode::StaleGeneration));
            }
            let sequence = state
                .next_permit
                .checked_add(1)
                .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
            state.next_permit = sequence;
            // A newer explicit selection invalidates any older unconsumed
            // capability. This bounds retained leases and prevents two
            // concurrent mutations even when both preflights succeeded.
            state.issued_permits.clear();
            state.issued_permits.insert(sequence);
            sequence
        };
        Ok(BranchSwitchPermit {
            service_nonce: self.instance_nonce,
            permit_sequence,
            target_id: branch_id,
            target_short: target.short,
            target_oid: target.oid,
            preflight: second,
            authority,
        })
    }

    /// Executes exactly one trusted `git switch` and then refreshes
    /// authoritatively on every exit, including cancellation and failure.
    pub async fn execute_switch(
        &self,
        permit: BranchSwitchPermit,
        cancel: CancellationToken,
    ) -> BranchSwitchCompletion {
        let expected_target = permit.target_short.clone();
        let expected_oid = permit.target_oid.clone();
        let permit_sequence = permit.permit_sequence;
        if let Err(failure) = self.consume_permit(&permit) {
            return BranchSwitchCompletion {
                outcome: BranchSwitchOutcome::Failed(failure.code()),
                snapshot: None,
            };
        }
        let mutation = {
            let before = self.capture(cancel.clone()).await;
            match before {
                Ok(identity) if identity == permit.preflight => {
                    let root = self.root.clone();
                    let root_identity = self.root_identity;
                    let branch = permit.target_short.clone();
                    let current_oid = current_oid(&identity).map(<[u8]>::to_vec);
                    let target_oid = permit.target_oid.clone();
                    #[cfg(test)]
                    let executable = self.executable.clone();
                    #[cfg(test)]
                    let mutation_executable = self.mutation_executable.clone();
                    let mutation_cancel = cancel.clone();
                    match current_oid {
                        Err(failure) => Err(failure),
                        Ok(current_oid) => tokio::task::spawn_blocking(move || {
                            let runner = Runner::new(
                                root,
                                root_identity,
                                #[cfg(test)]
                                executable,
                            );
                            let authority = validate_target_changes(
                                &runner,
                                &current_oid,
                                &target_oid,
                                branch_identity_retained(&identity)?,
                                branch
                                    .as_bytes()
                                    .len()
                                    .checked_add(target_oid.len())
                                    .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?,
                                &mutation_cancel,
                            )?;
                            if authority != permit.authority {
                                return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
                            }
                            #[cfg(test)]
                            if let Some(executable) = mutation_executable {
                                return runner
                                    .run_trusted_switch_with_executable(
                                        &branch,
                                        &mutation_cancel,
                                        &executable,
                                    )
                                    .map(|_| ());
                            }
                            runner
                                .run_trusted_switch(&branch, &mutation_cancel)
                                .map(|_| ())
                        })
                        .await
                        .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))
                        .and_then(|result| result),
                    }
                }
                Ok(_) => Err(error(GitWorkspaceErrorCode::ChangedDuringRead)),
                Err(failure) => Err(failure),
            }
        };

        // Cleanup refresh deliberately ignores the caller's cancelled token.
        let refreshed = self.owner_refresh(permit_sequence).await;
        let target_is_current = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .branches
            .iter()
            .any(|branch| {
                branch.current && branch.short == expected_target && branch.oid == expected_oid
            });
        let outcome = match (mutation, &refreshed) {
            (Ok(()), Ok(_)) if target_is_current => BranchSwitchOutcome::Switched,
            (Ok(()), _) => BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::ChangedDuringRead),
            (Err(failure), _) => BranchSwitchOutcome::Failed(failure.code()),
        };
        release_mutation(
            &mut self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
            permit_sequence,
        );
        BranchSwitchCompletion {
            outcome,
            snapshot: refreshed.ok(),
        }
    }

    fn consume_permit(&self, permit: &BranchSwitchPermit) -> Result<(), GitWorkspaceError> {
        if permit.service_nonce != self.instance_nonce || permit.permit_sequence == 0 {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if !state.issued_permits.remove(&permit.permit_sequence) || state.active_mutation.is_some()
        {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        if state.generation != permit.target_id.generation {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        let slot = usize::try_from(permit.target_id.slot)
            .map_err(|_| error(GitWorkspaceErrorCode::UnknownFile))?;
        let target = state
            .branches
            .get(slot)
            .filter(|branch| branch.id == permit.target_id)
            .ok_or_else(|| error(GitWorkspaceErrorCode::UnknownFile))?;
        if target.current || target.oid != permit.target_oid || target.short != permit.target_short
        {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        state.active_mutation = Some(permit.permit_sequence);
        Ok(())
    }

    fn current_branch(&self, id: BranchId) -> Result<PrivateBranch, GitWorkspaceError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.generation != id.generation {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        state
            .branches
            .get(usize::try_from(id.slot).map_err(|_| error(GitWorkspaceErrorCode::UnknownFile))?)
            .filter(|branch| branch.id == id)
            .cloned()
            .ok_or_else(|| error(GitWorkspaceErrorCode::UnknownFile))
    }

    async fn capture(
        &self,
        cancel: CancellationToken,
    ) -> Result<BranchIdentity, GitWorkspaceError> {
        let root = self.root.clone();
        let identity = self.root_identity;
        #[cfg(test)]
        let executable = self.executable.clone();
        tokio::task::spawn_blocking(move || {
            build_branch_identity(
                &Runner::new(
                    root,
                    identity,
                    #[cfg(test)]
                    executable,
                ),
                &cancel,
            )
        })
        .await
        .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))?
    }

    async fn owner_refresh(
        &self,
        owner_sequence: u64,
    ) -> Result<BranchSnapshot, GitWorkspaceError> {
        let result = self.capture(CancellationToken::new()).await;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active_mutation != Some(owner_sequence) {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        let identity = match result {
            Ok(identity) => identity,
            Err(failure) => {
                invalidate_branch_state(&mut state);
                return Err(failure);
            }
        };
        commit_branch_identity(
            &mut state,
            identity,
            self.root_identity,
            self.instance_nonce,
        )
    }

    #[cfg(test)]
    fn new_with_mutation_for_test(
        root: &Path,
        mutation_executable: PathBuf,
    ) -> Result<Self, GitWorkspaceError> {
        let mut service = Self::new_inner(root, None)?;
        service.mutation_executable = Some(mutation_executable);
        Ok(service)
    }

    #[cfg(test)]
    fn new_with_executables_for_test(
        root: &Path,
        executable: PathBuf,
        mutation_executable: PathBuf,
    ) -> Result<Self, GitWorkspaceError> {
        let mut service = Self::new_inner(root, Some(executable))?;
        service.mutation_executable = Some(mutation_executable);
        Ok(service)
    }
}

fn commit_branch_identity(
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

fn build_branch_identity(
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

fn capture_branch_filter_identity(
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

fn parse_clean_head(
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

fn parse_refs(
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

fn validate_branch_short(short: &[u8]) -> Result<(), GitWorkspaceError> {
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

fn valid_oid_width(oid: &[u8], width: usize) -> bool {
    oid.len() == width
        && matches!(width, 40 | 64)
        && oid
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_oid(oid: &[u8]) -> bool {
    matches!(oid.len(), 40 | 64)
        && oid
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn reject_operation_markers(
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

fn canonical_git_dir(
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

fn validate_target_changes(
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

fn charge_paths(budget: &mut RetainedBudget, paths: &[Vec<u8>]) -> Result<(), GitWorkspaceError> {
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

fn exact_single_line(bytes: &[u8]) -> Result<&[u8], GitWorkspaceError> {
    let line = bytes
        .strip_suffix(b"\n")
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    if line.is_empty() || line.contains(&0) || line.contains(&b'\n') || line.contains(&b'\r') {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok(line)
}

fn parse_target_paths(bytes: &[u8]) -> Result<ParsedTargetPaths, GitWorkspaceError> {
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

fn parse_acmrt_status(status: &[u8]) -> Result<u8, GitWorkspaceError> {
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

fn parse_delete_paths(bytes: &[u8]) -> Result<Vec<Vec<u8>>, GitWorkspaceError> {
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

fn is_gitattributes(path: &[u8]) -> bool {
    path.rsplit(|byte| *byte == b'/').next() == Some(b".gitattributes")
}

fn validate_branch_attrs(paths: &[Vec<u8>], bytes: &[u8]) -> Result<(), GitWorkspaceError> {
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

fn current_oid(identity: &BranchIdentity) -> Result<&[u8], GitWorkspaceError> {
    identity
        .branches
        .iter()
        .find(|branch| branch.current)
        .map(|branch| branch.oid.as_slice())
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))
}

fn verify_target_matches(
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

fn same_branch_identity(left: &BranchIdentity, right: &BranchIdentity) -> bool {
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

fn assign_branch_ids(
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

fn project_snapshot(generation: u64, branches: &[PrivateBranch]) -> BranchSnapshot {
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

fn ensure_retained(identity: &BranchIdentity) -> Result<(), GitWorkspaceError> {
    ensure_retained_parts(
        &identity.filter_paths,
        &identity.filter_attrs,
        &identity.status,
        &identity.refs,
        &identity.branches,
    )
}

fn branch_identity_retained(identity: &BranchIdentity) -> Result<usize, GitWorkspaceError> {
    retained_size(
        &identity.filter_paths,
        &identity.filter_attrs,
        &identity.status,
        &identity.refs,
        &identity.branches,
    )
}

fn ensure_retained_parts(
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

fn retained_size(
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

fn invalidate_branch_state(state: &mut BranchState) {
    state.generation = 0;
    state.identity = None;
    state.snapshot = None;
    state.branches.clear();
    state.issued_permits.clear();
}

#[cfg(test)]
fn acquire_mutation(state: &mut BranchState, sequence: u64) -> bool {
    if !state.issued_permits.remove(&sequence) || state.active_mutation.is_some() {
        return false;
    }
    state.active_mutation = Some(sequence);
    true
}

fn release_mutation(state: &mut BranchState, sequence: u64) {
    if state.active_mutation == Some(sequence) {
        state.active_mutation = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Repo(tempfile::TempDir);

    impl Repo {
        fn new() -> Self {
            let directory = tempfile::Builder::new()
                .prefix("vega-branch-")
                .tempdir()
                .expect("create temp repo");
            git(directory.path(), &["init", "-q", "-b", "main"]);
            git(
                directory.path(),
                &["config", "user.email", "vega@example.invalid"],
            );
            git(directory.path(), &["config", "user.name", "Vega Test"]);
            fs::write(directory.path().join("README.md"), "main\n").expect("fixture write");
            git(directory.path(), &["add", "README.md"]);
            git(directory.path(), &["commit", "-q", "-m", "initial"]);
            Self(directory)
        }

        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let mut command = Command::new(GIT);
        command.current_dir(root).args(args);
        scrub_git_environment(&mut command);
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        assert!(
            command.status().expect("git fixture").success(),
            "git {args:?}"
        );
    }

    fn git_output(root: &Path, args: &[&str]) -> Vec<u8> {
        let mut command = Command::new(GIT);
        command.current_dir(root).args(args);
        scrub_git_environment(&mut command);
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        let output = command.output().expect("git fixture output");
        assert!(output.status.success(), "git {args:?}");
        output.stdout
    }

    fn fake_runner(repo: &Repo, name: &str, body: &str) -> Runner {
        let script = repo.path().join(name);
        fs::write(&script, format!("#!/bin/sh\n{body}\n")).expect("fake git script");
        let mut permissions = fs::metadata(&script).expect("metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).expect("chmod");
        let root = fs::canonicalize(repo.path()).expect("canonical root");
        let metadata = fs::metadata(&root).expect("root metadata");
        Runner::new(
            root,
            RootIdentity {
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
            Some(script),
        )
    }

    fn branch_id(snapshot: &BranchSnapshot, label: &str) -> BranchId {
        snapshot
            .branches
            .iter()
            .find(|branch| branch.label == label)
            .expect("fixture branch")
            .id
    }

    fn error_code<T>(result: Result<T, GitWorkspaceError>) -> GitWorkspaceErrorCode {
        match result {
            Ok(_) => panic!("expected failure"),
            Err(failure) => failure.code(),
        }
    }

    #[tokio::test]
    async fn unchanged_refresh_keeps_ids_and_branch_change_rotates() {
        let repo = Repo::new();
        git(repo.path(), &["branch", "topic"]);
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        let first = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let second = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        assert_eq!(first, second);
        git(repo.path(), &["branch", "another"]);
        let third = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        assert_ne!(third.generation, first.generation);
        assert_ne!(third.branches[0].id, first.branches[0].id);
    }

    #[tokio::test]
    async fn opaque_ids_are_service_generation_slot_and_seal_bound() {
        let repo = Repo::new();
        git(repo.path(), &["branch", "topic"]);
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let topic = branch_id(&snapshot, "topic");
        let current = snapshot
            .branches
            .iter()
            .find(|branch| branch.current)
            .expect("current")
            .id;
        assert_eq!(
            service
                .prepare_switch(current, CancellationToken::new())
                .await
                .expect_err("already current")
                .code(),
            GitWorkspaceErrorCode::BranchAlreadyCurrent
        );
        for forged in [
            BranchId {
                generation: topic.generation,
                slot: u32::MAX,
                seal: topic.seal,
            },
            BranchId {
                generation: topic.generation,
                slot: topic.slot,
                seal: topic.seal ^ 1,
            },
        ] {
            assert!(
                service
                    .prepare_switch(forged, CancellationToken::new())
                    .await
                    .is_err()
            );
        }

        let permit = service
            .prepare_switch(topic, CancellationToken::new())
            .await
            .expect("permit");
        let other = BranchWorkspaceService::new(repo.path()).expect("other service");
        other
            .refresh(CancellationToken::new())
            .await
            .expect("other refresh");
        let rejected = other.execute_switch(permit, CancellationToken::new()).await;
        assert_eq!(
            rejected.outcome,
            BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::StaleGeneration)
        );
        assert!(rejected.snapshot.is_none());

        git(repo.path(), &["branch", "temporary"]);
        let changed = service
            .refresh(CancellationToken::new())
            .await
            .expect("changed");
        git(repo.path(), &["branch", "-D", "temporary"]);
        let aba = service
            .refresh(CancellationToken::new())
            .await
            .expect("aba");
        assert_ne!(changed.generation, snapshot.generation);
        assert_ne!(aba.generation, snapshot.generation);
        assert!(
            service
                .prepare_switch(topic, CancellationToken::new())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn stale_permit_after_generation_rotation_does_not_leak_mutation_lease() {
        let repo = Repo::new();
        git(repo.path(), &["switch", "-q", "-c", "topic"]);
        fs::write(repo.path().join("topic.txt"), "topic\n").expect("topic file");
        git(repo.path(), &["add", "topic.txt"]);
        git(repo.path(), &["commit", "-q", "-m", "topic"]);
        git(repo.path(), &["switch", "-q", "main"]);
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let permit = service
            .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
            .await
            .expect("permit");
        git(repo.path(), &["branch", "-f", "topic", "main"]);
        let rotated = service
            .refresh(CancellationToken::new())
            .await
            .expect("rotated refresh");
        assert_ne!(rotated.generation, snapshot.generation);

        let stale = service
            .execute_switch(permit, CancellationToken::new())
            .await;
        assert_eq!(
            stale.outcome,
            BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::StaleGeneration)
        );
        assert!(stale.snapshot.is_none());
        assert!(
            service
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .active_mutation
                .is_none()
        );
        assert_eq!(
            git_output(repo.path(), &["branch", "--show-current"]),
            b"main\n"
        );

        let current = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh remains available");
        let fresh = service
            .prepare_switch(branch_id(&current, "topic"), CancellationToken::new())
            .await
            .expect("fresh permit");
        let completion = service
            .execute_switch(fresh, CancellationToken::new())
            .await;
        assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
    }

    #[tokio::test]
    async fn shared_oid_refs_are_distinct_and_current_is_selected_by_raw_ref() {
        let repo = Repo::new();
        git(repo.path(), &["branch", "alias-a"]);
        git(repo.path(), &["branch", "alias-b"]);
        let snapshot = BranchWorkspaceService::new(repo.path())
            .expect("service")
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        assert_eq!(snapshot.branches.len(), 3);
        assert_eq!(
            snapshot
                .branches
                .iter()
                .filter(|branch| branch.current)
                .map(|branch| branch.label.as_str())
                .collect::<Vec<_>>(),
            vec!["main"]
        );
    }

    #[tokio::test]
    async fn dirty_detached_and_operation_state_fail_closed() {
        let repo = Repo::new();
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        fs::write(repo.path().join("README.md"), "dirty\n").expect("dirty");
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .expect_err("dirty")
                .code(),
            GitWorkspaceErrorCode::BranchDirty
        );
        git(repo.path(), &["restore", "README.md"]);
        git(repo.path(), &["checkout", "--detach", "-q"]);
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .expect_err("detached")
                .code(),
            GitWorkspaceErrorCode::BranchDetached
        );
        git(repo.path(), &["switch", "main", "-q"]);
        fs::write(repo.path().join(".git/MERGE_HEAD"), "fixture").expect("marker");
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .expect_err("operation")
                .code(),
            GitWorkspaceErrorCode::BranchOperationInProgress
        );
    }

    #[tokio::test]
    async fn staged_and_untracked_states_are_dirty_and_every_marker_is_rejected() {
        let repo = Repo::new();
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged");
        git(repo.path(), &["add", "staged.txt"]);
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .expect_err("staged dirty")
                .code(),
            GitWorkspaceErrorCode::BranchDirty
        );
        git(repo.path(), &["reset", "-q", "--", "staged.txt"]);
        fs::remove_file(repo.path().join("staged.txt")).expect("remove staged");
        fs::write(repo.path().join("untracked.txt"), "untracked\n").expect("untracked");
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .expect_err("untracked dirty")
                .code(),
            GitWorkspaceErrorCode::BranchDirty
        );
        fs::remove_file(repo.path().join("untracked.txt")).expect("remove untracked");
        for marker in OPERATION_MARKERS {
            let path = repo.path().join(".git").join(marker);
            if marker.contains('-') || *marker == "sequencer" {
                fs::create_dir(&path).expect("marker directory");
            } else {
                fs::write(&path, "marker\n").expect("marker file");
            }
            assert_eq!(
                service
                    .refresh(CancellationToken::new())
                    .await
                    .expect_err("operation")
                    .code(),
                GitWorkspaceErrorCode::BranchOperationInProgress,
                "marker {marker}"
            );
            if path.is_dir() {
                fs::remove_dir(&path).expect("remove marker dir");
            } else {
                fs::remove_file(&path).expect("remove marker file");
            }
        }
    }

    #[tokio::test]
    async fn unmerged_index_is_dirty_and_never_enumerated_as_switchable() {
        let repo = Repo::new();
        git(repo.path(), &["switch", "-q", "-c", "side"]);
        fs::write(repo.path().join("README.md"), "side\n").expect("side");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-q", "-m", "side"]);
        git(repo.path(), &["switch", "-q", "main"]);
        fs::write(repo.path().join("README.md"), "main changed\n").expect("main");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-q", "-m", "main"]);
        let mut merge = Command::new(GIT);
        merge.current_dir(repo.path()).args(["merge", "side"]);
        scrub_git_environment(&mut merge);
        merge
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        assert!(!merge.status().expect("conflicting merge").success());
        fs::remove_file(repo.path().join(".git/MERGE_HEAD")).expect("remove operation marker");
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .expect_err("unmerged")
                .code(),
            GitWorkspaceErrorCode::BranchDirty
        );
    }

    #[tokio::test]
    async fn marker_symlink_and_linked_worktree_gitdir_are_rejected_nofollow() {
        use std::os::unix::fs::symlink;

        let repo = Repo::new();
        let outside = tempfile::NamedTempFile::new().expect("outside marker");
        symlink(outside.path(), repo.path().join(".git/MERGE_HEAD")).expect("marker symlink");
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .expect_err("symlink marker")
                .code(),
            GitWorkspaceErrorCode::BranchOperationInProgress
        );
        fs::remove_file(repo.path().join(".git/MERGE_HEAD")).expect("remove symlink");

        let linked_parent = tempfile::Builder::new()
            .prefix("vega-linked-worktree-")
            .tempdir()
            .expect("linked parent");
        let linked = linked_parent.path().join("checkout");
        let linked_text = linked.to_str().expect("fixture utf8 path");
        git(
            repo.path(),
            &["worktree", "add", "-q", "-b", "linked", linked_text],
        );
        let linked_service = BranchWorkspaceService::new(&linked).expect("linked service");
        linked_service
            .refresh(CancellationToken::new())
            .await
            .expect("linked refresh");
        let marker_output = git_output(&linked, &["rev-parse", "--git-path", "MERGE_HEAD"]);
        let marker = PathBuf::from(OsString::from_vec(
            exact_single_line(&marker_output)
                .expect("marker line")
                .to_vec(),
        ));
        let marker = if marker.is_absolute() {
            marker
        } else {
            linked.join(marker)
        };
        fs::write(marker, "linked marker\n").expect("linked marker");
        assert_eq!(
            linked_service
                .refresh(CancellationToken::new())
                .await
                .expect_err("linked operation")
                .code(),
            GitWorkspaceErrorCode::BranchOperationInProgress
        );
    }

    #[tokio::test]
    async fn safe_temp_repo_switch_is_exact_and_authoritatively_refreshed() {
        let repo = Repo::new();
        git(repo.path(), &["switch", "-q", "-c", "topic"]);
        fs::write(repo.path().join("topic.txt"), "topic\n").expect("write topic");
        git(repo.path(), &["add", "topic.txt"]);
        git(repo.path(), &["commit", "-q", "-m", "topic"]);
        git(repo.path(), &["switch", "-q", "main"]);
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let target = snapshot
            .branches
            .iter()
            .find(|branch| branch.label == "topic")
            .expect("topic");
        let permit = service
            .prepare_switch(target.id, CancellationToken::new())
            .await
            .expect("preflight");
        let completion = service
            .execute_switch(permit, CancellationToken::new())
            .await;
        assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
        assert!(
            completion
                .snapshot
                .expect("authoritative snapshot")
                .branches
                .iter()
                .any(|branch| branch.label == "topic" && branch.current)
        );
    }

    #[tokio::test]
    async fn ignored_collision_is_not_overwritten_and_failure_refresh_is_authoritative() {
        let repo = Repo::new();
        fs::write(repo.path().join(".gitignore"), "ignored.txt\n").expect("gitignore");
        git(repo.path(), &["add", ".gitignore"]);
        git(repo.path(), &["commit", "-q", "-m", "ignore"]);
        git(repo.path(), &["switch", "-q", "-c", "tracked-ignored"]);
        fs::write(repo.path().join("ignored.txt"), "target tracked\n").expect("target file");
        git(repo.path(), &["add", "-f", "ignored.txt"]);
        git(repo.path(), &["commit", "-q", "-m", "tracked ignored"]);
        git(repo.path(), &["switch", "-q", "main"]);
        fs::write(repo.path().join("ignored.txt"), "local ignored\n").expect("local ignored");

        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("ignored remains clean");
        let permit = service
            .prepare_switch(
                branch_id(&snapshot, "tracked-ignored"),
                CancellationToken::new(),
            )
            .await
            .expect("permit");
        let completion = service
            .execute_switch(permit, CancellationToken::new())
            .await;
        assert_eq!(
            completion.outcome,
            BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::GitFailed)
        );
        assert!(
            completion
                .snapshot
                .expect("failure refresh")
                .branches
                .iter()
                .any(|branch| branch.label == "main" && branch.current)
        );
        assert_eq!(
            fs::read(repo.path().join("ignored.txt")).expect("preserved ignored"),
            b"local ignored\n"
        );
    }

    #[tokio::test]
    async fn target_gitattributes_and_explicit_filter_are_rejected() {
        let repo = Repo::new();
        git(repo.path(), &["switch", "-q", "-c", "unsafe-attrs"]);
        fs::write(repo.path().join(".gitattributes"), "*.txt text\n").expect("attrs");
        git(repo.path(), &["add", ".gitattributes"]);
        git(repo.path(), &["commit", "-q", "-m", "attrs"]);
        git(repo.path(), &["switch", "-q", "main"]);
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let target = snapshot
            .branches
            .iter()
            .find(|branch| branch.label == "unsafe-attrs")
            .expect("branch");
        assert_eq!(
            service
                .prepare_switch(target.id, CancellationToken::new())
                .await
                .expect_err("reject attrs")
                .code(),
            GitWorkspaceErrorCode::BranchUnsafeFilter
        );

        let filter_repo = Repo::new();
        fs::write(
            filter_repo.path().join(".gitattributes"),
            "*.txt filter=demo\n",
        )
        .expect("attrs");
        fs::write(filter_repo.path().join("file.txt"), "base\n").expect("base file");
        git(filter_repo.path(), &["add", "."]);
        git(filter_repo.path(), &["commit", "-q", "-m", "shared attrs"]);
        git(filter_repo.path(), &["switch", "-q", "-c", "unsafe-filter"]);
        fs::write(filter_repo.path().join("file.txt"), "filtered\n").expect("file");
        git(filter_repo.path(), &["add", "file.txt"]);
        git(filter_repo.path(), &["commit", "-q", "-m", "filter"]);
        git(filter_repo.path(), &["switch", "-q", "main"]);
        let recorder_dir = tempfile::Builder::new()
            .prefix("vega-filter-recorder-")
            .tempdir()
            .expect("recorder tempdir");
        let sentinel = recorder_dir.path().join("filter-side-effect");
        let recorder = recorder_dir.path().join("filter-recorder.sh");
        fs::write(
            &recorder,
            format!(
                "#!/bin/sh\nprintf side-effect >> '{}'\n/bin/cat\n",
                sentinel.display()
            ),
        )
        .expect("recorder");
        let mut permissions = fs::metadata(&recorder).expect("metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&recorder, permissions).expect("chmod");
        let recorder = recorder.to_str().expect("fixture utf8 path");
        git(
            filter_repo.path(),
            &["config", "filter.demo.clean", recorder],
        );
        git(
            filter_repo.path(),
            &["config", "filter.demo.smudge", recorder],
        );
        git(
            filter_repo.path(),
            &["config", "filter.demo.process", recorder],
        );
        let filter_service = BranchWorkspaceService::new(filter_repo.path()).expect("service");
        assert_eq!(
            filter_service
                .refresh(CancellationToken::new())
                .await
                .expect_err("reject filter")
                .code(),
            GitWorkspaceErrorCode::BranchUnsafeFilter
        );
        assert!(
            !sentinel.exists(),
            "filter driver executed during preflight"
        );
        assert_eq!(
            git_output(filter_repo.path(), &["branch", "--show-current"]),
            b"main\n"
        );
    }

    #[tokio::test]
    async fn deleted_and_renamed_away_gitattributes_are_rejected() {
        let repo = Repo::new();
        git(repo.path(), &["branch", "without-attrs"]);
        fs::write(repo.path().join(".gitattributes"), "*.txt text\n").expect("attrs");
        git(repo.path(), &["add", ".gitattributes"]);
        git(repo.path(), &["commit", "-q", "-m", "attrs on main"]);
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        assert_eq!(
            service
                .prepare_switch(
                    branch_id(&snapshot, "without-attrs"),
                    CancellationToken::new()
                )
                .await
                .expect_err("deleted attrs")
                .code(),
            GitWorkspaceErrorCode::BranchUnsafeFilter
        );

        git(repo.path(), &["switch", "-q", "-c", "rename-away"]);
        git(repo.path(), &["mv", ".gitattributes", "attributes.txt"]);
        git(repo.path(), &["commit", "-q", "-m", "rename attrs"]);
        git(repo.path(), &["switch", "-q", "main"]);
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        assert_eq!(
            service
                .prepare_switch(
                    branch_id(&snapshot, "rename-away"),
                    CancellationToken::new()
                )
                .await
                .expect_err("renamed attrs")
                .code(),
            GitWorkspaceErrorCode::BranchUnsafeFilter
        );
    }

    #[tokio::test]
    async fn newer_permit_invalidates_older_and_target_move_fails_before_switch() {
        let repo = Repo::new();
        git(repo.path(), &["switch", "-q", "-c", "topic"]);
        fs::write(repo.path().join("topic.txt"), "topic\n").expect("topic file");
        git(repo.path(), &["add", "topic.txt"]);
        git(repo.path(), &["commit", "-q", "-m", "topic"]);
        git(repo.path(), &["switch", "-q", "main"]);
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let id = branch_id(&snapshot, "topic");
        let older = service
            .prepare_switch(id, CancellationToken::new())
            .await
            .expect("older permit");
        let newer = service
            .prepare_switch(id, CancellationToken::new())
            .await
            .expect("newer permit");
        let rejected = service
            .execute_switch(older, CancellationToken::new())
            .await;
        assert_eq!(
            rejected.outcome,
            BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::StaleGeneration)
        );
        assert!(rejected.snapshot.is_none());
        let switched = service
            .execute_switch(newer, CancellationToken::new())
            .await;
        assert_eq!(switched.outcome, BranchSwitchOutcome::Switched);

        git(repo.path(), &["switch", "-q", "main"]);
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let permit = service
            .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
            .await
            .expect("permit");
        git(repo.path(), &["branch", "-f", "topic", "main"]);
        let raced = service
            .execute_switch(permit, CancellationToken::new())
            .await;
        assert_eq!(
            raced.outcome,
            BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::ChangedDuringRead)
        );
        assert!(raced.snapshot.is_some());
    }

    #[tokio::test]
    async fn dirty_and_operation_races_are_zero_switch_with_owner_cleanup() {
        let repo = Repo::new();
        git(repo.path(), &["branch", "topic"]);
        let service = BranchWorkspaceService::new(repo.path()).expect("service");
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let permit = service
            .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
            .await
            .expect("permit");
        fs::write(repo.path().join("raced.txt"), "dirty\n").expect("dirty race");
        let dirty = service
            .execute_switch(permit, CancellationToken::new())
            .await;
        assert_eq!(
            dirty.outcome,
            BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::BranchDirty)
        );
        assert!(dirty.snapshot.is_none());
        assert_eq!(
            git_output(repo.path(), &["branch", "--show-current"]),
            b"main\n"
        );

        fs::remove_file(repo.path().join("raced.txt")).expect("clean race");
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh after dirty");
        let permit = service
            .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
            .await
            .expect("permit after dirty");
        fs::write(repo.path().join(".git/MERGE_HEAD"), "marker\n").expect("marker race");
        let operation = service
            .execute_switch(permit, CancellationToken::new())
            .await;
        assert_eq!(
            operation.outcome,
            BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::BranchOperationInProgress)
        );
        assert!(operation.snapshot.is_none());
        assert_eq!(
            git_output(repo.path(), &["branch", "--show-current"]),
            b"main\n"
        );
    }

    #[test]
    fn trusted_switch_argv_is_exact_and_read_limits_remain_frozen() {
        let repo = Repo::new();
        let script = repo.path().join("fake-git.sh");
        fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\0' \"$@\" > switch-argv.bin\n",
        )
        .expect("script");
        let mut permissions = fs::metadata(&script).expect("metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).expect("chmod");
        let canonical_root = fs::canonicalize(repo.path()).expect("canonical root");
        let metadata = fs::metadata(&canonical_root).expect("root metadata");
        let runner = Runner::new(
            canonical_root,
            RootIdentity {
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
            Some(script),
        );
        runner
            .run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
            .expect("fake switch");
        let bytes = fs::read(repo.path().join("switch-argv.bin")).expect("argv");
        let actual: Vec<&[u8]> = bytes
            .split(|byte| *byte == 0)
            .filter(|field| !field.is_empty())
            .collect();
        let mut expected: Vec<&[u8]> = PREFIX.iter().map(|value| value.as_bytes()).collect();
        expected.extend(
            [
                "-c",
                "core.hooksPath=/dev/null",
                "switch",
                "--no-guess",
                "--no-overwrite-ignore",
                "--no-recurse-submodules",
                "topic",
            ]
            .iter()
            .map(|value| value.as_bytes()),
        );
        assert_eq!(actual, expected);
        assert_eq!(READ_TIMEOUT, Duration::from_secs(10));
        assert_eq!(MUTATION_TIMEOUT, Duration::from_secs(120));
        assert_eq!(MUTATION_STDOUT_LIMIT, 1024 * 1024);
        assert_eq!(STDERR_LIMIT, 64 * 1024);
    }

    #[test]
    fn target_check_attr_uses_exact_source_argv_and_literal_nul_stdin() {
        let repo = Repo::new();
        let runner = fake_runner(
            &repo,
            "authority-recorder.sh",
            "printf 'CALL\\n' >> authority-argv\nprintf '<%s>\\n' \"$@\" >> authority-argv\ncase \" $* \" in *' --diff-filter=ACMRT '*) printf 'M\\000literal path\\000';; esac\ncase \" $* \" in *' check-attr '*) /bin/cat > authority-stdin;; esac",
        );
        let current = b"0000000000000000000000000000000000000000";
        let target = b"1111111111111111111111111111111111111111";
        validate_target_changes(
            &runner,
            current,
            target,
            1024,
            target.len() + b"topic".len(),
            &CancellationToken::new(),
        )
        .expect("authority validation");
        assert_eq!(
            fs::read(repo.path().join("authority-stdin")).expect("stdin record"),
            b"literal path\0"
        );
        let argv = fs::read_to_string(repo.path().join("authority-argv")).expect("argv record");
        assert!(argv.contains(
            "<--source=1111111111111111111111111111111111111111>\n<-z>\n<--stdin>\n<--all>\n"
        ));
        assert_eq!(argv.matches("<check-attr>").count(), 1);
        assert_eq!(argv.matches("<diff>").count(), 2);
    }

    #[test]
    fn trusted_mutation_enforces_output_caps_nonzero_and_precancel_zero_spawn() {
        let repo = Repo::new();
        let exact_stdout = fake_runner(
            &repo,
            "stdout-exact.sh",
            "/usr/bin/yes x | /usr/bin/head -c 1048576",
        );
        assert!(
            exact_stdout
                .run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
                .is_ok()
        );
        let overflow_stdout = fake_runner(
            &repo,
            "stdout-overflow.sh",
            "/usr/bin/yes x | /usr/bin/head -c 1048577",
        );
        assert_eq!(
            error_code(
                overflow_stdout.run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
            ),
            GitWorkspaceErrorCode::OutputTooLarge
        );
        let exact_stderr = fake_runner(
            &repo,
            "stderr-exact.sh",
            "/usr/bin/yes x | /usr/bin/head -c 65536 >&2",
        );
        assert!(
            exact_stderr
                .run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
                .is_ok()
        );
        let overflow_stderr = fake_runner(
            &repo,
            "stderr-overflow.sh",
            "/usr/bin/yes x | /usr/bin/head -c 65537 >&2",
        );
        assert_eq!(
            error_code(
                overflow_stderr.run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
            ),
            GitWorkspaceErrorCode::OutputTooLarge
        );
        let nonzero = fake_runner(&repo, "nonzero.sh", "exit 17");
        assert_eq!(
            error_code(nonzero.run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())),
            GitWorkspaceErrorCode::GitFailed
        );
        let no_spawn = repo.path().join("no-spawn");
        let precancel = fake_runner(&repo, "precancel.sh", "printf spawned > no-spawn");
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(
            error_code(precancel.run_trusted_switch(OsStr::new("topic"), &token)),
            GitWorkspaceErrorCode::Cancelled
        );
        assert!(!no_spawn.exists());
    }

    #[test]
    fn trusted_mutation_cancellation_reaps_process_group_descendant() {
        let repo = Repo::new();
        let runner = fake_runner(
            &repo,
            "descendant.sh",
            "/bin/sleep 30 &\nprintf '%s' \"$!\" > descendant.pid\nwait",
        );
        let token = CancellationToken::new();
        let worker_token = token.clone();
        let worker = std::thread::spawn(move || {
            runner.run_trusted_switch(OsStr::new("topic"), &worker_token)
        });
        let pid_file = repo.path().join("descendant.pid");
        for _ in 0..500 {
            if fs::read_to_string(&pid_file).is_ok_and(|value| !value.is_empty()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let pid = fs::read_to_string(&pid_file)
            .expect("descendant pid")
            .parse::<u32>()
            .expect("numeric pid");
        token.cancel();
        assert_eq!(
            error_code(worker.join().expect("worker join")),
            GitWorkspaceErrorCode::Cancelled
        );
        let gone = (0..100).any(|_| {
            let status = Command::new(KILL)
                .args(["-0", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("kill probe");
            if status.success() {
                std::thread::sleep(Duration::from_millis(5));
                false
            } else {
                true
            }
        });
        assert!(gone, "mutation descendant survived cancellation");
    }

    #[tokio::test]
    async fn rejected_execute_cannot_compete_with_owner_cleanup_refresh() {
        let repo = Repo::new();
        git(repo.path(), &["switch", "-q", "-c", "topic"]);
        fs::write(repo.path().join("topic.txt"), "topic\n").expect("topic file");
        git(repo.path(), &["add", "topic.txt"]);
        git(repo.path(), &["commit", "-q", "-m", "topic"]);
        git(repo.path(), &["switch", "-q", "main"]);

        let controls = tempfile::Builder::new()
            .prefix("vega-branch-barrier-")
            .tempdir()
            .expect("controls");
        let started = controls.path().join("started");
        let release = controls.path().join("release");
        let attempts = controls.path().join("attempts");
        let script = controls.path().join("blocking-switch.sh");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf started > '{}'\nwhile [ ! -f '{}' ]; do /bin/sleep 0.01; done\nprintf 'attempt\\n' >> '{}'\nexec /usr/bin/git \"$@\"\n",
                started.display(),
                release.display(),
                attempts.display()
            ),
        )
        .expect("blocking script");
        let mut permissions = fs::metadata(&script).expect("metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).expect("chmod");

        let service = Arc::new(
            BranchWorkspaceService::new_with_mutation_for_test(repo.path(), script)
                .expect("service"),
        );
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("refresh");
        let target = branch_id(&snapshot, "topic");
        let owner_permit = service
            .prepare_switch(target, CancellationToken::new())
            .await
            .expect("owner permit");
        let owner_service = service.clone();
        let owner = tokio::spawn(async move {
            owner_service
                .execute_switch(owner_permit, CancellationToken::new())
                .await
        });
        for _ in 0..500 {
            if started.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(started.exists());
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .expect_err("owner-exclusive refresh")
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
        assert_eq!(
            service
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .generation,
            snapshot.generation
        );

        let rejected_permit = service
            .prepare_switch(target, CancellationToken::new())
            .await
            .expect("concurrent permit");
        let rejected = service
            .execute_switch(rejected_permit, CancellationToken::new())
            .await;
        assert_eq!(
            rejected.outcome,
            BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::StaleGeneration)
        );
        assert!(rejected.snapshot.is_none());
        assert!(
            service
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .active_mutation
                .is_some()
        );

        let third_permit = service
            .prepare_switch(target, CancellationToken::new())
            .await
            .expect("third permit");
        let third = service
            .execute_switch(third_permit, CancellationToken::new())
            .await;
        assert!(third.snapshot.is_none());
        assert!(
            service
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .active_mutation
                .is_some()
        );

        fs::write(&release, "release\n").expect("release");
        let completion = owner.await.expect("owner join");
        assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
        assert!(
            completion
                .snapshot
                .expect("owner snapshot")
                .branches
                .iter()
                .any(|branch| branch.label == "topic" && branch.current)
        );
        assert_eq!(
            fs::read_to_string(&attempts).expect("attempts"),
            "attempt\n"
        );
        assert!(
            service
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .active_mutation
                .is_none()
        );
    }

    #[tokio::test]
    async fn refresh_registered_before_owner_cannot_commit_after_lease_acquisition() {
        let repo = Repo::new();
        git(repo.path(), &["switch", "-q", "-c", "topic"]);
        fs::write(repo.path().join("topic.txt"), "topic\n").expect("topic file");
        git(repo.path(), &["add", "topic.txt"]);
        git(repo.path(), &["commit", "-q", "-m", "topic"]);
        git(repo.path(), &["switch", "-q", "main"]);

        let controls = tempfile::Builder::new()
            .prefix("vega-refresh-owner-race-")
            .tempdir()
            .expect("controls");
        let read_arm = controls.path().join("read-arm");
        let read_claim = controls.path().join("read-claim");
        let read_entered = controls.path().join("read-entered");
        let read_release = controls.path().join("read-release");
        let mutation_entered = controls.path().join("mutation-entered");
        let mutation_release = controls.path().join("mutation-release");
        let attempts = controls.path().join("attempts");
        let read_wrapper = controls.path().join("read-wrapper.sh");
        fs::write(
            &read_wrapper,
            format!(
                "#!/bin/sh\nif [ -f '{}' ] && /bin/mkdir '{}' 2>/dev/null; then\n  printf entered > '{}'\n  while [ ! -f '{}' ]; do /bin/sleep 0.01; done\nfi\nexec /usr/bin/git \"$@\"\n",
                read_arm.display(),
                read_claim.display(),
                read_entered.display(),
                read_release.display()
            ),
        )
        .expect("read wrapper");
        let mutation_wrapper = controls.path().join("mutation-wrapper.sh");
        fs::write(
            &mutation_wrapper,
            format!(
                "#!/bin/sh\nprintf entered > '{}'\nwhile [ ! -f '{}' ]; do /bin/sleep 0.01; done\nprintf 'attempt\\n' >> '{}'\nexec /usr/bin/git \"$@\"\n",
                mutation_entered.display(),
                mutation_release.display(),
                attempts.display()
            ),
        )
        .expect("mutation wrapper");
        for script in [&read_wrapper, &mutation_wrapper] {
            let mut permissions = fs::metadata(script).expect("metadata").permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(script, permissions).expect("chmod");
        }

        let service = Arc::new(
            BranchWorkspaceService::new_with_executables_for_test(
                repo.path(),
                read_wrapper,
                mutation_wrapper,
            )
            .expect("service"),
        );
        let snapshot = service
            .refresh(CancellationToken::new())
            .await
            .expect("initial refresh");
        let permit = service
            .prepare_switch(branch_id(&snapshot, "topic"), CancellationToken::new())
            .await
            .expect("permit");

        fs::write(&read_arm, "arm\n").expect("arm read");
        let refresh_service = service.clone();
        let refresh =
            tokio::spawn(async move { refresh_service.refresh(CancellationToken::new()).await });
        for _ in 0..500 {
            if read_entered.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(
            read_entered.exists(),
            "refresh did not enter capture barrier"
        );

        let owner_service = service.clone();
        let owner = tokio::spawn(async move {
            owner_service
                .execute_switch(permit, CancellationToken::new())
                .await
        });
        for _ in 0..500 {
            if mutation_entered.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(
            mutation_entered.exists(),
            "owner did not enter mutation barrier"
        );
        assert!(
            service
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .active_mutation
                .is_some()
        );

        fs::write(&read_release, "release\n").expect("release read");
        assert_eq!(
            refresh
                .await
                .expect("refresh join")
                .expect_err("late refresh stale")
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
        {
            let state = service
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            assert_eq!(state.generation, snapshot.generation);
            assert!(state.snapshot.as_ref().is_some_and(|current| {
                current.generation == snapshot.generation
                    && current
                        .branches
                        .iter()
                        .any(|branch| branch.label == "main" && branch.current)
            }));
            assert!(state.active_mutation.is_some());
        }

        fs::write(&mutation_release, "release\n").expect("release mutation");
        let completion = owner.await.expect("owner join");
        assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
        assert!(
            completion
                .snapshot
                .expect("authoritative snapshot")
                .branches
                .iter()
                .any(|branch| branch.label == "topic" && branch.current)
        );
        assert_eq!(
            fs::read_to_string(&attempts).expect("attempts"),
            "attempt\n"
        );
        assert!(
            service
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .active_mutation
                .is_none()
        );
    }

    #[test]
    fn retained_cap_is_inclusive_and_one_more_fails_closed() {
        let branch = PrivateBranch {
            id: BranchId {
                generation: 0,
                slot: 0,
                seal: 0,
            },
            short: OsString::from("main"),
            full: b"refs/heads/main".to_vec(),
            oid: vec![b'0'; 40],
            current: true,
        };
        let base = retained_size(&[], &[], &[], &[], std::slice::from_ref(&branch)).expect("base");
        let exact = vec![0; BRANCH_RETAINED_LIMIT - base];
        assert!(
            ensure_retained_parts(&[], &[], &exact, &[], std::slice::from_ref(&branch)).is_ok()
        );
        let plus_one = vec![0; BRANCH_RETAINED_LIMIT - base + 1];
        assert_eq!(
            error_code(ensure_retained_parts(
                &[],
                &[],
                &plus_one,
                &[],
                std::slice::from_ref(&branch)
            )),
            GitWorkspaceErrorCode::OutputTooLarge
        );
    }

    #[test]
    fn rejected_concurrent_call_cannot_release_another_mutation_lease() {
        let mut state = BranchState::default();
        state.issued_permits.extend([1, 2]);
        assert!(acquire_mutation(&mut state, 1));
        assert!(!acquire_mutation(&mut state, 2));
        release_mutation(&mut state, 2);
        assert_eq!(state.active_mutation, Some(1));

        state.issued_permits.insert(3);
        assert!(!acquire_mutation(&mut state, 3));
        assert_eq!(state.active_mutation, Some(1));
        release_mutation(&mut state, 1);
        state.issued_permits.insert(4);
        assert!(acquire_mutation(&mut state, 4));
    }

    #[test]
    fn parser_rejects_delete_duplicate_and_malformed_records() {
        assert_eq!(
            error_code(parse_target_paths(b"D\0gone\0")),
            GitWorkspaceErrorCode::MalformedOutput
        );
        assert!(
            parse_refs(
                b"0000000000000000000000000000000000000000\0refs/heads/main\0",
                b"main",
                b"0000000000000000000000000000000000000000"
            )
            .is_err()
        );
        assert_eq!(
            error_code(parse_target_paths(b"M\0same\0A\0same\0")),
            GitWorkspaceErrorCode::MalformedOutput
        );
        assert_eq!(
            error_code(parse_refs(
                b"bad\0refs/heads/main\0\n",
                b"main",
                b"0000000000000000000000000000000000000000"
            )),
            GitWorkspaceErrorCode::MalformedOutput
        );
        assert_eq!(
            error_code(parse_delete_paths(b"M\0file\0")),
            GitWorkspaceErrorCode::MalformedOutput
        );
    }

    #[test]
    fn raw_ref_oid_status_and_line_codecs_are_strict_and_bytes_first() {
        let sha1 = b"0000000000000000000000000000000000000000";
        let sha256 = b"0000000000000000000000000000000000000000000000000000000000000000";
        let mixed = [sha256.as_slice(), b"\0refs/heads/topic\0\n"].concat();
        assert_eq!(
            error_code(parse_refs(&mixed, b"main", sha1)),
            GitWorkspaceErrorCode::MalformedOutput
        );
        let raw = b"team/non-utf8-\xff";
        assert!(validate_branch_short(raw).is_ok());
        assert_eq!(escape_ref(raw), "team/non-utf8-\\xff");
        for invalid in [
            b"-topic".as_slice(),
            b"HEAD",
            b"@",
            b"a..b",
            b"a//b",
            b"a/@{b",
            b".hidden/topic",
            b"a/.hidden",
            b"a.lock",
            b"a/b.lock",
            b"a.",
            b"a b",
            b"a~b",
            b"a^b",
            b"a:b",
            b"a?b",
            b"a*b",
            b"a[b",
            b"a\\b",
        ] {
            assert!(validate_branch_short(invalid).is_err(), "{invalid:?}");
        }
        for valid in [b"A".as_slice(), b"M", b"T", b"R0", b"R100", b"C007"] {
            assert!(parse_acmrt_status(valid).is_ok(), "{valid:?}");
        }
        for invalid in [
            b"".as_slice(),
            b"Mgarbage",
            b"D",
            b"R",
            b"R101",
            b"R1000",
            b"R-1",
            b"C1x",
        ] {
            assert!(parse_acmrt_status(invalid).is_err(), "{invalid:?}");
        }
        assert_eq!(exact_single_line(b"one\n").expect("line"), b"one");
        for invalid in [b"one".as_slice(), b"one\n\n", b"one\r\n", b"one\0\n"] {
            assert!(exact_single_line(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn path_counts_and_filter_values_are_inclusive_and_fail_closed() {
        let oid = b"0000000000000000000000000000000000000000";
        let mut refs = Vec::new();
        for index in 0..BRANCH_LIMIT {
            refs.extend_from_slice(oid);
            refs.extend_from_slice(b"\0refs/heads/b");
            refs.extend_from_slice(index.to_string().as_bytes());
            refs.extend_from_slice(b"\0\n");
        }
        let parsed_refs = parse_refs(&refs, b"b0", oid).expect("exact branch count");
        assert_eq!(parsed_refs.len(), BRANCH_LIMIT);
        refs.extend_from_slice(oid);
        refs.extend_from_slice(b"\0refs/heads/overflow\0\n");
        assert_eq!(
            error_code(parse_refs(&refs, b"b0", oid)),
            GitWorkspaceErrorCode::OutputTooLarge
        );

        let mut exact = Vec::new();
        for index in 0..PATH_LIMIT {
            exact.extend_from_slice(b"A\0");
            exact.extend_from_slice(format!("p{index}").as_bytes());
            exact.push(0);
        }
        let parsed = parse_target_paths(&exact).expect("exact path count");
        assert_eq!(parsed.materialized.len(), PATH_LIMIT);
        exact.extend_from_slice(b"A\0overflow\0");
        assert_eq!(
            error_code(parse_target_paths(&exact)),
            GitWorkspaceErrorCode::OutputTooLarge
        );

        let mut rename_exact = Vec::new();
        for index in 0..(PATH_LIMIT / 2) {
            rename_exact.extend_from_slice(b"R100\0");
            rename_exact.extend_from_slice(format!("old-{index}").as_bytes());
            rename_exact.push(0);
            rename_exact.extend_from_slice(format!("new-{index}").as_bytes());
            rename_exact.push(0);
        }
        let parsed = parse_target_paths(&rename_exact).expect("exact authority count");
        assert_eq!(parsed.authority.len(), PATH_LIMIT);
        rename_exact.extend_from_slice(b"R100\0old-overflow\0new-overflow\0");
        assert_eq!(
            error_code(parse_target_paths(&rename_exact)),
            GitWorkspaceErrorCode::OutputTooLarge
        );

        let literal = parse_target_paths(b"M\0:(glob)**\0A\0:!safe\0T\0space tab\t\xff\0")
            .expect("literal path bytes");
        assert_eq!(literal.materialized.len(), 3);

        let paths = vec![b"file.txt".to_vec()];
        for value in [b"set".as_slice(), b"unset", b"unspecified", b"demo", b""] {
            let mut output = b"file.txt\0filter\0".to_vec();
            output.extend_from_slice(value);
            output.push(0);
            assert_eq!(
                error_code(validate_branch_attrs(&paths, &output)),
                GitWorkspaceErrorCode::BranchUnsafeFilter,
                "filter value {value:?}"
            );
        }
    }

    #[test]
    fn switch_authority_uses_one_checked_combined_budget() {
        let fixed =
            std::mem::size_of::<BranchSwitchPermit>() + std::mem::size_of::<SwitchAuthority>();
        let paths = vec![b"old/name".to_vec(), b"new/name".to_vec()];
        let mut budget = RetainedBudget::new(BRANCH_RETAINED_LIMIT);
        budget.charge(fixed).expect("fixed");
        budget.charge(1024).expect("acmrt raw");
        charge_paths(&mut budget, &paths).expect("materialized");
        charge_paths(&mut budget, &paths).expect("authority");
        budget.charge(2048).expect("delete raw");
        budget.charge(128).expect("stdin");
        budget
            .charge(2 * std::mem::size_of::<usize>())
            .expect("arc counters");
        budget.charge(4096).expect("attrs");
        let remaining = budget.remaining();
        budget.charge(remaining).expect("inclusive cap");
        assert_eq!(
            error_code(budget.charge(1)),
            GitWorkspaceErrorCode::OutputTooLarge
        );
    }
}
