use super::*;
use gpui_kit::{
    Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, size,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Harness(Entity<SettingsView>);
impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}
fn click(cx: &mut TestAppContext, window: WindowHandle<Harness>, selector: &'static str) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn model_context_editor_projects_assumed_unknown_and_saved_states(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let provider = ProviderConfig {
        name: "owned".into(),
        enabled: true,
        base_url: "https://owned.invalid/v1".into(),
        models: vec!["model-a".into()],
        key_ref: "owned".into(),
    };
    let view = cx.new(|cx| {
        SettingsView::from_config(
            AppConfig {
                providers: vec![provider],
                ..Default::default()
            },
            None,
            cx,
        )
    });
    let loads = Arc::new(std::sync::Mutex::new(
        Vec::<ModelContextLoadRequested>::new(),
    ));
    let saves = Arc::new(std::sync::Mutex::new(
        Vec::<ModelContextSaveRequested>::new(),
    ));
    cx.update(|cx| {
        let captured = loads.clone();
        cx.subscribe(&view, move |_, request: &ModelContextLoadRequested, _| {
            captured.lock().unwrap().push(request.clone());
        })
        .detach();
        let captured = saves.clone();
        cx.subscribe(&view, move |_, request: &ModelContextSaveRequested, _| {
            captured.lock().unwrap().push(request.clone());
        })
        .detach();
    });
    let window = cx
        .update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| Harness(view.clone()))
            })
        })
        .unwrap();
    view.update(cx, |view, cx| {
        view.section = 0;
        view.provider_management.selected = Some("owned".into());
        view.provider_command(Command::EditModel("model-a".into()), cx);
    });
    let load = loads.lock().unwrap().last().cloned().expect("typed load");
    assert_eq!(
        (load.provider.as_str(), load.model.as_str()),
        ("owned", "model-a")
    );
    view.update(cx, |view, cx| {
        view.apply_model_context_loaded(
            &load,
            Ok(ModelContextLoaded {
                policy: None,
                legacy_present: true,
            }),
            cx,
        );
        let editor = view.provider_management.model_editor.as_ref().unwrap();
        assert_eq!(editor.source, PolicySource::AssumedDefault);
        assert_eq!(editor.input_limit.read(cx).text(), "300000");
        assert_eq!(editor.output_limit.read(cx).text(), "128000");
        assert!(editor.automatic);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("model-context-source").is_some());
    assert!(visual.debug_bounds("model-context-legacy-notice").is_some());
    assert!(visual.debug_bounds("model-context-input").is_some());
    assert!(visual.debug_bounds("model-context-output").is_some());

    view.update(cx, |view, cx| {
        view.provider_command(Command::ToggleModelUnknown, cx);
        view.provider_command(Command::SaveModel, cx);
    });
    let save = saves.lock().unwrap().last().cloned().expect("typed save");
    assert_eq!((save.input_limit, save.output_limit), (None, None));
    assert!(save.automatic_compaction);
    view.update(cx, |view, cx| {
        view.apply_model_context_saved(
            &save,
            Ok(ModelContextPolicy::unconfigured("owned", "model-a")),
            cx,
        );
        assert_eq!(
            view.provider_management
                .model_editor
                .as_ref()
                .unwrap()
                .source,
            PolicySource::Unknown
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("model-context-input").is_none());
    assert!(visual.debug_bounds("model-context-output").is_none());
    view.update(cx, |view, cx| {
        view.provider_command(Command::EditModel("model-a".into()), cx);
    });
    let reload = loads.lock().unwrap().last().cloned().expect("reload");
    assert!(reload.request_id > load.request_id);
    let saved = ModelContextPolicy {
        provider: "owned".into(),
        model: "model-a".into(),
        input_limit: Some(120_000),
        output_reserve: Some(16_000),
        automatic_compaction: false,
        updated_at: 5,
    };
    view.update(cx, |view, cx| {
        // An old ACK cannot replace the newly opened editor's values.
        view.apply_model_context_loaded(
            &load,
            Ok(ModelContextLoaded {
                policy: None,
                legacy_present: false,
            }),
            cx,
        );
        view.apply_model_context_loaded(
            &reload,
            Ok(ModelContextLoaded {
                policy: Some(saved),
                legacy_present: false,
            }),
            cx,
        );
        let editor = view.provider_management.model_editor.as_ref().unwrap();
        assert_eq!(editor.source, PolicySource::Saved);
        assert_eq!(editor.input_limit.read(cx).text(), "120000");
        assert_eq!(editor.output_limit.read(cx).text(), "16000");
        assert!(!editor.automatic);
    });
}

#[gpui_kit::test]
async fn new_or_renamed_model_stays_in_editor_without_inheriting_an_old_policy(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    AppConfig {
        providers: vec![ProviderConfig {
            name: "owned".into(),
            enabled: true,
            base_url: "https://owned.invalid/v1".into(),
            models: vec![],
            key_ref: "owned".into(),
        }],
        ..Default::default()
    }
    .save_to(&path)
    .unwrap();
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx));
    let loads = Arc::new(std::sync::Mutex::new(
        Vec::<ModelContextLoadRequested>::new(),
    ));
    let saves = Arc::new(std::sync::Mutex::new(
        Vec::<ModelContextSaveRequested>::new(),
    ));
    cx.update(|cx| {
        let captured = loads.clone();
        cx.subscribe(&view, move |_, request: &ModelContextLoadRequested, _| {
            captured.lock().unwrap().push(request.clone());
        })
        .detach();
        let captured = saves.clone();
        cx.subscribe(&view, move |_, request: &ModelContextSaveRequested, _| {
            captured.lock().unwrap().push(request.clone());
        })
        .detach();
    });
    view.update(cx, |view, cx| {
        view.section = 0;
        view.provider_management.selected = Some("owned".into());
        view.provider_command(Command::AddModel, cx);
        view.provider_management
            .model_editor
            .as_ref()
            .unwrap()
            .model_input
            .update(cx, |input, cx| input.set_text("new-model", cx));
        view.provider_command(Command::SaveModel, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        config::read_from(&path).unwrap().providers[0].models,
        vec!["new-model"]
    );
    assert_eq!(
        view.read_with(cx, |view, _| {
            view.provider_management
                .model_editor
                .as_ref()
                .and_then(|editor| editor.original.as_deref().map(str::to_string))
        }),
        Some("new-model".into())
    );
    let first_load = loads
        .lock()
        .unwrap()
        .last()
        .cloned()
        .expect("new model load");
    view.update(cx, |view, cx| {
        view.apply_model_context_loaded(
            &first_load,
            Ok(ModelContextLoaded {
                policy: None,
                legacy_present: false,
            }),
            cx,
        );
        let editor = view.provider_management.model_editor.as_ref().unwrap();
        editor
            .input_limit
            .update(cx, |input, cx| input.set_text("20000", cx));
        editor
            .output_limit
            .update(cx, |input, cx| input.set_text("2000", cx));
        view.provider_command(Command::SaveModel, cx);
    });
    let save = saves
        .lock()
        .unwrap()
        .last()
        .cloned()
        .expect("new model policy save");
    assert_eq!(save.model, "new-model");
    assert_eq!(
        (save.input_limit, save.output_limit),
        (Some(20_000), Some(2_000))
    );
    view.update(cx, |view, cx| {
        view.apply_model_context_saved(
            &save,
            Ok(ModelContextPolicy {
                provider: "owned".into(),
                model: "new-model".into(),
                input_limit: Some(20_000),
                output_reserve: Some(2_000),
                automatic_compaction: true,
                updated_at: 1,
            }),
            cx,
        );
        let editor = view.provider_management.model_editor.as_ref().unwrap();
        editor
            .model_input
            .update(cx, |input, cx| input.set_text("renamed-model", cx));
        view.provider_command(Command::SaveModel, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        config::read_from(&path).unwrap().providers[0].models,
        vec!["renamed-model"]
    );
    let renamed = loads.lock().unwrap().last().cloned().expect("rename load");
    assert!(renamed.request_id > first_load.request_id);
    assert_eq!(renamed.model, "renamed-model");
    assert_eq!(
        view.read_with(cx, |view, _| {
            view.provider_management
                .model_editor
                .as_ref()
                .and_then(|editor| editor.renamed_from.clone())
        }),
        Some("new-model".into())
    );
}

#[gpui_kit::test]
async fn pointer_credential_recovery_patch_and_reload_clear_obsolete_network_results(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    let provider = ProviderConfig {
        name: "Owned".into(),
        enabled: true,
        base_url: "https://owned.invalid/v1".into(),
        models: vec!["owned-model".into()],
        key_ref: "Owned".into(),
    };
    AppConfig {
        providers: vec![provider],
        ..Default::default()
    }
    .save_to(&path)
    .unwrap();
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx));
    let window = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1280.), px(750.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Harness(view.clone())),
            )
        })
        .unwrap();
    cx.run_until_parked();
    click(cx, window, "settings-nav-providers");
    click(cx, window, "model-test-0");
    assert!(view.read_with(cx, |view, _| {
        view.provider_management
            .message
            .as_ref()
            .is_some_and(|message| message.contains("本地凭据"))
    }));
    click(cx, window, "provider-edit");
    window
        .update(cx, |_, window, cx| {
            view.read(cx)
                .key_input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx)
        })
        .unwrap();
    cx.simulate_input(window.into(), "owned-recovery-secret");
    cx.simulate_keystrokes(window.into(), "cmd-enter");
    cx.run_until_parked();
    assert_eq!(
        keystore::get_key(root.path(), "Owned").unwrap(),
        "owned-recovery-secret"
    );
    assert!(
        view.read_with(cx, |view, _| view.provider_management.message.is_none()
            && view.provider_management.statuses.is_empty()
            && view.available_key_refs.contains(&"Owned".into()))
    );
    // A successful config patch clears earlier failures without claiming a new test.
    keystore::delete_key(root.path(), "Owned").unwrap();
    click(cx, window, "model-test-0");
    assert!(view.read_with(cx, |view, _| !view.provider_management.statuses.is_empty()));
    click(cx, window, "provider-enabled");
    assert!(
        view.read_with(cx, |view, _| view.provider_management.message.is_none()
            && view.provider_management.statuses.is_empty())
    );
    click(cx, window, "provider-enabled");
    click(cx, window, "model-test-0");
    assert!(view.read_with(cx, |view, _| !view.provider_management.statuses.is_empty()));
    click(cx, window, "provider-reload");
    assert!(
        view.read_with(cx, |view, _| view.provider_management.message.is_none()
            && view.provider_management.statuses.is_empty())
    );
}

