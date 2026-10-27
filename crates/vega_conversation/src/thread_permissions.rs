use std::path::PathBuf;

use vega_store::{Store, threads};

use crate::types::{
    FullAccessGrant, FullAccessScope, PermissionMode, TaskBackend, Thread, ThreadPermissionChange,
    ThreadPermissionError, ThreadStatus,
};

#[derive(Clone)]
pub struct ThreadPermissionService {
    database_path: PathBuf,
}

impl ThreadPermissionService {
    pub fn new(database_path: PathBuf) -> Self {
        Self { database_path }
    }

    pub fn read(&self, id: &str) -> Result<Thread, ThreadPermissionError> {
        let store = Store::open_read_only(&self.database_path).map_err(unavailable)?;
        require_open(&store)?;
        if vega_store::permission_contracts::load_state(store.conn())
            .map_err(unavailable)?
            .reset_pending
        {
            return Err(ThreadPermissionError::ResetPending);
        }
        crate::threads::get_thread(&store, id).map_err(unavailable)
    }

    pub fn prepare(
        &self,
        source: &Thread,
        mode: PermissionMode,
    ) -> Result<ThreadPermissionChange, ThreadPermissionError> {
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        let store = Store::open_read_only(&self.database_path).map_err(unavailable)?;
        require_open(&store)?;
        if source.backend != TaskBackend::Native
            || source.status != ThreadStatus::Active
            || ulid::Ulid::from_string(&source.id).is_err()
            || !(0..i64::MAX).contains(&source.permission_revision)
        {
            return Err(ThreadPermissionError::Conflict);
        }
        let tx = store.conn().unchecked_transaction().map_err(unavailable)?;
        let state = vega_store::permission_contracts::load_state(&tx).map_err(unavailable)?;
        if state.reset_pending {
            return Err(ThreadPermissionError::ResetPending);
        }
        let current = threads::find(&tx, &source.id).map_err(unavailable)?;
        let draft = match current {
            Some(row) => {
                let current = crate::threads::thread_from_row(&row).map_err(unavailable)?;
                if !same_source(&current, source) {
                    return Err(ThreadPermissionError::Conflict);
                }
                false
            }
            None if source.parent_thread_id.is_none() && source.full_access_grant.is_none() => true,
            None => return Err(ThreadPermissionError::Conflict),
        };
        tx.commit().map_err(unavailable)?;
        Ok(ThreadPermissionChange {
            source: source.clone(),
            database: store.database_identity(),
            authority_epoch: state.authority_epoch,
            mode,
            draft,
        })
    }

    pub fn save(
        &self,
        change: &ThreadPermissionChange,
        confirmed: bool,
    ) -> Result<Thread, ThreadPermissionError> {
        if change.mode == PermissionMode::FullAccess && !confirmed {
            return Err(ThreadPermissionError::ConfirmationRequired);
        }
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        require_open(&store)?;
        if store.database_identity() != change.database {
            return Err(ThreadPermissionError::Conflict);
        }
        let _admission = if change.draft {
            Some(crate::thread_access::acquire(&store, &change.source.id).map_err(unavailable)?)
        } else {
            None
        };
        let tx = store.immediate_transaction().map_err(unavailable)?;
        let state = vega_store::permission_contracts::load_state(&tx).map_err(unavailable)?;
        if state.reset_pending {
            return Err(ThreadPermissionError::ResetPending);
        }
        if state.authority_epoch != change.authority_epoch {
            return Err(ThreadPermissionError::Conflict);
        }
        let current = threads::find(&tx, &change.source.id).map_err(unavailable)?;
        match current {
            None if change.draft => {}
            Some(row) if !change.draft => {
                let current = crate::threads::thread_from_row(&row).map_err(unavailable)?;
                if !same_source(&current, &change.source) {
                    return Err(ThreadPermissionError::Conflict);
                }
            }
            _ => return Err(ThreadPermissionError::Conflict),
        }
        let issued_at = crate::threads::now_ms();
        let grant = if change.mode == PermissionMode::FullAccess {
            Some(
                FullAccessGrant::confirmed(
                    &state.authority_epoch,
                    FullAccessScope::Thread {
                        thread_id: change.source.id.clone(),
                    },
                    issued_at,
                )
                .ok_or(ThreadPermissionError::Unavailable)?,
            )
        } else {
            None
        };
        let encoded = grant
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(unavailable)?;
        let revision = change
            .source
            .permission_revision
            .checked_add(1)
            .ok_or(ThreadPermissionError::Unavailable)?;
        if change.draft {
            let next = Thread {
                permission_mode: change.mode,
                permission_revision: revision,
                full_access_grant: grant.clone(),
                default_permission: None,
                ..change.source.clone()
            };
            crate::threads::materialize_draft_in_transaction(&tx, &next).map_err(unavailable)?;
        } else if !vega_store::permission_contracts::compare_exchange_thread(
            &tx,
            &change.source.id,
            change.source.permission_revision,
            &state.authority_epoch,
            change.mode.as_str(),
            encoded.as_deref(),
            issued_at,
        )
        .map_err(unavailable)?
        {
            return Err(ThreadPermissionError::Conflict);
        }
        tx.commit_with_rollback()
            .map_err(|_| ThreadPermissionError::OutcomeUnknown)?;
        let saved = crate::threads::get_thread(&store, &change.source.id)
            .map_err(|_| ThreadPermissionError::OutcomeUnknown)?;
        if saved.permission_mode != change.mode
            || saved.permission_revision != revision
            || saved.full_access_grant != grant
            || saved.project_id != change.source.project_id
        {
            return Err(ThreadPermissionError::OutcomeUnknown);
        }
        Ok(saved)
    }
}

fn require_open(store: &Store) -> Result<(), ThreadPermissionError> {
    if vega_store::thread_admission::is_database_paused(store.database_identity())
        .map_err(unavailable)?
    {
        return Err(ThreadPermissionError::ResetPending);
    }
    Ok(())
}

fn same_source(current: &Thread, expected: &Thread) -> bool {
    current.id == expected.id
        && current.project_id == expected.project_id
        && current.backend == expected.backend
        && current.status == expected.status
        && current.mode == expected.mode
        && current.model == expected.model
        && current.permission_mode == expected.permission_mode
        && current.permission_revision == expected.permission_revision
        && current.full_access_grant == expected.full_access_grant
        && current.parent_thread_id == expected.parent_thread_id
}

fn unavailable(_: impl std::fmt::Display) -> ThreadPermissionError {
    ThreadPermissionError::Unavailable
}

#[cfg(test)]
mod tests;
