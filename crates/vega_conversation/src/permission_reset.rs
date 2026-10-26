use std::path::Path;

use vega_store::Store;
use vega_store::config::{self, AppConfig};
use vega_store::permission_reset::{self as persistence, ResetConfigSource, ResetRecord};
use vega_store::thread_admission::try_pause_database;

use crate::types::{DatabaseAdmissionPause, PermissionResetError, PermissionResetReceipt};

#[cfg(test)]
mod tests;

pub fn pause_admission(store: &Store) -> Result<DatabaseAdmissionPause, PermissionResetError> {
    let _coordination =
        crate::permission_contracts::coordinate().map_err(|_| PermissionResetError::Unavailable)?;
    try_pause_database(store.database_identity())
        .map_err(unavailable)?
        .ok_or(PermissionResetError::Busy)
}

pub fn recover_pending(
    store: &Store,
    config_path: &Path,
) -> Result<Option<PermissionResetReceipt>, PermissionResetError> {
    let _coordination =
        crate::permission_contracts::coordinate().map_err(|_| PermissionResetError::Unavailable)?;
    let Some(record) = persistence::load(store.conn()).map_err(unavailable)? else {
        return Ok(None);
    };
    if record.completed {
        return Ok(None);
    }
    let pause = try_pause_database(store.database_identity())
        .map_err(unavailable)?
        .ok_or(PermissionResetError::Busy)?;
    if !pause.owners_released().map_err(unavailable)? {
        return Err(PermissionResetError::Busy);
    }
    let identity = configuration_identity(config_path)?;
    if identity != record.source.configuration_id {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    let path = Path::new(&identity);
    let mut edit = config::begin_existing_edit(path)
        .map_err(|_| PermissionResetError::ConfigurationUnavailable)?;
    let current = config_source(&identity, &edit.config);
    if !record.accepts_config(&current) {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    let tx = store.immediate_transaction().map_err(unavailable)?;
    verify_pending(&tx, &record)?;
    require_settled(&tx)?;
    crate::automation::revoke_in_transaction(&tx, crate::threads::now_ms()).map_err(unavailable)?;
    tx.commit_with_rollback()
        .map_err(|_| PermissionResetError::OutcomeUnknown)?;
    let latest =
        config::read_from(path).map_err(|_| PermissionResetError::ConfigurationUnavailable)?;
    if configuration_identity(config_path)? != identity
        || config_source(&identity, &latest) != current
    {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    if !record.config_is_reset(&current) {
        edit.config = latest;
        edit.config.defaults.permission_mode = "confirm".into();
        edit.config.defaults.permission_revision = record.target_revision;
        edit.config.defaults.full_access_grant = None;
        edit.save()
            .map_err(|_| PermissionResetError::OutcomeUnknown)?;
    }
    let saved = config::read_from(path).map_err(|_| PermissionResetError::OutcomeUnknown)?;
    let saved_source = config_source(&identity, &saved);
    if !record.config_is_reset(&saved_source) || configuration_identity(config_path)? != identity {
        return Err(PermissionResetError::OutcomeUnknown);
    }
    let tx = store.immediate_transaction().map_err(unavailable)?;
    verify_pending(&tx, &record)?;
    require_settled(&tx)?;
    persistence::finish(&tx, &record, &saved_source).map_err(unavailable)?;
    tx.commit_with_rollback()
        .map_err(|_| PermissionResetError::OutcomeUnknown)?;
    let mut expected = record.clone();
    expected.completed = true;
    if persistence::load(store.conn()).map_err(unavailable)? != Some(expected) {
        return Err(PermissionResetError::OutcomeUnknown);
    }
    Ok(Some(PermissionResetReceipt {
        reset_id: record.id,
        authority_epoch: record.authority_epoch,
        default_permission_revision: record.target_revision,
    }))
}

fn verify_pending(
    conn: &vega_store::Connection,
    record: &ResetRecord,
) -> Result<(), PermissionResetError> {
    if persistence::load(conn).map_err(unavailable)?.as_ref() != Some(record) {
        return Err(PermissionResetError::Unavailable);
    }
    Ok(())
}

fn require_settled(conn: &vega_store::Connection) -> Result<(), PermissionResetError> {
    let unsettled: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE status='streaming') OR EXISTS(SELECT 1 FROM automation_runs WHERE finished=0) OR EXISTS(SELECT 1 FROM delegated_executions WHERE state IN ('running','cancelling')) OR EXISTS(SELECT 1 FROM delegated_execution_recovery WHERE release_state IN ('held','quarantined')) OR EXISTS(SELECT 1 FROM tool_calls WHERE status IN ('approved','running','failed','cancelled') AND lower(tool) NOT IN ('read','glob','grep','ask_user_question','checkpoint_stage')) OR EXISTS(SELECT 1 FROM credential_continuations WHERE state IN ('claimed','unknown')) OR EXISTS(SELECT 1 FROM command_scope_audits WHERE state='unknown')",
        [],
        |row| row.get(0),
    ).map_err(unavailable)?;
    if unsettled {
        return Err(PermissionResetError::UnsettledWork);
    }
    Ok(())
}

fn configuration_identity(path: &Path) -> Result<String, PermissionResetError> {
    let path = path
        .canonicalize()
        .map_err(|_| PermissionResetError::ConfigurationUnavailable)?;
    if !path.is_file() {
        return Err(PermissionResetError::ConfigurationUnavailable);
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or(PermissionResetError::ConfigurationUnavailable)
}

fn config_source(identity: &str, config: &AppConfig) -> ResetConfigSource {
    ResetConfigSource {
        configuration_id: identity.into(),
        mode: config.defaults.permission_mode.clone(),
        revision: config.defaults.permission_revision,
        grant: config.defaults.full_access_grant.clone(),
    }
}

fn unavailable(_: impl std::fmt::Display) -> PermissionResetError {
    PermissionResetError::Unavailable
}
