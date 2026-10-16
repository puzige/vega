use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, WindowHandle};
use vega_conversation::types::{
    ContextAccountingRecord, ContextAccountingSource, ContextAccountingStage,
    ContextCompactionUsageState, TaskBackend, ThreadMode, ThreadStatus,
};

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
async fn restored_provider_failures_keep_the_specific_notice(cx: &mut TestAppContext) {
    let window = setup(cx);
    window.update(cx, |stream, _, cx| {
        use vega_conversation::history::{AssistantStatus, HistoryEntry, HistoryPage};
        let kinds = [RunFailureKind::ProviderOutputLimit, RunFailureKind::ProviderProtocol, RunFailureKind::ProviderTransport, RunFailureKind::ProviderRejected, RunFailureKind::ProviderUnknown];
        stream.apply_history_page(HistoryPage {
            entries: kinds.iter().enumerate().map(|(index, _)| HistoryEntry::AssistantText {
                seq: index as i64 + 1,
                message_id: format!("failure-{index}"),
                content: String::new(),
                status: AssistantStatus::Failed,
                execution_duration_ms: Some(1),
            }).collect(), older_cursor: None, newer_cursor: None, newest_seq: None,
        }, cx);
        let failures = kinds.iter().enumerate().map(|(index, kind)| (format!("failure-{index}"), *kind)).collect::<Vec<_>>();
        stream.restore_context_run_failures(&failures, cx);
        stream.refresh_run_notices(cx);
        assert_eq!(stream.run_notices.len(), kinds.len());
        for (message_id, kind) in failures {
            let notice = stream.run_notices.values().find(|notice| notice.message_id == message_id).unwrap();
            assert!(matches!(notice.kind, run_notice::RunNoticeKind::Failure(value) if value == kind));
            assert_eq!(notice.text(), kind.message());
            assert!(!notice.text().contains("Context compaction"));
        }
    }).unwrap();
}
