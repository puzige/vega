use super::*;

fn successful_retry_response(text: &str) -> Vec<u8> {
    sse_response(
        &[
            &serde_json::json!({"choices":[{"delta":{"content":text}}]}).to_string(),
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    )
}

#[tokio::test(start_paused = true)]
async fn issue350_setup_and_stream_failures_share_one_recovery_budget() {
    let server = mock_transport(scripted_responses(vec![
        status_response("503 Unavailable", &[], "private body"),
        sse_response(
            &[r#"{"choices":[{"delta":{"content":"失效片段"}}]}"#],
            false,
        ),
        status_response("429 Too Many Requests", &[("Retry-After", "120")], ""),
        status_response("503 Unavailable", &[], ""),
        sse_response(
            &[r#"{"choices":[{"delta":{"content":"另一片段"}}]}"#],
            false,
        ),
        successful_retry_response("恢复"),
    ]))
    .await;
    let provider = provider_for(&server, primary_policy());
    let started = tokio::time::Instant::now();
    let call = provider
        .chat_stream_recovering(request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(server.attempt_count(), 0);
    let events = collect_recovery(call.events, 24).await;
    let attempts = events
        .iter()
        .filter_map(|event| match event {
            Ok(ProviderEvent::RetryScheduled {
                attempt,
                max_retries,
            }) => {
                assert_eq!(*max_retries, 5);
                Some(*attempt)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(attempts, vec![1, 2, 3, 4, 5]);
    assert_eq!(server.attempt_count(), 6);
    assert_eq!(started.elapsed(), Duration::from_secs(57));
    assert!(matches!(
        events.last(),
        Some(Ok(ProviderEvent::Done {
            stop_reason: StopReason::End
        }))
    ));
    assert!(events.iter().all(Result::is_ok));
    let captured = server.captured();
    assert!(captured.windows(2).all(|pair| pair[0].body == pair[1].body));
}

#[tokio::test(start_paused = true)]
async fn issue350_recovery_exhaustion_and_cancellation_are_terminal() {
    let server = mock_transport(Arc::new(|_| {
        Ok(fixture_response(status_response(
            "408 Request Timeout",
            &[],
            "",
        )))
    }))
    .await;
    let provider = provider_for(&server, primary_policy());
    let call = provider
        .chat_stream_recovering(request(), CancellationToken::new())
        .await
        .unwrap();
    let events = collect_recovery(call.events, 8).await;
    assert_eq!(server.attempt_count(), 6);
    assert!(matches!(
        events.last(),
        Some(Err(VegaError::ProviderDiagnostic {
            retry_count: Some(5),
            retryable: false,
            ..
        }))
    ));

    let server = mock_transport(Arc::new(|_| {
        Ok(fixture_response(status_response(
            "503 Unavailable",
            &[],
            "",
        )))
    }))
    .await;
    let provider = provider_for(&server, primary_policy());
    let cancel = CancellationToken::new();
    let mut stream = provider
        .chat_stream_recovering(request(), cancel.clone())
        .await
        .unwrap()
        .events;
    assert!(matches!(
        stream.next().await,
        Some(Ok(ProviderEvent::RetryScheduled { attempt: 1, .. }))
    ));
    cancel.cancel();
    assert!(matches!(
        stream.next().await,
        Some(Err(VegaError::Cancelled))
    ));
    assert!(stream.next().await.is_none());
    assert_eq!(server.attempt_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn issue350_rejected_and_malformed_responses_never_retry() {
    for response in [
        status_response("400 Bad Request", &[], ""),
        status_response("401 Unauthorized", &[], ""),
        status_response("403 Forbidden", &[], ""),
        sse_response(&["invalid json"], false),
        sse_response(&[], true),
    ] {
        let server = mock_transport(scripted_responses(vec![response])).await;
        let provider = provider_for(&server, primary_policy());
        let call = provider
            .chat_stream_recovering(request(), CancellationToken::new())
            .await
            .unwrap();
        let events = collect_recovery(call.events, 8).await;
        assert_eq!(server.attempt_count(), 1);
        assert!(events.iter().any(Result::is_err));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Ok(ProviderEvent::RetryScheduled { .. })))
        );
    }
}

#[tokio::test(start_paused = true)]
async fn issue350_retry_guard_runs_before_next_http_attempt() {
    let server = mock_transport(scripted_responses(vec![
        status_response("503 Unavailable", &[], ""),
        successful_retry_response("unused"),
    ]))
    .await;
    let count = Arc::new(AtomicUsize::new(0));
    let attempts = count.clone();
    let provider = provider_for(&server, primary_policy()).with_pre_attempt_guard(move |_| {
        if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(())
        } else {
            Err(VegaError::CredentialExposureBlocked)
        }
    });
    let call = provider
        .chat_stream_recovering(request(), CancellationToken::new())
        .await
        .unwrap();
    let events = collect_recovery(call.events, 8).await;
    assert!(matches!(
        events.last(),
        Some(Err(VegaError::CredentialExposureBlocked))
    ));
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(server.attempt_count(), 1);
}

async fn collect_recovery(
    stream: EventStream,
    limit: usize,
) -> Vec<Result<ProviderEvent, VegaError>> {
    tokio::time::timeout(Duration::from_secs(1500), stream.take(limit).collect())
        .await
        .unwrap()
}

fn recovery_agent_request() -> crate::AgentRequest {
    crate::AgentRequest {
        model: MODEL.into(),
        system_prompt: "Inspect fixture".into(),
        history: vec![ChatMessage::new(ChatRole::User, "Read README.md")],
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: Default::default(),
        pricing_catalog: None,
        reasoning: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

#[tokio::test(start_paused = true)]
async fn issue350_transport_fault_and_response_eof_recover_without_replaying_tools() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("README.md"), "fixture").unwrap();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    let first = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"已检查。","tool_calls":[{"index":0,"id":"read-once","function":{"name":"read","arguments":"{\"path\":\"README.md\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        ],
        true,
    );
    let failed = sse_response(
        &[
            r#"{"choices":[{"delta":{"reasoning_content":"失效推理","content":"旧片段🦀"}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"incomplete","function":{"name":"read","arguments":"{\"path\":"}}]}}]}"#,
            usage_chunk(),
        ],
        false,
    );
    let succeeded = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"新答案。"},"finish_reason":"stop"}]}"#,
            usage_chunk(),
        ],
        true,
    );
    let server = mock_transport(scripted_responses(vec![first, failed, succeeded])).await;
    let provider = provider_for(&server, primary_policy());
    let outcome = crate::run_agent(
        &provider,
        &tools,
        recovery_agent_request(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed);
    assert_eq!(outcome.final_text, "已检查。新答案。");
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_eq!(outcome.tool_call_count, 1);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, crate::RuntimeEvent::ToolCallRunning { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, crate::RuntimeEvent::UsageUpdated { .. }))
            .count(),
        2
    );
    assert!(outcome.events.iter().any(|event| matches!(event, crate::RuntimeEvent::ResponseRetrying { text_start_bytes, attempt: 1, .. } if *text_start_bytes == "已检查。".len())));
    assert!(
        !outcome
            .messages
            .iter()
            .any(|message| message.content.contains("旧片段"))
    );
    assert_eq!(server.attempt_count(), 3);
    let requests = server.captured();
    assert_eq!(requests[1].body, requests[2].body);
    assert!(
        requests[2].body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool" && message["tool_call_id"] == "read-once")
    );
}

