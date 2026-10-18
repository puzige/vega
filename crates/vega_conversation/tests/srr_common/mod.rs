pub use std::error::Error;
pub use std::fs;
pub use std::path::{Path, PathBuf};
pub use std::sync::{Arc, Mutex};
pub use std::time::{Duration, Instant};

pub use futures::future::BoxFuture;
pub use tempfile::{TempDir, tempdir};
pub use tokio_util::sync::CancellationToken;
pub use vega_conversation::agent::{PermissionHook, run_thread_task_with_permission_sink};
pub use vega_conversation::types::{
    Approval, ApprovalAudit, ApprovalSource, ConversationEvent, PermissionDecision,
};
pub use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason, VegaError};
pub use vega_store::{Store, projects, threads};

pub const THREAD_ID: &str = "t46-stop-thread";
pub const MODEL: &str = "mock-t46";
pub const WRITE_BODY: &str = "T46_WRITE_BODY_SENTINEL";

pub type MessageRows = Vec<(i64, String, String, String)>;

/// Scripted permission hook: parks on the runtime child token like the
/// production `PermissionQueue` (cancel resolves it as Timeout) and records
/// every content-free request for assertions.
#[derive(Clone, Default)]
pub struct ParkingPermissionHook {
    pub requests: Arc<Mutex<Vec<String>>>,
}

impl PermissionHook for ParkingPermissionHook {
    fn request(
        &self,
        request: vega_conversation::types::PermissionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        if let Ok(mut requests) = self.requests.lock() {
            requests.push(request.tool);
        }
        Box::pin(async move {
            cancel.cancelled().await;
            Ok(PermissionDecision::Timeout)
        })
    }
}

pub fn usage(input: u64, output: u64) -> ProviderEvent {
    ProviderEvent::Usage {
        input,
        output,
        cache_read: Some(0),
        cache_write: Some(0),
    }
}

#[allow(dead_code)]
pub struct Fixture {
    pub _root: TempDir,
    pub db_path: PathBuf,
    pub repo: PathBuf,
    pub store: Store,
}

pub fn message_rows(store: &Store) -> Result<MessageRows, Box<dyn Error>> {
    let mut statement = store.conn().prepare(
        "SELECT seq, role, content, status FROM messages \
         WHERE thread_id = ?1 ORDER BY seq",
    )?;
    let rows = statement
        .query_map([THREAD_ID], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Journey E2E: partial text + permission wait → production Stop.
// ---------------------------------------------------------------------------
