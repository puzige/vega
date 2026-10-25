use super::VegaWindow;
use crate::app_agent::{
    AgentBatch, AgentBatchIngress, PendingAgentRun, ordinary_effective_authority,
};
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{AppContext, Entity, TestAppContext, WindowHandle, px, size};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use vega_conversation::history::{HistoryEntry, HistoryPage};
use vega_conversation::ordinary_submissions::for_thread;
use vega_conversation::types::{
    CommitFact, CommittedReceipt, ConversationEvent, OrdinarySubmission, PendingMessage,
    PendingMessageBehavior, PendingQueueProjection, PersistedPayload, RawSubmission,
    ReasoningChoice, SubmissionFailure, Thread,
};
use vega_store::thread_admission::{DatabaseIdentity, try_acquire};
use vega_theme::Theme;
use vega_ui::conversation_stream::{
    ComposerStopRequested, ComposerSubmitted, ConversationStream, OrdinaryAction,
    OrdinaryActionRequested, PendingQueueAction, PendingQueueRequested, PrivacyAction,
    PrivacyRequested,
};
use vega_ui::settings::SettingsOpen;
use vega_ui::sidebar::{
    NewTaskDefaults, OpenedThread, PendingDeleteConfirm, ProjectsCollapsed, SelectedProject,
    SessionsCollapsed, SidebarCollapsed, VegaStore,
};

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
            cx.subscribe(
                &stream,
                |controller, stream, request: &PendingQueueRequested, cx| {
                    controller.pending_queue_requested(stream, request, cx);
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
        assert_eq!(control.label(), Some(vega_i18n::t("ui.cancel")));
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

#[gpui_kit::test]
fn title_drain_wait_exposes_real_composer_stop_and_cancels_without_launching(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx);
    let message = queued(&fixture, cx);
    let requests = Rc::new(RefCell::new(0));
    let captured = requests.clone();
    cx.update(|cx| {
        cx.subscribe(&fixture.stream, move |_, _: &ComposerStopRequested, _| {
            *captured.borrow_mut() += 1;
        })
        .detach();
    });
    let owner = try_acquire(fixture.database, &fixture.thread.id)
        .unwrap()
        .unwrap();
    let drain = owner.drain();
    let title = owner.into_title_drain();
    let cancel = fixture.controller.update(cx, |controller, cx| {
        controller
            .agent_controller
            .run_drains
            .insert(fixture.thread.id.clone(), drain.clone());
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        assert!(
            !controller
                .agent_controller
                .active
                .contains_key(&fixture.thread.id)
        );
        assert!(!fixture.stream.read(cx).pending_queue().paused);
        controller.agent_controller.queue_drain_waits[&fixture.thread.id]
            .cancel
            .clone()
    });
    cx.run_until_parked();
    frame(&fixture, cx);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        let stop = window.find("composer-stop");
        assert!(stop.visible());
        assert_eq!(stop.label(), Some(vega_i18n::t("ui.stop")));
        assert!(stop.bounds().size.width > px(0.) && stop.bounds().size.height > px(0.));
        assert!(window.try_find("composer-running-send").is_none());
        assert!(window.try_find("pending-resume").is_none());
        window.click("composer-stop", cx);
    })
    .unwrap();
    cx.run_until_parked();
    frame(&fixture, cx);
    assert_eq!(*requests.borrow(), 1);
    assert!(cancel.is_cancelled());
    assert!(!drain.is_released());
    assert!(
        try_acquire(fixture.database, &fixture.thread.id)
            .unwrap()
            .is_none()
    );
    fixture
        .stream
        .update(cx, |stream, cx| stream.request_composer_stop(cx));
    assert_eq!(*requests.borrow(), 1);
    fixture.controller.update(cx, |controller, cx| {
        assert!(controller.agent_controller.queue_drain_waits.is_empty());
        assert!(controller.agent_test_launches.as_ref().unwrap().is_empty());
        assert!(fixture.stream.read(cx).pending_queue().paused);
        assert!(fixture.stream.read(cx).pending_queue().messages[0] == message);
    });
    cx.update_window(fixture.window.into(), |_, window, _| {
        assert!(window.try_find("composer-stop").is_none());
        assert!(window.find("pending-resume").visible());
    })
    .unwrap();
    drop(title);
    cx.executor().advance_clock(Duration::from_secs(31));
    cx.run_until_parked();
    fixture.controller.update(cx, |controller, cx| {
        assert!(controller.agent_test_launches.as_ref().unwrap().is_empty());
        assert!(fixture.stream.read(cx).pending_queue().messages[0] == message);
    });
}

