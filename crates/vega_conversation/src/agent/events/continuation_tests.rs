use super::*;
use vega_runtime::{ChatMessage, ChatRole, RuntimeSteeringInput};
use vega_store::{messages::PageCursor, pending_messages, threads};

fn fixture() -> Store {
    fixture_at(std::path::Path::new(":memory:"))
}

fn fixture_at(path: &std::path::Path) -> Store {
    let store = Store::open(path).unwrap();
    store.migrate().unwrap();
    threads::create_standalone(
        store.conn(),
        threads::NewThread {
            id: "thread",
            project_id: "",
            title: "Continuation",
            mode: "ask",
            permission_mode: "readonly",
            model: "model",
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
            id: "previous".into(),
            thread_id: "thread".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: "before".into(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    pending_messages::insert(
        store.conn(),
        &pending_messages::PendingMessageRow {
            id: "user".into(),
            thread_id: "thread".into(),
            ordinal: 0,
            behavior: "steer".into(),
            generation: "unrelated-generation".into(),
            payload: "original payload".into(),
            created_at: 2,
        },
    )
    .unwrap();
    store
}

fn input(seq: i64) -> RuntimeEvent {
    RuntimeEvent::SteeringAccepted(RuntimeSteeringInput {
        original_content: "original instruction".into(),
        user_message_seq: seq,
        id: "user".into(),
        assistant_message_id: "successor".into(),
        message: ChatMessage::new(ChatRole::User, "expanded model instruction"),
    })
}

fn accept(store: &Store, seq: i64) -> Result<(), VegaError> {
    persist_runtime_event(
        store,
        "",
        "thread",
        "previous",
        "model",
        false,
        "before acceptance",
        &mut 0,
        &input(seq),
        None,
    )
}

fn predecessor(store: &Store) -> Option<String> {
    store
        .conn()
        .query_row(
            "SELECT accepted_after_assistant_id FROM pending_messages WHERE id = 'user'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn acceptance_atomically_records_exact_predecessor_without_segment_duration() {
    let store = fixture();
    accept(&store, 2).unwrap();
    assert_eq!(predecessor(&store).as_deref(), Some("previous"));
    let rows: Vec<(String, String, String, i64)> = store
        .conn()
        .prepare("SELECT id, content, status, seq FROM messages ORDER BY seq")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            (
                "previous".into(),
                "before acceptance".into(),
                "done".into(),
                1
            ),
            (
                "user".into(),
                "original instruction".into(),
                "done".into(),
                2
            ),
            ("successor".into(), "".into(), "streaming".into(), 3),
        ]
    );
    let page = messages::page_before(store.conn(), "thread", PageCursor::Head, 10).unwrap();
    assert!(!page.execution_durations_ms.contains_key("previous"));
    assert!(
        pending_messages::find(store.conn(), "thread", "user")
            .unwrap()
            .is_none()
    );
    assert!(accept(&store, 2).is_err());
    assert_eq!(predecessor(&store).as_deref(), Some("previous"));
    assert_eq!(messages::next_seq(store.conn(), "thread").unwrap(), 4);
}

#[test]
fn rejected_acceptance_rolls_back_delivery_relation_and_messages() {
    let store = fixture();
    assert!(accept(&store, 99).is_err());
    assert_eq!(predecessor(&store), None);
    assert_eq!(messages::next_seq(store.conn(), "thread").unwrap(), 2);
    assert_eq!(
        pending_messages::find(store.conn(), "thread", "user")
            .unwrap()
            .unwrap()
            .payload,
        "original payload"
    );
    let status: String = store
        .conn()
        .query_row(
            "SELECT status FROM messages WHERE id = 'previous'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "streaming");
}

#[test]
fn steer_delivered_as_independent_run_has_no_continuation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("conversation.sqlite");
    let store = fixture_at(&path);
    store
        .conn()
        .execute(
            "UPDATE pending_messages SET payload = ?1 WHERE id = 'user'",
            [r#"{"content":"Independent instruction","images":[]}"#],
        )
        .unwrap();
    assert!(
        pending_messages::claim_steer(store.conn(), "thread", "user", "unrelated-generation")
            .unwrap()
    );
    pending_messages::release_claims(store.conn(), "thread").unwrap();
    messages::finish(store.conn(), "previous", "before", "done").unwrap();
    super::super::pipeline::prepare_run_with_images_and_reasoning(
        path,
        "thread".into(),
        "Independent instruction".into(),
        String::new(),
        "user".into(),
        "successor".into(),
        PersistenceActorConfig {
            pending_message_id: Some("user".into()),
            ..Default::default()
        },
        false,
        None,
        None,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(predecessor(&store), None);
    let rows: Vec<(String, String)> = store
        .conn()
        .prepare("SELECT id, role FROM messages WHERE id IN ('user', 'successor') ORDER BY seq")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            ("user".into(), "user".into()),
            ("successor".into(), "assistant".into())
        ]
    );
    let page = messages::page_before(store.conn(), "thread", PageCursor::Head, 10).unwrap();
    assert!(page.continuations.is_empty());
}

#[test]
fn every_page_resolves_explicit_successor_outside_its_rows() {
    let store = fixture();
    accept(&store, 2).unwrap();
    let pages = [
        messages::page_before(store.conn(), "thread", PageCursor::Before(2), 1).unwrap(),
        messages::page_after(store.conn(), "thread", 0, 1).unwrap(),
        messages::page_containing_message(store.conn(), "thread", "previous", 1)
            .unwrap()
            .unwrap(),
    ];
    for page in pages {
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].id, "previous");
        assert_eq!(page.continuations.len(), 1);
        let relation = &page.continuations[0];
        assert_eq!(relation.previous_assistant_id, "previous");
        assert_eq!(relation.user_message_id, "user");
        assert_eq!(relation.next_assistant_id, "successor");
    }
}

