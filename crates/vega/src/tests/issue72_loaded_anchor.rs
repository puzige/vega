use super::*;
use gpui_kit::{
    Bounds, KeyDownEvent, Keystroke, Modifiers, MouseButton, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, point, px,
    size,
};

const FIRST_ID: &str = "issue72-owned-message-00";
const ALPHA_ID: &str = "issue72-owned-message-13";

#[derive(serde::Deserialize)]
struct FixtureMessage {
    id: String,
    seq: i64,
    role: String,
    content: String,
}

struct Fixture {
    _data: TempDir,
    store: Store,
    thread: Thread,
    other_thread: Thread,
    root: Entity<VegaWindow>,
    window: WindowHandle<VegaWindow>,
    provider: Arc<vega_runtime::MockProvider>,
    source: Vec<MessageRow>,
    requested: Arc<Mutex<Vec<MessageLocationRequested>>>,
}

fn draw_frame(visual: &mut VisualTestContext) {
    visual.update(|window, cx| {
        _ = window.draw(cx);
    });
    visual.run_until_parked();
}

fn fixture(cx: &mut gpui_kit::TestAppContext) -> Fixture {
    let messages: Vec<FixtureMessage> =
        serde_json::from_str(include_str!("fixtures/issue72_loaded_markdown.json"))
            .expect("owned Markdown text fixture");
    assert_eq!(messages.len(), 24);
    assert_eq!(messages[13].id, ALPHA_ID);
    assert_eq!(
        messages[13].content,
        "Alpha\n\n- First\n- Second\n\n| Name | Status |\n|---|---|\n| Vega | PASS |\n| Loom | PASS |\n\n```text\nline-one\nline-two\n```\n\nOmega"
    );
    let data = tempfile::tempdir().expect("owned anchor root");
    let config_path = data.path().join("config.toml");
    super::model_selection::model_selection_config(&config_path);
    let config = fs::read_to_string(&config_path).expect("owned anchor config");
    fs::write(
        &config_path,
        config.replace("theme = \"dark\"", "theme = \"light\""),
    )
    .expect("owned anchor light theme");
    let database = data.path().join("vega.db");
    let store = Store::open(&database).expect("owned anchor store");
    store.migrate().expect("owned anchor migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data.path().to_str().expect("owned anchor UTF-8 project"),
        "issue72-loaded-anchor",
        None,
    )
    .expect("owned anchor project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("owned anchor thread");
    let other_thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("owned anchor route B");
    for message in messages {
        insert(
            store.conn(),
            &MessageRow {
                id: message.id,
                thread_id: thread.id.clone(),
                seq: message.seq,
                role: message.role,
                kind: "text".into(),
                content: message.content,
                status: "done".into(),
                created_at: message.seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .expect("owned anchor persisted source");
    }
    let source = recent(store.conn(), &thread.id, PAGE_LIMIT).expect("owned source snapshot");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database).expect("owned root store connection"),
            thread.clone(),
            cx,
        );
        cx.set_global(SidebarWidth(336.0));
    });
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config_path);
        root.agent_provider_override = Some(provider.clone());
    });
    let window_root = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(1404.0), px(860.0)),
                ))),
                ..Default::default()
            },
            move |_, _| window_root,
        )
        .expect("owned anchor production window")
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            root.stream_view.as_ref().is_some_and(|(id, stream)| {
                id == &thread.id && stream.read(cx).hydrated_entry_count() >= 24
            })
        })
    });
    let requested = Arc::new(Mutex::new(Vec::new()));
    Fixture {
        _data: data,
        store,
        thread,
        other_thread,
        root,
        window,
        provider,
        source,
        requested,
    }
}

fn observe_requests(
    f: &Fixture,
    stream: &Entity<ConversationStream>,
    cx: &mut gpui_kit::TestAppContext,
) {
    let requested = f.requested.clone();
    f.root.update(cx, |_, cx| {
        cx.subscribe(stream, move |_, _, event: &MessageLocationRequested, _| {
            requested
                .lock()
                .expect("owned request observer")
                .push(event.clone());
        })
        .detach();
    });
}

