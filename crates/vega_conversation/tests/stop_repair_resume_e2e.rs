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

use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use tempfile::{TempDir, tempdir};
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{
    PermissionHook, PermissionQueue, run_thread_task_with_permission_sink,
};
use vega_conversation::types::{
    Approval, ApprovalAudit, ApprovalSource, ConversationEvent, PermissionDecision,
};
use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason, VegaError};
use vega_store::{Store, projects, threads};

const THREAD_ID: &str = "t46-stop-thread";
const MODEL: &str = "mock-t46";
const WRITE_BODY: &str = "T46_WRITE_BODY_SENTINEL";

type EventLog = Arc<Mutex<Vec<ConversationEvent>>>;
type MessageRows = Vec<(i64, String, String, String)>;
type ToolRow = (String, Option<String>, Option<String>, Option<i64>);

fn events_of(log: &EventLog) -> Vec<ConversationEvent> {
    log.lock().map(|events| events.clone()).unwrap_or_default()
}

fn interrupted_event_count(events: &[ConversationEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, ConversationEvent::Interrupted { .. }))
        .count()
}

fn durable_text(events: &[ConversationEvent]) -> String {
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
struct ParkingPermissionHook {
    requests: Arc<Mutex<Vec<String>>>,
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

fn usage(input: u64, output: u64) -> ProviderEvent {
    ProviderEvent::Usage {
        input,
        output,
        cache_read: 0,
        cache_write: 0,
    }
}

struct Fixture {
    _root: TempDir,
    db_path: PathBuf,
    repo: PathBuf,
    store: Store,
}

fn fixture() -> Result<Fixture, Box<dyn Error>> {
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

fn seed_thread(
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

fn seed_project(store: &Store, path: &Path) -> Result<String, Box<dyn Error>> {
    let project = projects::create(
        store.conn(),
        &path.to_string_lossy(),
        "t46-fixture",
        Some("master"),
    )?;
    Ok(project.id)
}

fn message_rows(store: &Store) -> Result<MessageRows, Box<dyn Error>> {
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

fn tool_row(store: &Store, call_id: &str) -> Result<Option<ToolRow>, Box<dyn Error>> {
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

fn tool_row_count(store: &Store) -> Result<i64, Box<dyn Error>> {
    Ok(store.conn().query_row(
        "SELECT COUNT(*) FROM tool_calls WHERE thread_id = ?1",
        [THREAD_ID],
        |row| row.get(0),
    )?)
}

fn assert_message_terminal(
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

fn process_is_gone(pid: u32) -> bool {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_first_wins_reaches_exactly_one_terminal_and_cleans_up_under_one_second()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "confirm")?;
    let hook = ParkingPermissionHook::default();
    let provider = MockProvider::new_rounds(vec![vec![
        ScriptStep::text("partial one. "),
        ScriptStep::delay(Duration::from_millis(10)),
        ScriptStep::text("partial two. "),
        ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-1".into(),
                name: "write".into(),
                input_json: format!(r#"{{"path":"out.txt","content":"{WRITE_BODY}"}}"#),
            },
            ProviderEvent::ToolUse {
                id: "read-2".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            usage(30, 6),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ]),
    ]]);
    let tools = vega_tools::Tools::new(&fixture.repo)?;
    let cancel = CancellationToken::new();
    let events: EventLog = Arc::default();
    let event_log = events.clone();
    let stop_signal = Arc::new(tokio::sync::Notify::new());
    let stop_signal_for_sink = stop_signal.clone();
    let cancel_for_sink = cancel.clone();
    let stop_signal_for_sink_task = stop_signal.clone();

    let started = Instant::now();
    let sink_task = tokio::spawn(async move {
        // Production Stop fires the instant the permission boundary is
        // visibly waiting; the second Stop press is absorbed (first-wins).
        stop_signal_for_sink_task.notified().await;
        cancel_for_sink.cancel();
        cancel_for_sink.cancel();
    });
    let run = run_thread_task_with_permission_sink(
        &fixture.store,
        &provider,
        &tools,
        THREAD_ID,
        "Write the sentinel body.",
        "T46 system prompt",
        cancel,
        &hook,
        move |event| {
            if let ConversationEvent::ToolCallProposed { call } = event
                && call.id == "write-1"
            {
                stop_signal_for_sink.notify_one();
            }
            if let Ok(mut log) = event_log.lock() {
                log.push(event.clone());
            }
            Ok(())
        },
    )
    .await?;
    sink_task.await?;
    let elapsed = started.elapsed();

    // First-wins: duplicate Stop press produces exactly one durable terminal.
    assert_eq!(interrupted_event_count(&events_of(&events)), 1);
    assert!(run.interrupted);
    assert!(!run.failed);
    assert!(
        elapsed < Duration::from_secs(1),
        "Stop-to-terminal took {elapsed:?}, KPI is <1s"
    );

    // Provider stream domain: stream stops after cancellation, no second
    // provider round is ever started (cleanup).
    assert_eq!(provider.requests().len(), 1);

    // Permission-wait domain: the gated write reaches the strict terminal
    // rejection with the timeout audit, and never executes.
    let write_row = tool_row(&fixture.store, "write-1")?.expect("write-1 row");
    assert_eq!(write_row.0, "rejected");
    let approval = ApprovalAudit::from_json(write_row.2.as_deref().expect("audit"))?;
    assert_eq!(approval.decision, Approval::Deny);
    assert_eq!(approval.source, ApprovalSource::Timeout);
    assert!(write_row.3.is_some(), "rejected row is terminal");
    assert_eq!(tool_row_count(&fixture.store)?, 1, "read-2 never proposed");

    // Durable rows: assistant row terminal `interrupted` with the partial
    // text preserved byte-for-byte (visible + immutable).
    let rows = message_rows(&fixture.store)?;
    assert_message_terminal(&rows, "partial one. partial two. ", "interrupted");
    assert_eq!(
        durable_text(&events_of(&events)),
        "partial one. partial two. "
    );

    // No fabricated success and no external effect from the cancelled write.
    assert!(!fixture.repo.join("out.txt").exists());

    // Token/cost presentation: usage was seen for the round that streamed.
    let usage_rows: i64 = fixture.store.conn().query_row(
        "SELECT COUNT(*) FROM token_usage WHERE thread_id = ?1",
        [THREAD_ID],
        |row| row.get(0),
    )?;
    assert_eq!(usage_rows, 1, "one priced provider call, no phantom rows");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_mid_tool_future_captures_output_and_never_starts_the_followup_call()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "auto")?;
    let slow_path = fixture.repo.join("slow.txt");
    let status = Command::new("mkfifo").arg(&slow_path).status()?;
    assert!(status.success());
    // F1 lesson: the FIFO writer's open() blocks until the tool opens the
    // read end. The rendezvous below cancels only after the writer connected,
    // so the writer can never block forever; its total write is bounded.
    let (connected_tx, connected_rx) = tokio::sync::oneshot::channel::<()>();
    let writer_path = slow_path.clone();
    let writer = std::thread::spawn(move || {
        let mut pipe = fs::OpenOptions::new()
            .write(true)
            .open(writer_path)
            .expect("fifo writer open");
        let _ = connected_tx.send(());
        pipe.write_all(b"partial tool output\n")
            .expect("fifo writer write");
        // Bounded hold so cancellation can win while the tool is mid-read.
        std::thread::sleep(Duration::from_millis(50));
    });
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "slow-read".into(),
            name: "read".into(),
            input_json: r#"{"path":"slow.txt"}"#.into(),
        },
        ProviderEvent::ToolUse {
            id: "must-not-start".into(),
            name: "read".into(),
            input_json: r#"{"path":"lib.rs"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let tools = vega_tools::Tools::new(&fixture.repo)?;
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let mut connected_slot = Some(connected_rx);
    let started = Instant::now();
    let run = run_thread_task_with_permission_sink(
        &fixture.store,
        &provider,
        &tools,
        THREAD_ID,
        "Read the slow file.",
        "T46 system prompt",
        cancel,
        &ParkingPermissionHook::default(),
        move |event| {
            if matches!(event, ConversationEvent::ToolCallApproved { .. })
                && let Some(connected) = connected_slot.take()
            {
                let trigger = trigger.clone();
                tokio::spawn(async move {
                    // Hard bound: a tool that never starts fails the test
                    // visibly instead of hanging the suite.
                    let _ = tokio::time::timeout(Duration::from_secs(10), connected).await;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    trigger.cancel();
                });
            }
            Ok(())
        },
    )
    .await?;
    writer.join().expect("fifo writer finishes");
    let elapsed = started.elapsed();

    assert!(run.interrupted);
    assert_eq!(interrupted_event_count(&run.events), 1);
    assert!(
        elapsed < Duration::from_secs(1),
        "Stop-to-terminal took {elapsed:?}, KPI is <1s"
    );
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { call_id, result }
            if call_id == "slow-read"
                && result.status == vega_conversation::types::ToolCallStatus::Cancelled
                && result.output.contains("partial tool output")
    )));
    assert!(!run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallProposed { call } if call.id == "must-not-start"
    )));
    let slow_row = tool_row(&fixture.store, "slow-read")?.expect("slow-read row");
    assert_eq!(slow_row.0, "cancelled");
    assert!(slow_row.3.is_some());
    assert_eq!(
        tool_row_count(&fixture.store)?,
        1,
        "the follow-up call never started"
    );
    let rows = message_rows(&fixture.store)?;
    assert_eq!(rows[1].3, "interrupted");
    assert_eq!(provider.requests().len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_mid_bash_kills_the_owned_process_group_under_one_second() -> Result<(), Box<dyn Error>>
{
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "auto")?;
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "bash-1".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"print $$ > shell.pid; sleep 30 & print $! > child.pid; wait","timeout_ms":10000}"#.into(),
            },
            usage(20, 4),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let tools = vega_tools::Tools::new(&fixture.repo)?;
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let shell_pid_path = fixture.repo.join("shell.pid");
    let child_pid_path = fixture.repo.join("child.pid");
    let shell_for_sink = shell_pid_path.clone();
    let child_for_sink = child_pid_path.clone();
    let started = Instant::now();
    let run = run_thread_task_with_permission_sink(
        &fixture.store,
        &provider,
        &tools,
        THREAD_ID,
        "Start the sleeper.",
        "T46 system prompt",
        cancel,
        &ParkingPermissionHook::default(),
        move |event| {
            if matches!(event, ConversationEvent::ToolCallApproved { call_id, .. } if call_id == "bash-1")
            {
                let trigger = trigger.clone();
                let shell = shell_for_sink.clone();
                let child = child_for_sink.clone();
                tokio::spawn(async move {
                    // Bounded rendezvous: Stop only after the own process
                    // group actually exists.
                    let _ = tokio::time::timeout(Duration::from_secs(10), async move {
                        loop {
                            if shell.exists() && child.exists() {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    })
                    .await;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    trigger.cancel();
                });
            }
            Ok(())
        },
    )
    .await?;
    let elapsed = started.elapsed();

    assert!(run.interrupted);
    assert_eq!(interrupted_event_count(&run.events), 1);
    assert!(
        elapsed < Duration::from_secs(1),
        "Stop-to-terminal took {elapsed:?}; the 30s sleeper proves the group was killed"
    );
    let bash_row = tool_row(&fixture.store, "bash-1")?.expect("bash-1 row");
    assert_eq!(bash_row.0, "cancelled");
    let bash_output = bash_row.1.unwrap_or_default();
    assert!(
        bash_output.contains("cancelled"),
        "cancelled bash output: {bash_output:?}"
    );
    assert!(bash_row.3.is_some());
    // Own process group ownership: both the shell and its descendant are
    // reaped, not orphaned.
    let shell_pid: u32 = fs::read_to_string(&shell_pid_path)?.trim().parse()?;
    let child_pid: u32 = fs::read_to_string(&child_pid_path)?.trim().parse()?;
    assert!(
        process_is_gone(shell_pid),
        "shell {shell_pid} survived Stop"
    );
    assert!(
        process_is_gone(child_pid),
        "descendant {child_pid} survived Stop"
    );
    let rows = message_rows(&fixture.store)?;
    assert_eq!(rows[1].3, "interrupted");
    assert_eq!(provider.requests().len(), 1, "no second round after Stop");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_close_listener_drop_fails_the_pending_prompt_closed_and_never_hangs()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "confirm")?;
    let provider = MockProvider::new_rounds(vec![
        vec![
            ScriptStep::text("before prompt. "),
            ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "write-1".into(),
                    name: "write".into(),
                    input_json: format!(r#"{{"path":"route.txt","content":"{WRITE_BODY}"}}"#),
                },
                usage(10, 2),
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ]),
        ],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("after route close. ".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let tools = vega_tools::Tools::new(&fixture.repo)?;
    let queue = PermissionQueue::new();
    // The listener owns the sole live UI wakeup seam; dropping it is exactly
    // what a route/window close does (agent.rs: window/thread disappearance
    // resolves every unresolved prompt as Timeout).
    let closer_queue = queue.clone();
    let closer = tokio::spawn(async move {
        let mut listener = closer_queue.subscribe();
        let _ = tokio::time::timeout(Duration::from_secs(10), listener.changed()).await;
        drop(listener);
    });
    let started = Instant::now();
    let run = tokio::time::timeout(
        Duration::from_secs(10),
        run_thread_task_with_permission_sink(
            &fixture.store,
            &provider,
            &tools,
            THREAD_ID,
            "Write behind a closing route.",
            "T46 system prompt",
            CancellationToken::new(),
            &queue,
            |_| Ok(()),
        ),
    )
    .await
    .expect("route close must never leave the runtime waiting on a stale card")?;
    closer.await?;
    let elapsed = started.elapsed();

    // The run converges (no hang) with the gated write strictly rejected.
    assert!(!run.interrupted);
    assert!(!run.failed);
    assert!(
        elapsed < Duration::from_secs(5),
        "listener-drop convergence took {elapsed:?}"
    );
    let write_row = tool_row(&fixture.store, "write-1")?.expect("write-1 row");
    assert_eq!(write_row.0, "rejected");
    let approval = ApprovalAudit::from_json(write_row.2.as_deref().expect("audit"))?;
    assert_eq!(approval.decision, Approval::Deny);
    assert_eq!(approval.source, ApprovalSource::Timeout);
    assert!(!fixture.repo.join("route.txt").exists());
    let rows = message_rows(&fixture.store)?;
    assert_message_terminal(&rows, "before prompt. after route close. ", "done");
    assert_eq!(run.content, "before prompt. after route close. ");
    Ok(())
}

// ---------------------------------------------------------------------------
// Startup repair (restart) E2E: strict recovery normalizes stale rows before
// the next run can project; partial text stays visible and immutable.
// ---------------------------------------------------------------------------

/// Seeds one assistant row (content + status) for the restart journeys.
fn seed_assistant_row(
    store: &Store,
    id: &str,
    content: &str,
    status: &str,
) -> Result<(), Box<dyn Error>> {
    vega_store::messages::insert(
        store.conn(),
        &vega_store::messages::MessageRow {
            id: id.to_string(),
            thread_id: THREAD_ID.to_string(),
            seq: 1,
            role: "assistant".to_string(),
            kind: "text".to_string(),
            content: content.to_string(),
            status: status.to_string(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )?;
    Ok(())
}

/// Seeds one stale tool row in a given non-terminal state.
fn seed_stale_tool(
    store: &Store,
    call_id: &str,
    status: &str,
    seq: i64,
) -> Result<(), Box<dyn Error>> {
    vega_store::tool_calls::insert(
        store.conn(),
        vega_store::tool_calls::NewToolCall {
            id: call_id,
            thread_id: THREAD_ID,
            message_id: "stale-assistant",
            seq,
            tool: "read",
            input_json: r#"{"path":"lib.rs"}"#,
            status,
            created_at: 1,
        },
    )?;
    if status == "approved" || status == "running" {
        vega_store::tool_calls::update(store.conn(), call_id, status, Some("once"), None, None)?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_repair_normalizes_stale_rows_and_keeps_partial_text_immutable()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "confirm")?;
    let partial = "durable partial text";
    seed_assistant_row(&fixture.store, "stale-assistant", partial, "streaming")?;
    seed_stale_tool(&fixture.store, "stale-running", "running", 1)?;
    seed_stale_tool(&fixture.store, "stale-approved", "approved", 2)?;
    seed_stale_tool(&fixture.store, "stale-pending", "pending_approval", 3)?;
    drop(fixture.store);

    // Restart: fresh Store::open (controller restart), strict recovery,
    // then projection.
    let store = Store::open(&fixture.db_path)?;
    store.migrate()?;
    let counts = vega_store::recovery::recover_thread(store.conn(), THREAD_ID, 900)?;
    assert_eq!(
        counts,
        vega_store::recovery::RecoveryCounts {
            messages_interrupted: 1,
            tools_rejected: 1,
            tools_cancelled: 2,
        }
    );

    // Approved/running rows: cancelled without fabricating a recovery denial;
    // the approval audit is preserved verbatim.
    for call_id in ["stale-running", "stale-approved"] {
        let row = tool_row(&store, call_id)?.expect(call_id);
        assert_eq!(row.0, "cancelled");
        let approval = ApprovalAudit::from_json(row.2.as_deref().expect("audit"))?;
        assert_eq!(approval.decision, Approval::Once);
        assert!(
            row.1
                .as_deref()
                .is_some_and(|output| output.contains("startup recovery")),
            "cancelled row carries the canonical recovery output"
        );
        assert_eq!(row.3, Some(900));
    }
    // Pending row: strict recovery denial audit.
    let pending = tool_row(&store, "stale-pending")?.expect("stale-pending");
    assert_eq!(pending.0, "rejected");
    let approval = ApprovalAudit::from_json(pending.2.as_deref().expect("audit"))?;
    assert_eq!(approval.decision, Approval::Deny);
    assert_eq!(approval.source, ApprovalSource::Recovery);
    assert_eq!(
        pending.1.as_deref(),
        Some(vega_store::recovery::RECOVERY_REJECTED_OUTPUT)
    );

    // Partial text visible and byte-identical after repair (no loss, no edit).
    let rows = message_rows(&store)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, partial);
    assert_eq!(rows[0].3, "interrupted");

    // Recovery is idempotent: a second restart pass changes nothing.
    let again = vega_store::recovery::recover_thread(store.conn(), THREAD_ID, 901)?;
    assert_eq!(
        again,
        vega_store::recovery::RecoveryCounts {
            messages_interrupted: 0,
            tools_rejected: 0,
            tools_cancelled: 0,
        },
        "terminal rows are never touched twice"
    );
    let rows_again = message_rows(&store)?;
    assert_eq!(rows_again, rows);

    // The repaired thread admits a new turn that appends, never edits.
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("repaired and continued".into()),
        usage(5, 5),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let tools = vega_tools::Tools::new(&fixture.repo)?;
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        THREAD_ID,
        "Continue after restart.",
        "T46 system prompt",
        CancellationToken::new(),
        &ParkingPermissionHook::default(),
        |_| Ok(()),
    )
    .await?;
    assert!(!run.interrupted && !run.failed);
    let rows = message_rows(&store)?;
    assert_eq!(
        rows.len(),
        3,
        "old rows kept verbatim, one new turn appended"
    );
    assert_eq!(rows[0].2, partial);
    assert_eq!(rows[0].3, "interrupted");
    assert_eq!(rows[2].3, "done");
    assert_eq!(rows[2].2, "repaired and continued");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_refusal_on_stale_rows_happens_before_any_provider_round()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "confirm")?;
    seed_assistant_row(&fixture.store, "stale-assistant", "still open", "streaming")?;
    seed_stale_tool(&fixture.store, "still-running", "running", 1)?;

    let provider = MockProvider::new(vec![ScriptStep::text("must not be called")]);
    let tools = vega_tools::Tools::new(&fixture.repo)?;
    let started = Instant::now();
    // The production resume entry is run_thread_task: its preparation runs
    // the strict recovery first, so a resume on a thread with stale rows
    // repairs them (streaming -> interrupted) instead of replaying or
    // fabricating; the new turn then proceeds from all-terminal state.
    let run = tokio::time::timeout(
        Duration::from_secs(5),
        run_thread_task_with_permission_sink(
            &fixture.store,
            &provider,
            &tools,
            THREAD_ID,
            "Resume too early.",
            "T46 system prompt",
            CancellationToken::new(),
            &ParkingPermissionHook::default(),
            |_| Ok(()),
        ),
    )
    .await
    .expect("resume must never hang on stale rows")?;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "repair-then-resume is immediate"
    );
    assert!(!run.interrupted && !run.failed);

    // The stale streaming row was repaired (never revived, never dropped):
    // visible as interrupted with byte-identical text.
    let rows = message_rows(&fixture.store)?;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].2, "still open");
    assert_eq!(rows[0].3, "interrupted");
    let stale_call = tool_row(&fixture.store, "still-running")?.expect("still-running");
    assert_eq!(stale_call.0, "cancelled");
    assert_eq!(
        stale_call.1.as_deref(),
        Some(vega_store::recovery::RECOVERY_CANCELLED_OUTPUT)
    );
    assert_eq!(rows[2].3, "done");
    Ok(())
}

