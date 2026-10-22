use super::*;
use gpui_kit::test::{ElementSnapshot, TestWindowExt};
use gpui_kit::{
    Bounds, FollowMode, Hsla, ListOffset, Modifiers, Point, ScrollDelta, ScrollWheelEvent,
    SharedString, Size, TestAppContext, TestSupportExt, TouchPhase, VisualTestContext, point, size,
};
use vega_conversation::history::{
    SkillHistoryActivation, SkillHistoryOrigin, SkillHistorySource, SkillHistoryStatus,
    SkillHistoryVerification,
};
use vega_conversation::types::ThreadStatus;
use vega_theme::Theme;

struct Fixture {
    heights: Vec<f32>,
    trailing_activity: bool,
    mount_list: bool,
    list_size: Option<Size<Pixels>>,
    foreground: Pixels,
    dark: bool,
}

impl Fixture {
    fn messages(heights: impl Into<Vec<f32>>) -> Self {
        Self {
            heights: heights.into(),
            trailing_activity: false,
            mount_list: true,
            list_size: None,
            foreground: px(180.),
            dark: false,
        }
    }
}

struct AnchorHost {
    stream: Entity<ConversationStream>,
    fixture: Fixture,
    requests: Vec<MessageLocationRequested>,
}

impl Render for AnchorHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let heights = self.fixture.heights.clone();
        let mount_list = self.fixture.mount_list;
        let list_size = self.fixture.list_size;
        let foreground = self.fixture.foreground;
        self.stream.update(cx, |stream, cx| {
            stream.set_workspace_width(f32::from(window.viewport_size().width), cx);
            stream.foreground_height = foreground;
            let (rail, preview, _) = stream.render_message_anchor_rail(window, cx);
            div()
                .size_full()
                .relative()
                .bg(theme(cx).colors.bg_base)
                .when(mount_list, |root| {
                    root.child(
                        div()
                            .size_full()
                            .when_some(list_size, |container, size| {
                                container.w(size.width).h(size.height)
                            })
                            .overflow_hidden()
                            .child(
                                list(
                                    stream.list.clone(),
                                    cx.processor(move |stream, index, window, cx| {
                                        div()
                                            .id(SharedString::from(format!("body-{index}")))
                                            .w_full()
                                            .h(px(heights[index]))
                                            .overflow_hidden()
                                            .child(render_rows::render_entry(
                                                &stream.entries[index],
                                                window,
                                                cx,
                                            ))
                                            .test_support()
                                            .into_any_element()
                                    }),
                                )
                                .size_full()
                                .pb(foreground),
                            ),
                    )
                })
                .child(rail)
                .children(preview)
        })
    }
}

