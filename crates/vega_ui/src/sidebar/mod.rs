use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};

use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonCustomVariant, ButtonVariants},
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

use crate::interaction::InteractionExt;
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

pub struct SidebarCollapsed(pub bool);

impl Global for SidebarCollapsed {}

/// Persisted user-selected Sidebar width. The value is normalized by
/// `vega_store::config` before it reaches the layout.
pub struct SidebarWidth(pub f32);

impl Global for SidebarWidth {}

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

pub struct SelectedProject(pub Option<String>);

impl Global for SelectedProject {}

/// Thin GPUI bridge for the conversation layer's worker-lifetime registry.
/// It belongs to the app, not a particular window or sidebar instance.
pub struct ProjectWorkerActivity(pub vega_conversation::types::ProjectWorkerActivity);

impl Global for ProjectWorkerActivity {}

/// Acquire ownership before spawning a project Agent worker. The returned
/// token must live inside the worker closure until that worker truly exits.
pub fn register_project_worker(project_id: &str, cx: &mut App) -> Arc<()> {
    if cx.try_global::<ProjectWorkerActivity>().is_none() {
        cx.set_global(ProjectWorkerActivity(Default::default()));
    }
    cx.update_global::<ProjectWorkerActivity, _>(|activity, _| activity.0.register(project_id))
}

/// Fail closed on project unregistration while any window's worker is alive.
pub fn project_worker_is_active(project_id: &str, cx: &App) -> bool {
    cx.try_global::<ProjectWorkerActivity>()
        .is_some_and(|activity| activity.0.is_active(project_id))
}

/// Thin GPUI bridge for the conversation layer's thread-scoped liveness
/// projection. App-owned (not per window/sidebar) so every sidebar reads the
/// same truth and a run in a background thread still lights its own row.
pub struct RunningThreadsGlobal(pub vega_conversation::types::RunningThreads);

impl Global for RunningThreadsGlobal {}

pub fn set_thread_running(thread_id: &str, running: bool, cx: &mut App) {
    let mut next = cx
        .try_global::<RunningThreadsGlobal>()
        .map(|global| global.0.clone())
        .unwrap_or_default();
    next.set(thread_id, running);
    if cx
        .try_global::<RunningThreadsGlobal>()
        .is_some_and(|global| global.0 == next)
    {
        return;
    }
    cx.set_global(RunningThreadsGlobal(next));
    cx.refresh_windows();
}

/// Whether `thread_id` currently owns a live Agent worker. A missing global
/// (isolated embedder or an un-seeded test) degrades to `false`, matching
/// [`project_worker_is_active`]; the UI never invents running state.
pub fn thread_is_running(thread_id: &str, cx: &App) -> bool {
    cx.try_global::<RunningThreadsGlobal>()
        .is_some_and(|running| running.0.is_running(thread_id))
}

pub struct OpenedThread(pub Option<Thread>);

impl Global for OpenedThread {}

pub struct PendingDeleteConfirm(pub Option<Thread>);

impl Global for PendingDeleteConfirm {}

pub enum SidebarEvent {
    ThreadDeleted(String),
}

impl EventEmitter<SidebarEvent> for Sidebar {}

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

struct StoreInitializationError(Option<crate::localized_copy::LocalizedCopy>);

impl Global for StoreInitializationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTaskDefaults {
    /// `defaults.model` (empty until a provider is configured).
    pub model: String,
    /// `defaults.permission_mode` (`readonly` | `confirm` | `auto` | `full_access`).
    pub permission_mode: String,
}

impl Default for NewTaskDefaults {
    fn default() -> Self {
        let defaults = config::AppConfig::default().defaults;
        Self {
            model: defaults.model,
            permission_mode: defaults.permission_mode,
        }
    }
}

impl NewTaskDefaults {
    /// Reads `config.defaults` from the resolved config root. A missing or
    /// malformed file falls back to the same `AppConfig::default()` values the
    /// generated template carries, so the draft's defaults match what
    /// `create_task` would have used on the missing-config path.
    pub fn load() -> Self {
        match config::load() {
            Ok(config) => Self {
                model: config.defaults.model,
                permission_mode: config.defaults.permission_mode,
            },
            Err(error) => {
                tracing::error!(%error, "failed to read defaults from config.toml");
                Self::default()
            }
        }
    }
}

impl Global for NewTaskDefaults {}

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
    let initialization_error = store.as_ref().err().cloned();
    cx.set_global(StoreInitializationError(initialization_error));
    cx.set_global(SelectedProject(selected));
    cx.set_global(VegaStore(store.map_err(|error| error.render())));
    let (projects_collapsed, sessions_collapsed) = load_block_state();
    cx.set_global(ProjectsCollapsed(projects_collapsed));
    cx.set_global(SessionsCollapsed(sessions_collapsed));
    cx.set_global(OpenedThread(None));
    cx.set_global(PendingDeleteConfirm(None));
    cx.set_global(SidebarWidth(load_width()));
    cx.set_global(NewTaskDefaults::load());
}

