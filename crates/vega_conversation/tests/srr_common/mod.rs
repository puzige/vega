//! T46 (A2-17/A3-10) Stop / startup repair / explicit Resume E2E.
//!
//! Contract authority: docs/vega-s8-sdd.md §6 (C5, frozen). Card:
//! docs/vega-s8-tasks.md §T46. Evidence class: `E2E-REAL` (owned TempDir,
//! MockProvider at the provider/network boundary only, real file-backed
//! store, real `vega_store::recovery` path, real tools) — zero real keys,
//! zero network.
//!
//! Production entry under test: `run_thread_task_with_permission_sink` (the
//! exact chain the app worker drives: prepare_run with strict recovery →
//! runtime loop → persistence actor → sink). Stop is the production
//! ownership handle the app controller holds: one `CancellationToken`.
//!
//! p99 KPI measurement note (T46 vs T43): latency here is measured inside
//! the test with `std::time::Instant` from Stop request to run convergence
//! plus durable terminal rows. T43 instruments the production controller
//! ingress receive-to-render path; the two numbers are intentionally not
//! comparable and this suite does not depend on T43 instrumentation.

pub use std::error::Error;
pub use std::fs;
pub use std::path::{Path, PathBuf};
pub use std::process::{Command, Stdio};
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

pub type EventLog = Arc<Mutex<Vec<ConversationEvent>>>;
pub type MessageRows = Vec<(i64, String, String, String)>;
pub type ToolRow = (String, Option<String>, Option<String>, Option<i64>);

pub fn events_of(log: &EventLog) -> Vec<ConversationEvent> {
    log.lock().map(|events| events.clone()).unwrap_or_default()
}

pub fn interrupted_event_count(events: &[ConversationEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, ConversationEvent::Interrupted { .. }))
        .count()
}

#[allow(dead_code)]
pub fn durable_text(events: &[ConversationEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::TextDelta { delta, .. } => Some(delta.clone()),
            _ => None,
        })
        .collect()
}

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
        cache_read: 0,
        cache_write: 0,
    }
}

#[allow(dead_code)]
pub struct Fixture {
    pub _root: TempDir,
    pub db_path: PathBuf,
    pub repo: PathBuf,
    pub store: Store,
}

pub fn fixture() -> Result<Fixture, Box<dyn Error>> {
    let root = tempdir()?;
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir_all(&repo)?;
    fs::create_dir_all(&state)?;
    fs::write(repo.join("lib.rs"), "pub fn cached() {}\n// T46 sentinel\n")?;
    let db_path = state.join("vega.db");
    let store = Store::open(&db_path)?;
    store.migrate()?;
    Ok(Fixture {
        _root: root,
        db_path,
        repo,
        store,
    })
}

pub fn seed_thread(
    store: &Store,
    project_id: &str,
    permission_mode: &str,
) -> Result<(), Box<dyn Error>> {
    threads::create(
        store.conn(),
        threads::NewThread {
            id: THREAD_ID,
            project_id,
            title: "T46 stop journey",
            mode: "execute",
            permission_mode,
            model: MODEL,
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )?;
    Ok(())
}

pub fn seed_project(store: &Store, path: &Path) -> Result<String, Box<dyn Error>> {
    let project = projects::create(
        store.conn(),
        &path.to_string_lossy(),
        "t46-fixture",
        Some("master"),
    )?;
    Ok(project.id)
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

pub fn tool_row(store: &Store, call_id: &str) -> Result<Option<ToolRow>, Box<dyn Error>> {
    let mut statement = store.conn().prepare(
        "SELECT status, output_text, approval, finished_at FROM tool_calls \
         WHERE id = ?1 AND thread_id = ?2",
    )?;
    let mut rows = statement.query_map([call_id, THREAD_ID], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<i64>>(3)?,
        ))
    })?;
    Ok(rows.next().transpose()?)
}

#[allow(dead_code)]
pub fn tool_row_count(store: &Store) -> Result<i64, Box<dyn Error>> {
    Ok(store.conn().query_row(
        "SELECT COUNT(*) FROM tool_calls WHERE thread_id = ?1",
        [THREAD_ID],
        |row| row.get(0),
    )?)
}

pub fn assert_message_terminal(
    rows: &[(i64, String, String, String)],
    assistant_content: &str,
    status: &str,
) {
    assert_eq!(rows.len(), 2, "exactly one user + one assistant row");
    assert_eq!(rows[0].1, "user");
    assert_eq!(rows[0].3, "done");
    assert_eq!(rows[1].1, "assistant");
    assert_eq!(rows[1].2, assistant_content, "durable partial text");
    assert_eq!(rows[1].3, status);
}

#[allow(dead_code)]
pub fn process_is_gone(pid: u32) -> bool {
    !Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("kill -0 probe")
        .success()
}

// ---------------------------------------------------------------------------
// Journey E2E: partial text + permission wait → production Stop.
// ---------------------------------------------------------------------------
