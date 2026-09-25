//! Actual root subscriptions, owned DB/project and real worker; provider boundary only is mocked.
use super::*;
use gpui_kit::{Bounds, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, px, size};

#[path = "context_compaction.rs"]
mod context_compaction;

struct Fixture {
    _data: TempDir,
    repo: ControllerRepo,
    store: Store,
    thread: Thread,
    root: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: WindowHandle<VegaWindow>,
}

fn fixture(
    cx: &mut gpui_kit::TestAppContext,
    provider: Arc<vega_runtime::MockProvider>,
) -> Fixture {
    let data = tempfile::tempdir().expect("owned composer data");
    let repo = diff_controller_repo();
    fs::write(repo.path().join("context.txt"), "owned context").expect("context fixture");
    let config = data.path().join("config.toml");
    super::model_selection::model_selection_config(&config);
    vega_store::keystore::set_key(data.path(), "owned", "composer-test-key")
        .expect("owned composer credential");
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
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider));
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

fn edit(f: &Fixture, text: &str, cx: &mut gpui_kit::TestAppContext) {
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

fn draft(f: &Fixture, cx: &mut gpui_kit::TestAppContext) -> String {
    f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).text().to_owned()
    })
}

fn click(f: &Fixture, selector: &'static str, cx: &mut gpui_kit::TestAppContext) {
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    let bounds = visual
        .debug_bounds(selector)
        .expect("visible production control");
    visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
}

fn assert_composer_control_size(
    f: &Fixture,
    selector: &'static str,
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.run_until_parked();
    let bounds = VisualTestContext::from_window(f.window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    for (axis, actual) in [("width", bounds.size.width), ("height", bounds.size.height)] {
        let actual = f32::from(actual);
        assert!(
            (actual - Layout::COMPOSER_SEND_SIZE).abs() <= 1.0,
            "{selector} {axis}: expected {}±1px, got {actual}px",
            Layout::COMPOSER_SEND_SIZE
        );
    }
}

fn assert_terminal(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_empty())
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
    assert!(visual.debug_bounds("composer-stopped").is_none());
    assert!(visual.debug_bounds("composer-stop").is_none());
}

#[gpui_kit::test]
async fn i61_provider_reasoning_reaches_live_ui_without_persisting_as_answer(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    use vega_runtime::{ProviderEvent, ScriptStep, StopReason};
    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("inspect ".into()),
            ProviderEvent::ThinkingDelta("owned context".into()),
            ProviderEvent::TextDelta("Checking context. ".into()),
            ProviderEvent::ToolUse {
                id: "thinking-grep".into(),
                name: "grep".into(),
                input_json: r#"{"pattern":"owned","path":"."}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("validate result".into()),
            ProviderEvent::TextDelta("Context checked.".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]));
    let f = fixture(cx, provider.clone());
    edit(&f, "check owned context", cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        provider.requests().len() == 2
            && f.root
                .read_with(cx, |root, _| root.agent_controller.active.is_empty())
    });
    let answer: String = f.store.conn().query_row(
        "SELECT content FROM messages WHERE thread_id = ?1 AND role = 'assistant' ORDER BY seq DESC LIMIT 1",
        [&f.thread.id], |row| row.get(0),
    ).expect("persisted answer");
    assert!(answer.contains("Checking context.") && answer.contains("Context checked."));
    assert!(!answer.contains("inspect") && !answer.contains("validate result"));
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    assert!(visual.debug_bounds("run-activity-toggle").is_some());
    assert!(visual.debug_bounds("thinking-block").is_none());
    let run_toggle = visual
        .debug_bounds("run-activity-toggle")
        .expect("terminal run header");
    visual.simulate_click(run_toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-block").is_some());
    assert!(visual.debug_bounds("thinking-content").is_none());
    // #151 R151-5: a manual toggle still discloses the block's bounded text.
    let toggle = visual
        .debug_bounds("thinking-toggle")
        .expect("production thinking header");
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_some());
}