/// Opens and migrates `vega.db` under the platform data root (tech-spec §6).
///
/// Failures come back as ready-to-render messages.
fn open_default_store() -> Result<Store, crate::localized_copy::LocalizedCopy> {
    let dir = vega_store::paths::data_dir().ok_or_else(|| {
        crate::localized_copy::LocalizedCopy::new(
            "ui.cannot_determine_the_home_directory_home_is_not",
        )
    })?;
    std::fs::create_dir_all(&dir).map_err(|error| {
        crate::localized_copy::LocalizedCopy::with_args(
            "ui.cannot_create_path_error",
            vec![
                ("path", dir.display().to_string().into()),
                ("error", error.to_string().into()),
            ],
        )
    })?;
    let path = dir.join("vega.db");
    let store = Store::open(&path).map_err(|error| {
        crate::localized_copy::LocalizedCopy::with_args(
            "ui.cannot_open_path_error",
            vec![
                ("path", path.display().to_string().into()),
                ("error", error.to_string().into()),
            ],
        )
    })?;
    store.migrate().map_err(|error| {
        crate::localized_copy::LocalizedCopy::with_args(
            "ui.database_migration_failed_error",
            vec![("error", error.to_string().into())],
        )
    })?;
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

fn persist_ui(mutate: impl FnOnce(&mut config::AppConfig), what: &'static str, cx: &mut App) {
    if let Err(error) = config::update(mutate) {
        tracing::error!(%error, "failed to persist {what} to config.toml");
    }
    cx.refresh_windows();
}

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
    operation: impl FnOnce(&Store) -> Result<R, crate::localized_copy::LocalizedCopy>,
) -> Result<R, crate::localized_copy::LocalizedCopy> {
    match cx.try_global::<VegaStore>() {
        Some(VegaStore(Ok(store))) => operation(store),
        Some(VegaStore(Err(error))) => Err(crate::localized_copy::LocalizedCopy::with_args(
            "ui.project_storage_unavailable_error",
            vec![(
                "error",
                cx.try_global::<StoreInitializationError>()
                    .and_then(|failure| failure.0.clone())
                    .unwrap_or_else(|| error.clone().into()),
            )],
        )),
        None => Err(crate::localized_copy::LocalizedCopy::new(
            "ui.project_storage_unavailable_initialization_did_not_complete_at",
        )),
    }
}

pub struct Sidebar {
    projects_block: Entity<ProjectsBlock>,
    sessions_block: Entity<ThreadsBlock>,
    sidebar_scroll: gpui_kit::ScrollHandle,
    /// Inline error from thread creation (ui-spec §4.6: no modals).
    new_task_error: Option<crate::localized_copy::LocalizedCopy>,
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
        cx.observe(&projects_block, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&sessions_block, Self::on_sessions_event)
            .detach();
        Self {
            projects_block,
            sessions_block,
            sidebar_scroll: gpui_kit::ScrollHandle::new(),
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

    pub fn confirm_pending_delete(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = cx.global::<PendingDeleteConfirm>().0.clone() else {
            return;
        };
        if task_mutation_busy(cx) {
            self.sessions_block.update(cx, |block, cx| {
                block.error = Some(crate::localized_copy::LocalizedCopy::new(
                    "ui.a_task_is_being_saved_try_deleting_it",
                ));
                cx.notify();
            });
            return;
        }
        cx.set_global(PendingDeleteConfirm(None));
        crate::navigation::begin_task_mutation(cx);
        let result = with_store(cx, |store| {
            conversation::delete_thread(store, &thread.id)
                .map_err(crate::localized_copy::LocalizedCopy::from)
        });
        crate::navigation::finish_task_mutation(cx);
        match result {
            Ok(()) => {
                cx.emit(SidebarEvent::ThreadDeleted(thread.id.clone()));
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
                block.error = Some(crate::localized_copy::LocalizedCopy::with_args(
                    "ui.could_not_delete_the_conversation_message",
                    vec![("message", message)],
                ));
            }),
        }
        cx.refresh_windows();
    }

    /// [新建任务] / Cmd+N shared entry point. An active project owns the new
    /// task; with no active project the task is standalone and appears only in
    /// SESSIONS.
    pub fn create_thread(&mut self, cx: &mut Context<Self>) {
        if task_mutation_busy(cx) {
            self.new_task_error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.a_task_is_being_saved_try_again_shortly",
            ));
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

