use super::super::turn_placement::TurnList;
use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, FollowMode, ListOffset, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext,
};
use vega_conversation::types::ThreadStatus;

struct ArrowFixture {
    stream: Entity<ConversationStream>,
}

impl Render for ArrowFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.stream.update(cx, |stream, cx| {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    TurnList::new(
                        stream.list.clone(),
                        stream.turn_placement.clone(),
                        px(20.),
                        8,
                        |_, _, _| div().h(px(80.)).w_full().into_any_element(),
                    )
                    .h(px(300.))
                    .w_full(),
                )
                .child(stream.render_resume_tail(cx))
        })
    }
}

fn fixture(cx: &mut TestAppContext) -> (Entity<ConversationStream>, &mut VisualTestContext) {
    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    let (view, cx) = cx.add_window_view(|_, cx| {
        let thread = Thread {
            id: "arrow-fixture".into(),
            backend: TaskBackend::Native,
            project_id: String::new(),
            title: "Arrow regression".into(),
            mode: ThreadMode::Execute,
            permission_mode: PermissionMode::Confirm,
            permission_revision: 0,
            draft_authority_epoch: None,
            full_access_grant: None,
            default_permission: None,
            model: "fixture-model".into(),
            status: ThreadStatus::Active,
            pinned: false,
            unread: false,
            created_at: 0,
            updated_at: 0,
            reasoning_preferences: None,
            parent_thread_id: None,
            forked_from_thread_id: None,
            forked_from_message_id: None,
        };
        let stream = cx.new(|cx| ConversationStream::new(thread, cx));
        stream.update(cx, |stream, _| {
            stream.list.reset_with_uniform_height(8, px(80.));
            stream.list.set_follow_mode(FollowMode::Tail);
        });
        ArrowFixture { stream }
    });
    let stream = view.read_with(cx, |view, _| view.stream.clone());
    frame(cx);
    (stream, cx)
}

fn frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.render_frame(cx));
}

fn arrow_visible(cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds("resume-tail").is_some()
}

fn scroll(cx: &mut VisualTestContext, delta: f32) {
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(100.), px(100.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
        modifiers: Default::default(),
        touch_phase: TouchPhase::Moved,
    });
    frame(cx);
}

fn begin(stream: &Entity<ConversationStream>, cx: &mut VisualTestContext) {
    stream.update(cx, |stream, _| {
        stream.turn_placement.borrow_mut().begin(7, &stream.list);
    });
}

#[gpui_kit::test]
fn automatic_placement_hides_arrow_without_changing_anchor_or_spacer(cx: &mut TestAppContext) {
    let (stream, cx) = fixture(cx);
    assert!(!arrow_visible(cx));
    begin(&stream, cx);
    stream.update(cx, |stream, cx| {
        let top = stream.list.logical_scroll_top();
        assert_eq!(top.item_ix, 7);
        assert_eq!(top.offset_in_item, px(0.));
        let _ = stream.render_resume_tail(cx);
        let after = stream.list.logical_scroll_top();
        assert_eq!(after.item_ix, top.item_ix);
        assert_eq!(after.offset_in_item, top.offset_in_item);
        assert!(!stream.following_tail());
    });
    frame(cx);
    assert!(!arrow_visible(cx));
    let anchor = stream.read_with(cx, |stream, _| {
        assert!(!stream.following_tail());
        let row = stream.list.bounds_for_item(7).unwrap();
        assert_eq!(row.top(), stream.list.viewport_bounds().top());
        assert!(row.bottom() + px(20.) < stream.list.viewport_bounds().bottom());
        stream.list.logical_scroll_top()
    });
    frame(cx);
    assert!(!arrow_visible(cx));
    stream.read_with(cx, |stream, _| {
        let after = stream.list.logical_scroll_top();
        assert_eq!(after.item_ix, anchor.item_ix);
        assert_eq!(after.offset_in_item, anchor.offset_in_item);
        assert!(stream.turn_placement.borrow().automatically_placed());
    });
}

#[gpui_kit::test]
fn manual_scroll_restores_arrow_and_existing_return_to_tail(cx: &mut TestAppContext) {
    let (stream, cx) = fixture(cx);
    begin(&stream, cx);
    frame(cx);
    assert!(!arrow_visible(cx));
    scroll(cx, 50.);
    assert!(arrow_visible(cx));
    stream.read_with(cx, |stream, _| {
        assert!(!stream.turn_placement.borrow().automatically_placed());
        assert!(!stream.following_tail());
    });
    for step in 0..20 {
        scroll(cx, -25.);
        stream.read_with(cx, |stream, _| {
            eprintln!(
                "down step {step}: top={:?} tail={:?} viewport={:?} following={} active={}",
                stream.list.logical_scroll_top(),
                stream.list.bounds_for_item(7),
                stream.list.viewport_bounds(),
                stream.following_tail(),
                stream.turn_placement.borrow().active()
            );
        });
        if stream.read_with(cx, |stream, _| stream.following_tail()) {
            break;
        }
    }
    frame(cx);
    assert!(!arrow_visible(cx));
    stream.read_with(cx, |stream, _| {
        assert!(stream.following_tail());
        assert!(!stream.turn_placement.borrow().active());
    });
}

#[gpui_kit::test]
fn history_navigation_keeps_focus_and_return_action_and_new_turn_hides_arrow(
    cx: &mut TestAppContext,
) {
    let (stream, cx) = fixture(cx);
    stream.update(cx, |stream, _| {
        stream.list.set_follow_mode(FollowMode::Normal);
        stream.list.scroll_to(ListOffset {
            item_ix: 2,
            offset_in_item: px(0.),
        });
    });
    frame(cx);
    assert!(arrow_visible(cx));
    let focus = stream.read_with(cx, |stream, _| stream.resume_tail_focus.clone());
    cx.update(|window, cx| window.focus(&focus, cx));
    frame(cx);
    cx.update(|window, _| assert!(focus.is_focused(window)));
    begin(&stream, cx);
    frame(cx);
    assert!(!arrow_visible(cx));
    scroll(cx, 50.);
    assert!(arrow_visible(cx));
    let button = cx.debug_bounds("resume-tail").unwrap();
    cx.simulate_click(button.center(), Default::default());
    frame(cx);
    assert!(!arrow_visible(cx));
    stream.read_with(cx, |stream, _| assert!(stream.following_tail()));
}
