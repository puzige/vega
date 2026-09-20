use super::*;
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
    let mut record = status(1, Status::Failed);
    record.failure = Some(Failure::TooLarge);
    assert!(status_label(&record).contains("安全分段上限"));
    assert!(!status_label(&record).contains("模型容量"));
    record.failure = Some(Failure::OverLimit);
    assert!(status_label(&record).contains("本地上下文预算检查未通过"));
    assert!(!status_label(&record).contains("安全分段上限"));
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
async fn context_ui_composer_has_no_context_control_and_real_status_has_separate_band(
    cx: &mut TestAppContext,
) {
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
    let status_band = visual
        .debug_bounds("context-status-band")
        .expect("status band");
    let composer = visual
        .debug_bounds("composer-shell")
        .expect("composer shell");
    assert!(status_band.bottom() <= composer.top());
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