#[gpui_kit::test]
async fn small_provider_detail_retains_url_height_with_multiple_models(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    AppConfig {
        providers: vec![ProviderConfig {
            name: "Owned".into(),
            enabled: true,
            base_url: "https://owned.invalid/v1".into(),
            models: vec![
                "owned-model".into(),
                format!("vendor/{}", "long-id-".repeat(20)),
                "third-model".into(),
            ],
            key_ref: "Owned".into(),
        }],
        ..Default::default()
    }
    .save_to(&path)
    .unwrap();
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(|cx| SettingsView::from_path(Some(path), cx));
    let window = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(960.), px(560.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Harness(view.clone())),
            )
        })
        .unwrap();
    cx.run_until_parked();
    click(cx, window, "settings-nav-providers");
    // Seed after navigation because changing settings pages intentionally cancels and clears
    // transient provider operations.
    view.update(cx, |view, cx| {
        view.provider_management.message = Some("本地凭据不存在，请重新保存 API Key".into());
        view.provider_management
            .statuses
            .insert("owned-model".into(), "本地凭据不存在".into());
        cx.notify();
    });
    cx.run_until_parked();
    for dark in [false, true] {
        cx.update(|cx| {
            cx.set_global(if dark {
                vega_theme::Theme::dark()
            } else {
                vega_theme::Theme::light()
            });
            cx.refresh_windows();
        });
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let url = visual.debug_bounds("provider-base-url").unwrap();
        let viewport = visual.debug_bounds("provider-detail-viewport").unwrap();
        assert!(
            url.size.height >= px((Typography::BODY * Typography::BODY_LINE_HEIGHT).floor()),
            "URL text must retain a complete line, got {:?}",
            url.size
        );
        assert!(url.origin.y >= viewport.origin.y && url.bottom() <= viewport.bottom());
        assert!(url.size.width > px(0.) && url.right() <= viewport.right());
        let flow = visual.debug_bounds("provider-detail-flow").unwrap();
        let message = visual.debug_bounds("provider-network-message").unwrap();
        assert!(
            flow.size.height > viewport.size.height,
            "overflow must expand the scrollable flow: flow={:?}, viewport={:?}",
            flow.size,
            viewport.size
        );
        assert!(
            message.size.height >= px((Typography::BODY * Typography::BODY_LINE_HEIGHT).floor())
        );
        assert!(message.bottom() <= flow.bottom());
    }
}

