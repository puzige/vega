//! Production context subscriptions and workers; only the provider is mocked.
use super::*;

fn reply(text: &str) -> Vec<vega_runtime::ScriptStep> {
    vec![vega_runtime::ScriptStep::events(vec![
        vega_runtime::ProviderEvent::TextDelta(text.into()),
        vega_runtime::ProviderEvent::Done {
            stop_reason: vega_runtime::StopReason::End,
        },
    ])]
}

fn send_body(f: &Fixture, body: &str, cx: &mut gpui_kit::TestAppContext) {
    let before = vega_store::messages::recent(f.store.conn(), &f.thread.id, 100)
        .expect("messages")
        .len();
    edit(f, body, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        vega_store::messages::recent(f.store.conn(), &f.thread.id, 100)
            .expect("messages")
            .len()
            >= before + 2
            && f.root.read_with(cx, |root, _| {
                root.agent_controller.active.is_empty() && !root.trusted_actions.is_busy()
            })
    });
}

fn runtime_status(status: ContextCompactionStatus) -> ContextCompactionStatusRecord {
    ContextCompactionStatusRecord {
        generation: 1,
        status,
        updated_at: 1,
        estimated_tokens: Some(100),
        input_budget: Some(18000),
        target_tokens: Some(10800),
        source_version: Some(1),
        failure: None,
        usage: ContextCompactionUsageState::Pending,
    }
}

fn input_context(
    f: &Fixture,
    selector: &'static str,
    value: &str,
    cx: &mut gpui_kit::TestAppContext,
) {
    click(f, selector, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-a");
    let keys = value
        .chars()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    cx.simulate_keystrokes(f.window.into(), &keys);
}

fn open_model_context_editor(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| root.settings_view.is_some())
    });
    click(f, "settings-nav-providers", cx);
    click(f, "model-edit-0", cx);
    pump_test_app(cx, |cx| {
        VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds("model-context-input")
            .is_some()
    });
}

