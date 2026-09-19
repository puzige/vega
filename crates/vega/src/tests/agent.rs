use super::*;

#[test]
fn commit_provider_policy_disables_retries() {
    assert_eq!(commit_retry_policy().max_retries, 0);
}

#[test]
fn bounded_agent_channel_preserves_burst_order_and_terminal() {
    let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    let producer = std::thread::spawn(move || {
        for index in 0..(AGENT_EVENT_CAPACITY + AGENT_EVENT_BATCH + 17) {
            sender
                .send(AgentUpdate::Event(
                    vega_conversation::types::ConversationEvent::TextDelta {
                        message_id: "message".into(),
                        delta: index.to_string(),
                    },
                ))
                .expect("bounded event send");
        }
        sender
            .send(AgentUpdate::Finished {
                success: true,
                reference_failure: None,
                credential_failure: false,
            })
            .expect("terminal send");
    });
    let mut seen = Vec::new();
    let finished = loop {
        let batch = drain_agent_updates(&receiver);
        assert!(batch.events.len() <= AGENT_EVENT_BATCH);
        for event in batch.events {
            if let vega_conversation::types::ConversationEvent::TextDelta { delta, .. } = event {
                seen.push(delta.parse::<usize>().expect("ordered index"));
            }
        }
        if let Some(finished) = batch.finished {
            break finished;
        }
        std::thread::yield_now();
    };
    producer.join().expect("bounded producer");
    assert!(finished);
    assert_eq!(seen, (0..seen.len()).collect::<Vec<_>>());
    assert_eq!(seen.len(), AGENT_EVENT_CAPACITY + AGENT_EVENT_BATCH + 17);
    assert!(AGENT_EVENT_POLL < Duration::from_millis(16));
}

#[test]
fn same_batch_applies_events_before_terminal() {
    let (sender, receiver) = mpsc::sync_channel(4);
    sender
        .send(AgentUpdate::Event(
            vega_conversation::types::ConversationEvent::MessageStarted {
                message_id: "durable".into(),
                seq: 2,
            },
        ))
        .expect("event send");
    sender
        .send(AgentUpdate::Finished {
            success: false,
            reference_failure: None,
            credential_failure: false,
        })
        .expect("terminal send");
    let batch = drain_agent_updates(&receiver);
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.finished, Some(false));
    assert!(matches!(
        &batch.events[0],
        vega_conversation::types::ConversationEvent::MessageStarted { message_id, .. }
            if message_id == "durable"
    ));
}

struct AgentWindowHarness {
    root: Entity<VegaWindow>,
}

impl Render for AgentWindowHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.root.clone()
    }
}

