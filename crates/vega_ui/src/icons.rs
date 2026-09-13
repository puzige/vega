//! Shared functional icons for Vega's compact native chrome.

//! Generic UI symbols come from the embedded GPUI Kit icon set. The source
//! assets are Lucide-style SVGs with a 24px viewBox and round line joins; the
//! wrapper below keeps every icon on Vega's fixed 16px optical grid. Keeping
//! the renderer SVG-backed avoids fractional `PathBuilder` geometry at small
//! sizes. The two symbols that are not present in the bundled set use the
//! same Lucide SVG grammar through GPUI's inline SVG renderer.

use gpui_kit::{
    AnyElement, IntoElement, Rgba,
    component::{Icon as KitIcon, IconName, Sizable as _},
    prelude::*,
    px, svg,
};

const ICON_SIZE: f32 = 16.0;

/// The small set of functional symbols shared by Vega's native chrome.
#[derive(Clone, Copy)]
pub enum Icon {
    Search,
    Sidebar,
    Plus,
    ArrowUp,
    ArrowLeft,
    ArrowRight,
    Folder,
    FolderOpen,
    Settings,
    Close,
    DockBottom,
    DockRight,
    DockMove,
    Maximize,
    Minimize,
    More,
    ArrowUpDown,
    Pin,
    ChevronDown,
    ChevronRight,
    FolderPlus,
    ArrowDown,
    Refresh,
    Split,
    Terminal,
    Warning,
    Document,
    Summary,
}

/// Lucide's pin path is kept inline because gpui-kit 0.6.0 does not ship a
/// pin asset. It is rendered by GPUI's normal SVG pipeline, rather than by a
/// bespoke canvas path.
const PIN_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 17v5"/><path d="M5 17h14"/><path d="M8 17V7a4 4 0 0 1 8 0v10"/><path d="M6 7h12"/></svg>"#;

/// Lucide's folder-plus silhouette is kept as one SVG so the add affordance
/// remains legible at 16px without relying on two independently laid out
/// elements.
const FOLDER_PLUS_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 10v6"/><path d="M9 13h6"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/></svg>"#;

/// Lucide's list silhouette is kept inline because gpui-kit 0.6.0 does not
/// ship a list asset. Three rules with three leading dots read as a summary
/// surface at 16px and reuse the same 24px, round-corner grammar as the
/// embedded icon set.
const SUMMARY_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h.01"/><path d="M8 6h13"/><path d="M3 12h.01"/><path d="M8 12h13"/><path d="M3 18h.01"/><path d="M8 18h13"/></svg>"#;

/// Vega's pane-move glyph (R47 §2.2) is kept inline because the bundled icon
/// set only offers the shell-slot dock shapes (panel + bottom/right bar), and
/// reusing one of them for the pane-local "move dock" action read as the same
/// control twice in one header. The glyph is a panel rectangle offset toward
/// the trailing edge plus one arrow entering it from outside, on the same
/// 24px viewBox, stroke-2, currentColor grammar as the other inline icons. At
/// 16px it stays clearly distinct from `DockBottom` (rect + bottom bar) and
/// `DockRight` (rect + right bar): neither carries an arrow.
const DOCK_MOVE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="5" width="14" height="14" rx="2"/><path d="M2 12h9"/><path d="m8 9 3 3-3 3"/></svg>"#;

fn icon_name(kind: Icon) -> IconName {
    match kind {
        Icon::Search => IconName::Search,
        Icon::Sidebar => IconName::PanelLeft,
        Icon::Plus => IconName::Plus,
        Icon::ArrowUp => IconName::ArrowUp,
        Icon::ArrowLeft => IconName::ArrowLeft,
        Icon::ArrowRight => IconName::ArrowRight,
        Icon::Folder => IconName::Folder,
        Icon::FolderOpen => IconName::FolderOpen,
        Icon::FolderPlus => unreachable!("inline SVG icons are handled before mapping"),
        Icon::Settings => IconName::Settings,
        Icon::Close => IconName::Close,
        Icon::DockBottom => IconName::PanelBottom,
        Icon::DockRight => IconName::PanelRight,
        Icon::Maximize => IconName::Maximize,
        Icon::Minimize => IconName::Minimize,
        Icon::More => IconName::Ellipsis,
        Icon::ArrowUpDown => IconName::ChevronsUpDown,
        Icon::ChevronDown => IconName::ChevronDown,
        Icon::ChevronRight => IconName::ChevronRight,
        Icon::ArrowDown => IconName::ArrowDown,
        Icon::Refresh => IconName::RotateCw,
        // A panel outline is the closest mature symbol for a split view and
        // preserves the same visual language as the dock controls.
        Icon::Split => IconName::PanelLeft,
        Icon::Terminal => IconName::SquareTerminal,
        Icon::Warning => IconName::TriangleAlert,
        Icon::Document => IconName::FileText,
        Icon::Pin | Icon::Summary | Icon::DockMove => {
            unreachable!("inline SVG icons are handled before mapping")
        }
    }
}