fn save_legacy_manual_budget(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
    // The removed manual UI is no longer a configuration path. These fixtures
    // exercise the still-internal cancellation/restart service and prove old
    // per-thread rows remain readable without becoming automatic-run fallback.
    vega_store::context_compaction::save_settings(
        f.store.conn(),
        &vega_store::context_compaction::ContextSettings {
            thread_id: f.thread.id.clone(),
            model: f.thread.model.clone(),
            context_limit: Some(20_000),
            output_reserve: 2_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .expect("legacy manual fixture");
    f.root
        .update(cx, |root, cx| root.refresh_context_projection(true, cx));
    pump_test_app(cx, |cx| {
        !f.root
            .read_with(cx, |root, _| root.trusted_actions.is_busy())
    });
}

fn request_internal_manual_compaction(f: &Fixture, cx: &mut gpui_kit::TestAppContext) -> u64 {
    f.stream.update(cx, |stream, cx| {
        let request_id = stream
            .reserve_context_operation_id(cx)
            .expect("operation id");
        cx.emit(ContextCompactionRequested {
            thread_id: f.thread.id.clone(),
            model: f.thread.model.clone(),
            request_id,
        });
        request_id
    })
}

#[gpui_kit::test]
async fn i76_context_settings_real_inputs_persist_and_reopen(cx: &mut gpui_kit::TestAppContext) {
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let f = fixture(cx, provider.clone());
    edit(&f, "keep this draft", cx);
    open_model_context_editor(&f, cx);
    input_context(&f, "model-context-input", "20000", cx);
    input_context(&f, "model-context-output", "2000", cx);
    click(&f, "model-save", cx);
    pump_test_app(cx, |_| {
        vega_store::context_compaction::load_model_policy(f.store.conn(), "owned", &f.thread.model)
            .is_ok_and(|policy| policy.is_some())
    });
    let reopened = Store::open(f.store.database_path().expect("database")).expect("reopen");
    let policy = vega_store::context_compaction::load_model_policy(
        reopened.conn(),
        "owned",
        &f.thread.model,
    )
    .expect("read policy")
    .expect("persisted policy");
    assert_eq!(policy.input_limit, Some(20_000));
    assert_eq!(policy.output_reserve, Some(2_000));
    assert!(policy.automatic_compaction);
    assert_eq!(draft(&f, cx), "keep this draft");
    assert!(provider.requests().is_empty());
    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    open_model_context_editor(&f, cx);
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    assert!(visual.debug_bounds("model-context-source").is_some());
    assert!(visual.debug_bounds("composer-context").is_none());
}

#[gpui_kit::test]
async fn i76_context_invalid_input_never_persists_or_calls_provider(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let f = fixture(cx, provider.clone());
    open_model_context_editor(&f, cx);
    input_context(&f, "model-context-input", "100", cx);
    input_context(&f, "model-context-output", "0", cx);
    click(&f, "model-save", cx);
    cx.run_until_parked();
    assert!(
        vega_store::context_compaction::load_model_policy(
            f.store.conn(),
            "owned",
            &f.thread.model,
        )
            .expect("policy")
            .is_none()
    );
    assert!(
        !f.root
            .read_with(cx, |root, _| root.trusted_actions.is_busy())
    );
    assert!(provider.requests().is_empty());
}

#[gpui_kit::test]
async fn i76_ambiguous_provider_cannot_freeze_either_model_policy_in_worker(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new(reply("must not run")));
    let f = fixture(cx, provider.clone());
    let config_path = f._data.path().join("config.toml");
    let mut config = vega_store::config::read_from(&config_path).expect("config");
    let mut second = config.providers[0].clone();
    second.name = "second".into();
    second.key_ref = "second".into();
    config.providers.push(second);
    config.save_to(&config_path).expect("ambiguous config");
    for (owner, input_limit) in [("owned", 10_000), ("second", 20_000)] {
        vega_store::context_compaction::save_model_policy(
            f.store.conn(),
            &vega_store::context_compaction::ModelContextPolicy {
                provider: owner.into(),
                model: f.thread.model.clone(),
                input_limit: Some(input_limit),
                output_reserve: Some(2_000),
                automatic_compaction: true,
                updated_at: 1,
            },
        )
        .expect("separate policy");
    }
    let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    run_agent_worker(
        f.store.database_path().expect("database").into(),
        f.repo.path().to_path_buf(),
        f.thread.clone(),
        PendingAgentRun::UserMessage("ambiguous model owner".into()),
        vega_conversation::agent::PermissionQueue::new(),
        tokio_util::sync::CancellationToken::new(),
        sender,
        None,
        Some(config_path),
        None,
        None,
        Some(provider.clone()),
        Arc::new(AgentWorkerStartProbe::default()),
    );
    assert_eq!(drain_agent_updates(&receiver).finished, Some(false));
    assert!(provider.requests().is_empty());
    assert!(
        vega_store::messages::recent(f.store.conn(), &f.thread.id, 10)
            .expect("messages")
            .is_empty(),
        "ambiguous policy selection must fail before transcript mutation"
    );
}

#[gpui_kit::test]
async fn i76_context_manual_real_worker_cancel_retry_preserves_transcript_and_draft(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![
        reply("First completed answer."),
        reply("Second completed answer."),
        vec![vega_runtime::ScriptStep::delay(Duration::from_secs(30))],
        reply("The user asked for an initial analysis. Completed it; retain the latest task."),
    ]));
    let f = fixture(cx, provider.clone());
    send_body(&f, "first historical task", cx);
    send_body(&f, "second current task", cx);
    save_legacy_manual_budget(&f, cx);
    edit(&f, "unsent draft stays", cx);
    let before = vega_store::messages::recent(f.store.conn(), &f.thread.id, 100).expect("messages");
    let first_request_id = request_internal_manual_compaction(&f, cx);
    pump_test_app(cx, |_| provider.requests().len() == 3);
    assert!(
        f.root
            .read_with(cx, |root, _| root.trusted_actions.is_busy())
    );
    let cancelled = Arc::new(AtomicBool::new(false));
    let observed = cancelled.clone();
    f.root.update(cx, |_, cx| {
        cx.subscribe(
            &f.stream,
            move |_, _, _: &ContextCompactionCancelRequested, _| {
                observed.store(true, Ordering::SeqCst)
            },
        )
        .detach();
    });
    f.stream.update(cx, |_, cx| {
        cx.emit(ContextCompactionCancelRequested {
            thread_id: f.thread.id.clone(),
            model: f.thread.model.clone(),
            request_id: first_request_id,
        });
    });
    assert!(
        cancelled.load(Ordering::SeqCst),
        "internal cancel intent reaches the real app worker"
    );
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| root.context_controller.manual.is_none())
    });
    assert!(
        !f.stream
            .read_with(cx, |stream, _| stream.context_operation_busy())
    );
    assert_eq!(draft(&f, cx), "unsent draft stays");
    request_internal_manual_compaction(&f, cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 4
            && f.root
                .read_with(cx, |root, _| root.context_controller.manual.is_none())
    });
    assert_eq!(
        before,
        vega_store::messages::recent(f.store.conn(), &f.thread.id, 100)
            .expect("unchanged transcript")
    );
    let projection = vega_conversation::agent::read_context_projection(
        &f.store,
        &f.thread.id,
        &f.thread.model,
        SYSTEM_PROMPT,
    )
    .expect("projection");
    assert_eq!(
        projection.last_status.expect("terminal status").status,
        ContextCompactionStatus::Succeeded
    );
    assert_eq!(draft(&f, cx), "unsent draft stays");
    let requests = provider.requests();
    assert!(requests[2].tools.is_empty());
    assert!(requests[3].tools.is_empty());
    assert_eq!(requests[3].model, f.thread.model);
}

