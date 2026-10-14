use super::*;
use crate::types::{PendingMessage, PendingMessageBehavior, RunFailureKind};
use vega_runtime::{ChatRequest, EventStream, REASONING_DELTA_MAX_BYTES, ReasoningBudgetScope};
use vega_store::run_diagnostics::{DiagnosticFailureCode, DiagnosticPhase, DiagnosticState};

fn turn_failure() -> Vec<ProviderEvent> {
    (0..4)
        .map(|_| {
            ProviderEvent::ThinkingDelta(
                "private reasoning ".repeat(REASONING_DELTA_MAX_BYTES / 18),
            )
        })
        .chain([
            ProviderEvent::SummaryDelta("s".repeat(100)),
            ProviderEvent::ThinkingDelta("x".repeat(REASONING_DELTA_MAX_BYTES)),
        ])
        .collect()
}

#[tokio::test]
async fn issue372_budget_failure_agrees_between_live_primary_root_and_reopen() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(turn_failure())]);
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "finish the task",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(run.failed);
    assert!(!run.interrupted);
    let failures = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::Error {
                message_id: Some(id),
                error,
                ..
            } => Some((id, RunFailureKind::from_runtime(error))),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        failures,
        [(
            &run.assistant_message_id,
            RunFailureKind::ReasoningLimit(Some(ReasoningBudgetScope::Turn))
        )]
    );
    let diagnostics = vega_store::run_diagnostics::read_by_run(
        store.conn(),
        "thread-1",
        &run.assistant_message_id,
    )
    .unwrap();
    for phase in [DiagnosticPhase::PrimaryModel, DiagnosticPhase::Run] {
        let terminal = diagnostics
            .iter()
            .find(|event| {
                event.event.phase == phase && event.event.state == DiagnosticState::Failed
            })
            .unwrap();
        assert_eq!(
            terminal.event.failure_code,
            Some(DiagnosticFailureCode::ReasoningLimit)
        );
    }
    let export = vega_store::run_diagnostics::export_run(
        store.conn(),
        "thread-1",
        &run.assistant_message_id,
    )
    .unwrap();
    assert!(!export.contains("private reasoning"));
    let database = store.database_path().unwrap().to_owned();
    drop(store);
    let reopened = Store::open(database).unwrap();
    let projection =
        read_context_projection(&reopened, "thread-1", "mock-model", "system").unwrap();
    assert_eq!(
        projection.run_failures,
        [(
            run.assistant_message_id,
            RunFailureKind::ReasoningLimit(None)
        )]
    );
    let raw = messages::recent(reopened.conn(), "thread-1", 20).unwrap();
    assert!(
        raw.iter()
            .all(|message| !message.content.contains("private reasoning"))
    );
}

