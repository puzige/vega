use super::*;
use gpui_kit::gpui;
use gpui_kit::test::TestWindowExt;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use vega_conversation::history::{HistoryEntry, HistoryPage};
use vega_conversation::ordinary_submissions::for_thread;
use vega_store::thread_admission::try_acquire;

struct Fixture {
    _directory: tempfile::TempDir,
    thread: Thread,
    database: DatabaseIdentity,
    controller: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: WindowHandle<ConversationStream>,
}

fn fixture(cx: &mut TestAppContext) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let store = vega_store::Store::open(directory.path().join("store.sqlite")).unwrap();
    store.migrate().unwrap();
    let thread =
        vega_conversation::threads::create_standalone_thread(&store, "model", "confirm").unwrap();
    let database = store.database_identity();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(None));
        cx.set_global(OpenedThread(Some(thread.clone())));
        cx.set_global(SidebarCollapsed(false));
        cx.set_global(ProjectsCollapsed(false));
        cx.set_global(SessionsCollapsed(false));
        cx.set_global(PendingDeleteConfirm(None));
        cx.set_global(NewTaskDefaults::default());
        vega_ui::init(cx);
    });
    let controller = cx.update(|cx| {
        cx.new(|cx| {
            let mut controller = VegaWindow::new_with_services(cx, false);
            controller.ordinary_test_preparations = Some(Vec::new());
            controller.agent_test_launches = Some(Vec::new());
            controller
        })
    });
    let window = cx.open_window(size(px(1100.), px(900.)), |_, cx| {
        ConversationStream::new(thread.clone(), cx)
    });
    let stream = window.root(cx).unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        controller.update(cx, |controller, cx| {
            controller.stream_view = Some((thread.id.clone(), stream.clone()));
            cx.subscribe(
                &stream,
                |controller, stream, request: &ComposerSubmitted, cx| {
                    controller.submit_composer(stream, request, cx);
                },
            )
            .detach();
            cx.subscribe(
                &stream,
                |controller, stream, request: &OrdinaryActionRequested, cx| {
                    controller.ordinary_action(stream, request, cx);
                },
            )
            .detach();
            cx.subscribe(
                &stream,
                |controller, stream, request: &ComposerStopRequested, cx| {
                    controller.stop_composer(stream, request, cx);
                },
            )
            .detach();
        });
        window.render_frame(cx);
    })
    .unwrap();
    Fixture {
        _directory: directory,
        thread,
        database,
        controller,
        stream,
        window,
    }
}

fn frame(fixture: &Fixture, cx: &mut TestAppContext) {
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx)
    })
    .unwrap();
}

fn press(fixture: &Fixture, key: &str, cx: &mut TestAppContext) {
    cx.update_window(fixture.window.into(), |_, window, cx| window.press(key, cx))
        .unwrap();
}

fn empty_batch() -> AgentBatch {
    AgentBatch {
        composer_prediction: None,
        subagents: Vec::new(),
        events: Vec::new(),
        codex_mode_confirmed: false,
        computer_use_ready: None,
        mcp_unavailable: Vec::new(),
        reference_failure: None,
        credential_failure: false,
        finished: None,
    }
}

fn history(id: &str, content: &str, seq: i64) -> HistoryPage {
    HistoryPage {
        subagents: Vec::new(),
        entries: vec![HistoryEntry::UserText {
            seq,
            message_id: id.into(),
            content: content.into(),
        }],
        older_cursor: None,
        newer_cursor: None,
        newest_seq: Some(seq),
    }
}

