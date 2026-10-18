use super::*;
use crate::native_subagents::NativeSubagentUpdate;
use vega_conversation::agent::{PermissionQueue, UserQuestionQueue};
use vega_conversation::types::{
    SubagentAdmission, SubagentOpenRequested, SubagentSnapshot, SubagentState,
    SubagentStopRequested,
};
use vega_ui::conversation_stream::{ComposerStopRequested, ComposerSubmitted};

fn batch(events: Vec<ConversationEvent>, subagents: Vec<NativeSubagentUpdate>) -> AgentBatch {
    AgentBatch {
        events,
        subagents,
        codex_mode_confirmed: false,
        computer_use_ready: None,
        mcp_unavailable: Vec::new(),
        reference_failure: None,
        credential_failure: false,
        finished: None,
    }
}

#[gpui_kit::test]
async fn m02_m07_m12_m13_m14_native_child_admission_navigation_queue_and_generation_ownership(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let repo = tempfile::tempdir().unwrap();
    let _git = vega_conversation::register_git_test_executor(
        repo.path(),
        Arc::new(|_, _, _| {
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
        "Native delegation",
        None,
    )
    .unwrap();
    let parent =
        vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm").unwrap();
    let mut child =
        vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm").unwrap();
    let other =
        vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm").unwrap();
    child.parent_thread_id = Some(parent.id.clone());
    let snapshot = SubagentSnapshot {
        agent_id: child.id.clone(),
        parent_thread_id: parent.id.clone(),
        parent_message_id: "parent-run".into(),
        message_id: "child-run".into(),
        name: "Research".into(),
        state: SubagentState::Running,
        result: String::new(),
        truncated: false,
    };
    vega_store::subagents::insert(
        store.conn(),
        &vega_store::subagents::DelegationRow {
            child_thread_id: child.id.clone(),
            parent_thread_id: parent.id.clone(),
            parent_message_id: snapshot.parent_message_id.clone(),
            spawn_call_id: "spawn".into(),
            child_message_id: snapshot.message_id.clone(),
            name: snapshot.name.clone(),
            task: "Research files".into(),
            state: "running".into(),
            result: String::new(),
            truncated: false,
            created_at: 1,
        },
    )
    .unwrap();
    cx.update(|cx| install_diff_window_globals(store, parent.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(parent.clone(), cx));
    let root = cx.new(VegaWindow::new);
    let (generation, parent_cancel) = root.update(cx, |root, _| {
        root.agent_provider_override = Some(Arc::new(vega_runtime::MockProvider::new(Vec::new())));
        root.stream_view = Some((parent.id.clone(), stream.clone()));
        root.agent_controller
            .begin(parent.id.clone(), stream.clone(), None, None)
            .unwrap()
    });
    let window_root = root.clone();
    let _window = cx
        .update(|cx| cx.open_window(Default::default(), move |_, _| window_root))
        .unwrap();
    cx.run_until_parked();
    let child_cancel = parent_cancel.child_token();
    let (reply, mut received) = tokio::sync::oneshot::channel();
    let admission = SubagentAdmission {
        thread: child.clone(),
        snapshot: snapshot.clone(),
        cancel: child_cancel.clone(),
        permission_queue: PermissionQueue::new(),
        user_questions: UserQuestionQueue::new(),
        task: "Research files".into(),
        queue_generation: "child-queue".into(),
    };
    root.update(cx, |root, cx| {
        assert!(matches!(
            root.apply_agent_batch_ingress(
                generation,
                &parent.id,
                &stream,
                batch(
                    vec![ConversationEvent::MessageStarted {
                        message_id: "parent-run".into(),
                        seq: 1
                    }],
                    vec![NativeSubagentUpdate::Admit {
                        admission: Box::new(admission),
                        reply
                    }]
                ),
                cx
            ),
            AgentBatchIngress::Running
        ));
    });
    assert!(received.try_recv().unwrap().is_ok());
    let (child_generation, child_stream) = root.read_with(cx, |root, _| {
        let active = &root.agent_controller.active[&child.id];
        (active.generation, active.stream.clone())
    });
    root.update(cx, |root, cx| {
        assert!(
            root.agent_controller
                .begin(child.id.clone(), child_stream.clone(), None, None)
                .is_none()
        );
        for event in [
            ConversationEvent::MessageStarted {
                message_id: "child-run".into(),
                seq: 2,
            },
            ConversationEvent::TextDelta {
                message_id: "child-run".into(),
                delta: "Child progress".into(),
            },
        ] {
            root.apply_subagent_update(
                &parent.id,
                &stream,
                NativeSubagentUpdate::Event {
                    thread_id: child.id.clone(),
                    message_id: "child-run".into(),
                    event,
                },
                cx,
            );
        }
        root.open_subagent(
            stream.clone(),
            &SubagentOpenRequested {
                source_thread_id: parent.id.clone(),
                thread_id: child.id.clone(),
            },
            cx,
        );
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, view)| id == &child.id && view == &child_stream)
        })
    });
    let composer = child_stream.read_with(cx, |stream, _| stream.composer_input());
    composer.update(cx, |input, cx| input.set_text("Preserved child draft", cx));
    root.update(cx, |root, cx| {
        root.enqueue_composer(
            child_stream.clone(),
            &ComposerSubmitted {
                thread_id: child.id.clone(),
                content: "Child follow-up".into(),
                pending_behavior: Some(PendingMessageBehavior::Queue),
                images: Vec::new(),
                skill_intent: None,
                reasoning: Ok(None),
            },
            cx,
        )
    });
    pump_test_app(cx, |cx| {
        child_stream.read_with(cx, |stream, _| {
            stream.pending_queue().messages.len() == 1 && !stream.pending_queue().busy
        })
    });
    let queued = child_stream.read_with(cx, |stream, _| stream.pending_queue().messages[0].clone());
    assert_eq!(queued.thread_id, child.id);
    assert_eq!(queued.generation, "child-queue");
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(other.clone())));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, _)| id == &other.id)
        })
    });
    root.update(cx, |root, cx| root.cancel_active_agent(cx));
    assert!(!child_cancel.is_cancelled());
    assert!(!parent_cancel.is_cancelled());
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(child.clone())));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(_, view)| view == &child_stream)
        })
    });
    assert_eq!(
        composer.read_with(cx, |input, _| input.text().to_owned()),
        "Preserved child draft"
    );
    root.update(cx, |root, cx| {
        root.stop_subagent(
            stream.clone(),
            &SubagentStopRequested {
                source_thread_id: parent.id.clone(),
                thread_id: child.id.clone(),
                message_id: "child-run".into(),
            },
            cx,
        )
    });
    assert!(!child_cancel.is_cancelled());
    assert!(!parent_cancel.is_cancelled());
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(parent.clone())));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, view)| id == &parent.id && view == &stream)
        })
    });
    root.update(cx, |root, cx| {
        root.stop_subagent(
            stream.clone(),
            &SubagentStopRequested {
                source_thread_id: parent.id.clone(),
                thread_id: child.id.clone(),
                message_id: "child-run".into(),
            },
            cx,
        )
    });
    assert!(child_cancel.is_cancelled());
    assert!(!parent_cancel.is_cancelled());
    let mut terminal = snapshot.clone();
    terminal.state = SubagentState::Interrupted;
    terminal.result = "Stopped".into();
    root.update(cx, |root, cx| {
        root.apply_subagent_update(
            &parent.id,
            &stream,
            NativeSubagentUpdate::Finished {
                snapshot: terminal,
                usage_complete: false,
            },
            cx,
        )
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            !root.agent_controller.active.contains_key(&child.id)
                && !root
                    .agent_controller
                    .queue_operations
                    .contains_key(&child.id)
                && !root
                    .agent_controller
                    .queue_dispatches
                    .contains_key(&child.id)
        }) && child_stream.read_with(cx, |stream, _| {
            !stream.pending_queue().busy && stream.pending_queue().paused
        })
    });
    child_stream.read_with(cx, |stream, _| {
        let messages = &stream.pending_queue().messages;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, queued.id);
        assert_eq!(messages[0].generation, "child-queue");
        assert_eq!(messages[0].content, "Child follow-up");
    });
    let (manual_generation, manual_cancel) = root.update(cx, |root, _| {
        root.agent_controller
            .begin(
                child.id.clone(),
                child_stream.clone(),
                Some("Continue independently".into()),
                None,
            )
            .unwrap()
    });
    assert!(manual_generation > child_generation);
    root.read_with(cx, |root, _| {
        let active = &root.agent_controller.active[&child.id];
        assert!(active.delegated_message_id.is_none());
        assert!(active.delegated_parent_thread.is_none());
        assert_ne!(active.queue_generation, queued.generation);
        assert_eq!(
            active.pending_user_content.as_deref(),
            Some("Continue independently")
        );
        assert!(
            !root
                .agent_controller
                .queue_dispatches
                .contains_key(&child.id)
        );
    });
    root.update(cx, |root, cx| {
        root.stop_subagent(
            stream.clone(),
            &SubagentStopRequested {
                source_thread_id: parent.id.clone(),
                thread_id: child.id.clone(),
                message_id: "child-run".into(),
            },
            cx,
        )
    });
    parent_cancel.cancel();
    assert!(!manual_cancel.is_cancelled());
    assert_eq!(
        child_stream.read_with(cx, |stream, _| stream.task_backend()),
        TaskBackend::Native
    );
    root.update(cx, |root, _| root.window_terminal_cleanup());
    assert!(manual_cancel.is_cancelled());
}

