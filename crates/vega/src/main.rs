//! Vega application entry point: boots the GPUI app and opens the main window.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Entity, KeyBinding, MouseButton, MouseUpEvent, TitlebarOptions,
    Window, WindowBounds, WindowOptions, actions, div, px, size,
};
use gpui_platform::application;
use vega_theme::{Theme, ThemeColors, Typography, theme};
use vega_ui::projects::{CloseProjects, OpenProjects, ProjectsOpen, ProjectsView};
use vega_ui::settings::{CloseSettings, OpenSettings, SettingsOpen, SettingsView};
use vega_ui::sidebar::{
    AUTO_COLLAPSE_WIDTH, CONTENT_MAX_WIDTH, CONTENT_MIN_PADDING, Sidebar, SidebarCollapsed,
    ToggleSidebar, load_collapsed, toggle_persisted,
};
use vega_ui::threads::{CloseThreads, NewThread, OpenThreads, ThreadsOpen, ThreadsView};

actions!(vega, [Quit, ToggleTheme]);

/// Initial (and minimum) main window size in logical pixels (UI spec §1).
const WINDOW_MIN_WIDTH: f32 = 960.0;
const WINDOW_MIN_HEIGHT: f32 = 600.0;

/// Quick-template placeholder labels for the empty state (ui-spec §4.6);
/// intentionally inert until the template feature lands (A7-02).
const EMPTY_STATE_TEMPLATES: [&str; 3] = ["快捷模板 1", "快捷模板 2", "快捷模板 3"];

/// Root view of the main window: the A1 layout shell — a sidebar (260px,
/// collapsible) next to a content column (max 820px, centered) that hosts
/// either the empty state, the projects view (temporary T10 entry), the
/// temporary threads view (T11 entry), or the settings view (Cmd+, / Esc).
struct VegaWindow {
    /// Sidebar shell; its placeholder blocks are filled by T10 (projects)
    /// and T12 (sessions).
    sidebar: Entity<Sidebar>,
    /// Cached settings view entity. Kept while settings is open so re-renders
    /// (e.g. the theme toggle) never rebuild the form mid-typing; dropped when
    /// settings closes so the next open reloads the config from disk.
    settings_view: Option<Entity<SettingsView>>,
    /// Cached projects view entity (T10 temporary mount; T12 moves projects
    /// into the sidebar and this page is retired).
    projects_view: Option<Entity<ProjectsView>>,
    /// Cached temporary threads view (T11 stopgap until T12 integrates the
    /// sidebar); rebuilt on each open so the list reloads from the store.
    threads_view: Option<Entity<ThreadsView>>,
}

impl VegaWindow {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            sidebar: cx.new(|_| Sidebar),
            settings_view: None,
            projects_view: None,
            threads_view: None,
        }
    }

    /// Whether the viewport is narrower than the auto-collapse threshold
    /// (ui-spec §1). Reads the live viewport size: every platform resize is
    /// delivered as an event (`Window::bounds_changed` → redraw), so each
    /// render sees the current size and no polling is involved.
    fn auto_collapsed(&self, window: &Window) -> bool {
        window.viewport_size().width < px(AUTO_COLLAPSE_WIDTH)
    }

    /// Cmd+N entry point: opens the temporary threads view and creates a
    /// thread in the current project (the thread opens after creation).
    fn open_new_thread(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        cx.global_mut::<ThreadsOpen>().0 = true;
        let view = self
            .threads_view
            .get_or_insert_with(|| cx.new(ThreadsView::new));
        view.update(cx, ThreadsView::create_thread);
        cx.refresh_windows();
    }
}

impl Render for VegaWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Palette comes from the global theme so Cmd+Shift+L repaints instantly.
        let colors = theme(cx).colors;
        // Effective visibility: the user preference (Cmd+B, persisted) AND the
        // viewport auto-collapse rule (ui-spec §1).
        let sidebar_visible = !cx.global::<SidebarCollapsed>().0 && !self.auto_collapsed(window);

        // Settings opens inside the content area (T09 layout change of the
        // T08 view switching): the sidebar stays visible unless collapsed.
        // T10's projects view uses the same mechanism (temporary entry under
        // the empty state) until T12 moves it into the sidebar.
        let content: AnyElement = if cx.global::<SettingsOpen>().0 {
            // 设置视图：缓存 Entity，避免主题刷新等重渲染时重建导致表单输入丢失。
            let settings = self
                .settings_view
                .get_or_insert_with(|| cx.new(SettingsView::new));
            settings.clone().into_any_element()
        } else if cx.global::<ProjectsOpen>().0 {
            // 项目视图：同样缓存 Entity，保持列表/排序状态不被重渲染重建。
            let projects = self
                .projects_view
                .get_or_insert_with(|| cx.new(ProjectsView::new));
            projects.clone().into_any_element()
        } else if cx.global::<ThreadsOpen>().0 {
            // T11 临时会话视图：由「会话(临时)」入口 / Cmd+N 打开，T12 归位。
            let view = self
                .threads_view
                .get_or_insert_with(|| cx.new(ThreadsView::new));
            view.clone().into_any_element()
        } else {
            // 视图均已关闭：丢弃缓存，下次打开时重新构造并载入最新数据。
            self.settings_view = None;
            self.projects_view = None;
            self.threads_view = None;
            render_empty_state(colors).into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .when(sidebar_visible, |row| row.child(self.sidebar.clone()))
            .child(
                // Content column host: settings brings its own 820px column,
                // the empty state is centered by its own layout.
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(content),
            )
            .into_any_element()
    }
}

