use super::*;

mod arming_tests;
mod consume_tests;
mod registry_tests;
use std::sync::Mutex;
use vega_store::context_compaction::{latest_checkpoint, load_source};
use vega_store::context_precompute::{accounting, ready};
use vega_store::context_projection::ProjectionStrategy;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct BackgroundState {
    requests: Mutex<Vec<ChatRequest>>,
    mode: &'static str,
    pause: bool,
    started: tokio::sync::Notify,
    proceed: tokio::sync::Notify,
}

#[derive(Clone)]
struct BackgroundProvider(Arc<BackgroundState>);

impl BackgroundProvider {
    fn new(mode: &'static str, pause: bool) -> Self {
        Self(Arc::new(BackgroundState {
            requests: Mutex::new(Vec::new()),
            mode,
            pause,
            started: tokio::sync::Notify::new(),
            proceed: tokio::sync::Notify::new(),
        }))
    }
}

impl Provider for BackgroundProvider {
    fn compaction_provider_factory(&self) -> Option<vega_runtime::CompactionProviderFactory> {
        let provider = self.clone();
        Some(Box::new(move || Ok(Arc::new(provider))))
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        assert!(request.tools.is_empty());
        self.0.requests.lock().unwrap().push(request);
        let state = self.0.clone();
        Box::pin(async move {
            state.started.notify_one();
            if state.pause {
                state.proceed.notified().await;
            }
            let usage = Ok(ProviderEvent::Usage {
                input: 100,
                output: 10,
                cache_read: None,
                cache_write: Some(0),
            });
            if state.mode == "cancel" {
                let stream: vega_runtime::EventStream = Box::pin(
                    futures::stream::iter([usage]).chain(futures::stream::poll_fn(move |_| {
                        cancel.cancel();
                        std::task::Poll::Ready(Some(Err(VegaError::Cancelled)))
                    })),
                );
                return Ok(stream);
            }
            let mut events = Vec::new();
            if state.mode != "unknown" {
                events.push(usage);
            }
            match state.mode {
                "overflow" => events.push(Err(VegaError::Provider {
                    status: Some(400),
                    message: "{\"error\":{\"code\":\"prompt_too_long\"}}".into(),
                    retryable: false,
                })),
                "failure" => events.push(Err(VegaError::Provider {
                    status: Some(503),
                    message: "Synthetic unavailable".into(),
                    retryable: false,
                })),
                _ => {
                    events.push(Ok(ProviderEvent::TextDelta(
                        "Continue the implementation.".into(),
                    )));
                    events.push(Ok(ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    }));
                }
            }
            let stream: vega_runtime::EventStream = Box::pin(futures::stream::iter(events));
            Ok(stream)
        })
    }
}

fn input(store: &Store) -> Result<ContextCompactionRequest, Box<dyn std::error::Error>> {
    store
        .conn()
        .execute("UPDATE messages SET status='done' WHERE id='owner'", [])?;
    store.conn().execute(
        "UPDATE tool_calls SET seq=1,output_text=?1 WHERE id='call-1'",
        ["older tool data ".repeat(2000)],
    )?;
    store
        .conn()
        .execute("UPDATE tool_calls SET seq=2 WHERE id='call-2'", [])?;
    let source = load_source(store.conn(), "thread")?;
    let messages = crate::agent::pipeline::history_from_context_source(&source, "__none__", None)?;
    let budget = ContextBudget::new(200_000, 32_000, true)?;
    Ok(ContextCompactionRequest {
        trigger: ContextCompactionTrigger::Threshold,
        system_prompt: "system".into(),
        estimate: estimate_chat_context("system", &messages, &[])?,
        messages,
        tools: Vec::new(),
        budget,
        target_tokens: budget.target_tokens()?,
        source_version: source.source_version,
        source_fingerprint: Some(source.fingerprint),
        require_source_fence: true,
        source_owner_id: None,
    })
}

fn count(store: &Store, table: &str) -> Result<i64, Box<dyn std::error::Error>> {
    Ok(store
        .conn()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?)
}

