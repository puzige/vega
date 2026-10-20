use super::*;

#[tokio::test]
async fn default_conversation_request_binds_model_and_persists_only_visible_result_and_usage() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let dir = tempfile::tempdir().unwrap();
        let store = database(dir.path(), "new-model");
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let mut server =
            HttpFixture::new(vec![Reply::events(&complete(api, "Visible answer"))]).await;
        let observed = Arc::new(Mutex::new(Vec::new()));
        let provider = provider(api, &server.base, observed.clone());
        let outcome = crate::agent::run_thread_task(
            &store,
            &provider,
            &tools,
            "thread",
            "Question",
            "Follow the request.",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!outcome.failed);
        assert_eq!(
            server.received.lock().unwrap()[0].path,
            if api == ModelApi::Responses {
                "/v1/responses"
            } else {
                "/v1/chat/completions"
            }
        );
        assert_eq!(outcome.content, "Visible answer");
        server.finish().await;
        let captured = observed.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0].model_snapshot.as_deref(),
            Some(&model(api, &server.base))
        );
        let content: String = store
            .conn()
            .query_row(
                "SELECT content FROM messages WHERE id=?1",
                [&outcome.assistant_message_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(content, "Visible answer");
        let (input,output,cached):(i64,i64,i64)=store.conn().query_row("SELECT input_tokens,output_tokens,cache_read_tokens FROM token_usage WHERE thread_id='thread' ORDER BY rowid DESC LIMIT 1",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
        assert_eq!((input, output, cached), (17, 3, 5));
    }
}

#[tokio::test]
async fn original_private_reasoning_and_details_replay_in_run_but_never_enter_sqlite_or_debug() {
    let dir = tempfile::tempdir().unwrap();
    let store = database(dir.path(), "new-model");
    std::fs::write(dir.path().join("README.md"), "owned file").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let raw = "private-reasoning-sentinel";
    let opaque = "opaque-signed-sentinel";
    let first = vec![
        json!({"choices":[{"delta":{"reasoning_text":raw,"reasoning_details":[{"type":"reasoning.encrypted","id":"sealed","data":opaque}],"tool_calls":[{"index":0,"id":"call-one","function":{"name":"read","arguments":"{\"path\":\"README.md\"}"}}]},"finish_reason":"tool_calls"}]}),
    ];
    let mut server = HttpFixture::new(vec![
        Reply::events(&first),
        Reply::events(&complete(ModelApi::ChatCompletions, "Visible answer")),
    ])
    .await;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let provider = provider(ModelApi::ChatCompletions, &server.base, observed.clone());
    let outcome = crate::agent::run_thread_task(
        &store,
        &provider,
        &tools,
        "thread",
        "Read README.md",
        "Read owned data.",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed);
    assert_eq!(outcome.content, "Visible answer");
    server.finish().await;
    let requests = server.received.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].body.to_string().contains(raw));
    assert!(requests[1].body.to_string().contains(opaque));
    let mut statement = store
        .conn()
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .unwrap();
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for name in names {
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        );
        let mut statement = store
            .conn()
            .prepare(&format!("SELECT * FROM {name}"))
            .unwrap();
        let count = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for column in 0..count {
                if let Ok(value) = row.get::<_, String>(column) {
                    assert!(!value.contains(raw), "{name}");
                    assert!(!value.contains(opaque), "{name}");
                }
            }
        }
    }
    assert!(!format!("{outcome:?}").contains(raw));
    assert!(!format!("{outcome:?}").contains(opaque));
}

