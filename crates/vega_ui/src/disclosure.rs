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
    focus: &FocusHandle,
    expanded: bool,
    enabled: bool,
    colors: ThemeColors,
    window: &Window,
) -> AnyElement {
    #[cfg(test)]
    let _paint_scope =
        crate::disclosure_paint_probe::scope(group.clone(), expanded, enabled, focus);
    let glyph = div().relative().child(icon(
        if expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        colors.text_tertiary,
    ));
    div()
        .w(px(Layout::DISCLOSURE_TRAILING_SLOT))
        .h(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .opacity(
                    if enabled
                        && (expanded
                            || (focus.is_focused(window) && window.last_input_was_keyboard()))
                    {
                        1.
                    } else {
                        0.
                    },
                )
                .when(enabled, |glyph| {
                    glyph.group_hover(group, |style| style.opacity(1.))
                })
                .child(glyph)
                .map(|glyph| {
                    #[cfg(test)]
                    let glyph = crate::disclosure_paint_probe::observe_visibility(glyph);
                    glyph
                }),
        )
        .into_any_element()
}

pub(crate) fn inline_chevron(
    group: SharedString,
    focus: &FocusHandle,
    expanded: bool,
    enabled: bool,
    colors: ThemeColors,
    window: &Window,
) -> AnyElement {
    #[cfg(test)]
    let _paint_scope =
        crate::disclosure_paint_probe::scope(group.clone(), expanded, enabled, focus);
    let glyph = div().relative().child(crate::icons::sized_icon(
        if expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        colors.conversation_tool,
        Layout::CONVERSATION_CHEVRON_SIZE,
    ));
    div()
        .size(px(Layout::CONVERSATION_CHEVRON_SIZE))
        .flex_shrink_0()
        .child(
            div()
                .opacity(
                    if enabled
                        && (expanded
                            || (focus.is_focused(window) && window.last_input_was_keyboard()))
                    {
                        1.
                    } else {
                        0.
                    },
                )
                .when(enabled, |glyph| {
                    glyph.group_hover(group, |style| style.opacity(1.))
                })
                .child(glyph)
                .map(|glyph| {
                    #[cfg(test)]
                    let glyph = crate::disclosure_paint_probe::observe_visibility(glyph);
                    glyph
                }),
        )
        .into_any_element()
}
