use super::model_selection::model_selection_config;
use super::*;

use vega_store::config::AppConfig;
use vega_ui::settings::{ReasoningSettingsProjection, ReasoningTemplate};

#[gpui_kit::test]
async fn reasoning_authority_reconcile_error_blocks_controller_before_provider(
    cx: &mut gpui_kit::TestAppContext,
) {
    let config_root = tempfile::tempdir().expect("reasoning controller config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);

    let data_root = tempfile::tempdir().expect("reasoning controller data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("reasoning controller store");
    store.migrate().expect("reasoning controller migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 reasoning controller path"),
        "reasoning-controller-e2e",
        None,
    )
    .expect("reasoning controller project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("reasoning controller thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("reasoning controller global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let provider = Arc::new(vega_runtime::MockProvider::new(Vec::new()));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path);
        root.agent_provider_override = Some(provider.clone());
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.configured_models = Some(vec![thread.model.clone()]);
        root.configured_reasoning = Some(vec![ReasoningProfileProjection::unknown(
            "owned",
            thread.model.clone(),
        )]);
        // This is the post-rename-readback-failed state: no disk authority is
        // available, so the stale projection cannot be submitted as default.
        root.configured_reasoning_error = Some(vega_ui::settings::ReasoningSettingsErrorCode::Io);
        root.configured_reasoning_authority = None;
        root.apply_reasoning_profile_to_stream(&stream, &thread.model, cx);
    });

    assert!(stream.read_with(cx, |stream, _| stream.reasoning_unavailable()));
    root.update(cx, |root, cx| {
        root.start_agent_run(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("must remain local".into()),
            cx,
        );
    });
    assert!(provider.requests().is_empty());
    assert!(root.read_with(cx, |root, _| root.agent_controller.active.is_none()));

    // An authority may be readable while its typed projection is invalid
    // after a future grammar/conversion drift. The error must not turn the
    // unknown row into a legal provider-default submit.
    root.update(cx, |root, cx| {
        root.configured_reasoning_authority = Some((
            vega_store::reasoning::ReasoningConfig::default(),
            vega_store::reasoning::ReasoningFileSnapshot::absent(),
        ));
        root.configured_reasoning_error =
            Some(vega_ui::settings::ReasoningSettingsErrorCode::Invalid);
        root.apply_reasoning_profile_to_stream(&stream, &thread.model, cx);
    });
    assert!(stream.read_with(cx, |stream, _| stream.reasoning_unavailable()));
    root.update(cx, |root, cx| {
        root.start_agent_run(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("invalid profile must remain local".into()),
            cx,
        );
    });
    assert!(provider.requests().is_empty());
}

#[gpui_kit::test]
async fn settings_unknown_reasoning_template_saves_reads_back_and_reaches_mock_run(
    cx: &mut gpui_kit::TestAppContext,
) {
    // Start with a provider/model config but no reasoning file. The test uses
    // an owned Settings entity through the same public view constructor; the
    // rendered focus/keyboard path is covered by vega_ui tests, while this
    // app test drives the controller and provider boundary. No hand-written
    // capability profile is used.
    let config_root = tempfile::tempdir().expect("settings reasoning config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);
    let reasoning_path = config_root.path().join("reasoning.toml");
    assert!(!reasoning_path.exists());

    let data_root = tempfile::tempdir().expect("settings reasoning data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("settings reasoning store");
    store.migrate().expect("settings reasoning migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 settings reasoning path"),
        "settings-reasoning-e2e",
        None,
    )
    .expect("settings reasoning project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("settings reasoning thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("settings reasoning global store"),
            thread.clone(),
            cx,
        )
    });
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("settings path ok".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config_path.clone());
        root.agent_provider_override = Some(provider.clone());
        root.stream_view = Some((thread.id.clone(), stream.clone()));
    });
    cx.update(|cx| {
        cx.set_global(SettingsOpen(true));
    });
    // Install the same typed Settings subscriptions that the production
    // VegaWindow renderer installs. Constructing from an owned config keeps
    // this controller acceptance from reading the host user's AppConfig;
    // separate UI tests exercise the rendered focus path.
    let settings =
        cx.new(|cx| SettingsView::from_config(vega_store::config::AppConfig::default(), None, cx));
    root.update(cx, |root, cx| {
        cx.subscribe(
            &settings,
            |root, view, request: &ReasoningProfileSaveRequested, cx| {
                root.request_reasoning_profile_save(view.clone(), request, cx);
            },
        )
        .detach();
        root.settings_view = Some(settings.clone());
        root.start_model_catalog_load(cx);
    });

    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.configured_models.is_some()
                && root.configured_reasoning_authority.is_some()
                && root.settings_view.is_some()
        })
    });
    let settings = root
        .read_with(cx, |root, _| root.settings_view.clone())
        .expect("owned Settings view");
    assert!(settings.read_with(cx, |settings, _| matches!(
        settings.reasoning_projection(),
        vega_ui::settings::ReasoningSettingsProjection::Ready {
            profiles,
            error: None,
            ..
        } if profiles.iter().any(|profile| profile.provider == "owned"
            && profile.model == "gpt-5.6-terra"
            && profile.protocol == vega_conversation::types::ReasoningProtocol::Unknown)
    )));

    // This is the Settings-page action behind the keyboard/mouse template
    // control. It emits the normal app-owned save request and waits for the
    // durable worker acknowledgement/readback.
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, vega_ui::settings::ReasoningTemplate::OpenAi, cx);
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root.configured_reasoning_authority.is_some()
                && root.configured_reasoning_error.is_none()
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                vega_ui::settings::ReasoningSettingsProjection::Ready {
                    profiles,
                    error: None,
                    ..
                } if profiles.iter().any(|profile| {
                    profile.provider == "owned"
                        && profile.model == "gpt-5.6-terra"
                        && profile.protocol
                            == vega_conversation::types::ReasoningProtocol::OpenAiChatCompletions
                })
            )
        })
    });
    let first_generation = root.read_with(cx, |root, _| root.model_catalog_generation);
    // A second edit must use the generation carried by the first acknowledged
    // projection. This catches publishing Ready with the pre-ack generation,
    // which would make an immediate follow-up save look stale.
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root.configured_reasoning_authority.is_some()
                && root.configured_reasoning_error.is_none()
                && root.model_catalog_generation > first_generation
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                vega_ui::settings::ReasoningSettingsProjection::Ready {
                    profiles,
                    error: None,
                    ..
                } if profiles.iter().any(|profile| {
                    profile.provider == "owned"
                        && profile.model == "gpt-5.6-terra"
                        && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                })
            )
        })
    });
    let saved = vega_store::reasoning::read_from(&reasoning_path)
        .expect("read settings-selected reasoning authority");
    assert_eq!(saved.profiles.len(), 1);
    assert_eq!(saved.profiles[0].provider, "owned");
    assert_eq!(saved.profiles[0].model, "gpt-5.6-terra");
    assert_eq!(saved.profiles[0].protocol, "openai_chat_completions");
    assert_eq!(saved.profiles[0].support, "optional");

    // Closing Settings lets the live window render the conversation stream;
    // the already-read-back projection is then frozen for the real app
    // submit. The provider boundary remains MockProvider, so no keychain or
    // network is involved.
    cx.update(|cx| {
        cx.set_global(SettingsOpen(false));
        cx.refresh_windows();
    });
    let frozen = stream
        .read_with(cx, |stream, _| stream.frozen_reasoning_for_submit())
        .expect("saved profile freezes for submit")
        .expect("explicit OpenAI profile");
    assert_eq!(frozen.model, "gpt-5.6-terra");
    root.update(cx, |root, cx| {
        root.start_agent_run_with_reasoning(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("use saved thinking settings".into()),
            Some(frozen),
            cx,
        );
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.agent_controller.active.is_none())
            && provider.requests().len() == 1
    });
    let request = provider
        .requests()
        .into_iter()
        .next()
        .expect("mock request");
    assert_eq!(request.model, "gpt-5.6-terra");
    assert_eq!(
        request.reasoning.map(|reasoning| reasoning.protocol),
        Some(vega_runtime::ReasoningProtocol::OpenAiChatCompletions)
    );
}

