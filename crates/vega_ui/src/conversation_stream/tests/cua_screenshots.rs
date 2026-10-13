use super::*;
use gpui_kit::{ScrollDelta, ScrollWheelEvent, VisualTestContext, point, size};

fn picture(name: &str) -> ImageAttachment {
    let bytes: &[u8] = match name {
        "landscape" => include_bytes!("fixtures/screenshot-landscape.png"),
        "portrait" => include_bytes!("fixtures/screenshot-portrait.png"),
        "strip" => include_bytes!("fixtures/screenshot-strip.png"),
        "column" => include_bytes!("fixtures/screenshot-column.png"),
        "tiny" => include_bytes!("fixtures/screenshot-tiny.png"),
        _ => unreachable!(),
    };
    ImageAttachment::from_bytes(bytes.to_vec()).unwrap()
}

fn deliver(stream: &Entity<ConversationStream>, image: ImageAttachment, cx: &mut TestAppContext) {
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "shot-message".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ScreenshotDelivered {
                message_id: "shot-message".into(),
                call_id: "shot".into(),
                image,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                message_id: "shot-message".into(),
                execution_duration_ms: Some(100),
            },
            cx,
        );
    });
    cx.run_until_parked();
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Default::default());
    visual.run_until_parked();
}

fn screenshot_history(name: &str, call: &str) -> HistoryEntry {
    HistoryEntry::AssistantScreenshot {
        seq: 1,
        message_id: "assistant-1".into(),
        call_id: call.into(),
        image: picture(name),
    }
}

#[gpui_kit::test]
async fn issue75_screenshot_card_wraps_actual_thumbnail_instead_of_full_conversation(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "screenshot-layout");
    for theme in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        let colors = theme.colors;
        cx.update(|cx| cx.set_global(theme));
        for (name, full_width, full_height) in [
            ("landscape", 360.0, 240.0),
            ("portrait", 160.0, 240.0),
            ("strip", 360.0, 32.0),
            ("column", 64.0, 240.0),
            ("tiny", 64.0, 32.0),
        ] {
            stream.update(cx, |stream, cx| {
                assert!(stream.replace_history_window(
                    hydration_page(vec![screenshot_history(name, name)], None),
                    "assistant-1",
                    cx,
                ));
            });
            for viewport in [1100.0, 360.0, 240.0] {
                let mut visual = VisualTestContext::from_window(window.into(), cx);
                visual.simulate_resize(size(px(viewport), px(850.0)));
                visual.run_until_parked();
                let card = visual.debug_bounds("assistant-screenshot").unwrap();
                let image = visual.debug_bounds("screenshot-expand").unwrap();
                let save = visual.debug_bounds("screenshot-save").unwrap();
                if name == "tiny" {
                    let pixels = visual.debug_bounds("screenshot-pixels").unwrap();
                    assert_eq!(
                        pixels.size,
                        size(px(16.0), px(8.0)),
                        "original small pixels are never enlarged"
                    );
                    assert_eq!(pixels.center(), image.center());
                }
                let expected_width = (full_width + 2.0_f32).min(viewport - 64.0);
                assert!(
                    (f32::from(card.size.width) - expected_width).abs() < 1.0,
                    "{name}/{viewport}: {card:?}"
                );
                assert!(
                    (f32::from(image.size.height)
                        - full_height * (expected_width - 2.0) / full_width)
                        .abs()
                        < 1.0,
                    "{name}/{viewport}: {image:?}"
                );
                assert!((card.size.height - image.size.height - px(34.0)).abs() < px(1.0));
                assert_eq!(save.size, size(px(28.0), px(28.0)));
                assert!(save.left() >= card.left() && save.right() <= card.right());
                assert!(save.bottom() <= image.top());
                let (quads, scale) =
                    visual.update(|window, _| (window.painted_quads(), window.scale_factor()));
                assert!(
                    quads.iter().any(|quad| {
                        (quad.bounds.size.width.as_f32() / scale - f32::from(card.size.width)).abs()
                            < 1.0
                            && (quad.bounds.size.height.as_f32() / scale
                                - f32::from(card.size.height))
                            .abs()
                                < 1.0
                            && quad.border_color == colors.border_subtle.into()
                            && (quad.border_widths.top.as_f32() / scale - 1.0).abs() < 0.1
                    }),
                    "visible semantic boundary for {name}/{viewport}"
                );
                visual.simulate_mouse_move(point(px(0.0), px(0.0)), None, Default::default());
                visual.simulate_mouse_move(image.center(), None, Default::default());
                visual.run_until_parked();
                assert_eq!(visual.debug_bounds("assistant-screenshot").unwrap(), card);
                assert_eq!(visual.debug_bounds("screenshot-expand").unwrap(), image);
                let (quads, scale) =
                    visual.update(|window, _| (window.painted_quads(), window.scale_factor()));
                assert!(
                    quads.iter().any(|quad| {
                        (quad.bounds.size.width.as_f32() / scale - f32::from(image.size.width))
                            .abs()
                            < 1.0
                            && (quad.bounds.size.height.as_f32() / scale
                                - f32::from(image.size.height))
                            .abs()
                                < 1.0
                            && quad.border_color == colors.text_secondary.into()
                            && (quad.border_widths.top.as_f32() / scale - 1.0).abs() < 0.1
                    }),
                    "thumbnail hover remains visible over opaque pixels"
                );
                visual.simulate_mouse_move(point(px(0.0), px(0.0)), None, Default::default());
                visual.run_until_parked();
            }
        }
    }
}

