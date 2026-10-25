use super::*;

impl ThreadsBlock {
    pub(super) fn section_label(&self, text: &str, cx: &App) -> AnyElement {
        div()
            .px_2()
            .py_1()
            .text_size(px(Typography::METADATA))
            .font_weight(Typography::HEADING_CARD_WEIGHT)
            .text_color(theme(cx).colors.text_tertiary)
            .child(text.to_string())
            .into_any_element()
    }
}

pub(super) fn sorted_threads(
    threads: &[Thread],
    preferences: &vega_conversation::types::SidebarPreferences,
) -> Vec<Thread> {
    service::sorted_threads(threads, preferences)
}
