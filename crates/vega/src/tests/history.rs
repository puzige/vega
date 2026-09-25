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

fn message_location_root(
    cx: &mut gpui_kit::TestAppContext,
    store: Store,
    thread: &Thread,
) -> (Entity<VegaWindow>, Entity<ConversationStream>) {
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.update_message_location_route(Some(&thread.id));
    });
    (root, stream)
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

#[test]
fn message_location_worker_returns_the_bounded_containing_page() {
    let (store, thread, _dir) = seed_hydration_thread(300);
    let database_path = store
        .database_path()
        .expect("durable database path")
        .to_path_buf();
    drop(store);
    let request = MessageLocationWorkerRequest {
        thread_id: thread.id,
        message_id: "assistant-100".into(),
        route_generation: 7,
        request_generation: 19,
        restore_anchor: None,
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    run_message_location_worker(database_path, request.clone(), sender);
    let (delivered, outcome) = receiver.recv().expect("worker result");
    assert_eq!(delivered, request);
    let page = outcome
        .expect("message location page")
        .expect("known target");
    assert_eq!(page.entries.len(), vega_store::messages::PAGE_LIMIT);
    let target_index = page
        .entries
        .iter()
        .position(|entry| {
            matches!(
                entry,
                vega_conversation::history::HistoryEntry::AssistantText { message_id, .. }
                    if message_id == "assistant-100"
            )
        })
        .expect("target remains in the containing page");
    assert_eq!(target_index, 99);
    assert_eq!(page.older_cursor, Some(103));
    assert_eq!(page.newer_cursor, Some(302));
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
        root.update_message_location_route(Some(&thread.id));
    });
    let route_generation = root.read_with(cx, |root, _| root.message_location_route_generation);
    root.update(cx, |root, cx| {
        root.finish_history_page(stream_a.clone(), route_generation, Ok(page.clone()), cx);
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
        root.update_message_location_route(Some(&thread_b.id));
    });
    root.update(cx, |root, cx| {
        root.finish_history_page(stream_a.clone(), route_generation, Ok(page), cx);
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
async fn active_message_location_defers_without_mutating_live_entries_then_resumes(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (store, thread, _dir) = seed_hydration_thread(300);
    let (root, stream) = message_location_root(cx, store, &thread);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "live-message".into(),
                seq: 301,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live-message".into(),
                delta: "live response".into(),
            },
            cx,
        );
    });
    let before = stream.read_with(cx, |stream, _| {
        (
            stream.hydrated_entry_count(),
            stream.scroll_anchor_snapshot(),
        )
    });
    root.update(cx, |root, _| {
        root.agent_controller
            .begin(thread.id.clone(), stream.clone(), None, None)
            .expect("active run");
    });
    let request = MessageLocationRequested {
        thread_id: thread.id.clone(),
        message_id: "assistant-100".into(),
        restore_anchor: None,
    };
    root.update(cx, |root, cx| {
        root.request_message_location(stream.clone(), &request, cx)
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.message_location_status()),
        Some(MessageLocationStatus::Deferred)
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| {
            (
                stream.hydrated_entry_count(),
                stream.scroll_anchor_snapshot(),
            )
        }),
        before
    );
    root.update(cx, |root, cx| {
        root.agent_controller.active.remove(&thread.id);
        root.resume_deferred_message_location(&thread.id, cx);
    });
    for _ in 0..200 {
        cx.executor().advance_clock(Duration::from_millis(5));
        cx.run_until_parked();
        if stream.read_with(cx, |stream, _| {
            stream.message_location_status() == Some(MessageLocationStatus::Located)
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.message_location_status()),
        Some(MessageLocationStatus::Located)
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.hydrated_entry_count()),
        vega_store::messages::PAGE_LIMIT
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.scroll_anchor_snapshot().message_id),
        Some("assistant-100".into())
    );
}