fn kit_icon(kind: Icon, color: Rgba) -> AnyElement {
    KitIcon::new(icon_name(kind))
        .with_size(px(ICON_SIZE))
        .text_color(color)
        .flex_shrink_0()
        .into_any_element()
}

fn inline_icon(data: &'static [u8], color: Rgba) -> AnyElement {
    svg()
        .data(data)
        .size(px(ICON_SIZE))
        .flex_shrink_0()
        .text_color(color)
        .into_any_element()
}

/// Paints a 16px icon in the caller's semantic text color.
pub fn icon(kind: Icon, color: Rgba) -> AnyElement {
    match kind {
        Icon::Pin => inline_icon(PIN_SVG, color),
        Icon::FolderPlus => inline_icon(FOLDER_PLUS_SVG, color),
        Icon::Summary => inline_icon(SUMMARY_SVG, color),
        Icon::DockMove => inline_icon(DOCK_MOVE_SVG, color),
        _ => kit_icon(kind, color),
    }
}

/// Native tooltip body: a label plus an optional shortcut keycap chip. The
/// chip reuses existing semantic tokens (`bg_hover`, `border_subtle`,
/// `METADATA`, `text_secondary`) so no new color or font size is introduced.
struct IconTooltip(gpui_kit::SharedString, Option<gpui_kit::SharedString>);
impl gpui_kit::Render for IconTooltip {
    fn render(
        &mut self,
        _: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        let colors = vega_theme::theme(cx).colors;
        let shortcut = self.1.clone();
        gpui_kit::div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(colors.bg_elevated)
            .border_1()
            .border_color(colors.border_subtle)
            .text_size(px(vega_theme::Typography::METADATA))
            .text_color(colors.text_primary)
            .child(self.0.clone())
            .children(shortcut.map(|shortcut| {
                gpui_kit::div()
                    .px_1()
                    .rounded_md()
                    .bg(colors.bg_hover)
                    .border_1()
                    .border_color(colors.border_subtle)
                    .text_size(px(vega_theme::Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(shortcut)
            }))
    }
}

/// Compact labeled control with a native tooltip and Enter/Space activation.
pub fn icon_button(
    kind: Icon,
    label: impl Into<gpui_kit::SharedString>,
    colors: vega_theme::ThemeColors,
    activate: impl Fn(&(), &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    use gpui_kit::{MouseButton, div};
    let label = label.into();
    let tooltip_label = label.clone();
    let accessible_label = label.clone();
    let activate = std::rc::Rc::new(activate);
    let keyboard = activate.clone();
    div()
        .id(label)
        .aria_label(accessible_label)
        .focusable()
        .tab_stop(true)
        .size(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .hover(move |style| style.bg(colors.bg_hover))
        .focus(move |style| style.bg(colors.bg_active))
        .tooltip(move |_, cx| cx.new(|_| IconTooltip(tooltip_label.clone(), None)).into())
        .on_mouse_up(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            activate(&(), window, cx);
        })
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                keyboard(&(), window, cx);
            }
        })
        .child(icon(kind, colors.text_secondary))
}

/// Builds a native tooltip using the current theme.
pub fn tooltip(
    label: impl Into<gpui_kit::SharedString>,
    cx: &mut gpui_kit::App,
) -> gpui_kit::AnyView {
    let label = label.into();
    cx.new(|_| IconTooltip(label, None)).into()
}

/// Permanent 28x28 shell control (R45 main-header slots): a 16px centered
/// icon on the frozen titlebar hitbox with a persistent selected surface,
/// hover only while unselected, a disabled presentation that keeps its
/// geometry, and a tooltip that can carry a shortcut keycap. Selection wins
/// over hover: the hover surface is only attached while unselected, so the
/// two states can never be confused.
pub fn shell_icon_button(
    kind: Icon,
    label: impl Into<gpui_kit::SharedString>,
    shortcut: Option<gpui_kit::SharedString>,
    selected: bool,
    enabled: bool,
    colors: vega_theme::ThemeColors,
    activate: impl Fn(&(), &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    use gpui_kit::{MouseButton, div};
    let label = label.into();
    let tooltip_label = label.clone();
    let accessible_label = label;
    let activate = std::rc::Rc::new(activate);
    let keyboard = activate.clone();
    div()
        .id(tooltip_label.clone())
        .aria_label(accessible_label)
        .focusable()
        .tab_stop(enabled)
        .size(px(vega_theme::Layout::TITLEBAR_CONTROL_SIZE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .when(selected, |button| button.bg(colors.bg_active))
        .when(enabled && !selected, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(colors.bg_hover))
                .focus(move |style| style.bg(colors.bg_active))
        })
        .tooltip(move |_, cx| {
            cx.new(|_| IconTooltip(tooltip_label.clone(), shortcut.clone()))
                .into()
        })
        .on_mouse_up(MouseButton::Left, move |_, window, cx| {
            if enabled {
                cx.stop_propagation();
                activate(&(), window, cx);
            }
        })
        .on_key_down(move |event, window, cx| {
            if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                keyboard(&(), window, cx);
            }
        })
        .child(icon(
            kind,
            if enabled {
                colors.text_secondary
            } else {
                colors.text_tertiary
            },
        ))
}
