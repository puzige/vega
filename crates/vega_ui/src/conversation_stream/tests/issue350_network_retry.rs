use super::*;
use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};

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

#[gpui_kit::test]
async fn issue350_retry_replaces_partial_text_and_thinking_and_keeps_stop_available(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "retry-ui");
    let stops = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured_stops = stops.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &ComposerStopRequested, _| {
            captured_stops.lock().unwrap().push(event.thread_id.clone());
        })
        .detach();
    });
    stream.update(cx, |stream, cx| {
        start(stream, cx);
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "retry".into(),
                delta: "旧思考".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "retry".into(),
                delta: "旧正文🦀".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::UsageUpdated {
                message_id: "retry".into(),
                usage: vega_conversation::types::TokenUsage {
                    input: 100,
                    output: 10,
                    cache_read: 0,
                    cache_write: 0,
                },
                cost: Microcents(120),
                pricing: Some(vega_conversation::types::UsagePricing {
                    version: "pricing_v1".into(),
                    profile: "base".into(),
                    call_started_at: 1_700_000_000,
                }),
            },
            cx,
        );
        retry(stream, 0, 1, cx);
        assert!(text(stream).is_empty());
        assert_eq!(stream.thinking_blocks, 0);
        assert_eq!(stream.thinking_bytes, 0);
        assert_eq!(stream.meter.snapshot().display(), "110 tok · US$0.00012");
        assert!(stream.actions.running);
        assert!(stream.has_active_agent());
        assert_eq!(stream.run_notice_projection(cx).len(), 1);
        assert_eq!(
            stream
                .run_notice_projection(cx)
                .values()
                .next()
                .unwrap()
                .text(),
            "正在重新连接… 1/5"
        );
        retry(stream, 0, 2, cx);
        assert_eq!(stream.run_notice_projection(cx).len(), 1);
        assert_eq!(
            stream
                .run_notice_projection(cx)
                .values()
                .next()
                .unwrap()
                .text(),
            "正在重新连接… 2/5"
        );
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("assistant-network-retry").is_some());
    assert!(visual.debug_bounds("assistant-run-failure").is_none());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "retry".into(),
                delta: "新回复".into(),
            },
            cx,
        );
        assert_eq!(text(stream), "新回复");
        assert!(stream.run_notice_projection(cx).is_empty());
        retry(stream, 0, 3, cx);
        stream.request_composer_stop(cx);
        stream.request_composer_stop(cx);
        assert!(stream.actions.running);
        stream.apply_event(
            ConversationEvent::Interrupted {
                message_id: "retry".into(),
                execution_duration_ms: Some(42),
            },
            cx,
        );
        assert!(stream.network_retry.is_none());
        assert!(stream.response_checkpoint.is_none());
        assert!(!stream.has_active_agent());
        assert!(stream.finish_composer_run(true, cx));
    });
    visual.run_until_parked();
    assert_eq!(stops.lock().unwrap().as_slice(), &["retry-ui"]);
}

#[gpui_kit::test]
async fn issue350_retry_checkpoint_keeps_prior_tool_and_prepended_history(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "retry-history");
    stream.update(cx, |stream, cx| {
        start(stream, cx);
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "retry".into(),
                delta: "保留。".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: "retry".into(),
                response: AssistantResponseMetadata {
                    response_id: "prior".into(),
                    text_start_bytes: 0,
                    text_end_bytes: "保留。".len(),
                    kind: AssistantResponseKind::Commentary,
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("completed", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "completed".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: "fixture output".into(),
                    reused: false,
                    exit_code: Some(0),
                    duration_ms: Some(1),
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
        let original_tool = stream.tool_cards["completed"].clone();
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: "retry".into(),
                text_start_bytes: "保留。".len(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "retry".into(),
                delta: "失败推理".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "retry".into(),
                delta: "失败正文".into(),
            },
            cx,
        );
        stream.apply_history_page(
            hydration_page(vec![hydration_user(1, "earlier history")], None),
            cx,
        );
        let earlier = stream.entry_identities[0].key.clone();
        retry(stream, "保留。".len(), 1, cx);
        assert_eq!(text(stream), "保留。");
        assert_eq!(stream.entry_identities[0].key, earlier);
        assert_eq!(stream.tool_cards["completed"], original_tool);
        assert!(
            stream.run_activity_groups["retry"]
                .read(cx)
                .contains_tool(&original_tool, cx)
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "retry".into(),
                delta: "新尾段。".into(),
            },
            cx,
        );
        assert_eq!(text(stream), "保留。新尾段。");
        assert_eq!(stream.tool_cards.len(), 1);
    });
}

#[gpui_kit::test]
async fn issue350_empty_and_thinking_only_retry_clear_on_recovery_and_ignore_stale_run(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "retry-stale");
    stream.update(cx, |stream, cx| {
        start(stream, cx);
        retry(stream, 0, 1, cx);
        assert_eq!(stream.run_notice_projection(cx).len(), 1);
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "retry".into(),
                delta: "只有思考".into(),
            },
            cx,
        );
        assert!(stream.run_notice_projection(cx).is_empty());
        retry(stream, 0, 2, cx);
        assert!(
            stream.run_activity_groups["retry"]
                .read(cx)
                .children()
                .is_empty()
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "retry".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
        assert!(stream.network_retry.is_none());
        assert!(stream.run_notice_projection(cx).is_empty());
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "new-run".into(),
                seq: 21,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: "new-run".into(),
                text_start_bytes: 0,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "new-run".into(),
                delta: "current response".into(),
            },
            cx,
        );
        let meter = stream.meter.snapshot();
        retry(stream, 0, 5, cx);
        assert_eq!(text(stream), "current response");
        assert_eq!(stream.meter.snapshot(), meter);
        assert!(stream.network_retry.is_none());
    });
}
