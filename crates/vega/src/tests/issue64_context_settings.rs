use super::*;
use gpui_kit::{
    Bounds, Modifiers, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, point, px,
    size,
};

struct Fixture {
    _data: TempDir,
    store: Store,
    thread: Thread,
    root: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: WindowHandle<VegaWindow>,
    provider: Arc<vega_runtime::MockProvider>,
}

fn fixture(cx: &mut gpui_kit::TestAppContext) -> Fixture {
    let data = tempfile::tempdir().expect("context Settings owned data");
    let config = data.path().join("config.toml");
    super::model_selection::model_selection_config(&config);
    let database = data.path().join("vega.db");
    let store = Store::open(&database).expect("context Settings owned database");
    store.migrate().expect("context Settings migrations");
    let thread =
        vega_conversation::threads::create_standalone_thread(&store, "gpt-5.6-terra", "confirm")
            .expect("context Settings standalone thread");
    vega_store::messages::insert(
        store.conn(),
        &vega_store::messages::MessageRow {
            id: "context-settings-owned-message".into(),
            thread_id: thread.id.clone(),
            seq: 1,
            role: "user".into(),
            kind: "text".into(),
            content: "owned context Settings history".into(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .expect("context Settings owned message");
    vega_store::context_compaction::save_model_policy(
        store.conn(),
        &vega_store::context_compaction::ModelContextPolicy {
            provider: "owned".into(),
            model: thread.model.clone(),
            input_limit: Some(100_000),
            output_reserve: Some(2_000),
            automatic_compaction: false,
            updated_at: 1,
        },
    )
    .expect("context Settings owned model policy");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database).expect("context Settings root database"),
            thread.clone(),
            cx,
        );
        cx.set_global(vega_ui::sidebar::SelectedProject(None));
    });
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config);
        root.agent_provider_override = Some(provider.clone());
    });
    let view = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1403.0), px(860.0)),
                    cx,
                ))),
                ..Default::default()
            },
            move |_, _| view,
        )
        .expect("context Settings production window")
    });
    cx.update(|cx| crate::app_palette::bind_shortcuts(window.into(), root.downgrade(), cx));
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view.is_some()
                && !root.model_catalog_loading
                && root.configured_models.is_some()
                && root.context_controller.last_load_succeeded == Some(true)
        })
    });
    let stream = root.read_with(cx, |root, _| {
        root.stream_view
            .as_ref()
            .expect("context Settings stream")
            .1
            .clone()
    });
    Fixture {
        _data: data,
        store,
        thread,
        root,
        stream,
        window,
        provider,
    }
}