#[gpui_kit::test]
fn controller_pending_is_visible_with_prepare_latched_and_duplicate_delivery_is_inert(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx);
    let requests = Rc::new(RefCell::new(Vec::<ComposerSubmitted>::new()));
    let captured = requests.clone();
    cx.update(|cx| {
        cx.subscribe(&fixture.stream, move |_, event: &ComposerSubmitted, _| {
            captured.borrow_mut().push(event.clone())
        })
        .detach()
    });
    fixture.stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            history("historical", "formal history positive control", 1),
            cx,
        )
    });
    frame(&fixture, cx);
    fixture.stream.update(cx, |stream, cx| {
        assert!(stream.reveal_loaded_message("historical", cx))
    });
    cx.update_window(fixture.window.into(), |_, window, cx| {
        fixture.stream.update(cx, |stream, cx| {
            stream.composer_input().update(cx, |input, cx| {
                input.set_text("ordinary captured input", cx)
            });
            stream.focus_composer(window, cx);
        });
    })
    .unwrap();
    frame(&fixture, cx);
    press(&fixture, "cmd-enter", cx);
    frame(&fixture, cx);
    assert_eq!(requests.borrow().len(), 1);
    let request = requests.borrow()[0].clone();
    let submission = for_thread(fixture.database, &fixture.thread.id)
        .unwrap()
        .pop()
        .unwrap();
    fixture.controller.update(cx, |controller, cx| {
        assert_eq!(
            controller
                .ordinary_test_preparations
                .as_ref()
                .unwrap()
                .len(),
            1
        );
        assert!(controller.agent_controller.active.is_empty());
        assert!(
            controller
                .agent_controller
                .ordinary_preparing
                .contains_key(&fixture.thread.id)
        );
        controller.submit_composer(fixture.stream.clone(), &request, cx);
        assert_eq!(
            controller
                .ordinary_test_preparations
                .as_ref()
                .unwrap()
                .len(),
            1
        );
    });
    let facts = submission.snapshot();
    assert_eq!(facts.attempts.len(), 1);
    assert_eq!(facts.attempts[0].workers, 2);
    assert!(facts.attempts[0].failure.is_none());
    assert!(matches!(facts.attempts[0].commit, CommitFact::NotAttempted));
    assert!(facts.accepted.is_none());
    assert!(
        try_acquire(fixture.database, &fixture.thread.id)
            .unwrap()
            .is_none()
    );
    let action_id = format!("ordinary-action-{}", submission.user_message_id());
    cx.update_window(fixture.window.into(), |_, window, cx| {
        let control = window.find(action_id.clone());
        assert!(control.visible());
        assert_eq!(control.label(), Some("取消"));
        assert!(control.bounds().size.width > px(0.) && control.bounds().size.height > px(0.));
        fixture.stream.update(cx, |stream, cx| {
            assert!(stream.composer_input().read(cx).text().is_empty());
            stream.composer_input().update(cx, |input, cx| {
                input.set_text("ordinary captured input", cx)
            });
            assert_ne!(
                stream.composer_input().read(cx).content_revision(),
                request.draft_revision
            );
        });
    })
    .unwrap();
    press(&fixture, "cmd-enter", cx);
    press(&fixture, "cmd-enter", cx);
    assert_eq!(requests.borrow().len(), 1);
    let (generation, _) = fixture.controller.update(cx, |controller, _| {
        controller
            .agent_controller
            .begin(
                fixture.thread.id.clone(),
                fixture.stream.clone(),
                None,
                None,
            )
            .unwrap()
    });
    let preparation = fixture.controller.update(cx, |controller, _| {
        controller
            .ordinary_test_preparations
            .as_mut()
            .unwrap()
            .pop()
            .unwrap()
    });
    let worker = &preparation.run.worker;
    worker
        .bind_accepted_payload(request.content.clone())
        .unwrap();
    worker
        .bind_persisted_payload(PersistedPayload::new(request.content.clone(), Vec::new()))
        .unwrap();
    worker.begin_commit().unwrap();
    let committed = CommittedReceipt {
        user_message_id: submission.user_message_id().into(),
        user_seq: 2,
        assistant_message_id: submission.snapshot().attempts[0]
            .assistant_message_id
            .clone(),
        assistant_seq: 3,
        next_seq: 4,
        persisted_text: request.content.clone(),
        persisted_images: Vec::new(),
    };
    worker.record_committed(committed.clone()).unwrap();
    worker.record_committed(committed.clone()).unwrap();
    fixture.controller.update(cx, |controller, cx| {
        assert!(matches!(
            controller.apply_agent_batch_ingress(
                generation,
                &fixture.thread.id,
                &fixture.stream,
                empty_batch(),
                cx
            ),
            AgentBatchIngress::Running
        ));
        assert!(matches!(
            controller.apply_agent_batch_ingress(
                generation,
                &fixture.thread.id,
                &fixture.stream,
                empty_batch(),
                cx
            ),
            AgentBatchIngress::Running
        ));
    });
    frame(&fixture, cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        assert!(window.try_find(action_id.clone()).is_none());
        fixture.stream.update(cx, |stream, cx| {
            assert_eq!(
                stream.composer_input().read(cx).text(),
                "ordinary captured input"
            );
            assert!(stream.reveal_loaded_message(submission.user_message_id(), cx));
        });
    })
    .unwrap();
    let assistant = committed.assistant_message_id.clone();
    fixture.controller.update(cx, |controller, cx| {
        let mut batch = empty_batch();
        batch.events = vec![
            ConversationEvent::MessageStarted {
                message_id: assistant.clone(),
                seq: 3,
            },
            ConversationEvent::MessageStarted {
                message_id: assistant,
                seq: 3,
            },
        ];
        assert!(matches!(
            controller.apply_agent_batch_ingress(
                generation,
                &fixture.thread.id,
                &fixture.stream,
                batch,
                cx
            ),
            AgentBatchIngress::Running
        ));
    });
    preparation.registered.complete(Ok(())).unwrap();
    crate::ordinary_submission::finish_root(preparation.run, Err(SubmissionFailure::LaunchFailed));
}

