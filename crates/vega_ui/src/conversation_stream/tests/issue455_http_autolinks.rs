use super::*;
use gpui_kit::{
    CursorStyle, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, VisualTestContext, point,
    size,
};

use super::issue147_markdown_selection::{assistant_entry, drag, layout_for, text_point};
use super::issue357_markdown_links::{
    LinkCapture, MenuCapture, activate_harness, dismiss_context_menu, finish_click, init_link_ui,
    link_point, model, open_read_harness, open_selection_harness, painted_link, redraw,
};

fn user_entry(source: &str) -> StreamEntry {
    StreamEntry::User {
        lines: user_message_lines(455, source),
        copy: MessageCopy::new(source),
    }
}

fn entry(user: bool, source: &str) -> StreamEntry {
    if user {
        user_entry(source)
    } else {
        assistant_entry(source)
    }
}

fn model_spans(model: &StreamModel) -> Vec<&StreamSpan> {
    let mut spans = Vec::new();
    for line in model.committed_lines.iter().chain(&model.pending_lines) {
        spans.extend(&line.spans);
        if let Some(table) = &line.table {
            spans.extend(table.rows.iter().flatten().flatten());
        }
    }
    spans
}

#[test]
fn issue455_user_materialization_keeps_plain_source_lines_and_exact_targets() {
    let source = "请看 https://github.com/puzige/vega/issues/455，和 HTTP://localhost:8080/中文🙂?q=%20#部分。\n\n**literal** [label](https://target.example)\n";
    let StreamEntry::User { lines, copy } = user_entry(source) else {
        panic!("user entry")
    };
    let user_lines: Vec<_> = lines
        .iter()
        .filter(|line| matches!(line.kind, LineKind::UserLine { .. }))
        .collect();
    assert_eq!(user_lines.len(), 3);
    assert!(matches!(
        user_lines[0].kind,
        LineKind::UserLine {
            first: true,
            last: false
        }
    ));
    assert!(user_lines[1].spans.is_empty());
    assert!(matches!(
        user_lines[2].kind,
        LineKind::UserLine {
            first: false,
            last: true
        }
    ));
    let visible = user_lines
        .iter()
        .map(|line| spans_text(line))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, source.trim_end_matches('\n'));
    assert_eq!(copy.original_text(), source);
    let linked: Vec<_> = user_lines
        .iter()
        .flat_map(|line| &line.spans)
        .filter_map(|span| {
            assert!(matches!(span.style, SpanStyle::Plain | SpanStyle::Link));
            span.link.as_deref().inspect(|&target| {
                assert_eq!(span.text, target);
            })
        })
        .collect();
    assert_eq!(
        linked,
        vec![
            "https://github.com/puzige/vega/issues/455",
            "HTTP://localhost:8080/中文🙂?q=%20#部分",
            "https://target.example",
        ]
    );
}

#[test]
fn issue455_markdown_explicit_targets_override_bare_labels_and_code_stays_unlinked() {
    let source = "[https://visible.example](https://target.example) <https://angle.example> [**https://bold-label.example**](https://bold-target.example) **https://strong.example** *https://italic.example* `https://code.example`\n\n```rust\nlet url = \"https://fenced.example\";\n```\n\n    https://indented.example\n";
    let model = model(source);
    let spans = model_spans(&model);
    for (label, target, style) in [
        (
            "https://visible.example",
            "https://target.example",
            SpanStyle::Link,
        ),
        (
            "https://angle.example",
            "https://angle.example",
            SpanStyle::Link,
        ),
        (
            "https://bold-label.example",
            "https://bold-target.example",
            SpanStyle::Strong,
        ),
        (
            "https://strong.example",
            "https://strong.example",
            SpanStyle::Strong,
        ),
        (
            "https://italic.example",
            "https://italic.example",
            SpanStyle::Emphasis,
        ),
    ] {
        let span = spans.iter().find(|span| span.text == label).unwrap();
        assert_eq!(span.link.as_deref(), Some(target));
        assert_eq!(span.style, style);
    }
    let inline_code = spans
        .iter()
        .find(|span| span.text == "https://code.example")
        .unwrap();
    assert_eq!(inline_code.style, SpanStyle::Code);
    assert_eq!(inline_code.link, None);
    let code: Vec<_> = model
        .committed_lines
        .iter()
        .filter(|line| line.kind == LineKind::Code)
        .collect();
    assert!(
        code.iter()
            .any(|line| spans_text(line).contains("fenced.example"))
    );
    assert!(
        code.iter()
            .any(|line| spans_text(line).contains("indented.example"))
    );
    assert!(
        code.iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.link.is_none())
    );
}