#[gpui_kit::test]
async fn message_location_generation_fences_loaded_reselection_and_a_b_a(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (store, thread, _dir) = seed_hydration_thread(150);
    let loaded_page = vega_conversation::history::history_page_containing_message(
        &store,
        &thread.id,
        "assistant-100",
        vega_store::messages::PAGE_LIMIT,
    )
    .unwrap()
    .unwrap();
    let stale_page = vega_conversation::history::history_page_containing_message(
        &store,
        &thread.id,
        "assistant-10",
        vega_store::messages::PAGE_LIMIT,
    )
    .unwrap()
    .unwrap();
    let (root, stream) = message_location_root(cx, store, &thread);
    stream.update(cx, |stream, cx| stream.apply_history_page(loaded_page, cx));
    root.update(cx, |root, _| {
        root.agent_controller
            .begin(thread.id.clone(), stream.clone(), None, None)
            .expect("active stream remains cached across route switches");
    });
    let route_generation = root.read_with(cx, |root, _| root.message_location_route_generation);
    let first = MessageLocationRequested {
        thread_id: thread.id.clone(),
        message_id: "assistant-100".into(),
        restore_anchor: None,
    };
    root.update(cx, |root, cx| {
        root.request_message_location(stream.clone(), &first, cx)
    });
    let first_request_generation =
        root.read_with(cx, |root, _| root.message_location_request_generation);
    root.update(cx, |root, cx| {
        root.request_message_location(stream.clone(), &first, cx)
    });
    let second_request_generation =
        root.read_with(cx, |root, _| root.message_location_request_generation);
    assert!(second_request_generation > first_request_generation);
    let before_stale_request = stream.read_with(cx, |stream, _| {
        (
            stream.hydrated_entry_count(),
            stream.scroll_anchor_snapshot(),
        )
    });
    root.update(cx, |root, cx| {
        root.finish_message_location_worker(
            stream.clone(),
            MessageLocationWorkerRequest {
                thread_id: thread.id.clone(),
                message_id: "assistant-10".into(),
                route_generation,
                request_generation: first_request_generation,
                restore_anchor: None,
            },
            Ok(Some(stale_page.clone())),
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| {
            (
                stream.hydrated_entry_count(),
                stream.scroll_anchor_snapshot(),
            )
        }),
        before_stale_request,
        "a prior unloaded lookup cannot replace a newer loaded selection"
    );
    let latest_request = MessageLocationWorkerRequest {
        thread_id: thread.id.clone(),
        message_id: "assistant-10".into(),
        route_generation,
        request_generation: second_request_generation,
        restore_anchor: None,
    };
    let mut thread_b = thread.clone();
    thread_b.id = "message-location-thread-b".into();
    let stream_b = cx.new(|cx| ConversationStream::new(thread_b.clone(), cx));
    root.update(cx, |root, _| {
        root.stream_view = Some((thread_b.id.clone(), stream_b));
        root.update_message_location_route(Some(&thread_b.id));
    });
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.update_message_location_route(Some(&thread.id));
    });
    root.update(cx, |root, cx| {
        root.finish_message_location_worker(
            stream.clone(),
            latest_request,
            Ok(Some(stale_page)),
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| {
            (
                stream.hydrated_entry_count(),
                stream.scroll_anchor_snapshot(),
            )
        }),
        before_stale_request,
        "A→B→A rejects a result from the first route generation"
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.message_location_status()),
        Some(MessageLocationStatus::Located)
    );
}

