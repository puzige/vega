//! Sidebar (T09 shell + T12 content + T13 session management): the fixed
//! 260px left column of the main window layout
//! ([vega-ui-spec.md §1](../../docs/vega-ui-spec.md)).
//!
//! Structure per the T12 architect ruling: the top [新建任务] button, then two
//! independent block components — [`ProjectsBlock`] (project list: select /
//! add / remove / sort toggle, branch suffix per row) and [`ThreadsBlock`]
//! (the selected project's sessions: pinned group first, `updated_at` desc)
//! — orchestrated by [`Sidebar`], which owns the cross-block wiring. The
//! automation entry stays grayed out until Phase 3 (A1-13).
//!
//! T13 (A1-05) adds the session management operations to [`ThreadsBlock`]:
//! per-row hover action groups (置顶 / 归档或恢复 / 删除), double-click inline
//! renaming (reusing [`crate::text_input::TextInput`]; Enter submits, Esc
//! cancels, an empty title cancels), the 「已归档 (N)」 collapsed section at
//! the bottom of the block, and the delete confirmation overlay
//! ([`render_delete_confirm_overlay`], driven by the [`PendingDeleteConfirm`]
//! global and rendered by the window root).
//!
//! State model:
//!
//! - Cmd+B ([`toggle_persisted`]) flips the [`SidebarCollapsed`] global and
//!   persists it as `ui.sidebar_collapsed` (T09 mechanism, serde default).
//! - Block collapse states persist the same way (`ui.projects_collapsed` /
//!   `ui.sessions_collapsed`, serde defaults keep older configs loadable).
//!   The archive-section expansion is in-memory only (T13 卡允许不做记忆).
//! - The selected project is cached in the [`SelectedProject`] global so
//!   per-frame renders never query the store; it is seeded once at startup
//!   from `vega_conversation`'s latest-project semantics and rewritten on
//!   row click. `None` → the session block shows guidance copy.
//! - The opened thread is cached in the [`OpenedThread`] global; the window
//!   root renders it as a [`crate::conversation_stream::ConversationStream`]
//!   view since S3-T17.
//!
//! The viewport auto-collapse rule (ui-spec §1) is applied by the window
//! root at render time.

use std::path::Path;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, ElementId, Entity, EventEmitter, Focusable, Global, MouseButton,
    MouseDownEvent, MouseUpEvent, PathPromptOptions, Window, actions, div, px,
};
use vega_conversation::threads as conversation;
use vega_conversation::types::{Thread, ThreadStatus};
use vega_store::Store;
use vega_store::config;
use vega_store::git_detect;
use vega_store::projects::{self, Project, ProjectSort};
use vega_theme::{ThemeColors, Typography, theme};

use crate::settings::CloseSettings;
use crate::text_input::TextInput;

actions!(vega_sidebar, [ToggleSidebar, NewThread, ConfirmRename]);

/// Sidebar width in logical pixels (ui-spec §1).
pub const SIDEBAR_WIDTH: f32 = 260.0;

/// Viewport width below which the sidebar auto-collapses (ui-spec §1).
pub const AUTO_COLLAPSE_WIDTH: f32 = 960.0;

/// Content column max width in logical pixels (ui-spec §1).
pub const CONTENT_MAX_WIDTH: f32 = 820.0;

/// Content column minimum horizontal padding in logical pixels (ui-spec §1).
pub const CONTENT_MIN_PADDING: f32 = 24.0;

/// Whether the user collapsed the sidebar with Cmd+B (T09, persisted as
/// `ui.sidebar_collapsed`). The effective sidebar visibility is
/// `!self.0 && viewport_width >= AUTO_COLLAPSE_WIDTH`.
pub struct SidebarCollapsed(pub bool);

impl Global for SidebarCollapsed {}

/// The project the 「会话」 block is scoped to (T12 architect ruling: cached
/// as a global so renders never query the store). Seeded at startup from
/// latest-project semantics; rewritten on project row click. `None` →
/// guidance copy instead of a thread list.
pub struct SelectedProject(pub Option<String>);

impl Global for SelectedProject {}

/// The opened-thread content column is rendered by the window root since
/// S3-T17: an inline [`crate::conversation_stream::ConversationStream`] view
/// (thread header + virtualized stream) replaces the former
/// `render_opened_thread_pane` placeholder, which was deleted with this card.
pub struct OpenedThread(pub Option<Thread>);

impl Global for OpenedThread {}

/// The thread awaiting deletion in the T13 confirmation overlay (裁决②：a
/// global carries the pending delete; `None` = nothing pending). The overlay
/// is rendered by the window root; clicking the scrim outside the card or
/// pressing Esc (routed through the existing global `CloseSettings` handler,
/// which consumes the overlay first) cancels it.
pub struct PendingDeleteConfirm(pub Option<Thread>);

impl Global for PendingDeleteConfirm {}

/// Whether the 「项目」 block is collapsed (persisted as
/// `ui.projects_collapsed`, T12 ruling: config 载体).
pub struct ProjectsCollapsed(pub bool);

impl Global for ProjectsCollapsed {}

/// Whether the 「会话」 block is collapsed (persisted as
/// `ui.sessions_collapsed`).
pub struct SessionsCollapsed(pub bool);

impl Global for SessionsCollapsed {}

/// The app store handle, installed once at startup by [`init`].
///
/// An init failure is carried as the `Err` half instead of aborting startup;
/// blocks render it as an inline danger bar (ui-spec §4.6).
pub struct VegaStore(pub Result<Store, String>);

impl Global for VegaStore {}

/// Loads the persisted Cmd+B collapse preference; `false` when the config
/// cannot be read (error logged, sidebar stays visible — the safe default).
pub fn load_collapsed() -> bool {
    match config::load() {
        Ok(config) => config.ui.sidebar_collapsed,
        Err(error) => {
            tracing::error!(%error, "failed to read sidebar_collapsed from config.toml");
            false
        }
    }
}

/// Cmd+B handler: flips the preference, persists it to `config.toml`, and
/// refreshes windows. Persistence failures degrade to in-memory state
/// (ui-spec §4.6: no modals); the next successful toggle rewrites the file.
pub fn toggle_persisted(cx: &mut App) {
    let collapsed = !cx.global::<SidebarCollapsed>().0;
    cx.set_global(SidebarCollapsed(collapsed));
    persist_ui(
        |config| config.ui.sidebar_collapsed = collapsed,
        "sidebar_collapsed",
        cx,
    );
}