#[test]
fn ordinary_worker_authority_reuses_original_default_and_rejects_reconfigured_provider() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let reasoning_path = path.with_file_name(vega_store::reasoning::REASONING_FILE_NAME);
    let provider = vega_store::config::ProviderConfig {
        api: vega_store::config::ProviderApi::Responses,
        enabled: true,
        name: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        models: vec!["gpt-5.1".into()],
        key_ref: "test-provider".into(),
    };
    let mut config = vega_store::config::AppConfig {
        providers: vec![provider],
        ..Default::default()
    };
    config.save_to(&path).unwrap();
    std::fs::write(&reasoning_path, "version = 1\n[[preferences]]\nprovider = 'openai'\nmodel = 'gpt-5.1'\npreference = 'low'\n").unwrap();
    let store = vega_store::Store::open(":memory:").unwrap();
    let (submission, worker) = OrdinarySubmission::begin(
        store.database_identity(),
        "authority-thread".into(),
        RawSubmission::new("raw".into(), Vec::new(), 1, "gpt-5.1".into(), None, None),
        try_acquire(store.database_identity(), "authority-thread")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let run = vega_conversation::agent::OrdinaryRun::new(submission.clone(), Arc::new(worker));
    let first =
        ordinary_effective_authority(&run, &vega_store::config::read_from(&path).unwrap(), &path)
            .unwrap();
    assert_eq!(
        first.reasoning().choice,
        ReasoningChoice::Effort("low".into())
    );
    let attempt = run.worker.attempt_id();
    crate::ordinary_submission::finish_root(run, Err(SubmissionFailure::PreflightRejected));
    let retry = submission.try_retry(attempt).unwrap();
    let retry = vega_conversation::agent::OrdinaryRun::new(submission.clone(), Arc::new(retry));
    std::fs::write(&reasoning_path, "version = 1\n[[preferences]]\nprovider = 'openai'\nmodel = 'gpt-5.1'\npreference = 'high'\n").unwrap();
    let next = ordinary_effective_authority(
        &retry,
        &vega_store::config::read_from(&path).unwrap(),
        &path,
    )
    .unwrap();
    assert!(Arc::ptr_eq(&first, &next));
    assert_eq!(
        next.reasoning().choice,
        ReasoningChoice::Effort("low".into())
    );
    for drift in ["provider", "route", "disabled", "removed"] {
        let mut changed = config.clone();
        match drift {
            "provider" => changed.providers[0].name = "replacement".into(),
            "route" => changed.providers[0].base_url = "https://replacement.invalid/v1".into(),
            "disabled" => changed.providers[0].enabled = false,
            "removed" => changed.providers[0].models.clear(),
            _ => unreachable!(),
        }
        changed.save_to(&path).unwrap();
        assert!(
            ordinary_effective_authority(
                &retry,
                &vega_store::config::read_from(&path).unwrap(),
                &path
            )
            .is_err()
        );
        assert!(Arc::ptr_eq(
            &first,
            &submission.snapshot().authority.unwrap()
        ));
    }
    config.providers[0].enabled = true;
    config.save_to(&path).unwrap();
    assert!(
        ordinary_effective_authority(
            &retry,
            &vega_store::config::read_from(&path).unwrap(),
            &path
        )
        .is_ok()
    );
    crate::ordinary_submission::finish_root(retry, Err(SubmissionFailure::PreparationFailed));
}

