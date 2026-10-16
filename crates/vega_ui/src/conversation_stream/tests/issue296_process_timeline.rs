use super::*;
use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};

fn response(id: &str, start: usize, end: usize, kind: AssistantResponseKind) -> ConversationEvent {
    ConversationEvent::AssistantResponseCompleted {
        message_id: "timeline".into(),
        response: AssistantResponseMetadata {
            response_id: id.into(),
            text_start_bytes: start,
            text_end_bytes: end,
            kind,
        },
    }
}

fn append(stream: &mut ConversationStream, text: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: "timeline".into(),
            delta: text.into(),
        },
        cx,
    );
}

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "timeline".into(),
            seq: 1,
        },
        cx,
    );
}

fn tool(stream: &mut ConversationStream, id: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::ToolCallProposed {
            call: bash_call(id, "pwd"),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::ToolCallApproved {
            call_id: id.into(),
            approval: Approval::Once,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::ToolCallRunning { call_id: id.into() },
        cx,
    );
    stream.apply_event(
        ConversationEvent::ToolCallFinished {
            call_id: id.into(),
            result: ToolResult {
                status: ToolCallStatus::Success,
                output: "synthetic output".into(),
                reused: false,
                exit_code: Some(0),
                duration_ms: Some(10),
                truncated: Some(false),
                invalid: None,
            },
        },
        cx,
    );
}

fn finish(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(response("final", 2, 4, AssistantResponseKind::Final), cx);
    stream.apply_event(
        ConversationEvent::MessageFinished {
            message_id: "timeline".into(),
            stop_reason: vega_conversation::types::ConversationStopReason::End,
            execution_duration_ms: None,
        },
        cx,
    );
}

fn staged(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    begin(stream, cx);
    append(stream, "A", cx);
    stream.apply_event(response("a", 0, 1, AssistantResponseKind::Commentary), cx);
    tool(stream, "one", cx);
    append(stream, "B", cx);
    stream.apply_event(response("b", 1, 2, AssistantResponseKind::Commentary), cx);
    tool(stream, "two", cx);
    append(stream, "OK", cx);
}

fn source_text(stream: &ConversationStream) -> String {
    stream
        .entries
        .iter()
        .filter_map(|entry| match entry {
            StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
            _ => None,
        })
        .collect()
}

fn history_body(seq: i64, message_id: &str, content: &str) -> HistoryEntry {
    HistoryEntry::AssistantText {
        seq,
        message_id: message_id.into(),
        content: content.into(),
        status: vega_conversation::history::AssistantStatus::Done,
        execution_duration_ms: None,
    }
}

fn metadata(
    seq: i64,
    id: &str,
    start: usize,
    end: usize,
    kind: AssistantResponseKind,
) -> HistoryEntry {
    let ConversationEvent::AssistantResponseCompleted {
        message_id,
        response,
    } = response(id, start, end, kind)
    else {
        unreachable!()
    };
    HistoryEntry::AssistantResponseMetadata {
        seq,
        message_id,
        response,
    }
}
