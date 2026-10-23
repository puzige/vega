use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll, Waker};
use std::time::{Duration, Instant};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Entity, Global, SystemNotificationResponse, TestAppContext, VisualTestContext,
    WindowHandle,
};
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{
    PermissionHook, PermissionQueue, PersistenceActorConfig, RejectPermissionHook,
    UserQuestionQueue, run_thread_task_with_images_reasoning_and_mcp,
};
use vega_conversation::history::{AssistantStatus, HistoryEntry, HistoryPage};
use vega_conversation::types::{
    Approval, AssistantResponseKind, AssistantResponseMetadata, ConversationEvent,
    ConversationStopReason, NavigationRoute, PermissionDecision, PermissionRequest,
    SubagentAdmission, SubagentSnapshot, SubagentState, Thread, ToolCall, ToolCallStatus,
    ToolResult, UserQuestionAnswers, UserQuestionInput,
};
use vega_runtime::{
    ChatRequest, EventStream, ProviderEvent, RuntimeUserQuestionHook, RuntimeUserQuestionRequest,
    RuntimeUserQuestionResponse, StopReason, USER_QUESTION_TOOL, VegaError,
};
use vega_store::Store;
use vega_ui::conversation_stream::ConversationStream;
use vega_ui::settings::SettingsOpen;
use vega_ui::sidebar::{
    NewTaskDefaults, OpenedThread, PendingDeleteConfirm, ProjectsCollapsed, SelectedProject,
    SessionsCollapsed, SidebarCollapsed, SidebarWidth, VegaStore,
};

use super::VegaWindow;

pub(super) struct QuestionTestShell(pub std::path::PathBuf);

impl Global for QuestionTestShell {}

fn poll_once<T>(future: Pin<&mut (dyn Future<Output = T> + Send)>) -> Poll<T> {
    future.poll(&mut TaskContext::from_waker(Waker::noop()))
}

fn question_input() -> UserQuestionInput {
    UserQuestionInput::from_json(
        r#"{"questions":[{"id":"color","header":"Color","question":"Which color?","options":[{"label":"Blue","description":"Cool"},{"label":"Red","description":"Warm"}]}]}"#,
    )
    .unwrap()
}

fn expected_answer(skip: bool) -> UserQuestionAnswers {
    UserQuestionAnswers::from_json(
        if skip {
            r#"{"answers":{"color":{"answers":[]}}}"#
        } else {
            r#"{"answers":{"color":{"answers":["Blue"]}}}"#
        },
        &question_input(),
    )
    .unwrap()
}

fn question_result(skip: bool) -> ToolResult {
    ToolResult {
        status: ToolCallStatus::Success,
        output: expected_answer(skip).to_json(&question_input()).unwrap(),
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: Some(false),
        invalid: None,
    }
}

struct Fixture {
    directory: Arc<tempfile::TempDir>,
    window: WindowHandle<VegaWindow>,
    thread: Thread,
    other: Thread,
    stream: Entity<ConversationStream>,
    queue: UserQuestionQueue,
    cancel: CancellationToken,
}