struct AgentWorkerGuard {
    cancel: tokio_util::sync::CancellationToken,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for AgentWorkerGuard {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[gpui_kit::test]
async fn production_agent_request_first_keeps_permission_until_proposal_ingress(
    cx: &mut gpui_kit::TestAppContext,
) {
    let repo = diff_controller_repo();
    let data = tempfile::tempdir().expect("permission order data root");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("permission order store");
    store.migrate().expect("permission order migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 permission order repo"),
        "permission-order-e2e",
        None,
    )
    .expect("permission order project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("permission order thread");
    let queue = vega_conversation::agent::PermissionQueue::new();
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx
        .new(|cx| ConversationStream::new_with_permission_queue(thread.clone(), queue.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
    });
    let window_root = root.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| AgentWindowHarness { root: window_root })
            })
        })
        .expect("permission order window");

    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![
        vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::ToolUse {
                id: "permission-order-call".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf once >> permission-order-recorder"}"#.into(),
            },
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::ToolUse,
            },
        ])],
        vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("finished".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ])],
    ]));
    let (generation, cancel) = root.update(cx, |root, _| {
        root.agent_controller.begin(
            thread.id.clone(),
            stream.clone(),
            Some("request-first".into()),
            None,
        )
    });
    let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    let worker_cancel = cancel.clone();
    let worker_thread = std::thread::spawn({
        let project_path = repo.path().to_path_buf();
        let worker_thread = thread.clone();
        let worker_queue = queue.clone();
        move || {
            run_agent_worker(
                database_path,
                project_path,
                worker_thread,
                PendingAgentRun::UserMessage("request-first".into()),
                worker_queue,
                worker_cancel,
                sender,
                None,
                None,
                None,
                None,
                Some(provider),
                Arc::new(AgentWorkerStartProbe::default()),
            );
        }
    });
    let mut worker = AgentWorkerGuard {
        cancel,
        handle: Some(worker_thread),
    };

    for _ in 0..400 {
        if queue.has_pending() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        queue.has_pending(),
        "the real worker must leave the permission latch pending before ingress"
    );
    cx.run_until_parked();
    assert!(
        queue.has_pending(),
        "a queue listener wake cannot consume a request before its proposal"
    );

    let first_batch = drain_agent_updates(&receiver);
    assert!(first_batch.finished.is_none());
    assert!(first_batch.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallProposed { call } if call.id == "permission-order-call"
    )));
    assert!(matches!(
        root.update(cx, |root, cx| root.apply_agent_batch_ingress(
            generation,
            &thread.id,
            &stream,
            first_batch,
            cx,
        )),
        AgentBatchIngress::Running
    ));
    cx.run_until_parked();
    assert!(stream.read_with(cx, |stream, _| stream.has_pending_permission()));
    assert!(!repo.path().join("permission-order-recorder").exists());

    cx.simulate_keystrokes(window.into(), "enter");
    let mut terminal = false;
    for _ in 0..400 {
        let batch = drain_agent_updates(&receiver);
        if !batch.events.is_empty() || batch.finished.is_some() {
            terminal = root.update(cx, |root, cx| {
                matches!(
                    root.apply_agent_batch_ingress(generation, &thread.id, &stream, batch, cx),
                    AgentBatchIngress::Finished { .. }
                )
            });
            if terminal {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if !terminal {
        worker.cancel.cancel();
    }
    worker
        .handle
        .take()
        .expect("permission order worker handle")
        .join()
        .expect("permission order worker");
    assert!(
        terminal,
        "the approved worker must reach the fenced terminal"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("permission-order-recorder"))
            .expect("approved command recorder"),
        "once"
    );
}

#[gpui_kit::test]
async fn production_agent_start_entry_surfaces_write_permission_and_continues(
    cx: &mut gpui_kit::TestAppContext,
) {
    // The production entry owns a dedicated worker thread. Permit its
    // PermissionQueue wakeup to cross the deterministic test scheduler; all
    // app-side assertions still advance through the GPUI pump below.
    cx.executor().allow_parking();
    let repo = diff_controller_repo();
    let config_root = tempfile::tempdir().expect("permission entry config root");
    let config_path = config_root.path().join("config.toml");
    fs::write(
        &config_path,
        r#"[[providers]]
name = "owned"
base_url = "https://provider.invalid/v1"
models = ["gpt-5.6-luna"]
key_ref = "owned"

[defaults]
model = "gpt-5.6-luna"
permission_mode = "confirm"

[ui]
theme = "light"
"#,
    )
    .expect("permission entry config");
    vega_store::keystore::set_key(config_root.path(), "owned", "permission-entry-test-key")
        .expect("permission entry credential");
    let data = tempfile::tempdir().expect("permission entry data root");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("permission entry store");
    store.migrate().expect("permission entry migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 permission entry repo"),
        "permission-entry-e2e",
        None,
    )
    .expect("permission entry project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-luna",
        PermissionMode::Confirm.as_str(),
    )
    .expect("permission entry thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));

    const WRITE_CALL: &str = "permission-entry-write";
    const WRITE_PATH: &str = "permission-entry.txt";
    const WRITE_BODY: &str = "PERMISSION_ENTRY_ONCE";
    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![
        vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::ToolUse {
                id: WRITE_CALL.into(),
                name: "write".into(),
                input_json: format!(r#"{{"path":"{WRITE_PATH}","content":"{WRITE_BODY}"}}"#),
            },
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::ToolUse,
            },
        ])],
        vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("write completed".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ])],
    ]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        // Both paths are owned fixture seams: catalog reads stay off the
        // user's config, while the worker never constructs a real provider.
        root.model_selection_config_override = Some(config_path.clone());
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
    });
    let window_root = root.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| AgentWindowHarness { root: window_root })
            })
        })
        .expect("permission entry window");

    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view.is_some()
                && !root.model_catalog_loading
                && root.configured_models.is_some()
                && matches!(
                    root.pricing_controller.state,
                    PricingControllerState::Ready { .. }
                )
        })
    });
    let stream = root
        .read_with(cx, |root, _| {
            root.stream_view.as_ref().map(|(_, stream)| stream.clone())
        })
        .expect("permission entry stream");
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    input.update(cx, |input, cx| input.set_text("write once", cx));
    let input_focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
    window
        .update(cx, |_, window, cx| window.focus(&input_focus, cx))
        .expect("permission entry composer focus");

    // This key dispatch goes through ConversationStream::submit_message,
    // ComposerSubmitted, VegaWindow::submit_composer and start_agent_run.
    cx.simulate_keystrokes(window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1
            && root.read_with(cx, |root, _| root.agent_controller.active.is_some())
            && stream.read_with(cx, |stream, _| {
                stream.has_pending_permission() && stream.has_active_permission_card()
            })
    });
    assert!(
        !repo.path().join(WRITE_PATH).exists(),
        "the write stays blocked while the PermissionCard is pending"
    );

    // The card's initial focus is the real "允许一次" action for an ordinary
    // write proposal; no queue responder is touched directly by this test.
    cx.simulate_keystrokes(window.into(), "enter");
    pump_test_app(cx, |cx| {
        provider.requests().len() == 2
            && repo.path().join(WRITE_PATH).is_file()
            && root.read_with(cx, |root, _| root.agent_controller.active.is_none())
    });

    assert_eq!(
        fs::read_to_string(repo.path().join(WRITE_PATH)).expect("approved write output"),
        WRITE_BODY
    );
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "one initial request and one observe round"
    );
    assert!(
        requests[1].messages.iter().any(|message| {
            message.role == vega_runtime::ChatRole::Tool
                && message.tool_call_id.as_deref() == Some(WRITE_CALL)
        }),
        "the next MockProvider request receives the write tool result"
    );

    let observer = Store::open(&database_path).expect("permission entry observer");
    let terminal = vega_store::tool_calls::terminal_results(observer.conn(), &thread.id)
        .expect("permission entry terminal audit");
    assert_eq!(terminal.len(), 1, "the run has one terminal tool audit");
    assert_eq!(
        terminal.get(WRITE_CALL).map(|call| call.status.as_str()),
        Some("success")
    );
    let state = vega_store::tool_calls::find_state(observer.conn(), WRITE_CALL)
        .expect("permission entry tool row")
        .expect("persisted write tool row");
    assert_eq!(state.status, "success");
    let approval =
        ApprovalAudit::from_json(state.approval.as_deref().expect("persisted write approval"))
            .expect("write approval audit");
    assert_eq!(approval.decision, Approval::Once);
    assert_eq!(approval.source, ApprovalSource::User);
    assert!(!stream.read_with(cx, |stream, _| stream.has_active_permission_card()));
}

