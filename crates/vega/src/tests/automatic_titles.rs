use super::*;

#[test]
fn automatic_title_production_worker_records_real_http_wire() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let captured = recorded.clone();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut connections = Vec::new();
        while connections.len() < 2 && std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    // Darwin may inherit O_NONBLOCK; timeouts do not clear it.
                    socket.set_nonblocking(false).unwrap();
                    let captured = captured.clone();
                    connections.push(std::thread::spawn(move || {
                        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                        socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                        let mut bytes = Vec::new();
                        let (header_end, length) = loop {
                            let mut chunk = [0; 4096];
                            let read = socket.read(&mut chunk).unwrap(); assert!(read > 0); bytes.extend_from_slice(&chunk[..read]);
                            assert!(bytes.len() < 128 * 1024);
                            if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                                let headers = String::from_utf8_lossy(&bytes[..index]);
                                let length = headers.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|value| value.trim().parse::<usize>().unwrap())).unwrap();
                                break (index + 4, length);
                            }
                        };
                        while bytes.len() < header_end + length { let mut chunk=[0;4096]; let read=socket.read(&mut chunk).unwrap(); assert!(read>0); bytes.extend_from_slice(&chunk[..read]); }
                        let request = String::from_utf8(bytes[header_end..header_end+length].to_vec()).unwrap();
                        let title = request.contains("\"max_tokens\":512");
                        captured.lock().unwrap().push(request);
                        if title { std::thread::sleep(Duration::from_millis(150)); }
                        let text = if title { "HTTP 标题" } else { "正文完成" };
                        let delta=format!("{{\"choices\":[{{\"delta\":{{\"content\":\"{text}\"}},\"finish_reason\":null}}]}}");
                        let body=format!("data: {delta}\n\ndata: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n");
                        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
                    }));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error) => panic!("local test server: {error}"),
            }
        }
        assert_eq!(connections.len(), 2);
        for connection in connections {
            connection.join().unwrap();
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let store = Store::open(&path).unwrap();
    store.migrate().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "NEVER_IN_TITLE_WIRE").unwrap();
    let project =
        vega_store::projects::create(store.conn(), dir.path().to_str().unwrap(), "http", None)
            .unwrap();
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "owned-model",
        PermissionMode::Confirm.as_str(),
    )
    .unwrap();
    let provider = Arc::new(
        vega_runtime::OpenAiProvider::new(format!("http://{address}/v1"), "owned-test-key")
            .unwrap(),
    );
    let (sender, updates) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    let (notifications, receiver) = mpsc::channel();
    run_agent_worker(
        path,
        dir.path().into(),
        thread.clone(),
        PendingAgentRun::UserMessage("@notes.txt 请总结".into()),
        vega_conversation::agent::PermissionQueue::new(),
        tokio_util::sync::CancellationToken::new(),
        sender,
        None,
        None,
        None,
        Some(notifications),
        Some(provider),
        Arc::new(AgentWorkerStartProbe::default()),
    );
    assert_eq!(drain_agent_updates(&updates).finished, Some(true));
    receiver.recv_timeout(Duration::from_secs(3)).unwrap();
    receiver.recv_timeout(Duration::from_secs(3)).unwrap();
    server.join().unwrap();
    let recorded = recorded.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    let title = recorded
        .iter()
        .find(|request| request.contains("\"max_tokens\":512"))
        .unwrap();
    assert!(title.contains("\"model\":\"owned-model\""));
    assert_eq!(title.matches("\"role\":").count(), 2);
    assert!(title.contains("\"content\":\"@notes.txt 请总结\""));
    assert!(!title.contains("\"function\""));
    assert!(!title.contains("NEVER_IN_TITLE_WIRE"));
    assert_eq!(
        vega_store::threads::find(store.conn(), &thread.id)
            .unwrap()
            .unwrap()
            .title,
        "HTTP 标题"
    );
    assert_eq!(
        vega_store::messages::recent(store.conn(), &thread.id, 10)
            .unwrap()
            .len(),
        2
    );
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM token_usage WHERE thread_id=?1 AND message_id IS NULL",
            [&thread.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

struct TitleProvider {
    primary: vega_runtime::MockProvider,
    title: vega_runtime::MockProvider,
    title_gate: Arc<tokio::sync::Notify>,
}

#[gpui_kit::test]
async fn automatic_title_real_notifications_refresh_current_after_other_task_epoch(
    cx: &mut gpui_kit::TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let store = Store::open(&path).unwrap();
    store.migrate().unwrap();
    let project =
        vega_store::projects::create(store.conn(), dir.path().to_str().unwrap(), "p", None)
            .unwrap();
    let a = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .unwrap();
    let b = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .unwrap();
    cx.update(|cx| install_diff_window_globals(store, a.clone(), cx));
    let root = cx.new(VegaWindow::new);
    let (send_a, receive_a) = mpsc::channel();
    let (send_b, receive_b) = mpsc::channel();
    root.update(cx, |root, cx| {
        root.watch_automatic_titles(path.clone(), a.id.clone(), receive_a, cx);
        root.watch_automatic_titles(path.clone(), b.id.clone(), receive_b, cx);
    });
    let writer = Store::open(&path).unwrap();
    // Completions arrive together. Their independent watchers advance the epoch;
    // A's older read must be retried, not dropped with the disconnected sender.
    vega_store::threads::update(writer.conn(), &a.id, Some("A final"), None, None, None).unwrap();
    vega_store::threads::update(writer.conn(), &b.id, Some("B final"), None, None, None).unwrap();
    send_a.send(()).unwrap();
    send_b.send(()).unwrap();
    drop(send_b);
    for _ in 0..20 {
        cx.executor().advance_clock(AGENT_EVENT_POLL);
        cx.run_until_parked();
    }
    cx.update(|cx| {
        let current = cx.global::<OpenedThread>().0.as_ref().unwrap();
        assert_eq!(current.id, a.id);
        assert_eq!(current.title, "A final");
        assert_eq!(
            cx.global::<vega_ui::navigation::TaskMutationState>()
                .pending,
            0
        );
    });
    cx.update(|cx| cx.set_global(OpenedThread(Some(b.clone()))));
    vega_store::threads::update(writer.conn(), &a.id, Some("A late"), None, None, None).unwrap();
    send_a.send(()).unwrap();
    drop(send_a);
    for _ in 0..20 {
        cx.executor().advance_clock(AGENT_EVENT_POLL);
        cx.run_until_parked();
    }
    cx.update(|cx| {
        let current = cx.global::<OpenedThread>().0.as_ref().unwrap();
        assert_eq!(current.id, b.id);
        assert_eq!(current.title, b.title);
    });
}

#[gpui_kit::test]
async fn automatic_title_notification_recovers_after_owned_transient_read_error(
    cx: &mut gpui_kit::TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let store = Store::open(&path).unwrap();
    store.migrate().unwrap();
    let project =
        vega_store::projects::create(store.conn(), dir.path().to_str().unwrap(), "p", None)
            .unwrap();
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .unwrap();
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    let writer = Store::open(&path).unwrap();
    vega_store::threads::update(
        writer.conn(),
        &thread.id,
        Some("recovered"),
        None,
        None,
        None,
    )
    .unwrap();
    // Owned fixture fault at the real facade: SELECT fails once, rather than
    // treating an I/O error as the semantically different deleted-thread None.
    writer
        .conn()
        .execute_batch("ALTER TABLE threads RENAME TO temporarily_unavailable_threads")
        .unwrap();
    let (sender, receiver) = mpsc::channel();
    root.update(cx, |root, cx| {
        root.watch_automatic_titles(path, thread.id.clone(), receiver, cx)
    });
    sender.send(()).unwrap();
    drop(sender);
    cx.run_until_parked();
    cx.executor().advance_clock(AGENT_EVENT_POLL);
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(cx.global::<OpenedThread>().0.as_ref().unwrap().title, ""));
    writer
        .conn()
        .execute_batch("ALTER TABLE temporarily_unavailable_threads RENAME TO threads")
        .unwrap();
    for _ in 0..10 {
        cx.executor().advance_clock(AGENT_EVENT_POLL);
        cx.run_until_parked();
    }
    cx.update(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().title,
            "recovered"
        )
    });
}