impl Fixture {
    fn new(cx: &mut TestAppContext, notifications_available: bool) -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let store = Store::open(directory.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        let mut thread =
            vega_conversation::threads::create_standalone_thread(&store, "test-model", "confirm")
                .unwrap();
        vega_conversation::threads::rename_thread(&store, &thread.id, "Choose release color")
            .unwrap();
        thread.title = "Choose release color".into();
        let other =
            vega_conversation::threads::create_standalone_thread(&store, "test-model", "confirm")
                .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
            vega_ui::init(cx);
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(QuestionTestShell(directory.path().join("config.toml")));
            cx.set_global(VegaStore(Ok(store)));
            cx.set_global(SelectedProject(None));
            cx.set_global(OpenedThread(Some(thread.clone())));
            cx.set_global(SettingsOpen(false));
            cx.set_global(PendingDeleteConfirm(None));
            cx.set_global(SidebarCollapsed(true));
            cx.set_global(SidebarWidth(240.));
            cx.set_global(ProjectsCollapsed(false));
            cx.set_global(SessionsCollapsed(false));
            cx.set_global(NewTaskDefaults::default());
            if !notifications_available {
                cx.set_global(crate::app_notifications::QuestionNotificationUnavailable);
            }
        });
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let other_stream = cx.new(|cx| ConversationStream::new(other.clone(), cx));
        let queue = cx.update(|cx| stream.read(cx).user_question_queue());
        let window = cx.add_window(|_, cx| {
            let mut owner = VegaWindow::new(cx);
            owner.stream_view = Some((thread.id.clone(), stream.clone()));
            owner.configured_models = Some(vec!["test-model".into()]);
            owner
        });
        let cancel = window
            .update(cx, |owner, window, cx| {
                owner.bind_conversation_stream(&stream, window, cx);
                owner.bind_conversation_stream(&other_stream, window, cx);
                let (_, cancel) = owner
                    .agent_controller
                    .begin(thread.id.clone(), stream.clone(), None, None)
                    .unwrap();
                owner
                    .agent_controller
                    .begin(other.id.clone(), other_stream.clone(), None, None)
                    .unwrap();
                owner.stream_view = Some((thread.id.clone(), stream.clone()));
                owner.sync_completion_attention(window, cx);
                crate::app_notifications::bind(
                    window.window_handle().downcast().unwrap(),
                    cx.entity().downgrade(),
                    cx,
                );
                window.activate_window();
                stream.update(cx, |stream, cx| stream.begin_composer_run(cx));
                cancel
            })
            .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
                .unwrap();
        });
        Self {
            directory,
            window,
            thread,
            other,
            stream,
            queue,
            cancel,
        }
    }

    fn admit_child(&self, cx: &mut TestAppContext) -> Self {
        let thread = cx.update(|cx| {
            let store = cx.global::<VegaStore>().0.as_ref().unwrap();
            let child = vega_conversation::threads::create_standalone_thread(
                store, "test-model", "confirm",
            ).unwrap();
            store.conn().execute(
                "UPDATE threads SET parent_thread_id = ?1, title = 'Delegated color choice' WHERE id = ?2",
                [&self.thread.id, &child.id],
            ).unwrap();
            vega_conversation::threads::get_thread(store, &child.id).unwrap()
        });
        let queue = UserQuestionQueue::new();
        let cancel = CancellationToken::new();
        let (reply, mut admitted) = tokio::sync::oneshot::channel();
        let stream = self
            .window
            .update(cx, |owner, _, cx| {
                owner.apply_subagent_update(
                    &self.thread.id,
                    &self.stream,
                    crate::native_subagents::NativeSubagentUpdate::Admit {
                        admission: Box::new(SubagentAdmission {
                            thread: thread.clone(),
                            snapshot: SubagentSnapshot {
                                agent_id: thread.id.clone(),
                                parent_thread_id: self.thread.id.clone(),
                                parent_message_id: "run".into(),
                                message_id: "child-run".into(),
                                name: thread.title.clone(),
                                state: SubagentState::Running,
                                result: String::new(),
                                truncated: false,
                                checkpoint: None,
                                release_state: String::new(),
                                ready_for_review: false,
                            },
                            cancel: cancel.clone(),
                            permission_queue: PermissionQueue::new(),
                            user_questions: queue.clone(),
                            task: "Ask the user for a color".into(),
                            queue_generation: "child-generation".into(),
                        }),
                        reply,
                    },
                    cx,
                );
                assert!(admitted.try_recv().unwrap().is_ok());
                owner.apply_subagent_update(
                    &self.thread.id,
                    &self.stream,
                    crate::native_subagents::NativeSubagentUpdate::Event {
                        thread_id: thread.id.clone(),
                        message_id: "child-run".into(),
                        event: ConversationEvent::MessageStarted {
                            message_id: "child-run".into(),
                            seq: 1,
                        },
                    },
                    cx,
                );
                assert!(
                    owner
                        .agent_controller
                        .delegated_unbound
                        .contains(&thread.id)
                );
                owner
                    .agent_controller
                    .active_stream_for_thread(&thread.id)
                    .unwrap()
            })
            .unwrap();
        Self {
            directory: self.directory.clone(),
            window: self.window,
            thread,
            other: self.thread.clone(),
            stream,
            queue,
            cancel,
        }
    }

    fn frame(&self, cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.update_window(self.window.into(), |_, window, cx| window.render_frame(cx))
                .unwrap();
        });
        cx.run_until_parked();
    }

    fn start_message(&self, cx: &mut TestAppContext) {
        self.stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "run".into(),
                    seq: 1,
                },
                cx,
            );
        });
        self.window
            .update(cx, |owner, _, _| {
                let run = owner
                    .agent_controller
                    .active
                    .get_mut(&self.thread.id)
                    .unwrap();
                run.root_message_id = Some("run".into());
                run.message_ids.push("run".into());
            })
            .unwrap();
    }

    fn navigate(&self, other: bool, cx: &mut TestAppContext) {
        let thread = if other { &self.other } else { &self.thread };
        self.window
            .update(cx, |owner, window, cx| {
                assert!(owner.accept_palette_thread(thread.clone(), cx));
                owner.sync_completion_attention(window, cx);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        self.frame(cx);
        self.window
            .read_with(cx, |owner, _| {
                assert_eq!(
                    owner.stream_view.as_ref().map(|(id, _)| id),
                    Some(&thread.id)
                )
            })
            .unwrap();
    }

    fn propose(&self, id: &str, cx: &mut TestAppContext) {
        self.stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: ToolCall {
                        id: id.into(),
                        tool: USER_QUESTION_TOOL.into(),
                        input_json: question_input().to_json().unwrap(),
                    },
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallApproved {
                    call_id: id.into(),
                    approval: Approval::Once,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallRunning { call_id: id.into() },
                cx,
            );
        });
    }

    fn ask(
        &self,
        id: &str,
        cx: &mut TestAppContext,
    ) -> Pin<Box<dyn Future<Output = RuntimeUserQuestionResponse> + Send>> {
        self.propose(id, cx);
        let future = self.queue.request(
            RuntimeUserQuestionRequest {
                call_id: id.into(),
                input: question_input(),
            },
            self.cancel.clone(),
        );
        cx.run_until_parked();
        self.frame(cx);
        future
    }

    fn answer(&self, skip: bool, cx: &mut TestAppContext) {
        self.window
            .update(cx, |_, window, cx| {
                self.stream.update(cx, |stream, cx| {
                    stream.question_test_answer(skip, window, cx)
                });
            })
            .unwrap();
        cx.run_until_parked();
    }

    fn label(&self, cx: &TestAppContext) -> String {
        cx.update(|cx| self.stream.read(cx).question_test_label(cx).unwrap())
    }

    fn panel_present(&self, cx: &TestAppContext) -> bool {
        cx.update(|cx| self.stream.read(cx).question_test_panel_present())
    }

    fn assert_submitted(&self, skip: bool, cx: &mut TestAppContext) {
        assert!(self.panel_present(cx));
        assert!(!self.queue.has_pending());
        assert_ne!(self.label(cx), "Waiting for your answer");
        let submission = cx.update(|cx| self.stream.read(cx).question_test_submission(cx).unwrap());
        assert!(submission.0);
        assert!(
            submission.1 == vec![Some(expected_answer(skip).answers["color"].clone())],
            "submitted answer did not match"
        );
        let shown = cx.shown_system_notifications().len();
        for repeated_skip in [!skip, skip] {
            self.answer(repeated_skip, cx);
            assert!(
                cx.update(|cx| self.stream.read(cx).question_test_submission(cx))
                    == Some(submission.clone()),
                "submitted state changed after repeated action"
            );
            assert!(!self.queue.has_pending());
            assert_ne!(self.label(cx), "Waiting for your answer");
            assert_eq!(cx.shown_system_notifications().len(), shown);
        }
    }

    fn finish_tool(&self, id: &str, skip: bool, cx: &mut TestAppContext) {
        self.stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: id.into(),
                    result: question_result(skip),
                },
                cx,
            )
        });
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn foreground_question_keeps_focus_stop_and_navigation_without_notifications(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(cx, true);
    fixture.start_message(cx);
    let focus = fixture
        .window
        .update(cx, |_, window, cx| {
            fixture.stream.update(cx, |stream, cx| {
                stream.question_test_focus_composer(window, cx)
            })
        })
        .unwrap();
    let mut answer = fixture.ask("foreground", cx);
    assert!(fixture.panel_present(cx));
    assert_eq!(fixture.label(cx), "Waiting for your answer");
    assert!(cx.shown_system_notifications().is_empty());
    fixture
        .window
        .update(cx, |_, window, _| assert!(focus.is_focused(window)))
        .unwrap();
    fixture.navigate(true, cx);
    fixture.navigate(false, cx);
    fixture.stream.update(cx, |stream, cx| {
        stream.question_test_reconcile(cx);
        stream.question_test_reconcile(cx);
    });
    cx.run_until_parked();
    assert!(fixture.panel_present(cx));
    assert!(fixture.queue.has_pending());
    assert!(matches!(poll_once(answer.as_mut()), Poll::Pending));
    assert_eq!(fixture.label(cx), "Waiting for your answer");
    assert!(cx.shown_system_notifications().is_empty());
    fixture
        .stream
        .update(cx, |stream, cx| stream.request_composer_stop(cx));
    cx.run_until_parked();
    assert!(fixture.cancel.is_cancelled());
    assert_eq!(
        poll_once(answer.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Cancelled)
    );
    cx.run_until_parked();
    assert!(!fixture.panel_present(cx));
    assert!(!fixture.queue.has_pending());
}