#[gpui_kit::test]
async fn pointer_stop_clears_pending_connection_projection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let provider = ProviderConfig {
        name: "Owned".into(),
        enabled: true,
        base_url: "https://owned.invalid/v1".into(),
        models: vec!["owned-model".into()],
        key_ref: "Owned".into(),
    };
    let view = cx.new(|cx| {
        SettingsView::from_config(
            AppConfig {
                providers: vec![provider],
                ..Default::default()
            },
            None,
            cx,
        )
    });
    let token = CancellationToken::new();
    let window = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1280.), px(750.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Harness(view.clone())),
            )
        })
        .unwrap();
    cx.run_until_parked();
    click(cx, window, "settings-nav-providers");
    // Seed only an in-flight UI projection after navigation; real transport cancellation has
    // D/root evidence and page changes intentionally cancel transient provider operations.
    view.update(cx, |view, cx| {
        view.provider_management.cancel = Some(token.clone());
        view.provider_management.message = Some("正在连接…".into());
        view.provider_management
            .statuses
            .insert("owned-model".into(), "正在测试…".into());
        cx.notify();
    });
    cx.run_until_parked();
    click(cx, window, "provider-cancel");
    assert!(token.is_cancelled());
    assert!(
        view.read_with(cx, |view, _| view.provider_management.message.is_none()
            && view.provider_management.statuses.is_empty()
            && view.provider_management.cancel.is_none())
    );
}

