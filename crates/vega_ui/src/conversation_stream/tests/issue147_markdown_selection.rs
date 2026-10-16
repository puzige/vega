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
fn issue416_real_process_selection_clears_on_final_and_preserves_composer_focus(
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
        assert!(stream.process_entry_hidden(1, cx));
        let group = stream.run_activity_groups["selection-run"].read(cx);
        assert!(group.process_in_details());
        assert!(!group.expanded());
        let (index, copy) = stream
            .entries
            .iter()
            .enumerate()
            .find_map(|(index, entry)| match entry {
                StreamEntry::Assistant { copy, .. } if copy.original_text() == "Final." => {
                    Some((index, copy))
                }
                _ => None,
            })
            .expect("visible Final");
        assert!(!stream.process_entry_hidden(index, cx));
        assert_eq!(copy.original_text(), "Final.");
    });
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        ""
    );
    assert!(!visual.update(gpui_kit::base::TextSelection::has_selection));
    let group = stream.read_with(&visual, |stream, _| {
        stream.run_activity_groups["selection-run"].clone()
    });
    assert_run_disclosure_keyboard_focus(&group, &mut visual, "native-commentary-selection");
    visual.update(|window, cx| stream.update(cx, |stream, cx| stream.focus_composer(window, cx)));
    visual.run_until_parked();
    visual.update(|window, cx| {
        assert!(
            stream
                .read(cx)
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        );
    });
    stream.read_with(&visual, |stream, cx| {
        assert!(stream.process_entry_hidden(1, cx));
        let StreamEntry::Assistant { copy, .. } = &stream.entries[1] else {
            panic!("process")
        };
        assert_eq!(copy.original_text(), process);
        assert!(!copy.has_selected_text());
    });
}

struct InlineReadView {
    model: StreamModel,
    colors: vega_theme::ThemeColors,
}

impl Render for InlineReadView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(markdown_item(&self.model, None, &self.colors))
    }
}

fn inline_layout_view(
    cx: &mut TestAppContext,
    source: &str,
    selectable: bool,
    palette: vega_theme::Theme,
) -> VisualTestContext {
    cx.update(gpui_kit::init);
    if selectable {
        let (_, mut visual) = open_selection_harness(cx, assistant_entry(source));
        visual.update(|_, cx| cx.set_global(palette));
        visual.refresh().unwrap();
        visual.run_until_parked();
        visual
    } else {
        let StreamEntry::Assistant { model, .. } = assistant_entry(source) else {
            unreachable!()
        };
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| InlineReadView {
                    model,
                    colors: palette.colors,
                })
            })
            .unwrap()
        });
        VisualTestContext::from_window(window.into(), cx)
    }
}

#[gpui_kit::test]
fn issue296_padding_t01_code_reserves_space_for_both_edges(cx: &mut TestAppContext) {
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        for selectable in [true, false] {
            let mut visual = inline_layout_view(cx, "甲`code`乙", selectable, palette);
            visual.simulate_resize(size(px(800.), px(600.)));
            visual.run_until_parked();
            let (scale, quads, glyph_width) = visual.update(|window, _| {
                let mut style = message_run_style(palette.colors.text_primary);
                apply_span_style(&mut style, SpanStyle::Code, &palette.colors);
                let shaped = window.text_system().shape_line(
                    "code".into(),
                    px(Typography::CONVERSATION),
                    &[style.to_run("code".len())],
                    None,
                );
                (
                    window.scale_factor(),
                    window.painted_quads(),
                    shaped.width(),
                )
            });
            let backgrounds = quads
                .iter()
                .filter(|quad| {
                    quad.background.as_solid() == Some(palette.colors.conversation_code.into())
                })
                .collect::<Vec<_>>();
            assert_eq!(backgrounds.len(), 1);
            let background = backgrounds[0];
            assert!(
                (background.bounds.size.width - (glyph_width + px(8.)).scale(scale))
                    .0
                    .abs()
                    <= 1.,
                "code background must reserve 4px at each edge: {:?}",
                background.bounds
            );
            assert_eq!(background.bounds.size.height, px(17.5).scale(scale));
            if selectable {
                let (layout, offset) = selection::SELECTION_RUN_LAYOUTS.with_borrow(|runs| {
                    runs.iter()
                        .rev()
                        .find_map(|(text, layout, _)| {
                            text.find("code").map(|offset| (layout.clone(), offset))
                        })
                        .unwrap()
                });
                let start = layout.position_for_index(offset).unwrap();
                let end = layout.position_for_index(offset + "code".len()).unwrap();
                assert!(
                    (background.bounds.left() - (start.x - px(4.)).scale(scale))
                        .0
                        .abs()
                        <= 0.5
                );
                assert!(
                    (background.bounds.right() - (end.x + px(4.)).scale(scale))
                        .0
                        .abs()
                        <= 0.5
                );
                assert_eq!(layout.line_height(), px(23.));
            }
        }
    }
}

