use super::*;

fn encoded(
    endpoint: &str,
    model: &str,
    messages: Vec<ChatMessage>,
    tools: Vec<ToolDefinition>,
) -> Value {
    OpenAiProvider::new(endpoint, "fixture-key")
        .expect("transport")
        .request_body(&ChatRequest {
            model: model.into(),
            messages,
            tools,
            max_tokens: Some(321),
            reasoning: None,
        })
        .expect("encoded request")
}

fn strict_tool() -> ToolDefinition {
    ToolDefinition {
        name: "inspect".into(),
        description: "Read structured data".into(),
        input_schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        strict: true,
    }
}

#[test]
fn real_encoder_resolves_cap_roles_usage_and_strict_from_actual_service_not_aliases() {
    let cases = [
        (
            "https://api.groq.com/openai/v1",
            "openai/gpt-oss-120b",
            "max_completion_tokens",
            "developer",
            true,
            true,
        ),
        (
            "https://api.together.ai/v1",
            "Qwen/Qwen3.5-9B",
            "max_tokens",
            "system",
            true,
            false,
        ),
        (
            "https://inference.baseten.co/v1",
            "moonshotai/Kimi-K2.6",
            "max_tokens",
            "system",
            true,
            true,
        ),
        (
            "https://api.z.ai/api/coding/paas/v4",
            "glm-5.3",
            "max_tokens",
            "system",
            true,
            true,
        ),
        (
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "max_tokens",
            "system",
            true,
            true,
        ),
        (
            "https://api.ant-ling.com/v1",
            "Ring-2.6-1T",
            "max_tokens",
            "system",
            true,
            true,
        ),
        (
            "https://proxy.example/v1",
            "unknown-model",
            "max_tokens",
            "system",
            true,
            true,
        ),
    ];
    for (endpoint, model, cap, role, usage, strict) in cases {
        let definition = strict_tool();
        let body = encoded(
            endpoint,
            model,
            vec![
                ChatMessage::new(ChatRole::System, "system text"),
                ChatMessage::new(ChatRole::User, "question"),
            ],
            vec![definition.clone()],
        );
        assert_eq!(body[cap], 321, "{endpoint}/{model}");
        assert!(
            body.get(if cap == "max_tokens" {
                "max_completion_tokens"
            } else {
                "max_tokens"
            })
            .is_none()
        );
        assert_eq!(body["messages"][0]["role"], role);
        assert_eq!(body["messages"][0]["content"], "system text");
        assert_eq!(body.get("stream_options").is_some(), usage);
        assert_eq!(
            body["tools"][0]["function"]
                .get("strict")
                .and_then(Value::as_bool),
            strict.then_some(true)
        );
        assert_eq!(
            body["tools"][0]["function"]["parameters"],
            definition.input_schema
        );
        assert!(body.get("store").is_none());
        assert!(body.get("temperature").is_none());
        assert_eq!(
            body.get("tool_stream").and_then(Value::as_bool),
            (endpoint == "https://api.z.ai/api/coding/paas/v4").then_some(true)
        );
    }
    for endpoint in [
        "https://api.groq.com.evil.example/openai/v1",
        "https://api.groq.com/openai/v1/extra",
        "https://api.groq.com/openai/v1?route=trusted",
        "https://alias@api.groq.com/openai/v1",
    ] {
        let body = encoded(
            endpoint,
            "openai/gpt-oss-120b",
            vec![ChatMessage::new(ChatRole::System, "system text")],
            vec![],
        );
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["max_tokens"], 321);
    }
}

#[test]
fn model_transport_overrides_and_cloudflare_placeholders_do_not_broaden_service_identity() {
    for endpoint in [
        "https://gateway.ai.cloudflare.com/v1/account-one/gateway-two/compat",
        "https://gateway.ai.cloudflare.com/v1/a/b/compat/",
    ] {
        let body = encoded(
            endpoint,
            "openai/gpt-oss-120b",
            vec![ChatMessage::new(ChatRole::System, "instructions")],
            vec![strict_tool()],
        );
        assert!(
            body["tools"][0]["function"].get("strict").is_none(),
            "{endpoint}"
        );
    }
    for endpoint in [
        "https://gateway.ai.cloudflare.com/v1/a/b/c/compat",
        "https://gateway.ai.cloudflare.com/v1//b/compat",
        "https://gateway.ai.cloudflare.com.evil/v1/a/b/compat",
    ] {
        assert_eq!(
            encoded(endpoint, "unknown", vec![], vec![strict_tool()])["tools"][0]["function"]["strict"],
            true
        );
    }
    let body = encoded(
        "https://api.together.ai/v1",
        "openai/gpt-oss-120b",
        vec![],
        vec![strict_tool()],
    );
    assert_eq!(body["max_tokens"], 321);
    assert!(body.get("max_completion_tokens").is_none());
    assert!(body["tools"][0]["function"].get("strict").is_none());
}