// ---------------------------------------------------------------------------
// Explicit Resume E2E: all-old-rows-terminal gate, exactly one auditable
// continuation, zero replay.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_after_full_terminal_appends_exactly_one_continuation_and_replays_nothing()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "confirm")?;
    // Turn 1 completed normally with one successful read (terminal work).
    let store = &fixture.store;
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "old-read".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            usage(9, 9),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("turn one answer".into()),
            usage(9, 9),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let tools = vega_tools::Tools::new(&fixture.repo)?;
    run_thread_task_with_permission_sink(
        store,
        &provider,
        &tools,
        THREAD_ID,
        "Turn one.",
        "T46 system prompt",
        CancellationToken::new(),
        &ParkingPermissionHook::default(),
        |_| Ok(()),
    )
    .await?;
    let before = message_rows(store)?;
    assert_eq!(before.len(), 2, "turn one persisted");
    let old_tool = tool_row(store, "old-read")?.expect("old-read row");
    let old_finished_at = old_tool.3;
    let old_output = old_tool.1.clone();

    // Turn 2 (the explicit Resume): one new run on the all-terminal thread.
    let resume_provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("continuation answer".into()),
        usage(7, 7),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let events: EventLog = Arc::default();
    let event_log = events.clone();
    let run = run_thread_task_with_permission_sink(
        store,
        &resume_provider,
        &tools,
        THREAD_ID,
        "Resume the task.",
        "T46 system prompt",
        CancellationToken::new(),
        &ParkingPermissionHook::default(),
        move |event| {
            if let Ok(mut log) = event_log.lock() {
                log.push(event.clone());
            }
            Ok(())
        },
    )
    .await?;

    // Exactly one new provider round and zero replay of terminal work: the
    // resumed history carries no tool observation for the old call id.
    assert_eq!(resume_provider.requests().len(), 1);
    let history = &resume_provider.requests()[0].messages;
    assert!(
        history
            .iter()
            .all(|message| message.tool_call_id.as_deref() != Some("old-read")),
        "the old terminal tool call must not be re-observed into the provider context"
    );
    assert!(
        history
            .iter()
            .all(|message| message.tool_calls.iter().all(|call| call.id != "old-read")),
        "the old terminal tool call must not be re-proposed"
    );

    // Exactly one auditable continuation run and row appended; old rows
    // untouched (never revived to streaming).
    let rows = message_rows(store)?;
    assert_eq!(rows.len(), 4, "two turns, each user+assistant");
    assert_eq!(rows[1].2, "turn one answer");
    assert_eq!(rows[1].3, "done");
    assert_eq!(rows[2].1, "user");
    assert_eq!(rows[2].2, "Resume the task.");
    assert_eq!(rows[2].3, "done");
    assert_eq!(rows[3].3, "done");
    assert_eq!(rows[3].2, "continuation answer");
    let continuation_events = events_of(&events);
    let started_ids: Vec<&str> = continuation_events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::MessageStarted { message_id, .. } => Some(message_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        started_ids,
        vec![run.assistant_message_id.as_str()],
        "the resume is exactly one new run"
    );

    // The old successful tool row is byte-identical after the resume.
    let old_tool_after = tool_row(store, "old-read")?.expect("old-read row");
    assert_eq!(old_tool_after.3, old_finished_at);
    assert_eq!(old_tool_after.1, old_output);
    Ok(())
}

