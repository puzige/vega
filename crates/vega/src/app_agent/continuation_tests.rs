use super::{ActiveAgentRun, AppAgentController, RunCompletionOutcome};
use gpui_kit::TestAppContext;
use vega_conversation::types::{
    ConversationEvent, ConversationStopReason, PermissionMode, RunFailureKind, TaskBackend, Thread,
    ThreadMode, ThreadStatus,
};
use vega_ui::conversation_stream::ConversationStream;

#[gpui_kit::test]
fn continuation_keeps_controller_active_and_queue_blocked_until_final_terminal(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::dark());
    });
    let (stream, _) =
        cx.add_window_view(|_, cx| ConversationStream::new(continuation_thread(), cx));
    let mut controller = AppAgentController::default();
    let (generation, cancel) = controller
        .begin("thread".into(), stream.clone(), None, None)
        .unwrap();
    for event in [
        ConversationEvent::MessageStarted {
            message_id: "previous".into(),
            seq: 1,
        },
        ConversationEvent::RunContinued {
            previous_assistant_id: "previous".into(),
            user_message_id: "user".into(),
            next_assistant_id: "next".into(),
        },
        ConversationEvent::PendingMessageDelivered {
            pending_id: "user".into(),
            message_id: "user".into(),
            seq: 2,
            content: "Continue".into(),
            images: Vec::new(),
        },
        ConversationEvent::MessageStarted {
            message_id: "next".into(),
            seq: 3,
        },
    ] {
        controller.observe_terminal_message(generation, "thread", &stream, &event);
        assert!(controller.matches(generation, "thread", &stream));
        let active = &controller.active["thread"];
        assert!(!active.queue_ready);
        assert!(active.completion_outcome.is_none());
        assert!(active.terminal_message_id.is_none());
        assert!(!cancel.is_cancelled());
        assert!(
            controller
                .begin("thread".into(), stream.clone(), None, None)
                .is_none()
        );
    }
    controller.observe_terminal_message(
        generation,
        "thread",
        &stream,
        &ConversationEvent::MessageFinished {
            message_id: "next".into(),
            stop_reason: ConversationStopReason::End,
            execution_duration_ms: Some(15000),
        },
    );
    let active = &controller.active["thread"];
    assert!(active.queue_ready);
    assert!(matches!(
        active.completion_outcome,
        Some(RunCompletionOutcome::Completed)
    ));
    assert_eq!(active.terminal_message_id.as_deref(), Some("next"));
}

#[gpui_kit::test]
fn predecessor_terminals_preserve_current_run_before_and_after_completion(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::dark());
    });
    let (stream, _) =
        cx.add_window_view(|_, cx| ConversationStream::new(continuation_thread(), cx));
    for continuation_count in [1, 2] {
        let current_id = if continuation_count == 1 {
            "next"
        } else {
            "final"
        };
        for (terminal_index, current_terminal) in
            terminal_events(current_id).into_iter().enumerate()
        {
            let mut controller = AppAgentController::default();
            let (generation, cancel) = controller
                .begin("thread".into(), stream.clone(), None, None)
                .unwrap();
            controller.observe_terminal_message(
                generation,
                "thread",
                &stream,
                &ConversationEvent::MessageStarted {
                    message_id: "previous".into(),
                    seq: 1,
                },
            );
            for (index, (previous_id, next_id)) in [("previous", "next"), ("next", "final")]
                .into_iter()
                .take(continuation_count)
                .enumerate()
            {
                let continued = ConversationEvent::RunContinued {
                    previous_assistant_id: previous_id.into(),
                    user_message_id: format!("user-{index}"),
                    next_assistant_id: next_id.into(),
                };
                for event in [
                    continued.clone(),
                    continued.clone(),
                    ConversationEvent::MessageStarted {
                        message_id: next_id.into(),
                        seq: 3 + index as u64 * 2,
                    },
                    continued,
                ] {
                    controller.observe_terminal_message(generation, "thread", &stream, &event);
                    assert_eq!(
                        terminal_state(&controller.active["thread"]),
                        (false, None, None, None)
                    );
                    assert!(controller.matches(generation, "thread", &stream));
                    assert!(!cancel.is_cancelled());
                }
            }
            for predecessor_id in ["previous", "next"].into_iter().take(continuation_count) {
                for event in terminal_events(predecessor_id) {
                    controller.observe_terminal_message(generation, "thread", &stream, &event);
                    assert_eq!(
                        terminal_state(&controller.active["thread"]),
                        (false, None, None, None)
                    );
                    assert!(controller.matches(generation, "thread", &stream));
                    assert!(!cancel.is_cancelled());
                }
            }
            let expected = match terminal_index {
                0 => (true, Some("completed"), Some(current_id), None),
                1 => (
                    false,
                    Some("failed"),
                    Some(current_id),
                    Some(RunFailureKind::ProviderRateLimited),
                ),
                2 => (false, Some("interrupted"), Some(current_id), None),
                _ => unreachable!(),
            };
            controller.observe_terminal_message(generation, "thread", &stream, &current_terminal);
            assert_eq!(terminal_state(&controller.active["thread"]), expected);
            for predecessor_id in ["previous", "next"].into_iter().take(continuation_count) {
                for event in terminal_events(predecessor_id) {
                    controller.observe_terminal_message(generation, "thread", &stream, &event);
                    assert_eq!(terminal_state(&controller.active["thread"]), expected);
                    assert!(controller.matches(generation, "thread", &stream));
                    assert!(!cancel.is_cancelled());
                }
            }
            assert!(
                controller
                    .begin("thread".into(), stream.clone(), None, None)
                    .is_none()
            );
            let finished = controller.finish(generation, "thread", &stream).unwrap();
            assert_eq!(terminal_state(&finished), expected);
            assert!(!finished.cancel.is_cancelled());
            assert!(!controller.matches(generation, "thread", &stream));
        }
    }
}

