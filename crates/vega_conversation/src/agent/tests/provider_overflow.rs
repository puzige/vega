use super::*;

#[tokio::test]
async fn issue375_primary_context_overflow_recovers_through_durable_summary() {
    let (store, dir, _) = setup();
    store.conn().execute("INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES ('goal', 'thread-1', 1, 'user', 'text', 'Task goal: finish project', 'done', 1), ('progress', 'thread-1', 2, 'assistant', 'text', 'Progress: implementation complete; pending review', 'done', 2)", []).unwrap();
    store
        .conn()
        .execute(
            "UPDATE messages SET content = content || ?1 WHERE id = 'progress'",
            ["details ".repeat(1000)],
        )
        .unwrap();
    vega_store::context_compaction::save_model_policy(
        store.conn(),
        &vega_store::context_compaction::ModelContextPolicy {
            provider: "mock-provider".into(),
            model: "mock-model".into(),
            input_limit: Some(100_000),
            output_reserve: Some(1_000),
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::Error {
            status: Some(400),
            message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
            retryable: false,
        }],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta(
                "Task goal: finish project. Progress: implementation complete. Pending review."
                    .into(),
            ),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Review complete".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    run_thread_task_with_permission_config_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "continue review",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        Some(FrozenReasoning::unknown("mock-provider", "mock-model")),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0].tools.is_empty());
    assert!(requests[1].tools.is_empty());
    assert!(!requests[2].tools.is_empty());
    let text = requests[2]
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Task goal"));
    assert!(text.contains("Progress"));
    assert!(text.contains("Pending review"));
    assert!(text.contains("continue review"));
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_some()
    );
    let rows: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE id IN ('goal', 'progress')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 2);
    let calls: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))
        .unwrap();
    assert_eq!(calls, 0);
}

#[tokio::test]
async fn issue375_display_thinking_is_bounded_without_ending_long_durable_task() {
    let (store, dir, _) = setup();
    vega_store::context_compaction::save_model_policy(
        store.conn(),
        &vega_store::context_compaction::ModelContextPolicy {
            provider: "mock-provider".into(),
            model: "mock-model".into(),
            input_limit: Some(1_000_000),
            output_reserve: Some(1_000),
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let mut rounds = (0..8)
        .map(|index| {
            let mut events = (0..4)
                .map(|_| {
                    ProviderEvent::ThinkingDelta(
                        "r".repeat(vega_runtime::REASONING_DELTA_MAX_BYTES),
                    )
                })
                .collect::<Vec<_>>();
            events.extend([
                ProviderEvent::ToolUse {
                    id: format!("read-{index}"),
                    name: "read".into(),
                    input_json: r#"{"path":"lib.rs"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ]);
            vec![ScriptStep::events(events)]
        })
        .collect::<Vec<_>>();
    rounds.push(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("task finished".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let provider = MockProvider::new_rounds(rounds);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let mut displayed = 0usize;
    let result = run_thread_task_with_permission_config_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Task goal: inspect implementation",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |event| {
            if let ConversationEvent::ThinkingDelta { delta, .. } = event {
                displayed += delta.len();
            }
            Ok(())
        },
        PersistenceActorConfig::default(),
        None,
        None,
        Some(FrozenReasoning::unknown("mock-provider", "mock-model")),
    )
    .await
    .unwrap();
    assert!(!result.failed);
    assert!(displayed > vega_runtime::REASONING_RUN_MAX_BYTES);
    assert_eq!(provider.requests().len(), 9);
    assert!(
        provider
            .requests()
            .iter()
            .all(|request| !request.tools.is_empty())
    );
    let retained = result
        .events
        .iter()
        .map(|event| match event {
            ConversationEvent::ThinkingDelta { delta, .. }
            | ConversationEvent::SummaryDelta { delta, .. } => delta.len(),
            _ => 0,
        })
        .sum::<usize>();
    assert!(retained <= vega_runtime::REASONING_TURN_MAX_BYTES);
    let calls: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE thread_id = 'thread-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(calls, 8);
}
