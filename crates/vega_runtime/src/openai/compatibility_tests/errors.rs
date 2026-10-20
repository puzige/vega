use super::http::{HttpFixture, Reply};
use super::*;

#[tokio::test]
async fn native_http_failures_preserve_status_bounded_reason_and_redact_credentials() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        for (status, body) in [
            (
                403,
                json!({"error":{"message":"blocked by gateway WAF"}}).to_string(),
            ),
            (
                403,
                json!({"error":{"message":"upstream WAF blocked policy XYZ","metadata":{"raw":"upstream WAF blocked policy XYZ"}}}).to_string(),
            ),
            (
                400,
                json!({"error":{"message":"owned-fixture-key invalid request"}}).to_string(),
            ),
            (422, String::new()),
            (400, "x".repeat(10_000)),
        ] {
            let mut server = HttpFixture::new(vec![Reply {
                status,
                body: body.clone(),
            }])
            .await;
            let model = fixed(api, &server.base, "fixture", "new-model");
            let provider = OpenAiProvider::new(&server.base, "owned-fixture-key")
                .unwrap()
                .with_responses_api(api == ModelApi::Responses)
                .with_model_snapshot(model.clone())
                .unwrap()
                .with_retry_policy(RetryPolicy {
                    max_retries: 0,
                    base_delay: Duration::ZERO,
                    max_delay: Duration::ZERO,
                });
            let error = match provider
                .chat_stream_once(
                    bound_request(&model, ReasoningChoice::ProviderDefault),
                    CancellationToken::new(),
                )
                .await
            {
                Err(error) => error,
                Ok(_) => panic!("rejected HTTP request unexpectedly succeeded"),
            };
            match &error {
                VegaError::ProviderDiagnostic {
                    status: actual,
                    retryable,
                    message,
                    ..
                }
                | VegaError::Provider {
                    status: actual,
                    retryable,
                    message,
                } => {
                    assert_eq!(*actual, Some(status));
                    assert!(!retryable);
                    assert!(message.len() <= 4200);
                    assert!(!message.contains("owned-fixture-key"));
                    if status == 403 && body.contains("blocked by gateway WAF") {
                        assert!(message.contains("blocked by gateway WAF"));
                    }
                    if body.contains("upstream WAF blocked policy XYZ") {
                        assert_eq!(message.matches("upstream WAF blocked policy XYZ").count(), 1);
                    }
                }
                other => panic!("wrong native error {other:?}"),
            }
            assert!(!format!("{error:?}").contains("owned-fixture-key"));
            assert!(!format!("{error}").contains("owned-fixture-key"));
            server.finish().await;
        }
    }
}