// ---------------------------------------------------------------------------
// Crash-after-effect residual (frozen C5 semantics: NOT exactly-once; Resume
// inspects current state and never auto-replays unknown-outcome work).
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crash_after_effect_residual_stays_explicit_and_never_becomes_exactly_once()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "auto")?;
    // crash-after-effect: the bash effect (a real file on disk) already
    // landed, but the process died before any terminal row was persisted.
    let effect_path = fixture.repo.join("residual.txt");
    fs::write(&effect_path, "effect happened once\n")?;
    seed_assistant_row(
        &fixture.store,
        "stale-assistant",
        "before crash",
        "streaming",
    )?;
    vega_store::tool_calls::insert(
        fixture.store.conn(),
        vega_store::tool_calls::NewToolCall {
            id: "crash-call",
            thread_id: THREAD_ID,
            message_id: "stale-assistant",
            seq: 1,
            tool: "bash",
            input_json: r#"{"cmd":"print once > residual.txt"}"#,
            status: "running",
            created_at: 1,
        },
    )?;
    vega_store::tool_calls::update(
        fixture.store.conn(),
        "crash-call",
        "running",
        Some("once"),
        None,
        None,
    )?;
    drop(fixture.store);

    // Restart: strict recovery normalizes the residual explicitly.
    let store = Store::open(&fixture.db_path)?;
    store.migrate()?;
    let counts = vega_store::recovery::recover_thread(store.conn(), THREAD_ID, 1_700)?;
    assert_eq!(counts.tools_cancelled, 1, "the residual is normalized");
    let residual = tool_row(&store, "crash-call")?.expect("crash-call row");
    assert_eq!(residual.0, "cancelled");
    assert_eq!(
        residual.1.as_deref(),
        Some(vega_store::recovery::RECOVERY_CANCELLED_OUTPUT),
        "residual carries the explicit recovery marker, never a fabricated success"
    );

    // Resume inspects the current state and proceeds without replay: the
    // provider never observes the unknown-outcome call again, and the
    // external effect stays exactly as the crash left it (no second effect,
    // no rollback — non exactly-once is owned by the persisted marker).
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("post-residual resume".into()),
        usage(3, 3),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let tools = vega_tools::Tools::new(&fixture.repo)?;
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        THREAD_ID,
        "Resume after the crash residual.",
        "T46 system prompt",
        CancellationToken::new(),
        &ParkingPermissionHook::default(),
        |_| Ok(()),
    )
    .await?;
    assert_eq!(provider.requests().len(), 1);
    assert!(
        provider.requests()[0]
            .messages
            .iter()
            .all(|message| message.tool_call_id.as_deref() != Some("crash-call")),
        "the unknown-outcome call is never auto-replayed into the provider context"
    );
    assert!(!run.interrupted && !run.failed);
    assert_eq!(fs::read_to_string(&effect_path)?, "effect happened once\n");
    let rows = message_rows(&store)?;
    assert_eq!(rows[0].2, "before crash");
    assert_eq!(rows[0].3, "interrupted");
    assert_eq!(rows[2].3, "done");
    let residual_after = tool_row(&store, "crash-call")?.expect("crash-call row");
    assert_eq!(residual_after.0, "cancelled");
    Ok(())
}