#[gpui_kit::test]
async fn i76_context_automatic_runtime_generation_restarts_are_remapped(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let f = fixture(cx, Arc::new(vega_runtime::MockProvider::new(vec![])));
    for _ in 0..2 {
        f.root.update(cx, |root, cx| {
            let (run, _) = root
                .agent_controller
                .begin(f.thread.id.clone(), f.stream.clone(), None, None)
                .expect("thread admission");
            root.begin_context_primary_owner(run, cx);
            root.project_automatic_context(
                run,
                &f.stream,
                runtime_status(ContextCompactionStatus::Compacting),
                cx,
            );
            assert!(
                f.stream.read(cx).context_operation_busy(),
                "each run's raw generation 1 starts a new operation"
            );
            root.project_automatic_context(
                run,
                &f.stream,
                runtime_status(ContextCompactionStatus::Succeeded),
                cx,
            );
            assert!(!f.stream.read(cx).context_operation_busy());
            root.agent_controller
                .finish(run, &f.thread.id, &f.stream)
                .expect("owner");
        });
    }
}

#[gpui_kit::test]
async fn issue67_context_same_entity_route_aba_retains_live_primary_status(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let f = fixture(cx, Arc::new(vega_runtime::MockProvider::new(vec![])));
    let run = f.root.update(cx, |root, cx| {
        let (run, _) = root
            .agent_controller
            .begin(f.thread.id.clone(), f.stream.clone(), None, None)
            .expect("thread admission");
        root.begin_context_primary_owner(run, cx);
        run
    });
    // C67: Settings invalidates route loads, but preserves exact live run ownership.
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.run_until_parked();
    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    cx.run_until_parked();
    f.root.update(cx, |root, cx| {
        root.sync_context_route(&f.stream, cx);
        root.project_automatic_context(
            run,
            &f.stream,
            runtime_status(ContextCompactionStatus::Compacting),
            cx,
        );
        assert!(f.stream.read(cx).context_operation_busy());
        root.agent_controller.finish(run, &f.thread.id, &f.stream);
    });
}

#[gpui_kit::test]
async fn i76_context_primary_pre_message_started_blocks_manual_worker(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new(reply("must not start")));
    let f = fixture(cx, provider.clone());
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let probe = f
        .root
        .read_with(cx, |root, _| root.agent_worker_start_probe.clone());
    *probe.provider_construction_gate.lock().expect("gate") = Some((entered_tx, release_rx));
    edit(&f, "pending primary", cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| !root.agent_controller.active.is_empty())
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("real preparation boundary");
    assert!(
        vega_store::messages::recent(f.store.conn(), &f.thread.id, 100)
            .expect("messages")
            .is_empty()
    );
    f.stream.update(cx, |stream, cx| {
        let request_id = stream.reserve_context_operation_id(cx).expect("id");
        cx.emit(ContextCompactionRequested {
            thread_id: f.thread.id.clone(),
            model: f.thread.model.clone(),
            request_id,
        });
    });
    assert!(
        f.root
            .read_with(cx, |root, _| root.context_controller.manual.is_none())
    );
    assert!(provider.requests().is_empty());
    f.root.update(cx, |root, cx| root.cancel_active_agent(cx));
    release_tx.send(()).expect("release");
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_empty())
    });
    assert_eq!(draft(&f, cx), "pending primary");
    assert!(provider.requests().is_empty());
}