impl vega_runtime::Provider for TitleProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<vega_runtime::EventStream, vega_runtime::VegaError>,
                > + Send,
        >,
    > {
        if request.max_tokens == Some(512) {
            let provider = self.title.clone();
            let gate = self.title_gate.clone();
            Box::pin(async move {
                gate.notified().await;
                provider.chat_stream(request, cancel).await
            })
        } else {
            self.primary.chat_stream(request, cancel)
        }
    }
}

fn title_provider() -> Arc<TitleProvider> {
    Arc::new(TitleProvider {
        title_gate: Arc::new(tokio::sync::Notify::new()),
        primary: vega_runtime::MockProvider::new(vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("body done".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ])]),
        title: vega_runtime::MockProvider::new(vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("“文件摘要”".into()),
            vega_runtime::ProviderEvent::Usage {
                input: 10,
                output: 3,
                cache_read: 0,
                cache_write: 0,
            },
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ])]),
    })
}

#[test]
fn automatic_title_survives_primary_runtime_and_route_cancel_without_reference_leak_or_retry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "PRIVATE_FILE_CONTENT").unwrap();
    let path = dir.path().join("vega.db");
    let store = Store::open(&path).unwrap();
    store.migrate().unwrap();
    let project =
        vega_store::projects::create(store.conn(), dir.path().to_str().unwrap(), "title", None)
            .unwrap();
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .unwrap();
    let provider = title_provider();
    let (notifications, receiver) = mpsc::channel();
    let cancel = tokio_util::sync::CancellationToken::new();
    let (sender, updates) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    run_agent_worker(
        path.clone(),
        dir.path().into(),
        thread.clone(),
        PendingAgentRun::UserMessage("@notes.txt 总结这个文件".into()),
        vega_conversation::agent::PermissionQueue::new(),
        cancel.clone(),
        sender,
        None,
        None,
        None,
        Some(notifications),
        Some(provider.clone()),
        Arc::new(AgentWorkerStartProbe::default()),
    );
    assert_eq!(drain_agent_updates(&updates).finished, Some(true));
    receiver.recv_timeout(Duration::from_secs(3)).unwrap(); // committed fallback
    assert_eq!(
        vega_store::threads::find(store.conn(), &thread.id)
            .unwrap()
            .unwrap()
            .title,
        "@notes.txt 总结这个文件"
    );
    let before = vega_store::threads::find(store.conn(), &thread.id)
        .unwrap()
        .unwrap()
        .updated_at;
    cancel.cancel(); // route cancels the old primary, not the naming lifetime
    provider.title_gate.notify_one();
    receiver.recv_timeout(Duration::from_secs(3)).unwrap();
    let row = vega_store::threads::find(store.conn(), &thread.id)
        .unwrap()
        .unwrap();
    assert_eq!(row.title, "文件摘要");
    assert_eq!(row.updated_at, before);
    let requests = provider.title.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].messages.len(), 2);
    assert_eq!(requests[0].messages[1].content, "@notes.txt 总结这个文件");
    assert!(
        requests[0]
            .messages
            .iter()
            .all(|message| message.images.is_empty()
                && !message.content.contains("PRIVATE_FILE_CONTENT"))
    );
    assert!(requests[0].tools.is_empty());
    assert_eq!(requests[0].max_tokens, Some(512));
    assert_eq!(
        vega_store::messages::recent(store.conn(), &thread.id, 10)
            .unwrap()
            .len(),
        2
    );
    let usage: (i64, i64, i64) = store.conn().query_row("SELECT COUNT(*), SUM(input_tokens), COUNT(pricing_version) FROM token_usage WHERE thread_id=?1 AND message_id IS NULL", [&thread.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!(usage, (1, 10, 0)); // unknown pricing is never priced zero
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        vega_store::threads::find(reopened.conn(), &thread.id)
            .unwrap()
            .unwrap()
            .title,
        "文件摘要"
    );
    let (notifications, receiver) = mpsc::channel();
    let (sender, updates) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    run_agent_worker(
        path,
        dir.path().into(),
        thread,
        PendingAgentRun::UserMessage("second turn".into()),
        vega_conversation::agent::PermissionQueue::new(),
        tokio_util::sync::CancellationToken::new(),
        sender,
        None,
        None,
        None,
        Some(notifications),
        Some(provider.clone()),
        Arc::new(AgentWorkerStartProbe::default()),
    );
    assert_eq!(drain_agent_updates(&updates).finished, Some(true));
    assert!(receiver.recv_timeout(Duration::from_secs(1)).is_err());
    assert_eq!(provider.title.requests().len(), 1);
}

