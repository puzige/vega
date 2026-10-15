use super::*;

fn start_thinking(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "thinking-performance".into(),
            seq: 2,
        },
        cx,
    );
}

fn append(stream: &mut ConversationStream, delta: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::ThinkingDelta {
            message_id: "thinking-performance".into(),
            delta: delta.into(),
        },
        cx,
    );
}

#[gpui_kit::test]
async fn issue395_collapsed_deltas_keep_source_without_preparing_or_invalidating(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "thinking-idle-body");
    let card = stream.update(cx, |stream, cx| {
        start_thinking(stream, cx);
        append(stream, "```text\n", cx);
        stream.active_thinking.clone().expect("thinking card")
    });
    cx.run_until_parked();
    let group = stream.read_with(cx, |stream, _| {
        stream.run_activity_groups["thinking-performance"].clone()
    });
    let changes = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = changes.clone();
    let _subscription =
        cx.update(|cx| cx.observe(&group, move |_, _| observed.set(observed.get() + 1)));
    let display_changes = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed_display = display_changes.clone();
    let _display_subscription = cx.update(|cx| {
        cx.subscribe(
            &group,
            move |_, _: &run_activity::RunActivityDisplayChanged, _| {
                observed_display.set(observed_display.get() + 1);
            },
        )
    });
    for _ in 0..128 {
        stream.update(cx, |stream, cx| append(stream, "汉字 body\n", cx));
        cx.run_until_parked();
    }
    card.read_with(cx, |card, _| {
        assert!(!card.test_body_preparing());
        assert_eq!(card.test_body_fragment_count(), 0);
        assert_eq!(
            card.source_text(),
            format!("```text\n{}", "汉字 body\n".repeat(128))
        );
        assert_eq!(card.test_source_copy().original_text(), card.source_text());
    });
    assert_eq!(changes.get(), 0);
    assert_eq!(display_changes.get(), 0);
}

#[gpui_kit::test]
async fn issue395_giant_paragraph_and_code_render_only_viewport_fragments(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "thinking-virtual-body");
    let card = stream.update(cx, |stream, cx| {
        start_thinking(stream, cx);
        for _ in 0..3 {
            append(
                stream,
                &"A **bold** paragraph with `inline code` 汉字. ".repeat(1000),
                cx,
            );
        }
        append(stream, "\n\n```rust\n", cx);
        append(stream, &"identifier_".repeat(4000), cx);
        append(stream, "\n```\n", cx);
        let card = stream.active_thinking.clone().expect("thinking card");
        stream.run_activity_groups["thinking-performance"].update(cx, |group, cx| {
            group.reveal_segment(0, cx);
        });
        assert_eq!(card.read(cx).test_body_fragment_count(), 0);
        card
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let bounds = visual.debug_bounds("thinking-content").expect("disclosure");
    assert!(bounds.size.height <= px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT));
    let (prepared, painted) = card.read_with(&visual, |card, _| {
        (
            card.test_body_fragment_count(),
            card.test_body_rendered_count(),
        )
    });
    assert!(
        prepared > 500,
        "giant paragraph is divided by real line wraps"
    );
    assert!(painted < 100, "only visible fragments enter GPUI layout");
    assert!(painted < prepared);
    let scroll = card.read_with(&visual, |card, _| card.scroll_handle());
    scroll.set_offset(gpui_kit::point(px(0.), -scroll.max_offset().y));
    window
        .update(&mut visual, |_, window, _| window.refresh())
        .expect("refresh");
    visual.run_until_parked();
    assert!(card.read_with(&visual, |card, _| card.test_body_rendered_count()) < 100);
    assert!(
        card.read_with(&visual, |card, _| card.test_source_copy().original_text())
            .contains("```rust")
    );
}

#[gpui_kit::test]
async fn issue395_hidden_ancestor_rejects_preparation_and_reopens_latest_source(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "thinking-hidden-parent");
    let (group, card) = stream.update(cx, |stream, cx| {
        start_thinking(stream, cx);
        append(stream, &"Long paragraph 汉字. ".repeat(2000), cx);
        let group = stream.run_activity_groups["thinking-performance"].clone();
        group.update(cx, |group, cx| {
            group.reveal_segment(0, cx);
        });
        (
            group,
            stream.active_thinking.clone().expect("thinking card"),
        )
    });
    cx.run_until_parked();
    let revision = card.read_with(cx, |card, _| card.test_body_revision());
    stream.update(cx, |stream, cx| {
        append(stream, "\n\nlatest before hide", cx)
    });
    group.update(cx, |group, cx| {
        group.confirm_final(HashSet::new(), false, cx);
    });
    stream.update(cx, |stream, cx| {
        append(stream, "\n\nreceived while hidden", cx)
    });
    cx.run_until_parked();
    assert_eq!(
        card.read_with(cx, |card, _| card.test_body_revision()),
        revision
    );
    assert!(!card.read_with(cx, |card, _| card.test_body_preparing()));
    group.update(cx, RunActivityGroup::reveal);
    cx.run_until_parked();
    assert!(card.read_with(cx, |card, _| card.test_body_revision()) > revision);
    assert!(
        gpui_kit::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("thinking-content")
            .is_some()
    );
    assert!(
        card.read_with(cx, |card, _| card.source_text())
            .ends_with("received while hidden")
    );
}

