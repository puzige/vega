use super::*;
use crate::types::{PendingMessage, PendingMessageBehavior};

fn queued(id: &str, behavior: PendingMessageBehavior, content: &str) -> PendingMessage {
    PendingMessage {
        id: id.into(),
        thread_id: "thread-1".into(),
        generation: "run-1".into(),
        behavior,
        content: content.into(),
        images: Vec::new(),
    }
}

#[tokio::test]
async fn issue299_multiple_steers_persist_causal_history_and_reference_snapshot() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    crate::pending_messages::enqueue(
        &store,
        &queued("pending-b", PendingMessageBehavior::Steer, "Read @lib.rs"),
    )
    .unwrap();
    crate::pending_messages::enqueue(
        &store,
        &queued(
            "pending-c",
            PendingMessageBehavior::Steer,
            "Then explain it",
        ),
    )
    .unwrap();
    let source = crate::pending_messages::QueueSteeringSource::new(
        store.database_path().unwrap().into(),
        dir.path().into(),
        "thread-1".into(),
        "run-1".into(),
        Arc::new(|| Ok(Vec::new())),
        CancellationToken::new(),
    );
    let provider = MockProvider::new_rounds(
        ["answer-a", "answer-b", "answer-c"]
            .into_iter()
            .map(|text| {
                let mut steps = Vec::new();
                if text == "answer-b" {
                    steps.extend([
                        ScriptStep::text("discarded response tail"),
                        ScriptStep::events(vec![ProviderEvent::RetryScheduled {
                            attempt: 1,
                            max_retries: 5,
                        }]),
                    ]);
                }
                steps.extend([
                    ScriptStep::text(text),
                    ScriptStep::events(vec![ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    }]),
                ]);
                steps
            })
            .collect(),
    );
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "original",
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
    assert!(!run.failed);
    assert_eq!(run.content, "answer-c");
    let rows = messages::recent(store.conn(), "thread-1", 50).unwrap();
    let ordered = rows
        .iter()
        .map(|message| (message.role.as_str(), message.content.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        ordered,
        [
            ("user", "original"),
            ("assistant", "answer-a"),
            ("user", "Read @lib.rs"),
            ("assistant", "answer-b"),
            ("user", "Then explain it"),
            ("assistant", "answer-c")
        ]
    );
    assert!(rows.iter().all(|message| message.status == "done"));
    let retries = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ResponseRetrying {
                message_id,
                text_start_bytes,
                ..
            } => Some((message_id.as_str(), *text_start_bytes)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(retries, [(rows[3].id.as_str(), 0)]);
    let page =
        messages::page_before(store.conn(), "thread-1", messages::PageCursor::Head, 50).unwrap();
    assert_eq!(page.assistant_responses.len(), 3);
    for (response, assistant) in page
        .assistant_responses
        .iter()
        .zip(rows.iter().filter(|message| message.role == "assistant"))
    {
        assert_eq!(response.message_id, assistant.id);
        assert_eq!(response.text_start_bytes, 0);
        assert_eq!(response.text_end_bytes, assistant.content.len());
        assert_eq!(response.kind, "final");
    }
    assert!(
        crate::pending_messages::load(&store, "thread-1")
            .unwrap()
            .is_empty()
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.role == vega_runtime::ChatRole::User
                && message.content.contains("TODO: persist me"))
    );
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let source = vega_store::context_compaction::load_source(reopened.conn(), "thread-1").unwrap();
    assert!(
        source
            .messages
            .iter()
            .find(|message| message.id == "pending-b")
            .unwrap()
            .content
            .contains("TODO: persist me")
    );
    let history = crate::history::latest_history_page(&reopened, "thread-1", 50).unwrap();
    assert!(!format!("{history:?}").contains("TODO: persist me"));
    assert_eq!(
        run.events
            .iter()
            .filter(|event| matches!(event, ConversationEvent::PendingMessageDelivered { .. }))
            .count(),
        2
    );
}

