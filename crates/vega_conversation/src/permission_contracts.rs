use std::sync::{Mutex, MutexGuard};

use vega_store::Store;

use crate::types::{ConversationError, FullAccessGrant, FullAccessScope, PermissionMode, Thread};

static COORDINATION: Mutex<()> = Mutex::new(());

pub(crate) fn coordinate() -> Result<MutexGuard<'static, ()>, ConversationError> {
    COORDINATION
        .lock()
        .map_err(|_| ConversationError::InvalidTaskIdentity)
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

pub fn change_default_permission(
    store: &Store,
    config_path: &std::path::Path,
    expected_revision: i64,
    mode: PermissionMode,
    confirmed_full_access: bool,
    issued_at: i64,
) -> Result<vega_store::config::Defaults, ConversationError> {
    let _coordination = coordinate()?;
    let mut edit = vega_store::config::begin_edit(config_path).map_err(storage_error)?;
    let transaction = store.immediate_transaction().map_err(storage_error)?;
    let state =
        vega_store::permission_contracts::load_state(&transaction).map_err(storage_error)?;
    if state.reset_pending
        || expected_revision < 0
        || expected_revision == i64::MAX
        || edit.config.defaults.permission_revision != expected_revision
        || (mode == PermissionMode::FullAccess && !confirmed_full_access)
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let grant = if mode == PermissionMode::FullAccess {
        Some(
            FullAccessGrant::confirmed(
                &state.authority_epoch,
                FullAccessScope::FutureThreads {
                    configuration_id: config_path
                        .canonicalize()
                        .map_err(storage_error)?
                        .to_str()
                        .ok_or(ConversationError::InvalidTaskIdentity)?
                        .to_owned(),
                },
                issued_at,
            )
            .ok_or(ConversationError::InvalidTaskIdentity)?,
        )
    } else {
        None
    };
    edit.config.defaults.permission_mode = mode.as_str().to_owned();
    edit.config.defaults.permission_revision = expected_revision + 1;
    edit.config.defaults.full_access_grant = grant
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    edit.save().map_err(storage_error)?;
    let saved = vega_store::config::read_from(config_path).map_err(storage_error)?;
    if saved.defaults != edit.config.defaults {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    Ok(saved.defaults)
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

pub fn bind_submission_permission(
    store: &Store,
    submission: &crate::types::OrdinarySubmission,
    expected: &Thread,
) -> Result<(), ConversationError> {
    let _coordination = coordinate()?;
    let transaction = store.immediate_transaction().map_err(storage_error)?;
    let thread = vega_store::threads::find(&transaction, &expected.id)
        .map_err(storage_error)?
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    let fence = capture_fence(store, &thread)?;
    if fence.project_id != expected.project_id
        || fence.run_mode != expected.mode.as_str()
        || fence.permission_mode != expected.permission_mode.as_str()
        || fence.permission_revision != expected.permission_revision
        || fence.full_access_grant != expected.full_access_grant
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    submission
        .bind_permission_fence(fence)
        .map_err(|_| ConversationError::InvalidTaskIdentity)?;
    transaction.commit().map_err(storage_error)
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
