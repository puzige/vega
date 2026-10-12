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
    let stream = new_selection_stream(cx);
    stream.update(cx, |stream, _| {
        stream.entries.push(entry);
        stream.list_append(0);
    });
    open_stream_harness(cx, stream)
}

pub(super) fn open_empty_selection_harness(
    cx: &mut TestAppContext,
) -> (Entity<ConversationStream>, VisualTestContext) {
    let stream = new_selection_stream(cx);
    open_stream_harness(cx, stream)
}

fn new_selection_stream(cx: &mut TestAppContext) -> Entity<ConversationStream> {
    init_permission_test(cx);
    let mut thread = permission_thread();
    thread.id = "issue147-selection".into();
    cx.new(|cx| ConversationStream::new(thread, cx))
}

fn open_stream_harness(
    cx: &mut TestAppContext,
    stream: Entity<ConversationStream>,
) -> (Entity<ConversationStream>, VisualTestContext) {
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
fn issue147_streaming_markdown_delta_invalidates_stable_selection_before_repaint(
    cx: &mut TestAppContext,
) {
    let initial_text = "prefix Alpha suffix";
    let message_id = "issue147-live-assistant";
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    let (stream, mut visual) = open_empty_selection_harness(cx);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: message_id.into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: message_id.into(),
                delta: initial_text.into(),
            },
            cx,
        );
    });
    visual.run_until_parked();
    let copy = stream
        .read_with(&visual, |stream, _| {
            stream.entries.iter().find_map(|entry| match entry {
                StreamEntry::Assistant { copy, .. } => Some(copy.clone()),
                _ => None,
            })
        })
        .expect("active assistant markdown entry");

    let layout = layout_for(initial_text);
    let start = initial_text.find("Alpha").expect("selection start");
    let end = start + "Alpha".len();
    drag(
        &mut visual,
        text_point(&layout, start, false),
        text_point(&layout, end, true),
    );
    visual.run_until_parked();
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        "Alpha"
    );
    assert!(copy.has_selected_text());

    visual.write_to_clipboard(gpui_kit::ClipboardItem::new_string("sentinel".into()));
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: message_id.into(),
                delta: " and **new** text".into(),
            },
            cx,
        );
    });

    assert!(
        !copy.has_selected_text(),
        "stream delta retained the previous selection before repaint"
    );
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("sentinel")
    );

    visual.run_until_parked();
    assert!(
        visual
            .update(gpui_kit::base::TextSelection::selected_text)
            .is_empty()
    );
    assert_eq!(
        copy.visible_text().as_deref(),
        Some("prefix Alpha suffix and new text")
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

const INLINE_HIGHLIGHT_MARKDOWN: &str = "## 开始 Alpha\n中英文混排 Vega 测试🙂 — alpha beta.\n- 列表项一\n- `inline_code`\n| 名称 | 状态 |\n|---|---|\n| Vega | PASS |\n```text\nline-one\nline-two\n```\n末尾 end.";
const INLINE_HIGHLIGHT_PARAGRAPH: &str = "中英文混排 Vega 测试🙂 — alpha beta.";

fn assert_background_at(
    visual: &mut VisualTestContext,
    point: Point<Pixels>,
    expected: gpui_kit::Hsla,
    label: &str,
) {
    let mut layers = visual.update(|window, _| {
        let point = point.scale(window.scale_factor());
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| {
                quad.bounds.contains(&point) && quad.content_mask.bounds.contains(&point)
            })
            .filter_map(|quad| {
                quad.background
                    .as_solid()
                    .filter(|color| color.a > 0.)
                    .map(|color| (quad.order, color, quad.bounds))
            })
            .collect::<Vec<_>>()
    });
    layers.sort_by_key(|layer| layer.0);
    assert_eq!(
        layers.last().map(|layer| layer.1),
        Some(expected),
        "{label}: selected background must survive later paint; point={point:?}, ordered_layers={layers:?}"
    );
}

fn assert_range_background(
    visual: &mut VisualTestContext,
    layout: &gpui_kit::TextLayout,
    range: std::ops::Range<usize>,
    expected: gpui_kit::Hsla,
    label: &str,
) {
    let start = layout
        .position_for_index(range.start)
        .expect("range start geometry");
    let end = layout
        .position_for_index(range.end)
        .expect("range end geometry");
    assert_eq!(start.y, end.y, "fixture selected range stays on one line");
    assert!(end.x > start.x);
    for ratio in [0.25, 0.5, 0.75] {
        assert_background_at(
            visual,
            Point::new(
                start.x + (end.x - start.x) * ratio,
                start.y + layout.line_height() / 2.,
            ),
            expected,
            label,
        );
    }
}