#[gpui_kit::test]
fn background_question_notifies_once_and_click_routes_to_its_live_task(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx, true);
    fixture.start_message(cx);
    fixture.navigate(true, cx);
    let mut answer = fixture.ask("background", cx);
    let notifications = cx.shown_system_notifications();
    assert_eq!(notifications.len(), 1);
    let notification = notifications[0].clone();
    assert_eq!(notification.title.as_ref(), "Choose release color");
    assert_eq!(notification.body.as_ref(), "Waiting for your answer");
    assert_eq!(cx.delivered_system_notifications().len(), 1);
    assert_eq!(
        cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        fixture.other.id
    );
    fixture.stream.update(cx, |stream, cx| {
        stream.question_test_reconcile(cx);
        stream.question_test_reconcile(cx);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(cx.shown_system_notifications().len(), 1);
    assert!(matches!(poll_once(answer.as_mut()), Poll::Pending));
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: notification.tag.clone(),
        action_id: None,
    });
    cx.update(|_| {});
    cx.run_until_parked();
    fixture.frame(cx);
    assert_eq!(
        cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        fixture.thread.id
    );
    fixture
        .window
        .update(cx, |owner, _, cx| {
            assert_eq!(
                owner
                    .agent_controller
                    .active_stream_for_thread(&fixture.thread.id),
                Some(fixture.stream.clone())
            );
            assert_eq!(
                owner
                    .stream_view
                    .as_ref()
                    .map(|(id, stream)| (id.as_str(), stream)),
                Some((fixture.thread.id.as_str(), &fixture.stream))
            );
            assert!(fixture.stream.read(cx).question_test_panel_present());
        })
        .unwrap();
    assert!(fixture.queue.has_pending());
    fixture.answer(false, cx);
    assert_eq!(
        poll_once(answer.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(
            false
        )))
    );
    cx.run_until_parked();
    fixture.assert_submitted(false, cx);
    assert!(cx.delivered_system_notifications().is_empty());
    assert!(
        cx.dismissed_system_notifications()
            .contains(&notification.tag)
    );
    fixture.finish_tool("background", false, cx);
    assert!(!fixture.panel_present(cx));
    assert_eq!(cx.shown_system_notifications().len(), 1);
}

