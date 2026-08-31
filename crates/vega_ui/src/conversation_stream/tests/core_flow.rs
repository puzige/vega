use super::*;

#[gpui::test]
async fn settings_keyboard_emits_scoped_requests_without_optimistic_state(cx: &mut TestAppContext) {
    let (window, stream, events) = open_controller_stream(cx, "settings-thread");
    focus_setting(window, &stream, 1, cx);
    cx.simulate_keystrokes(window.into(), "enter");
    focus_setting(window, &stream, 5, cx);
    cx.simulate_keystrokes(window.into(), "space");

    let events = events.lock().expect("settings event capture");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].thread_id, "settings-thread");
    assert_eq!(events[0].mode, Some(ThreadMode::Plan));
    assert_eq!(events[0].permission_mode, None);
    assert_eq!(events[1].thread_id, "settings-thread");
    assert_eq!(events[1].mode, None);
    assert_eq!(events[1].permission_mode, Some(PermissionMode::Auto));
    drop(events);

    let selected = stream.read_with(cx, |stream, _| {
        (stream.thread.mode, stream.thread.permission_mode)
    });
    assert_eq!(selected, (ThreadMode::Execute, PermissionMode::Confirm));
    stream.update(cx, ConversationStream::apply_controller_error);
    let selected = stream.read_with(cx, |stream, _| {
        (stream.thread.mode, stream.thread.permission_mode)
    });
    assert_eq!(selected, (ThreadMode::Execute, PermissionMode::Confirm));

    let mut persisted = permission_thread();
    persisted.id = "settings-thread".into();
    persisted.mode = ThreadMode::Plan;
    persisted.permission_mode = PermissionMode::Auto;
    stream.update(cx, |stream, cx| stream.apply_thread(persisted, cx));
    let selected = stream.read_with(cx, |stream, _| {
        (stream.thread.mode, stream.thread.permission_mode)
    });
    assert_eq!(selected, (ThreadMode::Plan, PermissionMode::Auto));
}

#[gpui::test]
async fn multiline_history_continues_and_is_thread_scoped(cx: &mut TestAppContext) {
    let (first_window, first, _) = open_controller_stream(cx, "history-a");
    let (_second_window, second, _) = open_controller_stream(cx, "history-b");
    first.update(cx, |stream, cx| {
        stream.composer_history = vec!["older\nfirst".into(), "newer\nfirst".into()];
        stream
            .input
            .update(cx, |input, cx| input.set_text("draft", cx));
    });
    second.update(cx, |stream, cx| {
        stream.composer_history = vec!["only\nsecond".into()];
        stream
            .input
            .update(cx, |input, cx| input.set_text("second draft", cx));
    });
    focus_composer(first_window, &first, cx);
    cx.simulate_keystrokes(first_window.into(), "up");
    assert_eq!(
        first.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "newer\nfirst"
    );
    cx.simulate_keystrokes(first_window.into(), "up");
    assert_eq!(
        first.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "older\nfirst"
    );
    assert_eq!(
        second.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "second draft"
    );
}

#[gpui::test]
async fn composer_echo_waits_for_durable_acceptance(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "durable-submit");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("keep this draft", cx));
        stream.submit_message(cx);
    });
    let pending = stream.read_with(cx, |stream, cx| {
        (
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.composer_history.len(),
            stream.entries.len(),
        )
    });
    assert_eq!(pending, (true, "keep this draft".into(), 0, 0));

    stream.update(cx, ConversationStream::reject_composer_submission);
    let rejected = stream.read_with(cx, |stream, cx| {
        (
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.composer_history.len(),
            stream.entries.len(),
        )
    });
    assert_eq!(rejected, (false, "keep this draft".into(), 0, 0));

    stream.update(cx, |stream, cx| {
        stream.submit_message(cx);
        stream.accept_composer_submission("keep this draft", cx);
    });
    let accepted = stream.read_with(cx, |stream, cx| {
        (
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.composer_history.clone(),
            stream.entries.len(),
        )
    });
    assert_eq!(
        accepted,
        (false, String::new(), vec!["keep this draft".into()], 1)
    );
}

