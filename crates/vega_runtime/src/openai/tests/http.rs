use super::*;

#[test]
fn response_request_id_accepts_only_allowlisted_valid_values() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-request-id",
        reqwest::header::HeaderValue::from_static("req_abc-123.X:y"),
    );
    assert_eq!(
        request_id_from_headers(&headers).as_deref(),
        Some("req_abc-123.X:y")
    );

    let mut unsupported = reqwest::header::HeaderMap::new();
    unsupported.insert(
        "x-secret-token",
        reqwest::header::HeaderValue::from_static("not-allowed"),
    );
    assert_eq!(request_id_from_headers(&unsupported), None);

    let mut invalid = reqwest::header::HeaderMap::new();
    invalid.insert(
        "request-id",
        reqwest::header::HeaderValue::from_static("Bearer secret"),
    );
    assert_eq!(request_id_from_headers(&invalid), None);
    invalid.remove("request-id");
    invalid.insert(
        "openai-request-id",
        reqwest::header::HeaderValue::from_bytes(&[b'x'; 129]).unwrap(),
    );
    assert_eq!(request_id_from_headers(&invalid), None);
}

#[tokio::test(start_paused = true)]
async fn typed_chat_call_returns_response_metadata_and_retry_count() {
    let mut response = http_head(
        "200 OK",
        &[
            ("Content-Type", "text/event-stream"),
            ("openai-request-id", "request-171"),
        ],
    );
    response.extend_from_slice(
        b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    );
    let server = mock_transport(scripted_responses(vec![response])).await;
    let provider = provider_for(&server, fast_policy(1));
    let call = provider
        .chat_stream_with_metadata(request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(call.metadata.http_status, Some(200));
    assert_eq!(call.metadata.request_id.as_deref(), Some("request-171"));
    assert_eq!(call.metadata.retry_count, Some(0));
}

#[tokio::test(start_paused = true)]
async fn happy_path_sends_openai_wire_format_and_streams_events() {
    let body = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"},"finish_reason":null}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            usage_chunk(),
            r#"{"junk-after-usage":true}"#,
        ],
        true,
    );
    let server = mock_transport(scripted_responses(vec![body])).await;
    let provider = provider_for(&server, fast_policy(25));

    let req = ChatRequest {
        model: MODEL.to_string(),
        messages: vec![ChatMessage::new(ChatRole::User, "hello")],
        tools: vec![ToolDefinition {
            name: "read".into(),
            description: "read a file".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }),
            strict: true,
        }],
        max_tokens: Some(64),
        reasoning: None,
    };
    let cancel = CancellationToken::new();
    let stream = tokio::time::timeout(Duration::from_secs(10), provider.chat_stream(req, cancel))
        .await
        .expect("chat_stream stalled")
        .unwrap();
    let events = collect_events(stream, 8).await;
    assert_items_eq(
        &events,
        &[
            Ok(ProviderEvent::TextDelta("Hi".into())),
            Ok(ProviderEvent::Usage {
                input: 10,
                output: 2,
                cache_read: 6,
                cache_write: 0,
            }),
            Ok(ProviderEvent::Done {
                stop_reason: StopReason::End,
            }),
        ],
    );

    let captured = server.captured();
    assert_eq!(captured.len(), 1);
    // Authorization 只出现在请求头；body/路径不带 key
    assert!(
        captured[0].authorization == format!("{AUTH_SCHEME} {KEY}"),
        "authorization mismatch"
    );
    let wire = &captured[0].body;
    assert!(wire["model"] == MODEL, "wire model mismatch");
    assert!(wire["stream"] == true, "wire stream flag mismatch");
    assert!(
        wire["stream_options"]["include_usage"] == true,
        "wire usage flag mismatch"
    );
    assert!(wire["max_tokens"] == 64, "wire token cap mismatch");
    assert!(wire["messages"][0]["role"] == "user", "wire role mismatch");
    assert!(
        wire["messages"][0]["content"] == "hello",
        "wire content mismatch"
    );
    assert!(
        wire["tools"][0]["type"] == "function",
        "wire tool type mismatch"
    );
    assert!(
        wire["tools"][0]["function"]["name"] == "read",
        "wire tool name mismatch"
    );
    assert!(
        wire["tools"][0]["function"]["parameters"]["type"] == "object",
        "wire schema mismatch"
    );
    assert!(
        wire["tools"][0]["function"]["strict"] == true,
        "wire strict intent mismatch"
    );
}