#[gpui_kit::test]
async fn frozen_reasoning_owner_change_same_model_fails_before_provider(
    cx: &mut gpui_kit::TestAppContext,
) {
    let config_root = tempfile::tempdir().expect("owner change config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);

    let data_root = tempfile::tempdir().expect("owner change data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("owner change store");
    store.migrate().expect("owner change migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 owner change project"),
        "owner-change-e2e",
        None,
    )
    .expect("owner change project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("owner change thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("owner change global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("must not run"),
    ]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.agent_provider_override = Some(provider.clone());
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.configured_models = Some(vec![thread.model.clone()]);
        root.configured_reasoning = Some(vec![ReasoningProfileProjection {
            provider: "owned".into(),
            model: thread.model.clone(),
            protocol: ReasoningProtocol::OpenAiChatCompletions,
            support: ReasoningSupport::Optional,
            efforts: vec!["low".into(), "high".into(), "max".into()],
            supports_disabled: false,
            disabled_wire: None,
            preserve_reasoning_content: false,
            preference: ReasoningChoice::Effort("high".into()),
        }]);
        root.configured_reasoning_authority = Some((
            vega_store::reasoning::ReasoningConfig::default(),
            vega_store::reasoning::ReasoningFileSnapshot::absent(),
        ));
        root.apply_reasoning_profile_to_stream(&stream, &thread.model, cx);
    });

    let frozen = stream
        .read_with(cx, |stream, _| stream.frozen_reasoning_for_submit())
        .expect("frozen owner profile")
        .expect("explicit owner profile");
    assert_eq!(frozen.provider, "owned");

    // Keep the model id stable while changing its configured provider. The
    // worker reads this owned path and must reject the stale A-owner snapshot
    // before invoking the test provider or entering the runtime.
    let mut changed = fs::read_to_string(&config_path).expect("read owner config");
    changed = changed.replace("name = \"owned\"", "name = \"other\"");
    changed = changed.replace("key_ref = \"owned\"", "key_ref = \"other\"");
    fs::write(&config_path, changed).expect("change configured owner");

    root.update(cx, |root, cx| {
        root.start_agent_run_with_reasoning(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("stale owner must remain local".into()),
            Some(frozen),
            cx,
        );
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.agent_controller.active.is_none())
    });
    assert!(
        provider.requests().is_empty(),
        "a changed provider owner must fail before any mock provider request"
    );
}