#[gpui_kit::test]
fn issue296_inline_t01_code_background_is_thin_and_rounded(cx: &mut TestAppContext) {
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        for selectable in [true, false] {
            let mut visual =
                inline_layout_view(cx, "中文 `inline_code`，继续🙂。", selectable, palette);
            visual.simulate_resize(size(px(800.), px(600.)));
            visual.run_until_parked();
            let (scale, quads) =
                visual.update(|window, _| (window.scale_factor(), window.painted_quads()));
            let backgrounds = quads
                .iter()
                .filter(|quad| {
                    quad.background.as_solid() == Some(palette.colors.conversation_code.into())
                })
                .collect::<Vec<_>>();
            assert_eq!(backgrounds.len(), 1);
            let background = backgrounds[0];
            assert_eq!(background.bounds.size.height, px(17.5).scale(scale));
            for corner in [
                background.corner_radii.top_left,
                background.corner_radii.top_right,
                background.corner_radii.bottom_left,
                background.corner_radii.bottom_right,
            ] {
                assert_eq!(corner, px(4.).scale(scale));
            }
            if selectable {
                let layout = layout_for("inline_code");
                let start = layout.position_for_index(0).unwrap();
                let end = layout.position_for_index("inline_code".len()).unwrap();
                assert_eq!(layout.line_height(), px(23.));
                assert!(
                    (background.bounds.left() - (start.x - px(4.)).scale(scale))
                        .0
                        .abs()
                        <= 0.5
                );
                assert!(
                    (background.bounds.right() - (end.x + px(4.)).scale(scale))
                        .0
                        .abs()
                        <= 0.5
                );
                assert!(
                    (background.bounds.top() - (start.y + px(2.75)).scale(scale))
                        .0
                        .abs()
                        <= 0.5
                );
                assert_eq!(layout.line_layouts()[0].font_size(), px(13.5));
            }
        }
    }
}

#[gpui_kit::test]
fn issue296_inline_t03_tables_allocate_width_to_long_content(cx: &mut TestAppContext) {
    let source = format!(
        "| 文件 | 改动 |\n|---|---|\n| `a.rs` | {} |\n| `b.rs` | 短说明 |\n",
        "中文说明与 `identifier` 混排。".repeat(8)
    );
    let StreamEntry::Assistant { model, .. } = assistant_entry(&source) else {
        unreachable!()
    };
    let table = model
        .committed_lines
        .iter()
        .find_map(|line| line.table.clone())
        .unwrap();
    for selectable in [true, false] {
        let mut visual = inline_layout_view(cx, &source, selectable, vega_theme::Theme::light());
        for width in [800., 520.] {
            visual.simulate_resize(size(px(width), px(1000.)));
            visual.run_until_parked();
            let first = visual.debug_bounds("markdown-table-1-0-0-0").unwrap();
            let second = visual.debug_bounds("markdown-table-1-0-0-1").unwrap();
            assert!(second.size.width > first.size.width * 2.);
            let viewport = visual.debug_bounds("markdown-table-1-0").unwrap();
            let expected_widths = table.column_widths(f32::from(viewport.size.width));
            let scale = visual.update(|window, _| window.scale_factor());
            for (actual, expected) in [first, second].iter().zip(expected_widths) {
                assert!(
                    (f32::from(actual.size.width) - expected).abs() * scale <= 1.0,
                    "actual={} expected={expected} selectable={selectable} width={width}",
                    f32::from(actual.size.width)
                );
            }
            for row in 1..=2 {
                for (column, expected) in [first, second].into_iter().enumerate() {
                    let actual = visual
                        .debug_bounds(Box::leak(
                            format!("markdown-table-1-0-{row}-{column}").into_boxed_str(),
                        ))
                        .unwrap();
                    assert_eq!(actual.left(), expected.left());
                    assert_eq!(actual.right(), expected.right());
                    assert!(actual.size.width >= px(120.));
                }
            }
        }
    }
}

