use super::*;
use ::core::prelude::v1::test;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, TestAppContext, WindowHandle};

struct Harness;
impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

trait RealWorkerWait {
    fn wait_for_real(
        &mut self,
        window: AnyWindowHandle,
        timeout: std::time::Duration,
        predicate: impl FnMut(&mut Window, &mut App) -> bool,
    ) -> impl std::future::Future<Output = ()>;
}

impl RealWorkerWait for TestAppContext {
    async fn wait_for_real(
        &mut self,
        window: AnyWindowHandle,
        timeout: std::time::Duration,
        mut predicate: impl FnMut(&mut Window, &mut App) -> bool,
    ) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self
                .update_window(window, |_, window, cx| {
                    window.render_frame(cx);
                    predicate(window, cx)
                })
                .unwrap()
            {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "real controller worker timeout"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
            self.executor()
                .timer(std::time::Duration::from_millis(10))
                .await;
        }
    }
}

fn setup(
    cx: &mut TestAppContext,
    root: &std::path::Path,
) -> (
    Entity<VegaWindow>,
    Entity<SettingsView>,
    Entity<ConversationStream>,
    WindowHandle<Harness>,
    std::path::PathBuf,
) {
    let path = root.join("config.toml");
    let config = vega_store::config::AppConfig {
        providers: vec![vega_store::config::ProviderConfig {
            name: "fixture".into(),
            base_url: "https://unknown.invalid/v1".into(),
            models: vec!["future".into(), "second".into()],
            key_ref: "key".into(),
            api: vega_store::config::ProviderApi::ChatCompletions,
            enabled: true,
        }],
        ..Default::default()
    };
    config.save_to(&path).unwrap();
    let store = vega_store::Store::open(root.join("store.db")).unwrap();
    store.migrate().unwrap();
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread",
            project_id: "",
            title: "Fixture",
            mode: "execute",
            permission_mode: "readonly",
            model: "future",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    let thread = vega_conversation::threads::get_thread(&store, "thread").unwrap();
    let (owner, settings, stream) = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(OpenedThread(Some(thread.clone())));
        cx.set_global(SelectedProject(None));
        cx.set_global(SettingsOpen(true));
        cx.set_global(NewTaskDefaults::default());
        cx.set_global(vega_ui::sidebar::ProjectsCollapsed(false));
        cx.set_global(vega_ui::sidebar::SessionsCollapsed(false));
        cx.set_global(vega_ui::sidebar::SidebarCollapsed(false));
        cx.set_global(vega_ui::sidebar::SidebarWidth(240.));
        let settings = cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread, cx));
        let owner = cx.new(|cx| VegaWindow::new_for_test(path.clone(), cx));
        owner.update(cx, |owner, _| {
            owner.settings_view = Some(settings.clone());
            owner.stream_view = Some(("thread".into(), stream.clone()));
            owner.configured_models = Some(vec!["future".into()]);
            owner.loaded_replay_config = Some(config);
        });
        (owner, settings, stream)
    });
    let window = cx.add_window(|_, _| Harness);
    (owner, settings, stream, window, path)
}

async fn load(
    cx: &mut TestAppContext,
    owner: &Entity<VegaWindow>,
    settings: &Entity<SettingsView>,
    window: WindowHandle<Harness>,
) -> ModelCapabilitySettingsProjection {
    cx.update(|cx| {
        owner.update(cx, |owner, cx| {
            owner.request_model_capability_reload(settings.clone(), cx)
        })
    });
    cx.wait_for_real(window.into(), std::time::Duration::from_secs(5), |_, cx| {
        matches!(
            owner.read(cx).model_capability_settings_projection(),
            ModelCapabilitySettingsProjection::Ready { .. }
        )
    })
    .await;
    cx.update(|cx| owner.read(cx).model_capability_settings_projection())
}

