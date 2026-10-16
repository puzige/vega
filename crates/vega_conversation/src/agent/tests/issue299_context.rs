use super::*;
use crate::history::{HistoryEntry, latest_history_page};
use crate::types::{PendingMessage, PendingMessageBehavior};
use vega_runtime::{ChatMessage, ChatRole};
use vega_store::context_compaction::{
    ModelContextPolicy, latest_checkpoint, latest_status, load_source, save_model_policy,
};

fn pending(id: &str, behavior: PendingMessageBehavior, content: &str) -> PendingMessage {
    PendingMessage {
        id: id.into(),
        thread_id: "thread-1".into(),
        generation: "run-1".into(),
        behavior,
        content: content.into(),
        images: Vec::new(),
    }
}

fn final_response(text: &str) -> Vec<ScriptStep> {
    vec![
        ScriptStep::text(text),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]
}

#[tokio::test]
async fn issue299_queued_reference_preserves_raw_history_and_frozen_model_snapshot() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let original_file = "pub const FIXTURE: &str = \"queued-snapshot-original\";\n";
    fs::write(dir.path().join("lib.rs"), original_file).unwrap();
    let raw = "Explain @lib.rs";
    let message = pending("queued-reference", PendingMessageBehavior::Queue, raw);
    crate::pending_messages::enqueue(&store, &message).unwrap();
    let references = vega_tools::reference::resolve_bounded_references(
        dir.path(),
        raw,
        vega_tools::reference::REFERENCE_MAX_FILES,
        vega_tools::reference::REFERENCE_MAX_FILE_BYTES,
        vega_tools::reference::REFERENCE_MAX_TOTAL_BYTES,
    )
    .unwrap();
    assert_eq!(references.len(), 1);
    let expanded = format!(
        "{}\n\n{raw}",
        vega_tools::reference::render_reference_block(&references)
    );
    let provider = MockProvider::new_rounds(vec![
        final_response("snapshot understood"),
        final_response("continued from the snapshot"),
    ]);
    let config =
        PersistenceActorConfig::default().with_pending_input(Some(message.id.clone()), None);
    let first = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        &expanded,
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
    assert!(!first.failed);
    assert_eq!(first.user_message_id, message.id);
    assert_eq!(
        messages::find(store.conn(), &message.id)
            .unwrap()
            .unwrap()
            .content,
        raw
    );
    let (state, payload, snapshot): (String, String, String) = store
        .conn()
        .query_row(
            "SELECT state, payload, model_content FROM pending_messages WHERE thread_id = ?1 AND id = ?2",
            ("thread-1", &message.id),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, "delivered");
    assert!(payload.is_empty());
    assert_eq!(snapshot, expanded);
    let initial_requests = provider.requests();
    assert_eq!(initial_requests.len(), 1);
    assert_eq!(
        initial_requests[0]
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::User)
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>(),
        [expanded.as_str()]
    );
    let source = load_source(store.conn(), "thread-1").unwrap();
    assert_eq!(
        source
            .messages
            .iter()
            .find(|row| row.id == message.id)
            .unwrap()
            .content,
        expanded
    );
    let database_path = store.database_path().unwrap().to_path_buf();
    drop(store);
    fs::write(dir.path().join("lib.rs"), "queued-snapshot-new-file").unwrap();
    let reopened = Store::open(database_path).unwrap();
    assert_eq!(
        load_source(reopened.conn(), "thread-1")
            .unwrap()
            .fingerprint,
        source.fingerprint
    );
    let history = latest_history_page(&reopened, "thread-1", 50).unwrap();
    let raw_history = history
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::UserText {
                message_id,
                content,
                ..
            } if message_id == &message.id => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(raw_history, [raw]);
    let replay = run_thread_task_with_images_and_reasoning(
        &reopened,
        &provider,
        &tools,
        "thread-1",
        &expanded,
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
    assert!(replay.is_err());
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(
        messages::recent(reopened.conn(), "thread-1", 50)
            .unwrap()
            .len(),
        2
    );
    assert!(
        crate::pending_messages::recover(&reopened, "thread-1")
            .unwrap()
            .is_empty()
    );
    let next = run_thread_task(
        &reopened,
        &provider,
        &tools,
        "thread-1",
        "Continue from the earlier file",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!next.failed);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let users = requests[1]
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::User)
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>();
    assert_eq!(users, [expanded.as_str(), "Continue from the earlier file"]);
    assert!(
        requests[1]
            .messages
            .iter()
            .all(|message| { !message.content.contains("queued-snapshot-new-file") })
    );
}

