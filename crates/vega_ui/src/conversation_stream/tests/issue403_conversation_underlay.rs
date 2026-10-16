use super::r64_popup_deferred::{PROJECT_BINDING, bounds, click, install_utility_globals};
use super::*;
use crate::conversation_stream::attachments::ImagePreview;
use gpui_kit::{
    Bounds, ListOffset, Modifiers, ScrollDelta, ScrollWheelEvent, VisualTestContext,
    linear_color_stop, linear_gradient, point, size,
};
use vega_conversation::types::{
    AssistantResponseKind, AssistantResponseMetadata, ConversationStopReason, PendingMessage,
};

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
    stage: &str,
    cx: &mut TestAppContext,
) {
    let foreground = bounds(window, "conversation-foreground", cx);
    stream.read_with(cx, |stream, _| {
        assert!(
            stream.following_tail(),
            "{stage}: tail follow must remain active"
        );
        assert_eq!(stream.list.item_count(), stream.entries.len());
        let last = stream
            .list
            .bounds_for_item(stream.entries.len() - 1)
            .expect("last business entry is laid out");
        let viewport = stream.list.viewport_bounds();
        assert!(
            last.bottom() <= foreground.top() + px(0.5),
            "{stage}: last={last:?}, foreground={foreground:?}, viewport={viewport:?}, \
             padding={:?}, scroll_top={:?}",
            stream.foreground_height,
            stream.list.logical_scroll_top()
        );
        assert!(
            last.top() >= viewport.top(),
            "{stage}: last={last:?}, viewport={viewport:?}"
        );
    });
}

fn assert_anchor_clear(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) {
    let viewport = bounds(window, "conversation-message-list", cx);
    let foreground = bounds(window, "conversation-foreground", cx);
    let rail = bounds(window, "message-anchor-rail", cx);
    let readable_center = (viewport.top() + foreground.top()) / 2.;
    assert!((f32::from(rail.center().y - readable_center)).abs() < 1.);
    assert!(rail.top() >= viewport.top() && rail.bottom() <= foreground.top());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_mouse_move(rail.center(), None, Modifiers::default());
    visual.run_until_parked();
    let preview = visual
        .debug_bounds("message-anchor-preview")
        .expect("hovered marker preview");
    let marker_offset = stream.read_with(&visual, |stream, _| {
        let hovered = stream
            .message_anchor_hovered
            .as_deref()
            .expect("hovered marker");
        let index = stream
            .message_anchor_projections()
            .iter()
            .position(|anchor| anchor.message_id == hovered)
            .expect("hovered marker identity");
        px((index - stream.message_anchor_window_start) as f32 * Layout::CONVERSATION_MARKER_PITCH)
    });
    let expected_top = (rail.top() + marker_offset)
        .min(foreground.top() - preview.size.height)
        .max(viewport.top());
    assert!((f32::from(preview.top() - expected_top)).abs() < 1.);
    assert!(preview.top() >= viewport.top() && preview.bottom() <= foreground.top());
    visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
    visual.run_until_parked();
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
            assert_tail_clear(window, &stream, "themed backend", cx);
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
    assert_tail_clear(window, &stream, "initial", cx);
    assert_anchor_clear(window, &stream, cx);
    let initial = bounds(window, "conversation-foreground", cx);
    stream.update(cx, |stream, cx| {
        stream.input.update(cx, |input, cx| {
            input.set_text("one\ntwo\nthree\nfour\nfive\nsix", cx)
        });
    });
    assert_tail_clear(window, &stream, "multiline", cx);
    assert_anchor_clear(window, &stream, cx);
    let multiline = bounds(window, "conversation-foreground", cx);
    assert!(multiline.size.height > initial.size.height);

    stream.update(cx, |stream, cx| {
        let image =
            ImageAttachment::from_bytes(include_bytes!("fixtures/screenshot-tiny.png").to_vec())
                .expect("owned image fixture");
        let preview = ImagePreview::new(image, "image.png".into(), cx);
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
    assert_tail_clear(window, &stream, "attachments queue errors privacy", cx);
    assert_anchor_clear(window, &stream, cx);
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
    assert_tail_clear(window, &stream, "collapsed", cx);
    assert_anchor_clear(window, &stream, cx);
    assert_eq!(bounds(window, "conversation-foreground", cx), initial);
    let visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(594.), px(554.)));
    visual.run_until_parked();
    assert_tail_clear(window, &stream, "narrow window", cx);
    assert_anchor_clear(window, &stream, cx);
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
    assert_tail_clear(window, &stream, "resumed history", cx);
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
        window
            .update(cx, |_, window, cx| {
                let focus = stream.read_with(cx, |stream, cx| {
                    if trigger == "composer-model" {
                        stream.model_focus.clone()
                    } else {
                        stream.input.read(cx).focus_handle(cx)
                    }
                });
                if trigger == "composer-model" {
                    window.focus(&focus, cx);
                }
                assert!(focus.contains_focused(window, cx), "{menu}: keyboard focus");
            })
            .expect("menu keyboard context");
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds(menu)
                .is_none(),
            "{menu}: Escape must unmount the open picker"
        );
        window
            .update(cx, |_, window, cx| {
                let focus = stream.read_with(cx, |stream, cx| {
                    if trigger == "composer-model" {
                        stream.model_focus.clone()
                    } else {
                        stream.input.read(cx).focus_handle(cx)
                    }
                });
                assert!(focus.contains_focused(window, cx), "{menu}: returned focus");
            })
            .expect("closed menu focus");
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

