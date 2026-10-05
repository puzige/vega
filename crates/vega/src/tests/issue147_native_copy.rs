use super::*;
use gpui_kit::{
    Bounds, InputEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, ScrollDelta, ScrollWheelEvent, TouchPhase, VisualTestContext, WindowBounds,
    WindowOptions, point, px, size,
};
use vega_conversation::types::ThreadScrollAnchor;

const MESSAGE_ID: &str = "issue147-native-copy-code";

fn code_lines(start: usize, end: usize) -> String {
    (start..=end)
        .map(|line| format!("CODE-{line:02} token-alpha token-beta token-gamma"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn insert_message(store: &Store, thread: &Thread, id: &str, seq: i64, role: &str, text: &str) {
    insert(
        store.conn(),
        &MessageRow {
            id: id.into(),
            thread_id: thread.id.clone(),
            seq,
            role: role.into(),
            kind: "text".into(),
            content: text.into(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .expect("owned copy history");
}

fn restored_anchor(thread: &Thread) -> ThreadScrollAnchor {
    let kind = "assistant-text-0";
    ThreadScrollAnchor {
        identity: Some(format!(
            "route:{}:{}:message:{}:{}:kind:{}:{}",
            thread.id.len(),
            thread.id,
            MESSAGE_ID.len(),
            MESSAGE_ID,
            kind.len(),
            kind
        )),
        message_id: Some(MESSAGE_ID.into()),
        offset_in_item_px: 4.0
            + 6.0 * (Typography::MESSAGE * Typography::MESSAGE_LINE_HEIGHT + 2.0),
        following_tail: false,
    }
}

fn draw_frame(visual: &mut VisualTestContext) {
    visual.update(|window, cx| {
        _ = window.draw(cx);
    });
    visual.run_until_parked();
}

async fn open_restored_code(
    cx: &mut gpui_kit::TestAppContext,
) -> (
    TempDir,
    Entity<VegaWindow>,
    Entity<ConversationStream>,
    VisualTestContext,
) {
    let data = tempfile::tempdir().expect("owned copy root");
    let config_path = data.path().join("config.toml");
    super::model_selection::model_selection_config(&config_path);
    let config = fs::read_to_string(&config_path).expect("owned copy config");
    fs::write(
        &config_path,
        config.replace("theme = \"dark\"", "theme = \"light\""),
    )
    .expect("owned light theme");
    let store = Store::open(data.path().join("vega.db")).expect("owned copy store");
    store.migrate().expect("owned copy migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data.path().to_str().expect("owned UTF-8 project"),
        "issue147-native-copy",
        None,
    )
    .expect("owned copy project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("owned copy thread");
    let other_thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("owned cache-switch thread");
    insert_message(
        &store,
        &thread,
        "issue147-native-copy-prompt",
        1,
        "user",
        "Generate a 32-line code block with token. Preserve every original line.",
    );
    insert_message(
        &store,
        &thread,
        MESSAGE_ID,
        2,
        "assistant",
        &format!("```plaintext\n{}\n```", code_lines(1, 32)),
    );
    cx.update(|cx| {
        install_diff_window_globals(store, thread.clone(), cx);
        cx.set_global(SidebarWidth(336.0));
    });
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config_path);
    });
    let window_root = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(1403.0), px(860.0)),
                ))),
                ..Default::default()
            },
            move |_, _| window_root,
        )
        .expect("owned production root copy window")
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            root.stream_view
                .as_ref()
                .is_some_and(|(_, stream)| stream.read(cx).hydrated_entry_count() == 3)
        })
    });
    let initial_stream = root.read_with(cx, |root, _| {
        root.stream_view
            .as_ref()
            .expect("owned hydrated copy view")
            .1
            .clone()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    draw_frame(&mut visual);
    initial_stream.update(cx, |stream, cx| {
        assert!(stream.restore_scroll_anchor(&restored_anchor(&thread)));
        cx.notify();
    });
    draw_frame(&mut visual);
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(other_thread.clone())));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, _)| id == &other_thread.id)
        })
    });
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(thread.clone())));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            root.stream_view.as_ref().is_some_and(|(id, stream)| {
                id == &thread.id && stream.read(cx).hydrated_entry_count() == 3
            })
        })
    });
    let stream = root.read_with(cx, |root, _| {
        root.stream_view
            .as_ref()
            .expect("returned cached copy view")
            .1
            .clone()
    });
    draw_frame(&mut visual);
    let list = visual
        .debug_bounds("conversation-message-list")
        .expect("owned copy viewport");
    let before = visual
        .debug_bounds("assistant-message")
        .expect("restored owned code");
    let row_height = (before.size.height - px(12.0)) / 32.0;
    let anchor = stream.read_with(&visual, |stream, _| stream.scroll_anchor_snapshot());
    assert!(!anchor.following_tail, "restored copy anchor: {anchor:?}");
    assert_eq!(anchor.message_id.as_deref(), Some(MESSAGE_ID));
    assert!(before.top() < list.top() - row_height * 4.0);
    let delta = list.top() + px(8.0) - before.top();
    visual.simulate_event(ScrollWheelEvent {
        position: list.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), delta)),
        touch_phase: TouchPhase::Moved,
        ..Default::default()
    });
    draw_frame(&mut visual);
    let after = visual
        .debug_bounds("assistant-message")
        .expect("small-scroll owned code");
    assert!((after.top() - list.top() - px(8.0)).abs() < px(1.0));
    (data, root, stream, visual)
}