/// The content-area empty state (ui-spec §4.6): centered guidance with inert
/// quick-template placeholder buttons, inside the 820px content column —
/// no large logo illustration. Carries the temporary T10/T11 entry buttons
/// ("项目管理（临时）" / "会话(临时)"); T12 moves both into the sidebar.
fn render_empty_state(colors: ThemeColors) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .px(px(CONTENT_MIN_PADDING))
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_size(px(Typography::HEADING_PAGE))
                        .font_weight(Typography::HEADING_PAGE_WEIGHT)
                        .child("✦ Vega"),
                )
                .child(
                    div()
                        .text_size(px(Typography::BODY))
                        .text_color(colors.text_secondary)
                        .child("开始一个新会话"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children(EMPTY_STATE_TEMPLATES.map(|label| {
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(colors.border_subtle)
                                .bg(colors.bg_elevated)
                                .text_size(px(Typography::SIDEBAR))
                                .text_color(colors.text_secondary)
                                .child(label)
                        })),
                )
                // T10 临时入口：进入项目注册视图（T12 归位侧边栏后移除）。
                .child(
                    div()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(colors.border_subtle)
                        .bg(colors.bg_elevated)
                        .text_size(px(Typography::SIDEBAR))
                        .text_color(colors.text_secondary)
                        .cursor_pointer()
                        .hover(move |s| s.bg(colors.bg_hover).text_color(colors.text_primary))
                        .on_mouse_up(
                            MouseButton::Left,
                            |_: &MouseUpEvent, window: &mut Window, cx: &mut App| {
                                window.dispatch_action(Box::new(OpenProjects), cx);
                            },
                        )
                        .child("项目管理（临时）"),
                )
                // T11 临时入口：进入临时会话视图（T12 归位侧边栏后移除）。
                .child(
                    div()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(colors.border_subtle)
                        .bg(colors.bg_elevated)
                        .text_size(px(Typography::SIDEBAR))
                        .text_color(colors.text_secondary)
                        .cursor_pointer()
                        .hover(move |s| s.bg(colors.bg_hover).text_color(colors.text_primary))
                        .on_mouse_up(
                            MouseButton::Left,
                            |_: &MouseUpEvent, window: &mut Window, cx: &mut App| {
                                window.dispatch_action(Box::new(OpenThreads), cx);
                            },
                        )
                        .child("会话(临时)"),
                ),
        )
        .into_any_element()
}

fn main() {
    application().run(|cx: &mut App| {
        // Seed the global theme from the macOS appearance; components read it
        // via `vega_theme::theme(cx)`.
        let theme = Theme::system(cx);
        cx.set_global(theme);

        // Sidebar collapse preference, restored from config.toml before the
        // window opens so the first frame already matches the stored state.
        cx.set_global(SidebarCollapsed(load_collapsed()));

        // Settings view starts closed; the window render reads this global.
        cx.set_global(SettingsOpen(false));

        // Projects view starts closed (T10 temporary mount).
        cx.set_global(ProjectsOpen(false));

        // Temporary threads view starts closed; toggled by 会话(临时) / Cmd+N.
        cx.set_global(ThreadsOpen(false));

        // Key bindings for the vega_ui text input components.
        vega_ui::init(cx);

        // Open + migrate the project store (platform data root, tech-spec §6)
        // and install it as a global for the projects view (T10).
        vega_ui::projects::init(cx);

        // T11: open the persistent store for the temporary threads UI; on
        // failure the app still boots and the view degrades to an inline
        // error (ui-spec §4.6: no modals).
        if let Err(error) = vega_ui::threads::init(cx) {
            tracing::error!(%error, "failed to open the vega store");
        }

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

        cx.activate(true);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            // Temporary verification binding for the theme token mechanism.
            KeyBinding::new("cmd-shift-l", ToggleTheme, None),
            // Settings view switching (T08).
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("escape", CloseSettings, None),
            // Sidebar collapse toggle (T09).
            KeyBinding::new("cmd-b", ToggleSidebar, None),
            // Thread creation (T11): button and Cmd+N share one entry point.
            KeyBinding::new("cmd-n", NewThread, None),
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
        cx.on_action(|_: &OpenProjects, cx| {
            cx.set_global(ProjectsOpen(true));
            cx.refresh_windows();
        });
        cx.on_action(|_: &CloseProjects, cx| {
            cx.set_global(ProjectsOpen(false));
            cx.refresh_windows();
        });
        // Temporary threads view switching (T11).
        cx.on_action(|_: &OpenThreads, cx| {
            cx.set_global(ThreadsOpen(true));
            cx.refresh_windows();
        });
        cx.on_action(|_: &CloseThreads, cx| {
            cx.set_global(ThreadsOpen(false));
            cx.refresh_windows();
        });
        cx.on_action(move |_: &NewThread, cx| {
            if let Err(error) = window.update(cx, VegaWindow::open_new_thread) {
                tracing::error!(%error, "failed to handle Cmd+N in the main window");
            }
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