#[tokio::test(start_paused = true)]
async fn issue350_responses_eof_and_transport_fault_retry_but_invalid_streams_do_not() {
    let partial = sse_response(
        &[r#"{"type":"response.output_text.delta","item_id":"answer","delta":"discarded"}"#],
        false,
    );
    let complete = sse_response(
        &[
            r#"{"type":"response.completed","response":{"status":"completed","output":[{"id":"answer","type":"message","role":"assistant","content":[{"type":"output_text","text":"replacement"}]}]}}"#,
        ],
        false,
    );
    let server = mock_transport(scripted_responses(vec![partial, complete])).await;
    let provider = provider_for(&server, primary_policy()).with_responses_api(true);
    let directory = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    let outcome = crate::run_agent(
        &provider,
        &tools,
        recovery_agent_request(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.final_text, "replacement");
    assert!(!outcome.failed);
    assert_eq!(server.attempt_count(), 2);

    for response in [
        sse_response(&["invalid json"], false),
        sse_response(&[&"x".repeat(4 * 1024 * 1024)], false),
    ] {
        let server = mock_transport(scripted_responses(vec![response])).await;
        let provider = provider_for(&server, primary_policy()).with_responses_api(true);
        let events = collect_recovery(
            provider
                .chat_stream_recovering(request(), CancellationToken::new())
                .await
                .unwrap()
                .events,
            8,
        )
        .await;
        assert_eq!(server.attempt_count(), 1);
        assert!(events.last().unwrap().is_err());
    }

    let response: reqwest::Response = ::http::Response::builder()
        .body(reqwest::Body::wrap_stream(futures::stream::once(async {
            Err::<Vec<u8>, _>(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "fixture disconnect",
            ))
        })))
        .unwrap()
        .into();
    let error = Arc::new(Mutex::new(Some(response.bytes().await.unwrap_err())));
    let server = mock_transport(Arc::new(move |index| {
        if index == 0 {
            Err(error.lock().unwrap().take().unwrap())
        } else {
            Ok(fixture_response(successful_retry_response("connected")))
        }
    }))
    .await;
    let provider = provider_for(&server, primary_policy());
    let events = collect_recovery(
        provider
            .chat_stream_recovering(request(), CancellationToken::new())
            .await
            .unwrap()
            .events,
        8,
    )
    .await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Ok(ProviderEvent::RetryScheduled { attempt: 1, .. })))
    );
    assert_eq!(server.attempt_count(), 2);
}

