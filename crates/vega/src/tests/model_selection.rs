use super::*;

pub(super) fn model_selection_config(path: &std::path::Path) {
    fs::write(
        path,
        r#"[[providers]]
name = "owned"
base_url = "https://provider.invalid/v1"
models = ["gpt-5.6-terra", "gpt-5.6-luna"]
key_ref = "owned"

[defaults]
model = "gpt-5.6-terra"
permission_mode = "confirm"

[ui]
theme = "dark"
"#,
    )
    .expect("owned model config");
}

#[gpui::test]
async fn model_selection_app_handler_persists_and_runs_exact_model(cx: &mut gpui::TestAppContext) {
    let config_root = tempfile::tempdir().expect("config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);

    let data_root = tempfile::tempdir().expect("data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("model selection store");
    store.migrate().expect("model selection migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root.path().to_str().expect("UTF-8 project path"),
        "model-selection-e2e",
        None,
    )
    .expect("model selection project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("model selection thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("global model-selection store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("ok".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.agent_provider_override = Some(provider.clone());
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.start_model_catalog_load(cx);
    });

    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            matches!(
                root.pricing_controller.state,
                PricingControllerState::Ready { .. }
            ) && root.configured_models.is_some()
        })
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.displayed_model().to_string()),
        "gpt-5.6-terra"
    );
    assert!(root.read_with(cx, |root, _| {
        root.model_options_for_pricing()
            .contains(&"gpt-5.6-luna".to_string())
    }));
    let settings = cx.new(|cx| {
        SettingsView::from_config(
            vega_store::config::read_from(&config_path).expect("owned settings config"),
            None,
            cx,
        )
    });
    root.update(cx, |_, cx| {
        cx.subscribe(&settings, |root, _, _: &SettingsSaved, cx| {
            root.on_settings_saved(cx);
        })
        .detach();
    });

    // Settings provider/model edits invalidate the catalog on close. A
    // temporary removal proves an empty/old result is not retained for the
    // window lifetime.
    let original_config = fs::read_to_string(&config_path).expect("read model config");
    let without_luna = original_config
        .replace(", \"gpt-5.6-luna\"", "")
        .to_string();
    fs::write(&config_path, &without_luna).expect("remove model from config");
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    root.update(cx, |root, cx| root.start_model_catalog_load(cx));
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.configured_models.is_some()
                && !root.model_catalog_loading
                && !root
                    .model_options_for_pricing()
                    .contains(&"gpt-5.6-luna".to_string())
        })
    });
    let current_thread_before = cx
        .update(|cx| cx.global::<OpenedThread>().0.clone())
        .expect("current thread before settings event");
    let displayed_model_before =
        stream.read_with(cx, |stream, _| stream.displayed_model().to_string());
    let pending_before = stream.read_with(cx, |stream, _| stream.has_pending_model_selection());
    fs::write(&config_path, &original_config).expect("restore model config");
    settings.update(cx, |_, cx| cx.emit(SettingsSaved));
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.configured_models.is_some()
                && !root.model_catalog_loading
                && root
                    .model_options_for_pricing()
                    .contains(&"gpt-5.6-luna".to_string())
        })
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.displayed_model().to_string()),
        displayed_model_before
    );
    assert_eq!(
        cx.update(|cx| cx.global::<OpenedThread>().0.clone()),
        Some(current_thread_before)
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.has_pending_model_selection()),
        pending_before
    );

    let gate = Arc::new(std::sync::Barrier::new(2));
    root.update(cx, |root, _| {
        root.model_selection_worker_gate = Some(gate.clone());
    });
    root.update(cx, |_root, cx| {
        cx.subscribe(&stream, |root, stream, request, cx| {
            root.apply_thread_model_selection(stream.clone(), request, cx);
        })
        .detach();
    });
    stream.update(cx, |stream, cx| {
        stream.request_model_selection("gpt-5.6-luna", cx);
    });
    assert!(stream.read_with(cx, |stream, _| { stream.has_pending_model_selection() }));
    assert!(root.read_with(cx, |root, _| root.trusted_actions.is_busy()));

    // The real submit app handler sees the same single-flight owner and must
    // refuse to start a provider request while the durable model write waits.
    let worker_starts = root.read_with(cx, |root, _| root.agent_worker_start_probe.clone());
    let starts_before = worker_starts.load();
    root.update(cx, |root, cx| {
        root.start_agent_run(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("blocked while selecting".into()),
            cx,
        );
    });
    assert_eq!(worker_starts.load(), starts_before);
    assert!(provider.requests().is_empty());
    assert!(root.read_with(cx, |root, _| root.trusted_actions.is_busy()));

    // The same production handler rejects a mode/permission write while the
    // model owner is pending, so a late model ack cannot overwrite a newer
    // thread-settings projection.
    root.update(cx, |root, cx| {
        root.persist_thread_settings(
            stream.clone(),
            &ThreadSettingsRequested {
                thread_id: thread.id.clone(),
                mode: Some(ThreadMode::Plan),
                permission_mode: Some(PermissionMode::Auto),
            },
            cx,
        );
    });
    let guarded_thread = vega_conversation::threads::open_thread(&store, &thread.id)
        .expect("guarded settings thread");
    assert_eq!(guarded_thread.mode, ThreadMode::Execute);
    assert_eq!(guarded_thread.permission_mode, PermissionMode::Confirm);

    // A→B→A while the worker is still in flight creates a fresh A entity.
    // The callback must reconcile that current entity from the worker's
    // authoritative row instead of writing the captured, stale stream.
    let other_thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("model selection route B");
    cx.update(|cx| cx.set_global(OpenedThread(Some(other_thread.clone()))));
    let reopened_route_stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    cx.update(|cx| cx.set_global(OpenedThread(Some(thread.clone()))));
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), reopened_route_stream.clone()));
    });

    gate.wait();
    pump_test_app(cx, |cx| {
        reopened_route_stream.read_with(cx, |stream, _| stream.displayed_model() == "gpt-5.6-luna")
            && root.read_with(cx, |root, _| root.agent_controller.active.is_none())
            && cx.update(|cx| {
                cx.global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|thread| thread.model == "gpt-5.6-luna")
            })
    });
    assert!(!stream.read_with(cx, |stream, _| stream.has_pending_model_selection()));
    assert!(!stream.read_with(cx, |stream, cx| stream.model_selection_blocked(cx)));
    assert_eq!(
        reopened_route_stream.read_with(cx, |stream, _| stream.displayed_model().to_string()),
        "gpt-5.6-luna"
    );
    assert!(!root.read_with(cx, |root, _| root.trusted_actions.is_busy()));

    let defaults = vega_store::config::read_from(&config_path).expect("read owned config");
    assert_eq!(defaults.defaults.model, "gpt-5.6-terra");

    root.update(cx, |root, cx| {
        root.start_agent_run(
            reopened_route_stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("run on selected model".into()),
            cx,
        );
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.agent_controller.active.is_none())
            && provider.requests().len() == 1
    });
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(provider.requests()[0].model, "gpt-5.6-luna");

    let reopened = Store::open(&database_path).expect("reopen model store");
    let reopened_thread = vega_conversation::threads::open_thread(&reopened, &thread.id)
        .expect("reopen durable thread");
    assert_eq!(reopened_thread.model, "gpt-5.6-luna");
    let restarted_stream = cx.new(|cx| ConversationStream::new(reopened_thread, cx));
    assert_eq!(
        restarted_stream.read_with(cx, |stream, _| stream.displayed_model().to_string()),
        "gpt-5.6-luna"
    );

    // Recreate the app controller as well as the stream (simulated restart)
    // and prove the next real submit still sends the durable model B.
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("restarted global model-selection store"),
            thread.clone(),
            cx,
        )
    });
    let restarted_root = cx.new(VegaWindow::new);
    restarted_root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.agent_provider_override = Some(provider.clone());
        root.stream_view = Some((thread.id.clone(), restarted_stream.clone()));
        root.start_model_catalog_load(cx);
    });
    restarted_root.update(cx, |_root, cx| {
        cx.subscribe(&restarted_stream, |root, stream, request, cx| {
            root.apply_thread_model_selection(stream.clone(), request, cx);
        })
        .detach();
    });
    pump_test_app(cx, |cx| {
        restarted_root.read_with(cx, |root, _| {
            matches!(
                root.pricing_controller.state,
                PricingControllerState::Ready { .. }
            ) && root.configured_models.is_some()
        })
    });
    restarted_root.update(cx, |root, cx| {
        root.start_agent_run(
            restarted_stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("run after restart".into()),
            cx,
        );
    });
    pump_test_app(cx, |cx| {
        restarted_root.read_with(cx, |root, _| root.agent_controller.active.is_none())
            && provider.requests().len() == 2
    });
    assert_eq!(provider.requests().len(), 2);
    assert_eq!(provider.requests()[1].model, "gpt-5.6-luna");

    // A representative persistence failure is still authoritative failure:
    // the missing owned config must not create a file, change the durable
    // model, or produce a provider request. The app handler must release the
    // exact busy owner so the route can be retried.
    let failure_thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("model selection failure thread");
    let failure_stream = cx.new(|cx| ConversationStream::new(failure_thread.clone(), cx));
    let missing_config = config_root.path().join("missing-selection.toml");
    cx.update(|cx| cx.set_global(OpenedThread(Some(failure_thread.clone()))));
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(missing_config.clone());
        root.stream_view = Some((failure_thread.id.clone(), failure_stream.clone()));
    });
    root.update(cx, |_root, cx| {
        cx.subscribe(&failure_stream, |root, stream, request, cx| {
            root.apply_thread_model_selection(stream.clone(), request, cx);
        })
        .detach();
    });
    failure_stream.update(cx, |stream, cx| {
        stream.request_model_selection("gpt-5.6-luna", cx);
    });
    assert!(failure_stream.read_with(cx, |stream, _| stream.has_pending_model_selection()));
    pump_test_app(cx, |cx| {
        failure_stream.read_with(cx, |stream, _| !stream.has_pending_model_selection())
            && root.read_with(cx, |root, _| !root.trusted_actions.is_busy())
    });
    let failed_thread = vega_conversation::threads::open_thread(&store, &failure_thread.id)
        .expect("failed model selection thread");
    assert_eq!(failed_thread.model, "gpt-5.6-terra");
    assert_eq!(
        failure_stream.read_with(cx, |stream, _| stream.displayed_model().to_string()),
        "gpt-5.6-terra"
    );
    assert!(cx.update(|cx| {
        cx.global::<OpenedThread>()
            .0
            .as_ref()
            .is_some_and(|thread| thread.id == failure_thread.id && thread.model == "gpt-5.6-terra")
    }));
    assert_eq!(provider.requests().len(), 2);
    assert!(!missing_config.exists());

    // An active controller is another fail-closed boundary: a selection
    // emitted during an existing run is rejected without touching SQLite or
    // the provider recorder.
    root.update(cx, |root, _| {
        root.agent_controller.active = Some(ActiveAgentRun {
            generation: 7,
            thread_id: failure_thread.id.clone(),
            stream: failure_stream.clone(),
            cancel: tokio_util::sync::CancellationToken::new(),
            pending_user_content: None,
            pending_approved_instruction: None,
            started: std::time::Instant::now(),
            terminal_message_id: None,
        });
    });
    failure_stream.update(cx, |stream, cx| {
        stream.request_model_selection("gpt-5.6-luna", cx);
    });
    pump_test_app(cx, |cx| {
        failure_stream.read_with(cx, |stream, _| !stream.has_pending_model_selection())
            && root.read_with(cx, |root, _| !root.trusted_actions.is_busy())
    });
    let active_guarded_thread = vega_conversation::threads::open_thread(&store, &failure_thread.id)
        .expect("active guarded model thread");
    assert_eq!(active_guarded_thread.model, "gpt-5.6-terra");
    assert_eq!(provider.requests().len(), 2);
    root.update(cx, |root, _| {
        root.agent_controller.active = None;
    });
}

