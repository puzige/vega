use super::*;
use gpui_kit::{
    Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, size,
};
use std::sync::{Arc, Mutex};

struct Harness(Entity<SettingsView>);
impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}
fn model() -> CatalogModel {
    CatalogModel {
        id: "catalog-model".into(),
        name: "Catalog model".into(),
        canonical_id: Some("lab/canonical".into()),
        context: 200000,
        input: Some(150000),
        output: 50000,
        reasoning: Some(true),
        tool_call: Some(true),
        reasoning_options: vec![CatalogReasoningOption::Effort(vec![
            Some("low".into()),
            Some("high".into()),
        ])],
        cost: Some(vega_conversation::types::CatalogCost {
            input: Some("1".into()),
            output: Some("2".into()),
            cache_read: Some("0.1".into()),
            cache_write: Some("0.2".into()),
            complex: false,
        }),
        text_pricing: true,
        status: None,
    }
}
fn projection(model: CatalogModel) -> ModelCatalogProjection {
    ModelCatalogProjection {
        providers: Arc::new(vec![vega_conversation::types::CatalogProvider {
            id: "official".into(),
            name: "Official".into(),
            models: vec![model],
        }]),
        mapped: Some(CatalogIdentity {
            provider: "official".into(),
            model: "catalog-model".into(),
        }),
        fetched_at_ms: Some(1),
        schema_version: 1,
        ..Default::default()
    }
}
type Fixture = (
    Entity<SettingsView>,
    WindowHandle<Harness>,
    Arc<Mutex<Vec<ModelCatalogRequested>>>,
);
fn mount(cx: &mut TestAppContext) -> Fixture {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        crate::init(cx);
    });
    let view = cx.new(|cx| {
        SettingsView::from_config(
            AppConfig {
                providers: vec![ProviderConfig {
                    name: "relay".into(),
                    models: vec!["local-model".into()],
                    base_url: "https://example.invalid/v1".into(),
                    enabled: true,
                    api: Default::default(),
                    key_ref: "owned".into(),
                }],
                ..Default::default()
            },
            None,
            cx,
        )
    });
    let requests = Arc::new(Mutex::new(Vec::new()));
    cx.update(|cx| {
        let requests = requests.clone();
        cx.subscribe(&view, move |_, request: &ModelCatalogRequested, _| {
            requests.lock().unwrap().push(request.clone())
        })
        .detach();
        cx.subscribe(
            &view,
            move |view, request: &ModelContextLoadRequested, cx| {
                view.update(cx, |view, cx| {
                    view.apply_model_context_loaded(
                        request,
                        Ok(ModelContextLoaded {
                            policy: Some(ModelContextPolicy {
                                provider: "relay".into(),
                                model: "local-model".into(),
                                input_limit: Some(10000),
                                output_reserve: Some(1000),
                                automatic_compaction: false,
                                updated_at: 1,
                            }),
                            legacy_present: false,
                        }),
                        cx,
                    )
                });
            },
        )
        .detach();
    });
    view.update(cx, |view, cx| {
        view.open_model_settings("relay", "local-model", 0, cx);
    });
    let root = view.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1400.), px(2200.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Harness(root)),
            )
        })
        .unwrap();
    cx.run_until_parked();
    (view, window, requests)
}
fn click(cx: &mut TestAppContext, window: WindowHandle<Harness>, id: &'static str) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds(id)
        .unwrap_or_else(|| panic!("missing {id}"));
    visual.simulate_click(bounds.center(), Default::default());
    visual.run_until_parked();
}

#[gpui_kit::test]
async fn issue296_catalog_refresh_preserves_manual_values_and_only_explicit_input_is_applied(
    cx: &mut TestAppContext,
) {
    let (view, window, requests) = mount(cx);
    let saves = Arc::new(Mutex::new(Vec::new()));
    cx.update(|cx| {
        let saves = saves.clone();
        cx.subscribe(&view, move |_, request: &ModelContextSaveRequested, _| {
            saves.lock().unwrap().push(request.clone())
        })
        .detach();
    });
    let request = requests.lock().unwrap().last().cloned().unwrap();
    view.update(cx, |view, cx| {
        assert!(view.apply_model_catalog(&request, projection(model()), cx));
        assert!(!view.provider_has_draft(cx));
    });
    assert!(saves.lock().unwrap().is_empty());
    click(cx, window, "model-catalog-toggle");
    click(cx, window, "model-catalog-input");
    assert!(view.read_with(cx, |view, cx| view.provider_has_draft(cx)));
    let mut updated = model();
    updated.input = Some(99999);
    view.update(cx, |view, cx| {
        view.apply_model_catalog(&request, projection(updated), cx);
    });
    click(cx, window, "model-save");
    let saved = saves.lock().unwrap().last().cloned().unwrap();
    assert_eq!(
        (
            saved.input_limit,
            saved.output_limit,
            saved.automatic_compaction
        ),
        (Some(150000), Some(1000), false)
    );
    assert_eq!(
        (saved.provider.as_str(), saved.model.as_str()),
        ("relay", "local-model")
    );
}