#[tokio::test(start_paused = true)]
async fn issue350_silent_setup_and_stream_timeouts_enter_bounded_recovery() {
    let mut provider = OpenAiProvider::new("http://fixture.invalid", KEY).unwrap();
    provider.test_transport = Some(Arc::new(|_| Box::pin(futures::future::pending())));
    let cancel = CancellationToken::new();
    let started = tokio::time::Instant::now();
    let mut stream = provider
        .chat_stream_recovering(request(), cancel.clone())
        .await
        .unwrap()
        .events;
    assert!(matches!(
        stream.next().await,
        Some(Ok(ProviderEvent::RetryScheduled { attempt: 1, .. }))
    ));
    assert_eq!(started.elapsed(), Duration::from_secs(60));
    cancel.cancel();
    assert!(matches!(
        stream.next().await,
        Some(Err(VegaError::Cancelled))
    ));

    provider.test_transport = Some(Arc::new(|_| {
        Box::pin(async {
            let response: reqwest::Response = ::http::Response::builder()
                .status(200)
                .body(reqwest::Body::wrap_stream(futures::stream::pending::<
                    Result<Vec<u8>, std::io::Error>,
                >()))
                .unwrap()
                .into();
            Ok(response)
        })
    }));
    let started = tokio::time::Instant::now();
    let mut stream = provider
        .chat_stream_recovering(request(), CancellationToken::new())
        .await
        .unwrap()
        .events;
    assert!(matches!(
        stream.next().await,
        Some(Ok(ProviderEvent::ResponseMetadata(_)))
    ));
    assert!(matches!(
        stream.next().await,
        Some(Ok(ProviderEvent::RetryScheduled { attempt: 1, .. }))
    ));
    assert_eq!(started.elapsed(), Duration::from_secs(300));
}

