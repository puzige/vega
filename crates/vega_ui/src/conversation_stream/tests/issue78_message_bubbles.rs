use super::*;

struct EntryView(StreamEntry);
impl Render for EntryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().w_full().child(render_entry(&self.0, window, cx))
    }
}