/// Opens + migrates the store at the platform data root
/// ([`vega_store::paths::data_dir`](vega_store::paths)/`vega.db`, tech-spec
/// §6) and seeds the sidebar globals.
///
/// The selected project is seeded from latest-project semantics so the first
/// frame already shows the last-used project's sessions; block collapse
/// preferences come from config.toml. Store failures degrade to inline error
/// bars instead of aborting startup.
pub fn init(cx: &mut App) {
    let store = open_default_store();
    let selected = match &store {
        Ok(store) => conversation::current_project(store)
            .ok()
            .flatten()
            .map(|project| project.id),
        Err(_) => None,
    };
    cx.set_global(SelectedProject(selected));
    cx.set_global(VegaStore(store));
    let (projects_collapsed, sessions_collapsed) = load_block_state();
    cx.set_global(ProjectsCollapsed(projects_collapsed));
    cx.set_global(SessionsCollapsed(sessions_collapsed));
    cx.set_global(OpenedThread(None));
    cx.set_global(PendingDeleteConfirm(None));
}

/// Opens and migrates `vega.db` under the platform data root (tech-spec §6).
///
/// Failures come back as ready-to-render messages.
fn open_default_store() -> Result<Store, String> {
    let dir = vega_store::paths::data_dir()
        .ok_or_else(|| "未能确定用户主目录（HOME 未设置）".to_string())?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("创建 {} 失败：{error}", dir.display()))?;
    let path = dir.join("vega.db");
    let store =
        Store::open(&path).map_err(|error| format!("打开 {} 失败：{error}", path.display()))?;
    store
        .migrate()
        .map_err(|error| format!("数据库迁移失败：{error}"))?;
    Ok(store)
}

/// Reads both block collapse preferences; failures default to expanded and
/// are logged (same degradation as the sidebar collapse toggle).
fn load_block_state() -> (bool, bool) {
    match config::load() {
        Ok(config) => (config.ui.projects_collapsed, config.ui.sessions_collapsed),
        Err(error) => {
            tracing::error!(%error, "failed to read block collapse state from config.toml");
            (false, false)
        }
    }
}

/// Persists one `ui.*` preference change to config.toml and repaints the
/// windows. Persistence failures degrade to in-memory state (ui-spec §4.6);
/// the next successful write repairs the file.
fn persist_ui(mutate: impl FnOnce(&mut config::AppConfig), what: &'static str, cx: &mut App) {
    match config::load() {
        Ok(mut config) => {
            mutate(&mut config);
            if let Err(error) = config.save() {
                tracing::error!(%error, "failed to persist {what} to config.toml");
            }
        }
        Err(error) => {
            tracing::error!(%error, "failed to load config.toml to persist {what}");
        }
    }
    cx.refresh_windows();
}

/// 「项目」block collapse toggle: flips the global and persists it as
/// `ui.projects_collapsed` (T12 折叠记忆，同 T09 机制).
fn toggle_projects_block(cx: &mut App) {
    let collapsed = !cx.global::<ProjectsCollapsed>().0;
    cx.set_global(ProjectsCollapsed(collapsed));
    persist_ui(
        |config| config.ui.projects_collapsed = collapsed,
        "projects_collapsed",
        cx,
    );
}

/// 「会话」block collapse toggle (persists `ui.sessions_collapsed`).
fn toggle_sessions_block(cx: &mut App) {
    let collapsed = !cx.global::<SessionsCollapsed>().0;
    cx.set_global(SessionsCollapsed(collapsed));
    persist_ui(
        |config| config.ui.sessions_collapsed = collapsed,
        "sessions_collapsed",
        cx,
    );
}

/// Runs a store operation against the global store; store init failures
/// become ready-to-render messages.
fn with_store<R>(
    cx: &App,
    operation: impl FnOnce(&Store) -> Result<R, String>,
) -> Result<R, String> {
    match cx.try_global::<VegaStore>() {
        Some(VegaStore(Ok(store))) => operation(store),
        Some(VegaStore(Err(error))) => Err(format!("项目存储不可用：{error}")),
        None => Err("项目存储不可用：应用启动时未完成初始化".to_string()),
    }
}

/// The sidebar orchestrator: [新建任务] on top, the two block entities, and
/// the automation placeholder. Block components own their data + row
/// interactions; this struct only wires cross-block reactions (project
/// selection resyncs the session list, opening a thread refreshes the
/// project order) plus the T13 delete-confirmation execution
/// ([`Self::confirm_pending_delete`], invoked by the window-root overlay).
pub struct Sidebar {
    projects_block: Entity<ProjectsBlock>,
    sessions_block: Entity<ThreadsBlock>,
    /// Inline error from thread creation (ui-spec §4.6: no modals).
    new_task_error: Option<String>,
}

