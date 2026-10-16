use super::*;
use gpui_kit::{
    Bounds, Modifiers, MouseButton, Pixels, Point, TextLayout, VisualTestContext, point, px, size,
};
use std::time::Duration;

const BODY_LINES: usize = 90;
const LINE_HEIGHT_SLACK: Pixels = px(20.);

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
    let mut visual = VisualTestContext::from_window(window.into(), cx);
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

fn body_text() -> String {
    (0..BODY_LINES)
        .map(|line| format!("issue402 body line {line:03}"))
        .collect::<Vec<_>>()
        .join("\n")
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

fn visible_slice(run: Bounds<Pixels>, viewport: Bounds<Pixels>) -> Pixels {
    run.intersect(&viewport).size.height
}

fn widest_visible_run(viewport: Bounds<Pixels>) -> Option<(String, TextLayout, Bounds<Pixels>)> {
    selection::SELECTION_RUN_LAYOUTS.with_borrow(|runs| {
        runs.iter()
            .filter(|(text, _, bounds)| {
                !text.trim().is_empty()
                    && bounds.center().x > viewport.left()
                    && bounds.center().x < viewport.right()
            })
            .max_by(|left, right| {
                visible_slice(left.2, viewport)
                    .partial_cmp(&visible_slice(right.2, viewport))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(text, layout, bounds)| (text.clone(), layout.clone(), *bounds))
    })
}

fn selectable_run(
    visual: &mut VisualTestContext,
    stream: &Entity<ConversationStream>,
    viewport: Bounds<Pixels>,
) -> (String, TextLayout, Bounds<Pixels>) {
    for _ in 0..12 {
        if let Some((text, layout, bounds)) = widest_visible_run(viewport)
            && visible_slice(bounds, viewport) >= LINE_HEIGHT_SLACK
        {
            return (text, layout, bounds);
        }
        stream.update(visual, |stream, cx| {
            stream.list.scroll_by(px(-24.));
            cx.notify();
        });
        visual.run_until_parked();
        draw(visual);
    }
    panic!("no message body line stayed visible inside the transcript viewport");
}

fn run_edges(
    layout: &TextLayout,
    run: Bounds<Pixels>,
    text: &str,
    viewport: Bounds<Pixels>,
) -> (Point<Pixels>, Point<Pixels>) {
    let y = run
        .center()
        .y
        .clamp(viewport.top() + px(2.), viewport.bottom() - px(2.))
        .clamp(run.top() + px(1.), run.bottom() - px(1.));
    let last = text.trim_end().len().saturating_sub(1);
    let start = layout
        .position_for_index(0)
        .expect("run start geometry")
        .x
        .clamp(viewport.left() + px(2.), viewport.right() - px(2.))
        + px(0.5);
    let end = layout
        .position_for_index(last)
        .expect("run end geometry")
        .x
        .clamp(viewport.left() + px(2.), viewport.right() - px(2.))
        + px(2.);
    (point(start, y), point(end, y))
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
    point(viewport.center().x, viewport.bottom() - px(4.))
}

fn select_body_text(visual: &mut VisualTestContext, stream: &Entity<ConversationStream>) -> String {
    draw(visual);
    let viewport = viewport(visual);
    let (text, layout, run) = selectable_run(visual, stream, viewport);
    let (start, end) = run_edges(&layout, run, &text, viewport);
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    visual.run_until_parked();
    visual.update(gpui_kit::base::TextSelection::selected_text)
}

#[gpui_kit::test]
fn issue402_a1_hover_in_body_after_selection_does_not_scroll(cx: &mut TestAppContext) {
    let (stream, mut visual) = open_hover_harness(cx);
    seed_scrollable(&mut visual, &stream);
    detach_tail(&mut visual, &stream);

    let selected = select_body_text(&mut visual, &stream);
    assert!(
        !selected.trim().is_empty(),
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
    let (text, layout, run) = selectable_run(&mut visual, &stream, viewport);
    let (start, _) = run_edges(&layout, run, &text, viewport);
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
        !selected.trim().is_empty(),
        "dragging across body text must still produce a selection"
    );
}