#[test]
fn missing_duration_and_steer_metadata_do_not_reconstruct_legacy_relation() {
    let store = fixture();
    accept(&store, 2).unwrap();
    store
        .conn()
        .execute(
            "UPDATE pending_messages SET accepted_after_assistant_id = NULL",
            [],
        )
        .unwrap();
    let page = messages::page_before(store.conn(), "thread", PageCursor::Head, 10).unwrap();
    assert!(page.continuations.is_empty());
    assert!(!page.execution_durations_ms.contains_key("previous"));
}

#[tokio::test]
async fn accepted_input_emits_continuation_before_delivery_without_finishing_run() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("conversation.sqlite");
    let store = fixture_at(&path);
    let actor = PersistenceActor::start(super::super::persistence::PersistenceActorStart {
        database_path: path,
        project_id: String::new(),
        thread_id: "thread".into(),
        message_id: "previous".into(),
        model: "model".into(),
        is_plan: false,
        next_tool_seq: 0,
        config: Default::default(),
    })
    .await
    .unwrap();
    let (sender, receiver) = mpsc::channel(4);
    let (ack, acknowledged) = tokio::sync::oneshot::channel();
    sender
        .send(RuntimeEnvelope {
            event: input(2),
            ack: Some(ack),
        })
        .await
        .unwrap();
    drop(sender);
    let started_at = std::time::Instant::now() - std::time::Duration::from_secs(12);
    let mut content = "before acceptance".to_owned();
    let mut events = Vec::new();
    let mut delivered = Vec::new();
    let mut sink = |event: &ConversationEvent| {
        delivered.push(event.clone());
        Ok(())
    };
    let diagnostics = super::super::diagnostics::DiagnosticsWriter::buffered();
    process_runtime_events(
        receiver,
        &actor,
        RuntimeEventContext {
            message_id: "previous",
            streamed_content: &mut content,
            events: &mut events,
            event_sink: &mut sink,
            cancel: CancellationToken::new(),
            run_started_at: started_at,
            diagnostics: &diagnostics,
            thread_id: "thread",
            run_id: "run",
            root_attempt_id: "attempt",
            subagent_receipts: None,
        },
    )
    .await
    .unwrap();
    acknowledged.await.unwrap().unwrap();
    actor.close().await.unwrap();
    assert!(content.is_empty());
    for output in [&events, &delivered] {
        assert_eq!(output.len(), 4);
        assert!(matches!(&output[0], ConversationEvent::RunContinued {
            previous_assistant_id, user_message_id, next_assistant_id,
        } if previous_assistant_id == "previous" && user_message_id == "user" && next_assistant_id == "successor"));
        assert!(
            matches!(&output[1], ConversationEvent::PendingMessageDelivered {
            pending_id, message_id, seq: 2, content, images,
        } if pending_id == "user" && message_id == "user" && content == "original instruction" && images.is_empty())
        );
        assert!(
            matches!(&output[2], ConversationEvent::MessageStarted { message_id, seq: 3 } if message_id == "successor")
        );
        assert!(
            matches!(&output[3], ConversationEvent::RunStarted { message_id, started_at: actual } if message_id == "successor" && *actual == started_at)
        );
        assert!(
            !output
                .iter()
                .any(|event| matches!(event, ConversationEvent::MessageFinished { .. }))
        );
    }
    assert_eq!(predecessor(&store).as_deref(), Some("previous"));
}

