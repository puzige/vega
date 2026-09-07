//! Actual root subscriptions, owned DB/project and real worker; provider boundary only is mocked.
use super::*;
use gpui::{Bounds, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, px, size};

struct Fixture {
    _data: TempDir,
    repo: TempDir,
    store: Store,
    thread: Thread,
    root: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: WindowHandle<VegaWindow>,
}

fn fixture(cx: &mut gpui::TestAppContext, provider: Arc<vega_runtime::MockProvider>) -> Fixture {
    let data = tempfile::tempdir().expect("owned composer data");
    let repo = diff_controller_repo();
    fs::write(repo.path().join("context.txt"), "owned context").expect("context fixture");
    let config = data.path().join("config.toml");
    super::model_selection::model_selection_config(&config);
    let database = data.path().join("vega.db");
    let store = Store::open(&database).expect("owned DB");
    store.migrate().expect("migrate");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("path"),
        "composer",
        None,
    )
    .expect("project");
    let thread =
        vega_conversation::threads::create_thread(&store, &project.id, "gpt-5.6-terra", "confirm")
            .expect("thread");
    cx.update(|cx| {
        install_diff_window_globals(Store::open(&database).expect("root DB"), thread.clone(), cx)
    });
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config);
        root.agent_provider_override = Some(provider);
    });
    let window_root = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.), px(900.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |_, _| window_root,
        )
        .expect("production root")
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view.is_some()
                && root.configured_models.is_some()
                && !root.model_catalog_loading
                && matches!(
                    root.pricing_controller.state,
                    PricingControllerState::Ready { .. }
                )
        })
    });
    let stream = root.read_with(cx, |root, _| {
        root.stream_view.as_ref().expect("stream").1.clone()
    });
    Fixture {
        _data: data,
        repo,
        store,
        thread,
        root,
        stream,
        window,
    }
}

fn edit(f: &Fixture, text: &str, cx: &mut gpui::TestAppContext) {
    let input = f.stream.read_with(cx, |stream, _| stream.composer_input());
    input.update(cx, |input, cx| input.set_text(text, cx));
    f.window
        .update(cx, |_, window, cx| {
            f.stream
                .update(cx, |stream, cx| stream.focus_composer(window, cx))
        })
        .expect("focus input");
    cx.run_until_parked();
}

fn draft(f: &Fixture, cx: &mut gpui::TestAppContext) -> String {
    f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).text().to_owned()
    })
}

fn click(f: &Fixture, selector: &'static str, cx: &mut gpui::TestAppContext) {
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    let bounds = visual
        .debug_bounds(selector)
        .expect("visible production control");
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
    visual.run_until_parked();
}

fn assert_terminal(f: &Fixture, cx: &mut gpui::TestAppContext) {
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_none())
    });
    assert!(
        !f.stream
            .read_with(cx, |stream, cx| stream.model_selection_blocked(cx))
    );
    assert!(
        !f.stream
            .read_with(cx, |stream, _| stream.composer_submission_pending())
    );
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.controller_error_message().is_none())
    );
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    assert!(visual.debug_bounds("composer-stopped").is_some());
    assert!(visual.debug_bounds("composer-stop").is_none());
}

