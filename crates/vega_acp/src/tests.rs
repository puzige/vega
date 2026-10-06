use crate::{
    Connection, Error, Event, LaunchConfig, MAX_BATCH_ELEMENTS, MAX_EVENT_BYTES, MAX_EVENT_COUNT,
    MAX_FRAME_BYTES, MAX_PENDING_INBOUND, MAX_PENDING_OUTBOUND, MAX_SEEN_INBOUND_REQUEST_ID_BYTES,
    MAX_SEEN_INBOUND_REQUEST_IDS, PermissionOption, PermissionOutcome, PromptResult, RequestId,
};
use serde_json::{Value, json};
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, duplex, split},
    task::JoinHandle,
    time::timeout,
};

async fn connection_pair(size: usize) -> (Connection, DuplexStream) {
    let (client, peer) = duplex(size);
    let (reader, writer) = split(client);
    (Connection::from_io(reader, writer, None), peer)
}

async fn read_line<R: AsyncRead + Unpin>(reader: &mut R) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        assert_eq!(reader.read(&mut byte).await.unwrap(), 1);
        if byte[0] == b'\n' {
            return bytes;
        }
        bytes.push(byte[0]);
    }
}

async fn read_json<R: AsyncRead + Unpin>(reader: &mut R) -> Value {
    serde_json::from_slice(&read_line(reader).await).unwrap()
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) {
    writer
        .write_all(&serde_json::to_vec(value).unwrap())
        .await
        .unwrap();
    writer.write_all(b"\n").await.unwrap();
}

fn permission_request(id: Value) -> Value {
    json!({
        "jsonrpc":"2.0",
        "id":id,
        "method":"session/request_permission",
        "params":{"sessionId":"s","toolCall":{"toolCallId":format!("call-{id}")},"options":[{"optionId":"allow","name":"Allow"}]}
    })
}

fn unknown_request(id: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"unsupported/method","params":{}})
}

fn prompt_request_value(id: u64, text: &str) -> Value {
    json!({
        "jsonrpc":"2.0",
        "id":id,
        "method":"session/prompt",
        "params":{"sessionId":"s","prompt":[{"type":"text","text":text}]}
    })
}

async fn initialize_peer(peer: &mut DuplexStream) {
    let request = read_json(peer).await;
    assert_eq!(request["method"], "initialize");
    assert_eq!(request["params"]["protocolVersion"], 1);
    assert_eq!(request["params"]["clientCapabilities"], json!({}));
    write_frame(
        peer,
        &json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": {"loadSession": true, "resumeSession": true},
                "authMethods": [],
                "agentInfo": {"name": "scripted", "version": "1"}
            }
        }),
    )
    .await;
}

fn launch_config(executable: &str, cwd: &str, args: Vec<String>) -> LaunchConfig {
    LaunchConfig::new(PathBuf::from(executable), args, PathBuf::from(cwd))
}

#[tokio::test]
async fn c1_01_launch_validation_precedes_the_spawn_seam() {
    let invalid = [
        launch_config("codex-acp", "/tmp", Vec::new()),
        launch_config("/usr/bin/codex-acp", "workspace", Vec::new()),
        launch_config("/usr/bin/codex-acp", "/tmp", vec![String::from("x"); 129]),
        launch_config("/usr/bin/codex-acp", "/tmp", vec!["x".repeat(65_537)]),
    ];
    for config in invalid {
        let spawned = Arc::new(AtomicBool::new(false));
        let marker = Arc::clone(&spawned);
        let result = Connection::validate_before_spawn(&config, move || {
            marker.store(true, Ordering::SeqCst);
            Ok(())
        });
        assert!(matches!(result, Err(Error::InvalidLaunchConfiguration)));
        assert!(!spawned.load(Ordering::SeqCst));
    }
    let valid = launch_config("/usr/bin/codex-acp", "/tmp", vec![String::from("--stdio")]);
    assert!(valid.validate().is_ok());
    assert!(
        launch_config("/usr/bin/codex-acp", "/tmp", vec![String::from("x"); 128])
            .validate()
            .is_ok()
    );
    assert!(
        launch_config(
            "/usr/bin/codex-acp",
            "/tmp",
            vec![String::from("x").repeat(65_536)]
        )
        .validate()
        .is_ok()
    );
}

