use super::quote_behavior_tests::{Path, continuous, drag_selection, frame, open, text_bounds};
use super::*;
use gpui_kit::{InputEvent, TestAppContext, point};

const PATHS: [Path; 3] = [Path::Ordinary, Path::Selectable, Path::Thinking];

fn quoted(source: &str, quote: bool) -> String {
    if quote {
        source.lines().map(|line| format!("> {line}\n")).collect()
    } else {
        source.to_owned()
    }
}

fn first_list(nodes: &[RenderNode]) -> Option<&vega_markdown::ListBlock> {
    nodes.iter().find_map(|node| match node {
        RenderNode::List(list) => Some(list),
        RenderNode::BlockQuote { children } => first_list(children),
        _ => None,
    })
}

fn verify_parser(source: &str, code: bool) {
    let mut stream = MarkdownStream::new();
    stream.append(source);
    stream.finish();
    let snapshot = stream.snapshot();
    let list = snapshot
        .blocks
        .iter()
        .find_map(|block| first_list(block.nodes))
        .or_else(|| {
            snapshot
                .pending
                .and_then(|pending| first_list(pending.nodes))
        })
        .expect("fixture must parse as a list");
    let item = list.items.first().expect("fixture list item");
    assert_eq!(item.checked, None);
    if code {
        assert!(matches!(
            item.children.first(),
            Some(RenderNode::CodeBlock { .. })
        ));
    } else {
        assert!(matches!(
            item.children.first(),
            Some(RenderNode::Heading { level: 1, .. })
        ));
    }
}

#[gpui_kit::test]
fn heading_first_list_items_keep_visible_marker_and_heading_style(cx: &mut TestAppContext) {
    for quote in [false, true] {
        let source = quoted("before\n\n- # heading-first\n\nafter", quote);
        verify_parser(&source, false);
        for path in PATHS {
            let handle = open(cx, path, &source, 600.);
            let painted = frame(handle, cx);
            let marker = text_bounds(&painted, "•");
            let heading = painted
                .text
                .iter()
                .find(|run| run.text == "heading-first")
                .unwrap();
            assert!(marker.left() < heading.layout.bounds().left(), "{path:?}");
            let shaped = heading.layout.line_layout_for_index(0).unwrap();
            assert_eq!(shaped.font_size(), px(Typography::MARKDOWN_H1), "{path:?}");
            let weight = heading.runs.first().unwrap().font.weight;
            assert_eq!(weight, Typography::HEADING_PAGE_WEIGHT, "{path:?}");
            assert_eq!(
                heading.runs.iter().map(|run| run.len).sum::<usize>(),
                heading.text.len(),
                "{path:?}"
            );
            assert!(
                heading.runs.iter().all(|run| run.font.weight == weight),
                "{path:?}"
            );
            let body = painted
                .text
                .iter()
                .find(|run| run.text == "before")
                .unwrap();
            assert_eq!(
                body.layout.line_layout_for_index(0).unwrap().font_size(),
                px(Typography::CONVERSATION),
                "{path:?}"
            );
            let body_weight = body.runs.first().unwrap().font.weight;
            assert_eq!(body_weight, FontWeight::NORMAL, "{path:?}");
            assert_ne!(body_weight, weight, "{path:?}");
            if quote {
                continuous(&painted, "before", "after");
            }
        }
    }
}

#[gpui_kit::test]
fn code_first_list_items_keep_marker_physical_lines_and_horizontal_scroll(cx: &mut TestAppContext) {
    let long = format!("HEAD{}TAIL", "abcdefghijklmnopqrstuvwxyz".repeat(14));
    for quote in [false, true] {
        let source = quoted(
            &format!("before\n\n- ```text\n  {long}\n  second-line\n  ```\n\nafter"),
            quote,
        );
        verify_parser(&source, true);
        for path in PATHS {
            let handle = open(cx, path, &source, 260.);
            let before = frame(handle, cx);
            let marker = text_bounds(&before, "•");
            let code = before
                .text
                .iter()
                .find(|run| run.text.contains("HEAD"))
                .unwrap();
            assert!(marker.left() < code.layout.bounds().left(), "{path:?}");
            assert_eq!(code.layout.line_layouts().len(), 1, "{path:?}");
            assert_eq!(
                code.layout.line_layout_for_index(0).unwrap().font_size(),
                px(Typography::CODE)
            );
            let second = text_bounds(&before, "second-line");
            assert!(second.top() >= code.layout.bounds().bottom() - px(0.2));
            let tail_x = code
                .layout
                .position_for_index(code.text.find("TAIL").unwrap())
                .unwrap()
                .x;
            assert!(tail_x > px(360.));
            let position = point(px(130.), code.layout.bounds().center().y);
            cx.update_window(handle.into(), |_, window, cx| {
                window.dispatch_event(
                    gpui_kit::MouseMoveEvent {
                        position,
                        ..Default::default()
                    }
                    .to_platform_input(),
                    cx,
                );
                window.dispatch_event(
                    gpui_kit::ScrollWheelEvent {
                        position,
                        delta: gpui_kit::ScrollDelta::Pixels(point(-(tail_x - px(180.)), px(0.))),
                        ..Default::default()
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
            let after = frame(handle, cx);
            let tail = after
                .text
                .iter()
                .find(|run| run.text.contains("TAIL"))
                .unwrap();
            let moved = tail
                .layout
                .position_for_index(tail.text.find("TAIL").unwrap())
                .unwrap()
                .x;
            assert!(moved >= px(0.) && moved < px(260.), "{path:?}: {moved:?}");
            assert!(moved < tail_x - px(100.));
            assert_eq!(
                text_bounds(&after, "before"),
                text_bounds(&before, "before")
            );
            assert_eq!(text_bounds(&after, "after"), text_bounds(&before, "after"));
            assert_eq!(text_bounds(&after, "•"), marker);
            if quote {
                continuous(&after, "before", "after");
            }
        }
    }
}

#[gpui_kit::test]
fn nonparagraph_first_list_items_copy_markers_in_visible_order(cx: &mut TestAppContext) {
    for (source, expected, code) in [
        (
            "before\n\n- # heading-first\n\nafter",
            "before\n\n• heading-first\n\nafter",
            false,
        ),
        (
            "before\n\n- ```text\n  code-first\n  second-line\n  ```\n\nafter",
            "before\n\n• code-first\nsecond-line\n\nafter",
            true,
        ),
    ] {
        for quote in [false, true] {
            let source = quoted(source, quote);
            verify_parser(&source, code);
            for path in [Path::Selectable, Path::Thinking] {
                let handle = open(cx, path, &source, 600.);
                let painted = frame(handle, cx);
                let first = painted
                    .text
                    .iter()
                    .find(|run| run.text == "before")
                    .unwrap();
                let last = painted.text.iter().find(|run| run.text == "after").unwrap();
                let from = first.layout.position_for_index(0).unwrap() + point(px(0.1), px(3.));
                let to = last.layout.position_for_index(last.text.len()).unwrap()
                    + point(px(0.1), px(3.));
                let copy = handle.update(cx, |view, _, _| view.copy.clone()).unwrap();
                drag_selection(handle, cx, from, to);
                cx.update(|cx| copy.copy_selected_text(cx));
                assert_eq!(
                    cx.read_from_clipboard()
                        .and_then(|item| item.text())
                        .as_deref(),
                    Some(expected),
                    "{path:?} quote={quote}"
                );
            }
        }
    }
}