#[tokio::test]
async fn credential_guard_checks_all_private_channels_and_opaque_keys_without_rewriting_signed_data()
 {
    let secret = "sk-owned-credential-protected-123456789";
    for channel in [
        "reasoning",
        "reasoning_text",
        "detail-text",
        "detail-data",
        "detail-key",
        "responses",
    ] {
        let api = if channel == "responses" {
            ModelApi::Responses
        } else {
            ModelApi::ChatCompletions
        };
        let server = HttpFixture::new(Vec::new()).await;
        let observed = Arc::new(Mutex::new(Vec::new()));
        let real = Arc::new(provider(api, &server.base, observed));
        let wrapper = crate::agent::OwnerCredentialProvider::new(
            real,
            Arc::new(move || Ok(vec![secret.into()])),
        );
        let mut request = ChatRequest {
            model: "new-model".into(),
            messages: Vec::new(),
            model_snapshot: Some(Box::new(model(api, &server.base))),
            ..Default::default()
        };
        let mut assistant = ChatMessage::new(ChatRole::Assistant, "visible");
        if channel == "responses" {
            let events=vega_runtime::test_support::decode_responses(&[json!({"type":"response.completed","response":{"id":"r","status":"completed","output":[{"type":"reasoning","id":"rs","encrypted_content":secret,"summary":[]}]}})]).unwrap();
            assistant.response_reasoning = events
                .into_iter()
                .find_map(|event| match event {
                    ProviderEvent::ReasoningReplay(items) => Some(items),
                    _ => None,
                })
                .unwrap();
        } else {
            let mut delta = json!({});
            match channel {
                "detail-text" => {
                    delta["reasoning_details"] =
                        json!([{"type":"reasoning.text","id":"x","text":secret}])
                }
                "detail-data" => {
                    delta["reasoning_details"] =
                        json!([{"type":"reasoning.encrypted","id":"x","data":secret}])
                }
                "detail-key" => {
                    let mut detail = json!({"type":"reasoning.encrypted","id":"x","data":"signed"});
                    detail[secret] = json!("unchanged");
                    delta["reasoning_details"] = json!([detail]);
                }
                _ => delta[channel] = json!(secret),
            }
            let events = vega_runtime::test_support::decode_chat(
                &[json!({"choices":[{"delta":delta,"finish_reason":"stop"}]})],
                true,
            )
            .unwrap();
            for event in events {
                match event {
                    ProviderEvent::ChatReasoningContent(
                        vega_runtime::ChatReasoningPresence::NonEmpty(text),
                    ) => assistant.reasoning_content = Some(text),
                    ProviderEvent::ChatReasoningState(raw) => assistant.chat_reasoning = Some(raw),
                    _ => {}
                }
            }
        }
        let original = assistant.clone();
        request.messages.push(assistant);
        let result = wrapper
            .chat_stream_once(request, CancellationToken::new())
            .await;
        assert!(
            matches!(
                result,
                Err(vega_runtime::VegaError::CredentialExposureBlocked)
            ),
            "{channel}"
        );
        assert_eq!(original.content, "visible");
        assert!(server.received.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn credential_reader_failure_and_cancel_do_not_send_auxiliary_requests() {
    let server = HttpFixture::new(Vec::new()).await;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(provider(ModelApi::ChatCompletions, &server.base, observed));
    let wrapper = crate::agent::OwnerCredentialProvider::new(provider, Arc::new(|| Err(())));
    let request = ChatRequest {
        model: "new-model".into(),
        messages: vec![ChatMessage::new(ChatRole::User, "safe")],
        ..Default::default()
    };
    assert!(
        wrapper
            .chat_stream_once(request, CancellationToken::new())
            .await
            .is_err()
    );
    assert!(server.received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn credential_projection_redacts_mutable_text_before_wire_and_keeps_source_request_owned() {
    let secret = "sk-owned-credential-protected-123456789";
    let mut server = HttpFixture::new(vec![Reply::events(&complete(
        ModelApi::ChatCompletions,
        "Safe answer",
    ))])
    .await;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let real = Arc::new(provider(
        ModelApi::ChatCompletions,
        &server.base,
        observed.clone(),
    ));
    let wrapper =
        crate::agent::OwnerCredentialProvider::new(real, Arc::new(move || Ok(vec![secret.into()])));
    let mut assistant = ChatMessage::new(ChatRole::Assistant, "visible");
    assistant.reasoning_content = Some(secret.into());
    let original = assistant.clone();
    let request = ChatRequest {
        model: "new-model".into(),
        messages: vec![assistant, ChatMessage::new(ChatRole::User, "Continue")],
        ..Default::default()
    };
    let mut stream = wrapper
        .chat_stream_once(request, CancellationToken::new())
        .await
        .unwrap();
    let mut content = String::new();
    while let Some(event) = stream.next().await {
        if let ProviderEvent::TextDelta(text) = event.unwrap() {
            content.push_str(&text);
        }
    }
    assert_eq!(content, "Safe answer");
    server.finish().await;
    assert_eq!(original.reasoning_content.as_deref(), Some(secret));
    assert!(
        !server.received.lock().unwrap()[0]
            .body
            .to_string()
            .contains(secret)
    );
}
