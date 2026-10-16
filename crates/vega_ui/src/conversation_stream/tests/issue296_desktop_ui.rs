use super::*;
use gpui_kit::{Modifiers, VisualTestContext, size};

fn history() -> HistoryPage {
    hydration_page(
        (1..=60)
            .map(|index| {
                if index % 2 == 0 {
                    hydration_assistant(
                        index,
                        "## Result\n\nReadable answer.\n\n```rust\nlet value = 1;\n```\n",
                    )
                } else {
                    hydration_user(index, "A user request with enough context to read.")
                }
            })
            .collect(),
        None,
    )
}

struct MessageFixture {
    user: StreamEntry,
    assistant: StreamEntry,
    focus: Option<FocusHandle>,
}

impl Render for MessageFixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render = |entry: &StreamEntry, window: &mut Window, cx: &mut App| {
            if let Some(focus) = &self.focus {
                render_entry_with_selection(entry, window, cx, focus)
            } else {
                render_entry(entry, window, cx)
            }
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(render(&self.user, window, cx))
            .child(render(&self.assistant, window, cx))
    }
}