#[gpui::test]
async fn approved_not_started_projection_preserves_and_blocks_new_draft(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "approved-recovery");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("do not lose", cx));
        stream.apply_approved_not_started(cx);
        stream.submit_message(cx);
    });
    let state = stream.read_with(cx, |stream, cx| {
        (
            stream.approved_not_started,
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.entries.len(),
        )
    });
    assert_eq!(state, (true, false, "do not lose".into(), 0));
}

#[gpui::test]
async fn durable_assistant_events_require_exact_active_message(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "durable-events");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "assistant".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "foreign".into(),
                delta: "hidden".into(),
            },
            cx,
        );
    });
    let foreign_ignored = stream.read_with(cx, |stream, _| {
        let (_, index) = stream
            .active_agent_message
            .as_ref()
            .expect("active message");
        match &stream.entries[*index] {
            StreamEntry::Assistant { stream, .. } => stream.snapshot().pending.is_none(),
            _ => false,
        }
    });
    assert!(foreign_ignored);

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "assistant".into(),
                delta: "visible".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "foreign".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    assert!(stream.read_with(cx, |stream, _| stream.active_agent_message.is_some()));
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "assistant".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    assert!(stream.read_with(cx, |stream, _| stream.active_agent_message.is_none()));
}

#[gpui::test]
async fn completed_plan_replaces_streaming_assistant_after_older_plan_refresh(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "plan-dedup");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "plan-message".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "plan-message".into(),
                delta: "1. inspect".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "plan-message".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
        stream.apply_plan(
            Plan {
                id: "older-plan".into(),
                thread_id: "plan-dedup".into(),
                content: "older".into(),
                status: PlanStatus::Abandoned,
                review_note: Some("superseded".into()),
                reviewed_at: Some(1),
            },
            cx,
        );
        stream.apply_plan(
            Plan {
                id: "plan-message".into(),
                thread_id: "plan-dedup".into(),
                content: "1. inspect".into(),
                status: PlanStatus::Pending,
                review_note: None,
                reviewed_at: None,
            },
            cx,
        );
    });
    let (plans, assistants, entries) = stream.read_with(cx, |stream, _| {
        let plans = stream
            .entries
            .iter()
            .filter(|entry| matches!(entry, StreamEntry::Plan { .. }))
            .count();
        let assistants = stream
            .entries
            .iter()
            .filter(|entry| matches!(entry, StreamEntry::Assistant { .. }))
            .count();
        (plans, assistants, stream.entries.len())
    });
    assert_eq!((plans, assistants, entries), (2, 0, 2));
}

#[gpui::test]
async fn task_summary_card_appends_once_and_ignores_duplicates(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "summary-card");
    let summary = TaskCostSummary {
        message_id: "assistant-summary".into(),
        outcome: TaskSummaryOutcome::Completed,
        usage: Some(vega_conversation::types::TokenUsage {
            input: 150_000,
            output: 15_000,
            cache_read: 50_000,
            cache_write: 0,
        }),
        cost: vega_conversation::types::SummaryCost::Priced(vega_conversation::types::Microcents(
            135_000,
        )),
        duration_ms: Some(12_400),
        tool_count: 2,
        cache_hit_percent: Some(33),
    };
    stream.update(cx, |stream, cx| {
        stream.apply_task_summary(summary.clone(), cx);
        stream.apply_task_summary(summary, cx);
    });
    let (summaries, rows, text) = stream.read_with(cx, |stream, cx| {
        let mut text = String::new();
        let mut summaries = 0;
        let mut rows = 0;
        for entry in &stream.entries {
            rows += entry.row_count(cx);
            if let StreamEntry::Summary { card } = entry {
                summaries += 1;
                text = card.read(cx).visible_text();
            }
        }
        (summaries, rows, text)
    });
    assert_eq!(summaries, 1, "duplicate/stale summaries are ignored");
    assert_eq!(rows, 5, "the card contributes its five fixed rows");
    assert!(text.contains("任务摘要 · 完成"));
    assert!(text.contains("成本 US$0.135000"));
    assert!(text.contains("耗时 12.4s"));
    assert!(text.contains("工具 2 · 缓存命中 33%"));
}
