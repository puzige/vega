use super::*;
use crate::icons::Icon;
use gpui_kit::{TestAppContext, VisualTestContext, WindowHandle};
use vega_conversation::types::{
    ContextAccountingRecord, ContextAccountingSource, ContextAccountingStage,
    ContextCompactionUsageState, TaskBackend, ThreadMode, ThreadStatus,
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
                ContextUsageProjection::new(None, Some(9_000)),
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
                ContextUsageProjection::new(None, Some(1_300)),
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
                        backend: TaskBackend::Native,
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
    assert!(status_label_for(Status::Failed, Some(Failure::TooLarge)).contains("无法安全压缩"));
    assert!(!status_label_for(Status::Failed, Some(Failure::TooLarge)).contains("模型容量"));
    assert!(status_label_for(Status::Failed, Some(Failure::OverLimit)).contains("上下文超出预算"));
    assert!(!status_label_for(Status::Failed, Some(Failure::OverLimit)).contains("无法安全压缩"));
}

#[gpui_kit::test]
async fn issue356_timeout_label_survives_restoration_and_keeps_invalid_summary_distinct(
    cx: &mut TestAppContext,
) {
    assert_eq!(
        status_label_for(Status::Failed, Some(Failure::TimedOut)),
        "上下文压缩超时，请重试"
    );
    assert_eq!(
        status_label_for(Status::Failed, Some(Failure::InvalidSummary)),
        "压缩结果无效，请重试"
    );
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            let mut record = status(1, Status::Failed);
            record.failure = Some(Failure::TimedOut);
            assert!(stream.restore_context_status("context-thread", "mock", record, cx));
            let Some(StreamEntry::ContextCompaction {
                record, restored, ..
            }) = stream.entries.last()
            else {
                panic!("restored timeout row missing");
            };
            assert!(*restored);
            assert_eq!(record.failure, Some(Failure::TimedOut));
            assert_eq!(
                context_compaction_label(record.status, record.failure, *restored),
                "上下文压缩超时，请重试"
            );
            assert!(!stream.context_operation_busy());
        })
        .unwrap();
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
        "上下文压缩已取消"
    );
    assert_eq!(
        status_label_for(Status::Failed, Some(Failure::InvalidSummary)),
        "压缩结果无效，请重试"
    );
    assert_eq!(
        context_compaction_label(Status::Succeeded, None, true),
        "上下文已压缩"
    );
    assert_eq!(
        context_compaction_label(Status::Failed, Some(Failure::SourceChanged), true),
        "历史已变化，请重试压缩"
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
async fn issue168_compaction_row_remains_visible_after_primary_provider_failure(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.begin_composer_run(cx);
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "issue168-active-run".into(),
                    seq: 1,
                },
                cx,
            );
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(1, Status::Succeeded),
                cx,
            ));
            stream.apply_event(
                ConversationEvent::Error {
                    message_id: Some("issue168-active-run".into()),
                    error: Arc::new(vega_runtime::VegaError::Provider {
                        status: Some(503),
                        message: "provider unavailable".into(),
                        retryable: false,
                    }),
                    execution_duration_ms: Some(2_345),
                },
                cx,
            );
        })
        .expect("active compaction error window");
    cx.run_until_parked();

    window
        .update(cx, |stream, _, cx| {
            assert!(compaction_timeline(stream).contains(&"1:Succeeded:live".into()));
            let (message_id, assistant_index) =
                stream.last_finished_agent_message.as_ref().unwrap();
            assert_eq!(message_id, "issue168-active-run");
            assert!(matches!(
                &stream.entries[*assistant_index],
                StreamEntry::Assistant {
                    failure: Some(RunFailureKind::ProviderHttp(503)),
                    ..
                }
            ));
            assert_eq!(
                stream.controller_error.as_deref(),
                Some("供应商请求失败（HTTP 503）；请检查供应商状态、模型和额度后重试")
            );
            let group = stream
                .run_activity_groups
                .get("issue168-active-run")
                .cloned()
                .expect("run activity group");
            assert_eq!(
                group.read(cx).test_projection(),
                (RunActivityStatus::Failed, Some(2_345), false, true)
            );
        })
        .expect("failed run projection");

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-activity-status").is_some());
    assert!(visual.debug_bounds("run-activity-content").is_none());
    assert!(visual.debug_bounds("context-compaction-row").is_some());
    assert!(visual.debug_bounds("assistant-run-failure").is_some());
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
                3,
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
            assert_eq!(stream.entries.len(), 4, "later text follows compaction");
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
                13,
                "compaction operations keep one row and the run keeps its duration header"
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
        .filter_map(|entry| match entry {
            StreamEntry::Assistant {
                stream: parser,
                model,
                ..
            } => {
                model.sync(&parser.snapshot(), &stream.counters);
                Some(
                    model
                        .committed_lines
                        .iter()
                        .chain(&model.pending_lines)
                        .flat_map(|line| &line.spans)
                        .map(|span| span.text.as_str())
                        .collect(),
                )
            }
            StreamEntry::ContextCompaction {
                record, restored, ..
            } => Some(format!(
                "{}:{:?}:{}",
                record.generation,
                record.status,
                if *restored { "restored" } else { "live" }
            )),
            StreamEntry::RunActivity { .. } => None,
            StreamEntry::RunActivitySegment { .. } => Some("tool".into()),
            StreamEntry::Tool { .. } => Some("tool".into()),
            StreamEntry::User { .. } => Some("user".into()),
            _ => Some("other".into()),
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
                    newer_cursor: None,
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
async fn issue365_compaction_row_icon_and_label_align_to_first_line(cx: &mut TestAppContext) {
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

    assert!(
        (slot_center - label_center).abs() < 0.5,
        "icon center {slot_center} and label center {label_center} must share one optical center"
    );
    assert_eq!(slot.origin.y, row.origin.y);
    assert_eq!(label.origin.y, row.origin.y);
}

#[gpui_kit::test]
async fn issue365_compaction_failure_deduplicates_only_owned_budget_error(cx: &mut TestAppContext) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            let mut record = status(1, Status::Failed);
            record.failure = Some(Failure::OverLimit);
            stream.apply_context_status("context-thread", "mock", record, cx);
            let index = stream.entries.len() - 1;
            let identity = stream.durable_entry_identity("owned-run", "compaction-1", None);
            stream.set_entry_identity(index, identity);
            stream
                .run_failures
                .insert("owned-run".into(), RunFailureKind::ContextOverLimit);
            stream.refresh_run_notices(cx);
            assert_eq!(stream.run_notices.len(), 1);
            assert!(matches!(
                stream.run_notices[&index].kind,
                run_notice::RunNoticeKind::ContextFailure
            ));
            assert!(stream.compaction_failure_is_represented(index));
            stream
                .run_failures
                .insert("owned-run".into(), RunFailureKind::ProviderTransport);
            let notices = stream.run_notice_projection(cx);
            assert_eq!(notices.len(), 1);
            assert!(matches!(
                notices[&index].kind,
                run_notice::RunNoticeKind::Failure(RunFailureKind::ProviderTransport)
            ));
            stream.run_failures.clear();
            stream
                .run_failures
                .insert("other-run".into(), RunFailureKind::ContextOverLimit);
            assert!(
                !stream
                    .compaction_reports_run_failure("other-run", RunFailureKind::ContextOverLimit)
            );
            assert!(!stream.compaction_reports_run_failure("owned-run", RunFailureKind::Persisted));
        })
        .unwrap();
}