#[gpui_kit::test]
async fn i76_context_reopened_controller_recovers_abandoned_status_without_busy(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let f = fixture(cx, Arc::new(vega_runtime::MockProvider::new(vec![])));
    save_legacy_manual_budget(&f, cx);
    vega_store::context_compaction::insert_status(
        f.store.conn(),
        vega_store::context_compaction::NewContextCompactionStatus {
            thread_id: &f.thread.id,
            model: &f.thread.model,
            operation_key: "previous-process-summary",
            generation: 1,
            phase: "started",
            usage_state: "pending",
            failure: None,
            source_version: 0,
            estimated_tokens: 100,
            input_budget: 18000,
            target_tokens: 10800,
            created_at: 1,
        },
    )
    .expect("interrupted durable operation fixture");
    f.window
        .update(cx, |_, window, _| window.remove_window())
        .expect("close previous app window");
    let database = f.store.database_path().expect("path");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(database).expect("reopened store"),
            f.thread.clone(),
            cx,
        )
    });
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(f._data.path().join("config.toml"))
    });
    let new_root = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.), px(900.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |_, _| new_root,
        )
        .expect("fresh controller window")
    });
    pump_test_app(cx, |cx| {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.debug_bounds("context-status").is_some()
    });
    let stream = root.read_with(cx, |root, _| {
        root.stream_view
            .as_ref()
            .expect("reopened stream")
            .1
            .clone()
    });
    assert_ne!(stream, f.stream);
    assert!(!stream.read_with(cx, |stream, _| stream.context_operation_busy()));
    let reopened = Store::open(database).expect("independent reopen");
    let projection = vega_conversation::agent::read_context_projection(
        &reopened,
        &f.thread.id,
        &f.thread.model,
        SYSTEM_PROMPT,
    )
    .expect("recovered projection");
    assert_eq!(
        projection
            .settings
            .expect("restored settings")
            .context_limit,
        Some(20000)
    );
    assert_eq!(
        projection.last_status.expect("interrupted status").status,
        ContextCompactionStatus::Cancelled
    );
    assert!(projection.unknown_usage);
}

#[gpui_kit::test]
async fn i76_context_model_aba_same_stream_rejects_old_run(cx: &mut gpui_kit::TestAppContext) {
    cx.executor().allow_parking();
    let f = fixture(cx, Arc::new(vega_runtime::MockProvider::new(vec![])));
    let run = f.root.update(cx, |root, cx| {
        let (run, _) = root
            .agent_controller
            .begin(f.thread.id.clone(), f.stream.clone(), None, None)
            .expect("thread admission");
        root.begin_context_primary_owner(run, cx);
        run
    });
    for model in ["gpt-5.6-luna", "gpt-5.6-terra"] {
        let thread = vega_conversation::threads::set_thread_model(&f.store, &f.thread.id, model)
            .expect("external authoritative model change");
        f.stream.update(cx, |stream, cx| {
            stream.apply_authoritative_thread(thread.clone(), cx)
        });
        cx.update(|cx| cx.set_global(OpenedThread(Some(thread))));
        cx.run_until_parked();
        f.root
            .update(cx, |root, cx| root.sync_context_route(&f.stream, cx));
    }
    f.root.update(cx, |root, cx| {
        root.project_automatic_context(
            run,
            &f.stream,
            runtime_status(ContextCompactionStatus::Compacting),
            cx,
        );
        assert!(!f.stream.read(cx).context_operation_busy());
        root.agent_controller.finish(run, &f.thread.id, &f.stream);
    });
}

