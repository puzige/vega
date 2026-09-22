use super::*;
use serde_json::{Value, json};
use std::io::Read;

#[tokio::test]
async fn i71_responses_http_controller_tool_replay_and_private_history() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::<Value>::new();
        for round in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(15);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("owned server: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
            assert!(headers.starts_with("POST /responses HTTP/1.1"));
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() < header_end + length {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
            }
            requests.push(serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap());
            let reasoning = json!({"type":"reasoning","id":format!("rs-{round}"),"summary":[{"type":"summary_text","text":"owned summary"}],"encrypted_content":"opaque-owned-replay"});
            let mut events = vec![
                json!({"type":"response.reasoning_text.delta","item_id":format!("rs-{round}"),"content_index":0,"delta":"raw owned thinking"}),
                json!({"type":"response.reasoning_summary_text.delta","item_id":format!("rs-{round}"),"summary_index":0,"delta":"owned summary"}),
                json!({"type":"response.reasoning_summary_text.done","item_id":format!("rs-{round}"),"summary_index":0,"text":"owned summary"}),
                json!({"type":"response.output_item.done","output_index":0,"item":reasoning}),
            ];
            let output = if round == 0 {
                let call = json!({"type":"function_call","id":"fc-1","call_id":"read-owned","name":"read","arguments":"{\"path\":\"lib.rs\"}"});
                events.extend([
                    json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"fc-1","call_id":"read-owned","name":"read","arguments":""}}),
                    json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"path\":"}),
                    json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"\"lib.rs\"}"}),
                    json!({"type":"response.output_item.done","output_index":1,"item":call}),
                ]);
                vec![reasoning, call]
            } else {
                events.push(json!({"type":"response.output_text.delta","item_id":"message","content_index":0,"delta":"Found the TODO."}));
                vec![
                    reasoning,
                    json!({"type":"message","id":"message","role":"assistant","content":[{"type":"output_text","text":"Found the TODO."}]}),
                ]
            };
            events.push(json!({"type":"response.completed","response":{"status":"completed","output":output,"usage":{"input_tokens":12,"output_tokens":3,"input_tokens_details":{"cached_tokens":2}}}}));
            let body = events
                .iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect::<String>();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    let provider = vega_runtime::OpenAiProvider::new(format!("http://{address}"), "owned-key")
        .unwrap()
        .with_responses_api(true);
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Read lib.rs",
        "Owned system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert_eq!(run.content, "Found the TODO.");
    assert_eq!(
        run.events
            .iter()
            .filter(|event| matches!(event, ConversationEvent::SummaryDelta { .. }))
            .count(),
        2
    );
    assert!(
        run.events
            .iter()
            .any(|event| matches!(event, ConversationEvent::ThinkingDelta { .. }))
    );
    assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallFinished { result, .. } if result.status == ToolCallStatus::Success && result.output.contains("TODO"))));
    let requests = server.join().unwrap();
    assert_eq!(requests[0]["reasoning"]["summary"], "auto");
    assert_eq!(requests[0]["store"], false);
    let input = requests[1]["input"].as_array().unwrap();
    assert!(
        input.iter().any(|item| item["type"] == "reasoning"
            && item["encrypted_content"] == "opaque-owned-replay")
    );
    assert!(
        input
            .iter()
            .any(|item| item["type"] == "function_call_output"
                && item["call_id"] == "read-owned"
                && item["output"].as_str().unwrap().contains("TODO"))
    );
    let persisted: String = store
        .conn()
        .query_row(
            "SELECT content FROM messages WHERE id=?1",
            [&run.assistant_message_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted, "Found the TODO.");
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let history = crate::history::latest_history_page(&reopened, "thread-1", 50).unwrap();
    assert!(!format!("{history:?}").contains("owned summary"));
    assert!(!format!("{history:?}").contains("opaque-owned-replay"));
    let usage: (i64, i64, i64) = store
        .conn()
        .query_row(
            "SELECT SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens) FROM token_usage",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(usage, (24, 6, 4));
}
