use super::*;

struct FileReferenceWindowHarness {
    root: Entity<VegaWindow>,
}

impl Render for FileReferenceWindowHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.root.clone()
    }
}

// ---------- S8-T45/C7 顶部水合：worker + 路由 fence ----------

/// Seeds `count` user/assistant exchanges (2 rows per step) plus a
/// project/thread fixture, returning the store, its thread, and the root
/// temp directory.
fn seed_hydration_thread(count: usize) -> (Store, Thread, TempDir) {
    let dir = tempfile::tempdir().expect("hydration data root");
    let store = Store::open(dir.path().join("vega.db")).expect("hydration store");
    store.migrate().expect("hydration migrations");
    let project =
        vega_store::projects::create(store.conn(), "/tmp/hydration-fixture", "hydration", None)
            .expect("hydration project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Auto.as_str(),
    )
    .expect("hydration thread");
    for step in 0..count {
        insert(
            store.conn(),
            &MessageRow {
                id: format!("user-{step}"),
                thread_id: thread.id.clone(),
                seq: (2 * step + 1) as i64,
                role: "user".into(),
                kind: "text".into(),
                content: format!("第 {step} 问"),
                status: "done".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .expect("seed user row");
        insert(
            store.conn(),
            &MessageRow {
                id: format!("assistant-{step}"),
                thread_id: thread.id.clone(),
                seq: (2 * step + 2) as i64,
                role: "assistant".into(),
                kind: "text".into(),
                content: format!("第 {step} 答"),
                status: "done".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .expect("seed assistant row");
    }
    (store, thread, dir)
}

#[test]
fn history_page_worker_reads_one_keyset_page_off_thread() {
    let (store, thread, _dir) = seed_hydration_thread(150); // 300 rows
    let database_path = store
        .database_path()
        .expect("durable database path")
        .to_path_buf();
    drop(store);
    let request = HistoryPageRequested {
        thread_id: thread.id.clone(),
        before: 301,
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    run_history_page_worker(database_path, request.clone(), sender);
    let (delivered, outcome) = receiver.recv().expect("worker result");
    assert_eq!(delivered, request, "the request round-trips for fencing");
    let page = outcome.expect("one page below the cursor");
    // The newest 200 durable rows below seq 301 (seqs 101..=300). A pure
    // scroll-up page re-projects no summary reference — it belongs to the
    // newest page (C7: S7 summary 引用只在最新页).
    assert_eq!(page.entries.len(), 200);
    assert_eq!(page.older_cursor, Some(101));
    let heads: Vec<i64> = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            vega_conversation::history::HistoryEntry::UserText { seq, .. } => Some(*seq),
            _ => None,
        })
        .collect();
    assert_eq!(heads.first(), Some(&101), "page starts at the oldest seq");
    assert_eq!(heads.last(), Some(&299), "page ends below the cursor");
    assert_eq!(heads.len(), 100);
}

#[gpui_kit::test]
async fn late_hydration_page_is_dropped_after_route_replacement(cx: &mut gpui_kit::TestAppContext) {
    let (store, thread, _dir) = seed_hydration_thread(150);
    // Read the typed page while the seed store is alive, then hand the
    // store to the route globals (the app never re-reads it here).
    let page = vega_conversation::history::history_page_before(
        &store,
        &thread.id,
        vega_store::messages::PageCursor::Before(201),
        vega_store::messages::PAGE_LIMIT,
    )
    .expect("hydration page");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));

    // Route A open: stream A is the cached view of the opened thread.
    let stream_a = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), stream_a.clone()));
    });
    root.update(cx, |root, cx| {
        root.finish_history_page(stream_a.clone(), Ok(page.clone()), cx);
    });
    let applied = stream_a.read_with(cx, |stream, _| stream.hydrated_entry_count());
    assert!(applied > 0, "the live route applies its own page");
    assert_eq!(
        stream_a.read_with(cx, |stream, _| stream.hydration_cursor()),
        page.older_cursor,
    );

    // Route switch A→B: the cached view is replaced; the late page for A
    // must be dropped (A→B→A 晚到页丢弃).
    let mut thread_b = thread.clone();
    thread_b.id = "hydration-thread-b".into();
    let stream_b = cx.new(|cx| ConversationStream::new(thread_b.clone(), cx));
    root.update(cx, |root, _| {
        root.stream_view = Some((thread_b.id.clone(), stream_b.clone()));
    });
    root.update(cx, |root, cx| {
        root.finish_history_page(stream_a.clone(), Ok(page), cx);
    });
    let after_switch = stream_a.read_with(cx, |stream, _| stream.hydrated_entry_count());
    assert_eq!(
        applied, after_switch,
        "a late page never mutates a replaced route's stream"
    );
    let b_entries = stream_b.read_with(cx, |stream, _| stream.hydrated_entry_count());
    assert_eq!(b_entries, 0, "the late page never reaches the new route");
}

