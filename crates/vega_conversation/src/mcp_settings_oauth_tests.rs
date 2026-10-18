use super::*;
use crate::agent::{
    PermissionHook, PersistenceActorConfig, run_thread_task_with_images_reasoning_and_mcp,
};
use crate::types::{McpCallIdentity, PermissionDecision, PermissionRequest};
use futures::future::BoxFuture;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Notify;
use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason, VegaError};

struct ApproveOwnedMcp;

impl PermissionHook for ApproveOwnedMcp {
    fn request(
        &self,
        _request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        Box::pin(async { Ok(PermissionDecision::Once) })
    }
}
