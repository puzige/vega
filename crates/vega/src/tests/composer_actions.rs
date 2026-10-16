//! Actual root subscriptions, owned DB/project and real worker; provider boundary only is mocked.
use super::*;
use gpui_kit::{Bounds, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, px, size};

#[path = "context_compaction.rs"]
mod context_compaction;

#[path = "pending_messages.rs"]
mod pending_messages;

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