fn append(store: &Store) -> TestResult {
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "appended-task".into(),
            thread_id: "thread".into(),
            seq: 3,
            role: "user".into(),
            kind: "text".into(),
            content: "Keep this later instruction.".into(),
            status: "done".into(),
            created_at: 3,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )?;
    Ok(())
}

#[tokio::test]
async fn prepared_background_summary_releases_admission_and_recovers_safe_appends() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let provider = BackgroundProvider::new("success", true);
    let hook =
        ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None);
    let prepared = hook
        .prepare_precompute(request, CancellationToken::new())
        .await?
        .ok_or("not prepared")?;
    assert!(crate::thread_access::acquire(&store, "thread").is_ok());
    let generation = prepared.generation().to_owned();
    let mut worker = prepared.launch()?;
    assert_eq!(worker.generation(), generation);
    tokio::time::timeout(Duration::from_secs(10), provider.0.started.notified()).await?;
    assert!(crate::thread_access::acquire(&store, "thread").is_ok());
    append(&store)?;
    assert!(latest_checkpoint(store.conn(), "thread", "model")?.is_none());
    provider.0.proceed.notify_one();
    tokio::time::timeout(Duration::from_secs(10), worker.wait()).await??;
    let candidate = ready::recover(store.conn(), "thread", now_ms())?.ok_or("ready missing")?;
    assert_eq!(candidate.generation, generation);
    assert_eq!(
        candidate
            .checkpoint
            .projection_plan
            .as_ref()
            .ok_or("plan missing")?
            .strategy,
        ProjectionStrategy::Precompute
    );
    assert!(
        candidate
            .checkpoint
            .projection_plan
            .as_ref()
            .unwrap()
            .omitted
            .is_empty()
    );
    let transaction = store.conn().unchecked_transaction()?;
    let current = candidate.validate_current(&transaction, now_ms())?;
    assert!(current.messages.iter().any(|row| row.id == "appended-task"));
    transaction.commit()?;
    assert_eq!(count(&store, "context_checkpoints")?, 0);
    assert_eq!(count(&store, "token_usage")?, 1);
    assert_eq!(count(&store, "context_precompute_stages")?, 1);
    drop(store);
    let reopened = Store::open(path)?;
    assert_eq!(
        ready::recover(reopened.conn(), "thread", now_ms())?,
        Some(candidate)
    );
    assert_eq!(count(&reopened, "token_usage")?, 1);
    assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
    Ok(())
}

#[tokio::test]
async fn background_failure_cancel_and_source_edit_settle_usage_without_installing() -> TestResult {
    for mode in ["failure", "cancel", "edit", "model"] {
        let (_directory, path, store) = source_fixture()?;
        let request = input(&store)?;
        let provider = BackgroundProvider::new(mode, false);
        let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
        let prepared = hook
            .prepare_precompute(request, CancellationToken::new())
            .await?
            .ok_or("not prepared")?;
        let generation = prepared.generation().to_owned();
        if mode == "edit" {
            store
                .conn()
                .execute("UPDATE messages SET content='edited' WHERE id='task'", [])?;
        }
        if mode == "model" {
            store.conn().execute(
                "UPDATE threads SET pending_model='other' WHERE id='thread'",
                [],
            )?;
        }
        let result = prepared.generate(CancellationToken::new()).await;
        assert!(result.is_err(), "{mode}");
        assert_eq!(count(&store, "context_checkpoints")?, 0);
        assert_eq!(count(&store, "context_precompute_ready")?, 0);
        let billed = if mode == "model" { 0 } else { 1 };
        assert_eq!(count(&store, "token_usage")?, billed, "{mode}");
        if mode != "model" {
            let (stages, complete, retired): (u32, bool, bool) = store.conn().query_row(
                "SELECT stage_count,usage_complete,retired FROM context_precompute_accounting WHERE generation=?1",
                [&generation], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            assert_eq!(stages, 1);
            assert!(complete && retired);
            accounting::finish(store.conn(), &generation, "thread", "model", 1, true)?;
            assert_eq!(count(&store, "token_usage")?, 1);
        }
    }
    Ok(())
}

#[tokio::test]
async fn background_unknown_usage_remains_unknown_and_prestart_cancel_has_no_bill() -> TestResult {
    for cancelled in [false, true] {
        let (_directory, path, store) = source_fixture()?;
        let request = input(&store)?;
        let provider = BackgroundProvider::new("unknown", false);
        let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
        let prepared = hook
            .prepare_precompute(request, CancellationToken::new())
            .await?
            .ok_or("not prepared")?;
        let cancel = CancellationToken::new();
        if cancelled {
            cancel.cancel();
        }
        let result = prepared.generate(cancel).await;
        assert_eq!(result.is_err(), cancelled);
        assert_eq!(count(&store, "token_usage")?, 0);
        assert_eq!(
            count(&store, "context_precompute_stages")?,
            i64::from(!cancelled)
        );
        if !cancelled {
            let candidate =
                ready::recover(store.conn(), "thread", now_ms())?.ok_or("ready missing")?;
            assert!(!candidate.usage_complete);
            assert_eq!(candidate.stage_count, 1);
        }
    }
    Ok(())
}

#[tokio::test]
async fn precompute_overflow_never_falls_back_to_new_omission() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let provider = BackgroundProvider::new("overflow", false);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
    let prepared = hook
        .prepare_precompute(request, CancellationToken::new())
        .await?
        .ok_or("not prepared")?;
    assert!(prepared.generate(CancellationToken::new()).await.is_err());
    let requests = provider.0.requests.lock().unwrap();
    assert!(!requests.is_empty() && requests.len() <= SUMMARY_MAX_STAGES);
    for request in requests.iter() {
        let history = request
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(history.contains("Continue implementation; no local tests."));
        assert!(!history.contains("result-2"));
    }
    assert_eq!(count(&store, "context_precompute_ready")?, 0);
    assert_eq!(count(&store, "context_checkpoints")?, 0);
    assert_eq!(count(&store, "token_usage")?, requests.len() as i64);
    Ok(())
}

