use super::*;
use gpui_kit::{Modifiers, VisualTestContext};
use vega_runtime::{
    RuntimeUserQuestionHook, RuntimeUserQuestionRequest, RuntimeUserQuestionResponse,
    UserQuestionInput,
};

#[gpui_kit::test]
async fn question_navigation_retains_production_stream_and_stop_is_thread_scoped(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let repo = tempfile::tempdir().unwrap();
    let _git = vega_conversation::register_git_test_executor(
        repo.path(),
        Arc::new(|command, _, _| {
            let args = command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(args.ends_with(&["rev-parse".into(), "--show-toplevel".into()]));
            Err(vega_conversation::types::GitWorkspaceError::for_test(
                vega_conversation::types::GitWorkspaceErrorCode::NotRepository,
            ))
        }),
    )
    .unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = Store::open(data.path().join("vega.db")).unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().unwrap(),
        "question-navigation",
        None,
    )
    .unwrap();
    let origin =
        vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm").unwrap();
    let other =
        vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm").unwrap();
    cx.update(|cx| install_diff_window_globals(store, origin.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(origin.clone(), cx));
    let root = cx.new(VegaWindow::new);
    let (generation, cancel) = root.update(cx, |root, _| {
        root.stream_view = Some((origin.id.clone(), stream.clone()));
        root.agent_controller
            .begin(origin.id.clone(), stream.clone(), None, None)
            .unwrap()
    });
    let window_root = root.clone();
    let window = cx
        .update(|cx| cx.open_window(Default::default(), move |_, _| window_root))
        .unwrap();
    cx.run_until_parked();
    let input = UserQuestionInput::from_json(&serde_json::json!({"questions": (0..2).map(|index| serde_json::json!({
        "id": format!("choice_{index}"), "header":"Choice", "question":format!("Which approach for {index}?"),
        "options":[{"label":"A","description":"First"},{"label":"B","description":"Second"}]
    })).collect::<Vec<_>>()}).to_string()).unwrap();
    let queue = stream.read_with(cx, |stream, _| stream.user_question_queue());
    let future = queue.request(
        RuntimeUserQuestionRequest {
            call_id: "navigation-question".into(),
            input: input.clone(),
        },
        cancel.clone(),
    );
    root.update(cx, |root, cx| {
        assert!(matches!(
            root.apply_agent_batch_ingress(
                generation,
                &origin.id,
                &stream,
                AgentBatch {
                    events: vec![
                        ConversationEvent::MessageStarted {
                            message_id: "question-message".into(),
                            seq: 1
                        },
                        ConversationEvent::ToolCallProposed {
                            call: ToolCall {
                                id: "navigation-question".into(),
                                tool: "ask_user_question".into(),
                                input_json: input.to_json().unwrap()
                            }
                        },
                        ConversationEvent::ToolCallApproved {
                            call_id: "navigation-question".into(),
                            approval: Approval::Once
                        },
                        ConversationEvent::ToolCallRunning {
                            call_id: "navigation-question".into()
                        },
                    ],
                    codex_mode_confirmed: false,
                    computer_use_ready: None,
                    mcp_unavailable: Vec::new(),
                    reference_failure: None,
                    credential_failure: false,
                    finished: None,
                },
                cx
            ),
            AgentBatchIngress::Running
        ));
    });
    cx.run_until_parked();
    let composer = stream.read_with(cx, |stream, _| stream.composer_input());
    composer.update(cx, |input, cx| {
        input.set_text("preserved composer draft", cx)
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let option = visual
        .debug_bounds("question-option-1")
        .expect("first page option");
    visual.simulate_click(option.center(), Modifiers::default());
    visual.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(other.clone())));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, view)| id == &other.id && view != &stream)
        })
    });
    let other_stream = root.read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone());
    assert!(!other_stream.read_with(cx, |stream, _| stream.has_pending_user_question()));
    root.update(cx, |root, cx| root.cancel_active_agent(cx));
    assert!(!cancel.is_cancelled());
    for open in [true, false] {
        cx.update(|cx| {
            cx.set_global(SettingsOpen(open));
            cx.refresh_windows();
        });
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| root.settings_view.is_some() == open)
        });
        assert!(stream.read_with(cx, |stream, _| stream.has_pending_user_question()));
        assert!(!cancel.is_cancelled());
    }
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(origin.clone())));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, view)| id == &origin.id && view == &stream)
        })
    });
    assert!(root.read_with(cx, |root, _| {
        root.agent_controller
            .matches(generation, &origin.id, &stream)
    }));
    assert_eq!(
        composer.read_with(cx, |input, _| input.text().to_owned()),
        "preserved composer draft"
    );
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let option = visual
        .debug_bounds("question-option-0")
        .expect("retained second page option");
    visual.simulate_click(option.center(), Modifiers::default());
    visual.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    assert!(
        !queue.has_pending(),
        "retained second page must submit the original request"
    );
    let RuntimeUserQuestionResponse::Answered(answers) = future.await else {
        panic!("retained request must answer");
    };
    assert_eq!(answers.answers["choice_0"].answers, ["B"]);
    assert_eq!(answers.answers["choice_1"].answers, ["A"]);
    assert!(stream.read_with(cx, |stream, _| stream.has_pending_user_question()));
    root.update(cx, |root, cx| root.cancel_active_agent(cx));
    assert!(cancel.is_cancelled());
    assert!(!stream.read_with(cx, |stream, _| stream.has_pending_user_question()));
}
