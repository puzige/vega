use super::*;
use crate::icons::Icon;
use gpui_kit::{TestAppContext, VisualTestContext, WindowHandle};
use vega_conversation::types::{
    ContextAccountingRecord, ContextAccountingSource, ContextAccountingStage,
    ContextCompactionUsageState, ThreadMode, ThreadStatus,
};

fn accounting(
    source: ContextAccountingSource,
    predicted_input: u64,
    revision: u64,
) -> ContextAccountingRecord {
    ContextAccountingRecord {
        source,
        stage: ContextAccountingStage::PrimaryPreflight,
        provider_input_baseline: (source == ContextAccountingSource::UsageAnchored).then_some(500),
        incremental_estimate: predicted_input.saturating_sub(500),
        predicted_input,
        input_budget: 10_000,
        trigger_tokens: 8_000,
        target_tokens: 6_000,
        revision,
        covered_messages: 2,
    }
}

#[gpui_kit::test]
async fn issue91_live_accounting_fences_reload_and_accepts_fallback_revision(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.begin_composer_run(cx);
            stream.apply_event(
                vega_conversation::types::ConversationEvent::MessageStarted {
                    message_id: "run-one".into(),
                    seq: 1,
                },
                cx,
            );
            assert!(stream.apply_context_accounting(
                "context-thread",
                "mock",
                "run-one",
                accounting(ContextAccountingSource::UsageAnchored, 800, 1),
                cx
            ));
            assert_eq!(stream.context_control.estimate, Some(800));
            assert!(!stream.apply_context_accounting(
                "other-thread",
                "mock",
                "run-one",
                accounting(ContextAccountingSource::Estimated, 900, 2),
                cx
            ));
            assert!(!stream.apply_context_accounting(
                "context-thread",
                "other-model",
                "run-one",
                accounting(ContextAccountingSource::Estimated, 900, 2),
                cx
            ));
            assert!(stream.apply_context_projection(
                "context-thread",
                "mock",
                None,
                Some(9_000),
                true,
                cx
            ));
            assert_eq!(stream.context_control.estimate, Some(800));
            assert!(!stream.apply_context_accounting(
                "context-thread",
                "mock",
                "other",
                accounting(ContextAccountingSource::Estimated, 900, 2),
                cx
            ));
            assert!(!stream.apply_context_accounting(
                "context-thread",
                "mock",
                "run-one",
                accounting(ContextAccountingSource::Estimated, 900, 0),
                cx
            ));
            assert!(stream.apply_context_accounting(
                "context-thread",
                "mock",
                "run-one",
                accounting(ContextAccountingSource::Estimated, 1_100, 2),
                cx
            ));
            assert_eq!(stream.context_control.estimate, Some(1_100));
            assert_eq!(
                stream
                    .context_control
                    .live_accounting
                    .as_ref()
                    .unwrap()
                    .1
                    .source,
                ContextAccountingSource::Estimated
            );
            stream.finish_composer_run(false, cx);
            assert!(stream.context_control.live_accounting.is_none());
            assert_eq!(stream.context_control.estimate, None);
            assert!(stream.apply_context_projection(
                "context-thread",
                "mock",
                None,
                Some(1_300),
                true,
                cx
            ));
            assert_eq!(stream.context_control.estimate, Some(1_300));
        })
        .expect("window");
}

fn setup(cx: &mut TestAppContext) -> WindowHandle<ConversationStream> {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(false));
        crate::init(cx);
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                ConversationStream::new(
                    Thread {
                        id: "context-thread".into(),
                        project_id: "project".into(),
                        title: "Context".into(),
                        model: "mock".into(),
                        mode: ThreadMode::Execute,
                        permission_mode: PermissionMode::Confirm,
                        status: ThreadStatus::Active,
                        pinned: false,
                        unread: false,
                        created_at: 1,
                        updated_at: 1,
                    },
                    cx,
                )
            })
        })
        .expect("window")
    })
}

fn status(generation: u64, status: Status) -> ContextCompactionStatusRecord {
    ContextCompactionStatusRecord {
        generation,
        status,
        updated_at: 1,
        estimated_tokens: Some(5000),
        input_budget: Some(300_000),
        target_tokens: Some(180_000),
        source_version: Some(1),
        failure: None,
        usage: ContextCompactionUsageState::Unknown,
    }
}

