use super::*;
use crate::types::{ContextCompactionFailureCode, ContextCompactionStatus, Microcents};
use vega_store::permissions;

type UsageRow = (Option<String>, i64, Option<String>, Option<String>);

fn save_issue76_model_policy(store: &Store, input_limit: u64, output_reserve: u64) {
    vega_store::context_compaction::save_model_policy(
        store.conn(),
        &vega_store::context_compaction::ModelContextPolicy {
            provider: "mock-provider".into(),
            model: "mock-model".into(),
            input_limit: Some(input_limit),
            output_reserve: Some(output_reserve),
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
}

async fn run_issue76_model_owned(
    store: &Store,
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    user_content: &str,
    system_prompt: &str,
    pricing: Option<vega_token::PricingCatalog>,
) -> Result<ConversationRun, ConversationError> {
    run_thread_task_with_permission_config_and_reasoning(
        store,
        provider,
        tools,
        "thread-1",
        user_content,
        system_prompt,
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        pricing,
        Some(FrozenReasoning::unknown("mock-provider", "mock-model")),
    )
    .await
}

struct GatedSummaryProvider {
    started: std::sync::Arc<tokio::sync::Notify>,
    release: std::sync::Arc<tokio::sync::Notify>,
}

struct MutatingSecondStageProvider {
    inner: MockProvider,
    database_path: std::path::PathBuf,
    calls: AtomicUsize,
}

/// Models a reasoning-capable provider that needs more than 4096 output
/// tokens before it can finish the requested concise summary.
struct Issue88OutputCapProvider {
    requests: std::sync::Mutex<Vec<vega_runtime::ChatRequest>>,
}

/// Reproduces a provider that finishes the first dense stage but exhausts
/// its bounded output while processing another similarly large stage.
struct Issue88DenseSecondStageProvider {
    requests: std::sync::Mutex<Vec<vega_runtime::ChatRequest>>,
}

fn issue88_stage_source_bytes(request: &vega_runtime::ChatRequest) -> usize {
    request
        .messages
        .iter()
        .map(|message| message.content.len())
        .sum()
}

struct Issue88LargeRollingSummaryProvider {
    requests: std::sync::Mutex<Vec<vega_runtime::ChatRequest>>,
}

struct Issue91AccumulatingSummaryProvider {
    requests: std::sync::Mutex<Vec<vega_runtime::ChatRequest>>,
}

struct Issue91AdaptiveLengthProvider {
    attempts: std::sync::Mutex<Vec<(vega_runtime::ChatRequest, bool)>>,
}

struct Issue91LaterChildFailureProvider {
    attempts: std::sync::Mutex<Vec<vega_runtime::ChatRequest>>,
}

struct Issue91AttemptCeilingProvider {
    exhausted: std::sync::Mutex<Vec<bool>>,
}

impl vega_runtime::Provider for Issue91AttemptCeilingProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let _ = request;
        self.exhausted.lock().unwrap().push(true);
        Box::pin(async {
            Err(VegaError::Provider {
                status: Some(400),
                message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
                retryable: false,
            })
        })
    }
}

impl vega_runtime::Provider for Issue91LaterChildFailureProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let mut attempts = self.attempts.lock().unwrap();
        attempts.push(request);
        let ordinal = attempts.len();
        let success = ordinal == 2;
        let mut events = vec![Ok(ProviderEvent::TextDelta(if success {
            "left leaf committed only in memory".into()
        } else {
            "FAILED_PARTIAL_MARKER".into()
        }))];
        if ordinal != 1 {
            events.push(Ok(ProviderEvent::Usage {
                input: 9_406,
                output: if success { 800 } else { 8_192 },
                cache_read: 0,
                cache_write: 0,
            }));
        }
        events.push(Ok(ProviderEvent::Done {
            stop_reason: if success {
                StopReason::End
            } else {
                StopReason::Length
            },
        }));
        Box::pin(
            async move { Ok(Box::pin(futures::stream::iter(events)) as vega_runtime::EventStream) },
        )
    }
}

impl vega_runtime::Provider for Issue91AdaptiveLengthProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let exhausted = issue88_stage_source_bytes(&request) > 16 * 1024;
        self.attempts.lock().unwrap().push((request, exhausted));
        let events = vec![
            Ok(ProviderEvent::TextDelta(if exhausted {
                "FAILED_PARTIAL_MARKER".into()
            } else {
                "completed leaf".into()
            })),
            Ok(ProviderEvent::ThinkingDelta("bounded thinking".into())),
            Ok(ProviderEvent::Usage {
                input: 9_406,
                output: if exhausted { 8_192 } else { 900 },
                cache_read: 0,
                cache_write: 0,
            }),
            Ok(ProviderEvent::Done {
                stop_reason: if exhausted {
                    StopReason::Length
                } else {
                    StopReason::End
                },
            }),
        ];
        Box::pin(
            async move { Ok(Box::pin(futures::stream::iter(events)) as vega_runtime::EventStream) },
        )
    }
}

impl vega_runtime::Provider for Issue91AccumulatingSummaryProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let mut requests = self.requests.lock().unwrap();
        let stage = requests.len() + 1;
        let inherited = stage > 1
            && request.messages[1]
                .content
                .contains(&format!("fact-stage-{}", stage - 1));
        requests.push(request);
        let exhausted = stage == 6 && inherited;
        let events = vec![
            Ok(ProviderEvent::TextDelta(format!("fact-stage-{stage}"))),
            Ok(ProviderEvent::Usage {
                input: 10_000,
                output: if exhausted { 8192 } else { 1200 },
                cache_read: 0,
                cache_write: 0,
            }),
            Ok(ProviderEvent::Done {
                stop_reason: if exhausted {
                    StopReason::Length
                } else {
                    StopReason::End
                },
            }),
        ];
        Box::pin(
            async move { Ok(Box::pin(futures::stream::iter(events)) as vega_runtime::EventStream) },
        )
    }
}

