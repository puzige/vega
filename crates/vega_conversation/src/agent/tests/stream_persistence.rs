use super::*;

struct DelayedIssue146Permission;

impl PermissionHook for DelayedIssue146Permission {
    fn request(
        &self,
        _request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        Box::pin(async {
            tokio::time::sleep(Duration::from_millis(60)).await;
            Ok(PermissionDecision::Once)
        })
    }
}

#[tokio::test]
async fn persists_messages_tool_lifecycle_and_zero_cost_usage() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = scripted_provider("call-1", r#"{"pattern":"TODO","path":"lib.rs"}"#);
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Find TODO",
        "You inspect repositories.",
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(run.content, "Checking. Found the TODO.");
    assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ThinkingDelta { delta, .. } if delta == "Need grep")));
    assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallApproved { call_id, .. } if call_id == "call-1")));
    assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallRunning { call_id } if call_id == "call-1")));
    assert!(run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallFinished { result, .. }
                if result.status == ToolCallStatus::Success && result.output.contains("lib.rs:1:// TODO")
        )));

    let assistant: (String, String, i64) = store
        .conn()
        .query_row(
            "SELECT content, status, seq FROM messages WHERE id = ?1",
            [&run.assistant_message_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        assistant,
        ("Checking. Found the TODO.".into(), "done".into(), 2)
    );
    let tool: (String, String, String, String) = store
        .conn()
        .query_row(
            "SELECT status, approval, output_text, input_json FROM tool_calls WHERE id = 'call-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(tool.0, "success");
    let approval = ApprovalAudit::from_json(&tool.1).unwrap();
    assert_eq!(approval.decision, Approval::Once);
    assert_eq!(approval.source, ApprovalSource::ReadonlyTool);
    assert!(tool.2.contains("lib.rs:1:// TODO"));
    assert_eq!(tool.3, r#"{"pattern":"TODO","path":"lib.rs"}"#);
    let usage: (i64, i64, i64, i64, i64, String) = store
        .conn()
        .query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
                 cost_microcents, model FROM token_usage",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(usage, (12, 3, 2, 1, 0, "mock-model".into()));
    let usage_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM token_usage", [], |row| row.get(0))
        .unwrap();
    let nonzero_cost_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM token_usage WHERE cost_microcents != 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(usage_count, 2);
    assert_eq!(nonzero_cost_count, 0);

    let diagnostic_events = wait_for_diagnostics(&store, &run.assistant_message_id, 8).await;
    assert_eq!(diagnostic_events.len(), 8);
    let root = diagnostic_events
        .iter()
        .find(|event| event.event.phase == vega_store::run_diagnostics::DiagnosticPhase::Run)
        .unwrap();
    assert!(root.event.parent_attempt_id.is_none());
    assert_eq!(
        root.event.state,
        vega_store::run_diagnostics::DiagnosticState::Started
    );
    let root_attempt_id = root.event.attempt_id.clone();
    let primary_attempts = diagnostic_events
        .iter()
        .filter(|event| {
            event.event.phase == vega_store::run_diagnostics::DiagnosticPhase::PrimaryModel
        })
        .collect::<Vec<_>>();
    assert_eq!(primary_attempts.len(), 4);
    assert!(primary_attempts.iter().all(|event| event.event.parent_attempt_id.as_deref() == Some(root_attempt_id.as_str())));
    assert!(
        primary_attempts
            .iter()
            .any(|event| event.event.metrics.stop_reason
                == Some(vega_store::run_diagnostics::DiagnosticStopReason::ToolUse))
    );
    let tool_attempts = diagnostic_events
        .iter()
        .filter(|event| event.event.phase == vega_store::run_diagnostics::DiagnosticPhase::Tool)
        .collect::<Vec<_>>();
    assert_eq!(tool_attempts.len(), 2);
    assert!(
        tool_attempts
            .iter()
            .all(|event| event.event.tool_call_id.as_deref() == Some("call-1"))
    );
    assert!(tool_attempts.iter().any(|event| {
        event.event.state == vega_store::run_diagnostics::DiagnosticState::Succeeded
            && event
                .event
                .metrics
                .tool_output_bytes
                .is_some_and(|bytes| bytes > 0)
    }));
    let diagnostic_json = vega_store::run_diagnostics::export_run(
        store.conn(),
        "thread-1",
        &run.assistant_message_id,
    )
    .unwrap();
    for content in ["Find TODO", "lib.rs:1:// TODO", r#"{"pattern":"TODO""#] {
        assert!(!diagnostic_json.contains(content));
    }

    let updated_at: i64 = store
        .conn()
        .query_row(
            "SELECT updated_at FROM threads WHERE id = 'thread-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(updated_at > 1);

    let tables: Vec<String> = {
        let mut stmt = store
                .conn()
                .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(
        tables,
        vec![
            "assistant_run_durations",
            "context_checkpoints",
            "context_compaction_status",
            "context_settings",
            "image_attachments",
            "mcp_secret_cleanup",
            "mcp_servers",
            "messages",
            "model_context_policies",
            "permissions",
            "projects",
            "run_diagnostic_events",
            "sidebar_groups",
            "sidebar_memberships",
            "sidebar_organization",
            "sidebar_project_order",
            "skill_activation_audits",
            "skill_approvals",
            "skill_project_settings",
            "skill_run_snapshots",
            "skill_settings",
            "skill_sources",
            "thread_skill_pins",
            "threads",
            "token_usage",
            "tool_calls"
        ]
    );
}

async fn wait_for_diagnostics(
    store: &Store,
    run_id: &str,
    minimum_count: usize,
) -> Vec<vega_store::run_diagnostics::DiagnosticEvent> {
    for _ in 0..100 {
        let events =
            vega_store::run_diagnostics::read_by_run(store.conn(), "thread-1", run_id).unwrap();
        if events.len() >= minimum_count {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    vega_store::run_diagnostics::read_by_run(store.conn(), "thread-1", run_id).unwrap()
}

#[tokio::test]
async fn diagnostics_keep_tool_success_before_later_provider_failure() {
    const FAILURE_CANARY: &str = "VEGA_PROVIDER_FAILURE_CANARY";
    let (store, directory, _project_id) = setup();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("before tool".into()),
            ProviderEvent::ToolUse {
                id: "call-after-tool".into(),
                name: "grep".into(),
                input_json: r#"{"pattern":"TODO","path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Usage {
                input: 12,
                output: 3,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::Error {
            status: Some(503),
            message: FAILURE_CANARY.into(),
            retryable: false,
        }],
    ]);
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Find TODO then report",
        "Inspect repositories.",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(run.failed);

    let events = wait_for_diagnostics(&store, &run.assistant_message_id, 8).await;
    assert_eq!(events.len(), 8);
    let tool_terminal_index = events
        .iter()
        .position(|event| {
            event.event.phase == vega_store::run_diagnostics::DiagnosticPhase::Tool
                && event.event.state == vega_store::run_diagnostics::DiagnosticState::Succeeded
        })
        .unwrap();
    let failed_model_index = events
        .iter()
        .position(|event| {
            event.event.phase == vega_store::run_diagnostics::DiagnosticPhase::PrimaryModel
                && event.event.state == vega_store::run_diagnostics::DiagnosticState::Failed
        })
        .unwrap();
    assert!(tool_terminal_index < failed_model_index);
    assert_eq!(
        events[failed_model_index].event.failure_code,
        Some(vega_store::run_diagnostics::DiagnosticFailureCode::ProviderHttp)
    );
    assert_eq!(
        events[failed_model_index].event.metrics.http_status,
        Some(503)
    );
    let export = vega_store::run_diagnostics::export_run(
        store.conn(),
        "thread-1",
        &run.assistant_message_id,
    )
    .unwrap();
    assert!(!export.contains(FAILURE_CANARY));
    assert!(events.iter().any(|event| {
        event.event.phase == vega_store::run_diagnostics::DiagnosticPhase::Tool
            && event.event.tool_call_id.as_deref() == Some("call-after-tool")
    }));
}

#[tokio::test]
async fn diagnostic_sql_write_failure_does_not_change_required_run_result() {
    let (store, directory, _project_id) = setup();
    store
        .conn()
        .execute_batch(
            "CREATE TRIGGER fail_run_diagnostic_insert
             BEFORE INSERT ON run_diagnostic_events
             BEGIN SELECT RAISE(FAIL, 'diagnostic write failure'); END;",
        )
        .unwrap();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("completed safely".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);

    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Canary prompt excluded from diagnostics",
        "System canary excluded from diagnostics",
        CancellationToken::new(),
    )
    .await
    .expect("diagnostic-only SQLite failure must not fail a completed run");

    assert_eq!(run.content, "completed safely");
    let assistant_status: String = store
        .conn()
        .query_row(
            "SELECT status FROM messages WHERE id = ?1",
            [&run.assistant_message_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(assistant_status, "done");
    let diagnostic_rows: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM run_diagnostic_events", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(diagnostic_rows, 0);
}

#[tokio::test]
async fn issue146_runtime_duration_includes_permission_wait_and_tool_continuation() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(|command, _, _| {
            assert_eq!(command, "printf issue146-ok");
            Box::pin(async {
                Ok(vega_tools::BashOutput {
                    text: "issue146-ok".into(),
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "issue146-permission-tool".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf issue146-ok"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("continued answer".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Run approved tool",
        "System",
        CancellationToken::new(),
        &DelayedIssue146Permission,
        |_| Ok(()),
    )
    .await
    .unwrap();

    assert_eq!(run.content, "continued answer");
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallApproved { call_id, .. }
            if call_id == "issue146-permission-tool"
    )));
    let duration_ms = run
        .events
        .iter()
        .find_map(|event| match event {
            ConversationEvent::MessageFinished {
                execution_duration_ms,
                ..
            } => *execution_duration_ms,
            _ => None,
        })
        .expect("terminal event carries run duration");
    assert!(duration_ms >= 50, "duration_ms={duration_ms}");

    let page =
        messages::page_before(store.conn(), "thread-1", messages::PageCursor::Head, 20).unwrap();
    assert_eq!(
        page.execution_durations_ms.get(&run.assistant_message_id),
        Some(&(duration_ms as i64))
    );
    let history = crate::history::latest_history_page(&store, "thread-1", 20).unwrap();
    assert!(history.entries.iter().any(|entry| matches!(
        entry,
        crate::history::HistoryEntry::AssistantText {
            message_id,
            execution_duration_ms: Some(duration),
            ..
        } if message_id == &run.assistant_message_id && *duration == duration_ms
    )));
}

#[tokio::test]
async fn forwards_events_live_only_after_critical_state_is_persisted() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = scripted_provider("live-call", r#"{"pattern":"TODO","path":"lib.rs"}"#);
    let mut observed = Vec::new();
    let mut expected_text = String::new();

    let run = run_thread_task_with_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Find TODO",
        "System",
        CancellationToken::new(),
        |event| {
            match event {
                ConversationEvent::MessageStarted { message_id, .. } => {
                    let row = messages::find(store.conn(), message_id)?.ok_or_else(|| {
                        VegaError::Tool {
                            tool: "test".into(),
                            message: "started message missing".into(),
                        }
                    })?;
                    assert_eq!(row.status, "streaming");
                    observed.push("started");
                }
                ConversationEvent::TextDelta { message_id, delta } => {
                    expected_text.push_str(delta);
                    let row = messages::find(store.conn(), message_id)?.ok_or_else(|| {
                        VegaError::Tool {
                            tool: "test".into(),
                            message: "streaming message missing".into(),
                        }
                    })?;
                    assert_eq!(row.status, "streaming");
                    assert_eq!(row.content, expected_text);
                    observed.push("text");
                }
                ConversationEvent::ToolCallApproved { call_id, .. } => {
                    let status: String = store.conn().query_row(
                        "SELECT status FROM tool_calls WHERE id = ?1",
                        [call_id],
                        |row| row.get(0),
                    )?;
                    assert_eq!(status, "approved");
                    observed.push("approved");
                }
                ConversationEvent::ToolCallRunning { call_id } => {
                    let status: String = store.conn().query_row(
                        "SELECT status FROM tool_calls WHERE id = ?1",
                        [call_id],
                        |row| row.get(0),
                    )?;
                    assert_eq!(status, "running");
                    observed.push("running");
                }
                ConversationEvent::ToolCallFinished {
                    call_id, result, ..
                } => {
                    let persisted: (String, String) = store.conn().query_row(
                        "SELECT status, output_text FROM tool_calls WHERE id = ?1",
                        [call_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )?;
                    assert_eq!(persisted.0, "success");
                    assert_eq!(persisted.1, result.output);
                    observed.push("tool-finished");
                }
                ConversationEvent::ToolCallOutput { call_id, .. } => {
                    let status: String = store.conn().query_row(
                        "SELECT status FROM tool_calls WHERE id = ?1",
                        [call_id],
                        |row| row.get(0),
                    )?;
                    assert_eq!(status, "success");
                    observed.push("output");
                }
                ConversationEvent::UsageUpdated { .. } => {
                    let count: i64 =
                        store
                            .conn()
                            .query_row("SELECT COUNT(*) FROM token_usage", [], |row| row.get(0))?;
                    assert!(count >= 1);
                    observed.push("usage");
                }
                ConversationEvent::MessageFinished { message_id, .. } => {
                    let row = messages::find(store.conn(), message_id)?.ok_or_else(|| {
                        VegaError::Tool {
                            tool: "test".into(),
                            message: "finished message missing".into(),
                        }
                    })?;
                    assert_eq!(row.status, "done");
                    assert_eq!(row.content, "Checking. Found the TODO.");
                    observed.push("finished");
                }
                _ => {}
            }
            Ok(())
        },
    )
    .await
    .unwrap();

    assert_eq!(run.content, "Checking. Found the TODO.");
    assert_eq!(expected_text, "Checking. Found the TODO.");
    let approved = observed
        .iter()
        .position(|item| *item == "approved")
        .unwrap();
    let terminal = observed
        .iter()
        .position(|item| *item == "tool-finished")
        .unwrap();
    let running = observed.iter().position(|item| *item == "running").unwrap();
    let output = observed.iter().position(|item| *item == "output").unwrap();
    let finished = observed
        .iter()
        .position(|item| *item == "finished")
        .unwrap();
    assert!(approved < running && running < output && output < terminal && terminal < finished);
}

#[tokio::test]
async fn fast_text_deltas_coalesce_and_each_displayed_delta_is_durable() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let mut provider_events = (0..100)
        .map(|_| ProviderEvent::TextDelta("x".into()))
        .collect::<Vec<_>>();
    provider_events.push(ProviderEvent::Done {
        stop_reason: StopReason::End,
    });
    let provider = MockProvider::new(vec![ScriptStep::events(provider_events)]);
    let writes = Arc::new(AtomicUsize::new(0));
    let config = PersistenceActorConfig {
        snapshot_writes: Some(writes.clone()),
        ..PersistenceActorConfig::default()
    };
    let mut displayed = String::new();

    let run = run_thread_task_with_sink_config(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Stream",
        "System",
        CancellationToken::new(),
        |event| {
            if let ConversationEvent::TextDelta { message_id, delta } = event {
                displayed.push_str(delta);
                let durable = messages::find(store.conn(), message_id)?
                    .ok_or_else(|| persistence_actor_error("streaming message missing"))?;
                assert!(durable.content.starts_with(&displayed));
            }
            Ok(())
        },
        config,
    )
    .await
    .unwrap();

    assert_eq!(run.content.len(), 100);
    assert_eq!(displayed.len(), 100);
    let write_count = writes.load(Ordering::SeqCst);
    assert!(write_count > 0 && write_count < 100, "writes={write_count}");
}

#[tokio::test]
async fn ten_thousand_deltas_keep_the_channel_bounded_and_writes_coalesced() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let mut provider_events = (0..10_000)
        .map(|_| ProviderEvent::TextDelta("x".into()))
        .collect::<Vec<_>>();
    provider_events.push(ProviderEvent::Done {
        stop_reason: StopReason::End,
    });
    let provider = MockProvider::new(vec![ScriptStep::events(provider_events)]);
    let writes = Arc::new(AtomicUsize::new(0));
    let config = PersistenceActorConfig {
        snapshot_writes: Some(writes.clone()),
        ..PersistenceActorConfig::default()
    };

    let run = run_thread_task_with_sink_config(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Stream",
        "System",
        CancellationToken::new(),
        |_| Ok(()),
        config,
    )
    .await
    .unwrap();

    assert_eq!(run.content.len(), 10_000);
    assert_eq!(
        run.events
            .iter()
            .filter(|event| matches!(event, ConversationEvent::TextDelta { .. }))
            .count(),
        10_000
    );
    let write_count = writes.load(Ordering::SeqCst);
    assert!(write_count > 0 && write_count < 100, "writes={write_count}");
}

#[tokio::test]
#[ignore = "load-sensitive: asserts a wall-clock budget (<16ms), fails under parallel test load; run with --ignored"]
async fn lone_text_delta_flushes_during_provider_stall_within_sixteen_ms() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![
        ScriptStep::events(vec![ProviderEvent::TextDelta("partial".into())]),
        ScriptStep::delay(Duration::from_millis(100)),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]);
    let mut started = None;
    let mut display_delay = None;

    run_thread_task_with_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Stream",
        "System",
        CancellationToken::new(),
        |event| {
            match event {
                ConversationEvent::MessageStarted { .. } => started = Some(Instant::now()),
                ConversationEvent::TextDelta { message_id, .. } => {
                    let began = started
                        .ok_or_else(|| persistence_actor_error("message start was not observed"))?;
                    display_delay = Some(began.elapsed());
                    let durable = messages::find(store.conn(), message_id)?
                        .ok_or_else(|| persistence_actor_error("streaming message missing"))?;
                    assert_eq!(durable.content, "partial");
                }
                _ => {}
            }
            Ok(())
        },
    )
    .await
    .unwrap();

    assert!(display_delay.unwrap() < Duration::from_millis(16));
}

#[tokio::test]
async fn running_persistence_failure_prevents_tool_start_and_next_round() {
    let (store, dir, _project_id) = setup();
    store
        .conn()
        .execute(
            "UPDATE threads SET permission_mode = 'full_access' WHERE id = 'thread-1'",
            [],
        )
        .unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    let tools = vega_tools::Tools::new(dir.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |_, _, _| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {
                Err(vega_tools::BashError::for_test(
                    vega_tools::BashErrorCode::SpawnFailed,
                ))
            })
        }));
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "blocked-call".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"never-run"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let config = PersistenceActorConfig {
        fail_event: Some(InjectedPersistenceFailure::Running),
        ..PersistenceActorConfig::default()
    };

    let result = tokio::time::timeout(
        Duration::from_millis(500),
        run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Read",
            "System",
            CancellationToken::new(),
            |_| Ok(()),
            config,
        ),
    )
    .await
    .expect("running barrier failure must stop before tool execution")
    .unwrap_err();

    assert!(result.to_string().contains("critical persistence failure"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(provider.requests().len(), 1);
    let status: String = store
        .conn()
        .query_row(
            "SELECT status FROM tool_calls WHERE id = 'blocked-call'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "approved");
}

#[tokio::test]
async fn terminal_persistence_failure_prevents_the_next_provider_round() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = scripted_provider("terminal-fail", r#"{"pattern":"TODO"}"#);
    let config = PersistenceActorConfig {
        fail_event: Some(InjectedPersistenceFailure::Finished),
        ..PersistenceActorConfig::default()
    };

    let error = run_thread_task_with_sink_config(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Find",
        "System",
        CancellationToken::new(),
        |_| Ok(()),
        config,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("critical persistence failure"));
    assert_eq!(provider.requests().len(), 1);
    let status: String = store
        .conn()
        .query_row(
            "SELECT status FROM tool_calls WHERE id = 'terminal-fail'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "running");
}

#[tokio::test]
async fn actor_panic_closes_ack_and_returns_without_hanging() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "panic-call".into(),
            name: "read".into(),
            input_json: r#"{"path":"lib.rs"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);
    let config = PersistenceActorConfig {
        fail_event: Some(InjectedPersistenceFailure::PanicRunning),
        ..PersistenceActorConfig::default()
    };
    let mut surfaced = None;

    let error = tokio::time::timeout(
        Duration::from_millis(500),
        run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Read",
            "System",
            CancellationToken::new(),
            |event| {
                if let ConversationEvent::Error { error, .. } = event {
                    surfaced = Some(error.clone());
                }
                Ok(())
            },
            config,
        ),
    )
    .await
    .expect("actor panic must close its async acknowledgement")
    .unwrap_err();

    assert!(error.to_string().contains("dropped event acknowledgement"));
    assert!(matches!(
        error,
        ConversationError::Runtime(ref error)
            if matches!(error.as_ref(), VegaError::Io(_))
    ));
    assert!(matches!(surfaced.as_deref(), Some(VegaError::Io(_))));
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn delayed_actor_does_not_block_the_tokio_executor() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("heartbeat".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let beats = Arc::new(AtomicUsize::new(0));
    let beat_counter = beats.clone();
    let heartbeat_cancel = CancellationToken::new();
    let heartbeat_stop = heartbeat_cancel.clone();
    let heartbeat = tokio::spawn(async move {
        while !heartbeat_stop.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(1)).await;
            beat_counter.fetch_add(1, Ordering::SeqCst);
        }
    });
    let config = PersistenceActorConfig {
        command_delay: Some(Duration::from_millis(30)),
        ..PersistenceActorConfig::default()
    };

    run_thread_task_with_sink_config(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Stream",
        "System",
        CancellationToken::new(),
        |_| Ok(()),
        config,
    )
    .await
    .unwrap();
    heartbeat_cancel.cancel();
    heartbeat.await.unwrap();

    assert!(beats.load(Ordering::SeqCst) >= 10);
}

