use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gpui_kit::{
    Bounds, KeyBinding, Render, TestAppContext, VisualTestContext, WindowBounds, WindowHandle,
    WindowOptions, size,
};

use super::*;
use crate::settings::state::{
    PRICING_INPUT_BYTES_LIMIT, PROVIDER_MODELS_FRAME_INSET, PROVIDER_MODELS_MAX_ROWS,
    PROVIDER_MODELS_MIN_ROWS,
};

struct SettingsHarness {
    view: Entity<SettingsView>,
    closes: Arc<AtomicUsize>,
}

impl Render for SettingsHarness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .on_action(cx.listener(|this, _: &CloseSettings, _, _| {
                this.closes.fetch_add(1, Ordering::SeqCst);
            }))
            .child(self.view.clone())
    }
}

fn provider(name: &str, models: &[&str]) -> ProviderConfig {
    ProviderConfig {
        enabled: true,
        name: name.to_string(),
        base_url: format!("https://{name}.example.com"),
        models: models.iter().map(|m| m.to_string()).collect(),
        key_ref: name.to_string(),
    }
}

fn assert_pixel_close(actual: gpui_kit::Pixels, expected: f32, label: &str) {
    let actual = f32::from(actual);
    assert!(
        (actual - expected).abs() <= 1.0,
        "{label}: expected {expected}±1px, got {actual}px"
    );
}

#[gpui_kit::test]
async fn r21_settings_shell_opens_general_and_tracks_sidebar_width(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        cx.set_global(crate::sidebar::SidebarWidth(Layout::SIDEBAR_WIDTH));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    assert_eq!(view.read_with(cx, |view, _| view.section), 1);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("R21 Settings window");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let nav = visual
            .debug_bounds("settings-navigation")
            .expect("Settings navigation");
        let content = visual
            .debug_bounds("settings-content-column")
            .expect("Settings content column");
        assert_pixel_close(nav.size.width, Layout::SIDEBAR_WIDTH, "Settings rail width");
        assert_pixel_close(
            content.size.width,
            Layout::SETTINGS_CONTENT_MAX_WIDTH,
            "Settings content cap",
        );
        assert!(visual.debug_bounds("settings-page-general").is_some());
        let sidebar_switch = visual
            .debug_bounds("settings-sidebar-switch")
            .expect("Settings Sidebar switch");
        assert_pixel_close(
            sidebar_switch.size.width,
            Layout::SETTINGS_SWITCH_WIDTH,
            "Settings switch width",
        );
        assert_pixel_close(
            sidebar_switch.size.height,
            Layout::SETTINGS_SWITCH_HEIGHT,
            "Settings switch height",
        );
        let rows = [
            "settings-nav-general",
            "settings-nav-providers",
            "settings-nav-reasoning",
            "settings-nav-pricing",
            "settings-nav-usage",
        ]
        .map(|selector| {
            visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing {selector}"))
        });
        for row in rows {
            assert_pixel_close(
                row.size.height,
                Typography::SIDEBAR_LINE_HEIGHT,
                "Settings navigation row height",
            );
        }
        assert!(rows.windows(2).all(|pair| pair[0].top() <= pair[1].top()));

        let providers = visual
            .debug_bounds("settings-nav-providers")
            .expect("Providers navigation");
        visual.simulate_click(providers.center(), Default::default());
    }
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.section), 0);
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-page-providers")
            .is_some()
    );

    cx.update(|cx| crate::sidebar::set_width(Layout::SIDEBAR_MAX_WIDTH, cx));
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert_pixel_close(
        visual
            .debug_bounds("settings-navigation")
            .expect("resized Settings navigation")
            .size
            .width,
        Layout::SIDEBAR_MAX_WIDTH,
        "resized Settings rail width",
    );
    assert_pixel_close(
        visual
            .debug_bounds("settings-content-column")
            .expect("resized Settings content")
            .size
            .width,
        Layout::SETTINGS_CONTENT_MAX_WIDTH,
        "resized Settings content cap",
    );
}

#[test]
fn form_rejects_empty_fields() {
    assert!(!form_is_submittable("", "https://x", "k"));
    assert!(!form_is_submittable("n", "", "k"));
    assert!(!form_is_submittable("n", "https://x", ""));
    // 空白 name 视为空。
    assert!(!form_is_submittable("   ", "https://x", "k"));
    assert!(form_is_submittable("n", "https://x", "k"));
    assert!(provider_form_is_submittable("n", "https://x", "", true));
    assert!(!provider_form_is_submittable("n", "https://x", "", false));
}

