use super::*;
use vega_runtime::{ChatRole, MockProvider, Provider, ProviderEvent, ScriptStep, StopReason};
use vega_ui::conversation_stream::{PendingQueueAction, PendingQueueRequested};

struct FirstRequestGate {
    provider: Arc<MockProvider>,
    release: Arc<tokio::sync::Notify>,
    first: AtomicBool,
}

impl Provider for FirstRequestGate {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> futures::future::BoxFuture<
        'static,
        Result<vega_runtime::EventStream, vega_runtime::VegaError>,
    > {
        let future = self.provider.chat_stream(request, cancel.clone());
        let wait = self.first.swap(false, Ordering::SeqCst);
        let release = self.release.clone();
        Box::pin(async move {
            if wait {
                tokio::select! {
                    _ = cancel.cancelled() => return Err(vega_runtime::VegaError::Cancelled),
                    _ = release.notified() => {}
                }
            }
            future.await
        })
    }
}

fn done(text: &str) -> Vec<ScriptStep> {
    vec![
        ScriptStep::text(text),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]
}

fn gate(
    f: &Fixture,
    provider: Arc<MockProvider>,
    cx: &mut gpui_kit::TestAppContext,
) -> Arc<tokio::sync::Notify> {
    let release = Arc::new(tokio::sync::Notify::new());
    f.root.update(cx, |root, _| {
        root.agent_provider_override =
            Some(with_auxiliary_title_fixture(Arc::new(FirstRequestGate {
                provider,
                release: release.clone(),
                first: AtomicBool::new(true),
            })));
    });
    release
}

fn send(f: &Fixture, text: &str, cx: &mut gpui_kit::TestAppContext) {
    edit(f, text, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
}

fn queued(f: &Fixture, id: &str, behavior: PendingMessageBehavior) -> PendingMessage {
    PendingMessage {
        id: id.into(),
        thread_id: f.thread.id.clone(),
        generation: "prior-process".into(),
        behavior,
        content: id.into(),
        images: Vec::new(),
    }
}

fn queue_action(f: &Fixture, action: PendingQueueAction, cx: &mut gpui_kit::TestAppContext) {
    f.root.update(cx, |root, cx| {
        root.pending_queue_requested(
            f.stream.clone(),
            &PendingQueueRequested {
                thread_id: f.thread.id.clone(),
                action,
            },
            cx,
        )
    });
}

fn queue_idle(f: &Fixture, cx: &mut gpui_kit::TestAppContext) -> bool {
    f.root.read_with(cx, |root, _| {
        root.agent_controller.active.is_empty() && root.agent_controller.queue_operations.is_empty()
    }) && !f
        .stream
        .read_with(cx, |stream, _| stream.pending_queue().busy)
}

#[gpui_kit::test]
async fn issue299_controller_fifo_continues_background_thread_without_changing_route(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![
        done("a"),
        done("b"),
        done("c"),
    ]));
    let f = fixture(cx, provider.clone());
    let release = gate(&f, provider.clone(), cx);
    send(&f, "first", cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1 && draft(&f, cx).is_empty()
    });
    send(&f, "second", cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            stream.pending_queue().messages.len() == 1 && !stream.pending_queue().busy
        })
    });
    send(&f, "third", cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            stream.pending_queue().messages.len() == 2 && !stream.pending_queue().busy
        })
    });
    let other = vega_conversation::threads::create_thread(
        &f.store,
        &f.thread.project_id,
        "gpt-5.6-terra",
        "confirm",
    )
    .unwrap();
    cx.update(|cx| cx.set_global(OpenedThread(Some(other.clone()))));
    cx.run_until_parked();
    release.notify_one();
    pump_test_app(cx, |cx| {
        provider.requests().len() == 3 && queue_idle(&f, cx)
    });
    let requests = provider.requests();
    let inputs = requests
        .iter()
        .map(|request| {
            request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == ChatRole::User)
                .unwrap()
                .content
                .as_str()
        })
        .collect::<Vec<_>>();
    assert_eq!(inputs, ["first", "second", "third"]);
    let rows = vega_store::messages::recent(f.store.conn(), &f.thread.id, 50).unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| row.role == "user")
            .map(|row| row.content.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
    assert_eq!(rows.len(), 6);
    assert!(
        vega_store::messages::recent(f.store.conn(), &other.id, 50)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        cx.update(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        other.id
    );
    assert!(
        vega_conversation::pending_messages::load(&f.store, &f.thread.id)
            .unwrap()
            .is_empty()
    );
}

#[gpui_kit::test]
async fn issue299_controller_stop_pauses_queue_until_explicit_resume(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![
        done("unused"),
        done("resumed"),
    ]));
    let f = fixture(cx, provider.clone());
    let _release = gate(&f, provider.clone(), cx);
    send(&f, "first", cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1 && draft(&f, cx).is_empty()
    });
    send(&f, "after-stop", cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            stream.pending_queue().messages.len() == 1 && !stream.pending_queue().busy
        })
    });
    click(&f, "composer-stop", cx);
    pump_test_app(cx, |cx| queue_idle(&f, cx));
    assert_eq!(provider.requests().len(), 1);
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.pending_queue().paused)
    );
    assert_eq!(
        vega_conversation::pending_messages::load(&f.store, &f.thread.id)
            .unwrap()
            .len(),
        1
    );
    queue_action(&f, PendingQueueAction::Resume, cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 2 && queue_idle(&f, cx)
    });
    assert_eq!(
        provider.requests()[1]
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::User && message.content == "after-stop")
            .count(),
        1
    );
    assert!(
        vega_conversation::pending_messages::load(&f.store, &f.thread.id)
            .unwrap()
            .is_empty()
    );
}

