//! Vega application entry point: boots the GPUI app and opens the main window.

use gpui::prelude::*;
use gpui::{
    App, Bounds, Entity, KeyBinding, TitlebarOptions, Window, WindowBounds, WindowOptions, actions,
    div, px, size,
};
use gpui_platform::application;
use vega_theme::{Theme, Typography, theme};
use vega_ui::settings::{CloseSettings, OpenSettings, SettingsOpen, SettingsView};

actions!(vega, [Quit, ToggleTheme]);

/// Initial (and minimum) main window size in logical pixels (UI spec §1).
const WINDOW_MIN_WIDTH: f32 = 960.0;
const WINDOW_MIN_HEIGHT: f32 = 600.0;

/// Root view of the main window: the "✦ Vega" session placeholder, or the
/// settings view while [`SettingsOpen`] is set (Cmd+, / Esc).
struct VegaWindow {
    /// Cached settings view entity. Kept while settings is open so re-renders
    /// (e.g. the theme toggle) never rebuild the form mid-typing; dropped when
    /// settings closes so the next open reloads the config from disk.
    settings_view: Option<Entity<SettingsView>>,
}

impl Render for VegaWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Palette comes from the global theme so Cmd+Shift+L repaints instantly.
        let colors = theme(cx).colors;
        if cx.global::<SettingsOpen>().0 {
            // 设置视图：缓存 Entity，避免主题刷新等重渲染时重建导致表单输入丢失。
            let settings = self
                .settings_view
                .get_or_insert_with(|| cx.new(SettingsView::new));
            return div()
                .size_full()
                .bg(colors.bg_base)
                .text_color(colors.text_primary)
                .child(settings.clone())
                .into_any_element();
        }
        // 设置已关闭：丢弃缓存，下次打开时重新构造并从 config.toml 载入最新配置。
        self.settings_view = None;
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
            .into_any_element()
    }
}

fn main() {
    application().run(|cx: &mut App| {
        // Seed the global theme from the macOS appearance; components read it
        // via `vega_theme::theme(cx)`.
        let theme = Theme::system(cx);
        cx.set_global(theme);

        // Settings view starts closed; the window render reads this global.
        cx.set_global(SettingsOpen(false));

        // Key bindings for the vega_ui text input components.
        vega_ui::init(cx);

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
            |_, cx| {
                cx.new(|_| VegaWindow {
                    settings_view: None,
                })
            },
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
            // Settings view switching (T08).
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("escape", CloseSettings, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &ToggleTheme, cx| {
            cx.global_mut::<Theme>().toggle();
            // Redraw all windows so the new palette is visible immediately.
            cx.refresh_windows();
        });
        cx.on_action(|_: &OpenSettings, cx| {
            cx.set_global(SettingsOpen(true));
            cx.refresh_windows();
        });
        cx.on_action(|_: &CloseSettings, cx| {
            cx.set_global(SettingsOpen(false));
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