fn open_fixture(
    cx: &mut TestAppContext,
    fixture: Fixture,
) -> (Entity<AnchorHost>, &mut VisualTestContext) {
    cx.update(|cx| {
        if cx.try_global::<Theme>().is_none() {
            gpui_kit::init(cx);
        }
        cx.set_global(if fixture.dark {
            Theme::dark()
        } else {
            Theme::light()
        });
    });
    let (host, cx) = cx.add_window_view(move |_, cx: &mut Context<AnchorHost>| {
        let stream = cx.new(|cx| {
            let mut stream = ConversationStream::new(
                Thread {
                    parent_thread_id: None,
                    id: "anchor-thread".into(),
                    backend: TaskBackend::Native,
                    project_id: String::new(),
                    title: "Message navigation".into(),
                    mode: ThreadMode::Ask,
                    permission_mode: PermissionMode::ReadOnly,
                    model: "fixture-model".into(),
                    status: ThreadStatus::Active,
                    pinned: false,
                    unread: false,
                    created_at: 0,
                    updated_at: 0,
                },
                cx,
            );
            stream.set_reading_preview(true, cx);
            let message_count = fixture.heights.len() - usize::from(fixture.trailing_activity);
            let mut entries = (0..message_count)
                .map(|index| HistoryEntry::UserText {
                    seq: index as i64,
                    message_id: message_id(index),
                    content: format!("Message {index}"),
                })
                .collect::<Vec<_>>();
            if fixture.trailing_activity {
                entries.push(HistoryEntry::SkillActivation {
                    seq: message_count as i64,
                    activation: SkillHistoryActivation {
                        run_id: "activity-run".into(),
                        name: "Synthetic activity".into(),
                        source_scope: SkillHistorySource::Project,
                        content_sha256: "0".repeat(64),
                        origin: SkillHistoryOrigin::ExplicitUser,
                        status: SkillHistoryStatus::Loaded,
                        verification: SkillHistoryVerification::Unavailable,
                    },
                });
            }
            stream.apply_history_page(
                HistoryPage {
                    subagents: Vec::new(),
                    entries,
                    older_cursor: None,
                    newer_cursor: None,
                    newest_seq: message_count.checked_sub(1).map(|index| index as i64),
                },
                cx,
            );
            assert_eq!(stream.entries.len(), fixture.heights.len());
            stream.list.set_follow_mode(FollowMode::Normal);
            stream.list.scroll_to(ListOffset::default());
            stream
        });
        cx.observe(&stream, |_, _, cx| cx.notify()).detach();
        cx.subscribe(
            &stream,
            |host, stream, request: &MessageLocationRequested, cx| {
                host.requests.push(request.clone());
                assert!(stream.update(cx, |stream, cx| {
                    stream.reveal_loaded_message(&request.message_id, cx)
                }));
                cx.notify();
            },
        )
        .detach();
        AnchorHost {
            stream,
            fixture,
            requests: Vec::new(),
        }
    });
    cx.simulate_resize(size(px(900.), px(600.)));
    settle(cx);
    move_outside(cx);
    (host, cx)
}

fn message_id(index: usize) -> String {
    format!("durable-message-{index}")
}

fn marker_id(index: usize) -> SharedString {
    format!("message-anchor-{}", message_id(index)).into()
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..8 {
        cx.run_until_parked();
        if cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
            return;
        }
    }
    panic!("queued frame callbacks did not settle");
}

fn move_outside(cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(point(px(850.), px(30.)), None, Modifiers::default());
    settle(cx);
}

fn hover_marker(index: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    let marker = cx.update(|window, _| window.find(marker_id(index)));
    assert!(marker.visible());
    let position = marker.bounds().center();
    cx.simulate_mouse_move(position, None, Modifiers::default());
    settle(cx);
    position
}

fn wheel(delta: f32, cx: &mut VisualTestContext) {
    let position = cx.update(|window, _| point(window.viewport_size().width - px(40.), px(200.)));
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    settle(cx);
}