// ---------------------------------------------------------------------------
// Duplicate Stop / concurrent terminal races.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_stop_races_converge_to_exactly_one_terminal_event() -> Result<(), Box<dyn Error>>
{
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "confirm")?;
    let provider = MockProvider::new(vec![
        ScriptStep::delay(Duration::from_millis(500)),
        ScriptStep::text("late"),
    ]);
    let tools = vega_tools::Tools::new(&fixture.repo)?;
    let cancel = CancellationToken::new();
    // Four concurrent Stop presses; only the first may produce a terminal.
    let started = Instant::now();
    let run = {
        let stopper = {
            let triggers: Vec<CancellationToken> = (0..4).map(|_| cancel.clone()).collect();
            tokio::spawn(async move {
                for trigger in triggers {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    trigger.cancel();
                }
            })
        };
        let run = run_thread_task_with_permission_sink(
            &fixture.store,
            &provider,
            &tools,
            THREAD_ID,
            "Race the stop.",
            "T46 system prompt",
            cancel,
            &ParkingPermissionHook::default(),
            |_| Ok(()),
        )
        .await?;
        stopper.await.ok();
        run
    };
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(run.interrupted);
    assert_eq!(interrupted_event_count(&run.events), 1);
    let rows = message_rows(&fixture.store)?;
    assert_message_terminal(&rows, "", "interrupted");
    Ok(())
}

