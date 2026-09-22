use super::*;

#[gpui_kit::test]
fn issue78_user_bubble_hugs_text_and_wraps_within_column(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let visual = cx.add_empty_window();
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
                lines: user_message_lines(1, text),
            };
            visual.draw(
                gpui_kit::point(px(0.), px(0.)),
                gpui_kit::size(px(width), px(4000.)),
                |window, cx| {
                    div()
                        .w(px(width))
                        .child(render_entry(&entry, &StreamCounters::default(), window, cx))
                        .into_any_element()
                },
            );
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
