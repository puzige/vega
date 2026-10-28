use super::*;
use crate::agent::diagnostics::{DiagnosticsContext, DiagnosticsWriter};
use std::sync::atomic::{AtomicBool, Ordering};
use vega_store::run_diagnostics::{
    DiagnosticMetrics, DiagnosticPhase, DiagnosticState, export_run,
};

#[derive(Clone)]
struct DiagnosticProvider {
    mode: &'static str,
    reader_available: Arc<AtomicBool>,
}

impl Provider for DiagnosticProvider {
    fn compaction_provider_factory(&self) -> Option<vega_runtime::CompactionProviderFactory> {
        let provider = self.clone();
        Some(Box::new(move || Ok(Arc::new(provider))))
    }

    fn chat_stream_with_metadata(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
        let response = self.chat_stream(request, cancel);
        Box::pin(async move {
            Ok(vega_runtime::ProviderStream {
                events: response.await?,
                metadata: vega_runtime::ProviderResponseMetadata {
                    http_status: Some(200),
                    request_id: Some("synthetic-secret-abcdefghijklmno".into()),
                    retry_count: Some(2),
                },
            })
        })
    }

    fn chat_stream(
        &self,
        _request: ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let mode = self.mode;
        let reader = self.reader_available.clone();
        Box::pin(async move {
            let usage = Ok(ProviderEvent::Usage {
                input: 100,
                output: 10,
                cache_read: None,
                cache_write: None,
            });
            if mode == "cancel" {
                let stream: vega_runtime::EventStream = Box::pin(
                    futures::stream::iter([usage]).chain(futures::stream::poll_fn(move |_| {
                        cancel.cancel();
                        std::task::Poll::Ready(Some(Err(VegaError::Cancelled)))
                    })),
                );
                return Ok(stream);
            }
            let mut events = vec![usage];
            if mode == "failure" {
                events.push(Err(VegaError::ProviderDiagnostic {
                    kind: vega_runtime::ProviderFailureKind::Http,
                    protocol_reason: None,
                    status: Some(503),
                    message: "private provider body".into(),
                    retryable: false,
                    retry_count: Some(2),
                    request_id: Some("synthetic-secret-abcdefghijklmno".into()),
                }));
            } else {
                if mode == "reader" {
                    reader.store(false, Ordering::Release);
                }
                events.push(Ok(ProviderEvent::TextDelta(
                    "Continue the implementation.".into(),
                )));
                events.push(Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }));
            }
            let stream: vega_runtime::EventStream = Box::pin(futures::stream::iter(events));
            Ok(stream)
        })
    }
}

