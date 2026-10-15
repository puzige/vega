use super::*;
use futures::future::BoxFuture;
use std::sync::Mutex;
use vega_runtime::{
    EventStream, MockProvider, ProviderResponseMetadata, ProviderStream, ScriptStep,
};

struct TimedSummaryProvider {
    inner: MockProvider,
    acquisition_delay: Duration,
    tokens: Mutex<Vec<CancellationToken>>,
}

impl TimedSummaryProvider {
    fn new(scripts: Vec<Vec<ScriptStep>>, acquisition_seconds: u64) -> Self {
        Self {
            inner: MockProvider::new_rounds(scripts),
            acquisition_delay: Duration::from_secs(acquisition_seconds),
            tokens: Mutex::new(Vec::new()),
        }
    }
}

impl Provider for TimedSummaryProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.tokens.lock().unwrap().push(cancel.clone());
        let inner = self.inner.clone();
        let delay = self.acquisition_delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            inner.chat_stream(request, cancel).await
        })
    }

    fn chat_stream_with_metadata(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        let stream = self.chat_stream(request, cancel);
        Box::pin(async move {
            Ok(ProviderStream {
                events: stream.await?,
                metadata: ProviderResponseMetadata {
                    http_status: Some(200),
                    request_id: None,
                    retry_count: Some(0),
                },
            })
        })
    }
}

fn usage(output: u64) -> ScriptStep {
    ScriptStep::events(vec![ProviderEvent::Usage {
        input: 42,
        output,
        cache_read: 0,
        cache_write: 0,
    }])
}

fn done() -> ScriptStep {
    ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])
}

fn delay(seconds: u64) -> ScriptStep {
    ScriptStep::delay(Duration::from_secs(seconds))
}

fn assert_timeout(error: &VegaError) {
    assert!(matches!(
        error,
        VegaError::Context(ContextRuntimeError::SummaryTimedOut)
    ));
    assert_eq!(
        vega_runtime::ContextCompactionStatusFailure::from_error(error),
        vega_runtime::ContextCompactionStatusFailure::TimedOut
    );
}

fn assert_child_lifecycle(provider: &TimedSummaryProvider, parent: &CancellationToken) {
    let tokens = provider.tokens.lock().unwrap();
    assert!(!tokens.is_empty());
    assert!(tokens.iter().all(CancellationToken::is_cancelled));
    assert!(!parent.is_cancelled());
}

#[tokio::test(start_paused = true)]
async fn issue356_meaningful_deltas_allow_summaries_beyond_sixty_and_three_hundred_seconds() {
    let mut cases = vec![(
        vec![ScriptStep::text("complete"), delay(70), usage(3), done()],
        70,
        "complete",
    )];
    for (event, expected) in [
        (
            ProviderEvent::TextDelta(" summary".into()),
            "complete summary",
        ),
        (ProviderEvent::ThinkingDelta("working".into()), "complete"),
        (ProviderEvent::SummaryDelta("organizing".into()), "complete"),
    ] {
        cases.push((
            vec![
                ScriptStep::text("complete"),
                delay(210),
                ScriptStep::events(vec![event]),
                delay(210),
                usage(3),
                done(),
            ],
            420,
            expected,
        ));
    }
    for (script, seconds, expected) in cases {
        let provider = TimedSummaryProvider::new(vec![script], 0);
        let parent = CancellationToken::new();
        let started = tokio::time::Instant::now();
        let (summary, observed) = collect_summary_with_timeout(
            &provider,
            ChatRequest::default(),
            None,
            parent.clone(),
            SUMMARY_OPERATION_TIMEOUT,
        )
        .await
        .unwrap();
        assert_eq!(summary, expected);
        assert_eq!(observed.unwrap().usage.output, 3);
        assert_eq!(started.elapsed(), Duration::from_secs(seconds));
        assert_child_lifecycle(&provider, &parent);
    }
}