fn write_test_pi_source(path: &std::path::Path, body: serde_json::Value) {
    std::fs::write(path, serde_json::to_vec(&body).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn mounted_pi_fixture(
    cx: &mut TestAppContext,
    source_exists: bool,
) -> (
    tempfile::TempDir,
    Entity<SettingsView>,
    WindowHandle<Harness>,
    ProviderConfig,
    std::path::PathBuf,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    let pi_path = root.path().join("pi-models.json");
    let provider = ProviderConfig {
        enabled: false,
        name: "cpa".into(),
        base_url: "https://cpa.example.test/v1".into(),
        models: vec!["glm-5.3-flash".into()],
        key_ref: "cpa".into(),
    };
    AppConfig {
        providers: vec![provider.clone()],
        ..Default::default()
    }
    .save_to(&path)
    .unwrap();
    if source_exists {
        write_test_pi_source(
            &pi_path,
            serde_json::json!({"providers": {"cpa": {
                "api": "openai-completions",
                "baseUrl": "https://cpa.example.test/v1",
                "apiKey": "fake-pi-agent-ui-key",
                "models": [{"id": "glm-5.3-flash"}]
            }}}),
        );
    }
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(|cx| SettingsView::from_path(Some(path), cx));
    let window = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1280.), px(750.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Harness(view.clone())),
            )
        })
        .unwrap();
    cx.run_until_parked();
    click(cx, window, "settings-nav-providers");
    view.update(cx, |view, _| {
        view.pi_models_path = Some(pi_path.clone());
    });
    cx.run_until_parked();
    (root, view, window, provider, pi_path)
}