fn queued(fixture: &Fixture, cx: &mut TestAppContext) -> PendingMessage {
    let message = PendingMessage {
        id: "queued-message".into(),
        thread_id: fixture.thread.id.clone(),
        generation: "queue-generation".into(),
        behavior: PendingMessageBehavior::Queue,
        content: "next queued content".into(),
        images: Vec::new(),
    };
    fixture.controller.update(cx, |controller, cx| {
        controller
            .agent_controller
            .queue_continue
            .insert(fixture.thread.id.clone(), true);
        controller.project_pending_queue(
            &fixture.thread.id,
            &fixture.stream,
            PendingQueueProjection {
                messages: vec![message.clone()],
                ..Default::default()
            },
            cx,
        );
    });
    message
}

#[gpui_kit::test]
fn retained_title_admission_wakes_exact_queue_once_without_pausing_or_releasing_early(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx);
    let message = queued(&fixture, cx);
    let owner = try_acquire(fixture.database, &fixture.thread.id)
        .unwrap()
        .unwrap();
    let drain = owner.drain();
    let title = owner.clone().into_title_drain();
    fixture.controller.update(cx, |controller, cx| {
        controller
            .agent_controller
            .run_drains
            .insert(fixture.thread.id.clone(), drain.clone());
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        assert_eq!(controller.agent_controller.queue_drain_waits.len(), 1);
        assert!(controller.agent_test_launches.as_ref().unwrap().is_empty());
        assert!(!fixture.stream.read(cx).pending_queue().paused);
        assert!(fixture.stream.read(cx).pending_queue().error.is_none());
        assert_eq!(
            fixture.stream.read(cx).pending_queue().messages[0].id,
            message.id
        );
    });
    drop(owner);
    cx.run_until_parked();
    assert!(!drain.is_released());
    assert!(
        try_acquire(fixture.database, &fixture.thread.id)
            .unwrap()
            .is_none()
    );
    drop(title);
    assert!(drain.is_released());
    cx.executor().advance_clock(Duration::from_millis(10));
    cx.run_until_parked();
    fixture.controller.update(cx, |controller, cx| {
        assert!(controller.agent_controller.queue_drain_waits.is_empty());
        let launches = controller.agent_test_launches.as_ref().unwrap();
        assert_eq!(launches.len(), 1);
        assert!(
            launches[0]
                .admission
                .verifies(fixture.database, &fixture.thread.id)
        );
        assert_eq!(
            launches[0]
                .queue_context
                .as_ref()
                .unwrap()
                .message_id
                .as_deref(),
            Some(message.id.as_str())
        );
        assert!(!launches[0].cancel.is_cancelled());
        match &launches[0].run {
            PendingAgentRun::UserMessage(submission) => {
                assert_eq!(submission.content, message.content);
                assert!(submission.ordinary.is_none());
            }
            _ => panic!("queue must remain a nonordinary user run"),
        }
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        assert_eq!(controller.agent_test_launches.as_ref().unwrap().len(), 1);
        assert!(!fixture.stream.read(cx).pending_queue().paused);
    });
}