#[gpui_kit::test]
async fn provider_catalog_refresh_gates_settings_until_new_generation_is_ready(
    cx: &mut gpui_kit::TestAppContext,
) {
    let config_root = tempfile::tempdir().expect("catalog refresh config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);
    let data_root = tempfile::tempdir().expect("catalog refresh data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("catalog refresh store");
    store.migrate().expect("catalog refresh migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 catalog refresh project path"),
        "catalog-refresh-e2e",
        None,
    )
    .expect("catalog refresh project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("catalog refresh thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("catalog refresh global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let settings = cx.new(|cx| SettingsView::from_config(AppConfig::default(), None, cx));
    let root = cx.new(VegaWindow::new);
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.stream_view = Some((thread.id.clone(), stream));
        root.settings_view = Some(settings.clone());
        cx.subscribe(
            &settings,
            |root, view, request: &ReasoningProfileSaveRequested, cx| {
                root.request_reasoning_profile_save(view.clone(), request, cx);
            },
        )
        .detach();
        root.start_model_catalog_load(cx);
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.configured_models.is_some()
                && root.configured_reasoning_authority.is_some()
                && !root.model_catalog_loading
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready { error: None, .. }
            )
        })
    });
    let old_generation = root.read_with(cx, |root, _| root.model_catalog_generation);
    let catalog_gate = Arc::new(std::sync::Barrier::new(2));
    root.update(cx, |root, _| {
        root.model_catalog_worker_gate = Some(catalog_gate.clone());
    });

    // Provider SettingsSaved invalidates the catalog and starts a new worker.
    // The live Settings view must leave Ready(old_generation) before the gate
    // is released, so an attempted edit cannot become an unacknowledged stale
    // Saving request.
    root.update(cx, |root, cx| root.on_settings_saved(cx));
    assert!(root.read_with(cx, |root, _| {
        root.model_catalog_loading && root.model_catalog_generation > old_generation
    }));
    assert!(settings.read_with(cx, |settings, _| matches!(
        settings.reasoning_projection(),
        ReasoningSettingsProjection::Loading
    )));
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_none()));
    assert!(settings.read_with(cx, |settings, _| matches!(
        settings.reasoning_projection(),
        ReasoningSettingsProjection::Loading
    )));

    catalog_gate.wait();
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.configured_models.is_some()
                && root.configured_reasoning_authority.is_some()
                && !root.model_catalog_loading
                && root.model_catalog_generation > old_generation
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready { error: None, .. }
            )
        })
    });

    // After the new authority is Ready the same real Settings action emits a
    // request carrying the new generation and receives its durable ack.
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_some()));
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root.configured_reasoning_authority.is_some()
                && root.configured_reasoning_error.is_none()
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready {
                    profiles,
                    error: None,
                    ..
                } if profiles.iter().any(|profile| {
                    profile.provider == "owned"
                        && profile.model == "gpt-5.6-terra"
                        && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                })
            )
        })
    });
}