#[gpui_kit::test]
async fn i91_accounting_event_owner_retires_after_model_aba(cx: &mut gpui_kit::TestAppContext) {
    cx.executor().allow_parking();
    let f = fixture(cx, Arc::new(vega_runtime::MockProvider::new(vec![])));
    let run = f.root.update(cx, |root, cx| {
        let (run, _) = root
            .agent_controller
            .begin(f.thread.id.clone(), f.stream.clone(), None, None)
            .expect("thread admission");
        root.begin_context_primary_owner(run, cx);
        assert!(root.owns_primary_context_event(run, &f.stream, cx));
        run
    });
    let changed =
        vega_conversation::threads::set_thread_model(&f.store, &f.thread.id, "gpt-5.6-luna")
            .expect("authoritative model change");
    f.stream.update(cx, |stream, cx| {
        stream.apply_authoritative_thread(changed.clone(), cx)
    });
    cx.update(|cx| cx.set_global(OpenedThread(Some(changed))));
    cx.run_until_parked();
    f.root.update(cx, |root, cx| {
        root.sync_context_route(&f.stream, cx);
        assert!(!root.owns_primary_context_event(run, &f.stream, cx));
        root.agent_controller.finish(run, &f.thread.id, &f.stream);
    });
}

#[gpui_kit::test]
async fn i76_context_stale_settings_ack_retires_pending_without_applying_values(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let f = fixture(cx, Arc::new(vega_runtime::MockProvider::new(vec![])));
    edit(&f, "draft survives settings ABA", cx);
    open_model_context_editor(&f, cx);
    input_context(&f, "model-context-input", "20000", cx);
    input_context(&f, "model-context-output", "2000", cx);
    let old_view = f
        .root
        .read_with(cx, |root, _| root.settings_view.clone().expect("settings"));
    let routed = Arc::new(AtomicBool::new(false));
    let observed = routed.clone();
    // Navigate away immediately after the real model-policy save intent,
    // before its worker ACK can mutate the old SettingsView.
    f.root.update(cx, |_, cx| {
        cx.subscribe(
            &old_view,
            move |_, _, _: &vega_ui::settings::ModelContextSaveRequested, cx| {
                if observed.swap(true, Ordering::SeqCst) {
                    return;
                }
                cx.set_global(SettingsOpen(false));
                cx.notify();
            },
        )
        .detach();
    });
    click(&f, "model-save", cx);
    pump_test_app(cx, |_| {
        vega_store::context_compaction::load_model_policy(f.store.conn(), "owned", &f.thread.model)
            .is_ok_and(|policy| policy.is_some())
    });
    assert!(routed.load(Ordering::SeqCst));
    assert!(!cx.update(|cx| cx.global::<SettingsOpen>().0));
    assert_eq!(draft(&f, cx), "draft survives settings ABA");
    open_model_context_editor(&f, cx);
    let new_view = f.root.read_with(cx, |root, _| {
        root.settings_view.clone().expect("new settings")
    });
    assert_ne!(old_view, new_view, "route must replace the old ACK target");
    let saved =
        vega_store::context_compaction::load_model_policy(f.store.conn(), "owned", &f.thread.model)
            .expect("policy")
            .expect("saved");
    assert_eq!(saved.input_limit, Some(20_000));
    assert_eq!(saved.output_reserve, Some(2_000));
}

#[gpui_kit::test]
async fn i76_context_projection_restores_unknown_usage_from_other_model_idempotently(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let f = fixture(cx, Arc::new(vega_runtime::MockProvider::new(vec![])));
    vega_store::context_compaction::insert_status(
        f.store.conn(),
        vega_store::context_compaction::NewContextCompactionStatus {
            thread_id: &f.thread.id,
            model: "gpt-5.6-luna",
            operation_key: "unknown-previous-model",
            generation: 1,
            phase: "failed",
            usage_state: "unknown",
            failure: Some("unavailable"),
            source_version: 0,
            estimated_tokens: 100,
            input_budget: 18000,
            target_tokens: 10800,
            created_at: 1,
        },
    )
    .expect("durable unknown summary");
    let projection = vega_conversation::agent::read_context_projection(
        &f.store,
        &f.thread.id,
        &f.thread.model,
        SYSTEM_PROMPT,
    )
    .expect("projection");
    assert!(
        projection.last_status.is_none(),
        "unknown state is not restored via a current-model status"
    );
    assert!(projection.unknown_usage);
    f.stream.update(cx, |stream, cx| {
        stream.restore_meter(
            RestoredUsage {
                tokens: 50,
                cost: Some(Microcents(10)),
            },
            cx,
        )
    });
    assert_eq!(
        f.stream
            .read_with(cx, |stream, _| stream.meter_snapshot().cost),
        Some(Microcents(10))
    );
    f.root
        .update(cx, |root, cx| root.refresh_context_projection(false, cx));
    pump_test_app(cx, |cx| {
        f.stream
            .read_with(cx, |stream, _| stream.meter_snapshot().cost.is_none())
    });
    assert_eq!(
        f.stream
            .read_with(cx, |stream, _| stream.meter_snapshot().tokens),
        50
    );
    f.root
        .update(cx, |root, cx| root.refresh_context_projection(false, cx));
    cx.run_until_parked();
    assert_eq!(
        f.stream
            .read_with(cx, |stream, _| stream.meter_snapshot().tokens),
        50
    );
    assert!(
        f.stream
            .read_with(cx, |stream, _| stream.meter_snapshot().cost.is_none())
    );
}

