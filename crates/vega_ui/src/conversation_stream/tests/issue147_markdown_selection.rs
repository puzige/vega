use super::*;
use gpui_kit::{Modifiers, MouseButton, Pixels, Point, VisualTestContext, size};

struct SelectionHarness {
    stream: Entity<ConversationStream>,
}

impl Render for SelectionHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .size_full()
            .child(gpui_kit::base::TextSelectionLayer)
            .child(self.stream.clone())
    }
}

fn open_selection_harness(
    cx: &mut TestAppContext,
    entry: StreamEntry,
) -> (Entity<ConversationStream>, VisualTestContext) {
    init_permission_test(cx);
    let mut thread = permission_thread();
    thread.id = "issue147-selection".into();
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    stream.update(cx, |stream, _| {
        stream.entries.push(entry);
        stream.list_append(0);
    });
    let root_stream = stream.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(move |_| SelectionHarness {
                stream: root_stream,
            })
        })
        .expect("selection test window")
    });
    cx.run_until_parked();
    let window = window.into();
    let visual = VisualTestContext::from_window(window, cx);
    visual.simulate_resize(size(px(1000.), px(1600.)));
    visual.run_until_parked();
    (stream, visual)
}

fn assistant_entry(document: &str) -> StreamEntry {
    let mut stream = MarkdownStream::new();
    stream.append(document);
    stream.finish();
    let mut model = StreamModel::default();
    model.sync(&stream.snapshot(), &StreamCounters::default());
    StreamEntry::Assistant {
        copy: MessageCopy::new(document),
        stream: Box::new(stream),
        model,
        failure: None,
    }
}

fn layout_for(text: &str) -> gpui_kit::TextLayout {
    selection::SELECTION_RUN_LAYOUTS.with_borrow(|runs| {
        runs.iter()
            .rev()
            .find(|(run_text, _, _)| run_text == text)
            .map(|(_, layout, _)| layout.clone())
            .expect("production selection text layout")
    })
}

fn drag(visual: &mut VisualTestContext, start: Point<Pixels>, end: Point<Pixels>) {
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
}

fn text_point(layout: &gpui_kit::TextLayout, index: usize, edge: bool) -> Point<Pixels> {
    let mut point = layout
        .position_for_index(index)
        .expect("text index geometry");
    point.y += layout.line_height() / 2.;
    if edge {
        point.x += px(2.);
    } else {
        point.x += px(0.5);
    }
    point
}

#[gpui_kit::test]
fn issue147_user_text_drag_supports_unicode_cmd_c_and_empty_copy(cx: &mut TestAppContext) {
    let text = "prefix Alpha你好 😀 suffix";
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    let entry = StreamEntry::User {
        copy: MessageCopy::new(text),
        lines: user_message_lines(147, text),
    };
    let (_stream, mut visual) = open_selection_harness(cx, entry);
    let layout = layout_for(text);
    let start = text.find("Alpha").expect("target start");
    let end = text.find(" suffix").expect("target end");
    let start_point = text_point(&layout, start, false);
    let end_point = text_point(&layout, end, true);
    drag(&mut visual, start_point, end_point);
    visual.run_until_parked();
    let selected = visual.update(gpui_kit::base::TextSelection::selected_text);
    assert_eq!(selected, "Alpha你好 😀");
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("Alpha你好 😀")
    );

    visual.update(gpui_kit::base::TextSelection::clear);
    visual.write_to_clipboard(gpui_kit::ClipboardItem::new_string("sentinel".into()));
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("sentinel")
    );
}

#[gpui_kit::test]
fn issue147_stream_append_invalidates_stale_copy_before_repaint(cx: &mut TestAppContext) {
    let text = "Alpha beta";
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    let copy = MessageCopy::new(text);
    let entry = StreamEntry::User {
        copy: copy.clone(),
        lines: user_message_lines(150, text),
    };
    let (stream, mut visual) = open_selection_harness(cx, entry);
    let layout = layout_for(text);
    drag(
        &mut visual,
        text_point(&layout, 0, false),
        text_point(&layout, 5, true),
    );
    visual.run_until_parked();
    assert!(copy.has_selected_text());

    visual.write_to_clipboard(gpui_kit::ClipboardItem::new_string("sentinel".into()));
    let updated_text = "Alpha beta updated";
    visual.update(|_, cx| {
        stream.update(cx, |stream, cx| {
            if let StreamEntry::User { lines, copy, .. } = &mut stream.entries[0] {
                *lines = user_message_lines(150, updated_text);
                copy.append(" updated");
            }
            cx.notify();
        });
    });
    visual.update(|window, cx| {
        _ = window.draw(cx);
    });
    visual.run_until_parked();
    assert!(!copy.has_selected_text());
    let stale_projection = visual.update(gpui_kit::base::TextSelection::selected_text);
    assert!(
        stale_projection.is_empty(),
        "stale projection: {stale_projection:?}"
    );
    visual.simulate_keystrokes("cmd-c");

    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("sentinel")
    );
}

#[gpui_kit::test]
fn issue147_assistant_markdown_drag_copies_visible_block_text(cx: &mut TestAppContext) {
    let markdown = "# 标题\n\n开头\n**粗体** 和 [链接](https://hidden) 🙂。\n\n- 项目\n- [x] 完成\n\n```text\n代码\n  行2\n```\n\n> 引用\n\n| A | B |\n|---|---|\n| 1 | 中文 |\n\n---";
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    let (stream, mut visual) = open_selection_harness(cx, assistant_entry(markdown));
    let runs = selection::SELECTION_RUN_LAYOUTS.with_borrow(|runs| {
        runs.iter()
            .map(|(text, layout, _)| (text.clone(), layout.clone()))
            .collect::<Vec<_>>()
    });
    let (first_text, first_layout) = runs.first().expect("first markdown run");
    let (last_text, last_layout) = runs.last().expect("last markdown run");
    let start = text_point(first_layout, 0, false);
    let end = text_point(last_layout, last_text.len(), true);
    assert!(!first_text.is_empty());
    drag(&mut visual, start, end);
    visual.run_until_parked();
    let selected = visual.update(gpui_kit::base::TextSelection::selected_text);
    assert_eq!(
        selected,
        "标题\n\n开头 粗体 和 链接 🙂。\n\n• 项目\n• [x] 完成\n\n代码\n  行2\n\n引用\n\nA\tB\n1\t中文"
    );
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(
            "标题\n\n开头 粗体 和 链接 🙂。\n\n• 项目\n• [x] 完成\n\n代码\n  行2\n\n引用\n\nA\tB\n1\t中文"
        )
    );
    assert!(stream.read_with(&visual, |stream, _| !stream.entries.is_empty()));
}
