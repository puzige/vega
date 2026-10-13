use super::*;
use gpui_kit::{VisualTestContext, point, size};

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
                assert!(visual.debug_bounds("screenshot-save").is_none());
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
                assert!((card.size.height - image.size.height - px(2.0)).abs() < px(1.0));
                assert!((image.top() - card.top() - px(1.0)).abs() < px(1.0));
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
async fn issue75_screenshot_requests_shared_preview_without_covering_conversation(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "screenshot-preview");
    deliver(&stream, picture("landscape"), cx);
    let requests = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = requests.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, request: &ImagePreviewRequested, _| {
            captured
                .borrow_mut()
                .push((request.image.clone(), request.action));
        })
        .detach()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let before = visual.debug_bounds("assistant-screenshot").unwrap();
    click(window, "screenshot-expand", cx);
    let image = requests.borrow().last().unwrap().0.clone();
    assert_eq!(
        image.read_with(cx, |image, _| image.image().clone()),
        picture("landscape")
    );
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("screenshot-preview").is_none());
    assert_eq!(visual.debug_bounds("assistant-screenshot").unwrap(), before);
    visual.update(|window, cx| image.read(cx).thumbnail_focus().focus(window, cx));
    visual.simulate_keystrokes("enter space");
    assert_eq!(requests.borrow().len(), 3);
    assert!(
        requests
            .borrow()
            .iter()
            .all(|(source, action)| source == &image
                && matches!(action, crate::image_preview::ImagePreviewAction::Open))
    );
    stream.update(cx, |stream, cx| {
        assert!(stream.replace_history_window(
            hydration_page(vec![hydration_user(80, "Different page")], None),
            "user-80",
            cx
        ));
    });
    assert!(!stream.read_with(cx, |stream, cx| stream.owns_image_preview(&image, cx)));
}