async fn exercise_catalog_reasoning_save_order(
    cx: &mut gpui_kit::TestAppContext,
    reasoning_first: bool,
) {
    let config_root = tempfile::tempdir().expect("catalog order config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);
    let data_root = tempfile::tempdir().expect("catalog order data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("catalog order store");
    store.migrate().expect("catalog order migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root.path().to_str().expect("UTF-8 catalog order path"),
        "catalog-order-e2e",
        None,
    )
    .expect("catalog order project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("catalog order thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("catalog order global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let settings = cx.new(|cx| SettingsView::from_config(AppConfig::default(), None, cx));
    let catalog_gate = Arc::new(std::sync::Barrier::new(2));
    let reasoning_gate = Arc::new(std::sync::Barrier::new(2));
    let root = cx.new(VegaWindow::new);
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.configured_reasoning = Some(vec![ReasoningProfileProjection::unknown(
            "owned",
            thread.model.clone(),
        )]);
        root.configured_reasoning_authority = Some((
            vega_store::reasoning::ReasoningConfig::default(),
            vega_store::reasoning::ReasoningFileSnapshot::absent(),
        ));
        root.model_catalog_worker_gate = Some(catalog_gate.clone());
        root.reasoning_save_worker_gate = Some(reasoning_gate.clone());
        root.start_model_catalog_load(cx);
        cx.subscribe(
            &settings,
            |root, view, request: &ReasoningProfileSaveRequested, cx| {
                root.request_reasoning_profile_save(view.clone(), request, cx);
            },
        )
        .detach();
        root.settings_view = Some(settings.clone());
        settings.update(cx, |settings, cx| {
            settings.apply_reasoning_projection(
                ReasoningSettingsProjection::Ready {
                    generation: root.model_catalog_generation,
                    profiles: root.configured_reasoning.clone().unwrap_or_default(),
                    error: None,
                },
                cx,
            )
        });
    });

    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_some()));

    // A Provider SettingsSaved signal arrives while reasoning owns the
    // authority. It records intent only; it cannot launch a worker that would
    // overwrite Saving or leave model_catalog_loading stuck.
    root.update(cx, |root, cx| root.on_settings_saved(cx));
    assert!(root.read_with(cx, |root, _| {
        root.model_catalog_refresh_pending && root.model_catalog_loading
    }));

    // Make the provider catalog change visible between the old worker and the
    // coalesced reload. Both completion orders must end with this new model.
    let mut changed = fs::read_to_string(&config_path).expect("read catalog order config");
    changed.push_str(
        "\n[[providers]]\nname = \"fresh\"\nbase_url = \"https://fresh.invalid/v1\"\nmodels = [\"fresh-model\"]\nkey_ref = \"fresh\"\n",
    );
    fs::write(&config_path, changed).expect("write catalog order provider");

    if reasoning_first {
        reasoning_gate.wait();
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                root.reasoning_save_pending.is_none()
                    && root.configured_models.is_some()
                    && root.configured_reasoning_authority.is_some()
                    && !root.model_catalog_loading
                    && !root.model_catalog_refresh_pending
                    && root.configured_reasoning.as_ref().is_some_and(|profiles| {
                        profiles.iter().any(|profile| {
                            profile.provider == "owned"
                                && profile.model == "gpt-5.6-terra"
                                && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                        })
                    })
            })
        });
        // The original catalog worker is now stale. Its late completion must
        // not regress the freshly reconciled provider/reasoning authority.
        catalog_gate.wait();
    } else {
        catalog_gate.wait();
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                root.reasoning_save_pending.is_some()
                    && !root.model_catalog_loading
                    && root.model_catalog_refresh_pending
            }) && settings.read_with(cx, |settings, _| {
                matches!(
                    settings.reasoning_projection(),
                    ReasoningSettingsProjection::Saving { .. }
                )
            })
        });
        reasoning_gate.wait();
    }

    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root
                    .configured_models
                    .as_ref()
                    .is_some_and(|models| models.iter().any(|model| model == "fresh-model"))
                && root.configured_reasoning_authority.is_some()
                && !root.model_catalog_loading
                && !root.model_catalog_refresh_pending
                && root.configured_reasoning.as_ref().is_some_and(|profiles| {
                    profiles.iter().any(|profile| {
                        profile.provider == "owned"
                            && profile.model == "gpt-5.6-terra"
                            && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                    })
                })
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready { profiles, error: None, .. }
                    if profiles.iter().any(|profile| {
                        profile.provider == "owned"
                            && profile.model == "gpt-5.6-terra"
                            && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                    })
            )
        })
    });
}