#[tokio::test]
async fn c1_02_initialize_negotiates_v1_and_preserves_capabilities() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move { initialize_peer(&mut peer).await });
    let initialized = connection.initialize().await.unwrap();
    assert_eq!(initialized.protocol_version, 1);
    assert_eq!(
        initialized.agent_capabilities,
        json!({"loadSession": true, "resumeSession": true})
    );
    assert_eq!(initialized.auth_methods, json!([]));
    server.await.unwrap();
}

#[tokio::test]
async fn c1_02_rejects_unsupported_protocol_before_creating_a_session() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        let request = read_json(&mut peer).await;
        assert_eq!(request["params"]["protocolVersion"], 1);
        write_frame(
            &mut peer,
            &json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": {"protocolVersion": 2, "agentCapabilities": {}, "authMethods": []}
            }),
        )
        .await;
        let mut byte = [0u8; 1];
        assert_eq!(
            timeout(Duration::from_millis(40), peer.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    assert!(matches!(
        connection.initialize().await,
        Err(Error::UnsupportedProtocolVersion { offered: 2 })
    ));
    assert!(matches!(
        connection.new_session("/workspace").await,
        Err(Error::NotInitialized)
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn c1_02_malformed_initialize_result_fails_connection_before_session_creation() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        let request = read_json(&mut peer).await;
        write_frame(
            &mut peer,
            &json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": {
                    "protocolVersion": 1,
                    "agentCapabilities": [],
                    "authMethods": []
                }
            }),
        )
        .await;
        let mut byte = [0u8; 1];
        assert_eq!(
            timeout(Duration::from_secs(2), peer.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    assert!(matches!(
        connection.initialize().await,
        Err(Error::InvalidResult)
    ));
    assert_eq!(
        timeout(
            Duration::from_millis(100),
            connection.wait_for_terminal_for_test()
        )
        .await
        .unwrap()
        .unwrap(),
        Error::InvalidResult
    );
    assert!(matches!(
        connection.new_session("/workspace").await,
        Err(Error::NotInitialized)
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn c1_03_session_operations_preserve_paths_ids_and_mode() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        for (method, session_id, expected_cwd) in [
            ("session/new", "new-session", "/project/new"),
            ("session/load", "loaded-session", "/project/load"),
            ("session/resume", "resumed-session", "/project/resume"),
        ] {
            let request = read_json(&mut peer).await;
            assert_eq!(request["method"], method);
            assert_eq!(request["params"]["cwd"], expected_cwd);
            assert_eq!(request["params"]["mcpServers"], json!([]));
            write_frame(
                &mut peer,
                &json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "result": {"sessionId": session_id, "configOptions": []}
                }),
            )
            .await;
        }
        let mode = read_json(&mut peer).await;
        assert_eq!(mode["method"], "session/set_mode");
        assert_eq!(mode["params"]["sessionId"], "new-session");
        assert_eq!(mode["params"]["modeId"], "workspace-write");
        write_frame(
            &mut peer,
            &json!({"jsonrpc": "2.0", "id": mode["id"], "result": {"modeId": "workspace-write"}}),
        )
        .await;
    });
    connection.initialize().await.unwrap();
    assert_eq!(
        connection
            .new_session("/project/new")
            .await
            .unwrap()
            .session_id,
        "new-session"
    );
    assert_eq!(
        connection
            .load_session("loaded-session", "/project/load")
            .await
            .unwrap()
            .session_id,
        "loaded-session"
    );
    assert_eq!(
        connection
            .resume_session("resumed-session", "/project/resume")
            .await
            .unwrap()
            .session_id,
        "resumed-session"
    );
    connection
        .set_mode("new-session", "workspace-write")
        .await
        .unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn c1_04_prompt_updates_keep_order_and_completion_returns_terminal_reason() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        let request = read_json(&mut peer).await;
        assert_eq!(request["method"], "session/prompt");
        assert_eq!(
            request["params"]["prompt"],
            json!([{"type":"text","text":"hello"}])
        );
        for text in ["one", "two"] {
            write_frame(
                &mut peer,
                &json!({
                    "jsonrpc":"2.0",
                    "method":"session/update",
                    "params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}}
                }),
            )
            .await;
        }
        write_frame(
            &mut peer,
            &json!({"jsonrpc":"2.0","id":request["id"],"result":{"stopReason":"end_turn"}}),
        )
        .await;
    });
    connection.initialize().await.unwrap();
    let completion = connection.prompt("s-1", "hello").await.unwrap();
    let first = connection.recv_event().await.unwrap().unwrap();
    let second = connection.recv_event().await.unwrap().unwrap();
    assert!(matches!(first, Event::Notification { ref method, .. } if method == "session/update"));
    assert!(matches!(second, Event::Notification { ref method, .. } if method == "session/update"));
    let result: PromptResult = completion.wait().await.unwrap();
    assert_eq!(result.stop_reason, "end_turn");
    server.await.unwrap();
}

