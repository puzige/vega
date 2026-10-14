use super::*;

fn start(id: &str) -> ConversationEvent {
    ConversationEvent::MessageStarted {
        message_id: id.into(),
        seq: 2,
    }
}

fn thinking(id: &str, delta: &str) -> ConversationEvent {
    ConversationEvent::ThinkingDelta {
        message_id: id.into(),
        delta: delta.into(),
    }
}

fn text(id: &str, delta: &str) -> ConversationEvent {
    ConversationEvent::TextDelta {
        message_id: id.into(),
        delta: delta.into(),
    }
}

fn finish_message(id: &str) -> ConversationEvent {
    ConversationEvent::MessageFinished {
        message_id: id.into(),
        stop_reason: vega_conversation::types::ConversationStopReason::End,
        execution_duration_ms: None,
    }
}

fn read_call(id: &str, tool: &str, raw_input: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        tool: tool.into(),
        input_json: raw_input.into(),
    }
}

fn group(stream: &ConversationStream, cx: &App) -> Entity<ToolActivityGroup> {
    stream
        .entries
        .iter()
        .rev()
        .find_map(|entry| match entry {
            StreamEntry::ToolGroup { group } => Some(group.clone()),
            StreamEntry::RunActivitySegment { group, segment } => group
                .read(cx)
                .segment_children(*segment)
                .into_iter()
                .find_map(|child| match child {
                    RunActivityChild::ToolGroup(group) => Some(group),
                    _ => None,
                }),
            _ => None,
        })
        .expect("tool activity group")
}

fn activity_expansion(stream: &ConversationStream, cx: &App) -> Vec<bool> {
    stream
        .entries
        .iter()
        .flat_map(|entry| match entry {
            StreamEntry::RunActivitySegment { group, segment } => group
                .read(cx)
                .segment_children(*segment)
                .into_iter()
                .filter_map(|child| match child {
                    RunActivityChild::Thinking(card) => Some(card.read(cx).expanded),
                    RunActivityChild::Tool(card) => Some(card.read(cx).is_expanded()),
                    RunActivityChild::ToolGroup(group) => Some(group.read(cx).expanded()),
                    RunActivityChild::Artifact(_) => None,
                })
                .collect::<Vec<_>>(),
            StreamEntry::Tool { card } => vec![card.read(cx).is_expanded()],
            StreamEntry::ToolGroup { group } => vec![group.read(cx).expanded()],
            _ => Vec::new(),
        })
        .collect()
}

fn thinking_sources(stream: &ConversationStream, cx: &App) -> Vec<String> {
    stream
        .entries
        .iter()
        .flat_map(|entry| match entry {
            StreamEntry::RunActivitySegment { group, segment } => {
                group.read(cx).segment_children(*segment)
            }
            _ => Vec::new(),
        })
        .filter_map(|child| match child {
            RunActivityChild::Thinking(card) => Some(card.read(cx).source_text()),
            _ => None,
        })
        .collect()
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let target = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing clickable selector {selector}"));
    visual.simulate_click(target.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
}

fn mounted(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    cx.run_until_parked();
    gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .is_some()
}

fn history_tool(seq: i64, id: &str, tool: ReadOnlyToolKind) -> HistoryEntry {
    HistoryEntry::Tool {
        seq,
        message_id: "hydrated-message".into(),
        call_id: id.into(),
        status: ToolCallStatus::Success,
        approval: Some(Approval::Once),
        input: Some(ToolCardInputProjection::ReadOnly {
            tool,
            permission_path: None,
        }),
        result: Some(ToolCardResultProjection::ReadOnly {
            status: ToolCallStatus::Success,
            output: String::new(),
            reused: false,
        }),
    }
}

#[gpui_kit::test]
async fn issue151_v3_activity_units_default_collapsed(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "issue151-current-unit");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "第一段推理"), cx);
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [false],
        "v3 thinking details default collapsed"
    );

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("first", "pwd"),
            },
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [false, false],
        "v3 thinking and tool details default collapsed"
    );

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("second", "pwd"),
            },
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [false, false],
        "v3 grouped tools default collapsed"
    );

    stream.update(cx, |stream, cx| {
        stream.apply_event(text("m", "开始执行"), cx);
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [false, false],
        "v3 body streaming leaves detail disclosure unchanged"
    );
    assert!(
        stream.read_with(cx, |stream, cx| activity_expansion(stream, cx)
            .iter()
            .filter(|expanded| **expanded)
            .count())
            <= 1,
        "R151-2: never more than one auto-expanded unit"
    );
}

