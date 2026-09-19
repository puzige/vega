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
                root.agent_controller.active.is_none() && !root.trusted_actions.is_busy()
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

fn save_budget(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
    click(f, "composer-context", cx);
    input_context(f, "context-limit-input", "20000", cx);
    input_context(f, "context-reserve-input", "2000", cx);
    click(f, "context-save", cx);
    pump_test_app(cx, |cx| {
        !f.stream
            .read_with(cx, |stream, _| stream.context_operation_busy())
            && vega_conversation::agent::read_context_settings(
                &f.store,
                &f.thread.id,
                &f.thread.model,
            )
            .is_ok_and(|settings| settings.is_some())
    });
}

#[gpui_kit::test]
async fn i76_context_settings_real_inputs_persist_and_reopen(cx: &mut gpui_kit::TestAppContext) {
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let f = fixture(cx, provider.clone());
    edit(&f, "keep this draft", cx);
    save_budget(&f, cx);
    let reopened = Store::open(f.store.database_path().expect("database")).expect("reopen");
    let settings =
        vega_conversation::agent::read_context_settings(&reopened, &f.thread.id, &f.thread.model)
            .expect("read settings")
            .expect("persisted settings");
    assert_eq!(settings.context_limit, Some(20000));
    assert_eq!(settings.output_reserve, 2000);
    assert_eq!(draft(&f, cx), "keep this draft");
    assert!(provider.requests().is_empty());
    f.root
        .update(cx, |root, cx| root.refresh_context_projection(true, cx));
    pump_test_app(cx, |cx| {
        !f.root
            .read_with(cx, |root, _| root.trusted_actions.is_busy())
    });
    assert!(
        !f.stream
            .read_with(cx, |stream, _| stream.context_operation_busy())
    );
}

#[gpui_kit::test]
async fn i76_context_invalid_input_never_persists_or_calls_provider(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let f = fixture(cx, provider.clone());
    click(&f, "composer-context", cx);
    input_context(&f, "context-limit-input", "100", cx);
    input_context(&f, "context-reserve-input", "100", cx);
    click(&f, "context-save", cx);
    cx.run_until_parked();
    assert!(
        vega_conversation::agent::read_context_settings(&f.store, &f.thread.id, &f.thread.model)
            .expect("settings")
            .is_none()
    );
    assert!(
        !f.root
            .read_with(cx, |root, _| root.trusted_actions.is_busy())
    );
    assert!(provider.requests().is_empty());
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
    save_budget(&f, cx);
    edit(&f, "unsent draft stays", cx);
    let before = vega_store::messages::recent(f.store.conn(), &f.thread.id, 100).expect("messages");
    click(&f, "context-compact", cx);
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
    click(&f, "context-cancel", cx);
    assert!(
        cancelled.load(Ordering::SeqCst),
        "visible cancel button emits its real intent"
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
    click(&f, "context-compact", cx);
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
            let (run, _) =
                root.agent_controller
                    .begin(f.thread.id.clone(), f.stream.clone(), None, None);
            root.begin_context_primary_owner(run);
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
async fn i76_context_same_entity_route_aba_rejects_old_primary_status(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let f = fixture(cx, Arc::new(vega_runtime::MockProvider::new(vec![])));
    let run = f.root.update(cx, |root, _| {
        let (run, _) =
            root.agent_controller
                .begin(f.thread.id.clone(), f.stream.clone(), None, None);
        root.begin_context_primary_owner(run);
        run
    });
    // Settings route keeps the exact stream entity cached. Returning to A
    // must still retire its prior route incarnation, not only compare ids.
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
        assert!(!f.stream.read(cx).context_operation_busy());
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
            .read_with(cx, |root, _| root.agent_controller.active.is_some())
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
            .read_with(cx, |root, _| root.agent_controller.active.is_none())
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
    save_budget(&f, cx);
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
    let run = f.root.update(cx, |root, _| {
        let (run, _) =
            root.agent_controller
                .begin(f.thread.id.clone(), f.stream.clone(), None, None);
        root.begin_context_primary_owner(run);
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
async fn i76_context_stale_settings_ack_retires_pending_without_applying_values(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let f = fixture(cx, Arc::new(vega_runtime::MockProvider::new(vec![])));
    edit(&f, "draft survives settings ABA", cx);
    click(&f, "composer-context", cx);
    input_context(&f, "context-limit-input", "20000", cx);
    input_context(&f, "context-reserve-input", "2000", cx);
    let routed = Arc::new(AtomicBool::new(false));
    let observed = routed.clone();
    // Interleave navigation immediately after the real save intent, before
    // its worker result can run. The same cached stream returns to route A.
    f.root.update(cx, |_, cx| {
        cx.subscribe(
            &f.stream,
            move |root, stream, _: &ContextSettingsRequested, cx| {
                if observed.swap(true, Ordering::SeqCst) {
                    return;
                }
                cx.set_global(SettingsOpen(true));
                root.cancel_context_if_route_stale(cx);
                cx.set_global(SettingsOpen(false));
                root.sync_context_route(&stream, cx);
            },
        )
        .detach();
    });
    click(&f, "context-save", cx);
    pump_test_app(cx, |cx| {
        !f.root
            .read_with(cx, |root, _| root.trusted_actions.is_busy())
    });
    assert!(routed.load(Ordering::SeqCst));
    assert!(
        !f.stream
            .read_with(cx, |stream, _| stream.context_operation_busy()),
        "old exact ACK must retire pending even after route ownership changes"
    );
    assert_eq!(draft(&f, cx), "draft survives settings ABA");
    // Existing input values remain retryable through the same actual button.
    click(&f, "context-save", cx);
    pump_test_app(cx, |cx| {
        !f.stream
            .read_with(cx, |stream, _| stream.context_operation_busy())
    });
    let saved =
        vega_conversation::agent::read_context_settings(&f.store, &f.thread.id, &f.thread.model)
            .expect("settings")
            .expect("saved");
    assert_eq!(saved.context_limit, Some(20000));
    assert_eq!(saved.output_reserve, 2000);
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
    save_budget(&f, cx);
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