fn mounted_issue82_provider_fixture(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<SettingsView>,
    WindowHandle<Harness>,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    AppConfig {
        providers: vec![
            ProviderConfig {
                name: "Stored".into(),
                enabled: true,
                base_url: "https://stored.invalid/v1".into(),
                models: vec!["stored-model".into()],
                key_ref: "stored".into(),
            },
            ProviderConfig {
                name: "Missing".into(),
                enabled: true,
                base_url: "https://missing.invalid/v1".into(),
                models: vec!["missing-model".into()],
                key_ref: "missing".into(),
            },
        ],
        ..Default::default()
    }
    .save_to(&path)
    .unwrap();
    keystore::set_key(root.path(), "stored", "issue82-test-secret").unwrap();
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(|cx| SettingsView::from_path(Some(path), cx));
    let window = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(960.), px(750.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Harness(view.clone())),
            )
        })
        .unwrap();
    cx.run_until_parked();
    click(cx, window, "settings-nav-providers");
    (root, view, window)
}

#[gpui_kit::test]
async fn issue82_provider_help_opens_on_hover_and_focus_and_keeps_credential_states(
    cx: &mut TestAppContext,
) {
    let (_root, view, window) = mounted_issue82_provider_fixture(cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("provider-key-stored").is_some());
    assert!(visual.debug_bounds("provider-key-missing").is_none());
    assert!(visual.debug_bounds("provider-import-pi").is_some());
    assert!(visual.debug_bounds("provider-test-help-discover").is_some());
    assert!(visual.debug_bounds("provider-test-help-model-0").is_some());
    assert_eq!(
        PROVIDER_TEST_HELP_COPY,
        "测试会发送少量固定文本请求；不会使用聊天内容。"
    );

    let discover_help = visual.debug_bounds("provider-test-help-discover").unwrap();
    let discover_button = visual.debug_bounds("provider-discover").unwrap();
    assert!(discover_button.right() <= discover_help.left());
    visual.simulate_mouse_move(discover_help.center(), None, Default::default());
    cx.run_until_parked();
    assert!(
        visual
            .debug_bounds("provider-test-help-discover-tooltip")
            .is_some()
    );
    visual.simulate_mouse_move(gpui_kit::point(px(1.), px(1.)), None, Default::default());
    cx.run_until_parked();
    assert!(
        visual
            .debug_bounds("provider-test-help-discover-tooltip")
            .is_none()
    );

    let discover_focus = view.read_with(cx, |view, _| view.provider_test_help_focuses[0].clone());
    window
        .update(cx, |_, window, cx| discover_focus.focus(window, cx))
        .unwrap();
    cx.run_until_parked();
    assert!(
        visual
            .debug_bounds("provider-test-help-discover-tooltip")
            .is_some()
    );
    let model_focus = view.read_with(cx, |view, _| view.provider_test_help_focuses[1].clone());
    window
        .update(cx, |_, window, cx| model_focus.focus(window, cx))
        .unwrap();
    cx.run_until_parked();
    assert!(
        visual
            .debug_bounds("provider-test-help-discover-tooltip")
            .is_none()
    );
    assert!(
        visual
            .debug_bounds("provider-test-help-model-0-tooltip")
            .is_some()
    );
    let test_button = visual.debug_bounds("model-test-0").unwrap();
    let model_help = visual.debug_bounds("provider-test-help-model-0").unwrap();
    let edit_button = visual.debug_bounds("model-edit-0").unwrap();
    assert!(test_button.right() <= model_help.left());
    assert!(model_help.right() <= edit_button.left());

    view.update(cx, |view, cx| {
        view.provider_management.selected = Some("Missing".into());
        cx.notify();
    });
    cx.run_until_parked();
    assert!(visual.debug_bounds("provider-key-stored").is_none());
    assert!(visual.debug_bounds("provider-key-missing").is_some());
    assert!(visual.debug_bounds("provider-import-pi").is_some());
    assert!(visual.debug_bounds("provider-test-help-model-0").is_some());

    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::dark());
        cx.refresh_windows();
    });
    visual.simulate_resize(size(px(960.), px(750.)));
    cx.run_until_parked();
    let help = visual.debug_bounds("provider-test-help-model-0").unwrap();
    assert!(help.right() <= px(960.));
    assert!(help.bottom() <= px(750.));
}