#[tokio::test]
async fn issue299_steer_insert_failure_rolls_back_delivery_and_preserves_response() {
    let (store, dir, project_id) = setup();
    let prior = messages::MessageRow {
        id: "old-assistant".into(),
        thread_id: "thread-1".into(),
        seq: 1,
        role: "assistant".into(),
        kind: "text".into(),
        content: "answer-a".into(),
        status: "streaming".into(),
        created_at: 1,
        plan_status: None,
        plan_review_note: None,
        plan_reviewed_at: None,
    };
    messages::insert(store.conn(), &prior).unwrap();
    let response = messages::AssistantResponseRow {
        message_id: prior.id.clone(),
        response_id: "old-response".into(),
        text_start_bytes: 0,
        text_end_bytes: prior.content.len(),
        kind: "final".into(),
    };
    assert!(
        messages::record_assistant_response(store.conn(), &prior.id, &prior.content, &response)
            .unwrap()
    );
    crate::pending_messages::enqueue(
        &store,
        &queued("pending-b", PendingMessageBehavior::Steer, "continue"),
    )
    .unwrap();
    let source = crate::pending_messages::QueueSteeringSource::new(
        store.database_path().unwrap().into(),
        dir.path().into(),
        "thread-1".into(),
        "run-1".into(),
        Arc::new(|| Ok(Vec::new())),
        CancellationToken::new(),
    );
    let mut input = vega_runtime::RuntimeSteeringSource::take(&source)
        .await
        .unwrap()
        .unwrap();
    input.assistant_message_id = prior.id.clone();
    let original_pending = vega_store::pending_messages::find(store.conn(), "thread-1", &input.id)
        .unwrap()
        .unwrap();
    let mut next_tool_seq = 0;
    let error = persist_runtime_event(
        &store,
        &project_id,
        "thread-1",
        &prior.id,
        "mock-model",
        false,
        &prior.content,
        &mut next_tool_seq,
        &RuntimeEvent::SteeringAccepted(input.clone()),
        Some(24),
    )
    .unwrap_err();
    assert!(
        matches!(error, VegaError::Store(error) if error.to_string() == "UNIQUE constraint failed: messages.id")
    );
    assert_eq!(
        messages::find(store.conn(), &prior.id).unwrap(),
        Some(prior.clone())
    );
    assert!(messages::find(store.conn(), &input.id).unwrap().is_none());
    let pending: (String, String, Option<String>) = store
        .conn()
        .query_row(
            "SELECT state, payload, model_content FROM pending_messages WHERE id = ?1",
            [&input.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(pending, ("claimed".into(), original_pending.payload, None));
    let durations: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM assistant_run_durations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(durations, 0);
    let response_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM assistant_responses WHERE message_id = ?1",
            [&prior.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(response_count, 1);
    input.assistant_message_id = "new-assistant".into();
    persist_runtime_event(
        &store,
        &project_id,
        "thread-1",
        &prior.id,
        "mock-model",
        false,
        &prior.content,
        &mut next_tool_seq,
        &RuntimeEvent::SteeringAccepted(input.clone()),
        Some(24),
    )
    .unwrap();
    let page =
        messages::page_before(store.conn(), "thread-1", messages::PageCursor::Head, 50).unwrap();
    assert_eq!(page.assistant_responses, [response]);
    assert_eq!(page.execution_durations_ms.get(&prior.id), Some(&24));
    assert_eq!(
        messages::find(store.conn(), &prior.id)
            .unwrap()
            .unwrap()
            .status,
        "done"
    );
    assert_eq!(
        messages::find(store.conn(), &input.id)
            .unwrap()
            .unwrap()
            .content,
        "continue"
    );
    assert_eq!(
        messages::find(store.conn(), &input.assistant_message_id)
            .unwrap()
            .unwrap()
            .status,
        "streaming"
    );
    assert!(
        crate::pending_messages::load(&store, "thread-1")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn issue299_queue_delivery_is_atomic_and_cannot_replay() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    crate::pending_messages::enqueue(
        &store,
        &queued("pending-b", PendingMessageBehavior::Queue, "second"),
    )
    .unwrap();
    let provider = MockProvider::new(vec![
        ScriptStep::text("done"),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]);
    let config =
        PersistenceActorConfig::default().with_pending_input(Some("pending-b".into()), None);
    let first = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "second",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        config.clone(),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert_eq!(first.user_message_id, "pending-b");
    let retry = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "second",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        config,
        None,
        None,
        None,
        Vec::new(),
    )
    .await;
    assert!(retry.is_err());
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(
        messages::recent(store.conn(), "thread-1", 50)
            .unwrap()
            .len(),
        2
    );
    assert!(
        crate::pending_messages::recover(&store, "thread-1")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn issue299_stale_generation_and_failed_preparation_keep_pending_message() {
    let (store, dir, _) = setup();
    crate::pending_messages::enqueue(
        &store,
        &queued(
            "pending-b",
            PendingMessageBehavior::Steer,
            "missing @absent.rs",
        ),
    )
    .unwrap();
    let old = crate::pending_messages::QueueSteeringSource::new(
        store.database_path().unwrap().into(),
        dir.path().into(),
        "thread-1".into(),
        "another-run".into(),
        Arc::new(|| Ok(Vec::new())),
        CancellationToken::new(),
    );
    assert!(
        vega_runtime::RuntimeSteeringSource::take(&old)
            .await
            .unwrap()
            .is_none()
    );
    let current = crate::pending_messages::QueueSteeringSource::new(
        store.database_path().unwrap().into(),
        dir.path().into(),
        "thread-1".into(),
        "run-1".into(),
        Arc::new(|| Ok(Vec::new())),
        CancellationToken::new(),
    );
    assert!(
        vega_runtime::RuntimeSteeringSource::take(&current)
            .await
            .is_err()
    );
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let recovered = crate::pending_messages::recover(&reopened, "thread-1").unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].content, "missing @absent.rs");
}

#[test]
fn issue299_enqueue_projection_failure_rolls_back_new_message() {
    let (store, _, _) = setup();
    vega_store::pending_messages::insert(
        store.conn(),
        &vega_store::pending_messages::PendingMessageRow {
            id: "older-corrupt".into(),
            thread_id: "thread-1".into(),
            ordinal: 0,
            behavior: "queue".into(),
            generation: "run-1".into(),
            payload: "invalid fixture payload".into(),
            created_at: 1,
        },
    )
    .unwrap();
    assert!(
        crate::pending_messages::enqueue(
            &store,
            &queued(
                "not-accepted",
                PendingMessageBehavior::Queue,
                "retain draft"
            )
        )
        .is_err()
    );
    assert!(
        vega_store::pending_messages::find(store.conn(), "thread-1", "not-accepted")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        vega_store::pending_messages::count(store.conn(), "thread-1").unwrap(),
        1
    );
}

#[test]
fn issue299_cached_projection_reuses_payload_and_refreshes_behavior_without_cross_thread_alias() {
    let (store, _, _) = setup();
    let message = queued("cached", PendingMessageBehavior::Queue, "original payload");
    let cached = crate::pending_messages::enqueue(&store, &message).unwrap();
    crate::pending_messages::promote(&store, "thread-1", "cached", "run-2").unwrap();
    let refreshed = crate::pending_messages::load_cached(&store, "thread-1", &cached).unwrap();
    assert_eq!(refreshed[0].content, message.content);
    assert_eq!(refreshed[0].generation, "run-2");
    assert_eq!(refreshed[0].behavior, PendingMessageBehavior::Steer);
    let mut foreign = cached;
    foreign[0].thread_id = "foreign-thread".into();
    foreign[0].content = "must not reuse".into();
    let refreshed = crate::pending_messages::load_cached(&store, "thread-1", &foreign).unwrap();
    assert_eq!(refreshed[0].content, message.content);
}