#[gpui_kit::test]
async fn automatic_title_projection_fences_routes_and_manual_epoch_without_replacing_thread(
    cx: &mut gpui_kit::TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).unwrap();
    store.migrate().unwrap();
    let project =
        vega_store::projects::create(store.conn(), dir.path().to_str().unwrap(), "p", None)
            .unwrap();
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .unwrap();
    cx.update(|cx| {
        install_diff_window_globals(store, thread.clone(), cx);
        cx.set_global(vega_ui::navigation::TaskMutationState::default());
    });
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        assert!(root.apply_automatic_title(&thread.id, 0, Some("generated".into()), cx))
    });
    cx.update(|cx| {
        let current = cx.global::<OpenedThread>().0.as_ref().unwrap();
        assert_eq!(current.title, "generated");
        assert_eq!(current.model, thread.model);
        assert_eq!(current.updated_at, thread.updated_at);
    });
    cx.update(|cx| {
        vega_ui::navigation::begin_task_mutation(cx);
        vega_ui::navigation::finish_task_mutation(cx);
        let mut manual = thread.clone();
        manual.title = "manual".into();
        cx.set_global(OpenedThread(Some(manual)));
    });
    root.update(cx, |root, cx| {
        assert!(!root.apply_automatic_title(&thread.id, 0, Some("stale".into()), cx))
    });
    cx.update(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().title,
            "manual"
        )
    });
    let mut other = thread.clone();
    other.id = "other".into();
    other.title = "other title".into();
    cx.update(|cx| cx.set_global(OpenedThread(Some(other.clone()))));
    root.update(cx, |root, cx| {
        let epoch = cx.global::<vega_ui::navigation::TaskMutationState>().epoch;
        assert!(root.apply_automatic_title(&thread.id, epoch, Some("late A".into()), cx));
    });
    cx.update(|cx| {
        let current = cx.global::<OpenedThread>().0.as_ref().unwrap();
        assert_eq!(current.id, other.id);
        assert_eq!(current.title, other.title);
        assert_eq!(
            cx.global::<vega_ui::navigation::TaskMutationState>()
                .pending,
            0
        );
    });
}
