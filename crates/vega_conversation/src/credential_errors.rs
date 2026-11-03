use vega_runtime::{CredentialReadError, CredentialReadReason, CredentialReadStage};

pub(crate) fn credential_error(
    stage: CredentialReadStage,
    error: vega_store::keystore::Error,
) -> CredentialReadError {
    use vega_store::keystore::Error;
    let reason = match error {
        Error::Missing => CredentialReadReason::Missing,
        Error::Io => CredentialReadReason::CredentialIo,
        Error::Malformed => CredentialReadReason::Malformed,
        Error::Limit => CredentialReadReason::Limit,
        Error::Unsafe => CredentialReadReason::Unsafe,
        Error::Unsupported => CredentialReadReason::Unsupported,
    };
    CredentialReadError::new(stage, reason)
}

pub fn provider_credential_error(error: vega_store::keystore::Error) -> CredentialReadError {
    credential_error(CredentialReadStage::Provider, error)
}

pub(crate) fn state_error(stage: CredentialReadStage) -> CredentialReadError {
    CredentialReadError::new(stage, CredentialReadReason::State)
}

impl From<CredentialReadError> for crate::types::ConversationError {
    fn from(error: CredentialReadError) -> Self {
        Self::Runtime(std::sync::Arc::new(error.into()))
    }
}