#[gpui_kit::test]
async fn i76_context_metadata_failure_retries_after_real_settings_route_roundtrip(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let f = fixture(cx, Arc::new(vega_runtime::MockProvider::new(vec![])));
    save_legacy_manual_budget(&f, cx);
    f.stream.update(cx, |stream, cx| {
        stream.restore_meter(
            RestoredUsage {
                tokens: 50,
                cost: Some(Microcents(10)),
            },
            cx,
        )
    });
    let before = f.stream.read_with(cx, |stream, _| stream.meter_snapshot());
    // A real service failure: durable model diverges from captured projection
    // identity. No fake service or UI result is injected.
    vega_conversation::threads::set_thread_model(&f.store, &f.thread.id, "gpt-5.6-luna")
        .expect("change durable source");
    f.root
        .update(cx, |root, cx| root.refresh_context_projection(false, cx));
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.context_controller.last_load_succeeded == Some(false)
        })
    });
    assert_eq!(
        f.stream.read_with(cx, |stream, _| stream.meter_snapshot()),
        before,
        "metadata failure is not billed usage"
    );
    assert!(
        !f.stream
            .read_with(cx, |stream, _| stream.context_operation_busy())
    );
    vega_conversation::threads::set_thread_model(&f.store, &f.thread.id, &f.thread.model)
        .expect("source available again");
    // Existing user path: open Settings and return to the same conversation.
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.run_until_parked();
    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.context_controller.last_load_succeeded == Some(true)
        })
    });
    assert_eq!(
        f.stream.read_with(cx, |stream, _| stream.meter_snapshot()),
        before
    );
    assert!(
        !f.stream
            .read_with(cx, |stream, _| stream.context_operation_busy())
    );
}

