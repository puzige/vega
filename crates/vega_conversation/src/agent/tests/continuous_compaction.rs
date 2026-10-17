use super::super::pipeline::history_from_context_source_with_checkpoint;
use super::*;

fn append_round(store: &Store, round: i64) {
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    }
    .to_json()
    .unwrap();
    for parallel in 0..2 {
        let seq = round * 2 + parallel;
        let id = format!("call-{seq}");
        store.conn().execute(
            "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, output_text, status, approval, created_at, finished_at, text_offset_bytes) VALUES (?1, 'thread-1', 'active', ?2, 'read', '{\"path\":\"test.txt\"}', ?3, 'success', ?4, 2, 2, 0)",
            (&id, seq, format!("progress round {round} item {parallel}"), &approval),
        ).unwrap();
        store.conn().execute("INSERT INTO tool_call_rounds (message_id, call_id, response_id) VALUES ('active', ?1, ?2)", (&id, format!("round-{round}"))).unwrap();
    }
}

#[tokio::test]
async fn continuous_compaction_twice_same_assistant_reopens_without_duplicate_or_missing_rounds() {
    repeated_partial_compaction(false).await;
}

#[tokio::test]
async fn model_switch_partial_cursor_reopens_and_advances_without_replaying_covered_rounds() {
    repeated_partial_compaction(true).await;
}

async fn repeated_partial_compaction(switch_model: bool) {
    repeated_partial_compaction_case(switch_model, false).await;
}