#[gpui_kit::test]
async fn m07_child_route_composer_stop_preserves_parent_run(cx: &mut gpui_kit::TestAppContext) {
    cx.executor().allow_parking();
    let data = tempfile::tempdir().unwrap();
    let store = Store::open(data.path().join("stop.db")).unwrap();
    store.migrate().unwrap();
    let parent =
        vega_conversation::threads::create_standalone_thread(&store, "mock", "confirm").unwrap();
    let mut child =
        vega_conversation::threads::create_standalone_thread(&store, "mock", "confirm").unwrap();
    child.parent_thread_id = Some(parent.id.clone());
    cx.update(|cx| install_diff_window_globals(store, child.clone(), cx));
    let parent_stream = cx.new(|cx| ConversationStream::new(parent.clone(), cx));
    let child_stream = cx.new(|cx| ConversationStream::new(child.clone(), cx));
    let root = cx.new(VegaWindow::new);
    let (parent_cancel, child_cancel) = root.update(cx, |root, _| {
        let (_, parent_cancel) = root
            .agent_controller
            .begin(parent.id.clone(), parent_stream.clone(), None, None)
            .unwrap();
        let (_, child_cancel) = root
            .agent_controller
            .begin(child.id.clone(), child_stream.clone(), None, None)
            .unwrap();
        root.stream_view = Some((child.id.clone(), child_stream.clone()));
        (parent_cancel, child_cancel)
    });
    root.update(cx, |root, cx| {
        root.stop_composer(
            child_stream.clone(),
            &ComposerStopRequested {
                thread_id: child.id.clone(),
            },
            cx,
        )
    });
    assert!(child_cancel.is_cancelled());
    assert!(!parent_cancel.is_cancelled());
    root.update(cx, |root, _| root.window_terminal_cleanup());
    assert!(parent_cancel.is_cancelled());
}