fn stream(f: &Fixture, cx: &gpui_kit::TestAppContext) -> Entity<ConversationStream> {
    f.root.read_with(cx, |root, _| {
        root.stream_view
            .as_ref()
            .expect("owned mounted stream")
            .1
            .clone()
    })
}

fn click_first(f: &Fixture, visual: &mut VisualTestContext) {
    let rail = visual
        .debug_bounds("message-anchor-rail")
        .expect("owned overflowing rail");
    let count = f.requested.lock().expect("owned requests").len();
    visual.simulate_mouse_down(
        point(rail.center().x, rail.top() + px(3.0)),
        MouseButton::Left,
        Modifiers::default(),
    );
    visual.simulate_mouse_up(
        point(rail.right() + px(240.0), px(20.0)),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw_frame(visual);
    for _ in 0..24 {
        visual.simulate_event(KeyDownEvent {
            keystroke: Keystroke::parse("up").expect("owned Up"),
            is_held: false,
            prefer_character_input: false,
        });
        draw_frame(visual);
    }
    assert_eq!(
        f.requested.lock().expect("first marker requests").len(),
        count
    );
    let start = selected_marker(visual);
    visual.simulate_click(start, Modifiers::default());
    draw_frame(visual);
    let requested = f.requested.lock().expect("first request");
    assert_eq!(requested.len(), count + 1);
    assert_eq!(requested.last().expect("first event").message_id, FIRST_ID);
    let stream = stream(f, visual);
    let anchor = stream.read_with(visual, |stream, _| stream.scroll_anchor_snapshot());
    assert_eq!(anchor.message_id.as_deref(), Some(FIRST_ID));
    assert!(!anchor.following_tail);
}

fn selected_marker(visual: &mut VisualTestContext) -> Point<Pixels> {
    let rail = visual
        .debug_bounds("message-anchor-rail")
        .expect("selected rail");
    let quads = visual.update(|window, _| (window.scale_factor(), window.painted_quads()));
    let selected = quads
        .1
        .iter()
        .filter(|quad| {
            (quad.bounds.size.width.0 / quads.0 - 7.0).abs() < 0.1
                && (quad.bounds.size.height.0 / quads.0 - 7.0).abs() < 0.1
                && rail.contains(&point(
                    px(quad.bounds.center().x.0 / quads.0),
                    px(quad.bounds.center().y.0 / quads.0),
                ))
        })
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1, "one selected durable anchor marker");
    point(
        px(selected[0].bounds.center().x.0 / quads.0),
        px(selected[0].bounds.center().y.0 / quads.0),
    )
}

