use std::path::Path;

use vega_store::{Store, config};

use crate::types::{PermissionResetError, StartupError, StartupWorkspace};

pub fn prepare_default_workspace() -> Result<StartupWorkspace, StartupError> {
    let paths =
        vega_store::paths::AppPaths::resolve().map_err(|_| StartupError::PathsUnavailable)?;
    paths
        .initialize_data_dir()
        .map_err(|_| StartupError::PathsUnavailable)?;
    prepare_workspace(&paths.database_file(), &paths.config_file())
}

pub fn prepare_workspace(
    database: &Path,
    config_path: &Path,
) -> Result<StartupWorkspace, StartupError> {
    if let Some(parent) = database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|_| StartupError::StorageUnavailable)?;
    }
    let store = Store::open(database).map_err(|_| StartupError::StorageUnavailable)?;
    store
        .migrate()
        .map_err(|_| StartupError::StorageUnavailable)?;
    vega_store::shell_operations::recover_generation(
        store.conn(),
        vega_runtime::shell_generation(),
        0,
    )
    .map_err(|_| StartupError::StorageUnavailable)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |value| value.as_millis().min(i64::MAX as u128) as i64);
    for item in vega_store::shell_operations::list_blockers(store.conn())
        .map_err(|_| StartupError::StorageUnavailable)?
    {
        if item.facts.resources_closed
            || !item
                .facts
                .cleanup_reasons
                .iter()
                .any(|reason| reason == "control_continuity_lost")
        {
            continue;
        }
        let closed = match vega_store::shell_operations::resource(store.conn(), &item.operation_id)
            .map_err(|_| StartupError::StorageUnavailable)?
        {
            Some(identity) => serde_json::from_str::<vega_tools::BashSessionResource>(&identity)
                .is_ok_and(|resource| resource.recover().is_ok()),
            None => true,
        };
        if closed {
            let mut facts = item.facts;
            facts.resources_closed = true;
            facts.cleanup_state = "unresolved".into();
            let observation = format!(
                "reopen-resources:{}:{}",
                vega_runtime::shell_generation(),
                item.operation_id
            );
            vega_store::shell_operations::append_cleanup_observation(
                store.conn(),
                &item.operation_id,
                &observation,
                &facts,
                now,
            )
            .map_err(|_| StartupError::StorageUnavailable)?;
        }
    }
    crate::permission_reset::recover_pending(&store, config_path)
        .map_err(StartupError::PermissionReset)?;
    let (config, selected_project) = {
        let _coordination = crate::permission_contracts::coordinate()
            .map_err(|_| StartupError::StorageUnavailable)?;
        require_ready(&store)?;
        let edit =
            config::begin_edit(config_path).map_err(|_| StartupError::ConfigurationUnavailable)?;
        let tx = store
            .immediate_transaction()
            .map_err(|_| StartupError::StorageUnavailable)?;
        require_ready(&store)?;
        if !config_path.exists() {
            edit.save()
                .map_err(|_| StartupError::ConfigurationUnavailable)?;
        }
        let config = edit.config.clone();
        let selected_project = crate::threads::current_project(&store)
            .map_err(|_| StartupError::StorageUnavailable)?
            .map(|project| project.id);
        tx.commit().map_err(|_| StartupError::StorageUnavailable)?;
        (config, selected_project)
    };
    let config_root = config_path.parent().ok_or(StartupError::PathsUnavailable)?;
    crate::mcp_settings::McpServerSettingsService::new(
        database.to_path_buf(),
        config_root.to_path_buf(),
    )
    .reconcile_declarations()
    .map_err(|_| StartupError::ConfigurationUnavailable)?;
    crate::automation::recover_startup(&store).map_err(|_| StartupError::AutomationRecovery)?;
    let _coordination =
        crate::permission_contracts::coordinate().map_err(|_| StartupError::StorageUnavailable)?;
    require_ready(&store)?;
    Ok(StartupWorkspace {
        store,
        config,
        config_path: config_path.to_path_buf(),
        selected_project,
    })
}

fn require_ready(store: &Store) -> Result<(), StartupError> {
    if vega_store::thread_admission::is_database_paused(store.database_identity())
        .map_err(|_| StartupError::StorageUnavailable)?
    {
        return Err(StartupError::PermissionReset(PermissionResetError::Busy));
    }
    if vega_store::permission_reset::load(store.conn())
        .map_err(|_| StartupError::StorageUnavailable)?
        .is_some_and(|record| !record.completed)
    {
        return Err(StartupError::PermissionReset(PermissionResetError::Busy));
    }
    Ok(())
}