impl Sidebar {
    /// Builds the two block entities and subscribes to their events.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let projects_block = cx.new(ProjectsBlock::new);
        let sessions_block = cx.new(ThreadsBlock::new);
        cx.subscribe(&projects_block, Self::on_projects_event)
            .detach();
        cx.subscribe(&sessions_block, Self::on_sessions_event)
            .detach();
        Self {
            projects_block,
            sessions_block,
            new_task_error: None,
        }
    }

    /// Cross-block wiring for project selection/removal: the session block
    /// reloads, and a no-longer-valid opened thread is cleared.
    fn on_projects_event(
        &mut self,
        _: Entity<ProjectsBlock>,
        event: &ProjectsBlockEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            ProjectsBlockEvent::Selected(project_id) => {
                clear_opened_thread_of_other_project(project_id, cx);
                self.sessions_block.update(cx, ThreadsBlock::reload);
            }
            ProjectsBlockEvent::Removed(project_id) => {
                // 移除的是当前项目：回退到无选中态（会话块显示引导文案）。
                if cx.global::<SelectedProject>().0.as_deref() == Some(project_id.as_str()) {
                    cx.set_global(SelectedProject(None));
                }
                clear_opened_thread_of_other_project(project_id, cx);
                self.sessions_block.update(cx, ThreadsBlock::reload);
            }
        }
        cx.refresh_windows();
    }

    /// Opening a thread bumps the owning project's `last_opened_at`, so the
    /// project list (recently-opened sort) refreshes too.
    fn on_sessions_event(
        &mut self,
        _: Entity<ThreadsBlock>,
        _: &ThreadsBlockEvent,
        cx: &mut Context<Self>,
    ) {
        self.projects_block.update(cx, ProjectsBlock::reload);
        cx.refresh_windows();
    }

    /// [删除] confirmed in the T13 overlay: deletes the thread (the store
    /// layer removes its messages/tool_calls in the same transaction), falls
    /// back to the §4.6 empty state when the deleted thread was open, and
    /// reloads the session block. Failures surface as its inline error bar.
    pub fn confirm_pending_delete(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = cx.global::<PendingDeleteConfirm>().0.clone() else {
            return;
        };
        cx.set_global(PendingDeleteConfirm(None));
        let result = with_store(cx, |store| {
            conversation::delete_thread(store, &thread.id).map_err(|error| error.to_string())
        });
        match result {
            Ok(()) => {
                // 删除的是打开中的会话：内容区回落 §4.6 空态。
                if cx
                    .global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|opened| opened.id == thread.id)
                {
                    cx.set_global(OpenedThread(None));
                }
                self.sessions_block.update(cx, ThreadsBlock::reload);
            }
            Err(message) => self.sessions_block.update(cx, |block, _| {
                block.error = Some(format!("会话删除失败：{message}"));
            }),
        }
        cx.refresh_windows();
    }

    /// [新建任务] / Cmd+N shared entry point (环境限制：合成键盘事件送不进
    /// GPUI，按钮与快捷键共用 handler). Creates a thread in the selected
    /// project from the config defaults and opens it (T11 semantics).
    pub fn create_thread(&mut self, cx: &mut Context<Self>) {
        self.new_task_error = None;
        let Some(project_id) = cx.global::<SelectedProject>().0.clone() else {
            // 无项目：按钮已是禁用态；守卫保证即便触发也无副作用，行内提示。
            self.new_task_error = Some("暂无项目：先在下方「项目」区添加并选择".into());
            cx.notify();
            return;
        };
        let (model, permission_mode) = match config::load() {
            Ok(config) => (config.defaults.model, config.defaults.permission_mode),
            Err(error) => {
                self.new_task_error = Some(format!("配置加载失败：{error}"));
                cx.notify();
                return;
            }
        };
        let result = with_store(cx, |store| {
            let thread = conversation::create_thread(store, &project_id, &model, &permission_mode)
                .map_err(|error| error.to_string())?;
            // 建后打开：touch thread.updated_at + project.last_opened_at。
            conversation::open_thread(store, &thread.id).map_err(|error| error.to_string())
        });
        match result {
            Ok(opened) => {
                self.new_task_error = None;
                cx.set_global(OpenedThread(Some(opened)));
                self.sessions_block.update(cx, ThreadsBlock::reload);
                self.projects_block.update(cx, ProjectsBlock::reload);
            }
            Err(message) => self.new_task_error = Some(message),
        }
        cx.refresh_windows();
    }

    /// The [新建任务] button + the no-project inline hint. Disabled (inert,
    /// tertiary colors) while no project is selected — no modal (ui-spec
    /// §4.6).
    fn render_new_task(
        &mut self,
        cx: &mut Context<Self>,
        has_project: bool,
        colors: &ThemeColors,
    ) -> AnyElement {
        let (bg, fg) = if has_project {
            (colors.accent, colors.bg_base)
        } else {
            (colors.bg_hover, colors.text_tertiary)
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .bg(bg)
                    .text_color(fg)
                    .text_size(px(Typography::SIDEBAR))
                    .font_weight(Typography::HEADING_CARD_WEIGHT)
                    .when(has_project, |button| {
                        button.cursor_pointer().on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseUpEvent, _, cx| this.create_thread(cx)),
                        )
                    })
                    .child("新建任务"),
            )
            .when(!has_project, |hint| {
                hint.child(
                    div()
                        .text_size(px(Typography::SIDEBAR))
                        .text_color(colors.text_tertiary)
                        .child("暂无项目：先在下方「项目」区添加"),
                )
            })
            .children(
                self.new_task_error
                    .clone()
                    .map(|message| error_bar(message, colors)),
            )
            .into_any_element()
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let has_project = cx.global::<SelectedProject>().0.is_some();
        div()
            .id("sidebar")
            .flex()
            .flex_col()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_shrink_0()
            .bg(colors.bg_sidebar)
            .px_4()
            .pt_4()
            .pb_4()
            .gap_4()
            .overflow_y_scroll()
            .child(self.render_new_task(cx, has_project, &colors))
            .child(self.projects_block.clone())
            .child(self.sessions_block.clone())
            .child(automation_entry(&colors))
            .into_any_element()
    }
}

/// The automation entry (A1-13): grayed out and inert until Phase 3 (T09).
fn automation_entry(colors: &ThemeColors) -> AnyElement {
    div()
        .h(px(Typography::SIDEBAR_LINE_HEIGHT))
        .flex()
        .items_center()
        .text_size(px(Typography::SIDEBAR))
        .text_color(colors.text_tertiary)
        .child("自动化")
        .into_any_element()
}

/// Events emitted by the projects block; the sidebar orchestrator reacts.
pub enum ProjectsBlockEvent {
    /// A project row was clicked and is now the selected project.
    Selected(String),
    /// A project row was removed.
    Removed(String),
}

impl EventEmitter<ProjectsBlockEvent> for ProjectsBlock {}

/// The 「项目」 block: registered projects (name + branch suffix, non-git
/// rows show no suffix), click = select (`touch_last_opened`), [+] add via
/// the platform folder picker, per-row remove, and a name / recently-opened
/// sort toggle (in-memory per T12 ruling; drag ordering is deferred). The
/// collapse state persists in config (`ui.projects_collapsed`).
///
/// Independent component struct so T13 can add inline row actions here.
pub struct ProjectsBlock {
    /// Cached rows, refreshed from the store after every mutation.
    projects: Vec<Project>,
    /// Sort order — in-memory only (T12 ruling: 不持久化).
    sort: ProjectSort,
    /// Inline error message (ui-spec §4.6); empty until a failure occurs.
    error: Option<String>,
}

