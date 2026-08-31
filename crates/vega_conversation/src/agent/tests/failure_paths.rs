use super::*;

#[tokio::test]
async fn live_sink_failure_stops_runtime_and_marks_assistant_failed() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("partial".into()),
        ProviderEvent::ToolUse {
            id: "must-not-run".into(),
            name: "read".into(),
            input_json: r#"{"path":"lib.rs"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]);

    let error = run_thread_task_with_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Fail sink",
        "System",
        CancellationToken::new(),
        |event| {
            if matches!(event, ConversationEvent::TextDelta { .. }) {
                return Err(VegaError::Tool {
                    tool: "event-sink".into(),
                    message: "consumer unavailable".into(),
                });
            }
            Ok(())
        },
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("consumer unavailable"));
    let assistant: (String, String) = store
            .conn()
            .query_row(
                "SELECT status, content FROM messages WHERE role = 'assistant' ORDER BY seq DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
    assert_eq!(assistant, ("failed".into(), "partial".into()));
    let tool_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))
        .unwrap();
    assert_eq!(tool_count, 0);
}

#[tokio::test]
async fn message_started_sink_failure_is_finalized_without_hanging() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let mut surfaced = None;

    let error = tokio::time::timeout(
        Duration::from_millis(500),
        run_thread_task_with_sink(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Fail start sink",
            "System",
            CancellationToken::new(),
            |event| match event {
                ConversationEvent::MessageStarted { .. } => Err(VegaError::Tool {
                    tool: "event-sink".into(),
                    message: "start consumer unavailable".into(),
                }),
                ConversationEvent::Error { error, .. } => {
                    surfaced = Some(error.clone());
                    Ok(())
                }
                _ => Ok(()),
            },
        ),
    )
    .await
    .expect("MessageStarted sink failure must not hang")
    .unwrap_err();

    assert!(matches!(
        error,
        ConversationError::Runtime(ref error)
            if matches!(
                error.as_ref(),
                VegaError::Tool { tool, message }
                    if tool == "event-sink" && message == "start consumer unavailable"
            )
    ));
    assert!(matches!(
        surfaced.as_deref(),
        Some(VegaError::Tool { tool, message })
            if tool == "event-sink" && message == "start consumer unavailable"
    ));
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

#[tokio::test]
async fn provider_error_maps_to_error_event_and_failed_message() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![
        ScriptStep::text("partial"),
        ScriptStep::Error {
            status: Some(503),
            message: "unavailable".into(),
            retryable: false,
        },
    ]);
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Fail",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(run.failed);
    assert!(matches!(
        run.events.as_slice(),
        [
            ConversationEvent::MessageStarted { .. },
            ConversationEvent::TextDelta { delta, .. },
            ConversationEvent::Error { error, .. }
        ]
            if matches!(
                error.as_ref(),
                VegaError::Provider {
                    status: Some(503),
                    message,
                    retryable: false,
                } if message == "unavailable"
            ) && delta == "partial"
    ));
    let persisted: (String, String) = store
        .conn()
        .query_row(
            "SELECT content, status FROM messages WHERE id = ?1",
            [&run.assistant_message_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(persisted, ("partial".into(), "failed".into()));
}

#[tokio::test]
async fn tool_failure_is_persisted_and_the_model_can_still_converge() {
    let (store, dir, _project_id) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "missing-read".into(),
                name: "read".into(),
                input_json: r#"{"path":"missing.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Handled the missing file.".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Read missing",
        "System",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(run.content, "Handled the missing file.");
    let persisted: (String, String) = store
        .conn()
        .query_row(
            "SELECT status, output_text FROM tool_calls WHERE id = 'missing-read'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(persisted.0, "failed");
    assert!(persisted.1.contains("not found"));
}
