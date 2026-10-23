use super::{AppAgentController, RunCompletionOutcome};
use gpui_kit::{TestAppContext, gpui};
use vega_conversation::types::{
    ConversationEvent, ConversationStopReason, PermissionMode, TaskBackend, Thread, ThreadMode,
    ThreadStatus,
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
    let (stream, _) = cx.add_window_view(|_, cx| {
        ConversationStream::new(
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
            },
            cx,
        )
    });
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