impl ProjectsBlock {
    /// Creates the block and loads the project list.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            projects: Vec::new(),
            // 侧边栏默认「最近打开」：与初始选中项目（latest_project 语义）一致。
            sort: ProjectSort::RecentlyOpened,
            error: None,
        };
        view.reload(cx);
        view
    }

    /// Re-reads the project list in the current sort order.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let sort = self.sort;
        match with_store(cx, |store| {
            projects::list(store.conn(), sort).map_err(|error| format!("项目列表加载失败：{error}"))
        }) {
            Ok(projects) => {
                self.projects = projects;
                self.error = None;
            }
            Err(message) => self.error = Some(message),
        }
        cx.notify();
    }

    /// Click = select: touch `last_opened_at`, cache the selection in the
    /// global, then let the orchestrator resync the session block.
    fn select_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let result = with_store(cx, |store| {
            projects::touch_last_opened(store.conn(), project_id)
                .map_err(|error| format!("项目状态更新失败：{error}"))
        });
        match result {
            Ok(_) => {
                self.error = None;
                cx.set_global(SelectedProject(Some(project_id.to_string())));
                self.reload(cx);
                cx.emit(ProjectsBlockEvent::Selected(project_id.to_string()));
            }
            Err(message) => {
                self.error = Some(message);
                cx.notify();
            }
        }
    }

    /// Removes the project row (database only; files on disk are never
    /// touched — S2 ruling: no confirmation layer).
    fn remove_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let result = with_store(cx, |store| {
            projects::remove(store.conn(), project_id)
                .map_err(|error| format!("项目移除失败：{error}"))
        });
        match result {
            Ok(_) => {
                self.error = None;
                self.reload(cx);
                cx.emit(ProjectsBlockEvent::Removed(project_id.to_string()));
            }
            Err(message) => {
                self.error = Some(message);
                cx.notify();
            }
        }
    }

    /// Opens the platform folder picker and registers the picked folder
    /// (T10 logic, relocated). The picker answers asynchronously (oneshot);
    /// the future runs on the foreground executor, so every store access
    /// stays on the main thread.
    fn on_add_clicked(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择要注册为项目的文件夹".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = match receiver.await {
                Ok(Ok(Some(paths))) => paths,
                // 用户取消（None）或通道关闭（应用退出）都静默结束。
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    let message = format!("文件夹选择失败：{error}");
                    this.update(cx, |this, cx| {
                        this.error = Some(message);
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            this.update(cx, |this, cx| {
                for path in &picked {
                    this.register_path(path, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Registers `path` as a project: detects the git branch (zero-dependency
    /// `.git`/HEAD parsing) and inserts a row; the folder's own file name is
    /// the display name. Re-registering an already-registered path surfaces
    /// as the inline danger bar.
    fn register_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        // 项目名取文件夹名；无名可用（如文件系统根）时退回完整路径。
        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_string(),
            None => path.to_string_lossy().into_owned(),
        };
        let path_text = path.to_string_lossy().into_owned();
        let branch = git_detect::detect_git(path);
        let result = with_store(cx, |store| {
            match projects::create(store.conn(), &path_text, &name, branch.as_deref()) {
                Ok(_) => Ok(()),
                Err(vega_store::projects::ProjectsError::PathAlreadyRegistered(registered)) => {
                    Err(format!("该文件夹已注册过项目：{registered}"))
                }
                Err(error) => Err(format!("项目注册失败：{error}")),
            }
        });
        match result {
            Ok(()) => self.error = None,
            Err(message) => self.error = Some(message),
        }
        self.reload(cx);
    }

    /// Block header: collapsible title (chevron shows the state) + [+].
    fn render_header(&self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        let collapsed = cx.global::<ProjectsCollapsed>().0;
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|_, _: &MouseUpEvent, _, cx| toggle_projects_block(cx)),
                    )
                    .child(
                        div()
                            .text_size(px(Typography::HEADING_BLOCK))
                            .font_weight(Typography::HEADING_BLOCK_WEIGHT)
                            .text_color(colors.text_primary)
                            .child("项目"),
                    )
                    .child(div().text_color(colors.text_tertiary).child(if collapsed {
                        "▸"
                    } else {
                        "▾"
                    })),
            )
            .child(
                // [+] 添加：T10 的系统文件夹选择器逻辑原样复用。
                div()
                    .px_1()
                    .rounded_md()
                    .text_size(px(Typography::HEADING_BLOCK))
                    .text_color(colors.text_secondary)
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover).text_color(colors.text_primary))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_add_clicked))
                    .child("+"),
            )
            .into_any_element()
    }

    /// The sort toggle (名称 / 最近打开, T10 capability kept) + the project
    /// rows. Row = name (truncate) + branch suffix (git only) + remove.
    fn render_body(&self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        let selected = cx.global::<SelectedProject>().0.clone();
        div()
            .flex()
            .flex_col()
            .child(self.render_sort_row(cx, colors))
            .children(self.projects.is_empty().then(|| {
                div()
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex()
                    .items_center()
                    .px_1()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_tertiary)
                    .child("点击 [+] 添加文件夹")
                    .into_any_element()
            }))
            .children(self.projects.iter().map(|project| {
                let project_id = project.id.clone();
                let remove_id = project.id.clone();
                let is_selected = selected.as_deref() == Some(project.id.as_str());
                // 分支后缀：仅 git 目录显示（T12 卡：非 git 不显示）。
                let branch = project.git_default_branch.clone();
                div()
                    .flex()
                    .items_center()
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .rounded_md()
                    .overflow_hidden()
                    .text_size(px(Typography::SIDEBAR))
                    .when(is_selected, move |row| row.bg(colors.bg_active))
                    .child(
                        // 可点击主体与移除按钮是兄弟节点，避免嵌套命中
                        // 一次点击触发两个操作（T10 经验）。
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .pl_2()
                            .cursor_pointer()
                            .when(!is_selected, move |main| {
                                main.hover(move |s| s.bg(colors.bg_hover))
                            })
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                    this.select_project(&project_id, cx);
                                }),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_color(colors.text_primary)
                                    .child(project.name.clone()),
                            )
                            .children(branch.map(|branch| {
                                div()
                                    .flex_shrink_0()
                                    .text_color(colors.text_tertiary)
                                    .child(branch)
                            })),
                    )
                    .child(
                        div()
                            .px_2()
                            .h_full()
                            .flex()
                            .items_center()
                            .text_color(colors.text_secondary)
                            .cursor_pointer()
                            .hover(move |s| s.text_color(colors.danger))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                    this.remove_project(&remove_id, cx);
                                }),
                            )
                            .child("×"),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// 名称 / 最近打开 chips (T10 capability, sidebar-compact form).
    fn render_sort_row(&self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .children(
                [
                    (ProjectSort::Name, "名称"),
                    (ProjectSort::RecentlyOpened, "最近打开"),
                ]
                .map(|(sort, label)| {
                    let selected = self.sort == sort;
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_size(px(Typography::SIDEBAR))
                        .cursor_pointer()
                        .text_color(if selected {
                            colors.text_primary
                        } else {
                            colors.text_secondary
                        })
                        .when(selected, move |chip| chip.bg(colors.bg_active))
                        .when(!selected, move |chip| {
                            chip.hover(move |s| s.bg(colors.bg_hover))
                        })
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                this.sort = sort;
                                this.reload(cx);
                            }),
                        )
                        .child(label)
                }),
            )
            .into_any_element()
    }
}

impl Render for ProjectsBlock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let collapsed = cx.global::<ProjectsCollapsed>().0;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(self.render_header(cx, &colors))
            .children(
                self.error
                    .clone()
                    .map(|message| error_bar(message, &colors)),
            )
            .when(!collapsed, |block| {
                block.child(self.render_body(cx, &colors))
            })
            .into_any_element()
    }
}

/// Events emitted by the session block.
pub enum ThreadsBlockEvent {
    /// A thread row was opened; the content column switched to it.
    Opened,
}

impl EventEmitter<ThreadsBlockEvent> for ThreadsBlock {}