#[tokio::test(start_paused = true)]
async fn issue356_acquisition_and_first_meaningful_delta_share_the_idle_interval() {
    for (acquisition, script, expected_usage) in [
        (301, vec![ScriptStep::text("late"), done()], 0),
        (150, vec![delay(151), ScriptStep::text("late"), done()], 0),
        (
            0,
            vec![ScriptStep::text("partial"), usage(9), delay(301), done()],
            1,
        ),
    ] {
        let provider = TimedSummaryProvider::new(vec![script], acquisition);
        let parent = CancellationToken::new();
        let started = tokio::time::Instant::now();
        let failure = collect_summary_with_timeout(
            &provider,
            ChatRequest::default(),
            None,
            parent.clone(),
            SUMMARY_OPERATION_TIMEOUT,
        )
        .await
        .unwrap_err();
        assert_timeout(failure.error.as_ref());
        assert_eq!(started.elapsed(), Duration::from_secs(300));
        assert_eq!(failure.usages.len(), expected_usage);
        assert_eq!(failure.usage_complete, expected_usage != 0);
        assert_child_lifecycle(&provider, &parent);
    }
}

#[tokio::test(start_paused = true)]
async fn issue356_empty_deltas_replay_and_usage_do_not_renew_idle_waiting() {
    let provider = TimedSummaryProvider::new(
        vec![vec![
            usage(9),
            delay(100),
            ScriptStep::events(vec![
                ProviderEvent::TextDelta(String::new()),
                ProviderEvent::ThinkingDelta(String::new()),
                ProviderEvent::SummaryDelta(String::new()),
            ]),
            delay(100),
            ScriptStep::events(vec![ProviderEvent::ReasoningReplay(vec![
                serde_json::json!({"opaque": "replay"}),
            ])]),
            delay(100),
            ScriptStep::text("too late"),
            done(),
        ]],
        0,
    );
    let parent = CancellationToken::new();
    let started = tokio::time::Instant::now();
    let failure = collect_summary_with_timeout(
        &provider,
        ChatRequest::default(),
        None,
        parent.clone(),
        SUMMARY_OPERATION_TIMEOUT,
    )
    .await
    .unwrap_err();
    assert_timeout(failure.error.as_ref());
    assert_eq!(started.elapsed(), Duration::from_secs(300));
    assert_eq!(failure.usages.len(), 1);
    assert_eq!(failure.usages[0].usage.output, 9);
    assert_child_lifecycle(&provider, &parent);
}

#[tokio::test(start_paused = true)]
async fn issue356_progress_cannot_renew_the_six_hundred_second_operation_deadline() {
    let provider = TimedSummaryProvider::new(
        vec![vec![
            ScriptStep::text("partial"),
            usage(9),
            delay(200),
            ScriptStep::events(vec![ProviderEvent::ThinkingDelta("working".into())]),
            delay(200),
            ScriptStep::events(vec![ProviderEvent::SummaryDelta("organizing".into())]),
            delay(200),
            ScriptStep::text(" late"),
            done(),
        ]],
        0,
    );
    let parent = CancellationToken::new();
    let started = tokio::time::Instant::now();
    let failure = collect_summary_with_timeout(
        &provider,
        ChatRequest::default(),
        None,
        parent.clone(),
        SUMMARY_OPERATION_TIMEOUT,
    )
    .await
    .unwrap_err();
    assert_timeout(failure.error.as_ref());
    assert_eq!(started.elapsed(), Duration::from_secs(600));
    assert_eq!(failure.usages.len(), 1);
    assert_child_lifecycle(&provider, &parent);
}