#[tokio::test(start_paused = true)]
async fn issue350_retry_sink_failure_prevents_the_next_http_request() {
    let server = mock_transport(scripted_responses(vec![
        status_response("503 Unavailable", &[], ""),
        successful_retry_response("never"),
    ]))
    .await;
    let provider = provider_for(&server, primary_policy());
    let directory = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    let result = crate::run_agent_with_sink(
        &provider,
        &tools,
        recovery_agent_request(),
        CancellationToken::new(),
        |event| async move {
            if matches!(event, crate::RuntimeEvent::ResponseRetrying { .. }) {
                Err(VegaError::Io(std::io::Error::other(
                    "fixture persistence rejected",
                )))
            } else {
                Ok(())
            }
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(server.attempt_count(), 1);
}

fn primary_policy() -> RetryPolicy {
    RetryPolicy {
        max_retries: 5,
        ..RetryPolicy::default()
    }
}

#[test]
fn issue350_primary_retry_defaults_are_five_exponential_attempts() {
    let provider = OpenAiProvider::new("http://fixture.invalid", KEY).unwrap();
    assert_eq!(provider.retry.max_retries, 3);
    assert_eq!(provider.recovery_retry.max_retries, 5);
    assert_eq!(
        (0..5)
            .map(|attempt| provider.recovery_retry.backoff(attempt).as_secs())
            .collect::<Vec<_>>(),
        vec![1, 2, 4, 8, 16]
    );
}

#[tokio::test(start_paused = true)]
async fn issue350_body_transport_errors_recover_in_both_protocols() {
    for responses in [false, true] {
        let prefix = if responses {
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"answer\",\"delta\":\"discarded\"}\n\n"
        } else {
            "data: {\"choices\":[{\"delta\":{\"content\":\"discarded\"}}]}\n\n"
        };
        let server = mock_transport(Arc::new(move |index| {
            if index == 0 {
                let chunks = futures::stream::iter(vec![
                    Ok(prefix.as_bytes().to_vec()),
                    Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "fixture disconnect")),
                ]);
                Ok(::http::Response::builder().status(200).body(reqwest::Body::wrap_stream(chunks)).unwrap().into())
            } else if responses {
                Ok(fixture_response(sse_response(&[r#"{"type":"response.completed","response":{"status":"completed","output":[{"id":"answer","type":"message","role":"assistant","content":[{"type":"output_text","text":"recovered"}]}]}}"#], false)))
            } else {
                Ok(fixture_response(successful_retry_response("recovered")))
            }
        })).await;
        let provider = provider_for(&server, primary_policy()).with_responses_api(responses);
        let directory = tempfile::tempdir().unwrap();
        let tools = vega_tools::Tools::new(directory.path()).unwrap();
        let outcome = crate::run_agent(
            &provider,
            &tools,
            recovery_agent_request(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.final_text, "recovered");
        assert!(!outcome.failed);
        assert_eq!(server.attempt_count(), 2);
    }
}

#[tokio::test(start_paused = true)]
async fn issue350_exhaustion_keeps_last_partial_and_emits_one_failure() {
    let partial = sse_response(
        &[r#"{"choices":[{"delta":{"content":"末次片段🦀"}}]}"#],
        false,
    );
    let server = mock_transport(scripted_responses(vec![partial; 6])).await;
    let provider = provider_for(&server, primary_policy());
    let directory = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(directory.path()).unwrap();
    let outcome = crate::run_agent(
        &provider,
        &tools,
        recovery_agent_request(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(outcome.failed);
    assert!(!outcome.interrupted);
    assert_eq!(outcome.final_text, "末次片段🦀");
    assert_eq!(server.attempt_count(), 6);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, crate::RuntimeEvent::ResponseRetrying { .. }))
            .count(),
        5
    );
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, crate::RuntimeEvent::Error(_)))
            .count(),
        1
    );
    assert!(!outcome.events.iter().any(|event| matches!(
        event,
        crate::RuntimeEvent::AssistantResponseCompleted { .. }
    )));
}

#[tokio::test(start_paused = true)]
async fn issue350_invalid_request_does_not_retry_and_active_request_can_cancel() {
    let server = mock_transport(Arc::new(|_| Err(simulated_transport_error()))).await;
    let provider = provider_for(&server, primary_policy());
    let events = collect_recovery(
        provider
            .chat_stream_recovering(request(), CancellationToken::new())
            .await
            .unwrap()
            .events,
        8,
    )
    .await;
    assert_eq!(events.len(), 1);
    assert!(events[0].is_err());
    assert_eq!(server.attempt_count(), 1);

    let mut provider = OpenAiProvider::new("http://fixture.invalid", KEY).unwrap();
    provider.test_transport = Some(Arc::new(|_| Box::pin(futures::future::pending())));
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let stopping = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        stop.cancel();
    });
    let started = tokio::time::Instant::now();
    let mut stream = provider
        .chat_stream_recovering(request(), cancel)
        .await
        .unwrap()
        .events;
    assert!(matches!(
        stream.next().await,
        Some(Err(VegaError::Cancelled))
    ));
    assert_eq!(started.elapsed(), Duration::from_secs(2));
    assert!(stream.next().await.is_none());
    stopping.await.unwrap();
}