/// The 「会话」 block: the selected project's threads, pinned group first,
/// then `updated_at` desc (store ordering, ui-spec §4.1 置顶组优先). Rows =
/// truncated title + relative time ("2h" style); the selected row gets
/// `bg_active` + a 2px accent bar on the left; unread rows render medium
/// weight + a dot (the field stays 0 until S3 produces unread state).
///
/// T13 (A1-05) session management: the main list reads `status = active`
/// only; archived threads hide here and surface in the 「已归档 (N)」
/// collapsed section at the bottom of the block (展开可查看，行上有「恢复」).
/// Hovering a row reveals its action group (裁决①：置顶 / 归档或恢复 / 删除,
/// ≤3 small buttons); double-clicking a row enters inline renaming via the
/// shared [`TextInput`] (Enter submits, Esc cancels, empty title = cancel —
/// the keyboard path itself is manual-acceptance, see [`resolve_rename`]).
/// No project selected → guidance copy. Collapse state persists in config
/// (`ui.sessions_collapsed`); the archive expansion is in-memory only.
pub struct ThreadsBlock {
    /// Cached active rows for the project in [`Self::loaded_project`].
    threads: Vec<Thread>,
    /// Cached archived rows (shown in the 「已归档」 collapsed section).
    archived: Vec<Thread>,
    /// Project id the cache was loaded for (`None` → guidance copy).
    loaded_project: Option<String>,
    /// Thread id currently under the mouse; drives the hover action group.
    hovered: Option<String>,
    /// Whether the 「已归档 (N)」 section is expanded (in-memory, T13 卡允许
    /// 不做折叠记忆).
    archive_expanded: bool,
    /// Active inline rename session (`None` = not renaming).
    editing: Option<RenameSession>,
    /// Inline error message (ui-spec §4.6).
    error: Option<String>,
}

/// An inline rename in progress: the thread being renamed plus the shared
/// [`TextInput`] entity pre-filled with its current title.
struct RenameSession {
    thread_id: String,
    input: Entity<TextInput>,
}

