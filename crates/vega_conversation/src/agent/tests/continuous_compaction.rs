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
        assert_eq!(result.messages, replay);
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
    assert!(
        provider.requests()[1]
            .messages
            .iter()
            .any(|m| m.content.contains("Task goal"))
    );
}
