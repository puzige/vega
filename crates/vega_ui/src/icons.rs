//! Shared functional icons for Vega's compact native chrome.

//! Generic UI symbols come from the embedded GPUI Kit icon set. The source
//! assets are Lucide-style SVGs with a 24px viewBox and round line joins; the
//! wrapper below keeps every icon on Vega's fixed 16px optical grid. Keeping
//! the renderer SVG-backed avoids fractional `PathBuilder` geometry at small

use gpui_kit::{
    AnyElement, IntoElement, Rgba,
    component::{Icon as KitIcon, IconName, Sizable as _},
    prelude::*,
    px, svg,
};

use crate::interaction::{Interaction, InteractionExt};
use vega_theme::Layout;

/// The small set of functional symbols shared by Vega's native chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Copy,
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
    Queue,
    Steer,
    Trash,
    Goal,
    Pause,
    Play,
    Connection,
    Wifi,
    Plug,
    Bot,
    Blocks,
    Chart,
    ArrowUpDown,
    GitBranch,
    Pin,
    ChevronDown,
    ChevronRight,
    FolderPlus,
    ArrowDown,
    Refresh,
    Split,
    Terminal,
    Warning,
    Help,
    Document,
    BookOpen,
    Pencil,
    Wrench,
    Monitor,
    Camera,
    MousePointer,
    Keyboard,
    Globe,
    Summary,
    /// Selection marker for menu rows (R62 R8/R10).
    Check,
    /// R62 R8: the "ask for approval" permission row.
    Hand,
    /// R62 R8: the "approve for me" permission row.
    Shield,
    /// #117: context compaction. Lucide's `text-select` silhouette is the
    /// reference product's own compaction marker; one glyph covers every
    /// compaction state, so the row never swaps shape as the status changes.
    TextSelect,
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

const CHART_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3v16a2 2 0 0 0 2 2h16M8 16v-5m5 5V7m5 9V3"/></svg>"#;
const PLUG_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 3v5m6-5v5M7 8h10v4a5 5 0 0 1-10 0V8Zm5 9v4"/></svg>"#;
const WIFI_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h.01"/><path d="M2 8.82a15 15 0 0 1 20 0"/><path d="M5 12.86a10 10 0 0 1 14 0"/><path d="M8.5 16.43a5 5 0 0 1 7 0"/></svg>"#;
const BLOCKS_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="2"/><rect x="3" y="14" width="7" height="7" rx="2"/><rect x="14" y="14" width="7" height="7" rx="2"/><path d="M14 6.5h7m-3.5-3.5v7"/></svg>"#;
const PENCIL_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 5l4 4M4 16 16 4a2.83 2.83 0 0 1 4 4L8 20l-5 1 1-5Z"/></svg>"#;
const WRENCH_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94L14.7 6.3Z"/></svg>"#;
const MONITOR_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg>"#;
const CAMERA_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3Z"/><circle cx="12" cy="13" r="3"/></svg>"#;
const MOUSE_POINTER_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3 3 7.07 17 2.51-7.39L20 10.07 3 3Z"/></svg>"#;
const KEYBOARD_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="5" width="20" height="14" rx="2"/><path d="M6 9h.01M10 9h.01M14 9h.01M18 9h.01M6 12h.01M10 12h.01M14 12h.01M18 12h.01M8 15h8"/></svg>"#;
const QUEUE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4v12a3 3 0 0 0 3 3h13m-4-4 4 4-4 4M9 5h11M9 10h7"/></svg>"#;
const STEER_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5v5a3 3 0 0 0 3 3h13m-5-5 5 5-5 5"/></svg>"#;
const GOAL_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="5"/><circle cx="12" cy="12" r="1"/></svg>"#;
const PAUSE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 5v14M16 5v14"/></svg>"#;
const PLAY_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m8 4 12 8-12 8Z"/></svg>"#;
const TRASH_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M9 6V4h6v2M5 6l1 14h12l1-14M10 10v6M14 10v6"/></svg>"#;

const HELP_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.82 1c0 2-3 3-3 3"/><path d="M12 17h.01"/></svg>"#;

/// Vega's pane-move glyph (R47 §2.2) is kept inline because the bundled icon
/// set only offers the shell-slot dock shapes (panel + bottom/right bar), and
/// reusing one of them for the pane-local "move dock" action read as the same
/// control twice in one header. The glyph is a panel rectangle offset toward
/// the trailing edge plus one arrow entering it from outside, on the same
/// 24px viewBox, stroke-2, currentColor grammar as the other inline icons. At
/// 16px it stays clearly distinct from `DockBottom` (rect + bottom bar) and
/// `DockRight` (rect + right bar): neither carries an arrow.
const DOCK_MOVE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="5" width="14" height="14" rx="2"/><path d="M2 12h9"/><path d="m8 9 3 3-3 3"/></svg>"#;

/// R62 R8: Lucide's open hand, the reference implementation's "Ask for
/// approval" glyph. gpui-kit 0.6.0 ships no hand asset, so the same 24px,
/// stroke-2, round-join grammar is carried inline here (the file's existing
/// convention for missing symbols).
const HAND_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2"/><path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2"/><path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8"/><path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15"/></svg>"#;

/// R62 R8: Lucide's shield, the reference implementation's "Approve for me"
/// glyph. Inline for the same reason as [`HAND_SVG`].
const SHIELD_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/></svg>"#;

/// The Git branch outline is authored inline because gpui-kit 0.6.0 does not
/// ship a Git branch asset. It follows the shared 24px viewBox, stroke-2,
/// round-cap/round-join convention used by the other inline functional icons.
const GIT_BRANCH_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="6" y1="7" x2="6" y2="17"/><circle cx="6" cy="5" r="2"/><circle cx="18" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><path d="M18 7v3a9 9 0 0 1-9 9H8"/></svg>"#;

