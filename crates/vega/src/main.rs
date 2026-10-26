use gpui_kit::application;
use gpui_kit::prelude::*;
use gpui_kit::*;
use vega_theme::*;
use vega_ui::conversation_stream::bench as render_frame_bench;
use vega_ui::settings::*;
use vega_ui::sidebar::*;

actions!(vega, [Quit, ToggleTheme]);

const WINDOW_INITIAL_WIDTH: f32 = 1403.0;
const WINDOW_INITIAL_HEIGHT: f32 = 860.0;

/// Minimum main window size in logical pixels (UI spec §1).
const WINDOW_MIN_WIDTH: f32 = 960.0;
const WINDOW_MIN_HEIGHT: f32 = 600.0;

mod agent_context;
mod app_agent;
mod app_localization;
mod app_notifications;
mod app_palette;
mod app_usage;
mod artifact_controller;
mod automation_controller;
mod branch_controller;
mod codex_agent;
mod commit_controller;
mod diff_controller;
mod model_catalog;
mod native_subagents;
mod ordinary_submission;
mod pricing_controller;
mod reasoning_capabilities;
mod thread_reload;
mod trusted_action;
mod updater;
mod window;

use window::VegaWindow;

fn main() {
    if updater::run_helper_if_requested() {
        return;
    }

    if let Some(output) = render_frame_bench::output_path_from_args() {
        let app = application().with_assets(gpui_kit::assets::Assets);
        app.run(|cx: &mut App| {
            gpui_kit::init(cx);
            render_frame_bench::start(output, cx);
        });
        return;
    }

    let app = application().with_assets(gpui_kit::assets::Assets);
    app.run(|cx: &mut App| {
        app_notifications::init(cx);
        // GPUI Kit owns the shared component/base globals before any Vega
        // view can render a Kit control.
        gpui_kit::init(cx);

        // Seed the global theme from the macOS appearance; components read it
        // via `vega_theme::theme(cx)`.
        let config = vega_store::config::load();
        vega_i18n::set_language(
            config
                .as_ref()
                .map(|config| config.ui.language.as_str())
                .unwrap_or("en"),
        );
        let theme = match config.as_ref().map(|config| config.ui.theme.as_str()) {
            Ok("light") => Theme::light(),
            Ok("dark") => Theme::dark(),
            _ => Theme::system(cx),
        };
        let component_mode = match theme.appearance {
            Appearance::Light => gpui_kit::component::ThemeMode::Light,
            Appearance::Dark => gpui_kit::component::ThemeMode::Dark,
        };
        gpui_kit::component::Theme::change(component_mode, None, cx);
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
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("escape", CloseSettings, Some("VegaWindow")),
            KeyBinding::new("escape", CloseSettings, Some("WorkspaceMenu")),
            KeyBinding::new("cmd-b", ToggleSidebar, None),
        ]);
        // Key bindings for the vega_ui text input components.
        vega_ui::init(cx);
        vega_ui::interaction::install_system_motion(cx);

        vega_ui::sidebar::init(cx);
        automation_controller::startup(cx);

        let mcp_settings = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .ok()
            .and_then(|store| store.database_path())
            .and_then(|database| {
                vega_store::paths::config_dir().map(|config_root| {
                    vega_conversation::privacy::register_database(
                        database.to_path_buf(),
                        config_root.clone(),
                    );
                    vega_conversation::McpServerSettingsService::new(
                        database.to_path_buf(),
                        config_root,
                    )
                })
            });
        cx.set_global(app_agent::AppMcpSettings(mcp_settings));
        cx.set_global(app_agent::AppReplaySessions::default());

        let bounds = Bounds::centered(
            None,
            size(px(WINDOW_INITIAL_WIDTH), px(WINDOW_INITIAL_HEIGHT)),
            cx,
        );
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
            |window, cx| {
                cx.new(|cx| {
                    vega_ui::interaction::observe_system_motion(window, cx);
                    VegaWindow::new(cx)
                })
            },
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
            app_palette::bind_shortcuts(window.into(), root.clone(), cx);
            app_notifications::bind(window, root, cx);
        }
        cx.activate(true);
        updater::acknowledge_startup();
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &ToggleTheme, cx| {
            let appearance = {
                let theme = cx.global_mut::<Theme>();
                theme.toggle();
                theme.appearance
            };
            let component_mode = match appearance {
                Appearance::Light => gpui_kit::component::ThemeMode::Light,
                Appearance::Dark => gpui_kit::component::ThemeMode::Dark,
            };
            gpui_kit::component::Theme::change(component_mode, None, cx);
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
