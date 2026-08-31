use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gpui::{
    Bounds, KeyBinding, Render, TestAppContext, WindowBounds, WindowHandle, WindowOptions, size,
};

use super::*;
use crate::settings::state::PRICING_INPUT_BYTES_LIMIT;

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
        name: name.to_string(),
        base_url: format!("https://{name}.example.com"),
        models: models.iter().map(|m| m.to_string()).collect(),
        key_ref: name.to_string(),
    }
}

#[test]
fn form_rejects_empty_fields() {
    assert!(!form_is_submittable("", "https://x", "k"));
    assert!(!form_is_submittable("n", "", "k"));
    assert!(!form_is_submittable("n", "https://x", ""));
    // 空白 name 视为空。
    assert!(!form_is_submittable("   ", "https://x", "k"));
    assert!(form_is_submittable("n", "https://x", "k"));
}

#[test]
fn upsert_appends_new_and_updates_same_name() {
    let mut providers = vec![provider("deepseek", &["deepseek-chat"])];
    // 异名追加。
    assert!(!upsert_provider(&mut providers, provider("openai", &[])));
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[1].name, "openai");
    // 同名更新：表单字段刷新，models 保留。
    let mut replacement = provider("deepseek", &[]);
    replacement.base_url = "https://api.deepseek.com/v1".to_string();
    assert!(upsert_provider(&mut providers, replacement));
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0].base_url, "https://api.deepseek.com/v1");
    assert_eq!(providers[0].models, vec!["deepseek-chat"]);
}

#[test]
fn permission_mode_accepts_only_the_fixed_set() {
    let mut config = AppConfig::default();
    for mode in PERMISSION_MODES {
        select_permission_mode(&mut config, mode).unwrap();
        assert_eq!(config.defaults.permission_mode, mode);
    }
    assert!(select_permission_mode(&mut config, "yolo").is_err());
    assert_eq!(config.defaults.permission_mode, "auto");
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

#[gpui::test]
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