    pub fn reload_sessions(&mut self, cx: &mut Context<Self>) {
        self.sessions_block.update(cx, ThreadsBlock::reload);
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
            .h(px(Layout::MAIN_HEADER_HEIGHT))
            .flex_shrink_0()
            // The parent already contributes the standard section gap.
            .mb(px(
                Layout::SIDEBAR_TOOLBAR_CONTENT_GAP - Layout::SIDEBAR_SECTION_GAP
            ))
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
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .rounded_lg()
                    .text_color(colors.text_primary)
                    .text_size(px(Typography::SIDEBAR))
                    .cursor_pointer()
                    .interactive(*colors, true, false)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| this.create_thread(cx)),
                    )
                    .child(crate::icons::icon(
                        crate::icons::Icon::Plus,
                        colors.text_secondary,
                    ))
                    .child(div().child(vega_i18n::t("ui.new_task")))
                    .child(div().flex_1())
                    .child(
                        div()
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
    fn render_settings_entry(colors: &ThemeColors, cx: &App) -> AnyElement {
        div()
            .w_full()
            .flex_shrink_0()
            .px(px(Layout::SIDEBAR_PADDING))
            .pt(px(Layout::SIDEBAR_FOOTER_TOP))
            .pb(px(Layout::SIDEBAR_PADDING))
            .child(
                div()
                    .id("sidebar-settings-surface")
                    .w_full()
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .rounded_lg()
                    .overflow_hidden()
                    .interactive(*colors, true, false)
                    .capture_native_button_press()
                    .child(
                        Button::new("sidebar-settings")
                            .custom(
                                ButtonCustomVariant::new(cx).foreground(colors.text_primary.into()),
                            )
                            .small()
                            .w_full()
                            .h_full()
                            .rounded_lg()
                            .px_2()
                            .text_color(colors.text_primary)
                            .accessibility_label(vega_i18n::t("ui.settings"))
                            .tooltip(vega_i18n::t("ui.settings"))
                            .on_click(|_, _, cx| {
                                cx.set_global(SettingsOpen(true));
                                cx.refresh_windows();
                            })
                            .child(crate::icons::icon(
                                crate::icons::Icon::Settings,
                                colors.text_secondary,
                            ))
                            .child(div().child(vega_i18n::t("ui.settings_284")))
                            .child(div().flex_1())
                            .child(
                                div()
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
        let projects_error = self.projects_block.read(cx).error.clone();
        div()
            .id("sidebar")
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
                    .gap_3()
                    .child(self.render_brand(&colors, cx))
                    .child(self.render_new_task(cx, &colors))
                    .children(
                        projects_error.map(|message| {
                            div().flex_shrink_0().child(error_bar(message, &colors))
                        }),
                    )
                    .child(
                        div()
                            .on_children_prepainted({
                                let scroll = self.sidebar_scroll.clone();
                                let sessions = self.sessions_block.clone();
                                move |_, _, cx| {
                                    // `offset.y ∈ [-max_offset.y, 0]`; the sum is 0
                                    // exactly at the bottom (and for content that
                                    // does not overflow at all), so `> 0` means
                                    // there is still content below the viewport.
                                    if scroll.offset().y + scroll.max_offset().y > px(0.0) {
                                        return;
                                    }
                                    // Nothing left to append: skip the defer so an
                                    // already-fully-loaded list stops scheduling work.
                                    if !sessions
                                        .read_with(cx, |block, _| block.has_lazy_list_growth())
                                    {
                                        return;
                                    }
                                    let sessions = sessions.clone();
                                    // Prepaint runs with `draw_phase != None`, where
                                    // `notify` is dropped; deferring to the end of the
                                    // effect cycle both grows the window and produces
                                    // the follow-up draw in the same flush.
                                    cx.defer(move |cx| {
                                        sessions.update(cx, |block, cx| {
                                            block.grow_lazy_lists(cx);
                                        });
                                    });
                                }
                            })
                            .id("sidebar-scroll")
                            // Organization content keeps an 8px application-edge inset;
                            // Pinned row surfaces extend 8px left of the title column.
                            .ml(px(-4.0))
                            .pl_2()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .overflow_y_scroll()
                            .track_scroll(&self.sidebar_scroll)
                            .child(self.sessions_block.clone())
                            .tracked_scroll_edge_fade(
                                self.sidebar_scroll.clone(),
                                colors.bg_sidebar,
                                ScrollFade::compact(),
                            ),
                    ),
            )
            .child(Self::render_settings_entry(&colors, cx))
            .into_any_element()
    }
}

mod projects_block;
mod row_helpers;
mod threads_block;

pub use projects_block::{ProjectsBlock, ProjectsBlockEvent};
pub use row_helpers::*;
pub use threads_block::{ThreadsBlock, ThreadsBlockEvent};