#[tokio::test]
async fn c1_05_cancel_sends_notification_without_completing_the_prompt() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        let prompt = read_json(&mut peer).await;
        assert_eq!(prompt["method"], "session/prompt");
        let cancel = read_json(&mut peer).await;
        assert_eq!(cancel["method"], "session/cancel");
        assert_eq!(cancel["params"]["sessionId"], "s-2");
        tokio::time::sleep(Duration::from_millis(50)).await;
        write_frame(
            &mut peer,
            &json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"cancelled"}}),
        )
        .await;
    });
    connection.initialize().await.unwrap();
    let completion = connection.prompt("s-2", "wait").await.unwrap();
    connection.cancel("s-2").await.unwrap();
    let mut waiter: JoinHandle<Result<PromptResult, Error>> = tokio::spawn(completion.wait());
    assert!(
        timeout(Duration::from_millis(20), &mut waiter)
            .await
            .is_err()
    );
    server.await.unwrap();
    assert_eq!(waiter.await.unwrap().unwrap().stop_reason, "cancelled");
}

#[tokio::test]
async fn c1_06_permission_projection_and_response_preserve_original_options() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        write_frame(
            &mut peer,
            &json!({
                "jsonrpc":"2.0",
                "id":71,
                "method":"session/request_permission",
                "params":{
                    "sessionId":"s-3",
                    "toolCall":{"toolCallId":"call-1","title":"Run command","kind":"execute","rawInput":{"command":"ls"}},
                    "options":[
                        {"optionId":"allow_once","name":"Allow once","kind":"allow_once"},
                        {"optionId":"allow_always","name":"Always allow","kind":"allow_always"},
                        {"optionId":"reject_once","name":"Reject","kind":"reject_once"}
                    ]
                }
            }),
        )
        .await;
        let response = read_json(&mut peer).await;
        assert_eq!(response["id"], 71);
        assert_eq!(response["result"]["outcome"]["outcome"], "selected");
        assert_eq!(response["result"]["outcome"]["optionId"], "allow_always");
        tokio::time::sleep(Duration::from_millis(50)).await;
    });
    connection.initialize().await.unwrap();
    let Some(Event::PermissionRequest(request)) = connection.recv_event().await.unwrap() else {
        panic!("expected permission request");
    };
    assert_eq!(request.tool_call["title"], "Run command");
    assert_eq!(
        request.options,
        vec![
            PermissionOption {
                option_id: String::from("allow_once"),
                name: String::from("Allow once"),
                kind: Some(String::from("allow_once"))
            },
            PermissionOption {
                option_id: String::from("allow_always"),
                name: String::from("Always allow"),
                kind: Some(String::from("allow_always"))
            },
            PermissionOption {
                option_id: String::from("reject_once"),
                name: String::from("Reject"),
                kind: Some(String::from("reject_once"))
            },
        ]
    );
    connection
        .respond_permission(request.request_id.clone(), "allow_always")
        .await
        .unwrap();
    assert!(matches!(
        connection
            .respond_permission(request.request_id, "allow_once")
            .await,
        Err(Error::PermissionRequestClosed)
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn c1_07_out_of_order_responses_resolve_only_their_matching_waiters() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        let first = read_json(&mut peer).await;
        let second = read_json(&mut peer).await;
        assert_ne!(first["id"], second["id"]);
        assert_eq!(first["params"]["modeId"], "first");
        assert_eq!(second["params"]["modeId"], "second");
        write_frame(
            &mut peer,
            &json!({"jsonrpc":"2.0","id":second["id"],"result":{"modeId":second["params"]["modeId"]}}),
        )
        .await;
        write_frame(
            &mut peer,
            &json!({"jsonrpc":"2.0","id":first["id"],"result":{"modeId":first["params"]["modeId"]}}),
        )
        .await;
    });
    connection.initialize().await.unwrap();
    let (first, second) = tokio::join!(
        connection.set_mode("s-4", "first"),
        connection.set_mode("s-4", "second")
    );
    assert_eq!(first.unwrap(), json!({"modeId":"first"}));
    assert_eq!(second.unwrap(), json!({"modeId":"second"}));
    server.await.unwrap();
}