impl vega_runtime::Provider for Issue88LargeRollingSummaryProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let mut requests = self.requests.lock().unwrap();
        let first = requests.is_empty();
        requests.push(request);
        let text = if first {
            "s".repeat(24 * 1024)
        } else {
            "short rolling summary".to_string()
        };
        Box::pin(async move {
            Ok(Box::pin(futures::stream::iter(vec![
                Ok(ProviderEvent::TextDelta(text)),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ])) as vega_runtime::EventStream)
        })
    }
}

impl vega_runtime::Provider for Issue88DenseSecondStageProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let bytes = issue88_stage_source_bytes(&request);
        let mut requests = self.requests.lock().unwrap();
        let stage = requests.len();
        requests.push(request);
        let exhausted = stage == 1 && bytes > 32 * 1024;
        let events = vec![
            Ok(ProviderEvent::TextDelta("bounded rolling summary".into())),
            Ok(ProviderEvent::Usage {
                input: 200,
                output: if exhausted { 8192 } else { 1852 },
                cache_read: 0,
                cache_write: 0,
            }),
            Ok(ProviderEvent::Done {
                stop_reason: if exhausted {
                    StopReason::Length
                } else {
                    StopReason::End
                },
            }),
        ];
        Box::pin(
            async move { Ok(Box::pin(futures::stream::iter(events)) as vega_runtime::EventStream) },
        )
    }
}

impl vega_runtime::Provider for Issue88OutputCapProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let cap = request.max_tokens.unwrap_or_default();
        self.requests.lock().unwrap().push(request);
        let events = vec![
            Ok(ProviderEvent::TextDelta("bounded rolling summary".into())),
            Ok(ProviderEvent::Usage {
                input: 200,
                output: if cap <= 4096 { cap } else { 5000 } as u64,
                cache_read: 0,
                cache_write: 0,
            }),
            Ok(ProviderEvent::Done {
                stop_reason: if cap <= 4096 {
                    StopReason::Length
                } else {
                    StopReason::End
                },
            }),
        ];
        Box::pin(
            async move { Ok(Box::pin(futures::stream::iter(events)) as vega_runtime::EventStream) },
        )
    }
}