fn position_at(host: &Entity<AnchorHost>, index: usize, cx: &mut VisualTestContext) {
    host.update(cx, |host, cx| {
        host.stream.update(cx, |stream, cx| {
            stream.list.scroll_to(ListOffset {
                item_ix: index,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
    });
    settle(cx);
}

fn list_geometry(
    host: &Entity<AnchorHost>,
    index: usize,
    cx: &mut VisualTestContext,
) -> (Bounds<Pixels>, Bounds<Pixels>, ListOffset) {
    cx.update(|_, cx| {
        let host = host.read(cx);
        let stream = host.stream.read(cx);
        let mut viewport = stream.list.viewport_bounds();
        viewport.size.height -= stream.foreground_height;
        assert!(viewport.size.width > px(0.) && viewport.size.height > px(0.));
        (
            stream.list.bounds_for_item(index).expect("measured row"),
            viewport,
            stream.list.logical_scroll_top(),
        )
    })
}

fn assert_body_visible(host: &Entity<AnchorHost>, index: usize, cx: &mut VisualTestContext) {
    let (bounds, viewport, _) = list_geometry(host, index, cx);
    assert!(bounds.top() >= viewport.top());
    assert!(bounds.bottom() <= viewport.bottom() + px(0.5));
    let body = cx.update(|window, _| window.find(SharedString::from(format!("body-{index}"))));
    assert!(body.visible());
    assert_eq!(body.bounds(), bounds);
    cx.update(|window, cx| {
        let scale = window.scale_factor();
        let color: Hsla = theme(cx).colors.conversation_bubble.into();
        assert!(window.painted_quads().iter().any(|quad| {
            let center = point(
                px(quad.bounds.center().x.0 / scale),
                px(quad.bounds.center().y.0 / scale),
            );
            bounds.contains(&center)
                && viewport.contains(&center)
                && quad.content_mask.bounds.contains(&quad.bounds.center())
                && quad.background.as_solid() == Some(color)
        }));
    });
}

fn assert_current(
    indices: impl IntoIterator<Item = usize>,
    current: usize,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        let colors = theme(cx).colors;
        let scale = window.scale_factor();
        let quads = window.painted_quads();
        for index in indices {
            let marker = window.find(marker_id(index));
            assert!(marker.visible());
            let tracks = quads
                .iter()
                .filter(|quad| {
                    let center = point(
                        px(quad.bounds.center().x.0 / scale),
                        px(quad.bounds.center().y.0 / scale),
                    );
                    marker.bounds().contains(&center)
                        && quad.content_mask.bounds.contains(&quad.bounds.center())
                        && (quad.bounds.size.height.0 / scale - Layout::MESSAGE_ANCHOR_TRACK_WIDTH)
                            .abs()
                            < 0.01
                })
                .collect::<Vec<_>>();
            assert_eq!(tracks.len(), 1, "painted marker {index}");
            let (width, color) = if index == current {
                (Layout::MESSAGE_ANCHOR_ACTIVE_SIZE, colors.text_secondary)
            } else {
                (
                    Layout::CONVERSATION_MARKER_WIDTH,
                    colors.conversation_marker,
                )
            };
            assert!(
                (tracks[0].bounds.size.width.0 / scale - width).abs() < 0.01,
                "marker {index} must project current message {current}"
            );
            assert_eq!(tracks[0].background.as_solid(), Some(color.into()));
        }
    });
}

fn assert_preview(expected: Option<usize>, cx: &mut VisualTestContext) -> Option<ElementSnapshot> {
    cx.update(|window, cx| {
        let preview = window.try_find("message-anchor-preview");
        let scale = window.scale_factor();
        let color: Hsla = theme(cx).colors.bg_elevated.into();
        let panels = window
            .painted_quads()
            .into_iter()
            .filter(|quad| {
                quad.background.as_solid() == Some(color)
                    && quad.content_mask.bounds.contains(&quad.bounds.center())
                    && (quad.bounds.size.width.0 / scale - Layout::MESSAGE_ANCHOR_PREVIEW_WIDTH)
                        .abs()
                        < 0.01
                    && (quad.bounds.left().0 / scale
                        - (Layout::MESSAGE_ANCHOR_LEFT_INSET
                            + Layout::CONVERSATION_NAVIGATION_GUTTER
                            + 4.))
                        .abs()
                        < 0.01
            })
            .collect::<Vec<_>>();
        if let Some(index) = expected {
            let preview = preview.as_ref().expect("visible message preview");
            assert!(preview.visible());
            assert_eq!(
                preview.label(),
                Some(format!("用户消息 · Message {index}").as_str())
            );
            assert_eq!(panels.len(), 1);
            assert!(
                (panels[0].bounds.size.height.0 / scale - f32::from(preview.bounds().size.height))
                    .abs()
                    < 0.01
            );
            assert!(preview.bounds().size.height > px(0.));
            assert!(preview.bounds().size.height <= px(Layout::MESSAGE_ANCHOR_PREVIEW_HEIGHT));
        } else {
            assert!(
                preview.is_none(),
                "preview must be removed, not merely unfocused"
            );
            assert!(panels.is_empty(), "preview panel must stop painting");
        }
        preview
    })
}

fn assert_requests(host: &Entity<AnchorHost>, expected: &[usize], cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        assert_eq!(
            host.read(cx).requests,
            expected
                .iter()
                .map(|index| MessageLocationRequested {
                    thread_id: "anchor-thread".into(),
                    message_id: message_id(*index),
                    restore_anchor: None,
                })
                .collect::<Vec<_>>()
        );
    });
}

fn assert_focus(
    host: &Entity<AnchorHost>,
    focused: bool,
    keyboard: bool,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        assert_eq!(
            host.read(cx)
                .stream
                .read(cx)
                .message_anchor_focus
                .is_focused(window),
            focused
        );
        assert_eq!(window.find("message-anchor-rail").focused(), Some(focused));
        assert_eq!(window.last_input_was_keyboard(), keyboard);
    });
}