fn alpha_point_after_first(f: &Fixture, visual: &mut VisualTestContext) -> Point<Pixels> {
    let stream = stream(f, visual);
    let before = stream.read_with(visual, |stream, _| stream.scroll_anchor_snapshot());
    let generation = f
        .root
        .read_with(visual, |root, _| root.message_location_request_generation);
    let count = f.requested.lock().expect("pre-selection requests").len();
    assert!(
        !alpha_is_visible(f, visual),
        "Alpha is outside the pre-click viewport"
    );
    let rail = visual
        .debug_bounds("message-anchor-rail")
        .expect("pre-selection rail");
    visual.simulate_mouse_down(
        point(rail.center().x, rail.top() + px(3.0)),
        MouseButton::Left,
        Modifiers::default(),
    );
    visual.simulate_mouse_up(
        point(rail.right() + px(240.0), px(20.0)),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw_frame(visual);
    for _ in 0..13 {
        visual.simulate_event(KeyDownEvent {
            keystroke: Keystroke::parse("down").expect("owned Down"),
            is_held: false,
            prefer_character_input: false,
        });
        draw_frame(visual);
    }
    let after = stream.read_with(visual, |stream, _| stream.scroll_anchor_snapshot());
    assert_eq!(
        after, before,
        "preview selection preserves the actual viewport"
    );
    assert_eq!(
        f.requested.lock().expect("post-selection requests").len(),
        count
    );
    assert_eq!(
        f.root
            .read_with(visual, |root, _| root.message_location_request_generation),
        generation
    );
    assert!(f.provider.requests().is_empty());
    eprintln!(
        "Alpha pre-click: not visible; preview selection emitted zero requests; generation={generation}; anchor={after:?}"
    );
    selected_marker(visual)
}

fn alpha_is_visible(f: &Fixture, visual: &mut VisualTestContext) -> bool {
    let stream = stream(f, visual);
    let before = stream.read_with(visual, |stream, _| stream.scroll_anchor_snapshot());
    let count = f.requested.lock().expect("visibility probe requests").len();
    let generation = f
        .root
        .read_with(visual, |root, _| root.message_location_request_generation);
    let mut viewport = visual
        .debug_bounds("conversation-message-list")
        .expect("actual anchor viewport");
    visual.update(gpui_kit::base::TextSelection::clear);
    let mut viewport_changes = 0;
    let mut visible = false;
    let mut y = viewport.top() + px(3.0);
    while y < viewport.bottom() - px(3.0) {
        let start = point(viewport.left() + px(0.5), y);
        let end = point(
            (viewport.left() + px(240.0)).min(viewport.right() - px(1.0)),
            y,
        );
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        draw_frame(visual);
        let selected = visual.update(gpui_kit::base::TextSelection::selected_text);
        let actual_viewport = visual
            .debug_bounds("conversation-message-list")
            .expect("selection probe viewport");
        if actual_viewport != viewport {
            eprintln!(
                "Selection probe viewport changed: cached={viewport:?}, actual={actual_viewport:?}, selected={selected:?}"
            );
            viewport_changes += 1;
            assert_eq!(viewport_changes, 1, "one preview-lane dismissal per probe");
            viewport = actual_viewport;
            y = viewport.top() + px(3.0);
            continue;
        }
        if selected == "Alpha" {
            let (scale, quads, selection_color) = visual.update(|window, cx| {
                (
                    window.scale_factor(),
                    window.painted_quads(),
                    gpui_kit::base::Theme::global(cx).tokens.colors.selection,
                )
            });
            assert!(
                quads.iter().any(|quad| {
                    let bounds = Bounds::new(
                        point(
                            px(quad.bounds.origin.x.0 / scale),
                            px(quad.bounds.origin.y.0 / scale),
                        ),
                        size(
                            px(quad.bounds.size.width.0 / scale),
                            px(quad.bounds.size.height.0 / scale),
                        ),
                    );
                    let mask = Bounds::new(
                        point(
                            px(quad.content_mask.bounds.origin.x.0 / scale),
                            px(quad.content_mask.bounds.origin.y.0 / scale),
                        ),
                        size(
                            px(quad.content_mask.bounds.size.width.0 / scale),
                            px(quad.content_mask.bounds.size.height.0 / scale),
                        ),
                    );
                    let painted = bounds.intersect(&mask).intersect(&viewport);
                    quad.background.as_solid() == Some(selection_color)
                        && painted.size.width > px(0.0)
                        && painted.size.height > px(0.0)
                }),
                "Alpha selection paints inside the actual viewport"
            );
            visible = true;
            break;
        }
        y += px(3.0);
    }
    assert_eq!(
        stream.read_with(visual, |stream, _| stream.scroll_anchor_snapshot()),
        before,
        "visibility observation preserves the stable viewport anchor"
    );
    assert_eq!(
        f.requested.lock().expect("probe final requests").len(),
        count
    );
    assert_eq!(
        f.root
            .read_with(visual, |root, _| root.message_location_request_generation),
        generation
    );
    assert!(f.provider.requests().is_empty());
    visible
}

fn click_alpha(f: &Fixture, visual: &mut VisualTestContext, position: Point<Pixels>, stage: &str) {
    visual.simulate_mouse_move(position, None, Modifiers::default());
    draw_frame(visual);
    let preview = visual
        .debug_bounds("message-anchor-preview")
        .expect("Alpha preview");
    let viewport = visual
        .debug_bounds("conversation-message-list")
        .expect("Alpha viewport");
    let composer = visual
        .debug_bounds("composer-shell")
        .expect("owned composer");
    assert!(!preview.intersects(&viewport));
    assert!(!preview.intersects(&composer));
    let count = f.requested.lock().expect("owned pre-click request").len();
    let generation = f
        .root
        .read_with(visual, |root, _| root.message_location_request_generation);
    visual.simulate_click(position, Modifiers::default());
    draw_frame(visual);
    let requests = f.requested.lock().expect("owned Alpha requests").clone();
    assert_eq!(
        requests.len(),
        count + 1,
        "{stage}: one gesture, one request"
    );
    assert_eq!(
        requests.last().expect("owned Alpha event").message_id,
        ALPHA_ID,
        "{stage}: stable Alpha identity"
    );
    assert_eq!(
        requests.last().expect("owned Alpha route").thread_id,
        f.thread.id
    );
    assert_eq!(
        f.root
            .read_with(visual, |root, _| root.message_location_request_generation),
        generation + 1
    );
    let stream = stream(f, visual);
    let (anchor, status) = stream.read_with(visual, |stream, _| {
        (
            stream.scroll_anchor_snapshot(),
            stream.message_location_status(),
        )
    });
    eprintln!(
        "{stage}: request={ALPHA_ID}, anchor={anchor:?}, status={status:?}, viewport={viewport:?}"
    );
    assert_eq!(status, Some(MessageLocationStatus::Located));
    assert!(!anchor.following_tail);
    assert!(
        alpha_is_visible(f, visual),
        "{stage}: one click must render the exact Alpha target inside the actual viewport; anchor={anchor:?}"
    );
    assert_eq!(
        recent(f.store.conn(), &f.thread.id, PAGE_LIMIT).expect("owned final source"),
        f.source
    );
    assert!(f.provider.requests().is_empty());
    assert_eq!(
        f.root.read_with(visual, |root, _| root
            .stream_view
            .as_ref()
            .map(|(id, _)| id.clone())),
        Some(f.thread.id.clone())
    );
}

#[gpui_kit::test]
async fn issue72_loaded_alpha_rail_click_enters_actual_viewport_once(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let initial_stream = stream(&f, cx);
    observe_requests(&f, &initial_stream, cx);
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    draw_frame(&mut visual);
    assert!(initial_stream.read_with(&visual, |stream, _| {
        stream.scroll_anchor_snapshot().following_tail
    }));
    click_first(&f, &mut visual);
    let native_point = alpha_point_after_first(&f, &mut visual);
    visual.simulate_event(ScrollWheelEvent {
        position: native_point,
        delta: ScrollDelta::Pixels(point(px(0.0), px(0.001))),
        ..Default::default()
    });
    draw_frame(&mut visual);
    click_alpha(&f, &mut visual, native_point, "cold native-shaped click");

    click_first(&f, &mut visual);
    let measured_point = alpha_point_after_first(&f, &mut visual);
    click_alpha(&f, &mut visual, measured_point, "repeated measured click");

    visual.simulate_resize(size(px(960.0), px(600.0)));
    draw_frame(&mut visual);
    click_first(&f, &mut visual);
    let narrow_point = alpha_point_after_first(&f, &mut visual);
    click_alpha(&f, &mut visual, narrow_point, "remeasured narrow click");

    drop(visual);
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(f.other_thread.clone())));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, _)| id == &f.other_thread.id)
        })
    });
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(f.thread.clone())));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, cx| {
            root.stream_view.as_ref().is_some_and(|(id, stream)| {
                id == &f.thread.id && stream.read(cx).hydrated_entry_count() >= 24
            })
        })
    });
    let returned_stream = stream(&f, cx);
    assert_ne!(initial_stream, returned_stream);
    observe_requests(&f, &returned_stream, cx);
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    draw_frame(&mut visual);
    click_first(&f, &mut visual);
    let returned_point = alpha_point_after_first(&f, &mut visual);
    click_alpha(&f, &mut visual, returned_point, "returned A route click");
}