#[tokio::test(start_paused = true)]
async fn empty_name_stream_continuations_execute_one_real_read_and_observe() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("README.md"), "# Vega E2E\n").unwrap();
    let tool_round = sse_response(
        &[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-read","type":"function","function":{"name":"read","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"","arguments":"{\"path\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"","arguments":"\"README.md\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            usage_chunk(),
        ],
        true,
    );
    let answer_round = sse_response(
        &[
            r##"{"choices":[{"delta":{"content":"# Vega E2E"}}]}"##,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            usage_chunk(),
        ],
        true,
    );
    let server = mock_transport(scripted_responses(vec![tool_round, answer_round])).await;
    let provider = provider_for(&server, fast_policy(1));
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let outcome = crate::run_agent(
        &provider,
        &tools,
        crate::AgentRequest {
            model: MODEL.into(),
            system_prompt: "Read the requested file.".into(),
            history: vec![ChatMessage::new(ChatRole::User, "Read README.md")],
            max_tokens: None,
            completed_tool_results: std::collections::HashMap::new(),
            tool_config: crate::RuntimeToolConfig::default(),
            pricing_catalog: None,
            reasoning: None,
            context_budget: Some(crate::ContextBudget::new(428_000, 128_000, true).unwrap()),
            context_source_version: None,
            context_source_fingerprint: None,
            context_operation_id: None,
            context_compaction_hook: None,
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert!(!outcome.failed);
    assert_eq!(outcome.final_text, "# Vega E2E");
    assert_eq!(outcome.tool_call_count, 1);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        crate::RuntimeEvent::ToolCallFinished(crate::RuntimeToolResult {
            status: crate::RuntimeToolStatus::Success,
            ..
        })
    )));
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, crate::RuntimeEvent::Error(_)))
    );
    let captured = server.captured();
    assert_eq!(captured.len(), 2);
    assert!(
        captured
            .iter()
            .all(|request| request.body.get("max_tokens").is_none()),
        "output capacity must not become an HTTP generation cap in any tool round"
    );
    let follow_up = &captured[1].body["messages"];
    assert!(follow_up.as_array().is_some_and(|messages| {
        messages.iter().any(|message| {
            message["role"] == "assistant"
                && message["tool_calls"][0]["function"]["name"] == "read"
                && message["tool_calls"][0]["function"]["arguments"] == r#"{"path":"README.md"}"#
        }) && messages.iter().any(|message| {
            message["role"] == "tool"
                && message["tool_call_id"] == "call-read"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("# Vega E2E"))
        })
    }));
    assert_eq!(
        std::fs::read_to_string(project.path().join("README.md")).unwrap(),
        "# Vega E2E\n"
    );
}

#[test]
fn captured_request_debug_redacts_distinct_authorization_and_body_sentinels() {
    let sentinels = [
        "VEGA_AUTHORIZATION_SENTINEL",
        "VEGA_MODEL_SENTINEL",
        "VEGA_PROMPT_SENTINEL",
        "VEGA_TOOL_SENTINEL",
    ];
    let captured = CapturedRequest {
        authorization: sentinels[0].into(),
        body: serde_json::json!({
            "model": sentinels[1],
            "messages": [{"content": sentinels[2]}],
            "tools": [{"name": sentinels[3]}],
        }),
    };
    let rendered = format!("{captured:?}");
    for sentinel in sentinels {
        assert!(
            !rendered.contains(sentinel),
            "captured request Debug leaked payload"
        );
    }
    assert!(rendered.contains("authorization_bytes"));
    assert!(rendered.contains("[redacted]"));
}

