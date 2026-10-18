use super::*;

use gpui_kit::{TestAppContext, WindowHandle};
use vega_conversation::types::{
    ContextCompactionUsageState, TaskBackend, ThreadMode, ThreadStatus, ToolCall, ToolCallStatus,
    ToolResult,
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
                        parent_thread_id: None,
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
async fn active_run_phases_follow_tools_and_owned_compaction_without_resetting_clock(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.begin_composer_run(cx);
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "phase-run".into(),
                    seq: 1,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::RunStarted {
                    message_id: "phase-run".into(),
                    started_at: Instant::now() - std::time::Duration::from_secs(22),
                },
                cx,
            );
            let group = stream.run_activity_groups["phase-run"].clone();
            let started_at = group.read(cx).test_clock().0;
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: ToolCall {
                        id: "phase-tool".into(),
                        tool: "bash".into(),
                        input_json: r#"{"cmd":"pwd"}"#.into(),
                    },
                },
                cx,
            );
            assert!(matches!(
                group.read(cx).test_clock().2,
                run_activity::RunPhase::Working
            ));
            assert_eq!(group.read(cx).test_label(), "Working for 22s");
            stream.apply_event(
                ConversationEvent::ToolCallRunning {
                    call_id: "phase-tool".into(),
                },
                cx,
            );
            assert!(matches!(
                group.read(cx).test_clock().2,
                run_activity::RunPhase::Tool
            ));
            assert_eq!(group.read(cx).test_label(), "Working for 22s");
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: "phase-tool".into(),
                    result: ToolResult {
                        status: ToolCallStatus::Success,
                        output: "observed".into(),
                        reused: false,
                        exit_code: Some(0),
                        duration_ms: Some(1),
                        truncated: Some(false),
                        invalid: None,
                    },
                },
                cx,
            );
            assert!(matches!(
                group.read(cx).test_clock().2,
                run_activity::RunPhase::Working
            ));
            assert_eq!(group.read(cx).test_label(), "Working for 22s");
            assert!(!stream.apply_context_status(
                "other-thread",
                "mock",
                status(1, Status::Compacting),
                cx
            ));
            assert!(matches!(
                group.read(cx).test_clock().2,
                run_activity::RunPhase::Working
            ));
            assert_eq!(group.read(cx).test_label(), "Working for 22s");
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(1, Status::Compacting),
                cx
            ));
            assert!(matches!(
                group.read(cx).test_clock().2,
                run_activity::RunPhase::Compacting
            ));
            assert_eq!(group.read(cx).test_label(), "Working for 22s");
            group.update(cx, |group, cx| group.set_waiting(true, cx));
            assert_eq!(group.read(cx).test_label(), "Awaiting confirmation for 22s");
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(1, Status::Succeeded),
                cx
            ));
            assert_eq!(group.read(cx).test_label(), "Awaiting confirmation for 22s");
            group.update(cx, |group, cx| group.set_waiting(false, cx));
            assert!(matches!(
                group.read(cx).test_clock().2,
                run_activity::RunPhase::Working
            ));
            assert_eq!(group.read(cx).test_label(), "Working for 22s");
            stream.apply_event(
                ConversationEvent::AssistantResponseStarted {
                    message_id: "phase-run".into(),
                    text_start_bytes: 0,
                },
                cx,
            );
            assert!(matches!(
                group.read(cx).test_clock().2,
                run_activity::RunPhase::Requesting
            ));
            assert_eq!(group.read(cx).test_label(), "Working for 22s");
            assert!(!stream.apply_context_status(
                "context-thread",
                "mock",
                status(1, Status::Compacting),
                cx
            ));
            assert!(matches!(
                group.read(cx).test_clock().2,
                run_activity::RunPhase::Requesting
            ));
            assert_eq!(group.read(cx).test_label(), "Working for 22s");
            assert_eq!(group.read(cx).test_clock().0, started_at);
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "phase-run".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                    execution_duration_ms: Some(23_000),
                },
                cx,
            );
            assert_eq!(group.read(cx).test_label(), "Worked for 23s");
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(2, Status::Compacting),
                cx
            ));
            assert_eq!(group.read(cx).test_label(), "Worked for 23s");
            assert!(!group.read(cx).test_clock().1);
        })
        .unwrap();
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
async fn compaction_failure_same_typed_cause_has_one_owned_notice(cx: &mut TestAppContext) {
    for status_first in [true, false] {
        let window = setup(cx);
        window
            .update(cx, |stream, _, cx| {
                stream.begin_composer_run(cx);
                stream.apply_event(
                    ConversationEvent::MessageStarted {
                        message_id: "owned-run".into(),
                        seq: 1,
                    },
                    cx,
                );
                let failure = RunFailureKind::Compaction(Failure::SourceChanged);
                let failed_compaction_index = if status_first {
                    let mut record = status(1, Status::Failed);
                    record.failure = Some(Failure::SourceChanged);
                    assert!(stream.apply_context_status("context-thread", "mock", record, cx));
                    let index = stream.entries.len() - 1;
                    let identity = stream.durable_entry_identity("owned-run", "compaction-1", None);
                    stream.set_entry_identity(index, identity);
                    Some(index)
                } else {
                    None
                };
                stream.apply_event(
                    ConversationEvent::Error {
                        message_id: Some("owned-run".into()),
                        error: std::sync::Arc::new(vega_runtime::VegaError::Context(
                            vega_runtime::ContextRuntimeError::SourceChanged,
                        )),
                        execution_duration_ms: Some(1),
                    },
                    cx,
                );
                let compaction_index = match failed_compaction_index {
                    Some(index) => index,
                    None => {
                        let mut record = status(1, Status::Failed);
                        record.failure = Some(Failure::SourceChanged);
                        assert!(stream.apply_context_status("context-thread", "mock", record, cx));
                        let index = stream.entries.len() - 1;
                        let identity =
                            stream.durable_entry_identity("owned-run", "compaction-1", None);
                        stream.set_entry_identity(index, identity);
                        index
                    }
                };
                stream.refresh_run_notices(cx);

                assert_eq!(stream.run_notices.len(), 1);
                let notice_index = *stream.run_notices.keys().next().expect("run notice");
                assert_eq!(
                    stream.run_notices[&notice_index].kind,
                    run_notice::RunNoticeKind::Failure(failure)
                );
                assert_eq!(
                    stream.run_notices[&notice_index].text(),
                    status_label_for(Status::Failed, Some(Failure::SourceChanged))
                );
                assert!(stream.compaction_failure_is_represented(compaction_index));
                assert!(stream.compaction_reports_run_failure("owned-run", failure));
                assert!(!stream.compaction_reports_run_failure("other-run", failure));
                assert!(!stream.compaction_reports_run_failure(
                    "owned-run",
                    RunFailureKind::Compaction(Failure::TimedOut)
                ));
                assert!(stream.controller_error_is_run_notice());
            })
            .unwrap();
    }
}