fn selection_points(visual: &mut VisualTestContext) -> (Point<Pixels>, Point<Pixels>) {
    let message = visual
        .debug_bounds("assistant-message")
        .expect("owned code geometry");
    let viewport = visual
        .debug_bounds("conversation-message-list")
        .expect("owned code viewport");
    let row_height = (message.size.height - px(12.0)) / 32.0;
    let start = point(
        message.left() + px(8.5),
        message.top() + px(4.0) + row_height * 6.5,
    );
    let end = point(
        message.left() + px(450.0),
        message.top() + px(4.0) + row_height * 20.5,
    );
    assert!(viewport.contains(&start) && viewport.contains(&end));
    (start, end)
}

fn drag_rows(visual: &mut VisualTestContext, start: Point<Pixels>, end: Point<Pixels>) {
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    for step in 1..=32 {
        visual.simulate_mouse_move(
            start + (end - start) * (step as f32 / 32.0),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
    }
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw_frame(visual);
}

fn assert_copy(visual: &mut VisualTestContext, expected: &str) {
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        expected,
        "owned production selection preserves the complete visible interval"
    );
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(expected),
        "owned production Cmd+C preserves every selected line"
    );
}

fn assert_selected_quads(visual: &mut VisualTestContext, rows: usize) {
    let painted = visual.update(|window, cx| {
        let color = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background.as_solid() == Some(color))
            .count()
    });
    assert_eq!(painted, rows, "owned visible selected row rectangles");
}

#[gpui_kit::test]
async fn issue147_native_copy_restored_small_scroll_forward_keeps_all_fifteen_lines(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (_data, _root, _stream, mut visual) = open_restored_code(cx).await;
    let (start, end) = selection_points(&mut visual);
    drag_rows(&mut visual, start, end);
    assert_selected_quads(&mut visual, 15);
    assert_copy(&mut visual, &code_lines(7, 21));
}

#[gpui_kit::test]
async fn issue147_native_copy_restored_small_scroll_reverse_keeps_all_fifteen_lines(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (_data, _root, _stream, mut visual) = open_restored_code(cx).await;
    let (start, end) = selection_points(&mut visual);
    drag_rows(&mut visual, end, start);
    assert_selected_quads(&mut visual, 15);
    assert_copy(&mut visual, &code_lines(7, 21));
}