#[tokio::test(start_paused = true)]
async fn retry_policy_zero_makes_exactly_one_local_http_attempt() {
    let success = sse_response(
        &[r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#],
        true,
    );
    let server = mock_transport(scripted_responses(vec![
        status_response("500 Internal Server Error", &[], "first failure"),
        success,
    ]))
    .await;
    let provider = provider_for(
        &server,
        RetryPolicy {
            max_retries: 0,
            base_delay: Duration::from_millis(1),
            ..RetryPolicy::default()
        },
    );
    let result = provider
        .chat_stream(request(), CancellationToken::new())
        .await;
    assert!(matches!(
        result,
        Err(VegaError::ProviderDiagnostic {
            status: Some(500),
            retryable: false,
            ..
        })
    ));
    assert_eq!(server.attempt_count(), 1);
    assert_eq!(server.captured().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn automatic_title_once_disables_retries_without_mutating_primary_policy() {
    let success = sse_response(
        &[r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#],
        true,
    );
    let server = mock_transport(scripted_responses(vec![
        status_response("500 Internal Server Error", &[], "first"),
        status_response("500 Internal Server Error", &[], "second"),
        success,
    ]))
    .await;
    let provider = provider_for(&server, fast_policy(1));
    assert!(
        provider
            .chat_stream_once(request(), CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(server.attempt_count(), 1);
    let stream = provider
        .chat_stream(request(), CancellationToken::new())
        .await
        .unwrap();
    let _ = collect_events(stream, 8).await;
    assert_eq!(server.attempt_count(), 3);
}

#[tokio::test(start_paused = true)]
async fn mismatched_reasoning_model_fails_before_loopback_http() {
    let server = mock_transport(scripted_responses(vec![])).await;
    let provider = provider_for(&server, fast_policy(1));
    let request = ChatRequest {
        model: MODEL.into(),
        reasoning: Some(FrozenReasoning {
            provider: "openai".into(),
            model: "different-model".into(),
            protocol: ReasoningProtocol::OpenAiChatCompletions,
            choice: ReasoningChoice::Effort("low".into()),
            supports_disabled: false,
            preserve_reasoning_content: false,
            disabled_wire: None,
            declared_efforts: vec!["low".into()],
        }),
        ..Default::default()
    };
    let result = provider
        .chat_stream(request, CancellationToken::new())
        .await;
    assert!(matches!(
        result,
        Err(VegaError::ReasoningSelectionInvalid { .. })
    ));
    assert_eq!(server.attempt_count(), 0);
    assert!(server.captured().is_empty());
}

#[tokio::test(start_paused = true)]
async fn missing_finish_reason_is_protocol_error_for_done_and_raw_eof() {
    let partial = r#"{"choices":[{"delta":{"content":"partial"},"finish_reason":null}]}"#;
    for done in [false, true] {
        let server = mock_transport(scripted_responses(vec![sse_response(&[partial], done)])).await;
        let provider = provider_for(&server, fast_policy(25));
        let stream = provider
            .chat_stream(
                ChatRequest {
                    model: MODEL.into(),
                    ..ChatRequest::default()
                },
                CancellationToken::new(),
            )
            .await
            .expect("stream setup");
        let events = collect_events(stream, 4).await;
        assert!(matches!(
            events.as_slice(),
            [Ok(ProviderEvent::TextDelta(text)), Err(VegaError::ProviderDiagnostic { retryable: false, .. })]
                if text == "partial"
        ));
    }
}

#[tokio::test(start_paused = true)]
async fn retries_5xx_with_backoff_then_succeeds() {
    let ok = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    );
    let server = mock_transport(scripted_responses(vec![
        status_response("500 Internal Server Error", &[], "boom"),
        status_response("500 Internal Server Error", &[], "boom"),
        ok,
    ]))
    .await;
    let started = tokio::time::Instant::now();
    let provider = provider_for(&server, fast_policy(25));
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        provider.chat_stream(request(), CancellationToken::new()),
    )
    .await
    .expect("chat_stream stalled")
    .unwrap();
    let events = collect_events(stream, 4).await;
    assert_items_eq(
        &events,
        &[
            Ok(ProviderEvent::TextDelta("Hi".into())),
            Ok(ProviderEvent::Done {
                stop_reason: StopReason::End,
            }),
        ],
    );
    assert_eq!(server.attempt_count(), 3, "2 failures + 1 success");
    // 退避被调用：两次延迟 25ms + 50ms（下界校验，tokio sleep 不会提前触发）
    assert!(started.elapsed() >= Duration::from_millis(70));
}

#[tokio::test(start_paused = true)]
async fn retry_429_honors_retry_after_header() {
    let ok = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    );
    let server = mock_transport(scripted_responses(vec![
        status_response(
            "429 Too Many Requests",
            &[("Retry-After", "0")],
            "slow down",
        ),
        ok,
    ]))
    .await;
    let started = tokio::time::Instant::now();
    // 指数退避会是 1s；尊重 Retry-After: 0 应几乎立即重试
    let provider = provider_for(&server, RetryPolicy::default());
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        provider.chat_stream(request(), CancellationToken::new()),
    )
    .await
    .expect("chat_stream stalled")
    .unwrap();
    let events = collect_events(stream, 4).await;
    assert_eq!(events.len(), 2, "expected the retried stream to succeed");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "Retry-After: 0 must be honored over the 1s exponential schedule"
    );
    assert_eq!(server.attempt_count(), 2);
}

#[tokio::test(start_paused = true)]
async fn retry_429_without_retry_after_falls_back_to_backoff() {
    let ok = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    );
    let server = mock_transport(scripted_responses(vec![
        status_response("429 Too Many Requests", &[], "slow down"),
        ok,
    ]))
    .await;
    let started = tokio::time::Instant::now();
    let provider = provider_for(&server, fast_policy(25));
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        provider.chat_stream(request(), CancellationToken::new()),
    )
    .await
    .expect("chat_stream stalled")
    .unwrap();
    collect_events(stream, 4).await;
    assert_eq!(server.attempt_count(), 2);
    assert!(started.elapsed() >= Duration::from_millis(20));
}