#[gpui_kit::test]
async fn issue365_compaction_states_keep_compact_natural_height(cx: &mut TestAppContext) {
    for (generation, state) in [
        Status::Compacting,
        Status::Succeeded,
        Status::Cancelled,
        Status::Failed,
    ]
    .into_iter()
    .enumerate()
    {
        let window = setup(cx);
        window
            .update(cx, |stream, _, cx| {
                let mut record = status(generation as u64 + 1, state);
                record.failure = (state == Status::Failed).then_some(Failure::OverLimit);
                stream.apply_context_status("context-thread", "mock", record, cx);
            })
            .unwrap();
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let row = visual
            .debug_bounds("context-compaction-row")
            .expect("status row");
        if state != Status::Failed {
            let icon = visual
                .debug_bounds("context-compaction-icon-slot")
                .expect("icon");
            assert_eq!(row.size.height, px(Layout::DISCLOSURE_HEADER_HEIGHT));
            assert_eq!(icon.size.width, px(Layout::ICON_SIZE));
            assert_eq!(icon.origin.y, row.origin.y);
        } else {
            assert!(
                visual
                    .debug_bounds("context-compaction-icon-slot")
                    .is_none()
            );
        }
        if state == Status::Failed {
            visual.simulate_resize(gpui_kit::size(px(160.), px(700.)));
            visual.run_until_parked();
            visual.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
            let wrapped = visual
                .debug_bounds("context-compaction-row")
                .expect("wrapped status");

            assert!(
                wrapped.size.height > row.size.height,
                "wide={row:?}, narrow={wrapped:?}"
            );
        }
    }
}

