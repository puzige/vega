use super::*;
use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};

fn apply(
    stream: &Entity<ConversationStream>,
    events: impl IntoIterator<Item = ConversationEvent>,
    cx: &mut TestAppContext,
) {
    stream.update(cx, |stream, cx| {
        for event in events {
            stream.apply_event(event, cx);
        }
    });
    cx.run_until_parked();
}

fn start(id: &str) -> ConversationEvent {
    ConversationEvent::MessageStarted {
        message_id: id.into(),
        seq: 2,
    }
}

fn text(id: &str, content: &str) -> ConversationEvent {
    ConversationEvent::TextDelta {
        message_id: id.into(),
        delta: content.into(),
    }
}

fn end(id: &str) -> ConversationEvent {
    ConversationEvent::MessageFinished {
        message_id: id.into(),
        stop_reason: vega_conversation::types::ConversationStopReason::End,
        execution_duration_ms: Some(10),
    }
}

fn response(
    message: &str,
    response: &str,
    start: usize,
    end: usize,
    kind: AssistantResponseKind,
) -> ConversationEvent {
    ConversationEvent::AssistantResponseCompleted {
        message_id: message.into(),
        response: AssistantResponseMetadata {
            response_id: response.into(),
            text_start_bytes: start,
            text_end_bytes: end,
            kind,
        },
    }
}

fn tool() -> ConversationEvent {
    ConversationEvent::ToolCallProposed {
        call: ToolCall {
            id: "synthetic-tool".into(),
            tool: "bash".into(),
            input_json: r#"{"command":"synthetic-tool-output"}"#.into(),
        },
    }
}

fn preview(
    stream: &Entity<ConversationStream>,
    ids: &[&str],
    cx: &TestAppContext,
) -> Option<String> {
    let ids = ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
    stream.read_with(cx, |stream, cx| stream.completion_response(&ids, cx))
}

#[gpui_kit::test]
async fn completion_response_uses_only_owned_messages_and_the_last_visible_segment(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "response-owner");
    apply(
        &stream,
        [
            start("history"),
            text("history", "historical answer"),
            end("history"),
            start("current"),
            text("current", "before the tool"),
            tool(),
            text("current", "最后答案 😀"),
            end("current"),
        ],
        cx,
    );
    assert_eq!(
        preview(&stream, &["current"], cx).as_deref(),
        Some("最后答案 😀")
    );
    assert_eq!(preview(&stream, &["missing"], cx), None);
    assert_eq!(preview(&stream, &[], cx), None);
}

#[gpui_kit::test]
async fn completion_response_excludes_thinking_tools_whitespace_and_prior_rounds(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "response-empty");
    apply(
        &stream,
        [start("old"), text("old", "previous answer"), end("old")],
        cx,
    );
    for (message, events) in [
        ("empty", vec![]),
        (
            "thinking",
            vec![ConversationEvent::ThinkingDelta {
                message_id: "thinking".into(),
                delta: "private reasoning".into(),
            }],
        ),
        ("tool-only", vec![tool()]),
        ("whitespace", vec![text("whitespace", " \n\t ")]),
    ] {
        let mut run = vec![start(message)];
        run.extend(events);
        run.push(end(message));
        apply(&stream, run, cx);
        assert_eq!(preview(&stream, &[message], cx), None, "{message}");
    }
    assert_eq!(
        preview(&stream, &["old"], cx).as_deref(),
        Some("previous answer")
    );
}

#[gpui_kit::test]
async fn completion_response_uses_final_byte_range_and_never_commentary(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "response-final");
    let commentary = "正在处理中文。";
    let final_text = "结果已准备 😀";
    apply(
        &stream,
        [
            start("final"),
            text("final", commentary),
            response(
                "final",
                "progress",
                0,
                commentary.len(),
                AssistantResponseKind::Commentary,
            ),
            text("final", final_text),
            response(
                "final",
                "answer",
                commentary.len(),
                commentary.len() + final_text.len(),
                AssistantResponseKind::Final,
            ),
            end("final"),
        ],
        cx,
    );
    assert_eq!(
        preview(&stream, &["final"], cx).as_deref(),
        Some(final_text)
    );
    apply(
        &stream,
        [
            start("commentary-only"),
            text("commentary-only", commentary),
            response(
                "commentary-only",
                "progress-only",
                0,
                commentary.len(),
                AssistantResponseKind::Commentary,
            ),
            end("commentary-only"),
        ],
        cx,
    );
    assert_eq!(preview(&stream, &["commentary-only"], cx), None);
}

#[gpui_kit::test]
async fn completion_response_rejects_invalid_conflicting_or_incomplete_response_metadata(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "response-invalid");
    for (message, metadata) in [
        (
            "invalid-boundary",
            vec![response(
                "invalid-boundary",
                "bad",
                1,
                "中文".len(),
                AssistantResponseKind::Final,
            )],
        ),
        (
            "conflict",
            vec![
                response(
                    "conflict",
                    "same",
                    0,
                    "中文".len(),
                    AssistantResponseKind::Final,
                ),
                response(
                    "conflict",
                    "same",
                    0,
                    "中文".len(),
                    AssistantResponseKind::Commentary,
                ),
            ],
        ),
        (
            "not-final",
            vec![response(
                "not-final",
                "partial",
                0,
                "中".len(),
                AssistantResponseKind::Commentary,
            )],
        ),
    ] {
        let mut events = vec![start(message), text(message, "中文")];
        events.extend(metadata);
        events.push(end(message));
        apply(&stream, events, cx);
        assert_eq!(preview(&stream, &[message], cx), None, "{message}");
    }
}

#[gpui_kit::test]
async fn completion_response_follows_steering_messages_without_reusing_a_later_round(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "response-steering");
    apply(
        &stream,
        [
            start("root"),
            text("root", "first response"),
            end("root"),
            start("steered"),
            text("steered", "steered answer"),
            end("steered"),
            start("later"),
            text("later", "another run"),
            end("later"),
        ],
        cx,
    );
    assert_eq!(
        preview(&stream, &["root", "steered"], cx).as_deref(),
        Some("steered answer")
    );
    assert_eq!(
        preview(&stream, &["later"], cx).as_deref(),
        Some("another run")
    );
}