#[test]
fn history_projection_carries_page_external_continuation_without_inventing_duration() {
    let store = fixture();
    accept(&store, 2).unwrap();
    let pages = [
        crate::history::history_page_before(&store, "thread", PageCursor::Before(2), 1).unwrap(),
        crate::history::history_page_after(&store, "thread", 0, 1).unwrap(),
        crate::history::history_page_containing_message(&store, "thread", "previous", 1)
            .unwrap()
            .unwrap(),
    ];
    for page in pages {
        assert!(page.entries.iter().any(|entry| matches!(entry,
            crate::history::HistoryEntry::RunContinuation { previous_assistant_id, user_message_id, next_assistant_id }
            if previous_assistant_id == "previous" && user_message_id == "user" && next_assistant_id == "successor"
        )));
        assert!(page.entries.iter().any(|entry| matches!(entry,
            crate::history::HistoryEntry::AssistantText { message_id, content, execution_duration_ms: None, .. }
            if message_id == "previous" && content == "before acceptance"
        )));
    }
}

struct ScriptedProvider(
    std::sync::Mutex<std::collections::VecDeque<Vec<vega_runtime::ProviderEvent>>>,
);

impl vega_runtime::Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        _: vega_runtime::ChatRequest,
        _: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let events = self
            .0
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected provider round");
        Box::pin(async move {
            Ok(Box::pin(futures::stream::iter(events.into_iter().map(Ok)))
                as vega_runtime::EventStream)
        })
    }
}

struct SteeringOnce(std::sync::Mutex<Option<RuntimeSteeringInput>>);

impl vega_runtime::RuntimeSteeringSource for SteeringOnce {
    fn take(
        &self,
    ) -> futures::future::BoxFuture<'_, Result<Option<RuntimeSteeringInput>, VegaError>> {
        let input = self.0.lock().unwrap().take();
        Box::pin(async move { Ok(input) })
    }
}

#[tokio::test]
async fn response_end_and_tool_batch_acceptance_keep_one_runtime_until_successor_finishes() {
    use vega_runtime::{ProviderEvent, StopReason};
    for tool_boundary in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("input.txt"), "retained tool output").unwrap();
        let tools = vega_tools::Tools::new(directory.path()).unwrap();
        let first = if tool_boundary {
            vec![
                ProviderEvent::ToolUse {
                    id: "read-call".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"input.txt"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ]
        } else {
            vec![
                ProviderEvent::TextDelta("before".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]
        };
        let provider = ScriptedProvider(std::sync::Mutex::new(std::collections::VecDeque::from([
            first,
            vec![
                ProviderEvent::TextDelta("after".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ],
        ])));
        let RuntimeEvent::SteeringAccepted(input) = input(2) else {
            unreachable!()
        };
        let request = vega_runtime::AgentRequest {
            model: "model".into(),
            system_prompt: "Read the task".into(),
            history: vec![ChatMessage::new(ChatRole::User, "Start")],
            max_tokens: None,
            completed_tool_results: Default::default(),
            tool_config: vega_runtime::RuntimeToolConfig::default()
                .with_steering_source(Arc::new(SteeringOnce(std::sync::Mutex::new(Some(input))))),
            pricing_catalog: None,
            reasoning: None,
            model_snapshot: None,
            replay_frame: None,
            context_budget: None,
            context_source_version: None,
            context_source_fingerprint: None,
            context_operation_id: None,
            context_compaction_hook: None,
        };
        let outcome = vega_runtime::run_agent(&provider, &tools, request, CancellationToken::new())
            .await
            .unwrap();
        let accepted = outcome
            .events
            .iter()
            .position(|event| matches!(event, RuntimeEvent::SteeringAccepted(_)))
            .unwrap();
        assert_eq!(
            outcome
                .events
                .iter()
                .filter(|event| matches!(event, RuntimeEvent::SteeringAccepted(_)))
                .count(),
            1
        );
        assert!(!outcome.events[..accepted].iter().any(|event| matches!(
            event,
            RuntimeEvent::Finished(_) | RuntimeEvent::Interrupted | RuntimeEvent::Error(_)
        )));
        assert_eq!(
            outcome
                .events
                .iter()
                .filter(|event| matches!(event, RuntimeEvent::Finished(_)))
                .count(),
            1
        );
        assert!(
            outcome.events[accepted + 1..]
                .iter()
                .any(|event| matches!(event, RuntimeEvent::TextDelta(text) if text == "after"))
        );
        if tool_boundary {
            assert!(outcome.events[..accepted].iter().any(|event| matches!(event, RuntimeEvent::ToolCallFinished(result) if result.call_id == "read-call" && result.output.contains("retained tool output"))));
        }
        assert!(provider.0.lock().unwrap().is_empty());
    }
}

#[test]
fn malformed_successor_does_not_merge_history() {
    let store = fixture();
    accept(&store, 2).unwrap();
    store
        .conn()
        .execute(
            "UPDATE messages SET role = 'user' WHERE id = 'successor'",
            [],
        )
        .unwrap();
    let page = messages::page_before(store.conn(), "thread", PageCursor::Head, 10).unwrap();
    assert!(page.continuations.is_empty());
}
