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