#[test]
fn issue455_visible_html_fallback_and_explicit_disallowed_targets_keep_current_boundaries() {
    let model = model(
        "<div>https://html.example</div>\n\n[https://visible.example](javascript:alert) [mail](mailto:user@example.com)",
    );
    let spans = model_spans(&model);
    assert!(spans.iter().any(|span| span.text == "https://html.example"
        && span.link.as_deref() == Some("https://html.example")));
    let blocked = spans
        .iter()
        .find(|span| span.text == "https://visible.example")
        .unwrap();
    assert_eq!(blocked.link.as_deref(), Some("javascript:alert"));
    assert!(!links::allowed_link(blocked.link.as_deref().unwrap()));
    assert!(
        spans
            .iter()
            .any(|span| span.text == "mail"
                && span.link.as_deref() == Some("mailto:user@example.com"))
    );
}

#[test]
fn issue455_autolink_style_uses_existing_accent_underline_and_nested_fonts() {
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        let colors = palette.colors;
        let model =
            model("plain https://plain.example **https://bold.example** *https://italic.example*");
        for line in &model.committed_lines {
            let base = message_run_style(colors.text_primary);
            let runs = render_rows::block_text_runs(&line.spans, base.clone(), &colors);
            for (span, run) in line.spans.iter().zip(runs) {
                if span.link.is_none() {
                    assert_eq!(run.color, colors.text_primary.into());
                    assert!(run.underline.is_none());
                    continue;
                }
                assert_eq!(run.color, colors.accent.into());
                let underline = run.underline.unwrap();
                assert_eq!(underline.thickness, px(1.));
                assert_eq!(underline.color, Some(colors.accent.into()));
                assert!(!underline.wavy);
                match span.style {
                    SpanStyle::Strong => assert_eq!(run.font.weight, FontWeight::BOLD),
                    SpanStyle::Emphasis => assert_eq!(run.font.style, FontStyle::Italic),
                    _ => assert_eq!(run.font, base.font()),
                }
            }
        }
    }
}

#[test]
fn issue455_streaming_url_updates_only_mutable_tail_and_matches_finished_model() {
    let mut stream = MarkdownStream::new();
    let mut streamed = StreamModel::default();
    let counters = StreamCounters::default();
    let prefix = "Frozen https://frozen.example.\n\nTail ";
    let mut source = prefix.to_owned();
    stream.append(prefix);
    streamed.sync(&stream.snapshot(), &counters);
    let frozen = streamed.committed_lines.clone();
    assert!(!frozen.is_empty());
    for delta in [
        "ht",
        "tps",
        ":",
        "//",
        "stream.example",
        "/中",
        "文?q=%E4",
        "%B8%AD#部分",
    ] {
        source.push_str(delta);
        stream.append(delta);
        streamed.sync(&stream.snapshot(), &counters);
        assert_eq!(&streamed.committed_lines[..frozen.len()], frozen.as_slice());
        assert_eq!(
            counters.frozen_rematerializations.load(Ordering::Relaxed),
            0
        );
    }
    assert!(model_spans(&streamed).iter().any(|span| {
        span.link.as_deref() == Some("https://stream.example/中文?q=%E4%B8%AD#部分")
    }));
    let committed = counters.committed_materializations.load(Ordering::Relaxed);
    let pending = counters.pending_materializations.load(Ordering::Relaxed);
    assert!(!streamed.sync(&stream.snapshot(), &counters));
    assert_eq!(
        counters.committed_materializations.load(Ordering::Relaxed),
        committed
    );
    assert_eq!(
        counters.pending_materializations.load(Ordering::Relaxed),
        pending
    );
    stream.finish();
    streamed.sync(&stream.snapshot(), &counters);
    assert_eq!(streamed.committed_lines, model(&source).committed_lines);
    assert_eq!(
        counters.frozen_rematerializations.load(Ordering::Relaxed),
        0
    );
}