fn fixture() -> (Store, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().join("vega.db")).unwrap();
    store.migrate().unwrap();
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            id: "timeout-thread",
            project_id: "",
            title: "fixture",
            mode: "execute",
            permission_mode: "readonly",
            model: "mock-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    for (index, role, content) in [
        (1, "user", "old request"),
        (2, "assistant", "old answer"),
        (3, "user", "later request"),
        (4, "assistant", "later answer"),
        (5, "user", "continue"),
    ] {
        vega_store::messages::insert(
            store.conn(),
            &vega_store::messages::MessageRow {
                id: format!("message-{index}"),
                thread_id: "timeout-thread".into(),
                seq: index,
                role: role.into(),
                kind: String::new(),
                content: content.into(),
                status: "done".into(),
                created_at: index,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    save_settings(
        store.conn(),
        &StoreContextSettings {
            thread_id: "timeout-thread".into(),
            model: "mock-model".into(),
            context_limit: Some(50_000),
            output_reserve: 1_000,
            automatic_compaction: false,
            updated_at: 1,
        },
    )
    .unwrap();
    (store, directory)
}

#[tokio::test(start_paused = true)]
async fn issue356_progressing_manual_summary_installs_one_complete_checkpoint() {
    let (store, _directory) = fixture();
    let before =
        vega_store::context_compaction::load_source(store.conn(), "timeout-thread").unwrap();
    let provider = TimedSummaryProvider::new(
        vec![vec![
            ScriptStep::text("complete"),
            delay(70),
            usage(3),
            done(),
        ]],
        0,
    );
    let result = compact_thread_manually(
        &store,
        &provider,
        "timeout-thread",
        "mock-model",
        "system",
        Vec::new(),
        ContextBudget::new(50_000, 1_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(result.usages.len(), 1);
    let checkpoint = vega_store::context_compaction::latest_checkpoint(
        store.conn(),
        "timeout-thread",
        "mock-model",
    )
    .unwrap()
    .unwrap();
    assert_eq!(checkpoint.summary, "complete");
    assert_eq!(checkpoint.covered_through_seq, 4);
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM context_checkpoints", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    let after =
        vega_store::context_compaction::load_source(store.conn(), "timeout-thread").unwrap();
    assert_eq!(before.fingerprint, after.fingerprint);
    assert_eq!(result.messages.last().unwrap().content, "continue");
}

#[tokio::test(start_paused = true)]
async fn issue356_input_overflow_retry_shares_operation_deadline_without_partial_checkpoint() {
    let (store, _directory) = fixture();
    let before =
        vega_store::context_compaction::load_source(store.conn(), "timeout-thread").unwrap();
    let provider = TimedSummaryProvider::new(
        vec![
            vec![
                ScriptStep::text("partial first attempt"),
                delay(190),
                ScriptStep::events(vec![ProviderEvent::ThinkingDelta("working".into())]),
                delay(190),
                usage(6),
                ScriptStep::Error {
                    status: Some(400),
                    message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
                    retryable: false,
                },
            ],
            vec![
                ScriptStep::text("partial retry"),
                usage(9),
                delay(190),
                ScriptStep::text(" progressing"),
                delay(190),
                done(),
            ],
        ],
        0,
    );
    let parent = CancellationToken::new();
    let started = tokio::time::Instant::now();
    let events = compact_thread_manually_accounted(
        &store,
        &provider,
        "timeout-thread",
        "mock-model",
        "system",
        parent.clone(),
        None,
        None,
        1,
    )
    .await
    .unwrap();
    assert_eq!(started.elapsed(), Duration::from_secs(600));
    let requests = provider.inner.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0]
            .messages
            .iter()
            .any(|message| message.content == "old request")
    );
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.content == "old request")
    );
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.content == "later request")
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                crate::types::ConversationEvent::ContextCompactionUsageUpdated { .. }
            ))
            .count(),
        2
    );
    assert!(matches!(
        events.last(),
        Some(crate::types::ConversationEvent::ContextCompactionStatus { record })
            if record.status == crate::types::ContextCompactionStatus::Failed
                && record.failure == Some(crate::types::ContextCompactionFailureCode::TimedOut)
    ));
    let usage_rows: (i64, i64) = store
        .conn()
        .query_row(
            "SELECT COUNT(*), SUM(output_tokens) FROM token_usage",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(usage_rows, (2, 15));
    assert!(
        vega_store::context_compaction::latest_checkpoint(
            store.conn(),
            "timeout-thread",
            "mock-model"
        )
        .unwrap()
        .is_none()
    );
    let after =
        vega_store::context_compaction::load_source(store.conn(), "timeout-thread").unwrap();
    assert_eq!(before.fingerprint, after.fingerprint);
    assert_child_lifecycle(&provider, &parent);
    let reopened = Store::open_read_only(store.database_path().unwrap()).unwrap();
    let projection =
        read_context_projection(&reopened, "timeout-thread", "mock-model", "system").unwrap();
    assert_eq!(
        projection.last_status.unwrap().failure,
        Some(crate::types::ContextCompactionFailureCode::TimedOut)
    );
}