fn fragment_point(
    runs: &[(String, gpui_kit::TextLayout, gpui_kit::Bounds<Pixels>)],
    index: usize,
    end: bool,
) -> Point<Pixels> {
    let mut offset = 0;
    for (text, layout, _) in runs {
        if index < offset + text.len() || (end && index == offset + text.len()) {
            return text_point(layout, index - offset, end);
        }
        offset += text.len();
    }
    panic!("source index missing: {index}")
}

#[gpui_kit::test]
fn issue296_inline_t02_wrapped_code_preserves_ranges_and_copy(cx: &mut TestAppContext) {
    let identifier = "long_identifier_".repeat(14);
    let source = format!("前文🙂，`{identifier}`，`尾部` **强调** [链接](https://example.com)");
    let visible = format!("前文🙂，{identifier}，尾部 强调 链接");
    let first = "前文🙂，".len().."前文🙂，".len() + identifier.len();
    let second = first.end + "，".len()..first.end + "，尾部".len();
    let (stream, mut visual) = open_selection_harness(cx, assistant_entry(&source));
    visual.simulate_resize(size(px(380.), px(1000.)));
    visual.run_until_parked();
    let runs = selection::SELECTION_RUN_LAYOUTS.with_borrow(Clone::clone);
    assert_eq!(
        runs.iter()
            .map(|(text, _, _)| text.as_str())
            .collect::<String>(),
        visible
    );
    assert!(
        runs.iter()
            .map(|(_, layout, _)| layout.bounds().top())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            >= 4
    );
    let (scale, quads, color) = visual.update(|window, cx| {
        (
            window.scale_factor(),
            window.painted_quads(),
            theme(cx).colors.conversation_code,
        )
    });
    let backgrounds = quads
        .iter()
        .filter(|quad| quad.background.as_solid() == Some(color.into()))
        .collect::<Vec<_>>();
    let mut code_lines = std::collections::BTreeSet::new();
    let mut offset = 0;
    for (text, layout, bounds) in &runs {
        for (index, ch) in text.char_indices() {
            let start = layout.position_for_index(index).unwrap();
            let end = layout.position_for_index(index + ch.len_utf8()).unwrap();
            let midpoint = Point::new((start.x + end.x) / 2., start.y + px(11.5)).scale(scale);
            let source_index = offset + index;
            let is_code = first.contains(&source_index) || second.contains(&source_index);
            assert_eq!(
                backgrounds
                    .iter()
                    .any(|quad| quad.bounds.contains(&midpoint)),
                is_code,
                "glyph {source_index}: {ch}"
            );
            if is_code {
                code_lines.insert((if first.contains(&source_index) { 0 } else { 1 }, start.y));
            }
        }
        assert!(bounds.right() <= px(348.5));
        offset += text.len();
    }
    assert_eq!(backgrounds.len(), code_lines.len());
    for background in backgrounds {
        assert_eq!(background.bounds.size.height, px(17.5).scale(scale));
        assert_eq!(background.corner_radii.top_left, px(4.).scale(scale));
    }
    for range in [first.clone(), first.start + 5..first.end - 3] {
        drag(
            &mut visual,
            fragment_point(&runs, range.start, false),
            fragment_point(&runs, range.end, true),
        );
        visual.run_until_parked();
        assert_eq!(
            visual.update(gpui_kit::base::TextSelection::selected_text),
            visible[range.clone()]
        );
        visual.simulate_keystrokes("cmd-c");
        assert_eq!(
            visual.read_from_clipboard().unwrap().text().unwrap(),
            visible[range]
        );
    }
    stream.read_with(&visual, |stream, _| {
        assert!(matches!(&stream.entries[0], StreamEntry::Assistant { copy, .. } if copy.original_text() == source));
    });
}