#[tokio::test]
async fn delayed_preparation_does_not_block_the_tokio_executor() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let beats = Arc::new(AtomicUsize::new(0));
    let beat_counter = beats.clone();
    let heartbeat_cancel = CancellationToken::new();
    let heartbeat_stop = heartbeat_cancel.clone();
    let heartbeat = tokio::spawn(async move {
        while !heartbeat_stop.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(1)).await;
            beat_counter.fetch_add(1, Ordering::SeqCst);
        }
    });
    let config = PersistenceActorConfig {
        preparation_delay: Some(Duration::from_millis(30)),
        ..PersistenceActorConfig::default()
    };

    run_thread_task_with_sink_config(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Prepare",
        "System",
        CancellationToken::new(),
        |_| Ok(()),
        config,
    )
    .await
    .unwrap();
    heartbeat_cancel.cancel();
    heartbeat.await.unwrap();

    assert!(beats.load(Ordering::SeqCst) >= 10);
}

#[tokio::test]
async fn preparation_store_failure_is_structured_and_forwarded() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let beats = Arc::new(AtomicUsize::new(0));
    let beat_counter = beats.clone();
    let heartbeat_cancel = CancellationToken::new();
    let heartbeat_stop = heartbeat_cancel.clone();
    let heartbeat = tokio::spawn(async move {
        while !heartbeat_stop.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(1)).await;
            beat_counter.fetch_add(1, Ordering::SeqCst);
        }
    });
    let config = PersistenceActorConfig {
        preparation_delay: Some(Duration::from_millis(30)),
        preparation_query_only: true,
        ..PersistenceActorConfig::default()
    };
    let mut surfaced = None;

    let error = tokio::time::timeout(
        Duration::from_millis(500),
        run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Prepare",
            "System",
            CancellationToken::new(),
            |event| {
                if let ConversationEvent::Error { error, .. } = event {
                    surfaced = Some(error.clone());
                }
                Ok(())
            },
            config,
        ),
    )
    .await
    .expect("failing preparation must not block the executor")
    .unwrap_err();
    heartbeat_cancel.cancel();
    heartbeat.await.unwrap();

    assert!(matches!(
        error,
        ConversationError::Runtime(ref error)
            if matches!(error.as_ref(), VegaError::Store(_))
    ));
    assert!(matches!(surfaced.as_deref(), Some(VegaError::Store(_))));
    assert!(provider.requests().is_empty());
    assert!(beats.load(Ordering::SeqCst) >= 10);
    let message_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .unwrap();
    assert_eq!(message_count, 0);
}