#[gpui_kit::test]
async fn issue75_screenshot_image_opens_root_preview_without_changing_chat_geometry(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "screenshot-preview");
    deliver(&stream, picture("landscape"), cx);
    for width in [1100.0, 360.0] {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_resize(size(px(width), px(850.0)));
        visual.run_until_parked();
        let before = visual.debug_bounds("assistant-screenshot").unwrap();
        let trigger = visual.debug_bounds("screenshot-expand").unwrap();
        visual.simulate_click(trigger.center(), Default::default());
        visual.run_until_parked();
        let panel = visual.debug_bounds("screenshot-preview-panel").unwrap();
        assert!(visual.debug_bounds("screenshot-preview").is_some());
        assert_eq!(visual.debug_bounds("assistant-screenshot").unwrap(), before);
        assert!(panel.left() >= px(24.0) && panel.right() <= px(width - 24.0));
        assert!(panel.size.width <= px(960.0) && panel.size.height <= px(720.0));
        let colors = visual.update(|_, cx| theme(cx).colors);
        let (quads, scale) =
            visual.update(|window, _| (window.painted_quads(), window.scale_factor()));
        assert!(
            quads.iter().any(|quad| {
                (quad.bounds.size.width.as_f32() / scale - f32::from(panel.size.width - px(2.0)))
                    .abs()
                    < 1.0
                    && (quad.bounds.size.height.as_f32() / scale - 32.0).abs() < 1.0
                    && quad.background.as_solid() == Some(colors.bg_elevated.into())
                    && (quad.corner_radii.top_left.as_f32() / scale - 7.0).abs() < 0.1
                    && (quad.corner_radii.top_right.as_f32() / scale - 7.0).abs() < 0.1
            }),
            "preview header paints its own inset top corners"
        );
        visual.simulate_keystrokes("escape");
        assert!(visual.debug_bounds("screenshot-preview").is_none());
        visual.simulate_keystrokes("enter");
        assert!(
            visual.debug_bounds("screenshot-preview").is_some(),
            "Escape returns focus to thumbnail"
        );
        visual.simulate_keystrokes("escape space");
        assert!(
            visual.debug_bounds("screenshot-preview").is_some(),
            "Space opens focused thumbnail"
        );
        visual.simulate_keystrokes("tab");
        let save_focus = visual.update(|window, cx| window.focused(cx));
        visual.simulate_keystrokes("tab");
        let close_focus = visual.update(|window, cx| window.focused(cx));
        assert_ne!(save_focus, close_focus);
        visual.simulate_keystrokes("tab");
        assert_eq!(visual.update(|window, cx| window.focused(cx)), save_focus);
        visual.simulate_keystrokes("shift-tab");
        assert_eq!(visual.update(|window, cx| window.focused(cx)), close_focus);
        visual.simulate_keystrokes("space");
        assert!(
            visual.debug_bounds("screenshot-preview").is_none(),
            "keyboard close"
        );
        let (quads, scale) =
            visual.update(|window, _| (window.painted_quads(), window.scale_factor()));
        assert!(
            quads.iter().any(|quad| {
                (quad.bounds.size.width.as_f32() / scale - f32::from(trigger.size.width)).abs()
                    < 1.0
                    && quad.border_color == colors.brand_primary.into()
                    && (quad.border_widths.top.as_f32() / scale - 1.0).abs() < 0.1
            }),
            "return focus has a visible border on the image target"
        );
        visual.simulate_keystrokes("enter");
        let backdrop = visual.debug_bounds("screenshot-preview").unwrap();
        visual.simulate_click(
            backdrop.origin + point(px(4.0), px(4.0)),
            Default::default(),
        );
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("screenshot-preview").is_none(),
            "outside click closes"
        );
        visual.simulate_keystrokes("space");
        assert!(
            visual.debug_bounds("screenshot-preview").is_some(),
            "outside close returns focus"
        );
        visual.simulate_keystrokes("escape");
    }
}

