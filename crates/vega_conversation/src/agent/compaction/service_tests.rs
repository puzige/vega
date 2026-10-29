use super::*;
use crate::types::{
    ContextCompactionFailureCode, ContextCompactionStatus, ContextCompactionUsageState,
    ConversationEvent, PartialCompactionDirection, PartialCompactionSelection,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use vega_runtime::model_compat::{
    ModelApi, ModelIdentity, ModelRoute, ResolvedModel, lookup_model_definition, resolve_model,
};
use vega_store::{context_compaction as durable, messages, threads};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    directory: PathBuf,
    store: Store,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

impl Fixture {
    fn new() -> TestResult<Self> {
        let directory = std::env::temp_dir().join(format!(
            "vega-accounted-compaction-{}",
            ulid::Ulid::generate()
        ));
        std::fs::create_dir(&directory)?;
        let store = Store::open(directory.join("test.sqlite"))?;
        store.migrate()?;
        let fixture = Self { directory, store };
        threads::create_standalone(
            fixture.store.conn(),
            threads::NewThread {
                id: "thread",
                project_id: "",
                title: "Synthetic accounted compaction",
                mode: "execute",
                permission_mode: "read_only",
                permission_revision: 0,
                full_access_grant: None,
                reasoning_preferences: None,
                model: "gpt-4.1",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )?;
        for (seq, id, role, text) in [
            (1, "earlier-task", "user", "Earlier task"),
            (2, "earlier-answer", "assistant", "Earlier answer"),
            (3, "selected", "user", "Selected task"),
            (4, "selected-answer", "assistant", "Selected answer"),
            (
                5,
                "latest-task",
                "user",
                "Continue the current implementation",
            ),
        ] {
            messages::insert(
                fixture.store.conn(),
                &messages::MessageRow {
                    id: id.into(),
                    thread_id: "thread".into(),
                    seq,
                    role: role.into(),
                    kind: "text".into(),
                    content: format!("{text}: {}", "synthetic history ".repeat(200)),
                    status: "done".into(),
                    created_at: seq,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )?;
        }
        durable::save_model_policy(
            fixture.store.conn(),
            &durable::ModelContextPolicy {
                provider: "openai".into(),
                model: "gpt-4.1".into(),
                input_limit: Some(168_000),
                output_reserve: Some(32_000),
                automatic_compaction: false,
                updated_at: 1,
            },
        )?;
        Ok(fixture)
    }

    fn selection(
        &self,
        direction: PartialCompactionDirection,
    ) -> TestResult<PartialCompactionSelection> {
        Ok(PartialCompactionSelection {
            message_id: "selected".into(),
            source_fingerprint: durable::load_source(self.store.conn(), "thread")?.fingerprint,
            direction,
        })
    }

    fn reopen(&self) -> TestResult<Store> {
        Ok(Store::open(self.directory.join("test.sqlite"))?)
    }
}

struct AccountedProvider {
    snapshot: ResolvedModel,
    calls: AtomicUsize,
    report_usage: bool,
    cancel_in_provider: bool,
}

impl AccountedProvider {
    fn new(report_usage: bool, cancel_in_provider: bool) -> TestResult<Self> {
        let definition = lookup_model_definition(
            &ModelRoute::new(ModelApi::Responses, "https://api.openai.com/v1")?,
            &ModelIdentity {
                provider: "openai".into(),
                model: "gpt-4.1".into(),
            },
        )?;
        Ok(Self {
            snapshot: resolve_model(&definition, None)?,
            calls: AtomicUsize::new(0),
            report_usage,
            cancel_in_provider,
        })
    }
}

impl Provider for AccountedProvider {
    fn model_snapshot(&self) -> Option<ResolvedModel> {
        Some(self.snapshot.clone())
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(request.tools.is_empty());
        assert_eq!(request.model, "gpt-4.1");
        let report_usage = self.report_usage;
        let cancel_in_provider = self.cancel_in_provider;
        Box::pin(async move {
            if cancel_in_provider {
                cancel.cancel();
                return Err(VegaError::Cancelled);
            }
            let mut events = vec![Ok(ProviderEvent::TextDelta(
                "The implementation remains in progress under the original constraints.".into(),
            ))];
            if report_usage {
                events.push(Ok(ProviderEvent::Usage {
                    input: 123,
                    output: 17,
                    cache_read: Some(11),
                    cache_write: None,
                }));
            }
            events.push(Ok(ProviderEvent::Done {
                stop_reason: StopReason::End,
            }));
            let stream: vega_runtime::EventStream = Box::pin(futures::stream::iter(events));
            Ok(stream)
        })
    }
}

async fn run_service(
    fixture: &Fixture,
    provider: &AccountedProvider,
    selection: PartialCompactionSelection,
) -> TestResult<Vec<ConversationEvent>> {
    Ok(compact_thread_selection_accounted_with_credential_reader(
        &fixture.store,
        provider,
        "thread",
        "gpt-4.1",
        "Synthetic system instruction",
        CancellationToken::new(),
        None,
        None,
        41,
        Some(Arc::new(|| Ok(Vec::new()))),
        Some(selection),
        String::new(),
    )
    .await?)
}

fn assert_lifecycle(
    events: &[ConversationEvent],
    terminal: ContextCompactionStatus,
    usage: ContextCompactionUsageState,
) {
    let records = events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ContextCompactionStatus { record } => Some(record),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].status, ContextCompactionStatus::Compacting);
    assert_eq!(records[0].usage, ContextCompactionUsageState::Pending);
    assert_eq!(records[1].status, terminal);
    assert_eq!(records[1].usage, usage);
    assert!(records.iter().all(|record| record.generation == 41));
}

#[tokio::test]
async fn accounted_selection_directions_persist_usage_and_reload_without_rewriting_audit()
-> TestResult {
    for direction in [
        PartialCompactionDirection::UpTo,
        PartialCompactionDirection::From,
    ] {
        for report_usage in [false, true] {
            let fixture = Fixture::new()?;
            let provider = AccountedProvider::new(report_usage, false)?;
            let source = durable::load_source(fixture.store.conn(), "thread")?;
            let events = run_service(&fixture, &provider, fixture.selection(direction)?).await?;
            assert_lifecycle(
                &events,
                ContextCompactionStatus::Succeeded,
                if report_usage {
                    ContextCompactionUsageState::Known { priced: false }
                } else {
                    ContextCompactionUsageState::Unknown
                },
            );
            assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
            let usages = events
                .iter()
                .filter_map(|event| match event {
                    ConversationEvent::ContextCompactionUsageUpdated {
                        usage,
                        cost,
                        pricing,
                    } => {
                        assert!(cost.is_none());
                        assert!(pricing.is_none());
                        Some(usage)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(usages.len(), usize::from(report_usage));
            if report_usage {
                assert_eq!(usages[0].input, 123);
                assert_eq!(usages[0].output, 17);
                assert_eq!(usages[0].cache_read, Some(11));
            }
            let checkpoint = durable::latest_checkpoint(fixture.store.conn(), "thread", "gpt-4.1")?
                .ok_or("checkpoint missing")?;
            assert!(checkpoint.projection_plan.is_some());
            let reopened = fixture.reopen()?;
            let reloaded_source = durable::load_source(reopened.conn(), "thread")?;
            assert_eq!(source, reloaded_source);
            let reloaded_checkpoint =
                durable::latest_checkpoint(reopened.conn(), "thread", "gpt-4.1")?
                    .ok_or("reloaded checkpoint missing")?;
            assert!(checkpoint == reloaded_checkpoint);
            let history = super::super::pipeline::history_from_context_source_with_checkpoint(
                &reloaded_source,
                Some(&reloaded_checkpoint),
                "__none__",
            )?;
            let summary = history
                .iter()
                .position(|message| message.content.contains("Historical context summary"))
                .ok_or("summary missing")?;
            let retained: &[&str] = match direction {
                PartialCompactionDirection::UpTo => {
                    assert_eq!(summary, 0);
                    &["selected", "selected-answer", "latest-task"]
                }
                PartialCompactionDirection::From => {
                    assert_eq!(summary, history.len() - 1);
                    &["earlier-task", "earlier-answer"]
                }
            };
            for row in &source.messages {
                assert_eq!(
                    history.iter().any(|message| message.content == row.content),
                    retained.contains(&row.id.as_str())
                );
            }
            let status = durable::latest_status(reopened.conn(), "thread", "gpt-4.1")?
                .ok_or("durable status missing")?;
            assert_eq!(status.phase, "succeeded");
            assert_eq!(status.generation, 41);
            assert_eq!(
                status.usage_state,
                if report_usage {
                    "known_unpriced"
                } else {
                    "unknown"
                }
            );
            assert!(status.failure.is_none());
        }
    }
    Ok(())
}

#[tokio::test]
async fn stale_selection_fingerprint_fails_without_provider_or_checkpoint() -> TestResult {
    let fixture = Fixture::new()?;
    let provider = AccountedProvider::new(false, false)?;
    let selection = fixture.selection(PartialCompactionDirection::From)?;
    fixture.store.conn().execute(
        "UPDATE messages SET content='Edited selected task' WHERE id='selected'",
        [],
    )?;
    let source = durable::load_source(fixture.store.conn(), "thread")?;
    let events = run_service(&fixture, &provider, selection).await?;
    assert_lifecycle(
        &events,
        ContextCompactionStatus::Failed,
        ContextCompactionUsageState::Unknown,
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert!(events.iter().any(|event| matches!(event,
        ConversationEvent::ContextCompactionStatus { record }
        if record.failure == Some(ContextCompactionFailureCode::SourceChanged))));
    assert_failed_audit(&fixture, &source, "failed")?;
    Ok(())
}

#[tokio::test]
async fn cancellation_during_summary_is_terminal_cancelled_not_failed_or_succeeded() -> TestResult {
    let fixture = Fixture::new()?;
    let provider = AccountedProvider::new(false, true)?;
    let source = durable::load_source(fixture.store.conn(), "thread")?;
    let events = run_service(
        &fixture,
        &provider,
        fixture.selection(PartialCompactionDirection::UpTo)?,
    )
    .await?;
    assert_lifecycle(
        &events,
        ContextCompactionStatus::Cancelled,
        ContextCompactionUsageState::Unknown,
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(events.iter().any(|event| matches!(event,
        ConversationEvent::ContextCompactionStatus { record }
        if record.failure == Some(ContextCompactionFailureCode::Cancelled))));
    assert_failed_audit(&fixture, &source, "cancelled")?;
    Ok(())
}

fn assert_failed_audit(
    fixture: &Fixture,
    source: &durable::ContextSource,
    phase: &str,
) -> TestResult {
    let reopened = fixture.reopen()?;
    assert_eq!(&durable::load_source(reopened.conn(), "thread")?, source);
    assert!(durable::latest_checkpoint(reopened.conn(), "thread", "gpt-4.1")?.is_none());
    let status = durable::latest_status(reopened.conn(), "thread", "gpt-4.1")?
        .ok_or("terminal status missing")?;
    assert_eq!(status.phase, phase);
    assert_eq!(status.generation, 41);
    assert_eq!(status.usage_state, "unknown");
    Ok(())
}