struct UserReadHarness {
    entry: StreamEntry,
    focus: FocusHandle,
}

impl Render for UserReadHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus)
            .child(gpui_kit::base::TextSelectionLayer)
            .child(render_entry(&self.entry, window, cx))
    }
}

fn open_user_read_harness(
    cx: &mut TestAppContext,
    source: &str,
) -> (Entity<UserReadHarness>, VisualTestContext) {
    init_link_ui(cx);
    init_permission_test(cx);
    let entry = user_entry(source);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|cx| UserReadHarness {
                entry,
                focus: cx.focus_handle(),
            })
        })
        .unwrap()
    });
    cx.run_until_parked();
    let root = window.root(cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(600.), px(1500.)));
    visual.update(|window, cx| {
        let focus = root.read(cx).focus.clone();
        focus.focus(window, cx);
    });
    activate_harness(&mut visual);
    (root, visual)
}

fn assert_user_renderer_links(cx: &mut TestAppContext, selectable: bool) {
    let _capture = LinkCapture::start();
    let source =
        "请看 https://one.example/中文🙂，还有 http://two.example:8080/a(b)。\n\n**literal**";
    let (copy, mut visual) = if selectable {
        let copy = MessageCopy::new(source);
        let (_, visual) = open_selection_harness(
            cx,
            StreamEntry::User {
                lines: user_message_lines(455, source),
                copy: copy.clone(),
            },
        );
        (copy, visual)
    } else {
        let (root, visual) = open_user_read_harness(cx, source);
        let copy = root.read_with(&visual, |root, _| {
            let StreamEntry::User { copy, .. } = &root.entry else {
                panic!("user entry")
            };
            copy.clone()
        });
        (copy, visual)
    };
    let bubble = visual.debug_bounds("user-message-bubble").unwrap();
    let before = (
        bubble,
        painted_link("https://one.example/中文🙂").layout.bounds(),
    );
    assert_eq!(visual.opened_url(), None);
    for target in ["https://one.example/中文🙂", "http://two.example:8080/a(b)"] {
        visual.simulate_click(link_point(target), Modifiers::default());
        finish_click(&mut visual);
        assert_eq!(visual.opened_url().as_deref(), Some(target));
    }
    assert_eq!(
        before,
        (
            visual.debug_bounds("user-message-bubble").unwrap(),
            painted_link("https://one.example/中文🙂").layout.bounds()
        )
    );
    assert_eq!(copy.original_text(), source);
    visual.update(|_, cx| copy.copy(cx));
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(source)
    );
}

#[gpui_kit::test]
fn issue455_read_user_renderer_keeps_geometry_source_copy_and_link_targets(
    cx: &mut TestAppContext,
) {
    assert_user_renderer_links(cx, false);
}

#[gpui_kit::test]
fn issue455_selectable_user_renderer_keeps_geometry_source_copy_and_link_targets(
    cx: &mut TestAppContext,
) {
    assert_user_renderer_links(cx, true);
}

