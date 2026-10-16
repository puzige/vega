use super::*;
use gpui_kit::{TestAppContext, VisualTestContext};

struct Harness(Entity<ThinkingBlock>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .w_full()
            .child(self.0.clone())
    }
}
