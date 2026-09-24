mod srr_common;

use srr_common::*;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_mid_mock_tool_records_cancellation_and_never_starts_the_followup_call()
-> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let project_id = seed_project(&fixture.store, &fixture.repo)?;
    seed_thread(&fixture.store, &project_id, "full_access")?;
    let (connected_tx, connected_rx) = tokio::sync::oneshot::channel::<()>();
    let connected_tx = Arc::new(Mutex::new(Some(connected_tx)));
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "slow-read".into(),
            name: "bash".into(),
            input_json: r#"{"cmd":"cat slow.txt"}"#.into(),
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
    let tools = vega_tools::Tools::new(&fixture.repo)?.with_bash_test_executor(Arc::new(
        move |command, full_access, cancel| {
            assert_eq!(command, "cat slow.txt");
            assert!(full_access);
            let started = connected_tx.lock().unwrap().take().unwrap();
            Box::pin(async move {
                started.send(()).unwrap();
                cancel.cancelled().await;
                Err(vega_tools::BashError::for_test(
                    vega_tools::BashErrorCode::Cancelled,
                ))
            })
        },
    ));
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
                    tokio::time::timeout(Duration::from_secs(10), connected)
                        .await
                        .expect("mock tool must start")
                        .expect("mock tool start signal");
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
        "Stop-to-terminal took {elapsed:?}, KPI is <1s"
    );
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { call_id, result }
            if call_id == "slow-read"
                && result.status == vega_conversation::types::ToolCallStatus::Cancelled
                && result.output == "Tool error: bash failed (cancelled)"
    )));
    assert!(!run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallProposed { call } if call.id == "must-not-start"
    )));
    let slow_row = tool_row(&fixture.store, "slow-read")?.expect("slow-read row");
    assert_eq!(slow_row.0, "cancelled");
    assert!(slow_row.3.is_some());
    assert_eq!(
        slow_row.1.as_deref(),
        Some("Tool error: bash failed (cancelled)")
    );
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