#[test]
fn provider_models_normalize_and_preserve_exact_ids() {
    let parsed =
        parse_provider_models("  OpenAI/GPT-4.1-mini  \n\nclaude-3.5-sonnet\nvendor/model-v1.2\n")
            .expect("valid provider models");
    assert_eq!(
        parsed,
        vec![
            "OpenAI/GPT-4.1-mini",
            "claude-3.5-sonnet",
            "vendor/model-v1.2"
        ]
    );
    assert!(parse_provider_models("Foo\nfoo").is_ok());
}

#[test]
fn provider_models_reject_empty_invalid_duplicate_and_limits() {
    assert_eq!(
        parse_provider_models(" \n\t").unwrap_err(),
        ProviderModelsError::Empty
    );
    assert_eq!(
        parse_provider_models("valid/model\nvalid/model").unwrap_err(),
        ProviderModelsError::Duplicate { line: 2 }
    );
    assert_eq!(
        parse_provider_models("valid model").unwrap_err(),
        ProviderModelsError::Invalid { line: 1 }
    );
    assert_eq!(
        parse_provider_models(&"a".repeat(PROVIDER_MODEL_ID_MAX_BYTES + 1)).unwrap_err(),
        ProviderModelsError::TooLong { line: 1 }
    );
    let too_many = (0..=PROVIDER_MODEL_COUNT_MAX)
        .map(|index| format!("model-{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        parse_provider_models(&too_many).unwrap_err(),
        ProviderModelsError::TooMany {
            line: PROVIDER_MODEL_COUNT_MAX + 1
        }
    );
    assert_eq!(
        parse_provider_models(&"x".repeat(PROVIDER_MODELS_INPUT_BYTES_LIMIT + 1)).unwrap_err(),
        ProviderModelsError::InputTooLarge
    );
}

#[test]
fn provider_key_ref_keeps_existing_empty_key_semantics() {
    let mut existing = provider("legacy", &["model"]);
    existing.key_ref = "legacy-ref".to_string();
    assert_eq!(
        provider_key_ref(Some(&existing), "legacy", ""),
        "legacy-ref"
    );
    assert_eq!(
        provider_key_ref(Some(&existing), "legacy", "new-key"),
        "legacy"
    );
    assert_eq!(provider_key_ref(None, "new", "new-key"), "new");
    assert_eq!(provider_key_ref(None, "new", ""), "new");
}

#[test]
fn upsert_appends_new_and_updates_same_name_models() {
    let mut providers = vec![provider("deepseek", &["deepseek-chat"])];
    // 异名追加。
    assert!(!upsert_provider(&mut providers, provider("openai", &[])));
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[1].name, "openai");
    // 同名更新：表单字段和 models 一起替换。
    let mut replacement = provider("deepseek", &["OpenAI/GPT-4.1-mini", "claude-3.5-sonnet"]);
    replacement.base_url = "https://api.deepseek.com/v1".to_string();
    assert!(upsert_provider(&mut providers, replacement));
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0].base_url, "https://api.deepseek.com/v1");
    assert_eq!(
        providers[0].models,
        vec!["OpenAI/GPT-4.1-mini", "claude-3.5-sonnet"]
    );
}