#[gpui_kit::test]
async fn issue151_manual_expansion_of_an_older_unit_survives_newer_content(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue151-older-unit");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "推理"), cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("first", "pwd"),
            },
            cx,
        );
    });
    assert_eq!(stream.read_with(cx, activity_expansion), [false, false]);

    open_run_details(window, cx);
    click(window, "run-stage-toggle-0", cx);
    assert_eq!(stream.read_with(cx, activity_expansion), [false, false]);
    assert!(!mounted(window, "thinking-content", cx));
    click(window, "thinking-toggle", cx);
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [true, false],
        "manual expansion of an older unit is allowed alongside the current one"
    );

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("second", "pwd"),
            },
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [true, false],
        "the superseded tool row steps down while the manual older expansion stays"
    );
    assert!(
        stream.read_with(cx, |stream, cx| {
            let group = group(stream, cx);
            let group = group.read(cx);
            !group
                .children()
                .iter()
                .any(|child| child.read(cx).is_expanded())
        }),
        "R151-3: an upgraded group never opens a child's own detail"
    );
    assert!(mounted(window, "thinking-content", cx));
    assert_eq!(stream.read_with(cx, thinking_sources), ["推理"]);
}

#[gpui_kit::test]
async fn issue151_group_join_keeps_the_group_expanded_with_compact_children(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "issue151-group-join");
    stream.update(cx, |stream, cx| {
        for id in ["a", "b"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "pwd"),
                },
                cx,
            );
        }
    });
    stream.update(cx, |stream, cx| {
        group(stream, cx).update(cx, |group, cx| group.set_expanded(true, cx));
    });
    stream.read_with(cx, |stream, cx| {
        let group = group(stream, cx);
        let group = group.read(cx);
        assert!(group.expanded(), "manual expansion remains open");
        assert_eq!(group.len(), 2);
        assert_eq!(
            group.row_count(cx),
            3,
            "aggregate plus two compact children"
        );
        assert!(
            !group
                .children()
                .iter()
                .any(|child| child.read(cx).is_expanded()),
            "no child carries its own detail inside an auto-expanded group"
        );
    });

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("c", "pwd"),
            },
            cx,
        );
    });
    stream.read_with(cx, |stream, cx| {
        let group = group(stream, cx);
        let group = group.read(cx);
        assert!(group.expanded(), "the group is still the newest unit");
        assert_eq!(group.len(), 3);
        assert_eq!(group.row_count(cx), 4);
        assert!(
            !group
                .children()
                .iter()
                .any(|child| child.read(cx).is_expanded()),
            "appending must not change any child's disclosure state"
        );
    });
}

