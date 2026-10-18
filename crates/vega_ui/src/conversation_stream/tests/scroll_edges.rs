use super::*;
use gpui_kit::{ListOffset, Modifiers, VisualTestContext, linear_color_stop, linear_gradient};

#[gpui_kit::test]
async fn scroll_edge_production_conversation_only_adds_top_fade_and_preserves_composer(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "scroll-edge-conversation");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                (1..=60)
                    .map(|index| hydration_user(index, "A bounded request."))
                    .collect(),
                None,
            ),
            cx,
        );
    });
    cx.run_until_parked();
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        let colors = palette.colors;
        cx.update(|cx| {
            cx.set_global(palette);
            cx.refresh_windows();
        });
        cx.run_until_parked();
        let viewport = bounds(window, "conversation-message-list", cx);
        let composer = bounds(window, "conversation-bottom-fade", cx);
        let top_gradient = linear_gradient(
            180.,
            linear_color_stop(colors.bg_base, 0.),
            linear_color_stop(
                colors.bg_base.opacity(0.),
                Layout::SCROLL_EDGE_FADE_READING / f32::from(viewport.size.height),
            ),
        );
        let composer_gradient = linear_gradient(
            180.,
            linear_color_stop(colors.bg_base.opacity(0.), 0.),
            linear_color_stop(colors.bg_base, 1.),
        );
        window
            .update(cx, |_, window, _| {
                let scale = window.scale_factor();
                let quads = window.painted_quads();
                assert_eq!(
                    quads
                        .iter()
                        .filter(|quad| quad.background == top_gradient
                            && quad.bounds.top() == viewport.scale(scale).top())
                        .count(),
                    1
                );
                assert_eq!(
                    quads
                        .iter()
                        .filter(|quad| quad.background == composer_gradient
                            && quad.bounds == composer.scale(scale))
                        .count(),
                    1
                );
                assert_eq!(
                    composer.size.height,
                    px(Layout::CONVERSATION_BOTTOM_FADE_HEIGHT)
                );
            })
            .expect("production fade paint");
        stream.update(cx, |stream, cx| {
            stream.list.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        cx.run_until_parked();
        window
            .update(cx, |_, window, _| {
                assert!(
                    !window
                        .painted_quads()
                        .iter()
                        .any(|quad| quad.background == top_gradient)
                );
            })
            .expect("top endpoint has no top fade");
        stream.update(cx, |stream, cx| stream.resume_tail(cx));
        cx.run_until_parked();
        assert_composer_resize_preserves_reading_and_tail(window, &stream, colors.bg_base, cx);
    }
}

fn assert_composer_resize_preserves_reading_and_tail(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    color: gpui_kit::Rgba,
    cx: &mut TestAppContext,
) {
    let input = stream.read_with(cx, |stream, _| stream.input.clone());
    let count = stream.read_with(cx, |stream, _| stream.entries.len());
    let assert_tail_readable = |cx: &mut TestAppContext| {
        let foreground = bounds(window, "conversation-foreground", cx);
        stream.read_with(cx, |stream, _| {
            assert!(stream.following_tail());
            let viewport = stream.list.viewport_bounds();
            let last = stream
                .list
                .bounds_for_item(count - 1)
                .expect("last message");
            assert!(last.size.height > px(0.));
            assert!(last.top() >= viewport.top());
            assert!(last.bottom() <= foreground.top() + px(1.));
            assert!(
                (f32::from(viewport.bottom() - last.bottom() - stream.foreground_height)).abs()
                    <= 1.
            );
        });
    };
    for reviewing in [false, true] {
        if reviewing {
            stream.update(cx, |stream, cx| {
                stream.list.scroll_to(ListOffset {
                    item_ix: 10,
                    offset_in_item: px(8.),
                });
                cx.notify();
            });
            cx.run_until_parked();
        }
        let anchor = stream.read_with(cx, |stream, _| stream.scroll_anchor_snapshot());
        let mut heights = Vec::new();
        for draft in [
            "Short draft",
            "First line\nSecond line\nThird line\nFourth line\nFifth line\nSixth line\nSeventh line\nEighth line",
            "Short draft",
        ] {
            input.update(cx, |input, cx| input.set_text(draft, cx));
            cx.run_until_parked();
            window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .expect("deliver the input row layout frame");
            cx.run_until_parked();
            let foreground = bounds(window, "conversation-foreground", cx);
            let fade = bounds(window, "conversation-bottom-fade", cx);
            let surface = bounds(window, "conversation-foreground-surface", cx);
            let viewport = bounds(window, "conversation-message-list", cx);
            heights.push(foreground.size.height);
            assert_eq!(
                fade.size.height,
                px(Layout::CONVERSATION_BOTTOM_FADE_HEIGHT)
            );
            assert_eq!(
                fade.top(),
                foreground.top()
                    + px(
                        Layout::READING_NAVIGATION_HEIGHT - Layout::CONVERSATION_BOTTOM_FADE_HEIGHT
                    )
            );
            assert_eq!(
                surface.top(),
                foreground.top() + px(Layout::READING_NAVIGATION_HEIGHT)
            );
            assert_eq!(foreground.bottom(), viewport.bottom());
            stream.read_with(cx, |stream, cx| {
                assert_eq!(stream.foreground_height, foreground.size.height);
                assert_eq!(stream.entries.len(), count);
                assert_eq!(stream.list.item_count(), count);
                assert_eq!(stream.input.read(cx).text(), draft);
                assert_eq!(stream.following_tail(), !reviewing);
                if reviewing {
                    let current = stream.scroll_anchor_snapshot();
                    assert_eq!(current.identity, anchor.identity);
                    assert_eq!(current.offset_in_item_px, anchor.offset_in_item_px);
                }
            });
            window
                .update(cx, |_, window, _| {
                    let expected = linear_gradient(
                        180.,
                        linear_color_stop(color.opacity(0.), 0.),
                        linear_color_stop(color, 1.),
                    );
                    assert_eq!(
                        window
                            .painted_quads()
                            .iter()
                            .filter(|quad| quad.background == expected
                                && quad.bounds == fade.scale(window.scale_factor()))
                            .count(),
                        1
                    );
                })
                .expect("dynamic composer keeps its original foreground gradient");
            if !reviewing {
                assert_tail_readable(cx);
            }
        }
        assert!(heights[1] > heights[0]);
        assert_eq!(heights[2], heights[0]);
    }
    stream.update(cx, |stream, cx| stream.resume_tail(cx));
    cx.run_until_parked();
    assert_tail_readable(cx);
}

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .expect(selector)
}

