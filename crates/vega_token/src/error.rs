use thiserror::Error;

/// Stable, content-free failures emitted by the pricing engine.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PricingError {
    #[error("pricing file exceeds the 1 MiB limit")]
    FileTooLarge,
    #[error("pricing catalog contains too many models")]
    TooManyModels,
    #[error("pricing input/output failed during {operation}")]
    Io { operation: &'static str },
    #[error("pricing JSON is malformed")]
    MalformedJson,
    #[error("pricing schema is invalid at {field}")]
    InvalidSchema { field: &'static str },
    #[error("pricing model id is invalid")]
    InvalidModelId,
    #[error("pricing model id is duplicated: {model}")]
    DuplicateModel { model: String },
    #[error("pricing model was not found: {model}")]
    ModelNotFound { model: String },
    #[error("pricing decimal is invalid at {field}")]
    InvalidDecimal { field: &'static str },
    #[error("pricing integer overflowed")]
    Overflow,
    #[error("cache_read exceeds total input tokens")]
    InvalidCacheUsage,
    #[error("model {model} supports standard pricing only through {max_tokens} input tokens")]
    UnsupportedInputLimit { model: String, max_tokens: u64 },
    #[error("pricing save target is not a private regular file")]
    UnsafeSaveTarget,
    #[error("pricing save target changed before commit")]
    SaveTargetChanged,
    #[error("pricing save committed, but directory durability could not be confirmed")]
    CommittedDurabilityUnknown,
}

impl PricingError {
    pub(crate) fn io(operation: &'static str) -> Self {
        Self::Io { operation }
    }
}