#[tokio::test]
async fn background_exit_persists_isolated_content_free_diagnostics_for_every_terminal_path()
-> TestResult {
    for mode in [
        "success",
        "failure",
        "cancel",
        "reader",
        "accounting",
        "telemetry",
    ] {
        let (_directory, path, store) = source_fixture()?;
        let request = input(&store)?;
        if mode == "accounting" {
            store.conn().execute_batch("CREATE TRIGGER reject_stage BEFORE INSERT ON context_precompute_stages BEGIN SELECT RAISE(FAIL, 'synthetic stage failure'); END;")?;
        } else if mode == "telemetry" {
            store.conn().execute_batch("CREATE TRIGGER reject_diagnostic BEFORE INSERT ON run_diagnostic_events BEGIN SELECT RAISE(FAIL, 'synthetic diagnostic failure'); END;")?;
        }
        let reader_available = Arc::new(AtomicBool::new(true));
        let provider = DiagnosticProvider {
            mode,
            reader_available: reader_available.clone(),
        };
        let foreground = Arc::new(DiagnosticsWriter::buffered());
        let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
            .with_diagnostics(DiagnosticsContext {
                writer: foreground.clone(),
                thread_id: "thread".into(),
                run_id: "foreground".into(),
                root_attempt_id: "foreground-attempt".into(),
            })
            .with_credential_reader(Some(Arc::new(move || {
                if reader_available.load(Ordering::Acquire) {
                    Ok(vec!["synthetic-secret-abcdefghijklmno".into()])
                } else {
                    Err(())
                }
            })));
        let mut worker = hook
            .prepare_precompute(request, CancellationToken::new())
            .await?
            .ok_or("prepared")?
            .launch()?;
        let generation = worker.generation().to_owned();
        let result = tokio::time::timeout(Duration::from_secs(10), worker.wait()).await?;
        assert_eq!(
            result.is_ok(),
            matches!(mode, "success" | "telemetry"),
            "{mode}: {result:?}"
        );
        foreground.flush_to(&store);
        let foreground_count: i64 = store.conn().query_row(
            "SELECT COUNT(*) FROM run_diagnostic_events WHERE run_id='foreground'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(foreground_count, 0);
        assert_eq!(count(&store, "context_checkpoints")?, 0);
        assert_eq!(
            count(&store, "context_precompute_ready")?,
            i64::from(matches!(mode, "success" | "telemetry"))
        );
        assert_eq!(
            count(&store, "token_usage")?,
            i64::from(mode != "accounting")
        );
        if mode == "telemetry" {
            assert_eq!(count(&store, "run_diagnostic_events")?, 0);
            continue;
        }
        let exported = export_run(store.conn(), "thread", &generation)?;
        assert!(!exported.contains("synthetic-secret"));
        assert!(!exported.contains("private provider body"));
        assert!(!exported.contains("Continue the implementation"));
        let export: serde_json::Value = serde_json::from_str(&exported)?;
        assert!(
            export["incomplete_attempt_ids"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{mode}"
        );
        let events = export["events"].as_array().unwrap();
        assert_eq!(events.len(), 4, "{mode}");
        let terminal = if mode == "success" {
            "succeeded"
        } else if mode == "cancel" {
            "cancelled"
        } else {
            "failed"
        };
        assert_eq!(
            events
                .iter()
                .filter(|event| event["state"] == terminal)
                .count(),
            2,
            "{mode}"
        );
        assert!(
            events
                .iter()
                .all(|event| event["metrics"]["request_id"].is_null())
        );
    }
    Ok(())
}

#[test]
fn background_diagnostics_fit_all_bounded_stages_and_root_receipts_without_overflow() -> TestResult
{
    let (_directory, _path, store) = source_fixture()?;
    let writer = Arc::new(DiagnosticsWriter::for_background_compaction(32));
    let started = std::time::Instant::now();
    writer.root_event(
        "thread",
        "bounded-run",
        "root",
        DiagnosticState::Started,
        (None, Default::default()),
        started,
    );
    let context = DiagnosticsContext {
        writer: writer.clone(),
        thread_id: "thread".into(),
        run_id: "bounded-run".into(),
        root_attempt_id: "root".into(),
    };
    for stage in 0..32 {
        let id = format!("stage-{stage}");
        for state in [DiagnosticState::Started, DiagnosticState::Succeeded] {
            context.stage_event(
                &id,
                DiagnosticPhase::ContextSummary,
                state,
                None,
                started,
                DiagnosticMetrics {
                    request_id: Some("synthetic-secret-abcdefghijklmno".into()),
                    ..Default::default()
                },
            );
        }
    }
    writer.root_event(
        "thread",
        "bounded-run",
        "root",
        DiagnosticState::Succeeded,
        (None, Default::default()),
        started,
    );
    writer.root_event(
        "thread",
        "bounded-run",
        "overflow",
        DiagnosticState::Started,
        (None, Default::default()),
        started,
    );
    writer.flush_background_compaction(&store)?;
    assert_eq!(count(&store, "run_diagnostic_events")?, 66);
    let exported = export_run(store.conn(), "thread", "bounded-run")?;
    assert!(!exported.contains("synthetic-secret"));
    assert!(!exported.contains("overflow"));
    let export: serde_json::Value = serde_json::from_str(&exported)?;
    assert!(
        export["incomplete_attempt_ids"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    Ok(())
}