fn bounds(
    f: &Fixture,
    selector: &'static str,
    cx: &mut gpui_kit::TestAppContext,
) -> Bounds<gpui_kit::Pixels> {
    cx.run_until_parked();
    VisualTestContext::from_window(f.window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing context Settings control {selector}"))
}

fn roundtrip(cx: &mut gpui_kit::TestAppContext, popover: bool, route_seam: bool) {
    cx.executor().allow_parking();
    let f = fixture(cx);
    let before_messages = vega_store::messages::recent(f.store.conn(), &f.thread.id, 100)
        .expect("owned messages before");
    let before_policy =
        vega_store::context_compaction::load_model_policy(f.store.conn(), "owned", &f.thread.model)
            .expect("owned policy before");
    let input = f.stream.read_with(cx, |stream, _| stream.composer_input());
    let input_focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
    f.window
        .update(cx, |_, window, cx| window.focus(&input_focus, cx))
        .expect("context Settings input focus");
    let indicator = bounds(&f, "composer-context-usage", cx);
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_mouse_move(indicator.center(), None, Modifiers::default());
    visual.run_until_parked();
    let tooltip = visual
        .debug_bounds("context-usage-tooltip")
        .expect("hovered context tooltip");
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-percentage")
            .is_some()
    );
    if popover {
        visual.simulate_mouse_move(tooltip.center(), None, Modifiers::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("context-usage-tooltip").is_some());
    }
    if route_seam {
        cx.update(|cx| {
            cx.set_global(SettingsOpen(true));
            cx.refresh_windows();
        });
    } else {
        let settings = bounds(&f, "sidebar-settings", cx);
        visual.simulate_click(settings.center(), Modifiers::default());
    }
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| root.settings_view.is_some())
    });
    assert!(cx.update(|cx| cx.global::<SettingsOpen>().0));
    assert!(visual.debug_bounds("composer-context-usage").is_none());
    assert!(visual.debug_bounds("context-usage-tooltip").is_none());
    assert_eq!(
        f.root.read_with(cx, |root, _| root
            .stream_view
            .as_ref()
            .expect("cached Settings stream")
            .1
            .clone()),
        f.stream
    );
    visual.simulate_mouse_move(point(px(1.0), px(1.0)), None, Modifiers::default());
    let back = bounds(&f, "settings-back", cx);
    visual.simulate_click(back.center(), Modifiers::default());
    pump_test_app(cx, |cx| {
        !cx.update(|cx| cx.global::<SettingsOpen>().0)
            && f.root.read_with(cx, |root, _| {
                root.settings_view.is_none()
                    && root.context_controller.last_load_succeeded == Some(true)
            })
    });
    assert_eq!(
        f.root.read_with(cx, |root, _| root
            .stream_view
            .as_ref()
            .expect("returned stream")
            .1
            .clone()),
        f.stream
    );
    let shell = bounds(&f, "composer-shell", cx);
    let composer_point = point(shell.center().x, shell.top() + px(25.0));
    visual.simulate_mouse_move(composer_point, None, Modifiers::default());
    visual.simulate_click(composer_point, Modifiers::default());
    visual.run_until_parked();
    assert!(
        f.window
            .update(cx, |_, window, _| input_focus.is_focused(window))
            .expect("actual Composer focus after return")
    );
    assert!(visual.debug_bounds("composer-context-usage").is_some());
    assert!(
        visual.debug_bounds("context-usage-tooltip").is_none(),
        "Settings return must not revive a stale context hover while Composer owns focus"
    );
    cx.simulate_keystrokes(f.window.into(), "tab");
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip").is_none());
    cx.simulate_keystrokes(f.window.into(), "tab");
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("context-usage-tooltip").is_some(),
        "legitimate keyboard focus must reopen the context tooltip after Settings"
    );
    cx.simulate_keystrokes(f.window.into(), "tab");
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip").is_none());
    cx.simulate_keystrokes(f.window.into(), "shift-tab");
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip").is_some());
    f.window
        .update(cx, |_, window, cx| window.focus(&input_focus, cx))
        .expect("return to Composer");
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip").is_none());
    let current_indicator = bounds(&f, "composer-context-usage", cx);
    visual.simulate_mouse_move(current_indicator.center(), None, Modifiers::default());
    visual.run_until_parked();
    let current_tooltip = visual
        .debug_bounds("context-usage-tooltip")
        .expect("fresh context hover after Settings");
    visual.simulate_mouse_move(current_tooltip.center(), None, Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip").is_some());
    visual.simulate_mouse_move(composer_point, None, Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip").is_none());
    assert_eq!(input.read_with(cx, |input, _| input.text().to_owned()), "");
    assert_eq!(
        vega_store::messages::recent(f.store.conn(), &f.thread.id, 100)
            .expect("owned messages after"),
        before_messages
    );
    assert_eq!(
        vega_store::context_compaction::load_model_policy(f.store.conn(), "owned", &f.thread.model)
            .expect("owned policy after"),
        before_policy
    );
    assert_eq!(
        f.stream
            .read_with(cx, |stream, _| stream.displayed_model().to_owned()),
        f.thread.model
    );
    assert!(f.provider.requests().is_empty());
}

#[gpui_kit::test]
async fn issue64_context_settings_sidebar_roundtrip_drops_trigger_hover(
    cx: &mut gpui_kit::TestAppContext,
) {
    roundtrip(cx, false, false);
}

#[gpui_kit::test]
async fn issue64_context_settings_sidebar_roundtrip_drops_popover_hover(
    cx: &mut gpui_kit::TestAppContext,
) {
    roundtrip(cx, true, false);
}

#[gpui_kit::test]
async fn issue64_context_settings_route_roundtrip_drops_trigger_hover(
    cx: &mut gpui_kit::TestAppContext,
) {
    roundtrip(cx, false, true);
}

#[gpui_kit::test]
async fn issue64_context_settings_route_roundtrip_drops_popover_hover(
    cx: &mut gpui_kit::TestAppContext,
) {
    roundtrip(cx, true, true);
}