#[tokio::test]
async fn c1_07_dropping_enqueued_request_fails_connection_closed() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let (request_sender, request_receiver) = tokio::sync::oneshot::channel();
    let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        request_sender.send(read_json(&mut peer).await).unwrap();
        release_receiver.await.unwrap();
    });
    connection.initialize().await.unwrap();
    let connection = Arc::new(connection);
    let request_connection = Arc::clone(&connection);
    let request =
        tokio::spawn(async move { request_connection.set_mode("s", "workspace-write").await });
    let request_message = timeout(Duration::from_secs(2), request_receiver)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(request_message["method"], "session/set_mode");
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());

    assert_eq!(
        timeout(
            Duration::from_millis(100),
            connection.wait_for_terminal_for_test()
        )
        .await
        .unwrap()
        .unwrap(),
        Error::RequestAbandoned
    );
    assert!(matches!(
        connection.set_mode("s", "another-mode").await,
        Err(Error::RequestAbandoned)
    ));
    release_sender.send(()).unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn c1_07_pending_outbound_limit_is_sixteen() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let connection = Arc::new(connection);
    let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
    let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        let mut requests = Vec::new();
        for _ in 0..MAX_PENDING_OUTBOUND {
            requests.push(read_json(&mut peer).await);
        }
        ready_sender.send(requests.clone()).unwrap();
        release_receiver.await.unwrap();
        for request in requests {
            write_frame(
                &mut peer,
                &json!({"jsonrpc":"2.0","id":request["id"],"result":{"modeId":request["params"]["modeId"]}}),
            )
            .await;
        }
    });
    connection.initialize().await.unwrap();
    let handles = (0..MAX_PENDING_OUTBOUND)
        .map(|index| {
            let connection = Arc::clone(&connection);
            tokio::spawn(async move { connection.set_mode("s", &format!("mode-{index}")).await })
        })
        .collect::<Vec<_>>();
    let requests = timeout(Duration::from_secs(2), ready_receiver)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), MAX_PENDING_OUTBOUND);
    assert!(matches!(
        connection.set_mode("s", "overflow").await,
        Err(Error::TooManyPendingRequests)
    ));
    release_sender.send(()).unwrap();
    for handle in handles {
        assert!(handle.await.unwrap().is_ok());
    }
    server.await.unwrap();
}

async fn expect_transport_failure(frame: Vec<u8>) {
    let (connection, mut peer) = connection_pair(2 * MAX_FRAME_BYTES + 16).await;
    let writer = tokio::spawn(async move {
        peer.write_all(&frame).await.unwrap();
        peer.shutdown().await.unwrap();
    });
    assert!(connection.recv_event().await.is_err());
    writer.await.unwrap();
}

#[tokio::test]
async fn c1_08_malformed_frames_batches_ids_and_eof_fail_closed() {
    expect_transport_failure(vec![0xff, b'\n']).await;
    expect_transport_failure(b"{bad json}\n".to_vec()).await;
    expect_transport_failure(b"{\"jsonrpc\":\"2.0\"}".to_vec()).await;
    expect_transport_failure(vec![b'a'; MAX_FRAME_BYTES + 1]).await;
    expect_transport_failure(b"[]\n".to_vec()).await;
    expect_transport_failure(
        b"{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":\"scalar\"}\n".to_vec(),
    )
    .await;
    expect_transport_failure(
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"session/update\",\"result\":{}}\n".to_vec(),
    )
    .await;
    let over_batch = format!(
        "{}\n",
        serde_json::to_string(&vec![Value::Null; MAX_BATCH_ELEMENTS + 1]).unwrap()
    );
    expect_transport_failure(over_batch.into_bytes()).await;
    expect_transport_failure(
        b"[{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{}},null]\n".to_vec(),
    )
    .await;
    expect_transport_failure(b"{\"jsonrpc\":\"2.0\",\"id\":700,\"result\":{}}\n".to_vec()).await;
    let (connection, peer) = connection_pair(65_536).await;
    drop(peer);
    assert!(connection.recv_event().await.is_err());
}

