mod connection;
mod error;
mod framing;
mod protocol;

#[cfg(test)]
pub(crate) use connection::ChildControl;
pub use connection::{
    Connection, Event, InitializeResult, LaunchConfig, PermissionOption, PermissionOutcome,
    PermissionRequest, PromptHandle, PromptResult, SessionInfo,
};
pub use error::{Error, ErrorCategory};
pub use protocol::RequestId;

pub const MAX_FRAME_BYTES: usize = 1_048_576;
pub const MAX_BATCH_ELEMENTS: usize = 16;
pub const READ_SCRATCH_BYTES: usize = 8_192;
pub const MAX_PENDING_OUTBOUND: usize = 16;
pub const MAX_PENDING_INBOUND: usize = 16;
pub const MAX_SEEN_INBOUND_REQUEST_IDS: usize = 4_096;
pub const MAX_SEEN_INBOUND_REQUEST_ID_BYTES: usize = 1_048_576;
pub const MAX_EVENT_COUNT: usize = 128;
pub const MAX_EVENT_BYTES: usize = 4_194_304;
pub const EVENT_WEIGHT_BYTES: usize = 4_096;

#[cfg(test)]
mod tests;