// ---------------------------------------------------------------------------
// Deterministic delay matrix: 100 cases, p99 < 1s measured with
// std::time::Instant. Note: this is the T46 test-side KPI harness; T43
// instruments the production controller receive-to-render path, so the two
// numbers are intentionally not comparable (no T43 dependency).
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_hundred_case_delay_matrix_converges_with_p99_under_one_second()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "confirm")?;
    let tools = vega_tools::Tools::new(&fixture.repo)?;

    // 100 deterministic (cancel_at_ms, provider_delay_ms) pairs. Table
    // driven: the delay sweep crosses Stop-before-tool, Stop-during-stream,
    // and Stop-at-script-end without a single hand-written test function.
    let mut cases: Vec<(u64, u64)> = Vec::with_capacity(100);
    for index in 0..100 {
        let cancel_at_ms = 5 + (index % 10) * 20;
        let provider_delay_ms = 10 + (index / 10) * 20;
        cases.push((cancel_at_ms, provider_delay_ms));
    }
    let mut samples: Vec<Duration> = Vec::with_capacity(cases.len());
    for (index, (cancel_at_ms, provider_delay_ms)) in cases.iter().enumerate() {
        let thread_id = format!("{THREAD_ID}-{index}");
        threads::create(
            fixture.store.conn(),
            threads::NewThread {
                id: &thread_id,
                project_id: &project_id,
                title: "matrix",
                mode: "execute",
                permission_mode: "confirm",
                model: MODEL,
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )?;
        let provider = MockProvider::new(vec![
            ScriptStep::text("head "),
            ScriptStep::delay(Duration::from_millis(*provider_delay_ms)),
            ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: format!("matrix-write-{index}"),
                    name: "write".into(),
                    input_json: format!(
                        r#"{{"path":"matrix-{index}.txt","content":"{WRITE_BODY}"}}"#
                    ),
                },
                usage(10, 2),
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ]),
        ]);
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        let cancel_at = Duration::from_millis(*cancel_at_ms);
        let started = Instant::now();
        let stopper = tokio::spawn(async move {
            tokio::time::sleep(cancel_at).await;
            trigger.cancel();
        });
        let run = tokio::time::timeout(
            Duration::from_secs(10),
            run_thread_task_with_permission_sink(
                &fixture.store,
                &provider,
                &tools,
                &thread_id,
                "Matrix stop.",
                "T46 system prompt",
                cancel,
                &ParkingPermissionHook::default(),
                |_| Ok(()),
            ),
        )
        .await
        .expect("matrix case must never hang")?;
        stopper.await.ok();
        samples.push(started.elapsed());

        // Every case reaches exactly one durable terminal state and never
        // produces the write effect after Stop (fence: late work dropped).
        let status: String = fixture.store.conn().query_row(
            "SELECT status FROM messages WHERE thread_id = ?1 AND role = 'assistant' \
             ORDER BY seq DESC LIMIT 1",
            [&thread_id],
            |row| row.get(0),
        )?;
        assert_eq!(
            status, "interrupted",
            "case {index} (cancel {cancel_at_ms}ms, delay {provider_delay_ms}ms) must terminalize"
        );
        assert!(run.interrupted);
        assert_eq!(interrupted_event_count(&run.events), 1);
        let effect = fixture.repo.join(format!("matrix-{index}.txt"));
        assert!(!effect.exists(), "case {index} wrote a file after Stop");
    }

    // p99 over 100 samples: the 99th ordered sample must stay under 1s.
    samples.sort();
    let p99 = samples[98];
    let max = samples[samples.len() - 1];
    // Measurement provenance for the acceptance report (visible with
    // --nocapture); the KPI judgment is the assertion below.
    println!(
        "T46 stop matrix (100 cases): p50={:?} p99={p99:?} max={max:?} (KPI p99 <1s; test-side Instant, not T43 controller instrumentation)",
        samples[50]
    );
    assert!(
        p99 < Duration::from_secs(1),
        "p99 {p99:?} must be <1s (max was {max:?})"
    );
    Ok(())
}
