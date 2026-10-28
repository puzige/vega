use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultPermissionSnapshot {
    pub config_path: Option<std::path::PathBuf>,
    pub configuration_id: Option<String>,
    pub permission_mode: String,
    pub permission_revision: i64,
    pub full_access_grant: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultPermissionValues {
    pub permission_mode: String,
    pub permission_revision: i64,
    pub full_access_grant: Option<String>,
}

impl From<&vega_store::config::Defaults> for DefaultPermissionValues {
    fn from(defaults: &vega_store::config::Defaults) -> Self {
        Self {
            permission_mode: defaults.permission_mode.clone(),
            permission_revision: defaults.permission_revision,
            full_access_grant: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultPermissionChange {
    pub(crate) source: DefaultPermissionSnapshot,
    pub(crate) database_identity: String,
    pub(crate) authority_epoch: String,
    pub(crate) mode: super::PermissionMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultPermissionError {
    Unavailable,
    Conflict,
    ResetPending,
    ConfirmationRequired,
    OutcomeUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadPermissionChange {
    pub(crate) source: super::Thread,
    pub(crate) database: super::DatabaseIdentity,
    pub(crate) authority_epoch: String,
    pub(crate) mode: super::PermissionMode,
    pub(crate) draft: bool,
}

impl ThreadPermissionChange {
    pub fn source(&self) -> &super::Thread {
        &self.source
    }

    pub fn mode(&self) -> super::PermissionMode {
        self.mode
    }

    pub fn is_draft(&self) -> bool {
        self.draft
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadPermissionError {
    Unavailable,
    Conflict,
    ResetPending,
    ConfirmationRequired,
    OutcomeUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadPermissionReloadRequested {
    pub operation: u64,
    pub source: super::Thread,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FullAccessScope {
    Thread { thread_id: String },
    FutureThreads { configuration_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FullAccessOrigin {
    UserConfirmation,
    DefaultGrant { grant_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FullAccessGrant {
    schema_version: u32,
    contract_version: u32,
    grant_id: String,
    authority_epoch: String,
    scope: FullAccessScope,
    origin: FullAccessOrigin,
    issued_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionFence {
    pub schema_version: u32,
    pub execution_policy_version: u32,
    pub authority_epoch: String,
    pub database_identity: String,
    pub thread_id: String,
    pub project_id: String,
    pub workspace: String,
    pub run_mode: String,
    pub permission_mode: String,
    pub permission_revision: i64,
    pub exact_rules: Vec<(String, String)>,
    #[serde(deserialize_with = "required_grant")]
    pub full_access_grant: Option<FullAccessGrant>,
    #[serde(deserialize_with = "required_delegation")]
    pub delegation: Option<DelegatedPermissionSource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegatedPermissionSource {
    pub parent_message_id: String,
    pub child_message_id: String,
    pub parent_fence: Box<PermissionFence>,
}

impl PermissionFence {
    pub fn is_valid(&self) -> bool {
        self.schema_version == SCHEMA_VERSION
            && self.execution_policy_version == 2
            && valid_id(&self.authority_epoch)
            && valid_id(&self.thread_id)
            && !self.database_identity.is_empty()
            && self.permission_revision >= 0
            && std::path::Path::new(&self.workspace).is_absolute()
            && super::ThreadMode::parse(&self.run_mode).is_some()
            && if let Some(source) = &self.delegation {
                let parent = &source.parent_fence;
                parent.delegation.is_none()
                    && parent.is_valid()
                    && valid_id(&source.parent_message_id)
                    && valid_id(&source.child_message_id)
                    && source.parent_message_id != source.child_message_id
                    && parent.thread_id != self.thread_id
                    && parent.authority_epoch == self.authority_epoch
                    && parent.database_identity == self.database_identity
                    && parent.project_id == self.project_id
                    && parent.workspace == self.workspace
                    && parent.run_mode == self.run_mode
                    && parent.permission_mode == self.permission_mode
                    && parent.exact_rules == self.exact_rules
                    && self.full_access_grant.is_none()
            } else {
                super::PermissionMode::parse(&self.permission_mode).is_some()
                    && self.full_access_grant.is_none()
            }
    }
}

fn required_delegation<'de, D>(
    deserializer: D,
) -> Result<Option<DelegatedPermissionSource>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<DelegatedPermissionSource>::deserialize(deserializer)
}

fn required_grant<'de, D>(deserializer: D) -> Result<Option<FullAccessGrant>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<FullAccessGrant>::deserialize(deserializer)
}

fn valid_id(value: &str) -> bool {
    ulid::Ulid::from_string(value).is_ok()
}
