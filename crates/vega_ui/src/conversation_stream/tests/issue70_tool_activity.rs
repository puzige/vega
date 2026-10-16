use super::*;
use gpui_kit::{Bounds, Pixels, Quad};

fn read_call(id: &str, tool: &str, raw_input: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        tool: tool.into(),
        input_json: raw_input.into(),
    }
}

fn finish(
    stream: &mut ConversationStream,
    id: &str,
    status: ToolCallStatus,
    output: &str,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    cx: &mut Context<ConversationStream>,
) {
    if status != ToolCallStatus::Rejected {
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: id.into(),
                approval: Approval::Once,
            },
            cx,
        );
    }
    stream.apply_event(
        ConversationEvent::ToolCallFinished {
            call_id: id.into(),
            result: ToolResult {
                status,
                output: output.into(),
                reused: false,
                exit_code,
                duration_ms,
                truncated: Some(false),
                invalid: None,
            },
        },
        cx,
    );
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

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    cx.run_until_parked();
    gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing selector {selector}"))
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let target = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing clickable selector {selector}"));
    visual.simulate_click(target.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
}

fn history_tool(
    seq: i64,
    id: &str,
    input: ToolCardInputProjection,
    result: ToolCardResultProjection,
) -> HistoryEntry {
    HistoryEntry::Tool {
        seq,
        message_id: "hydrated-message".into(),
        call_id: id.into(),
        status: ToolCallStatus::Success,
        approval: Some(Approval::Once),
        input: Some(input),
        result: Some(result),
    }
}