#[gpui_kit::test]
async fn compaction_failure_preserves_provider_cause_in_both_arrival_orders(
    cx: &mut TestAppContext,
) {
    for provider_first in [true, false] {
        let window = setup(cx);
        window
            .update(cx, |stream, _, cx| {
                stream.begin_composer_run(cx);
                stream.apply_event(
                    ConversationEvent::MessageStarted {
                        message_id: "owned-run".into(),
                        seq: 1,
                    },
                    cx,
                );
                if provider_first {
                    stream.apply_event(
                        ConversationEvent::Error {
                            message_id: Some("owned-run".into()),
                            error: std::sync::Arc::new(vega_runtime::VegaError::Provider {
                                status: Some(401),
                                message: "unauthorized".into(),
                                retryable: false,
                            }),
                            execution_duration_ms: Some(1),
                        },
                        cx,
                    );
                    stream.apply_event(
                        ConversationEvent::Error {
                            message_id: Some("owned-run".into()),
                            error: std::sync::Arc::new(vega_runtime::VegaError::Context(
                                vega_runtime::ContextRuntimeError::SummaryFormatInvalid,
                            )),
                            execution_duration_ms: Some(2),
                        },
                        cx,
                    );
                } else {
                    stream.apply_event(
                        ConversationEvent::Error {
                            message_id: Some("owned-run".into()),
                            error: std::sync::Arc::new(vega_runtime::VegaError::Context(
                                vega_runtime::ContextRuntimeError::SummaryFormatInvalid,
                            )),
                            execution_duration_ms: Some(1),
                        },
                        cx,
                    );
                    stream.apply_event(
                        ConversationEvent::Error {
                            message_id: Some("owned-run".into()),
                            error: std::sync::Arc::new(vega_runtime::VegaError::Provider {
                                status: Some(401),
                                message: "unauthorized".into(),
                                retryable: false,
                            }),
                            execution_duration_ms: Some(2),
                        },
                        cx,
                    );
                }
                stream.apply_event(
                    ConversationEvent::Error {
                        message_id: Some("owned-run".into()),
                        error: std::sync::Arc::new(vega_runtime::VegaError::Tool {
                            tool: "fixture".into(),
                            message: "unavailable".into(),
                        }),
                        execution_duration_ms: Some(3),
                    },
                    cx,
                );
                let mut record = status(1, Status::Failed);
                record.failure = Some(Failure::InvalidSummary);
                assert!(stream.apply_context_status("context-thread", "mock", record, cx));
                let index = stream.entries.len() - 1;
                let identity = stream.durable_entry_identity("owned-run", "compaction-1", None);
                stream.set_entry_identity(index, identity);
                stream.refresh_run_notices(cx);

                let notice = &stream.run_notices[&index];
                assert_eq!(stream.run_notices.len(), 1);
                assert_eq!(
                    notice.kind,
                    run_notice::RunNoticeKind::FailureWithCompaction(
                        RunFailureKind::ProviderAuthorization,
                        Some(Failure::InvalidSummary)
                    )
                );
                assert_eq!(notice.text().lines().count(), 2);
                assert!(notice.text().contains("rejected your credentials"));
                assert!(notice.text().contains("compaction result was invalid"));
                assert!(!notice.text().contains("runtime environment"));
                assert!(stream.compaction_failure_is_represented(index));
            })
            .unwrap();
    }
}