#[gpui_kit::test]
async fn provider_submit_uses_owned_backends_and_emits_only_after_save(cx: &mut TestAppContext) {
    let view = cx.new(SettingsView::new_for_test);
    let saved = Arc::new(Mutex::new(Vec::<AppConfig>::new()));
    let key_writes = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(AtomicUsize::new(0));
    let saved_copy = saved.clone();
    let key_writes_copy = key_writes.clone();
    let events_copy = events.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, _: &SettingsSaved, _| {
            events_copy.fetch_add(1, Ordering::SeqCst);
        })
        .detach();
    });
    view.update(cx, |view, cx| {
        view.config.providers = vec![provider("legacy", &["old/model-v1"])];
        view.key_writer = Some(Arc::new(move |_, _| {
            key_writes_copy.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        view.config_saver = Some(Arc::new(move |config| {
            saved_copy
                .lock()
                .expect("owned config capture")
                .push(config.clone());
            Ok(())
        }));
        view.name_input
            .update(cx, |input, cx| input.set_text("legacy", cx));
        view.base_url_input.update(cx, |input, cx| {
            input.set_text("https://legacy.invalid/v1", cx)
        });
        view.models_input.update(cx, |input, cx| {
            input.set_text("OpenAI/GPT-4.1-mini\nclaude-3.5-sonnet", cx)
        });
        view.key_input.update(cx, TextInput::clear);
        view.submit_provider(cx);
    });
    assert_eq!(key_writes.load(Ordering::SeqCst), 0);
    assert_eq!(events.load(Ordering::SeqCst), 1);
    let saved_config = saved.lock().expect("saved config capture");
    assert_eq!(saved_config[0].providers[0].key_ref, "legacy");
    assert_eq!(
        saved_config[0].providers[0].models,
        vec!["OpenAI/GPT-4.1-mini", "claude-3.5-sonnet"]
    );
    drop(saved_config);

    view.update(cx, |view, cx| {
        view.name_input
            .update(cx, |input, cx| input.set_text("new-provider", cx));
        view.base_url_input
            .update(cx, |input, cx| input.set_text("https://new.invalid/v1", cx));
        view.models_input
            .update(cx, |input, cx| input.set_text("vendor/model-v1.2", cx));
        view.key_input
            .update(cx, |input, cx| input.set_text("owned-test-key", cx));
        view.submit_provider(cx);
    });
    assert_eq!(key_writes.load(Ordering::SeqCst), 1);
    assert_eq!(events.load(Ordering::SeqCst), 2);
    let saved_config = saved.lock().expect("new saved config capture");
    assert_eq!(saved_config[1].providers[1].name, "new-provider");
    assert_eq!(
        saved_config[1].providers[1].models,
        vec!["vendor/model-v1.2"]
    );
    assert_eq!(saved_config[1].providers[1].key_ref, "new-provider");
    assert!(view.read_with(cx, |view, cx| {
        view.name_input.read(cx).text().is_empty()
            && view.models_input.read(cx).text().is_empty()
            && view.key_input.read(cx).text().is_empty()
    }));
}

#[gpui_kit::test]
async fn provider_submit_keeps_draft_and_authority_on_validation_or_save_failure(
    cx: &mut TestAppContext,
) {
    let view = cx.new(SettingsView::new_for_test);
    let saved = Arc::new(Mutex::new(Vec::<AppConfig>::new()));
    let key_writes = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(AtomicUsize::new(0));
    let saved_copy = saved.clone();
    let key_writes_copy = key_writes.clone();
    let events_copy = events.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, _: &SettingsSaved, _| {
            events_copy.fetch_add(1, Ordering::SeqCst);
        })
        .detach();
    });
    view.update(cx, |view, cx| {
        view.config.providers = vec![provider("legacy", &["old/model-v1"])];
        view.key_writer = Some(Arc::new(move |_, _| {
            key_writes_copy.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        view.config_saver = Some(Arc::new(move |config| {
            saved_copy
                .lock()
                .expect("failed config capture")
                .push(config.clone());
            Err("owned config rejected".to_string())
        }));
        view.name_input
            .update(cx, |input, cx| input.set_text("legacy", cx));
        view.base_url_input.update(cx, |input, cx| {
            input.set_text("https://legacy.invalid/v2", cx)
        });
        view.models_input.update(cx, |input, cx| {
            input.set_text("old/model-v1\nold/model-v1", cx)
        });
        view.key_input.update(cx, TextInput::clear);
        view.submit_provider(cx);
    });
    assert_eq!(key_writes.load(Ordering::SeqCst), 0);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert!(view.read_with(cx, |view, cx| {
        view.config.providers[0].models == vec!["old/model-v1"]
            && view.models_input.read(cx).text() == "old/model-v1\nold/model-v1"
            && view
                .error
                .as_deref()
                .is_some_and(|error| error.contains("重复"))
    }));

    view.update(cx, |view, cx| {
        view.models_input
            .update(cx, |input, cx| input.set_text("new/model-v2", cx));
        view.submit_provider(cx);
    });
    assert_eq!(key_writes.load(Ordering::SeqCst), 0);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert!(view.read_with(cx, |view, cx| {
        view.config.providers[0].models == vec!["old/model-v1"]
            && view.models_input.read(cx).text() == "new/model-v2"
            && view
                .error
                .as_deref()
                .is_some_and(|error| error.contains("当前输入仍未保存"))
    }));
}

#[gpui_kit::test]
async fn provider_form_focus_and_edit_action_follow_the_real_ui_path(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    let save_count = Arc::new(AtomicUsize::new(0));
    let save_count_copy = save_count.clone();
    view.update(cx, |view, _| {
        view.config.providers = vec![provider("legacy", &["old/model-v1"])];
        view.config_saver = Some(Arc::new(move |_| {
            save_count_copy.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
    });
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            let bounds = Bounds::centered(None, size(px(960.), px(600.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("provider settings window");
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let providers = visual
            .debug_bounds("settings-nav-providers")
            .expect("providers navigation");
        visual.simulate_click(providers.center(), Default::default());
    }
    cx.run_until_parked();

    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let edit = visual.debug_bounds("provider-edit").expect("edit button");
        visual.simulate_click(edit.center(), Default::default());
    }
    cx.run_until_parked();
    window
        .update(cx, |_, window, cx| {
            let name = view.read(cx).name_input.read(cx).focus_handle(cx);
            name.focus(window, cx);
        })
        .expect("focus provider name");
    cx.simulate_keystrokes(window.into(), "tab tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                view.read(cx)
                    .models_input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            })
            .expect("models focus")
    );

    // The provider selector and edit form are separate surfaces in R14.
    // Reopen through the actual detail action, then verify loaded inputs.
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let cancel = visual
            .debug_bounds("provider-form-cancel")
            .expect("cancel form");
        visual.simulate_click(cancel.center(), Default::default());
    }
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let edit = visual.debug_bounds("provider-edit").expect("edit button");
        visual.simulate_click(edit.center(), Default::default());
    }
    window
        .update(cx, |_, window, cx| {
            view.read(cx)
                .name_input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx)
        })
        .expect("focus form");
    assert!(view.read_with(cx, |view, cx| {
        view.models_input.read(cx).text() == "old/model-v1"
    }));
    assert!(
        window
            .update(cx, |_, window, cx| {
                view.read(cx)
                    .name_input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            })
            .expect("edit focuses provider name")
    );

    view.update(cx, |view, cx| {
        view.models_input.update(cx, |input, cx| {
            input.set_text("old/model-v1\nold/model-v1", cx)
        });
    });
    window
        .update(cx, |_, window, cx| {
            view.read(cx)
                .models_input
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        })
        .expect("focus invalid models");
    cx.simulate_keystrokes(window.into(), "cmd-enter");
    assert!(view.read_with(cx, |view, cx| {
        view.models_input.read(cx).text() == "old/model-v1\nold/model-v1"
            && view
                .error
                .as_deref()
                .is_some_and(|error| error.contains("重复"))
    }));

    view.update(cx, |view, cx| {
        view.base_url_input.update(cx, |input, cx| {
            input.set_text("https://legacy.invalid/v3", cx)
        });
        view.models_input
            .update(cx, |input, cx| input.set_text("new/model-v2", cx));
        view.key_input.update(cx, TextInput::clear);
    });
    cx.simulate_keystrokes(window.into(), "tab tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                view.read(cx).provider_save_focus.is_focused(window)
            })
            .expect("save focus")
    );
    cx.simulate_keystrokes(window.into(), "space");
    assert_eq!(save_count.load(Ordering::SeqCst), 1);
}