#[gpui_kit::test]
async fn issue299_controller_failure_pauses_and_keeps_later_input(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new(vec![ScriptStep::Error {
        status: Some(400),
        message: "fixture failure".into(),
        retryable: false,
    }]));
    let f = fixture(cx, provider.clone());
    let release = gate(&f, provider.clone(), cx);
    send(&f, "first", cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1 && draft(&f, cx).is_empty()
    });
    send(&f, "retained", cx);
    release.notify_one();
    pump_test_app(cx, |cx| queue_idle(&f, cx));
    assert_eq!(provider.requests().len(), 1);
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.pending_queue().paused)
    );
    let pending = vega_conversation::pending_messages::load(&f.store, &f.thread.id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].content, "retained");
}

#[gpui_kit::test]
async fn issue299_controller_restart_recovers_claim_but_does_not_autorun(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new(done("recovered")));
    let f = fixture(cx, provider.clone());
    let message = queued(&f, "restart-item", PendingMessageBehavior::Steer);
    vega_conversation::pending_messages::enqueue(&f.store, &message).unwrap();
    assert!(
        vega_store::pending_messages::claim_steer(
            f.store.conn(),
            &f.thread.id,
            &message.id,
            &message.generation
        )
        .unwrap()
    );
    f.root.update(cx, |root, cx| {
        root.load_pending_queue(f.stream.clone(), f.thread.id.clone(), cx);
        root.start_agent_run(
            f.stream.clone(),
            &f.thread.id,
            PendingAgentRun::UserMessage("racing manual run".into()),
            cx,
        );
        assert!(root.agent_controller.active.is_empty());
        assert!(
            root.agent_controller
                .queue_operations
                .contains_key(&f.thread.id)
        );
    });
    pump_test_app(cx, |cx| queue_idle(&f, cx));
    assert!(provider.requests().is_empty());
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.pending_queue().paused)
    );
    assert!(
        vega_store::pending_messages::next_steer(f.store.conn(), &f.thread.id, &message.generation)
            .unwrap()
            .is_some()
    );
    queue_action(&f, PendingQueueAction::Resume, cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1 && queue_idle(&f, cx)
    });
    assert_eq!(
        vega_store::messages::recent(f.store.conn(), &f.thread.id, 50).unwrap()[0].id,
        message.id
    );
}

