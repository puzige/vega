use super::r64_popup_deferred::{PROJECT_BINDING, bounds, click, install_utility_globals};
use super::*;
use gpui_kit::{
    Modifiers, ScrollDelta, ScrollWheelEvent, VisualTestContext, linear_color_stop,
    linear_gradient, point, size,
};
use vega_conversation::types::PendingMessage;

fn seed(stream: &Entity<ConversationStream>, count: i64, cx: &mut TestAppContext) {
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                (1..=count)
                    .map(|index| hydration_user(index, "A request in the conversation."))
                    .collect(),
                None,
            ),
            cx,
        );
    });
    cx.run_until_parked();
}

fn assert_tail_clear(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) {
    let foreground = bounds(window, "conversation-foreground", cx);
    stream.read_with(cx, |stream, _| {
        assert!(stream.following_tail());
        assert_eq!(stream.list.item_count(), stream.entries.len());
        let last = stream
            .list
            .bounds_for_item(stream.entries.len() - 1)
            .expect("last business entry is laid out");
        assert!(last.bottom() <= foreground.top() + px(0.5));
        assert!(last.top() >= stream.list.viewport_bounds().top());
    });
}

#[gpui_kit::test]
async fn issue403_transcript_extends_beneath_themed_foreground(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue403-overlap");
    seed(&stream, 60, cx);
    for backend in [TaskBackend::Native, TaskBackend::Codex] {
        stream.update(cx, |stream, cx| {
            stream.thread.backend = backend;
            cx.notify();
        });
        for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
            let colors = palette.colors;
            cx.update(|cx| {
                cx.set_global(palette);
                cx.refresh_windows();
            });
            assert_tail_clear(window, &stream, cx);
            stream.update(cx, |stream, cx| {
                stream.list.scroll_by(px(-300.));
                cx.notify();
            });
            let viewport = bounds(window, "conversation-message-list", cx);
            let shell = bounds(window, "composer-shell", cx);
            let surface = bounds(window, "conversation-foreground-surface", cx);
            let fade = bounds(window, "conversation-bottom-fade", cx);
            assert_eq!(viewport.center().x, shell.center().x);
            assert!(viewport.bottom() >= shell.bottom());
            assert_eq!(fade.size.height, px(48.));
            assert_eq!(fade.bottom(), surface.top());
            assert!(fade.top() < shell.top());
            assert!(stream.read_with(cx, |stream, _| {
                (0..stream.entries.len()).any(|index| {
                    stream
                        .list
                        .bounds_for_item(index)
                        .is_some_and(|item| item.size.width > px(0.) && item.intersects(&shell))
                })
            }));
            let (quads, scale) = window
                .update(cx, |_, window, _| {
                    (window.painted_quads(), window.scale_factor())
                })
                .expect("underlay window");
            let gradient = linear_gradient(
                180.,
                linear_color_stop(colors.bg_base.opacity(0.), 0.),
                linear_color_stop(colors.bg_base, 1.),
            );
            let fade_index = quads
                .iter()
                .position(|quad| quad.bounds == fade.scale(scale) && quad.background == gradient)
                .expect("theme gradient is painted over the extended viewport");
            let surface_index = quads
                .iter()
                .position(|quad| {
                    quad.bounds == surface.scale(scale)
                        && quad.background.as_solid() == Some(colors.bg_base.into())
                })
                .expect("foreground sides and bottom have an opaque reading surface");
            assert!(surface_index > fade_index);
            assert!(quads.iter().skip(surface_index + 1).any(|quad| {
                quad.bounds == shell.scale(scale)
                    && quad.background.as_solid() == Some(colors.bg_elevated.into())
            }));
            stream.update(cx, |stream, cx| stream.resume_tail(cx));
        }
    }
}