#[gpui_kit::test]
fn title_drain_timeout_exposes_real_resume_and_dispatches_once_after_release(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx);
    let message = queued(&fixture, cx);
    cx.update(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        let messages = vega_conversation::pending_messages::enqueue(store, &message).unwrap();
        assert!(messages == vec![message.clone()]);
    });
    let requests = Rc::new(RefCell::new(Vec::<PendingQueueAction>::new()));
    let captured = requests.clone();
    cx.update(|cx| {
        cx.subscribe(
            &fixture.stream,
            move |_, event: &PendingQueueRequested, _| {
                captured.borrow_mut().push(event.action.clone());
            },
        )
        .detach();
    });
    let owner = try_acquire(fixture.database, &fixture.thread.id)
        .unwrap()
        .unwrap();
    let drain = owner.drain();
    let title = owner.into_title_drain();
    let cancel = fixture.controller.update(cx, |controller, cx| {
        controller
            .agent_controller
            .run_drains
            .insert(fixture.thread.id.clone(), drain.clone());
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        assert_eq!(controller.agent_controller.queue_drain_waits.len(), 1);
        assert!(!fixture.stream.read(cx).pending_queue().paused);
        assert!(fixture.stream.read(cx).pending_queue().error.is_none());
        controller.agent_controller.queue_drain_waits[&fixture.thread.id]
            .cancel
            .clone()
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(31));
    cx.run_until_parked();
    frame(&fixture, cx);
    assert!(!drain.is_released());
    assert!(cancel.is_cancelled());
    assert!(
        try_acquire(fixture.database, &fixture.thread.id)
            .unwrap()
            .is_none()
    );
    fixture.controller.update(cx, |controller, cx| {
        assert!(controller.agent_controller.queue_drain_waits.is_empty());
        assert!(controller.agent_test_launches.as_ref().unwrap().is_empty());
        assert!(fixture.stream.read(cx).pending_queue().paused);
        assert!(!fixture.stream.read(cx).pending_queue().busy);
        assert!(fixture.stream.read(cx).pending_queue().error.is_some());
        assert!(fixture.stream.read(cx).pending_queue().messages[0] == message);
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        assert!(controller.agent_controller.queue_drain_waits.is_empty());
    });
    cx.update_window(fixture.window.into(), |_, window, _| {
        let resume = window.find("pending-resume");
        assert!(resume.visible());
        assert_eq!(resume.label(), Some(vega_i18n::t("ui.continue_queue")));
        assert!(resume.bounds().size.width > px(0.) && resume.bounds().size.height > px(0.));
        assert!(window.try_find("composer-stop").is_none());
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(31));
    cx.run_until_parked();
    fixture.controller.update(cx, |controller, _| {
        assert!(controller.agent_controller.queue_drain_waits.is_empty());
        assert!(controller.agent_test_launches.as_ref().unwrap().is_empty());
    });
    drop(title);
    cx.executor().advance_clock(Duration::from_millis(10));
    cx.run_until_parked();
    fixture.controller.update(cx, |controller, _| {
        assert!(controller.agent_test_launches.as_ref().unwrap().is_empty());
    });
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.click("pending-resume", cx);
    })
    .unwrap();
    cx.run_until_parked();
    frame(&fixture, cx);
    assert_eq!(*requests.borrow(), vec![PendingQueueAction::Resume]);
    fixture.controller.update(cx, |controller, cx| {
        assert!(controller.agent_controller.queue_drain_waits.is_empty());
        assert!(!fixture.stream.read(cx).pending_queue().paused);
        assert!(!fixture.stream.read(cx).pending_queue().busy);
        assert!(fixture.stream.read(cx).pending_queue().error.is_none());
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
            _ => panic!("queued user run expected"),
        }
    });
    cx.executor().advance_clock(Duration::from_secs(31));
    cx.run_until_parked();
    fixture.controller.update(cx, |controller, cx| {
        controller.continue_pending_queue(&fixture.thread.id, &fixture.stream, cx);
        assert_eq!(controller.agent_test_launches.as_ref().unwrap().len(), 1);
        assert!(controller.agent_controller.queue_drain_waits.is_empty());
    });
}