#[gpui_kit::test]
fn issue296_inline_t02_center_and_right_table_backgrounds_follow_wrapped_glyphs(
    cx: &mut TestAppContext,
) {
    let left = "center_identifier_".repeat(6);
    let right = "right_identifier_".repeat(6);
    let source = format!("| C | R |\n|:---:|---:|\n| `{left}`。 | `{right}`！ |\n");
    let (_, mut visual) = open_selection_harness(cx, assistant_entry(&source));
    visual.simulate_resize(size(px(500.), px(1000.)));
    visual.run_until_parked();
    let (scale, quads, color) = visual.update(|window, cx| {
        (
            window.scale_factor(),
            window.painted_quads(),
            theme(cx).colors.conversation_code,
        )
    });
    let backgrounds = quads
        .iter()
        .filter(|quad| quad.background.as_solid() == Some(color.into()))
        .collect::<Vec<_>>();
    let mut expected = 0;
    for (text, punctuation, selector, centered) in [
        (&left, "。", "markdown-table-1-0-1-0", true),
        (&right, "！", "markdown-table-1-0-1-1", false),
    ] {
        let visible = format!("{text}{punctuation}");
        let cell = visual.debug_bounds(selector).unwrap();
        let runs = selection::SELECTION_RUN_LAYOUTS.with_borrow(|runs| {
            runs.iter()
                .filter(|(_, _, bounds)| cell.contains(&bounds.center()))
                .cloned()
                .collect::<Vec<_>>()
        });
        assert_eq!(
            runs.iter()
                .map(|(text, _, _)| text.as_str())
                .collect::<String>(),
            visible
        );
        let mut rows = std::collections::BTreeMap::<Pixels, (Pixels, Pixels)>::new();
        let mut offset = 0;
        for (fragment, layout, _) in runs {
            let start = layout.position_for_index(0).unwrap();
            let end = layout.position_for_index(fragment.len()).unwrap();
            let padding = if offset < text.len() { px(4.) } else { px(0.) };
            let left = start.x - padding;
            let right = end.x + padding;
            let row = rows.entry(start.y).or_insert((left, right));
            row.0 = row.0.min(left);
            row.1 = row.1.max(right);
            if offset < text.len() {
                let y = start.y + px(2.75);
                assert!(backgrounds.iter().any(|quad| {
                    (quad.bounds.left() - left.scale(scale)).0.abs() <= 0.5
                        && (quad.bounds.right() - right.scale(scale)).0.abs() <= 0.5
                        && (quad.bounds.top() - y.scale(scale)).0.abs() <= 0.5
                }));
                expected += 1;
            }
            offset += fragment.len();
        }
        assert!(rows.len() > 1);
        for (left, right) in rows.values() {
            assert!(*left >= cell.left() + px(11.5));
            assert!(*right <= cell.right() - px(11.5));
            if centered {
                assert!(((*left + *right) / 2. - cell.center().x).abs() * scale <= px(1.));
            } else {
                assert!((*right - (cell.right() - px(12.))).abs() * scale <= px(1.));
            }
        }
    }
    assert_eq!(backgrounds.len(), expected);
}