const OCCLUSION_RUN: &str = "occlusion-run";

fn run_header_fixture(
    cx: &mut TestAppContext,
    completed: bool,
) -> (WindowHandle<StreamHarness>, Entity<ConversationStream>) {
    let (window, stream, _) = open_controller_stream(cx, OCCLUSION_RUN);
    let visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(960.), px(600.)));
    visual.run_until_parked();
    populate_run_header(&stream, completed, cx);
    (window, stream)
}

fn populate_run_header(
    stream: &Entity<ConversationStream>,
    completed: bool,
    cx: &mut TestAppContext,
) {
    seed(stream, 16, cx);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: OCCLUSION_RUN.into(),
                seq: 17,
            },
            cx,
        );
        for index in 0..2 {
            let id = format!("occlusion-tool-{index}");
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(&id, "pwd"),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallApproved {
                    call_id: id.clone(),
                    approval: Approval::Once,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallRunning {
                    call_id: id.clone(),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: id,
                    result: ToolResult {
                        status: ToolCallStatus::Success,
                        output: "synthetic output".into(),
                        reused: false,
                        exit_code: Some(0),
                        duration_ms: Some(10),
                        truncated: Some(false),
                        invalid: None,
                    },
                },
                cx,
            );
        }
        let body = "Following visible conversation content.\n\n".repeat(80);
        let body_len = body.len();
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: OCCLUSION_RUN.into(),
                delta: body,
            },
            cx,
        );
        if completed {
            stream.apply_event(
                ConversationEvent::AssistantResponseCompleted {
                    message_id: OCCLUSION_RUN.into(),
                    response: AssistantResponseMetadata {
                        response_id: "occlusion-final".into(),
                        text_start_bytes: 0,
                        text_end_bytes: body_len,
                        kind: AssistantResponseKind::Final,
                    },
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: OCCLUSION_RUN.into(),
                    stop_reason: ConversationStopReason::End,
                    execution_duration_ms: Some(1000),
                },
                cx,
            );
        }
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        let group = stream.run_activity_groups[OCCLUSION_RUN].read(cx);
        assert_eq!(group.process_in_details(), completed);
        assert_eq!(
            group.status(),
            if completed {
                RunActivityStatus::Completed
            } else {
                RunActivityStatus::Running
            }
        );
        if !completed {
            assert_eq!(group.stage_title(0, cx), "Run commands");
        }
    });
}

