use crate::icons::{Icon, icon};
use gpui_kit::{AnyElement, App, FocusHandle, SharedString, Window, div, prelude::*, px};
use vega_theme::{Layout, ThemeColors};

pub(crate) fn focus(id: SharedString, window: &mut Window, cx: &mut App) -> FocusHandle {
    window
        .use_keyed_state(id, cx, |_, cx| cx.focus_handle().tab_stop(true))
        .read(cx)
        .clone()
}

pub(crate) fn chevron(
    group: SharedString,
    selector: String,
    focus: &FocusHandle,
    expanded: bool,
    enabled: bool,
    colors: ThemeColors,
    window: &Window,
) -> AnyElement {
    #[cfg(test)]
    let probe_selector = format!("{selector}-paint");
    let glyph = div().relative().child(icon(
        if expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        colors.text_tertiary,
    ));
    #[cfg(test)]
    let glyph = glyph.child(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size(px(1.))
            .bg(gpui_kit::rgba(0x12fe34ff))
            .debug_selector(move || probe_selector.clone()),
    );
    div()
        .debug_selector(move || selector.clone())
        .w(px(Layout::DISCLOSURE_TRAILING_SLOT))
        .h(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .opacity(
                    if enabled && focus.is_focused(window) && window.last_input_was_keyboard() {
                        1.
                    } else {
                        0.
                    },
                )
                .when(enabled, |glyph| {
                    glyph.group_hover(group, |style| style.opacity(1.))
                })
                .child(glyph),
        )
        .into_any_element()
}
