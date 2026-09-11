//! Sidebar (T09 shell + T12 content + T13 session management): the fixed
//! Compact left column of the main window layout
//! ([vega-ui-spec.md §1](../../docs/vega-ui-spec.md)).
//!
//! Structure per the T12 architect ruling: a small Vega brand row, a light
//! [新建任务] entry, then two independent block components — [`ProjectsBlock`]
//! (project list: select / add / remove / sort toggle, branch suffix per row)
//! and [`ThreadsBlock`] (the selected project's sessions: pinned group first,
//! `updated_at` desc) — orchestrated by [`Sidebar`], which owns the
//! cross-block wiring. Settings remains a stable low-frequency entry at the
//! bottom of the rail.
//!
//! R13 projects the same task rows into nested projects, a local-calendar
//! timeline, or cross-project custom groups. Organization metadata uses one
//! background revision-checked lane; project registration/live branches and
//! the existing task action/navigation boundaries remain shared.
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

use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, AnchoredPositionMode, AnyElement, App, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, Global, MouseButton, MouseDownEvent, MouseUpEvent, PathPromptOptions,
    Subscription, Window, actions, anchored, deferred, div, point, px,
};
use vega_conversation::threads as conversation;
use vega_conversation::types::{Thread, ThreadStatus};
use vega_store::Store;
use vega_store::config;
use vega_store::git_detect;
use vega_store::projects::{self, Project, ProjectSort};
use vega_theme::{Layout, ThemeColors, Typography, theme};

use crate::settings::{CloseSettings, SettingsOpen};
use crate::text_input::TextInput;

actions!(
    vega_sidebar,
    [
        ToggleSidebar,
        NewThread,
        ConfirmRename,
        OpenThreadActions,
        NextThreadAction,
        PreviousThreadAction,
        ActivateThreadAction,
        CloseThreadActions
    ]
);

/// Sidebar width in logical pixels (ui-spec §1).
pub const SIDEBAR_WIDTH: f32 = Layout::SIDEBAR_WIDTH;

/// Viewport width below which the sidebar auto-collapses (ui-spec §1).
pub const AUTO_COLLAPSE_WIDTH: f32 = 960.0;

/// Content column max width in logical pixels (ui-spec §1).
pub const CONTENT_MAX_WIDTH: f32 = Layout::CONTENT_MAX_WIDTH;

/// Content column minimum horizontal padding in logical pixels (ui-spec §1).
pub const CONTENT_MIN_PADDING: f32 = Layout::CONTENT_PADDING;

/// Whether the user collapsed the sidebar with Cmd+B (T09, persisted as
/// `ui.sidebar_collapsed`). The effective sidebar visibility is
/// `!self.0 && viewport_width >= AUTO_COLLAPSE_WIDTH`.
pub struct SidebarCollapsed(pub bool);

impl Global for SidebarCollapsed {}

/// Persisted user-selected Sidebar width. The value is normalized by
/// `vega_store::config` before it reaches the layout.
pub struct SidebarWidth(pub f32);

impl Global for SidebarWidth {}

/// Returns the effective stored Sidebar width, falling back to the R21
/// default for isolated embedders that have not installed the global yet.
pub fn width(cx: &App) -> f32 {
    cx.try_global::<SidebarWidth>()
        .map_or(Layout::SIDEBAR_WIDTH, |width| {
            config::clamp_sidebar_width(width.0)
        })
}

/// Updates only the in-memory Sidebar width during a pointer drag.
pub fn set_width(width: f32, cx: &mut App) {
    cx.set_global(SidebarWidth(config::clamp_sidebar_width(width)));
    cx.refresh_windows();
}

/// Persists the accepted width when a pointer drag completes.
pub fn persist_width(cx: &mut App) {
    let width = width(cx);
    persist_ui(
        |config| config.ui.sidebar_width = width,
        "sidebar_width",
        cx,
    );
}

/// Explicit reveal overrides automatic narrow-window collapse for this app session.
#[derive(Default)]
pub struct SidebarExplicitlyShown(pub bool);
impl Global for SidebarExplicitlyShown {}

/// Show the sidebar using its existing persisted preference path.
pub fn show_persisted(cx: &mut App) {
    cx.set_global(SidebarExplicitlyShown(true));
    cx.set_global(SidebarCollapsed(false));
    cx.set_global(ProjectsCollapsed(false));
    cx.set_global(SessionsCollapsed(false));
    persist_ui(
        |config| {
            config.ui.sidebar_collapsed = false;
            config.ui.projects_collapsed = false;
            config.ui.sessions_collapsed = false;
        },
        "sidebar reveal",
        cx,
    );
}

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