fn run_header_selector(completed: bool) -> &'static str {
    if completed {
        "run-activity-toggle"
    } else {
        "run-stage-toggle-0"
    }
}

fn run_header_bounds<V: 'static>(
    window: WindowHandle<V>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

fn position_run_header<V: 'static>(
    window: WindowHandle<V>,
    stream: &Entity<ConversationStream>,
    completed: bool,
    center_y: Pixels,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    stream.update(cx, |stream, cx| {
        let index = stream
            .entries
            .iter()
            .position(|entry| {
                if completed {
                    matches!(entry, StreamEntry::RunActivity { .. })
                } else {
                    matches!(entry, StreamEntry::RunActivitySegment { segment: 0, .. })
                }
            })
            .expect("event-created activity header entry");
        assert!(!stream.process_entry_hidden(index, cx));
        stream.list.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    let selector = run_header_selector(completed);
    let natural = run_header_bounds(window, selector, cx);
    stream.update(cx, |stream, cx| {
        stream.list.scroll_by(natural.center().y - center_y);
        cx.notify();
    });
    let actual = run_header_bounds(window, selector, cx);
    assert!(
        (f32::from(actual.center().y - center_y)).abs() < 1.,
        "{selector}: actual={actual:?}, desired center={center_y:?}"
    );
    actual
}

fn assert_run_header_layering(cx: &mut TestAppContext, completed: bool) {
    let (window, stream) = run_header_fixture(cx, completed);
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        let colors = palette.colors;
        cx.update(|cx| {
            cx.set_global(palette);
            cx.refresh_windows();
        });
        for region in ["conversation-bottom-fade", "composer-shell"] {
            let target = bounds(window, region, cx);
            let header = position_run_header(window, &stream, completed, target.center().y, cx);
            let viewport = bounds(window, "conversation-message-list", cx);
            let fade = bounds(window, "conversation-bottom-fade", cx);
            let surface = bounds(window, "conversation-foreground-surface", cx);
            let shell = bounds(window, "composer-shell", cx);
            assert!(viewport.bottom() >= shell.bottom());
            let overlap = header.intersect(&target);
            assert!(
                overlap.size.width > px(0.) && overlap.size.height >= header.size.height - px(1.),
                "{} must really overlap {region}: header={header:?}, target={target:?}",
                run_header_selector(completed)
            );
            if region == "composer-shell" {
                let surface_overlap = header.intersect(&surface);
                assert!(
                    surface_overlap.size.width > px(0.)
                        && surface_overlap.size.height >= header.size.height - px(1.),
                    "{} inside Composer must also overlap the opaque surface: header={header:?}, surface={surface:?}",
                    run_header_selector(completed)
                );
            }
            if !completed {
                let icon = bounds(window, "run-stage-icon-0", cx);
                let title = bounds(window, "run-stage-title-0", cx);
                assert!(header.contains(&icon.center()) && header.contains(&title.center()));
            }
            window.update(cx, |_, window, _| {
                let quads = window.painted_quads();
                let scale = window.scale_factor();
                let solid_index = |bounds: Bounds<Pixels>, color: Rgba| {
                    quads
                        .iter()
                        .rposition(|quad| {
                            quad.bounds == bounds.scale(scale)
                                && quad.background.as_solid() == Some(color.into())
                        })
                        .unwrap_or_else(|| panic!("missing real background quad at {bounds:?}"))
                };
                let header_index = solid_index(header, colors.bg_base);
                let fade_index = quads
                    .iter()
                    .position(|quad| {
                        quad.bounds == fade.scale(scale)
                            && quad.background
                                == linear_gradient(
                                    180.,
                                    linear_color_stop(colors.bg_base.opacity(0.), 0.),
                                    linear_color_stop(colors.bg_base, 1.),
                                )
                    })
                    .expect("painted theme fade");
                let surface_index = solid_index(surface, colors.bg_base);
                let shell_index = solid_index(shell, colors.bg_elevated);
                let header_order = quads[header_index].order;
                let fade_order = quads[fade_index].order;
                let surface_order = quads[surface_index].order;
                let shell_order = quads[shell_index].order;
                if region == "conversation-bottom-fade" {
                    assert!(
                        header_order < fade_order,
                        "{} overlapping {region}: real header quad #{header_index} order {header_order} must precede fade #{fade_index} order {fade_order}",
                        run_header_selector(completed)
                    );
                } else {
                    assert!(
                        header_order < surface_order && surface_order < shell_order,
                        "{} overlapping {region}: real header quad #{header_index} order {header_order} must precede surface #{surface_index} order {surface_order}, then shell #{shell_index} order {shell_order}",
                        run_header_selector(completed)
                    );
                }
            }).expect("header paint order");
        }
    }
}