#[tokio::test]
async fn c1_08_malformed_batch_dispatches_no_valid_prefix() {
    let (connection, mut peer) = connection_pair(65_536).await;
    write_frame(
        &mut peer,
        &json!([
            {"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"x"}}},
            {"jsonrpc":"2.0","method":"session/update","params":"scalar"}
        ]),
    )
    .await;
    assert!(connection.recv_event().await.is_err());
}

#[tokio::test]
async fn c1_08_accepts_a_line_at_the_exact_frame_limit() {
    let mut message = json!({
        "jsonrpc":"2.0",
        "method":"session/update",
        "params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":""}}}
    });
    let base_len = serde_json::to_vec(&message).unwrap().len();
    let text = "x".repeat(MAX_FRAME_BYTES - base_len);
    message["params"]["update"]["content"]["text"] = Value::String(text.clone());
    let frame = serde_json::to_vec(&message).unwrap();
    assert_eq!(frame.len(), MAX_FRAME_BYTES);
    let (connection, mut peer) = connection_pair(MAX_FRAME_BYTES + 1).await;
    peer.write_all(&frame).await.unwrap();
    peer.write_all(b"\n").await.unwrap();
    let Some(Event::Notification { params, .. }) = connection.recv_event().await.unwrap() else {
        panic!("expected exact-limit event");
    };
    assert_eq!(
        params["update"]["content"]["text"].as_str().unwrap().len(),
        text.len()
    );
}

#[tokio::test]
async fn c1_08_accepts_an_outgoing_line_at_the_exact_frame_limit() {
    let base_len = serde_json::to_vec(&prompt_request_value(2, ""))
        .unwrap()
        .len();
    let text = "x".repeat(MAX_FRAME_BYTES - base_len);
    let text_len = text.len();
    let request_value = prompt_request_value(2, &text);
    assert_eq!(
        serde_json::to_vec(&request_value).unwrap().len(),
        MAX_FRAME_BYTES
    );
    let (connection, mut peer) = connection_pair(2 * MAX_FRAME_BYTES).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        let request = read_json(&mut peer).await;
        assert_eq!(
            request["params"]["prompt"][0]["text"]
                .as_str()
                .unwrap()
                .len(),
            text_len
        );
        write_frame(
            &mut peer,
            &json!({"jsonrpc":"2.0","id":request["id"],"result":{"stopReason":"end_turn"}}),
        )
        .await;
    });
    connection.initialize().await.unwrap();
    assert_eq!(
        connection
            .prompt("s", &text)
            .await
            .unwrap()
            .wait()
            .await
            .unwrap()
            .stop_reason,
        "end_turn"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn c1_08_rejects_an_outgoing_line_over_the_frame_limit() {
    let base_len = serde_json::to_vec(&prompt_request_value(2, ""))
        .unwrap()
        .len();
    let text = "x".repeat(MAX_FRAME_BYTES - base_len + 1);
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        let mut byte = [0u8; 1];
        assert_eq!(
            timeout(Duration::from_secs(2), peer.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    connection.initialize().await.unwrap();
    assert!(matches!(
        connection.prompt("s", &text).await,
        Err(Error::FrameTooLarge)
    ));
    assert_eq!(
        connection.wait_for_terminal_for_test().await.unwrap(),
        Error::FrameTooLarge
    );
    server.await.unwrap();
}

#[tokio::test]
async fn c1_09_event_queue_fails_closed_at_count_and_byte_limits() {
    assert_eq!(MAX_EVENT_COUNT, 128);
    assert_eq!(MAX_EVENT_BYTES, 4_194_304);
    assert_eq!(MAX_PENDING_INBOUND, 16);
    assert_eq!(MAX_PENDING_OUTBOUND, 16);
    let (connection, mut peer) = connection_pair(1_000_000).await;
    let mut frames = Vec::new();
    for _ in 0..=MAX_EVENT_COUNT {
        frames.extend_from_slice(
            &serde_json::to_vec(&json!({
                "jsonrpc":"2.0",
                "method":"session/update",
                "params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"x"}}}
            }))
            .unwrap(),
        );
        frames.push(b'\n');
    }
    peer.write_all(&frames).await.unwrap();
    assert_eq!(
        timeout(
            Duration::from_secs(2),
            connection.wait_for_terminal_for_test()
        )
        .await
        .unwrap()
        .unwrap(),
        Error::EventQueueOverflow
    );
    let mut received = 0;
    loop {
        match connection.recv_event().await {
            Ok(Some(_)) => received += 1,
            Err(Error::EventQueueOverflow) => break,
            other => panic!("unexpected event queue result: {other:?}"),
        }
    }
    assert_eq!(received, MAX_EVENT_COUNT);
}

#[tokio::test]
async fn c1_09_batch_permission_responses_preserve_original_order() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        let requests = (0..MAX_BATCH_ELEMENTS)
            .map(|index| {
            json!({
                "jsonrpc":"2.0",
                "id":100 + index,
                "method":"session/request_permission",
                "params":{"sessionId":"s-5","toolCall":{"toolCallId":format!("call-{index}")},"options":[{"optionId":format!("option-{index}"),"name":"Allow"}]}
            })
            })
            .collect::<Vec<_>>();
        write_frame(&mut peer, &json!(requests)).await;
        let response = read_json(&mut peer).await;
        assert_eq!(response.as_array().unwrap().len(), MAX_BATCH_ELEMENTS);
        for index in 0..MAX_BATCH_ELEMENTS {
            assert_eq!(response[index]["id"], 100 + index);
            assert_eq!(
                response[index]["result"]["outcome"]["optionId"],
                format!("option-{index}")
            );
        }
    });
    connection.initialize().await.unwrap();
    let mut requests = Vec::new();
    for _ in 0..MAX_BATCH_ELEMENTS {
        let Some(Event::PermissionRequest(request)) = connection.recv_event().await.unwrap() else {
            panic!("expected permission request");
        };
        requests.push(request);
    }
    for request in requests.into_iter().rev() {
        let index = request.request_id.to_value().as_u64().unwrap() as usize - 100;
        connection
            .respond_permission(request.request_id, &format!("option-{index}"))
            .await
            .unwrap();
    }
    server.await.unwrap();
}