#[tokio::test]
async fn issue372_legacy_failure_after_steer_owns_only_the_final_failed_assistant() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    crate::pending_messages::enqueue(
        &store,
        &PendingMessage {
            id: "steer-372".into(),
            thread_id: "thread-1".into(),
            generation: "owned-generation".into(),
            behavior: PendingMessageBehavior::Steer,
            content: "continue with my correction".into(),
            images: Vec::new(),
        },
    )
    .unwrap();
    let source = crate::pending_messages::QueueSteeringSource::new(
        store.database_path().unwrap().into(),
        dir.path().into(),
        "thread-1".into(),
        "owned-generation".into(),
        Arc::new(|| Ok(Vec::new())),
        CancellationToken::new(),
    );
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("first response".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(turn_failure())],
    ]);
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "original goal",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_pending_input(None, Some(Arc::new(source))),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(run.failed);
    let rows = messages::recent(store.conn(), "thread-1", 20).unwrap();
    let initial = &rows[1];
    assert_eq!(initial.status, "done");
    assert_eq!(rows[2].id, "steer-372");
    assert_eq!(rows[3].status, "failed");
    assert_ne!(initial.id, run.assistant_message_id);
    let diagnostics =
        vega_store::run_diagnostics::read_by_run(store.conn(), "thread-1", &initial.id).unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|event| event.event.phase == DiagnosticPhase::PrimaryModel
                && event.event.failure_code == Some(DiagnosticFailureCode::ReasoningLimit))
    );
    assert_eq!(store.conn().execute("UPDATE run_diagnostic_events SET failure_code = 'unknown_safe_failure' WHERE thread_id = 'thread-1' AND run_id = ?1 AND phase = 'run' AND state = 'failed'", [&initial.id]).unwrap(), 1);
    assert_eq!(
        read_context_projection(&store, "thread-1", "mock-model", "system")
            .unwrap()
            .run_failures,
        [(
            run.assistant_message_id.clone(),
            RunFailureKind::ReasoningLimit(None)
        )]
    );
    let mut root = diagnostics
        .iter()
        .find(|event| {
            event.event.phase == DiagnosticPhase::Run
                && event.event.state == DiagnosticState::Failed
        })
        .unwrap()
        .event
        .clone();
    store.conn().execute("DELETE FROM run_diagnostic_events WHERE thread_id = 'thread-1' AND run_id = ?1 AND phase = 'run' AND state = 'failed'", [&initial.id]).unwrap();
    let operation_key = format!(
        "{}:context-generation-1:attempt-one",
        run.assistant_message_id
    );
    let mut validation = vega_store::run_diagnostics::NewDiagnosticEvent {
        thread_id: "thread-1".into(),
        run_id: initial.id.clone(),
        attempt_id: operation_key.clone(),
        parent_attempt_id: Some(root.attempt_id.clone()),
        tool_call_id: None,
        phase: DiagnosticPhase::ContextSummary,
        state: DiagnosticState::Started,
        failure_code: None,
        occurred_at: root.occurred_at,
        duration_ms: None,
        metrics: vega_store::run_diagnostics::DiagnosticMetrics::default(),
    };
    vega_store::run_diagnostics::insert(store.conn(), &validation).unwrap();
    validation.state = DiagnosticState::Failed;
    validation.failure_code = Some(DiagnosticFailureCode::ReasoningLimit);
    validation.duration_ms = Some(0);
    vega_store::run_diagnostics::insert(store.conn(), &validation).unwrap();
    root.failure_code = Some(DiagnosticFailureCode::ReasoningLimit);
    vega_store::run_diagnostics::insert(store.conn(), &root).unwrap();
    vega_store::context_compaction::insert_status(
        store.conn(),
        vega_store::context_compaction::NewContextCompactionStatus {
            thread_id: "thread-1",
            model: "mock-model",
            operation_key: &operation_key,
            generation: 1,
            phase: "failed",
            usage_state: "unknown",
            failure: Some("too_large"),
            source_version: 1,
            estimated_tokens: 900,
            input_budget: 1_000,
            target_tokens: 600,
            created_at: 3,
        },
    )
    .unwrap();
    let other = MockProvider::new(vec![ScriptStep::Error {
        status: None,
        message: "unrelated transport failure".into(),
        retryable: false,
    }]);
    let unrelated = run_thread_task(
        &store,
        &other,
        &tools,
        "thread-1",
        "independent request",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(unrelated.failed);
    let database = store.database_path().unwrap().to_owned();
    drop(store);
    let reopened = Store::open(database).unwrap();
    let projection =
        read_context_projection(&reopened, "thread-1", "mock-model", "system").unwrap();
    assert_eq!(
        projection.last_status_message_id.as_deref(),
        Some(run.assistant_message_id.as_str())
    );
    assert_eq!(
        projection.last_status.as_ref().unwrap().failure,
        Some(crate::types::ContextCompactionFailureCode::ReasoningLimit)
    );
    assert_eq!(
        projection.run_failures,
        [(
            run.assistant_message_id.clone(),
            RunFailureKind::ReasoningLimit(None)
        )]
    );
    assert!(
        !projection
            .run_failures
            .iter()
            .any(|(message_id, _)| message_id == &initial.id
                || message_id == &unrelated.assistant_message_id)
    );
    assert!(crate::history::latest_history_page(&reopened, "thread-1", 20).unwrap().entries.iter().any(|entry| matches!(entry, crate::history::HistoryEntry::AssistantText { message_id, status: crate::history::AssistantStatus::Failed, .. } if message_id == &run.assistant_message_id)));
    let stale_operation = format!("{}:context-generation-2:attempt-two", initial.id);
    vega_store::context_compaction::insert_status(
        reopened.conn(),
        vega_store::context_compaction::NewContextCompactionStatus {
            thread_id: "thread-1",
            model: "mock-model",
            operation_key: &stale_operation,
            generation: 2,
            phase: "failed",
            usage_state: "unknown",
            failure: Some("too_large"),
            source_version: 1,
            estimated_tokens: 900,
            input_budget: 1_000,
            target_tokens: 600,
            created_at: 4,
        },
    )
    .unwrap();
    let unrelated_status =
        read_context_projection(&reopened, "thread-1", "mock-model", "system").unwrap();
    assert_eq!(
        unrelated_status.last_status_message_id.as_deref(),
        Some(initial.id.as_str())
    );
    assert_eq!(
        unrelated_status.last_status.unwrap().failure,
        Some(crate::types::ContextCompactionFailureCode::TooLarge)
    );
}