#[gpui_kit::test]
async fn issue75_screenshot_preview_blocks_background_scroll_and_disappears_with_owner(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "screenshot-owner");
    let mut entries: Vec<_> = (2..35)
        .map(|seq| hydration_user(seq, "Earlier conversation line."))
        .collect();
    entries.push(screenshot_history("landscape", "last"));
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(hydration_page(entries, None), cx)
    });
    cx.run_until_parked();
    click(window, "screenshot-expand", cx);
    let before = stream.read_with(cx, |stream, _| stream.list.logical_scroll_top());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let list = visual.debug_bounds("conversation-message-list").unwrap();
    visual.simulate_event(ScrollWheelEvent {
        position: list.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(250.0))),
        ..Default::default()
    });
    visual.run_until_parked();
    let after = stream.read_with(&visual, |stream, _| stream.list.logical_scroll_top());
    assert_eq!(before.item_ix, after.item_ix);
    assert_eq!(before.offset_in_item, after.offset_in_item);
    let composer_add = visual.debug_bounds("composer-add").unwrap();
    visual.simulate_click(composer_add.center(), Default::default());
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("composer-action-images").is_none(),
        "preview must not activate covered composer control"
    );
    if visual.debug_bounds("screenshot-preview").is_some() {
        visual.simulate_keystrokes("escape");
    }
    visual.simulate_click(composer_add.center(), Default::default());
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("composer-action-images").is_some(),
        "same click activates composer after preview closes"
    );
    visual.simulate_click(composer_add.center(), Default::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("composer-action-images").is_none());
    visual.simulate_event(ScrollWheelEvent {
        position: list.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(250.0))),
        ..Default::default()
    });
    visual.run_until_parked();
    let moved = stream.read_with(&visual, |stream, _| stream.list.logical_scroll_top());
    assert!(
        before.item_ix != moved.item_ix || before.offset_in_item != moved.offset_in_item,
        "same wheel moves list after preview closes"
    );
    stream.update(cx, |stream, cx| {
        stream.list.scroll_to_end();
        cx.notify();
    });
    cx.run_until_parked();
    click(window, "screenshot-expand", cx);
    stream.update(cx, |stream, cx| {
        assert!(stream.replace_history_window(
            hydration_page(vec![hydration_user(80, "Different page")], None),
            "user-80",
            cx
        ));
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("screenshot-preview").is_none());
    assert!(stream.read_with(&visual, |stream, _| stream.screenshot_preview.is_none()));
    assert!(visual.update(|window, cx| stream.read(cx).selection_focus.is_focused(window)));
}

#[gpui_kit::test]
async fn issue75_screenshot_history_multiple_images_keep_save_identity_and_native_outcomes(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "screenshot-multiple");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    screenshot_history("landscape", "first"),
                    screenshot_history("portrait", "second"),
                ],
                None,
            ),
            cx,
        )
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(1100.0), px(1100.0)));
    visual.run_until_parked();
    click(window, "screenshot-save", cx);
    assert!(
        cx.did_prompt_for_new_path(),
        "card icon opens native picker"
    );
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    let temp = tempfile::tempdir().unwrap();
    for (index, name) in ["landscape", "portrait"].into_iter().enumerate() {
        let bounds = stream.read_with(&visual, |stream, _| {
            stream.list.bounds_for_item(index).unwrap()
        });
        visual.simulate_click(
            bounds.origin + point(px(20.0), px(65.0)),
            Default::default(),
        );
        visual.run_until_parked();
        assert!(visual.debug_bounds("screenshot-preview").is_some());
        visual.simulate_keystrokes("tab enter");
        assert!(cx.did_prompt_for_new_path());
        cx.simulate_new_path_selection(|parent| {
            assert_ne!(parent, std::path::Path::new("/"));
            None
        });
        cx.run_until_parked();
        click(window, "screenshot-preview-save", cx);
        assert!(cx.did_prompt_for_new_path());
        click(window, "screenshot-preview-save", cx);
        let saved = temp.path().join(format!("{name}.png"));
        cx.simulate_new_path_selection(|_| Some(saved.clone()));
        cx.run_until_parked();
        assert_eq!(std::fs::read(&saved).unwrap(), picture(name).bytes());
        assert_eq!(
            stream.read_with(cx, |stream, _| stream
                .list
                .bounds_for_item(index)
                .unwrap()
                .size
                .height),
            bounds.size.height,
            "saving in preview must not grow the underlying conversation row"
        );
        assert!(
            !cx.did_prompt_for_new_path(),
            "busy activation must not open another picker"
        );
        click(window, "screenshot-preview-save", cx);
        cx.simulate_new_path_selection(|_| Some(saved.clone()));
        cx.run_until_parked();
        assert_eq!(
            std::fs::read(&saved).unwrap(),
            picture(name).bytes(),
            "failed overwrite keeps original intact"
        );
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual.debug_bounds("screenshot-save-status").is_some(),
            "native save failure stays by its card"
        );
        visual.simulate_keystrokes("escape");
    }
    click(window, "screenshot-expand", cx);
    let next = cx.new(|cx| ConversationStream::new(permission_thread(), cx));
    window
        .update(cx, |root, _, cx| {
            root.stream = next;
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("screenshot-preview")
            .is_none(),
        "thread switch unmounts preview"
    );
}