#[gpui_kit::test]
fn issue296_inline_t04_table_estimates_update_and_measured_anchor_survives_resize(
    cx: &mut TestAppContext,
) {
    let mut markdown = MarkdownStream::new();
    let counters = StreamCounters::default();
    let mut model = StreamModel::default();
    markdown.append("| 文件 | 描述 |\n|---|---|\n| a.rs | 短 |\n");
    model.sync(&markdown.snapshot(), &counters);
    let initial = model
        .pending_lines
        .iter()
        .find_map(|line| line.table.clone())
        .unwrap();
    assert_eq!(initial.column_widths(736.), vec![368., 368.]);
    markdown.append(&format!("| b.rs | {} |\n", "中".repeat(100)));
    model.sync(&markdown.snapshot(), &counters);
    let updated = model
        .pending_lines
        .iter()
        .find_map(|line| line.table.clone())
        .unwrap();
    assert!(!Arc::ptr_eq(&initial, &updated));
    assert!(updated.column_widths(736.)[1] > 590.);
    assert!(!model.sync(&markdown.snapshot(), &counters));
    assert!(Arc::ptr_eq(
        &updated,
        model
            .pending_lines
            .iter()
            .find_map(|line| line.table.as_ref())
            .unwrap()
    ));
    markdown.finish();
    model.sync(&markdown.snapshot(), &counters);
    assert_eq!(
        message_anchors::estimate_markdown_height(&model, 736.),
        5. * 23. + 3. * 17.
    );
    assert_eq!(
        message_anchors::estimate_markdown_height(&model, 320.),
        12. * 23. + 3. * 17.
    );
    let source = format!(
        "| 文件 | 描述 |\n|---|---|\n| a.rs | 短 |\n| b.rs | {} |\n",
        "中".repeat(100)
    );
    let (stream, mut visual) = open_selection_harness(cx, assistant_entry(&source));
    stream.update(&mut visual, |stream, cx| {
        stream
            .entries
            .push(assistant_entry(&"Tail paragraph.\n\n".repeat(40)));
        stream.list_append(1);
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    let mut previous_height = px(0.);
    for width in [800., 384.] {
        visual.simulate_resize(size(px(width), px(300.)));
        visual.run_until_parked();
        let table_bounds = visual.debug_bounds("markdown-table-1-0").unwrap();
        assert!(table_bounds.size.height > previous_height);
        previous_height = table_bounds.size.height;
        let anchor = stream.update(&mut visual, |stream, cx| {
            stream.capture_visible_entry_heights();
            let actual = stream.list.bounds_for_item(0).unwrap().size.height;
            assert_eq!(
                stream.message_anchor_geometry(cx, 300.).entry_heights[0],
                f32::from(actual)
            );
            let anchor = ThreadScrollAnchor {
                identity: Some(stream.entry_identities[0].key.clone()),
                message_id: stream.entry_identities[0].message_id.clone(),
                offset_in_item_px: 35.,
                following_tail: false,
            };
            assert!(stream.restore_scroll_anchor(&anchor));
            cx.notify();
            anchor
        });
        visual.run_until_parked();
        stream.read_with(&visual, |stream, _| {
            let restored = stream.scroll_anchor_snapshot();
            assert_eq!(restored.identity, anchor.identity);
            assert_eq!(restored.offset_in_item_px, 35.);
            assert!(!restored.following_tail);
            assert!(matches!(&stream.entries[0], StreamEntry::Assistant { copy, .. } if copy.original_text() == source));
        });
    }
    let nested_source = format!(
        "- Intro\n\n{}",
        source
            .lines()
            .map(|line| format!("  {line}\n"))
            .collect::<String>()
    );
    let nested_entry = assistant_entry(&nested_source);
    let StreamEntry::Assistant {
        model: nested_model,
        ..
    } = &nested_entry
    else {
        unreachable!()
    };
    let nested_table = nested_model
        .committed_lines
        .iter()
        .find_map(|line| line.table.clone())
        .unwrap();
    assert_eq!(
        message_anchors::estimate_markdown_height(nested_model, 320.),
        message_anchors::estimate_markdown_height(&model, 296.) + 23. + 8.
    );
    let (nested_stream, mut nested_visual) = open_selection_harness(cx, nested_entry);
    nested_visual.simulate_resize(size(px(384.), px(1000.)));
    nested_visual.run_until_parked();
    let viewport = nested_visual.debug_bounds("markdown-table-1-0").unwrap();
    assert_eq!(viewport.size.width, px(296.));
    let widths = nested_table.column_widths(296.);
    let scale = nested_visual.update(|window, _| window.scale_factor());
    for (selector, expected) in ["markdown-table-1-0-1-0", "markdown-table-1-0-1-1"]
        .into_iter()
        .zip(widths)
    {
        assert!(
            (f32::from(nested_visual.debug_bounds(selector).unwrap().size.width) - expected).abs()
                * scale
                <= 1.0
        );
    }
    let first_source = format!("- {}", source.replace('\n', "\n  "));
    let first_entry = assistant_entry(&first_source);
    let StreamEntry::Assistant {
        model: first_model, ..
    } = &first_entry
    else {
        unreachable!()
    };
    assert!(
        first_model
            .committed_lines
            .iter()
            .any(|line| line.kind == LineKind::ListItem && line.table.is_some())
    );
    assert_eq!(
        message_anchors::estimate_markdown_height(first_model, 320.),
        message_anchors::estimate_markdown_height(&model, 320.)
    );
    nested_stream.update(&mut nested_visual, |stream, cx| {
        stream.entries[0] = first_entry;
        stream.invalidate_item(Some(0));
        cx.notify();
    });
    nested_visual.run_until_parked();
    assert_eq!(
        nested_visual
            .debug_bounds("markdown-table-1-0")
            .unwrap()
            .size
            .width,
        px(320.)
    );
}

struct NarrowInlineView;

impl Render for NarrowInlineView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let style = TextStyle {
            font_family: MONOFONT.into(),
            background_color: Some(vega_theme::Theme::light().colors.conversation_code.into()),
            ..TextStyle::default()
        };
        div()
            .size_full()
            .text_size(px(10.))
            .line_height(px(23.))
            .child(inline_code::styled_text("x".into(), vec![style.to_run(1)]))
    }
}

