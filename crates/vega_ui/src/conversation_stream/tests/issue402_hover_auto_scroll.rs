use super::*;
use gpui_kit::{Bounds, Modifiers, MouseButton, Pixels, Point, VisualTestContext, point, px, size};
use std::time::Duration;

const SEGMENT_COUNT: usize = 220;

struct HoverScrollHarness {
    stream: Entity<ConversationStream>,
}

impl Render for HoverScrollHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .size_full()
            .child(gpui_kit::base::TextSelectionLayer)
            .child(self.stream.clone())
    }
}

fn open_hover_harness(cx: &mut TestAppContext) -> (Entity<ConversationStream>, VisualTestContext) {
    init_permission_test(cx);
    let mut thread = permission_thread();
    thread.id = "issue402-hover".into();
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let root = stream.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(move |_| HoverScrollHarness { stream: root })
        })
        .expect("issue402 hover window")
    });
    cx.run_until_parked();
    let visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(1000.), px(900.)));
    visual.run_until_parked();
    (stream, visual)
}

fn draw(visual: &mut VisualTestContext) {
    visual.update(|window, cx| {
        _ = window.draw(cx);
    });
    visual.run_until_parked();
}

/// One long single-line body. GPUI wraps it inside the bubble, so the fixture
/// stays a single selection run whose text equals its layout text.
fn body_text() -> String {
    (0..SEGMENT_COUNT)
        .map(|segment| format!("issue402 segment {segment:03} "))
        .collect()
}

fn seed_scrollable(visual: &mut VisualTestContext, stream: &Entity<ConversationStream>) {
    let text = body_text();
    stream.update(visual, |stream, cx| {
        stream.entries = vec![StreamEntry::User {
            copy: MessageCopy::new(&text),
            lines: user_message_lines(402, &text),
        }];
        stream.list_append(0);
        cx.notify();
    });
    visual.run_until_parked();
}

fn detach_tail(visual: &mut VisualTestContext, stream: &Entity<ConversationStream>) {
    stream.update(visual, |stream, cx| {
        stream.list.scroll_by(px(-600.));
        cx.notify();
    });
    visual.run_until_parked();
    assert!(
        !stream.read_with(visual, |stream, _| stream.following_tail()),
        "the upward scroll must detach tail follow before hovering is observed"
    );
}

fn viewport(visual: &mut VisualTestContext) -> Bounds<Pixels> {
    visual
        .debug_bounds("conversation-message-list")
        .expect("transcript viewport")
}