async fn repeated_partial_compaction_case(switch_model: bool, with_reasoning: bool) {
    let (store, dir, _) = setup();
    store.conn().execute("INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES ('goal', 'thread-1', 1, 'user', 'text', 'Implement task goal; preserve progress', 'done', 1), ('active', 'thread-1', 2, 'assistant', 'text', '', 'done', 2)", []).unwrap();
    append_round(&store, 1);
    append_round(&store, 2);
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let history = history_from_context_source_with_checkpoint(&source, None, "").unwrap();
    let batches = history
        .iter()
        .filter(|m| !m.tool_calls.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(batches.len(), 2);
    assert_eq!(
        batches[0]
            .tool_calls
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["call-2", "call-3"]
    );
    assert_eq!(
        batches[1]
            .tool_calls
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["call-4", "call-5"]
    );
    store
        .conn()
        .execute(
            "UPDATE messages SET status = 'streaming' WHERE id = 'active'",
            [],
        )
        .unwrap();
    let provider = MockProvider::new_rounds((0..2).map(|_| vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Task goal: implement task. Progress: completed prior read rounds. Next: continue remaining rounds.".into()),
        ProviderEvent::Done { stop_reason: StopReason::End },
    ])]).collect());
    let budget = vega_runtime::ContextBudget::new(428_000, 128_000, true).unwrap();
    let mut history = history;
    for iteration in 0..2 {
        let model = if switch_model && iteration == 1 {
            "deepseek-model"
        } else {
            "mock-model"
        };
        if with_reasoning {
            attach_all_runtime_reasoning(&mut history, iteration);
        }
        let retained_call_id = if iteration == 0 { "call-4" } else { "call-6" };
        let retained_index = history
            .iter()
            .position(|message| {
                message
                    .tool_calls
                    .iter()
                    .any(|call| call.id == retained_call_id)
            })
            .unwrap();
        let expected_suffix = history[retained_index..].to_vec();
        store
            .conn()
            .execute(
                "UPDATE threads SET model = ?1 WHERE id = 'thread-1'",
                [model],
            )
            .unwrap();
        let hook = ConversationCompactionHook::new(
            &provider,
            dir.path().join("vega.db"),
            "thread-1",
            model,
            None,
            None,
        );
        let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
        let estimate = vega_runtime::estimate_chat_context("system", &history, &[]).unwrap();
        let result = vega_runtime::ContextCompactionHook::compact(
            &hook,
            vega_runtime::ContextCompactionRequest {
                system_prompt: "system".into(),
                messages: history,
                tools: vec![],
                budget,
                estimate,
                target_tokens: budget.target_tokens().unwrap(),
                source_version: source.source_version,
                source_fingerprint: Some(source.fingerprint.clone()),
                require_source_fence: false,
                source_owner_id: Some("active".into()),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let checkpoint = vega_store::context_compaction::latest_checkpoint(
            store.conn(),
            "thread-1",
            "mock-model",
        )
        .unwrap()
        .unwrap();
        assert_eq!(checkpoint.model, model);
        assert_eq!(checkpoint.covered_assistant_id.as_deref(), Some("active"));
        assert_eq!(
            checkpoint.covered_assistant_tool_seq,
            if iteration == 0 { 3 } else { 5 }
        );
        let reopened = Store::open(dir.path().join("vega.db")).unwrap();
        let reopened_source =
            vega_store::context_compaction::load_source(reopened.conn(), "thread-1").unwrap();
        let replay =
            history_from_context_source_with_checkpoint(&reopened_source, Some(&checkpoint), "")
                .unwrap();
        if with_reasoning {
            assert_eq!(&result.messages[1..], expected_suffix.as_slice());
            let mut durable_result = result.messages.clone();
            for message in &mut durable_result {
                message.reasoning_content = None;
                message.response_reasoning.clear();
            }
            assert_eq!(durable_result, replay);
        } else {
            assert_eq!(result.messages, replay);
        }
        assert_eq!(reopened_source, source);
        assert!(result.messages[0].content.contains("Task goal"));
        assert!(result.messages[0].content.contains("Progress"));
        if iteration == 0 {
            append_round(&store, 3);
            let next_source =
                vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
            history =
                history_from_context_source_with_checkpoint(&next_source, Some(&checkpoint), "")
                    .unwrap();
        } else {
            history = result.messages;
        }
    }
    assert_eq!(provider.requests().len(), 2);
    if with_reasoning {
        assert!(provider.requests().iter().all(|request| {
            request.messages.iter().all(|message| {
                message.reasoning_content.is_none() && message.response_reasoning.is_empty()
            })
        }));
    }
    assert!(
        provider.requests()[1]
            .messages
            .iter()
            .any(|m| m.content.contains("Task goal"))
    );
}

#[tokio::test]
async fn automatic_compaction_keeps_reasoning_out_of_durable_source_comparison() {
    for reasoning in [
        RuntimeReasoning::Empty,
        RuntimeReasoning::Text,
        RuntimeReasoning::OpaqueResponse,
    ] {
        let (store, dir, _) = setup();
        let mut messages = seed_two_completed_rounds(&store);
        let before_reasoning = messages.clone();
        attach_runtime_reasoning(&mut messages, reasoning);
        let assistant_index = messages
            .iter()
            .position(|message| {
                message.role == vega_runtime::ChatRole::Assistant && !message.tool_calls.is_empty()
            })
            .unwrap();
        assert_ne!(messages[assistant_index], before_reasoning[assistant_index]);
        let retained_index = messages
            .iter()
            .position(|message| message.tool_calls.iter().any(|call| call.id == "call-4"))
            .unwrap();
        let expected_suffix = messages[retained_index..].to_vec();
        let debug = format!("{messages:?}");
        let raw_source =
            vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
        let provider = summary_provider(
            "Task goal: implement task. Progress: completed prior read rounds. Next: continue remaining rounds.",
        );

        let result = compact_live_history(
            &store,
            dir.path().join("vega.db"),
            &provider,
            messages,
            "mock-model",
        )
        .await
        .unwrap();

        assert_eq!(provider.requests().len(), 1);
        assert!(provider.requests()[0].messages.iter().all(|message| {
            message.reasoning_content.is_none() && message.response_reasoning.is_empty()
        }));
        assert_eq!(&result.messages[1..], expected_suffix.as_slice());
        assert!(result.messages[0].content.contains("Task goal"));
        assert_eq!(
            result.messages[1]
                .tool_calls
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            ["call-4", "call-5"]
        );
        assert_eq!(
            vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap(),
            raw_source
        );
        assert!(!debug.contains("private runtime reasoning"));
        assert!(!debug.contains("opaque provider item"));
        let checkpoint = vega_store::context_compaction::latest_checkpoint(
            store.conn(),
            "thread-1",
            "mock-model",
        )
        .unwrap()
        .unwrap();
        assert_eq!(checkpoint.covered_assistant_id.as_deref(), Some("active"));
        assert_eq!(checkpoint.covered_assistant_tool_seq, 3);
    }
}

#[tokio::test]
async fn automatic_compaction_reopens_after_repeated_reasoning_bearing_rounds() {
    repeated_partial_compaction_case(false, true).await;
}

#[tokio::test]
async fn automatic_compaction_rejects_owned_durable_changes_and_preserves_checkpoint() {
    for mutation in [
        OwnedMutation::Role,
        OwnedMutation::Content,
        OwnedMutation::Image,
        OwnedMutation::ToolCallId,
        OwnedMutation::ToolId,
        OwnedMutation::ToolName,
        OwnedMutation::ToolInput,
        OwnedMutation::ToolCallOrder,
        OwnedMutation::MissingToolCall,
        OwnedMutation::AddedToolCall,
        OwnedMutation::ToolMessageOrder,
        OwnedMutation::MissingToolMessage,
        OwnedMutation::AddedToolMessage,
    ] {
        let (store, dir, _) = setup();
        let history = seed_two_completed_rounds(&store);
        let first_provider = summary_provider("first durable summary");
        let initial = compact_live_history(
            &store,
            dir.path().join("vega.db"),
            &first_provider,
            history,
            "mock-model",
        )
        .await
        .unwrap();
        assert_eq!(first_provider.requests().len(), 1);
        let checkpoint_before = vega_store::context_compaction::latest_checkpoint(
            store.conn(),
            "thread-1",
            "mock-model",
        )
        .unwrap()
        .unwrap();
        let source_before =
            vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
        let mut request_messages = history_from_context_source_with_checkpoint(
            &source_before,
            Some(&checkpoint_before),
            "",
        )
        .unwrap();
        mutate_owned_messages(mutation, &mut request_messages);
        let rejecting_provider = MockProvider::new(Vec::new());

        let rejected = compact_live_history(
            &store,
            dir.path().join("vega.db"),
            &rejecting_provider,
            request_messages,
            "mock-model",
        )
        .await;

        assert!(
            matches!(rejected, Err(failure) if matches!(failure.error.as_ref(), VegaError::Context(vega_runtime::ContextRuntimeError::SourceChanged))),
            "{mutation:?}"
        );
        assert!(rejecting_provider.requests().is_empty(), "{mutation:?}");
        assert!(
            initial.messages[0]
                .content
                .contains("first durable summary")
        );
        let source_after =
            vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
        assert_eq!(source_after, source_before, "{mutation:?}");
        let checkpoint_after = vega_store::context_compaction::latest_checkpoint(
            store.conn(),
            "thread-1",
            "mock-model",
        )
        .unwrap()
        .unwrap();
        assert_eq!(checkpoint_after.id, checkpoint_before.id, "{mutation:?}");
        assert_eq!(
            checkpoint_after.summary, checkpoint_before.summary,
            "{mutation:?}"
        );
        assert_eq!(
            checkpoint_after.covered_assistant_tool_seq,
            checkpoint_before.covered_assistant_tool_seq,
            "{mutation:?}"
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum RuntimeReasoning {
    Empty,
    Text,
    OpaqueResponse,
}

#[derive(Clone, Copy, Debug)]
enum OwnedMutation {
    Role,
    Content,
    Image,
    ToolCallId,
    ToolId,
    ToolName,
    ToolInput,
    ToolCallOrder,
    MissingToolCall,
    AddedToolCall,
    ToolMessageOrder,
    MissingToolMessage,
    AddedToolMessage,
}

fn seed_two_completed_rounds(store: &Store) -> Vec<vega_runtime::ChatMessage> {
    store.conn().execute(
        "INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES ('goal', 'thread-1', 1, 'user', 'text', 'Implement task goal; preserve progress', 'done', 1), ('active', 'thread-1', 2, 'assistant', 'text', '', 'streaming', 2)",
        [],
    ).unwrap();
    append_round(store, 1);
    append_round(store, 2);
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    history_from_context_source_with_checkpoint(&source, None, "").unwrap()
}

fn attach_runtime_reasoning(
    messages: &mut [vega_runtime::ChatMessage],
    reasoning: RuntimeReasoning,
) {
    for message in messages.iter_mut().filter(|message| {
        message.role == vega_runtime::ChatRole::Assistant && !message.tool_calls.is_empty()
    }) {
        match reasoning {
            RuntimeReasoning::Empty => message.reasoning_content = Some(String::new()),
            RuntimeReasoning::Text => {
                message.reasoning_content = Some("private runtime reasoning".into())
            }
            RuntimeReasoning::OpaqueResponse => {
                message.response_reasoning = vec![serde_json::json!({
                    "opaque": [17, true, {"provider_item": "opaque provider item"}]
                })]
            }
        }
    }
}

fn attach_all_runtime_reasoning(messages: &mut [vega_runtime::ChatMessage], iteration: usize) {
    for (index, message) in messages.iter_mut().enumerate().filter(|(_, message)| {
        message.role == vega_runtime::ChatRole::Assistant && !message.tool_calls.is_empty()
    }) {
        message.reasoning_content = Some(format!("runtime reasoning {iteration}:{index}"));
        message.response_reasoning = vec![serde_json::json!({
            "opaque": {"iteration": iteration, "message": index, "provider_item": true}
        })];
    }
}

fn mutate_owned_messages(mutation: OwnedMutation, messages: &mut Vec<vega_runtime::ChatMessage>) {
    let assistant_index = messages
        .iter()
        .position(|message| {
            message.role == vega_runtime::ChatRole::Assistant && !message.tool_calls.is_empty()
        })
        .unwrap();
    let first_result_index = assistant_index + 1;
    let second_result_index = assistant_index + 2;
    match mutation {
        OwnedMutation::Role => {
            messages[assistant_index].role = vega_runtime::ChatRole::Tool;
            messages[assistant_index].tool_call_id = Some("call-4".into());
        }
        OwnedMutation::Content => messages[first_result_index].content.push_str(" changed"),
        OwnedMutation::Image => {
            let image = image::RgbImage::from_pixel(1, 1, image::Rgb([20, 40, 60]));
            let mut png = std::io::Cursor::new(Vec::new());
            image.write_to(&mut png, image::ImageFormat::Png).unwrap();
            messages[first_result_index]
                .images
                .push(crate::types::ImageAttachment::from_bytes(png.into_inner()).unwrap());
        }
        OwnedMutation::ToolCallId => {
            messages[first_result_index].tool_call_id = Some("call-5".into())
        }
        OwnedMutation::ToolId => {
            messages[assistant_index].tool_calls[0].id = "different-call".into()
        }
        OwnedMutation::ToolName => {
            messages[assistant_index].tool_calls[0].name = "different-tool".into()
        }
        OwnedMutation::ToolInput => {
            messages[assistant_index].tool_calls[0].input_json = "{\"path\":\"changed\"}".into()
        }
        OwnedMutation::ToolCallOrder => messages[assistant_index].tool_calls.reverse(),
        OwnedMutation::MissingToolCall => {
            messages[assistant_index].tool_calls.pop();
        }
        OwnedMutation::AddedToolCall => {
            let call = messages[assistant_index].tool_calls[0].clone();
            messages[assistant_index].tool_calls.push(call);
        }
        OwnedMutation::ToolMessageOrder => messages.swap(first_result_index, second_result_index),
        OwnedMutation::MissingToolMessage => {
            messages.remove(second_result_index);
        }
        OwnedMutation::AddedToolMessage => {
            messages.push(messages[first_result_index].clone());
        }
    }
}

fn summary_provider(summary: &str) -> MockProvider {
    MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta(summary.into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])])
}

async fn compact_live_history(
    store: &Store,
    database_path: std::path::PathBuf,
    provider: &MockProvider,
    messages: Vec<vega_runtime::ChatMessage>,
    model: &str,
) -> Result<vega_runtime::ContextCompactionResult, vega_runtime::ContextCompactionFailure> {
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let estimate = vega_runtime::estimate_chat_context("system", &messages, &[]).unwrap();
    let budget = vega_runtime::ContextBudget::new(428_000, 128_000, true).unwrap();
    let hook =
        ConversationCompactionHook::new(provider, database_path, "thread-1", model, None, None);
    vega_runtime::ContextCompactionHook::compact(
        &hook,
        vega_runtime::ContextCompactionRequest {
            system_prompt: "system".into(),
            messages,
            tools: vec![],
            budget,
            estimate,
            target_tokens: budget.target_tokens().unwrap(),
            source_version: source.source_version,
            source_fingerprint: Some(source.fingerprint),
            require_source_fence: false,
            source_owner_id: Some("active".into()),
        },
        CancellationToken::new(),
    )
    .await
}