#[test]
fn issue88_internal_stage_limit_and_model_budget_have_distinct_copy() {
    assert!(status_label_for(Status::Failed, Some(Failure::TooLarge)).contains("安全分段上限"));
    assert!(!status_label_for(Status::Failed, Some(Failure::TooLarge)).contains("模型容量"));
    assert!(
        status_label_for(Status::Failed, Some(Failure::OverLimit))
            .contains("本地上下文预算检查未通过")
    );
    assert!(!status_label_for(Status::Failed, Some(Failure::OverLimit)).contains("安全分段上限"));
}

#[test]
fn issue117_compaction_copy_matches_reference_and_marks_restored_rows() {
    assert_eq!(status_label_for(Status::Compacting, None), "正在压缩上下文");
    assert_eq!(status_label_for(Status::Succeeded, None), "上下文已压缩");
    assert!(
        !status_label_for(Status::Compacting, None).contains('…'),
        "the in-progress label must not keep the trailing ellipsis"
    );
    assert_eq!(
        status_label_for(Status::Cancelled, None),
        "上下文压缩已取消，原始对话未更改"
    );
    assert_eq!(
        status_label_for(Status::Failed, Some(Failure::InvalidSummary)),
        "压缩结果无效，请重试"
    );
    assert_eq!(
        context_compaction_label(Status::Succeeded, None, true),
        "已恢复 · 上下文已压缩"
    );
    assert_eq!(
        context_compaction_label(Status::Failed, Some(Failure::SourceChanged), true),
        "已恢复 · 历史已变化，请重试压缩"
    );
    assert_eq!(
        context_compaction_label(Status::Succeeded, None, false),
        "上下文已压缩"
    );
    for restored in [false, true] {
        for (state, failure) in [
            (Status::Compacting, None),
            (Status::Succeeded, None),
            (Status::Failed, Some(Failure::InvalidSummary)),
            (Status::Cancelled, None),
        ] {
            let label = context_compaction_label(state, failure, restored);
            assert!(!label.contains("上次"), "restored={restored}: {label}");
            assert!(
                !label.contains("上下文压缩完成"),
                "restored={restored}: {label}"
            );
        }
    }
}

/// #117 C8: every compaction state renders the same shared `text-select`
/// glyph, so the row never changes shape as the operation advances. Only the
/// failure state is recolored; the label carries the transition.
#[test]
fn issue117_compaction_row_uses_one_glyph_for_every_state() {
    let colors = vega_theme::Theme::light().colors;
    let states = [
        Status::Compacting,
        Status::Succeeded,
        Status::Failed,
        Status::Cancelled,
    ];
    for state in states {
        let (glyph, color) = context_compaction_visual(state, &colors);
        assert!(
            matches!(glyph, Icon::TextSelect),
            "{state:?} must share the single text-select glyph"
        );
        let expected = if state == Status::Failed {
            colors.danger
        } else {
            colors.text_secondary
        };
        assert_eq!(color, expected, "{state:?} has the wrong icon color");
    }
    // Ready/Unknown never produce a row, so they are not part of the contract.
    for state in [Status::Ready, Status::Unknown] {
        let (glyph, color) = context_compaction_visual(state, &colors);
        assert!(matches!(glyph, Icon::TextSelect));
        assert_eq!(color, colors.text_secondary);
    }
}

#[gpui_kit::test]
async fn context_ui_unknown_usage_restore_is_idempotent_and_load_error_is_not_usage(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.restore_meter(
                RestoredUsage {
                    tokens: 50,
                    cost: Some(vega_conversation::types::Microcents(10)),
                },
                cx,
            );
            let before = stream.meter_snapshot();
            assert!(stream.apply_context_load_error("context-thread", "mock", cx));
            assert_eq!(stream.meter_snapshot(), before);
            assert!(!stream.restore_context_unknown_usage("other", "mock", true, cx));
            assert_eq!(stream.meter_snapshot(), before);
            assert!(stream.restore_context_unknown_usage("context-thread", "mock", true, cx));
            let unknown = stream.meter_snapshot();
            assert_eq!(unknown.tokens, 50);
            assert!(unknown.display().ends_with("—"));
            stream.restore_context_unknown_usage("context-thread", "mock", true, cx);
            stream.restore_context_unknown_usage("context-thread", "mock", false, cx);
            assert_eq!(stream.meter_snapshot(), unknown);
        })
        .expect("meter");
}