fn base(projection: ModelCapabilitySettingsProjection) -> (u64, ModelCapabilityProjection) {
    match projection {
        ModelCapabilitySettingsProjection::Ready {
            generation,
            models,
            error: None,
        } => (generation, models[0].clone()),
        other => panic!("unexpected authority {other:?}"),
    }
}

fn begin(
    cx: &mut TestAppContext,
    owner: &Entity<VegaWindow>,
    settings: &Entity<SettingsView>,
    generation: u64,
    operation_id: u64,
    base: ModelCapabilityProjection,
    patch: ModelCapabilityPatch,
) {
    let event = ModelCapabilitySaveRequested {
        generation,
        operation_id,
        base: base.clone(),
        patch,
    };
    cx.update(|cx| {
        settings.update(cx, |view, cx| {
            view.apply_model_capability_projection(
                ModelCapabilitySettingsProjection::Saving {
                    generation,
                    operation_id,
                    model: base,
                },
                cx,
            )
        });
        owner.update(cx, |owner, cx| {
            owner.request_model_capability_save(settings.clone(), &event, cx)
        });
    });
}

#[gpui_kit::test]
async fn window_capability_save_and_reset_use_real_service_without_editing_preferences(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (owner, settings, _, window, path) = setup(cx, dir.path());
    let preference = path.with_file_name("reasoning-preferences.toml");
    let pref =
        b"version=1\n[[preferences]]\nprovider='fixture'\nmodel='future'\npreference='high'\n";
    std::fs::write(&preference, pref).unwrap();
    let (generation, model) = base(load(cx, &owner, &settings, window).await);
    let original = model.resolved.fingerprint.clone();
    let patch = ModelCapabilityPatch::Upsert(vega_runtime::model_compat::ModelCapabilityOverride {
        identity: model.resolved.identity.clone(),
        allowed_efforts: Some(vec!["low".into()]),
        image_input: None,
    });
    begin(cx, &owner, &settings, generation, 1, model, patch);
    cx.wait_for_real(window.into(), std::time::Duration::from_secs(5), |_, cx| {
        matches!(
            owner.read(cx).model_capability_settings_projection(),
            ModelCapabilitySettingsProjection::Ready { .. }
        )
    })
    .await;
    let (generation, saved) =
        base(cx.update(|cx| owner.read(cx).model_capability_settings_projection()));
    assert_eq!(saved.resolved.capabilities.allowed_efforts, vec!["low"]);
    assert_ne!(saved.resolved.fingerprint, original);
    assert_eq!(saved.user_override.as_ref().unwrap().image_input, None);
    assert_eq!(std::fs::read(&preference).unwrap(), pref);
    let identity = saved.resolved.identity.clone();
    begin(
        cx,
        &owner,
        &settings,
        generation,
        2,
        saved,
        ModelCapabilityPatch::Reset { identity },
    );
    cx.wait_for_real(window.into(), std::time::Duration::from_secs(5), |_, cx| {
        matches!(
            owner.read(cx).model_capability_settings_projection(),
            ModelCapabilitySettingsProjection::Ready { .. }
        )
    })
    .await;
    let (_, reset) = base(cx.update(|cx| owner.read(cx).model_capability_settings_projection()));
    assert_eq!(reset.resolved.capabilities.allowed_efforts.len(), 6);
    assert_eq!(reset.resolved.fingerprint, original);
    assert_eq!(std::fs::read(&preference).unwrap(), pref);
}

#[gpui_kit::test]
async fn stale_generation_or_foreign_settings_entity_cannot_write_model_capabilities(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (owner, settings, _, window, path) = setup(cx, dir.path());
    let (generation, model) = base(load(cx, &owner, &settings, window).await);
    let patch = ModelCapabilityPatch::Upsert(vega_runtime::model_compat::ModelCapabilityOverride {
        identity: model.resolved.identity.clone(),
        allowed_efforts: Some(Vec::new()),
        image_input: None,
    });
    begin(
        cx,
        &owner,
        &settings,
        generation + 1,
        1,
        model.clone(),
        patch.clone(),
    );
    cx.run_until_parked();
    assert!(!path.with_file_name("model-capabilities.toml").exists());
    let foreign = cx.update(|cx| cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx)));
    begin(cx, &owner, &foreign, generation, 2, model, patch);
    cx.run_until_parked();
    assert!(!path.with_file_name("model-capabilities.toml").exists());
}