impl ThreadsBlock {
    /// Creates the block and loads the selected project's thread list.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            threads: Vec::new(),
            archived: Vec::new(),
            loaded_project: None,
            hovered: None,
            archive_expanded: false,
            editing: None,
            error: None,
        };
        view.reload(cx);
        view
    }

    /// Re-reads the selected project's thread lists: active rows for the
    /// main list, archived rows for the 「已归档」 section (both pinned
    /// first, then updated_at desc — store ordering).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let selected = cx.global::<SelectedProject>().0.clone();
        self.loaded_project = selected.clone();
        let result = match selected {
            None => Ok((Vec::new(), Vec::new())),
            Some(project_id) => with_store(cx, |store| {
                let active =
                    conversation::list_threads(store, &project_id, Some(ThreadStatus::Active))
                        .map_err(|error| error.to_string())?;
                let archived =
                    conversation::list_threads(store, &project_id, Some(ThreadStatus::Archived))
                        .map_err(|error| error.to_string())?;
                Ok((active, archived))
            }),
        };
        match result {
            Ok((threads, archived)) => {
                self.threads = threads;
                self.archived = archived;
                self.error = None;
            }
            Err(message) => self.error = Some(message),
        }
        // 编辑中的线程被删除后不再渲染其编辑器（输入实体随之释放）。
        if let Some(session) = &self.editing {
            let exists = self
                .threads
                .iter()
                .chain(self.archived.iter())
                .any(|thread| thread.id == session.thread_id);
            if !exists {
                self.editing = None;
            }
        }
        cx.notify();
    }

    /// Click = open: bumps `threads.updated_at` + the owning project's
    /// `last_opened_at` (single transaction) and switches the content column
    /// via the [`OpenedThread`] global.
    fn open_thread(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let result = with_store(cx, |store| {
            conversation::open_thread(store, thread_id).map_err(|error| error.to_string())
        });
        match result {
            Ok(opened) => {
                self.error = None;
                cx.set_global(OpenedThread(Some(opened)));
                self.reload(cx);
                cx.emit(ThreadsBlockEvent::Opened);
            }
            Err(message) => {
                self.error = Some(message);
                cx.notify();
            }
        }
    }

    /// Runs one thread mutation, reloads the cached rows, and surfaces a
    /// failure as the block's inline error bar (ui-spec §4.6).
    fn mutate_thread(
        &mut self,
        operation: impl FnOnce(&Store) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let result = with_store(cx, operation);
        self.reload(cx);
        if let Err(message) = result {
            self.error = Some(message);
        }
        cx.notify();
    }

    /// Hover pin toggle (裁决①：hover 操作组里的置顶切换，再点取消).
    fn toggle_pin(&mut self, thread_id: &str, pinned: bool, cx: &mut Context<Self>) {
        self.mutate_thread(
            |store| {
                conversation::set_thread_pinned(store, thread_id, !pinned)
                    .map_err(|error| error.to_string())
            },
            cx,
        );
    }

    /// 归档 (active → archived) or 恢复 (archived → active).
    fn set_thread_status(&mut self, thread_id: &str, status: ThreadStatus, cx: &mut Context<Self>) {
        self.mutate_thread(
            |store| {
                conversation::set_thread_status(store, thread_id, status)
                    .map_err(|error| error.to_string())
            },
            cx,
        );
    }

    /// 「删除」 hover entry: parks the thread in [`PendingDeleteConfirm`];
    /// the window root renders the confirmation overlay (ui-spec §4.6: no
    /// system modal). Any inline rename is folded first so the overlay's Esc
    /// semantics stay unambiguous.
    fn request_delete(&mut self, thread: &Thread, cx: &mut Context<Self>) {
        self.editing = None;
        cx.set_global(PendingDeleteConfirm(Some(thread.clone())));
        cx.refresh_windows();
    }

    /// Double-click = rename: builds the inline editor pre-filled with the
    /// current title and moves focus into it. (合成键盘事件送不进 GPUI：键入
    /// 与 Enter/Esc 提交路径为人工验收；提交/取消的纯逻辑见 [`resolve_rename`]。)
    fn start_rename(&mut self, thread: &Thread, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .editing
            .as_ref()
            .is_some_and(|session| session.thread_id == thread.id)
        {
            return;
        }
        let input = cx.new(|cx| TextInput::new(cx, "会话标题", false));
        input.update(cx, |input, cx| input.set_text(&thread.title, cx));
        self.editing = Some(RenameSession {
            thread_id: thread.id.clone(),
            input: input.clone(),
        });
        let focus_handle = input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
        cx.notify();
    }

    /// Enter on the rename editor: an empty title cancels (不写库); otherwise
    /// the trimmed title is persisted and the opened thread's cached copy is
    /// resynced so the content header shows the new title.
    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.editing.take() else {
            return;
        };
        let raw = session.input.read(cx).text().to_string();
        match resolve_rename(&raw) {
            RenameResolution::Cancel => {
                // 空标题提交视为取消：直接退出编辑态，不写库。
                cx.notify();
            }
            RenameResolution::Commit(title) => {
                let result = with_store(cx, |store| {
                    conversation::rename_thread(store, &session.thread_id, &title)
                        .map_err(|error| error.to_string())
                });
                // reload 先行：失败信息在其后写入，避免被 reload 清空。
                self.reload(cx);
                match result {
                    Ok(renamed) => {
                        let mut opened = cx.global::<OpenedThread>().0.clone();
                        if let Some(opened) =
                            opened.as_mut().filter(|opened| opened.id == renamed.id)
                        {
                            *opened = renamed;
                        }
                        cx.set_global(OpenedThread(opened));
                    }
                    Err(message) => self.error = Some(message),
                }
                cx.refresh_windows();
            }
        }
    }

    /// Esc on the rename editor (intercepted from the global `CloseSettings`
    /// action while the editor is mounted): exits editing without writing.
    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            cx.notify();
        }
    }

    /// Hover bookkeeping for one row; only real changes notify.
    fn set_hovered(&mut self, thread_id: &str, hovered: bool, cx: &mut Context<Self>) {
        let changed = if hovered {
            if self.hovered.as_deref() != Some(thread_id) {
                self.hovered = Some(thread_id.to_string());
                true
            } else {
                false
            }
        } else if self.hovered.as_deref() == Some(thread_id) {
            self.hovered = None;
            true
        } else {
            false
        };
        if changed {
            cx.notify();
        }
    }

    /// Block header: collapsible title (chevron shows the state).
    fn render_header(&self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        let collapsed = cx.global::<SessionsCollapsed>().0;
        div()
            .flex()
            .items_center()
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|_, _: &MouseUpEvent, _, cx| toggle_sessions_block(cx)),
                    )
                    .child(
                        div()
                            .text_size(px(Typography::HEADING_BLOCK))
                            .font_weight(Typography::HEADING_BLOCK_WEIGHT)
                            .text_color(colors.text_primary)
                            .child("会话"),
                    )
                    .child(div().text_color(colors.text_tertiary).child(if collapsed {
                        "▸"
                    } else {
                        "▾"
                    })),
            )
            .into_any_element()
    }

    /// The session rows (active only), or the guidance copy when no project
    /// is selected / no thread exists. The 「已归档 (N)」 collapsed section
    /// trails the active rows.
    fn render_body(&self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        let opened_id = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone());
        let guidance = |message: &'static str| {
            div()
                .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                .flex()
                .items_center()
                .px_1()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_tertiary)
                .child(message)
                .into_any_element()
        };
        if self.loaded_project.is_none() {
            return guidance("暂无项目：先在「项目」区选择").into_any_element();
        }
        if self.threads.is_empty() && self.archived.is_empty() {
            return guidance("暂无会话：点顶部「新建任务」开始").into_any_element();
        }
        let mut body = div().flex().flex_col();
        if self.threads.is_empty() {
            // 仅剩归档线程：主列表给一行引导，归档折叠区照常可见可展开。
            body = body.child(guidance("暂无活跃会话"));
        } else {
            body = body.children(
                self.threads
                    .iter()
                    .map(|thread| self.render_row(thread, &opened_id, false, cx)),
            );
        }
        body.children(
            archive_section_visible(self.archived.len())
                .then(|| self.render_archive_header(cx, colors)),
        )
        .when(
            self.archive_expanded && archive_section_visible(self.archived.len()),
            |column| {
                column.children(
                    self.archived
                        .iter()
                        .map(|thread| self.render_row(thread, &opened_id, true, cx)),
                )
            },
        )
        .into_any_element()
    }

    /// 「已归档 (N)」折叠区入口：chevron 显示展开态，点击切换（本卡不持久化
    /// 该折叠状态）。
    fn render_archive_header(&self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        div()
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(colors.bg_hover))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.archive_expanded = !this.archive_expanded;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_secondary)
                    .child(format!("已归档 ({})", self.archived.len())),
            )
            .child(
                div()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_tertiary)
                    .child(if self.archive_expanded { "▾" } else { "▸" }),
            )
            .into_any_element()
    }

    /// One session row per ui-spec §4.1: [2px accent bar][pin mark][title…]
    /// [dot] [relative time | hover action group]. The selected row gets
    /// `bg_active`; hovering a non-editing row swaps the time label for its
    /// action group (裁决①：置顶 / 归档或恢复 / 删除；行内编辑行除外)。
    ///
    /// The clickable body and the right side are sibling nodes — clicks on
    /// the action buttons must not re-trigger open (T10 经验：兄弟节点避免
    /// 嵌套命中).
    fn render_row(
        &self,
        thread: &Thread,
        opened_id: &Option<String>,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let selected = opened_id.as_deref() == Some(thread.id.as_str());
        let hovered = self.hovered.as_deref() == Some(thread.id.as_str());
        let editing_session = self
            .editing
            .as_ref()
            .filter(|session| session.thread_id == thread.id);
        let editing_this_row = editing_session.is_some();
        let actions_visible = row_shows_actions(hovered, editing_this_row);
        let thread_id = thread.id.clone();
        let row_thread = thread.clone();
        let mut row = div()
            .id(ElementId::Name(format!("thread-row-{thread_id}").into()))
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .flex()
            .items_center()
            .rounded_md()
            .overflow_hidden()
            .text_size(px(Typography::SIDEBAR))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.set_hovered(&thread_id, *hovered, cx);
            }))
            .when(selected || (hovered && !editing_this_row), move |row| {
                row.bg(if selected {
                    colors.bg_active
                } else {
                    colors.bg_hover
                })
            })
            // 左侧 2px 强调条（选中态着 accent 色，未选中占位保持对齐）。
            .child(
                div()
                    .w(px(2.))
                    .h_full()
                    .flex_shrink_0()
                    .when(selected, move |bar| bar.bg(colors.accent)),
            );
        if let Some(session) = editing_session {
            row = row.child(self.render_rename_editor(session, cx));
        } else {
            row = row
                // 可点击主体与右侧操作是兄弟节点：双击进入行内编辑，单击
                // 打开会话（双击序列的第一次单击仍会先打开，属预期）。
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .pl_2()
                        .cursor_pointer()
                        .when(!selected, move |main| {
                            main.hover(move |s| s.bg(colors.bg_hover))
                        })
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                                if event.click_count >= 2 {
                                    this.start_rename(&row_thread, window, cx);
                                } else {
                                    this.open_thread(&row_thread.id, cx);
                                }
                            }),
                        )
                        .children(thread.pinned.then(|| {
                            // 置顶小标记：token 色着色（裁决③）。
                            div().flex_shrink_0().text_color(colors.accent).child("▲")
                        }))
                        .child(
                            div()
                                .truncate()
                                .text_color(colors.text_primary)
                                .when(thread.unread, |title| {
                                    title.font_weight(Typography::HEADING_CARD_WEIGHT)
                                })
                                .child(thread_title(thread)),
                        ),
                )
                // 未读圆点（数据恒 0 至 S3；显示逻辑本卡落地）。
                .children(
                    thread
                        .unread
                        .then(|| div().size(px(6.)).rounded_full().bg(colors.accent)),
                );
            if actions_visible {
                row = row.child(self.render_row_actions(thread, archived, cx));
            } else {
                row = row.child(
                    div()
                        .flex_shrink_0()
                        .px_2()
                        .text_color(colors.text_secondary)
                        .child(relative_time(thread.updated_at)),
                );
            }
        }
        row.into_any_element()
    }

    /// The hover action group (裁决①：每组最多 3 个小按钮). Active rows get
    /// [置顶/取消置顶][归档][删除]; archived rows get [恢复][删除].
    fn render_row_actions(
        &self,
        thread: &Thread,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let thread_id = thread.id.clone();
        let mut group = div().flex().items_center().gap_0p5().flex_shrink_0().pr_1();
        if archived {
            group = group.child(row_action_button(
                "恢复",
                colors.text_secondary,
                colors.bg_active,
                move |this, _: &MouseUpEvent, _, cx| {
                    this.set_thread_status(&thread_id, ThreadStatus::Active, cx);
                },
                cx,
            ));
        } else {
            let (pin_label, now_pinned) = if thread.pinned {
                ("取消置顶", true)
            } else {
                ("置顶", false)
            };
            group = group.child(row_action_button(
                pin_label,
                colors.text_secondary,
                colors.bg_active,
                move |this, _: &MouseUpEvent, _, cx| {
                    this.toggle_pin(&thread_id, now_pinned, cx);
                },
                cx,
            ));
            let thread_id = thread.id.clone();
            group = group.child(row_action_button(
                "归档",
                colors.text_secondary,
                colors.bg_active,
                move |this, _: &MouseUpEvent, _, cx| {
                    this.set_thread_status(&thread_id, ThreadStatus::Archived, cx);
                },
                cx,
            ));
        }
        let thread_for_delete = thread.clone();
        group
            .child(row_action_button(
                "删除",
                colors.danger,
                colors.bg_active,
                move |this, _: &MouseUpEvent, _, cx| {
                    let thread = thread_for_delete.clone();
                    this.request_delete(&thread, cx);
                },
                cx,
            ))
            .into_any_element()
    }

    /// The inline rename editor mounted in place of the row body: the shared
    /// [`TextInput`] under a `ThreadRename` key context. Enter dispatches
    /// [`ConfirmRename`] (bound to this context in `vega_ui::init`); Esc is
    /// intercepted from the global [`CloseSettings`] action, which the
    /// editor consumes so settings never closes mid-rename.
    fn render_rename_editor(&self, session: &RenameSession, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .min_w_0()
            .pr_1()
            .key_context("ThreadRename")
            .track_focus(&session.input.read(cx).focus_handle(cx))
            .on_action(cx.listener(|this, _: &ConfirmRename, _, cx| this.commit_rename(cx)))
            .on_action(cx.listener(|this, _: &CloseSettings, _, cx| this.cancel_rename(cx)))
            .child(session.input.clone())
            .into_any_element()
    }
}

