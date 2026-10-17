use super::*;
use gpui_kit::{Modifiers, VisualTestContext};
use vega_conversation::types::{
    AutomationCommand, AutomationControl, AutomationKind, AutomationProjection, AutomationStatus,
    AutomationTask,
};
use vega_runtime::{
    RuntimeUserQuestionHook, RuntimeUserQuestionRequest, RuntimeUserQuestionResponse,
    UserQuestionInput,
};

fn visible(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .is_some()
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual.debug_bounds(selector).unwrap();
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

fn goal(status: AutomationStatus) -> AutomationTask {
    AutomationTask {
        thread_id: "automation-ui".into(),
        kind: AutomationKind::Goal,
        identity: "goal".into(),
        revision: 1,
        instruction: "Verify the full requested result".into(),
        status,
        token_budget: Some(100),
        used_tokens: 12,
        usage_complete: true,
        elapsed_ms: 50,
        interval_seconds: None,
        next_due_ms: 1000,
        expires_at_ms: None,
        no_progress: 0,
        blocker_count: 0,
        blocker: None,
        reason: Some("Waiting for your review".into()),
        updated_at_ms: 1000,
    }
}

#[gpui_kit::test]
async fn automation_menu_preserves_draft_keyboard_selection_and_escape(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "automation-ui");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("existing request", cx))
    });
    click_composer_add(window, cx);
    assert!(visible(window, "composer-action-goal", cx));
    assert!(visible(window, "composer-action-loop", cx));
    click(window, "composer-action-goal", cx);
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "/goal existing request"
    );
    assert!(!visible(window, "composer-action-goal", cx));
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("check", cx))
    });
    click_composer_add(window, cx);
    assert!(visible(window, "composer-action-loop", cx));
    let selectors = [
        "composer-action-file",
        "composer-action-mode-ask",
        "composer-action-mode-plan",
        "composer-action-mode-execute",
        "composer-action-permission-readonly",
        "composer-action-permission-confirm",
        "composer-action-permission-auto",
        "composer-action-permission-full_access",
        "composer-action-images",
        "composer-action-credentials",
        "composer-action-goal",
        "composer-action-loop",
    ];
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = selectors.map(|selector| visual.debug_bounds(selector).unwrap());
    assert!(bounds.windows(2).all(|pair| pair[0].top() < pair[1].top()));
    let loop_index = selectors
        .iter()
        .position(|selector| *selector == "composer-action-loop")
        .unwrap();
    for _ in 0..loop_index {
        cx.simulate_keystrokes(window.into(), "down");
    }
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "/loop check"
    );
    assert!(!visible(window, "composer-action-loop", cx));
    click_composer_add(window, cx);
    cx.simulate_keystrokes(window.into(), "escape");
    assert!(!visible(window, "composer-action-loop", cx));
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "/loop check"
    );
}

#[gpui_kit::test]
async fn automation_status_actions_details_and_budget_focus_are_accessible(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "automation-ui");
    let events = Arc::new(Mutex::new(Vec::new()));
    let capture = events.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &AutomationRequested, _| {
            capture.lock().unwrap().push(event.command.clone())
        })
        .detach()
    });
    stream.update(cx, |stream, cx| {
        stream.apply_automation(
            AutomationProjection {
                tasks: vec![goal(AutomationStatus::Paused)],
                ..Default::default()
            },
            cx,
        )
    });
    assert!(visible(window, "automation-goal-control", cx));
    assert!(!visible(window, "automation-goal-instruction", cx));
    click(window, "automation-goal-details", cx);
    assert!(visible(window, "automation-goal-instruction", cx));
    window
        .update(cx, |_, window, cx| {
            let focus = stream.read(cx).automation.focus["goal"][1].clone();
            window.focus(&focus, cx);
        })
        .unwrap();
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        &[AutomationCommand::Control(
            AutomationKind::Goal,
            AutomationControl::Resume
        )]
    );
    stream.update(cx, |stream, cx| {
        let mut task = goal(AutomationStatus::Paused);
        task.usage_complete = false;
        stream.apply_automation(
            AutomationProjection {
                tasks: vec![task],
                error: Some("无法核实完整用量，请清除后重建".into()),
                ..Default::default()
            },
            cx,
        );
    });
    assert!(!visible(window, "automation-goal-control", cx));
    assert!(visible(window, "automation-goal-clear", cx));
    assert!(visible(window, "automation-error", cx));
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.automation_focuses().len()),
        2
    );
    stream.update(cx, |stream, cx| {
        stream.apply_automation(
            AutomationProjection {
                tasks: vec![goal(AutomationStatus::Complete)],
                ..Default::default()
            },
            cx,
        )
    });
    assert!(!visible(window, "automation-goal-control", cx));
    assert!(!visible(window, "automation-error", cx));
    click(window, "automation-goal-clear", cx);
    assert_eq!(
        events.lock().unwrap().last(),
        Some(&AutomationCommand::Control(
            AutomationKind::Goal,
            AutomationControl::Clear
        ))
    );
}

