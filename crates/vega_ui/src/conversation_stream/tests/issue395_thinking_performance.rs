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