struct LongThinkingTask {
    primary_calls: Arc<AtomicUsize>,
    summary_calls: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl Provider for LongThinkingTask {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.requests.lock().unwrap().push(request.clone());
        let events = if request.tools.is_empty() {
            self.summary_calls.fetch_add(1, Ordering::SeqCst);
            vec![ProviderEvent::TextDelta("Task goal: complete the task. Progress: completed prior read rounds. Latest correction: preserve paired results.".into()), ProviderEvent::Done { stop_reason: StopReason::End }]
        } else {
            let index = self.primary_calls.fetch_add(1, Ordering::SeqCst);
            if index == 10 {
                vec![
                    ProviderEvent::TextDelta("task finished".into()),
                    ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    },
                ]
            } else {
                let mut events = (0..4)
                    .map(|_| ProviderEvent::ThinkingDelta("r".repeat(REASONING_DELTA_MAX_BYTES)))
                    .collect::<Vec<_>>();
                events.extend([
                    ProviderEvent::TextDelta("progress ".repeat(4_000)),
                    ProviderEvent::ToolUse {
                        id: format!("read-372-{index}"),
                        name: "read".into(),
                        input_json: r#"{"path":"lib.rs"}"#.into(),
                    },
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ]);
                events
            }
        };
        Box::pin(async move {
            Ok(Box::pin(futures::stream::iter(events.into_iter().map(Ok))) as EventStream)
        })
    }
}

#[tokio::test]
async fn issue372_production_compaction_continues_same_task_and_bounds_conversation_buffer() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    vega_store::context_compaction::save_model_policy(
        store.conn(),
        &vega_store::context_compaction::ModelContextPolicy {
            provider: "fixture".into(),
            model: "mock-model".into(),
            input_limit: Some(45_000),
            output_reserve: Some(1_000),
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let primary_calls = Arc::new(AtomicUsize::new(0));
    let summary_calls = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let provider = LongThinkingTask {
        primary_calls: primary_calls.clone(),
        summary_calls: summary_calls.clone(),
        requests: requests.clone(),
    };
    let mut thinking_bytes = 0usize;
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Task goal: complete the task. Latest correction: preserve paired results.",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |event| {
            if let ConversationEvent::ThinkingDelta { delta, .. }
            | ConversationEvent::SummaryDelta { delta, .. } = event
            {
                thinking_bytes += delta.len();
            }
            Ok(())
        },
        PersistenceActorConfig::default(),
        None,
        None,
        Some(FrozenReasoning::unknown("fixture", "mock-model")),
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert!(!run.interrupted);
    assert!(run.content.ends_with("task finished"));
    assert_eq!(primary_calls.load(Ordering::SeqCst), 11);
    assert!(summary_calls.load(Ordering::SeqCst) >= 2);
    assert!(thinking_bytes > vega_runtime::REASONING_RUN_MAX_BYTES * 2);
    assert!(
        run.events
            .iter()
            .filter_map(|event| match event {
                ConversationEvent::ThinkingDelta { delta, .. }
                | ConversationEvent::SummaryDelta { delta, .. } => Some(delta.len()),
                _ => None,
            })
            .sum::<usize>()
            <= vega_runtime::REASONING_RUN_MAX_BYTES
    );
    let calls = vega_store::tool_calls::terminal_results(store.conn(), "thread-1").unwrap();
    assert_eq!(calls.len(), 10);
    assert!(calls.values().all(|call| call.status == "success"));
    let requests = requests.lock().unwrap();
    let last = requests
        .iter()
        .rfind(|request| !request.tools.is_empty())
        .unwrap();
    assert!(
        last.messages
            .iter()
            .any(|message| message.content.contains("Task goal")
                && message.content.contains("Progress")
                && message.content.contains("Latest correction"))
    );
    for message in last
        .messages
        .iter()
        .filter(|message| !message.tool_calls.is_empty())
    {
        for call in &message.tool_calls {
            assert!(
                last.messages
                    .iter()
                    .any(|result| result.tool_call_id.as_deref() == Some(call.id.as_str()))
            );
        }
    }
    let raw = messages::recent(store.conn(), "thread-1", 20).unwrap();
    assert_eq!(raw.len(), 2);
    assert_eq!(
        raw[0].content,
        "Task goal: complete the task. Latest correction: preserve paired results."
    );
    assert_eq!(raw[1].content, run.content);
    assert_eq!(raw[1].status, "done");
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_some()
    );
}
