use super::http::{HttpFixture, Reply};
use super::*;
use std::sync::{Arc, Mutex};

fn no_retry() -> RetryPolicy {
    RetryPolicy {
        max_retries: 0,
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
    }
}

fn local_model(api: ModelApi, endpoint: &str) -> ResolvedModel {
    resolved(
        json!({"baseUrl":endpoint,"api":if api==ModelApi::Responses{"openai-responses"}else{"openai-completions"},"thinkingLevelMap":{"high":"high"},"compat":{"supportsReasoningEffort":true}}),
    )
}

fn native_complete(api: ModelApi) -> Vec<Value> {
    if api == ModelApi::Responses {
        vec![
            json!({"type":"response.completed","response":{"id":"r","status":"completed","output":[{"type":"message","id":"m","role":"assistant","content":[{"type":"output_text","text":"OK"}]}],"usage":{"input_tokens":11,"output_tokens":2}}}),
        ]
    } else {
        vec![
            chat_chunk(json!({}), json!("")),
            chat_chunk(json!({"content":"OK"}), Value::Null),
            json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":11,"completion_tokens":2}}),
        ]
    }
}

#[tokio::test]
async fn actual_provider_binds_default_snapshot_and_encodes_each_native_api() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let mut server = HttpFixture::new(vec![Reply::events(&native_complete(api))]).await;
        let model = local_model(api, &server.base);
        let inspected = Arc::new(Mutex::new(Vec::new()));
        let observed = inspected.clone();
        let provider = OpenAiProvider::new(&server.base, "owned-fixture-key")
            .unwrap()
            .with_responses_api(api == ModelApi::Responses)
            .with_retry_policy(no_retry())
            .with_model_snapshot(model.clone())
            .unwrap()
            .with_pre_attempt_guard(move |request| {
                observed.lock().unwrap().push(request.clone());
                Ok(())
            });
        let request = ChatRequest {
            model: model.identity.model.clone(),
            messages: vec![ChatMessage::new(ChatRole::User, "Reply OK")],
            ..Default::default()
        };
        let mut stream = provider
            .chat_stream_once(request, CancellationToken::new())
            .await
            .unwrap();
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event.unwrap());
        }
        assert_eq!(text(&events), "OK");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ProviderEvent::Done {
                        stop_reason: StopReason::End
                    }
                ))
                .count(),
            1
        );
        assert_eq!(inspected.lock().unwrap().len(), 1);
        assert_eq!(
            inspected.lock().unwrap()[0].model_snapshot.as_deref(),
            Some(&model)
        );
        assert!(
            inspected.lock().unwrap()[0]
                .reasoning
                .as_ref()
                .is_none_or(|reasoning| reasoning.choice == ReasoningChoice::ProviderDefault)
        );
        server.finish().await;
        let received = server.received.lock().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(
            received[0].path,
            if api == ModelApi::Responses {
                "/v1/responses"
            } else {
                "/v1/chat/completions"
            }
        );
        assert_eq!(received[0].body["model"], model.identity.model);
        assert!(received[0].body.get("reasoning_effort").is_none());
        assert!(received[0].body.pointer("/reasoning/effort").is_none());
        assert!(received[0].body.get("max_tokens").is_none());
        assert!(received[0].body.get("max_output_tokens").is_none());
        assert!(!format!("{provider:?}").contains("owned-fixture-key"));
    }
}

#[tokio::test]
async fn recovery_retry_uses_same_snapshot_wire_and_actual_terminal_once() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let mut server = HttpFixture::new(vec![
            Reply::status(503),
            Reply::events(&native_complete(api)),
        ])
        .await;
        let model = local_model(api, &server.base);
        let inspected = Arc::new(Mutex::new(Vec::new()));
        let observed = inspected.clone();
        let provider = OpenAiProvider::new(&server.base, "owned-fixture-key")
            .unwrap()
            .with_responses_api(api == ModelApi::Responses)
            .with_model_snapshot(model.clone())
            .unwrap()
            .with_retry_policy(RetryPolicy {
                max_retries: 1,
                base_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            })
            .with_pre_attempt_guard(move |request| {
                observed.lock().unwrap().push(request.clone());
                Ok(())
            });
        let mut request = bound_request(&model, ReasoningChoice::Effort("high".into()));
        request.max_tokens = Some(321);
        let mut stream = provider
            .chat_stream_recovering(request, CancellationToken::new())
            .await
            .unwrap()
            .events;
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event.unwrap());
        }
        assert_eq!(text(&events), "OK");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ProviderEvent::Done { .. }))
                .count(),
            1
        );
        {
            let inspected = inspected.lock().unwrap();
            assert_eq!(inspected.len(), 2);
            assert!(
                inspected
                    .iter()
                    .all(|request| request.model_snapshot.as_deref() == Some(&model))
            );
        }
        server.finish().await;
        let received = server.received.lock().unwrap();
        assert_eq!(received.len(), 2);
        assert_eq!(received[0].body, received[1].body);
        assert_eq!(
            received[1].body.pointer(if api == ModelApi::Responses {
                "/reasoning/effort"
            } else {
                "/reasoning_effort"
            }),
            Some(&json!("high"))
        );
        assert_eq!(
            received[1].body[if api == ModelApi::Responses {
                "max_output_tokens"
            } else {
                "max_tokens"
            }],
            321
        );
    }
}