#[gpui_kit::test]
async fn capability_save_conflict_preserves_external_record_and_does_not_publish_draft_as_authority(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (owner, settings, _, window, path) = setup(cx, dir.path());
    let (generation, model) = base(load(cx, &owner, &settings, window).await);
    let cap = path.with_file_name("model-capabilities.toml");
    let external =
        "version=1\n[[models]]\nprovider='fixture'\nmodel='future'\nallowed_efforts=['low']\n";
    std::fs::write(&cap, external).unwrap();
    let patch = ModelCapabilityPatch::Upsert(vega_runtime::model_compat::ModelCapabilityOverride {
        identity: model.resolved.identity.clone(),
        allowed_efforts: Some(vec!["high".into()]),
        image_input: None,
    });
    begin(cx, &owner, &settings, generation, 1, model, patch);
    cx.wait_for_real(window.into(), std::time::Duration::from_secs(5), |_, cx| {
        matches!(
            owner.read(cx).model_capability_settings_projection(),
            ModelCapabilitySettingsProjection::Ready {
                error: Some(ModelCapabilitySettingsErrorCode::Conflict),
                ..
            }
        )
    })
    .await;
    assert_eq!(std::fs::read_to_string(cap).unwrap(), external);
    assert!(!path.with_file_name("reasoning-preferences.toml").exists());
}

#[gpui_kit::test]
async fn invalid_saved_effort_recovers_through_actual_composer_controller_default_save(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (owner, settings, stream, window, path) = setup(cx, dir.path());
    let preference = path.with_file_name("reasoning-preferences.toml");
    std::fs::write(
        &preference,
        "version=1\n[[preferences]]\nprovider='fixture'\nmodel='future'\npreference='high'\n",
    )
    .unwrap();
    let cap = path.with_file_name("model-capabilities.toml");
    std::fs::write(
        &cap,
        "version=1\n[[models]]\nprovider='fixture'\nmodel='future'\nallowed_efforts=[]\n",
    )
    .unwrap();
    let capability_bytes = std::fs::read(&cap).unwrap();
    let _ = load(cx, &owner, &settings, window).await;
    cx.update(|cx| cx.set_global(SettingsOpen(false)));
    cx.run_until_parked();
    cx.update(|cx| {
        owner.update(cx, |owner, cx| {
            owner.invalidate_model_catalog();
            owner.start_model_catalog_load(cx);
        })
    });
    cx.wait_for_real(window.into(), std::time::Duration::from_secs(5), |_, cx| {
        !owner.read(cx).model_catalog_loading && owner.read(cx).configured_reasoning.is_some()
    })
    .await;
    let profile = cx
        .update(|cx| owner.read(cx).reasoning_profile_for_model("future"))
        .expect("recoverable invalid preference remains visible");
    assert_eq!(profile.preference, ReasoningChoice::Effort("high".into()));
    assert!(profile.efforts.is_empty());
    assert!(!profile.supports_preference());
    cx.update(|cx| {
        owner.update(cx, |owner, cx| {
            owner.apply_reasoning_profile_to_stream(&stream, "future", cx)
        });
        assert!(stream.read(cx).frozen_reasoning_for_submit().is_err());
        assert!(!stream.read(cx).reasoning_unavailable());
        owner.update(cx, |owner, cx| {
            owner.persist_composer_thinking(
                stream.clone(),
                &ComposerDefaultsRequested {
                    thread_id: "thread".into(),
                    defaults: ComposerDefaults {
                        model: "future".into(),
                        thinking: "provider_default".into(),
                        reasoning: Some(profile),
                        reasoning_unavailable: false,
                    },
                },
                cx,
            );
        });
    });
    cx.wait_for_real(window.into(), std::time::Duration::from_secs(5), |_, cx| {
        owner.read(cx).reasoning_save_pending.is_none()
    })
    .await;
    let saved = vega_store::reasoning::read_from(&preference).unwrap();
    assert_eq!(saved.profiles[0].preference, "provider_default");
    assert_eq!(std::fs::read(&cap).unwrap(), capability_bytes);
    let (reasoning, snapshot) = cx.update(|cx| {
        (
            stream.read(cx).frozen_reasoning_for_submit().unwrap(),
            stream.read(cx).model_snapshot_for_submit().unwrap(),
        )
    });
    assert_eq!(
        reasoning.as_ref().unwrap().choice,
        ReasoningChoice::ProviderDefault
    );
    let body = vega_runtime::test_support::encode_request(
        "https://unknown.invalid/v1",
        &vega_runtime::ChatRequest {
            model: "future".into(),
            messages: Vec::new(),
            reasoning,
            model_snapshot: snapshot,
            ..Default::default()
        },
        false,
    )
    .unwrap();
    assert!(body.get("reasoning_effort").is_none());
}