#[gpui_kit::test]
async fn issue67_queued_background_plan_continuation_preserves_peer_and_artifact_owner(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("owned continuation"),
        vega_runtime::ScriptStep::delay(Duration::from_millis(800)),
        vega_runtime::ScriptStep::events(vec![vega_runtime::ProviderEvent::Done {
            stop_reason: vega_runtime::StopReason::End,
        }]),
    ]));
    let f = fixture(cx, provider.clone());
    let plan_thread =
        vega_conversation::threads::set_thread_mode(&f.store, &f.thread.id, ThreadMode::Plan)
            .expect("plan mode");
    insert(
        f.store.conn(),
        &MessageRow {
            id: "concurrent-plan".into(),
            thread_id: f.thread.id.clone(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .expect("plan message");
    complete_plan(
        f.store.conn(),
        &f.thread.id,
        "concurrent-plan",
        "Inspect the owned repository",
        2,
    )
    .expect("ready plan");
    f.stream.update(cx, |stream, cx| {
        stream.apply_thread(plan_thread.clone(), cx)
    });
    cx.update(|cx| cx.set_global(OpenedThread(Some(plan_thread))));
    // A controlled terminal handshake is the only injected boundary; review persistence and resumed worker are production.
    let (generation_a, cancel_a) = f.root.update(cx, |root, _| {
        root.agent_controller
            .begin(f.thread.id.clone(), f.stream.clone(), None, None)
            .expect("A draining owner")
    });
    let request = PlanReviewRequested {
        thread_id: f.thread.id.clone(),
        plan_id: "concurrent-plan".into(),
        action: PlanReviewAction::Approve,
    };
    f.root.update(cx, |root, cx| {
        root.review_plan(f.stream.clone(), &request, cx)
    });
    assert!(cancel_a.is_cancelled());
    let b = vega_conversation::threads::create_thread(
        &f.store,
        &f.thread.project_id,
        &f.thread.model,
        "confirm",
    )
    .expect("B");
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(b.clone())));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.stream_view.as_ref().is_some_and(|(id, _)| id == &b.id)
        })
    });
    let b_stream = f.root.read_with(cx, |root, _| {
        root.stream_view.as_ref().expect("B stream").1.clone()
    });
    f.root.update(cx, |root, cx| {
        root.start_agent_run(
            b_stream.clone(),
            &b.id,
            PendingAgentRun::UserMessage("B independent".into()),
            cx,
        )
    });
    pump_test_app(cx, |_| !provider.requests().is_empty());
    let cancel_b = f.root.read_with(cx, |root, _| {
        root.agent_controller
            .active
            .get(&b.id)
            .expect("B active")
            .cancel
            .clone()
    });
    let b_artifact = f.root.read_with(cx, |root, _| {
        root.artifact_controller
            .active
            .as_ref()
            .expect("B artifact route")
            .identity
            .clone()
    });
    f.root.update(cx, |root, cx| {
        root.agent_controller
            .finish(generation_a, &f.thread.id, &f.stream)
            .expect("A terminal");
        root.finish_owned_plan_review(f.stream.clone(), &request, cx);
        let resumed = root
            .agent_controller
            .active
            .get(&f.thread.id)
            .expect("A resumed in background");
        assert_ne!(resumed.generation, generation_a);
        assert!(
            root.artifact_controller
                .agent_route(resumed.generation, &f.stream)
                .is_some()
        );
        assert_eq!(root.agent_controller.active.len(), 2);
        assert!(root.agent_controller.pending_review.is_empty());
        assert!(!cancel_b.is_cancelled());
        assert!(
            root.artifact_controller
                .active
                .as_ref()
                .is_some_and(|route| route.identity == b_artifact)
        );
        assert!(
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.id == b.id)
        );
    });
    pump_test_app(cx, |_| provider.requests().len() == 2);
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_empty())
    });
    assert!(!cancel_b.is_cancelled());
    let plans =
        vega_conversation::plans::list_plans(&f.store, &f.thread.id).expect("durable plans");
    assert_eq!(plans[0].status, PlanStatus::Approved);
    for thread in [&f.thread, &b] {
        let status: String = f.store.conn().query_row("SELECT status FROM messages WHERE thread_id = ?1 AND role = 'assistant' ORDER BY seq DESC LIMIT 1", [&thread.id], |row| row.get(0)).expect("durable continuation");
        assert_eq!(status, "done");
    }
}

#[gpui_kit::test]
async fn issue67_manual_context_is_not_blocked_by_another_thread_owner(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![
        reply("First answer"),
        reply("Second answer"),
        reply("Summarized the earlier task; retain latest task"),
    ]));
    let f = fixture(cx, provider.clone());
    send_body(&f, "first task", cx);
    send_body(&f, "second task", cx);
    save_legacy_manual_budget(&f, cx);
    let other = vega_conversation::threads::create_thread(
        &f.store,
        &f.thread.project_id,
        &f.thread.model,
        "confirm",
    )
    .expect("peer");
    let other_stream = cx.new(|cx| ConversationStream::new(other.clone(), cx));
    let (generation, cancel) = f.root.update(cx, |root, _| {
        root.agent_controller
            .begin(other.id.clone(), other_stream.clone(), None, None)
            .expect("peer owner")
    });
    request_internal_manual_compaction(&f, cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 3
            && f.root
                .read_with(cx, |root, _| root.context_controller.manual.is_none())
    });
    assert!(!cancel.is_cancelled());
    f.root.update(cx, |root, _| {
        root.agent_controller
            .finish(generation, &other.id, &other_stream)
            .expect("peer unchanged");
    });
    let projection = vega_conversation::agent::read_context_projection(
        &f.store,
        &f.thread.id,
        &f.thread.model,
        SYSTEM_PROMPT,
    )
    .expect("manual result");
    assert_eq!(
        projection.last_status.expect("manual terminal").status,
        ContextCompactionStatus::Succeeded
    );
}