#[gpui_kit::test]
fn replacement_queue_generation_cancels_old_wake_and_unrelated_owner_does_not_wait(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx);
    let message = queued(&fixture, cx);
    let owner = try_acquire(fixture.database, &fixture.thread.id)
        .unwrap()
        .unwrap();
    let drain = owner.drain();
    let title = owner.into_title_drain();
    fixture.controller.update(cx, |controller, cx| {
        controller
            .agent_controller
            .run_drains
            .insert(fixture.thread.id.clone(), drain);
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        let cancel = controller.agent_controller.queue_drain_waits[&fixture.thread.id]
            .cancel
            .clone();
        let mut replacement = message.clone();
        replacement.generation = "replacement-generation".into();
        controller.project_pending_queue(
            &fixture.thread.id,
            &fixture.stream,
            PendingQueueProjection {
                messages: vec![replacement],
                ..Default::default()
            },
            cx,
        );
        assert!(cancel.is_cancelled());
        assert!(controller.agent_controller.queue_drain_waits.is_empty());
    });
    drop(title);
    cx.executor().advance_clock(Duration::from_millis(10));
    cx.run_until_parked();
    let unrelated = try_acquire(fixture.database, &fixture.thread.id)
        .unwrap()
        .unwrap();
    fixture.controller.update(cx, |controller, cx| {
        assert!(controller.agent_test_launches.as_ref().unwrap().is_empty());
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        assert!(controller.agent_controller.queue_drain_waits.is_empty());
        assert!(controller.agent_test_launches.as_ref().unwrap().is_empty());
        assert!(fixture.stream.read(cx).pending_queue().paused);
        assert!(fixture.stream.read(cx).pending_queue().error.is_some());
        assert_eq!(
            fixture.stream.read(cx).pending_queue().messages[0].generation,
            "replacement-generation"
        );
    });
    drop(unrelated);
}

#[gpui_kit::test]
fn stopping_title_drain_wait_cancels_wake_and_retains_queued_message(cx: &mut TestAppContext) {
    let fixture = fixture(cx);
    let message = queued(&fixture, cx);
    let owner = try_acquire(fixture.database, &fixture.thread.id)
        .unwrap()
        .unwrap();
    let drain = owner.drain();
    let title = owner.into_title_drain();
    let cancel = fixture.controller.update(cx, |controller, cx| {
        controller
            .agent_controller
            .run_drains
            .insert(fixture.thread.id.clone(), drain);
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        let cancel = controller.agent_controller.queue_drain_waits[&fixture.thread.id]
            .cancel
            .clone();
        controller.stop_composer(
            fixture.stream.clone(),
            &ComposerStopRequested {
                thread_id: fixture.thread.id.clone(),
            },
            cx,
        );
        assert!(controller.agent_controller.queue_drain_waits.is_empty());
        assert!(fixture.stream.read(cx).pending_queue().paused);
        cancel
    });
    assert!(cancel.is_cancelled());
    drop(title);
    cx.executor().advance_clock(Duration::from_millis(10));
    cx.run_until_parked();
    fixture.controller.update(cx, |controller, cx| {
        assert!(controller.agent_test_launches.as_ref().unwrap().is_empty());
        assert!(fixture.stream.read(cx).pending_queue().paused);
        assert_eq!(
            fixture.stream.read(cx).pending_queue().messages[0].id,
            message.id
        );
    });
}
