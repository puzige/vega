//! Conversation-layer orchestration for the headless runtime (S4-T20).

use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::FutureExt;
use futures::future::BoxFuture;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    AgentRequest, Provider, RuntimeEvent, RuntimeExactRule, RuntimeMutatingTool,
    RuntimePermissionHook, RuntimePermissionMode, RuntimeRunMode, RuntimeToolConfig,
    RuntimeToolStatus, RuntimeUserDecision, VegaError, run_agent_with_permission_sink,
};
use vega_store::{Store, messages, permissions, token_usage, tool_calls};

use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, ConversationError, ConversationEvent,
    PermissionDecision, PermissionRequest, ThreadMode, approval_audit_from_runtime,
    approval_audit_to_runtime, from_runtime_event, permission_decision_to_runtime,
    permission_request_from_runtime,
};

const HISTORY_WINDOW: usize = 50;
const TEXT_BATCH_MAX_DELAY: Duration = Duration::from_millis(4);
const TEXT_BATCH_MAX_BYTES: usize = 4 * 1024;
const PERSISTENCE_CHANNEL_CAPACITY: usize = 64;

/// Authoritative production permission-card lifetime.
pub const PERMISSION_TIMEOUT: Duration = vega_runtime::PERMISSION_TIMEOUT;

/// Shared cancellable permission boundary implemented by the S5 UI.
pub trait PermissionHook: Send + Sync {
    /// Requests one content-free permission decision.
    fn request(
        &self,
        request: PermissionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>>;
}

mod entry;
mod events;
mod permission_queue;
mod persistence;
mod pipeline;

#[cfg(test)]
mod tests;

pub use entry::*;
pub(crate) use events::*;
pub use permission_queue::*;
pub use persistence::PersistenceActorConfig;
pub(crate) use persistence::*;
pub(crate) use pipeline::*;
