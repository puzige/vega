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

mod parsing;

#[cfg(test)]
mod tests;

pub(crate) use parsing::reject_operation_markers;
pub(crate) use parsing::*;

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
