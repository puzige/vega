use super::*;
use crate::database_writer::{DatabaseCommand, Sealed};

#[cfg(test)]
mod tests;

pub(super) fn source_key(source: &DefaultPermissionSnapshot) -> Result<String, ConversationError> {
    serde_json::to_string(&(
        &source.config_path,
        &source.configuration_id,
        &source.permission_mode,
        source.permission_revision,
        &source.full_access_grant,
    ))
    .map_err(|_| ConversationError::InvalidTaskIdentity)
}

pub(super) struct PrepareDraft {
    pub draft: Thread,
    pub operation: String,
    pub source: Option<String>,
}
impl Sealed for PrepareDraft {}
impl DatabaseCommand for PrepareDraft {
    type Output = Result<(String, String), ConversationError>;
    fn apply(self, store: &Store) -> Self::Output {
        let _coordination = coordinate()?;
        require_admission_open(store)?;
        let tx = store.immediate_transaction().map_err(storage_error)?;
        let state = vega_store::permission_contracts::load_state(&tx).map_err(storage_error)?;
        let (generation, pending): (String, Option<String>) = tx
            .query_row(
                "SELECT generation,pending_id FROM default_permission_intent WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(storage_error)?;
        if state.reset_pending
            || pending.is_some()
            || self
                .draft
                .draft_authority_epoch
                .as_ref()
                .is_some_and(|epoch| epoch != &state.authority_epoch)
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        tx.execute(
            "DELETE FROM draft_creation_intents WHERE thread_id=?1 AND state='prepared'",
            [&self.draft.id],
        )
        .map_err(storage_error)?;
        tx.execute("INSERT INTO draft_creation_intents(operation_id,thread_id,authority_epoch,configuration_generation,source_json,state,created_at) VALUES(?1,?2,?3,?4,?5,'prepared',?6)",
            (&self.operation, &self.draft.id, &state.authority_epoch, &generation, &self.source, crate::threads::now_ms())).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok((state.authority_epoch, generation))
    }
}

pub(super) enum DefaultIntent {
    Begin {
        operation: String,
        source: String,
        target: String,
        epoch: String,
    },
    Finish {
        operation: String,
        source: String,
        target: String,
        epoch: String,
    },
    Cancel {
        operation: String,
        source: String,
        target: String,
        epoch: String,
    },
}
impl Sealed for DefaultIntent {}
impl DatabaseCommand for DefaultIntent {
    type Output = Result<String, DefaultPermissionError>;
    fn apply(self, store: &Store) -> Self::Output {
        let _coordination = coordinate().map_err(|_| DefaultPermissionError::Unavailable)?;
        require_admission_open(store).map_err(|_| DefaultPermissionError::ResetPending)?;
        let tx = store
            .immediate_transaction()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let state = vega_store::permission_contracts::load_state(&tx)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let finish = matches!(self, Self::Finish { .. } | Self::Cancel { .. });
        let (operation, source, target, epoch) = match self {
            Self::Begin {
                operation,
                source,
                target,
                epoch,
            }
            | Self::Finish {
                operation,
                source,
                target,
                epoch,
            }
            | Self::Cancel {
                operation,
                source,
                target,
                epoch,
            } => (operation, source, target, epoch),
        };
        if state.reset_pending {
            return Err(DefaultPermissionError::ResetPending);
        }
        if state.authority_epoch != epoch {
            return Err(DefaultPermissionError::Conflict);
        }
        let (pending, old_source, old_target): (Option<String>,Option<String>,Option<String>) = tx.query_row(
            "SELECT pending_id,source_json,target_json FROM default_permission_intent WHERE singleton=1", [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(|_| DefaultPermissionError::Unavailable)?;
        let id = if let Some(pending) = pending {
            if old_source.as_deref() != Some(&source) || old_target.as_deref() != Some(&target) {
                return Err(DefaultPermissionError::Conflict);
            }
            if finish && pending != operation {
                return Err(DefaultPermissionError::Conflict);
            }
            pending
        } else if finish {
            return Err(DefaultPermissionError::Conflict);
        } else {
            operation
        };
        if finish {
            tx.execute("UPDATE default_permission_intent SET pending_id=NULL,source_json=NULL,target_json=NULL WHERE singleton=1 AND pending_id=?1", [&id])
                .map_err(|_| DefaultPermissionError::Unavailable)?;
        } else {
            tx.execute("UPDATE default_permission_intent SET generation=?1,pending_id=?2,source_json=?3,target_json=?4 WHERE singleton=1",
                (ulid::Ulid::generate().to_string(), &id, source, target)).map_err(|_| DefaultPermissionError::Unavailable)?;
        }
        tx.commit()
            .map_err(|_| DefaultPermissionError::OutcomeUnknown)?;
        Ok(id)
    }
}

pub(super) fn reconcile_default(
    database: &std::path::Path,
    configuration: &std::path::Path,
) -> Result<(), DefaultPermissionError> {
    let store = Store::open_read_only(database).map_err(|_| DefaultPermissionError::Unavailable)?;
    let pending: (Option<String>,Option<String>,Option<String>) = store.conn().query_row(
        "SELECT pending_id,source_json,target_json FROM default_permission_intent WHERE singleton=1", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(|_| DefaultPermissionError::Unavailable)?;
    let Some(operation) = pending.0 else {
        return Ok(());
    };
    let source = pending.1.ok_or(DefaultPermissionError::OutcomeUnknown)?;
    let target = pending.2.ok_or(DefaultPermissionError::OutcomeUnknown)?;
    let coordination = coordinate().map_err(|_| DefaultPermissionError::Unavailable)?;
    let identity =
        configuration_identity(configuration).map_err(|_| DefaultPermissionError::Unavailable)?;
    let edit = vega_store::config::begin_existing_edit(std::path::Path::new(&identity))
        .map_err(|_| DefaultPermissionError::Unavailable)?;
    let current = source_key(&default_permission_snapshot(
        Some(std::path::Path::new(&identity)),
        &edit.config.defaults,
    ))
    .map_err(|_| DefaultPermissionError::Unavailable)?;
    let epoch = vega_store::permission_contracts::load_state(store.conn())
        .map_err(|_| DefaultPermissionError::Unavailable)?
        .authority_epoch;
    let command = if current == target {
        DefaultIntent::Finish {
            operation,
            source,
            target,
            epoch,
        }
    } else if current == source {
        DefaultIntent::Cancel {
            operation,
            source,
            target,
            epoch,
        }
    } else {
        return Err(DefaultPermissionError::Conflict);
    };
    drop(edit);
    drop(coordination);
    default_commit(&store, command)?;
    Ok(())
}

pub(super) fn default_commit(
    store: &Store,
    command: DefaultIntent,
) -> Result<String, DefaultPermissionError> {
    let path = store
        .database_path()
        .ok_or(DefaultPermissionError::Unavailable)?;
    crate::database_writer::commit(path, store.database_identity(), command)
        .map_err(|_| DefaultPermissionError::OutcomeUnknown)?
}