#[gpui_kit::test]
async fn issue117_context_status_is_a_conversation_item(cx: &mut TestAppContext) {
    let window = setup(cx);
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("composer-shell").is_some());
    assert!(visual.debug_bounds("composer-context").is_none());
    assert!(visual.debug_bounds("context-popup").is_none());
    assert!(visual.debug_bounds("context-status-band").is_none());

    window
        .update(cx, |stream, _, cx| {
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(1, Status::Ready),
                cx,
            ));
        })
        .expect("ready");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("context-status-band").is_none());

    window
        .update(cx, |stream, _, cx| {
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(2, Status::Compacting),
                cx,
            ));
        })
        .expect("compacting");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("context-status-band").is_none());
    assert!(visual.debug_bounds("context-compaction-row").is_some());
    window
        .update(cx, |stream, _, cx| {
            assert_eq!(stream.entries.len(), 1);
            stream.apply_context_status("context-thread", "mock", status(2, Status::Succeeded), cx);
            assert_eq!(
                stream.entries.len(),
                1,
                "terminal updates the same list item"
            );
        })
        .expect("terminal");
    assert!(visual.debug_bounds("composer-context").is_none());
}

#[gpui_kit::test]
async fn context_ui_terminal_and_aba_status_transitions_remain_fenced(cx: &mut TestAppContext) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream
                .input
                .update(cx, |input, cx| input.set_text("preserved draft", cx));
            let id = stream
                .reserve_context_operation_id(cx)
                .expect("first operation");
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(id, Status::Compacting),
                cx,
            ));
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(id, Status::Cancelled),
                cx,
            ));
            for next in [Status::Succeeded, Status::Failed, Status::Compacting] {
                let mut late = status(id, next);
                late.updated_at = 99;
                assert!(!stream.apply_context_status("context-thread", "mock", late, cx));
            }
            assert_eq!(stream.input.read(cx).text(), "preserved draft");
            let retry = stream.reserve_context_operation_id(cx).expect("retry");
            assert!(retry > id);
            assert!(!stream.apply_context_status(
                "context-thread",
                "mock",
                status(id, Status::Succeeded),
                cx,
            ));
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(retry, Status::Failed),
                cx,
            ));
            let auto = stream
                .reserve_context_operation_id(cx)
                .expect("next run auto");
            assert!(auto > retry);
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(auto, Status::Compacting),
                cx,
            ));
            assert!(!stream.apply_context_status(
                "context-thread",
                "other",
                status(auto, Status::Succeeded),
                cx,
            ));
            stream.reset_context_control(cx);
            assert!(!stream.apply_context_status(
                "context-thread",
                "mock",
                status(auto, Status::Succeeded),
                cx,
            ));
            assert!(stream.reserve_context_operation_id(cx).expect("ABA") > auto);
        })
        .expect("status fencing");
}

#[gpui_kit::test]
async fn issue117_compaction_preserves_stream_order_and_detached_tail(cx: &mut TestAppContext) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "run".into(),
                    seq: 1,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "run".into(),
                    delta: "before".into(),
                },
                cx,
            );
            stream.list.set_follow_mode(gpui_kit::FollowMode::Normal);
            stream.apply_context_status(
                "context-thread",
                "mock",
                status(1, Status::Compacting),
                cx,
            );
            assert_eq!(
                stream.entries.len(),
                2,
                "compaction is a semantic list item"
            );
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "run".into(),
                    delta: "after".into(),
                },
                cx,
            );
            stream.apply_context_status("context-thread", "mock", status(1, Status::Succeeded), cx);
            assert_eq!(stream.entries.len(), 3, "later text follows compaction");
            assert!(!stream.following_tail());
            assert_eq!(
                compaction_timeline(stream),
                vec!["before", "1:Succeeded:live", "after"]
            );
            let mut late = status(1, Status::Compacting);
            late.updated_at = 99;
            assert!(!stream.apply_context_status("context-thread", "mock", late, cx));
            assert_eq!(
                compaction_timeline(stream),
                vec!["before", "1:Succeeded:live", "after"]
            );
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: vega_conversation::types::ToolCall {
                        id: "tool".into(),
                        tool: "bash".into(),
                        input_json: serde_json::json!({"cmd": "pwd"}).to_string(),
                    },
                },
                cx,
            );
            assert_eq!(
                compaction_timeline(stream),
                vec!["before", "1:Succeeded:live", "after", "tool"]
            );
            for generation in 2..10 {
                stream.apply_context_status(
                    "context-thread",
                    "mock",
                    status(generation, Status::Compacting),
                    cx,
                );
                stream.apply_context_status(
                    "context-thread",
                    "mock",
                    status(generation, Status::Cancelled),
                    cx,
                );
            }
            assert_eq!(
                stream.entries.len(),
                12,
                "each operation keeps one row without eviction"
            );
            assert!(!stream.following_tail());
            let timeline = compaction_timeline(stream);
            assert_eq!(
                &timeline[..4],
                &["before", "1:Succeeded:live", "after", "tool"]
            );
            assert_eq!(
                &timeline[4..],
                (2..10)
                    .map(|id| format!("{id}:Cancelled:live"))
                    .collect::<Vec<_>>()
            );
        })
        .expect("ordered compaction");
}