#[gpui_kit::test]
async fn issue365_reopened_context_error_uses_owned_shared_notice(cx: &mut TestAppContext) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            use vega_conversation::history::{AssistantStatus, HistoryEntry, HistoryPage};
            stream.apply_history_page(
                HistoryPage {
                    entries: vec![
                        HistoryEntry::AssistantText {
                            seq: 1,
                            message_id: "budget-run".into(),
                            content: String::new(),
                            status: AssistantStatus::Failed,
                            execution_duration_ms: Some(1),
                        },
                        HistoryEntry::AssistantText {
                            seq: 2,
                            message_id: "provider-run".into(),
                            content: String::new(),
                            status: AssistantStatus::Failed,
                            execution_duration_ms: Some(1),
                        },
                    ],
                    older_cursor: None,
                    newer_cursor: None,
                    newest_seq: None,
                },
                cx,
            );
            stream.restore_context_run_failures(
                &[("budget-run".into(), RunFailureKind::ContextOverLimit)],
                cx,
            );
            let mut record = status(1, Status::Failed);
            record.failure = Some(Failure::OverLimit);
            assert!(stream.restore_owned_context_status(
                "context-thread",
                "mock",
                record,
                Some("budget-run"),
                cx
            ));
            stream.refresh_run_notices(cx);
            assert_eq!(stream.run_notices.len(), 2);
            let budget = stream
                .run_notices
                .values()
                .find(|notice| notice.message_id == "budget-run")
                .unwrap();
            assert!(matches!(
                budget.kind,
                run_notice::RunNoticeKind::ContextFailure
            ));
            assert!(budget.text().contains("上下文超出预算"));
            let provider = stream
                .run_notices
                .values()
                .find(|notice| notice.message_id == "provider-run")
                .unwrap();
            assert!(matches!(
                provider.kind,
                run_notice::RunNoticeKind::Failure(RunFailureKind::Persisted)
            ));
            assert!(stream.compaction_failure_is_represented(stream.entries.len() - 1));
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-notice-budget-run").is_some());
    assert!(visual.debug_bounds("run-notice-provider-run").is_some());
    assert!(visual.debug_bounds("context-compaction-row").is_none());
}

