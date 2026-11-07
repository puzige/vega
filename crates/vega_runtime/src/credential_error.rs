use crate::{RuntimeFailureDetail, RuntimeFailureReason, RuntimeFailureStage, VegaError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialReadStage {
    Provider,
    Bindings,
    Mcp,
    Privacy,
    Authority,
    Worker,
}

impl CredentialReadStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Bindings => "bindings",
            Self::Mcp => "mcp",
            Self::Privacy => "privacy",
            Self::Authority => "authority",
            Self::Worker => "worker",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialReadReason {
    Missing,
    Unsafe,
    Malformed,
    Limit,
    CredentialIo,
    Unsupported,
    Database,
    Configuration,
    ConfigurationPoisoned,
    ConfigurationDirectory,
    ConfigurationLockOpen,
    ConfigurationLockValidation,
    ConfigurationLockAcquire,
    ConfigurationRead,
    ConfigurationParse,
    State,
    Worker,
}

impl CredentialReadReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Unsafe => "unsafe",
            Self::Malformed => "malformed",
            Self::Limit => "limit",
            Self::CredentialIo => "credential_io",
            Self::Unsupported => "unsupported",
            Self::Database => "database",
            Self::Configuration => "configuration",
            Self::ConfigurationPoisoned => "configuration_poisoned",
            Self::ConfigurationDirectory => "configuration_directory",
            Self::ConfigurationLockOpen => "configuration_lock_open",
            Self::ConfigurationLockValidation => "configuration_lock_validation",
            Self::ConfigurationLockAcquire => "configuration_lock_acquire",
            Self::ConfigurationRead => "configuration_read",
            Self::ConfigurationParse => "configuration_parse",
            Self::State => "state",
            Self::Worker => "worker",
        }
    }

    pub fn needs_credential_settings(self) -> bool {
        matches!(
            self,
            Self::Missing
                | Self::Unsafe
                | Self::Malformed
                | Self::Limit
                | Self::CredentialIo
                | Self::Unsupported
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("credential safety check unavailable ({stage:?}, {reason:?})")]
pub struct CredentialReadError {
    pub stage: CredentialReadStage,
    pub reason: CredentialReadReason,
    pub storage: Option<RuntimeFailureDetail>,
    pub configuration_lock: Option<&'static str>,
    pub configuration_lock_attempts: Option<u64>,
    pub configuration_lock_wait_ms: Option<u64>,
}

impl CredentialReadError {
    pub fn new(stage: CredentialReadStage, reason: CredentialReadReason) -> Self {
        Self {
            stage,
            reason,
            storage: None,
            configuration_lock: None,
            configuration_lock_attempts: None,
            configuration_lock_wait_ms: None,
        }
    }

    pub fn database(stage: CredentialReadStage, error: rusqlite::Error) -> Self {
        let mut detail = RuntimeFailureDetail::new(RuntimeFailureReason::Database);
        detail.stage = RuntimeFailureStage::RequestPreparation;
        if let Some(code) = error.sqlite_error() {
            detail.sqlite_code = Some(code.extended_code & 255);
            detail.sqlite_extended_code = Some(code.extended_code);
        }
        Self {
            stage,
            reason: CredentialReadReason::Database,
            storage: Some(detail),
            configuration_lock: None,
            configuration_lock_attempts: None,
            configuration_lock_wait_ms: None,
        }
    }

    pub fn configuration_io(stage: CredentialReadStage, kind: std::io::ErrorKind) -> Self {
        let mut detail = RuntimeFailureDetail::new(RuntimeFailureReason::Io);
        detail.stage = RuntimeFailureStage::RequestPreparation;
        detail.io_kind = Some(kind.into());
        Self {
            stage,
            reason: CredentialReadReason::Configuration,
            storage: Some(detail),
            configuration_lock: None,
            configuration_lock_attempts: None,
            configuration_lock_wait_ms: None,
        }
    }
}

impl From<CredentialReadError> for VegaError {
    fn from(error: CredentialReadError) -> Self {
        Self::CredentialRead(error)
    }
}