/// #117: Lucide's public `text-select` icon (ISC), carried inline because
/// `gpui-kit-assets 0.6.0` does not ship it. Two bracket halves with three
/// shortening rules read as "condensed text" at 16px, which is the reference
/// product's own marker for a compacted context.
const TEXT_SELECT_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 3a2 2 0 0 0-2 2"/><path d="M19 3a2 2 0 0 1 2 2"/><path d="M21 19a2 2 0 0 1-2 2"/><path d="M5 21a2 2 0 0 1-2-2"/><path d="M9 3h1"/><path d="M9 21h1"/><path d="M14 3h1"/><path d="M14 21h1"/><path d="M3 9v1"/><path d="M21 9v1"/><path d="M3 14v1"/><path d="M21 14v1"/><line x1="7" x2="15" y1="8" y2="8"/><line x1="7" x2="17" y1="12" y2="12"/><line x1="7" x2="13" y1="16" y2="16"/></svg>"#;

fn icon_name(kind: Icon) -> IconName {
    match kind {
        Icon::Copy => IconName::Copy,
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
        Icon::Connection => IconName::Network,
        Icon::Wifi => unreachable!("inline SVG icons are handled before mapping"),
        Icon::Bot => IconName::Bot,
        Icon::ArrowUpDown => IconName::ChevronsUpDown,
        Icon::GitBranch => unreachable!("inline SVG icons are handled before mapping"),
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
        Icon::BookOpen => IconName::BookOpen,
        Icon::Globe => IconName::Globe,
        Icon::Check => IconName::Check,
        Icon::Chart
        | Icon::Plug
        | Icon::Blocks
        | Icon::Pin
        | Icon::Summary
        | Icon::DockMove
        | Icon::Help
        | Icon::Pencil
        | Icon::Wrench
        | Icon::Monitor
        | Icon::Camera
        | Icon::MousePointer
        | Icon::Keyboard
        | Icon::Hand
        | Icon::Shield
        | Icon::TextSelect => {
            unreachable!("inline SVG icons are handled before mapping")
        }
        Icon::Queue | Icon::Steer | Icon::Trash | Icon::Goal | Icon::Pause | Icon::Play => {
            unreachable!("inline SVG icons are handled before mapping")
        }
    }
}

fn kit_icon(kind: Icon, color: Rgba, size: f32) -> AnyElement {
    KitIcon::new(icon_name(kind))
        .with_size(px(size))
        .text_color(color)
        .flex_shrink_0()
        .into_any_element()
}

fn inline_icon(data: &'static [u8], color: Rgba, size: f32) -> AnyElement {
    svg()
        .data(data)
        .size(px(size))
        .flex_shrink_0()
        .text_color(color)
        .into_any_element()
}

/// Paints a 16px icon in the caller's semantic text color.
pub fn icon(kind: Icon, color: Rgba) -> AnyElement {
    sized_icon(kind, color, Layout::ICON_SIZE)
}

pub(crate) fn sized_icon(kind: Icon, color: Rgba, size: f32) -> AnyElement {
    let glyph = match kind {
        Icon::Chart => inline_icon(CHART_SVG, color, size),
        Icon::Plug => inline_icon(PLUG_SVG, color, size),
        Icon::Wifi => inline_icon(WIFI_SVG, color, size),
        Icon::Blocks => inline_icon(BLOCKS_SVG, color, size),
        Icon::Pin => inline_icon(PIN_SVG, color, size),
        Icon::FolderPlus => inline_icon(FOLDER_PLUS_SVG, color, size),
        Icon::Summary => inline_icon(SUMMARY_SVG, color, size),
        Icon::DockMove => inline_icon(DOCK_MOVE_SVG, color, size),
        Icon::Hand => inline_icon(HAND_SVG, color, size),
        Icon::Shield => inline_icon(SHIELD_SVG, color, size),
        Icon::GitBranch => inline_icon(GIT_BRANCH_SVG, color, size),
        Icon::TextSelect => inline_icon(TEXT_SELECT_SVG, color, size),
        Icon::Help => inline_icon(HELP_SVG, color, size),
        Icon::Pencil => inline_icon(PENCIL_SVG, color, size),
        Icon::Wrench => inline_icon(WRENCH_SVG, color, size),
        Icon::Monitor => inline_icon(MONITOR_SVG, color, size),
        Icon::Camera => inline_icon(CAMERA_SVG, color, size),
        Icon::MousePointer => inline_icon(MOUSE_POINTER_SVG, color, size),
        Icon::Keyboard => inline_icon(KEYBOARD_SVG, color, size),
        Icon::Queue => inline_icon(QUEUE_SVG, color, size),
        Icon::Steer => inline_icon(STEER_SVG, color, size),
        Icon::Trash => inline_icon(TRASH_SVG, color, size),
        Icon::Goal => inline_icon(GOAL_SVG, color, size),
        Icon::Pause => inline_icon(PAUSE_SVG, color, size),
        Icon::Play => inline_icon(PLAY_SVG, color, size),
        _ => kit_icon(kind, color, size),
    };
    gpui_kit::div()
        .size(px(size))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(glyph)
        .into_any_element()
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
) -> Interaction {
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
        .interactive(colors, true, false)
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
) -> Interaction {
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
        .interactive(colors, enabled, selected)
        .border_2()
        .border_color(gpui_kit::transparent_black())
        .when(enabled, |button| {
            button.focus_visible(move |style| style.border_2().border_color(colors.accent))
        })
        .when(enabled && !selected, |button| button.cursor_pointer())
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