fn text_point(layout: &gpui_kit::TextLayout, index: usize) -> gpui_kit::Point<gpui_kit::Pixels> {
    let mut point = layout.position_for_index(index).expect("text geometry");
    point.y += layout.line_height() / 2.;
    point.x += px(0.5);
    point
}

#[gpui_kit::test]
fn issue395_virtual_thinking_selection_keeps_logical_paragraph_and_rejects_new_source(
    cx: &mut TestAppContext,
) {
    let (stream, mut visual) = issue147_markdown_selection::open_empty_selection_harness(cx);
    let word = format!("code{}", "_identifier".repeat(8));
    let source = format!(
        "before `{word}` {} after",
        "汉字 wrapped text. ".repeat(800)
    );
    let card = stream.update(&mut visual, |stream, cx| {
        start_thinking(stream, cx);
        append(stream, &source, cx);
        let group = stream.run_activity_groups["thinking-performance"].clone();
        group.update(cx, |group, cx| {
            group.reveal_segment(0, cx);
        });
        stream.active_thinking.clone().expect("thinking card")
    });
    visual.simulate_resize(gpui_kit::size(px(500.), px(1000.)));
    visual.run_until_parked();
    let expected = card
        .read_with(&visual, |card, _| card.test_prepared_text())
        .expect("prepared text");
    assert_eq!(expected.as_ref(), source.replace('`', ""));
    let copy = card.read_with(&visual, |card, _| card.test_source_copy());
    assert_eq!(copy.visible_text().as_deref(), Some(expected.as_ref()));
    for (click_count, expected_selection) in [(2, word.as_str()), (3, expected.as_ref())] {
        let layout = selection::SELECTION_RUN_LAYOUTS.with_borrow(|runs| {
            runs.iter()
                .rev()
                .find(|(text, _, _)| text.contains("code"))
                .map(|(_, layout, _)| layout.clone())
                .expect("visible code fragment")
        });
        let point = text_point(&layout, 1);
        visual.simulate_event(gpui_kit::MouseDownEvent {
            button: MouseButton::Left,
            position: point,
            modifiers: gpui_kit::Modifiers::default(),
            click_count,
            first_mouse: false,
        });
        visual.simulate_event(gpui_kit::MouseUpEvent {
            button: MouseButton::Left,
            position: point,
            modifiers: gpui_kit::Modifiers::default(),
            click_count,
        });
        visual.run_until_parked();
        assert_eq!(
            visual.update(gpui_kit::base::TextSelection::selected_text),
            expected_selection
        );
        visual.simulate_keystrokes("cmd-c");
        assert_eq!(
            visual
                .read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some(expected_selection)
        );
    }
    for width in [420., 700.] {
        visual.simulate_resize(gpui_kit::size(px(width), px(1000.)));
        visual.run_until_parked();
        assert_eq!(
            visual.update(gpui_kit::base::TextSelection::selected_text),
            expected.as_ref()
        );
    }
    let scroll = card.read_with(&visual, |card, _| card.scroll_handle());
    scroll.set_offset(gpui_kit::point(px(0.), -scroll.max_offset().y));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        expected.as_ref()
    );
    assert!(card.read_with(&visual, |card, _| card.test_body_rendered_count()) < 100);
    assert_eq!(copy.visible_text().as_deref(), Some(expected.as_ref()));
    card.update(&mut visual, |card, cx| {
        card.test_replace_source("replacement", cx)
    });
    visual.run_until_parked();
    assert!(
        visual
            .update(gpui_kit::base::TextSelection::selected_text)
            .is_empty()
    );
    assert_eq!(scroll.offset().y, px(0.));
    assert_eq!(scroll.max_offset().y, px(0.));
    assert_eq!(copy.visible_text().as_deref(), Some("replacement"));
}

#[gpui_kit::test]
fn issue395_code_horizontal_offset_survives_vertical_recycling(cx: &mut TestAppContext) {
    let (stream, mut visual) = issue147_markdown_selection::open_empty_selection_harness(cx);
    let card = stream.update(&mut visual, |stream, cx| {
        start_thinking(stream, cx);
        append(
            stream,
            &format!(
                "```rust\n{}\n```\n\n{}",
                "identifier_".repeat(1500),
                "later paragraph\n\n".repeat(100)
            ),
            cx,
        );
        stream.run_activity_groups["thinking-performance"].update(cx, |group, cx| {
            group.reveal_segment(0, cx);
        });
        stream.active_thinking.clone().expect("thinking card")
    });
    visual.run_until_parked();
    let horizontal = card
        .read_with(&visual, |card, _| card.test_horizontal_scroll())
        .expect("horizontal code scroll");
    assert!(horizontal.max_offset().x > px(100.));
    horizontal.set_offset(gpui_kit::point(px(-100.), px(0.)));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert_eq!(horizontal.offset().x, px(-100.));
    let vertical = card.read_with(&visual, |card, _| card.scroll_handle());
    vertical.set_offset(gpui_kit::point(px(0.), -vertical.max_offset().y));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    vertical.set_offset(gpui_kit::point(px(0.), px(0.)));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert_eq!(horizontal.offset().x, px(-100.));
    assert!(card.read_with(&visual, |card, _| card.test_body_rendered_count()) < 100);
}

