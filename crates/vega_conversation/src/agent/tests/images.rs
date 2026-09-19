use super::*;
use crate::types::ImageAttachment;
use base64::Engine;
use std::io::Read;

fn fixture() -> ImageAttachment {
    let image = image::RgbImage::from_pixel(4, 3, image::Rgb([200, 20, 70]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    ImageAttachment::from_bytes(encoded.into_inner()).unwrap()
}

#[tokio::test]
async fn issue63_controller_http_tool_round_and_restart_preserve_exact_images() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let mut recorded = Vec::new();
        for round in 0..3 {
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
                    Err(error) => panic!("bounded owned server accept failed: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let mut buffer = [0; 4096];
                let n = stream.read(&mut buffer).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
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
                let n = stream.read(&mut buffer).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            recorded.push(
                serde_json::from_slice::<serde_json::Value>(
                    &bytes[header_end..header_end + length],
                )
                .unwrap(),
            );
            let payload = if round == 0 {
                serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"read-image-context","type":"function","function":{"name":"read","arguments":"{\"path\":\"lib.rs\"}"}}]},"finish_reason":"tool_calls"}]})
            } else {
                serde_json::json!({"choices":[{"delta":{"content":"Image received."},"finish_reason":"stop"}]})
            };
            let response = format!("data: {payload}\n\ndata: [DONE]\n\n");
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
        }
        recorded
    });
    let provider =
        vega_runtime::OpenAiProvider::new(format!("http://{address}"), "owned-test-only").unwrap();
    let image = fixture();
    let mut started = false;
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "",
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |event| {
            if matches!(event, ConversationEvent::MessageStarted { .. }) {
                started = true;
                let count: i64 = store
                    .conn()
                    .query_row("SELECT COUNT(*) FROM image_attachments", [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(count, 1, "durable before acknowledgment");
            }
            Ok(())
        },
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        vec![image.clone()],
    )
    .await
    .unwrap();
    assert!(started && !run.failed);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let page = crate::history::latest_history_page(&reopened, "thread-1", 50).unwrap();
    assert!(page.entries.iter().any(|entry| matches!(entry, crate::history::HistoryEntry::UserImages { images, .. } if images == &vec![image.clone()])));
    assert!(
        crate::history::latest_history_page(&reopened, "not-this-thread", 50)
            .unwrap()
            .entries
            .is_empty()
    );
    run_thread_task(
        &reopened,
        &provider,
        &tools,
        "thread-1",
        "Next text only",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let recorded = server.join().unwrap();
    assert_eq!(recorded.len(), 3);
    let expected = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(image.bytes())
    );
    for request in &recorded {
        assert_eq!(
            request["messages"][1]["content"][0]["image_url"]["url"],
            expected
        );
        assert_eq!(request["messages"][0]["content"], "system");
    }
    assert!(
        recorded[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool")
    );
    assert_eq!(
        recorded[2]["messages"].as_array().unwrap().last().unwrap()["content"],
        "Next text only"
    );
}

#[tokio::test]
async fn issue63_transaction_failure_leaves_no_half_turn_and_invalid_batch_never_starts() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(Vec::new());
    store.conn().execute_batch("CREATE TRIGGER reject_image BEFORE INSERT ON image_attachments BEGIN SELECT RAISE(ABORT, 'owned fault'); END;").unwrap();
    let mut started = false;
    let result = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "text",
        "",
        CancellationToken::new(),
        &RejectPermissionHook,
        |event| {
            started |= matches!(event, ConversationEvent::MessageStarted { .. });
            Ok(())
        },
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        vec![fixture()],
    )
    .await;
    assert!(result.is_err());
    assert!(!started);
    for table in ["messages", "image_attachments"] {
        let count: i64 = store
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
    store
        .conn()
        .execute_batch("DROP TRIGGER reject_image")
        .unwrap();
    assert!(
        run_thread_task_with_images_and_reasoning(
            &store,
            &provider,
            &tools,
            "thread-1",
            "",
            "",
            CancellationToken::new(),
            &RejectPermissionHook,
            |_| Ok(()),
            PersistenceActorConfig::default(),
            None,
            None,
            None,
            vec![fixture(); 5]
        )
        .await
        .is_err()
    );
    assert!(provider.requests().is_empty());
}

#[tokio::test]
async fn issue63_text_history_system_rows_stay_omitted_and_image_history_fails_closed() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    store.conn().execute_batch("INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at) VALUES ('system-history','thread-1',1,'system','text','not injected','done',1);").unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("ok".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "text only",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        provider.requests()[0]
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>(),
        vec!["system", "text only"]
    );
    // R5: exceed aggregate budget using schema-valid blobs. Length preflight
    // must reject before attempting image decode or copying the fifth blob.
    for index in 0..5 {
        let id = format!("budget-{index}");
        store.conn().execute("INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at) VALUES (?1,'thread-1',?2,'user','text','','done',1)", (id.as_str(), index + 10)).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO image_attachments VALUES (?1,0,zeroblob(8388608))",
                [&id],
            )
            .unwrap();
    }
    let error = crate::history::latest_history_page(&store, "thread-1", 50).unwrap_err();
    assert!(error.to_string().contains("32 MiB"));
    assert!(
        run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "too much history",
            "system",
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    let latest = messages::recent(store.conn(), "thread-1", 50).unwrap();
    assert!(
        !latest
            .iter()
            .any(|message| message.content == "too much history")
    );
}

#[tokio::test]
async fn issue63_provider_rejection_keeps_images_and_message_delete_cascades() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::Error {
        status: Some(400),
        message: "owned vision unsupported".into(),
        retryable: false,
    }]);
    let image = fixture();
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "describe",
        "",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        vec![image.clone()],
    )
    .await
    .unwrap();
    assert!(run.failed);
    let page = crate::history::latest_history_page(&store, "thread-1", 50).unwrap();
    assert!(page.entries.iter().any(|entry| matches!(entry, crate::history::HistoryEntry::UserImages {images, ..} if images == &vec![image.clone()])));
    store
        .conn()
        .execute("DELETE FROM messages WHERE id = ?1", [&run.user_message_id])
        .unwrap();
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM image_attachments", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}