#[gpui_kit::test]
async fn issue372_live_reasoning_failure_has_one_owned_actionable_surface(cx: &mut TestAppContext) {
    use vega_conversation::types::{ConversationEvent, ReasoningBudgetScope};
    for scope in [
        ReasoningBudgetScope::Delta,
        ReasoningBudgetScope::Turn,
        ReasoningBudgetScope::Run,
    ] {
        let window = setup(cx);
        window
            .update(cx, |stream, _, cx| {
                stream.begin_composer_run(cx);
                stream.apply_event(
                    ConversationEvent::MessageStarted {
                        message_id: "thinking-run".into(),
                        seq: 1,
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::ThinkingDelta {
                        message_id: "thinking-run".into(),
                        delta: "owned thinking".into(),
                    },
                    cx,
                );
                if scope == ReasoningBudgetScope::Run {
                    let mut record = status(1, Status::Failed);
                    record.failure = Some(Failure::ReasoningLimit);
                    stream.apply_context_status("context-thread", "mock", record, cx);
                }
                stream.apply_event(
                    ConversationEvent::Error {
                        message_id: Some("thinking-run".into()),
                        error: std::sync::Arc::new(
                            vega_runtime::VegaError::ReasoningBudgetExceeded {
                                scope,
                                limit_bytes: 1,
                                observed_bytes: 2,
                            },
                        ),
                        execution_duration_ms: Some(1),
                    },
                    cx,
                );
                stream.refresh_run_notices(cx);
                assert_eq!(stream.run_notices.len(), 1);
                let notice = stream.run_notices.values().next().unwrap();
                assert_eq!(notice.message_id, "thinking-run");
                assert!(notice.text().contains("思考内容超出限制"));
                assert!(notice.text().contains("思考强度"));
                assert!(!notice.text().contains("运行环境"));
                assert!(!notice.text().contains("供应商"));
                assert!(stream.controller_error_is_run_notice());
                stream.actions.running = false;
                stream.restore_context_run_failures(
                    &[("thinking-run".into(), RunFailureKind::ReasoningLimit(None))],
                    cx,
                );
                assert_eq!(stream.run_notices.len(), 1);
                assert!(stream.controller_error_is_run_notice());
            })
            .unwrap();
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("run-notice-thinking-run").is_some());
        assert!(
            visual
                .debug_bounds("conversation-controller-error")
                .is_none()
        );
        assert!(visual.debug_bounds("context-compaction-row").is_none());
    }
}

#[gpui_kit::test]
async fn issue372_reopened_steered_reasoning_error_keeps_one_card_and_independent_failure(
    cx: &mut TestAppContext,
) {
    use vega_conversation::history::{AssistantStatus, HistoryEntry, HistoryPage};
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.apply_history_page(
                HistoryPage {
                    entries: vec![
                        HistoryEntry::AssistantText {
                            seq: 2,
                            message_id: "original-run".into(),
                            content: "completed before steer".into(),
                            status: AssistantStatus::Done,
                            execution_duration_ms: None,
                        },
                        HistoryEntry::AssistantText {
                            seq: 4,
                            message_id: "steered-failure".into(),
                            content: String::new(),
                            status: AssistantStatus::Failed,
                            execution_duration_ms: Some(1),
                        },
                        HistoryEntry::AssistantText {
                            seq: 6,
                            message_id: "other-failure".into(),
                            content: String::new(),
                            status: AssistantStatus::Failed,
                            execution_duration_ms: Some(1),
                        },
                    ],
                    older_cursor: None,
                    newer_cursor: None,
                    newest_seq: None,
                },
                cx,
            );
            stream.restore_context_run_failures(
                &[(
                    "steered-failure".into(),
                    RunFailureKind::ReasoningLimit(None),
                )],
                cx,
            );
            let mut record = status(1, Status::Failed);
            record.failure = Some(Failure::ReasoningLimit);
            assert!(stream.restore_owned_context_status(
                "context-thread",
                "mock",
                record,
                Some("steered-failure"),
                cx
            ));
            stream.refresh_run_notices(cx);
            assert_eq!(stream.run_notices.len(), 2);
            let reasoning = stream
                .run_notices
                .values()
                .filter(|notice| {
                    matches!(
                        notice.kind,
                        run_notice::RunNoticeKind::Failure(RunFailureKind::ReasoningLimit(_))
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(reasoning.len(), 1);
            assert_eq!(reasoning[0].message_id, "steered-failure");
            assert!(reasoning[0].text().contains("思考内容超出限制"));
            assert!(!reasoning[0].text().contains("运行环境"));
            assert!(
                stream
                    .run_notices
                    .values()
                    .any(|notice| notice.message_id == "other-failure"
                        && notice.kind
                            == run_notice::RunNoticeKind::Failure(RunFailureKind::Persisted))
            );
            assert!(
                !stream
                    .run_notices
                    .values()
                    .any(|notice| notice.message_id == "original-run")
            );
            assert!(stream.compaction_failure_is_represented(stream.entries.len() - 1));
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-notice-steered-failure").is_some());
    assert!(visual.debug_bounds("run-notice-other-failure").is_some());
    assert!(visual.debug_bounds("run-notice-original-run").is_none());
    assert!(visual.debug_bounds("context-compaction-row").is_none());
    assert!(
        visual
            .debug_bounds("conversation-controller-error")
            .is_none()
    );
}