#[tokio::test]
async fn waiting_cancel_does_not_stop_background_but_worker_cancel_does() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let provider = BackgroundProvider::new("success", true);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
    let prepared = hook
        .prepare_precompute(request, CancellationToken::new())
        .await?
        .ok_or("not prepared")?;
    let mut worker = prepared.launch()?;
    tokio::time::timeout(Duration::from_secs(10), provider.0.started.notified()).await?;
    assert!(
        tokio::time::timeout(Duration::from_millis(10), worker.wait())
            .await
            .is_err()
    );
    worker.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), worker.wait())
            .await?
            .is_err()
    );
    assert_eq!(count(&store, "context_precompute_ready")?, 0);
    assert_eq!(count(&store, "context_precompute_stages")?, 1);
    assert_eq!(count(&store, "token_usage")?, 0);
    Ok(())
}

#[tokio::test]
async fn background_factory_preserves_owner_credential_reader_and_exceptions() -> TestResult {
    let provider = BackgroundProvider::new("success", false);
    let available = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let reads = available.clone();
    let wrapper = crate::agent::OwnerCredentialProvider::new(
        Arc::new(provider.clone()),
        Arc::new(move || {
            if reads.load(std::sync::atomic::Ordering::Acquire) {
                Ok(vec!["synthetic-secret-abcdefghijklmno".into()])
            } else {
                Err(())
            }
        }),
    )
    .with_credential_exceptions(Arc::new(|_, _| true));
    let factory = wrapper
        .compaction_provider_factory()
        .ok_or("factory missing")?;
    drop(wrapper);
    let background = factory()?;
    let formatted = format!("{}{}", "sk-", "a".repeat(40));
    assert!(
        vega_runtime::inspect_credential_text(&formatted, &[])
            .iter()
            .any(|finding| { finding.confidence == vega_runtime::CredentialConfidence::High })
    );
    let request = ChatRequest {
        model: "model".into(),
        messages: vec![ChatMessage::new(
            ChatRole::User,
            format!("synthetic-secret-abcdefghijklmno {formatted}"),
        )],
        ..Default::default()
    };
    let submission = vega_runtime::ProviderRequestSubmission::default();
    let stream = background
        .chat_stream_with_submission(
            request.clone(),
            CancellationToken::new(),
            submission.clone(),
        )
        .await?;
    drop(stream);
    assert!(submission.may_have_been_submitted());
    assert_eq!(
        provider.0.requests.lock().unwrap()[0].messages[0].content,
        format!(
            "{} {formatted}",
            vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER
        ),
    );
    let without_exception = crate::agent::OwnerCredentialProvider::new(
        Arc::new(provider.clone()),
        Arc::new(|| Ok(vec!["synthetic-secret-abcdefghijklmno".into()])),
    );
    drop(
        without_exception
            .chat_stream(request.clone(), CancellationToken::new())
            .await?,
    );
    assert_eq!(
        provider.0.requests.lock().unwrap()[1].messages[0].content,
        format!(
            "{0} {0}",
            vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER
        ),
    );
    available.store(false, std::sync::atomic::Ordering::Release);
    let rejected = vega_runtime::ProviderRequestSubmission::default();
    assert!(matches!(
        background
            .chat_stream_with_submission(request, CancellationToken::new(), rejected.clone())
            .await,
        Err(VegaError::CredentialCheckUnavailable)
    ));
    assert!(!rejected.may_have_been_submitted());
    assert_eq!(provider.0.requests.lock().unwrap().len(), 2);
    let unsupported = crate::agent::OwnerCredentialProvider::new(
        Arc::new(SummaryProvider),
        Arc::new(|| Ok(Vec::new())),
    );
    assert!(unsupported.compaction_provider_factory().is_none());
    Ok(())
}