#[tokio::test(start_paused = true)]
async fn issue356_cancellation_during_acquisition_or_stream_keeps_history_and_usage() {
    for acquisition in [0, 200] {
        let (store, _directory) = fixture();
        let before =
            vega_store::context_compaction::load_source(store.conn(), "timeout-thread").unwrap();
        let provider = TimedSummaryProvider::new(
            vec![vec![
                ScriptStep::text("partial"),
                usage(9),
                delay(200),
                done(),
            ]],
            acquisition,
        );
        let parent = CancellationToken::new();
        let compact = compact_thread_manually_accounted(
            &store,
            &provider,
            "timeout-thread",
            "mock-model",
            "system",
            parent.clone(),
            None,
            None,
            1,
        );
        let cancel = async {
            while provider.tokens.lock().unwrap().is_empty() {
                tokio::task::yield_now().await;
            }
            tokio::time::sleep(Duration::from_secs(100)).await;
            parent.cancel();
        };
        let (events, ()) = tokio::join!(compact, cancel);
        let events = events.unwrap();
        assert!(
            matches!(events.last(), Some(crate::types::ConversationEvent::ContextCompactionStatus { record }) if record.status == crate::types::ContextCompactionStatus::Cancelled)
        );
        let count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM token_usage", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, i64::from(acquisition == 0));
        assert!(
            vega_store::context_compaction::latest_checkpoint(
                store.conn(),
                "timeout-thread",
                "mock-model"
            )
            .unwrap()
            .is_none()
        );
        let after =
            vega_store::context_compaction::load_source(store.conn(), "timeout-thread").unwrap();
        assert_eq!(before.fingerprint, after.fingerprint);
        assert!(
            provider
                .tokens
                .lock()
                .unwrap()
                .iter()
                .all(CancellationToken::is_cancelled)
        );
    }
}

#[test]
fn issue356_automatic_status_events_persist_and_restore_timeout_without_reclassifying_legacy() {
    let (store, _directory) = fixture();
    for (generation, error, expected) in [
        (
            1,
            ContextRuntimeError::SummaryFormatInvalid,
            crate::types::ContextCompactionFailureCode::InvalidSummary,
        ),
        (
            2,
            ContextRuntimeError::SummaryTimedOut,
            crate::types::ContextCompactionFailureCode::TimedOut,
        ),
    ] {
        let event = vega_runtime::RuntimeEvent::ContextCompactionStatusUpdated {
            status: vega_runtime::ContextCompactionStatusUpdate {
                operation_key: format!("automatic-{generation}"),
                generation,
                phase: vega_runtime::ContextCompactionPhase::Failed,
                source_version: 5,
                estimated_tokens: 40_000,
                input_budget: 49_000,
                target_tokens: 29_400,
                usage: vega_runtime::ContextCompactionUsageState::Unknown,
                failure: Some(vega_runtime::ContextCompactionStatusFailure::from_error(
                    &VegaError::Context(error),
                )),
            },
        };
        let projected = crate::types::from_runtime_event("automatic", &event).unwrap();
        assert!(
            matches!(projected, crate::types::ConversationEvent::ContextCompactionStatus { record } if record.failure == Some(expected))
        );
        let mut seq = 1;
        super::super::events::persist_runtime_event(
            &store,
            "",
            "timeout-thread",
            "automatic",
            "mock-model",
            false,
            "",
            &mut seq,
            &event,
            None,
        )
        .unwrap();
        let reopened = Store::open_read_only(store.database_path().unwrap()).unwrap();
        let projection =
            read_context_projection(&reopened, "timeout-thread", "mock-model", "system").unwrap();
        assert_eq!(projection.last_status.unwrap().failure, Some(expected));
        assert!(projection.unknown_usage);
    }
}