fn pinned_fixture(
    cx: &mut TestAppContext,
    final_state: bool,
) -> (WindowHandle<StreamHarness>, Entity<ConversationStream>) {
    let (window, stream, _) = open_controller_stream(cx, "scroll-edge-pinned");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "sticky".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "sticky".into(),
                delta: "Synthetic process".into(),
            },
            cx,
        );
        for index in 0..20 {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(&format!("tool-{index}"), "pwd"),
                },
                cx,
            );
        }
        if final_state {
            confirm_test_final(stream, "sticky", cx);
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "sticky".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                    execution_duration_ms: Some(1000),
                },
                cx,
            );
            stream.composer_submit_pending = true;
            stream.accept_composer_submission(&"Following visible content.\n\n".repeat(80), cx);
        } else {
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "sticky".into(),
                    delta: "Following visible content.\n\n".repeat(80),
                },
                cx,
            );
        }
    });
    cx.run_until_parked();
    (window, stream)
}

fn position(
    stream: &Entity<ConversationStream>,
    index: usize,
    offset: f32,
    visual: &mut VisualTestContext,
) {
    stream.update(visual, |stream, cx| {
        stream.list.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: px(offset),
        });
        cx.notify();
    });
    visual.run_until_parked();
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual.debug_bounds(selector).expect(selector);
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

#[gpui_kit::test]
async fn scroll_edge_top_fade_yields_to_real_pinned_headers_and_recovers_on_reused_list(
    cx: &mut TestAppContext,
) {
    for final_state in [false, true] {
        let (window, stream) = pinned_fixture(cx, final_state);
        let original_list = stream.read_with(cx, |stream, _| stream.list.clone());
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let index = usize::from(!final_state);
        let selector = if final_state {
            "run-activity-toggle"
        } else {
            "run-stage-toggle-0"
        };
        let assert_top =
            |visual: &mut VisualTestContext,
             expected: bool,
             header: Option<gpui_kit::Bounds<gpui_kit::Pixels>>| {
                let viewport = stream.read_with(visual, |stream, _| stream.list.viewport_bounds());
                visual
                    .update_window(window.into(), |_, window, _| {
                        let color = vega_theme::LIGHT.bg_base;
                        let expected_gradient = gpui_kit::linear_gradient(
                            180.,
                            gpui_kit::linear_color_stop(color, 0.),
                            gpui_kit::linear_color_stop(
                                color.opacity(0.),
                                Layout::SCROLL_EDGE_FADE_READING / f32::from(viewport.size.height),
                            ),
                        );
                        let scale = window.scale_factor();
                        let quads = window.painted_quads();
                        let matching = quads
                            .iter()
                            .filter(|quad| {
                                quad.background == expected_gradient
                                    && quad.bounds == viewport.scale(scale)
                            })
                            .collect::<Vec<_>>();
                        assert_eq!(!matching.is_empty(), expected);
                        if let Some(header) = header {
                            assert!(matching.iter().all(|quad| {
                                quad.bounds
                                    .intersect(&quad.content_mask.bounds)
                                    .intersect(&header.scale(scale))
                                    .is_empty()
                            }));
                        }
                    })
                    .expect("actual top fade order and clip");
                stream.read_with(visual, |stream, _| {
                    assert_eq!(
                        stream.list.logical_scroll_top().item_ix,
                        original_list.logical_scroll_top().item_ix
                    );
                    assert_eq!(
                        stream.list.logical_scroll_top().offset_in_item,
                        original_list.logical_scroll_top().offset_in_item
                    );
                });
            };
        position(&stream, index, 40., &mut visual);
        assert_top(&mut visual, true, None);
        position(&stream, index, 0., &mut visual);
        click(&mut visual, selector);
        position(&stream, index, 100., &mut visual);
        for _ in 0..3 {
            let header = visual
                .debug_bounds(selector)
                .expect("pinned activity title");
            assert_eq!(header.top(), original_list.viewport_bounds().top());
            assert_top(&mut visual, false, Some(header));
            stream.update(&mut visual, |_, cx| cx.notify());
            visual.run_until_parked();
        }
        click(&mut visual, selector);
        position(&stream, index, 40., &mut visual);
        assert_top(&mut visual, true, None);
    }
}