#[tokio::test]
async fn background_preflight_rejection_and_cancellation_do_not_record_unsent_stages() -> TestResult
{
    use std::sync::atomic::{AtomicUsize, Ordering};

    for responses in [false, true] {
        for cancel_during_guard in [false, true] {
            let (_directory, path, store) = source_fixture()?;
            let request = input(&store)?;
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            listener.set_nonblocking(true)?;
            let cancel = CancellationToken::new();
            let guard_cancel = cancel.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let guard_calls = calls.clone();
            let provider = vega_runtime::OpenAiProvider::new(
                format!("http://{}/v1", listener.local_addr()?),
                "synthetic-not-a-secret",
            )?
            .with_responses_api(responses)
            .with_pre_attempt_guard(move |_| {
                guard_calls.fetch_add(1, Ordering::Relaxed);
                if cancel_during_guard {
                    guard_cancel.cancel();
                    Ok(())
                } else {
                    Err(VegaError::CredentialCheckUnavailable)
                }
            });
            let provider = crate::agent::OwnerCredentialProvider::new(
                Arc::new(provider),
                Arc::new(|| Ok(Vec::new())),
            );
            let hook =
                ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
            let prepared = hook
                .prepare_precompute(request, CancellationToken::new())
                .await?
                .ok_or("not prepared")?;
            let generation = prepared.generation().to_owned();
            let result =
                tokio::time::timeout(Duration::from_secs(10), prepared.generate(cancel)).await?;
            if cancel_during_guard {
                assert!(matches!(result, Err(VegaError::Cancelled)));
            } else {
                assert!(matches!(result, Err(VegaError::CredentialCheckUnavailable)));
            }
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            assert!(
                matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
            );
            assert_eq!(count(&store, "context_precompute_stages")?, 0);
            assert_eq!(count(&store, "token_usage")?, 0);
            assert_eq!(count(&store, "context_precompute_ready")?, 0);
            assert_eq!(count(&store, "context_checkpoints")?, 0);
            let settled: (u32, bool, bool) = store.conn().query_row(
                "SELECT stage_count,usage_complete,retired FROM context_precompute_accounting WHERE generation=?1",
                [&generation], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            assert_eq!(settled, (0, true, true));
        }
    }
    Ok(())
}

async fn receive_summary_request(stream: &mut tokio::net::TcpStream) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;

    let mut request = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        let read = stream.read(&mut buffer).await?;
        if read == 0 || request.len() + read > 2_000_000 {
            return Err(std::io::Error::other(
                "incomplete or oversized fixture request",
            ));
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let headers =
                std::str::from_utf8(&request[..header_end]).map_err(std::io::Error::other)?;
            let length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .ok_or_else(|| std::io::Error::other("fixture requires content length"))?
                .1
                .trim()
                .parse::<usize>()
                .map_err(std::io::Error::other)?;
            if request.len() >= header_end + 4 + length {
                return Ok(request);
            }
        }
    }
}