#[gpui_kit::test]
fn observed_rail_hover_and_loaded_location_are_live(cx: &mut TestAppContext) {
    let mut fixture = Fixture::messages([700., 320., 300., 80.]);
    fixture.dark = true;
    let (host, cx) = open_fixture(cx, fixture);
    assert_current(0..4, 0, cx);
    assert_preview(None, cx);
    let position = hover_marker(1, cx);
    assert_preview(Some(1), cx);
    cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
    settle(cx);
    assert_focus(&host, true, false, cx);
    assert_requests(&host, &[], cx);
    cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
    settle(cx);
    assert_requests(&host, &[1], cx);
    assert_body_visible(&host, 1, cx);
    assert_preview(Some(1), cx);
    assert_current(0..4, 1, cx);
    assert_preview(Some(1), cx);
}

#[gpui_kit::test]
fn short_last_message_click_reveals_stable_target_and_current_marker(cx: &mut TestAppContext) {
    let (host, cx) = open_fixture(cx, Fixture::messages([700., 320., 300., 80.]));
    let position = hover_marker(3, cx);
    cx.simulate_click(position, Modifiers::default());
    settle(cx);
    assert_requests(&host, &[3], cx);
    assert_body_visible(&host, 3, cx);
    let (last, viewport, top) = list_geometry(&host, 3, cx);
    assert_eq!(last.size.height, px(80.));
    assert!(top.item_ix < 3);
    assert!((f32::from(last.bottom() - viewport.bottom())).abs() < 0.5);
    move_outside(cx);
    assert_focus(&host, true, false, cx);
    assert_preview(None, cx);
    assert_current(0..4, 3, cx);
    wheel(400., cx);
    let (last, viewport, top) = list_geometry(&host, 3, cx);
    assert_eq!(top.item_ix, 0);
    assert!(last.top() > viewport.bottom() + px(180.));
    assert_current(0..4, 0, cx);
    assert_preview(None, cx);
    assert_requests(&host, &[3], cx);
}

#[gpui_kit::test]
fn manual_bottom_with_unknown_heights_selects_last_without_tail_follow(cx: &mut TestAppContext) {
    let mut heights = vec![260.; 128];
    heights[127] = 80.;
    let (host, cx) = open_fixture(cx, Fixture::messages(heights));
    position_at(&host, 124, cx);
    let (last, viewport, _) = list_geometry(&host, 127, cx);
    assert!(last.top() > viewport.bottom());
    assert_current(119..128, 124, cx);
    wheel(-2000., cx);
    cx.update(|_, cx| {
        let stream = host.read(cx).stream.read(cx);
        assert_eq!(stream.list.is_scrolled_to_end(), None);
        assert!(!stream.list.is_following_tail());
    });
    let (last, viewport, top) = list_geometry(&host, 127, cx);
    assert!(top.item_ix < 127);
    assert!((f32::from(last.bottom() - viewport.bottom())).abs() < 0.5);
    assert_body_visible(&host, 127, cx);
    assert_current(119..128, 127, cx);
    wheel(300., cx);
    let (_, _, top) = list_geometry(&host, 127, cx);
    assert_eq!(top.item_ix, 124);
    assert_current(119..128, 124, cx);
    assert_preview(None, cx);
    assert_requests(&host, &[], cx);
}

