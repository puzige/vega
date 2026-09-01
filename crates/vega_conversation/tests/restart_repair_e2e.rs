/// Seeds one assistant row (content + status) for the restart journeys.
mod srr_common;

use srr_common::*;

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
async fn resume_repairs_stale_rows_then_runs_exactly_one_provider_round()
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