#[gpui_kit::test]
async fn provider_models_frame_reserves_rows_and_keeps_tail_visible(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    let root = view.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            let bounds = Bounds::centered(None, size(px(960.), px(628.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("provider models layout window");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let providers = visual
        .debug_bounds("settings-nav-providers")
        .expect("providers navigation");
    visual.simulate_click(providers.center(), Default::default());
    cx.run_until_parked();
    let bounds = |visual: &mut VisualTestContext, selector: &'static str| {
        visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing debug bounds for {selector}"))
    };
    let frame = bounds(&mut visual, "provider-models-input-frame");
    let help = bounds(&mut visual, "provider-models-input-help");
    let body_row = Typography::BODY * Typography::BODY_LINE_HEIGHT;
    let minimum_frame =
        PROVIDER_MODELS_MIN_ROWS as f32 * body_row + PROVIDER_MODELS_FRAME_INSET - 1.0;
    assert!(frame.size.height >= px(minimum_frame));
    assert!(help.origin.y >= frame.bottom());

    view.update(cx, |view, cx| {
        view.models_input.update(cx, |input, cx| {
            input.set_text("vendor/model-1.0\nmodel-flash", cx)
        });
    });
    cx.run_until_parked();
    let two_line_frame = bounds(&mut visual, "provider-models-input-frame");
    let two_line_help = bounds(&mut visual, "provider-models-input-help");
    assert!(two_line_frame.size.height >= px(minimum_frame));
    assert!(two_line_help.origin.y >= two_line_frame.bottom());

    view.update(cx, |view, cx| {
        view.models_input
            .update(cx, |input, cx| input.set_text("one\ntwo\nthree\nfour", cx));
    });
    cx.run_until_parked();
    let four_line_frame = bounds(&mut visual, "provider-models-input-frame");
    let four_line_help = bounds(&mut visual, "provider-models-input-help");
    assert!(four_line_frame.size.height > two_line_frame.size.height);
    assert!(four_line_help.origin.y >= four_line_frame.bottom());

    let five_line_text = "one\ntwo\nthree\nfour\nfive";
    view.update(cx, |view, cx| {
        view.models_input
            .update(cx, |input, cx| input.set_text(five_line_text, cx));
    });
    cx.run_until_parked();
    let five_line_frame = bounds(&mut visual, "provider-models-input-frame");
    let five_line_help = bounds(&mut visual, "provider-models-input-help");
    assert_eq!(five_line_frame.size.height, four_line_frame.size.height);
    assert!(five_line_help.origin.y >= five_line_frame.bottom());
    assert!(view.read_with(cx, |view, cx| {
        let input = view.models_input.read(cx);
        input.visible_rows() == PROVIDER_MODELS_MAX_ROWS
            && input.first_visible_row() > 0
            && input.text() == five_line_text
    }));
}

#[test]
fn permission_mode_accepts_only_the_fixed_set() {
    let mut config = AppConfig::default();
    for mode in PERMISSION_MODES {
        select_permission_mode(&mut config, mode).unwrap();
        assert_eq!(config.defaults.permission_mode, mode);
    }
    assert!(select_permission_mode(&mut config, "yolo").is_err());
    assert_eq!(config.defaults.permission_mode, "full_access");
}

#[test]
fn default_model_changes_are_applied() {
    let mut config = AppConfig::default();
    assert!(config.defaults.model.is_empty());
    set_default_model(&mut config, "deepseek-chat");
    assert_eq!(config.defaults.model, "deepseek-chat");
}

#[test]
fn all_models_unions_and_dedups_providers() {
    let providers = vec![
        provider("deepseek", &["deepseek-chat", "deepseek-reasoner"]),
        provider("openai", &["gpt", "deepseek-chat"]),
    ];
    assert_eq!(
        all_models(&providers),
        vec!["deepseek-chat", "deepseek-reasoner", "gpt"]
    );
    assert!(all_models(&[]).is_empty());
}

#[test]
fn pricing_request_input_cap_is_checked_before_event_retention() {
    let rates = PricingRateInputs {
        input_usd_per_million: "0".repeat(PRICING_INPUT_BYTES_LIMIT - 4),
        output_usd_per_million: "0".to_string(),
        cache_read_usd_per_million: "0".to_string(),
        cache_write_usd_per_million: "0".to_string(),
    };
    let exact = PricingMutation::AddCustom {
        model: "m".to_string(),
        rates: rates.clone(),
    };
    assert_eq!(
        pricing_mutation_input_bytes(&exact),
        Some(PRICING_INPUT_BYTES_LIMIT)
    );
    let over = PricingMutation::AddCustom {
        model: "mm".to_string(),
        rates,
    };
    assert_eq!(
        pricing_mutation_input_bytes(&over),
        Some(PRICING_INPUT_BYTES_LIMIT + 1)
    );
}

fn explicit_openai_profile(provider: &str, model: &str) -> ReasoningProfileProjection {
    ReasoningProfileProjection {
        provider: provider.to_string(),
        model: model.to_string(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        support: ReasoningSupport::Optional,
        efforts: vec!["low".into(), "high".into()],
        supports_disabled: true,
        disabled_wire: Some(ReasoningDisabledWire::ReasoningEffortNone),
        preserve_reasoning_content: false,
        preference: ReasoningChoice::ProviderDefault,
    }
}

#[gpui_kit::test]
async fn reasoning_template_is_explicit_and_failed_draft_keeps_exact_owner(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    let events = Arc::new(Mutex::new(Vec::<ReasoningProfileSaveRequested>::new()));
    let captured = events.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &ReasoningProfileSaveRequested, _| {
            captured
                .lock()
                .expect("reasoning events")
                .push(event.clone());
        })
        .detach();
    });
    let unknown_a = ReasoningProfileProjection::unknown("provider-a", "model-a");
    let unknown_b = ReasoningProfileProjection::unknown("provider-b", "model-b");
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 1,
                profiles: vec![unknown_a.clone(), unknown_b.clone()],
                error: None,
            },
            cx,
        );
        // This is the same operation the unknown row's template button emits.
        view.apply_reasoning_template(0, ReasoningTemplate::OpenAi, cx);
    });
    let first = events
        .lock()
        .expect("first reasoning event")
        .first()
        .cloned()
        .expect("template save event");
    assert_eq!(first.profile.provider, "provider-a");
    assert_eq!(first.profile.model, "model-a");
    assert_eq!(
        first.profile.protocol,
        ReasoningProtocol::OpenAiChatCompletions
    );
    assert_eq!(first.profile.support, ReasoningSupport::Optional);
    assert!(first.profile.supports_disabled);
    assert_eq!(first.profile.preference, ReasoningChoice::ProviderDefault);

    // A provider catalog refresh may temporarily project Loading while this
    // failed operation is being reconciled. The exact draft must survive that
    // projection so the user can retry the same owner instead of re-entering
    // all capability fields.
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(ReasoningSettingsProjection::Loading, cx);
        assert_eq!(
            view.reasoning_draft
                .as_ref()
                .map(|draft| (&draft.provider, &draft.model)),
            Some((&first.profile.provider, &first.profile.model))
        );
    });

    // A failed save leaves draft A visible. A later click on row B must use B
    // authority, even though the stale draft is still present in the view.
    let profile_b = explicit_openai_profile("provider-b", "model-b");
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 2,
                profiles: vec![unknown_a, profile_b],
                error: Some(ReasoningSettingsErrorCode::Conflict),
            },
            cx,
        );
        view.cycle_reasoning_preference(1, cx);
    });
    let events = events.lock().expect("all reasoning events");
    let second = events.last().expect("row B event");
    assert_eq!(second.base.provider, "provider-b");
    assert_eq!(second.base.model, "model-b");
    assert_eq!(second.profile.provider, "provider-b");
    assert_eq!(second.profile.model, "model-b");
}