#[gpui_kit::test]
fn cached_tail_with_unknown_middle_keeps_top_marker_current(cx: &mut TestAppContext) {
    let mut heights = vec![260.; 128];
    let tail_index = heights.len() - 1;
    let gap_index = tail_index - 1;
    heights[tail_index] = 80.;
    let (host, cx) = open_fixture(cx, Fixture::messages(heights));
    position_at(&host, tail_index, cx);
    assert_body_visible(&host, 127, cx);
    cx.update(|_, cx| {
        let list = &host.read(cx).stream.read(cx).list;
        assert!(list.bounds_for_item(gap_index).is_some());
    });
    host.update(cx, |host, cx| {
        host.stream.update(cx, |stream, cx| {
            stream.list.scroll_to(ListOffset::default());
            stream.list.splice(5..tail_index, tail_index - 5);
            cx.notify();
        });
    });
    settle(cx);
    let (tail, viewport, top) = list_geometry(&host, tail_index, cx);
    assert_eq!(top.item_ix, 0);
    assert_eq!(top.offset_in_item, px(0.));
    assert!(top.item_ix < gap_index && gap_index < tail_index);
    assert_ne!(message_id(top.item_ix), message_id(tail_index));
    assert_eq!(tail.size, size(px(900.), px(80.)));
    assert_eq!(tail.top(), viewport.top() + px(1300.));
    assert!(tail.top() > viewport.bottom());
    cx.update(|window, cx| {
        let list = &host.read(cx).stream.read(cx).list;
        assert_eq!(list.item_count(), 128);
        assert!(list.bounds_for_item(5).is_none());
        assert!(list.bounds_for_item(gap_index).is_none());
        assert_eq!(list.is_scrolled_to_end(), None);
        assert!(!list.is_following_tail());
        assert!(window.try_find(marker_id(127)).is_none());
    });
    assert_focus(&host, false, false, cx);
    assert_current(0..9, 0, cx);
    assert_preview(None, cx);
    assert_requests(&host, &[], cx);
}

#[gpui_kit::test]
fn visible_last_anchor_does_not_hide_non_anchor_tail(cx: &mut TestAppContext) {
    let mut fixture = Fixture::messages([700., 300., 80., 700.]);
    fixture.trailing_activity = true;
    let (host, cx) = open_fixture(cx, fixture);
    position_at(&host, 1, cx);
    assert_body_visible(&host, 2, cx);
    let (tail, viewport, top) = list_geometry(&host, 3, cx);
    assert_eq!(top.item_ix, 1);
    assert!(tail.bottom() > viewport.bottom());
    assert_current(0..3, 1, cx);
    wheel(-2000., cx);
    let (tail, viewport, top) = list_geometry(&host, 3, cx);
    assert_eq!(top.item_ix, 3);
    assert!((f32::from(tail.bottom() - viewport.bottom())).abs() < 0.5);
    assert_current(0..3, 2, cx);
    assert_preview(None, cx);
    assert_requests(&host, &[], cx);

    let mut fixture = Fixture::messages([700., 300., 80., 0.]);
    fixture.trailing_activity = true;
    let (host, cx) = open_fixture(cx, fixture);
    position_at(&host, 3, cx);
    let (tail, viewport, top) = list_geometry(&host, 3, cx);
    assert_eq!(tail.size, size(px(900.), px(0.)));
    assert!((f32::from(tail.bottom() - viewport.bottom())).abs() < 0.5);
    assert_eq!(top.item_ix, 0);
    cx.update(|_, cx| {
        assert_eq!(
            host.read(cx).stream.read(cx).list.is_scrolled_to_end(),
            Some(true)
        );
    });
    assert_body_visible(&host, 2, cx);
    assert_focus(&host, false, false, cx);
    assert_current(0..3, 2, cx);
    assert_preview(None, cx);
    assert_requests(&host, &[], cx);
}

#[gpui_kit::test]
fn mouse_preview_closes_after_click_and_preview_hover_without_blurring(cx: &mut TestAppContext) {
    let (host, cx) = open_fixture(cx, Fixture::messages([700., 320., 300., 80.]));
    hover_marker(1, cx);
    let preview = assert_preview(Some(1), cx).unwrap();
    cx.simulate_mouse_move(preview.bounds().center(), None, Modifiers::default());
    settle(cx);
    assert_preview(Some(1), cx);
    move_outside(cx);
    assert_preview(None, cx);
    let position = hover_marker(2, cx);
    cx.simulate_click(position, Modifiers::default());
    settle(cx);
    assert_requests(&host, &[2], cx);
    move_outside(cx);
    assert_focus(&host, true, false, cx);
    assert_preview(None, cx);
    hover_marker(0, cx);
    let preview = assert_preview(Some(0), cx).unwrap();
    cx.simulate_mouse_move(preview.bounds().center(), None, Modifiers::default());
    settle(cx);
    assert_preview(Some(0), cx);
    move_outside(cx);
    assert_preview(None, cx);
    assert_focus(&host, true, false, cx);
    assert_requests(&host, &[2], cx);
}