#[test]
fn model_selection_config_read_is_strictly_read_only() {
    let root = tempfile::tempdir().expect("read-only config root");
    let path = root.path().join("missing.toml");
    assert!(vega_store::config::read_from(&path).is_err());
    assert!(!path.exists());
}

#[gpui::test]
async fn model_selection_generic_busy_rejects_without_releasing_other_owner(
    cx: &mut gpui::TestAppContext,
) {
    let data_root = tempfile::tempdir().expect("generic busy data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("generic busy store");
    store.migrate().expect("generic busy migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root.path().to_str().expect("UTF-8 generic busy path"),
        "generic-busy-e2e",
        None,
    )
    .expect("generic busy project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("generic busy thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("generic busy global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
    });

    // A real competing trusted action is already holding the coordinator.
    let competing = root.update(cx, |root, _| {
        root.trusted_actions
            .acquire(TrustedActionKind::Commit, 9, 1)
            .expect("competing trusted owner")
    });

    // Build the pending UI intent before the generic busy arrives, then route
    // it through the real app handler while the other owner is still held.
    stream.update(cx, |stream, cx| {
        stream.request_model_selection("gpt-5.6-luna", cx);
        stream.set_trusted_action_busy(true, cx);
    });
    let request = ThreadModelSelectionRequested {
        thread_id: thread.id.clone(),
        model: "gpt-5.6-luna".into(),
        request_id: 0,
    };
    root.update(cx, |root, cx| {
        root.apply_thread_model_selection(stream.clone(), &request, cx);
    });
    assert!(!stream.read_with(cx, |stream, _| stream.has_pending_model_selection()));
    assert_eq!(
        root.read_with(cx, |root, _| root.trusted_actions.active_token()),
        Some(competing)
    );
    assert!(stream.read_with(cx, |stream, cx| stream.model_selection_blocked(cx)));

    // The UI boundary also refuses a fresh selection while the generic busy
    // state remains set, so it cannot create another orphaned pending owner.
    stream.update(cx, |stream, cx| {
        stream.request_model_selection("gpt-5.6-luna", cx);
    });
    assert!(!stream.read_with(cx, |stream, _| stream.has_pending_model_selection()));

    stream.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
    root.update(cx, |root, _| {
        assert!(root.trusted_actions.release(competing))
    });
}

#[gpui::test]
async fn model_selection_settings_deferred_keeps_sidebar_rename(cx: &mut gpui::TestAppContext) {
    let config_root = tempfile::tempdir().expect("settings deferred config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);

    let data_root = tempfile::tempdir().expect("settings deferred data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("settings deferred store");
    store.migrate().expect("settings deferred migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 settings deferred path"),
        "settings-deferred-e2e",
        None,
    )
    .expect("settings deferred project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("settings deferred thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("settings deferred global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.start_model_catalog_load(cx);
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.configured_models.is_some()
                && matches!(
                    root.pricing_controller.state,
                    PricingControllerState::Ready { .. }
                )
        })
    });

    let gate = Arc::new(std::sync::Barrier::new(2));
    root.update(cx, |root, _| {
        root.model_selection_worker_gate = Some(gate.clone())
    });
    stream.update(cx, |stream, cx| {
        stream.request_model_selection("gpt-5.6-luna", cx);
    });
    let request = ThreadModelSelectionRequested {
        thread_id: thread.id.clone(),
        model: "gpt-5.6-luna".into(),
        request_id: 0,
    };
    root.update(cx, |root, cx| {
        root.apply_thread_model_selection(stream.clone(), &request, cx);
    });
    assert!(stream.read_with(cx, |stream, _| stream.has_pending_model_selection()));

    // The model worker is still waiting while Settings is visible.
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    gate.wait();
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            !root.trusted_actions.is_busy()
                && root
                    .deferred_model_refresh
                    .as_ref()
                    .is_some_and(|refresh| refresh.model == "gpt-5.6-luna")
        })
    });

    // Only after the deferred model refresh is ready, perform the same
    // durable rename + OpenedThread projection used by the Settings sidebar.
    // The refresh must merge its model field into this newer Thread rather
    // than replace the title with the worker's stale snapshot.
    let renamed = vega_conversation::threads::rename_thread(&store, &thread.id, "设置期间新标题")
        .expect("settings deferred rename");
    cx.update(|cx| cx.set_global(OpenedThread(Some(renamed.clone()))));

    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    let current = cx
        .update(|cx| cx.global::<OpenedThread>().0.clone())
        .expect("settings deferred current thread");
    assert_eq!(current.title, "设置期间新标题");
    assert_eq!(current.model, "gpt-5.6-luna");
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.displayed_model().to_string()),
        "gpt-5.6-luna"
    );
    let durable = vega_conversation::threads::open_thread(&store, &thread.id)
        .expect("settings deferred durable thread");
    assert_eq!(durable.title, "设置期间新标题");
    assert_eq!(durable.model, "gpt-5.6-luna");
}