#[gpui_kit::test]
async fn reasoning_unknown_template_is_tab_reachable_and_enter_activates(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, _| view.section = 2);
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 1,
                profiles: vec![ReasoningProfileProjection::unknown("provider-a", "model-a")],
                error: None,
            },
            cx,
        );
    });
    let events = Arc::new(Mutex::new(Vec::<ReasoningProfileSaveRequested>::new()));
    let captured = events.clone();
    let root = view.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &ReasoningProfileSaveRequested, _| {
            captured
                .lock()
                .expect("reasoning events")
                .push(event.clone());
        })
        .detach();
    });
    let closes = Arc::new(AtomicUsize::new(0));
    let harness_closes = closes.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            let bounds = Bounds::centered(None, size(px(960.), px(600.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_cx| SettingsHarness {
                        view: root,
                        closes: harness_closes,
                    })
                },
            )
        })
        .expect("settings window");
    cx.run_until_parked();
    window
        .update(cx, |_, window, cx| {
            let reload = view
                .read(cx)
                .reasoning_focus(&ReasoningFocusTarget::Reload)
                .expect("reasoning reload focus");
            reload.focus(window, cx);
        })
        .expect("focus reasoning reload");
    cx.simulate_keystrokes(window.into(), "tab enter");
    let event = events
        .lock()
        .expect("template event")
        .first()
        .cloned()
        .expect("keyboard template save");
    assert_eq!(event.profile.provider, "provider-a");
    assert_eq!(event.profile.model, "model-a");
    assert_eq!(
        event.profile.protocol,
        ReasoningProtocol::OpenAiChatCompletions
    );
    assert_eq!(closes.load(Ordering::SeqCst), 0);

    // Rehydrate the default projection and exercise the same route with
    // Space. The production app uses this projection after a save ack.
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 2,
                profiles: vec![ReasoningProfileProjection::unknown("provider-a", "model-a")],
                error: None,
            },
            cx,
        );
    });
    window
        .update(cx, |_, window, cx| {
            let reload = view
                .read(cx)
                .reasoning_focus(&ReasoningFocusTarget::Reload)
                .expect("rehydrated reasoning reload focus");
            reload.focus(window, cx);
        })
        .expect("refocus reasoning reload");
    cx.simulate_keystrokes(window.into(), "tab space");
    assert_eq!(
        events.lock().expect("template events").len(),
        2,
        "Space activates the same explicit template action"
    );
}