#[gpui_kit::test]
async fn compaction_source_change_survives_late_generic_failure_and_interruption(
    cx: &mut TestAppContext,
) {
    for source_changed_first in [true, false] {
        let window = setup(cx);
        window
            .update(cx, |stream, _, cx| {
                stream.begin_composer_run(cx);
                stream.apply_event(
                    ConversationEvent::MessageStarted {
                        message_id: "source-changed-run".into(),
                        seq: 1,
                    },
                    cx,
                );
                let source_changed = || ConversationEvent::Error {
                    message_id: Some("source-changed-run".into()),
                    error: std::sync::Arc::new(vega_runtime::VegaError::Context(
                        vega_runtime::ContextRuntimeError::SourceChanged,
                    )),
                    execution_duration_ms: Some(1),
                };
                let generic = || ConversationEvent::Error {
                    message_id: Some("source-changed-run".into()),
                    error: std::sync::Arc::new(vega_runtime::VegaError::Tool {
                        tool: "fixture".into(),
                        message: "consumer unavailable".into(),
                    }),
                    execution_duration_ms: Some(2),
                };
                if source_changed_first {
                    stream.apply_event(source_changed(), cx);
                    stream.apply_event(generic(), cx);
                } else {
                    stream.apply_event(generic(), cx);
                    stream.apply_event(source_changed(), cx);
                }
                stream.apply_event(
                    ConversationEvent::Interrupted {
                        message_id: "source-changed-run".into(),
                        execution_duration_ms: Some(3),
                    },
                    cx,
                );
                stream.refresh_run_notices(cx);
                assert_eq!(stream.run_notices.len(), 1);
                let notice = stream.run_notices.values().next().unwrap();
                assert_eq!(
                    notice.kind,
                    run_notice::RunNoticeKind::Failure(RunFailureKind::Compaction(
                        Failure::SourceChanged
                    ))
                );
                assert_eq!(
                    notice.text(),
                    status_label_for(Status::Failed, Some(Failure::SourceChanged))
                );
                assert!(!notice.text().contains("runtime environment"));
            })
            .unwrap();
    }

    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.begin_composer_run(cx);
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "cancelled-run".into(),
                    seq: 1,
                },
                cx,
            );
            let mut cancelled = status(1, Status::Cancelled);
            cancelled.failure = None;
            assert!(stream.apply_context_status("context-thread", "mock", cancelled, cx));
            assert!(!stream.run_failures.contains_key("cancelled-run"));
            assert!(stream.run_notice_projection(cx).is_empty());
        })
        .unwrap();
}

