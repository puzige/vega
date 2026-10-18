//! Actual root subscriptions, owned DB/project and real worker; provider boundary only is mocked.
use super::*;
use gpui_kit::{Bounds, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, px, size};

#[path = "context_compaction.rs"]
mod context_compaction;

#[path = "pending_messages.rs"]
mod pending_messages;

#[path = "automation.rs"]
mod automation;

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
                && root.agent_controller.queue_operations.is_empty()
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

#[gpui_kit::test]
async fn i61_v3_thinking_only_completion_keeps_summary_with_collapsed_details(
    cx: &mut gpui_kit::TestAppContext,
) {
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
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1
            && f.root
                .read_with(cx, |root, _| root.agent_controller.active.is_empty())
    });
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    let run_toggle = visual
        .debug_bounds("run-activity-toggle")
        .expect("terminal run header");
    assert!(visual.debug_bounds("run-activity-status").is_some());
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("thinking-content").is_none());
    visual.simulate_click(run_toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("run-activity-content").is_none());
    assert!(visual.debug_bounds("run-stage-title-0").is_some());
    assert!(visual.debug_bounds("thinking-content").is_none());
    let toggle = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("thought header");
    visual.simulate_click(toggle.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-block").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("thinking-content").is_some());
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
