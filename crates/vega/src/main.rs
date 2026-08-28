//! Vega application entry point: boots the GPUI app and opens the main window.

use gpui::prelude::*;
use gpui::{
    App, Bounds, KeyBinding, TitlebarOptions, Window, WindowBounds, WindowOptions, actions, div,
    px, size,
};
use gpui_platform::application;
use vega_theme::{Theme, Typography, theme};

actions!(vega, [Quit, ToggleTheme]);

/// Initial (and minimum) main window size in logical pixels (UI spec §1).
const WINDOW_MIN_WIDTH: f32 = 960.0;
const WINDOW_MIN_HEIGHT: f32 = 600.0;

/// Root view of the main window: a dark canvas with a centered "✦ Vega" mark.
struct VegaWindow;

impl Render for VegaWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Palette comes from the global theme so Cmd+Shift+L repaints instantly.
        let colors = theme(cx).colors;
        div()
            .size_full()
            .flex()
            .justify_center()
            .items_center()
            .bg(colors.bg_base)
            // Typography per UI spec §3: page-level title is 16px / weight 600.
            .text_size(px(Typography::HEADING_PAGE))
            .font_weight(Typography::HEADING_PAGE_WEIGHT)
            .text_color(colors.text_primary)
            .child("✦ Vega")
    }
}

fn main() {
    application().run(|cx: &mut App| {
        // Seed the global theme from the macOS appearance; components read it
        // via `vega_theme::theme(cx)`.
        let theme = Theme::system(cx);
        cx.set_global(theme);

        let bounds = Bounds::centered(None, size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT)), cx);
        let min_size = size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT));

        let window = cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Vega".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(min_size),
                ..Default::default()
            },
            |_, cx| cx.new(|_| VegaWindow),
        );

        if let Err(error) = window {
            // Degrade path: without the main window there is nothing to run.
            tracing::error!(%error, "failed to open the main window");
            cx.quit();
            return;
        }

        cx.activate(true);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            // Temporary verification binding for the theme token mechanism.
            KeyBinding::new("cmd-shift-l", ToggleTheme, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &ToggleTheme, cx| {
            cx.global_mut::<Theme>().toggle();
            // Redraw all windows so the new palette is visible immediately.
            cx.refresh_windows();
        });
        // Quit once the last window is closed so the process does not linger.
        cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