#[tokio::test]
async fn c1_09_event_byte_budget_overflow_fails_without_dropping_queued_events() {
    let (connection, mut peer) = connection_pair(7 * MAX_FRAME_BYTES).await;
    let content = "x".repeat(900_000);
    let params = json!({"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":content}}});
    let event_bytes = serde_json::to_vec(&json!({"method":"session/update","params":params}))
        .unwrap()
        .len();
    assert!(event_bytes.div_ceil(4096) * 5 > MAX_EVENT_BYTES / 4096);
    let writer = tokio::spawn(async move {
        let body = "x".repeat(900_000);
        let frame = serde_json::to_vec(&json!({
            "jsonrpc":"2.0",
            "method":"session/update",
            "params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":body}}}
        }))
        .unwrap();
        for _ in 0..5 {
            peer.write_all(&frame).await.unwrap();
            peer.write_all(b"\n").await.unwrap();
        }
        peer
    });
    let _peer = writer.await.unwrap();
    assert_eq!(
        timeout(
            Duration::from_secs(2),
            connection.wait_for_terminal_for_test()
        )
        .await
        .unwrap()
        .unwrap(),
        Error::EventQueueOverflow
    );
    let available_permits = connection.available_event_permits();
    assert_eq!(
        available_permits, 144,
        "available permits after four queued events"
    );
    let mut received = 0;
    loop {
        match timeout(Duration::from_secs(2), connection.recv_event()).await {
            Ok(Ok(Some(Event::Notification { params, .. }))) => {
                assert_eq!(
                    params["update"]["content"]["text"].as_str().unwrap().len(),
                    900_000
                );
                received += 1;
            }
            Ok(Err(Error::EventQueueOverflow)) => break,
            other => panic!("unexpected byte budget result after {received} events: {other:?}"),
        }
    }
    assert_eq!(received, 4);
}

#[tokio::test]
async fn c1_09_inbound_permission_limit_is_sixteen() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let writer = tokio::spawn(async move {
        for id in 0..=MAX_PENDING_INBOUND {
            write_frame(&mut peer, &permission_request(json!(id))).await;
        }
    });
    writer.await.unwrap();
    assert_eq!(
        timeout(
            Duration::from_secs(2),
            connection.wait_for_terminal_for_test()
        )
        .await
        .unwrap()
        .unwrap(),
        Error::TooManyPendingPermissions
    );
    for _ in 0..MAX_PENDING_INBOUND {
        assert!(matches!(
            connection.recv_event().await.unwrap(),
            Some(Event::PermissionRequest(_))
        ));
    }
    assert!(matches!(
        connection.recv_event().await,
        Err(Error::TooManyPendingPermissions)
    ));
}