/// #151 R151-1 in the real app: the newest live thinking block discloses its
/// bounded reasoning with no user action. This round emits only reasoning and
/// then ends, so nothing newer supersedes the block before the assertion.
#[gpui_kit::test]
async fn i61_newest_live_thinking_block_is_expanded_by_default(cx: &mut gpui_kit::TestAppContext) {
    cx.executor().allow_parking();
    use vega_runtime::{ProviderEvent, ScriptStep, StopReason};
    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![vec![
        ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("first block reasoning".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ]),
    ]]));
    let f = fixture(cx, provider.clone());
    edit(&f, "show me the newest reasoning", cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
    // This round emits only reasoning and then ends: no text delta and no tool
    // call can supersede it, so the block stays the current activity unit and
    // must be open with no user action.
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1
            && f.root
                .read_with(cx, |root, _| root.agent_controller.active.is_empty())
    });
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    assert!(visual.debug_bounds("run-activity-toggle").is_some());
    assert!(visual.debug_bounds("thinking-block").is_none());
    let run_toggle = visual
        .debug_bounds("run-activity-toggle")
        .expect("terminal run header");
    visual.simulate_click(run_toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-block").is_some());
    assert!(
        visual.debug_bounds("thinking-content").is_some(),
        "#151 R151-1: the newest reasoning block stays expanded inside the reopened run"
    );
}

