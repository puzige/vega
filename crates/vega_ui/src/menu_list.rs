use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, Entity, MouseButton, MouseUpEvent, SharedString, div, px,
};
use vega_theme::{ThemeColors, Typography};

use crate::icons::{Icon, icon};
use crate::interaction::{Interaction, InteractionExt};
use crate::text_input::TextInput;

pub const MENU_ROW_HEIGHT: f32 = Typography::SIDEBAR_LINE_HEIGHT;

pub const MENU_ROW_PADDING_X: f32 = 10.0;

pub fn selected_row_bg(colors: ThemeColors) -> gpui_kit::Rgba {
    colors.bg_active_alpha
}

pub fn separator(colors: ThemeColors) -> AnyElement {
    div()
        .flex_shrink_0()
        .mx_2()
        .my_1()
        .h(px(1.))
        .bg(colors.border_subtle)
        .into_any_element()
}

pub fn search_field(input: &Entity<TextInput>, colors: ThemeColors) -> AnyElement {
    div()
        .flex_shrink_0()
        .mx_1()
        .mb_1()
        .h(px(MENU_ROW_HEIGHT))
        .px(px(MENU_ROW_PADDING_X))
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .text_size(px(Typography::SIDEBAR))
        .child(icon(Icon::Search, colors.text_tertiary))
        .child(div().flex_1().min_w_0().child(input.clone()))
        .into_any_element()
}

pub fn action_row(
    selector: &'static str,
    glyph: Icon,
    label: SharedString,
    enabled: bool,
    disabled_hint: &'static str,
    colors: ThemeColors,
    activate: impl Fn(&MouseUpEvent, &mut gpui_kit::Window, &mut App) + 'static,
) -> AnyElement {
    let ink = if enabled {
        colors.text_primary
    } else {
        colors.text_tertiary
    };
    let row = div()
        .id(selector)
        .h(px(MENU_ROW_HEIGHT))
        .flex_shrink_0()
        .mx_1()
        .px(px(MENU_ROW_PADDING_X))
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .text_size(px(Typography::SIDEBAR))
        .text_color(ink)
        .child(icon(glyph, ink))
        .child(div().min_w_0().truncate().child(label));
    if !enabled {
        return row
            .tooltip(move |_, cx| crate::icons::tooltip(disabled_hint, cx))
            .into_any_element();
    }
    row.cursor_pointer()
        .interactive(colors, true, false)
        .on_mouse_up(MouseButton::Left, activate)
        .into_any_element()
}

pub fn row_container(
    id: impl Into<ElementId>,
    selected: bool,
    enabled: bool,
    colors: ThemeColors,
) -> Interaction {
    div()
        .id(id.into())
        .h(px(MENU_ROW_HEIGHT))
        .flex_shrink_0()
        .min_w_0()
        .overflow_hidden()
        .mx_1()
        .px(px(MENU_ROW_PADDING_X))
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .text_size(px(Typography::SIDEBAR))
        .interaction_surface(
            if selected {
                selected_row_bg(colors)
            } else {
                gpui_kit::Rgba {
                    a: 0.,
                    ..colors.bg_hover
                }
            },
            if selected {
                selected_row_bg(colors)
            } else {
                colors.bg_hover
            },
            colors.bg_active,
            enabled,
        )
        .when(enabled, |row| row.cursor_pointer())
}

pub fn selection_marker(selected: bool, colors: ThemeColors) -> AnyElement {
    div()
        .w(px(Typography::SIDEBAR))
        .flex_shrink_0()
        .flex()
        .justify_end()
        .when(selected, |marker| {
            marker.child(icon(Icon::Check, colors.brand_primary))
        })
        .into_any_element()
}