#[gpui_kit::test]
async fn issue299_controller_active_reload_never_releases_runtime_claim(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new(done("unused")));
    let f = fixture(cx, provider);
    let message = queued(&f, "claimed-item", PendingMessageBehavior::Steer);
    vega_conversation::pending_messages::enqueue(&f.store, &message).unwrap();
    assert!(
        vega_store::pending_messages::claim_steer(
            f.store.conn(),
            &f.thread.id,
            &message.id,
            &message.generation
        )
        .unwrap()
    );
    let generation = f.root.update(cx, |root, cx| {
        let (generation, _) = root
            .agent_controller
            .begin(f.thread.id.clone(), f.stream.clone(), None, None)
            .unwrap();
        root.load_pending_queue(f.stream.clone(), f.thread.id.clone(), cx);
        generation
    });
    pump_test_app(cx, |cx| {
        !f.stream
            .read_with(cx, |stream, _| stream.pending_queue().busy)
    });
    assert!(
        !vega_store::pending_messages::remove(f.store.conn(), &f.thread.id, &message.id).unwrap()
    );
    assert!(
        vega_store::pending_messages::next_steer(f.store.conn(), &f.thread.id, &message.generation)
            .unwrap()
            .is_none()
    );
    f.root.update(cx, |root, cx| {
        root.agent_controller
            .finish(generation, &f.thread.id, &f.stream)
            .unwrap();
        root.finish_pending_run(&f.thread.id, &f.stream, false, cx);
    });
    pump_test_app(cx, |cx| queue_idle(&f, cx));
    assert!(
        vega_store::pending_messages::next_steer(f.store.conn(), &f.thread.id, &message.generation)
            .unwrap()
            .is_some()
    );
}

#[gpui_kit::test]
async fn issue299_controller_operation_tokens_reject_stale_completion_and_run_start(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, Arc::new(MockProvider::new(done("unused"))));
    f.root.update(cx, |root, _| {
        let first = root
            .agent_controller
            .begin_queue_operation(&f.thread.id)
            .unwrap();
        assert!(
            root.agent_controller
                .begin_queue_operation(&f.thread.id)
                .is_none()
        );
        assert!(
            root.agent_controller
                .begin(f.thread.id.clone(), f.stream.clone(), None, None)
                .is_none()
        );
        assert!(
            !root
                .agent_controller
                .finish_queue_operation(&f.thread.id, first + 1)
        );
        assert!(
            root.agent_controller
                .finish_queue_operation(&f.thread.id, first)
        );
        let second = root
            .agent_controller
            .begin_queue_operation(&f.thread.id)
            .unwrap();
        assert!(
            !root
                .agent_controller
                .finish_queue_operation(&f.thread.id, first)
        );
        assert!(
            root.agent_controller
                .begin(f.thread.id.clone(), f.stream.clone(), None, None)
                .is_none()
        );
        assert!(
            root.agent_controller
                .finish_queue_operation(&f.thread.id, second)
        );
        let (generation, _) = root
            .agent_controller
            .begin(f.thread.id.clone(), f.stream.clone(), None, None)
            .unwrap();
        for reason in [
            ConversationStopReason::End,
            ConversationStopReason::Length,
            ConversationStopReason::TurnLimit,
        ] {
            root.agent_controller.observe_terminal_message(
                generation,
                &f.thread.id,
                &f.stream,
                &ConversationEvent::MessageFinished {
                    message_id: "assistant".into(),
                    stop_reason: reason,
                    execution_duration_ms: None,
                },
            );
            assert_eq!(
                root.agent_controller.active[&f.thread.id].queue_ready,
                reason == ConversationStopReason::End
            );
        }
        root.agent_controller.observe_terminal_message(
            generation,
            &f.thread.id,
            &f.stream,
            &ConversationEvent::MessageFinished {
                message_id: "assistant".into(),
                stop_reason: ConversationStopReason::End,
                execution_duration_ms: None,
            },
        );
        root.agent_controller.observe_terminal_message(
            generation,
            &f.thread.id,
            &f.stream,
            &ConversationEvent::MessageStarted {
                message_id: "next-assistant".into(),
                seq: 3,
            },
        );
        assert!(!root.agent_controller.active[&f.thread.id].queue_ready);
        root.agent_controller
            .finish(generation, &f.thread.id, &f.stream)
            .unwrap();
    });
}