#[gpui_kit::test]
fn continued_run_keeps_successor_question_and_attention_after_stale_predecessor_cleanup(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(cx, true);
    fixture.start_message(cx);
    fixture.navigate(true, cx);
    let mut first = fixture.ask("before-steering", cx);
    let notices = cx.shown_system_notifications();
    assert_eq!(notices.len(), 1);
    let first_notice = notices[0].clone();
    assert_eq!(first_notice.title.as_ref(), "Choose release color");
    assert_eq!(first_notice.body.as_ref(), "Waiting for your answer");
    assert_eq!(fixture.label(cx), "Waiting for your answer");
    assert!(matches!(poll_once(first.as_mut()), Poll::Pending));
    fixture.answer(false, cx);
    assert_eq!(
        poll_once(first.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(
            false
        )))
    );
    fixture.assert_submitted(false, cx);
    assert!(cx.delivered_system_notifications().is_empty());
    assert!(
        cx.dismissed_system_notifications()
            .contains(&first_notice.tag)
    );
    fixture.finish_tool("before-steering", false, cx);
    assert!(!fixture.panel_present(cx));

    let generation = fixture
        .window
        .update(cx, |owner, _, _| {
            owner.agent_controller.active[&fixture.thread.id].generation
        })
        .unwrap();
    let apply_events = |events, cx: &mut TestAppContext| {
        fixture
            .window
            .update(cx, |owner, _, cx| {
                assert!(matches!(
                    owner.apply_agent_batch_ingress(
                        generation,
                        &fixture.thread.id,
                        &fixture.stream,
                        crate::app_agent::AgentBatch {
                            composer_prediction: None,
                            subagents: Vec::new(),
                            events,
                            codex_mode_confirmed: false,
                            computer_use_ready: None,
                            mcp_unavailable: Vec::new(),
                            reference_failure: None,
                            credential_failure: false,
                            finished: None,
                        },
                        cx,
                    ),
                    crate::app_agent::AgentBatchIngress::Running
                ));
                let run = &owner.agent_controller.active[&fixture.thread.id];
                assert_eq!(run.generation, generation);
                assert_eq!(run.root_message_id.as_deref(), Some("run"));
                assert_eq!(run.message_ids, ["run", "next-run"]);
                assert!(run.terminal_message_id.is_none());
                assert!(run.completion_outcome.is_none());
                assert!(!run.queue_ready);
            })
            .unwrap();
        cx.run_until_parked();
    };
    apply_events(
        vec![
            ConversationEvent::RunContinued {
                previous_assistant_id: "run".into(),
                user_message_id: "steering-user".into(),
                next_assistant_id: "next-run".into(),
            },
            ConversationEvent::PendingMessageDelivered {
                pending_id: "steering-user".into(),
                message_id: "steering-user".into(),
                seq: 2,
                content: "Ask again".into(),
                images: Vec::new(),
            },
            ConversationEvent::MessageStarted {
                message_id: "next-run".into(),
                seq: 3,
            },
        ],
        cx,
    );
    assert_eq!(
        cx.update(|cx| fixture.stream.read(cx).question_test_run_label("run", cx)),
        Some("Activity".into())
    );
    let mut next = fixture.ask("after-steering", cx);
    let notices = cx.shown_system_notifications();
    assert_eq!(notices.len(), 2);
    let next_notice = notices[1].clone();
    assert_ne!(first_notice.tag, next_notice.tag);
    assert_eq!(next_notice.title.as_ref(), "Choose release color");
    assert_eq!(next_notice.body.as_ref(), "Waiting for your answer");
    assert_eq!(fixture.label(cx), "Waiting for your answer");
    assert_eq!(
        cx.update(|cx| fixture
            .stream
            .read(cx)
            .question_test_run_label("next-run", cx)),
        Some("Waiting for your answer".into())
    );
    apply_events(
        vec![ConversationEvent::MessageFinished {
            message_id: "run".into(),
            stop_reason: ConversationStopReason::End,
            execution_duration_ms: Some(1_000),
        }],
        cx,
    );
    fixture.finish_tool("before-steering", false, cx);
    assert!(!fixture.cancel.is_cancelled());
    assert!(fixture.queue.has_pending());
    assert!(matches!(poll_once(next.as_mut()), Poll::Pending));
    assert!(fixture.panel_present(cx));
    assert_eq!(fixture.label(cx), "Waiting for your answer");
    assert_eq!(
        cx.update(|cx| fixture.stream.read(cx).question_test_run_label("run", cx)),
        Some("Activity".into())
    );
    assert_eq!(
        cx.update(|cx| fixture
            .stream
            .read(cx)
            .question_test_run_label("next-run", cx)),
        Some("Waiting for your answer".into())
    );
    let delivered = cx.delivered_system_notifications();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].tag, next_notice.tag);
    assert!(
        !cx.dismissed_system_notifications()
            .contains(&next_notice.tag)
    );
    assert_eq!(cx.shown_system_notifications().len(), 2);

    fixture.answer(false, cx);
    assert_eq!(
        poll_once(next.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(
            false
        )))
    );
    fixture.assert_submitted(false, cx);
    assert!(cx.delivered_system_notifications().is_empty());
    assert!(
        cx.dismissed_system_notifications()
            .contains(&next_notice.tag)
    );
    fixture.finish_tool("after-steering", false, cx);
    assert!(!fixture.panel_present(cx));
    assert_eq!(cx.shown_system_notifications().len(), 2);
}

#[gpui_kit::test]
fn inactive_application_and_settings_each_receive_one_needs_answer_notification(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(cx, true);
    fixture.start_message(cx);
    let mut visual = VisualTestContext::from_window(fixture.window.into(), cx);
    visual.deactivate_window();
    let mut first = fixture.ask("inactive", cx);
    assert_eq!(cx.shown_system_notifications().len(), 1);
    assert!(cx.update(|cx| cx.active_window()).is_none());
    fixture.answer(true, cx);
    assert_eq!(
        poll_once(first.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(true)))
    );
    cx.run_until_parked();
    fixture.finish_tool("inactive", true, cx);
    fixture
        .window
        .update(cx, |_, window, cx| {
            window.activate_window();
            cx.set_global(SettingsOpen(true));
        })
        .unwrap();
    let mut second = fixture.ask("settings", cx);
    assert_eq!(cx.shown_system_notifications().len(), 2);
    assert!(cx.update(|cx| cx.global::<SettingsOpen>().0));
    assert!(matches!(poll_once(second.as_mut()), Poll::Pending));
    fixture.queue.cancel_active();
    assert_eq!(
        poll_once(second.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Cancelled)
    );
    cx.run_until_parked();
    assert!(!fixture.panel_present(cx));
    assert!(cx.delivered_system_notifications().is_empty());
}