#[test]
fn reference_rejection_is_carried_by_the_terminal_update() {
    let (sender, receiver) = mpsc::sync_channel(2);
    sender
        .send(AgentUpdate::Finished {
            success: false,
            reference_failure: Some(FileReferenceFailureCode::Missing),
            credential_failure: false,
        })
        .expect("terminal rejection send");
    let batch = drain_agent_updates(&receiver);
    assert_eq!(batch.finished, Some(false));
    assert_eq!(
        batch.reference_failure,
        Some(FileReferenceFailureCode::Missing)
    );
}

#[test]
fn finished_refresh_routes_only_to_matching_current_thread_cache() {
    assert!(current_cache_matches(Some("a"), Some("a"), "a"));
    assert!(
        !current_cache_matches(Some("b"), Some("a"), "a"),
        "A→B must not overwrite B's OpenedThread"
    );
    assert!(
        !current_cache_matches(Some("a"), Some("b"), "a"),
        "a stale cache cannot receive A's authoritative refresh"
    );
    assert!(
        current_cache_matches(Some("a"), Some("a"), "a"),
        "A→B→A must refresh the rebuilt A entity"
    );
}

#[gpui_kit::test]
async fn cancellation_keeps_active_until_durable_handshake_finishes(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        vega_ui::init(cx);
    });
    let (store, thread_id) = pending_plan();
    let thread =
        vega_conversation::threads::open_thread(&store, &thread_id).expect("thread projection");
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let mut controller = AppAgentController::default();
    let (generation, cancel) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        Some("draft".into()),
        None,
    );
    controller.request_active_cancel();
    assert!(cancel.is_cancelled());
    assert!(controller.active.is_some());
    assert_eq!(
        controller.accept_durable_start(generation + 1, &thread_id, &stream),
        None
    );
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        Some("draft".into())
    );
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        None
    );
    assert!(
        controller
            .finish(generation + 1, &thread_id, &stream)
            .is_none()
    );
    assert!(controller.active.is_some());
    let finished = controller
        .finish(generation, &thread_id, &stream)
        .expect("exact terminal owns active run");
    assert!(finished.pending_user_content.is_none());
    assert!(controller.active.is_none());

    let (next_generation, next_cancel) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        Some("second".into()),
        None,
    );
    assert_eq!(
        controller.accept_durable_start(next_generation, &thread_id, &stream),
        Some("second".into())
    );
    controller.request_active_cancel();
    assert!(next_cancel.is_cancelled());
    assert!(
        controller
            .finish(next_generation, &thread_id, &stream)
            .is_some()
    );

    let (prestart_generation, prestart_cancel) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        Some("retryable".into()),
        None,
    );
    controller.request_active_cancel();
    assert!(prestart_cancel.is_cancelled());
    let prestart = controller
        .finish(prestart_generation, &thread_id, &stream)
        .expect("cancelled pre-start worker still reaches terminal");
    assert_eq!(prestart.pending_user_content, Some("retryable".into()));

    let (approved_generation, _) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        None,
        Some("approved-instruction".into()),
    );
    let approved = controller
        .finish(approved_generation, &thread_id, &stream)
        .expect("approved pre-start failure reaches terminal");
    assert_eq!(
        approved.pending_approved_instruction.as_deref(),
        Some("approved-instruction")
    );
}

