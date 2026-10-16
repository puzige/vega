use super::*;
use gpui_kit::{Modifiers, VisualTestContext};

fn start(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.begin_composer_run(cx);
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "retry".into(),
            seq: 20,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::AssistantResponseStarted {
            message_id: "retry".into(),
            text_start_bytes: 0,
        },
        cx,
    );
}

fn retry(
    stream: &mut ConversationStream,
    start: usize,
    attempt: u32,
    cx: &mut Context<ConversationStream>,
) {
    stream.apply_event(
        ConversationEvent::ResponseRetrying {
            message_id: "retry".into(),
            text_start_bytes: start,
            attempt,
            max_retries: 5,
        },
        cx,
    );
}

fn text(stream: &ConversationStream) -> String {
    stream
        .entries
        .iter()
        .filter_map(|entry| match entry {
            StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
            _ => None,
        })
        .collect()
}

fn assert_retry_outside_stage(visual: &mut VisualTestContext) {
    let stage = visual.debug_bounds("run-timeline-segment-0").unwrap();
    let title = visual.debug_bounds("run-stage-title-0").unwrap();
    let retry = visual.debug_bounds("assistant-network-retry").unwrap();
    let wifi = visual.debug_bounds("assistant-retry-wifi-icon").unwrap();
    assert!(stage.bottom() <= retry.top());
    assert!(retry.left() <= title.left());
    assert!(retry.size.height > px(Typography::BODY));
    assert_eq!(
        wifi.size,
        gpui_kit::size(px(Layout::ICON_SIZE), px(Layout::ICON_SIZE))
    );
    assert!(wifi.left() >= retry.left() && wifi.right() <= retry.right());
}

#[gpui_kit::test]
async fn response_retry_restores_shared_stage_without_losing_prior_commands(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "retry-shared-stage");
    stream.update(cx, |stream, cx| {
        start(stream, cx);
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "retry".into(),
                delta: "First response".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("prior-command", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "prior-command".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: String::new(),
                    reused: false,
                    exit_code: Some(0),
                    duration_ms: Some(1),
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
        let prior = stream.tool_cards["prior-command"].clone();
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: "retry".into(),
                text_start_bytes: 0,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "retry".into(),
                delta: "Abandoned response".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("abandoned-command", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "abandoned-command".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: String::new(),
                    reused: false,
                    exit_code: Some(0),
                    duration_ms: Some(1),
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
        let group = stream.run_activity_groups["retry"].read(cx);
        assert_eq!(stream.active_run_activity_segment, Some(0));
        assert_eq!(group.segment_children(0).len(), 4);
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::RunActivitySegment { .. }))
                .count(),
            1
        );
        retry(stream, 0, 1, cx);
        let group = stream.run_activity_groups["retry"].read(cx);
        assert_eq!(stream.active_run_activity_segment, Some(0));
        assert_eq!(group.segment_children(0).len(), 2);
        assert!(group.contains_tool(&prior, cx));
        assert!(!group.contains_tool(&stream.tool_cards["abandoned-command"], cx));
        assert_eq!(stream.thinking_blocks, 1);
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "retry".into(),
                delta: "Recovered response".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("replacement-command", "pwd"),
            },
            cx,
        );
        assert_eq!(
            stream.run_activity_groups["retry"]
                .read(cx)
                .segment_children(0)
                .len(),
            4
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::RunActivitySegment { .. }))
                .count(),
            1
        );
    });
}