#[gpui_kit::test]
async fn unknown_message_location_result_projects_not_found(cx: &mut gpui_kit::TestAppContext) {
    let (store, thread, _dir) = seed_hydration_thread(1);
    let (root, stream) = message_location_root(cx, store, &thread);
    root.update(cx, |root, _| root.message_location_request_generation = 1);
    let route_generation = root.read_with(cx, |root, _| root.message_location_route_generation);
    root.update(cx, |root, cx| {
        root.finish_message_location_worker(
            stream.clone(),
            MessageLocationWorkerRequest {
                thread_id: thread.id.clone(),
                message_id: "unknown-message".into(),
                route_generation,
                request_generation: 1,
                restore_anchor: None,
            },
            Ok(None),
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.message_location_status()),
        Some(MessageLocationStatus::NotFound)
    );
}

#[gpui_kit::test]
async fn newer_history_worker_result_is_fenced_across_active_a_b_a_reuse(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (store, thread, _dir) = seed_hydration_thread(150);
    let current_page = vega_conversation::history::history_page_containing_message(
        &store,
        &thread.id,
        "assistant-75",
        vega_store::messages::PAGE_LIMIT,
    )
    .unwrap()
    .unwrap();
    let after = current_page
        .newer_cursor
        .expect("the containing page keeps a newer boundary");
    let newer_page = vega_conversation::history::history_page_after(
        &store,
        &thread.id,
        after,
        vega_store::messages::PAGE_LIMIT,
    )
    .unwrap();
    let (root, stream) = message_location_root(cx, store, &thread);
    stream.update(cx, |stream, cx| stream.apply_history_page(current_page, cx));
    root.update(cx, |root, _| {
        root.agent_controller
            .begin(thread.id.clone(), stream.clone(), None, None)
            .expect("active stream remains cached across route switches");
    });
    assert_eq!(
        root.read_with(cx, |root, _| root
            .agent_controller
            .active_stream_for_thread(&thread.id)),
        Some(stream.clone())
    );
    let route_generation = root.read_with(cx, |root, _| root.message_location_route_generation);
    let before = stream.read_with(cx, |stream, _| {
        (
            stream.hydrated_entry_count(),
            stream.scroll_anchor_snapshot(),
            stream.newer_history_cursor(),
        )
    });
    let request = NewerHistoryPageRequested {
        thread_id: thread.id.clone(),
        after,
    };
    let mut thread_b = thread.clone();
    thread_b.id = "newer-history-thread-b".into();
    let stream_b = cx.new(|cx| ConversationStream::new(thread_b.clone(), cx));
    cx.update(|cx| cx.set_global(OpenedThread(Some(thread_b.clone()))));
    root.update(cx, |root, _| {
        root.stream_view = Some((thread_b.id.clone(), stream_b));
        root.update_message_location_route(Some(&thread_b.id));
    });
    cx.update(|cx| cx.set_global(OpenedThread(Some(thread.clone()))));
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.update_message_location_route(Some(&thread.id));
    });
    assert!(root.read_with(cx, |root, _| {
        root.message_location_route_generation > route_generation
            && root
                .stream_view
                .as_ref()
                .is_some_and(|(thread_id, cached)| thread_id == &thread.id && cached == &stream)
            && root
                .agent_controller
                .active_stream_for_thread(&thread.id)
                .as_ref()
                == Some(&stream)
    }));
    root.update(cx, |root, cx| {
        root.finish_newer_history_page(
            stream.clone(),
            route_generation,
            request,
            Ok(newer_page),
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| {
            (
                stream.hydrated_entry_count(),
                stream.scroll_anchor_snapshot(),
                stream.newer_history_cursor(),
            )
        }),
        before,
        "entity reuse cannot admit a newer page from the prior route generation"
    );
}

#[gpui_kit::test]
async fn location_worker_completion_defers_if_a_run_started_in_flight(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (store, thread, _dir) = seed_hydration_thread(150);
    let page = vega_conversation::history::history_page_containing_message(
        &store,
        &thread.id,
        "assistant-100",
        vega_store::messages::PAGE_LIMIT,
    )
    .unwrap()
    .unwrap();
    let (root, stream) = message_location_root(cx, store, &thread);
    root.update(cx, |root, _| {
        root.message_location_request_generation = 1;
        root.agent_controller
            .begin(thread.id.clone(), stream.clone(), None, None)
            .expect("active run");
    });
    let request = MessageLocationWorkerRequest {
        thread_id: thread.id.clone(),
        message_id: "assistant-100".into(),
        route_generation: root.read_with(cx, |root, _| root.message_location_route_generation),
        request_generation: 1,
        restore_anchor: None,
    };
    root.update(cx, |root, cx| {
        root.finish_message_location_worker(stream.clone(), request, Ok(Some(page)), cx)
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.message_location_status()),
        Some(MessageLocationStatus::Deferred)
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.hydrated_entry_count()),
        0
    );
    assert!(root.read_with(cx, |root, _| root.deferred_message_location.is_some()));
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
