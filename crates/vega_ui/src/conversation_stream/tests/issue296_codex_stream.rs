use super::*;

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "visual".into(),
            seq: 1,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::RunStarted {
            message_id: "visual".into(),
            started_at: Instant::now() - Duration::from_secs(12),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: "visual".into(),
            delta: "检查合成项目。\n\n记录测试结果。".into(),
        },
        cx,
    );
}

fn summary(stream: &mut ConversationStream, text: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::SummaryDelta {
            message_id: "visual".into(),
            delta: text.into(),
        },
        cx,
    );
}

fn click_stage(visual: &mut gpui_kit::VisualTestContext) {
    let stage = visual.debug_bounds("run-stage-toggle-0").unwrap();
    visual.simulate_click(stage.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
}

#[gpui_kit::test]
async fn summary_replaces_raw_source_across_multiple_stages(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "visual-sources");
    let (first, second) = stream.update(cx, |stream, cx| {
        begin(stream, cx);
        summary(stream, "First phase", cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("one", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "visual".into(),
                delta: "Original independent reasoning.".into(),
            },
            cx,
        );
        summary(stream, "Second phase\nFull summary continuation.", cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("two", "pwd"),
            },
            cx,
        );
        let cards = stream.run_activity_groups["visual"]
            .read(cx)
            .segment_children(0)
            .into_iter()
            .filter_map(|child| {
                if let RunActivityChild::Thinking(card) = child {
                    Some(card)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        (cards[0].clone(), cards[1].clone())
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("run-stage-source-{}", first.entity_id()).into_boxed_str(),
            ))
            .is_none()
    );
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("run-stage-source-{}", second.entity_id()).into_boxed_str(),
            ))
            .is_none()
    );
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    first.read_with(&visual, |card, _| {
        assert_eq!(card.source_text(), "First phase")
    });
    second.read_with(&visual, |card, _| {
        assert_eq!(
            card.source_text(),
            "Second phase\nFull summary continuation."
        )
    });
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("thinking-content").is_none());
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    stream.read_with(&visual, |stream, cx| {
        assert_eq!(
            stream.run_activity_groups["visual"]
                .read(cx)
                .stage_title(0, cx),
            "run commands"
        );
    });
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "visual".into(),
                delta: "Later raw reasoning.".into(),
            },
            cx,
        );
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        let group = stream.run_activity_groups["visual"].read(cx);
        assert_eq!(group.stage_title(0, cx), "run commands");
        assert_eq!(
            group
                .segment_children(0)
                .iter()
                .filter(|child| matches!(child, RunActivityChild::Thinking(_)))
                .count(),
            3
        );
    });
    assert!(visual.debug_bounds("thinking-toggle").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_none());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_none());
    click_stage(&mut visual);
    assert!(visual.debug_bounds("run-stage-tool-0-0").is_some());
    assert!(visual.debug_bounds("run-stage-tool-0-1").is_some());
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("run-stage-source-{}", second.entity_id()).into_boxed_str(),
            ))
            .is_some()
    );
}
