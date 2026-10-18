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
