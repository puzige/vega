//! Original monochrome line icons for compact native chrome.
use gpui_kit::{IntoElement, PathBuilder, Rgba, canvas, point, prelude::*, px};

/// Small functional line icons, drawn without platform-dependent glyphs.
#[derive(Clone, Copy)]
pub enum Icon {
    Sidebar,
    Plus,
    ArrowUp,
    ArrowLeft,
    Folder,
    Settings,
    Close,
    DockBottom,
    DockRight,
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
    Mode,
    Shield,
    Thinking,
    Document,
}

/// Paints a 16px icon in the caller's semantic text color.
///
/// The geometry is deliberately local rather than font-backed: every icon
/// stays crisp at the 16px optical grid and can share the R18 soft-corner
/// language without introducing a runtime asset or Unicode glyph dependency.
pub fn icon(kind: Icon, color: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let at = |x: f32, y: f32| bounds.origin + point(px(x), px(y));
            let mut path = PathBuilder::stroke(px(1.45));

            match kind {
                Icon::Document => {
                    path.move_to(at(3.1, 3.0));
                    path.curve_to(at(3.1, 2.5), at(3.5, 2.2));
                    path.line_to(at(9.8, 2.2));
                    path.line_to(at(12.9, 5.2));
                    path.line_to(at(12.9, 13.2));
                    path.curve_to(at(12.9, 13.7), at(12.5, 14.0));
                    path.line_to(at(3.1, 14.0));
                    path.close();
                    path.move_to(at(9.8, 2.3));
                    path.line_to(at(9.8, 5.2));
                    path.line_to(at(12.7, 5.2));
                    path.move_to(at(5.0, 8.2));
                    path.line_to(at(11.0, 8.2));
                    path.move_to(at(5.0, 11.0));
                    path.line_to(at(10.0, 11.0));
                }
                Icon::Mode => {
                    path.move_to(at(3.0, 2.8));
                    path.curve_to(at(2.4, 2.8), at(2.1, 3.1));
                    path.line_to(at(2.1, 10.4));
                    path.curve_to(at(2.1, 11.0), at(2.4, 11.3));
                    path.line_to(at(3.5, 11.3));
                    path.line_to(at(3.5, 13.5));
                    path.line_to(at(7.0, 11.3));
                    path.line_to(at(13.0, 11.3));
                    path.curve_to(at(13.6, 11.3), at(13.9, 11.0));
                    path.line_to(at(13.9, 3.5));
                    path.curve_to(at(13.9, 3.0), at(13.6, 2.8));
                    path.close();
                }
                Icon::Shield => {
                    path.move_to(at(8.0, 2.0));
                    path.curve_to(at(8.8, 2.4), at(10.5, 3.2));
                    path.line_to(at(12.9, 4.2));
                    path.line_to(at(12.9, 8.0));
                    path.curve_to(at(12.9, 10.4), at(11.2, 12.4));
                    path.curve_to(at(10.2, 13.3), at(8.7, 14.0));
                    path.curve_to(at(8.5, 14.1), at(8.3, 14.1));
                    path.curve_to(at(8.1, 14.0), at(6.3, 13.3));
                    path.curve_to(at(4.8, 12.3), at(3.1, 10.2));
                    path.line_to(at(3.1, 4.2));
                    path.close();
                }
                Icon::Thinking => {
                    // The sole spark-like functional icon follows the slender
                    // single-star silhouette from the approved app logo.
                    path = PathBuilder::fill();
                    path.move_to(at(8.0, 1.5));
                    path.curve_to(at(8.4, 3.8), at(8.9, 5.7));
                    path.curve_to(at(9.9, 6.9), at(11.5, 7.6));
                    path.curve_to(at(12.7, 7.9), at(13.6, 8.0));
                    path.curve_to(at(12.4, 8.2), at(11.2, 8.7));
                    path.curve_to(at(9.8, 9.3), at(9.0, 10.9));
                    path.curve_to(at(8.6, 12.1), at(8.2, 13.5));
                    path.curve_to(at(7.8, 12.0), at(7.2, 10.5));
                    path.curve_to(at(6.4, 9.3), at(4.9, 8.6));
                    path.curve_to(at(3.6, 8.2), at(2.5, 8.1));
                    path.curve_to(at(3.7, 7.8), at(5.0, 7.2));
                    path.curve_to(at(6.5, 6.5), at(7.3, 4.9));
                    path.curve_to(at(7.7, 3.5), at(7.9, 2.2));
                    path.close();
                }
                Icon::Close => {
                    path.move_to(at(4.2, 4.2));
                    path.line_to(at(11.8, 11.8));
                    path.move_to(at(11.8, 4.2));
                    path.line_to(at(4.2, 11.8));
                }
                Icon::DockBottom => {
                    path.move_to(at(2.4, 3.2));
                    path.curve_to(at(2.4, 2.8), at(2.7, 2.5));
                    path.line_to(at(13.3, 2.5));
                    path.curve_to(at(13.7, 2.5), at(14.0, 2.8));
                    path.line_to(at(14.0, 12.8));
                    path.curve_to(at(14.0, 13.2), at(13.7, 13.5));
                    path.line_to(at(2.4, 13.5));
                    path.close();
                    path.move_to(at(2.4, 9.0));
                    path.line_to(at(14.0, 9.0));
                }
                Icon::DockRight => {
                    path.move_to(at(2.4, 3.2));
                    path.curve_to(at(2.4, 2.8), at(2.7, 2.5));
                    path.line_to(at(13.3, 2.5));
                    path.curve_to(at(13.7, 2.5), at(14.0, 2.8));
                    path.line_to(at(14.0, 12.8));
                    path.curve_to(at(14.0, 13.2), at(13.7, 13.5));
                    path.line_to(at(2.4, 13.5));
                    path.close();
                    path.move_to(at(10.0, 2.7));
                    path.line_to(at(10.0, 13.3));
                }
                Icon::Maximize => {
                    path.move_to(at(6.0, 3.1));
                    path.line_to(at(3.2, 3.1));
                    path.line_to(at(3.2, 6.0));
                    path.move_to(at(10.0, 3.1));
                    path.line_to(at(12.8, 3.1));
                    path.line_to(at(12.8, 6.0));
                    path.move_to(at(12.8, 10.0));
                    path.line_to(at(12.8, 12.9));
                    path.line_to(at(10.0, 12.9));
                    path.move_to(at(6.0, 12.9));
                    path.line_to(at(3.2, 12.9));
                    path.line_to(at(3.2, 10.0));
                }
                Icon::Minimize => {
                    path.move_to(at(3.2, 8.0));
                    path.curve_to(at(3.2, 7.7), at(3.5, 7.5));
                    path.line_to(at(12.8, 7.5));
                    path.curve_to(at(13.1, 7.5), at(13.2, 7.7));
                }
                Icon::More => {
                    path.move_to(at(2.9, 8.0));
                    path.curve_to(at(2.9, 7.7), at(3.2, 7.5));
                    path.curve_to(at(3.5, 7.5), at(3.8, 7.7));
                    path.curve_to(at(3.8, 8.3), at(3.5, 8.5));
                    path.curve_to(at(3.2, 8.5), at(2.9, 8.3));
                    path.move_to(at(7.0, 8.0));
                    path.curve_to(at(7.0, 7.7), at(7.3, 7.5));
                    path.curve_to(at(7.6, 7.5), at(7.9, 7.7));
                    path.curve_to(at(7.9, 8.3), at(7.6, 8.5));
                    path.curve_to(at(7.3, 8.5), at(7.0, 8.3));
                    path.move_to(at(11.1, 8.0));
                    path.curve_to(at(11.1, 7.7), at(11.4, 7.5));
                    path.curve_to(at(11.7, 7.5), at(12.0, 7.7));
                    path.curve_to(at(12.0, 8.3), at(11.7, 8.5));
                    path.curve_to(at(11.4, 8.5), at(11.1, 8.3));
                }
                Icon::ArrowUpDown => {
                    path.move_to(at(8.0, 2.5));
                    path.line_to(at(8.0, 13.5));
                    path.move_to(at(4.2, 5.9));
                    path.line_to(at(8.0, 2.5));
                    path.line_to(at(11.8, 5.9));
                    path.move_to(at(4.2, 10.1));
                    path.line_to(at(8.0, 13.5));
                    path.line_to(at(11.8, 10.1));
                }
                Icon::Pin => {
                    path.move_to(at(8.0, 2.2));
                    path.line_to(at(11.7, 5.9));
                    path.line_to(at(10.0, 7.6));
                    path.line_to(at(10.0, 11.2));
                    path.line_to(at(6.0, 11.2));
                    path.line_to(at(6.0, 7.6));
                    path.line_to(at(4.3, 5.9));
                    path.close();
                    path.move_to(at(8.0, 11.5));
                    path.line_to(at(8.0, 14.8));
                }
                Icon::ChevronDown => {
                    path.move_to(at(4.3, 5.9));
                    path.curve_to(at(4.5, 5.6), at(4.7, 5.6));
                    path.line_to(at(7.7, 8.55));
                    path.curve_to(at(8.0, 8.85), at(8.3, 8.85));
                    path.line_to(at(11.3, 5.95));
                    path.curve_to(at(11.5, 5.7), at(11.7, 5.7));
                }
                Icon::ChevronRight => {
                    path.move_to(at(5.25, 4.7));
                    path.curve_to(at(5.0, 4.9), at(5.0, 5.1));
                    path.line_to(at(8.0, 7.7));
                    path.curve_to(at(8.3, 8.0), at(8.3, 8.25));
                    path.line_to(at(5.25, 10.9));
                    path.curve_to(at(5.0, 11.1), at(5.0, 11.3));
                }
                Icon::ArrowDown => {
                    path.move_to(at(8.0, 2.8));
                    path.line_to(at(8.0, 13.2));
                    path.move_to(at(3.4, 8.6));
                    path.line_to(at(8.0, 13.2));
                    path.line_to(at(12.6, 8.6));
                }
                Icon::Refresh => {
                    path.move_to(at(12.2, 5.3));
                    path.curve_to(at(10.9, 3.5), at(9.6, 3.0));
                    path.curve_to(at(5.1, 1.4), at(2.4, 4.1));
                    path.curve_to(at(0.9, 6.0), at(1.5, 10.5));
                    path.curve_to(at(2.2, 13.0), at(5.3, 13.9));
                    path.curve_to(at(9.3, 15.1), at(12.2, 12.0));
                    path.move_to(at(9.1, 5.3));
                    path.line_to(at(12.2, 5.3));
                    path.line_to(at(12.2, 2.3));
                }
                Icon::Split => {
                    path.move_to(at(2.4, 3.2));
                    path.curve_to(at(2.4, 2.8), at(2.7, 2.5));
                    path.line_to(at(13.3, 2.5));
                    path.curve_to(at(13.7, 2.5), at(14.0, 2.8));
                    path.line_to(at(14.0, 12.8));
                    path.curve_to(at(14.0, 13.2), at(13.7, 13.5));
                    path.line_to(at(2.4, 13.5));
                    path.close();
                    path.move_to(at(8.0, 2.7));
                    path.line_to(at(8.0, 13.3));
                }
                Icon::Sidebar => {
                    path.move_to(at(2.4, 3.2));
                    path.curve_to(at(2.4, 2.8), at(2.7, 2.5));
                    path.line_to(at(13.3, 2.5));
                    path.curve_to(at(13.7, 2.5), at(14.0, 2.8));
                    path.line_to(at(14.0, 12.8));
                    path.curve_to(at(14.0, 13.2), at(13.7, 13.5));
                    path.line_to(at(2.4, 13.5));
                    path.close();
                    path.move_to(at(6.0, 2.7));
                    path.line_to(at(6.0, 13.3));
                }
                Icon::Plus => {
                    path.move_to(at(3.2, 8.0));
                    path.line_to(at(12.8, 8.0));
                    path.move_to(at(8.0, 3.2));
                    path.line_to(at(8.0, 12.8));
                }
                Icon::ArrowUp => {
                    path.move_to(at(8.0, 13.2));
                    path.line_to(at(8.0, 2.8));
                    path.move_to(at(3.4, 7.4));
                    path.line_to(at(8.0, 2.8));
                    path.line_to(at(12.6, 7.4));
                }
                Icon::ArrowLeft => {
                    path.move_to(at(13.2, 8.0));
                    path.line_to(at(2.8, 8.0));
                    path.move_to(at(7.4, 3.4));
                    path.line_to(at(2.8, 8.0));
                    path.line_to(at(7.4, 12.6));
                }
                Icon::Folder | Icon::FolderPlus => {
                    // Soft folder silhouette: the shallow bottom bow is the
                    // only logo-derived flourish, and remains non-character.
                    path.move_to(at(2.25, 5.35));
                    path.line_to(at(2.25, 4.3));
                    path.curve_to(at(2.25, 3.55), at(2.55, 3.3));
                    path.line_to(at(5.45, 3.3));
                    path.curve_to(at(6.0, 3.3), at(6.25, 3.55));
                    path.line_to(at(7.95, 5.25));
                    path.line_to(at(12.9, 5.25));
                    path.curve_to(at(13.7, 5.25), at(13.75, 5.6));
                    path.line_to(at(13.75, 11.25));
                    path.curve_to(at(13.75, 12.35), at(13.25, 12.5));
                    path.curve_to(at(8.0, 13.15), at(3.0, 12.5));
                    path.curve_to(at(2.25, 12.4), at(2.25, 11.25));
                    path.close();
                    if matches!(kind, Icon::FolderPlus) {
                        path.move_to(at(10.4, 8.8));
                        path.line_to(at(13.1, 8.8));
                        path.move_to(at(11.75, 7.45));
                        path.line_to(at(11.75, 10.15));
                    }
                }
                Icon::Settings => {
                    path.move_to(at(2.5, 4.2));
                    path.curve_to(at(2.5, 3.9), at(2.8, 3.7));
                    path.line_to(at(13.5, 3.7));
                    path.curve_to(at(13.8, 3.7), at(14.0, 3.9));
                    path.move_to(at(2.5, 12.3));
                    path.curve_to(at(2.5, 12.0), at(2.8, 11.8));
                    path.line_to(at(13.5, 11.8));
                    path.curve_to(at(13.8, 11.8), at(14.0, 12.0));
                    path.move_to(at(5.0, 2.4));
                    path.line_to(at(5.0, 5.9));
                    path.move_to(at(11.0, 10.1));
                    path.line_to(at(11.0, 13.6));
                }
            }

            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size(px(16.))
    .flex_shrink_0()
}

struct IconTooltip(gpui_kit::SharedString);
impl gpui_kit::Render for IconTooltip {
    fn render(
        &mut self,
        _: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        let colors = vega_theme::theme(cx).colors;
        gpui_kit::div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(colors.bg_elevated)
            .border_1()
            .border_color(colors.border_subtle)
            .text_size(px(vega_theme::Typography::METADATA))
            .text_color(colors.text_primary)
            .child(self.0.clone())
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
        .tooltip(move |_, cx| cx.new(|_| IconTooltip(tooltip_label.clone())).into())
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
    cx.new(|_| IconTooltip(label)).into()
}