fn inline_highlight_case(
    cx: &mut TestAppContext,
    dark: bool,
    text: &str,
    range: std::ops::Range<usize>,
) {
    cx.update(gpui_kit::init);
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    let (_stream, mut visual) =
        open_selection_harness(cx, assistant_entry(INLINE_HIGHLIGHT_MARKDOWN));
    visual.update(|window, cx| {
        let (theme, mode) = if dark {
            (
                vega_theme::Theme::dark(),
                gpui_kit::component::ThemeMode::Dark,
            )
        } else {
            (
                vega_theme::Theme::light(),
                gpui_kit::component::ThemeMode::Light,
            )
        };
        gpui_kit::component::Theme::change(mode, Some(window), cx);
        cx.set_global(theme);
    });
    visual.simulate_resize(size(px(1404.), px(860.)));
    visual.run_until_parked();
    visual.update(|window, cx| {
        _ = window.draw(cx);
    });
    let (layout, text_offset) = selection::SELECTION_RUN_LAYOUTS.with_borrow(|runs| {
        runs.iter()
            .rev()
            .find_map(|(run_text, layout, _)| {
                run_text.find(text).map(|offset| (layout.clone(), offset))
            })
            .expect("production run contains target text")
    });
    let layout_range = text_offset + range.start..text_offset + range.end;
    assert!(
        visual
            .update(gpui_kit::base::TextSelection::selected_text)
            .is_empty()
    );
    drag(
        &mut visual,
        text_point(&layout, layout_range.start, false),
        text_point(&layout, layout_range.end, true),
    );
    visual.run_until_parked();
    visual.update(|window, cx| {
        _ = window.draw(cx);
    });
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        text[range.clone()]
    );
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(&text[range.clone()])
    );
    let selection_color =
        visual.update(|_, cx| gpui_kit::base::Theme::global(cx).tokens.colors.selection);
    assert_range_background(
        &mut visual,
        &layout,
        layout_range.clone(),
        selection_color,
        text,
    );
    if text == "inline_code" && range.start > 0 {
        let code_background =
            visual.update(|_, cx| vega_theme::theme(cx).colors.conversation_code.into());
        assert_range_background(
            &mut visual,
            &layout,
            text_offset..layout_range.start,
            code_background,
            "unselected inline prefix",
        );
        assert_range_background(
            &mut visual,
            &layout,
            layout_range.end..text_offset + text.len(),
            code_background,
            "unselected inline suffix",
        );
    }
}

#[gpui_kit::test]
fn issue147_inline_highlight_light_complete_code(cx: &mut TestAppContext) {
    inline_highlight_case(cx, false, "inline_code", 0..11);
}

#[gpui_kit::test]
fn issue147_inline_highlight_dark_complete_code(cx: &mut TestAppContext) {
    inline_highlight_case(cx, true, "inline_code", 0..11);
}

#[gpui_kit::test]
fn issue147_inline_highlight_light_partial_code(cx: &mut TestAppContext) {
    inline_highlight_case(cx, false, "inline_code", 2..8);
}

#[gpui_kit::test]
fn issue147_inline_highlight_dark_partial_code(cx: &mut TestAppContext) {
    inline_highlight_case(cx, true, "inline_code", 2..8);
}

#[gpui_kit::test]
fn issue147_inline_highlight_light_paragraph_control(cx: &mut TestAppContext) {
    inline_highlight_case(
        cx,
        false,
        INLINE_HIGHLIGHT_PARAGRAPH,
        0..INLINE_HIGHLIGHT_PARAGRAPH.len(),
    );
}

#[gpui_kit::test]
fn issue147_inline_highlight_dark_paragraph_control(cx: &mut TestAppContext) {
    inline_highlight_case(
        cx,
        true,
        INLINE_HIGHLIGHT_PARAGRAPH,
        0..INLINE_HIGHLIGHT_PARAGRAPH.len(),
    );
}