#[tokio::test(start_paused = true)]
async fn guarded_429_retry_rechecks_and_preserves_normal_retry() {
    let ok = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"safe"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    );
    let server = mock_transport(scripted_responses(vec![
        status_response("429 Too Many Requests", &[], "slow down"),
        ok,
    ]))
    .await;
    let inspections = Arc::new(AtomicUsize::new(0));
    let seen = inspections.clone();
    let provider = provider_for(&server, fast_policy(1)).with_pre_attempt_guard(move |_request| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let stream = provider
        .chat_stream(request(), CancellationToken::new())
        .await
        .expect("safe retry succeeds");
    let events = collect_events(stream, 4).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Ok(ProviderEvent::TextDelta(text)) if text == "safe"))
    );
    assert_eq!(inspections.load(Ordering::SeqCst), 2);
    assert_eq!(server.attempt_count(), 2);
}

#[tokio::test(start_paused = true)]
async fn retries_exhausted_returns_non_retryable_provider_error() {
    let server = mock_transport(scripted_responses(vec![
        status_response("500 Internal Server Error", &[], "boom"),
        status_response("500 Internal Server Error", &[], "boom"),
        status_response("500 Internal Server Error", &[], "boom"),
        status_response("500 Internal Server Error", &[], "boom"),
    ]))
    .await;
    let provider = provider_for(&server, fast_policy(1));
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        provider.chat_stream(request(), CancellationToken::new()),
    )
    .await
    .expect("chat_stream stalled");
    let Err(err) = result else {
        panic!("expected exhausted provider error, got a successful stream");
    };
    match err {
        VegaError::ProviderDiagnostic {
            status,
            message,
            retryable,
            ..
        } => {
            assert_eq!(status, Some(500));
            assert!(
                !retryable,
                "exhausted retries must not advertise retryability"
            );
            assert!(message.contains("after 3 retries"), "retry summary missing");
            assert!(!message.contains(KEY), "provider message leaked key");
        }
        other => panic!("expected exhausted provider error, got {other:?}"),
    }
    assert_eq!(server.attempt_count(), 4, "1 initial + 3 retries");
}