#[gpui_kit::test]
async fn file_index_cancel_cancels_the_owned_worker_without_joining(
    cx: &mut gpui_kit::TestAppContext,
) {
    let workspace = tempfile::tempdir().expect("file index cancellation workspace");
    let store = Store::open(":memory:").expect("file index cancellation store");
    store.migrate().expect("file index cancellation migrations");
    let project = vega_store::projects::create(
        store.conn(),
        workspace
            .path()
            .to_str()
            .expect("UTF-8 cancellation workspace"),
        "file-index-cancel-project",
        None,
    )
    .expect("file index cancellation project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("file index cancellation thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    let token = tokio_util::sync::CancellationToken::new();
    root.update(cx, |root, _| {
        root.file_index_controller.active = Some(ActiveFileIndex {
            owner: FileIndexOwner {
                stream,
                thread_id: thread.id,
                project_id: thread.project_id,
                generation: 1,
            },
            cancel: token.clone(),
        });
        root.file_index_controller.cancel();
    });

    assert!(token.is_cancelled());
    assert!(root.read_with(cx, |root, _| root.file_index_controller.active.is_none()));
}

#[test]
fn at_reference_injection_persists_across_reopen() {
    // S8-T47 E2E (A2-12 主干, headless): fresh thread → user submits an
    // `@file` token → the worker injects the bounded reference block →
    // the persisted user row keeps the injection after a store reopen
    // (重启保持). MockProvider at the provider boundary only; no keys,
    // no network.
    let workspace = tempfile::tempdir().expect("workspace root");
    std::fs::write(workspace.path().join("notes.txt"), "LOREM_REFERENCE_MARKER")
        .expect("write referenced file");
    let data = tempfile::tempdir().expect("data root");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("reference store");
    store.migrate().expect("reference migrations");
    let project = vega_store::projects::create(
        store.conn(),
        workspace.path().to_str().expect("UTF-8 workspace"),
        "reference-e2e",
        None,
    )
    .expect("reference project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock-reference",
        PermissionMode::Confirm.as_str(),
    )
    .expect("reference thread");
    drop(store);

    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("ok".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let (sender, receiver) = mpsc::sync_channel::<AgentUpdate>(AGENT_EVENT_CAPACITY);
    run_agent_worker(
        database_path.clone(),
        workspace.path().to_path_buf(),
        thread.clone(),
        PendingAgentRun::UserMessage("@notes.txt 总结这个文件".into()),
        vega_conversation::agent::PermissionQueue::new(),
        tokio_util::sync::CancellationToken::new(),
        sender,
        None,
        None,
        None,
        None,
        #[cfg(test)]
        Some(provider),
        Arc::new(AgentWorkerStartProbe::default()),
    );
    while receiver.try_recv().is_ok() {}

    let reopened = Store::open(&database_path).expect("reopen store after restart");
    let rows = vega_store::messages::recent(reopened.conn(), &thread.id, 16)
        .expect("messages after restart");
    let user = rows
        .iter()
        .find(|row| row.role == "user")
        .expect("persisted user row");
    assert!(
        user.content.starts_with("[@notes.txt]"),
        "injected reference block leads the persisted message"
    );
    assert!(user.content.contains("LOREM_REFERENCE_MARKER"));
    assert!(
        user.content.contains("总结这个文件"),
        "original user text preserved after the injected block"
    );
}

#[test]
fn at_reference_rejection_keeps_provider_at_zero_calls() {
    let workspace = tempfile::tempdir().expect("workspace root");
    let data = tempfile::tempdir().expect("data root");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("reference rejection store");
    store.migrate().expect("reference rejection migrations");
    let project = vega_store::projects::create(
        store.conn(),
        workspace.path().to_str().expect("UTF-8 workspace root"),
        "reference-rejection-e2e",
        None,
    )
    .expect("reference rejection project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-luna",
        PermissionMode::Confirm.as_str(),
    )
    .expect("reference rejection thread");
    drop(store);

    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("must not run"),
    ]));
    let (title_sender, title_receiver) = mpsc::channel();
    let (sender, receiver) = mpsc::sync_channel::<AgentUpdate>(AGENT_EVENT_CAPACITY);
    run_agent_worker(
        database_path.clone(),
        workspace.path().to_path_buf(),
        thread.clone(),
        PendingAgentRun::UserMessage("请读 @missing.txt".into()),
        vega_conversation::agent::PermissionQueue::new(),
        tokio_util::sync::CancellationToken::new(),
        sender,
        None,
        None,
        None,
        Some(title_sender),
        #[cfg(test)]
        Some(provider.clone()),
        Arc::new(AgentWorkerStartProbe::default()),
    );
    let batch = drain_agent_updates(&receiver);
    assert_eq!(batch.finished, Some(false));
    assert_eq!(
        batch.reference_failure,
        Some(FileReferenceFailureCode::Missing)
    );
    assert!(
        provider.requests().is_empty(),
        "resolver failure precedes provider"
    );
    assert!(
        title_receiver.try_recv().is_err(),
        "preflight failure never schedules naming"
    );

    let reopened = Store::open(&database_path).expect("reopen rejection store");
    let rows =
        vega_store::messages::recent(reopened.conn(), &thread.id, 16).expect("rejection history");
    assert!(
        rows.is_empty(),
        "rejected draft must not create durable history"
    );
}

