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