#[gpui_kit::test]
async fn unchanged_loop_poll_does_not_notify_history_and_running_hides_waiting(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "automation-ui");
    let mut task = goal(AutomationStatus::Active);
    task.kind = AutomationKind::Loop;
    task.identity = "loop".into();
    task.token_budget = None;
    task.next_due_ms = vega_conversation::automation::now_ms() + 330_000;
    let projection = AutomationProjection {
        tasks: vec![task],
        ..Default::default()
    };
    stream.update(cx, |stream, cx| {
        stream.apply_automation(projection.clone(), cx)
    });
    assert!(visible(window, "automation-loop-wakeup", cx));
    let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = notifications.clone();
    let _observation =
        cx.update(|cx| cx.observe(&stream, move |_, _| observed.set(observed.get() + 1)));
    for _ in 0..3 {
        stream.update(cx, |stream, cx| {
            stream.apply_automation(projection.clone(), cx)
        });
        cx.run_until_parked();
    }
    assert_eq!(notifications.get(), 0);
    stream.update(cx, |stream, cx| {
        stream.actions.running = true;
        cx.notify();
    });
    assert!(!visible(window, "automation-loop-wakeup", cx));
    stream.update(cx, |stream, cx| {
        stream.actions.running = false;
        cx.notify();
    });
    assert!(visible(window, "automation-loop-wakeup", cx));
}

#[gpui_kit::test]
async fn codex_has_no_automation_menu_but_explicit_command_error_is_visible(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "automation-ui");
    stream.update(cx, |stream, cx| {
        stream.thread.backend = TaskBackend::Codex;
        stream.input.update(cx, |input, cx| {
            input.set_text("/goal preserve this draft", cx)
        });
        stream.apply_automation(
            AutomationProjection {
                error: Some("持续目标与循环任务仅适用于 Vega 原生任务".into()),
                ..Default::default()
            },
            cx,
        );
    });
    assert!(visible(window, "automation-error", cx));
    assert!(!visible(window, "automation-goal-details", cx));
    click_composer_add(window, cx);
    assert!(!visible(window, "composer-action-goal", cx));
    assert!(!visible(window, "composer-action-loop", cx));
    cx.simulate_keystrokes(window.into(), "escape");
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "/goal preserve this draft"
    );
}

#[gpui_kit::test]
async fn automation_eligibility_observes_drafts_preflight_and_busy_state(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "automation-ui");
    assert!(stream.read_with(cx, |stream, cx| stream.automation_can_run(cx)));
    let question = RuntimeUserQuestionRequest {
        call_id: "automation-question".into(),
        input: UserQuestionInput::from_json(r#"{"questions":[{"id":"approach","header":"Approach","question":"Which approach?","options":[{"label":"A","description":"Keep it"},{"label":"B","description":"Change it"}]}]}"#).unwrap(),
    };
    let queue = stream.read_with(cx, |stream, _| stream.user_question_queue());
    let answer = queue.request(question.clone(), CancellationToken::new());
    assert!(!stream.read_with(cx, |stream, cx| stream.automation_can_run(cx)));
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: ToolCall {
                    id: question.call_id.clone(),
                    tool: vega_runtime::USER_QUESTION_TOOL.into(),
                    input_json: question.input.to_json().unwrap(),
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: question.call_id.clone(),
                approval: Approval::Once,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallRunning {
                call_id: question.call_id.clone(),
            },
            cx,
        );
    });
    click(window, "question-option-0", cx);
    cx.simulate_keystrokes(window.into(), "enter");
    let RuntimeUserQuestionResponse::Answered(answers) = answer.await else {
        panic!("selected question answer");
    };
    assert!(!queue.has_pending());
    assert!(!stream.read_with(cx, |stream, cx| stream.automation_can_run(cx)));
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: question.call_id.clone(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: answers.to_json(&question.input).unwrap(),
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        )
    });
    assert!(stream.read_with(cx, |stream, cx| stream.automation_can_run(cx)));
    let interrupted = queue.request(
        RuntimeUserQuestionRequest {
            call_id: "next-question".into(),
            ..question
        },
        CancellationToken::new(),
    );
    assert!(!stream.read_with(cx, |stream, cx| stream.automation_can_run(cx)));
    stream.update(cx, |stream, cx| stream.cancel_user_questions(cx));
    assert!(matches!(
        interrupted.await,
        RuntimeUserQuestionResponse::Cancelled
    ));
    assert!(stream.read_with(cx, |stream, cx| stream.automation_can_run(cx)));
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("draft", cx))
    });
    assert!(!stream.read_with(cx, |stream, cx| stream.automation_can_run(cx)));
    stream.update(cx, |stream, cx| {
        stream.input.update(cx, TextInput::clear);
        stream.set_trusted_action_busy(true, cx);
    });
    assert!(!stream.read_with(cx, |stream, cx| stream.automation_can_run(cx)));
    stream.update(cx, |stream, cx| {
        stream.set_trusted_action_busy(false, cx);
        stream.apply_automation(
            AutomationProjection {
                tasks: vec![goal(AutomationStatus::Active)],
                busy: true,
                error: None,
            },
            cx,
        );
    });
    assert!(!stream.read_with(cx, |stream, cx| stream.automation_can_run(cx)));
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.automation_focuses().len()),
        1
    );
    click(window, "automation-goal-control", cx);
    assert!(stream.read_with(cx, |stream, _| stream.automation_projection().busy));
}