#[gpui_kit::test]
async fn issue403_tail_clearance_tracks_composer_and_required_states(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue403-clearance");
    seed(&stream, 60, cx);
    assert_tail_clear(window, &stream, cx);
    let initial = bounds(window, "conversation-foreground", cx);
    stream.update(cx, |stream, cx| {
        stream.input.update(cx, |input, cx| {
            input.set_text("one\ntwo\nthree\nfour\nfive\nsix", cx)
        });
    });
    assert_tail_clear(window, &stream, cx);
    let multiline = bounds(window, "conversation-foreground", cx);
    assert!(multiline.size.height > initial.size.height);

    stream.update(cx, |stream, cx| {
        let image =
            ImageAttachment::from_bytes(include_bytes!("fixtures/screenshot-tiny.png").to_vec())
                .expect("owned image fixture");
        let preview = attachments::ImagePreview::new(image, "image.png".into(), cx);
        stream.attachments.push((1, preview));
        stream.attachment_error = Some("Unable to add another image.");
        stream.controller_error = Some("The request needs attention.".into());
        stream.privacy.opened = true;
        stream.privacy.error = Some("Review could not finish. Try again.".into());
        stream.apply_pending_queue(
            PendingQueueProjection {
                messages: vec![PendingMessage {
                    id: "queued".into(),
                    thread_id: stream.thread.id.clone(),
                    generation: "run".into(),
                    behavior: PendingMessageBehavior::Queue,
                    content: "A queued request.".into(),
                    images: Vec::new(),
                }],
                ..Default::default()
            },
            cx,
        );
    });
    assert_tail_clear(window, &stream, cx);
    let expanded = bounds(window, "conversation-foreground", cx);
    assert!(expanded.size.height > multiline.size.height);
    let error = bounds(window, "conversation-controller-error", cx);
    let attachments = bounds(window, "composer-attachments", cx);
    let queue = bounds(window, "pending-message-rail", cx);
    for state in [error, attachments, queue] {
        assert!(state.top() >= expanded.top() && state.bottom() <= expanded.bottom());
    }

    stream.update(cx, |stream, cx| {
        stream.input.update(cx, |input, cx| input.clear(cx));
        stream.attachments.clear();
        stream.attachment_error = None;
        stream.controller_error = None;
        stream.privacy.opened = false;
        stream.apply_pending_queue(PendingQueueProjection::default(), cx);
    });
    assert_tail_clear(window, &stream, cx);
    assert_eq!(bounds(window, "conversation-foreground", cx), initial);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(594.), px(554.)));
    visual.run_until_parked();
    assert_tail_clear(window, &stream, cx);
}

#[gpui_kit::test]
async fn issue403_footer_resize_preserves_history_anchor_and_resume(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue403-history");
    seed(&stream, 60, cx);
    stream.update(cx, |stream, cx| {
        stream.list.scroll_by(px(-600.));
        cx.notify();
    });
    cx.run_until_parked();
    let anchor = stream.read_with(cx, |stream, _| stream.scroll_anchor_snapshot());
    let viewport = bounds(window, "conversation-message-list", cx);
    assert!(!anchor.following_tail);
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("one\ntwo\nthree\nfour", cx));
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "live-tail".into(),
                seq: 61,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live-tail".into(),
                delta: "New output while reading history. ".repeat(20),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let after = stream.read_with(cx, |stream, _| stream.scroll_anchor_snapshot());
    assert_eq!(after.identity, anchor.identity);
    assert_eq!(after.message_id, anchor.message_id);
    assert!((after.offset_in_item_px - anchor.offset_in_item_px).abs() < 1.);
    assert!(!after.following_tail);
    assert_eq!(bounds(window, "conversation-message-list", cx), viewport);
    click(window, "scroll-to-bottom", cx);
    assert_tail_clear(window, &stream, cx);
}