#[test]
fn complete_tool_history_without_current_definitions_keeps_empty_tools_and_image_observation_order()
{
    let mut assistant = ChatMessage::new(ChatRole::Assistant, "");
    assistant.tool_calls.push(crate::ChatToolCall {
        id: "historical".into(),
        name: "inspect".into(),
        input_json: "{\"path\":\"sample.txt\"}".into(),
    });
    let mut result = ChatMessage::new(ChatRole::Tool, "read result");
    result.tool_call_id = Some("historical".into());
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(1, 1)
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("fixture png");
    result
        .images
        .push(crate::ImageAttachment::from_bytes(png.into_inner()).expect("image"));
    let body = encoded(
        "https://proxy.example/v1",
        "unknown",
        vec![assistant, result, ChatMessage::new(ChatRole::Assistant, "")],
        vec![],
    );
    assert_eq!(body["tools"], json!([]));
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages[0]["tool_calls"][0]["id"], "historical");
    assert_eq!(messages[1]["role"], "tool");
    assert_eq!(messages[1]["tool_call_id"], "historical");
    assert_eq!(messages[2]["role"], "user");
    assert!(
        messages[2]["content"][0]["text"]
            .as_str()
            .expect("observation label")
            .contains("Untrusted external tool observation")
    );
    assert!(
        messages[2]["content"][1]["image_url"]["url"]
            .as_str()
            .expect("image url")
            .starts_with("data:image/png;base64,")
    );
    assert_eq!(messages[3]["role"], "assistant");
    assert_eq!(messages[3]["content"], "");
}

#[tokio::test]
async fn tools_are_aggregated_by_joint_index_and_id_and_unique_late_bindings() {
    let args = "{\"path\":\"sample.txt\",\"offset\":null,\"limit\":null}";
    let cases = [
        vec![
            json!({"id":"one","function":{"name":"read","arguments":args}}),
            json!({"id":"two","function":{"name":"read","arguments":args}}),
        ],
        vec![
            json!({"index":0,"function":{"name":"read","arguments":args}}),
            json!({"id":"one","index":0}),
            json!({"index":1,"id":"two","function":{"name":"read","arguments":args}}),
        ],
        vec![
            json!({"id":"one","function":{"name":"read","arguments":args}}),
            json!({"index":0,"id":"one"}),
            json!({"id":"two","index":1,"function":{"name":"read","arguments":args}}),
        ],
    ];
    for calls in cases {
        let mut script = Script::read("unused", json!({}));
        script.chunks[1] = json!({"choices":[{"delta":{"tool_calls":calls}}]});
        let (outcome, provider) = run_scripts(
            "https://proxy.example/v1",
            "unknown",
            vec![script, Script::final_reply(json!({}))],
        )
        .await;
        assert!(!outcome.failed);
        assert_eq!(outcome.executed_tool_call_count, 2);
        let bodies = provider.bodies();
        let calls = &tool_messages(&bodies[1])[0]["tool_calls"];
        assert_eq!(calls[0]["id"], "one");
        assert_eq!(calls[1]["id"], "two");
    }
    let mut script = Script::read("unused", json!({}));
    script.chunks = vec![
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"one","function":{"name":"read","arguments":"{\"path\":"}},{"index":1,"id":"two","function":{"name":"read","arguments":"{\"path\":"}}]}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"id":"two","function":{"arguments":"\"sample.txt\",\"offset\":null,\"limit\":null}"}},{"index":0,"function":{"arguments":"\"sample.txt\",\"offset\":null,\"limit\":null}"}}]}}]}),
        json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
    ];
    let (outcome, _) = run_scripts(
        "https://proxy.example/v1",
        "unknown",
        vec![script, Script::final_reply(json!({}))],
    )
    .await;
    assert!(!outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 2);
}