#[tokio::test]
async fn c1_06_completed_inbound_request_id_fails_closed() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        let request = permission_request(json!(71));
        write_frame(&mut peer, &request).await;
        assert_eq!(read_json(&mut peer).await["id"], 71);
        write_frame(&mut peer, &request).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    });
    connection.initialize().await.unwrap();
    let Some(Event::PermissionRequest(request)) = connection.recv_event().await.unwrap() else {
        panic!("expected permission request");
    };
    connection
        .respond_permission(request.request_id, "allow")
        .await
        .unwrap();
    assert_eq!(
        timeout(
            Duration::from_secs(2),
            connection.wait_for_terminal_for_test()
        )
        .await
        .unwrap()
        .unwrap(),
        Error::DuplicateRequestId
    );
    server.await.unwrap();
}

#[tokio::test]
async fn c1_06_writer_queue_overflow_fails_connection_before_permission_response() {
    let (connection, mut peer) = connection_pair(1).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        peer
    });
    connection.initialize().await.unwrap();
    let mut peer = server.await.unwrap();

    let connection = Arc::new(connection);
    let mut blocked_writes = Vec::new();
    for _ in 0..(crate::connection::WRITER_QUEUE_CAPACITY + 8) {
        let connection = Arc::clone(&connection);
        blocked_writes.push(tokio::spawn(async move { connection.cancel("s").await }));
    }
    timeout(Duration::from_secs(2), async {
        while connection.available_writer_slots_for_test() > 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    write_frame(&mut peer, &permission_request(json!(901))).await;
    let Some(Event::PermissionRequest(request)) =
        timeout(Duration::from_secs(2), connection.recv_event())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("expected permission request");
    };

    let mut responder = Box::pin(connection.respond_permission(request.request_id, "allow"));
    assert!(matches!(
        timeout(Duration::from_millis(100), &mut responder).await,
        Ok(Err(Error::WriterQueueFull))
    ));
    drop(responder);
    assert_eq!(
        timeout(
            Duration::from_millis(100),
            connection.wait_for_terminal_for_test()
        )
        .await
        .unwrap()
        .unwrap(),
        Error::WriterQueueFull
    );

    drop(peer);
    for write in blocked_writes {
        write.abort();
    }
}

#[tokio::test]
async fn c1_07_completed_outbound_request_id_fails_closed() {
    let (connection, mut peer) = connection_pair(65_536).await;
    let server = tokio::spawn(async move {
        initialize_peer(&mut peer).await;
        let request = read_json(&mut peer).await;
        let response =
            json!({"jsonrpc":"2.0","id":request["id"],"result":{"modeId":"workspace-write"}});
        write_frame(&mut peer, &response).await;
        write_frame(&mut peer, &response).await;
    });
    connection.initialize().await.unwrap();
    assert_eq!(
        connection.set_mode("s", "workspace-write").await.unwrap(),
        json!({"modeId":"workspace-write"})
    );
    assert_eq!(
        timeout(
            Duration::from_secs(2),
            connection.wait_for_terminal_for_test()
        )
        .await
        .unwrap()
        .unwrap(),
        Error::UnknownRequestId
    );
    server.await.unwrap();
}

#[tokio::test]
async fn c1_08_inbound_request_id_count_budget_fails_closed() {
    assert_eq!(MAX_SEEN_INBOUND_REQUEST_IDS, 4_096);
    let (connection, mut peer) = connection_pair(2 * MAX_FRAME_BYTES).await;
    let writer = tokio::spawn(async move {
        for start in (0..MAX_SEEN_INBOUND_REQUEST_IDS).step_by(MAX_BATCH_ELEMENTS) {
            let requests = (start..start + MAX_BATCH_ELEMENTS)
                .map(|id| unknown_request(json!(id)))
                .collect::<Vec<_>>();
            write_frame(&mut peer, &json!(requests)).await;
            assert_eq!(
                read_json(&mut peer).await.as_array().unwrap().len(),
                MAX_BATCH_ELEMENTS
            );
        }
        write_frame(
            &mut peer,
            &unknown_request(json!(MAX_SEEN_INBOUND_REQUEST_IDS)),
        )
        .await;
        peer
    });
    let _peer = writer.await.unwrap();
    assert_eq!(
        timeout(
            Duration::from_secs(2),
            connection.wait_for_terminal_for_test()
        )
        .await
        .unwrap()
        .unwrap(),
        Error::RequestIdHistoryFull
    );
}

