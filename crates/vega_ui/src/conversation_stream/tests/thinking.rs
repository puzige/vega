use super::*;

pub(super) fn thinking(id: &str, delta: &str) -> ConversationEvent {
    ConversationEvent::ThinkingDelta {
        message_id: id.into(),
        delta: delta.into(),
    }
}

fn summary(id: &str, delta: &str) -> ConversationEvent {
    ConversationEvent::SummaryDelta {
        message_id: id.into(),
        delta: delta.into(),
    }
}

pub(super) fn start(id: &str) -> ConversationEvent {
    ConversationEvent::MessageStarted {
        message_id: id.into(),
        seq: 2,
    }
}

pub(super) fn finish(id: &str) -> ConversationEvent {
    ConversationEvent::MessageFinished {
        message_id: id.into(),
        stop_reason: vega_conversation::types::ConversationStopReason::End,
        execution_duration_ms: None,
    }
}

pub(super) fn cards(stream: &ConversationStream, cx: &App) -> Vec<Entity<ThinkingBlock>> {
    stream
        .entries
        .iter()
        .flat_map(|entry| match entry {
            StreamEntry::RunActivitySegment { group, segment } => group
                .read(cx)
                .segment_children(*segment)
                .into_iter()
                .filter_map(|child| match child {
                    RunActivityChild::Thinking(card) => Some(card),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[gpui_kit::test]
async fn raw_reasoning_stage_keeps_preview_inside_manual_disclosure(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "raw-reasoning-stage");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "Reviewing files"), cx);
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, app| {
        let group = stream.run_activity_groups["m"].read(app);
        assert_eq!(group.stage_title(0, app), "Thinking");
        assert!(!group.segment_expanded(0));
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("thinking-content").is_none());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(thinking("m", "\n\nReading implementation"), cx)
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, app| {
        assert_eq!(
            stream.run_activity_groups["m"]
                .read(app)
                .stage_title(0, app),
            "Thinking"
        );
        assert_eq!(
            cards(stream, app)[0].read(app).title(),
            "Reading implementation"
        );
    });
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("thinking-content").is_some());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(thinking("m", "\n\nChecking result"), cx)
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, app| {
        let group = stream.run_activity_groups["m"].read(app);
        assert_eq!(group.stage_title(0, app), "Thinking");
        assert!(group.segment_expanded(0));
        assert_eq!(cards(stream, app)[0].read(app).title(), "Checking result");
    });
    assert!(visual.debug_bounds("thinking-content").is_some());
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(thinking("m", "\n\nLater result"), cx)
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    stream.read_with(&visual, |stream, app| {
        assert!(
            !stream.run_activity_groups["m"]
                .read(app)
                .segment_expanded(0)
        );
        assert_eq!(cards(stream, app)[0].read(app).title(), "Later result");
    });
}

#[gpui_kit::test]
async fn summary_takeover_releases_hidden_stage_reading_focus(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "summary-takeover-focus");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "Checking source"), cx);
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    visual.update(|_, cx| {
        let group = stream.read(cx).run_activity_groups["m"].clone();
        assert!(group.read(cx).segment_expanded(0));
    });
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(summary("m", "Checking result"), cx)
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-chevron-0").is_none());
    visual.update(|window, cx| {
        let group = stream.read(cx).run_activity_groups["m"].clone();
        assert!(group.read(cx).segment_header_focus(0).is_focused(window));
        assert!(
            group
                .read(cx)
                .test_visible_segment_thinking_texts(0, cx)
                .is_empty()
        );
    });
    stream.update(&mut visual, |stream, cx| {
        stream.resume_tail(cx);
        confirm_test_final(stream, "m", cx);
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert!(
            stream.run_activity_groups["m"]
                .read(cx)
                .process_in_details()
        );
    });
}

#[gpui_kit::test]
async fn summary_stage_nests_tools_without_duplicate_reasoning(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "summary-stage-tools");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(summary("m", "Reviewing issue"), cx);
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    stream.read_with(&visual, |stream, app| {
        let group = stream.run_activity_groups["m"].read(app);
        assert_eq!(group.stage_title(0, app), "Reviewing issue");
        assert!(!group.segment_expanded(0));
    });
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(stream.read_with(&visual, |stream, app| {
        !stream.run_activity_groups["m"]
            .read(app)
            .segment_expanded(0)
    }));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("summary-tool", "pwd"),
            },
            cx,
        );
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-chevron-0").is_some());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(summary("m", "Writing the fix"), cx);
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "summary-tool".into(),
                result: ToolResult {
                    status: ToolCallStatus::Rejected,
                    output: String::new(),
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: None,
                    invalid: None,
                },
            },
            cx,
        );
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, app| {
        let group = stream.run_activity_groups["m"].read(app);
        assert_eq!(group.stage_title(0, app), "Writing the fix");
        assert!(!group.segment_expanded(0));
        assert!(stream.run_notice_projection(app).is_empty());
        assert_eq!(
            group.test_visible_segment_thinking_texts(0, app),
            vec!["Reviewing issue".to_owned()]
        );
        let kinds = group
            .segment_children(0)
            .into_iter()
            .map(|child| match child {
                RunActivityChild::Thinking(_) => "thinking",
                RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_) => "tool",
                RunActivityChild::Artifact(_) => "artifact",
            })
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["thinking", "tool", "thinking"]);
    });
    assert!(visual.debug_bounds("run-stage-status-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
    assert!(visual.debug_bounds("thinking-toggle").is_some());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "m".into(),
                delta: "Answer begins.".into(),
            },
            cx,
        );
        stream.apply_event(summary("m", "Checking after answer"), cx);
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, app| {
        assert_eq!(
            stream.run_activity_groups["m"]
                .read(app)
                .stage_title(1, app),
            "Checking after answer"
        );
        assert!(matches!(stream.entries[2], StreamEntry::Assistant { .. }));
    });
}