#[gpui::test]
async fn r11_composer_preparation_stop_preserves_draft_and_prevents_late_start(
    cx: &mut gpui::TestAppContext,
) {
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("must not start"),
    ]));
    let f = fixture(cx, provider.clone());
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let probe = f
        .root
        .read_with(cx, |root, _| root.agent_worker_start_probe.clone());
    *probe
        .provider_construction_gate
        .lock()
        .expect("existing construction gate") = Some((entered_tx, release_rx));
    edit(&f, "retained early draft", cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("real worker reached provider boundary");
    cx.simulate_keystrokes(f.window.into(), "shift-tab enter");
    let cancel = f.root.read_with(cx, |root, _| {
        root.agent_controller
            .active
            .as_ref()
            .expect("ownership retained during drain")
            .cancel
            .clone()
    });
    assert!(cancel.is_cancelled());
    assert_eq!(draft(&f, cx), "retained early draft");
    release_tx.send(()).expect("release construction boundary");
    assert_terminal(&f, cx);
    assert_eq!(draft(&f, cx), "retained early draft");
    assert!(provider.requests().is_empty());
    let messages: i64 = f
        .store
        .conn()
        .query_row(
            "SELECT count(*) FROM messages WHERE thread_id = ?1",
            [&f.thread.id],
            |row| row.get(0),
        )
        .expect("message count");
    assert_eq!(
        messages, 0,
        "no late durable ack/commit after cancelled preparation"
    );
    assert_eq!(probe.load(), 1);
}

#[gpui::test]
async fn r11_composer_stream_stop_retains_partial_and_next_draft(cx: &mut gpui::TestAppContext) {
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("retained partial text"),
        vega_runtime::ScriptStep::delay(Duration::from_secs(30)),
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::ToolUse {
                id: "late-write".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf late > late-marker"}"#.into(),
            },
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::ToolUse,
            },
        ]),
    ]));
    let f = fixture(cx, provider.clone());
    edit(&f, "start cancellable stream", cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
    pump_test_app(cx, |_| {
        f.store.conn().query_row("SELECT count(*) FROM messages WHERE role = 'assistant' AND content LIKE '%retained partial text%'", [], |row| row.get::<_, i64>(0)).expect("partial persistence") == 1
    });
    assert_eq!(draft(&f, cx), "", "durable start accepted submitted text");
    edit(&f, "unsent next draft", cx);
    click(&f, "composer-stop", cx);
    assert_terminal(&f, cx);
    assert_eq!(draft(&f, cx), "unsent next draft");
    let (status, content): (String, String) = f
        .store
        .conn()
        .query_row(
            "SELECT status, content FROM messages WHERE role = 'assistant'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("durable partial");
    assert_eq!(status, "interrupted");
    assert!(content.contains("retained partial text"));
    assert!(!f.repo.path().join("late-marker").exists());
    assert_eq!(provider.requests().len(), 1);
    let tools: i64 = f
        .store
        .conn()
        .query_row("SELECT count(*) FROM tool_calls", [], |row| row.get(0))
        .expect("late tool count");
    assert_eq!(
        tools, 0,
        "cancelled delayed round emits no late tool proposal"
    );
    for _ in 0..20 {
        cx.executor().advance_clock(AGENT_EVENT_POLL);
        cx.run_until_parked();
    }
    assert_eq!(draft(&f, cx), "unsent next draft");
    assert!(!f.repo.path().join("late-marker").exists());
    assert!(!cx.update(|cx| cx.global::<SettingsOpen>().0));
}

#[gpui::test]
async fn r11_composer_context_and_slash_keyboard_use_real_mode_and_file_handlers(
    cx: &mut gpui::TestAppContext,
) {
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let f = fixture(cx, provider.clone());
    edit(&f, "kept context", cx);
    cx.simulate_keystrokes(f.window.into(), "tab enter enter");
    assert_eq!(draft(&f, cx), "kept context @");
    pump_test_app(cx, |cx| {
        f.stream
            .read_with(cx, |stream, _| stream.file_index_loaded())
    });
    assert!(f.stream.read_with(cx, |stream, _| {
        stream
            .file_index_candidates()
            .contains(&"context.txt".to_owned())
    }));
    edit(&f, "kept context @context", cx);
    cx.simulate_keystrokes(f.window.into(), "tab");
    assert!(draft(&f, cx).contains("@context.txt"));

    edit(&f, "/p  keep 日本語\nnext line", cx);
    cx.simulate_keystrokes(f.window.into(), "enter");
    cx.run_until_parked();
    let persisted =
        vega_conversation::threads::open_thread(&f.store, &f.thread.id).expect("real durable mode");
    assert_eq!(persisted.mode, ThreadMode::Plan);
    assert_eq!(draft(&f, cx), "  keep 日本語\nnext line");
    edit(&f, "/ask keep escape", cx);
    cx.simulate_keystrokes(f.window.into(), "escape");
    assert_eq!(draft(&f, cx), "/ask keep escape");
    assert_eq!(
        vega_conversation::threads::open_thread(&f.store, &f.thread.id)
            .expect("mode after escape")
            .mode,
        ThreadMode::Plan
    );
    edit(&f, "/  remainder", cx);
    cx.simulate_keystrokes(f.window.into(), "down down tab");
    assert_eq!(
        vega_conversation::threads::open_thread(&f.store, &f.thread.id)
            .expect("execute mode")
            .mode,
        ThreadMode::Execute
    );
    assert_eq!(draft(&f, cx), "  remainder");
    click(&f, "composer-add", cx);
    cx.simulate_keystrokes(f.window.into(), "down enter");
    assert_eq!(
        vega_conversation::threads::open_thread(&f.store, &f.thread.id)
            .expect("menu ask mode")
            .mode,
        ThreadMode::Ask
    );
    assert_eq!(draft(&f, cx), "  remainder");
    cx.update(|cx| {
        cx.global::<VegaStore>()
            .0
            .as_ref()
            .expect("global store")
            .conn()
            .execute_batch("PRAGMA query_only=ON")
            .expect("owned DB rejects mode write")
    });
    edit(&f, "/plan preserve rejected draft", cx);
    cx.simulate_keystrokes(f.window.into(), "enter");
    assert_eq!(draft(&f, cx), "/plan preserve rejected draft");
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.controller_error_message().is_some())
    );
    assert_eq!(
        vega_conversation::threads::open_thread(&f.store, &f.thread.id)
            .expect("unchanged mode")
            .mode,
        ThreadMode::Ask
    );
    cx.update(|cx| {
        cx.global::<VegaStore>()
            .0
            .as_ref()
            .expect("global store")
            .conn()
            .execute_batch("PRAGMA query_only=OFF")
            .expect("restore owned DB")
    });
    assert!(
        provider.requests().is_empty(),
        "local command never sends a prompt"
    );
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.agent_worker_start_probe.load()),
        0
    );
}

