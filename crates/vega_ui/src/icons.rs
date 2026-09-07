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
    ChevronDown,
    ArrowDown,
    Refresh,
    Split,
    Mode,
    Shield,
    Thinking,
    Document,
}

/// Paints a 16px icon in the caller's semantic text color.
pub fn icon(kind: Icon, color: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let segments: &[&[(f32, f32)]] = match kind {
                Icon::Document => &[
                    &[
                        (3., 2.),
                        (10., 2.),
                        (13., 5.),
                        (13., 14.),
                        (3., 14.),
                        (3., 2.),
                    ],
                    &[(10., 2.), (10., 5.), (13., 5.)],
                    &[(5., 8.), (11., 8.)],
                    &[(5., 11.), (10., 11.)],
                ],
                Icon::Mode => &[&[
                    (2., 3.),
                    (14., 3.),
                    (14., 11.),
                    (7., 11.),
                    (3., 14.),
                    (3., 11.),
                    (2., 11.),
                    (2., 3.),
                ]],
                Icon::Shield => &[&[
                    (8., 2.),
                    (13., 4.),
                    (13., 8.),
                    (11., 12.),
                    (8., 14.),
                    (5., 12.),
                    (3., 8.),
                    (3., 4.),
                    (8., 2.),
                ]],
                Icon::Thinking => &[&[
                    (8., 1.),
                    (10., 6.),
                    (15., 8.),
                    (10., 10.),
                    (8., 15.),
                    (6., 10.),
                    (1., 8.),
                    (6., 6.),
                    (8., 1.),
                ]],
                Icon::Close => &[&[(4., 4.), (12., 12.)], &[(12., 4.), (4., 12.)]],
                Icon::DockBottom => &[
                    &[(2., 3.), (14., 3.), (14., 13.), (2., 13.), (2., 3.)],
                    &[(2., 9.), (14., 9.)],
                ],
                Icon::DockRight => &[
                    &[(2., 3.), (14., 3.), (14., 13.), (2., 13.), (2., 3.)],
                    &[(10., 3.), (10., 13.)],
                ],
                Icon::Maximize => &[
                    &[(3., 6.), (3., 3.), (6., 3.)],
                    &[(10., 3.), (13., 3.), (13., 6.)],
                    &[(13., 10.), (13., 13.), (10., 13.)],
                    &[(6., 13.), (3., 13.), (3., 10.)],
                ],
                Icon::Minimize => &[&[(3., 8.), (13., 8.)]],
                Icon::More => &[
                    &[(3., 8.), (4., 8.)],
                    &[(7., 8.), (8., 8.)],
                    &[(11., 8.), (12., 8.)],
                ],
                Icon::ChevronDown => &[&[(4., 6.), (8., 10.), (12., 6.)]],
                Icon::ArrowDown => &[&[(8., 3.), (8., 13.)], &[(3., 8.), (8., 13.), (13., 8.)]],
                Icon::Refresh => &[
                    &[
                        (12., 5.),
                        (9., 3.),
                        (5., 3.),
                        (2., 6.),
                        (2., 10.),
                        (5., 13.),
                        (10., 13.),
                        (13., 10.),
                    ],
                    &[(9., 5.), (13., 5.), (13., 1.)],
                ],
                Icon::Split => &[
                    &[(2., 3.), (14., 3.), (14., 13.), (2., 13.), (2., 3.)],
                    &[(8., 3.), (8., 13.)],
                ],
                Icon::Sidebar => &[
                    &[(2., 3.), (14., 3.), (14., 13.), (2., 13.), (2., 3.)],
                    &[(6., 3.), (6., 13.)],
                ],
                Icon::Plus => &[&[(3., 8.), (13., 8.)], &[(8., 3.), (8., 13.)]],
                Icon::ArrowUp => &[&[(8., 13.), (8., 3.)], &[(3., 8.), (8., 3.), (13., 8.)]],
                Icon::ArrowLeft => &[&[(13., 8.), (3., 8.)], &[(8., 3.), (3., 8.), (8., 13.)]],
                Icon::Folder => &[&[
                    (2., 13.),
                    (2., 3.),
                    (6., 3.),
                    (8., 5.),
                    (14., 5.),
                    (14., 13.),
                    (2., 13.),
                ]],
                Icon::Settings => &[
                    &[(2., 4.), (14., 4.)],
                    &[(2., 12.), (14., 12.)],
                    &[(5., 2.), (5., 6.)],
                    &[(11., 10.), (11., 14.)],
                ],
            };
            let mut path = PathBuilder::stroke(px(1.25));
            for segment in segments {
                for (index, (x, y)) in segment.iter().enumerate() {
                    let position = bounds.origin + point(px(*x), px(*y));
                    if index == 0 {
                        path.move_to(position);
                    } else {
                        path.line_to(position);
                    }
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
