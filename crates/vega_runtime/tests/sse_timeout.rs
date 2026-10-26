use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    AgentRequest, ChatMessage, ChatRole, OpenAiProvider, RetryPolicy, RuntimeEvent,
    RuntimeToolConfig, run_agent,
};

fn frame(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn timeout() -> String {
    frame(
        json!({"error":{"type":"server_error","code":"request_timeout","message":"private upstream detail"}}),
    )
}

fn tool(arguments: &str) -> String {
    frame(
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_read","type":"function","function":{"name":"read","arguments":arguments}}]},"finish_reason":null}]}),
    )
}

fn finish(reason: &str) -> String {
    frame(json!({"choices":[{"delta":{},"finish_reason":reason}]}))
}

fn done() -> String {
    format!(
        "{}{}data: [DONE]\n\n",
        frame(json!({"choices":[{"delta":{"content":"finished"}}]})),
        finish("stop")
    )
}

async fn exercise(responses: Vec<String>, retries: u32) -> vega_runtime::AgentOutcome {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for body in responses {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(20), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 8192];
            loop {
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buffer[..n]);
                if let Some(end) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nx-request-id: regression\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let root = std::env::temp_dir().join(format!("vega-sse-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("sample.txt"), "fixture").unwrap();
    let tools = vega_tools::Tools::new(&root).unwrap();
    let provider = OpenAiProvider::new(format!("http://{address}/v1"), "test-key")
        .unwrap()
        .with_retry_policy(RetryPolicy {
            max_retries: retries,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        });
    let request = AgentRequest {
        model: "test-model".into(),
        system_prompt: String::new(),
        history: vec![ChatMessage::new(ChatRole::User, "read sample.txt")],
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::default(),
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    };
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        run_agent(&provider, &tools, request, CancellationToken::new()),
    )
    .await
    .unwrap()
    .unwrap();
    server.await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
    outcome
}

fn retry_count(outcome: &vega_runtime::AgentOutcome) -> usize {
    outcome
        .events
        .iter()
        .filter(|event| matches!(event, RuntimeEvent::ResponseRetrying { .. }))
        .count()
}

fn assert_tool_lifecycle(outcome: &vega_runtime::AgentOutcome, completed_before_retry: bool) {
    let retry = outcome
        .events
        .iter()
        .position(|event| matches!(event, RuntimeEvent::ResponseRetrying { .. }))
        .unwrap();
    let running: Vec<_> = outcome
        .events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            matches!(event, RuntimeEvent::ToolCallRunning { .. }).then_some(index)
        })
        .collect();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0] < retry, completed_before_retry);
    let results: Vec<_> = outcome
        .events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            RuntimeEvent::ToolCallFinished(result)
            | RuntimeEvent::ToolReadFinished { result, .. } => Some((index, result)),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), if completed_before_retry { 2 } else { 1 });
    for (_, result) in &results {
        assert_eq!(result.status, vega_runtime::RuntimeToolStatus::Success);
        assert!(result.output.contains("fixture"));
    }
    assert!(!results[0].1.reused);
    assert_eq!(results[0].0 < retry, completed_before_retry);
    if completed_before_retry {
        assert!(results[1].0 > retry);
        assert!(results[1].1.reused);
    } else {
        assert!(!outcome.events[..retry].iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallProposed(_)
                | RuntimeEvent::ToolCallRunning { .. }
                | RuntimeEvent::ToolCallFinished(_)
                | RuntimeEvent::ToolReadFinished { .. }
        )));
    }
}

#[tokio::test]
async fn timeout_recovers_without_executing_partial_arguments() {
    let outcome = exercise(vec![tool("{\"path\":") + &timeout(), done()], 1).await;
    assert!(!outcome.failed);
    assert_eq!(outcome.final_text, "finished");
    assert_eq!(retry_count(&outcome), 1);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ToolCallProposed(_)))
    );
}

#[tokio::test]
async fn retry_discards_complete_but_uncommitted_tool_proposal() {
    let args = r#"{"path":"sample.txt","offset":null,"limit":null}"#;
    let outcome = exercise(
        vec![
            tool(args) + &finish("tool_calls") + &timeout(),
            tool(args) + &finish("tool_calls") + "data: [DONE]\n\n",
            done(),
        ],
        1,
    )
    .await;
    assert!(!outcome.failed);
    assert_eq!(retry_count(&outcome), 1);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_tool_lifecycle(&outcome, false);
}

#[tokio::test]
async fn retry_after_completed_tool_does_not_execute_it_again() {
    let args = r#"{"path":"sample.txt","offset":null,"limit":null}"#;
    let outcome = exercise(
        vec![
            tool(args) + &finish("tool_calls") + "data: [DONE]\n\n",
            timeout(),
            tool(args) + &finish("tool_calls") + "data: [DONE]\n\n",
            done(),
        ],
        1,
    )
    .await;
    assert!(!outcome.failed);
    assert_eq!(retry_count(&outcome), 1);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_tool_lifecycle(&outcome, true);
}

#[tokio::test]
async fn repeated_timeouts_exhaust_budget_without_tools() {
    let outcome = exercise(vec![timeout(), timeout()], 1).await;
    assert!(outcome.failed);
    assert_eq!(retry_count(&outcome), 1);
    assert_eq!(outcome.executed_tool_call_count, 0);
}

#[tokio::test]
async fn structured_unknown_and_mixed_errors_do_not_retry() {
    for error in [
        json!({"error":{"type":"server_error","code":"unknown","message":"request_timeout"}}),
        json!({"error":{"type":"server_error","code":"request_timeout","message":"detail"},"choices":[]}),
        json!({"error":{"type":"server_error","code":"request_timeout","message":42}}),
    ] {
        let outcome = exercise(vec![frame(error)], 1).await;
        assert!(outcome.failed);
        assert_eq!(retry_count(&outcome), 0);
        assert_eq!(outcome.executed_tool_call_count, 0);
    }
}

#[tokio::test]
async fn malformed_error_before_visible_output_uses_master_recovery_policy() {
    let outcome = exercise(vec![frame(json!({"error":"request_timeout"})), done()], 1).await;
    assert!(!outcome.failed);
    assert_eq!(retry_count(&outcome), 1);
    assert_eq!(outcome.executed_tool_call_count, 0);
}

#[tokio::test]
async fn malformed_error_after_visible_output_stays_terminal() {
    let body = frame(json!({"choices":[{"delta":{"content":"visible"}}]}))
        + &frame(json!({"error":"request_timeout"}));
    let outcome = exercise(vec![body], 1).await;
    assert!(outcome.failed);
    assert_eq!(retry_count(&outcome), 0);
    assert_eq!(outcome.executed_tool_call_count, 0);
}
