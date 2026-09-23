use super::*;

#[gpui_kit::test]
fn issue78_user_bubble_hugs_text_and_wraps_within_column(cx: &mut TestAppContext) {
    init_permission_test(cx);
    for width in [320.0, 600.0, 768.0] {
        let mut short_height = px(0.);
        for (index, text) in [
            "Hello 你好".to_string(),
            "one\n\nthree".into(),
            "长文本 mixed Latin words ".repeat(40),
            "x".repeat(500),
        ]
        .iter()
        .enumerate()
        {
            let entry = StreamEntry::User {
                copy: MessageCopy::new(text),
                lines: user_message_lines(1, text),
            };
            let (_, visual) = cx.add_window_view(|_, _| EntryView(entry));
            visual.simulate_resize(gpui_kit::size(px(width), px(4000.)));
            visual.run_until_parked();
            let bubble = visual
                .debug_bounds("user-message-bubble")
                .expect("production user bubble");
            assert!(
                (f32::from(bubble.right()) - width).abs() < 1.,
                "bubble must align to column right: {bubble:?}"
            );
            assert!(
                f32::from(bubble.size.width) <= width * 0.8 + 1.,
                "bubble capped at 80%: {bubble:?}"
            );
            let layouts = render_rows::USER_TEXT_LAYOUTS.with_borrow_mut(std::mem::take);
            assert_eq!(
                layouts
                    .iter()
                    .map(|layout| layout.text())
                    .collect::<Vec<_>>(),
                text.split('\n')
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
            );
            for layout in layouts {
                let rendered = layout.text();
                for offset in rendered
                    .char_indices()
                    .map(|(index, _)| index)
                    .chain(std::iter::once(rendered.len()))
                {
                    let position = layout
                        .position_for_index(offset)
                        .expect("every character remains laid out");
                    assert!(
                        position.x >= bubble.left() + px(12.)
                            && position.x <= bubble.right() - px(12.) + px(1.),
                        "glyph outside bubble: {position:?} {bubble:?}"
                    );
                    assert!(
                        position.y >= bubble.top()
                            && position.y + layout.line_height() <= bubble.bottom() + px(1.),
                        "glyph vertically clipped"
                    );
                }
            }
            if index == 0 {
                assert!(
                    f32::from(bubble.size.width) < width * 0.6,
                    "short bubble must hug content"
                );
                short_height = bubble.size.height;
            } else if index == 1 {
                assert!(
                    (f32::from(bubble.size.height - short_height)
                        - 2. * Typography::MESSAGE * Typography::MESSAGE_LINE_HEIGHT)
                        .abs()
                        < 1.,
                    "internal blank line preserved"
                );
            } else {
                assert!(
                    bubble.size.height > short_height * 3.,
                    "long text must wrap without truncation"
                );
            }
        }
    }
}

#[gpui_kit::test]
fn issue78_image_entry_aligns_right(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let image = ImageAttachment::from_bytes(
        include_bytes!("../../../../../assets/logo/raster/vega-icon-f1-original.png").to_vec(),
    )
    .expect("valid image");
    let entry = StreamEntry::UserImages {
        images: vec![crate::conversation_stream::attachments::ImagePreview::new(
            image,
        )],
    };
    let (_, visual) = cx.add_window_view(|_, _| EntryView(entry));
    visual.simulate_resize(gpui_kit::size(px(600.), px(600.)));
    visual.run_until_parked();
    let image = visual
        .debug_bounds("attachment-thumbnail")
        .expect("production image thumbnail");
    assert_eq!(image.right(), px(600.), "user image right edge");
    assert_eq!(image.size.width, px(Layout::ATTACHMENT_THUMBNAIL));
}

struct EntryView(StreamEntry);
impl Render for EntryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().w_full().child(render_entry(
            &self.0,
            &StreamCounters::default(),
            window,
            cx,
        ))
    }
}