#[gpui_kit::test]
async fn issue299_controller_late_running_submit_after_end_dispatches_once(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![
        done("first done"),
        done("late done"),
    ]));
    let f = fixture(cx, provider.clone());
    send(&f, "first", cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1 && queue_idle(&f, cx)
    });
    f.root.update(cx, |root, cx| {
        root.submit_composer(
            f.stream.clone(),
            &vega_ui::conversation_stream::ComposerSubmitted {
                thread_id: f.thread.id.clone(),
                content: "late accepted input".into(),
                images: Vec::new(),
                skill_intent: None,
                reasoning: Ok(None),
                pending_behavior: Some(PendingMessageBehavior::Queue),
            },
            cx,
        )
    });
    pump_test_app(cx, |cx| {
        provider.requests().len() == 2 && queue_idle(&f, cx)
    });
    assert_eq!(
        vega_store::messages::recent(f.store.conn(), &f.thread.id, 50)
            .unwrap()
            .iter()
            .filter(|row| row.role == "user" && row.content == "late accepted input")
            .count(),
        1
    );
    assert!(
        vega_conversation::pending_messages::load(&f.store, &f.thread.id)
            .unwrap()
            .is_empty()
    );
}

#[gpui_kit::test]
async fn issue299_controller_promotes_exact_pending_id_for_current_generation(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![
        done("a"),
        done("c"),
        done("b"),
    ]));
    let f = fixture(cx, provider.clone());
    let release = gate(&f, provider.clone(), cx);
    send(&f, "first", cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1 && draft(&f, cx).is_empty()
    });
    send(&f, "second", cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            stream.pending_queue().messages.len() == 1 && !stream.pending_queue().busy
        })
    });
    send(&f, "third", cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            stream.pending_queue().messages.len() == 2 && !stream.pending_queue().busy
        })
    });
    let id = f.stream.read_with(cx, |stream, _| {
        stream.pending_queue().messages[1].id.clone()
    });
    queue_action(&f, PendingQueueAction::Steer(id.clone()), cx);
    pump_test_app(cx, |cx| {
        !f.stream
            .read_with(cx, |stream, _| stream.pending_queue().busy)
    });
    let generation = f.root.read_with(cx, |root, _| {
        root.agent_controller.active[&f.thread.id]
            .queue_generation
            .clone()
    });
    assert_eq!(
        vega_store::pending_messages::next_steer(f.store.conn(), &f.thread.id, &generation)
            .unwrap()
            .unwrap()
            .id,
        id
    );
    release.notify_one();
    pump_test_app(cx, |cx| {
        provider.requests().len() == 3 && queue_idle(&f, cx)
    });
    let requests = provider.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == ChatRole::User)
                .unwrap()
                .content
                .as_str())
            .collect::<Vec<_>>(),
        ["first", "third", "second"]
    );
    let rows = vega_store::messages::recent(f.store.conn(), &f.thread.id, 50).unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| row.role == "user")
            .map(|row| row.content.as_str())
            .collect::<Vec<_>>(),
        ["first", "third", "second"]
    );
}