impl vega_runtime::Provider for MutatingSecondStageProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            let store = Store::open(&self.database_path).unwrap();
            messages::insert(
                store.conn(),
                &messages::MessageRow {
                    id: "concurrent-user".into(),
                    thread_id: "thread-1".into(),
                    seq: 4,
                    role: "user".into(),
                    kind: "text".into(),
                    content: "arrived while staged summary was in flight".into(),
                    status: "done".into(),
                    created_at: 4,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
        }
        self.inner.chat_stream(request, cancel)
    }
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
        summary_request
            .messages
            .iter()
            .flat_map(|m| &m.tool_calls)
            .any(|call| call.name == "read")
    );
    assert!(
        summary_request
            .messages
            .iter()
            .any(|m| m.role == vega_runtime::ChatRole::Tool && m.content == "old read result")
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
                content: format!("historical-{seq}-{}", "x".repeat(1_000)),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    save_issue76_model_policy(&store, 9_000, 1_000);
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
    let run = run_issue76_model_owned(
        &store,
        &provider,
        &tools,
        "current constraint",
        "primary system",
        None,
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
    assert_eq!(requests[1].max_tokens, None);
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
    run_issue76_model_owned(
        &store,
        &reopened_provider,
        &tools,
        "next turn after restart",
        "primary system",
        None,
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
    let first_summary =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap()
            .summary;
    assert!(first_summary.contains("first durable summary"));

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
        requests[1]
            .messages
            .iter()
            .any(|message| message.content.contains("new uncovered durable result"))
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
    let second_summary =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap()
            .summary;
    assert!(!second_summary.contains(&first_summary));
    assert!(second_summary.contains("second durable summary"));
    assert!(second_summary.len() < first_summary.len() + 100);

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

#[tokio::test]
async fn codex_parity_large_prior_checkpoint_is_reduced_and_raw_history_preserved() {
    let (store, _dir, _project_id) = setup();
    seed_manual_compaction_history(&store);
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let previous_summary = "prior state ".repeat(2_000);
    let previous = vega_store::context_compaction::NewContextCheckpoint {
        thread_id: "thread-1".into(),
        model: "mock-model".into(),
        source_version: source.source_version,
        covered_through_seq: 1,
        source_fingerprint: source.fingerprint.clone(),
        summary: previous_summary.clone(),
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
    // A new source revision permits replacing the predecessor checkpoint.
    store.conn().execute("INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES ('later-user', 'thread-1', 4, 'user', 'text', 'continue corrected task', 'done', 4)", []).unwrap();
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("new uncovered fact".into()),
        ProviderEvent::Usage {
            input: 300,
            output: 50,
            cache_read: 0,
            cache_write: 0,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(12_000, 1_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(!result.usages.is_empty());
    assert!(result.usages.iter().all(|usage| usage.usage.output == 50));
    assert!(provider.requests().iter().any(|request| {
        request
            .messages
            .iter()
            .any(|message| message.content.contains("prior state"))
    }));
    let latest =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap();
    assert_ne!(latest.id, previous.id);
    assert!(latest.summary.len() < previous_summary.len());
    let after = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    assert_eq!(after.messages.len(), source.messages.len());
    assert_eq!(after.tool_calls.len(), source.tool_calls.len());
    assert!(
        after
            .messages
            .iter()
            .zip(source.messages.iter())
            .all(|(a, b)| a.id == b.id
                && a.seq == b.seq
                && a.content == b.content
                && a.status == b.status)
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
                content: format!("priced-history-{seq}-{}", "x".repeat(1_700)),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    save_issue76_model_policy(&store, 9_000, 1_000);
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
    let run = run_issue76_model_owned(
        &store,
        &provider,
        &tools,
        "priced current",
        "system",
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
                content: format!("failed-summary-history-{seq}-{}", "x".repeat(1_000)),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    save_issue76_model_policy(&store, 9_000, 1_000);
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
    let result = run_issue76_model_owned(
        &store,
        &provider,
        &tools,
        "failed summary current",
        "system",
        None,
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
    // Keep the initial request below the trigger with the expanded file-tool schemas.
    let input_limit = 11_200;
    let output_reserve = 1_000;
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
                content: format!("tool-threshold-history-{seq}-{}", "x".repeat(2_300)),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    save_issue76_model_policy(&store, input_limit, output_reserve);
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
    let run = run_issue76_model_owned(
        &store,
        &provider,
        &tools,
        "tool threshold current",
        "system",
        None,
    )
    .await
    .unwrap();
    assert_eq!(run.content, "continued after compaction");
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    let budget =
        vega_runtime::ContextBudget::new(input_limit + output_reserve, output_reserve, true)
            .unwrap();
    let trigger_tokens = budget.trigger_tokens().unwrap();
    let initial_estimate =
        vega_runtime::estimate_wire_context(&requests[0].messages, &requests[0].tools).unwrap();
    assert!(!requests[0].tools.is_empty());
    assert!(initial_estimate.input_tokens < trigger_tokens);
    let compaction =
        vega_store::context_compaction::latest_status(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap();
    assert!(compaction.estimated_tokens >= trigger_tokens);
    assert!(requests[1].tools.is_empty());
    assert!(
        requests[2]
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
async fn issue88_long_completed_tool_history_compacts_and_continues_same_run() {
    let (store, dir, _project_id) = setup();
    for seq in 1..=8_i64 {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: format!("long-history-{seq}"),
                thread_id: "thread-1".into(),
                seq,
                role: if seq % 2 == 1 { "user" } else { "assistant" }.into(),
                kind: "text".into(),
                content: format!("historical-{seq}-{}", "x".repeat(100_000)),
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
    for seq in 1..=120_i64 {
        store
            .conn()
            .execute(
                r#"INSERT INTO tool_calls
                 (id, thread_id, message_id, seq, tool, input_json, output_text, status,
                  approval, created_at, finished_at, text_offset_bytes)
                 VALUES (?1, 'thread-1', 'long-history-8', ?2, 'read', '{"path":"lib.rs"}', ?3,
                         'success', ?4, ?2, ?2, ?5)"#,
                (
                    format!("old-call-{seq:03}"),
                    seq,
                    format!("result-{seq:03}-{}", "r".repeat(1_700)),
                    &approval,
                    100_013_i64,
                ),
            )
            .unwrap();
    }
    save_issue76_model_policy(&store, 300_000, 128_000);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("continued after long history".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);

    let run = run_issue76_model_owned(
        &store,
        &provider,
        &tools,
        "current user request",
        "system",
        None,
    )
    .await
    .unwrap();
    assert_eq!(run.content, "continued after long history");
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "one direct summary and one primary request"
    );
    assert!(requests[0].tools.is_empty());
    for seq in 1..=120_i64 {
        let call_id = format!("old-call-{seq:03}");
        assert!(
            requests[0]
                .messages
                .iter()
                .flat_map(|message| &message.tool_calls)
                .any(|call| call.id == call_id)
        );
        assert!(
            requests[0]
                .messages
                .iter()
                .any(|message| message.tool_call_id.as_deref() == Some(call_id.as_str()))
        );
    }
    assert!(!requests.last().unwrap().tools.is_empty());
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM context_checkpoints WHERE thread_id = 'thread-1'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
        "all stages must yield one durable checkpoint"
    );
}

fn seed_issue88_large_tool_result(store: &Store, input: &str, output: &str) {
    for (id, seq, role) in [
        ("earlier-user", 1_i64, "user"),
        ("earlier-assistant", 2_i64, "assistant"),
        ("newest-user", 3_i64, "user"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: format!("message-{seq}"),
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
             VALUES ('long-result', 'thread-1', 'earlier-assistant', 1, 'read', ?1, ?2,
                     'success', ?3, 2, 2, 9)"#,
            (input, output, &approval),
        )
        .unwrap();
}

fn save_issue88_manual_settings(store: &Store) {
    vega_store::context_compaction::save_settings(
        store.conn(),
        &vega_store::context_compaction::ContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(428_000),
            output_reserve: 128_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
}

#[test]
fn issue88_v1_checkpoint_remains_readable_under_v2_with_shared_wrapper() {
    let (store, dir, _project_id) = setup();
    seed_issue88_large_tool_result(&store, r#"{"path":"lib.rs"}"#, "small result");
    let source = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let previous = vega_store::context_compaction::NewContextCheckpoint {
        thread_id: "thread-1".into(),
        model: "mock-model".into(),
        source_version: source.source_version,
        covered_through_seq: 2,
        source_fingerprint: source.fingerprint.clone(),
        summary: "legacy state".into(),
        estimator_version: "vega-context-estimator-v1".into(),
        expected_previous_id: None,
        created_at: 1,
    };
    vega_store::context_compaction::install_checkpoint(store.conn(), &previous).unwrap();
    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let source = vega_store::context_compaction::load_source(reopened.conn(), "thread-1").unwrap();
    let checkpoint = vega_store::context_compaction::latest_checkpoint(
        reopened.conn(),
        "thread-1",
        "mock-model",
    )
    .unwrap()
    .unwrap();
    assert_eq!(checkpoint.estimator_version, "vega-context-estimator-v1");
    assert_eq!(
        vega_runtime::CONTEXT_ESTIMATOR_VERSION,
        "vega-context-estimator-v2"
    );
    let history = super::super::pipeline::primary_history_from_context_source_with_checkpoint(
        &source,
        Some(&checkpoint),
        "",
    )
    .unwrap();
    assert_eq!(history[0].role, vega_runtime::ChatRole::User);
    assert!(history[0].content.contains("legacy state"));
    assert!(history[0].content.contains("untrusted data"));
    assert!(history[0].content.contains("continue the current task"));
    assert!(history.iter().any(|message| message.content == "message-3"));
    let wire = std::iter::once(vega_runtime::ChatMessage::new(
        vega_runtime::ChatRole::System,
        "system",
    ))
    .chain(history)
    .collect::<Vec<_>>();
    assert!(
        vega_runtime::estimate_wire_context(&wire, &[])
            .unwrap()
            .input_tokens
            > 0
    );
}

#[tokio::test]
async fn issue88_one_oversized_tool_result_and_input_reach_bounded_stages_without_loss() {
    let (store, dir, _project_id) = setup();
    let input = format!(r#"{{"path":"{}"}}"#, "i".repeat(20_000));
    let output = "0123456789abcdef".repeat(14_000);
    seed_issue88_large_tool_result(&store, &input, &output);
    let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("bounded summary".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let budget = vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap();
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        budget,
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(result.messages[0].content.contains("bounded summary"));
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].tools.is_empty());
    let estimate = vega_runtime::estimate_wire_context(&requests[0].messages, &[]).unwrap();
    assert!(estimate.input_tokens <= budget.input_budget());
    assert_eq!(
        requests[0]
            .messages
            .iter()
            .flat_map(|m| &m.tool_calls)
            .find(|call| call.id == "long-result")
            .unwrap()
            .input_json,
        input
    );
    assert_eq!(
        requests[0]
            .messages
            .iter()
            .find(|m| m.tool_call_id.as_deref() == Some("long-result"))
            .unwrap()
            .content,
        output
    );
    let after = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    assert_eq!(before, after, "raw messages and tool audit are unchanged");

    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let next_provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("continued after restart".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let resumed = run_thread_task(
        &reopened,
        &next_provider,
        &tools,
        "thread-1",
        "fresh request after restart",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(resumed.content, "continued after restart");
    let primary = &next_provider.requests()[0].messages;
    let restored_summary = primary
        .iter()
        .find(|message| message.content.contains("Historical context summary"))
        .unwrap();
    assert_eq!(restored_summary.content, result.messages[0].content);
    assert!(restored_summary.content.contains("earlier portion"));
    assert!(
        restored_summary
            .content
            .contains("retained recent conversation")
    );
    assert!(
        restored_summary
            .content
            .contains("continue the current task")
    );
    assert_eq!(
        primary
            .iter()
            .filter(|message| message.content.contains("Historical context summary"))
            .count(),
        1,
        "restart must project the durable checkpoint once"
    );
    assert!(
        primary
            .iter()
            .any(|message| message.content == "fresh request after restart")
    );
    assert!(primary.iter().all(|message| {
        message.tool_call_id.as_deref() != Some("long-result")
            && message
                .tool_calls
                .iter()
                .all(|call| call.id != "long-result")
    }));
    assert_eq!(
        reopened
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE id = 'long-result'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
        "restart must not re-execute the historical tool"
    );
}

#[tokio::test]
async fn claude_summary_preserves_reasoning_headroom_in_one_request() {
    let (store, _dir, _project_id) = setup();
    seed_issue88_large_tool_result(
        &store,
        r#"{"path":"lib.rs"}"#,
        &"0123456789abcdef".repeat(14_000),
    );
    let provider = Issue88OutputCapProvider {
        requests: std::sync::Mutex::new(Vec::new()),
    };
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1, "complete source fits one direct request");
    assert!(
        requests
            .iter()
            .all(|request| request.max_tokens == Some(20_000))
    );
    assert_eq!(result.usages.len(), requests.len());
    assert!(result.usage_complete);
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn claude_direct_request_has_no_fixed_byte_staging() {
    let (store, _dir, _project_id) = setup();
    seed_issue88_large_tool_result(
        &store,
        r#"{"path":"lib.rs"}"#,
        &"0123456789abcdef".repeat(14_000),
    );
    let provider = Issue88DenseSecondStageProvider {
        requests: std::sync::Mutex::new(Vec::new()),
    };
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1, "no fixed-byte source stages");
    assert!(requests.iter().all(|request| {
        request.max_tokens == Some(20_000) && issue88_stage_source_bytes(request) > 128 * 1024
    }));
    assert_eq!(result.usages.len(), requests.len());
    assert!(result.usage_complete);
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn claude_large_completed_summary_is_not_sliced_or_reduced() {
    let (store, _dir, _project_id) = setup();
    let output = "0123456789abcdef".repeat(14_000);
    seed_issue88_large_tool_result(&store, r#"{"path":"lib.rs"}"#, &output);
    let provider = Issue88LargeRollingSummaryProvider {
        requests: std::sync::Mutex::new(Vec::new()),
    };
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let first_segment_summary = "s".repeat(24 * 1024);
    assert_eq!(
        requests[0]
            .messages
            .iter()
            .find(|m| m.tool_call_id.as_deref() == Some("long-result"))
            .unwrap()
            .content,
        output
    );
    assert_eq!(
        result.messages[0]
            .content
            .matches(&first_segment_summary)
            .count(),
        1
    );
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM context_checkpoints WHERE thread_id = 'thread-1'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn claude_summary_does_not_accumulate_partial_checkpoints() {
    let (store, _dir, _project_id) = setup();
    let output = "0123456789abcdef".repeat(14_000);
    seed_issue88_large_tool_result(&store, r#"{"path":"lib.rs"}"#, &output);
    let provider = Issue91AccumulatingSummaryProvider {
        requests: std::sync::Mutex::new(Vec::new()),
    };
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].max_tokens, Some(20_000));
    assert_eq!(
        requests[0]
            .messages
            .iter()
            .find(|m| m.tool_call_id.as_deref() == Some("long-result"))
            .unwrap()
            .content,
        output
    );
    assert!(result.messages[0].content.contains("fact-stage-1"));
    assert_eq!(result.usages.len(), requests.len());
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM context_checkpoints WHERE thread_id = 'thread-1'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
    );
}

#[tokio::test]
async fn claude_output_length_does_not_trim_or_commit_partial_summary() {
    let (store, _dir, _) = setup();
    seed_issue88_large_tool_result(&store, "{}", &"0123456789abcdef".repeat(4_000));
    let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let provider = Issue91AdaptiveLengthProvider {
        attempts: std::sync::Mutex::new(Vec::new()),
    };
    let failure = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        failure.error.as_ref(),
        VegaError::Context(vega_runtime::ContextRuntimeError::SummaryOutputTruncated { .. })
    ));
    assert_eq!(provider.attempts.lock().unwrap().len(), 1);
    assert_eq!(failure.usages.len(), 1);
    assert_eq!(failure.usages[0].usage.output, 8192);
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap(),
        before
    );
}

#[tokio::test]
async fn claude_length_without_usage_preserves_predecessor() {
    let (store, _dir, _project_id) = setup();
    seed_issue88_large_tool_result(
        &store,
        r#"{"path":"lib.rs"}"#,
        &"0123456789abcdef".repeat(14_000),
    );
    let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let previous = vega_store::context_compaction::NewContextCheckpoint {
        thread_id: "thread-1".into(),
        model: "mock-model".into(),
        source_version: before.source_version,
        covered_through_seq: 1,
        source_fingerprint: before.fingerprint.clone(),
        summary: "stable prior checkpoint".into(),
        estimator_version: vega_runtime::CONTEXT_ESTIMATOR_VERSION.into(),
        expected_previous_id: None,
        created_at: 1,
    };
    let previous = match vega_store::context_compaction::install_checkpoint(store.conn(), &previous)
        .unwrap()
    {
        vega_store::context_compaction::ContextCheckpointInstall::Applied(checkpoint) => checkpoint,
        other => panic!("expected predecessor checkpoint, got {other:?}"),
    };
    let provider = Issue91LaterChildFailureProvider {
        attempts: std::sync::Mutex::new(Vec::new()),
    };
    let failure = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        failure.error.as_ref(),
        VegaError::Context(vega_runtime::ContextRuntimeError::SummaryOutputTruncated { .. })
    ));
    let attempts = provider.attempts.lock().unwrap();
    assert_eq!(attempts.len(), 1);
    assert!(
        failure.usages.is_empty(),
        "first failed attempt had no usage"
    );
    assert!(!failure.usage_complete);
    let current =
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .unwrap();
    assert_eq!(current.id, previous.id);
    assert_eq!(current.summary, "stable prior checkpoint");
    let after = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    assert_eq!(after.messages.len(), before.messages.len());
    assert_eq!(after.tool_calls.len(), before.tool_calls.len());
    assert!(
        after
            .messages
            .iter()
            .zip(before.messages.iter())
            .all(|(a, b)| a.id == b.id
                && a.seq == b.seq
                && a.content == b.content
                && a.status == b.status)
    );
    assert!(
        after
            .tool_calls
            .iter()
            .zip(before.tool_calls.iter())
            .all(|(a, b)| a.id == b.id
                && a.seq == b.seq
                && a.input_json == b.input_json
                && a.output_text == b.output_text
                && a.status == b.status)
    );
}

#[tokio::test]
async fn claude_repeated_input_overflow_never_starts_fifth_request() {
    let (store, _dir, _) = setup();
    seed_issue88_large_tool_result(&store, "{}", "result");
    for seq in 4..=11 {
        let role = if seq == 11 { "user" } else { "assistant" };
        store.conn().execute("INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at) VALUES (?1,'thread-1',?2,?3,'text','complete round','done',?2)",(format!("round-{seq}"),seq,role)).unwrap();
    }
    let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let provider = Issue91AttemptCeilingProvider {
        exhausted: std::sync::Mutex::new(Vec::new()),
    };
    let failure = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(provider.exhausted.lock().unwrap().len(), 4);
    assert!(failure.usages.is_empty());
    assert!(!failure.usage_complete);
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap(),
        before
    );
}

#[tokio::test]
async fn issue91_generic_invalid_summary_does_not_retry_a_splittable_source() {
    let (store, _dir, _project_id) = setup();
    seed_issue88_large_tool_result(
        &store,
        r#"{"path":"lib.rs"}"#,
        &"0123456789abcdef".repeat(14_000),
    );
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("<summary>unterminated".into()),
        ProviderEvent::Usage {
            input: 9_406,
            output: 300,
            cache_read: 0,
            cache_write: 0,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let failure = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        failure.error.as_ref(),
        VegaError::Context(vega_runtime::ContextRuntimeError::InvalidSummary)
    ));
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(failure.usages.len(), 1);
    assert_eq!(failure.usages[0].usage.output, 300);
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn issue88_empty_completed_tool_result_keeps_an_explicit_paired_marker() {
    let (store, _dir, _project_id) = setup();
    seed_issue88_large_tool_result(&store, r#"{"path":"lib.rs"}"#, "");
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("summary with an empty result".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    let request = &provider.requests()[0];
    assert!(
        request
            .messages
            .iter()
            .flat_map(|m| &m.tool_calls)
            .any(|call| call.id == "long-result")
    );
    assert!(
        request
            .messages
            .iter()
            .any(|m| m.tool_call_id.as_deref() == Some("long-result") && m.content.is_empty())
    );
}

#[tokio::test]
async fn claude_ptl_retry_failure_and_cancel_preserve_known_usage() {
    for (second_stage, expected_status, expected_failure) in [
        (
            ScriptStep::Error {
                status: Some(503),
                message: "later stage unavailable".into(),
                retryable: false,
            },
            ContextCompactionStatus::Failed,
            ContextCompactionFailureCode::Unavailable,
        ),
        (
            ScriptStep::Cancelled,
            ContextCompactionStatus::Cancelled,
            ContextCompactionFailureCode::Cancelled,
        ),
    ] {
        let (store, _dir, _project_id) = setup();
        seed_issue88_large_tool_result(
            &store,
            r#"{"path":"lib.rs"}"#,
            &"0123456789abcdef".repeat(14_000),
        );
        save_issue88_manual_settings(&store);
        let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![
                ScriptStep::events(vec![
                    ProviderEvent::TextDelta("first rolling summary".into()),
                    ProviderEvent::Usage {
                        input: 101,
                        output: 11,
                        cache_read: 0,
                        cache_write: 0,
                    },
                ]),
                ScriptStep::Error {
                    status: Some(400),
                    message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
                    retryable: false,
                },
            ],
            vec![second_stage],
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
            88,
        )
        .await
        .unwrap();
        assert_eq!(provider.requests().len(), 2);
        assert!(events.iter().any(|event| matches!(
            event,
            ConversationEvent::ContextCompactionUsageUpdated { usage, .. }
                if usage.input == 101 && usage.output == 11
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ConversationEvent::ContextCompactionStatus { record }
                if record.status == expected_status && record.failure == Some(expected_failure)
        )));
        let usage: (i64, i64, Option<String>) = store
            .conn()
            .query_row(
                "SELECT input_tokens, output_tokens, message_id FROM token_usage \
                 WHERE thread_id = 'thread-1' ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(usage, (101, 11, None));
        assert!(
            vega_store::context_compaction::latest_checkpoint(
                store.conn(),
                "thread-1",
                "mock-model"
            )
            .unwrap()
            .is_none()
        );
        let after = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
        assert_eq!(before, after);
    }
}

#[tokio::test]
async fn claude_source_mutation_rejects_checkpoint_and_retains_usage() {
    let (store, dir, _project_id) = setup();
    seed_issue88_large_tool_result(
        &store,
        r#"{"path":"lib.rs"}"#,
        &"0123456789abcdef".repeat(14_000),
    );
    save_issue88_manual_settings(&store);
    let provider = MutatingSecondStageProvider {
        inner: MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("rolling summary".into()),
            ProviderEvent::Usage {
                input: 103,
                output: 13,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]),
        database_path: dir.path().join("vega.db"),
        calls: AtomicUsize::new(0),
    };
    let events = compact_thread_manually_accounted(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        CancellationToken::new(),
        None,
        None,
        89,
    )
    .await
    .unwrap();
    let stages = provider.calls.load(Ordering::SeqCst);
    assert_eq!(stages, 1);
    assert!(events.iter().any(|event| matches!(
        event,
        ConversationEvent::ContextCompactionStatus { record }
            if record.status == ContextCompactionStatus::Failed
                && record.failure == Some(ContextCompactionFailureCode::SourceChanged)
    )));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ConversationEvent::ContextCompactionUsageUpdated { usage, .. } if usage.input == 103 && usage.output == 13))
            .count(),
        stages,
    );
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE id = 'concurrent-user'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
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
    tools.read("serial.txt", None, None).unwrap();
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

#[tokio::test]
async fn codex_parity_packs_large_history_into_few_requests() {
    let (store, _dir, _) = setup();
    seed_issue88_large_tool_result(&store, "{}", &"0123456789abcdef".repeat(14_000));
    let provider = Issue91AccumulatingSummaryProvider {
        requests: std::sync::Mutex::new(Vec::new()),
    };
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(
        provider.requests.lock().unwrap().len() <= 4,
        "bounded packing plus reduction"
    );
    assert_eq!(result.messages.last().unwrap().content, "message-3");
}

#[tokio::test]
async fn codex_parity_irreducible_suffix_makes_no_summary_call() {
    let (store, _dir, _) = setup();
    seed_manual_compaction_history(&store);
    store
        .conn()
        .execute(
            "UPDATE messages SET content=?1 WHERE id='manual-current-user'",
            [&"x".repeat(50_000)],
        )
        .unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("summary".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let failure = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        failure.error.as_ref(),
        VegaError::Context(vega_runtime::ContextRuntimeError::ResultOverLimit { .. })
    ));
    assert!(
        provider.requests().is_empty(),
        "fixed suffix cannot fit, avoid billed work"
    );
}

#[tokio::test]
async fn codex_parity_historical_images_survive_compaction_and_reload() {
    let (store, dir, _) = setup();
    seed_manual_compaction_history(&store);
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(4, 3, image::Rgb([20, 40, 60]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    for id in ["manual-old-user", "manual-current-user"] {
        vega_store::image_attachments::insert(store.conn(), id, 0, png.get_ref()).unwrap();
    }
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("visual task continuation".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let result = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        result
            .messages
            .iter()
            .map(|m| m.images.len())
            .sum::<usize>(),
        2
    );
    assert!(
        provider.requests()[0]
            .messages
            .iter()
            .all(|m| m.images.is_empty())
    );
    assert!(
        provider.requests()[0].messages[1]
            .content
            .contains("attachment")
    );
    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let source = vega_store::context_compaction::load_source(reopened.conn(), "thread-1").unwrap();
    let checkpoint = vega_store::context_compaction::latest_checkpoint(
        reopened.conn(),
        "thread-1",
        "mock-model",
    )
    .unwrap();
    let history = super::super::pipeline::primary_history_from_context_source_with_checkpoint(
        &source,
        checkpoint.as_ref(),
        "none",
    )
    .unwrap();
    assert_eq!(history, result.messages);
    assert_eq!(source.images.len(), 2);
}

#[tokio::test]
async fn codex_parity_realistic_images_repeat_budget_omission_and_corruption() {
    let (store, dir, _) = setup();
    seed_manual_compaction_history(&store);
    let mut state = 17u32;
    let image = image::RgbImage::from_fn(270, 260, |_, _| {
        let mut pixel = [0; 3];
        for value in &mut pixel {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *value = state as u8;
        }
        image::Rgb(pixel)
    });
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    assert!((210_000..220_000).contains(&png.get_ref().len()));
    for id in ["manual-old-user", "manual-current-user"] {
        vega_store::image_attachments::insert(store.conn(), id, 0, png.get_ref()).unwrap();
    }
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("visual task checkpoint".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let budget = vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap();
    let first = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        budget,
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        first.messages.iter().map(|m| m.images.len()).sum::<usize>(),
        2
    );
    assert_eq!(
        first.messages.last().unwrap().images[0].bytes(),
        png.get_ref()
    );
    store.conn().execute("INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) VALUES ('image-latest', 'thread-1', 4, 'user', 'text', 'latest visual task', 'done', 4)", []).unwrap();
    vega_store::image_attachments::insert(store.conn(), "image-latest", 0, png.get_ref()).unwrap();
    let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let second = compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        budget,
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        second
            .messages
            .iter()
            .map(|m| m.images.len())
            .sum::<usize>(),
        2
    );
    let retained = second
        .messages
        .iter()
        .find(|m| !m.images.is_empty())
        .unwrap();
    assert!(retained.content.contains("manual-current-user"));
    assert!(retained.content.contains("untrusted data"));
    assert_eq!(retained.images[0].bytes(), png.get_ref());
    assert!(second.messages.iter().any(|m| {
        m.content.contains("1 earlier attachments")
            && m.content
                .contains("visual data is not in the current projection")
    }));
    assert_eq!(
        second.messages.last().unwrap().content,
        "latest visual task"
    );
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap(),
        before
    );
    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let source = vega_store::context_compaction::load_source(reopened.conn(), "thread-1").unwrap();
    let checkpoint = vega_store::context_compaction::latest_checkpoint(
        reopened.conn(),
        "thread-1",
        "mock-model",
    )
    .unwrap();
    let history = super::super::pipeline::primary_history_from_context_source_with_checkpoint(
        &source,
        checkpoint.as_ref(),
        "none",
    )
    .unwrap();
    assert_eq!(history, second.messages);
    let projected = read_context_projection(&reopened, "thread-1", "mock-model", "system").unwrap();
    let tools = vega_runtime::tool_definitions(vega_runtime::RuntimeRunMode::Execute);
    assert_eq!(
        projected.estimated_tokens,
        Some(
            vega_runtime::estimate_chat_context("system", &history, &tools)
                .unwrap()
                .input_tokens
        )
    );
    let mut corrupt = source.clone();
    corrupt.images[0].encoded = vec![1, 2, 3];
    assert!(
        super::super::pipeline::primary_history_from_context_source_with_checkpoint(
            &corrupt,
            checkpoint.as_ref(),
            "none"
        )
        .is_err(),
        "even omitted image corruption must fail closed"
    );
}

#[tokio::test]
async fn claude_direct_summary_preserves_roles_and_uses_20k_cap() {
    let (store, _dir, _) = setup();
    let output = "0123456789abcdef".repeat(14_000);
    seed_issue88_large_tool_result(&store, "{}", &output);
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("direct continuation".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    let calls = provider.requests();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].max_tokens, Some(20_000));
    assert!(
        calls[0]
            .messages
            .iter()
            .any(|m| m.role == vega_runtime::ChatRole::Tool && m.content == output)
    );
    assert_eq!(
        calls[0]
            .messages
            .iter()
            .flat_map(|m| m.tool_calls.iter())
            .count(),
        1
    );
    assert!(calls[0].tools.is_empty());
}

#[tokio::test]
async fn claude_prompt_too_long_trims_complete_round_then_retries() {
    let (store, _dir, _) = setup();
    seed_issue88_large_tool_result(&store, "{}", "full tool result");
    let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::Error {
            status: Some(400),
            message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
            retryable: false,
        }],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("direct continuation".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(428_000, 128_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    let calls = provider.requests();
    assert_eq!(calls.len(), 2);
    assert!(
        calls[1]
            .messages
            .iter()
            .any(|m| m.content.contains("Earlier historical API rounds omitted"))
    );
    assert!(
        calls[1]
            .messages
            .iter()
            .any(|m| m.role == vega_runtime::ChatRole::Tool && m.content == "full tool result")
    );
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap(),
        before
    );
}

#[tokio::test]
async fn claude_failed_summary_rechecks_source_before_soft_fallback() {
    for overlarge in [false, true] {
        let (store, dir, _) = setup();
        seed_manual_compaction_history(&store);
        let mut steps = vec![ScriptStep::events(vec![ProviderEvent::Usage {
            input: 43,
            output: 7,
            cache_read: 0,
            cache_write: 0,
        }])];
        if overlarge {
            steps.push(ScriptStep::events(vec![
                ProviderEvent::TextDelta("x".repeat(64_000)),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]));
        } else {
            steps.push(ScriptStep::Error {
                status: Some(503),
                message: "temporary failure".into(),
                retryable: false,
            });
        }
        let provider = MutatingSecondStageProvider {
            inner: MockProvider::new(steps),
            database_path: dir.path().join("vega.db"),
            calls: AtomicUsize::new(0),
        };
        let failure = compact_thread_manually(
            &store,
            &provider,
            "thread-1",
            "mock-model",
            "system",
            vec![],
            vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                failure.error.as_ref(),
                VegaError::Context(vega_runtime::ContextRuntimeError::SourceChanged)
            ),
            "a failed request is not authority to continue stale context"
        );
        assert_eq!(failure.usages.len(), 1);
        assert_eq!(failure.usages[0].usage.output, 7);
        assert!(
            vega_store::context_compaction::latest_checkpoint(
                store.conn(),
                "thread-1",
                "mock-model"
            )
            .unwrap()
            .is_none()
        );
    }
}

#[tokio::test]
async fn claude_local_preflight_and_provider_overflow_share_three_trims() {
    let (store, _dir, _) = setup();
    seed_manual_compaction_history(&store);
    store
        .conn()
        .execute(
            "UPDATE messages SET content=?1 WHERE id='manual-old-user'",
            [&"x".repeat(80_000)],
        )
        .unwrap();
    for seq in 4..=10 {
        let role = if seq == 10 { "user" } else { "assistant" };
        store.conn().execute("INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at) VALUES (?1,'thread-1',?2,?3,'text','complete round','done',?2)",(format!("local-round-{seq}"),seq,role)).unwrap();
    }
    let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
    let provider = MockProvider::new(vec![ScriptStep::Error {
        status: Some(400),
        message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
        retryable: false,
    }]);
    compact_thread_manually(
        &store,
        &provider,
        "thread-1",
        "mock-model",
        "system",
        vec![],
        vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap_err();
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        3,
        "one trim already used before any HTTP request"
    );
    let mut previous = usize::MAX;
    for request in &requests {
        assert!(
            vega_runtime::estimate_wire_context(&request.messages, &[])
                .unwrap()
                .input_tokens
                <= 19_000
        );
        assert_eq!(
            request
                .messages
                .iter()
                .filter(|m| m.content.contains("Earlier historical API rounds omitted"))
                .count(),
            1
        );
        let retained = request
            .messages
            .iter()
            .filter(|m| !m.content.contains("Earlier historical API rounds omitted"))
            .count();
        assert!(
            retained < previous,
            "each retry removes real history, not its own marker"
        );
        previous = retained;
    }
    assert_eq!(
        vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap(),
        before
    );
    assert!(
        vega_store::context_compaction::latest_checkpoint(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn claude_unrelated_provider_failures_never_trim_history() {
    for (status, diagnostic) in [
        (400, "bad request"),
        (401, "prompt too long"),
        (429, "prompt too long"),
        (503, "prompt too long"),
    ] {
        let (store, _dir, _) = setup();
        seed_manual_compaction_history(&store);
        let before = vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap();
        let provider = MockProvider::new(vec![ScriptStep::Error {
            status: Some(status),
            message: diagnostic.into(),
            retryable: false,
        }]);
        compact_thread_manually(
            &store,
            &provider,
            "thread-1",
            "mock-model",
            "system",
            vec![],
            vega_runtime::ContextBudget::new(20_000, 1_000, false).unwrap(),
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(provider.requests().len(), 1);
        assert!(
            provider.requests()[0]
                .messages
                .iter()
                .any(|m| m.content == "the original goal")
        );
        assert_eq!(
            vega_store::context_compaction::load_source(store.conn(), "thread-1").unwrap(),
            before
        );
    }
}
