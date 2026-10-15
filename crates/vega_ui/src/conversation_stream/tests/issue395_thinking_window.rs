use super::thinking::{cards, finish, start, thinking};
use super::*;
use crate::conversation_stream::thinking::{
    THINKING_BLOCK_BYTES, THINKING_VIEW_BLOCKS, THINKING_VIEW_BYTES,
};

#[gpui_kit::test]
fn issue395_folded_window_keeps_latest_utf8_source_and_title(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "thinking-sliding-folded");
    let mut received = String::new();
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("window"), cx);
        for index in 0..96 {
            let delta = format!("\n\n{}\nlatest-{index} 🦀", "界".repeat(2048));
            received.push_str(&delta);
            stream.apply_event(thinking("window", &delta), cx);
            let card = stream
                .active_thinking
                .as_ref()
                .expect("latest card")
                .read(cx);
            let source = card.source_text();
            assert!(source.len() <= THINKING_BLOCK_BYTES);
            assert!(received.ends_with(&source));
            assert!(source.ends_with(&format!("latest-{index} 🦀")));
            assert_eq!(card.title(), format!("latest-{index} 🦀"));
            assert_eq!(card.test_source_copy().original_text(), source);
            assert!(!card.test_body_preparing());
            assert_eq!(card.test_body_fragment_count(), 0);
        }
        assert!(
            stream
                .active_thinking
                .as_ref()
                .expect("card")
                .read(cx)
                .truncated
        );
    });
    let card = stream.update(cx, |stream, cx| {
        stream.append_reasoning(
            "window",
            &format!("\n```text\n{}", "body\n".repeat(THINKING_BLOCK_BYTES / 5)),
            false,
            cx,
        );
        stream.run_activity_groups["window"].update(cx, |group, cx| {
            group.reveal_segment(0, cx);
        });
        stream.active_thinking.clone().expect("visible card")
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("thinking-content").is_some());
    let revision = card.read_with(&visual, |card, _| {
        assert!(card.truncated);
        assert_eq!(card.title(), "latest-95 🦀");
        assert!(card.test_body_fragment_count() > 0);
        card.test_body_revision()
    });
    let changes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let observed = changes.clone();
    let _subscription = visual.update(|_, cx| {
        cx.subscribe(
            &card,
            move |_, change: &crate::conversation_stream::thinking::ThinkingChanged, _| {
                observed
                    .borrow_mut()
                    .push((change.header, change.stage, change.geometry));
            },
        )
    });
    stream.update(&mut visual, |stream, cx| {
        stream.append_reasoning("window", &"body\n".repeat(16384), false, cx);
    });
    card.read_with(&visual, |card, _| {
        assert!(card.truncated);
        assert_eq!(card.title(), "latest-95 🦀");
        assert_eq!(card.test_body_revision(), revision);
    });
    assert_eq!(changes.borrow().as_slice(), &[(true, false, false)]);
}

#[gpui_kit::test]
fn issue395_large_event_keeps_contiguous_tail_and_summary_can_replace_full_raw(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "thinking-sliding-large");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("window"), cx);
        let source = format!("{}raw newest 🦀", "界".repeat(THINKING_BLOCK_BYTES));
        stream.apply_event(thinking("window", &source), cx);
        let card = stream.active_thinking.clone().expect("card");
        let raw = card.read(cx).source_text();
        assert!(source.ends_with(&raw));
        assert!(raw.ends_with("raw newest 🦀"));
        assert!(raw.len() <= THINKING_BLOCK_BYTES);
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "window".into(),
                delta: "summary after full raw".into(),
            },
            cx,
        );
        assert_eq!(card.read(cx).source_text(), "summary after full raw");
        let summary = format!("{}summary newest 😀", "文".repeat(THINKING_BLOCK_BYTES));
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "window".into(),
                delta: summary.clone(),
            },
            cx,
        );
        let retained = card.read(cx).source_text();
        assert!(summary.ends_with(&retained));
        assert!(retained.ends_with("summary newest 😀"));
        assert_eq!(card.read(cx).test_source_copy().original_text(), retained);
        assert!(stream.thinking_bytes <= THINKING_BLOCK_BYTES);
        stream.apply_event(thinking("window", "later raw must not replace summary"), cx);
        assert_eq!(card.read(cx).source_text(), retained);
        assert!(stream.thinking_bytes <= THINKING_BLOCK_BYTES);
    });
}