#[gpui_kit::test]
async fn issue299_controller_menu_default_persists_and_does_not_retag_pending(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, Arc::new(MockProvider::new(done("unused"))));
    let message = queued(&f, "unchanged-default", PendingMessageBehavior::Queue);
    vega_conversation::pending_messages::enqueue(&f.store, &message).unwrap();
    f.root.update(cx, |root, cx| {
        root.load_pending_queue(f.stream.clone(), f.thread.id.clone(), cx)
    });
    pump_test_app(cx, |cx| queue_idle(&f, cx));
    queue_action(&f, PendingQueueAction::ToggleDefault, cx);
    pump_test_app(cx, |cx| {
        queue_idle(&f, cx)
            && cx.update(|cx| {
                cx.global::<vega_ui::conversation_stream::RunningMessageDefault>()
                    .0
                    == PendingMessageBehavior::Steer
            })
    });
    let path = f._data.path().join("config.toml");
    assert_eq!(
        vega_store::config::read_from(&path)
            .unwrap()
            .agent
            .running_message_behavior,
        "steer"
    );
    assert_eq!(
        vega_conversation::pending_messages::load(&f.store, &f.thread.id).unwrap()[0].behavior,
        PendingMessageBehavior::Queue
    );
    queue_action(&f, PendingQueueAction::ToggleDefault, cx);
    pump_test_app(cx, |cx| {
        queue_idle(&f, cx)
            && cx.update(|cx| {
                cx.global::<vega_ui::conversation_stream::RunningMessageDefault>()
                    .0
                    == PendingMessageBehavior::Queue
            })
    });
    assert_eq!(
        vega_store::config::read_from(&path)
            .unwrap()
            .agent
            .running_message_behavior,
        "queue"
    );
}

#[gpui_kit::test]
async fn issue401_controller_steer_action_projects_before_the_boundary(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![done("a"), done("b")]));
    let f = fixture(cx, provider.clone());
    let release = gate(&f, provider.clone(), cx);
    send(&f, "first", cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1 && draft(&f, cx).is_empty()
    });
    send(&f, "second", cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            stream.pending_queue().messages.len() == 1 && !stream.pending_queue().busy
        })
    });
    let id = f.stream.read_with(cx, |stream, _| {
        stream.pending_queue().messages[0].id.clone()
    });
    let rows_before = f
        .stream
        .read_with(cx, |stream, cx| stream.virtual_row_count(cx));

    queue_action(&f, PendingQueueAction::Steer(id.clone()), cx);
    pump_test_app(cx, |cx| {
        !f.stream
            .read_with(cx, |stream, _| stream.pending_queue().busy)
    });

    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.projected_pending_ids().len() == 1),
        "the promoted steer must project into the conversation before the boundary"
    );
    assert!(f.stream.read_with(cx, |stream, _| {
        stream
            .rail_pending_ids()
            .iter()
            .all(|pending| pending != &id)
    }));
    assert!(
        f.stream
            .read_with(cx, |stream, cx| stream.virtual_row_count(cx))
            > rows_before,
        "the echo adds visible rows without waiting for the model boundary"
    );

    release.notify_one();
    pump_test_app(cx, |cx| {
        provider.requests().len() == 2 && queue_idle(&f, cx)
    });
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.projected_pending_ids().is_empty()),
        "the durable delivery must promote the echo in place"
    );
    let rows = vega_store::messages::recent(f.store.conn(), &f.thread.id, 50).unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| row.role == "user")
            .map(|row| row.content.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.role == "user" && row.content == "second")
            .count(),
        1,
        "the echo must not duplicate the durable user row"
    );
    assert!(
        vega_conversation::pending_messages::load(&f.store, &f.thread.id)
            .unwrap()
            .is_empty()
    );
}

#[gpui_kit::test]
async fn issue401_controller_stop_returns_the_unconsumed_steer_to_the_rail(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![
        done("unused"),
        done("resumed"),
    ]));
    let f = fixture(cx, provider.clone());
    let _release = gate(&f, provider.clone(), cx);
    send(&f, "first", cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1 && draft(&f, cx).is_empty()
    });
    send(&f, "second", cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            stream.pending_queue().messages.len() == 1 && !stream.pending_queue().busy
        })
    });
    let id = f.stream.read_with(cx, |stream, _| {
        stream.pending_queue().messages[0].id.clone()
    });
    queue_action(&f, PendingQueueAction::Steer(id.clone()), cx);
    pump_test_app(cx, |cx| {
        !f.stream
            .read_with(cx, |stream, _| stream.pending_queue().busy)
    });
    assert_eq!(
        f.stream
            .read_with(cx, |stream, _| stream.projected_pending_ids().len()),
        1
    );

    click(&f, "composer-stop", cx);
    pump_test_app(cx, |cx| queue_idle(&f, cx));
    assert_eq!(provider.requests().len(), 1);
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.projected_pending_ids().is_empty()),
        "a stopped run must not keep the optimistic entry"
    );
    assert!(f.stream.read_with(cx, |stream, _| {
        stream
            .rail_pending_ids()
            .iter()
            .any(|pending| pending == &id)
    }));
    assert_eq!(
        vega_conversation::pending_messages::load(&f.store, &f.thread.id)
            .unwrap()
            .len(),
        1
    );
}