#[tokio::test]
async fn ambiguous_conflicting_and_invalid_tool_fragments_fail_before_approval() {
    let args = "{\"path\":\"sample.txt\",\"offset\":null,\"limit\":null}";
    for calls in [
        vec![
            json!({"index":0,"id":"one","function":{"name":"read","arguments":args}}),
            json!({"index":0,"id":"two"}),
        ],
        vec![
            json!({"index":0,"id":"same","function":{"name":"read","arguments":args}}),
            json!({"index":1,"id":"same","function":{"name":"read","arguments":args}}),
        ],
        vec![
            json!({"id":"one","function":{"name":"read","arguments":"{"}}),
            json!({"id":"two","function":{"name":"read","arguments":"{"}}),
            json!({"function":{"arguments":"}"}}),
        ],
        vec![json!({"index":0,"function":{"name":"read","arguments":args}})],
        vec![json!({"index":0,"id":"one","function":{"arguments":args}})],
        vec![
            json!({"index":0,"id":"one","function":{"name":"read","arguments":"{\"path\":\"sample.txt\""}}),
        ],
        vec![json!({"index":-1,"id":"one","function":{"name":"read","arguments":args}})],
    ] {
        let mut script = Script::read("unused", json!({}));
        script.chunks[1] = json!({"choices":[{"delta":{"tool_calls":calls}}]});
        let (outcome, _) = run_scripts(
            "https://proxy.example/v1",
            "unknown",
            vec![script, Script::final_reply(json!({}))],
        )
        .await;
        assert_failed_without_tools(&outcome);
    }
}

#[tokio::test]
async fn wire_finish_is_required_and_error_finishes_do_not_execute_buffered_tools() {
    for finish in [
        None,
        Some("unknown"),
        Some("content_filter"),
        Some("network_error"),
    ] {
        let mut script = Script::read("unsafe", json!({"reasoning_text":"partial raw"}));
        script.chunks.pop();
        if let Some(finish) = finish {
            script
                .chunks
                .push(json!({"choices":[{"delta":{},"finish_reason":finish}]}));
        }
        let (outcome, _) = run_scripts(
            "https://proxy.example/v1",
            "unknown",
            vec![script, Script::final_reply(json!({}))],
        )
        .await;
        assert_failed_without_tools(&outcome);
    }
    let mut script = Script::read("unsafe", json!({}));
    script.raw_payload = Some("{malformed SSE".into());
    let (outcome, _) = run_scripts(
        "https://proxy.example/v1",
        "unknown",
        vec![script, Script::final_reply(json!({}))],
    )
    .await;
    assert_failed_without_tools(&outcome);
    let mut script = Script::read("unsafe", json!({}));
    script.finalize = false;
    let (outcome, _) = run_scripts(
        "https://proxy.example/v1",
        "unknown",
        vec![script, Script::final_reply(json!({}))],
    )
    .await;
    assert_failed_without_tools(&outcome);
    let (outcome, _) = run_scripts(
        "https://proxy.example/v1",
        "unknown",
        vec![
            Script::read("valid", json!({})),
            Script::final_reply(json!({})),
        ],
    )
    .await;
    assert!(!outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 1);
}

#[tokio::test]
async fn usage_sources_preserve_total_input_and_cache_write_without_reasoning_double_count() {
    for (usage, choice) in [
        (
            json!({"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":30,"cache_write_tokens":7},"completion_tokens_details":{"reasoning_tokens":8}}),
            false,
        ),
        (
            json!({"prompt_tokens":100,"completion_tokens":20,"prompt_cache_hit_tokens":30,"prompt_cache_miss_tokens":70,"prompt_tokens_details":{"cache_write_tokens":7}}),
            true,
        ),
        (
            json!({"prompt_tokens":100,"completion_tokens":20,"cached_tokens":30,"cache_write_tokens":7}),
            false,
        ),
    ] {
        let mut script = Script::final_reply(json!({}));
        if choice {
            script.chunks[0]["choices"][0]["usage"] = usage;
        } else {
            script.chunks[0]["usage"] = usage;
        }
        let (outcome, _) = run_scripts("https://proxy.example/v1", "unknown", vec![script]).await;
        assert!(!outcome.failed);
        let usage = outcome
            .events
            .iter()
            .find_map(|event| match event {
                RuntimeEvent::UsageUpdated { usage, .. } => Some(usage),
                _ => None,
            })
            .expect("usage event");
        assert_eq!(usage.input, 100);
        assert_eq!(usage.output, 20);
        assert_eq!(usage.cache_read, Some(30));
        assert_eq!(usage.cache_write, Some(7));
    }
}

