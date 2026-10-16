use super::*;
use gpui_kit::{
    Bounds, Context, Render, TestAppContext, VisualTestContext, Window, WindowBounds, WindowHandle,
    WindowOptions, size,
};

struct AssetHarness(Entity<ToolCard>);

impl Render for AssetHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(ToolCard::render(
            self.0.clone(),
            "s14-asset".into(),
            false,
            window,
            cx,
        ))
    }
}