#[gpui_kit::test]
fn sequential_notifications_ignore_old_cleanup_and_preserve_completion(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx, true);
    fixture.start_message(cx);
    fixture.navigate(true, cx);
    let mut first = fixture.ask("first", cx);
    let old_panel = cx.update(|cx| fixture.stream.read(cx).question_test_panel().unwrap());
    assert_eq!(cx.shown_system_notifications().len(), 1);
    let first_tag = cx.shown_system_notifications()[0].tag.clone();
    fixture.answer(false, cx);
    assert_eq!(
        poll_once(first.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(
            false
        )))
    );
    cx.run_until_parked();
    fixture.finish_tool("first", false, cx);
    let mut second = fixture.ask("second", cx);
    assert_eq!(cx.shown_system_notifications().len(), 2);
    let second_tag = cx.shown_system_notifications()[1].tag.clone();
    assert_ne!(first_tag, second_tag);
    fixture.finish_tool("first", false, cx);
    drop(first);
    cx.update(|cx| crate::app_notifications::dismiss(&first_tag, cx));
    assert!(fixture.queue.has_pending());
    assert_eq!(fixture.label(cx), "Waiting for your answer");
    assert!(
        cx.delivered_system_notifications()
            .iter()
            .any(|notification| notification.tag == second_tag)
    );
    cx.update(|cx| {
        crate::app_notifications::show(
            NavigationRoute::Task {
                project: None,
                task: fixture.thread.id.clone(),
            },
            &fixture.thread.title,
            90,
            crate::app_agent::RunCompletionOutcome::Completed,
            "Finished normally",
            cx,
        )
    });
    let completion = cx.shown_system_notifications().last().unwrap().clone();
    assert_ne!(completion.tag, second_tag);
    assert_eq!(completion.body.as_ref(), "Finished normally");
    fixture.answer(true, cx);
    assert_eq!(
        poll_once(second.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(true)))
    );
    cx.run_until_parked();
    let delivered = cx.delivered_system_notifications();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].tag, completion.tag);
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: completion.tag.clone(),
        action_id: None,
    });
    cx.update(|_| {});
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        fixture.thread.id
    );
    assert!(
        cx.dismissed_system_notifications()
            .contains(&completion.tag)
    );
    fixture.finish_tool("second", true, cx);
    fixture.stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "run".into(),
                stop_reason: ConversationStopReason::End,
                execution_duration_ms: Some(6_000),
            },
            cx,
        );
        stream.finish_composer_run(false, cx);
        assert!(stream.replace_history_window(
            HistoryPage {
                subagents: Vec::new(),
                entries: vec![HistoryEntry::AssistantText {
                    seq: 1,
                    message_id: "run".into(),
                    content: "Finished normally".into(),
                    status: AssistantStatus::Done,
                    execution_duration_ms: Some(6_000),
                }],
                older_cursor: None,
                newer_cursor: None,
                newest_seq: Some(1),
            },
            "run",
            cx,
        ));
    });
    let (old_generation, old_run, new_generation, new_cancel) = fixture
        .window
        .update(cx, |owner, _, cx| {
            let old_generation = owner.agent_controller.active[&fixture.thread.id].generation;
            let old_run = owner
                .agent_controller
                .finish(old_generation, &fixture.thread.id, &fixture.stream)
                .unwrap();
            let (new_generation, new_cancel) = owner
                .agent_controller
                .begin(
                    fixture.thread.id.clone(),
                    fixture.stream.clone(),
                    None,
                    None,
                )
                .unwrap();
            assert_ne!(old_generation, new_generation);
            let event = ConversationEvent::MessageStarted {
                message_id: "later-run".into(),
                seq: 2,
            };
            owner.agent_controller.observe_terminal_message(
                new_generation,
                &fixture.thread.id,
                &fixture.stream,
                &event,
            );
            fixture.stream.update(cx, |stream, cx| {
                stream.begin_composer_run(cx);
                stream.apply_event(event, cx);
            });
            assert_eq!(
                owner
                    .agent_controller
                    .active_stream_for_thread(&fixture.thread.id),
                Some(fixture.stream.clone())
            );
            (old_generation, old_run, new_generation, new_cancel)
        })
        .unwrap();
    fixture.cancel = new_cancel;
    fixture.navigate(true, cx);
    let shown = cx.shown_system_notifications().len();
    let mut later = fixture.ask("first", cx);
    let later_panel = cx.update(|cx| fixture.stream.read(cx).question_test_panel().unwrap());
    assert_ne!(old_panel.entity_id(), later_panel.entity_id());
    assert_eq!(cx.shown_system_notifications().len(), shown + 1);
    let later_tag = cx.shown_system_notifications().last().unwrap().tag.clone();
    assert_ne!(later_tag, first_tag);
    assert_ne!(later_tag, second_tag);
    assert_ne!(later_tag, completion.tag);
    old_run.cancel.cancel();
    drop(old_run);
    drop(old_panel);
    drop(second);
    cx.update(|cx| {
        crate::app_notifications::dismiss(&first_tag, cx);
        crate::app_notifications::dismiss(&second_tag, cx);
    });
    fixture
        .window
        .update(cx, |owner, _, cx| {
            let ingress = owner.apply_agent_batch_ingress(
                old_generation,
                &fixture.thread.id,
                &fixture.stream,
                crate::app_agent::AgentBatch {
                    composer_prediction: None,
                    subagents: Vec::new(),
                    events: vec![ConversationEvent::ToolCallFinished {
                        call_id: "first".into(),
                        result: question_result(false),
                    }],
                    codex_mode_confirmed: false,
                    computer_use_ready: None,
                    mcp_unavailable: Vec::new(),
                    reference_failure: None,
                    credential_failure: false,
                    finished: Some(true),
                },
                cx,
            );
            assert!(matches!(
                ingress,
                crate::app_agent::AgentBatchIngress::Stale
            ));
            assert!(owner.agent_controller.matches(
                new_generation,
                &fixture.thread.id,
                &fixture.stream
            ));
            assert!(!owner.agent_controller.matches(
                old_generation,
                &fixture.thread.id,
                &fixture.stream
            ));
        })
        .unwrap();
    fixture.stream.update(cx, |stream, cx| {
        stream.question_test_reconcile(cx);
        stream.question_test_reconcile(cx);
    });
    cx.run_until_parked();
    assert!(!fixture.cancel.is_cancelled());
    assert!(fixture.queue.has_pending());
    assert!(fixture.panel_present(cx));
    assert_eq!(fixture.label(cx), "Waiting for your answer");
    assert!(matches!(poll_once(later.as_mut()), Poll::Pending));
    assert_eq!(cx.shown_system_notifications().len(), shown + 1);
    let delivered = cx.delivered_system_notifications();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].tag, later_tag);
    assert!(!cx.dismissed_system_notifications().contains(&later_tag));
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: first_tag,
        action_id: None,
    });
    cx.update(|_| {});
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        fixture.other.id
    );
    fixture.answer(true, cx);
    assert_eq!(
        poll_once(later.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(true)))
    );
    cx.run_until_parked();
    fixture.assert_submitted(true, cx);
    assert_eq!(cx.shown_system_notifications().len(), shown + 1);
    assert!(cx.delivered_system_notifications().is_empty());
    assert!(cx.dismissed_system_notifications().contains(&later_tag));
    fixture.finish_tool("first", true, cx);
    assert!(!fixture.panel_present(cx));
}