#[gpui_kit::test]
async fn mounted_pi_import_has_explicit_reachable_action_and_idle_does_not_read_source(
    cx: &mut TestAppContext,
) {
    let (_root, view, window, _provider, pi_path) = mounted_pi_fixture(cx, true);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("provider-import-pi").is_some());
    assert!(view.read_with(cx, |view, cx| {
        view.provider_management.message.is_none() && view.key_input.read(cx).text().is_empty()
    }));
    assert!(std::fs::metadata(pi_path).is_ok());
}

#[gpui_kit::test]
async fn mounted_pi_import_success_shows_setup_status_enables_provider_and_keeps_key_blank(
    cx: &mut TestAppContext,
) {
    let (root, view, window, provider, _pi_path) = mounted_pi_fixture(cx, true);
    let events = Arc::new(AtomicUsize::new(0));
    let events_copy = events.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, _: &SettingsSaved, _| {
            events_copy.fetch_add(1, Ordering::SeqCst);
        })
        .detach();
    });
    click(cx, window, "provider-import-pi");
    assert!(view.read_with(cx, |view, cx| {
        view.provider_management
            .message
            .as_deref()
            .is_some_and(|message| message.contains("已从 Pi Agent 导入凭据"))
            && view.key_input.read(cx).text().is_empty()
            && view.available_key_refs.contains(&provider.key_ref)
    }));
    let saved = config::read_from(&root.path().join("config.toml")).unwrap();
    assert!(saved.providers[0].enabled);
    assert_eq!(events.load(Ordering::SeqCst), 1);
    assert!(
        !std::fs::read_to_string(root.path().join("config.toml"))
            .unwrap()
            .contains("fake-pi-agent-ui-key")
    );
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("provider-import-status").is_some());
}

#[gpui_kit::test]
async fn mounted_pi_import_failure_is_visible_and_does_not_enable_provider(
    cx: &mut TestAppContext,
) {
    let (root, view, window, _provider, _pi_path) = mounted_pi_fixture(cx, false);
    click(cx, window, "provider-import-pi");
    assert!(view.read_with(cx, |view, cx| {
        view.provider_management
            .message
            .as_deref()
            .is_some_and(|message| message.contains("导入凭据失败"))
            && view.key_input.read(cx).text().is_empty()
    }));
    let saved = config::read_from(&root.path().join("config.toml")).unwrap();
    assert!(!saved.providers[0].enabled);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("provider-import-status").is_some());
}

#[gpui_kit::test]
async fn mounted_pi_import_in_progress_disables_action(cx: &mut TestAppContext) {
    let (root, view, window, _provider, _pi_path) = mounted_pi_fixture(cx, true);
    view.update(cx, |view, cx| {
        view.provider_management.saving = true;
        view.provider_management.message = Some("正在从 Pi Agent 导入凭据…".into());
        cx.notify();
    });
    cx.run_until_parked();
    click(cx, window, "provider-import-pi");
    assert!(view.read_with(cx, |view, _| {
        view.provider_management.saving
            && view
                .provider_management
                .message
                .as_deref()
                .is_some_and(|message| message.contains("正在从 Pi Agent"))
    }));
    let saved = config::read_from(&root.path().join("config.toml")).unwrap();
    assert!(!saved.providers[0].enabled);
}