#[gpui_kit::test]
async fn r11_composer_preparation_stop_preserves_draft_and_prevents_late_start(
    cx: &mut gpui_kit::TestAppContext,
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
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| !root.agent_controller.active.is_empty())
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("real worker reached provider boundary");
    cx.simulate_keystrokes(f.window.into(), "shift-tab enter");
    let cancel = f.root.read_with(cx, |root, _| {
        root.agent_controller
            .active
            .values()
            .next()
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
    assert!(
        VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds("run-activity-status")
            .is_none(),
        "preparation cancellation has no durable activity to label"
    );
    assert_eq!(probe.load(), 1);
}

#[gpui_kit::test]
async fn r11_composer_stream_stop_retains_partial_and_next_draft(
    cx: &mut gpui_kit::TestAppContext,
) {
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
    assert_composer_control_size(&f, "composer-send", cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        f.store
            .conn()
            .query_row(
                "SELECT count(*) FROM messages WHERE role = 'assistant' AND content LIKE '%retained partial text%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("partial persistence")
            == 1
            && draft(&f, cx).is_empty()
    });
    assert_eq!(draft(&f, cx), "", "durable start accepted submitted text");
    edit(&f, "unsent next draft", cx);
    assert_composer_control_size(&f, "composer-stop", cx);
    click(&f, "composer-stop", cx);
    assert_terminal(&f, cx);
    assert!(
        VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds("run-activity-status")
            .is_some(),
        "a cancelled durable run keeps its terminal status in RunActivity"
    );
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

#[gpui_kit::test]
async fn r11_composer_context_and_slash_keyboard_use_real_mode_and_file_handlers(
    cx: &mut gpui_kit::TestAppContext,
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

#[gpui_kit::test]
async fn r57_plus_menu_permission_selection_persists_through_the_real_controller(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let f = fixture(cx, provider.clone());
    // The fixture thread starts at `execute` + `confirm`.
    assert_eq!(
        vega_conversation::threads::open_thread(&f.store, &f.thread.id)
            .expect("initial permission")
            .permission_mode,
        PermissionMode::Confirm
    );
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    let add = visual.debug_bounds("composer-add").expect("composer add");
    visual.simulate_click(add.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();

    // The group is really rendered, with the current value marked.
    assert!(
        visual
            .debug_bounds("composer-action-permission-readonly")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("composer-action-permission-auto")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("composer-action-permission-confirm-check")
            .is_some(),
        "the current permission mode must be marked"
    );

    // One click on the production row persists through the real
    // `ThreadSettingsRequested` -> `persist_thread_settings` path.
    let auto = visual
        .debug_bounds("composer-action-permission-auto")
        .expect("auto row");
    visual.simulate_click(auto.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();

    assert_eq!(
        vega_conversation::threads::open_thread(&f.store, &f.thread.id)
            .expect("durable permission after menu")
            .permission_mode,
        PermissionMode::Auto
    );
    // The composer reflects the new authoritative value: the bottom-row
    // control P2b will remove still reads the same durable thread, and the
    // reopened menu marks the new row.
    let displayed = f
        .stream
        .read_with(cx, |stream, _| stream.thread_permission_mode());
    assert_eq!(
        displayed,
        PermissionMode::Auto,
        "composer projection follows the durable permission mode"
    );
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    let add = visual
        .debug_bounds("composer-add")
        .expect("composer add again");
    visual.simulate_click(add.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(
        visual
            .debug_bounds("composer-action-permission-auto-check")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("composer-action-permission-confirm-check")
            .is_none()
    );
    // A local permission change never starts a run.
    assert!(provider.requests().is_empty());
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.agent_worker_start_probe.load()),
        0
    );
}

#[gpui_kit::test]
async fn i58_full_access_mouse_keyboard_and_reopen_use_the_real_controller(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let f = fixture(cx, provider.clone());
    click(&f, "composer-permission-status", cx);
    click(&f, "composer-permission-option-full_access", cx);
    let persisted = vega_conversation::threads::open_thread(&f.store, &f.thread.id)
        .expect("picker persisted full access");
    assert_eq!(persisted.permission_mode, PermissionMode::FullAccess);
    assert_eq!(
        f.stream
            .read_with(cx, |stream, _| stream.thread_permission_mode()),
        PermissionMode::FullAccess
    );

    click(&f, "composer-permission-status", cx);
    assert!(
        VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds("composer-permission-option-full_access-check")
            .is_some()
    );
    click(&f, "composer-permission-option-confirm", cx);
    click(&f, "composer-add", cx);
    click(&f, "composer-action-permission-full_access", cx);
    assert_eq!(
        vega_conversation::threads::open_thread(&f.store, &f.thread.id)
            .expect("plus menu persisted full access")
            .permission_mode,
        PermissionMode::FullAccess
    );
    click(&f, "composer-permission-status", cx);
    click(&f, "composer-permission-option-confirm", cx);
    click(&f, "composer-add", cx);
    // file, ask, plan, execute, readonly, confirm, auto, full_access.
    cx.simulate_keystrokes(f.window.into(), "down down down down down down down enter");
    cx.run_until_parked();
    let reopened_store = Store::open(f._data.path().join("vega.db")).expect("reopen owned DB");
    let reopened = vega_conversation::threads::open_thread(&reopened_store, &f.thread.id)
        .expect("reopen full access thread");
    assert_eq!(reopened.permission_mode, PermissionMode::FullAccess);

    // Visit another real thread, then reopen the persisted original through
    // the production navigation controller: permission never leaks by route.
    let other = vega_conversation::threads::create_thread(
        &f.store,
        &f.thread.project_id,
        "gpt-5.6-terra",
        "confirm",
    )
    .expect("independent thread");
    f.root.update(cx, |root, cx| {
        assert!(root.accept_palette_thread(other, cx));
    });
    cx.run_until_parked();
    assert_eq!(
        f.root.read_with(cx, |root, cx| {
            root.stream_view
                .as_ref()
                .expect("other stream")
                .1
                .read(cx)
                .thread_permission_mode()
        }),
        PermissionMode::Confirm
    );
    f.root.update(cx, |root, cx| {
        assert!(root.accept_palette_thread(reopened, cx));
    });
    cx.run_until_parked();
    assert_eq!(
        f.root.read_with(cx, |root, cx| {
            root.stream_view
                .as_ref()
                .expect("reopened stream")
                .1
                .read(cx)
                .thread_permission_mode()
        }),
        PermissionMode::FullAccess
    );
    click(&f, "composer-add", cx);
    assert!(
        VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds("composer-action-permission-full_access-check")
            .is_some()
    );
    click(&f, "composer-action-permission-full_access", cx);
    click(&f, "composer-permission-status", cx);
    click(&f, "composer-permission-option-confirm", cx);
    assert_eq!(
        vega_conversation::threads::open_thread(&f.store, &f.thread.id)
            .expect("restored confirm")
            .permission_mode,
        PermissionMode::Confirm
    );
    assert!(provider.requests().is_empty());
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.agent_worker_start_probe.load()),
        0
    );
}

#[gpui_kit::test]
async fn r57_plus_menu_thread_modes_persist_through_the_real_controller(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let f = fixture(cx, provider.clone());
    // P1 evidence for the pre-existing `/ask` `/plan` `/execute` entries: they
    // were reachable but had no production test proving the `+`-menu click
    // path persists. Rows: file(0) ask(1) plan(2) execute(3) permission(4..6).
    for (row, expected) in [
        ("composer-action-mode-ask", ThreadMode::Ask),
        ("composer-action-mode-plan", ThreadMode::Plan),
        ("composer-action-mode-execute", ThreadMode::Execute),
    ] {
        let mut visual = VisualTestContext::from_window(f.window.into(), cx);
        let add = visual.debug_bounds("composer-add").expect("composer add");
        visual.simulate_click(add.center(), gpui_kit::Modifiers::default());
        visual.run_until_parked();
        let bounds = visual
            .debug_bounds(row)
            .unwrap_or_else(|| panic!("missing {row}"));
        visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        visual.run_until_parked();
        cx.run_until_parked();
        assert_eq!(
            vega_conversation::threads::open_thread(&f.store, &f.thread.id)
                .expect("durable mode after menu")
                .mode,
            expected,
            "{row} must persist its thread mode"
        );
        assert!(
            f.stream
                .read_with(cx, |stream, _| stream.thread_mode() == expected),
            "{row} must be reflected by the composer"
        );
    }
    // A permission-mode change made alongside must not be reverted by a later
    // mode change: the two groups share one request payload.
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    let add = visual.debug_bounds("composer-add").expect("composer add");
    visual.simulate_click(add.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    let readonly = visual
        .debug_bounds("composer-action-permission-readonly")
        .expect("readonly row");
    visual.simulate_click(readonly.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    cx.run_until_parked();
    let persisted = vega_conversation::threads::open_thread(&f.store, &f.thread.id)
        .expect("durable settings after both groups");
    assert_eq!(persisted.permission_mode, PermissionMode::ReadOnly);
    assert_eq!(persisted.mode, ThreadMode::Execute);
    assert!(provider.requests().is_empty());
}

#[gpui_kit::test]
async fn r11_composer_stop_revokes_pending_permission_without_tool_execution(
    cx: &mut gpui_kit::TestAppContext,
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

/// #150 R14/R15: the running-state action is a brand-blue primary surface.
///
/// The assertion reads the real painted scene (the same `painted_quads`
/// production path the utility-bar tests use) rather than the source, so
/// reverting the fill to the old neutral `bg_hover` fails here. The
/// non-running `composer-send` branch must stay on `accent` (R15).
#[gpui_kit::test]
async fn issue150_running_stop_button_is_brand_blue_and_send_is_unchanged(
    cx: &mut gpui_kit::TestAppContext,
) {
    fn same_colour(actual: gpui_kit::Hsla, expected: gpui_kit::Rgba) -> bool {
        let actual = gpui_kit::Rgba::from(actual);
        [
            (actual.r, expected.r),
            (actual.g, expected.g),
            (actual.b, expected.b),
            (actual.a, expected.a),
        ]
        .into_iter()
        .all(|(actual, expected)| (actual - expected).abs() <= 1.0 / 255.0)
    }

    /// Whether a solid fill of `expected` is painted at exactly `selector`'s
    /// bounds. A containment check would let an ancestor surface pass.
    fn paints(
        f: &Fixture,
        selector: &'static str,
        expected: gpui_kit::Rgba,
        cx: &mut gpui_kit::TestAppContext,
    ) -> bool {
        cx.run_until_parked();
        let target = VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing {selector}"));
        f.window
            .update(cx, |_, window, _| {
                let scale = window.scale_factor();
                window.painted_quads().into_iter().any(|quad| {
                    let Some(solid) = quad.background.as_solid() else {
                        return false;
                    };
                    if !same_colour(solid, expected) {
                        return false;
                    }
                    let tolerance = 0.5;
                    (quad.bounds.left().as_f32() / scale - f32::from(target.left())).abs()
                        <= tolerance
                        && (quad.bounds.right().as_f32() / scale - f32::from(target.right())).abs()
                            <= tolerance
                        && (quad.bounds.top().as_f32() / scale - f32::from(target.top())).abs()
                            <= tolerance
                        && (quad.bounds.bottom().as_f32() / scale - f32::from(target.bottom()))
                            .abs()
                            <= tolerance
                })
            })
            .expect("composer window must remain open")
    }

    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("running"),
        vega_runtime::ScriptStep::delay(Duration::from_secs(30)),
    ]));
    let f = fixture(cx, provider);
    let colors = cx.update(|cx| vega_theme::theme(cx).colors);

    // R15: a sendable draft keeps the existing accent fill. The draft must be
    // non-empty first: `can_send` is what selects the enabled accent branch,
    // and an empty draft is a legitimately disabled (neutral) button.
    edit(&f, "start a run for the stop button", cx);
    assert!(
        paints(&f, "composer-send", colors.accent, cx),
        "the sendable send button must keep its accent fill"
    );

    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds("composer-stop")
            .is_some()
    });

    // R14: the running action is brand blue, not the former neutral hover grey.
    assert!(
        paints(&f, "composer-stop", colors.accent, cx),
        "the running stop button must be filled with the brand accent"
    );
    assert!(
        !paints(&f, "composer-stop", colors.bg_hover, cx),
        "the running stop button must no longer use the neutral hover fill"
    );
}