#[gpui_kit::test]
fn ordinary_completion_still_flows_through_window_policy_after_a_question(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx, true);
    fixture.start_message(cx);
    fixture.navigate(true, cx);
    let mut response = fixture.ask("answered", cx);
    assert_eq!(cx.shown_system_notifications().len(), 1);
    let question_tag = cx.shown_system_notifications()[0].tag.clone();
    fixture.answer(false, cx);
    assert_eq!(
        poll_once(response.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(
            false
        )))
    );
    cx.run_until_parked();
    fixture.finish_tool("answered", false, cx);
    fixture.stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: "run".into(),
                response_id: "final".into(),
                text_start_bytes: 0,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "run".into(),
                delta: "Finished normally".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: "run".into(),
                response: AssistantResponseMetadata {
                    response_id: "final".into(),
                    text_start_bytes: 0,
                    text_end_bytes: "Finished normally".len(),
                    kind: AssistantResponseKind::Final,
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "run".into(),
                stop_reason: ConversationStopReason::End,
                execution_duration_ms: Some(4_000),
            },
            cx,
        );
    });
    fixture
        .window
        .update(cx, |owner, _, cx| {
            let run = owner
                .agent_controller
                .active
                .remove(&fixture.thread.id)
                .unwrap();
            owner.finish_completion_attention(&run, true, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.shown_system_notifications().len(), 2);
    let delivered = cx.delivered_system_notifications();
    assert_eq!(delivered.len(), 1);
    assert_ne!(delivered[0].tag, question_tag);
    assert_eq!(delivered[0].body.as_ref(), "Finished normally");
    assert!(cx.dismissed_system_notifications().contains(&question_tag));
    assert!(cx.update(|cx| {
        vega_conversation::threads::get_thread(
            cx.global::<VegaStore>().0.as_ref().unwrap(),
            &fixture.thread.id,
        )
        .unwrap()
        .unread
    }));
}

#[gpui_kit::test]
fn cancellation_clears_question_and_stale_click_cannot_navigate(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx, true);
    fixture.start_message(cx);
    fixture.navigate(true, cx);
    let mut response = fixture.ask("cancelled", cx);
    assert_eq!(cx.shown_system_notifications().len(), 1);
    let tag = cx.shown_system_notifications()[0].tag.clone();
    fixture.cancel.cancel();
    assert_eq!(
        poll_once(response.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Cancelled)
    );
    cx.run_until_parked();
    assert!(!fixture.panel_present(cx));
    assert!(cx.delivered_system_notifications().is_empty());
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag,
        action_id: None,
    });
    cx.update(|_| {});
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        fixture.other.id
    );
    fixture.queue.cancel_active();
    fixture
        .stream
        .update(cx, |stream, cx| stream.cancel_user_questions(cx));
    assert!(!fixture.queue.has_pending());
}