#[gpui_kit::test]
fn delayed_ordinary_cancel_cannot_stop_newer_attempt_or_its_computer_use_controls(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx);
    let make = || {
        vega_conversation::types::OrdinarySubmission::begin(
            fixture.database,
            fixture.thread.id.clone(),
            vega_conversation::types::RawSubmission::new(
                "retained payload".into(),
                Vec::new(),
                1,
                "model".into(),
                None,
                None,
            ),
            try_acquire(fixture.database, &fixture.thread.id)
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    };
    let (a, worker_a) = make();
    let attempt_a = worker_a.attempt_id();
    let cancel_a = tokio_util::sync::CancellationToken::new();
    crate::ordinary_submission::retain_context(
        a.id(),
        crate::ordinary_submission::OrdinaryContext::new(fixture.thread.clone(), None, None, None),
    )
    .unwrap();
    crate::ordinary_submission::set_cancel(a.id(), attempt_a, cancel_a.clone()).unwrap();
    fixture.stream.update(cx, |stream, cx| {
        stream.project_ordinary_submission(a.clone(), cx)
    });
    worker_a
        .complete(Err(SubmissionFailure::PreparationFailed))
        .unwrap();
    a.seal(attempt_a).unwrap();
    let (b, worker_b) = make();
    let attempt_b = worker_b.attempt_id();
    let cancel_b = tokio_util::sync::CancellationToken::new();
    crate::ordinary_submission::retain_context(
        b.id(),
        crate::ordinary_submission::OrdinaryContext::new(fixture.thread.clone(), None, None, None),
    )
    .unwrap();
    crate::ordinary_submission::set_cancel(b.id(), attempt_b, cancel_b.clone()).unwrap();
    fixture.stream.update(cx, |stream, cx| {
        stream.project_ordinary_submission(b.clone(), cx)
    });
    let control = vega_runtime::computer_use::ComputerUseControl::new();
    let before = control.snapshot();
    let generation = fixture.controller.update(cx, |controller, _| {
        let (generation, _) = controller
            .agent_controller
            .begin(
                fixture.thread.id.clone(),
                fixture.stream.clone(),
                None,
                None,
            )
            .unwrap();
        let active = controller
            .agent_controller
            .active
            .get_mut(&fixture.thread.id)
            .unwrap();
        active.ordinary_attempt = Some((b.id(), attempt_b));
        active.cancel = cancel_b.clone();
        active.computer_use = Some(control.clone());
        active.computer_use_owner = "current computer-use owner".into();
        active.root_message_id = Some("current assistant".into());
        active.queue_ready = true;
        controller
            .agent_controller
            .ordinary_preparing
            .insert(fixture.thread.id.clone(), cancel_b.clone());
        generation
    });
    let stale = OrdinaryActionRequested {
        thread_id: fixture.thread.id.clone(),
        submission: a.id(),
        attempt: attempt_a,
        action: OrdinaryAction::Cancel,
    };
    for _ in 0..2 {
        fixture.controller.update(cx, |controller, cx| {
            controller.ordinary_action(fixture.stream.clone(), &stale, cx);
            let active = &controller.agent_controller.active[&fixture.thread.id];
            assert_eq!(active.generation, generation);
            assert_eq!(active.ordinary_attempt, Some((b.id(), attempt_b)));
            assert_eq!(active.root_message_id.as_deref(), Some("current assistant"));
            assert!(active.queue_ready);
            assert!(active.cancellation_reason.is_none());
            assert!(active.completion_outcome.is_none());
            assert_eq!(active.computer_use_owner, "current computer-use owner");
            assert!(
                active
                    .computer_use
                    .as_ref()
                    .unwrap()
                    .same_instance(&control)
            );
            assert!(!active.cancel.is_cancelled());
            assert!(
                !controller.agent_controller.ordinary_preparing[&fixture.thread.id].is_cancelled()
            );
        });
        assert!(cancel_a.is_cancelled());
        assert!(!cancel_b.is_cancelled());
        let after = control.snapshot();
        assert_eq!(after.epoch, before.epoch);
        assert_eq!(after.phase, before.phase);
        assert_eq!(after.capability, before.capability);
        assert_eq!(after.target_label, before.target_label);
        assert_eq!(after.error, before.error);
    }
    fixture.controller.update(cx, |controller, cx| {
        controller.ordinary_action(
            fixture.stream.clone(),
            &OrdinaryActionRequested {
                thread_id: fixture.thread.id.clone(),
                submission: b.id(),
                attempt: attempt_b,
                action: OrdinaryAction::Cancel,
            },
            cx,
        );
    });
    assert!(cancel_b.is_cancelled());
    assert_eq!(
        control.snapshot().phase,
        vega_runtime::computer_use::ControlPhase::Stopping
    );
    assert!(control.snapshot().epoch > before.epoch);
    worker_b
        .complete(Err(SubmissionFailure::Cancelled))
        .unwrap();
    b.seal(attempt_b).unwrap();
    crate::ordinary_submission::retire_context(a.id());
    crate::ordinary_submission::retire_context(b.id());
}