#[gpui_kit::test]
fn issue78_bubble_paints_theme_color_and_radius(cx: &mut TestAppContext) {
    init_permission_test(cx);
    for theme_value in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        let colors = theme_value.colors;
        cx.update(|cx| cx.set_global(theme_value));
        let (_, visual) = cx.add_window_view(|_, _| {
            EntryView(StreamEntry::User {
                copy: MessageCopy::new("Hello"),
                lines: user_message_lines(1, "Hello"),
            })
        });
        visual.run_until_parked();
        let bubble = visual.debug_bounds("user-message-bubble").expect("bubble");
        let (quads, scale) =
            visual.update(|window, _| (window.painted_quads(), window.scale_factor()));
        let painted = quads
            .iter()
            .find(|quad| quad.background.as_solid() == Some(colors.brand_soft.into()))
            .expect("brand_soft painted bubble");
        assert!((painted.bounds.size.width.0 / scale - f32::from(bubble.size.width)).abs() < 1.);
        assert!((painted.corner_radii.top_left.0 / scale - 16.).abs() < 1.);
        let layouts = render_rows::USER_TEXT_LAYOUTS.with_borrow_mut(std::mem::take);
        assert_eq!(layouts[0].text(), "Hello");
    }
}

#[gpui_kit::test]
async fn issue78_history_and_streaming_keep_assistant_outside_user_bubble(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue78-history");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, "Hello"),
                    hydration_assistant(2, "Assistant answer"),
                ],
                None,
            ),
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "live".into(),
                seq: 3,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live".into(),
                delta: "Streaming answer".into(),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let bubble = visual
        .debug_bounds("user-message-bubble")
        .expect("hydrated bubble");
    let column = visual
        .debug_bounds("conversation-column")
        .expect("message column");
    assert!(bubble.left() > column.left());
    assert!((f32::from(bubble.right() - column.right())).abs() < 1.);
    let assistant = visual
        .debug_bounds("assistant-message")
        .expect("assistant message");
    assert_eq!(assistant.left(), column.left());
    assert_eq!(assistant.right(), column.right());
    stream.read_with(&visual, |stream, _| {
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::User { .. }))
                .count(),
            1
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::Assistant { .. }))
                .count(),
            2
        );
    });
}

#[gpui_kit::test]
fn issue78_hover_copy_user_action_is_mounted(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (_, visual) = cx.add_window_view(|_, _| {
        EntryView(StreamEntry::User {
            copy: MessageCopy::new("原文\n\n末尾\n"),
            lines: user_message_lines(7, "原文\n\n末尾\n"),
        })
    });
    visual.run_until_parked();
    if MESSAGE_COPY_ACTIONS_ENABLED {
        assert!(
            visual.debug_bounds("message-copy-user").is_some(),
            "message must expose its copy action below the body"
        );
    } else {
        assert!(
            visual.debug_bounds("user-message-bubble").is_some(),
            "the message body must keep rendering while the action is disabled"
        );
        assert!(
            visual.debug_bounds("message-copy-user").is_none(),
            "the disabled action must not reserve or mount a row"
        );
    }
}

/// Builds one user/assistant entry carrying a `MessageCopy` buffer, shared by
/// the enabled/disabled halves of the geometry contract below.
fn copy_geometry_entry(user: bool, source: &str) -> StreamEntry {
    let copy = MessageCopy::new(source);
    if user {
        StreamEntry::User {
            lines: user_message_lines(1, source),
            copy,
        }
    } else {
        let mut stream = MarkdownStream::new();
        stream.append(source);
        stream.finish();
        let mut model = StreamModel::default();
        model.sync(&stream.snapshot(), &StreamCounters::default());
        StreamEntry::Assistant {
            stream: Box::new(stream),
            model,
            failure: None,
            copy,
        }
    }
}

/// While the hover-copy actions are disabled (Issue #78 follow-up) the wrapper
/// must not mount or reserve any action row: the message body renders at its
/// own geometry, and no copy can reach the clipboard. This is the regression
/// that proves the disabled state matches the pre-#144 baseline.
#[gpui_kit::test]
fn issue78_hover_copy_disabled_mounts_no_action_row(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, size};
    init_permission_test(cx);
    if MESSAGE_COPY_ACTIONS_ENABLED {
        // Re-enabling the action flips this contract; the enabled geometry
        // path is covered by the pointer/keyboard regression below.
        return;
    }
    for dark in [false, true] {
        cx.update(|cx| {
            cx.set_global(if dark {
                vega_theme::Theme::dark()
            } else {
                vega_theme::Theme::light()
            })
        });
        for user in [false, true] {
            let source = "中文\n\n**raw**\n";
            let (_, visual) =
                cx.add_window_view(|_, _| EntryView(copy_geometry_entry(user, source)));
            visual.simulate_resize(size(px(320.), px(600.)));
            visual.run_until_parked();
            let selector = if user {
                "message-copy-user"
            } else {
                "message-copy-assistant"
            };
            assert!(
                visual.debug_bounds(selector).is_none(),
                "no action row may be mounted while copy actions are disabled"
            );
            let body = visual
                .debug_bounds(if user {
                    "user-message-bubble"
                } else {
                    "assistant-message"
                })
                .expect("message body");
            visual.update(|_, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("sentinel".into()))
            });
            visual.simulate_mouse_move(body.center(), None, Modifiers::default());
            visual.run_until_parked();
            assert!(
                visual.debug_bounds(selector).is_none(),
                "hovering the message must not reveal a disabled action"
            );
            assert_eq!(
                visual.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
                Some("sentinel".to_string()),
                "no copy may reach the clipboard while the action is disabled"
            );
        }
    }
}