#[gpui_kit::test]
async fn pricing_actions_are_tab_reachable_and_enter_space_activate_once(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
        cx.bind_keys([KeyBinding::new(
            "escape",
            CloseSettings,
            Some("PricingSettings"),
        )]);
    });
    let long_cjk_model = format!("custom/{}", "模型-".repeat(20));
    let view = cx.new(SettingsView::new_for_test);
    cx.update(|cx| cx.set_global(PricingSettingsRequested(true)));
    view.update(cx, |view, cx| {
        view.apply_pricing_projection(
            PricingSettingsProjection::Ready {
                generation: 7,
                entries: vec![PricingEntryProjection {
                    model: long_cjk_model.clone(),
                    kind: PricingEntryKind::CustomStatic,
                    base: PricingRateInputs {
                        input_usd_per_million: "1".into(),
                        output_usd_per_million: "2".into(),
                        cache_read_usd_per_million: "3".into(),
                        cache_write_usd_per_million: "4".into(),
                    },
                    peak: None,
                }],
                notice: None,
                draft_reason: None,
                error: None,
            },
            cx,
        );
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let closes = Arc::new(AtomicUsize::new(0));
    let captured = events.clone();
    let root = view.clone();
    let projection_root = view.clone();
    let harness_closes = closes.clone();
    let window: WindowHandle<SettingsHarness> = cx
        .update(|cx| {
            let bounds = Bounds::centered(None, size(px(960.), px(600.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|cx| {
                        cx.subscribe(&root, move |_, _, event: &PricingMutationRequested, cx| {
                            let mut events = captured.lock().expect("pricing event capture");
                            events.push((event.generation, event.mutation.is_ok()));
                            let first = events.len() == 1;
                            drop(events);
                            if first {
                                projection_root.update(cx, |view, cx| {
                                    view.apply_pricing_projection(
                                        PricingSettingsProjection::Saving {
                                            generation: event.generation,
                                            entries: Vec::new(),
                                        },
                                        cx,
                                    );
                                });
                            }
                        })
                        .detach();
                        SettingsHarness {
                            view: root,
                            closes: harness_closes,
                        }
                    })
                },
            )
        })
        .expect("settings window");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.section), 3);
    assert!(!cx.update(|cx| cx.global::<PricingSettingsRequested>().0));
    assert_eq!(
        window
            .update(cx, |_, window, _| window.viewport_size())
            .expect("settings viewport"),
        size(px(960.), px(600.))
    );
    assert!(view.read_with(cx, |view, _| matches!(
        &view.pricing,
        PricingSettingsProjection::Ready { entries, .. }
            if entries.first().is_some_and(|entry| entry.model == long_cjk_model)
    )));
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::dark());
        cx.refresh_windows();
    });
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| cx.global::<vega_theme::Theme>().appearance),
        vega_theme::Appearance::Dark
    );
    window
        .update(cx, |_, window, cx| {
            let reload = view
                .read(cx)
                .pricing_focus(&PricingFocusTarget::Reload)
                .expect("reload focus");
            reload.focus(window, cx);
        })
        .expect("focus reload");
    cx.simulate_keystrokes(window.into(), "tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                view.read(cx)
                    .pricing_focus(&PricingFocusTarget::Add)
                    .is_some_and(|focus| focus.is_focused(window))
            })
            .expect("add focus")
    );
    cx.simulate_keystrokes(window.into(), "shift-tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                view.read(cx)
                    .pricing_focus(&PricingFocusTarget::Reload)
                    .is_some_and(|focus| focus.is_focused(window))
            })
            .expect("reload focused after shift-tab")
    );
    cx.simulate_keystrokes(window.into(), "escape");
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    cx.simulate_keystrokes(window.into(), "tab");
    cx.simulate_keystrokes(window.into(), "enter");
    assert!(view.read_with(cx, |view, _| view.pricing_editor.is_some()));

    window
        .update(cx, |_, window, cx| {
            let save = view
                .read(cx)
                .pricing_focus(&PricingFocusTarget::Save)
                .expect("save focus");
            save.focus(window, cx);
        })
        .expect("focus save");
    cx.simulate_keystrokes(window.into(), "space space");
    assert_eq!(*events.lock().expect("pricing events"), vec![(7, true)]);
}