#[gpui_kit::test]
async fn issue296_catalog_stale_owner_and_generation_cannot_replace_current_model(
    cx: &mut TestAppContext,
) {
    let (view, _, requests) = mount(cx);
    let request = requests.lock().unwrap().last().cloned().unwrap();
    view.update(cx, |view, cx| {
        view.request_model_catalog(CatalogAction::Refresh, cx);
        assert!(!view.apply_model_catalog(&request, projection(model()), cx));
        let newer = ModelCatalogRequested {
            generation: view.catalog.generation,
            ..request.clone()
        };
        assert!(view.apply_model_catalog(&newer, projection(model()), cx));
        view.request_settings_section(1, cx);
        assert!(!view.apply_model_catalog(&newer, projection(model()), cx));
        assert_eq!(view.config.providers[0].models, ["local-model"]);
    });
}

#[gpui_kit::test]
async fn issue296_catalog_price_import_uses_shared_local_model_and_original_save_event(
    cx: &mut TestAppContext,
) {
    let (view, window, requests) = mount(cx);
    let saves = Arc::new(Mutex::new(Vec::new()));
    cx.update(|cx| {
        let saves = saves.clone();
        cx.subscribe(&view, move |_, request: &PricingMutationRequested, _| {
            saves.lock().unwrap().push(request.mutation.clone())
        })
        .detach();
    });
    let request = requests.lock().unwrap().last().cloned().unwrap();
    view.update(cx, |view, cx| {
        view.apply_pricing_projection(
            PricingSettingsProjection::Ready {
                generation: 1,
                entries: vec![],
                error: None,
                draft_reason: None,
                notice: None,
            },
            cx,
        );
        view.apply_model_catalog(&request, projection(model()), cx);
    });
    click(cx, window, "model-catalog-toggle");
    click(cx, window, "model-catalog-prices");
    assert!(saves.lock().unwrap().is_empty());
    view.update(cx, |view, cx| {
        assert_eq!(view.section, 3);
        view.submit_pricing_editor(cx);
    });
    let saved = saves.lock().unwrap().last().cloned().unwrap();
    assert!(
        matches!(saved, Ok(PricingMutation::AddCustom { model, rates }) if model == "local-model" && rates.input_usd_per_million == "1" && rates.cache_write_usd_per_million == "0.2")
    );
}

#[test]
fn issue296_catalog_reasoning_and_prices_preserve_unsupported_semantics() {
    let mut model = model();
    assert_eq!(
        compatible_efforts(&model),
        Some(vec!["low".into(), "high".into()])
    );
    for value in [
        None,
        Some("default".into()),
        Some("none".into()),
        Some("unknown".into()),
    ] {
        model.reasoning_options = vec![CatalogReasoningOption::Effort(vec![value])];
        assert!(compatible_efforts(&model).is_none());
    }
    model.reasoning_options = vec![CatalogReasoningOption::Budget {
        min: Some(-1),
        max: Some(10000),
    }];
    assert!(compatible_efforts(&model).is_none());
    assert!(simple_prices(&model).is_some());
    model.cost.as_mut().unwrap().complex = true;
    assert!(simple_prices(&model).is_none());
    model.cost.as_mut().unwrap().complex = false;
    model.cost.as_mut().unwrap().cache_read = None;
    assert!(simple_prices(&model).is_none());
}

#[gpui_kit::test]
async fn issue296_catalog_effort_import_preserves_wire_preference_and_replay(
    cx: &mut TestAppContext,
) {
    let (view, window, requests) = mount(cx);
    let saves = Arc::new(Mutex::new(Vec::new()));
    cx.update(|cx| {
        let saves = saves.clone();
        cx.subscribe(
            &view,
            move |_, request: &ReasoningProfileSaveRequested, _| {
                saves.lock().unwrap().push(request.clone())
            },
        )
        .detach();
    });
    let request = requests.lock().unwrap().last().cloned().unwrap();
    let profile = ReasoningProfileProjection {
        provider: "relay".into(),
        model: "local-model".into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        support: ReasoningSupport::Optional,
        efforts: vec!["high".into()],
        supports_disabled: true,
        disabled_wire: Some(ReasoningDisabledWire::ReasoningEffortNone),
        replay_profile: vega_conversation::types::ReasoningReplaySetting::Auto,
        preference: ReasoningChoice::Effort("high".into()),
    };
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 1,
                profiles: vec![profile.clone()],
                error: None,
            },
            cx,
        );
        view.apply_model_catalog(&request, projection(model()), cx);
    });
    assert!(saves.lock().unwrap().is_empty());
    click(cx, window, "model-catalog-toggle");
    click(cx, window, "model-catalog-efforts");
    let saved = saves.lock().unwrap().last().cloned().unwrap();
    let mut expected = profile.clone();
    expected.efforts = vec!["low".into(), "high".into()];
    assert_eq!(saved.profile, expected);
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 2,
                profiles: vec![expected],
                error: None,
            },
            cx,
        );
    });
    click(cx, window, "model-settings-basic");
    click(cx, window, "model-catalog-efforts");
    assert_eq!(saves.lock().unwrap().len(), 1);
    let mut incompatible = model();
    incompatible.reasoning_options = vec![CatalogReasoningOption::Effort(vec![Some("low".into())])];
    view.update(cx, |view, cx| {
        view.apply_model_catalog(&request, projection(incompatible), cx);
    });
    click(cx, window, "model-catalog-efforts");
    assert_eq!(saves.lock().unwrap().len(), 1);
}