#[gpui_kit::test]
async fn started_worker_completion_cannot_replace_new_settings_entity_or_new_model_operation(
    cx: &mut TestAppContext,
) {
    for replacement in ["entity", "model"] {
        let dir = tempfile::tempdir().unwrap();
        let (owner, settings, _, window, path) = setup(cx, dir.path());
        let (generation, model) = base(load(cx, &owner, &settings, window).await);
        let patch =
            ModelCapabilityPatch::Upsert(vega_runtime::model_compat::ModelCapabilityOverride {
                identity: model.resolved.identity.clone(),
                allowed_efforts: Some(vec!["low".into()]),
                image_input: None,
            });
        begin(cx, &owner, &settings, generation, 1, model.clone(), patch);
        let replacement_model = if replacement == "model" {
            vega_conversation::ModelCapabilitySettingsService::new(path.clone())
                .load()
                .unwrap()
                .models
                .into_iter()
                .find(|projection| projection.resolved.identity.model == "second")
                .unwrap()
        } else {
            model
        };
        let replacement_view = if replacement == "entity" {
            cx.update(|cx| cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx)))
        } else {
            settings.clone()
        };
        cx.update(|cx| {
            replacement_view.update(cx, |view, cx| {
                view.open_model_settings(
                    "fixture",
                    if replacement == "model" {
                        "second"
                    } else {
                        "future"
                    },
                    2,
                    cx,
                );
                view.apply_model_capability_projection(
                    ModelCapabilitySettingsProjection::Saving {
                        generation: 900,
                        operation_id: 900,
                        model: replacement_model,
                    },
                    cx,
                )
            });
            owner.update(cx, |owner, _| {
                owner.settings_view = Some(replacement_view.clone())
            });
        });
        cx.wait_for_real(window.into(), std::time::Duration::from_secs(5), |_, cx| {
            path.with_file_name("model-capabilities.toml").exists()
                && matches!(
                    owner.read(cx).model_capability_settings_projection(),
                    ModelCapabilitySettingsProjection::Ready { .. }
                )
        })
        .await;
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(
                replacement_view
                    .read(cx)
                    .model_capability_save_is_current(900, 900),
                "{replacement}"
            )
        });
        assert_eq!(
            vega_store::model_capabilities::read_from(
                &path.with_file_name("model-capabilities.toml")
            )
            .unwrap()
            .models[0]
                .allowed_efforts
                .as_deref(),
            Some(["low".to_string()].as_slice())
        );
    }
}
