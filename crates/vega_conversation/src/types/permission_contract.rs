use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;
const CONTRACT_VERSION: u32 = 1;

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
            full_access_grant: defaults.full_access_grant.clone(),
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

impl FullAccessGrant {
    pub fn confirmed(
        authority_epoch: &str,
        scope: FullAccessScope,
        issued_at: i64,
    ) -> Option<Self> {
        let grant = Self {
            schema_version: SCHEMA_VERSION,
            contract_version: CONTRACT_VERSION,
            grant_id: ulid::Ulid::generate().to_string(),
            authority_epoch: authority_epoch.to_owned(),
            scope,
            origin: FullAccessOrigin::UserConfirmation,
            issued_at,
        };
        grant.is_valid().then_some(grant)
    }

    pub fn derive_thread(&self, thread_id: &str, issued_at: i64) -> Option<Self> {
        if !self.is_valid() || !matches!(self.scope, FullAccessScope::FutureThreads { .. }) {
            return None;
        }
        let mut grant = Self::confirmed(
            &self.authority_epoch,
            FullAccessScope::Thread {
                thread_id: thread_id.to_owned(),
            },
            issued_at,
        )?;
        grant.origin = FullAccessOrigin::DefaultGrant {
            grant_id: self.grant_id.clone(),
        };
        Some(grant)
    }

    pub fn verifies(&self, authority_epoch: &str, scope: &FullAccessScope) -> bool {
        self.is_valid() && self.authority_epoch == authority_epoch && &self.scope == scope
    }

    pub fn identity(&self) -> &str {
        &self.grant_id
    }

    pub fn is_valid(&self) -> bool {
        self.schema_version == SCHEMA_VERSION
            && self.contract_version == CONTRACT_VERSION
            && valid_id(&self.grant_id)
            && valid_id(&self.authority_epoch)
            && match &self.scope {
                FullAccessScope::Thread { thread_id } => valid_id(thread_id),
                FullAccessScope::FutureThreads { configuration_id } => {
                    !configuration_id.is_empty()
                        && configuration_id.len() <= 4096
                        && !configuration_id.chars().any(char::is_control)
                }
            }
            && match &self.origin {
                FullAccessOrigin::UserConfirmation => true,
                FullAccessOrigin::DefaultGrant { grant_id } => {
                    valid_id(grant_id)
                        && grant_id != &self.grant_id
                        && matches!(self.scope, FullAccessScope::Thread { .. })
                }
            }
    }
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
            && self.execution_policy_version == CONTRACT_VERSION
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
                match super::PermissionMode::parse(&self.permission_mode) {
                    Some(super::PermissionMode::FullAccess) => {
                        self.full_access_grant.as_ref().is_some_and(|grant| {
                            grant.verifies(
                                &self.authority_epoch,
                                &FullAccessScope::Thread {
                                    thread_id: self.thread_id.clone(),
                                },
                            )
                        })
                    }
                    Some(_) => self.full_access_grant.is_none(),
                    None => false,
                }
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
