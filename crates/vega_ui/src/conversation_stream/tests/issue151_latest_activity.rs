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
            display_target: None,
            permission_path: None,
        }),
        result: Some(ToolCardResultProjection::ReadOnly {
            status: ToolCallStatus::Success,
            output: String::new(),
            reused: false,
        }),
    }
}
