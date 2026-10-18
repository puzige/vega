use super::*;

struct ContextFailureProvider(std::sync::Mutex<Option<VegaError>>);

impl ContextFailureProvider {
    fn new(error: VegaError) -> Self {
        Self(std::sync::Mutex::new(Some(error)))
    }
}

impl vega_runtime::Provider for ContextFailureProvider {
    fn chat_stream(
        &self,
        _request: vega_runtime::ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let error = self.0.lock().unwrap().take().unwrap();
        Box::pin(async move {
            let events =
                futures::stream::once(async move { Err::<ProviderEvent, VegaError>(error) });
            Ok(Box::pin(events) as vega_runtime::EventStream)
        })
    }
}

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
    let [
        ConversationEvent::MessageStarted { message_id, seq },
        ConversationEvent::RunStarted {
            message_id: clock_id,
            started_at,
        },
        ConversationEvent::AssistantResponseStarted {
            message_id: response_id,
            text_start_bytes: 0,
        },
        ConversationEvent::TextDelta {
            message_id: text_id,
            delta,
        },
        ConversationEvent::Error {
            message_id: Some(error_id),
            error,
            execution_duration_ms: Some(duration),
        },
    ] = run.events.as_slice()
    else {
        panic!("unexpected failure event sequence: {:?}", run.events);
    };
    assert_eq!(*seq, 2);
    for id in [message_id, clock_id, response_id, text_id, error_id] {
        assert_eq!(id, &run.assistant_message_id);
    }
    assert_eq!(delta, "partial");
    assert!(*duration <= started_at.elapsed().as_millis() as u64);
    assert!(matches!(
        error.as_ref(),
        VegaError::Provider {
            status: Some(503),
            message,
            retryable: false,
        } if message == "unavailable"
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
    let persisted_duration: i64 = store
        .conn()
        .query_row(
            "SELECT execution_duration_ms FROM assistant_run_durations WHERE message_id = ?1",
            [&run.assistant_message_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted_duration, i64::try_from(*duration).unwrap());
    let responses: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM assistant_responses", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(responses, 0);
}

#[tokio::test]
async fn compaction_failure_runtime_errors_persist_and_restore_exact_owner() {
    use crate::types::{ContextCompactionFailureCode as Failure, RunFailureKind};
    use vega_runtime::{ContextEstimateError, ContextRuntimeError as ContextError, VegaError};
    use vega_store::run_diagnostics::{
        DiagnosticFailureCode as Code, DiagnosticPhase, DiagnosticState,
    };

    let (store, directory, _) = setup();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    let cases = [
        (
            ContextError::SourceChanged,
            Code::SummarySourceChanged,
            RunFailureKind::Compaction(Failure::SourceChanged),
        ),
        (
            ContextError::SummaryTimedOut,
            Code::SummaryTimeout,
            RunFailureKind::Compaction(Failure::TimedOut),
        ),
        (
            ContextError::SummaryOutputTruncated {
                visible_bytes: 10,
                thinking_bytes: 4,
                output_tokens: Some(3),
            },
            Code::SummaryTruncated,
            RunFailureKind::Compaction(Failure::InvalidSummary),
        ),
        (
            ContextError::SummaryEmpty,
            Code::SummaryEmpty,
            RunFailureKind::Compaction(Failure::InvalidSummary),
        ),
        (
            ContextError::InvalidSummary,
            Code::SummaryFormatInvalid,
            RunFailureKind::Compaction(Failure::InvalidSummary),
        ),
        (
            ContextError::SummaryFormatInvalid,
            Code::SummaryFormatInvalid,
            RunFailureKind::Compaction(Failure::InvalidSummary),
        ),
        (
            ContextError::SummaryProjectionInvalid,
            Code::SummaryProjectionInvalid,
            RunFailureKind::Compaction(Failure::InvalidSummary),
        ),
        (
            ContextError::InvalidProjection,
            Code::SummaryProjectionInvalid,
            RunFailureKind::Compaction(Failure::InvalidSummary),
        ),
        (
            ContextError::SystemMessageInResult,
            Code::SummaryProjectionInvalid,
            RunFailureKind::Compaction(Failure::InvalidSummary),
        ),
        (
            ContextError::NoCompactablePrefix,
            Code::SummaryNoCompactablePrefix,
            RunFailureKind::Compaction(Failure::NoCompactablePrefix),
        ),
        (
            ContextError::SourceTooLarge,
            Code::SummarySourceTooLarge,
            RunFailureKind::Compaction(Failure::TooLarge),
        ),
        (
            ContextError::AggregateTooLarge,
            Code::SummaryAggregateTooLarge,
            RunFailureKind::Compaction(Failure::TooLarge),
        ),
        (
            ContextError::ImagesUnsupported,
            Code::SummaryImagesUnsupported,
            RunFailureKind::Compaction(Failure::ImagesUnsupported),
        ),
        (
            ContextError::AlreadyAttempted,
            Code::SummaryAlreadyAttempted,
            RunFailureKind::Compaction(Failure::Unavailable),
        ),
        (
            ContextError::OverLimit {
                estimated_tokens: 101,
                input_budget: 100,
            },
            Code::ContextOverLimit,
            RunFailureKind::ContextOverLimit,
        ),
        (
            ContextError::SummaryInputOverLimit {
                estimated_tokens: 101,
                input_budget: 100,
            },
            Code::ContextOverLimit,
            RunFailureKind::ContextOverLimit,
        ),
        (
            ContextError::ResultOverLimit {
                estimated_tokens: 101,
                target_tokens: 100,
            },
            Code::ContextOverLimit,
            RunFailureKind::ContextOverLimit,
        ),
        (
            ContextError::Estimate(ContextEstimateError::InputTooLarge),
            Code::ContextOverLimit,
            RunFailureKind::ContextOverLimit,
        ),
        (
            ContextError::MissingHook,
            Code::UnknownSafeFailure,
            RunFailureKind::Runtime,
        ),
        (
            ContextError::Estimate(ContextEstimateError::InvalidBudget),
            Code::UnknownSafeFailure,
            RunFailureKind::Runtime,
        ),
        (
            ContextError::Estimate(ContextEstimateError::Overflow),
            Code::UnknownSafeFailure,
            RunFailureKind::Runtime,
        ),
    ];

    let mut expected_failures = Vec::new();
    for (index, (context_error, expected_code, expected_kind)) in cases.into_iter().enumerate() {
        let provider = ContextFailureProvider::new(VegaError::Context(context_error));
        let run = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            &format!("context failure {index}"),
            "System",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(run.failed);
        let live_error = run.events.iter().find_map(|event| match event {
            ConversationEvent::Error {
                message_id: Some(message_id),
                error,
                ..
            } if message_id == &run.assistant_message_id => Some(error.as_ref()),
            _ => None,
        });
        assert_eq!(
            live_error.map(RunFailureKind::from_runtime),
            Some(expected_kind),
            "live error {index}"
        );
        let root = vega_store::run_diagnostics::read_by_thread(store.conn(), "thread-1")
            .unwrap()
            .into_iter()
            .find(|row| {
                row.event.run_id == run.assistant_message_id
                    && row.event.phase == DiagnosticPhase::Run
                    && row.event.state == DiagnosticState::Failed
            })
            .expect("persisted run terminal");
        assert_eq!(root.event.failure_code, Some(expected_code), "root {index}");
        expected_failures.push((run.assistant_message_id, expected_kind));
    }

    let reopened = Store::open_read_only(store.database_path().unwrap()).unwrap();
    let projection = crate::agent::read_context_projection_with_provider(
        &reopened,
        "thread-1",
        "mock-model",
        None,
        "system",
    )
    .unwrap();
    for failure in expected_failures {
        assert!(projection.run_failures.contains(&failure), "{failure:?}");
    }
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

#[test]
fn credential_error_terminal_receipt_and_thread_update_commit_or_roll_back_together() {
    for reject_thread_update in [false, true] {
        let (store, _dir, project_id) = setup();
        store
            .conn()
            .execute_batch(
                "INSERT INTO messages (id,thread_id,seq,role,content,status,created_at)
             VALUES ('atomic-user','thread-1',1,'user','continue safely','done',0),
                    ('atomic-assistant','thread-1',2,'assistant','partial','streaming',0);
             UPDATE threads SET updated_at = 0 WHERE id = 'thread-1';
             UPDATE projects SET last_opened_at = 0;",
            )
            .unwrap();
        if reject_thread_update {
            store.conn().execute_batch(
                "CREATE TRIGGER reject_terminal_thread_update BEFORE UPDATE OF updated_at ON threads
                 BEGIN SELECT RAISE(ABORT, 'synthetic thread write failure'); END;",
            ).unwrap();
        }
        let result = persist_runtime_event(
            &store,
            &project_id,
            "thread-1",
            "atomic-assistant",
            "mock",
            false,
            "partial",
            &mut 1,
            &RuntimeEvent::Error(Arc::new(VegaError::CredentialCheckUnavailable)),
            Some(123),
        );
        assert_eq!(result.is_err(), reject_thread_update);
        let status: String = store
            .conn()
            .query_row(
                "SELECT status FROM messages WHERE id = 'atomic-assistant'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            status,
            if reject_thread_update {
                "streaming"
            } else {
                "failed"
            }
        );
        let receipt =
            vega_store::credential_continuation::find(store.conn(), "atomic-assistant").unwrap();
        assert_eq!(receipt.is_some(), !reject_thread_update);
        if let Some(receipt) = receipt {
            assert_eq!(receipt.state, "ready");
            assert_eq!(receipt.user_message_id, "atomic-user");
            assert_eq!(receipt.reason, "unavailable");
        }
        let (thread_updated, project_opened): (i64, i64) = store.conn().query_row(
            "SELECT t.updated_at, p.last_opened_at FROM threads t JOIN projects p ON p.id = t.project_id WHERE t.id = 'thread-1'", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(thread_updated > 0, !reject_thread_update);
        assert_eq!(thread_updated, project_opened);
        let duration_count: i64 = store.conn().query_row(
            "SELECT COUNT(*) FROM assistant_run_durations WHERE message_id = 'atomic-assistant'", [], |row| row.get(0),
        ).unwrap();
        assert_eq!(duration_count, i64::from(!reject_thread_update));
    }
}
