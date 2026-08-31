mod srr_common;

use srr_common::*;
use std::io::Write as _;
use vega_conversation::agent::PermissionQueue;

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