#[gpui_kit::test]
fn keyboard_navigation_and_mouse_to_keyboard_keep_preview_and_location_accessible(
    cx: &mut TestAppContext,
) {
    let (host, cx) = open_fixture(cx, Fixture::messages([700., 320., 300., 80.]));
    cx.simulate_keystrokes("shift-tab");
    cx.update(|window, cx| window.focus_prev(cx));
    settle(cx);
    assert_focus(&host, true, true, cx);
    assert_preview(Some(0), cx);
    cx.simulate_keystrokes("up down enter");
    settle(cx);
    assert_requests(&host, &[1], cx);
    assert_body_visible(&host, 1, cx);
    assert_preview(Some(1), cx);
    cx.simulate_keystrokes("down space");
    settle(cx);
    assert_requests(&host, &[1, 2], cx);
    assert_body_visible(&host, 2, cx);
    assert_preview(Some(2), cx);
    cx.simulate_keystrokes("up");
    settle(cx);
    assert_preview(Some(1), cx);
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert_preview(None, cx);
    assert_focus(&host, false, true, cx);
    let position = hover_marker(2, cx);
    cx.simulate_click(position, Modifiers::default());
    settle(cx);
    assert_requests(&host, &[1, 2, 2], cx);
    move_outside(cx);
    assert_preview(None, cx);
    assert_focus(&host, true, false, cx);
    cx.simulate_keystrokes("down down");
    settle(cx);
    assert_focus(&host, true, true, cx);
    assert_preview(Some(3), cx);
    cx.simulate_keystrokes("space escape");
    settle(cx);
    assert_requests(&host, &[1, 2, 2, 3], cx);
    assert_body_visible(&host, 3, cx);
    assert_preview(None, cx);
    assert_current(0..4, 3, cx);
}

#[gpui_kit::test]
fn long_rail_follows_scroll_after_mouse_click_and_remains_keyboard_accessible(
    cx: &mut TestAppContext,
) {
    let mut heights = vec![260.; 16];
    heights[15] = 80.;
    let (host, cx) = open_fixture(cx, Fixture::messages(heights));
    for index in [4, 8, 12] {
        position_at(&host, index, cx);
    }
    cx.update(|_, cx| {
        assert_eq!(
            host.read(cx).stream.read(cx).list.is_scrolled_to_end(),
            Some(false)
        );
    });
    let position = hover_marker(12, cx);
    cx.simulate_click(position, Modifiers::default());
    settle(cx);
    assert_requests(&host, &[12], cx);
    assert_body_visible(&host, 12, cx);
    move_outside(cx);
    assert_focus(&host, true, false, cx);
    assert_current(7..16, 12, cx);
    assert_preview(None, cx);
    wheel(2000., cx);
    let (_, _, top) = list_geometry(&host, 4, cx);
    assert_eq!(top.item_ix, 4);
    assert_eq!(top.offset_in_item, px(80.));
    assert_focus(&host, true, false, cx);
    assert_current(0..9, 4, cx);
    assert!(cx.update(|window, _| window.try_find(marker_id(12)).is_none()));
    assert_preview(None, cx);
    assert_requests(&host, &[12], cx);
    cx.simulate_keystrokes("up");
    settle(cx);
    assert_focus(&host, true, true, cx);
    assert_preview(Some(11), cx);
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_requests(&host, &[12, 11], cx);
    assert_body_visible(&host, 11, cx);
    assert_preview(Some(11), cx);
    cx.simulate_keystrokes("down space escape");
    settle(cx);
    assert_requests(&host, &[12, 11, 12], cx);
    assert_body_visible(&host, 12, cx);
    assert_focus(&host, false, true, cx);
    assert_preview(None, cx);
    assert_current(7..16, 12, cx);
}

#[gpui_kit::test]
fn empty_single_and_short_conversations_hide_navigation(cx: &mut TestAppContext) {
    for heights in [vec![], vec![900.], vec![80., 80.]] {
        let (_, cx) = open_fixture(cx, Fixture::messages(heights));
        assert!(cx.update(|window, _| window.try_find("message-anchor-rail").is_none()));
        assert_preview(None, cx);
    }
}