#[tokio::test]
async fn conflicting_and_invalid_usage_fails_before_any_tool_side_effect() {
    for usage in [
        json!({"prompt_tokens":100,"completion_tokens":20,"cached_tokens":30,"prompt_cache_hit_tokens":31}),
        json!({"prompt_tokens":100,"completion_tokens":20,"cached_tokens":101}),
        json!({"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cache_write_tokens":101}}),
        json!({"prompt_tokens":100,"completion_tokens":20,"cached_tokens":-1}),
        json!({"prompt_tokens":"100","completion_tokens":20}),
    ] {
        let mut script = Script::read("unsafe", json!({}));
        script.chunks[2]["usage"] = usage;
        let (outcome, _) = run_scripts(
            "https://proxy.example/v1",
            "unknown",
            vec![script, Script::final_reply(json!({}))],
        )
        .await;
        assert_failed_without_tools(&outcome);
    }
    let mut script = Script::read("unsafe", json!({}));
    script.chunks[2]["usage"] = json!({"prompt_tokens":10,"completion_tokens":2});
    script.chunks[2]["choices"][0]["usage"] = json!({"prompt_tokens":11,"completion_tokens":2});
    let (outcome, _) = run_scripts(
        "https://proxy.example/v1",
        "unknown",
        vec![script, Script::final_reply(json!({}))],
    )
    .await;
    assert_failed_without_tools(&outcome);
}

struct ServerWorker(tokio::task::JoinHandle<()>);

impl Drop for ServerWorker {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn serve(responses: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<Value>>>, ServerWorker) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned network fixture");
    let endpoint = format!("http://{}/v1", listener.local_addr().expect("address"));
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let captured = bodies.clone();
    let worker = tokio::spawn(async move {
        for (status, response) in responses {
            let (mut socket, _) = listener.accept().await.expect("fixture connection");
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            let split = loop {
                let read = socket.read(&mut buffer).await.expect("request headers");
                assert_ne!(read, 0);
                bytes.extend_from_slice(&buffer[..read]);
                if let Some(split) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                    break split + 4;
                }
            };
            let headers = std::str::from_utf8(&bytes[..split]).expect("headers");
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().expect("length"))
                })
                .expect("body length");
            while bytes.len() < split + length {
                let read = socket.read(&mut buffer).await.expect("request body");
                assert_ne!(read, 0);
                bytes.extend_from_slice(&buffer[..read]);
            }
            captured
                .lock()
                .expect("capture")
                .push(serde_json::from_slice(&bytes[split..split + length]).expect("request json"));
            socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).as_bytes()).await.expect("fixture response");
        }
    });
    (endpoint, bodies, ServerWorker(worker))
}

#[tokio::test]
async fn real_http_sse_accepts_finished_eof_and_done_and_retries_only_before_stream_consumption() {
    for done in [false, true] {
        let read = Script::read("http-read", json!({"reasoning_text":"http original raw"}));
        let mut streamed = read
            .chunks
            .iter()
            .map(|chunk| format!("data: {chunk}\n\n"))
            .collect::<String>();
        if done {
            streamed.push_str("data: [DONE]\n\n");
        }
        let final_reply = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            Script::final_reply(json!({})).chunks[0]
        );
        let (endpoint, bodies, mut worker) = serve(vec![
            (503, "retry fixture".into()),
            (200, streamed),
            (200, final_reply),
        ])
        .await;
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = attempts.clone();
        let provider = OpenAiProvider::new(endpoint, "http fixture key")
            .expect("transport")
            .with_retry_policy(crate::RetryPolicy {
                max_retries: 1,
                base_delay: Duration::ZERO,
                max_delay: Duration::ZERO,
            })
            .with_pre_attempt_guard(move |_| {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            });
        let (_root, tools) = fixture();
        let outcome = tokio::time::timeout(
            Duration::from_secs(10),
            crate::run_agent(
                &provider,
                &tools,
                request("unknown"),
                CancellationToken::new(),
            ),
        )
        .await
        .expect("network fixture timeout")
        .expect("runtime");
        assert!(!outcome.failed);
        assert_eq!(outcome.executed_tool_call_count, 1);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
        {
            let bodies = bodies.lock().expect("captures");
            assert_eq!(bodies[0], bodies[1]);
            assert_eq!(
                tool_messages(&bodies[2])[0]["reasoning_text"],
                "http original raw"
            );
        }
        tokio::time::timeout(Duration::from_secs(2), &mut worker.0)
            .await
            .expect("fixture worker timeout")
            .expect("fixture worker");
    }
}