#[gpui_kit::test]
async fn issue403_live_deferred_header_paints_beneath_foreground(cx: &mut TestAppContext) {
    assert_run_header_layering(cx, false);
}

#[gpui_kit::test]
async fn issue403_completed_deferred_header_paints_beneath_foreground(cx: &mut TestAppContext) {
    assert_run_header_layering(cx, true);
}

#[gpui_kit::test]
async fn issue403_foreground_occludes_real_run_header_clicks(cx: &mut TestAppContext) {
    for completed in [false, true] {
        let (window, stream) = run_header_fixture(cx, completed);
        let shell = bounds(window, "composer-shell", cx);
        let header = position_run_header(window, &stream, completed, shell.top() + px(4.), cx);
        let click_position = point(shell.center().x, header.center().y);
        assert!(shell.contains(&click_position) && header.contains(&click_position));
        let expanded = |cx: &TestAppContext| {
            stream.read_with(cx, |stream, cx| {
                let group = stream.run_activity_groups[OCCLUSION_RUN].read(cx);
                if completed {
                    group.expanded()
                } else {
                    group.segment_expanded(0)
                }
            })
        };
        assert!(!expanded(cx));
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(click_position, Modifiers::default());
        visual.run_until_parked();
        assert!(
            !expanded(cx),
            "{} must not expand through the Composer",
            run_header_selector(completed)
        );
        let viewport = bounds(window, "conversation-message-list", cx);
        let readable =
            position_run_header(window, &stream, completed, viewport.top() + px(100.), cx);
        let foreground = bounds(window, "conversation-foreground", cx);
        assert!(readable.top() >= viewport.top() && readable.bottom() < foreground.top());
        click(window, run_header_selector(completed), cx);
        assert!(expanded(cx), "visible activity header must remain operable");
    }
}

struct ConversationOverlayHarness {
    stream: Entity<ConversationStream>,
    overlay_visible: bool,
    overlay_clicks: usize,
}

impl Render for ConversationOverlayHarness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .key_context("VegaWindow")
            .relative()
            .size_full()
            .child(self.stream.clone())
            .when(self.overlay_visible, |root| {
                root.child(
                    div()
                        .id("issue403-application-overlay")
                        .debug_selector(|| "issue403-application-overlay".into())
                        .absolute()
                        .inset_0()
                        .bg(colors.bg_sidebar)
                        .occlude()
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseUpEvent, _, cx| {
                                this.overlay_clicks += 1;
                                cx.stop_propagation();
                            }),
                        ),
                )
            })
    }
}

