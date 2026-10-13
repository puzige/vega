use super::*;

fn retrying_provider() -> MockProvider {
    MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("保留。".into()),
            ProviderEvent::ToolUse {
                id: "completed-read".into(),
                name: "read".into(),
                input_json: r#"{"path":"lib.rs"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("旧片段🦀".into()),
            ProviderEvent::ThinkingDelta("discarded reasoning".into()),
            ProviderEvent::Usage {
                input: 10,
                output: 2,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::RetryScheduled {
                attempt: 1,
                max_retries: 5,
            },
            ProviderEvent::TextDelta("替换完成。".into()),
            ProviderEvent::Usage {
                input: 20,
                output: 3,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ])
}

#[tokio::test]
async fn issue350_retry_rollback_is_durable_before_ui_and_preserves_usage_and_tools() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = retrying_provider();
    let mut retry_seen = false;
    let run = run_thread_task_with_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Inspect",
        "System",
        CancellationToken::new(),
        |event| {
            if let ConversationEvent::ResponseRetrying {
                message_id,
                text_start_bytes,
                attempt,
                max_retries,
            } = event
            {
                let content: String = store
                    .conn()
                    .query_row(
                        "SELECT content FROM messages WHERE id = ?1",
                        [message_id],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(content, "保留。");
                assert_eq!(*text_start_bytes, content.len());
                assert_eq!((*attempt, *max_retries), (1, 5));
                retry_seen = true;
            }
            if let ConversationEvent::TextDelta { delta, .. } = event
                && delta == "替换完成。"
            {
                assert!(retry_seen);
            }
            Ok(())
        },
    )
    .await
    .unwrap();
    assert!(retry_seen);
    assert!(!run.failed);
    assert_eq!(run.content, "保留。替换完成。");
    let history = crate::history::latest_history_page(&store, "thread-1", 20).unwrap();
    let content = history
        .entries
        .iter()
        .filter_map(|entry| match entry {
            crate::history::HistoryEntry::AssistantText { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(content, run.content);
    let tool_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE status = 'success'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let usage: (i64, i64) = store
        .conn()
        .query_row(
            "SELECT COUNT(*), SUM(input_tokens) FROM token_usage",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(tool_count, 1);
    assert_eq!(usage, (2, 30));
    assert!(runtime_event_requires_ack(
        &RuntimeEvent::ResponseRetrying {
            text_start_bytes: 0,
            attempt: 1,
            max_retries: 5
        }
    ));
}

#[tokio::test]
async fn issue350_retry_persistence_failure_stops_before_replacement_delta() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = retrying_provider();
    let config = PersistenceActorConfig {
        fail_event: Some(InjectedPersistenceFailure::Retrying),
        ..Default::default()
    };
    let mut delivered_retry = false;
    let mut delivered_replacement = false;
    let result = run_thread_task_with_sink_config(&store, &provider, &tools, "thread-1", "Inspect", "System", CancellationToken::new(), |event| {
        delivered_retry |= matches!(event, ConversationEvent::ResponseRetrying { .. });
        delivered_replacement |= matches!(event, ConversationEvent::TextDelta { delta, .. } if delta == "替换完成。");
        Ok(())
    }, config).await;
    assert!(result.is_err());
    assert!(!delivered_retry);
    assert!(!delivered_replacement);
    assert_eq!(provider.requests().len(), 2);
}