#[gpui_kit::test]
async fn issue151_v3_manual_expansion_survives_text_segments(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue151-text-segment");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "推理"), cx);
        stream.apply_event(text("m", "第一段"), cx);
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [false],
        "the text segment supersedes the thinking block"
    );

    open_run_details(window, cx);
    click(window, "run-stage-toggle-0", cx);
    assert_eq!(stream.read_with(cx, activity_expansion), [true]);
    assert!(mounted(window, "thinking-content", cx));

    stream.update(cx, |stream, cx| {
        stream.apply_event(text("m", "同段继续"), cx);
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [true],
        "later deltas of the same segment do not re-run the step-down"
    );

    stream.update(cx, |stream, cx| {
        stream.apply_event(finish_message("m"), cx);
        stream.apply_event(start("n"), cx);
        stream.apply_event(text("n", "新消息正文"), cx);
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [true],
        "a new message preserves user expansion of prior details"
    );
    assert!(mounted(window, "thinking-content", cx));
    assert_eq!(stream.read_with(cx, thinking_sources), ["推理"]);
}

#[gpui_kit::test]
async fn issue151_user_collapsed_unit_is_never_re_expanded(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue151-manual-collapse");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "第一段"), cx);
    });
    assert!(!mounted(window, "thinking-content", cx));
    open_run_details(window, cx);
    click(window, "run-stage-toggle-0", cx);
    assert_eq!(stream.read_with(cx, activity_expansion), [true]);
    assert!(mounted(window, "thinking-content", cx));

    click(window, "run-stage-toggle-0", cx);
    assert_eq!(stream.read_with(cx, activity_expansion), [false]);
    assert!(!mounted(window, "thinking-content", cx));
    assert!(!mounted(window, "thinking-toggle", cx));

    stream.update(cx, |stream, cx| {
        stream.apply_event(thinking("m", "第二段"), cx);
        stream.apply_event(thinking("m", "第三段"), cx);
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [false],
        "more deltas of the same unit must not re-open a user-collapsed unit"
    );
    assert!(!mounted(window, "thinking-toggle", cx));
    click(window, "run-stage-toggle-0", cx);
    assert!(!mounted(window, "thinking-toggle", cx));
    assert!(mounted(window, "thinking-content", cx));
    assert_eq!(stream.read_with(cx, activity_expansion), [true]);
    assert_eq!(
        stream.read_with(cx, thinking_sources),
        ["第一段第二段第三段"]
    );
}

#[gpui_kit::test]
async fn issue151_hydration_and_reopen_stay_collapsed(cx: &mut TestAppContext) {
    let page = || {
        hydration_page(
            vec![
                hydration_assistant(1, "before"),
                history_tool(2, "hydrated-read", ReadOnlyToolKind::Read),
                history_tool(3, "hydrated-glob", ReadOnlyToolKind::Glob),
            ],
            None,
        )
    };

    let (_live_window, live, _) = open_controller_stream(cx, "issue151-hydration-live");
    live.update(cx, |stream, cx| {
        for id in ["live-read", "live-glob"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: read_call(id, "read", "{}"),
                },
                cx,
            );
        }
    });
    live.read_with(cx, |stream, cx| {
        assert!(!group(stream, cx).read(cx).expanded());
        assert_eq!(group(stream, cx).read(cx).row_count(cx), 1);
    });

    for thread_id in ["issue151-hydrated", "issue151-reopened"] {
        let (_window, hydrated, _) = open_controller_stream(cx, thread_id);
        hydrated.update(cx, |stream, cx| stream.apply_history_page(page(), cx));
        hydrated.read_with(cx, |stream, cx| {
            assert!(
                !group(stream, cx).read(cx).expanded(),
                "R151-6: a hydrated group is not the current activity unit"
            );
            assert_eq!(group(stream, cx).read(cx).row_count(cx), 1);
        });
    }
}

#[gpui_kit::test]
async fn issue151_expansion_remeasures_only_its_own_item(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue151-own-item");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(text("m", "正文先行"), cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("shell", "printf ok"),
            },
            cx,
        );
    });
    assert_eq!(stream.read_with(cx, activity_expansion), [false]);
    open_run_details(window, cx);
    click(window, "tool-activity-single-row", cx);
    assert_eq!(stream.read_with(cx, activity_expansion), [true]);
    assert!(mounted(window, "tool-activity-single-row-detail", cx));
    let root_before = gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("run-activity-group")
        .expect("root");
    let before = gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("assistant-message")
        .expect("assistant item below the process");

    click(window, "tool-activity-single-row", cx);
    assert_eq!(stream.read_with(cx, activity_expansion), [false]);
    assert!(!mounted(window, "tool-activity-single-row-detail", cx));

    let after = gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("assistant-message")
        .expect("assistant item below the process");
    let root_after = gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("run-activity-group")
        .expect("root");
    assert!(
        ((before.top() - after.top()) - (root_before.size.height - root_after.size.height)).abs()
            <= px(0.2)
    );
    for (axis, (before, after)) in [
        ("left", (before.origin.x, after.origin.x)),
        ("height", (before.size.height, after.size.height)),
    ] {
        assert!(
            (f32::from(before) - f32::from(after)).abs() <= 0.2,
            "the external body must retain its horizontal geometry and height ({axis})"
        );
    }
}