#[gpui_kit::test]
fn issue395_offscreen_thinking_defers_new_source_until_visible(cx: &mut TestAppContext) {
    let (stream, mut visual) = issue147_markdown_selection::open_empty_selection_harness(cx);
    let card = stream.update(&mut visual, |stream, cx| {
        start_thinking(stream, cx);
        append(stream, &"visible paragraph\n\n".repeat(40), cx);
        stream.run_activity_groups["thinking-performance"].update(cx, |group, cx| {
            group.reveal_segment(0, cx);
        });
        stream.active_thinking.clone().expect("thinking card")
    });
    visual.run_until_parked();
    let revision = card.read_with(&visual, |card, _| card.test_body_revision());
    stream.update(&mut visual, |stream, cx| {
        let start = stream.entries.len();
        for index in 0..60 {
            stream.entries.push(StreamEntry::User {
                lines: user_message_lines(1000 + index, "Later message"),
                copy: MessageCopy::new("Later message"),
            });
        }
        stream.list_append(start);
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: stream.entries.len() - 1,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_none());
    stream.update(&mut visual, |stream, cx| {
        append(stream, "new hidden source", cx)
    });
    visual.run_until_parked();
    assert_eq!(
        card.read_with(&visual, |card, _| card.test_body_revision()),
        revision
    );
    assert!(!card.read_with(&visual, |card, _| card.test_body_preparing()));
    stream.update(&mut visual, |stream, cx| {
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: 1,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    visual.run_until_parked();
    assert!(
        card.read_with(&visual, |card, _| card.test_prepared_text())
            .expect("latest body")
            .ends_with("new hidden source")
    );
}

#[gpui_kit::test]
fn issue395_centered_and_right_table_fragments_keep_inline_order(cx: &mut TestAppContext) {
    let (stream, mut visual) = issue147_markdown_selection::open_empty_selection_harness(cx);
    selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
    stream.update(&mut visual, |stream, cx| {
        start_thinking(stream, cx);
        append(
            stream,
            "| Center | Right |\n| :---: | ---: |\n| pre `center` post | left `right` tail |\n",
            cx,
        );
        stream.run_activity_groups["thinking-performance"].update(cx, |group, cx| {
            group.reveal_segment(0, cx);
        });
    });
    visual.run_until_parked();
    let runs = selection::SELECTION_RUN_LAYOUTS.with_borrow(Clone::clone);
    for [before, code, after] in [["pre ", "center", " post"], ["left ", "right", " tail"]] {
        let bounds = [before, code, after].map(|target| {
            runs.iter()
                .rev()
                .find(|(text, _, _)| text == target)
                .map(|(_, _, bounds)| *bounds)
                .expect("table text fragment")
        });
        assert_eq!(bounds[0].top(), bounds[1].top());
        assert_eq!(bounds[1].top(), bounds[2].top());
        assert!(bounds[0].right() <= bounds[1].left());
        assert!(bounds[1].right() <= bounds[2].left());
    }
}

#[gpui_kit::test]
fn issue395_summary_transition_removes_open_body_and_invalidates_stage_height(
    cx: &mut TestAppContext,
) {
    let (stream, mut visual) = issue147_markdown_selection::open_empty_selection_harness(cx);
    stream.update(&mut visual, |stream, cx| {
        start_thinking(stream, cx);
        append(stream, &"source paragraph\n\n".repeat(30), cx);
        stream.run_activity_groups["thinking-performance"].update(cx, |group, cx| {
            group.reveal_segment(0, cx);
        });
    });
    visual.run_until_parked();
    let group = stream.read_with(&visual, |stream, _| {
        stream.run_activity_groups["thinking-performance"].clone()
    });
    let invalidated = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = invalidated.clone();
    let _subscription = visual.update(|_, cx| {
        cx.subscribe(
            &group,
            move |_, change: &run_activity::RunActivityDisplayChanged, _| {
                observed.set(observed.get() || change.segment == Some(0) && change.geometry);
            },
        )
    });
    let before = visual
        .debug_bounds("run-timeline-segment-0")
        .expect("open stage")
        .size
        .height;
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "thinking-performance".into(),
                delta: "A short summary".into(),
            },
            cx,
        );
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_none());
    let after = visual
        .debug_bounds("run-timeline-segment-0")
        .expect("summary stage")
        .size
        .height;
    assert!(after < before);
    assert!(invalidated.get());
    assert!(!stream.read_with(&visual, |stream, cx| {
        stream
            .active_thinking
            .as_ref()
            .expect("thinking")
            .read(cx)
            .test_body_preparing()
    }));
}
