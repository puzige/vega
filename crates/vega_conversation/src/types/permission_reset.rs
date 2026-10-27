pub struct PermissionResetIntent {
    pub(crate) database: super::DatabaseIdentity,
    pub(crate) authority_epoch: String,
    pub(crate) source: vega_store::permission_reset::ResetConfigSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionResetReceipt {
    pub reset_id: String,
    pub authority_epoch: String,
    pub default_permission_revision: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionResetError {
    Unavailable,
    Busy,
    UnsettledWork,
    ConfigurationConflict,
    ConfigurationUnavailable,
    OutcomeUnknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PermissionResetProjection {
    #[default]
    Idle,
    Preparing,
    Confirming,
    Pausing,
    Stopping,
    Refreshing,
    Blocked {
        error: PermissionResetError,
        can_cancel: bool,
        thread_id: Option<String>,
    },
    Completed,
}

impl PermissionResetProjection {
    pub fn authority_blocked(&self) -> bool {
        matches!(
            self,
            Self::Pausing | Self::Stopping | Self::Refreshing | Self::Blocked { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionResetAction {
    Prepare,
    Confirm,
    Cancel,
    Retry,
    Inspect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermissionResetRequested(pub PermissionResetAction);

pub struct PermissionResetThreadProjection {
    pub thread: super::Thread,
    pub pending: Vec<super::PendingMessage>,
    pub automation: super::AutomationProjection,
}

pub struct PermissionResetRefresh {
    pub database: super::DatabaseIdentity,
    pub config: vega_store::config::AppConfig,
    pub threads: Vec<PermissionResetThreadProjection>,
}