#[gpui::test]
async fn r11_composer_stop_revokes_pending_permission_without_tool_execution(
    cx: &mut gpui::TestAppContext,
) {
    // Same real-worker wake allowance as production_app_entry_keeps_permission tests.
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::ToolUse {
                id: "stop-permission".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf denied > stop-permission-marker"}"#.into(),
            },
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::ToolUse,
            },
        ]),
    ]));
    let f = fixture(cx, provider.clone());
    edit(&f, "stop a pending permission", cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        f.stream
            .read_with(cx, |stream, _| stream.has_active_permission_card())
    });
    click(&f, "composer-stop", cx);
    assert_terminal(&f, cx);
    assert!(
        !f.stream
            .read_with(cx, |stream, _| stream.has_pending_permission())
    );
    assert!(
        !f.stream
            .read_with(cx, |stream, _| stream.has_active_permission_card())
    );
    assert!(!f.repo.path().join("stop-permission-marker").exists());
    let state = vega_store::tool_calls::find_state(f.store.conn(), "stop-permission")
        .expect("audit")
        .expect("persisted tool proposal");
    // Existing cancelled_permission contract is a rejected tool with Timeout audit.
    assert_eq!(state.status, "rejected");
    let audit = ApprovalAudit::from_json(state.approval.as_deref().expect("permission audit"))
        .expect("typed audit");
    assert_eq!(audit.decision, Approval::Deny);
    assert_eq!(audit.source, ApprovalSource::Timeout);
    assert_eq!(provider.requests().len(), 1);
}