#[tokio::test]
async fn actor_store_failure_is_structured_and_forwarded() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("partial".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let config = PersistenceActorConfig {
        actor_query_only: true,
        ..PersistenceActorConfig::default()
    };
    let mut surfaced = None;

    let error = run_thread_task_with_sink_config(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Stream",
        "System",
        CancellationToken::new(),
        |event| {
            if let ConversationEvent::Error { error, .. } = event {
                surfaced = Some(error.clone());
            }
            Ok(())
        },
        config,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        ConversationError::Runtime(ref error)
            if matches!(error.as_ref(), VegaError::Store(_))
    ));
    assert!(matches!(surfaced.as_deref(), Some(VegaError::Store(_))));
}

#[tokio::test]
async fn actor_start_failure_uses_background_cleanup_and_structured_error() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let config = PersistenceActorConfig {
        fail_start: true,
        ..PersistenceActorConfig::default()
    };
    let mut surfaced = None;

    let error = tokio::time::timeout(
        Duration::from_millis(500),
        run_thread_task_with_sink_config(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Start actor",
            "System",
            CancellationToken::new(),
            |event| {
                if let ConversationEvent::Error { error, .. } = event {
                    surfaced = Some(error.clone());
                }
                Ok(())
            },
            config,
        ),
    )
    .await
    .expect("actor startup failure cleanup must not block the executor")
    .unwrap_err();

    assert!(matches!(
        error,
        ConversationError::Runtime(ref error)
            if matches!(error.as_ref(), VegaError::Io(_))
    ));
    assert!(matches!(surfaced.as_deref(), Some(VegaError::Io(_))));
    assert!(provider.requests().is_empty());
    let assistant_status: String = store
        .conn()
        .query_row(
            "SELECT status FROM messages WHERE role = 'assistant' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(assistant_status, "failed");
}