fn assert_markdown_renderer_links(cx: &mut TestAppContext, selectable: bool) {
    let _capture = LinkCapture::start();
    let source = "## https://heading.example\n\nParagraph https://paragraph.example suffix\n\n- https://list.example\n\n> https://quote.example\n\n| Left | Right |\n|:---|---:|\n| https://left.example | https://right.example |\n\n[https://visible.example](https://target.example) <https://angle.example>";
    let mut visual = if selectable {
        open_selection_harness(cx, assistant_entry(source)).1
    } else {
        open_read_harness(cx, source).1
    };
    assert_eq!(visual.opened_url(), None);
    for target in [
        "https://heading.example",
        "https://paragraph.example",
        "https://list.example",
        "https://quote.example",
        "https://left.example",
        "https://right.example",
        "https://target.example",
        "https://angle.example",
    ] {
        visual.simulate_click(link_point(target), Modifiers::default());
        finish_click(&mut visual);
        assert_eq!(visual.opened_url().as_deref(), Some(target));
    }
}

#[gpui_kit::test]
fn issue455_read_markdown_renderer_opens_bare_links_in_every_container(cx: &mut TestAppContext) {
    assert_markdown_renderer_links(cx, false);
}

#[gpui_kit::test]
fn issue455_selectable_markdown_renderer_opens_bare_links_in_every_container(
    cx: &mut TestAppContext,
) {
    assert_markdown_renderer_links(cx, true);
}

#[gpui_kit::test]
fn issue455_user_and_markdown_drag_copy_preserve_url_bytes_without_opening(
    cx: &mut TestAppContext,
) {
    let capture = LinkCapture::start();
    let source = "prefix https://selection.example/中文🙂?q=%20#尾 suffix";
    let target = "https://selection.example/中文🙂?q=%20#尾";
    for user in [false, true] {
        capture.clear();
        let (_, mut visual) = open_selection_harness(cx, entry(user, source));
        let layout = layout_for(source);
        drag(
            &mut visual,
            text_point(&layout, source.find(target).unwrap(), false),
            text_point(&layout, source.find(" suffix").unwrap(), true),
        );
        finish_click(&mut visual);
        assert_eq!(visual.opened_url(), None);
        assert_eq!(
            visual.update(gpui_kit::base::TextSelection::selected_text),
            target
        );
        visual.simulate_keystrokes("cmd-c");
        assert_eq!(
            visual
                .read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some(target)
        );
    }
}

#[gpui_kit::test]
fn issue455_user_and_markdown_cancel_selection_and_stale_single_clicks(cx: &mut TestAppContext) {
    let capture = LinkCapture::start();
    let menus = MenuCapture::start(cx);
    let source = "https://first.example plain https://second.example";
    for user in [false, true] {
        for action in [
            "double",
            "triple",
            "shift",
            "right",
            "other-link",
            "return",
            "window-blur",
            "focus-out",
            "replace",
        ] {
            capture.clear();
            let (stream, mut visual) = open_selection_harness(cx, entry(user, source));
            let start = link_point("https://first.example");
            let other = link_point("https://second.example");
            match action {
                "double" | "triple" => {
                    visual.simulate_click(start, Modifiers::default());
                    visual.executor().advance_clock(Duration::from_millis(200));
                    visual.run_until_parked();
                    assert_eq!(visual.opened_url(), None);
                    let count = if action == "double" { 2 } else { 3 };
                    visual.simulate_event(MouseDownEvent {
                        position: start,
                        button: MouseButton::Left,
                        click_count: count,
                        modifiers: Modifiers::default(),
                        first_mouse: false,
                    });
                    visual.simulate_event(MouseUpEvent {
                        position: start,
                        button: MouseButton::Left,
                        click_count: count,
                        modifiers: Modifiers::default(),
                    });
                }
                "shift" => visual.simulate_click(
                    start,
                    Modifiers {
                        shift: true,
                        ..Modifiers::default()
                    },
                ),
                "right" => {
                    visual.simulate_mouse_down(start, MouseButton::Right, Modifiers::default());
                    visual.simulate_mouse_up(start, MouseButton::Right, Modifiers::default());
                }
                "other-link" => {
                    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
                    visual.simulate_mouse_up(other, MouseButton::Left, Modifiers::default());
                }
                "return" => {
                    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
                    visual.simulate_mouse_move(
                        other,
                        Some(MouseButton::Left),
                        Modifiers::default(),
                    );
                    visual.simulate_mouse_move(
                        start,
                        Some(MouseButton::Left),
                        Modifiers::default(),
                    );
                    visual.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
                }
                _ => {
                    visual.simulate_click(start, Modifiers::default());
                    match action {
                        "window-blur" => {
                            visual.deactivate_window();
                            activate_harness(&mut visual);
                        }
                        "focus-out" => {
                            let focus = visual.update(|_, cx| cx.focus_handle());
                            visual.update(|window, cx| focus.focus(window, cx));
                        }
                        _ => stream.update(&mut visual, |stream, cx| {
                            stream.entries[0] = entry(user, "https://replacement.example plain");
                            cx.notify();
                        }),
                    }
                    redraw(&mut visual);
                }
            }
            finish_click(&mut visual);
            assert_eq!(visual.opened_url(), None, "user={user}, action={action}");
            if action == "double" || action == "triple" {
                assert!(
                    !visual
                        .update(gpui_kit::base::TextSelection::selected_text)
                        .is_empty()
                );
            }
            if action == "right" {
                dismiss_context_menu(&mut visual);
                menus.close_fixture(&mut visual, cx);
            }
        }
    }
}

