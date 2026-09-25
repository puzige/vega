use super::*;
use crate::conversation_stream::run_activity::format_run_duration;

fn run_group(stream: &ConversationStream, message_id: &str) -> Entity<RunActivityGroup> {
    stream
        .run_activity_groups
        .get(message_id)
        .cloned()
        .expect("run activity group")
}

fn assistant_text(stream: &ConversationStream, _cx: &App) -> String {
    stream
        .entries
        .iter()
        .filter_map(|entry| match entry {
            StreamEntry::Assistant { model, .. } => Some(
                model
                    .committed_lines
                    .iter()
                    .chain(model.pending_lines.iter())
                    .flat_map(|line| line.spans.iter())
                    .map(|span| span.text.as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn issue146_duration_display_rounds_up_and_changes_units_at_thresholds() {
    assert_eq!(format_run_duration(0), "1 秒");
    assert_eq!(format_run_duration(1), "1 秒");
    assert_eq!(format_run_duration(1_000), "1 秒");
    assert_eq!(format_run_duration(1_001), "2 秒");
    assert_eq!(format_run_duration(59_001), "1 分 00 秒");
    assert_eq!(format_run_duration(3_599_001), "1 小时 0 分");
    assert_eq!(format_run_duration(3_723_000), "1 小时 2 分");
}

#[gpui_kit::test]
async fn issue146_live_activity_folds_once_and_keeps_final_answer_visible(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "issue146-live");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "live".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "live".into(),
                delta: "first thought".into(),
            },
            cx,
        );
    });
    let group = stream.read_with(cx, |stream, cx| {
        let group = run_group(stream, "live");
        assert_eq!(
            group.read(cx).test_projection(),
            (RunActivityStatus::Running, None, true, false)
        );
        group
    });

    group.update(cx, |group, cx| group.toggle_expanded(cx));
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "live".into(),
                delta: " continued".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("issue146-tool", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live".into(),
                delta: "final answer".into(),
            },
            cx,
        );
    });
    assert!(!group.read_with(cx, |group, _| group.test_projection().2));

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "live".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: Some(10_001),
            },
            cx,
        );
    });
    assert_eq!(
        group.read_with(cx, |group, _| group.test_projection()),
        (RunActivityStatus::Completed, Some(10_001), false, true)
    );
    assert_eq!(
        group.read_with(cx, |group, _| group.test_label()),
        "已完成 · 用时 11 秒"
    );
    assert_eq!(stream.read_with(cx, assistant_text), "final answer");

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("live".into()),
                error: Arc::new(std::io::Error::other("late terminal").into()),
                execution_duration_ms: Some(90_000),
            },
            cx,
        );
    });
    assert_eq!(
        group.read_with(cx, |group, _| group.test_projection()),
        (RunActivityStatus::Completed, Some(10_001), false, true)
    );

    group.update(cx, |group, cx| group.toggle_expanded(cx));
    assert!(group.read_with(cx, |group, cx| group.row_count(cx)) > 1);
    assert_eq!(stream.read_with(cx, assistant_text), "final answer");
}

#[gpui_kit::test]
async fn issue146_text_only_run_has_no_empty_live_row_and_shows_total_duration(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "issue146-text-only-duration");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "text-only".into(),
                seq: 2,
            },
            cx,
        );
        assert_eq!(stream.entries.len(), 1);
        assert!(matches!(
            stream.entries.first(),
            Some(StreamEntry::RunActivity { .. })
        ));
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "text-only".into(),
                delta: "visible answer".into(),
            },
            cx,
        );
        assert_eq!(stream.entries.len(), 2);
        assert!(matches!(
            stream.entries.get(1),
            Some(StreamEntry::Assistant { .. })
        ));
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "text-only".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: Some(6_543),
            },
            cx,
        );
    });

    stream.read_with(cx, |stream, cx| {
        assert_eq!(
            stream
                .entries
                .iter()
                .map(|entry| match entry {
                    StreamEntry::RunActivity { .. } => "run-activity",
                    StreamEntry::Assistant { .. } => "text",
                    _ => "unexpected",
                })
                .collect::<Vec<_>>(),
            ["run-activity", "text"]
        );
        let group = run_group(stream, "text-only");
        assert_eq!(
            group.read(cx).test_projection(),
            (RunActivityStatus::Completed, Some(6_543), false, true)
        );
        assert_eq!(group.read(cx).test_label(), "已完成 · 用时 7 秒");
        assert_eq!(assistant_text(stream, cx), "visible answer");
    });
}

#[gpui_kit::test]
async fn issue146_long_run_activity_is_bounded_and_keeps_its_scroll_position(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue146-bounded-run-activity");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "bounded".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "bounded".into(),
                delta: "reasoning line\n".repeat(80),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("bounded-tool", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "bounded".into(),
                delta: "follow-up line\n".repeat(80),
            },
            cx,
        );
    });
    let group = stream.read_with(cx, |stream, _| run_group(stream, "bounded"));
    let scroll = group.read_with(cx, |group, _| group.test_scroll_handle());
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let body = visual
        .debug_bounds("run-activity-content")
        .expect("bounded activity body");
    assert!(
        body.size.height <= px(Layout::TOOL_GROUP_MAX_HEIGHT),
        "run activity body: {:?}",
        body.size.height
    );
    assert!(scroll.max_offset().y > px(0.));
    let tool = visual
        .debug_bounds("tool-activity-single-row")
        .expect("tool activity row");
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: tool.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-10.))),
        modifiers: gpui_kit::Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    visual.run_until_parked();
    assert_eq!(scroll.offset().y, px(-10.));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "bounded".into(),
                delta: "streaming append\n".repeat(10),
            },
            cx,
        );
    });
    visual.run_until_parked();
    assert_eq!(scroll.offset().y, px(-10.));
}