#[gpui_kit::test]
fn issue296_v3_markdown_mixed_blocks_use_semantic_gaps_and_preserve_copy(cx: &mut TestAppContext) {
    let source = "中文正文 Latin text\n\n# 一级 Heading\n\n- 列表 Alpha\n- 列表 Beta\n\n```text\ncode one\n  第二行\n```\n\n| 中文列 | Latin |\n|---|---|\n| 数据 | Value |";
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    let (_stream, mut visual) = open_selection_harness(cx, assistant_entry(source));
    let paragraph = layout_for("中文正文 Latin text");
    let heading = layout_for("一级 Heading");
    let first_item = layout_for("列表 Alpha");
    let second_item = layout_for("列表 Beta");
    let code = layout_for("code one");
    let code_last = layout_for("  第二行");
    let table = layout_for("中文列");
    let code_surface = visual
        .debug_bounds("markdown-code-4")
        .expect("code container");
    let table_block = visual
        .debug_bounds("markdown-block-6")
        .expect("table block");
    assert_eq!(heading.line_layouts()[0].font_size(), px(22.));
    assert_eq!(paragraph.line_layouts()[0].font_size(), px(13.5));
    assert!((f32::from(paragraph.line_height()) - 23.).abs() <= 0.5);
    assert_eq!(
        heading.bounds().top() - paragraph.bounds().bottom(),
        px(24.)
    );
    assert_eq!(
        first_item.bounds().top() - heading.bounds().bottom(),
        px(16.)
    );
    assert_eq!(
        second_item.bounds().top() - first_item.bounds().bottom(),
        px(6.)
    );
    assert_eq!(
        first_item.bounds().left() - paragraph.bounds().left(),
        px(24.)
    );
    assert_eq!(code_surface.top() - second_item.bounds().bottom(), px(16.));
    assert_eq!(code.bounds().top() - code_surface.top(), px(12.));
    assert_eq!(code.bounds().left() - code_surface.left(), px(12.));
    assert_eq!(code.line_layouts()[0].font_size(), px(12.5));
    assert_eq!(code.line_height(), px(20.));
    assert_eq!(code_last.bounds().top(), code.bounds().bottom());
    assert_eq!(code_surface.bottom() - code_last.bounds().bottom(), px(12.));
    assert_eq!(table_block.top() - code_surface.bottom(), px(16.));
    assert_eq!(table.bounds().top() - table_block.top(), px(8.));
    assert_eq!(table.bounds().left() - table_block.left(), px(12.));
    assert_eq!(table.line_layouts()[0].font_size(), px(13.5));
    let last = layout_for("Value");
    drag(
        &mut visual,
        text_point(&paragraph, 0, false),
        text_point(&last, 5, true),
    );
    visual.run_until_parked();
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(
            "中文正文 Latin text\n\n一级 Heading\n\n• 列表 Alpha\n• 列表 Beta\n\ncode one\n  第二行\n\n中文列\tLatin\n数据\tValue"
        )
    );
}

#[gpui_kit::test]
fn issue296_v3_list_paragraphs_and_long_code_keep_local_geometry(cx: &mut TestAppContext) {
    let long_line = "very_long_identifier_".repeat(30);
    let source = format!(
        "- First 项\n\n  Second 段\n\n  Third 段\n\nOutside 正文\n\n```text\n{long_line}\n```\n\nEnd 结尾"
    );
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    let (_stream, mut visual) = open_selection_harness(cx, assistant_entry(&source));
    visual.simulate_resize(size(px(600.), px(1600.)));
    visual.run_until_parked();
    let first = layout_for("First 项");
    let second = layout_for("Second 段");
    let third = layout_for("Third 段");
    let outside = layout_for("Outside 正文");
    assert_eq!(second.bounds().top() - first.bounds().bottom(), px(8.));
    assert_eq!(third.bounds().top() - second.bounds().bottom(), px(8.));
    assert_eq!(outside.bounds().top() - third.bounds().bottom(), px(16.));
    assert_eq!(first.bounds().left(), second.bounds().left());
    assert_eq!(second.bounds().left(), third.bounds().left());
    let code = layout_for(&long_line);
    let surface = visual.debug_bounds("markdown-code-4").expect("code");
    assert_eq!(code.line_layouts().len(), 1);
    assert!(code.bounds().size.width > surface.size.width);
    assert_eq!(surface.size.height, px(44.));
    let end = layout_for("End 结尾");
    assert_eq!(end.bounds().left(), outside.bounds().left());
    assert_eq!(end.bounds().top() - surface.bottom(), px(16.));
}