#[gpui_kit::test]
async fn issue403_fade_passes_scroll_and_foreground_blocks_hidden_tools(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue403-hit-testing");
    let cards = stream.update(cx, |stream, cx| {
        let cards = (0..80)
            .map(|index| {
                cx.new(|_| ToolCard::proposed(&bash_call(&format!("tool-{index}"), "pwd")))
            })
            .collect::<Vec<_>>();
        stream.entries = cards
            .iter()
            .cloned()
            .map(|card| StreamEntry::Tool { card })
            .collect();
        stream.list_prepend(cards.len());
        cx.notify();
        cards
    });
    cx.run_until_parked();
    let fade = bounds(window, "conversation-bottom-fade", cx);
    let viewport = bounds(window, "conversation-message-list", cx);
    let scroll_position = point(viewport.left() + px(24.), fade.center().y);
    assert!(viewport.contains(&scroll_position) && fade.contains(&scroll_position));
    let before = stream.read_with(cx, |stream, _| stream.list.logical_scroll_top());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_event(ScrollWheelEvent {
        position: scroll_position,
        delta: ScrollDelta::Pixels(point(px(0.), px(200.))),
        ..Default::default()
    });
    visual.run_until_parked();
    let after = stream.read_with(cx, |stream, _| stream.list.logical_scroll_top());
    assert_ne!(
        (after.item_ix, after.offset_in_item),
        (before.item_ix, before.offset_in_item)
    );
    assert!(!stream.read_with(cx, |stream, _| stream.following_tail()));

    let shell = bounds(window, "composer-shell", cx);
    let covered = stream.read_with(cx, |stream, cx| {
        let insets = stream.stage_top_insets(cx);
        (0..cards.len()).find_map(|index| {
            let item = stream.list.bounds_for_item(index)?;
            let position = point(
                shell.center().x,
                item.top()
                    + px(insets.get(&index).copied().unwrap_or(0.))
                    + px(Layout::CONVERSATION_TOOL_HEIGHT / 2.),
            );
            (item.size.width > px(0.) && shell.contains(&position)).then_some((index, position))
        })
    });
    let (covered_index, position) = covered.expect("a real clickable tool row under the composer");
    let click_position = point(position.x, shell.top() + px(4.));
    stream.update(cx, |stream, cx| {
        stream.list.scroll_by(position.y - click_position.y);
        cx.notify();
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        let item = stream
            .list
            .bounds_for_item(covered_index)
            .expect("covered tool");
        let insets = stream.stage_top_insets(cx);
        let header_top = item.top() + px(insets.get(&covered_index).copied().unwrap_or(0.));
        assert!(click_position.y > header_top);
        assert!(click_position.y < header_top + px(Layout::CONVERSATION_TOOL_HEIGHT));
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(click_position, Modifiers::default());
    visual.run_until_parked();
    assert!(!cards[covered_index].read_with(cx, |card, _| card.is_expanded()));

    stream.update(cx, |stream, cx| {
        stream.apply_model_options(vec!["mock".into(), "another-model".into()], cx);
    });
    for (trigger, menu) in [
        ("composer-permission-status", "composer-permission-picker"),
        ("composer-model", "composer-model-menu"),
    ] {
        click(window, trigger, cx);
        let menu_bounds = bounds(window, menu, cx);
        let fade = bounds(window, "conversation-bottom-fade", cx);
        assert!(menu_bounds.top() < fade.bottom());
        window
            .update(cx, |_, window, cx| {
                let quads = window.painted_quads();
                let scale = window.scale_factor();
                let fade_index = quads
                    .iter()
                    .position(|quad| {
                        quad.bounds == fade.scale(scale) && quad.background.as_solid().is_none()
                    })
                    .expect("fade quad");
                assert!(quads.iter().skip(fade_index + 1).any(|quad| {
                    quad.bounds == menu_bounds.scale(scale)
                        && quad.background.as_solid() == Some(theme(cx).colors.bg_elevated.into())
                }));
            })
            .expect("menu window");
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds(menu)
                .is_none()
        );
    }
    click(window, "composer-add", cx);
    let action = bounds(window, "composer-action-file", cx);
    let surface = bounds(window, "conversation-foreground-surface", cx);
    assert!(action.top() >= surface.top() && action.bottom() <= surface.bottom());
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-action-file")
            .is_none()
    );
}

#[gpui_kit::test]
async fn issue403_empty_layout_and_short_overflow_navigation_remain_usable(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue403-short");
    install_utility_globals(cx, &[], Some(PROJECT_BINDING));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("conversation-foreground").is_none());
    assert!(visual.debug_bounds("conversation-bottom-fade").is_none());
    let utility = visual
        .debug_bounds("composer-utility-bar")
        .expect("empty-task utility bar");
    let shell = visual.debug_bounds("composer-shell").expect("composer");
    assert!(utility.bottom() <= shell.top());
    visual.simulate_resize(size(px(594.), px(320.)));
    visual.run_until_parked();
    seed(&stream, 2, cx);
    let foreground = bounds(window, "conversation-foreground", cx);
    let (geometry, viewport) = stream.read_with(cx, |stream, cx| {
        (
            stream.message_anchor_geometry(cx, 0.),
            stream.list.viewport_bounds(),
        )
    });
    assert!(geometry.total_height < f32::from(viewport.size.height));
    assert!(geometry.total_height > f32::from(foreground.top() - viewport.top()));
    assert!(geometry.should_show());
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("message-anchor-rail")
            .is_some()
    );
}