#[gpui_kit::test]
fn missing_or_unusable_list_geometry_does_not_expose_navigation(cx: &mut TestAppContext) {
    for (mount_list, list_size, foreground) in [
        (false, None, px(180.)),
        (true, Some(size(px(0.), px(600.))), px(180.)),
        (true, Some(size(px(900.), px(0.))), px(0.)),
        (true, None, px(600.)),
    ] {
        let mut fixture = Fixture::messages(vec![260.; 128]);
        fixture.mount_list = mount_list;
        fixture.list_size = list_size;
        fixture.foreground = foreground;
        let (host, cx) = open_fixture(cx, fixture);
        cx.update(|_, cx| {
            let stream = host.read(cx).stream.read(cx);
            let bounds = stream.list.viewport_bounds();
            assert!(bounds.size.width <= px(0.) || bounds.size.height <= stream.foreground_height);
        });
        assert!(cx.update(|window, _| window.try_find("message-anchor-rail").is_none()));
        assert_preview(None, cx);
        host.update(cx, |host, cx| {
            host.fixture.mount_list = true;
            host.fixture.list_size = None;
            host.fixture.foreground = px(180.);
            cx.notify();
        });
        settle(cx);
        let (_, viewport, top) = list_geometry(&host, 0, cx);
        assert_eq!(viewport.size, size(px(900.), px(420.)));
        assert_eq!(top.item_ix, 0);
        assert_eq!(top.offset_in_item, px(0.));
        assert_focus(&host, false, false, cx);
        assert_current(0..9, 0, cx);
        assert_preview(None, cx);
        assert_requests(&host, &[], cx);
    }
}

#[gpui_kit::test]
fn growing_tail_resize_and_foreground_recompute_current_geometry(cx: &mut TestAppContext) {
    let (host, cx) = open_fixture(cx, Fixture::messages([700., 320., 300., 80.]));
    position_at(&host, 3, cx);
    assert_current(0..4, 3, cx);
    host.update(cx, |host, cx| {
        host.fixture.heights[3] = 240.;
        host.stream.read(cx).list.remeasure_items(3..4);
        cx.notify();
    });
    settle(cx);
    let (last, viewport, top) = list_geometry(&host, 3, cx);
    assert_eq!(last.size.height, px(240.));
    assert!(last.bottom() > viewport.bottom());
    assert_eq!(top.item_ix, 1);
    assert_current(0..4, 1, cx);
    wheel(-2000., cx);
    assert_current(0..4, 3, cx);
    cx.simulate_resize(size(px(760.), px(480.)));
    settle(cx);
    let (last, viewport, top) = list_geometry(&host, 3, cx);
    assert_eq!(viewport.size, size(px(760.), px(300.)));
    assert!(last.bottom() > viewport.bottom());
    assert_eq!(top.item_ix, 2);
    assert_current(0..4, 2, cx);
    wheel(-2000., cx);
    assert_current(0..4, 3, cx);
    cx.simulate_resize(size(px(900.), px(600.)));
    settle(cx);
    assert_current(0..4, 3, cx);
    host.update(cx, |host, cx| {
        host.fixture.foreground = px(260.);
        cx.notify();
    });
    settle(cx);
    let (last, viewport, top) = list_geometry(&host, 3, cx);
    assert_eq!(viewport.size.height, px(340.));
    assert!(last.bottom() > viewport.bottom());
    assert_eq!(top.item_ix, 2);
    assert_current(0..4, 2, cx);
    wheel(-2000., cx);
    assert_current(0..4, 3, cx);
    host.update(cx, |host, cx| {
        host.fixture.foreground = px(100.);
        cx.notify();
    });
    settle(cx);
    let (last, viewport, _) = list_geometry(&host, 3, cx);
    assert_eq!(viewport.size.height, px(500.));
    assert!((f32::from(last.bottom() - viewport.bottom())).abs() < 0.5);
    assert_current(0..4, 3, cx);
    assert_preview(None, cx);
    assert_requests(&host, &[], cx);
    cx.update(|_, cx| assert!(!host.read(cx).stream.read(cx).list.is_following_tail()));
}