#[gpui_kit::test]
fn issue296_single_activity_folded_commentary_reopens_with_original_selection_and_copy(
    cx: &mut TestAppContext,
) {
    use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    let (stream, mut visual) = open_empty_selection_harness(cx);
    let process = "Review Alpha你好 😀 result.";
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "copy-run".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "copy-run".into(),
                delta: process.into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: "copy-run".into(),
                response: AssistantResponseMetadata {
                    response_id: "c".into(),
                    text_start_bytes: 0,
                    text_end_bytes: process.len(),
                    kind: AssistantResponseKind::Commentary,
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("read", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "copy-run".into(),
                delta: "Final.".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: "copy-run".into(),
                response: AssistantResponseMetadata {
                    response_id: "f".into(),
                    text_start_bytes: process.len(),
                    text_end_bytes: process.len() + 6,
                    kind: AssistantResponseKind::Final,
                },
            },
            cx,
        );
    });
    visual.run_until_parked();
    let original = stream.read_with(&visual, |stream, cx| {
        let index = (0..stream.entries.len())
            .find(|index| stream.process_entry_hidden(*index, cx))
            .expect("folded commentary");
        match &stream.entries[index] {
            StreamEntry::Assistant { copy, .. } => copy.clone(),
            _ => panic!("commentary"),
        }
    });
    let header = visual
        .debug_bounds("run-activity-toggle")
        .expect("root header");
    selection::SELECTION_TARGET_LAYOUT
        .with_borrow_mut(|target| *target = Some((process.into(), None)));
    visual.simulate_click(header.center(), Modifiers::default());
    visual.run_until_parked();
    let layout = selection::SELECTION_TARGET_LAYOUT.with_borrow_mut(|target| {
        target
            .take()
            .and_then(|(_, layout)| layout)
            .expect("commentary selection layout")
    });
    drag(
        &mut visual,
        text_point(&layout, process.find("Alpha").unwrap(), false),
        text_point(&layout, process.find(" result").unwrap(), true),
    );
    visual.run_until_parked();
    assert!(original.has_selected_text());
    assert_eq!(original.original_text(), process);
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("Alpha你好 😀")
    );
}

#[gpui_kit::test]
fn issue296_timeline_t06_real_selection_clear_and_composer_focus_release_final(
    cx: &mut TestAppContext,
) {
    use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    let (stream, mut visual) = open_empty_selection_harness(cx);
    let process = "Review Alpha你好 😀 result.";
    selection::SELECTION_TARGET_LAYOUT
        .with_borrow_mut(|target| *target = Some((process.into(), None)));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "selection-run".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "selection-run".into(),
                delta: process.into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: "selection-run".into(),
                response: AssistantResponseMetadata {
                    response_id: "process".into(),
                    text_start_bytes: 0,
                    text_end_bytes: process.len(),
                    kind: AssistantResponseKind::Commentary,
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "selection-run".into(),
                delta: "Final.".into(),
            },
            cx,
        );
    });
    visual.run_until_parked();
    let layout = selection::SELECTION_TARGET_LAYOUT.with_borrow_mut(|target| {
        target
            .take()
            .and_then(|(_, layout)| layout)
            .expect("commentary selection layout")
    });
    drag(
        &mut visual,
        text_point(&layout, process.find("Alpha").unwrap(), false),
        text_point(&layout, process.find(" result").unwrap(), true),
    );
    visual.run_until_parked();
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        "Alpha你好 😀"
    );
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: "selection-run".into(),
                response: AssistantResponseMetadata {
                    response_id: "final".into(),
                    text_start_bytes: process.len(),
                    text_end_bytes: process.len() + 6,
                    kind: AssistantResponseKind::Final,
                },
            },
            cx,
        );
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(!stream.process_entry_hidden(1, cx))
    });
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        "Alpha你好 😀"
    );
    visual.update(gpui_kit::base::TextSelection::clear);
    visual.update(|window, cx| stream.update(cx, |stream, cx| stream.focus_composer(window, cx)));
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(stream.process_entry_hidden(1, cx));
        let StreamEntry::Assistant { copy, .. } = &stream.entries[1] else {
            panic!("process")
        };
        assert_eq!(copy.original_text(), process);
    });
}
