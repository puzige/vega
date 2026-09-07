//! Vega application entry point: boots the GPUI app and opens the main window.
//! The hidden `--vega-bench-render <out.json>` flag instead runs the S3-T17
//! render_frame self-measurement probe (see
//! [`vega_ui::conversation_stream::bench`]).

use gpui::prelude::*;
use gpui::*;
use gpui_platform::application;
use vega_theme::*;
use vega_ui::conversation_stream::bench as render_frame_bench;
use vega_ui::settings::*;
use vega_ui::sidebar::*;

actions!(vega, [Quit, ToggleTheme]);

/// Initial (and minimum) main window size in logical pixels (UI spec §1).
const WINDOW_MIN_WIDTH: f32 = 960.0;
const WINDOW_MIN_HEIGHT: f32 = 600.0;

mod app_agent;
mod app_palette;
mod app_usage;
mod artifact_controller;
mod branch_controller;
mod commit_controller;
mod diff_controller;
mod pricing_controller;
mod thread_reload;
mod trusted_action;
mod window;

#[cfg(test)]
mod tests;

use window::VegaWindow;

fn main() {
    // S3-T17 隐藏自测量模式：`vega --vega-bench-render <out.json>` 跑完写
    // JSON 后退出（xtask bench render_frame 的数据来源），不进入正常应用。
    if let Some(output) = render_frame_bench::output_path_from_args() {
        application().run(|cx: &mut App| render_frame_bench::start(output, cx));
        return;
    }

    application().run(|cx: &mut App| {
        // Seed the global theme from the macOS appearance; components read it
        // via `vega_theme::theme(cx)`.
        let theme = match vega_store::config::load()
            .map(|config| config.ui.theme)
            .as_deref()
        {
            Ok("light") => Theme::light(),
            Ok("dark") => Theme::dark(),
            _ => Theme::system(cx),
        };
        cx.set_global(theme);

        // Sidebar collapse preference, restored from config.toml before the
        // window opens so the first frame already matches the stored state.
        cx.set_global(SidebarCollapsed(load_collapsed()));

        // Settings view starts closed; the window render reads this global.
        cx.set_global(SettingsOpen(false));

        // Keep Escape at the window context depth. An unscoped binding is
        // ranked at the deepest focus context and steals component actions.
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            // Temporary verification binding for the theme token mechanism.
            KeyBinding::new("cmd-shift-l", ToggleTheme, None),
            // Settings view switching (T08).
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("escape", CloseSettings, Some("VegaWindow")),
            KeyBinding::new("escape", CloseSettings, Some("WorkspaceMenu")),
            // Sidebar collapse toggle (T09).
            KeyBinding::new("cmd-b", ToggleSidebar, None),
        ]);
        // Key bindings for the vega_ui text input components.
        vega_ui::init(cx);

        // T12: open + migrate the store at the platform data root (tech-spec
        // §6) and seed the sidebar globals (selected project, block collapse
        // states, opened thread). On failure the app still boots and the
        // sidebar blocks degrade to inline error bars (ui-spec §4.6).
        vega_ui::sidebar::init(cx);

        let bounds = Bounds::centered(None, size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT)), cx);
        let min_size = size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT));

        let window = cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Vega".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(min_size),
                ..Default::default()
            },
            |_, cx| cx.new(VegaWindow::new),
        );

        let window = match window {
            Ok(window) => window,
            Err(error) => {
                // Degrade path: without the main window there is nothing to run.
                tracing::error!(%error, "failed to open the main window");
                cx.quit();
                return;
            }
        };

        if let Ok(root) = window.update(cx, |_, _, cx| cx.entity().downgrade()) {
            app_palette::bind_shortcuts(window.into(), root, cx);
        }
        cx.activate(true);
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
        cx.on_action(|_: &ToggleSidebar, cx| toggle_persisted(cx));
        // Quit once the last window is closed so the process does not linger.
        cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