#[gpui_kit::test]
async fn stop_resume_fences_drop_every_late_callback_per_c5_fence_class(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        vega_ui::init(cx);
    });
    let (store, thread_id) = pending_plan();
    let thread =
        vega_conversation::threads::open_thread(&store, &thread_id).expect("thread projection");
    let other_thread = vega_conversation::threads::create_thread(
        &store,
        &thread.project_id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("second thread for the run fence");
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let other_stream = cx.new(|cx| ConversationStream::new(other_thread.clone(), cx));
    let mut controller = AppAgentController::default();
    let (generation, cancel) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        Some("draft".into()),
        None,
    );

    // Stop is visible first-wins: the token is cancelled, the run stays
    // owned until the durable handshake, and every fenced lookup fails.
    controller.request_active_cancel();
    assert!(cancel.is_cancelled());

    // 1) generation fence: a stale/foreign generation is refused.
    assert_eq!(
        controller.accept_durable_start(generation + 7, &thread_id, &stream),
        None
    );
    controller.observe_terminal_message(
        generation + 7,
        &thread_id,
        &stream,
        &ConversationEvent::Interrupted {
            message_id: "late-message".into(),
        },
    );
    assert!(
        controller
            .active
            .as_ref()
            .expect("stale observe must not consume the run")
            .terminal_message_id
            .is_none(),
        "stale-generation terminal observation is dropped"
    );
    assert!(
        controller
            .finish(generation + 7, &thread_id, &stream)
            .is_none()
    );

    // 2) run fence: same generation, wrong thread id is refused.
    assert_eq!(
        controller.accept_durable_start(generation, &other_thread.id, &stream),
        None
    );
    assert!(
        controller
            .finish(generation, &other_thread.id, &stream)
            .is_none()
    );

    // 3) route fence: same generation+thread, wrong stream is refused.
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &other_stream),
        None
    );
    assert!(
        controller
            .finish(generation, &thread_id, &other_stream)
            .is_none()
    );

    // The exact run consumes the durable start exactly once and owns the
    // terminal observation.
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        Some("draft".into())
    );
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        None
    );
    controller.observe_terminal_message(
        generation,
        &thread_id,
        &stream,
        &ConversationEvent::Interrupted {
            message_id: "terminal-message".into(),
        },
    );
    assert_eq!(
        controller
            .active
            .as_ref()
            .expect("owned run")
            .terminal_message_id
            .as_deref(),
        Some("terminal-message")
    );

    // 4) terminal fence: after the exact finish, every late callback for
    // the finished run is refused (no double consume, no late start).
    let finished = controller
        .finish(generation, &thread_id, &stream)
        .expect("exact terminal owns the run");
    assert!(finished.pending_user_content.is_none());
    assert!(controller.active.is_none());
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        None
    );
    assert!(controller.finish(generation, &thread_id, &stream).is_none());

    // Resume: a new generation with a fresh token; the previous run's
    // cancelled token stays cancelled and cannot bleed into the new run.
    let (next_generation, next_cancel) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        Some("resumed".into()),
        None,
    );
    assert_ne!(next_generation, generation);
    assert!(cancel.is_cancelled());
    assert!(!next_cancel.is_cancelled());
    assert_eq!(
        controller.accept_durable_start(next_generation, &thread_id, &stream),
        Some("resumed".into())
    );

    // Window/route cache fence: a stale cache cannot receive another
    // thread's authoritative refresh (A→B switch invalidates A).
    assert!(!current_cache_matches(
        Some(&other_thread.id),
        Some(&thread_id),
        &thread_id
    ));
    assert!(current_cache_matches(
        Some(&thread_id),
        Some(&thread_id),
        &thread_id
    ));
}