#[gpui_kit::test]
fn issue455_read_user_pending_click_cancels_on_blur_and_same_identity_text_replacement(
    cx: &mut TestAppContext,
) {
    let capture = LinkCapture::start();
    for replace in [false, true] {
        capture.clear();
        let (root, mut visual) = open_user_read_harness(cx, "https://stale.example");
        visual.simulate_click(link_point("https://stale.example"), Modifiers::default());
        if replace {
            root.update(&mut visual, |root, cx| {
                let StreamEntry::User { lines, copy } = &mut root.entry else {
                    panic!("user entry")
                };
                *lines = user_message_lines(455, "https://replacement.example");
                copy.replace_text("https://replacement.example");
                cx.notify();
            });
            redraw(&mut visual);
        } else {
            visual.deactivate_window();
            activate_harness(&mut visual);
        }
        finish_click(&mut visual);
        assert_eq!(visual.opened_url(), None);
    }
}

fn assert_wrapped_bare_url(cx: &mut TestAppContext, user: bool, selectable: bool) {
    let _capture = LinkCapture::start();
    let target = format!("https://wrapped.example/{}终🙂", "中文/segment/".repeat(12));
    let source = format!("prefix {target} suffix");
    let mut visual = if selectable {
        open_selection_harness(cx, entry(user, &source)).1
    } else if user {
        open_user_read_harness(cx, &source).1
    } else {
        open_read_harness(cx, &source).1
    };
    visual.simulate_resize(size(px(330.), px(1800.)));
    redraw(&mut visual);
    let painted = painted_link(&target);
    assert!(painted.regions.len() > 2);
    assert_eq!(painted.layout.text(), source);
    for index in [0, source.find("suffix").unwrap()] {
        visual.simulate_click(
            text_point(&painted.layout, index, false),
            Modifiers::default(),
        );
        finish_click(&mut visual);
        assert_eq!(visual.opened_url(), None);
    }
    let end = source.find(" suffix").unwrap();
    let (glyph_start, _) = source[..end].char_indices().last().unwrap();
    let line = painted.layout.line_layout_for_index(end).unwrap();
    let advance =
        line.unwrapped_layout.x_for_index(end) - line.unwrapped_layout.x_for_index(glyph_start);
    assert!(advance > px(0.));
    let endpoint = painted.layout.position_for_index(end).unwrap();
    let glyph = point(
        endpoint.x - advance / 2.,
        endpoint.y + painted.layout.line_height() / 2.,
    );
    visual.simulate_click(glyph, Modifiers::default());
    finish_click(&mut visual);
    assert_eq!(visual.opened_url().as_deref(), Some(target.as_str()));
}

#[gpui_kit::test]
fn issue455_wrapped_read_markdown_url_keeps_last_glyph_hits_and_plain_prefix_suffix(
    cx: &mut TestAppContext,
) {
    assert_wrapped_bare_url(cx, false, false);
}