impl Render for ThreadsBlock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let collapsed = cx.global::<SessionsCollapsed>().0;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(self.render_header(cx, &colors))
            .children(
                self.error
                    .clone()
                    .map(|message| error_bar(message, &colors)),
            )
            .when(!collapsed, |block| {
                block.child(self.render_body(cx, &colors))
            })
            .into_any_element()
    }
}

/// A single hover action button on a session row (compact text label, token
/// colors only). The listener runs on the block entity, so button clicks do
/// not bubble into the row's clickable body (sibling nodes, T10 经验).
fn row_action_button(
    label: &'static str,
    text_color: gpui::Rgba,
    hover_bg: gpui::Rgba,
    listener: impl Fn(&mut ThreadsBlock, &MouseUpEvent, &mut Window, &mut Context<ThreadsBlock>)
    + 'static,
    cx: &mut Context<ThreadsBlock>,
) -> AnyElement {
    div()
        .px_1()
        .rounded_md()
        .text_size(px(Typography::SIDEBAR))
        .text_color(text_color)
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .on_mouse_up(MouseButton::Left, cx.listener(listener))
        .child(label)
        .into_any_element()
}

/// The full-window delete confirmation overlay (T13): a token-derived
/// semi-transparent scrim over everything with a centered small card in the
/// ui-spec §4.3 权限卡 style — no shadow, `border_subtle` border, buttons
/// [删除] (danger) + [取消]. Clicking the scrim (any mouse down outside the
/// card) or pressing Esc cancels — Esc routes through the global
/// `CloseSettings` handler, which consumes the overlay first (裁决②). No
/// system modal is used (ui-spec §4.6). Rendered by the window root while
/// [`PendingDeleteConfirm`] is `Some`.
pub fn render_delete_confirm_overlay(
    thread: &Thread,
    sidebar: Entity<Sidebar>,
    colors: ThemeColors,
) -> AnyElement {
    let cancel = |_event: &MouseDownEvent, _window: &mut Window, cx: &mut App| {
        cx.set_global(PendingDeleteConfirm(None));
        cx.refresh_windows();
    };
    div()
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(colors.text_primary.opacity(0.4))
        .child(
            div()
                .w(px(320.))
                .flex()
                .flex_col()
                .gap_3()
                .rounded_lg()
                .border_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_elevated)
                .px_4()
                .py_4()
                .on_mouse_down_out(cancel)
                .child(
                    div()
                        .text_size(px(Typography::HEADING_CARD))
                        .font_weight(Typography::HEADING_CARD_WEIGHT)
                        .text_color(colors.text_primary)
                        .child("删除会话"),
                )
                .child(
                    div()
                        .text_size(px(Typography::SIDEBAR))
                        .text_color(colors.text_secondary)
                        .child(format!(
                            "确定删除「{}」？将同时删除该会话的全部消息与工具调用记录，且无法恢复。",
                            thread_title(thread)
                        )),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            // [取消]
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(colors.border_subtle)
                                .text_size(px(Typography::SIDEBAR))
                                .text_color(colors.text_secondary)
                                .cursor_pointer()
                                .hover(move |s| s.bg(colors.bg_hover))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    |_: &MouseUpEvent, _window, cx: &mut App| {
                                        cx.set_global(PendingDeleteConfirm(None));
                                        cx.refresh_windows();
                                    },
                                )
                                .child("取消"),
                        )
                        .child(
                            // [删除]：danger 主操作；确认后由编排器执行删除。
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(colors.danger)
                                .text_size(px(Typography::SIDEBAR))
                                .text_color(colors.bg_base)
                                .cursor_pointer()
                                .hover(move |s| s.bg(colors.danger.opacity(0.85)))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    move |_: &MouseUpEvent, _window, cx: &mut App| {
                                        sidebar.update(cx, Sidebar::confirm_pending_delete);
                                    },
                                )
                                .child("删除"),
                        ),
                ),
        )
        .into_any_element()
}

