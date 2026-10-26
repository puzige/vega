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
mod startup_controller;
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
        gpui_kit::init(cx);
        apply_appearance(None, cx);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        let bounds = Bounds::centered(
            None,
            size(px(WINDOW_INITIAL_WIDTH), px(WINDOW_INITIAL_HEIGHT)),
            cx,
        );
        let window = cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Vega".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT))),
                ..Default::default()
            },
            |_, cx| cx.new(startup_controller::StartupView::new),
        );
        let window = match window {
            Ok(window) => window,
            Err(error) => {
                tracing::error!(%error, "failed to open the startup window");
                cx.quit();
                return;
            }
        };
        if let Err(error) = window.update(cx, |view, window, cx| view.start(window, cx)) {
            tracing::error!(%error, "failed to start workspace recovery");
            cx.quit();
            return;
        }
        cx.activate(true);
        cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}

fn apply_appearance(config: Option<&vega_store::config::AppConfig>, cx: &mut App) {
    vega_i18n::set_language(
        config
            .map(|config| config.ui.language.as_str())
            .unwrap_or("en"),
    );
    let theme = match config.map(|config| config.ui.theme.as_str()) {
        Some("light") => Theme::light(),
        Some("dark") => Theme::dark(),
        _ => Theme::system(cx),
    };
    let component_mode = match theme.appearance {
        Appearance::Light => gpui_kit::component::ThemeMode::Light,
        Appearance::Dark => gpui_kit::component::ThemeMode::Dark,
    };
    gpui_kit::component::Theme::change(component_mode, None, cx);
    cx.set_global(theme);
}

fn open_workspace(
    workspace: vega_conversation::types::StartupWorkspace,
    window: &mut Window,
    cx: &mut App,
) {
    apply_appearance(Some(workspace.config()), cx);
    cx.set_global(SettingsOpen(false));
    cx.bind_keys([
        KeyBinding::new("cmd-shift-l", ToggleTheme, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("escape", CloseSettings, Some("VegaWindow")),
        KeyBinding::new("escape", CloseSettings, Some("WorkspaceMenu")),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
    ]);
    vega_ui::init(cx);
    vega_ui::interaction::install_system_motion(cx);
    vega_ui::sidebar::init(workspace, cx);
    automation_controller::startup_ready(cx);
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
    let root = window.replace_root(cx, |window, cx| {
        vega_ui::interaction::observe_system_motion(window, cx);
        VegaWindow::new(cx)
    });
    let handle = window.window_handle();
    app_palette::bind_shortcuts(handle, root.downgrade(), cx);
    app_notifications::bind(handle, root.downgrade(), cx);
    updater::acknowledge_workspace();
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
        cx.refresh_windows();
    });
    cx.on_action(|_: &OpenSettings, cx| {
        cx.set_global(SettingsOpen(true));
        cx.refresh_windows();
    });
    cx.on_action(|_: &ToggleSidebar, cx| toggle_persisted(cx));
}