#[tokio::test]
async fn issue362_staged_summary_preserves_early_goal_and_every_progress_group() {
    let (store, _directory) = fixture();
    for (id, marker) in [
        ("message-2", "EARLY_PROGRESS"),
        ("message-4", "RECENT_PROGRESS"),
    ] {
        store
            .conn()
            .execute(
                "UPDATE messages SET content = ?1 WHERE id = ?2",
                (format!("{marker} {}", "abcdefgh ".repeat(2000)), id),
            )
            .unwrap();
    }
    let provider = TimedSummaryProvider::new(
        vec![
            vec![
                ScriptStep::text("old request; EARLY_PROGRESS; pending later request"),
                usage(3),
                done(),
            ],
            vec![
                ScriptStep::text(
                    "old request; EARLY_PROGRESS; RECENT_PROGRESS; continue pending work",
                ),
                usage(4),
                done(),
            ],
            vec![
                ScriptStep::text(
                    "old request; EARLY_PROGRESS; RECENT_PROGRESS; continue pending work",
                ),
                usage(5),
                done(),
            ],
        ],
        0,
    );
    let result = compact_thread_manually(
        &store,
        &provider,
        "timeout-thread",
        "mock-model",
        "system",
        Vec::new(),
        ContextBudget::new(10_000, 1_000, false).unwrap(),
        CancellationToken::new(),
        None,
        None,
    )
    .await
    .unwrap();
    let requests = provider.inner.requests();
    assert!(requests.len() > 1);
    assert!(
        requests[0]
            .messages
            .iter()
            .any(|message| message.content == "old request")
    );
    for marker in ["EARLY_PROGRESS", "RECENT_PROGRESS"] {
        assert_eq!(
            requests
                .iter()
                .flat_map(|request| &request.messages)
                .filter(|message| message.content.starts_with(marker))
                .count(),
            1
        );
    }
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.content.contains("old request; EARLY_PROGRESS"))
    );
    assert!(
        result
            .messages
            .iter()
            .any(|message| message.content.contains("RECENT_PROGRESS"))
    );
    assert_eq!(result.usages.len(), requests.len());
}

#[tokio::test]
async fn issue397_manual_compaction_keeps_returned_failure_when_token_is_cancelled() {
    let (store, _directory) = fixture();
    let provider = MockProvider::new(vec![]);
    let cancel = CancellationToken::new();
    let reader_cancel = cancel.clone();
    let reader: vega_runtime::CredentialReader = Arc::new(move || {
        reader_cancel.cancel();
        Err(())
    });
    let events = crate::agent::compact_thread_manually_accounted_with_credential_reader(
        &store,
        &provider,
        "timeout-thread",
        "mock-model",
        "system",
        cancel.clone(),
        None,
        None,
        397,
        Some(reader),
    )
    .await
    .unwrap();
    assert!(cancel.is_cancelled());
    assert!(provider.requests().is_empty());
    assert!(
        matches!(events.last(), Some(ConversationEvent::ContextCompactionStatus { record })
        if record.status == crate::types::ContextCompactionStatus::Failed
            && record.failure == Some(crate::types::ContextCompactionFailureCode::Unavailable))
    );
    let reopened = Store::open_read_only(store.database_path().unwrap()).unwrap();
    let projection =
        read_context_projection(&reopened, "timeout-thread", "mock-model", "system").unwrap();
    let status = projection.last_status.unwrap();
    assert_eq!(status.status, crate::types::ContextCompactionStatus::Failed);
    assert_eq!(
        status.failure,
        Some(crate::types::ContextCompactionFailureCode::Unavailable)
    );
    assert!(
        vega_store::context_compaction::latest_checkpoint(
            store.conn(),
            "timeout-thread",
            "mock-model"
        )
        .unwrap()
        .is_none()
    );
}