/// #150 C8 (E2E-REAL): the projection the sidebar reads is driven by the real
/// run lifecycle through the production submit and terminal paths — it is not
/// a render-time guess. The sidebar row therefore lights up and clears with the
/// actual worker, and the real controller's `active` map is the source.
#[gpui_kit::test]
async fn issue150_running_indicator_follows_the_real_run_lifecycle(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("lifecycle"),
        vega_runtime::ScriptStep::delay(Duration::from_secs(30)),
    ]));
    let f = fixture(cx, provider);
    let indicator: &'static str =
        Box::leak(format!("project-thread-running-{}", f.thread.id).into_boxed_str());

    // At rest the projection is empty and the row tail shows no indicator.
    assert!(
        !cx.update(|cx| vega_ui::sidebar::thread_is_running(&f.thread.id, cx)),
        "no run has started yet"
    );
    assert!(
        VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds(indicator)
            .is_none()
    );

    // A real submit starts the worker: the projection turns true and the
    // sidebar row renders the indicator.
    edit(&f, "run for the lifecycle proof", cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds("composer-stop")
            .is_some()
    });
    assert!(
        f.root.read_with(cx, |root, _| root
            .agent_controller
            .active
            .contains_key(&f.thread.id)),
        "the real controller must own the accepted run"
    );
    assert!(
        cx.update(|cx| vega_ui::sidebar::thread_is_running(&f.thread.id, cx)),
        "an accepted run must project thread liveness"
    );
    assert!(
        VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds(indicator)
            .is_some(),
        "the running row must render its indicator"
    );

    // Cancelling reaches the real terminal path, which must clear the
    // projection so the row stops spinning.
    click(&f, "composer-stop", cx);
    assert_terminal(&f, cx);
    assert!(
        !cx.update(|cx| vega_ui::sidebar::thread_is_running(&f.thread.id, cx)),
        "the terminal must clear thread liveness"
    );
    assert!(
        VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds(indicator)
            .is_none(),
        "a finished run must leave no indicator behind"
    );
}