#[gpui_kit::test]
async fn local_credentials_settings_save_and_runtime_read_share_owned_root(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().expect("owned root");
    let config_path = root.path().join("config.toml");
    let view = cx.new(|cx| SettingsView::from_path(Some(config_path.clone()), cx));
    view.update(cx, |view, cx| {
        view.name_input
            .update(cx, |input, cx| input.set_text("owned-provider", cx));
        view.base_url_input.update(cx, |input, cx| {
            input.set_text("https://owned.invalid/v1", cx)
        });
        view.models_input
            .update(cx, |input, cx| input.set_text("owned-model", cx));
        view.key_input
            .update(cx, |input, cx| input.set_text("owned-test-secret", cx));
        view.submit_provider(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, cx| {
        assert!(view.error.is_none());
        assert_eq!(view.available_key_refs, vec!["owned-provider"]);
        assert!(view.key_input.read(cx).text().is_empty());
    });
    let loaded = config::read_from(&config_path).expect("saved config");
    assert_eq!(
        keystore::get_key(config_path.parent().unwrap(), &loaded.providers[0].key_ref).unwrap(),
        "owned-test-secret"
    );
    assert!(
        !std::fs::read_to_string(&config_path)
            .unwrap()
            .contains("owned-test-secret")
    );
    let reopened = cx.new(|cx| SettingsView::from_path(Some(config_path.clone()), cx));
    reopened.read_with(cx, |view, _| {
        assert_eq!(view.available_key_refs, vec!["owned-provider"])
    });
    keystore::delete_key(root.path(), "owned-provider").unwrap();
    let missing = cx.new(|cx| SettingsView::from_path(Some(config_path), cx));
    missing.read_with(cx, |view, _| {
        assert_eq!(view.config.providers[0].key_ref, "owned-provider");
        assert!(
            view.available_key_refs.is_empty(),
            "key_ref is not evidence of storage"
        );
    });
}

#[gpui_kit::test]
async fn preference_worker_preserves_new_provider_fields_and_detects_same_field_conflict(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    let config = AppConfig {
        providers: vec![provider("owned", &["model"])],
        ..Default::default()
    };
    config.save_to(&path).unwrap();
    let view = cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx));
    config::update_from(&path, |config| {
        config.providers[0].enabled = false;
        config.providers[0].models.push("new-model".into());
        config.ui.sidebar_collapsed = true;
    })
    .unwrap();
    view.update(cx, |view, cx| view.select_mode("auto", cx));
    cx.run_until_parked();
    let saved = config::read_from(&path).unwrap();
    assert!(!saved.providers[0].enabled);
    assert_eq!(saved.providers[0].models, ["model", "new-model"]);
    assert!(saved.ui.sidebar_collapsed);
    assert_eq!(saved.defaults.permission_mode, "auto");
    config::update_from(&path, |config| {
        config.defaults.permission_mode = "readonly".into()
    })
    .unwrap();
    view.update(cx, |view, cx| view.select_mode("confirm", cx));
    cx.run_until_parked();
    assert_eq!(
        config::read_from(&path).unwrap().defaults.permission_mode,
        "readonly"
    );
    assert!(view.read_with(cx, |view, _| {
        view.error.as_ref().is_some_and(|e| e.contains("已更改"))
    }));
}

