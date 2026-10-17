use super::*;
use serde_json::json;

#[tokio::test]
async fn i71_response_replay_tool_loop_and_private_history() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let reasoning = json!({
        "type": "reasoning",
        "id": "rs-1",
        "summary": [{"type": "summary_text", "text": "owned summary"}],
        "encrypted_content": "opaque-owned-replay"
    });
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("raw owned thinking".into()),
            ProviderEvent::SummaryDelta("owned summary".into()),
            ProviderEvent::ReasoningReplay(vec![reasoning.clone()]),
            ProviderEvent::ToolUse {
                id: "read-owned".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Usage {
                input: 12,
                output: 3,
                cache_read: Some(2),
                cache_write: Some(0),
            },
            ProviderEvent::ResponseReplay(vega_runtime::ResponseReplayLayout {
                response_id: Some("resp-owned".into()),
                slots: vec![
                    vega_runtime::ResponseReplaySlot::Reasoning(reasoning.clone()),
                    vega_runtime::ResponseReplaySlot::FunctionCall {
                        item: json!({"type":"function_call","call_id":"read-owned","name":"read","arguments":"{\"path\":\"lib.rs\"}"}),
                        call_id: "read-owned".into(),
                        name: "read".into(),
                        arguments: r#"{"path":"lib.rs"}"#.into(),
                    },
                ],
            }),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Found the TODO.".into()),
            ProviderEvent::ResponseReplay(vega_runtime::ResponseReplayLayout {
                response_id: Some("resp-final".into()),
                slots: vec![vega_runtime::ResponseReplaySlot::Message(
                    json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"Found the TODO."}]}),
                )],
            }),
            ProviderEvent::Usage {
                input: 12,
                output: 3,
                cache_read: Some(2),
                cache_write: Some(0),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let mut live_thinking = Vec::new();
    let mut live_summaries = Vec::new();
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Read lib.rs",
        "Owned system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |event| {
            match event {
                ConversationEvent::ThinkingDelta { delta, .. } => live_thinking.push(delta.clone()),
                ConversationEvent::SummaryDelta { delta, .. } => live_summaries.push(delta.clone()),
                _ => {}
            }
            Ok(())
        },
        PersistenceActorConfig::default(),
        None,
        None,
        Some(vega_runtime::FrozenReasoning {
            provider: "fixture".into(),
            model: "mock-model".into(),
            protocol: vega_runtime::ReasoningProtocol::ResponsesApi,
            replay_policy: vega_runtime::ReasoningReplayPolicy::ResponsesItems,
            choice: vega_runtime::ReasoningChoice::ProviderDefault,
            supports_disabled: false,
            disabled_wire: None,
            declared_efforts: Vec::new(),
        }),
        Vec::new(),
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert_eq!(run.content, "Found the TODO.");
    assert_eq!(live_thinking, ["raw owned thinking"]);
    assert_eq!(live_summaries, ["owned summary"]);
    assert!(!run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ThinkingDelta { .. } | ConversationEvent::SummaryDelta { .. }
    )));
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.status == ToolCallStatus::Success && result.output.contains("TODO")
    )));
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.response_reasoning == [reasoning.clone()])
    );
    assert!(requests[1].messages.iter().any(|message| {
        message.tool_call_id.as_deref() == Some("read-owned") && message.content.contains("TODO")
    }));
    let persisted: String = store
        .conn()
        .query_row(
            "SELECT content FROM messages WHERE id=?1",
            [&run.assistant_message_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted, "Found the TODO.");
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let history = crate::history::latest_history_page(&reopened, "thread-1", 50).unwrap();
    assert!(!format!("{history:?}").contains("owned summary"));
    assert!(!format!("{history:?}").contains("opaque-owned-replay"));
    let usage: (i64, i64, i64) = store
        .conn()
        .query_row(
            "SELECT SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens) FROM token_usage",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(usage, (24, 6, 4));
}