#[gpui_kit::test]
fn unavailable_notification_platform_leaves_question_blocked_and_answerable(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(cx, false);
    fixture.start_message(cx);
    fixture.navigate(true, cx);
    let mut response = fixture.ask("no-delivery", cx);
    assert!(cx.app_identity().is_none());
    assert!(cx.shown_system_notifications().is_empty());
    assert!(cx.delivered_system_notifications().is_empty());
    assert!(matches!(poll_once(response.as_mut()), Poll::Pending));
    assert!(fixture.queue.has_pending());
    fixture.navigate(false, cx);
    assert!(fixture.panel_present(cx));
    assert_eq!(fixture.label(cx), "Waiting for your answer");
    fixture.answer(false, cx);
    assert_eq!(
        poll_once(response.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(
            false
        )))
    );
    cx.run_until_parked();
    fixture.assert_submitted(false, cx);
    assert!(!fixture.queue.has_pending());
    assert!(cx.shown_system_notifications().is_empty());
    assert!(cx.delivered_system_notifications().is_empty());
    fixture.finish_tool("no-delivery", false, cx);
    assert!(!fixture.panel_present(cx));
    assert_eq!(
        cx.update(|cx| cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .unwrap()
            .conn()
            .query_row(
                "SELECT permission_mode FROM threads WHERE id = ?1",
                [&fixture.thread.id],
                |row| row.get::<_, String>(0)
            )
            .unwrap()),
        "confirm"
    );
}

#[gpui_kit::test]
fn delegated_question_uses_child_identity_without_completion_suppression(cx: &mut TestAppContext) {
    let parent = Fixture::new(cx, true);
    parent.start_message(cx);
    parent.navigate(true, cx);
    let fixture = parent.admit_child(cx);
    let mut response = fixture.ask("child-question", cx);
    assert_eq!(cx.shown_system_notifications().len(), 1);
    let notification = cx.shown_system_notifications()[0].clone();
    assert_eq!(notification.title.as_ref(), fixture.thread.title);
    fixture
        .window
        .read_with(cx, |owner, _| {
            assert!(
                owner
                    .agent_controller
                    .delegated_unbound
                    .contains(&fixture.thread.id)
            );
            assert_ne!(
                owner.stream_view.as_ref().map(|(id, _)| id),
                Some(&fixture.thread.id)
            );
        })
        .unwrap();
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.run_until_parked();
    fixture.frame(cx);
    assert!(fixture.queue.has_pending());
    assert!(fixture.panel_present(cx));
    assert!(matches!(poll_once(response.as_mut()), Poll::Pending));
    assert_eq!(cx.shown_system_notifications().len(), 1);
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: notification.tag.clone(),
        action_id: None,
    });
    cx.update(|_| {});
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        fixture.thread.id
    );
    fixture.frame(cx);
    assert!(!cx.update(|cx| cx.global::<SettingsOpen>().0));
    assert!(fixture.panel_present(cx));
    assert_eq!(fixture.label(cx), "Waiting for your answer");
    fixture.answer(true, cx);
    assert_eq!(
        poll_once(response.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(true)))
    );
    cx.run_until_parked();
    fixture.assert_submitted(true, cx);
    assert!(cx.delivered_system_notifications().is_empty());
    assert!(
        cx.dismissed_system_notifications()
            .contains(&notification.tag)
    );
    assert_eq!(cx.shown_system_notifications().len(), 1);
    fixture.finish_tool("child-question", true, cx);
    assert!(!fixture.panel_present(cx));
}

#[gpui_kit::test]
fn question_cleanup_does_not_clear_permission_confirmation(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx, true);
    fixture.start_message(cx);
    let mut question = fixture.ask("question", cx);
    let permissions = cx.update(|cx| fixture.stream.read(cx).permission_queue());
    fixture.stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: ToolCall {
                    id: "permission".into(),
                    tool: "bash".into(),
                    input_json: r#"{"command":"echo fixture","timeout":null}"#.into(),
                },
            },
            cx,
        )
    });
    let mut permission = permissions.request(
        PermissionRequest {
            call_id: "permission".into(),
            tool: "bash".into(),
            display_target: "echo fixture".into(),
            danger_rule_id: None,
            danger_reason: None,
            external: None,
            acp_options: None,
        },
        CancellationToken::new(),
    );
    cx.run_until_parked();
    assert!(cx.update(|cx| fixture.stream.read(cx).has_pending_permission()));
    fixture.answer(false, cx);
    assert_eq!(
        poll_once(question.as_mut()),
        Poll::Ready(RuntimeUserQuestionResponse::Answered(expected_answer(
            false
        )))
    );
    cx.run_until_parked();
    fixture.finish_tool("question", false, cx);
    assert!(fixture.label(cx).starts_with("Awaiting confirmation"));
    assert!(permissions.has_pending());
    assert!(matches!(poll_once(permission.as_mut()), Poll::Pending));
    fixture
        .stream
        .update(cx, |stream, cx| stream.timeout_permission(cx));
    assert!(matches!(
        poll_once(permission.as_mut()),
        Poll::Ready(Ok(PermissionDecision::Timeout))
    ));
}