#[gpui_kit::test]
async fn pointer_settings_uses_real_service_config_and_mock_transport(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    let verbs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = verbs.clone();
    let transport = vega_runtime::provider_check::mock::Transport(Arc::new(move |request| {
        let mut verbs = captured.lock().unwrap();
        let index = verbs.len();
        verbs.push(format!(
            "{} {} HTTP/1.1",
            request.method(),
            request.url().path()
        ));
        assert_eq!(
            request.headers().get("authorization").unwrap(),
            "Bearer owned-ui-secret"
        );
        let response = match index {
            0 => r#"{"data":[{"id":"owned-model"}]}"#,
            1 => {
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
            }
            _ => panic!("unexpected extra request"),
        };
        Box::pin(async move {
            Ok(vega_runtime::provider_check::mock::response(
                200,
                "",
                response.as_bytes().to_vec(),
            ))
        })
    }));
    let a = ProviderConfig {
        name: "Owned A".into(),
        enabled: true,
        base_url: "http://fixture.invalid/v1".into(),
        models: vec![],
        key_ref: "owned-a".into(),
    };
    let b = ProviderConfig {
        name: "Owned B".into(),
        enabled: true,
        base_url: "https://owned.invalid/v1".into(),
        models: vec!["other-model".into()],
        key_ref: "owned-b".into(),
    };
    let config = AppConfig {
        providers: vec![a, b],
        ..Default::default()
    };
    config.save_to(&path).unwrap();
    keystore::set_key(root.path(), "owned-a", "owned-ui-secret").unwrap();
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx));
    view.update(cx, |view, _| {
        view.provider_management.transport = Some(transport)
    });
    let window = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1280.), px(750.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Harness(view.clone())),
            )
        })
        .unwrap();
    cx.run_until_parked();
    click(cx, window, "settings-nav-providers");
    click(cx, window, "provider-down");
    let saved = config::read_from(&path).unwrap();
    assert_eq!(
        saved
            .providers
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["Owned B", "Owned A"]
    );
    click(cx, window, "provider-enabled");
    assert!(!config::read_from(&path).unwrap().providers[1].enabled);
    click(cx, window, "provider-enabled");
    assert!(config::read_from(&path).unwrap().providers[1].enabled);
    click(cx, window, "provider-discover");
    assert!(view.read_with(cx, |view, _| {
        view.provider_management
            .candidates
            .as_ref()
            .is_some_and(|models| models == &["owned-model"])
    }));
    assert!(
        config::read_from(&path).unwrap().providers[1]
            .models
            .is_empty()
    );
    click(cx, window, "candidate-0");
    click(cx, window, "provider-import");
    assert_eq!(
        config::read_from(&path).unwrap().providers[1].models,
        ["owned-model"]
    );
    click(cx, window, "model-test-0");
    assert!(view.read_with(cx, |view, _| {
        view.provider_management
            .statuses
            .get("owned-model")
            .is_some_and(|s| s == "连接成功")
    }));
    assert_eq!(
        *verbs.lock().unwrap(),
        [
            "GET /v1/models HTTP/1.1",
            "POST /v1/chat/completions HTTP/1.1"
        ]
    );
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::dark());
        cx.refresh_windows();
    });
    cx.run_until_parked();
    click(cx, window, "provider-add-model");
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let cancel = visual.debug_bounds("model-cancel").unwrap();
    visual.simulate_click(cancel.center(), Default::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| {
        view.provider_management.model_editor.is_none()
    }));
    assert!(
        !std::fs::read_to_string(path)
            .unwrap()
            .contains("owned-ui-secret")
    );
}