#[test]
fn deferred_provider_construction_cancel_starts_no_request_or_durable_message() {
    let repo = diff_controller_repo();
    let data = tempfile::tempdir().expect("owned preparation data");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("owned preparation store");
    store.migrate().expect("owned preparation migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("fixture path"),
        "prepare",
        None,
    )
    .expect("project");
    let thread = vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm")
        .expect("thread");
    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("fixture response".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]]));
    let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    let probe = Arc::new(AgentWorkerStartProbe::default());
    *probe.provider_construction_gate.lock().expect("gate") =
        Some((entered_sender, release_receiver));
    let cancel = tokio_util::sync::CancellationToken::new();
    let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    let worker = std::thread::spawn({
        let project_path = repo.path().to_path_buf();
        let database_path = database_path.clone();
        let thread = thread.clone();
        let cancel = cancel.clone();
        let provider = provider.clone();
        move || {
            run_agent_worker(
                database_path,
                project_path,
                thread,
                PendingAgentRun::UserMessage("owned pending draft".into()),
                vega_conversation::agent::PermissionQueue::new(),
                cancel,
                sender,
                None,
                None,
                None,
                None,
                Some(provider),
                probe,
            )
        }
    });
    entered_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("worker reached deferred provider construction");
    cancel.cancel();
    release_sender.send(()).expect("return from construction");
    let update = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("bounded terminal");
    assert!(
        matches!(
            update,
            AgentUpdate::Finished {
                success: false,
                reference_failure: None,
                credential_failure: false,
            }
        ),
        "no durable event may precede cancellation terminal"
    );
    worker.join().expect("preparation worker");
    assert!(
        provider.requests().is_empty(),
        "late provider construction must not send"
    );
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT count(*) FROM messages WHERE thread_id = ?1",
            [&thread.id],
            |row| row.get(0),
        )
        .expect("owned message count");
    assert_eq!(
        count, 0,
        "cancellation before runtime must not create a local echo or assistant row"
    );
}

#[test]
fn local_credential_preparation_failure_has_no_durable_messages() {
    let root = tempfile::tempdir().expect("owned fixture");
    let database_path = root.path().join("store.db");
    let store = Store::open(&database_path).expect("store");
    store.migrate().expect("migrate");
    let project = vega_store::projects::create(
        store.conn(),
        root.path().to_str().expect("path"),
        "owned",
        None,
    )
    .expect("project");
    let thread =
        vega_conversation::threads::create_thread(&store, &project.id, "owned-model", "confirm")
            .expect("thread");
    let config_path = root.path().join("config.toml");
    let mut config = vega_store::config::AppConfig::default();
    config.providers.push(vega_store::config::ProviderConfig {
        enabled: true,
        name: "owned".into(),
        base_url: "https://owned.invalid/v1".into(),
        models: vec!["owned-model".into()],
        key_ref: "owned".into(),
    });
    config.save_to(&config_path).expect("owned config");
    for unreadable in [false, true] {
        if unreadable {
            vega_store::keystore::set_key(root.path(), "owned", "owned-test-value").unwrap();
            std::fs::write(
                root.path().join("credentials/credentials.toml"),
                "invalid TOML",
            )
            .unwrap();
        }
        let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
        run_agent_worker(
            database_path.clone(),
            root.path().into(),
            thread.clone(),
            PendingAgentRun::UserMessage("draft survives missing credentials".into()),
            vega_conversation::agent::PermissionQueue::new(),
            tokio_util::sync::CancellationToken::new(),
            sender,
            None,
            Some(config_path.clone()),
            None,
            None,
            None,
            Arc::new(AgentWorkerStartProbe::default()),
        );
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
            AgentUpdate::Finished {
                success: false,
                reference_failure: None,
                credential_failure: true,
            }
        ));
        let messages: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM messages WHERE thread_id = ?1",
                [&thread.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(messages, 0, "production prep must stop before durable run");
        assert!(commit_provider(&thread, Some(&config_path)).is_err());
    }
}