#[tokio::test(start_paused = true)]
async fn network_error_is_retried_then_succeeds() {
    let handler: Handler = Arc::new(|index| {
        if index == 0 {
            return Err(simulated_transport_error());
        }
        Ok(fixture_response(sse_response(
            &[
                r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
                r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            ],
            true,
        )))
    });
    let server = mock_transport(handler).await;
    let provider = provider_for(&server, fast_policy(5));
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        provider.chat_stream(request(), CancellationToken::new()),
    )
    .await
    .expect("chat_stream stalled")
    .unwrap();
    let events = collect_events(stream, 4).await;
    assert_items_eq(
        &events,
        &[
            Ok(ProviderEvent::TextDelta("Hi".into())),
            Ok(ProviderEvent::Done {
                stop_reason: StopReason::End,
            }),
        ],
    );
    assert_eq!(server.attempt_count(), 2);
}

#[tokio::test(start_paused = true)]
async fn non_retryable_4xx_fails_without_retry() {
    let server = mock_transport(scripted_responses(vec![status_response(
        "401 Unauthorized",
        &[],
        r#"{"error":{"message":"invalid credentials"}}"#,
    )]))
    .await;
    let provider = provider_for(&server, fast_policy(25));
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        provider.chat_stream(request(), CancellationToken::new()),
    )
    .await
    .expect("chat_stream stalled");
    let Err(err) = result else {
        panic!("expected 401 provider error, got a successful stream");
    };
    match err {
        VegaError::ProviderDiagnostic {
            status,
            message,
            retryable,
            ..
        } => {
            assert_eq!(status, Some(401));
            assert!(!retryable);
            assert!(
                message.contains("invalid credentials"),
                "provider detail missing"
            );
        }
        other => panic!("expected 401 provider error, got {other:?}"),
    }
    assert_eq!(server.attempt_count(), 1, "4xx must not be retried");
}