fn compaction_timeline(stream: &mut ConversationStream) -> Vec<String> {
    stream
        .entries
        .iter_mut()
        .map(|entry| match entry {
            StreamEntry::Assistant {
                stream: parser,
                model,
                ..
            } => {
                model.sync(&parser.snapshot(), &stream.counters);
                model
                    .committed_lines
                    .iter()
                    .chain(&model.pending_lines)
                    .flat_map(|line| &line.spans)
                    .map(|span| span.text.as_str())
                    .collect()
            }
            StreamEntry::ContextCompaction {
                record, restored, ..
            } => format!(
                "{}:{:?}:{}",
                record.generation,
                record.status,
                if *restored { "restored" } else { "live" }
            ),
            StreamEntry::Tool { .. } => "tool".into(),
            StreamEntry::User { .. } => "user".into(),
            _ => "other".into(),
        })
        .collect()
}

#[gpui_kit::test]
async fn issue117_restore_is_historical_idempotent_and_rejects_late_results(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            assert!(!stream.restore_context_status(
                "foreign",
                "mock",
                status(10, Status::Succeeded),
                cx
            ));
            assert!(!stream.restore_context_status(
                "context-thread",
                "foreign",
                status(10, Status::Succeeded),
                cx
            ));
            assert!(stream.restore_context_status(
                "context-thread",
                "mock",
                status(10, Status::Succeeded),
                cx
            ));
            assert_eq!(compaction_timeline(stream), vec!["1:Succeeded:restored"]);
            assert!(!stream.restore_context_status(
                "context-thread",
                "mock",
                status(10, Status::Succeeded),
                cx
            ));
            // History can finish asynchronously after the status projection; it
            // prepends before the explicitly historical row, never after it.
            stream.apply_history_page(
                HistoryPage {
                    entries: vec![HistoryEntry::UserText {
                        seq: 1,
                        message_id: "history-1".into(),
                        content: "history".into(),
                    }],
                    older_cursor: None,
                    newest_seq: Some(1),
                },
                cx,
            );
            assert_eq!(
                compaction_timeline(stream),
                vec!["user", "1:Succeeded:restored"]
            );
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "new".into(),
                    seq: 2,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "new".into(),
                    delta: "new answer".into(),
                },
                cx,
            );
            assert!(!stream.restore_context_status(
                "context-thread",
                "mock",
                status(10, Status::Failed),
                cx
            ));
            stream.apply_context_status(
                "context-thread",
                "mock",
                status(2, Status::Compacting),
                cx,
            );
            assert!(!stream.restore_context_status(
                "context-thread",
                "mock",
                status(10, Status::Failed),
                cx
            ));
            assert_eq!(
                compaction_timeline(stream),
                vec![
                    "user",
                    "1:Succeeded:restored",
                    "new answer",
                    "2:Compacting:live"
                ]
            );
        })
        .expect("historical restore");
}

#[gpui_kit::test]
async fn issue117_compaction_row_icon_and_label_are_vertically_centered(cx: &mut TestAppContext) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(2, Status::Compacting),
                cx,
            ));
        })
        .expect("compacting");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let row = visual
        .debug_bounds("context-compaction-row")
        .expect("row bounds");
    let slot = visual
        .debug_bounds("context-compaction-icon-slot")
        .expect("icon slot bounds");
    let label = visual
        .debug_bounds("context-compaction-live")
        .expect("label bounds");
    let slot_center = f32::from(slot.origin.y) + f32::from(slot.size.height) / 2.0;
    let label_center = f32::from(label.origin.y) + f32::from(label.size.height) / 2.0;
    let row_center = f32::from(row.origin.y) + f32::from(row.size.height) / 2.0;
    assert!(
        (slot_center - label_center).abs() < 0.5,
        "icon center {slot_center} and label center {label_center} must share one optical center"
    );
    assert!(
        (slot_center - row_center).abs() < 3.0,
        "icon center {slot_center} must sit near the row center {row_center}"
    );
}