#[gpui_kit::test]
async fn compaction_failure_hydration_drops_resolved_cause_and_keeps_provider_error(
    cx: &mut TestAppContext,
) {
    use vega_conversation::history::{AssistantStatus, HistoryEntry, HistoryPage};

    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.apply_history_page(
                HistoryPage {
                    subagents: Vec::new(),
                    entries: vec![HistoryEntry::AssistantText {
                        seq: 1,
                        message_id: "owned-run".into(),
                        content: String::new(),
                        status: AssistantStatus::Failed,
                        execution_duration_ms: Some(1),
                    }],
                    older_cursor: None,
                    newer_cursor: None,
                    newest_seq: None,
                },
                cx,
            );
            stream.restore_context_run_failures(
                &[(
                    "owned-run".into(),
                    RunFailureKind::Compaction(Failure::SourceChanged),
                )],
                cx,
            );
            stream.refresh_run_notices(cx);
            assert_eq!(
                stream.run_notices.values().next().unwrap().kind,
                run_notice::RunNoticeKind::Failure(RunFailureKind::Compaction(
                    Failure::SourceChanged
                ))
            );

            assert_eq!(stream.reserve_context_operation_id(cx), Some(1));
            assert!(stream.restore_owned_context_status(
                "context-thread",
                "mock",
                status(2, Status::Succeeded),
                Some("owned-run"),
                cx
            ));
            assert!(matches!(
                stream.entries.last(),
                Some(StreamEntry::ContextCompaction { record, .. })
                    if record.generation == 2 && record.status == Status::Succeeded
            ));
            let success_index = stream.entries.len() - 1;

            stream.set_entry_identity(
                success_index,
                stream.durable_entry_identity("other-run", "compaction-2", None),
            );
            assert!(
                !stream.compaction_failure_resolved_for_owner("owned-run", Failure::SourceChanged)
            );
            let unrelated_owner = stream.run_notice_projection(cx);
            assert_eq!(unrelated_owner.len(), 1);
            assert_eq!(
                unrelated_owner.values().next().unwrap().kind,
                run_notice::RunNoticeKind::Failure(RunFailureKind::Compaction(
                    Failure::SourceChanged
                ))
            );

            stream.set_entry_identity(
                success_index,
                stream.durable_entry_identity("owned-run", "compaction-2", None),
            );
            if let StreamEntry::ContextCompaction { model, .. } = &mut stream.entries[success_index]
            {
                *model = "other-model".into();
            }
            assert!(
                !stream.compaction_failure_resolved_for_owner("owned-run", Failure::SourceChanged)
            );
            assert_eq!(stream.run_notice_projection(cx).len(), 1);
            if let StreamEntry::ContextCompaction { model, .. } = &mut stream.entries[success_index]
            {
                *model = "mock".into();
            }
            assert!(stream.run_notice_projection(cx).is_empty());
            stream.apply_agent_runtime_error_for(
                Some("owned-run"),
                RunFailureKind::ProviderAuthorization,
                cx,
            );
            stream.refresh_run_notices(cx);
            assert_eq!(stream.run_notices.len(), 1);
            assert_eq!(
                stream.run_notices.values().next().unwrap().kind,
                run_notice::RunNoticeKind::Failure(RunFailureKind::ProviderAuthorization)
            );
            assert!(stream.controller_error_is_run_notice());
        })
        .unwrap();
}

