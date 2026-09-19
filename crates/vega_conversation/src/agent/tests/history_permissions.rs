use super::*;
use crate::types::{ContextCompactionFailureCode, ContextCompactionStatus, Microcents};
use vega_store::permissions;

type UsageRow = (Option<String>, i64, Option<String>, Option<String>);

struct GatedSummaryProvider {
    started: std::sync::Arc<tokio::sync::Notify>,
    release: std::sync::Arc<tokio::sync::Notify>,
}

impl vega_runtime::Provider for GatedSummaryProvider {
    fn chat_stream(
        &self,
        _request: vega_runtime::ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        self.started.notify_one();
        let release = self.release.clone();
        Box::pin(async move {
            release.notified().await;
            Ok(Box::pin(futures::stream::iter(vec![
                Ok(ProviderEvent::TextDelta("gated summary".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ])) as vega_runtime::EventStream)
        })
    }
}

#[tokio::test]
async fn assembles_system_and_history_by_sequence_with_current_user_last() {
    let (store, dir, _project_id) = setup();
    for (id, seq, role, content, status) in [
        ("history-3", 3, "assistant", "failed answer", "failed"),
        ("history-1", 1, "user", "old question", "done"),
        ("history-2", 2, "assistant", "partial answer", "interrupted"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: status.into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("answer".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "current question",
        "system first",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    let history: Vec<(vega_runtime::ChatRole, &str)> = requests[0]
        .messages
        .iter()
        .map(|message| (message.role, message.content.as_str()))
        .collect();
    assert_eq!(
        history,
        vec![
            (vega_runtime::ChatRole::System, "system first"),
            (vega_runtime::ChatRole::User, "old question"),
            (vega_runtime::ChatRole::Assistant, "partial answer"),
            (vega_runtime::ChatRole::Assistant, "failed answer"),
            (vega_runtime::ChatRole::User, "current question"),
        ]
    );
}

#[tokio::test]
async fn issue76_history_projection_keeps_early_constraint_beyond_old_window() {
    let (store, dir, _project_id) = setup();
    for seq in 1..=55_i64 {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: format!("history-{seq}"),
                thread_id: "thread-1".into(),
                seq,
                role: if seq % 2 == 1 { "user" } else { "assistant" }.into(),
                kind: "text".into(),
                content: if seq == 1 {
                    "ORCHID-76 early constraint".into()
                } else {
                    format!("turn-{seq}")
                },
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "current question",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(
        provider.requests()[0]
            .messages
            .iter()
            .any(|message| message.content.contains("ORCHID-76 early constraint"))
    );
}

#[tokio::test]
async fn issue76_history_projection_splits_text_and_pairs_each_tool_group() {
    let (store, dir, _project_id) = setup();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "assistant-history".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: "textA textB textC".into(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    }
    .to_json()
    .unwrap();
    for (id, seq, offset, output) in [
        ("call-1", 1_i64, 5_i64, "result1"),
        ("call-2", 2_i64, 11_i64, "result2"),
    ] {
        store
            .conn()
            .execute(
                r#"INSERT INTO tool_calls
                 (id, thread_id, message_id, seq, tool, input_json, output_text, status,
                  approval, created_at, finished_at, text_offset_bytes)
                 VALUES (?1, 'thread-1', 'assistant-history', ?2, 'read',
                         '{"path":"lib.rs"}', ?3, 'success', ?4, ?2, ?2, ?5)"#,
                (&id, seq, &output, &approval, offset),
            )
            .unwrap();
    }
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let rich = crate::agent::pipeline::history_from_context_source_with_checkpoint(
        &source, None, "current",
    )
    .unwrap();
    assert_eq!(rich.len(), 5);
    assert_eq!(
        (rich[0].role, rich[0].content.as_str()),
        (vega_runtime::ChatRole::Assistant, "textA")
    );
    assert_eq!(rich[0].tool_calls[0].id, "call-1");
    assert_eq!(
        (
            rich[1].role,
            rich[1].content.as_str(),
            rich[1].tool_call_id.as_deref()
        ),
        (vega_runtime::ChatRole::Tool, "result1", Some("call-1"))
    );
    assert_eq!(
        (rich[2].role, rich[2].content.as_str()),
        (vega_runtime::ChatRole::Assistant, " textB")
    );
    assert_eq!(rich[2].tool_calls[0].id, "call-2");
    assert_eq!(
        (
            rich[3].role,
            rich[3].content.as_str(),
            rich[3].tool_call_id.as_deref()
        ),
        (vega_runtime::ChatRole::Tool, "result2", Some("call-2"))
    );
    assert_eq!(
        (rich[4].role, rich[4].content.as_str()),
        (vega_runtime::ChatRole::Assistant, " textC")
    );

    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "current",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let messages = &provider.requests()[0].messages;
    let projection = messages.iter().skip(1).collect::<Vec<_>>();
    assert!(
        projection
            .iter()
            .all(|message| { message.tool_call_id.is_none() && message.tool_calls.is_empty() })
    );
    let rendered = projection
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("tool read input: {\"path\":\"lib.rs\"} status: success"));
    assert!(rendered.contains("result1"));
    assert!(rendered.contains("result2"));
    assert!(rendered.contains("textA"));
    assert!(rendered.contains("textB"));
    assert!(rendered.contains("textC"));
    assert_eq!(
        projection.last().map(|message| message.content.as_str()),
        Some("current")
    );
}

#[tokio::test]
async fn issue76_manual_tool_history_uses_rich_summary_and_labelled_primary_after_reload() {
    let (store, dir, _project_id) = setup();
    for (id, seq, role, content) in [
        ("old-tool-assistant", 1_i64, "assistant", "before tool"),
        ("old-current-user", 2_i64, "user", "continue after tool"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    }
    .to_json()
    .unwrap();
    store
        .conn()
        .execute(
            r#"INSERT INTO tool_calls
             (id, thread_id, message_id, seq, tool, input_json, output_text, status,
              approval, created_at, finished_at, text_offset_bytes)
             VALUES ('old-read', 'thread-1', 'old-tool-assistant', 1, 'read',
                     '{"path":"lib.rs"}', 'old read result', 'success', ?1, 1, 1, 6)"#,
            [&approval],
        )
        .unwrap();
    vega_store::context_compaction::save_settings(
        store.conn(),
        &vega_store::context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(20_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();

    let summary_provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("manual old tool summary".into()),
        ProviderEvent::Usage {
            input: 22,
            output: 5,
            cache_read: 0,
            cache_write: 0,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let events = compact_thread_manually_accounted(
        &store,
        &summary_provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        None,
        None,
        71,
    )
    .await
    .unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == ContextCompactionStatus::Succeeded
    )));
    let summary_request = &summary_provider.requests()[0];
    assert!(summary_request.tools.is_empty());
    assert!(
        summary_request.messages[1]
            .content
            .contains("tool read input")
    );
    assert!(
        summary_request.messages[1]
            .content
            .contains("old read result")
    );
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_some()
    );

    let projection = read_context_projection(&store, "thread-1", "mock-model", "system").unwrap();
    let estimated_tokens = projection.estimated_tokens;
    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let reopened_projection =
        read_context_projection(&reopened, "thread-1", "mock-model", "system").unwrap();
    assert_eq!(reopened_projection.estimated_tokens, estimated_tokens);

    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let next_provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("after manual reload".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    run_thread_task(
        &reopened,
        &next_provider,
        &tools,
        "thread-1",
        "next after manual reload",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let primary = &next_provider.requests()[0].messages;
    assert_eq!(
        primary
            .iter()
            .filter(|message| message.content.contains("Historical context summary"))
            .count(),
        1
    );
    assert!(
        primary
            .iter()
            .all(|message| { message.tool_call_id.is_none() && message.tool_calls.is_empty() })
    );
}

#[tokio::test]
async fn issue76_manual_primary_tail_keeps_terminal_tool_data_without_old_protocol() {
    let (store, dir, _project_id) = setup();
    for (id, seq, role, content) in [
        ("prefix-assistant", 1_i64, "assistant", "old text"),
        ("latest-user", 2_i64, "user", "latest question"),
        ("tail-assistant", 3_i64, "assistant", "tail text"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    }
    .to_json()
    .unwrap();
    for (id, message_id, seq, input, output, offset) in [
        (
            "prefix-read",
            "prefix-assistant",
            1_i64,
            r#"{"path":"lib.rs"}"#,
            "prefix result",
            3_i64,
        ),
        (
            "tail-read",
            "tail-assistant",
            2_i64,
            r#"{"path":"tail.rs"}"#,
            "tail result",
            4_i64,
        ),
    ] {
        store
            .conn()
            .execute(
                r#"INSERT INTO tool_calls
                 (id, thread_id, message_id, seq, tool, input_json, output_text, status,
                  approval, created_at, finished_at, text_offset_bytes)
                 VALUES (?1, 'thread-1', ?2, ?3, 'read', ?4, ?5, 'success', ?6, ?3, ?3, ?7)"#,
                (id, message_id, seq, input, output, &approval, offset),
            )
            .unwrap();
    }
    vega_store::context_compaction::save_settings(
        store.conn(),
        &vega_store::context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(20_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();

    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("summary before the retained tail".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let budget = vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap();
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vega_runtime::tool_definitions(vega_runtime::RuntimeRunMode::Execute),
        budget,
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(
        result
            .messages
            .iter()
            .all(|message| { message.tool_call_id.is_none() && message.tool_calls.is_empty() })
    );
    let result_text = result
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(result_text.contains("tail result"));
    assert!(result_text.contains("tool read input: {\"path\":\"tail.rs\"} status: success"));

    let projection = read_context_projection(&store, "thread-1", "mock-model", "system").unwrap();
    let estimated_tokens = projection.estimated_tokens;
    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let reopened_projection =
        read_context_projection(&reopened, "thread-1", "mock-model", "system").unwrap();
    assert_eq!(reopened_projection.estimated_tokens, estimated_tokens);

    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let next_provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("next answer".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    run_thread_task(
        &reopened,
        &next_provider,
        &tools,
        "thread-1",
        "next after retained tail",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let primary = &next_provider.requests()[0].messages;
    let primary_text = primary
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(primary_text.contains("tool read input: {\"path\":\"tail.rs\"} status: success"));
    assert!(primary_text.contains("tail result"));
    assert!(
        primary
            .iter()
            .all(|message| { message.tool_call_id.is_none() && message.tool_calls.is_empty() })
    );
}

#[tokio::test]
async fn issue76_configured_run_uses_summary_provider_then_primary_and_installs_checkpoint() {
    let (store, dir, _project_id) = setup();
    for seq in 1..=20_i64 {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: format!("compact-history-{seq}"),
                thread_id: "thread-1".into(),
                seq,
                role: if seq % 2 == 1 { "user" } else { "assistant" }.into(),
                kind: "text".into(),
                content: format!("historical-{seq}-{}", "x".repeat(700)),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    vega_store::context_compaction::save_settings(
        store.conn(),
        &vega_store::context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(20_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("summary of the durable history".into()),
            ProviderEvent::Usage {
                input: 100,
                output: 12,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("primary answer".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "current constraint",
        "primary system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(run.content, "primary answer");
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].tools.is_empty());
    assert!(
        requests[0].messages[0]
            .content
            .contains("context-compaction")
    );
    assert_eq!(requests[1].max_tokens, Some(1_000));
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.content.contains("Historical context summary"))
    );
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionUsageUpdated { usage, pricing: None, .. }
            if usage.input == 100 && usage.output == 12
    )));
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_some()
    );
    let reopened_provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("after restart".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    run_thread_task(
        &store,
        &reopened_provider,
        &tools,
        "thread-1",
        "next turn after restart",
        "primary system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let restored_messages = &reopened_provider.requests()[0].messages;
    assert_eq!(
        restored_messages
            .iter()
            .filter(|message| message.content.contains("Historical context summary"))
            .count(),
        1
    );
    assert!(
        !restored_messages
            .iter()
            .any(|message| message.content.contains("historical-1-"))
    );
}

#[tokio::test]
async fn issue76_repeated_manual_compaction_uses_checkpoint_and_only_uncovered_tail() {
    let (store, _dir, _project_id) = setup();
    seed_manual_compaction_history(&store);
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("first durable summary".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("second durable summary".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);

    compact_thread_manually_accounted(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        None,
        None,
        31,
    )
    .await
    .unwrap();
    assert_eq!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap()
            .summary,
        "first durable summary"
    );

    for (id, seq, role, content) in [
        (
            "manual-new-assistant",
            4_i64,
            "assistant",
            "new uncovered durable result",
        ),
        ("manual-new-user", 5_i64, "user", "new current question"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    compact_thread_manually_accounted(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        None,
        None,
        32,
    )
    .await
    .unwrap();

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1].messages[1]
            .content
            .contains("first durable summary")
    );
    assert!(
        requests[1].messages[1]
            .content
            .contains("new uncovered durable result")
    );
    assert!(
        !requests[1].messages[1]
            .content
            .contains("the original goal")
    );
    let projection = read_context_projection(&store, "thread-1", "mock-model", "system").unwrap();
    let estimated_after_second = projection.estimated_tokens;
    let source_version_after_second = projection.source_version;
    let source_fingerprint_after_second = projection.source_fingerprint.clone();
    let restored = projection
        .settings
        .as_ref()
        .expect("settings remain available after repeated compaction");
    assert_eq!(restored.model, "mock-model");
    assert_eq!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap()
            .summary,
        "second durable summary"
    );

    drop(store);
    let reopened = Store::open(_dir.path().join("vega.db")).unwrap();
    let reopened_projection =
        read_context_projection(&reopened, "thread-1", "mock-model", "system").unwrap();
    assert_eq!(reopened_projection.estimated_tokens, estimated_after_second);
    assert_eq!(
        reopened_projection.source_version,
        source_version_after_second
    );
    assert_eq!(
        reopened_projection.source_fingerprint,
        source_fingerprint_after_second
    );

    let tools = vega_tools::Tools::new(_dir.path()).unwrap();
    let next_provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("next after reload".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    run_thread_task(
        &reopened,
        &next_provider,
        &tools,
        "thread-1",
        "next after repeated compaction",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let next_request = &next_provider.requests()[0];
    assert_eq!(
        next_request
            .messages
            .iter()
            .filter(|message| message.content.contains("Historical context summary"))
            .count(),
        1
    );
    assert!(
        next_request
            .messages
            .iter()
            .any(|message| message.content == "new current question")
    );
}

fn issue76_priced_catalog() -> vega_token::PricingCatalog {
    vega_token::PricingCatalog::from_specs(vec![vega_token::ModelPricingSpec {
        model: "mock-model".into(),
        rates: vega_token::RateSpec {
            input_usd_per_million: "1".into(),
            output_usd_per_million: "2".into(),
            cache_read_usd_per_million: "0.1".into(),
            cache_write_usd_per_million: "0".into(),
        },
        max_standard_input_tokens: None,
        schedule: None,
    }])
    .unwrap()
}

#[tokio::test]
async fn issue76_priced_summary_usage_is_a_thread_level_audit_row() {
    let (store, dir, _project_id) = setup();
    for seq in 1..=12_i64 {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: format!("priced-history-{seq}"),
                thread_id: "thread-1".into(),
                seq,
                role: if seq % 2 == 1 { "user" } else { "assistant" }.into(),
                kind: "text".into(),
                content: format!("priced-history-{seq}-{}", "x".repeat(900)),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    vega_store::context_compaction::save_settings(
        store.conn(),
        &vega_store::context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(15_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("priced summary".into()),
            ProviderEvent::Usage {
                input: 100,
                output: 12,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("priced primary".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        "thread-1",
        "priced current",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        Some(issue76_priced_catalog()),
    )
    .await
    .unwrap();
    assert_eq!(run.content, "priced primary");
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionUsageUpdated {
            usage,
            cost: Some(Microcents(cost)),
            pricing: Some(_),
        } if usage.input == 100 && usage.output == 12 && *cost > 0
    )));
    let rows: Vec<UsageRow> = store
        .conn()
        .prepare(
            "SELECT message_id, cost_microcents, pricing_version, pricing_profile \
             FROM token_usage WHERE thread_id = 'thread-1' ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    assert!(rows.iter().any(|(message_id, cost, version, profile)| {
        message_id.is_none()
            && *cost > 0
            && version.as_deref() == Some(vega_store::token_usage::PRICED_VERSION)
            && profile.is_some()
    }));
}

#[tokio::test]
async fn issue76_summary_usage_received_before_failure_is_persisted_without_checkpoint() {
    let (store, dir, _project_id) = setup();
    for seq in 1..=20_i64 {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: format!("failed-summary-history-{seq}"),
                thread_id: "thread-1".into(),
                seq,
                role: if seq % 2 == 1 { "user" } else { "assistant" }.into(),
                kind: "text".into(),
                content: format!("failed-summary-history-{seq}-{}", "x".repeat(700)),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    vega_store::context_compaction::save_settings(
        store.conn(),
        &vega_store::context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(20_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![
        ScriptStep::events(vec![ProviderEvent::Usage {
            input: 77,
            output: 9,
            cache_read: 0,
            cache_write: 0,
        }]),
        ScriptStep::Error {
            status: Some(503),
            message: "summary provider failed".into(),
            retryable: false,
        },
    ]);
    let result = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "failed summary current",
        "system",
        CancellationToken::new(),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(
        provider.requests().len(),
        1,
        "primary must not follow failure"
    );
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
    let usage: (i64, i64, Option<String>) = store
        .conn()
        .query_row(
            "SELECT input_tokens, output_tokens, message_id FROM token_usage \
             WHERE thread_id = 'thread-1' ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(usage, (77, 9, None));
}

fn seed_manual_compaction_history(store: &Store) {
    for (id, seq, role, content) in [
        ("manual-old-user", 1_i64, "user", "the original goal"),
        (
            "manual-old-assistant",
            2_i64,
            "assistant",
            "the first result",
        ),
        ("manual-current-user", 3_i64, "user", "the current question"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    vega_store::context_compaction::save_settings(
        store.conn(),
        &vega_store::context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(20_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
}

#[tokio::test]
async fn issue76_manual_failure_returns_usage_and_typed_terminal_event() {
    let (store, _dir, _project_id) = setup();
    seed_manual_compaction_history(&store);
    let provider = MockProvider::new(vec![
        ScriptStep::events(vec![ProviderEvent::Usage {
            input: 41,
            output: 7,
            cache_read: 0,
            cache_write: 0,
        }]),
        ScriptStep::Error {
            status: Some(503),
            message: "summary failed after usage".into(),
            retryable: false,
        },
    ]);
    let events = compact_thread_manually_accounted(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        None,
        None,
        17,
    )
    .await
    .expect("summary failure is a typed terminal result, not a service failure");
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionUsageUpdated { usage, .. }
            if usage.input == 41 && usage.output == 7
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == ContextCompactionStatus::Failed
                && record.failure == Some(ContextCompactionFailureCode::Unavailable)
    )));
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
    let usage_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM token_usage WHERE thread_id = 'thread-1' AND message_id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(usage_count, 1);
}

#[tokio::test]
async fn issue76_manual_cancel_returns_usage_and_cancelled_terminal_event() {
    let (store, _dir, _project_id) = setup();
    seed_manual_compaction_history(&store);
    let provider = MockProvider::new(vec![
        ScriptStep::events(vec![ProviderEvent::Usage {
            input: 43,
            output: 8,
            cache_read: 0,
            cache_write: 0,
        }]),
        ScriptStep::Cancelled,
    ]);
    let events = compact_thread_manually_accounted(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        None,
        None,
        18,
    )
    .await
    .expect("summary cancellation is a typed terminal result, not a service failure");
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionUsageUpdated { usage, .. }
            if usage.input == 43 && usage.output == 8
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == ContextCompactionStatus::Cancelled
                && record.failure == Some(ContextCompactionFailureCode::Cancelled)
    )));
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn issue76_manual_cancel_before_install_keeps_raw_history_and_no_checkpoint() {
    let (store, _dir, _project_id) = setup();
    seed_manual_compaction_history(&store);
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("summary must not be requested".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let events = compact_thread_manually_accounted(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        cancel,
        None,
        None,
        19,
    )
    .await
    .unwrap();
    assert!(provider.requests().is_empty());
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == ContextCompactionStatus::Cancelled
                && record.failure == Some(ContextCompactionFailureCode::Cancelled)
    )));
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
    let raw_messages: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id = 'thread-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw_messages, 3);
}

#[tokio::test]
async fn issue76_cancel_while_checkpoint_install_waits_rolls_back_new_checkpoint() {
    let (store, dir, _project_id) = setup();
    seed_manual_compaction_history(&store);
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let previous = vega_store::context_compaction::NewContextCheckpoint {
        thread_id: "thread-1".into(),
        model: "mock-model".into(),
        source_version: source.source_version,
        covered_through_seq: 1,
        source_fingerprint: source.fingerprint.clone(),
        summary: "stable previous summary".into(),
        estimator_version: vega_runtime::CONTEXT_ESTIMATOR_VERSION.into(),
        expected_previous_id: None,
        created_at: 1,
    };
    let previous = match vega_store::context_compaction::install_checkpoint(store.conn(), &previous)
        .unwrap()
    {
        vega_store::context_compaction::ContextCheckpointInstall::Applied(checkpoint) => checkpoint,
        other => panic!("expected initial checkpoint, got {other:?}"),
    };

    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let provider = GatedSummaryProvider {
        started: started.clone(),
        release: release.clone(),
    };
    let cancel = CancellationToken::new();
    let compact = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
        cancel.clone(),
        None,
        None,
    );
    tokio::pin!(compact);
    tokio::select! {
        _ = started.notified() => {
            // Hold the writer lock from a dedicated OS thread.  The compaction
            // future must continue polling after the provider is released so
            // it reaches the real SQLite install wait; a `select!` branch
            // that awaits it only after releasing the lock would test the
            // wrong cancellation window.
            let lock_ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let lock_ready_thread = lock_ready.clone();
            let cancel_thread = cancel.clone();
            let lock_path = dir.path().join("vega.db");
            let lock_thread = std::thread::spawn(move || {
                let lock_store = Store::open(lock_path).unwrap();
                let writer_lock = lock_store.immediate_transaction().unwrap();
                lock_ready_thread.store(true, std::sync::atomic::Ordering::Release);
                std::thread::sleep(std::time::Duration::from_millis(100));
                cancel_thread.cancel();
                drop(writer_lock);
            });
            while !lock_ready.load(std::sync::atomic::Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
            release.notify_one();
            let result = compact.await;
            lock_thread.join().unwrap();
            assert!(matches!(
                result,
                Err(failure)
                    if matches!(failure.error.as_ref(), VegaError::Cancelled)
            ));
        }
        result = &mut compact => panic!("summary/install completed before test lock: {result:?}"),
    }

    let latest =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap();
    assert_eq!(latest.id, previous.id);
    assert_eq!(latest.summary, "stable previous summary");
    let raw_messages: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id = 'thread-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw_messages, 3);
}

#[tokio::test]
async fn issue76_manual_operation_key_is_unique_after_reopen_same_generation() {
    let (store, dir, _project_id) = setup();
    seed_manual_compaction_history(&store);
    let first_provider = MockProvider::new(vec![ScriptStep::Error {
        status: Some(503),
        message: "first summary attempt failed".into(),
        retryable: false,
    }]);
    compact_thread_manually_accounted(
        &store,
        &first_provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        None,
        None,
        23,
    )
    .await
    .unwrap();

    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let second_provider = MockProvider::new(vec![ScriptStep::Error {
        status: Some(503),
        message: "retry summary attempt failed".into(),
        retryable: false,
    }]);
    compact_thread_manually_accounted(
        &reopened,
        &second_provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        None,
        None,
        23,
    )
    .await
    .unwrap();

    let rows: Vec<(String, String, String)> = reopened
        .conn()
        .prepare(
            "SELECT operation_key, phase, usage_state FROM context_compaction_status \
             WHERE thread_id = 'thread-1' AND model = 'mock-model' ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].1, "started");
    assert_eq!(rows[1].1, "failed");
    assert_eq!(rows[2].1, "started");
    assert_eq!(rows[3].1, "failed");
    assert_eq!(rows[0].0, rows[1].0);
    assert_eq!(rows[2].0, rows[3].0);
    let first_attempt = rows[0]
        .0
        .rsplit_once("attempt-")
        .map(|(_, value)| value)
        .expect("manual operation key has an attempt suffix");
    assert!(ulid::Ulid::from_string(first_attempt).is_ok());
    assert_ne!(
        rows[0].0, rows[2].0,
        "same source/generation retries need independent durable identities"
    );
}

#[tokio::test]
async fn issue76_auto_source_fence_rejects_unrelated_history_before_summary_request() {
    let (store, dir, _project_id) = setup();
    seed_manual_compaction_history(&store);
    let owner_id = "live-assistant";
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: owner_id.into(),
            thread_id: "thread-1".into(),
            seq: 4,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 4,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let original = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let history = crate::agent::pipeline::history_from_context_source_with_checkpoint(
        &original, None, owner_id,
    )
    .unwrap();
    let wire_messages = std::iter::once(vega_runtime::ChatMessage::new(
        vega_runtime::ChatRole::System,
        "system",
    ))
    .chain(history.iter().cloned())
    .collect::<Vec<_>>();
    let budget = vega_runtime::ContextBudget::new(20_000, 1_000, true).unwrap();
    let estimate = vega_runtime::estimate_wire_context(&wire_messages, &[]).unwrap();

    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "unrelated-latest-user".into(),
            thread_id: "thread-1".into(),
            seq: 5,
            role: "user".into(),
            kind: "text".into(),
            content: "unrelated durable history mutation".into(),
            status: "done".into(),
            created_at: 5,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();

    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("must not be requested".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let hook = ConversationCompactionHook::new(
        &provider,
        dir.path().join("vega.db"),
        "thread-1",
        "mock-model",
        None,
        None,
    );
    let result = vega_runtime::ContextCompactionHook::compact(
        &hook,
        vega_runtime::ContextCompactionRequest {
            system_prompt: "system".into(),
            messages: history,
            tools: Vec::new(),
            budget,
            estimate,
            target_tokens: budget.target_tokens().unwrap(),
            source_version: original.source_version,
            source_fingerprint: Some(original.fingerprint),
            require_source_fence: false,
            source_owner_id: Some(owner_id.into()),
        },
        CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        result,
        Err(failure)
            if matches!(
                failure.error.as_ref(),
                VegaError::Context(vega_runtime::ContextRuntimeError::SourceChanged)
            )
    ));
    assert!(provider.requests().is_empty());
}

#[tokio::test]
async fn issue76_real_hook_compacts_after_persisted_tool_result_without_reexecution() {
    let (store, dir, _project_id) = setup();
    fs::write(dir.path().join("large.txt"), "L".repeat(6_000)).unwrap();
    for seq in 1..=8_i64 {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: format!("tool-threshold-history-{seq}"),
                thread_id: "thread-1".into(),
                seq,
                role: if seq % 2 == 1 { "user" } else { "assistant" }.into(),
                kind: "text".into(),
                content: format!("tool-threshold-history-{seq}-{}", "x".repeat(500)),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    vega_store::context_compaction::save_settings(
        store.conn(),
        &vega_store::context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(11_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "live-read".into(),
                name: "read".into(),
                input_json: r#"{"path":"large.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("tool-loop summary".into()),
            ProviderEvent::Usage {
                input: 120,
                output: 16,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("continued after compaction".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "tool threshold current",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(run.content, "continued after compaction");
    assert_eq!(provider.requests().len(), 3);
    assert!(provider.requests()[1].tools.is_empty());
    assert!(
        provider.requests()[2]
            .messages
            .iter()
            .any(|message| message.tool_call_id.as_deref() == Some("live-read"))
    );
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE id = 'live-read'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn always_permission_and_rule_are_durable_before_second_write() {
    let (store, project_dir, _data_dir, project_id) = setup_external("confirm");
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-first".into(),
                name: "write".into(),
                input_json: r#"{"path":"same.txt","content":"first-secret"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "write-second".into(),
                name: "write".into(),
                input_json: r#"{"path":"same.txt","content":"second-secret"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedPermissionHook {
        calls: calls.clone(),
        decision: PermissionDecision::Always,
    };
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "write twice",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read_to_string(project_dir.path().join("same.txt")).unwrap(),
        "second-secret"
    );
    let rules = permissions::list_exact(store.conn(), &project_id).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].tool, "write");
    assert_eq!(rules[0].pattern, "same.txt");
    let approvals = ["write-first", "write-second"].map(|id| {
        let json: String = store
            .conn()
            .query_row(
                "SELECT approval FROM tool_calls WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        ApprovalAudit::from_json(&json).unwrap()
    });
    assert_eq!(approvals[0].source, ApprovalSource::User);
    assert_eq!(approvals[0].decision, Approval::Always);
    assert_eq!(approvals[1].source, ApprovalSource::Rule);
    assert_eq!(approvals[1].decision, Approval::Always);
    assert!(run.events.iter().all(|event| {
        !format!("{event:?}").contains("first-secret")
            && !format!("{event:?}").contains("second-secret")
    }));
}

#[tokio::test]
async fn danger_readonly_always_rejects_and_persists_rule_atomically() {
    let (store, project_dir, data_dir, project_id) = setup_external("readonly");
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "danger-1".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"rm -rf /"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedPermissionHook {
        calls: calls.clone(),
        decision: PermissionDecision::Always,
    };
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "danger",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (status, approval_json): (String, String) = store
        .conn()
        .query_row(
            "SELECT status, approval FROM tool_calls WHERE id = 'danger-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "rejected");
    let approval = ApprovalAudit::from_json(&approval_json).unwrap();
    assert_eq!(approval.source, ApprovalSource::ReadOnly);
    assert_eq!(approval.decision, Approval::Deny);
    assert_eq!(
        approval.danger.as_ref().map(|danger| danger.decision),
        Some(Approval::Always)
    );
    let rules = permissions::list_exact(store.conn(), &project_id).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].tool, "bash");
    assert_eq!(rules[0].pattern, "rm -rf /");
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.status == ToolCallStatus::Rejected
    )));
    assert_eq!(
        fs::read_dir(data_dir.path().join("checkpoints"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn write_edit_and_bash_execute_serially_with_strict_db_results() {
    let (store, project_dir, data_dir, _project_id) = setup_external("auto");
    fs::write(project_dir.path().join("serial.txt"), "initial").unwrap();
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-1".into(),
                name: "write".into(),
                input_json: r#"{"path":"serial.txt","content":"hello"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "edit-1".into(),
                name: "edit".into(),
                input_json: r#"{"path":"serial.txt","old_string":"hello","new_string":"world"}"#
                    .into(),
            },
            ProviderEvent::ToolUse {
                id: "bash-1".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"cat serial.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "serial tools",
        "system",
        CancellationToken::new(),
        &FixedPermissionHook {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: PermissionDecision::Deny { note: None },
        },
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(
        fs::read_to_string(project_dir.path().join("serial.txt")).unwrap(),
        "world"
    );
    let finished = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } => {
                Some((call_id.as_str(), result.status))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        finished,
        vec![
            ("write-1", ToolCallStatus::Success),
            ("edit-1", ToolCallStatus::Success),
            ("bash-1", ToolCallStatus::Success),
        ]
    );
    let rows = ["write-1", "edit-1", "bash-1"].map(|id| {
            store
                .conn()
                .query_row(
                    "SELECT status, output_text, exit_code, duration_ms, output_full_path FROM tool_calls WHERE id = ?1",
                    [id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<i32>>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .unwrap()
        });
    assert!(rows.iter().all(|row| row.0 == "success" && row.4.is_none()));
    assert!(vega_tools::WriteSuccessOutput::from_json(&rows[0].1).is_ok());
    assert!(vega_tools::EditSuccessOutput::from_json(&rows[1].1).is_ok());
    assert!(rows[2].1.contains("world"));
    assert_eq!(rows[2].2, Some(0));
    assert!(rows[2].3.is_some());
    assert!(data_dir.path().join("checkpoints").exists());
}

#[test]
fn strict_projection_validation_is_semantic_and_binds_results_and_danger() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let audit = tools
        .audit_write_json(r#"{"path":"bound.txt","content":"body"}"#)
        .unwrap();
    let canonical = audit.to_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&canonical).unwrap();
    let reordered = format!(
        r#"{{"fingerprint_v1":{},"content_bytes":{},"path":{},"tool":{},"audit_version":{}}}"#,
        value["fingerprint_v1"],
        value["content_bytes"],
        value["path"],
        value["tool"],
        value["audit_version"]
    );
    assert!(tool_inputs_semantically_equal(
        "write", &canonical, &reordered
    ));

    let invalid = vega_tools::InvalidMutation::from_raw(
        vega_tools::MutationTool::Write,
        r#"{"path":"x","content":"secret","extra":true}"#,
        vega_tools::MutationErrorCode::UnexpectedField,
    )
    .unwrap();
    let invalid_json = invalid.audit().to_json().unwrap();
    let invalid_value: serde_json::Value = serde_json::from_str(&invalid_json).unwrap();
    let invalid_reordered = format!(
        r#"{{"validation_error_code":{},"raw_input_sha256":{},"raw_input_bytes":{},"tool":{},"audit_version":{}}}"#,
        invalid_value["validation_error_code"],
        invalid_value["raw_input_sha256"],
        invalid_value["raw_input_bytes"],
        invalid_value["tool"],
        invalid_value["audit_version"]
    );
    assert!(tool_inputs_semantically_equal(
        "write",
        &invalid_json,
        &invalid_reordered
    ));

    let ids = vega_tools::CheckpointIds::new("project", "thread", "call").unwrap();
    let success = vega_tools::WriteSuccessOutput {
        path: "bound.txt".to_string(),
        bytes_written: 4,
        checkpoint_ref: ids.checkpoint_ref(),
    };
    let success_json = success.to_json().unwrap();
    let auto = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::Auto,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "call",
            "write",
            &reordered,
            &success_json,
            RuntimeToolStatus::Success,
            &auto,
            None,
            None,
        )
        .is_ok()
    );
    for corrupt_output in [
        vega_tools::WriteSuccessOutput {
            path: "other.txt".to_string(),
            ..success.clone()
        }
        .to_json()
        .unwrap(),
        vega_tools::WriteSuccessOutput {
            bytes_written: 5,
            ..success.clone()
        }
        .to_json()
        .unwrap(),
        vega_tools::WriteSuccessOutput {
            checkpoint_ref: vega_tools::CheckpointIds::new("project", "thread", "other")
                .unwrap()
                .checkpoint_ref(),
            ..success.clone()
        }
        .to_json()
        .unwrap(),
        "SECRET_RECOVERY_BODY".to_string(),
    ] {
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "call",
                "write",
                &canonical,
                &corrupt_output,
                RuntimeToolStatus::Success,
                &auto,
                None,
                None,
            )
            .is_err()
        );
    }

    let validation = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Validation,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "invalid",
            "write",
            &invalid_reordered,
            invalid.tool_result(),
            RuntimeToolStatus::Rejected,
            &validation,
            None,
            None,
        )
        .is_ok()
    );

    let dangerous = r#"{"cmd":"rm -rf /"}"#;
    let safe = r#"{"cmd":"printf safe"}"#;
    let wrong_danger = crate::types::DangerAudit {
        rule_id: "wrong".to_string(),
        decision: Approval::Once,
        note: None,
    };
    for (input, approval) in [
        (dangerous, auto.clone()),
        (
            dangerous,
            ApprovalAudit {
                decision: Approval::Once,
                note: None,
                source: ApprovalSource::Danger,
                danger: Some(wrong_danger.clone()),
            },
        ),
        (
            dangerous,
            ApprovalAudit {
                decision: Approval::Once,
                note: None,
                source: ApprovalSource::Legacy,
                danger: None,
            },
        ),
        (
            safe,
            ApprovalAudit {
                decision: Approval::Once,
                note: None,
                source: ApprovalSource::Danger,
                danger: Some(wrong_danger),
            },
        ),
    ] {
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "bash-call",
                "bash",
                input,
                "output",
                RuntimeToolStatus::Success,
                &approval,
                Some(0),
                Some(1),
            )
            .is_err()
        );
    }

    let recovery = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Recovery,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "unknown",
            "future_tool",
            "{}",
            vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
            RuntimeToolStatus::Rejected,
            &recovery,
            None,
            None,
        )
        .is_ok()
    );
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "unknown",
            "future_tool",
            "{\"secret\":true}",
            vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
            RuntimeToolStatus::Rejected,
            &recovery,
            None,
            None,
        )
        .is_err()
    );

    let legacy_deny = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Legacy,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "legacy",
            "write",
            &canonical,
            &legacy_unavailable_output("write"),
            RuntimeToolStatus::Rejected,
            &legacy_deny,
            None,
            None,
        )
        .is_ok()
    );
}

#[tokio::test]
async fn file_backed_recovery_reuses_write_edit_and_unknown_without_execution() {
    let (store, project_dir, data_dir, _project_id) = setup_external("auto");
    fs::write(project_dir.path().join("edit.txt"), "old").unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "old-assistant".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "interrupted".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let write_raw = r#"{"path":"new.txt","content":"recovery-secret"}"#;
    let edit_raw = r#"{"path":"edit.txt","old_string":"old","new_string":"recovery-new"}"#;
    let write_audit = tools
        .audit_write_json(write_raw)
        .unwrap()
        .to_json()
        .unwrap();
    let edit_audit = tools.audit_edit_json(edit_raw).unwrap().to_json().unwrap();
    for (seq, (id, tool, input, status)) in [
        (
            "recover-write",
            "write",
            write_audit.as_str(),
            "pending_approval",
        ),
        ("recover-edit", "edit", edit_audit.as_str(), "running"),
        ("recover-unknown", "future_tool", "{}", "pending_approval"),
    ]
    .into_iter()
    .enumerate()
    {
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id,
                thread_id: "thread-1",
                message_id: "old-assistant",
                seq: i64::try_from(seq + 1).unwrap(),
                tool,
                input_json: input,
                status,
                created_at: 1,
            },
        )
        .unwrap();
    }
    let auto_json = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::Auto,
        danger: None,
    }
    .to_json()
    .unwrap();
    tool_calls::update(
        store.conn(),
        "recover-edit",
        "running",
        Some(&auto_json),
        None,
        None,
    )
    .unwrap();
    let database_path = data_dir.path().join("vega.db");
    drop(store);
    let reopened = Store::open(&database_path).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "recover-write".into(),
                name: "write".into(),
                input_json: write_raw.into(),
            },
            ProviderEvent::ToolUse {
                id: "recover-edit".into(),
                name: "edit".into(),
                input_json: edit_raw.into(),
            },
            ProviderEvent::ToolUse {
                id: "recover-unknown".into(),
                name: "future_tool".into(),
                input_json: r#"{"secret":"must-not-survive"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let run = run_thread_task(
        &reopened,
        &provider,
        &tools,
        "thread-1",
        "resume",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let reused = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } if result.reused => {
                Some((call_id.as_str(), result.status))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        reused,
        vec![
            ("recover-write", ToolCallStatus::Rejected),
            ("recover-edit", ToolCallStatus::Cancelled),
            ("recover-unknown", ToolCallStatus::Rejected),
        ]
    );
    assert!(!project_dir.path().join("new.txt").exists());
    assert_eq!(
        fs::read_to_string(project_dir.path().join("edit.txt")).unwrap(),
        "old"
    );
    let wire = format!("{:?}", provider.requests());
    assert!(!wire.contains("recovery-secret"));
    assert!(!wire.contains("recovery-new"));
    assert!(!wire.contains("must-not-survive"));
}
