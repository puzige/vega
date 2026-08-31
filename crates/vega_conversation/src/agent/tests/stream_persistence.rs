use super::*;

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
            "messages",
            "permissions",
            "projects",
            "threads",
            "token_usage",
            "tool_calls"
        ]
    );
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
    let output = observed.iter().position(|item| *item == "output").unwrap();
    let finished = observed
        .iter()
        .position(|item| *item == "finished")
        .unwrap();
    assert!(approved < output && output < terminal && terminal < finished);
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
    let fifo = dir.path().join("never-read");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(status.success());
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "blocked-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"never-read"}"#.into(),
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
    .expect("running barrier failure must not enter the blocking FIFO")
    .unwrap_err();

    assert!(result.to_string().contains("critical persistence failure"));
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