#[gpui_kit::test]
async fn issue146_empty_runtime_failure_keeps_reason_visible_without_assistant_text(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue146-empty-runtime-failure");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "empty-runtime-failure".into(),
                seq: 3,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "empty-runtime-failure".into(),
                delta: "progress before failure".into(),
            },
            cx,
        );
        assert_eq!(
            stream
                .active_agent_message
                .as_ref()
                .map(|(_, entry_index)| *entry_index),
            Some(usize::MAX)
        );
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("empty-runtime-failure".into()),
                error: Arc::new(std::io::Error::other("private runtime diagnostic").into()),
                execution_duration_ms: Some(2_345),
            },
            cx,
        );
    });
    cx.run_until_parked();

    stream.read_with(cx, |stream, cx| {
        assert!(stream.active_agent_message.is_none());
        let (message_id, assistant_index) = stream.last_finished_agent_message.as_ref().unwrap();
        assert_eq!(message_id, "empty-runtime-failure");
        assert_ne!(*assistant_index, usize::MAX);
        let assistant = &stream.entries[*assistant_index];
        assert_eq!(assistant.row_count(cx), 1);
        assert!(matches!(
            assistant,
            StreamEntry::Assistant {
                model,
                failure: Some(RunFailureKind::Runtime),
                ..
            } if model.row_count() == 0
        ));
        assert_eq!(
            RunFailureKind::Runtime.message(),
            "任务执行失败；请检查运行环境后重试"
        );
        assert_eq!(
            run_group(stream, "empty-runtime-failure")
                .read(cx)
                .test_projection(),
            (RunActivityStatus::Failed, Some(2_345), false, true)
        );
    });

    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("assistant-run-failure").is_some());
}

#[gpui_kit::test]
async fn issue146_terminal_failure_cancel_and_hydration_restore_truthful_statuses(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "issue146-terminal-history");
    for (message_id, status, duration, event) in [
        (
            "failed-live",
            RunActivityStatus::Failed,
            1_234,
            ConversationEvent::Error {
                message_id: Some("failed-live".into()),
                error: Arc::new(std::io::Error::other("run failed").into()),
                execution_duration_ms: Some(1_234),
            },
        ),
        (
            "stopped-live",
            RunActivityStatus::Interrupted,
            2_010,
            ConversationEvent::Interrupted {
                message_id: "stopped-live".into(),
                execution_duration_ms: Some(2_010),
            },
        ),
    ] {
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: message_id.into(),
                    seq: 3,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ThinkingDelta {
                    message_id: message_id.into(),
                    delta: "progress".into(),
                },
                cx,
            );
            stream.apply_event(event.clone(), cx);
        });
        let group = stream.read_with(cx, |stream, _| run_group(stream, message_id));
        assert_eq!(
            group.read_with(cx, |group, _| group.test_projection()),
            (status, Some(duration), false, true)
        );
        let expected = match status {
            RunActivityStatus::Failed => "已失败 · 用时 2 秒",
            RunActivityStatus::Interrupted => "已停止 · 用时 3 秒",
            _ => unreachable!(),
        };
        assert_eq!(group.read_with(cx, |group, _| group.test_label()), expected);
    }

    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    HistoryEntry::AssistantText {
                        seq: 10,
                        message_id: "restored".into(),
                        content: "restored answer".into(),
                        status: vega_conversation::history::AssistantStatus::Done,
                        execution_duration_ms: Some(60_001),
                    },
                    HistoryEntry::Tool {
                        seq: 11,
                        message_id: "restored".into(),
                        call_id: "restored-tool".into(),
                        status: vega_conversation::types::ToolCallStatus::Success,
                        approval: None,
                        input: None,
                        result: None,
                    },
                    HistoryEntry::AssistantText {
                        seq: 12,
                        message_id: "legacy".into(),
                        content: String::new(),
                        status: vega_conversation::history::AssistantStatus::Interrupted,
                        execution_duration_ms: None,
                    },
                    HistoryEntry::AssistantText {
                        seq: 13,
                        message_id: "empty-complete".into(),
                        content: String::new(),
                        status: vega_conversation::history::AssistantStatus::Done,
                        execution_duration_ms: Some(1_234),
                    },
                ],
                None,
            ),
            cx,
        );
    });
    let (restored, legacy, empty_complete, empty_answer_row, final_text) =
        stream.read_with(cx, |stream, cx| {
            let restored = run_group(stream, "restored").read(cx).test_projection();
            let legacy = stream.run_activity_groups.contains_key("legacy");
            let empty_complete = run_group(stream, "empty-complete")
                .read(cx)
                .test_projection();
            let empty_answer_row = stream.entry_identities.iter().any(|identity| {
                identity.message_id.as_deref() == Some("empty-complete")
                    && identity
                        .key
                        .rsplit_once(":kind:")
                        .is_some_and(|(_, kind)| kind.starts_with("assistant-text-"))
            });
            let answer = assistant_text(stream, cx);
            (restored, legacy, empty_complete, empty_answer_row, answer)
        });
    assert_eq!(
        restored,
        (RunActivityStatus::Completed, Some(60_001), false, true)
    );
    assert!(
        !legacy,
        "legacy interrupted history has no invented duration"
    );
    assert_eq!(
        empty_complete,
        (RunActivityStatus::Completed, Some(1_234), false, true)
    );
    assert!(!empty_answer_row, "empty success has no blank answer row");
    assert!(final_text.contains("restored answer"));
}
