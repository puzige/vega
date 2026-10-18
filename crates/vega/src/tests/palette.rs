use super::*;

struct PaletteHarness {
    root: Entity<VegaWindow>,
}
impl Render for PaletteHarness {
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        self.root.clone()
    }
}
