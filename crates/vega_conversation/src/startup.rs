use std::path::Path;

use vega_store::{Store, config};

use crate::types::{PermissionResetError, StartupError, StartupWorkspace};

pub fn prepare_default_workspace() -> Result<StartupWorkspace, StartupError> {
    let root = vega_store::paths::data_dir().ok_or(StartupError::PathsUnavailable)?;
    let config = config::config_path().map_err(|_| StartupError::PathsUnavailable)?;
    prepare_workspace(&root.join("vega.db"), &config)
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