#[tokio::test(start_paused = true)]
async fn issue85_strict_schema_rejection_fails_without_non_strict_fallback() {
    let success = sse_response(
        &[r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#],
        true,
    );
    let server = mock_transport(scripted_responses(vec![
        status_response(
            "400 Bad Request",
            &[],
            r#"{"error":{"message":"strict schema rejected"}}"#,
        ),
        success,
    ]))
    .await;
    let provider = provider_for(&server, fast_policy(1));
    let request = ChatRequest {
        model: MODEL.into(),
        messages: vec![ChatMessage::new(ChatRole::User, "hello")],
        tools: crate::tool_definitions(crate::RuntimeRunMode::Execute),
        ..Default::default()
    };

    let result = provider
        .chat_stream(request, CancellationToken::new())
        .await;
    assert!(matches!(
        result,
        Err(VegaError::ProviderDiagnostic {
            status: Some(400),
            retryable: false,
            ..
        })
    ));
    assert_eq!(server.attempt_count(), 1);
    let captured = server.captured();
    assert_eq!(captured.len(), 1);
    assert!(
        captured[0].body["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().all(|tool| tool["function"]["strict"] == true))
    );
}

#[tokio::test(start_paused = true)]
async fn error_body_echoing_the_key_is_redacted() {
    let server = mock_transport(scripted_responses(vec![status_response(
        "400 Bad Request",
        &[],
        &format!(r#"{{"error":{{"message":"bad key: {KEY}"}}}}"#),
    )]))
    .await;
    let provider = provider_for(&server, fast_policy(25));
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        provider.chat_stream(request(), CancellationToken::new()),
    )
    .await
    .expect("chat_stream stalled");
    let Err(err) = result else {
        panic!("expected provider error, got a successful stream");
    };
    match err {
        VegaError::ProviderDiagnostic { message, .. } => {
            assert!(!message.contains(KEY), "provider message leaked key");
            assert!(message.contains("<redacted>"), "redaction marker missing");
        }
        other => panic!("expected provider error, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn already_cancelled_token_fails_fast_without_connecting() {
    let server = mock_transport(scripted_responses(vec![])).await;
    let provider = provider_for(&server, fast_policy(1));
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = provider.chat_stream(request(), cancel).await;
    assert!(matches!(result, Err(VegaError::Cancelled)));
    assert_eq!(
        server.attempt_count(),
        0,
        "cancelled request must not connect"
    );
}

#[tokio::test(start_paused = true)]
async fn cancel_during_backoff_aborts_without_another_request() {
    let handler: Handler = Arc::new(|_| {
        Ok(fixture_response(status_response(
            "503 Service Unavailable",
            &[],
            "unavailable",
        )))
    });
    let server = mock_transport(handler).await;
    let provider = provider_for(
        &server,
        RetryPolicy {
            base_delay: Duration::from_secs(30),
            ..RetryPolicy::default()
        },
    );
    let cancel = CancellationToken::new();
    let request_cancel = cancel.clone();
    let task = tokio::spawn(async move { provider.chat_stream(request(), request_cancel).await });
    // 等第一个 503 处理完（进入 30s 退避），再取消
    for _ in 0..100 {
        if server.attempt_count() != 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(server.attempt_count(), 1, "first attempt must begin");
    let cancelled_at = tokio::time::Instant::now();
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("cancel during backoff must abort immediately")
        .unwrap();
    assert!(matches!(result, Err(VegaError::Cancelled)));
    assert!(cancelled_at.elapsed() < Duration::from_secs(2));
    assert_eq!(server.attempt_count(), 1, "no request after cancellation");
}

#[tokio::test(start_paused = true)]
async fn cancel_mid_stream_stops_immediately_with_no_further_events() {
    let handler: Handler = Arc::new(|_| {
        let bytes = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n".to_vec();
        let body = futures::stream::once(async move { Ok::<_, std::io::Error>(bytes) })
            .chain(futures::stream::pending());
        Ok(::http::Response::builder()
            .status(200)
            .body(reqwest::Body::wrap_stream(body))
            .unwrap()
            .into())
    });
    let server = mock_transport(handler).await;
    let provider = provider_for(&server, fast_policy(1));
    let cancel = CancellationToken::new();
    let mut stream = tokio::time::timeout(
        Duration::from_secs(10),
        provider.chat_stream(request(), cancel.clone()),
    )
    .await
    .expect("chat_stream stalled")
    .unwrap();
    match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
        Ok(Some(Ok(ev))) => assert_eq!(ev, ProviderEvent::TextDelta("Hel".into())),
        other => panic!("expected first text delta, got {other:?}"),
    }
    // 流中取消：立即断且不再产生任何事件
    let cancelled_at = tokio::time::Instant::now();
    cancel.cancel();
    let rest = tokio::time::timeout(Duration::from_secs(1), collect_events(stream, 4))
        .await
        .expect("stream cancellation must finish immediately");
    assert!(cancelled_at.elapsed() < Duration::from_secs(1));
    assert!(
        rest.is_empty(),
        "no events after cancellation, got {rest:?}"
    );
    drop(server);
}
