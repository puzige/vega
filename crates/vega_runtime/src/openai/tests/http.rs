use super::*;

// ---------- 集成：请求/响应走本地 TCP ----------

#[tokio::test]
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
    let server = spawn_server(scripted_server(vec![body])).await;
    let provider = provider_for(&server, fast_policy(25));

    let req = ChatRequest {
        model: MODEL.to_string(),
        messages: vec![ChatMessage::new(ChatRole::User, "hello")],
        tools: vec![ToolDefinition {
            name: "read".into(),
            description: "read a file".into(),
            input_schema: serde_json::json!({"type":"object"}),
        }],
        max_tokens: Some(64),
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

#[tokio::test]
async fn retry_policy_zero_makes_exactly_one_local_http_attempt() {
    let success = sse_response(
        &[r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#],
        true,
    );
    let server = spawn_server(scripted_server(vec![
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
        Err(VegaError::Provider {
            status: Some(500),
            retryable: false,
            ..
        })
    ));
    assert_eq!(server.connection_count(), 1);
    assert_eq!(server.captured().len(), 1);
}

#[tokio::test]
async fn missing_finish_reason_is_protocol_error_for_done_and_raw_eof() {
    let partial = r#"{"choices":[{"delta":{"content":"partial"},"finish_reason":null}]}"#;
    for done in [false, true] {
        let server = spawn_server(scripted_server(vec![sse_response(&[partial], done)])).await;
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
            [Ok(ProviderEvent::TextDelta(text)), Err(VegaError::Provider { retryable: false, .. })]
                if text == "partial"
        ));
    }
}

#[tokio::test]
async fn retries_5xx_with_backoff_then_succeeds() {
    let ok = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    );
    let server = spawn_server(scripted_server(vec![
        status_response("500 Internal Server Error", &[], "boom"),
        status_response("500 Internal Server Error", &[], "boom"),
        ok,
    ]))
    .await;
    let started = std::time::Instant::now();
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
    assert_eq!(server.connection_count(), 3, "2 failures + 1 success");
    // 退避被调用：两次延迟 25ms + 50ms（下界校验，tokio sleep 不会提前触发）
    assert!(started.elapsed() >= Duration::from_millis(70));
}

#[tokio::test]
async fn retry_429_honors_retry_after_header() {
    let ok = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    );
    let server = spawn_server(scripted_server(vec![
        status_response(
            "429 Too Many Requests",
            &[("Retry-After", "0")],
            "slow down",
        ),
        ok,
    ]))
    .await;
    let started = std::time::Instant::now();
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
    assert_eq!(server.connection_count(), 2);
}

#[tokio::test]
async fn retry_429_without_retry_after_falls_back_to_backoff() {
    let ok = sse_response(
        &[
            r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ],
        true,
    );
    let server = spawn_server(scripted_server(vec![
        status_response("429 Too Many Requests", &[], "slow down"),
        ok,
    ]))
    .await;
    let started = std::time::Instant::now();
    let provider = provider_for(&server, fast_policy(25));
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        provider.chat_stream(request(), CancellationToken::new()),
    )
    .await
    .expect("chat_stream stalled")
    .unwrap();
    collect_events(stream, 4).await;
    assert_eq!(server.connection_count(), 2);
    assert!(started.elapsed() >= Duration::from_millis(20));
}

#[tokio::test]
async fn retries_exhausted_returns_non_retryable_provider_error() {
    let server = spawn_server(scripted_server(vec![
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
        VegaError::Provider {
            status,
            message,
            retryable,
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
    assert_eq!(server.connection_count(), 4, "1 initial + 3 retries");
}

#[tokio::test]
async fn network_error_is_retried_then_succeeds() {
    // 连接 0：读完请求后直接断开（模拟网络错误）；连接 1：正常响应
    let handler: Handler = Arc::new(move |idx: u64, mut stream: TcpStream| {
        Box::pin(async move {
            if idx == 1 {
                let ok = sse_response(
                    &[
                        r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
                        r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
                    ],
                    true,
                );
                let _ = stream.write_all(&ok).await;
                let _ = stream.flush().await;
            }
            // idx == 0：直接 drop，客户端读到连接中断
        }) as HandlerFuture
    });
    let server = spawn_server(handler).await;
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
    assert_eq!(server.connection_count(), 2);
}

#[tokio::test]
async fn non_retryable_4xx_fails_without_retry() {
    let server = spawn_server(scripted_server(vec![status_response(
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
        VegaError::Provider {
            status,
            message,
            retryable,
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
    assert_eq!(server.connection_count(), 1, "4xx must not be retried");
}

#[tokio::test]
async fn error_body_echoing_the_key_is_redacted() {
    let server = spawn_server(scripted_server(vec![status_response(
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
        VegaError::Provider { message, .. } => {
            assert!(!message.contains(KEY), "provider message leaked key");
            assert!(message.contains("<redacted>"), "redaction marker missing");
        }
        other => panic!("expected provider error, got {other:?}"),
    }
}

#[tokio::test]
async fn already_cancelled_token_fails_fast_without_connecting() {
    let server = spawn_server(scripted_server(vec![])).await;
    let provider = provider_for(&server, fast_policy(1));
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = provider.chat_stream(request(), cancel).await;
    assert!(matches!(result, Err(VegaError::Cancelled)));
    assert_eq!(
        server.connection_count(),
        0,
        "cancelled request must not connect"
    );
}

#[tokio::test]
async fn cancel_during_backoff_aborts_without_another_request() {
    // 永远 503；取消发生在第一次退避期间
    let handler: Handler = Arc::new(|_idx: u64, mut stream: TcpStream| {
        Box::pin(async move {
            let resp = status_response("503 Service Unavailable", &[], "unavailable");
            let _ = stream.write_all(&resp).await;
            let _ = stream.flush().await;
        }) as HandlerFuture
    });
    let server = spawn_server(handler).await;
    let provider = provider_for(
        &server,
        RetryPolicy {
            base_delay: Duration::from_secs(30),
            ..RetryPolicy::default()
        },
    );
    let cancel = CancellationToken::new();
    let request_cancel = cancel.clone();
    let started = std::time::Instant::now();
    let task = tokio::spawn(async move { provider.chat_stream(request(), request_cancel).await });
    // 等第一个 503 处理完（进入 30s 退避），再取消
    tokio::time::sleep(Duration::from_millis(500)).await;
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("cancel during backoff must abort immediately")
        .unwrap();
    assert!(matches!(result, Err(VegaError::Cancelled)));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        server.connection_count(),
        1,
        "no request after cancellation"
    );
}

#[tokio::test]
async fn cancel_mid_stream_stops_immediately_with_no_further_events() {
    // 服务器发出第一个事件后挂住连接不关闭
    let handler: Handler = Arc::new(|_idx: u64, mut stream: TcpStream| {
        Box::pin(async move {
            let head = sse_response(&[r#"{"choices":[{"delta":{"content":"Hel"}}]}"#], false);
            let _ = stream.write_all(&head).await;
            let _ = stream.flush().await;
            // 保持连接打开，客户端在流中取消
            tokio::time::sleep(Duration::from_secs(30)).await;
        }) as HandlerFuture
    });
    let server = spawn_server(handler).await;
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
    cancel.cancel();
    let started = std::time::Instant::now();
    let rest = collect_events(stream, 4).await;
    assert!(
        rest.is_empty(),
        "no events after cancellation, got {rest:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "cancel must tear the stream down immediately"
    );
    drop(server);
}