#[gpui_kit::test]
async fn provider_rename_retains_exact_form_baseline_and_rejects_collision(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    let mut original = provider("first", &["model-a"]);
    original.enabled = false;
    let original_ref = original.key_ref.clone();
    AppConfig {
        providers: vec![original.clone(), provider("second", &["model-b"])],
        ..Default::default()
    }
    .save_to(&path)
    .unwrap();
    let view = cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx));
    view.update(cx, |view, cx| {
        view.begin_edit_provider("first", cx);
        view.name_input
            .update(cx, |input, cx| input.set_text("second", cx));
        view.submit_provider(cx);
    });
    cx.run_until_parked();
    assert_eq!(config::read_from(&path).unwrap().providers[0], original);
    assert_eq!(
        config::read_from(&path).unwrap().providers[1].models,
        ["model-b"]
    );
    assert!(view.read_with(cx, |view, cx| view.error.is_some()
        && view.name_input.read(cx).text() == "second"));
    view.update(cx, |view, cx| {
        view.name_input
            .update(cx, |input, cx| input.set_text("renamed", cx));
        view.submit_provider(cx);
    });
    cx.run_until_parked();
    let saved = config::read_from(&path).unwrap();
    assert_eq!(saved.providers[0].name, "renamed");
    assert!(!saved.providers[0].enabled);
    assert_eq!(saved.providers[0].key_ref, original_ref);
    assert_eq!(saved.providers[1].models, ["model-b"]);
}
