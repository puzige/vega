//! Vega application entry point: boots the GPUI app and opens the main window.
//! The hidden `--vega-bench-render <out.json>` flag instead runs the S3-T17
//! render_frame self-measurement probe (see
//! [`vega_ui::conversation_stream::bench`]).

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Entity, KeyBinding, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, px, size,
};
use gpui_platform::application;
use vega_theme::{Theme, ThemeColors, Typography, theme};
use vega_ui::conversation_stream::{ConversationStream, bench as render_frame_bench};
use vega_ui::settings::{CloseSettings, OpenSettings, SettingsOpen, SettingsView};
use vega_ui::sidebar::{
    AUTO_COLLAPSE_WIDTH, CONTENT_MAX_WIDTH, CONTENT_MIN_PADDING, NewThread, OpenedThread,
    PendingDeleteConfirm, Sidebar, SidebarCollapsed, ToggleSidebar, load_collapsed,
    render_delete_confirm_overlay, toggle_persisted,
};

actions!(vega, [Quit, ToggleTheme]);

/// Initial (and minimum) main window size in logical pixels (UI spec §1).
const WINDOW_MIN_WIDTH: f32 = 960.0;
const WINDOW_MIN_HEIGHT: f32 = 600.0;

/// Quick-template placeholder labels for the empty state (ui-spec §4.6);
/// intentionally inert until the template feature lands (A7-02).
const EMPTY_STATE_TEMPLATES: [&str; 3] = ["快捷模板 1", "快捷模板 2", "快捷模板 3"];

/// Root view of the main window: the A1 layout shell — a sidebar (260px,
/// collapsible) next to a content column (max 820px, centered) that hosts
/// either the settings view (Cmd+, / Esc), the opened session
/// ([`ConversationStream`], S3-T17), or the ui-spec §4.6 empty state.
struct VegaWindow {
    /// Sidebar with the [新建任务] button, projects block, and sessions block.
    sidebar: Entity<Sidebar>,
    /// Cached settings view entity. Kept while settings is open so re-renders
    /// (e.g. the theme toggle) never rebuild the form mid-typing; dropped when
    /// settings closes so the next open reloads the config from disk.
    settings_view: Option<Entity<SettingsView>>,
    /// Cached conversation stream for the open thread (id, view). S3-T17:
    /// built lazily on first render of an opened thread; rebuilt when another
    /// thread is opened. The stream itself is memory-only (no persistence).
    stream_view: Option<(String, Entity<ConversationStream>)>,
}

impl VegaWindow {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            sidebar: cx.new(Sidebar::new),
            settings_view: None,
            stream_view: None,
        }
    }

    /// Whether the viewport is narrower than the auto-collapse threshold
    /// (ui-spec §1). Reads the live viewport size: every platform resize is
    /// delivered as an event (`Window::bounds_changed` → redraw), so each
    /// render sees the current size and no polling is involved.
    fn auto_collapsed(&self, window: &Window) -> bool {
        window.viewport_size().width < px(AUTO_COLLAPSE_WIDTH)
    }

    /// Cmd+N entry point: creates a thread in the selected project and opens
    /// it (the sidebar [新建任务] button shares this handler).
    fn open_new_thread(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.update(cx, Sidebar::create_thread);
    }
}

impl Render for VegaWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Palette comes from the global theme so Cmd+Shift+L repaints instantly.
        let colors = theme(cx).colors;
        // Effective visibility: the user preference (Cmd+B, persisted) AND the
        // viewport auto-collapse rule (ui-spec §1).
        let sidebar_visible = !cx.global::<SidebarCollapsed>().0 && !self.auto_collapsed(window);
        // T13 delete confirmation overlay: rendered above everything (window
        // root, absolute) while a delete is pending (裁决②).
        let pending_delete = cx.global::<PendingDeleteConfirm>().0.clone();

        // Settings opens inside the content area (T09 layout change of the
        // T08 view switching): the sidebar stays visible unless collapsed.
        // 路由收敛（T12 + T17）：内容区 = 设置 or 会话流 or §4.6 空态。
        let content: AnyElement = if cx.global::<SettingsOpen>().0 {
            // 设置视图：缓存 Entity，避免主题刷新等重渲染时重建导致表单输入丢失。
            let settings = self
                .settings_view
                .get_or_insert_with(|| cx.new(SettingsView::new));
            settings.clone().into_any_element()
        } else {
            // 设置已关闭：丢弃缓存，下次打开时重新构造并载入最新配置。
            self.settings_view = None;
            match cx.global::<OpenedThread>().0.clone() {
                Some(thread) => {
                    // S3-T17：会话流视图（每线程一个实体，切换会话时重建；
                    // MarkdownStream 内存态构造，不落库）。
                    let cached = match &self.stream_view {
                        Some((thread_id, view)) if *thread_id == thread.id => Some(view.clone()),
                        _ => None,
                    };
                    let stream = match cached {
                        Some(view) => view,
                        None => {
                            if let Some((_, previous)) = self.stream_view.take() {
                                previous.update(cx, |stream, cx| stream.timeout_permission(cx));
                            }
                            let view = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
                            self.stream_view = Some((thread.id.clone(), view.clone()));
                            view
                        }
                    };
                    stream.into_any_element()
                }
                None => {
                    if let Some((_, previous)) = self.stream_view.take() {
                        previous.update(cx, |stream, cx| stream.timeout_permission(cx));
                    }
                    render_empty_state(colors)
                }
            }
        };

        div()
            .size_full()
            .flex()
            .flex_row()
            .relative()
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
            // T13 删除确认弹层：最后绘制以覆盖全窗口；遮罩点击 / Esc 取消。
            .children(
                pending_delete.map(|thread| {
                    render_delete_confirm_overlay(&thread, self.sidebar.clone(), colors)
                }),
            )
    }
}

/// The content-area empty state (ui-spec §4.6): centered guidance with inert
/// quick-template placeholder buttons, inside the 820px content column —
/// no large logo illustration. The temporary T10/T11 entry buttons were
/// retired in T12 (projects/sessions now live in the sidebar).
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
                ),
        )
        .into_any_element()
}

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
        let theme = Theme::system(cx);
        cx.set_global(theme);

        // Sidebar collapse preference, restored from config.toml before the
        // window opens so the first frame already matches the stored state.
        cx.set_global(SidebarCollapsed(load_collapsed()));

        // Settings view starts closed; the window render reads this global.
        cx.set_global(SettingsOpen(false));

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
            // Thread creation (T11→T12): button and Cmd+N share one handler.
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
            // T13 裁决②：删除确认弹层存在时优先消费 Esc（弹层关闭后设置
            // 视图保持不变），行内重命名的 Esc 由其编辑器在更内层拦截。
            let overlay_open = cx
                .try_global::<PendingDeleteConfirm>()
                .is_some_and(|pending| pending.0.is_some());
            if overlay_open {
                cx.set_global(PendingDeleteConfirm(None));
            } else {
                cx.set_global(SettingsOpen(false));
            }
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
