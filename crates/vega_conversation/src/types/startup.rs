use std::path::PathBuf;

use vega_store::{Store, config::AppConfig};

use super::PermissionResetError;

pub struct StartupWorkspace {
    pub(crate) store: Store,
    pub(crate) config: AppConfig,
    pub(crate) config_path: PathBuf,
    pub(crate) selected_project: Option<String>,
}

impl StartupWorkspace {
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn into_parts(self) -> (Store, AppConfig, PathBuf, Option<String>) {
        (
            self.store,
            self.config,
            self.config_path,
            self.selected_project,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupError {
    PathsUnavailable,
    StorageUnavailable,
    ConfigurationUnavailable,
    AutomationRecovery,
    PermissionReset(PermissionResetError),
    WorkerUnavailable,
}