/// Clears the cached opened thread when it belongs to a project other than
/// `project_id` (used on project selection/removal).
fn clear_opened_thread_of_other_project(project_id: &str, cx: &mut App) {
    let stale = cx
        .global::<OpenedThread>()
        .0
        .as_ref()
        .is_some_and(|thread| thread.project_id != project_id);
    if stale {
        cx.set_global(OpenedThread(None));
    }
}

/// Inline danger bar (ui-spec §4.6: errors are inline, never modals).
fn error_bar(message: String, colors: &ThemeColors) -> AnyElement {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(colors.bg_elevated)
        .border_1()
        .border_color(colors.danger)
        .text_color(colors.danger)
        .text_size(px(Typography::SIDEBAR))
        .child(message)
        .into_any_element()
}

/// Row/header label for a thread: 「未命名任务」 until T13 adds renaming.
fn thread_title(thread: &Thread) -> String {
    if thread.title.is_empty() {
        "未命名任务".to_string()
    } else {
        thread.title.clone()
    }
}

/// Outcome of a rename submission (pure decision — the Enter/Esc key path
/// itself is manual-acceptance because synthetic keyboard events cannot
/// reach GPUI in this environment; this decision must stay unit-tested).
enum RenameResolution {
    /// 空标题（含纯空白）提交视为取消：退出编辑态，不写库。
    Cancel,
    /// 提交去首尾空白后的新标题。
    Commit(String),
}

/// Classifies a rename submission: blank input cancels, anything else
/// commits trimmed.
fn resolve_rename(raw: &str) -> RenameResolution {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        RenameResolution::Cancel
    } else {
        RenameResolution::Commit(trimmed.to_string())
    }
}

/// Whether a session row currently shows its hover action group (裁决①)：
/// the group appears only while the row is hovered and never while that row
/// is in inline-rename editing (避免编辑态与行操作叠加).
fn row_shows_actions(row_hovered: bool, row_editing: bool) -> bool {
    row_hovered && !row_editing
}

/// 「已归档 (N)」折叠区只在确有归档线程时出现；归档计数即折叠区标题里的 N.
fn archive_section_visible(archived_count: usize) -> bool {
    archived_count > 0
}

/// Relative time for a session row (ui-spec §4.1, "2h" style).
fn relative_time(updated_at_ms: i64) -> String {
    relative_time_from(updated_at_ms, now_ms())
}

/// Pure core of [`relative_time`]: `<1m` → "now", `<1h` → `{n}m`, `<24h` →
/// `{n}h`, `<7d` → `{n}d`, otherwise the UTC date `YYYY-MM-DD` (plain
/// civil-from-days conversion, no external date crate). Future timestamps
/// (clock skew) read "now".
fn relative_time_from(updated_at_ms: i64, now_ms: i64) -> String {
    let elapsed_seconds = (now_ms - updated_at_ms).div_euclid(1000);
    if elapsed_seconds < 60 {
        return "now".to_string();
    }
    if elapsed_seconds < 3600 {
        return format!("{}m", elapsed_seconds / 60);
    }
    if elapsed_seconds < 86_400 {
        return format!("{}h", elapsed_seconds / 3600);
    }
    if elapsed_seconds < 7 * 86_400 {
        return format!("{}d", elapsed_seconds / 86_400);
    }
    let (year, month, day) = civil_from_days(updated_at_ms.div_euclid(86_400_000));
    format!("{year:04}-{month:02}-{day:02}")
}

/// Unix-milliseconds timestamp for the relative-time baseline.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
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
    use super::{
        RenameResolution, archive_section_visible, civil_from_days, relative_time_from,
        resolve_rename, row_shows_actions, thread_title,
    };
    use vega_conversation::types::Thread;

    fn thread_with_title(title: &str) -> Thread {
        Thread {
            id: "t1".to_string(),
            project_id: "p1".to_string(),
            title: title.to_string(),
            mode: vega_conversation::types::ThreadMode::Execute,
            permission_mode: vega_conversation::types::PermissionMode::Confirm,
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
    fn rename_submission_blank_is_cancel() {
        // 空标题提交视为取消（T13 卡面要求；Enter/Esc 键盘路径本身人工验收）。
        assert!(matches!(resolve_rename(""), RenameResolution::Cancel));
        assert!(matches!(resolve_rename("   "), RenameResolution::Cancel));
        assert!(matches!(resolve_rename("\t\n"), RenameResolution::Cancel));
    }

    #[test]
    fn rename_submission_trims_and_commits_non_blank() {
        match resolve_rename("新标题") {
            RenameResolution::Commit(title) => assert_eq!(title, "新标题"),
            RenameResolution::Cancel => panic!("expected commit"),
        }
        match resolve_rename("  两端空白  ") {
            RenameResolution::Commit(title) => assert_eq!(title, "两端空白"),
            RenameResolution::Cancel => panic!("expected commit"),
        }
    }

    #[test]
    fn hover_action_group_visibility() {
        // hover 才显示；行内编辑中的行永远不显示（裁决①）。
        assert!(row_shows_actions(true, false));
        assert!(!row_shows_actions(false, false));
        assert!(!row_shows_actions(true, true));
        assert!(!row_shows_actions(false, true));
    }

    #[test]
    fn archive_section_requires_archived_threads() {
        assert!(!archive_section_visible(0));
        assert!(archive_section_visible(1));
        assert!(archive_section_visible(3));
    }

    #[test]
    fn relative_time_uses_compact_units() {
        let now = 1_700_000_000_000;
        assert_eq!(relative_time_from(now, now), "now");
        // 60s 以内都算 "now"。
        assert_eq!(relative_time_from(now - 59_999, now), "now");
        assert_eq!(relative_time_from(now - 60_000, now), "1m");
        assert_eq!(relative_time_from(now - 3_600_000, now), "1h");
        assert_eq!(relative_time_from(now - 86_400_000, now), "1d");
        assert_eq!(relative_time_from(now - 6 * 86_400_000, now), "6d");
        // 恰好 7 天进入绝对日期分支（UTC）。
        assert_eq!(relative_time_from(now - 7 * 86_400_000, now), "2023-11-07");
        assert_eq!(relative_time_from(now - 30 * 86_400_000, now), "2023-10-15");
        // 未来时间戳（时钟偏差）不 panic，读作 "now"。
        assert_eq!(relative_time_from(now + 60_000, now), "now");
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2023-11-14 22:13:20 UTC 的日期部分。
        assert_eq!(civil_from_days(19_675), (2023, 11, 14));
        // 闰年边界：2024-02-29。
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }
}