#[gpui_kit::test]
async fn issue401_controller_default_steer_send_projects_before_the_boundary(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![done("a"), done("b")]));
    let f = fixture(cx, provider.clone());
    let release = gate(&f, provider.clone(), cx);
    cx.update(|cx| {
        cx.set_global(vega_ui::conversation_stream::RunningMessageDefault(
            PendingMessageBehavior::Steer,
        ))
    });
    send(&f, "first", cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1 && draft(&f, cx).is_empty()
    });
    send(&f, "steered", cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            stream.pending_queue().messages.len() == 1 && !stream.pending_queue().busy
        })
    });
    assert_eq!(
        f.stream
            .read_with(cx, |stream, _| stream.pending_queue().messages[0].behavior),
        PendingMessageBehavior::Steer
    );
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.projected_pending_ids().len() == 1),
        "a default-steer submission must project immediately after the durable enqueue"
    );
    release.notify_one();
    pump_test_app(cx, |cx| {
        provider.requests().len() == 2 && queue_idle(&f, cx)
    });
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.projected_pending_ids().is_empty())
    );
    assert_eq!(
        vega_store::messages::recent(f.store.conn(), &f.thread.id, 50)
            .unwrap()
            .iter()
            .filter(|row| row.role == "user" && row.content == "steered")
            .count(),
        1
    );
}

#[gpui_kit::test]
async fn issue401_controller_default_queue_send_stays_in_the_rail(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![done("a"), done("b")]));
    let f = fixture(cx, provider.clone());
    let release = gate(&f, provider.clone(), cx);
    cx.update(|cx| {
        cx.set_global(vega_ui::conversation_stream::RunningMessageDefault(
            PendingMessageBehavior::Queue,
        ))
    });
    send(&f, "first", cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1 && draft(&f, cx).is_empty()
    });
    send(&f, "queued", cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            stream.pending_queue().messages.len() == 1 && !stream.pending_queue().busy
        })
    });
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.projected_pending_ids().is_empty()),
        "queued messages keep the existing rail behaviour"
    );
    assert_eq!(
        f.stream
            .read_with(cx, |stream, _| stream.rail_pending_ids().len()),
        1
    );
    release.notify_one();
    pump_test_app(cx, |cx| {
        provider.requests().len() == 2 && queue_idle(&f, cx)
    });
    assert_eq!(
        vega_store::messages::recent(f.store.conn(), &f.thread.id, 50)
            .unwrap()
            .iter()
            .filter(|row| row.role == "user")
            .map(|row| row.content.as_str())
            .collect::<Vec<_>>(),
        ["first", "queued"]
    );
}

#[gpui_kit::test]
async fn issue401_controller_rejects_promotion_without_an_active_run(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new(done("unused")));
    let f = fixture(cx, provider);
    let message = queued(&f, "idle-steer", PendingMessageBehavior::Steer);
    vega_conversation::pending_messages::enqueue(&f.store, &message).unwrap();
    f.root.update(cx, |root, cx| {
        root.load_pending_queue(f.stream.clone(), f.thread.id.clone(), cx)
    });
    pump_test_app(cx, |cx| queue_idle(&f, cx));
    queue_action(&f, PendingQueueAction::Steer(message.id.clone()), cx);
    pump_test_app(cx, |cx| queue_idle(&f, cx));
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.projected_pending_ids().is_empty()),
        "a failed promotion must not fabricate a projected entry"
    );
    assert!(f.stream.read_with(cx, |stream, _| {
        stream
            .rail_pending_ids()
            .iter()
            .any(|pending| pending == &message.id)
    }));
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.pending_queue().error.is_some()),
        "the existing failure path stays visible"
    );
}