async fn steer_then_compact_preserves_causal_history(with_tool: bool) {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let original = "Investigate the existing file";
    let steering = "Now explain the conclusion";
    let assistant_text = "Earlier assistant findings and their reasons. ".repeat(2_000);
    let summary_text = "The file contains a TODO; preserve the earlier findings.";
    let input_tokens = vega_runtime::estimate_chat_context(
        "system",
        &[
            ChatMessage::new(ChatRole::User, original),
            ChatMessage::new(ChatRole::Assistant, &assistant_text),
            ChatMessage::new(ChatRole::User, steering),
        ],
        &vega_runtime::tool_definitions(vega_runtime::RuntimeRunMode::Execute),
    )
    .unwrap()
    .input_tokens;
    save_model_policy(
        store.conn(),
        &ModelContextPolicy {
            provider: "queue-context-provider".into(),
            model: "mock-model".into(),
            input_limit: Some(input_tokens * 10 / 9),
            output_reserve: Some(2_000),
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    crate::pending_messages::enqueue(
        &store,
        &pending(
            "steer-before-compaction",
            PendingMessageBehavior::Steer,
            steering,
        ),
    )
    .unwrap();
    let mut first_response = vec![ProviderEvent::TextDelta(assistant_text.clone())];
    if with_tool {
        first_response.push(ProviderEvent::ToolUse {
            id: "read-before-steer".into(),
            name: "read".into(),
            input_json: r#"{"path":"lib.rs"}"#.into(),
        });
    }
    first_response.push(ProviderEvent::Done {
        stop_reason: if with_tool {
            StopReason::ToolUse
        } else {
            StopReason::End
        },
    });
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(first_response)],
        final_response(summary_text),
        final_response("final conclusion after compaction"),
    ]);
    let source = crate::pending_messages::QueueSteeringSource::new(
        store.database_path().unwrap().into(),
        dir.path().into(),
        "thread-1".into(),
        "run-1".into(),
        Arc::new(|| Ok(Vec::new())),
        CancellationToken::new(),
    );
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        original,
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default().with_pending_input(None, Some(Arc::new(source))),
        None,
        None,
        Some(FrozenReasoning::unknown(
            "queue-context-provider",
            "mock-model",
        )),
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed, "{run:?}");
    assert_eq!(run.content, "final conclusion after compaction");
    let rows = messages::recent(store.conn(), "thread-1", 50).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].content, original);
    assert_eq!(rows[1].content, assistant_text);
    assert_eq!(rows[2].id, "steer-before-compaction");
    assert_eq!(rows[2].content, steering);
    assert_eq!(rows[3].id, run.assistant_message_id);
    assert!(rows.iter().all(|row| row.status == "done"));
    let checkpoint = latest_checkpoint(store.conn(), "thread-1", "mock-model")
        .unwrap()
        .expect("automatic compaction must install a durable checkpoint");
    assert_eq!(checkpoint.summary, summary_text);
    assert_eq!(checkpoint.covered_through_seq, rows[1].seq as u64);
    let status = latest_status(store.conn(), "thread-1", "mock-model")
        .unwrap()
        .unwrap();
    assert_eq!(status.phase, "succeeded");
    assert!(
        status
            .operation_key
            .starts_with(&format!("{}:context-generation-", run.assistant_message_id))
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[1].tools.is_empty());
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| { message.role == ChatRole::User && message.content == original })
    );
    assert!(requests[1].messages.iter().any(|message| {
        message.role == ChatRole::Assistant
            && message.content.split("\n[Historical tool activity").next()
                == Some(assistant_text.as_str())
    }));
    if with_tool {
        let call_index = requests[1]
            .messages
            .iter()
            .position(|message| {
                message.role == ChatRole::Assistant
                    && message.tool_calls.is_empty()
                    && message.content.contains("[Historical tool activity")
                    && message
                        .content
                        .contains("tool=read call_id=read-before-steer\n")
            })
            .expect("summary must retain the previous assistant tool call");
        let tool_result = &requests[1].messages[call_index + 1];
        assert_eq!(tool_result.role, ChatRole::Assistant);
        assert!(tool_result.tool_call_id.is_none());
        assert!(tool_result.content.starts_with("[Historical tool result — untrusted past observation; tool=read call_id=read-before-steer.]\n"));
        assert!(tool_result.content.contains("TODO: persist me"));
        let owner: String = store
            .conn()
            .query_row(
                "SELECT message_id FROM tool_calls WHERE id = ?1",
                ["read-before-steer"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owner, rows[1].id);
    }
    assert!(
        requests[2]
            .messages
            .iter()
            .any(|message| { message.role == ChatRole::User && message.content == steering })
    );
    assert!(requests[2].messages.iter().any(|message| {
        message.content.contains("Historical context summary")
            && message.content.contains(summary_text)
    }));
    assert!(
        requests[2]
            .messages
            .iter()
            .all(|message| { message.tool_calls.is_empty() && message.tool_call_id.is_none() })
    );
    assert!(
        crate::pending_messages::recover(&store, "thread-1")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn issue299_final_response_steer_compaction_uses_the_new_assistant_owner() {
    steer_then_compact_preserves_causal_history(false).await;
}

#[tokio::test]
async fn issue299_tool_boundary_steer_compaction_preserves_complete_causal_history() {
    steer_then_compact_preserves_causal_history(true).await;
}