#[gpui_kit::test]
fn issue78_hover_copy_geometry_pointer_path_and_keyboard(cx: &mut TestAppContext) {
    use gpui_kit::{Modifiers, point, size};
    init_permission_test(cx);
    if !MESSAGE_COPY_ACTIONS_ENABLED {
        // The enabled geometry contract only applies when the action is on;
        // `issue78_hover_copy_disabled_mounts_no_action_row` covers the off
        // state. Kept as a guard so re-enabling restores full coverage.
        return;
    }
    for dark in [false, true] {
        cx.update(|cx| {
            cx.set_global(if dark {
                vega_theme::Theme::dark()
            } else {
                vega_theme::Theme::light()
            })
        });
        for user in [false, true] {
            let source = "中文\n\n**raw**\n";
            let (_, visual) =
                cx.add_window_view(|_, _| EntryView(copy_geometry_entry(user, source)));
            visual.simulate_resize(size(px(320.), px(600.)));
            visual.run_until_parked();
            let selector = if user {
                "message-copy-user"
            } else {
                "message-copy-assistant"
            };
            let button = visual.debug_bounds(selector).expect("copy button");
            let body = visual
                .debug_bounds(if user {
                    "user-message-bubble"
                } else {
                    "assistant-message"
                })
                .expect("message body");
            assert_eq!(button.size, size(px(24.), px(24.)));
            assert!(button.top() >= body.bottom());
            assert_eq!(
                if user { button.right() } else { button.left() },
                if user { px(320.) } else { px(0.) }
            );
            visual.simulate_mouse_move(body.center(), None, Modifiers::default());
            visual.run_until_parked();
            visual.simulate_mouse_move(button.center(), None, Modifiers::default());
            visual.simulate_click(button.center(), Modifiers::default());
            visual.run_until_parked();
            assert_eq!(
                visual.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
                Some(source.to_string())
            );
            assert_eq!(visual.debug_bounds(selector), Some(button));
            // A focused button has a background. Its painted alpha observes
            // actual GPUI opacity rather than merely testing style builders.
            let has_visible_background = |visual: &mut gpui_kit::VisualTestContext| {
                visual.update(|window, _| {
                    let scale = window.scale_factor();
                    window.painted_quads().iter().any(|quad| {
                        quad.bounds == button.scale(scale)
                            && quad.background.as_solid().is_some_and(|color| color.a > 0.)
                    })
                })
            };
            assert!(
                has_visible_background(visual),
                "pointer on button exposes its background"
            );
            visual.simulate_mouse_move(body.center(), None, Modifiers::default());
            visual.run_until_parked();
            assert!(
                has_visible_background(visual),
                "message hover keeps the action visible"
            );
            visual.simulate_mouse_move(
                point(button.center().x, button.top() - px(1.)),
                None,
                Modifiers::default(),
            );
            visual.run_until_parked();
            assert!(
                has_visible_background(visual),
                "the gap to the action stays in the hover group"
            );
            visual.simulate_mouse_move(point(px(300.), px(550.)), None, Modifiers::default());
            visual.run_until_parked();
            assert!(
                !has_visible_background(visual),
                "mouse focus must not keep the action visible after leaving"
            );
            assert_eq!(visual.debug_bounds(selector), Some(button));
            visual.update(|window, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("before-enter".into()));
                window.blur(cx);
                window.focus_next(cx);
            });
            visual.simulate_keystrokes("enter");
            visual.run_until_parked();
            assert!(
                has_visible_background(visual),
                "keyboard focus exposes the copy action"
            );
            assert_eq!(
                visual.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
                Some(source.to_string())
            );
            visual.update(|_, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("before-space".into()))
            });
            visual.simulate_keystrokes("space");
            assert_eq!(
                visual.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
                Some(source.to_string())
            );
            assert_eq!(visual.debug_bounds(selector), Some(button));
        }
    }
}