#[gpui_kit::test]
fn issue296_inline_t01_narrow_fragment_clamps_rounding(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, visual) = cx.add_window_view(|_, _| NarrowInlineView);
    visual.run_until_parked();
    let (scale, quads) = visual.update(|window, _| (window.scale_factor(), window.painted_quads()));
    let color = vega_theme::Theme::light().colors.conversation_code;
    let backgrounds = quads
        .iter()
        .filter(|quad| quad.background.as_solid() == Some(color.into()))
        .collect::<Vec<_>>();
    assert_eq!(backgrounds.len(), 1);
    assert_eq!(backgrounds[0].bounds.size.width, px(14.).scale(scale));
    assert_eq!(backgrounds[0].bounds.size.height, px(14.).scale(scale));
    let corners = backgrounds[0].corner_radii;
    for radius in [
        corners.top_left,
        corners.top_right,
        corners.bottom_left,
        corners.bottom_right,
    ] {
        assert_eq!(radius, px(4.).scale(scale));
    }
}

fn padding_multi_click(cx: &mut TestAppContext, source: &str, expected: &str, count: usize) {
    let (stream, mut visual) = open_selection_harness(cx, assistant_entry(source));
    visual.simulate_resize(size(px(380.), px(1000.)));
    visual.run_until_parked();
    let runs = selection::SELECTION_RUN_LAYOUTS.with_borrow(Clone::clone);
    let (text, layout, _) = runs
        .iter()
        .find(|(text, _, _)| text.contains("code"))
        .unwrap();
    let point = text_point(layout, text.find("code").unwrap() + 1, false);
    visual.simulate_event(gpui_kit::MouseDownEvent {
        button: MouseButton::Left,
        position: point,
        modifiers: Modifiers::default(),
        click_count: count,
        first_mouse: false,
    });
    visual.simulate_event(gpui_kit::MouseUpEvent {
        button: MouseButton::Left,
        position: point,
        modifiers: Modifiers::default(),
        click_count: count,
    });
    visual.run_until_parked();
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        expected
    );
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual.read_from_clipboard().unwrap().text().unwrap(),
        expected
    );
    visual.simulate_click(point, Modifiers::default());
    visual.run_until_parked();
    assert!(
        visual
            .update(gpui_kit::base::TextSelection::selected_text)
            .is_empty()
    );
    drag(
        &mut visual,
        text_point(layout, 0, false),
        text_point(layout, 2, true),
    );
    visual.run_until_parked();
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        &text[..2]
    );
    stream.read_with(&visual, |stream, _| assert!(matches!(&stream.entries[0], StreamEntry::Assistant { copy, .. } if copy.original_text() == source)));
    visual.simulate_event(gpui_kit::MouseDownEvent {
        button: MouseButton::Left,
        position: point,
        modifiers: Modifiers::default(),
        click_count: count,
        first_mouse: false,
    });
    visual.simulate_event(gpui_kit::MouseUpEvent {
        button: MouseButton::Left,
        position: point,
        modifiers: Modifiers::default(),
        click_count: count,
    });
    visual.run_until_parked();
    visual.update(|_, cx| {
        stream.update(cx, |stream, cx| {
            let StreamEntry::Assistant {
                copy,
                stream: markdown,
                model,
                ..
            } = &mut stream.entries[0]
            else {
                panic!("assistant source");
            };
            assert!(copy.local_selection_range().is_some());
            let generation = copy.selection_generation();
            copy.append(" updated");
            assert!(copy.selection_generation() > generation);
            assert!(copy.local_selection_range().is_none());
            assert!(!copy.has_selected_text());
            **markdown = MarkdownStream::new();
            markdown.append(&copy.original_text());
            markdown.finish();
            model.sync(&markdown.snapshot(), &StreamCounters::default());
            cx.notify();
        });
    });
    visual.run_until_parked();
    assert!(
        visual
            .update(gpui_kit::base::TextSelection::selected_text)
            .is_empty()
    );
}

