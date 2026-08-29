//! Temporary threads UI (A1-02 stopgap before T12 integrates the sidebar).
//!
//! Mounted by the `vega` binary into the single-column content area behind
//! the 「会话(临时)」 entry; T12 replaces this mounting with the real sidebar
//! list. Provides the [新建任务] button + `Cmd+N` action, the minimal thread
//! list (title + updated_at), and the opened-thread placeholder pane
//! (「会话内容 S3 接入」空态).
//!
//! All SQLite access goes through `vega_conversation` (architecture red
//! line); this module only holds the store global and renders state.

use std::sync::Mutex;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Global, MouseButton, MouseUpEvent, Window, actions, div, px, relative,
};
use vega_conversation::threads as conversation;
use vega_conversation::types::{CurrentProject, Thread};
use vega_store::Store;
use vega_store::config;
use vega_theme::{Typography, theme};

actions!(vega_threads, [OpenThreads, CloseThreads, NewThread]);

/// Whether the temporary threads view currently replaces the placeholder.
///
/// Toggled by the app-level [`OpenThreads`]/[`CloseThreads`] handlers.
pub struct ThreadsOpen(pub bool);

impl Global for ThreadsOpen {}

/// The persistent SQLite store behind a mutex, registered as a GPUI global.
///
/// `rusqlite::Connection` is `Send` but not `Sync`; the mutex makes the
/// wrapper safe to keep in global state. Store failures surface as inline
/// errors in the threads view (ui-spec §4.6: no modals).
pub struct ThreadsStore(Mutex<Store>);

impl Global for ThreadsStore {}

/// Opens `vega.db` under the platform data root (tech-spec §6), applies
/// migrations, and registers [`ThreadsStore`].
///
/// Returns the failure message for the caller to log; the app still boots
/// and the threads view degrades to an inline error (ui-spec §4.6).
pub fn init(cx: &mut App) -> Result<(), String> {
    let dir = vega_store::paths::data_dir().ok_or_else(|| "未设置 HOME 环境变量".to_string())?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("创建 {} 失败：{error}", dir.display()))?;
    let path = dir.join("vega.db");
    let store = Store::open(&path)
        .map_err(|error| format!("打开数据库失败（{}）：{error}", path.display()))?;
    store
        .migrate()
        .map_err(|error| format!("数据库迁移失败：{error}"))?;
    cx.set_global(ThreadsStore(Mutex::new(store)));
    Ok(())
}

/// Runs `f` with the store; lock/global failures become inline error text.
fn with_store<R>(
    cx: &App,
    f: impl FnOnce(&Store) -> Result<R, vega_conversation::types::ConversationError>,
) -> Result<R, String> {
    let Some(store) = cx.try_global::<ThreadsStore>() else {
        return Err("数据库未初始化".to_string());
    };
    let guard = store.0.lock().map_err(|_| "数据库锁已损坏".to_string())?;
    f(&guard).map_err(|error| error.to_string())
}

/// The temporary threads view: minimal thread list + opened-thread pane.
///
/// Constructed each time the temporary view opens, so the list reloads from
/// the store (same cache-drop pattern as the settings view).
pub struct ThreadsView {
    project: Option<CurrentProject>,
    threads: Vec<Thread>,
    opened: Option<Thread>,
    /// Inline error message (ui-spec §4.6: no modals).
    error: Option<String>,
}