#[gpui_kit::test]
async fn at_reference_real_subscription_indexes_and_injects_request(
    cx: &mut gpui_kit::TestAppContext,
) {
    let workspace = tempfile::tempdir().expect("subscription workspace");
    std::fs::write(
        workspace.path().join("notes.txt"),
        "SUBSCRIPTION_REFERENCE_MARKER",
    )
    .expect("subscription file");
    std::fs::write(workspace.path().join("binary.bin"), [b'x', 0, b'y'])
        .expect("binary subscription file");
    std::fs::write(
        workspace.path().join("oversized.txt"),
        vec![b'x'; vega_tools::reference::REFERENCE_MAX_FILE_BYTES as usize + 1],
    )
    .expect("oversized subscription file");
    std::fs::create_dir(workspace.path().join("folder")).expect("subscription directory");
    for index in 0..5 {
        std::fs::write(
            workspace.path().join(format!("total-{index}.txt")),
            vec![b'x'; 12 * 1024],
        )
        .expect("total subscription file");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        workspace.path().join("notes.txt"),
        workspace.path().join("alias.txt"),
    )
    .expect("symlink subscription file");
    let data = tempfile::tempdir().expect("subscription data");
    let config_path = data.path().join("config.toml");
    super::model_selection::model_selection_config(&config_path);
    vega_store::keystore::set_key(data.path(), "owned", "subscription-test-key")
        .expect("subscription credential");
    let store = Store::open(data.path().join("vega.db")).expect("subscription store");
    store.migrate().expect("subscription migrations");
    let project = vega_store::projects::create(
        store.conn(),
        workspace
            .path()
            .to_str()
            .expect("UTF-8 subscription workspace"),
        "reference-subscription-e2e",
        None,
    )
    .expect("subscription project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-luna",
        PermissionMode::Confirm.as_str(),
    )
    .expect("subscription thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));

    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("ok".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config_path);
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()))
    });
    let window_root = root.clone();
    let _window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| FileReferenceWindowHarness { root: window_root })
            })
        })
        .expect("subscription window");

    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view.is_some()
                && matches!(
                    root.pricing_controller.state,
                    PricingControllerState::Ready { .. }
                )
        })
    });
    let stream = root
        .read_with(cx, |root, _| {
            root.stream_view.as_ref().map(|(_, stream)| stream.clone())
        })
        .expect("subscription stream");
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    stream.update(cx, |_, cx| {
        input.update(cx, |input, cx| input.set_text("@notes", cx));
    });
    pump_test_app(cx, |cx| {
        stream.read_with(cx, |stream, _| {
            stream.file_index_loaded() && !stream.file_index_loading()
        })
    });
    let candidate_index = stream
        .read_with(cx, |stream, _| {
            stream
                .file_index_candidates()
                .iter()
                .position(|entry| entry == "notes.txt")
        })
        .expect("indexed candidate");
    stream.update(cx, |stream, cx| {
        stream.accept_file_candidate(candidate_index, cx)
    });
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "@notes.txt "
    );

    let focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
    _window
        .update(cx, |_, window, cx| window.focus(&focus, cx))
        .expect("focus subscription composer");
    cx.simulate_keystrokes(_window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.agent_controller.active.is_empty())
            && !provider.requests().is_empty()
    });
    let request = provider
        .requests()
        .into_iter()
        .next()
        .expect("provider request");
    let user = request
        .messages
        .iter()
        .find(|message| message.role == vega_runtime::ChatRole::User)
        .expect("user request");
    assert!(user.content.contains("[@notes.txt]"));
    assert!(user.content.contains("SUBSCRIPTION_REFERENCE_MARKER"));
    assert!(user.content.contains("@notes.txt"));

    let entries_before_rejection = stream.read_with(cx, |stream, _| stream.hydrated_entry_count());
    let rejection_cases = [
        ("请读 @missing.txt", FileReferenceFailureCode::Missing),
        (
            "请读 @../escape.txt",
            FileReferenceFailureCode::OutsideProject,
        ),
        ("请读 @binary.bin", FileReferenceFailureCode::BinaryContent),
        (
            "请读 @oversized.txt",
            FileReferenceFailureCode::FileTooLarge,
        ),
        ("请读 @folder", FileReferenceFailureCode::NotRegularFile),
        ("请读 @alias.txt", FileReferenceFailureCode::SymlinkRejected),
        (
            "请读 @total-0.txt @total-1.txt @total-2.txt @total-3.txt @total-4.txt",
            FileReferenceFailureCode::TotalBytesExceeded,
        ),
        (
            "请读 @one @two @three @four @five @six @seven @eight @nine",
            FileReferenceFailureCode::TooManyReferences,
        ),
    ];
    for (content, code) in rejection_cases {
        input.update(cx, |input, cx| input.set_text(content, cx));
        cx.simulate_keystrokes(_window.into(), "cmd-enter");
        pump_test_app(cx, |cx| {
            stream.read_with(cx, |stream, cx| {
                !stream.composer_submission_pending()
                    && stream.controller_error_message().as_deref() == Some(code.message())
                    && stream.composer_input().read(cx).text() == content
            })
        });
        assert_eq!(
            provider.requests().len(),
            1,
            "rejected reference makes zero new calls"
        );
    }
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.hydrated_entry_count()),
        entries_before_rejection,
        "rejected reference adds no user echo"
    );
}