#[gpui_kit::test]
async fn compaction_failure_live_resolution_clears_controller_error_and_keeps_provider_failure(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.begin_composer_run(cx);
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "owned-run".into(),
                    seq: 1,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::Error {
                    message_id: Some("owned-run".into()),
                    error: std::sync::Arc::new(vega_runtime::VegaError::Context(
                        vega_runtime::ContextRuntimeError::SourceChanged,
                    )),
                    execution_duration_ms: Some(1),
                },
                cx,
            );
            stream.refresh_run_notices(cx);
            assert!(stream.controller_error_is_run_notice());

            let mut failed = status(1, Status::Failed);
            failed.failure = Some(Failure::SourceChanged);
            assert!(stream.apply_context_status("context-thread", "mock", failed, cx));
            let failed_index = stream.entries.len() - 1;
            stream.set_entry_identity(
                failed_index,
                stream.durable_entry_identity("owned-run", "compaction-1", None),
            );
            stream.refresh_run_notices(cx);
            assert_eq!(stream.run_notices.len(), 1);
            assert_eq!(
                stream.run_notices.values().next().unwrap().kind,
                run_notice::RunNoticeKind::Failure(RunFailureKind::Compaction(
                    Failure::SourceChanged
                ))
            );

            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(2, Status::Succeeded),
                cx
            ));
            let success_index = stream.entries.len() - 1;
            stream.set_entry_identity(
                success_index,
                stream.durable_entry_identity("owned-run", "compaction-2", None),
            );
            stream.refresh_run_notices(cx);
            assert!(stream.run_notice_projection(cx).is_empty());
            assert!(stream.run_notices.is_empty());
            assert!(stream.controller_error.is_none());
            assert!(stream.controller_runtime_error.is_none());

            stream.apply_agent_runtime_error_for(
                Some("owned-run"),
                RunFailureKind::ProviderAuthorization,
                cx,
            );
            stream.refresh_run_notices(cx);
            assert_eq!(stream.run_notices.len(), 1);
            assert_eq!(
                stream.run_notices.values().next().unwrap().kind,
                run_notice::RunNoticeKind::Failure(RunFailureKind::ProviderAuthorization)
            );
            assert!(stream.controller_error_is_run_notice());
        })
        .unwrap();
}

#[gpui_kit::test]
async fn compaction_failure_replaces_generic_notice_without_composer_duplicate(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.remember_run_failure("owned-run", RunFailureKind::Runtime);
            let mut record = status(1, Status::Failed);
            record.failure = Some(Failure::TimedOut);
            assert!(stream.apply_context_status("context-thread", "mock", record, cx));
            let index = stream.entries.len() - 1;
            let identity = stream.durable_entry_identity("owned-run", "compaction-1", None);
            stream.set_entry_identity(index, identity);
            stream.controller_runtime_error = Some(("owned-run".into(), RunFailureKind::Runtime));
            stream.controller_error = Some(RunFailureKind::Runtime.message());
            stream.refresh_run_notices(cx);

            assert_eq!(stream.run_notices.len(), 1);
            assert_eq!(
                stream.run_notices[&index].kind,
                run_notice::RunNoticeKind::CompactionFailure(Some(Failure::TimedOut))
            );
            assert!(stream.controller_error_is_run_notice());
        })
        .unwrap();
}

#[gpui_kit::test]
async fn restored_provider_failures_keep_the_specific_notice(cx: &mut TestAppContext) {
    let window = setup(cx);
    window.update(cx, |stream, _, cx| {
        use vega_conversation::history::{AssistantStatus, HistoryEntry, HistoryPage};
        let kinds = [RunFailureKind::ProviderOutputLimit, RunFailureKind::ProviderProtocol, RunFailureKind::ProviderTransport, RunFailureKind::ProviderRejected, RunFailureKind::ProviderUnknown];
        stream.apply_history_page(HistoryPage {
            subagents: Vec::new(),
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