impl vega_runtime::Provider for LiveProvider {
    fn chat_stream(
        &self,
        _: ChatRequest,
        _: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<EventStream, VegaError>> + Send>> {
        let mut turn = self.turn.lock().unwrap();
        *turn += 1;
        *self.time.lock().unwrap() += Duration::from_secs(2);
        let events = if *turn == 1 {
            vec![
                ProviderEvent::ToolUse {
                    id: "live".into(),
                    name: USER_QUESTION_TOOL.into(),
                    input_json: question_input().to_json().unwrap(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ]
        } else {
            vec![
                ProviderEvent::TextDelta("Done".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]
        };
        let finish = if *turn == 2 {
            self.resumed.send(LiveObservation::Resumed).unwrap();
            self.finish.lock().unwrap().take()
        } else {
            None
        };
        Box::pin(async move {
            if let Some(finish) = finish {
                finish.await.unwrap();
            }
            Ok(vega_runtime::question_test_event_stream(events))
        })
    }
}

struct LiveProvider {
    turn: Mutex<usize>,
    time: Arc<Mutex<Instant>>,
    resumed: tokio::sync::mpsc::UnboundedSender<LiveObservation>,
    finish: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
}

enum LiveObservation {
    Event(ConversationEvent),
    Waiting,
    Resumed,
    Deadline,
}

struct ObservedQuestionHook {
    queue: UserQuestionQueue,
    events: tokio::sync::mpsc::UnboundedSender<LiveObservation>,
}

impl RuntimeUserQuestionHook for ObservedQuestionHook {
    fn request(
        &self,
        request: RuntimeUserQuestionRequest,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = RuntimeUserQuestionResponse> + Send>> {
        let future = self.queue.request(request, cancel);
        self.events.send(LiveObservation::Waiting).unwrap();
        future
    }
}

#[gpui_kit::test]
async fn live_ui_uses_the_runtime_clock_and_matches_saved_terminal_work(cx: &mut TestAppContext) {
    cx.background_executor.allow_parking();
    let fixture = Fixture::new(cx, true);
    let database = fixture.directory.path().join("vega.db");
    let root = fixture.directory.path().to_path_buf();
    let thread_id = fixture.thread.id.clone();
    let queue = fixture.queue.clone();
    let cancel = fixture.cancel.clone();
    let time = Arc::new(Mutex::new(Instant::now()));
    let worker_time = time.clone();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let (finish_sender, finish_receiver) = tokio::sync::oneshot::channel();
    let (deadline_stop, deadline_done) = std::sync::mpsc::channel();
    let deadline_events = sender.clone();
    let deadline = std::thread::spawn(move || {
        if deadline_done.recv_timeout(Duration::from_secs(20)).is_err() {
            let _ = deadline_events.send(LiveObservation::Deadline);
        }
    });
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let store = Store::open(database).unwrap();
                let tools = vega_tools::Tools::new(root).unwrap();
                let provider = LiveProvider {
                    turn: Mutex::new(0),
                    time: worker_time.clone(),
                    resumed: sender.clone(),
                    finish: Mutex::new(Some(finish_receiver)),
                };
                let hook = ObservedQuestionHook {
                    queue,
                    events: sender.clone(),
                };
                let config = PersistenceActorConfig::default()
                    .with_user_questions(Arc::new(hook))
                    .with_owner_credential_reader(Arc::new(|| Ok(Vec::new())))
                    .with_work_time_source(Arc::new(move || *worker_time.lock().unwrap()));
                run_thread_task_with_images_reasoning_and_mcp(
                    &store,
                    &provider,
                    &tools,
                    &thread_id,
                    "Ask a question",
                    "Wait for the answer",
                    cancel,
                    &RejectPermissionHook,
                    move |event| {
                        sender.send(LiveObservation::Event(event.clone())).unwrap();
                        Ok(())
                    },
                    config,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                )
                .await
                .unwrap()
            })
    });
    let mut waited = false;
    let mut terminal = None;
    let mut message_id = None;
    let mut finish_sender = Some(finish_sender);
    let mut resumed = false;
    loop {
        let observation = receiver
            .recv()
            .await
            .expect("live worker stopped without terminal evidence");
        match observation {
            LiveObservation::Event(event) => {
                if let ConversationEvent::MessageStarted { message_id: id, .. } = &event {
                    message_id = Some(id.clone());
                }
                if let ConversationEvent::MessageFinished {
                    execution_duration_ms,
                    ..
                } = &event
                {
                    terminal = *execution_duration_ms;
                }
                fixture
                    .stream
                    .update(cx, |stream, cx| stream.apply_event(event, cx));
            }
            LiveObservation::Waiting => {}
            LiveObservation::Resumed => {
                resumed = true;
                cx.run_until_parked();
                assert!(!fixture.panel_present(cx));
                assert_eq!(fixture.label(cx), "Working for 4s");
                *time.lock().unwrap() += Duration::from_secs(3);
                assert_eq!(fixture.label(cx), "Working for 7s");
                finish_sender.take().unwrap().send(()).unwrap();
            }
            LiveObservation::Deadline => panic!("live question lifecycle did not reach completion"),
        }
        cx.run_until_parked();
        if !waited && fixture.queue.has_pending() && fixture.panel_present(cx) {
            waited = true;
            assert_eq!(fixture.label(cx), "Waiting for your answer");
            *time.lock().unwrap() += Duration::from_secs(900);
            fixture.navigate(true, cx);
            fixture.navigate(false, cx);
            assert_eq!(fixture.label(cx), "Waiting for your answer");
            fixture.answer(false, cx);
        }
        if terminal.is_some() {
            break;
        }
    }
    deadline_stop.send(()).unwrap();
    deadline.join().unwrap();
    let result = worker.join().unwrap();
    assert!(waited && resumed);
    assert!(!result.failed && !result.interrupted);
    assert_eq!(terminal, Some(7_000));
    assert_eq!(
        cx.update(|cx| fixture
            .stream
            .read(cx)
            .question_test_run_label(message_id.as_deref().unwrap(), cx))
            .as_deref(),
        Some("Worked for 7s")
    );
    let stored: u64 = cx.update(|cx| {
        let value = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .unwrap()
            .conn()
            .query_row(
                "SELECT execution_duration_ms FROM assistant_run_durations WHERE message_id = ?1",
                [&result.assistant_message_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        u64::try_from(value).unwrap()
    });
    assert_eq!(stored, 7_000);
}
