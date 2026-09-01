use super::*;

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

#[gpui::test]
async fn late_hydration_page_is_dropped_after_route_replacement(cx: &mut gpui::TestAppContext) {
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
        #[cfg(test)]
        Some(provider),
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