#[tokio::test]
async fn background_retry_guard_failure_retains_unknown_for_submitted_request() -> TestResult {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::AsyncWriteExt;

    for responses in [false, true] {
        for cancel_retry in [false, true] {
            let (_directory, path, store) = source_fixture()?;
            let request = input(&store)?;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let base_url = format!("http://{}/v1", listener.local_addr()?);
            let server = tokio::spawn(async move {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(10), listener.accept()).await??;
                let request = tokio::time::timeout(
                    Duration::from_secs(10),
                    receive_summary_request(&mut stream),
                )
                .await??;
                stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
                stream.shutdown().await?;
                Ok::<_, std::io::Error>((listener, request))
            });
            let cancel = CancellationToken::new();
            let guard_cancel = cancel.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let guard_calls = calls.clone();
            let provider = vega_runtime::OpenAiProvider::new(base_url, "synthetic-not-a-secret")?
                .with_responses_api(responses)
                .with_retry_policy(vega_runtime::RetryPolicy {
                    max_retries: 1,
                    base_delay: Duration::ZERO,
                    max_delay: Duration::ZERO,
                })
                .with_pre_attempt_guard(move |_| {
                    if guard_calls.fetch_add(1, Ordering::Relaxed) == 0 {
                        Ok(())
                    } else if cancel_retry {
                        guard_cancel.cancel();
                        Ok(())
                    } else {
                        Err(VegaError::CredentialCheckUnavailable)
                    }
                });
            let provider = crate::agent::OwnerCredentialProvider::new(
                Arc::new(provider),
                Arc::new(|| Ok(Vec::new())),
            );
            let hook =
                ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
            let prepared = hook
                .prepare_precompute(request, CancellationToken::new())
                .await?
                .ok_or("not prepared")?;
            let generation = prepared.generation().to_owned();
            let result =
                tokio::time::timeout(Duration::from_secs(10), prepared.generate(cancel)).await?;
            if cancel_retry {
                assert!(matches!(result, Err(VegaError::Cancelled)));
            } else {
                assert!(matches!(result, Err(VegaError::CredentialCheckUnavailable)));
            }
            let (listener, received) =
                tokio::time::timeout(Duration::from_secs(10), server).await???;
            let endpoint = if responses {
                "POST /v1/responses "
            } else {
                "POST /v1/chat/completions "
            };
            assert!(received.starts_with(endpoint.as_bytes()));
            assert_eq!(calls.load(Ordering::Relaxed), 2);
            assert!(
                tokio::time::timeout(Duration::from_millis(20), listener.accept())
                    .await
                    .is_err()
            );
            assert_eq!(count(&store, "context_precompute_stages")?, 1);
            assert_eq!(count(&store, "token_usage")?, 0);
            assert_eq!(count(&store, "context_precompute_ready")?, 0);
            assert_eq!(count(&store, "context_checkpoints")?, 0);
            let settled: (u32, bool, bool) = store.conn().query_row(
                "SELECT stage_count,usage_complete,retired FROM context_precompute_accounting WHERE generation=?1",
                [&generation], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            assert_eq!(settled, (1, false, true));
        }
    }
    Ok(())
}

#[tokio::test]
async fn provider_without_owned_factory_and_pending_model_do_not_start_background() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let request = input(&store)?;
    let hook = ConversationCompactionHook::new(
        &SummaryProvider,
        path.clone(),
        "thread",
        "model",
        None,
        None,
    );
    assert!(
        hook.prepare_precompute(request.clone(), CancellationToken::new())
            .await?
            .is_none()
    );
    let provider = BackgroundProvider::new("success", false);
    store.conn().execute(
        "UPDATE threads SET pending_model='other' WHERE id='thread'",
        [],
    )?;
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
    assert!(
        hook.prepare_precompute(request, CancellationToken::new())
            .await
            .is_err()
    );
    assert!(provider.0.requests.lock().unwrap().is_empty());
    assert_eq!(count(&store, "context_precompute_accounting")?, 0);
    Ok(())
}
