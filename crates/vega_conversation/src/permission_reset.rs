use std::path::Path;

use vega_store::Store;
use vega_store::config::{self, AppConfig};
use vega_store::permission_reset::{self as persistence, ResetConfigSource, ResetRecord};
use vega_store::thread_admission::try_pause_database;

use crate::agent::ReplaySessionRegistry;
use crate::types::{
    DatabaseAdmissionPause, PermissionResetError, PermissionResetIntent, PermissionResetReceipt,
};

mod session;
pub use session::PermissionResetSession;

pub fn pause_admission(store: &Store) -> Result<DatabaseAdmissionPause, PermissionResetError> {
    let _coordination =
        crate::permission_contracts::coordinate().map_err(|_| PermissionResetError::Unavailable)?;
    try_pause_database(store.database_identity())
        .map_err(unavailable)?
        .ok_or(PermissionResetError::Busy)
}

pub fn prepare_reset(
    store: &Store,
    config_path: &Path,
) -> Result<PermissionResetIntent, PermissionResetError> {
    let _coordination =
        crate::permission_contracts::coordinate().map_err(|_| PermissionResetError::Unavailable)?;
    if vega_store::thread_admission::is_database_paused(store.database_identity())
        .map_err(unavailable)?
    {
        return Err(PermissionResetError::Busy);
    }
    let identity = configuration_identity(config_path)?;
    let edit = config::begin_existing_edit(Path::new(&identity))
        .map_err(|_| PermissionResetError::ConfigurationUnavailable)?;
    let tx = store.conn().unchecked_transaction().map_err(unavailable)?;
    let state = vega_store::permission_contracts::load_state(&tx).map_err(unavailable)?;
    persistence::load(&tx).map_err(unavailable)?;
    if state.reset_pending {
        return Err(PermissionResetError::Busy);
    }
    if !(0..i64::MAX).contains(&edit.config.defaults.permission_revision) {
        return Err(PermissionResetError::Unavailable);
    }
    if configuration_identity(config_path)? != identity {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    tx.commit().map_err(unavailable)?;
    Ok(PermissionResetIntent {
        database: store.database_identity(),
        authority_epoch: state.authority_epoch,
        source: config_source(&identity, &edit.config),
    })
}

pub fn reset_quiescent(
    store: &Store,
    config_path: &Path,
    intent: &PermissionResetIntent,
    pause: &DatabaseAdmissionPause,
    replay: &ReplaySessionRegistry,
) -> Result<PermissionResetReceipt, PermissionResetError> {
    let _coordination =
        crate::permission_contracts::coordinate().map_err(|_| PermissionResetError::Unavailable)?;
    if store.database_identity() != intent.database {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    crate::ordinary_submissions::require_database_settled(store.database_identity(), pause)?;
    let _precompute = require_precompute_settled(store)?;
    let identity = configuration_identity(config_path)?;
    if identity != intent.source.configuration_id {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    let mut edit = config::begin_existing_edit(Path::new(&identity))
        .map_err(|_| PermissionResetError::ConfigurationUnavailable)?;
    if config_source(&identity, &edit.config) != intent.source
        || configuration_identity(config_path)? != identity
    {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    let tx = store.immediate_transaction().map_err(unavailable)?;
    let state = vega_store::permission_contracts::load_state(&tx).map_err(unavailable)?;
    if state.reset_pending {
        return Err(PermissionResetError::Busy);
    }
    if state.authority_epoch != intent.authority_epoch {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    require_settled(&tx)?;
    let record = persistence::begin(
        &tx,
        &intent.authority_epoch,
        &intent.source,
        crate::threads::now_ms(),
    )
    .map_err(unavailable)?;
    crate::automation::revoke_in_transaction(&tx, crate::threads::now_ms()).map_err(unavailable)?;
    tx.commit_with_rollback()
        .map_err(|_| PermissionResetError::OutcomeUnknown)?;
    finish_pending(store, config_path, pause, &record, &mut edit, Some(replay))
}

pub fn recover_pending_with_pause(
    store: &Store,
    config_path: &Path,
    pause: &DatabaseAdmissionPause,
    replay: &ReplaySessionRegistry,
) -> Result<Option<PermissionResetReceipt>, PermissionResetError> {
    let _coordination =
        crate::permission_contracts::coordinate().map_err(|_| PermissionResetError::Unavailable)?;
    let Some(record) = persistence::load(store.conn()).map_err(unavailable)? else {
        return Ok(None);
    };
    if record.completed {
        return Ok(None);
    }
    resume_record(store, config_path, pause, &record, Some(replay)).map(Some)
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
    resume_record(store, config_path, &pause, &record, None).map(Some)
}

fn resume_record(
    store: &Store,
    config_path: &Path,
    pause: &DatabaseAdmissionPause,
    record: &ResetRecord,
    replay: Option<&ReplaySessionRegistry>,
) -> Result<PermissionResetReceipt, PermissionResetError> {
    crate::ordinary_submissions::require_database_settled(store.database_identity(), pause)?;
    let identity = configuration_identity(config_path)?;
    if identity != record.source.configuration_id {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    let mut edit = config::begin_existing_edit(Path::new(&identity))
        .map_err(|_| PermissionResetError::ConfigurationUnavailable)?;
    finish_pending(store, config_path, pause, record, &mut edit, replay)
}

fn require_precompute_settled(
    store: &Store,
) -> Result<crate::types::ContextPrecomputePause, PermissionResetError> {
    let pause = crate::types::ContextPrecomputeRegistry::global()
        .pause_database(store.database_identity())
        .map_err(unavailable)?;
    if !pause.drain().is_finished().map_err(unavailable)? {
        return Err(PermissionResetError::Busy);
    }
    Ok(pause)
}

fn finish_pending(
    store: &Store,
    config_path: &Path,
    pause: &DatabaseAdmissionPause,
    record: &ResetRecord,
    edit: &mut config::ConfigEdit,
    replay: Option<&ReplaySessionRegistry>,
) -> Result<PermissionResetReceipt, PermissionResetError> {
    crate::ordinary_submissions::require_database_settled(store.database_identity(), pause)?;
    let _precompute = require_precompute_settled(store)?;
    let identity = configuration_identity(config_path)?;
    if identity != record.source.configuration_id {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    let path = Path::new(&identity);
    let current = config_source(&identity, &edit.config);
    if !record.accepts_config(&current) {
        return Err(PermissionResetError::ConfigurationConflict);
    }
    let tx = store.immediate_transaction().map_err(unavailable)?;
    verify_pending(&tx, record)?;
    require_settled(&tx)?;
    crate::automation::revoke_in_transaction(&tx, crate::threads::now_ms()).map_err(unavailable)?;
    tx.commit_with_rollback()
        .map_err(|_| PermissionResetError::OutcomeUnknown)?;
    crate::ordinary_submissions::invalidate_database_recovery(store.database_identity(), pause)
        .map_err(unavailable)?;
    if let Some(replay) = replay {
        replay.invalidate_all_acknowledged()?;
    }
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
    verify_pending(&tx, record)?;
    require_settled(&tx)?;
    persistence::finish(&tx, record, &saved_source).map_err(unavailable)?;
    tx.commit_with_rollback()
        .map_err(|_| PermissionResetError::OutcomeUnknown)?;
    let mut expected = record.clone();
    expected.completed = true;
    if persistence::load(store.conn()).map_err(unavailable)? != Some(expected) {
        return Err(PermissionResetError::OutcomeUnknown);
    }
    Ok(PermissionResetReceipt {
        reset_id: record.id.clone(),
        authority_epoch: record.authority_epoch.clone(),
        default_permission_revision: record.target_revision,
    })
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
        "SELECT EXISTS(SELECT 1 FROM messages WHERE status='streaming') OR EXISTS(SELECT 1 FROM automation_runs WHERE finished=0) OR EXISTS(SELECT 1 FROM delegated_executions WHERE state IN ('running','cancelling')) OR EXISTS(SELECT 1 FROM delegated_execution_recovery WHERE release_state IN ('held','quarantined')) OR EXISTS(SELECT 1 FROM tool_calls WHERE status IN ('approved','running','failed','cancelled') AND lower(tool) NOT IN ('read','glob','grep','ask_user_question','checkpoint_stage')) OR EXISTS(SELECT 1 FROM tool_calls WHERE status='success' AND lower(tool) NOT IN ('read','glob','grep','ask_user_question','checkpoint_stage','write','edit')) OR EXISTS(SELECT 1 FROM credential_continuations WHERE state IN ('claimed','unknown')) OR EXISTS(SELECT 1 FROM command_scope_audits WHERE state IN ('consumed','unknown'))",
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
