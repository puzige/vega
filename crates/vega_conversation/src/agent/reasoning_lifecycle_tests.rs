use super::{PersistenceActor, PersistenceActorConfig, PersistenceActorStart};
use crate::types::{ConversationEvent, ConversationMeter, from_runtime_event};
use vega_runtime::RuntimeEvent;
use vega_store::{Store, messages, projects, threads, tool_calls};

#[test]
fn response_projection_preserves_identity_and_order_without_ending_the_meter() {
    let message_id = "assistant";
    let runtime_events = [
        RuntimeEvent::AssistantResponseStarted {
            response_id: "first".into(),
            text_start_bytes: 0,
        },
        RuntimeEvent::SummaryDelta("Checking".into()),
        RuntimeEvent::ReasoningCompleted {
            response_id: "first".into(),
        },
        RuntimeEvent::AssistantResponseStarted {
            response_id: "second".into(),
            text_start_bytes: 0,
        },
        RuntimeEvent::ReasoningCompleted {
            response_id: "first".into(),
        },
        RuntimeEvent::ReasoningCompleted {
            response_id: "second".into(),
        },
    ];
    let events = runtime_events
        .iter()
        .map(|event| from_runtime_event(message_id, event).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 6);
    for (event, expected_id) in [(&events[0], "first"), (&events[3], "second")] {
        assert!(
            matches!(event, ConversationEvent::AssistantResponseStarted {
            message_id: actual_message, response_id, text_start_bytes: 0,
        } if actual_message == message_id && response_id == expected_id)
        );
    }
    assert!(matches!(&events[1], ConversationEvent::SummaryDelta {
        message_id: actual_message, delta,
    } if actual_message == message_id && delta == "Checking"));
    for (event, expected_id) in [
        (&events[2], "first"),
        (&events[4], "first"),
        (&events[5], "second"),
    ] {
        assert!(matches!(event, ConversationEvent::ReasoningCompleted {
            message_id: actual_message, response_id,
        } if actual_message == message_id && response_id == expected_id));
    }
    let mut meter = ConversationMeter::default();
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: message_id.into(),
        seq: 1,
    });
    assert!(meter.apply(&ConversationEvent::TextDelta {
        message_id: message_id.into(),
        delta: "abcd".into(),
    }));
    let before = meter.snapshot();
    assert!(before.provisional);
    assert_eq!(before.tokens, 1);
    for event in &events {
        assert!(!meter.apply(event));
        assert_eq!(meter.snapshot(), before);
    }
    assert!(meter.apply(&ConversationEvent::TextDelta {
        message_id: message_id.into(),
        delta: "efgh".into(),
    }));
    assert_eq!(meter.snapshot().tokens, 2);
    assert!(meter.snapshot().provisional);
}

#[tokio::test]
async fn reasoning_completion_is_transient_through_the_owned_store_actor() {
    let root = tempfile::tempdir().unwrap();
    let database_path = root.path().join("conversation.sqlite");
    let store = Store::open(&database_path).unwrap();
    store.migrate().unwrap();
    let project = projects::create(
        store.conn(),
        root.path().to_str().unwrap(),
        "Lifecycle",
        None,
    )
    .unwrap();
    threads::create(
        store.conn(),
        threads::NewThread {
            id: "thread",
            project_id: &project.id,
            title: "Lifecycle",
            mode: "ask",
            permission_mode: "readonly",
            model: "scripted",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "assistant".into(),
            thread_id: "thread".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    tool_calls::insert(
        store.conn(),
        tool_calls::NewToolCall {
            id: "parallel-tool",
            thread_id: "thread",
            message_id: "assistant",
            seq: 1,
            tool: "read",
            input_json: "{}",
            status: "running",
            created_at: 1,
        },
    )
    .unwrap();
    let tool_before = tool_calls::find_state(store.conn(), "parallel-tool")
        .unwrap()
        .unwrap();
    let actor = PersistenceActor::start(PersistenceActorStart {
        database_path,
        project_id: project.id,
        thread_id: "thread".into(),
        message_id: "assistant".into(),
        model: "scripted".into(),
        is_plan: false,
        next_tool_seq: 2,
        config: PersistenceActorConfig::default(),
    })
    .await
    .unwrap();
    for response_id in ["first", "first", "second"] {
        actor
            .event(
                RuntimeEvent::ReasoningCompleted {
                    response_id: response_id.into(),
                },
                String::new(),
                None,
            )
            .await
            .unwrap();
    }
    actor.close().await.unwrap();
    let (content, status): (String, String) = store
        .conn()
        .query_row(
            "SELECT content, status FROM messages WHERE id = 'assistant'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(content, "");
    assert_eq!(status, "streaming");
    let thread_status: String = store
        .conn()
        .query_row(
            "SELECT status FROM threads WHERE id = 'thread'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(thread_status, "active");
    assert_eq!(
        tool_calls::find_state(store.conn(), "parallel-tool")
            .unwrap()
            .unwrap(),
        tool_before,
    );
    for (query, expected) in [
        ("SELECT COUNT(*) FROM messages", 1),
        ("SELECT COUNT(*) FROM tool_calls", 1),
        ("SELECT COUNT(*) FROM assistant_responses", 0),
        ("SELECT COUNT(*) FROM tool_call_rounds", 0),
        ("SELECT COUNT(*) FROM token_usage", 0),
        ("SELECT COUNT(*) FROM assistant_run_durations", 0),
    ] {
        let count: i64 = store.conn().query_row(query, [], |row| row.get(0)).unwrap();
        assert_eq!(count, expected, "{query}");
    }
}
