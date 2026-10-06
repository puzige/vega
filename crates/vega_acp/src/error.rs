use thiserror::Error as ThisError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Configuration,
    Transport,
    Protocol,
    Capacity,
    Lifecycle,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum Error {
    #[error("invalid ACP launch configuration")]
    InvalidLaunchConfiguration,
    #[error("ACP transport failed")]
    TransportFailure,
    #[error("ACP connection closed")]
    ConnectionClosed,
    #[error("ACP connection interrupted")]
    Interrupted,
    #[error("ACP frame exceeds the configured limit")]
    FrameTooLarge,
    #[error("ACP frame ended before its newline delimiter")]
    TruncatedFrame,
    #[error("ACP frame is not valid UTF-8")]
    InvalidUtf8,
    #[error("ACP frame is not valid JSON")]
    MalformedJson,
    #[error("ACP JSON-RPC message is malformed")]
    MalformedMessage,
    #[error("ACP JSON-RPC batch must not be empty")]
    EmptyBatch,
    #[error("ACP JSON-RPC batch exceeds its element limit")]
    BatchTooLarge,
    #[error("ACP JSON-RPC request id is invalid")]
    InvalidRequestId,
    #[error("ACP JSON-RPC request id was reused")]
    DuplicateRequestId,
    #[error("ACP response references an unknown request id")]
    UnknownRequestId,
    #[error("ACP request limit reached")]
    TooManyPendingRequests,
    #[error("ACP permission request limit reached")]
    TooManyPendingPermissions,
    #[error("ACP inbound request ID history limit reached")]
    RequestIdHistoryFull,
    #[error("ACP event queue limit reached")]
    EventQueueOverflow,
    #[error("ACP writer queue limit reached")]
    WriterQueueFull,
    #[error("ACP permission request is no longer pending")]
    PermissionRequestClosed,
    #[error("ACP permission option was not offered")]
    InvalidPermissionOption,
    #[error("ACP connection has already been initialized")]
    AlreadyInitialized,
    #[error("ACP connection has not completed initialization")]
    NotInitialized,
    #[error("ACP protocol version {offered} is unsupported")]
    UnsupportedProtocolVersion { offered: u64 },
    #[error("ACP agent rejected a request with code {code}")]
    AgentRejected { code: i64 },
    #[error("ACP agent response is malformed")]
    InvalidResult,
    #[error("ACP request waiter was dropped before its response arrived")]
    RequestAbandoned,
    #[error("ACP prompt response has no stop reason")]
    MissingStopReason,
    #[error("ACP launch failed")]
    LaunchFailed,
}

impl Error {
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::InvalidLaunchConfiguration => ErrorCategory::Configuration,
            Self::TransportFailure
            | Self::ConnectionClosed
            | Self::FrameTooLarge
            | Self::TruncatedFrame
            | Self::InvalidUtf8 => ErrorCategory::Transport,
            Self::MalformedJson
            | Self::MalformedMessage
            | Self::EmptyBatch
            | Self::BatchTooLarge
            | Self::InvalidRequestId
            | Self::DuplicateRequestId
            | Self::UnknownRequestId
            | Self::UnsupportedProtocolVersion { .. }
            | Self::InvalidResult
            | Self::MissingStopReason => ErrorCategory::Protocol,
            Self::TooManyPendingRequests
            | Self::TooManyPendingPermissions
            | Self::RequestIdHistoryFull
            | Self::EventQueueOverflow
            | Self::WriterQueueFull => ErrorCategory::Capacity,
            Self::Interrupted
            | Self::RequestAbandoned
            | Self::PermissionRequestClosed
            | Self::AlreadyInitialized
            | Self::NotInitialized => ErrorCategory::Lifecycle,
            Self::InvalidPermissionOption | Self::AgentRejected { .. } => ErrorCategory::Agent,
            Self::LaunchFailed => ErrorCategory::Configuration,
        }
    }
}
