use super::*;

#[gpui::test]
async fn settings_keyboard_emits_scoped_requests_without_optimistic_state(cx: &mut TestAppContext) {
    let global_escapes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_escapes = global_escapes.clone();
    cx.update(|cx| {
        cx.on_action(move |_: &crate::settings::CloseSettings, _| {
            observed_escapes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        cx.bind_keys([gpui::KeyBinding::new(
            "escape",
            crate::settings::CloseSettings,
            Some("VegaWindow"),
        )])
    });
    let (window, stream, events) = open_controller_stream(cx, "settings-thread");
    window
        .update(cx, |_, window, cx| {
            stream.read(cx).compact_focus[0].clone().focus(window, cx)
        })
        .expect("mode trigger");
    cx.simulate_keystrokes(window.into(), "enter");
    cx.simulate_keystrokes(window.into(), "down right");
    cx.simulate_keystrokes(window.into(), "enter");
    window
        .update(cx, |_, window, cx| {
            stream.read(cx).compact_focus[1].clone().focus(window, cx)
        })
        .expect("permission trigger");
    cx.simulate_keystrokes(window.into(), "space");
    cx.simulate_keystrokes(window.into(), "down right right");
    cx.simulate_keystrokes(window.into(), "space");

    window
        .update(cx, |_, window, cx| {
            stream.read(cx).compact_focus[1].clone().focus(window, cx)
        })
        .expect("permission trigger");
    cx.simulate_keystrokes(window.into(), "space escape");
    assert!(!stream.read_with(cx, |stream, _| stream.permission_menu_open));
    assert_eq!(global_escapes.load(std::sync::atomic::Ordering::SeqCst), 0);
    cx.simulate_keystrokes(window.into(), "escape");
    assert_eq!(global_escapes.load(std::sync::atomic::Ordering::SeqCst), 1);

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
async fn credential_failure_keeps_draft_and_renders_recovery_error(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "credential-submit");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("keep this draft", cx));
        stream.submit_message(cx);
        stream.apply_credential_error(cx);
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        assert!(!stream.composer_submit_pending);
        assert_eq!(stream.input.read(cx).text(), "keep this draft");
        assert!(stream.entries.is_empty());
        assert!(stream.composer_history.is_empty());
        assert_eq!(
            stream.controller_error.as_deref(),
            Some("本地凭据缺失或无法读取，请在设置中重新填写 API Key 后重试")
        );
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds("conversation-controller-error")
            .is_some()
    );
}

#[gpui::test]
async fn reference_rejection_releases_submit_and_keeps_editable_draft(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "reference-rejection");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("总结 @missing.txt", cx));
        stream.submit_message(cx);
        stream.reject_composer_submission(cx);
        stream.apply_reference_error(FileReferenceFailureCode::Missing, cx);
    });
    let state = stream.read_with(cx, |stream, cx| {
        (
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.entries.len(),
            stream.controller_error.clone(),
        )
    });
    assert!(!state.0, "failed reference must release submit owner");
    assert_eq!(state.1, "总结 @missing.txt");
    assert_eq!(state.2, 0, "failed reference must not create a user echo");
    assert_eq!(
        state.3.as_deref(),
        Some(FileReferenceFailureCode::Missing.message())
    );
}

#[gpui::test]
async fn file_index_generation_overflow_fails_closed(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "reference-generation-overflow");
    stream.update(cx, |stream, _| {
        stream.file_index_generation = u64::MAX;
        assert!(
            stream.next_file_index_generation().is_none(),
            "generation overflow must not wrap and reuse an owner"
        );
        assert_eq!(stream.file_index_generation, u64::MAX);
    });
}

#[gpui::test]
async fn file_index_late_success_is_fenced_after_cancel(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "reference-late-result");
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    stream.update(cx, |stream, cx| {
        input.update(cx, |input, cx| input.set_text("@", cx));
        stream.sync_at_query(&input, cx);
    });
    let generation = stream.read_with(cx, |stream, _| stream.file_index_generation);
    assert!(stream.read_with(cx, |stream, _| stream.file_index_loading()));
    stream.update(cx, |stream, cx| stream.close_file_selector_and_cancel(cx));
    let accepted = stream.update(cx, |stream, cx| {
        stream.apply_file_index_result(
            generation,
            Ok(vega_conversation::types::FileIndexSnapshot {
                entries: vec!["late.txt".into()],
            }),
            cx,
        )
    });
    assert!(!accepted, "cancelled generation cannot reopen the selector");
    assert!(!stream.read_with(cx, |stream, _| stream.file_index_loaded()));
    assert!(stream.read_with(cx, |stream, _| stream.file_index_candidates().is_empty()));
}

#[gpui::test]
async fn failed_file_index_keys_restore_composer_scope(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "reference-failed-keys");
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    stream.update(cx, |stream, cx| {
        input.update(cx, |input, cx| input.set_text("@missing", cx));
        stream.sync_at_query(&input, cx);
    });
    let generation = stream.read_with(cx, |stream, _| stream.file_index_generation);
    stream.update(cx, |stream, cx| {
        assert!(stream.apply_file_index_result(
            generation,
            Err(FileIndexFailureCode::Traversal),
            cx
        ));
    });
    cx.run_until_parked();
    focus_composer(window, &stream, cx);

    // Tab reaches the real Retry focus stop without activating it.
    cx.simulate_keystrokes(window.into(), "tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                stream.read_with(cx, |stream, _| stream.file_retry_focus.is_focused(window))
            })
            .expect("retry focus")
    );
    assert!(stream.read_with(cx, |stream, _| stream.file_index_failure.is_some()));

    // Esc from that stop closes Failed and explicitly returns focus to input;
    // the next Enter is the Composer newline action, not FileSelect capture.
    cx.simulate_keystrokes(window.into(), "escape");
    assert!(
        window
            .update(cx, |_, window, cx| {
                stream
                    .read_with(cx, |stream, cx| stream.input.read(cx).focus_handle(cx))
                    .is_focused(window)
            })
            .expect("composer focus after escape")
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| {
            (
                stream.file_selector_wanted,
                stream.file_index_failure,
                stream.file_index_loading,
            )
        }),
        (false, None, false)
    );
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "@missing\n"
    );
}