#[gpui_kit::test]
async fn privacy_requested_ingress_refreshes_retained_ordinary_from_real_history(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(cx);
    let database = fixture._directory.path().join("store.sqlite");
    let root = fixture._directory.path().join("config");
    let secret = format!("opaque{}", fixture.thread.id);
    vega_conversation::privacy::register_database(database.clone(), root.clone());
    vega_conversation::privacy::refresh_authority(&database, &root).unwrap();
    let (submission, worker) = OrdinarySubmission::begin(
        fixture.database,
        fixture.thread.id.clone(),
        RawSubmission::new(secret.clone(), Vec::new(), 0, "model".into(), None, None),
        try_acquire(fixture.database, &fixture.thread.id)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let assistant = submission.snapshot().attempts[0]
        .assistant_message_id
        .clone();
    cx.update(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        for (id, seq, role, content, status) in [
            (
                submission.user_message_id(),
                1,
                "user",
                secret.as_str(),
                "done",
            ),
            (assistant.as_str(), 2, "assistant", "", "failed"),
        ] {
            vega_store::messages::insert(
                store.conn(),
                &vega_store::messages::MessageRow {
                    id: id.into(),
                    thread_id: fixture.thread.id.clone(),
                    seq,
                    role: role.into(),
                    kind: "text".into(),
                    content: content.into(),
                    status: status.into(),
                    created_at: 1,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
        }
    });
    fixture.controller.update(cx, |_, cx| {
        cx.subscribe(
            &fixture.stream,
            |controller, stream, request: &PrivacyRequested, cx| {
                controller.request_privacy(stream, request, cx);
            },
        )
        .detach();
    });
    fixture.stream.update(cx, |stream, cx| {
        stream.project_ordinary_submission(submission.clone(), cx)
    });
    worker.bind_accepted_payload(secret.clone()).unwrap();
    worker
        .bind_persisted_payload(PersistedPayload::new(secret.clone(), Vec::new()))
        .unwrap();
    worker.begin_commit().unwrap();
    worker
        .record_committed(CommittedReceipt {
            user_message_id: submission.user_message_id().into(),
            user_seq: 1,
            assistant_message_id: assistant,
            assistant_seq: 2,
            next_seq: 3,
            persisted_text: secret.clone(),
            persisted_images: Vec::new(),
        })
        .unwrap();
    worker.record_cleanup(Ok(())).unwrap();
    worker
        .complete(Err(SubmissionFailure::LaunchFailed))
        .unwrap();
    submission
        .seal(submission.snapshot().attempts[0].id)
        .unwrap();
    frame(&fixture, cx);
    let anchor = fixture.stream.update(cx, |stream, cx| {
        assert!(stream.reveal_loaded_message(submission.user_message_id(), cx));
        stream
            .composer_input()
            .update(cx, |input, cx| input.set_text("next private draft", cx));
        stream.scroll_anchor_snapshot()
    });
    assert_eq!(
        anchor.message_id.as_deref(),
        Some(submission.user_message_id())
    );
    vega_store::keystore::set_key(&root, "classified-owner", &secret).unwrap();
    vega_conversation::privacy::refresh_authority(&database, &root).unwrap();
    let review = cx.update(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        vega_conversation::privacy::review(store, &fixture.thread.id).unwrap()
    });
    assert!(review.findings.iter().any(|finding| finding.exact));
    let observed = Rc::new(RefCell::new(0));
    let captured = observed.clone();
    let refreshes = Rc::new(RefCell::new(0));
    let refreshed = refreshes.clone();
    cx.update(|cx| {
        cx.subscribe(&fixture.stream, move |_, _: &PrivacyRequested, _| {
            *captured.borrow_mut() += 1;
        })
        .detach();
        cx.observe(&fixture.stream, move |_, _| *refreshed.borrow_mut() += 1)
            .detach();
    });
    fixture.stream.update(cx, |_, cx| {
        cx.emit(PrivacyRequested {
            thread_id: fixture.thread.id.clone(),
            action: PrivacyAction::Clean { snapshot: review },
        })
    });
    cx.wait_for(fixture.window.into(), Duration::from_secs(2), |_, cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        !vega_store::messages::find(store.conn(), submission.user_message_id())
            .unwrap()
            .unwrap()
            .content
            .contains(&secret)
            && *refreshes.borrow() > 0
    })
    .await;
    assert_eq!(*observed.borrow(), 1);
    cx.update(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        let row = vega_store::messages::find(store.conn(), submission.user_message_id())
            .unwrap()
            .unwrap();
        assert!(!row.content.contains(&secret));
        assert!(
            vega_conversation::privacy::review(store, &fixture.thread.id)
                .unwrap()
                .findings
                .is_empty()
        );
    });
    fixture.stream.update(cx, |stream, cx| {
        assert!(stream.reveal_loaded_message(submission.user_message_id(), cx));
        assert_eq!(stream.scroll_anchor_snapshot().identity, anchor.identity);
        assert_eq!(
            stream.composer_input().read(cx).text(),
            "next private draft"
        );
    });
    frame(&fixture, cx);
    cx.update_window(fixture.window.into(), |_, window, _| {
        assert!(
            window
                .find(format!("ordinary-action-{}", submission.user_message_id()))
                .visible()
        );
    })
    .unwrap();
}