/// The single wrapped body run, matching the production text exactly.
fn body_layout(text: &str) -> gpui_kit::TextLayout {
    selection::SELECTION_RUN_LAYOUTS.with_borrow(|runs| {
        runs.iter()
            .rev()
            .find(|(run_text, _, _)| run_text == text)
            .map(|(_, layout, _)| layout.clone())
            .expect("production selection text layout")
    })
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

/// Picks a selection index whose line stays inside `viewport`, so the drag
/// never depends on how far the transcript happens to be scrolled.
fn index_in_viewport(
    layout: &gpui_kit::TextLayout,
    viewport: Bounds<Pixels>,
    target_y: Pixels,
) -> usize {
    let probe = point(viewport.center().x, target_y);
    layout
        .index_for_position(probe)
        .unwrap_or_else(|index| index)
}

fn selection_span(
    text: &str,
    layout: &gpui_kit::TextLayout,
    viewport: Bounds<Pixels>,
) -> (Point<Pixels>, Point<Pixels>) {
    let last = text.len().saturating_sub(1);
    let start = index_in_viewport(layout, viewport, viewport.top() + px(60.)).min(last);
    let end = index_in_viewport(layout, viewport, viewport.bottom() - px(60.)).min(last);
    let (first, last) = if start <= end {
        (start, (end + 1).min(last))
    } else {
        (end, (start + 1).min(last))
    };
    (
        text_point(layout, first, false),
        text_point(layout, last, true),
    )
}

fn body_copy(visual: &VisualTestContext, stream: &Entity<ConversationStream>) -> MessageCopy {
    stream.read_with(visual, |stream, _| match &stream.entries[0] {
        StreamEntry::User { copy, .. } => copy.clone(),
        _ => panic!("the fixture seeds exactly one user entry"),
    })
}

fn select_body_text(visual: &mut VisualTestContext, stream: &Entity<ConversationStream>) -> bool {
    draw(visual);
    let viewport = viewport(visual);
    let layout = body_layout(&body_text());
    let (start, end) = selection_span(&body_text(), &layout, viewport);
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    visual.run_until_parked();
    let copy = body_copy(visual, stream);
    copy.has_selected_text()
}

fn settle(visual: &mut VisualTestContext) {
    for _ in 0..6 {
        visual.executor().advance_clock(Duration::from_millis(20));
        visual.update(|window, cx| {
            window.simulate_next_frame(cx);
        });
        visual.run_until_parked();
    }
}

fn scroll_top(
    visual: &VisualTestContext,
    stream: &Entity<ConversationStream>,
) -> gpui_kit::ListOffset {
    stream.read_with(visual, |stream, _| stream.list.logical_scroll_top())
}

fn edge_point(viewport: Bounds<Pixels>) -> Point<Pixels> {
    point(viewport.center().x, viewport.top() + px(4.))
}

#[gpui_kit::test]
fn issue402_a1_hover_in_body_after_selection_does_not_scroll(cx: &mut TestAppContext) {
    let (stream, mut visual) = open_hover_harness(cx);
    seed_scrollable(&mut visual, &stream);
    detach_tail(&mut visual, &stream);

    let selected = select_body_text(&mut visual, &stream);
    assert!(
        selected,
        "the fixture must land on selectable body text, otherwise A1 passes vacuously"
    );
    assert!(
        !stream.read_with(&visual, |stream, _| stream.following_tail()),
        "the transcript must stay detached while the hover is observed"
    );

    let viewport = viewport(&mut visual);
    let before = scroll_top(&visual, &stream);
    visual.simulate_mouse_move(edge_point(viewport), None, Modifiers::default());
    settle(&mut visual);
    let after = scroll_top(&visual, &stream);

    assert_eq!(
        after.item_ix, before.item_ix,
        "plain hover in the body must not move the viewport item"
    );
    assert_eq!(
        after.offset_in_item, before.offset_in_item,
        "plain hover in the body must not move the viewport pixel offset"
    );
}

#[gpui_kit::test]
fn issue402_a2_drag_to_viewport_edge_still_auto_scrolls(cx: &mut TestAppContext) {
    let (stream, mut visual) = open_hover_harness(cx);
    seed_scrollable(&mut visual, &stream);
    detach_tail(&mut visual, &stream);
    draw(&mut visual);

    let viewport = viewport(&mut visual);
    let layout = body_layout(&body_text());
    let (start, _) = selection_span(&body_text(), &layout, viewport);
    let edge = edge_point(viewport);
    let before = scroll_top(&visual, &stream);

    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(edge, Some(MouseButton::Left), Modifiers::default());
    settle(&mut visual);
    let after = scroll_top(&visual, &stream);
    visual.simulate_mouse_up(edge, MouseButton::Left, Modifiers::default());
    visual.run_until_parked();

    assert!(
        after.item_ix != before.item_ix || after.offset_in_item != before.offset_in_item,
        "dragging a selection to the viewport edge must still auto-scroll, otherwise \
         A1 cannot observe the auto-scroll path at all"
    );
}

#[gpui_kit::test]
fn issue402_a3_body_drag_still_selects_text(cx: &mut TestAppContext) {
    let (stream, mut visual) = open_hover_harness(cx);
    seed_scrollable(&mut visual, &stream);
    detach_tail(&mut visual, &stream);

    let selected = select_body_text(&mut visual, &stream);
    assert!(
        selected,
        "dragging across body text must still produce a selection"
    );
}