#[gpui::test]
async fn failed_file_index_enter_retries_through_key_dispatch(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "reference-retry-key");
    let retries = Arc::new(Mutex::new(Vec::<FileIndexRetryRequested>::new()));
    let captured = retries.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &FileIndexRetryRequested, _| {
            if let Ok(mut retries) = captured.lock() {
                retries.push(event.clone());
            }
        })
        .detach();
    });
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    stream.update(cx, |stream, cx| {
        input.update(cx, |input, cx| input.set_text("@retry", cx));
        stream.sync_at_query(&input, cx);
    });
    let generation = stream.read_with(cx, |stream, _| stream.file_index_generation);
    stream.update(cx, |stream, cx| {
        assert!(stream.apply_file_index_result(
            generation,
            Err(FileIndexFailureCode::DeadlineExceeded),
            cx
        ));
    });
    cx.run_until_parked();
    focus_composer(window, &stream, cx);

    // The failed panel's Enter binding retries directly from the composer.
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        stream.read_with(cx, |stream, _| {
            (
                stream.file_selector_wanted,
                stream.file_index_loading,
                stream.file_index_failure,
            )
        }),
        (true, true, None)
    );
    let retries = retries.lock().expect("retry event capture");
    assert_eq!(retries.len(), 1);
    assert_eq!(retries[0].thread_id, "reference-retry-key");
}

#[gpui::test]
async fn unresolved_reasoning_authority_rejects_submit_but_missing_profile_defaults(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "reasoning-authority");
    stream.update(cx, ConversationStream::mark_reasoning_unavailable);
    assert!(stream.read_with(cx, |stream, _| {
        stream.reasoning_unavailable() && stream.frozen_reasoning_for_submit().is_err()
    }));

    stream.update(cx, ConversationStream::clear_reasoning_profile);
    assert!(stream.read_with(cx, |stream, _| {
        !stream.reasoning_unavailable() && stream.frozen_reasoning_for_submit() == Ok(None)
    }));
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
    assert_eq!(rows, 0, "completed summaries occupy no transcript rows");
    assert!(text.contains("任务摘要 · 完成"));
    assert!(text.contains("成本 US$0.135000"));
    assert!(text.contains("耗时 12.4s"));
    assert!(text.contains("工具 2 · 缓存命中 33%"));
}

#[gpui::test]
async fn transcript_hides_completed_statistics_but_keeps_failure_status(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "quiet-usage");
    stream.update(cx, |stream, cx| {
        for (id, outcome) in [
            ("complete", TaskSummaryOutcome::Completed),
            ("failed", TaskSummaryOutcome::Failed),
        ] {
            stream.apply_task_summary(
                TaskCostSummary {
                    message_id: id.into(),
                    outcome,
                    usage: None,
                    cost: SummaryCost::Unavailable,
                    duration_ms: None,
                    tool_count: 0,
                    cache_hit_percent: None,
                },
                cx,
            );
        }
    });
    cx.run_until_parked();
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(
        visual
            .debug_bounds("completed-task-summary-hidden")
            .expect("retained hidden summary")
            .size
            .height,
        px(0.)
    );
    assert!(
        visual
            .debug_bounds("task-outcome-status")
            .expect("visible failure")
            .size
            .height
            > px(0.)
    );
    assert_eq!(
        stream.read_with(&visual, |stream, _| stream.summary_cards.len()),
        2
    );
}

#[gpui::test]
async fn batch_finished_flush_materializes_the_final_committed_tail(cx: &mut TestAppContext) {
    // S8-T44 review P1-1: a batched ingress tail [TextDelta…, MessageFinished]
    // must materialize the frozen committed tail in finish_agent_message
    // itself — render's sync only covers the *active* turn, which is already
    // taken by the time the next frame runs.
    let (_window, stream, _) = open_controller_stream(cx, "batch-finish");
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
                message_id: "assistant".into(),
                delta: "```rust\nfn tail() {}\n```".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "assistant".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    let (frozen, committed, last_text, tail_kind) = stream.read_with(cx, |stream, _| {
        let index = match stream.active_agent_message.as_ref() {
            Some((_, index)) => *index,
            None => stream
                .last_finished_agent_message
                .as_ref()
                .map(|(_, index)| *index)
                .expect("the finished turn stays booked"),
        };
        match &stream.entries[index] {
            StreamEntry::Assistant { model, .. } => {
                let lines = &model.committed_lines;
                (
                    stream
                        .counters
                        .frozen_rematerializations
                        .load(Ordering::Relaxed),
                    stream
                        .counters
                        .committed_materializations
                        .load(Ordering::Relaxed),
                    lines
                        .last()
                        .map(|line| {
                            line.spans
                                .iter()
                                .map(|span| span.text.as_str())
                                .collect::<String>()
                        })
                        .unwrap_or_default(),
                    lines.last().map(|line| line.kind),
                )
            }
            _ => panic!("the finished entry is an assistant turn"),
        }
    });
    assert_eq!(frozen, 0, "finish materialization is not a frozen remat");
    assert!(committed >= 1, "the final tail block was materialized");
    assert_eq!(tail_kind, Some(LineKind::Code), "the tail block froze");
    assert_eq!(last_text, "fn tail() {}", "the closing fence line survives");
}
