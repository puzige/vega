//! #151 R151-1..R151-7: the newest live activity unit (thinking block, single
//! tool row, or tool group) opens by default, and the previous current unit
//! steps down as soon as newer content arrives. Every assertion here runs on
//! the mounted production stream (`ConversationStream` + real render path).

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

/// Expansion of every activity unit in timeline order. The length is the
/// number of activity units, so "at most one `true`" is exactly R151-2.
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

/// R151-1 + R151-2 + R151-4: thinking → single tool → group → text, with the
/// current unit opening each time and the superseded one stepping down.
#[gpui_kit::test]
async fn issue151_current_unit_opens_and_the_superseded_unit_steps_down(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "issue151-current-unit");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "第一段推理"), cx);
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [true],
        "R151-1: the first activity unit (no predecessor) opens by default"
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
        [false, true],
        "R151-2: the thinking block steps down, the newer tool row opens"
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
        [false, true],
        "R151-3: the single row upgrades into the current expanded group"
    );

    stream.update(cx, |stream, cx| {
        stream.apply_event(text("m", "开始执行"), cx);
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [false, false],
        "R151-4: the first nonempty text delta of a new segment steps the unit down"
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

/// R151-2: only the *newest* unit steps down. A manually opened older unit is
/// not the current unit and keeps its expansion.
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
    assert_eq!(stream.read_with(cx, activity_expansion), [false, true]);

    // The user reopens the older thinking block by hand.
    click(window, "thinking-toggle", cx);
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [true, true],
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
        [true, true],
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
}

/// R151-3: joining an existing group keeps the group expanded and never opens
/// a child's own bounded detail.
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
    stream.read_with(cx, |stream, cx| {
        let group = group(stream, cx);
        let group = group.read(cx);
        assert!(group.expanded(), "the current group opens by default");
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

/// R151-4: only the *first* nonempty delta of a text segment pays the
/// step-down scan, so a manual expansion made later in the same segment is not
/// fought by every token; the next segment steps the unit down again.
#[gpui_kit::test]
async fn issue151_text_segment_steps_down_once_per_segment(cx: &mut TestAppContext) {
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

    click(window, "thinking-toggle", cx);
    assert_eq!(stream.read_with(cx, activity_expansion), [true]);

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
        [false],
        "a new segment (new message) is newer content and steps the unit down"
    );
}

/// R151-5: the stream never re-expands a unit the user collapsed; only the
/// creation of a *newer* unit opens anything.
#[gpui_kit::test]
async fn issue151_user_collapsed_unit_is_never_re_expanded(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue151-manual-collapse");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start("m"), cx);
        stream.apply_event(thinking("m", "第一段"), cx);
    });
    assert!(mounted(window, "thinking-content", cx));

    click(window, "thinking-toggle", cx);
    assert_eq!(stream.read_with(cx, activity_expansion), [false]);
    assert!(!mounted(window, "thinking-content", cx));

    stream.update(cx, |stream, cx| {
        stream.apply_event(thinking("m", "第二段"), cx);
        stream.apply_event(thinking("m", "第三段"), cx);
    });
    assert_eq!(
        stream.read_with(cx, activity_expansion),
        [false],
        "more deltas of the same unit must not re-open a user-collapsed unit"
    );
    assert!(!mounted(window, "thinking-content", cx));
}

/// R151-6: hydration and route reopen start collapsed — there is no current
/// activity in a restored transcript.
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

    // Live control: the same two adjacent calls form the current unit and open.
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
        assert!(group(stream, cx).read(cx).expanded());
        assert_eq!(group(stream, cx).read(cx).row_count(cx), 3);
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

/// R151-7: expansion re-measures only the owning item. The entry above keeps
/// its exact measured bounds while the disclosed detail mounts and unmounts.
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
    assert_eq!(stream.read_with(cx, activity_expansion), [true]);
    assert!(mounted(window, "tool-activity-single-row-detail", cx));
    let before = gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("assistant-message")
        .expect("assistant item above the tool");

    click(window, "tool-activity-single-row", cx);
    assert_eq!(stream.read_with(cx, activity_expansion), [false]);
    assert!(!mounted(window, "tool-activity-single-row-detail", cx));

    let after = gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("assistant-message")
        .expect("assistant item above the tool");
    for (axis, (before, after)) in [
        ("top", (before.origin.y, after.origin.y)),
        ("left", (before.origin.x, after.origin.x)),
        ("height", (before.size.height, after.size.height)),
    ] {
        assert!(
            (f32::from(before) - f32::from(after)).abs() <= 0.2,
            "the item above must not be re-measured or re-laid-out ({axis})"
        );
    }
}
