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
                vec![
                    ScriptStep::text(text),
                    ScriptStep::events(vec![ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    }]),
                ]
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