#[tokio::test]
async fn c1_08_inbound_request_id_byte_budget_fails_closed() {
    assert_eq!(MAX_SEEN_INBOUND_REQUEST_ID_BYTES, 1_048_576);
    let (connection, mut peer) = connection_pair(2 * MAX_FRAME_BYTES).await;
    let server = tokio::spawn(async move {
        let first_id = "a".repeat(600_000);
        write_frame(&mut peer, &unknown_request(json!(first_id))).await;
        assert_eq!(
            read_json(&mut peer).await["id"].as_str().unwrap().len(),
            600_000
        );
        let second_id = "b".repeat(MAX_SEEN_INBOUND_REQUEST_ID_BYTES - 600_000);
        write_frame(&mut peer, &unknown_request(json!(second_id))).await;
        assert_eq!(
            read_json(&mut peer).await["id"].as_str().unwrap().len(),
            MAX_SEEN_INBOUND_REQUEST_ID_BYTES - 600_000
        );
        write_frame(&mut peer, &unknown_request(json!("one-byte-over"))).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    });
    server.await.unwrap();
    assert_eq!(
        timeout(
            Duration::from_secs(2),
            connection.wait_for_terminal_for_test()
        )
        .await
        .unwrap()
        .unwrap(),
        Error::RequestIdHistoryFull
    );
}

#[tokio::test]
async fn c1_10_stderr_is_drained_with_no_retained_bytes() {
    let (reader, mut writer) = duplex(32_768);
    let drain = tokio::spawn(Connection::drain_stderr(reader));
    writer.write_all(&vec![b's'; 32_768]).await.unwrap();
    writer.shutdown().await.unwrap();
    assert_eq!(drain.await.unwrap(), 0);
}

struct FakeChild(Arc<AtomicBool>);

impl crate::ChildControl for FakeChild {
    fn force_terminate(self: Box<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn graceful_shutdown(self: Box<Self>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            self.0.store(true, Ordering::SeqCst);
        })
    }
}

#[tokio::test]
async fn c1_11_shutdown_resolves_pending_work_and_terminates_only_owned_process() {
    let (first, mut first_peer) = connection_pair(65_536).await;
    let (second, mut second_peer) = connection_pair(65_536).await;
    let first_terminated = Arc::new(AtomicBool::new(false));
    let second_terminated = Arc::new(AtomicBool::new(false));
    first.attach_child(Box::new(FakeChild(Arc::clone(&first_terminated))));
    second.attach_child(Box::new(FakeChild(Arc::clone(&second_terminated))));
    let first_handshake = tokio::spawn(async move {
        initialize_peer(&mut first_peer).await;
        first_peer
    });
    first.initialize().await.unwrap();
    let mut first_peer = first_handshake.await.unwrap();
    let second_handshake = tokio::spawn(async move {
        initialize_peer(&mut second_peer).await;
        second_peer
    });
    second.initialize().await.unwrap();
    let mut second_peer = second_handshake.await.unwrap();
    let first = Arc::new(first);
    let second = Arc::new(second);
    let first_request_connection = Arc::clone(&first);
    let first_request =
        tokio::spawn(async move { first_request_connection.set_mode("s-6", "hold").await });
    let first_send = tokio::spawn(async move {
        let request = read_json(&mut first_peer).await;
        assert_eq!(request["method"], "session/set_mode");
        first_peer
    });
    let _first_peer = timeout(Duration::from_millis(20), first_send)
        .await
        .unwrap()
        .unwrap();
    first.shutdown().await;
    assert_eq!(first_request.await.unwrap(), Err(Error::Interrupted));
    assert!(first_terminated.load(Ordering::SeqCst));
    assert!(!second_terminated.load(Ordering::SeqCst));
    let second_request_connection = Arc::clone(&second);
    let second_mode =
        tokio::spawn(async move { second_request_connection.set_mode("s-7", "active").await });
    let second_response = tokio::spawn(async move {
        let request = read_json(&mut second_peer).await;
        write_frame(
            &mut second_peer,
            &json!({"jsonrpc":"2.0","id":request["id"],"result":{}}),
        )
        .await;
    });
    assert!(second_mode.await.unwrap().is_ok());
    second_response.await.unwrap();
}

#[test]
fn c1_11_request_id_projection_keeps_string_and_integer_identity() {
    assert_eq!(
        RequestId::from_value(&json!(12)).unwrap().to_value(),
        json!(12)
    );
    assert_eq!(
        RequestId::from_value(&json!("12")).unwrap().to_value(),
        json!("12")
    );
    assert!(RequestId::from_value(&Value::Null).is_err());
    assert_eq!(
        PermissionOutcome::Selected(String::from("allow_once")).option_id(),
        Some("allow_once")
    );
}