#[gpui_kit::test]
async fn issue403_later_application_overlay_covers_conversation(cx: &mut TestAppContext) {
    for completed in [false, true] {
        init_permission_test(cx);
        let mut thread = permission_thread();
        thread.id = OCCLUSION_RUN.into();
        let stream = cx.new(|cx| ConversationStream::new(thread, cx));
        let root_stream = stream.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| ConversationOverlayHarness {
                    stream: root_stream,
                    overlay_visible: false,
                    overlay_clicks: 0,
                })
            })
            .expect("conversation and application overlay window")
        });
        let visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(size(px(960.), px(600.)));
        visual.run_until_parked();
        populate_run_header(&stream, completed, cx);
        for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
            let colors = palette.colors;
            cx.update(|cx| {
                cx.set_global(palette);
                cx.refresh_windows();
            });
            let shell = run_header_bounds(window, "composer-shell", cx);
            let header = position_run_header(window, &stream, completed, shell.center().y, cx);
            let viewport = run_header_bounds(window, "conversation-message-list", cx);
            let overlap = header.intersect(&shell);
            assert!(viewport.bottom() >= shell.bottom());
            assert!(
                overlap.size.width > px(0.) && overlap.size.height >= header.size.height - px(1.),
                "real activity header must overlap Composer before application overlay: header={header:?}, shell={shell:?}"
            );
            window
                .update(cx, |root, _, cx| {
                    root.overlay_visible = true;
                    cx.notify();
                })
                .expect("show later application overlay");
            let overlay = run_header_bounds(window, "issue403-application-overlay", cx);
            assert_eq!(
                run_header_bounds(window, run_header_selector(completed), cx),
                header
            );
            let fade = run_header_bounds(window, "conversation-bottom-fade", cx);
            let surface = run_header_bounds(window, "conversation-foreground-surface", cx);
            let shell = run_header_bounds(window, "composer-shell", cx);
            assert!(overlay.contains(&header.center()) && overlay.contains(&shell.center()));
            window.update(cx, |_, window, _| {
                let quads = window.painted_quads();
                let scale = window.scale_factor();
                let solid_index = |bounds: Bounds<Pixels>, color: Rgba| {
                    quads
                        .iter()
                        .rposition(|quad| {
                            quad.bounds == bounds.scale(scale)
                                && quad.background.as_solid() == Some(color.into())
                        })
                        .unwrap_or_else(|| panic!("missing real background quad at {bounds:?}"))
                };
                let header_index = solid_index(header, colors.bg_base);
                let fade_index = quads
                    .iter()
                    .position(|quad| {
                        quad.bounds == fade.scale(scale)
                            && quad.background
                                == linear_gradient(
                                    180.,
                                    linear_color_stop(colors.bg_base.opacity(0.), 0.),
                                    linear_color_stop(colors.bg_base, 1.),
                                )
                    })
                    .expect("painted theme fade below application overlay");
                let surface_index = solid_index(surface, colors.bg_base);
                let shell_index = solid_index(shell, colors.bg_elevated);
                let overlay_index = solid_index(overlay, colors.bg_sidebar);
                for (name, index) in [
                    ("title wrapper", header_index),
                    ("fade", fade_index),
                    ("surface", surface_index),
                    ("shell", shell_index),
                ] {
                    assert!(index < overlay_index, "{}: {name} quad #{index} must precede ordinary later application overlay #{overlay_index}", run_header_selector(completed));
                }
            }).expect("application overlay scene order");
            let previous_clicks = window
                .read_with(cx, |root, _| root.overlay_clicks)
                .expect("overlay click count");
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            visual.simulate_click(header.center(), Modifiers::default());
            visual.run_until_parked();
            window
                .read_with(cx, |root, _| {
                    assert_eq!(
                        root.overlay_clicks,
                        previous_clicks + 1,
                        "later application overlay must receive the click above Composer and header"
                    )
                })
                .expect("overlay receives click");
            stream.read_with(cx, |stream, cx| {
                let group = stream.run_activity_groups[OCCLUSION_RUN].read(cx);
                assert!(
                    !(if completed {
                        group.expanded()
                    } else {
                        group.segment_expanded(0)
                    }),
                    "{} must not expand through the application overlay",
                    run_header_selector(completed)
                );
            });
            window
                .update(cx, |root, _, cx| {
                    root.overlay_visible = false;
                    cx.notify();
                })
                .expect("hide application overlay");
        }
    }
}