#[test]
fn issue296_catalog_prices_require_lossless_existing_decimal_range() {
    for value in ["1e-9", "0.0000001", "18446744073709551615"] {
        let mut model = model();
        model.cost.as_mut().unwrap().input = Some(value.into());
        assert!(simple_prices(&model).is_none());
    }
    let mut model = model();
    model.cost.as_mut().unwrap().input = Some("0".into());
    assert!(simple_prices(&model).is_some());
}

#[gpui_kit::test]
async fn issue296_catalog_failed_effort_save_retries_and_resumes_original_destination(
    cx: &mut TestAppContext,
) {
    let (view, window, requests) = mount(cx);
    let saves = Arc::new(Mutex::new(Vec::new()));
    let capacities = Arc::new(Mutex::new(Vec::new()));
    cx.update(|cx| {
        let saves = saves.clone();
        cx.subscribe(
            &view,
            move |_, request: &ReasoningProfileSaveRequested, _| {
                saves.lock().unwrap().push(request.clone())
            },
        )
        .detach();
        let capacities = capacities.clone();
        cx.subscribe(&view, move |_, request: &ModelContextSaveRequested, _| {
            capacities.lock().unwrap().push(request.clone())
        })
        .detach();
    });
    let request = requests.lock().unwrap().last().cloned().unwrap();
    let mut profile = ReasoningProfileProjection::unknown("relay", "local-model");
    profile.protocol = ReasoningProtocol::OpenAiChatCompletions;
    profile.support = ReasoningSupport::Optional;
    profile.efforts = vec!["high".into()];
    profile.preference = ReasoningChoice::Effort("high".into());
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 1,
                profiles: vec![profile.clone()],
                error: None,
            },
            cx,
        );
        view.apply_model_catalog(&request, projection(model()), cx);
    });
    click(cx, window, "model-catalog-toggle");
    click(cx, window, "model-catalog-efforts");
    let first = saves.lock().unwrap()[0].clone();
    assert_eq!(view.read_with(cx, |view, _| view.section), 2);
    click(cx, window, "model-settings-pricing");
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-unsaved-guard")
            .is_some()
    );
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 1,
                profiles: vec![profile],
                error: Some(ReasoningSettingsErrorCode::Io),
            },
            cx,
        );
    });
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("reasoning-save-error")
            .is_some()
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.reasoning_draft.clone()),
        Some(first.profile.clone())
    );
    click(cx, window, "settings-save-before-leave");
    let retried = saves.lock().unwrap()[1].clone();
    assert_eq!(retried.profile, first.profile);
    assert!(retried.operation_id > first.operation_id);
    assert!(capacities.lock().unwrap().is_empty());
    click(cx, window, "settings-discard-before-leave");
    assert_eq!(view.read_with(cx, |view, _| view.section), 2);
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 2,
                profiles: vec![retried.profile],
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.section, 3);
        assert!(view.pending_section.is_none());
        assert!(view.reasoning_draft.is_none());
        assert_eq!(
            view.model_settings.as_ref(),
            Some(&("relay".into(), "local-model".into()))
        );
    });
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-unsaved-guard")
            .is_none()
    );
}

#[gpui_kit::test]
async fn issue296_catalog_effort_apply_protects_existing_capacity_draft_and_help_is_opt_in(
    cx: &mut TestAppContext,
) {
    let (view, window, requests) = mount(cx);
    let request = requests.lock().unwrap().last().cloned().unwrap();
    let mut profile = ReasoningProfileProjection::unknown("relay", "local-model");
    profile.protocol = ReasoningProtocol::OpenAiChatCompletions;
    profile.support = ReasoningSupport::Optional;
    profile.efforts = vec!["high".into()];
    view.update(cx, |view, cx| {
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 1,
                profiles: vec![profile],
                error: None,
            },
            cx,
        );
        view.apply_model_catalog(&request, projection(model()), cx);
    });
    click(cx, window, "model-catalog-toggle");
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("model-catalog-source")
            .is_some()
    );
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-help-popup")
            .is_none()
    );
    click(cx, window, "model-catalog-help");
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-help-popup")
            .is_some()
    );
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-help-popup")
            .is_none()
    );
    assert!(
        window
            .update(cx, |_, window, cx| view
                .read(cx)
                .settings_help_focus
                .is_focused(window))
            .unwrap()
    );
    click(cx, window, "model-catalog-input");
    click(cx, window, "model-catalog-efforts");
    view.read_with(cx, |view, cx| {
        assert_eq!(view.section, 0);
        assert!(view.provider_has_draft(cx));
        assert!(view.reasoning_draft.is_none());
    });
}