#[gpui_kit::test]
async fn issue147_native_copy_restored_small_scroll_batched_input_keeps_all_fifteen_lines(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (_data, _root, _stream, mut visual) = open_restored_code(cx).await;
    let (start, end) = selection_points(&mut visual);
    visual.update(|window, cx| {
        window.dispatch_event(
            MouseDownEvent {
                position: start,
                button: MouseButton::Left,
                click_count: 1,
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        for step in 1..=32 {
            window.dispatch_event(
                MouseMoveEvent {
                    position: start + (end - start) * (step as f32 / 32.0),
                    pressed_button: Some(MouseButton::Left),
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
        }
        window.dispatch_event(
            MouseUpEvent {
                position: end,
                button: MouseButton::Left,
                click_count: 1,
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
    });
    draw_frame(&mut visual);
    assert_selected_quads(&mut visual, 15);
    assert_copy(&mut visual, &code_lines(7, 21));
}

#[gpui_kit::test]
async fn issue147_native_copy_selected_rows_survive_another_small_scroll(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (_data, _root, stream, mut visual) = open_restored_code(cx).await;
    let (start, end) = selection_points(&mut visual);
    drag_rows(&mut visual, start, end);
    assert_copy(&mut visual, &code_lines(7, 21));
    let viewport = visual
        .debug_bounds("conversation-message-list")
        .expect("owned copy viewport before second scroll");
    let before = visual
        .debug_bounds("assistant-message")
        .expect("owned code before second scroll");
    visual.simulate_event(ScrollWheelEvent {
        position: viewport.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-40.0))),
        touch_phase: TouchPhase::Moved,
        ..Default::default()
    });
    draw_frame(&mut visual);
    let after = stream.read_with(&visual, |stream, _| stream.scroll_anchor_snapshot());
    let moved = visual
        .debug_bounds("assistant-message")
        .expect("owned code after second scroll");
    assert_eq!(after.message_id.as_deref(), Some(MESSAGE_ID));
    assert!((moved.top() - before.top() + px(40.0)).abs() < px(1.0));
    assert_selected_quads(&mut visual, 15);
    assert_copy(&mut visual, &code_lines(7, 21));
}

#[gpui_kit::test]
async fn issue147_native_copy_single_line_control_after_clearing_multiline_selection(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (_data, _root, _stream, mut visual) = open_restored_code(cx).await;
    let (start, end) = selection_points(&mut visual);
    drag_rows(&mut visual, start, end);
    assert_copy(&mut visual, &code_lines(7, 21));
    visual.update(gpui_kit::base::TextSelection::clear);
    draw_frame(&mut visual);
    drag_rows(&mut visual, start, point(end.x, start.y));
    assert_selected_quads(&mut visual, 1);
    assert_copy(&mut visual, &code_lines(7, 7));
}

#[gpui_kit::test]
async fn issue147_native_copy_clipboard_and_composer_paste_keep_the_same_fifteen_lines(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (_data, _root, stream, mut visual) = open_restored_code(cx).await;
    let (start, end) = selection_points(&mut visual);
    drag_rows(&mut visual, start, end);
    assert_selected_quads(&mut visual, 15);
    let expected = code_lines(7, 21);
    assert_copy(&mut visual, &expected);
    let input = stream.read_with(&visual, |stream, _| stream.composer_input());
    assert!(input.read_with(&visual, |input, _| input.text().is_empty()));
    let composer = visual
        .debug_bounds("composer-shell")
        .expect("owned composer");
    visual.simulate_click(
        point(composer.center().x, composer.top() + px(24.0)),
        Modifiers::default(),
    );
    assert!(visual.update(|window, cx| input.read(cx).focus_handle(cx).is_focused(window)));
    visual.simulate_keystrokes("cmd-v");
    draw_frame(&mut visual);
    assert_eq!(
        input.read_with(&visual, |input, _| input.text().to_string()),
        expected,
        "owned Composer paste equals the complete original clipboard"
    );
    visual.simulate_keystrokes("cmd-up");
    draw_frame(&mut visual);
    assert_eq!(
        input.read_with(&visual, |input, _| input.text().to_string()),
        expected,
        "owned Composer start-of-document movement preserves every pasted line"
    );
    assert!(!stream.read_with(&visual, |stream, _| stream.has_active_agent()));
    visual.simulate_keystrokes("cmd-a backspace");
    assert!(input.read_with(&visual, |input, _| input.text().is_empty()));
}