impl ThreadsView {
    /// Loads the current project and its thread list from the store.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            project: None,
            threads: Vec::new(),
            opened: None,
            error: None,
        };
        view.reload(cx);
        view
    }

    /// Reloads the current project and its thread list from the store.
    fn reload(&mut self, cx: &mut Context<Self>) {
        match with_store(cx, |store| {
            let project = conversation::current_project(store)?;
            let threads = match &project {
                Some(project) => conversation::list_threads(store, &project.id)?,
                None => Vec::new(),
            };
            Ok((project, threads))
        }) {
            Ok((project, threads)) => {
                self.project = project;
                self.threads = threads;
            }
            Err(message) => self.error = Some(message),
        }
    }

    /// Creates a thread from the config defaults and opens it (card: 建后
    /// 打开该 thread)。
    ///
    /// `model` comes from `defaults.model` (empty allowed until S4),
    /// `permission_mode` from `defaults` (empty → DDL default `confirm`).
    /// When the config file is missing, `config::load()` generates the
    /// template (`model=""`, `permission_mode="confirm"`), so thread
    /// creation works without a config file too.
    pub fn create_thread(&mut self, cx: &mut Context<Self>) {
        let (model, permission_mode) = match config::load() {
            Ok(cfg) => (cfg.defaults.model, cfg.defaults.permission_mode),
            Err(error) => {
                self.error = Some(format!("配置加载失败：{error}"));
                cx.notify();
                return;
            }
        };
        let Some(project) = self.project.clone() else {
            self.error = Some("暂无项目，无法新建任务（T10 项目注册就位后可添加）".to_string());
            cx.notify();
            return;
        };
        match with_store(cx, |store| {
            let thread = conversation::create_thread(store, &project.id, &model, &permission_mode)?;
            // 建后打开：touch thread.updated_at 与 project.last_opened_at。
            conversation::open_thread(store, &thread.id)
        }) {
            Ok(opened) => {
                self.opened = Some(opened);
                self.error = None;
            }
            Err(message) => self.error = Some(message),
        }
        self.reload(cx);
        cx.notify();
    }

    /// Opens a thread from the list: touch + switch the content pane.
    fn open(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        match with_store(cx, |store| conversation::open_thread(store, thread_id)) {
            Ok(opened) => {
                self.opened = Some(opened);
                self.error = None;
            }
            Err(message) => self.error = Some(message),
        }
        self.reload(cx);
        cx.notify();
    }

    /// The toolbar: back entry, page title, and the [新建任务] button.
    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::SIDEBAR))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_back))
                    .child("返回"),
            )
            .child(
                div()
                    .text_size(px(Typography::HEADING_PAGE))
                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                    .child(format!("会话（临时）— {}", self.project_name())),
            )
            .child(
                // [新建任务]：与 Cmd+N 同一入口（create_thread）。
                div()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .bg(colors.accent)
                    .text_color(colors.bg_base)
                    .text_size(px(Typography::SIDEBAR))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| this.create_thread(cx)),
                    )
                    .child("新建任务"),
            )
            .into_any_element()
    }

    /// Leaves the temporary view (same action path as the Esc-style close).
    fn on_back(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.dispatch_action(Box::new(CloseThreads), cx);
    }

    /// Display name of the current project (or the empty-project hint).
    fn project_name(&self) -> String {
        self.project
            .as_ref()
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "无项目".to_string())
    }

    /// The minimal thread list (title + updated_at), newest first.
    fn render_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id("threads-list")
            .w(px(280.))
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_sidebar)
            .overflow_y_scroll()
            .children(self.threads.is_empty().then(|| {
                div()
                    .text_color(colors.text_tertiary)
                    .text_size(px(Typography::BODY))
                    .child("暂无任务，点右上角「新建任务」开始")
                    .into_any_element()
            }))
            .children(
                self.threads
                    .iter()
                    .map(|thread| self.render_thread_row(thread, cx)),
            )
            .into_any_element()
    }

    /// One list row: title (「未命名任务」 when empty) + updated_at.
    fn render_thread_row(&self, thread: &Thread, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let selected = self
            .opened
            .as_ref()
            .is_some_and(|opened| opened.id == thread.id);
        let thread_id = thread.id.clone();
        let title = thread_title(thread);
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .px_3()
            .rounded_md()
            .text_size(px(Typography::SIDEBAR))
            .cursor_pointer()
            .when(selected, move |row| row.bg(colors.bg_active))
            .when(!selected, move |row| {
                row.hover(move |s| s.bg(colors.bg_hover))
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseUpEvent, _, cx| this.open(&thread_id, cx)),
            )
            .child(title)
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .text_size(px(Typography::SIDEBAR))
                    .child(format_timestamp(thread.updated_at)),
            )
            .into_any_element()
    }

    /// The content pane: thread title header + 「会话内容 S3 接入」空态.
    fn render_pane(&self, cx: &Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let Some(thread) = &self.opened else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(colors.text_tertiary)
                .text_size(px(Typography::BODY))
                .child("从左侧选择一个任务，或用「新建任务」（Cmd+N）创建")
                .into_any_element();
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .child(
                // 当前 thread 标题头。
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .border_b_1()
                    .border_color(colors.border_subtle)
                    .text_size(px(Typography::HEADING_PAGE))
                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                    .child(thread_title(thread)),
            )
            .child(
                // 空态占位：会话内容由 S3 接入。
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(colors.text_tertiary)
                    .text_size(px(Typography::BODY))
                    .child("会话内容将在 S3 接入后显示"),
            )
            .into_any_element()
    }
}

impl Render for ThreadsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .line_height(relative(Typography::BODY_LINE_HEIGHT))
            .child(self.render_toolbar(cx))
            .children(self.error.clone().map(|message| {
                div()
                    .mx_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(colors.bg_elevated)
                    .border_1()
                    .border_color(colors.danger)
                    .text_color(colors.danger)
                    .text_size(px(Typography::BODY))
                    .child(message)
            }))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .min_h_0()
                    .child(self.render_list(cx))
                    .child(self.render_pane(cx)),
            )
    }
}

/// Row/header label for a thread: 「未命名任务」 until T13 adds renaming.
fn thread_title(thread: &Thread) -> String {
    if thread.title.is_empty() {
        "未命名任务".to_string()
    } else {
        thread.title.clone()
    }
}

/// Formats unix milliseconds as `YYYY-MM-DD HH:MM` (UTC).
///
/// T11 stopgap display; T12 owns the relative-time format ("2h" style).
fn format_timestamp(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60
    )
}

/// Days-since-epoch → `(year, month, day)` (Howard Hinnant's
/// civil_from_days; no external date crate needed).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (era * 400 + yoe + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::{format_timestamp, thread_title};
    use vega_conversation::types::Thread;

    fn thread_with_title(title: &str) -> Thread {
        Thread {
            id: "t1".to_string(),
            project_id: "p1".to_string(),
            title: title.to_string(),
            mode: vega_conversation::types::ThreadMode::Execute,
            permission_mode: "confirm".to_string(),
            model: String::new(),
            status: vega_conversation::types::ThreadStatus::Active,
            pinned: false,
            unread: false,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn empty_title_falls_back_to_unnamed() {
        assert_eq!(thread_title(&thread_with_title("")), "未命名任务");
        assert_eq!(thread_title(&thread_with_title("我的任务")), "我的任务");
    }

    #[test]
    fn timestamps_format_as_utc_date_time() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00");
        // 2023-11-14 22:13:20 UTC。
        assert_eq!(format_timestamp(1_700_000_000_000), "2023-11-14 22:13");
        // 闰年边界：2024-02-29 00:00 UTC。
        assert_eq!(format_timestamp(1_709_164_800_000), "2024-02-29 00:00");
        // 负毫秒（早于纪元）不 panic，格式保持。
        assert_eq!(format_timestamp(-1), "1969-12-31 23:59");
    }
}