#[gpui_kit::test]
async fn reasoning_and_provider_catalog_completion_orders_are_coordinated(
    cx: &mut gpui_kit::TestAppContext,
) {
    exercise_catalog_reasoning_save_order(cx, true).await;
    exercise_catalog_reasoning_save_order(cx, false).await;
}

#[gpui_kit::test]
async fn failed_reasoning_save_keeps_error_and_draft_through_catalog_refresh(
    cx: &mut gpui_kit::TestAppContext,
) {
    let config_root = tempfile::tempdir().expect("failed reasoning config root");
    let config_path = config_root.path().join("config.toml");
    model_selection_config(&config_path);
    let reasoning_path = config_root.path().join("reasoning.toml");
    let data_root = tempfile::tempdir().expect("failed reasoning data root");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("failed reasoning store");
    store.migrate().expect("failed reasoning migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root
            .path()
            .to_str()
            .expect("UTF-8 failed reasoning project path"),
        "failed-reasoning-e2e",
        None,
    )
    .expect("failed reasoning project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("failed reasoning thread");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("failed reasoning global store"),
            thread.clone(),
            cx,
        )
    });

    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let settings = cx.new(|cx| SettingsView::from_config(AppConfig::default(), None, cx));
    let catalog_gate = Arc::new(std::sync::Barrier::new(2));
    let fresh_catalog_gate = Arc::new(std::sync::Barrier::new(2));
    let reasoning_gate = Arc::new(std::sync::Barrier::new(2));
    let root = cx.new(VegaWindow::new);
    cx.update(|cx| cx.set_global(SettingsOpen(true)));
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.stream_view = Some((thread.id.clone(), stream));
        root.configured_reasoning = Some(vec![ReasoningProfileProjection::unknown(
            "owned",
            thread.model.clone(),
        )]);
        root.configured_reasoning_authority = Some((
            vega_store::reasoning::ReasoningConfig::default(),
            vega_store::reasoning::ReasoningFileSnapshot::absent(),
        ));
        root.model_catalog_worker_gate = Some(catalog_gate.clone());
        root.reasoning_save_worker_gate = Some(reasoning_gate.clone());
        root.start_model_catalog_load(cx);
        cx.subscribe(
            &settings,
            |root, view, request: &ReasoningProfileSaveRequested, cx| {
                root.request_reasoning_profile_save(view.clone(), request, cx);
            },
        )
        .detach();
        root.settings_view = Some(settings.clone());
        settings.update(cx, |settings, cx| {
            settings.apply_reasoning_projection(
                ReasoningSettingsProjection::Ready {
                    generation: root.model_catalog_generation,
                    profiles: root.configured_reasoning.clone().unwrap_or_default(),
                    error: None,
                },
                cx,
            )
        });
    });

    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_some()));
    root.update(cx, |root, cx| root.on_settings_saved(cx));
    assert!(root.read_with(cx, |root, _| {
        root.model_catalog_refresh_pending && root.model_catalog_loading
    }));

    // Change the independent authority while the save worker is gated. The
    // compare-before-rename returns a typed conflict and readback supplies the
    // external authority; this is the post-rename/uncertain path's visible
    // counterpart without any provider, Keychain, or network request.
    let external = vega_store::reasoning::ReasoningConfig {
        version: vega_store::reasoning::REASONING_CONFIG_VERSION,
        profiles: vec![vega_store::reasoning::ReasoningProfile {
            provider: "owned".into(),
            model: "gpt-5.6-terra".into(),
            protocol: "openai_chat_completions".into(),
            support: "optional".into(),
            efforts: vec!["low".into(), "high".into(), "max".into()],
            supports_disabled: true,
            disabled_wire: Some("reasoning_effort_none".into()),
            preserve_reasoning_content: false,
            preference: "provider_default".into(),
        }],
    };
    fs::write(
        &reasoning_path,
        vega_store::reasoning::encode(&external).expect("encode external reasoning authority"),
    )
    .expect("write external reasoning authority");
    root.update(cx, |root, _| {
        // The first catalog worker remains gated as the stale result. The
        // acknowledgement starts a second, fresh worker under a separate gate
        // so the real Settings view can be exercised while that generation is
        // still loading.
        root.model_catalog_worker_gate = Some(fresh_catalog_gate.clone());
    });
    reasoning_gate.wait();

    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none() && root.model_catalog_loading
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Loading
            )
        })
    });

    // The actual Settings action is now attempted while the fresh catalog
    // worker is gated. Loading must reject the action without creating a stale
    // Saving projection, while the UI-level draft remains retained.
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_none()));
    assert!(settings.read_with(cx, |settings, _| matches!(
        settings.reasoning_projection(),
        ReasoningSettingsProjection::Loading
    )));

    // The save acknowledgement starts the deferred catalog read. Its fresh
    // result must retain the failure hold and the Settings draft instead of
    // replacing them with a clean Ready/Loading projection.
    catalog_gate.wait();
    fresh_catalog_gate.wait();
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root.configured_reasoning_authority.is_some()
                && root.configured_reasoning_error
                    == Some(vega_ui::settings::ReasoningSettingsErrorCode::Conflict)
                && !root.model_catalog_loading
                && !root.model_catalog_refresh_pending
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready {
                    profiles,
                    error: Some(vega_ui::settings::ReasoningSettingsErrorCode::Conflict),
                    ..
                } if profiles.iter().any(|profile| {
                    profile.provider == "owned"
                        && profile.model == "gpt-5.6-terra"
                        && profile.protocol == ReasoningProtocol::OpenAiChatCompletions
                })
            )
        })
    });

    // Once the exact external authority is reloaded, the retained owner is
    // retryable through the same Settings action and a successful ack clears
    // the held error.
    settings.update(cx, |settings, cx| {
        settings.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    assert!(root.read_with(cx, |root, _| root.reasoning_save_pending.is_some()));
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.reasoning_save_pending.is_none()
                && root.configured_reasoning_error.is_none()
                && root.configured_reasoning_authority.is_some()
        }) && settings.read_with(cx, |settings, _| {
            matches!(
                settings.reasoning_projection(),
                ReasoningSettingsProjection::Ready { error: None, .. }
            )
        })
    });
}