#[tokio::test]
async fn retryable_statuses_retry_same_wire_while_rejected_requests_never_change_choice() {
    for (status, retry) in [
        (429, true),
        (500, true),
        (502, true),
        (503, true),
        (504, true),
        (400, false),
        (401, false),
        (403, false),
        (404, false),
        (422, false),
    ] {
        let final_chunk = chat_chunk(json!({"content":"Recovered"}), json!("stop"));
        let replies = if retry {
            vec![Reply::status(status), Reply::events(&[final_chunk])]
        } else {
            vec![Reply::status(status)]
        };
        let mut server = HttpFixture::new(replies).await;
        let model = fixed(
            ModelApi::ChatCompletions,
            &server.base,
            "fixture",
            "new-model",
        );
        let provider = OpenAiProvider::new(&server.base, "owned-key")
            .unwrap()
            .with_model_snapshot(model.clone())
            .unwrap()
            .with_retry_policy(RetryPolicy {
                max_retries: 1,
                base_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            });
        let result = provider
            .chat_stream(
                bound_request(&model, ReasoningChoice::Effort("high".into())),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(result.is_ok(), retry, "{status}");
        if let Ok(mut stream) = result {
            let mut answer = String::new();
            while let Some(event) = stream.next().await {
                if let ProviderEvent::TextDelta(delta) = event.unwrap() {
                    answer.push_str(&delta);
                }
            }
            assert_eq!(answer, "Recovered");
        }
        server.finish().await;
        let received = server.received.lock().unwrap();
        assert_eq!(received.len(), if retry { 2 } else { 1 });
        assert!(
            received
                .iter()
                .all(|request| request.body["reasoning_effort"] == "high")
        );
        if retry {
            assert_eq!(received[0].body, received[1].body);
        }
    }
    for (max_retries, attempts) in [(0, 1), (3, 4)] {
        let mut server =
            HttpFixture::new((0..attempts).map(|_| Reply::status(503)).collect()).await;
        let model = fixed(
            ModelApi::ChatCompletions,
            &server.base,
            "fixture",
            "new-model",
        );
        let provider = OpenAiProvider::new(&server.base, "owned-key")
            .unwrap()
            .with_model_snapshot(model.clone())
            .unwrap()
            .with_retry_policy(RetryPolicy {
                max_retries,
                base_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            });
        assert!(
            tokio::time::timeout(
                Duration::from_secs(10),
                provider.chat_stream(
                    bound_request(&model, ReasoningChoice::Effort("high".into())),
                    CancellationToken::new(),
                ),
            )
            .await
            .expect("bounded native retry exhaustion")
            .is_err()
        );
        server.finish().await;
        let received = server.received.lock().unwrap();
        assert_eq!(received.len(), attempts);
        assert!(received.iter().all(|request| {
            request.body == received[0].body && request.body["reasoning_effort"] == "high"
        }));
    }
}

#[test]
fn explicit_context_error_codes_and_phrases_do_not_confuse_usage_length_or_rate_limits() {
    for body in [
        json!({"error":{"code":"context_length_exceeded","message":"input is too large","requested_tokens":1200,"max_context_tokens":1000}}).to_string(),
        json!({"error":{"type":"prompt_too_long","message":"input is too large"}}).to_string(),
        "prompt too long; exceeded max context length by 100918 tokens".into(),
        "The input (516368 tokens) is longer than the model's context length (262144 tokens).".into(),
        "Input length (265330) exceeds model's maximum context length (262144).".into(),
        "Input length 131393 exceeds the maximum allowed input length of 131040 tokens.".into(),
        "Prompt has 256468 tokens, but the configured context size is 256000 tokens".into(),
        "Prompt has 5,958,968 tokens, but the configured context size is 256,000 tokens".into(),
    ] {
        let error=VegaError::Provider{status:Some(400),message:body.clone(),retryable:false};
        assert!(error.context_input_overflow().is_some(),"{body}");
    }
    for (status, body) in [
        (500, "model runner crashed unexpectedly"),
        (429, "Too many tokens, please wait"),
        (503, "Service unavailable"),
        (429, "Rate limit exceeded, please retry"),
        (400, "invalid temperature"),
        (400, "maximum output length reached"),
    ] {
        assert!(
            VegaError::Provider {
                status: Some(status),
                message: body.into(),
                retryable: false
            }
            .context_input_overflow()
            .is_none(),
            "{body}"
        );
    }
    let error=VegaError::Provider{status:Some(400),message:json!({"error":{"code":"context_length_exceeded","requested_tokens":1200,"max_context_tokens":1000}}).to_string(),retryable:false};
    assert_eq!(error.context_input_overflow(), Some(Some(200)));
}

#[test]
fn actual_generation_length_never_manufactures_provider_context_overflow() {
    for (input, cached, output) in [
        (1_048_570, 1_048_512, 0),
        (279_141, 253_584, 16),
        (4062, 0, 1024),
        (100, 0, 0),
        (1000, 0, 4096),
    ] {
        let events=decode_chat(&[json!({"choices":[{"delta":{},"finish_reason":"length"}],"usage":{"prompt_tokens":input,"completion_tokens":output,"prompt_tokens_details":{"cached_tokens":cached}}})],true).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            ProviderEvent::Done {
                stop_reason: StopReason::Length
            }
        )));
        assert!(events.iter().any(|event|matches!(event,ProviderEvent::Usage{input:observed,output:out,..} if *observed==input&&*out==output)));
    }
}

#[test]
fn deterministic_retry_schedule_is_finite_and_honors_bounded_server_delay() {
    let policy = RetryPolicy {
        max_retries: 3,
        base_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(45),
    };
    for (attempt, expected) in [(0, 10), (1, 20), (2, 40), (3, 45), (u32::MAX, 45)] {
        assert_eq!(policy.backoff(attempt), Duration::from_millis(expected));
    }
    assert_eq!(
        policy.delay_for(2, Some(Duration::from_millis(7))),
        Duration::from_millis(7)
    );
    assert_eq!(
        policy.delay_for(0, Some(Duration::from_secs(60))),
        Duration::from_millis(45)
    );
    assert_eq!(parse_retry_after(" 3 "), Some(Duration::from_secs(3)));
    assert_eq!(parse_retry_after("not-a-delay"), None);
    assert_eq!(parse_retry_after("-1"), None);
}

#[tokio::test]
async fn cancelled_transport_does_not_start_network_or_retry() {
    let server = HttpFixture::new(Vec::new()).await;
    let model = fixed(ModelApi::ChatCompletions, &server.base, "fixture", "future");
    let provider = OpenAiProvider::new(&server.base, "owned-key")
        .unwrap()
        .with_model_snapshot(model.clone())
        .unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        provider
            .chat_stream(
                bound_request(&model, ReasoningChoice::ProviderDefault),
                cancel
            )
            .await,
        Err(VegaError::Cancelled)
    ));
    assert!(server.received.lock().unwrap().is_empty());
}