#[gpui_kit::test]
async fn live_stage_title_switches_to_summary_without_revealing_raw_thinking(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "live-thinking-title");
    cx.update(|cx| {
        cx.set_reduce_motion(false);
        cx.set_global(shimmer::PaintProbe::default());
    });
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "## Aliyun server:"), cx);
    });
    open_run_details(window, cx);
    for (event, expected, stage_title) in [
        (summary("m", " \n## "), "Aliyun server:", "Thinking"),
        (
            thinking("m", "\nCache hit rate is unclear."),
            "Cache hit rate is unclear.",
            "Thinking",
        ),
        (summary("m", "Summary A"), "Summary A", "Summary A"),
        (
            thinking("m", " Further investigation"),
            "Summary A",
            "Summary A",
        ),
        (summary("m", "\nSummary B"), "Summary B", "Summary B"),
        (
            thinking("m", "\n\n```rust\n## fake heading\n```\n**"),
            "Summary B",
            "Summary B",
        ),
    ] {
        stream.update(cx, |stream, cx| stream.apply_event(event, cx));
        cx.run_until_parked();
        stream.read_with(cx, |stream, app| {
            let card = cards(stream, app)[0].read(app);
            assert_eq!(card.title(), expected);
            assert!(!card.expanded);
            assert_eq!(
                stream.run_activity_groups["m"]
                    .read(app)
                    .stage_title(0, app),
                stage_title
            );
        });
        assert!(
            gpui_kit::VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("thinking-content")
                .is_none()
        );
        assert!(
            gpui_kit::VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("run-stage-title-0")
                .is_some()
        );
        cx.update(|cx| cx.global::<shimmer::PaintProbe>().0.borrow_mut().clear());
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            let samples = cx.global::<shimmer::PaintProbe>().0.borrow();
            assert_eq!(
                samples
                    .iter()
                    .rev()
                    .find(|sample| sample.moving)
                    .map(|sample| sample.text.as_str()),
                Some(stage_title)
            );
        });
    }
    stream.read_with(cx, |stream, app| {
        let card = cards(stream, app)[0].read(app);
        assert_eq!(card.title(), "Summary B");
        assert!(card.text.contains("Further investigation"));
        assert_eq!(card.source_text(), " \n## Summary A\nSummary B");
        assert_eq!(card.test_source_copy().original_text(), card.source_text());
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let stage = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("stage disclosure");
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("thinking-content").is_none());
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-chevron-0").is_none());
    assert!(stream.read_with(&visual, |stream, app| {
        !stream.run_activity_groups["m"]
            .read(app)
            .segment_expanded(0)
    }));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(finish("m"), cx);
        stream.apply_event(thinking("m", "\nlate content"), cx);
    });
    stream.read_with(&visual, |stream, app| {
        let card = cards(stream, app)[0].read(app);
        assert_eq!(card.title(), "Summary B");
        assert_eq!(card.source_text(), " \n## Summary A\nSummary B");
    });
}

#[gpui_kit::test]
async fn summary_replacement_preserves_fallback_bounds_and_terminal(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "summary-priority");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "initial useful fragment"), cx);
    });
    assert_eq!(
        stream.read_with(cx, |stream, app| {
            cards(stream, app)[0].read(app).title().to_owned()
        }),
        "initial useful fragment"
    );
    stream.update(cx, |stream, cx| {
        stream.apply_event(summary("m", " \n"), cx);
        let card = cards(stream, cx)[0].read(cx);
        assert_eq!(card.title(), "initial useful fragment");
        assert_eq!(card.source_text(), "initial useful fragment");
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: "**".into(),
            },
            cx,
        );
        let card = cards(stream, cx)[0].read(cx);
        assert_eq!(card.title(), "initial useful fragment");
        assert_eq!(card.source_text(), " \n**");
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: "检查摘要**\nbody under heading".into(),
            },
            cx,
        );
        assert_eq!(cards(stream, cx)[0].read(cx).title(), "body under heading");
        stream.apply_event(thinking("m", "\n## raw now takes over"), cx);
        let card = cards(stream, cx)[0].read(cx);
        assert_eq!(card.title(), "body under heading");
        assert!(!card.source_text().contains("raw now takes over"));
        assert_eq!(card.test_source_copy().original_text(), card.source_text());
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: "\n```rust\n# not a heading\n```\n## 下个阶段\n正文".into(),
            },
            cx,
        );
        stream.apply_event(finish("m"), cx);
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "m".into(),
                delta: "\n## stale".into(),
            },
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, app| {
            cards(stream, app)[0].read(app).title().to_owned()
        }),
        "正文"
    );
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("next"), cx);
        stream.apply_event(thinking("next", &"中文 fragment ".repeat(150)), cx);
        stream.apply_event(thinking("next", "最新尾部"), cx);
        stream.apply_event(thinking("next", "\n\n**"), cx);
    });
    stream.read_with(cx, |stream, app| {
        let cards = cards(stream, app);
        let card = cards[1].read(app);
        assert!(card.title().chars().count() <= 96);
        assert!(card.title().ends_with("最新尾部"));
        assert_eq!(card.source_text(), card.text);
        assert_eq!(card.test_source_copy().original_text(), card.text);
    });
}