/// Loads the persisted R21 Sidebar width, using the finite default on error.
pub fn load_width() -> f32 {
    match config::load() {
        Ok(config) => config::clamp_sidebar_width(config.ui.sidebar_width),
        Err(error) => {
            tracing::error!(%error, "failed to read sidebar_width from config.toml");
            Layout::SIDEBAR_WIDTH
        }
    }
}

/// Cmd+B handler: flips the preference, persists it to `config.toml`, and
/// refreshes windows. Persistence failures degrade to in-memory state
/// (ui-spec §4.6: no modals); the next successful toggle rewrites the file.
pub fn toggle_persisted(cx: &mut App) {
    let collapsed = !cx.global::<SidebarCollapsed>().0;
    cx.set_global(SidebarExplicitlyShown(!collapsed));
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
    cx.set_global(SidebarWidth(load_width()));
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
    if let Err(error) = config::update(mutate) {
        tracing::error!(%error, "failed to persist {what} to config.toml");
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

/// Mutations and accepted navigation visits share one short application fence.
fn task_mutation_busy(cx: &App) -> bool {
    cx.try_global::<crate::navigation::TaskMutationState>()
        .is_some_and(|state| state.pending > 0)
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

/// The sidebar orchestrator: brand/new-task chrome, the two block entities,
/// and a stable settings entry. Block components own their data + row
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
        sessions_block.update(cx, |block, cx| {
            block.enable_organization(projects_block.clone(), cx)
        });
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
            ProjectsBlockEvent::Selected(project_id)
            | ProjectsBlockEvent::Registered(project_id) => {
                clear_opened_thread_of_other_project(project_id, cx);
                self.sessions_block.update(cx, ThreadsBlock::reload);
            }
            ProjectsBlockEvent::Removed(project_id) => {
                // 移除的是当前项目：回退到无选中态（会话块显示引导文案）。
                if cx.global::<SelectedProject>().0.as_deref() == Some(project_id.as_str()) {
                    cx.set_global(SelectedProject(None));
                }
                clear_opened_thread_of_project(project_id, cx);
                self.sessions_block.update(cx, ThreadsBlock::reload);
            }
        }
        if let ProjectsBlockEvent::Registered(id) = event {
            self.sessions_block
                .update(cx, |block, cx| block.reveal_registered_project(id, cx));
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
        if task_mutation_busy(cx) {
            self.sessions_block.update(cx, |block, cx| {
                block.error = Some("任务正在保存，请稍后重试删除".into());
                cx.notify();
            });
            return;
        }
        cx.set_global(PendingDeleteConfirm(None));
        crate::navigation::begin_task_mutation(cx);
        let result = with_store(cx, |store| {
            conversation::delete_thread(store, &thread.id).map_err(|error| error.to_string())
        });
        crate::navigation::finish_task_mutation(cx);
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

    /// [新建任务] / Cmd+N shared entry point. An active project owns the new
    /// task; with no active project the task is standalone and appears only in
    /// SESSIONS.
    pub fn create_thread(&mut self, cx: &mut Context<Self>) {
        if task_mutation_busy(cx) {
            self.new_task_error = Some("任务正在保存，请稍后重试".into());
            cx.notify();
            return;
        }
        if !crate::navigation::allow_task_navigation(None, cx) {
            return;
        }
        self.new_task_error = None;
        let project_id = cx.global::<SelectedProject>().0.clone();
        self.sessions_block
            .update(cx, |block, cx| block.create_task(project_id, cx));
        cx.set_global(SettingsOpen(false));
    }

    /// Returns an already-loaded project label without store or filesystem IO.
    pub fn project_label(&self, project_id: &str, cx: &App) -> Option<String> {
        self.projects_block
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.name.clone())
    }

    /// Empty-state entry point for the existing project picker. The block
    /// keeps all folder registration and error handling in its original
    /// handler; this wrapper only exposes the same action outside the rail.
    pub fn open_project_picker(&mut self, cx: &mut Context<Self>) {
        self.projects_block.update(cx, ProjectsBlock::open_picker);
    }

    /// Compact functional sidebar toolbar, aligned with the native titlebar.
    fn render_brand(&self, colors: &ThemeColors, cx: &App) -> AnyElement {
        div()
            .h(px(40.))
            .flex()
            .items_center()
            .justify_end()
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .px_2()
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.text_secondary)
            .child(crate::navigation::controls(cx, true))
            .into_any_element()
    }

    /// The light [新建任务] entry. It creates a task in the selected project
    /// or a standalone task when no project is selected.
    fn render_new_task(&mut self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap_1()
            .child(
                div()
                    .id("sidebar-new-task")
                    .debug_selector(|| "sidebar-new-task".into())
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .rounded_lg()
                    .text_color(colors.text_primary)
                    .text_size(px(Typography::SIDEBAR))
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| this.create_thread(cx)),
                    )
                    .child(crate::icons::icon(
                        crate::icons::Icon::Plus,
                        colors.text_secondary,
                    ))
                    .child(
                        div()
                            .debug_selector(|| "sidebar-new-task-label".into())
                            .child("新建任务"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .debug_selector(|| "sidebar-new-task-shortcut".into())
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_tertiary)
                            .child("⌘N"),
                    ),
            )
            .children(
                self.new_task_error
                    .clone()
                    .map(|message| error_bar(message, colors)),
            )
            .into_any_element()
    }

    /// Stable low-frequency settings entry. It stays in the rail even when
    /// project/session blocks grow or collapse, so the route is discoverable.
    fn render_settings_entry(&self, colors: &ThemeColors) -> AnyElement {
        const HOVER_GROUP: &str = "sidebar-settings-control";

        div()
            .w_full()
            .flex_shrink_0()
            .px(px(Layout::SIDEBAR_PADDING))
            .pb(px(Layout::SIDEBAR_PADDING))
            .child(
                div()
                    .id("sidebar-settings-surface")
                    .debug_selector(|| "sidebar-settings-surface".into())
                    .group(HOVER_GROUP)
                    .w_full()
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .rounded_lg()
                    .overflow_hidden()
                    .group_hover(HOVER_GROUP, move |style| style.bg(colors.bg_hover))
                    .group_active(HOVER_GROUP, move |style| style.bg(colors.bg_active))
                    .child(
                        Button::new("sidebar-settings")
                            .debug_selector(|| "sidebar-settings".into())
                            .group(HOVER_GROUP)
                            .text()
                            .small()
                            .w_full()
                            .h_full()
                            .rounded_lg()
                            .px_2()
                            .text_color(colors.text_primary)
                            .accessibility_label("设置 (⌘,)")
                            .tooltip("设置 (⌘,)")
                            .on_click(|_, _, cx| {
                                cx.set_global(SettingsOpen(true));
                                cx.refresh_windows();
                            })
                            .child(crate::icons::icon(
                                crate::icons::Icon::Settings,
                                colors.text_secondary,
                            ))
                            .child(
                                div()
                                    .debug_selector(|| "sidebar-settings-label".into())
                                    .child("设置"),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .debug_selector(|| "sidebar-settings-shortcut".into())
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.text_tertiary)
                                    .child("⌘,"),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .id("sidebar")
            .debug_selector(|| "sidebar".into())
            .flex()
            .flex_col()
            .w(px(width(cx)))
            .h_full()
            .flex_shrink_0()
            .bg(colors.bg_sidebar)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .px(px(Layout::SIDEBAR_PADDING))
                    .pt(px(Layout::SIDEBAR_PADDING))
                    .pb(px(Layout::SIDEBAR_PADDING))
                    .gap_3()
                    .child(self.render_brand(&colors, cx))
                    .child(self.render_new_task(cx, &colors))
                    .child(
                        div()
                            .id("sidebar-search")
                            .debug_selector(|| "sidebar-search".into())
                            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                            .px_2()
                            .rounded_lg()
                            .flex()
                            .items_center()
                            .text_size(px(Typography::SIDEBAR))
                            .text_color(colors.text_secondary)
                            .cursor_pointer()
                            .child(
                                div()
                                    .debug_selector(|| "sidebar-search-label".into())
                                    .child("搜索"),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .debug_selector(|| "sidebar-search-shortcut".into())
                                    .text_size(px(Typography::METADATA))
                                    .child("⌘K"),
                            )
                            .on_mouse_up(MouseButton::Left, |_, window, cx| {
                                window.dispatch_action(
                                    Box::new(crate::command_palette::OpenPalette),
                                    cx,
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("sidebar-scroll")
                            .debug_selector(|| "sidebar-scroll".into())
                            // Pinned row surfaces extend left of the title column.
                            .ml(px(-8.0))
                            .pl_2()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .overflow_y_scroll()
                            .child(self.sessions_block.clone()),
                    ),
            )
            .child(self.render_settings_entry(&colors))
            .into_any_element()
    }
}

mod projects_block;
mod row_helpers;
mod threads_block;

pub use projects_block::{ProjectsBlock, ProjectsBlockEvent};
pub use row_helpers::*;
pub use threads_block::{ThreadsBlock, ThreadsBlockEvent};

#[cfg(test)]
mod tests {

    use super::{
        OpenedThread, RenameResolution, archive_section_visible, civil_from_days,
        clear_opened_thread_of_project, relative_time_from, resolve_rename, row_shows_actions,
        thread_title,
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

    #[gpui_kit::test]
    async fn removing_opened_project_clears_opened_thread(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            cx.set_global(OpenedThread(Some(thread_with_title("active"))));
            clear_opened_thread_of_project("p1", cx);
            assert!(cx.global::<OpenedThread>().0.is_none());
        });
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