#[gpui_kit::test]
fn issue296_padding_t03_triple_click_selects_original_logical_line(cx: &mut TestAppContext) {
    padding_multi_click(cx, "hello `code` world", "hello code world", 3);
}

#[gpui_kit::test]
fn issue296_padding_t03_double_click_selects_soft_wrapped_word(cx: &mut TestAppContext) {
    let word = format!("code{}", "_identifier".repeat(8));
    padding_multi_click(cx, &format!("before `{word}` after"), &word, 2);
}

#[gpui_kit::test]
fn issue296_padding_t02_mixed_scripts_and_kerning_stay_within_column(cx: &mut TestAppContext) {
    for source in [
        "AVAVAVAV`code`AVAVAV",
        "שלום עולם `code_identifier` עוד טקסט",
        "中文🙂`a👨‍👩‍👧‍👦b_identifier`，`第二段`**加粗**[链接](https://example.com)",
    ] {
        let (_, visual) = open_selection_harness(cx, assistant_entry(source));
        for width in [180., 240., 380.] {
            visual.simulate_resize(size(px(width), px(1000.)));
            visual.run_until_parked();
            let runs = selection::SELECTION_RUN_LAYOUTS.with_borrow(Clone::clone);
            let expected = source
                .replace('`', "")
                .replace("**", "")
                .replace("[链接](https://example.com)", "链接");
            assert_eq!(
                runs.iter()
                    .map(|(text, _, _)| text.as_str())
                    .collect::<String>(),
                expected
            );
            for (text, layout, bounds) in &runs {
                assert!(bounds.left() >= px(31.5), "{source}: {text:?} {bounds:?}");
                assert!(
                    bounds.right() <= px(width - 31.5),
                    "{source}: {text:?} {bounds:?}"
                );
                assert_eq!(layout.line_height(), px(23.));
            }
        }
    }
}