#[gpui_kit::test]
fn issue455_wrapped_selectable_markdown_url_keeps_last_glyph_hits_and_plain_prefix_suffix(
    cx: &mut TestAppContext,
) {
    assert_wrapped_bare_url(cx, false, true);
}

#[gpui_kit::test]
fn issue455_wrapped_read_user_url_keeps_last_glyph_hits_and_plain_prefix_suffix(
    cx: &mut TestAppContext,
) {
    assert_wrapped_bare_url(cx, true, false);
}

#[gpui_kit::test]
fn issue455_wrapped_selectable_user_url_keeps_last_glyph_hits_and_plain_prefix_suffix(
    cx: &mut TestAppContext,
) {
    assert_wrapped_bare_url(cx, true, true);
}

#[gpui_kit::test]
fn issue455_user_link_hand_follows_ibeam_and_hover_does_not_open(cx: &mut TestAppContext) {
    let _capture = LinkCapture::start();
    let source = "plain https://cursor.example suffix";
    let (_, mut visual) = open_selection_harness(cx, user_entry(source));
    selection::CURSOR_REQUESTS.with_borrow_mut(|requests| *requests = Some(Vec::new()));
    let position = link_point("https://cursor.example");
    visual.simulate_mouse_move(position, None, Modifiers::default());
    redraw(&mut visual);
    let requests = selection::CURSOR_REQUESTS.with_borrow_mut(|requests| requests.take().unwrap());
    let hand = requests
        .iter()
        .rposition(|request| request.style == CursorStyle::PointingHand && request.hovered)
        .unwrap();
    assert!(
        requests[..hand]
            .iter()
            .any(|request| request.style == CursorStyle::IBeam
                && request.hovered
                && request.hitbox == requests[hand].hitbox
                && request.text == source)
    );
    finish_click(&mut visual);
    assert_eq!(visual.opened_url(), None);
}

#[gpui_kit::test]
async fn issue455_history_and_expanded_thinking_retain_automatic_targets(cx: &mut TestAppContext) {
    let _capture = LinkCapture::start();
    init_link_ui(cx);
    let (window, stream, _) = open_controller_stream(cx, "issue455-thinking");
    let source = "Thinking https://thinking.example/中文🙂 suffix";
    let card =
        stream.update(cx, |stream, cx| {
            stream.apply_history_page(
                hydration_page(
                    vec![
                        hydration_user(1, "User https://user-history.example"),
                        hydration_assistant(2, "History https://assistant-history.example"),
                    ],
                    None,
                ),
                cx,
            );
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
                1
            );
            for entry in &stream.entries {
                match entry {
                    StreamEntry::User { lines, .. } => {
                        assert!(lines.iter().flat_map(|line| &line.spans).any(|span| {
                            span.link.as_deref() == Some("https://user-history.example")
                        }))
                    }
                    StreamEntry::Assistant { model, .. } => {
                        assert!(model_spans(model).iter().any(|span| span.link.as_deref()
                            == Some("https://assistant-history.example")))
                    }
                    _ => {}
                }
            }
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "issue455-thinking".into(),
                    seq: 3,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ThinkingDelta {
                    message_id: "issue455-thinking".into(),
                    delta: source.into(),
                },
                cx,
            );
            let card = stream.active_thinking.clone().unwrap();
            stream.run_activity_groups["issue455-thinking"]
                .update(cx, |group, cx| group.reveal_segment(0, cx));
            card
        });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    activate_harness(&mut visual);
    assert!(card.read_with(&visual, |card, _| card.test_body_fragment_count()) > 0);
    visual.simulate_click(
        link_point("https://thinking.example/中文🙂"),
        Modifiers::default(),
    );
    finish_click(&mut visual);
    assert_eq!(
        visual.opened_url().as_deref(),
        Some("https://thinking.example/中文🙂")
    );
    assert_eq!(
        card.read_with(&visual, |card, _| card.test_source_copy().original_text()),
        source
    );
}
