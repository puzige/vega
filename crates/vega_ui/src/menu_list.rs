//! R62 R10: shared list chrome for the composer's two utility-bar dropdowns.
//!
//! Both dropdowns (project and branch) are the same list shape — a search
//! field on top, icon-bearing rows, a checkmark and a light-grey rounded
//! surface on the current selection, and a separator before the trailing
//! actions. The reference implementation renders them from one list
//! component, so the pieces they share live here rather than being spelled
//! twice and drifting.
//!
//! Nothing in this module owns state or decides behaviour: it only builds
//! elements from the caller's entities and colors.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, Entity, MouseButton, MouseUpEvent, SharedString, div, px,
};
use vega_theme::{ThemeColors, Typography};

use crate::icons::{Icon, icon};
use crate::text_input::TextInput;

/// Height of one dropdown row. Both dropdowns already use
/// [`Typography::SIDEBAR_LINE_HEIGHT`] for their rows; the constant is named
/// here so the shared pieces (search field, separator, action rows) agree
/// with them by construction.
///
/// R68 R7 (**withdrawn**) / R11: this stays 32. R68 v1 asked for 36 from the
/// reference's *browser* CSS token; measuring the Codex desktop screenshot
/// shows the reference row is 28.5 logical, i.e. **shorter** than Vega's 32,
/// so raising it would be a reverse optimisation. R68 R11 freezes it.
pub const MENU_ROW_HEIGHT: f32 = Typography::SIDEBAR_LINE_HEIGHT;

/// R68 R8: the horizontal padding one dropdown row spends between its fill
/// edge and its first/last child (the icon / the selection marker).
///
/// The Codex desktop screenshot measures 9.5 logical here; the reference's
/// own `--menu-item-padding` horizontal component is 10. Two independent
/// measurements agree on 10, so 10 it is. The 4px gap between the card edge
/// and the row fill (`mx_1` below) is **not** part of this: the reference
/// measures 4.5 there, which is inside the 1px error band, and R8 leaves it
/// alone.
///
/// R68 R10: every row-shaped piece in this module reads the constant rather
/// than spelling `10` — the two dropdowns share this chrome, so a change has
/// to reach both at once.
pub const MENU_ROW_PADDING_X: f32 = 10.0;

/// The rounded light-grey surface the current selection carries (R62 R10).
///
/// `bg_active_alpha` is the shared "5% foreground ink over the current
/// surface" token, so the selection reads correctly on the dropdown's
/// `bg_elevated` panel in both themes instead of reusing the sidebar's
/// pre-composited `bg_active`.
pub fn selected_row_bg(colors: ThemeColors) -> gpui_kit::Rgba {
    colors.bg_active_alpha
}

/// One 1px separator between the list's groups (R62 R10).
pub fn separator(colors: ThemeColors) -> AnyElement {
    div()
        .flex_shrink_0()
        .mx_2()
        .my_1()
        .h(px(1.))
        .bg(colors.border_subtle)
        .into_any_element()
}

/// The magnifier + editable query row at the top of a dropdown (R62 R10).
///
/// The input is mounted with [`TextInput::with_bare_chrome`] so the field
/// does not render a second box inside it. The caller owns the entity and
/// decides what the query filters.
///
/// R68 R9: the row paints **no** `bg_hover` fill of its own. Vega drew a grey
/// pill behind the magnifier; the reference (verified against the Codex
/// screenshot crop) has just the magnifier and the placeholder sitting
/// directly on the card's `bg_elevated` surface, and the extra pill was one
/// of the most visible differences. Removing it is why the popup now reads
/// lighter. The height is still [`MENU_ROW_HEIGHT`] and the horizontal
/// padding still [`MENU_ROW_PADDING_X`] (R8/R10), so the row's geometry is
/// unchanged by the fill removal — only its paint is.
pub fn search_field(
    input: &Entity<TextInput>,
    selector: &'static str,
    colors: ThemeColors,
) -> AnyElement {
    div()
        .debug_selector(move || selector.to_string())
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

/// One trailing action row (`+ 新建项目` / `× 不关联项目` / `+ 新建并切换分支`).
///
/// `enabled == false` renders the row disabled: it keeps its geometry and
/// stays readable, but carries no pointer cursor and no click handler, so a
/// click cannot silently do nothing while looking live (R62 R11). The disabled
/// row also carries a tooltip naming the reason, because the design guidelines
/// require a disabled control to explain itself rather than only dimming.
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
        .debug_selector(move || selector.to_string())
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
        .hover(move |style| style.bg(colors.bg_hover))
        .on_mouse_up(MouseButton::Left, activate)
        .into_any_element()
}

/// One list row's shared geometry: fixed height, one-line label, and the
/// selection surface. The leading icon, label and trailing marker stay with
/// the caller because the two dropdowns source them differently (projects
/// from the store, branches from a virtualized Git projection).
///
/// R68 R8/R10: the horizontal padding is [`MENU_ROW_PADDING_X`] here rather
/// than a literal, because this is the one place both dropdowns' rows get
/// their inset from. The `mx_1` card-edge gutter is deliberately untouched
/// (see the constant's doc comment).
pub fn row_container(
    id: impl Into<ElementId>,
    selected: bool,
    enabled: bool,
    colors: ThemeColors,
) -> gpui_kit::Stateful<gpui_kit::Div> {
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
        .when(selected, move |row| row.bg(selected_row_bg(colors)))
        .when(enabled, |row| {
            row.cursor_pointer()
                .hover(move |style| style.bg(colors.bg_hover))
        })
}

/// The trailing selection marker column: the checkmark exists only on the
/// selected row, and the column reserves the same width either way so every
/// label keeps one x axis (R62 R10).
///
/// The selector is a closure because the two dropdowns derive their row
/// selectors differently (project ids are strings, branch rows are indexed).
pub fn selection_marker(
    selected: bool,
    selector: impl FnOnce() -> String,
    colors: ThemeColors,
) -> AnyElement {
    div()
        .w(px(Typography::SIDEBAR))
        .flex_shrink_0()
        .flex()
        .justify_end()
        .when(selected, |marker| {
            marker
                .debug_selector(selector)
                .child(icon(Icon::Check, colors.brand_primary))
        })
        .into_any_element()
}
