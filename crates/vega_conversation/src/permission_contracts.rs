use std::sync::{Mutex, MutexGuard};

use vega_store::Store;

use crate::types::{
    ConversationError, DefaultPermissionChange, DefaultPermissionError, DefaultPermissionSnapshot,
    DefaultPermissionValues, FullAccessGrant, FullAccessScope, PermissionMode, TaskBackend, Thread,
    ThreadStatus,
};

static COORDINATION: Mutex<()> = Mutex::new(());

pub(crate) fn coordinate() -> Result<MutexGuard<'static, ()>, ConversationError> {
    COORDINATION
        .lock()
        .map_err(|_| ConversationError::InvalidTaskIdentity)
}

pub fn default_permission_snapshot(
    config_path: Option<&std::path::Path>,
    defaults: &vega_store::config::Defaults,
) -> DefaultPermissionSnapshot {
    DefaultPermissionSnapshot {
        config_path: config_path.map(std::path::Path::to_path_buf),
        configuration_id: config_path.and_then(|path| configuration_identity(path).ok()),
        permission_mode: defaults.permission_mode.clone(),
        permission_revision: defaults.permission_revision,
        full_access_grant: defaults.full_access_grant.clone(),
    }
}

fn configuration_identity(path: &std::path::Path) -> Result<String, ConversationError> {
    let path = path.canonicalize().map_err(storage_error)?;
    if !path.is_file() {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or(ConversationError::InvalidTaskIdentity)
}

pub(crate) struct DraftCreationGuard {
    config: Option<vega_store::config::ConfigEdit>,
    draft: Thread,
    _coordination: MutexGuard<'static, ()>,
}

impl DraftCreationGuard {
    pub(crate) fn begin(draft: &Thread) -> Result<Self, ConversationError> {
        Self::under_coordination(draft, coordinate()?)
    }

    fn under_coordination(
        draft: &Thread,
        coordination: MutexGuard<'static, ()>,
    ) -> Result<Self, ConversationError> {
        if draft.backend != TaskBackend::Native
            || draft.status != ThreadStatus::Active
            || draft.parent_thread_id.is_some()
            || draft.forked_from_thread_id.is_some()
            || draft.forked_from_message_id.is_some()
            || draft.permission_revision != 0
            || draft.full_access_grant.is_some()
            || ulid::Ulid::from_string(&draft.id).is_err()
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        let config = if let Some(snapshot) = &draft.default_permission {
            let path = snapshot
                .config_path
                .as_deref()
                .ok_or(ConversationError::InvalidTaskIdentity)?;
            let edit = vega_store::config::begin_edit(path).map_err(storage_error)?;
            if snapshot.configuration_id.as_deref() != Some(configuration_identity(path)?.as_str())
                || snapshot.permission_revision < 0
                || snapshot.permission_mode != draft.permission_mode.as_str()
                || edit.config.defaults.permission_mode != snapshot.permission_mode
                || edit.config.defaults.permission_revision != snapshot.permission_revision
                || edit.config.defaults.full_access_grant != snapshot.full_access_grant
            {
                return Err(ConversationError::InvalidTaskIdentity);
            }
            Some(edit)
        } else {
            if draft.permission_mode == PermissionMode::FullAccess {
                return Err(ConversationError::InvalidTaskIdentity);
            }
            None
        };
        Ok(Self {
            _coordination: coordination,
            config,
            draft: draft.clone(),
        })
    }

    pub(crate) fn materialize(
        &self,
        transaction: &vega_store::Transaction<'_>,
    ) -> Result<Thread, ConversationError> {
        let state =
            vega_store::permission_contracts::load_state(transaction).map_err(storage_error)?;
        if state.reset_pending {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        let mut thread = self.draft.clone();
        if let (Some(edit), Some(snapshot)) = (&self.config, &thread.default_permission) {
            let default_grant: Option<FullAccessGrant> = edit
                .config
                .defaults
                .full_access_grant
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|_| ConversationError::InvalidTaskIdentity)?;
            if thread.permission_mode == PermissionMode::FullAccess {
                let grant = default_grant.ok_or(ConversationError::InvalidTaskIdentity)?;
                let configuration_id = snapshot
                    .configuration_id
                    .clone()
                    .ok_or(ConversationError::InvalidTaskIdentity)?;
                if !grant.verifies(
                    &state.authority_epoch,
                    &FullAccessScope::FutureThreads { configuration_id },
                ) {
                    return Err(ConversationError::InvalidTaskIdentity);
                }
                thread.full_access_grant = Some(
                    grant
                        .derive_thread(&thread.id, crate::threads::now_ms())
                        .ok_or(ConversationError::InvalidTaskIdentity)?,
                );
            } else if default_grant.is_some() {
                return Err(ConversationError::InvalidTaskIdentity);
            }
        }
        thread.default_permission = None;
        crate::threads::materialize_draft_in_transaction(transaction, &thread)
    }
}

pub fn change_thread_permission(
    store: &Store,
    thread_id: &str,
    expected_revision: i64,
    mode: PermissionMode,
    confirmed_full_access: bool,
    issued_at: i64,
) -> Result<Thread, ConversationError> {
    let _coordination = coordinate()?;
    let _admission = crate::thread_access::acquire(store, thread_id)?;
    let transaction = store.immediate_transaction().map_err(storage_error)?;
    let state =
        vega_store::permission_contracts::load_state(&transaction).map_err(storage_error)?;
    if state.reset_pending || (mode == PermissionMode::FullAccess && !confirmed_full_access) {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let grant = if mode == PermissionMode::FullAccess {
        Some(
            FullAccessGrant::confirmed(
                &state.authority_epoch,
                FullAccessScope::Thread {
                    thread_id: thread_id.to_owned(),
                },
                issued_at,
            )
            .ok_or(ConversationError::InvalidTaskIdentity)?,
        )
    } else {
        None
    };
    let encoded = grant
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| ConversationError::InvalidTaskIdentity)?;
    if !vega_store::permission_contracts::compare_exchange_thread(
        &transaction,
        thread_id,
        expected_revision,
        &state.authority_epoch,
        mode.as_str(),
        encoded.as_deref(),
        issued_at,
    )
    .map_err(storage_error)?
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    transaction.commit().map_err(storage_error)?;
    crate::threads::get_thread(store, thread_id)
}

#[derive(Clone)]
pub struct DefaultPermissionService {
    database_path: std::path::PathBuf,
    config_path: std::path::PathBuf,
}

impl DefaultPermissionService {
    pub fn new(database_path: std::path::PathBuf, config_path: std::path::PathBuf) -> Self {
        Self {
            database_path,
            config_path,
        }
    }

    pub fn read(&self) -> Result<DefaultPermissionValues, DefaultPermissionError> {
        let change = self.capture(PermissionMode::Confirm)?;
        Ok(DefaultPermissionValues {
            permission_mode: change.source.permission_mode,
            permission_revision: change.source.permission_revision,
            full_access_grant: change.source.full_access_grant,
        })
    }

    pub fn prepare(
        &self,
        expected: &DefaultPermissionValues,
        mode: PermissionMode,
    ) -> Result<DefaultPermissionChange, DefaultPermissionError> {
        let change = self.capture(mode)?;
        if change.source.permission_mode != expected.permission_mode
            || change.source.permission_revision != expected.permission_revision
            || change.source.full_access_grant != expected.full_access_grant
        {
            return Err(DefaultPermissionError::Conflict);
        }
        Ok(change)
    }

    fn capture(
        &self,
        mode: PermissionMode,
    ) -> Result<DefaultPermissionChange, DefaultPermissionError> {
        let _coordination = coordinate().map_err(|_| DefaultPermissionError::Unavailable)?;
        let identity = configuration_identity(&self.config_path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let path = std::path::Path::new(&identity);
        let edit = vega_store::config::begin_edit(path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let store = Store::open_read_only(&self.database_path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let transaction = store
            .conn()
            .unchecked_transaction()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let state = vega_store::permission_contracts::load_state(&transaction)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        if state.reset_pending {
            return Err(DefaultPermissionError::ResetPending);
        }
        if !(0..i64::MAX).contains(&edit.config.defaults.permission_revision) {
            return Err(DefaultPermissionError::Unavailable);
        }
        let source = default_permission_snapshot(Some(path), &edit.config.defaults);
        if source.configuration_id.as_deref() != Some(identity.as_str())
            || configuration_identity(&self.config_path).ok().as_deref() != Some(identity.as_str())
        {
            return Err(DefaultPermissionError::Conflict);
        }
        transaction
            .commit()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        Ok(DefaultPermissionChange {
            source,
            database_identity: store.database_identity().permission_fence_key(),
            authority_epoch: state.authority_epoch,
            mode,
        })
    }

    pub fn save(
        &self,
        change: &DefaultPermissionChange,
        confirmed_full_access: bool,
    ) -> Result<DefaultPermissionValues, DefaultPermissionError> {
        if change.mode == PermissionMode::FullAccess && !confirmed_full_access {
            return Err(DefaultPermissionError::ConfirmationRequired);
        }
        let _coordination = coordinate().map_err(|_| DefaultPermissionError::Unavailable)?;
        let identity = configuration_identity(&self.config_path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let path = std::path::Path::new(&identity);
        let mut edit = vega_store::config::begin_edit(path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let store = Store::open_read_only(&self.database_path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let transaction = store
            .conn()
            .unchecked_transaction()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let state = vega_store::permission_contracts::load_state(&transaction)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        if state.reset_pending {
            return Err(DefaultPermissionError::ResetPending);
        }
        if change.source != default_permission_snapshot(Some(path), &edit.config.defaults)
            || change.database_identity != store.database_identity().permission_fence_key()
            || change.authority_epoch != state.authority_epoch
            || !(0..i64::MAX).contains(&change.source.permission_revision)
        {
            return Err(DefaultPermissionError::Conflict);
        }
        let grant = if change.mode == PermissionMode::FullAccess {
            Some(
                FullAccessGrant::confirmed(
                    &state.authority_epoch,
                    FullAccessScope::FutureThreads {
                        configuration_id: identity.clone(),
                    },
                    crate::threads::now_ms(),
                )
                .ok_or(DefaultPermissionError::Unavailable)?,
            )
        } else {
            None
        };
        edit.config.defaults.permission_mode = change.mode.as_str().to_owned();
        edit.config.defaults.permission_revision = change.source.permission_revision + 1;
        edit.config.defaults.full_access_grant = grant
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        transaction
            .commit()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        edit.save()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let saved = vega_store::config::read_from(path)
            .map_err(|_| DefaultPermissionError::OutcomeUnknown)?;
        let values = DefaultPermissionValues::from(&saved.defaults);
        if values != DefaultPermissionValues::from(&edit.config.defaults)
            || configuration_identity(&self.config_path).ok().as_deref() != Some(identity.as_str())
        {
            return Err(DefaultPermissionError::OutcomeUnknown);
        }
        Ok(values)
    }
}

pub(crate) fn verify_thread_grant(
    conn: &vega_store::Connection,
    thread_id: &str,
    grant: Option<&FullAccessGrant>,
) -> Result<(), ConversationError> {
    let state = vega_store::permission_contracts::load_state(conn).map_err(storage_error)?;
    if state.reset_pending
        || !grant.is_some_and(|grant| {
            grant.verifies(
                &state.authority_epoch,
                &FullAccessScope::Thread {
                    thread_id: thread_id.to_owned(),
                },
            )
        })
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    Ok(())
}

pub fn prepare_submission_permission(
    store: &Store,
    submission: &crate::types::OrdinarySubmission,
    worker: &crate::types::OrdinaryWorker,
    expected: &Thread,
    draft: Option<&Thread>,
) -> Result<Thread, ConversationError> {
    let coordination = coordinate()?;
    if worker.attempt_id().submission() != submission.id()
        || !worker
            .admission()
            .verifies(store.database_identity(), &expected.id)
        || expected.id != submission.snapshot().thread_id
        || draft.is_some_and(|draft| draft != expected)
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let accepted = submission
        .accepted_permission_fence()
        .map_err(|_| ConversationError::InvalidTaskIdentity)?;
    let (creation, _coordination) = match (draft, &accepted) {
        (Some(draft), None) => (
            Some(DraftCreationGuard::under_coordination(draft, coordination)?),
            None,
        ),
        _ => (None, Some(coordination)),
    };
    let transaction = store.immediate_transaction().map_err(storage_error)?;
    let exists = vega_store::threads::find(&transaction, &expected.id)
        .map_err(storage_error)?
        .is_some();
    let created = match (exists, &creation, &accepted) {
        (false, Some(creation), None) => Some(creation.materialize(&transaction)?),
        (true, None, _) => None,
        _ => return Err(ConversationError::InvalidTaskIdentity),
    };
    let row = vega_store::threads::open_thread_in_transaction(
        &transaction,
        &expected.id,
        crate::threads::now_ms(),
    )
    .map_err(storage_error)?
    .ok_or(ConversationError::InvalidTaskIdentity)?;
    let thread = crate::threads::thread_from_row(&row)?;
    let fence = capture_fence(store, &row)?;
    let expected_permission = created.as_ref().unwrap_or(expected);
    if thread.model != submission.raw().model()
        || thread.backend != TaskBackend::Native
        || thread.status != ThreadStatus::Active
        || thread.project_id != expected.project_id
        || thread.mode != expected.mode
        || thread.permission_mode != expected.permission_mode
        || match &accepted {
            Some(accepted) => accepted != &fence,
            None => {
                fence.permission_revision != expected_permission.permission_revision
                    || fence.full_access_grant != expected_permission.full_access_grant
            }
        }
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let pending = worker
        .stage_permission_fence(fence)
        .map_err(|_| ConversationError::InvalidTaskIdentity)?;
    match transaction.commit_with_rollback() {
        Ok(()) => pending.publish(),
        Err(vega_store::write_coordination::CommitFailure::RolledBack(error)) => {
            if created.is_some()
                && !matches!(
                    vega_store::threads::find(store.conn(), &expected.id),
                    Ok(None)
                )
            {
                pending.mark_unknown();
            }
            return Err(storage_error(error));
        }
        Err(vega_store::write_coordination::CommitFailure::Unknown(error)) => {
            pending.mark_unknown();
            return Err(storage_error(error));
        }
    }
    Ok(thread)
}

pub(crate) fn capture_fence(
    store: &Store,
    thread: &vega_store::threads::ThreadRow,
) -> Result<crate::types::PermissionFence, ConversationError> {
    let state =
        vega_store::permission_contracts::load_state(store.conn()).map_err(storage_error)?;
    if state.reset_pending {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let workspace = crate::threads::task_workspace_root(store, &thread.id)?;
    let fence = crate::types::PermissionFence {
        schema_version: 1,
        execution_policy_version: 1,
        authority_epoch: state.authority_epoch,
        database_identity: store.database_identity().permission_fence_key(),
        thread_id: thread.id.clone(),
        project_id: thread.project_id.clone(),
        workspace: workspace
            .to_str()
            .ok_or(ConversationError::InvalidTaskIdentity)?
            .to_owned(),
        run_mode: thread.mode.clone(),
        permission_mode: thread.permission_mode.clone(),
        permission_revision: thread.permission_revision,
        delegation: None,
        exact_rules: if thread.project_id.is_empty() {
            Vec::new()
        } else {
            let mut rules: Vec<_> =
                vega_store::permissions::list_exact(store.conn(), &thread.project_id)
                    .map_err(storage_error)?
                    .into_iter()
                    .map(|rule| (rule.tool, rule.pattern))
                    .collect();
            rules.sort();
            rules
        },
        full_access_grant: thread
            .full_access_grant
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| ConversationError::InvalidTaskIdentity)?,
    };
    if !fence.is_valid() {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    Ok(fence)
}

pub(crate) fn interrupted_authority_matches(
    conn: &vega_store::Connection,
    expected: &crate::types::InterruptedAssistant,
    current: &crate::types::PermissionFence,
) -> Result<bool, ConversationError> {
    if !current.is_valid()
        || current.delegation.is_some()
        || !vega_store::permission_reset::replay_allowed(conn, &current.thread_id, expected.seq)
            .map_err(storage_error)?
    {
        return Ok(false);
    }
    let Some(encoded) = vega_store::permission_contracts::load_interrupted_run_fence(
        conn,
        &expected.message_id,
        &current.thread_id,
        expected.seq,
    )
    .map_err(storage_error)?
    else {
        return Ok(false);
    };
    Ok(
        serde_json::from_str::<crate::types::PermissionFence>(&encoded)
            .is_ok_and(|saved| saved == *current),
    )
}

pub(crate) fn capture_delegated_fence(
    store: &Store,
    thread: &vega_store::threads::ThreadRow,
    parent_thread: &str,
    parent_message: &str,
    child_message: &str,
    accepted: &crate::types::PermissionFence,
) -> Result<crate::types::PermissionFence, ConversationError> {
    let state =
        vega_store::permission_contracts::load_state(store.conn()).map_err(storage_error)?;
    let encoded = vega_store::permission_contracts::load_run_fence(
        store.conn(),
        parent_message,
        parent_thread,
    )
    .map_err(storage_error)?
    .ok_or(ConversationError::InvalidTaskIdentity)?;
    let saved: crate::types::PermissionFence =
        serde_json::from_str(&encoded).map_err(|_| ConversationError::InvalidTaskIdentity)?;
    if state.reset_pending
        || &saved != accepted
        || !accepted.is_valid()
        || accepted.delegation.is_some()
        || accepted.thread_id != parent_thread
        || accepted.authority_epoch != state.authority_epoch
        || accepted.database_identity != store.database_identity().permission_fence_key()
        || thread.parent_thread_id.as_deref() != Some(parent_thread)
        || thread.project_id != accepted.project_id
        || crate::threads::task_workspace_root(store, &thread.id)?.to_str()
            != Some(accepted.workspace.as_str())
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    if let Some((previous_message, encoded)) =
        vega_store::permission_contracts::previous_delegated_fence(
            store.conn(),
            &thread.id,
            child_message,
            parent_thread,
            parent_message,
        )
        .map_err(storage_error)?
    {
        let previous: crate::types::PermissionFence = serde_json::from_str(
            encoded
                .as_deref()
                .ok_or(ConversationError::InvalidTaskIdentity)?,
        )
        .map_err(|_| ConversationError::InvalidTaskIdentity)?;
        let source = previous
            .delegation
            .as_ref()
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        if !previous.is_valid()
            || previous.thread_id != thread.id
            || source.child_message_id != previous_message
            || source.parent_fence.as_ref() != accepted
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
    }
    let mut fence = accepted.clone();
    fence.thread_id = thread.id.clone();
    fence.permission_revision = thread.permission_revision;
    fence.full_access_grant = None;
    fence.delegation = Some(crate::types::DelegatedPermissionSource {
        parent_message_id: parent_message.to_owned(),
        child_message_id: child_message.to_owned(),
        parent_fence: Box::new(accepted.clone()),
    });
    if !fence.is_valid() {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    Ok(fence)
}

fn storage_error(error: impl std::fmt::Display) -> ConversationError {
    ConversationError::Store(error.to_string())
}