#[gpui_kit::test]
fn issue395_whitespace_summary_cannot_leave_evicted_raw_in_copy(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "thinking-sliding-whitespace");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("window"), cx);
        stream.apply_event(
            thinking("window", &"raw ".repeat(THINKING_BLOCK_BYTES / 4)),
            cx,
        );
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "window".into(),
                delta: " ".into(),
            },
            cx,
        );
        let card = stream.active_thinking.as_ref().expect("card").read(cx);
        assert_eq!(card.source_text(), "");
        assert_eq!(card.test_source_copy().original_text(), "");
        assert!(!card.has_meaningful_source_text());
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "window".into(),
                delta: "latest summary".into(),
            },
            cx,
        );
        let card = stream.active_thinking.as_ref().expect("card").read(cx);
        assert_eq!(card.source_text(), " latest summary");
        assert_eq!(card.test_source_copy().original_text(), card.source_text());
    });
}

#[gpui_kit::test]
fn issue395_view_window_evicts_old_bodies_without_dropping_timeline(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "thinking-sliding-view");
    stream.update(cx, |stream, cx| {
        for index in 0..8 {
            let id = format!("window-{index}");
            stream.apply_event(start(&id), cx);
            let text = format!("{}latest-{index}", "x".repeat(THINKING_BLOCK_BYTES - 32));
            stream.apply_event(thinking(&id, &text), cx);
            stream.apply_event(finish(&id), cx);
            assert!(stream.thinking_bytes <= THINKING_VIEW_BYTES);
        }
        let cards = cards(stream, cx);
        assert_eq!(cards.len(), 8);
        let first = cards.first().expect("first card").read(cx);
        assert_eq!(first.source_text(), "");
        assert!(!first.has_meaningful_source_text());
        assert!(!first.has_source_text());
        assert_eq!(first.test_source_copy().visible_text(), None);
        assert!(!first.test_source_copy().has_selected_text());
        assert_eq!(first.test_body_fragment_count(), 0);
        assert!(
            cards
                .last()
                .expect("latest card")
                .read(cx)
                .source_text()
                .ends_with("latest-7")
        );
        assert_eq!(
            stream.thinking_bytes,
            cards
                .iter()
                .map(|card| card.read(cx).source_text().len())
                .sum::<usize>()
        );
    });
    let (_, stream, _) = open_controller_stream(cx, "thinking-sliding-count");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("window"), cx);
        for index in 0..THINKING_VIEW_BLOCKS + 5 {
            stream.apply_event(thinking("window", &format!("thinking-{index}")), cx);
            if index == 0 || index == THINKING_VIEW_BLOCKS + 3 {
                stream.apply_event(
                    ConversationEvent::ToolCallProposed {
                        call: bash_call(&format!("window-tool-{index}"), "pwd"),
                    },
                    cx,
                );
            }
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "window".into(),
                    delta: format!("answer-{index}"),
                },
                cx,
            );
        }
        let cards = cards(stream, cx);
        assert_eq!(cards.len(), THINKING_VIEW_BLOCKS + 5);
        assert_eq!(
            cards
                .iter()
                .filter(|card| card.read(cx).has_source_text())
                .count(),
            THINKING_VIEW_BLOCKS
        );
        assert_eq!(stream.thinking_blocks, THINKING_VIEW_BLOCKS);
        assert!(
            cards
                .iter()
                .take(5)
                .all(|card| !card.read(cx).has_meaningful_source_text())
        );
        assert_eq!(
            cards.last().expect("newest").read(cx).source_text(),
            format!("thinking-{}", THINKING_VIEW_BLOCKS + 4)
        );
        let order: Vec<_> = stream
            .entries
            .iter()
            .flat_map(|entry| match entry {
                StreamEntry::RunActivitySegment { group, segment } => group
                    .read(cx)
                    .segment_children(*segment)
                    .into_iter()
                    .map(|child| match child {
                        RunActivityChild::Thinking(_) => "thinking".to_owned(),
                        RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_) => {
                            "tool".to_owned()
                        }
                        RunActivityChild::Artifact(_) => "artifact".to_owned(),
                    })
                    .collect::<Vec<_>>(),
                StreamEntry::Assistant { copy, .. } if copy.has_text() => {
                    vec![copy.original_text()]
                }
                _ => Vec::new(),
            })
            .collect();
        let expected: Vec<_> = (0..THINKING_VIEW_BLOCKS + 5)
            .flat_map(|index| {
                let mut entries = vec!["thinking".to_owned()];
                if index == 0 || index == THINKING_VIEW_BLOCKS + 3 {
                    entries.push("tool".into());
                }
                entries.push(format!("answer-{index}"));
                entries
            })
            .collect();
        assert_eq!(order, expected);
        let answers: Vec<_> = stream
            .entries
            .iter()
            .filter_map(|entry| match entry {
                StreamEntry::Assistant { copy, .. } if copy.has_text() => {
                    Some(copy.original_text())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            answers,
            (0..THINKING_VIEW_BLOCKS + 5)
                .map(|index| format!("answer-{index}"))
                .collect::<Vec<_>>()
        );
    });
}