#[tokio::test]
async fn provider_probe_uses_real_codec_and_rejects_intermediate_only_or_foreign_terminal() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let mut server = HttpFixture::new(vec![Reply::events(&native_complete(api))]).await;
        let result = if api == ModelApi::Responses {
            crate::provider_check::probe_responses(
                &server.base,
                "owned-fixture-key",
                "new-model",
                CancellationToken::new(),
            )
            .await
        } else {
            crate::provider_check::probe(
                &server.base,
                "owned-fixture-key",
                "new-model",
                CancellationToken::new(),
            )
            .await
        };
        assert_eq!(result, Ok(()));
        server.finish().await;
        {
            let received = server.received.lock().unwrap();
            assert_eq!(received.len(), 1);
            assert_eq!(
                received[0].body[if api == ModelApi::Responses {
                    "max_output_tokens"
                } else {
                    "max_tokens"
                }],
                128
            );
            assert!(received[0].body.get("reasoning_effort").is_none());
            assert!(received[0].body.pointer("/reasoning/effort").is_none());
        }
        let chunks = if api == ModelApi::Responses {
            vec![json!({"type":"response.output_text.delta","delta":"partial"})]
        } else {
            vec![chat_chunk(json!({"content":"partial"}), json!(""))]
        };
        let mut server = HttpFixture::new(vec![Reply::events(&chunks)]).await;
        let result = if api == ModelApi::Responses {
            crate::provider_check::probe_responses(
                &server.base,
                "owned-fixture-key",
                "new-model",
                CancellationToken::new(),
            )
            .await
        } else {
            crate::provider_check::probe(
                &server.base,
                "owned-fixture-key",
                "new-model",
                CancellationToken::new(),
            )
            .await
        };
        assert_eq!(result, Err(crate::provider_check::CheckError::Malformed));
        server.finish().await;
    }
}

#[tokio::test]
async fn rejected_effort_or_bad_snapshot_never_falls_back_to_another_request() {
    let mut server = HttpFixture::new(vec![Reply::status(400)]).await;
    let model = local_model(ModelApi::ChatCompletions, &server.base);
    let provider = OpenAiProvider::new(&server.base, "owned-fixture-key")
        .unwrap()
        .with_model_snapshot(model.clone())
        .unwrap()
        .with_retry_policy(no_retry());
    let result = provider
        .chat_stream_once(
            bound_request(&model, ReasoningChoice::Effort("high".into())),
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_err());
    server.finish().await;
    assert_eq!(server.received.lock().unwrap().len(), 1);
    assert_eq!(
        server.received.lock().unwrap()[0].body["reasoning_effort"],
        "high"
    );
    let server = HttpFixture::new(Vec::new()).await;
    let other = local_model(ModelApi::ChatCompletions, &server.base);
    let provider = OpenAiProvider::new(&server.base, "owned-fixture-key")
        .unwrap()
        .with_model_snapshot(other)
        .unwrap()
        .with_retry_policy(no_retry());
    let result = provider
        .chat_stream_once(
            bound_request(&model, ReasoningChoice::ProviderDefault),
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_err());
    assert!(server.received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn incomplete_http_stream_never_reports_success_even_with_display_text() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let chunks = if api == ModelApi::Responses {
            vec![json!({"type":"response.output_text.delta","delta":"partial"})]
        } else {
            vec![chat_chunk(json!({"content":"partial"}), json!(""))]
        };
        let mut server = HttpFixture::new(vec![Reply::events(&chunks)]).await;
        let model = local_model(api, &server.base);
        let provider = OpenAiProvider::new(&server.base, "owned-fixture-key")
            .unwrap()
            .with_responses_api(api == ModelApi::Responses)
            .with_model_snapshot(model)
            .unwrap()
            .with_retry_policy(no_retry());
        let result = provider
            .chat_stream_once(
                ChatRequest {
                    model: "fixture-model".into(),
                    messages: vec![ChatMessage::new(ChatRole::User, "test")],
                    ..Default::default()
                },
                CancellationToken::new(),
            )
            .await;
        let mut failed = result.is_err();
        let mut done = false;
        if let Ok(mut stream) = result {
            while let Some(event) = stream.next().await {
                match event {
                    Err(_) => failed = true,
                    Ok(ProviderEvent::Done { .. }) => done = true,
                    _ => {}
                }
            }
        }
        assert!(failed);
        assert!(!done);
        server.finish().await;
    }
}