#[gpui_kit::test]
fn continuation_filter_is_run_owned_and_startup_errors_remain_failures(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::dark());
    });
    let (stream, _) =
        cx.add_window_view(|_, cx| ConversationStream::new(continuation_thread(), cx));
    let (other_stream, _) =
        cx.add_window_view(|_, cx| ConversationStream::new(continuation_thread(), cx));
    let mut controller = AppAgentController::default();
    for (generation_mismatch, thread_id, event_stream) in [
        (true, "thread", &stream),
        (false, "other-thread", &stream),
        (false, "thread", &other_stream),
    ] {
        let (generation, cancel) = controller
            .begin("thread".into(), stream.clone(), None, None)
            .unwrap();
        controller.observe_terminal_message(
            generation,
            "thread",
            &stream,
            &ConversationEvent::MessageStarted {
                message_id: "previous".into(),
                seq: 1,
            },
        );
        let event_generation = generation + u64::from(generation_mismatch);
        for event in std::iter::once(ConversationEvent::RunContinued {
            previous_assistant_id: "previous".into(),
            user_message_id: "user".into(),
            next_assistant_id: "next".into(),
        })
        .chain(terminal_events("previous"))
        {
            controller.observe_terminal_message(event_generation, thread_id, event_stream, &event);
            assert_eq!(
                terminal_state(&controller.active["thread"]),
                (false, None, None, None)
            );
            assert!(controller.matches(generation, "thread", &stream));
            assert!(!cancel.is_cancelled());
        }
        controller.observe_terminal_message(
            generation,
            "thread",
            &stream,
            &terminal_events("previous")[0],
        );
        let expected = (true, Some("completed"), Some("previous"), None);
        assert_eq!(terminal_state(&controller.active["thread"]), expected);
        assert!(
            controller
                .finish(event_generation, thread_id, event_stream)
                .is_none()
        );
        assert!(controller.matches(generation, "thread", &stream));
        let finished = controller.finish(generation, "thread", &stream).unwrap();
        assert_eq!(terminal_state(&finished), expected);
        assert!(!cancel.is_cancelled());
    }
    let (generation, cancel) = controller
        .begin("thread".into(), stream.clone(), None, None)
        .unwrap();
    controller.observe_terminal_message(generation, "thread", &stream, &rate_limited_error(None));
    let expected = (
        false,
        Some("failed"),
        None,
        Some(RunFailureKind::ProviderRateLimited),
    );
    assert_eq!(terminal_state(&controller.active["thread"]), expected);
    assert!(controller.matches(generation, "thread", &stream));
    assert!(!cancel.is_cancelled());
    let finished = controller.finish(generation, "thread", &stream).unwrap();
    assert_eq!(terminal_state(&finished), expected);
    assert!(!controller.matches(generation, "thread", &stream));
}

fn continuation_thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: "thread".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Continuation".into(),
        mode: ThreadMode::Ask,
        permission_mode: PermissionMode::ReadOnly,
        model: "model".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    }
}

fn terminal_events(message_id: &str) -> [ConversationEvent; 3] {
    [
        ConversationEvent::MessageFinished {
            message_id: message_id.into(),
            stop_reason: ConversationStopReason::End,
            execution_duration_ms: Some(15000),
        },
        rate_limited_error(Some(message_id)),
        ConversationEvent::Interrupted {
            message_id: message_id.into(),
            execution_duration_ms: Some(15000),
        },
    ]
}

fn rate_limited_error(message_id: Option<&str>) -> ConversationEvent {
    ConversationEvent::Error {
        message_id: message_id.map(str::to_owned),
        error: std::sync::Arc::new(vega_runtime::VegaError::Provider {
            status: Some(429),
            message: "rate limited".into(),
            retryable: false,
        }),
        execution_duration_ms: None,
    }
}

fn terminal_state(
    run: &ActiveAgentRun,
) -> (
    bool,
    Option<&'static str>,
    Option<&str>,
    Option<RunFailureKind>,
) {
    (
        run.queue_ready,
        run.completion_outcome.map(|outcome| match outcome {
            RunCompletionOutcome::Completed => "completed",
            RunCompletionOutcome::Failed => "failed",
            RunCompletionOutcome::Interrupted => "interrupted",
        }),
        run.terminal_message_id.as_deref(),
        run.terminal_failure,
    )
}
