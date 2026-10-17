use super::*;
use gpui_kit::{Modifiers, VisualTestContext};
use vega_runtime::{
    RuntimeUserQuestionHook, RuntimeUserQuestionRequest, RuntimeUserQuestionResponse,
    UserQuestionInput,
};

fn question_request() -> RuntimeUserQuestionRequest {
    RuntimeUserQuestionRequest {
        call_id: "question-call".into(),
        input: UserQuestionInput::from_json(r#"{"questions":[{"id":"approach","header":"Approach","question":"Which approach?","options":[{"label":"A (Recommended)","description":"Keep it"},{"label":"B","description":"Change it"}]}]}"#).unwrap(),
    }
}

#[gpui_kit::test]
async fn question_panel_waits_for_proposal_and_durable_matching_terminal(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, _) = open_permission_stream(cx);
    let queue = window
        .update(cx, |stream, window, cx| {
            stream.focus_composer(window, cx);
            stream.user_question_queue()
        })
        .unwrap();
    let request = question_request();
    let future = queue.request(request.clone(), CancellationToken::new());
    cx.run_until_parked();
    window
        .update(cx, |stream, _, _| {
            assert!(stream.has_pending_user_question());
            assert!(stream.active_user_question.is_none());
            assert!(stream.deferred_user_question.is_some());
        })
        .unwrap();
    propose(
        window,
        cx,
        ToolCall {
            id: request.call_id.clone(),
            tool: "ask_user_question".into(),
            input_json: request.input.to_json().unwrap(),
        },
    );
    cx.run_until_parked();
    let active = window
        .update(cx, |stream, window, cx| {
            assert!(stream.input.read(cx).focus_handle(cx).is_focused(window));
            stream.active_user_question.clone().unwrap()
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let option = visual
        .debug_bounds("question-option-0")
        .expect("question option");
    visual.simulate_click(option.center(), Modifiers::default());
    visual.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    assert!(!queue.has_pending());
    let RuntimeUserQuestionResponse::Answered(answers) = future.await else {
        panic!("selected answer must be delivered");
    };
    let result = ToolResult {
        status: ToolCallStatus::Success,
        output: answers.to_json(&request.input).unwrap(),
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: Some(false),
        invalid: None,
    };
    window
        .update(cx, |stream, _, cx| {
            assert_eq!(stream.active_user_question.as_ref(), Some(&active));
            assert!(stream.has_pending_user_question());
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: "unrelated".into(),
                    result: result.clone(),
                },
                cx,
            );
            assert_eq!(stream.active_user_question.as_ref(), Some(&active));
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: request.call_id.clone(),
                    result,
                },
                cx,
            );
            assert!(!stream.has_pending_user_question());
            assert!(stream.active_user_question.is_none());
            stream.tool_cards[&request.call_id].update(cx, |card, cx| card.set_expanded(true, cx));
            let text = stream.tool_cards[&request.call_id].read(cx).visible_text();
            assert!(text.contains("Questions completed"));
            assert!(text.contains("Which approach?"));
            assert!(text.contains("A (Recommended)"));
        })
        .unwrap();
}

#[gpui_kit::test]
async fn question_stop_releases_wait_and_stale_response_cannot_reopen_panel(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    let (window, _) = open_permission_stream(cx);
    let queue = window
        .update(cx, |stream, _, _| stream.user_question_queue())
        .unwrap();
    let request = question_request();
    propose(
        window,
        cx,
        ToolCall {
            id: request.call_id.clone(),
            tool: "ask_user_question".into(),
            input_json: request.input.to_json().unwrap(),
        },
    );
    let future = queue.request(request, CancellationToken::new());
    cx.run_until_parked();
    window
        .update(cx, |stream, _, cx| {
            assert!(stream.active_user_question.is_some());
            stream.cancel_user_questions(cx);
            stream.install_pending_user_question(cx);
            assert!(!stream.has_pending_user_question());
            assert!(stream.active_user_question.is_none());
        })
        .unwrap();
    assert!(matches!(
        future.await,
        RuntimeUserQuestionResponse::Cancelled
    ));
}
