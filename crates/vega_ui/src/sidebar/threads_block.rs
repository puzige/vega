use super::*;
use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};
use gpui_kit::MouseMoveEvent;
use gpui_kit::component::{IconName, spinner::Spinner};
use vega_conversation::types::ThreadUpdate;
mod organization;
use organization::{Organization, OrganizationSection};

/// Events emitted by the session block.
pub enum ThreadsBlockEvent {
    /// A thread row was opened; the content column switched to it.
    Opened {
        thread_id: String,
        started: std::time::Instant,
    },
    OpeningChanged,
    DraftOpened,
}

impl EventEmitter<ThreadsBlockEvent> for ThreadsBlock {}

pub struct ThreadsBlock {
    /// Cached active rows (all projects when organization is enabled).
    pub(crate) threads: Vec<Thread>,
    /// Cached archived rows (shown in the 「已归档」 collapsed section).
    pub(crate) archived: Vec<Thread>,
    /// Project id the cache was loaded for (`None` → guidance copy).
    pub(crate) loaded_project: Option<String>,
    /// Thread id currently under the mouse; drives the row hover background.
    pub(crate) hovered: Option<String>,
    hover_suppressed_until_move: bool,
    /// Project id currently under the mouse; reveals fixed project-row actions.
    pub(crate) hovered_project: Option<String>,
    /// Section header currently under the pointer; its fixed action rail is
    /// revealed without adding or removing controls.
    pub(crate) hovered_section: Option<OrganizationSection>,
    /// Focus scopes keep quiet controls visible for keyboard users.
    pub(crate) projects_header_focus: FocusHandle,
    pub(crate) recents_header_focus: FocusHandle,
    pub(crate) project_focuses: HashMap<String, FocusHandle>,
    pub(crate) thread_action_focuses: HashMap<String, FocusHandle>,
    pub(crate) focused_project: Option<String>,
    pub(crate) focused_thread_action: Option<String>,
    pub(crate) focused_section: Option<OrganizationSection>,
    pub(crate) projects_expanded: bool,
    pub(crate) projects_progressive_focus: FocusHandle,
    pub(crate) recents_visible: usize,
    pub(crate) recents_total: usize,
    /// Thread id whose compact low-frequency action menu is open.
    pub(crate) actions_open: Option<String>,
    actions_row: Option<String>,
    /// Highlighted action inside the open menu (arrow keys move it).
    pub(crate) actions_highlight: usize,
    /// Scope handle for the open action menu; leaving the row closes it.
    pub(crate) actions_scope_focus: FocusHandle,
    /// One focus-out subscription for the action-menu scope.
    pub(crate) actions_focus_subscription: Option<Subscription>,
    pub(crate) archive_expanded: bool,
    /// Active inline rename session (`None` = not renaming).
    pub(crate) editing: Option<RenameSession>,
    /// Inline error message (ui-spec §4.6).
    pub(crate) error: Option<crate::localized_copy::LocalizedCopy>,
    action_pending: bool,
    visit_pending: bool,
    visit_loading: Option<(String, std::time::Instant)>,
    deferred_visit: Option<(String, u64)>,
    visit_navigation_generation: u64,
    visit_route: (Option<String>, Option<String>, bool),
    project_generation: u64,
    actions_scroll: gpui_kit::ScrollHandle,
    menu_height: gpui_kit::Pixels,
    organization: Option<Organization>,
}

/// An inline rename in progress: the thread being renamed plus the shared
/// [`TextInput`] entity pre-filled with its current title.
pub(crate) struct RenameSession {
    pub(crate) thread_id: String,
    row_key: String,
    pub(crate) input: Entity<TextInput>,
}

impl ThreadsBlock {
    /// Creates the block and loads the selected project's thread list.
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<OpenedThread>(|this, cx| this.observe_visit_route(cx))
            .detach();
        cx.observe_global::<SelectedProject>(|this, cx| this.observe_visit_route(cx))
            .detach();
        cx.observe_global::<SettingsOpen>(|this, cx| this.observe_visit_route(cx))
            .detach();
        let mut view = Self {
            threads: Vec::new(),
            archived: Vec::new(),
            loaded_project: None,
            hovered: None,
            hover_suppressed_until_move: false,
            hovered_project: None,
            hovered_section: None,
            projects_header_focus: cx.focus_handle(),
            recents_header_focus: cx.focus_handle(),
            project_focuses: HashMap::new(),
            thread_action_focuses: HashMap::new(),
            focused_project: None,
            focused_thread_action: None,
            focused_section: None,
            projects_expanded: false,
            projects_progressive_focus: cx.focus_handle(),
            recents_visible: organization::RECENTS_PAGE,
            recents_total: 0,
            actions_open: None,
            actions_row: None,
            actions_highlight: 0,
            actions_scope_focus: cx.focus_handle(),
            actions_focus_subscription: None,
            archive_expanded: false,
            editing: None,
            error: None,
            action_pending: false,
            visit_pending: false,
            visit_loading: None,
            deferred_visit: None,
            visit_navigation_generation: 0,
            visit_route: Self::current_visit_route(cx),
            project_generation: 0,
            actions_scroll: gpui_kit::ScrollHandle::new(),
            menu_height: px(360.),
            organization: None,
        };
        view.reload(cx);
        view
    }

    /// Re-reads the selected project's thread lists: active rows for the
    /// main list, archived rows for the 「已归档」 section (both pinned
    /// first, then updated_at desc — store ordering).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.organization.is_some() {
            self.refresh_organization(cx);
            return;
        }
        let selected = cx.global::<SelectedProject>().0.clone();
        if self.loaded_project != selected {
            // Project changes invalidate the anchored row position and the
            // action target together; never leave the old menu mounted for a
            // new project's rows.
            self.close_actions();
            self.project_generation = self.project_generation.wrapping_add(1);
        }
        self.loaded_project = selected.clone();
        let result = match selected {
            None => Ok((Vec::new(), Vec::new())),
            Some(project_id) => with_store(cx, |store| {
                let active =
                    conversation::list_threads(store, &project_id, Some(ThreadStatus::Active))
                        .map_err(crate::localized_copy::LocalizedCopy::from)?;
                let archived =
                    conversation::list_threads(store, &project_id, Some(ThreadStatus::Archived))
                        .map_err(crate::localized_copy::LocalizedCopy::from)?;
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
        let existing_ids: Vec<String> = self
            .threads
            .iter()
            .chain(self.archived.iter())
            .map(|thread| thread.id.clone())
            .collect();
        if self
            .actions_open
            .as_ref()
            .is_some_and(|thread_id| !existing_ids.iter().any(|id| id == thread_id))
        {
            self.close_actions();
        }
        cx.notify();
    }

    pub(crate) fn create_task(&mut self, project_id: Option<String>, cx: &mut Context<Self>) {
        if task_mutation_busy(cx) {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.a_task_is_being_saved_try_again_shortly",
            ));
            cx.notify();
            return;
        }
        if !crate::navigation::allow_task_navigation(None, cx) {
            return;
        }
        // The project must exist before it becomes the draft's binding; this is
        // the same guard `conversation::create_thread` applied, without the
        // INSERT. An unregistered id is refused instead of silently becoming a
        // standalone draft.
        if let Some(project_id) = project_id.as_deref() {
            let exists = with_store(cx, |store| {
                vega_store::projects::find(store.conn(), project_id)
                    .map_err(crate::localized_copy::LocalizedCopy::storage)
                    .map(|row| row.is_some())
            });
            match exists {
                Ok(true) => {}
                Ok(false) => {
                    self.error = Some(crate::localized_copy::LocalizedCopy::new(
                        "ui.the_project_does_not_exist_cannot_create_a",
                    ));
                    cx.notify();
                    return;
                }
                Err(message) => {
                    self.error = Some(message);
                    cx.notify();
                    return;
                }
            }
        }
        self.error = None;
        cx.set_global(SelectedProject(project_id));
        cx.set_global(OpenedThread(None));
        self.reload(cx);
        cx.emit(ThreadsBlockEvent::DraftOpened);
        cx.refresh_windows();
    }

    /// Click = open: bumps `threads.updated_at` + the owning project's
    /// `last_opened_at` (single transaction) and switches the content column
    /// via the [`OpenedThread`] global.
    fn current_visit_route(cx: &App) -> (Option<String>, Option<String>, bool) {
        (
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.clone()),
            cx.global::<SelectedProject>().0.clone(),
            cx.global::<SettingsOpen>().0,
        )
    }

    fn observe_visit_route(&mut self, cx: &mut Context<Self>) {
        let route = Self::current_visit_route(cx);
        if self.visit_route != route {
            self.visit_route = route;
            self.visit_navigation_generation = self.visit_navigation_generation.wrapping_add(1);
            self.clear_visit_loading(cx);
        }
    }

    pub(super) fn opening_thread(&self, cx: &App) -> Option<(String, std::time::Instant)> {
        (self.visit_route == Self::current_visit_route(cx))
            .then(|| self.visit_loading.clone())
            .flatten()
    }

    fn clear_visit_loading(&mut self, cx: &mut Context<Self>) {
        if self.visit_loading.take().is_some() {
            cx.emit(ThreadsBlockEvent::OpeningChanged);
        }
    }

    fn start_visit_loading(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let started = std::time::Instant::now();
        self.visit_loading = Some((thread_id.to_owned(), started));
        cx.emit(ThreadsBlockEvent::OpeningChanged);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(
                    vega_theme::Motion::CONVERSATION_LOADING_DELAY_MS,
                ))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .visit_loading
                    .as_ref()
                    .is_some_and(|(_, epoch)| *epoch == started)
                {
                    cx.emit(ThreadsBlockEvent::OpeningChanged);
                }
            });
        })
        .detach();
    }

    fn open_thread(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        if cx.has_active_drag() {
            return;
        }
        self.observe_visit_route(cx);
        if self.visit_pending {
            if crate::navigation::allow_task_navigation(Some(thread_id), cx) {
                self.deferred_visit =
                    Some((thread_id.to_owned(), self.visit_navigation_generation));
                self.start_visit_loading(thread_id, cx);
            }
            return;
        }
        if task_mutation_busy(cx) {
            self.clear_visit_loading(cx);
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.a_task_is_being_saved_try_again_shortly",
            ));
            cx.notify();
            return;
        }
        if !crate::navigation::allow_task_navigation(Some(thread_id), cx) {
            self.clear_visit_loading(cx);
            return;
        }
        self.close_actions();
        if self
            .visit_loading
            .as_ref()
            .is_none_or(|(target, _)| target != thread_id)
        {
            self.start_visit_loading(thread_id, cx);
        }
        let path = cx
            .try_global::<VegaStore>()
            .and_then(|store| store.0.as_ref().ok())
            .and_then(|store| store.database_path())
            .map(std::path::Path::to_path_buf);
        let thread_id = thread_id.to_owned();
        let origin = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone());
        let project = cx.global::<SelectedProject>().0.clone();
        let settings = cx.global::<SettingsOpen>().0;
        self.visit_pending = true;
        let navigation_generation = self.visit_navigation_generation;
        crate::navigation::begin_task_mutation(cx);
        let worker = cx.background_executor().spawn(async move {
            let path = path.ok_or_else(|| {
                crate::localized_copy::LocalizedCopy::new("app.conversation.load_failed")
            })?;
            let store = Store::open_existing(path)
                .map_err(crate::localized_copy::LocalizedCopy::storage)?;
            conversation::visit_thread(&store, &thread_id)
                .map_err(crate::localized_copy::LocalizedCopy::from)
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            cx.update(crate::navigation::finish_task_mutation);
            let _ = this.update(cx, |this, cx| {
                this.observe_visit_route(cx);
                this.visit_pending = false;
                if let Some((target, generation)) = this.deferred_visit.take() {
                    if generation == this.visit_navigation_generation {
                        this.open_thread(&target, cx);
                    } else {
                        this.clear_visit_loading(cx);
                    }
                    return;
                }
                let started = this.visit_loading.as_ref().map(|(_, started)| *started);
                this.clear_visit_loading(cx);
                if navigation_generation != this.visit_navigation_generation {
                    return;
                }
                if cx
                    .global::<OpenedThread>()
                    .0
                    .as_ref()
                    .map(|thread| thread.id.clone())
                    != origin
                    || cx.global::<SelectedProject>().0 != project
                    || cx.global::<SettingsOpen>().0 != settings
                {
                    return;
                }
                match result {
                    Ok(opened) => {
                        if !crate::navigation::allow_task_navigation(Some(&opened.id), cx) {
                            return;
                        }
                        this.error = None;
                        let thread_id = opened.id.clone();
                        cx.set_global(SelectedProject(opened.project_binding().map(str::to_owned)));
                        cx.set_global(OpenedThread(Some(opened)));
                        this.reload(cx);
                        cx.emit(ThreadsBlockEvent::Opened {
                            thread_id,
                            started: started.unwrap_or_else(std::time::Instant::now),
                        });
                    }
                    Err(message) => {
                        this.error = Some(message);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Runs one thread mutation, reloads the cached rows, and surfaces a
    /// failure as the block's inline error bar (ui-spec §4.6).
    fn mutate_thread(
        &mut self,
        operation: impl FnOnce(&Store) -> Result<(), crate::localized_copy::LocalizedCopy>,
        cx: &mut Context<Self>,
    ) {
        if task_mutation_busy(cx) {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.a_task_is_being_saved_try_again_shortly",
            ));
            cx.notify();
            return;
        }
        crate::navigation::begin_task_mutation(cx);
        let result = with_store(cx, operation);
        crate::navigation::finish_task_mutation(cx);
        if result.is_ok() {
            self.hovered = None;
            self.hover_suppressed_until_move = true;
        }
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
                    .map_err(crate::localized_copy::LocalizedCopy::from)
            },
            cx,
        );
    }

    /// 归档 (active → archived) or 恢复 (archived → active).
    fn set_thread_status(&mut self, thread_id: &str, status: ThreadStatus, cx: &mut Context<Self>) {
        self.mutate_thread(
            |store| {
                conversation::set_thread_status(store, thread_id, status)
                    .map_err(crate::localized_copy::LocalizedCopy::from)
            },
            cx,
        );
    }

    /// 「删除」 hover entry: parks the thread in [`PendingDeleteConfirm`];
    /// the window root renders the confirmation overlay (ui-spec §4.6: no
    /// system modal). Any inline rename is folded first so the overlay's Esc
    /// semantics stay unambiguous.
    fn request_delete(&mut self, thread: &Thread, cx: &mut Context<Self>) {
        self.close_actions();
        self.editing = None;
        cx.set_global(PendingDeleteConfirm(Some(thread.clone())));
        cx.refresh_windows();
    }

    /// Double-click = rename: builds the inline editor pre-filled with the
    /// current title and moves focus into it. (合成键盘事件送不进 GPUI：键入
    /// 与 Enter/Esc 提交路径为人工验收；提交/取消的纯逻辑见 [`resolve_rename`]。)
    fn start_rename(
        &mut self,
        thread: &Thread,
        row_key: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_actions();
        if let Some(session) = self
            .editing
            .as_mut()
            .filter(|session| session.thread_id == thread.id)
        {
            session.row_key = row_key;
            window.focus(&session.input.read(cx).focus_handle(cx), cx);
            cx.notify();
            return;
        }
        let input = cx
            .new(|cx| TextInput::new(cx, "", false).with_placeholder_key("ui.conversation_title"));
        input.update(cx, |input, cx| input.set_text(&thread.title, cx));
        self.editing = Some(RenameSession {
            thread_id: thread.id.clone(),
            row_key,
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
        let Some(session) = self.editing.as_ref() else {
            return;
        };
        let id = session.thread_id.clone();
        let raw = session.input.read(cx).text().to_string();
        match resolve_rename(&raw) {
            RenameResolution::Cancel => {
                self.editing = None;
                cx.notify();
            }
            RenameResolution::Commit(title) => {
                if let Some(thread) = self
                    .threads
                    .iter()
                    .chain(self.archived.iter())
                    .find(|t| t.id == id)
                    .cloned()
                {
                    self.apply_update(
                        &thread,
                        ThreadUpdate {
                            title: Some(title),
                            ..Default::default()
                        },
                        cx,
                    );
                }
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
            if self.hover_suppressed_until_move {
                return;
            }
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

    fn on_thread_mouse_move(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        self.hover_suppressed_until_move = false;
        self.set_hovered(thread_id, true, cx);
    }

    /// Hover bookkeeping for project rows; action hitboxes stay mounted while
    /// their vector controls appear only for the active row.
    pub(super) fn set_hovered_project(
        &mut self,
        project_id: &str,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let changed = if hovered {
            if self.hovered_project.as_deref() != Some(project_id) {
                self.hovered_project = Some(project_id.to_string());
                true
            } else {
                false
            }
        } else if self.hovered_project.as_deref() == Some(project_id) {
            self.hovered_project = None;
            true
        } else {
            false
        };
        if changed {
            cx.notify();
        }
    }

    pub(super) fn set_hovered_section(
        &mut self,
        section: OrganizationSection,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let changed = if hovered {
            self.hovered_section.replace(section) != Some(section)
        } else if self.hovered_section == Some(section) {
            self.hovered_section = None;
            true
        } else {
            false
        };
        if changed {
            cx.notify();
        }
    }

    fn sync_contextual_focus(&mut self, window: &Window, cx: &mut Context<Self>) {
        let row_keys: HashSet<_> = self
            .threads
            .iter()
            .chain(self.archived.iter())
            .flat_map(|thread| {
                let prefixes: &[&str] = if self.organization.is_none() {
                    &["thread-row-"]
                } else if thread.pinned {
                    &["pinned-thread-row-"]
                } else if thread.is_standalone() {
                    &["standalone-thread-row-"]
                } else {
                    &["project-thread-row-", "standalone-thread-row-"]
                };
                prefixes
                    .iter()
                    .map(move |prefix| format!("{prefix}{}", thread.id))
            })
            .collect();
        self.thread_action_focuses
            .retain(|key, _| row_keys.contains(key));
        for key in row_keys {
            self.thread_action_focuses
                .entry(key)
                .or_insert_with(|| cx.focus_handle());
        }
        if let Some(snapshot) = self
            .organization
            .as_ref()
            .and_then(|organization| organization.snapshot.as_ref())
        {
            let project_ids: HashSet<_> = snapshot
                .projects
                .iter()
                .map(|project| project.id.clone())
                .collect();
            self.project_focuses
                .retain(|id, _| project_ids.contains(id));
            for project in &snapshot.projects {
                self.project_focuses
                    .entry(project.id.clone())
                    .or_insert_with(|| cx.focus_handle());
            }
        }
        self.focused_project = self
            .project_focuses
            .iter()
            .find_map(|(id, focus)| focus.contains_focused(window, cx).then(|| id.clone()));
        self.focused_thread_action = self
            .thread_action_focuses
            .iter()
            .find_map(|(id, focus)| focus.contains_focused(window, cx).then(|| id.clone()));
        self.focused_section = if self.projects_header_focus.contains_focused(window, cx) {
            Some(OrganizationSection::Projects)
        } else if self.recents_header_focus.contains_focused(window, cx) {
            Some(OrganizationSection::Recents)
        } else {
            None
        };
    }

    /// Opens or closes the compact action menu for one thread. The trigger
    /// and menu each have a scoped key context, so focusing the trigger is
    /// enough to operate the full menu with Enter/Space, arrows, and Esc.
    fn toggle_actions(
        &mut self,
        thread_id: &str,
        row_key: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if cx.has_active_drag() {
            return;
        }
        if self.actions_row.as_deref() == Some(row_key) {
            self.close_actions();
        } else {
            self.actions_open = Some(thread_id.to_string());
            self.actions_row = Some(row_key.to_string());
            self.actions_highlight = 0;
        }
        cx.notify();
    }

    fn close_actions(&mut self) {
        self.actions_row = None;
        if self.actions_open.take().is_some() {
            self.actions_highlight = 0;
        }
    }

    fn move_action_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(thread_id) = self.actions_open.as_ref() else {
            return;
        };
        let Some(thread) = self
            .threads
            .iter()
            .chain(self.archived.iter())
            .find(|thread| thread.id == *thread_id)
        else {
            return;
        };
        let base_count = if thread.is_standalone() { 7 } else { 9 };
        let count = (base_count + self.organization_actions(thread_id).len()) as isize;
        self.actions_highlight =
            (self.actions_highlight as isize + delta).rem_euclid(count) as usize;
        self.actions_scroll.scroll_to_item(self.actions_highlight);
        cx.notify();
    }

    fn previous_action(
        &mut self,
        _: &PreviousThreadAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_action_highlight(-1, cx);
    }

    fn next_action(&mut self, _: &NextThreadAction, _: &mut Window, cx: &mut Context<Self>) {
        self.move_action_highlight(1, cx);
    }

    fn activate_action(
        &mut self,
        _: &ActivateThreadAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread_id) = self.actions_open.clone() else {
            return;
        };
        let archived = self.is_archived(&thread_id);
        self.activate_action_index(&thread_id, archived, self.actions_highlight, window, cx);
    }

    fn close_actions_action(
        &mut self,
        _: &CloseThreadActions,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.actions_open.is_some() {
            self.close_actions();
            cx.notify();
        }
    }

    fn is_archived(&self, thread_id: &str) -> bool {
        self.archived.iter().any(|thread| thread.id == thread_id)
    }

    /// Applies only committed fields, never a stale task snapshot or a route change.
    fn apply_update(&mut self, thread: &Thread, update: ThreadUpdate, cx: &mut Context<Self>) {
        if self.action_pending || task_mutation_busy(cx) {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.task_actions_are_being_saved_try_again_shortly",
            ));
            cx.notify();
            return;
        }
        if let Some(unread) = update.unread
            && update.title.is_none()
            && update.status.is_none()
            && update.pinned.is_none()
        {
            crate::navigation::begin_task_mutation(cx);
            let result = with_store(cx, |store| {
                conversation::update_thread(store, &thread.id, &update)
                    .map_err(crate::localized_copy::LocalizedCopy::from)
            });
            crate::navigation::finish_task_mutation(cx);
            match result {
                Ok(()) => {
                    for row in self
                        .threads
                        .iter_mut()
                        .chain(self.archived.iter_mut())
                        .filter(|row| row.id == thread.id)
                    {
                        row.unread = unread;
                    }
                    if let Some(mut opened) = cx.global::<OpenedThread>().0.clone()
                        && opened.id == thread.id
                        && opened.project_id == thread.project_id
                    {
                        opened.unread = unread;
                        cx.set_global(OpenedThread(Some(opened)));
                    }
                    self.error = None;
                    if self.organization.is_some() {
                        self.refresh_organization(cx);
                    }
                }
                Err(error) => self.error = Some(error),
            }
            cx.refresh_windows();
            return;
        }
        let generation = self.project_generation;
        let id = thread.id.clone();
        let project = thread.project_id.clone();
        let database = with_store(cx, |store| {
            store.database_path().map(Path::to_path_buf).ok_or_else(|| {
                crate::localized_copy::LocalizedCopy::new(
                    "ui.task_storage_requires_a_file_backed_database",
                )
            })
        });
        let Ok(database) = database else {
            self.error = database.err();
            cx.notify();
            return;
        };
        self.action_pending = true;
        crate::navigation::begin_task_mutation(cx);
        let committed = update.clone();
        let worker = cx.background_executor().spawn(async move {
            let store =
                Store::open(database).map_err(crate::localized_copy::LocalizedCopy::storage)?;
            let renamed_at = if let Some(title) = &update.title {
                Some(
                    conversation::rename_thread(&store, &id, title)
                        .map_err(crate::localized_copy::LocalizedCopy::from)?
                        .updated_at,
                )
            } else {
                conversation::update_thread(&store, &id, &update)
                    .map_err(crate::localized_copy::LocalizedCopy::from)?;
                None
            };
            Ok::<_, crate::localized_copy::LocalizedCopy>((id, project, renamed_at))
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            // Release the application fence even when the originating block is gone.
            cx.update(crate::navigation::finish_task_mutation);
            this.update(cx, |this, cx| {
                this.action_pending = false;
                if this.project_generation != generation {
                    return;
                }
                match result {
                    Ok((id, project, renamed_at)) => {
                        if this.organization.is_none()
                            && this.loaded_project.as_deref() != Some(&project)
                        {
                            return;
                        }
                        for row in this
                            .threads
                            .iter_mut()
                            .chain(this.archived.iter_mut())
                            .filter(|row| row.id == id)
                        {
                            merge_update(row, &committed);
                            if let Some(at) = renamed_at {
                                row.updated_at = row.updated_at.max(at);
                            }
                        }
                        if let Some(mut opened) = cx
                            .global::<OpenedThread>()
                            .0
                            .clone()
                            .filter(|row| row.id == id)
                        {
                            merge_update(&mut opened, &committed);
                            if let Some(at) = renamed_at {
                                opened.updated_at = opened.updated_at.max(at);
                            }
                            cx.set_global(OpenedThread(Some(opened)));
                        }
                        if committed.title.is_some()
                            && this.editing.as_ref().is_some_and(|s| {
                                s.thread_id == id
                                    && committed.title.as_deref()
                                        == Some(s.input.read(cx).text().trim())
                            })
                        {
                            this.editing = None;
                        }
                        if renamed_at.is_some() {
                            this.threads.sort_by_key(|row| {
                                (
                                    std::cmp::Reverse(row.pinned),
                                    std::cmp::Reverse(row.updated_at),
                                )
                            });
                            this.archived.sort_by_key(|row| {
                                (
                                    std::cmp::Reverse(row.pinned),
                                    std::cmp::Reverse(row.updated_at),
                                )
                            });
                        }
                        this.error = None;
                        if this.organization.is_some() {
                            this.refresh_organization(cx);
                        }
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.refresh_windows();
            })
            .ok();
        })
        .detach();
    }

    fn project_action(&mut self, thread: &Thread, finder: bool, cx: &mut Context<Self>) {
        if self.action_pending {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.task_actions_are_being_saved_try_again_shortly",
            ));
            cx.notify();
            return;
        }
        let generation = self.project_generation;
        let id = thread.id.clone();
        let project = thread.project_id.clone();
        let target_id = id.clone();
        let route = cx.global::<OpenedThread>().0.as_ref().map(|t| t.id.clone());
        let database = with_store(cx, |store| {
            store.database_path().map(Path::to_path_buf).ok_or_else(|| {
                crate::localized_copy::LocalizedCopy::new(
                    "ui.task_storage_requires_a_file_backed_database",
                )
            })
        });
        let Ok(database) = database else {
            self.error = database.err();
            cx.notify();
            return;
        };
        self.action_pending = true;
        let worker = cx.background_executor().spawn(async move {
            let store = Store::open_read_only(database)
                .map_err(crate::localized_copy::LocalizedCopy::storage)?;
            conversation::task_project_path(&store, &id)
                .map_err(crate::localized_copy::LocalizedCopy::from)
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            this.update(cx, |this, cx| {
                this.action_pending = false;
                if this.project_generation != generation {
                    return;
                }
                if this.organization.is_none() && this.loaded_project.as_deref() != Some(&project) {
                    return;
                }
                if cx.global::<OpenedThread>().0.as_ref().map(|t| t.id.clone()) != route
                    || !this
                        .threads
                        .iter()
                        .chain(this.archived.iter())
                        .any(|t| t.id == target_id)
                {
                    return;
                }
                match result {
                    Ok(path) => {
                        if finder {
                            cx.reveal_path(&path);
                        } else {
                            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                path.to_string_lossy().into_owned(),
                            ));
                        }
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn activate_action_index(
        &mut self,
        thread_id: &str,
        _archived: bool,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self
            .threads
            .iter()
            .chain(self.archived.iter())
            .find(|t| t.id == thread_id)
            .cloned()
        else {
            return;
        };
        let row_key = self
            .actions_row
            .clone()
            .unwrap_or_else(|| format!("thread-row-{thread_id}"));
        self.close_actions();
        let project_bound = !thread.is_standalone();
        match (project_bound, index) {
            (_, 0) => self.toggle_pin(thread_id, thread.pinned, cx),
            (_, 1) => self.start_rename(&thread, row_key, window, cx),
            (_, 2) => self.set_thread_status(
                thread_id,
                if thread.status == ThreadStatus::Archived {
                    ThreadStatus::Active
                } else {
                    ThreadStatus::Archived
                },
                cx,
            ),
            (_, 3) => self.apply_update(
                &thread,
                ThreadUpdate {
                    unread: Some(!thread.unread),
                    ..Default::default()
                },
                cx,
            ),
            (true, 4) => self.project_action(&thread, true, cx),
            (true, 5) => self.project_action(&thread, false, cx),
            (true, 6) | (false, 4) => {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(thread.id))
            }
            (true, 7) | (false, 5) => {
                cx.set_global(SettingsOpen(true));
                cx.refresh_windows();
            }
            (true, 8) | (false, 6) => self.request_delete(&thread, cx),
            (_, index) => {
                let organization_offset = if project_bound { 9 } else { 7 };
                if let Some((_, action)) = self
                    .organization_actions(thread_id)
                    .get(index.saturating_sub(organization_offset))
                    .cloned()
                {
                    self.submit_organization(action, cx);
                }
            }
        }
        cx.notify();
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
                            .text_size(px(Typography::METADATA))
                            .font_weight(Typography::HEADING_CARD_WEIGHT)
                            .text_color(colors.text_secondary)
                            .child(vega_i18n::t("ui.sessions")),
                    )
                    .child(crate::icons::icon(
                        if collapsed {
                            crate::icons::Icon::ChevronRight
                        } else {
                            crate::icons::Icon::ChevronDown
                        },
                        colors.text_tertiary,
                    )),
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
            return guidance(vega_i18n::t("ui.no_project_choose_one_in_projects_first"))
                .into_any_element();
        }
        if self.threads.is_empty() && self.archived.is_empty() {
            return guidance(vega_i18n::t(
                "ui.no_conversations_click_new_task_above_to_start",
            ))
            .into_any_element();
        }
        let mut body = div().flex().flex_col();
        if self.threads.is_empty() {
            // 仅剩归档线程：主列表给一行引导，归档折叠区照常可见可展开。
            body = body.child(guidance(vega_i18n::t("ui.no_active_conversations")));
        } else {
            body =
                body.children(self.threads.iter().map(|thread| {
                    self.render_row(thread, &opened_id, false, "thread-row-", true, cx)
                }));
        }
        body.children(
            archive_section_visible(self.archived.len())
                .then(|| self.render_archive_header(cx, colors)),
        )
        .when(
            self.archive_expanded && archive_section_visible(self.archived.len()),
            |column| {
                column.children(self.archived.iter().map(|thread| {
                    self.render_row(thread, &opened_id, true, "thread-row-", true, cx)
                }))
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
            .rounded_lg()
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
                    .child(vega_i18n::t_args(
                        "ui.archived_count",
                        &[("count", &(self.archived.len()).to_string())],
                    )),
            )
            .child(crate::icons::icon(
                if self.archive_expanded {
                    crate::icons::Icon::ChevronDown
                } else {
                    crate::icons::Icon::ChevronRight
                },
                colors.text_tertiary,
            ))
            .into_any_element()
    }

    fn render_row(
        &self,
        thread: &Thread,
        opened_id: &Option<String>,
        archived: bool,
        selector_prefix: &str,
        actions_enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_row_at_height(
            thread,
            opened_id,
            archived,
            selector_prefix,
            actions_enabled,
            true,
            true,
            0.0,
            Layout::SIDEBAR_NAV_CONTENT_INSET,
            0.0,
            Typography::SIDEBAR_LINE_HEIGHT,
            cx,
        )
    }

    pub(super) fn render_pi_row(
        &self,
        thread: &Thread,
        opened_id: &Option<String>,
        archived: bool,
        selector_prefix: &str,
        actions_enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_row_at_height(
            thread,
            opened_id,
            archived,
            selector_prefix,
            actions_enabled,
            true,
            false,
            0.0,
            Layout::SIDEBAR_ROW_INSET,
            Layout::SIDEBAR_ROW_INSET - Layout::SIDEBAR_RENAME_CHROME_INSET,
            Typography::SIDEBAR_LINE_HEIGHT,
            cx,
        )
    }

    pub(super) fn render_recent_row(
        &self,
        thread: &Thread,
        opened_id: &Option<String>,
        archived: bool,
        selector_prefix: &str,
        actions_enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_row_at_height(
            thread,
            opened_id,
            archived,
            selector_prefix,
            actions_enabled,
            true,
            false,
            0.0,
            Layout::SIDEBAR_LABEL_INSET,
            Layout::SIDEBAR_LABEL_INSET - Layout::SIDEBAR_RENAME_CHROME_INSET,
            Typography::SIDEBAR_LINE_HEIGHT,
            cx,
        )
    }

    pub(super) fn render_pinned_row(
        &self,
        thread: &Thread,
        opened_id: &Option<String>,
        archived: bool,
        selector_prefix: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_row_at_height(
            thread,
            opened_id,
            archived,
            selector_prefix,
            true,
            false,
            false,
            0.0,
            Layout::SIDEBAR_LABEL_INSET,
            Layout::SIDEBAR_LABEL_INSET - Layout::SIDEBAR_RENAME_CHROME_INSET,
            Typography::SIDEBAR_LINE_HEIGHT,
            cx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row_at_height(
        &self,
        thread: &Thread,
        opened_id: &Option<String>,
        archived: bool,
        selector_prefix: &str,
        actions_enabled: bool,
        show_pin_indicator: bool,
        show_timestamp_at_rest: bool,
        surface_leading_outset: f32,
        content_inset: f32,
        rename_inset: f32,
        row_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let selected = opened_id.as_deref() == Some(thread.id.as_str());
        let row_key = format!("{selector_prefix}{}", thread.id);
        let hovered = self.hovered.as_deref() == Some(row_key.as_str());
        let editing_session = self
            .editing
            .as_ref()
            .filter(|session| session.row_key == row_key);
        let editing_this_row = editing_session.is_some();
        let actions_visible = row_shows_actions(hovered, editing_this_row)
            || self.actions_row.as_deref() == Some(row_key.as_str())
            || self.focused_thread_action.as_deref() == Some(row_key.as_str());
        let thread_id = thread.id.clone();
        let hover_thread_id = row_key.clone();
        let mouse_move_thread_id = row_key.clone();
        let rename_row_key = row_key.clone();
        let row_thread = thread.clone();
        let accessibility_label = if thread_is_running(&thread.id, cx) {
            vega_i18n::t_args("ui.title_running", &[("title", &thread.title)])
        } else {
            thread.title.clone()
        };
        let mut row = div()
            .id(ElementId::Name(
                format!("{selector_prefix}{thread_id}").into(),
            ))
            .aria_label(accessibility_label)
            .h(px(row_height))
            .flex_shrink_0()
            .ml(px(-surface_leading_outset))
            .flex()
            .items_center()
            .rounded_lg()
            .overflow_hidden()
            .text_size(px(Typography::SIDEBAR))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.set_hovered(&hover_thread_id, *hovered, cx);
            }))
            .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                this.on_thread_mouse_move(&mouse_move_thread_id, cx);
            }))
            .on_mouse_up(
                MouseButton::Right,
                cx.listener({
                    let thread_id = thread.id.clone();
                    let row_key = row_key.clone();
                    move |this, _, window, cx| {
                        cx.stop_propagation();
                        if this
                            .editing
                            .as_ref()
                            .is_some_and(|session| session.thread_id == thread_id)
                            || cx.has_active_drag()
                        {
                            return;
                        }
                        this.toggle_actions(&thread_id, &row_key, window, cx);
                        if let Some(focus) = this.thread_action_focuses.get(&row_key) {
                            focus.focus(window, cx);
                        }
                    }
                }),
            )
            .interaction_surface(
                if selected {
                    colors.bg_active
                } else if !editing_this_row
                    && (self.actions_row.as_deref() == Some(row_key.as_str())
                        || self.focused_thread_action.as_deref() == Some(row_key.as_str()))
                {
                    colors.bg_hover
                } else {
                    gpui_kit::Rgba {
                        a: 0.,
                        ..colors.bg_hover
                    }
                },
                if selected {
                    colors.bg_active
                } else {
                    colors.bg_hover
                },
                colors.bg_active,
                !editing_this_row && !self.hover_suppressed_until_move,
            );
        if let Some(session) = editing_session {
            row = row.child(self.render_rename_editor(session, rename_inset, cx));
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
                        .pl(px(content_inset))
                        .pr_2()
                        .cursor_pointer()
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                                if cx.has_active_drag() {
                                    return;
                                }
                                if event.click_count >= 2 {
                                    this.start_rename(
                                        &row_thread,
                                        rename_row_key.clone(),
                                        window,
                                        cx,
                                    );
                                } else {
                                    this.open_thread(&row_thread.id, cx);
                                }
                            }),
                        )
                        .children((show_pin_indicator && thread.pinned).then(|| {
                            // 置顶小标记：token 色着色（裁决③）。
                            div()
                                .flex_shrink_0()
                                .text_color(colors.accent)
                                .child(crate::icons::icon(crate::icons::Icon::Pin, colors.accent))
                        }))
                        .child(
                            div()
                                .id(ElementId::Name(
                                    format!("{selector_prefix}title-{}", thread.id).into(),
                                ))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(colors.text_primary)
                                .when(thread.unread, |title| {
                                    title.font_weight(Typography::HEADING_CARD_WEIGHT)
                                })
                                .child(thread_title(thread)),
                        ),
                );
            row = if actions_enabled {
                row.child(self.render_row_actions(
                    thread,
                    archived,
                    actions_visible,
                    show_timestamp_at_rest,
                    selector_prefix,
                    cx,
                ))
            } else {
                row.child(
                    div()
                        .w(px(
                            Layout::SIDEBAR_ACTIONS_WIDTH + Layout::SIDEBAR_ACTIONS_INSET
                        ))
                        .flex_shrink_0(),
                )
            };
        }
        if self.organization.is_some() && !editing_this_row {
            self.render_reorder_row(
                if selector_prefix == "standalone-thread-row-" {
                    organization::ReorderItem::RecentThread(thread.id.clone())
                } else {
                    organization::ReorderItem::Thread(thread.id.clone())
                },
                thread_title(thread),
                row,
                cx,
            )
        } else {
            row.into_any_element()
        }
    }

    fn render_row_actions(
        &self,
        thread: &Thread,
        archived: bool,
        actions_visible: bool,
        show_timestamp_at_rest: bool,
        selector_prefix: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let thread_id = thread.id.clone();
        let row_key = format!("{selector_prefix}{thread_id}");
        let action_prefix = selector_prefix
            .strip_suffix("thread-row-")
            .unwrap_or(selector_prefix);

        let running = thread_is_running(&thread.id, cx);
        let menu_open = self.actions_row.as_deref() == Some(row_key.as_str());
        let keyboard_focused = self.focused_thread_action.as_deref() == Some(row_key.as_str());
        let quick_actions_visible =
            self.hovered.as_deref() == Some(row_key.as_str()) && !menu_open && !keyboard_focused;
        let mut trigger = div()
            .id(ElementId::Name(
                format!("{action_prefix}thread-actions-{thread_id}").into(),
            ))
            .relative()
            .size(px(Layout::SIDEBAR_ACTION_SIZE))
            .rounded_md()
            .text_size(px(Typography::HEADING_BLOCK))
            .text_color(colors.text_secondary)
            .aria_label(vega_i18n::t("ui.conversation_actions"))
            .focusable()
            .tab_stop(true)
            .when_some(
                self.organization
                    .as_ref()
                    .and_then(|_| self.thread_action_focuses.get(&row_key)),
                |trigger, focus| trigger.track_focus(focus),
            )
            .focus_visible(|style| {
                style
                    .opacity(1.)
                    .bg(colors.bg_hover)
                    .text_color(colors.text_primary)
            })
            .cursor_pointer()
            .hover(move |style| style.bg(colors.bg_hover));

        if menu_open {
            trigger = trigger
                .key_context("ThreadActionsMenu")
                .on_action(cx.listener(Self::previous_action))
                .on_action(cx.listener(Self::next_action))
                .on_action(cx.listener(Self::activate_action))
                .on_action(cx.listener(Self::close_actions_action));
        } else {
            trigger = trigger
                .key_context("ThreadActionTrigger")
                .on_action(cx.listener({
                    let thread_id = thread.id.clone();
                    let row_key = row_key.clone();
                    move |this, _: &OpenThreadActions, window, cx| {
                        this.toggle_actions(&thread_id, &row_key, window, cx);
                    }
                }));
        }

        let menu = menu_open.then(|| {
            // Pin an absolute zero-origin layer to the trigger, then give
            // Anchored an explicit relative containing block. This keeps the
            // trigger button's block position out of the popup's static origin.
            div().absolute().top_0().left_0().w_full().h_full().child(
                div().relative().size_full().child(
                    anchored()
                        .anchor(Anchor::TopRight)
                        .position_mode(AnchoredPositionMode::Local)
                        // The popup is anchored to the 28px trigger's bottom-right.
                        // Anchored prepaint stays in the local containing block;
                        // only the menu surface is deferred so it escapes the
                        // row/sidebar content mask with the computed offset.
                        .position(point(px(28.), px(28.)))
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            deferred(self.render_action_menu(thread, archived, action_prefix, cx))
                                .with_priority(2),
                        ),
                ),
            )
        });

        // Keep the click hitbox separate from the popup. The trigger owns the
        // focus stop and keyboard scope, while the sibling deferred menu can
        // receive its own mouse actions without the trigger's capture handler
        // swallowing them.
        let trigger_button = div()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .capture_any_mouse_up(cx.listener({
                let row_key = row_key.clone();
                move |this, event: &MouseUpEvent, _, cx| {
                    if event.button == MouseButton::Left
                        && this.actions_row.as_deref() == Some(row_key.as_str())
                    {
                        this.close_actions();
                        cx.notify();
                        cx.stop_propagation();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener({
                    let thread_id = thread.id.clone();
                    let row_key = row_key.clone();
                    move |this, _: &MouseUpEvent, window, cx| {
                        this.toggle_actions(&thread_id, &row_key, window, cx);
                    }
                }),
            )
            .child(crate::icons::icon(
                crate::icons::Icon::More,
                colors.text_secondary,
            ));
        trigger = trigger.child(trigger_button);
        if let Some(menu) = menu {
            trigger = trigger.child(menu);
        }
        if !menu_open && !keyboard_focused {
            trigger = trigger.opacity(0.);
        }
        if quick_actions_visible {
            trigger = trigger.size(px(0.));
        }
        trigger = trigger.absolute().right_0().top_0();

        let mut group = div()
            .relative()
            .w(px(Layout::SIDEBAR_ACTIONS_WIDTH))
            .mr(px(Layout::SIDEBAR_ACTIONS_INSET))
            .h(px(Layout::SIDEBAR_ACTION_SIZE))
            .flex()
            .items_center()
            .justify_end()
            .flex_shrink_0();
        if menu_open {
            group = group.track_focus(&self.actions_scope_focus);
        }
        let trailing = session_row_trailing(
            actions_visible,
            running,
            thread.unread,
            show_timestamp_at_rest,
        );
        if trailing == SessionRowTrailing::Timestamp {
            group = group.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_right()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(relative_time(thread.updated_at)),
            );
        }
        let running_selector_id = thread_id.clone();
        let running_selector_prefix = action_prefix.to_owned();
        let running_indicator = || {
            div()
                .id(gpui_kit::SharedString::from(format!(
                    "{running_selector_prefix}thread-running-{running_selector_id}"
                )))
                .size(px(Layout::SIDEBAR_ACTION_SIZE))
                .aria_label(vega_i18n::t("ui.task_running"))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Spinner::new()
                        .ease(gpui_kit::linear)
                        .with_size(px(Layout::ICON_SIZE))
                        .icon(IconName::LoaderCircle)
                        .color(colors.text_secondary.into()),
                )
        };
        if trailing == SessionRowTrailing::Running {
            group = group.child(running_indicator());
        }
        if trailing == SessionRowTrailing::Unread {
            group = group.child(
                div()
                    .size(px(Layout::SIDEBAR_ACTION_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(Layout::MESSAGE_ANCHOR_TRACK_WIDTH * 3.))
                            .rounded_full()
                            .bg(colors.accent),
                    ),
            );
        }
        group = group.child(trigger);
        if quick_actions_visible {
            let pinned = thread.pinned;
            let pin_label = if pinned {
                vega_i18n::t("ui.unpin")
            } else {
                vega_i18n::t("ui.pin")
            };
            let pin_thread_id = thread.id.clone();
            let pin_button = div()
                .id(ElementId::Name(
                    format!("{action_prefix}thread-pin-{thread_id}").into(),
                ))
                .aria_label(pin_label)
                .tooltip(move |_, cx| crate::icons::tooltip(pin_label, cx))
                .size(px(24.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .hover(move |style| style.bg(colors.bg_hover))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle_pin(&pin_thread_id, pinned, cx);
                    }),
                )
                .child(crate::icons::icon(
                    crate::icons::Icon::Pin,
                    if pinned {
                        colors.accent
                    } else {
                        colors.text_secondary
                    },
                ));
            let archive_button = div()
                .id(ElementId::Name(
                    format!("{action_prefix}thread-archive-{thread_id}").into(),
                ))
                .aria_label(if archived {
                    vega_i18n::t("ui.restore_task")
                } else {
                    vega_i18n::t("ui.archive_task")
                })
                .tooltip(move |_, cx| {
                    crate::icons::tooltip(
                        if archived {
                            vega_i18n::t("ui.restore_task")
                        } else {
                            vega_i18n::t("ui.archive_task")
                        },
                        cx,
                    )
                })
                .size(px(24.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .hover(move |style| style.bg(colors.bg_hover))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.set_thread_status(
                            &thread_id,
                            if archived {
                                ThreadStatus::Active
                            } else {
                                ThreadStatus::Archived
                            },
                            cx,
                        );
                    }),
                )
                .child(
                    gpui_kit::component::Icon::new(if archived {
                        IconName::Undo2
                    } else {
                        IconName::Inbox
                    })
                    .with_size(px(16.))
                    .text_color(colors.text_secondary)
                    .flex_shrink_0(),
                );
            let quick_actions = div()
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .flex()
                .gap(px(Layout::SIDEBAR_ACTION_GAP))
                .items_center();
            group = group.child(quick_actions.child(pin_button).child(archive_button));
        }
        group.into_any_element()
    }

    /// Focusable popup menu for one row. Mouse activation calls the same
    /// mutation methods as the former inline action group; keyboard activation
    /// uses the highlighted index maintained by [`Self::actions_highlight`].
    fn render_action_menu(
        &self,
        thread: &Thread,
        archived: bool,
        action_prefix: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let mut actions = vec![
            (
                if thread.pinned {
                    vega_i18n::t("ui.unpin")
                } else {
                    vega_i18n::t("ui.pin")
                },
                false,
            ),
            (vega_i18n::t("ui.rename"), false),
            (
                if archived {
                    vega_i18n::t("ui.resume")
                } else {
                    vega_i18n::t("ui.archive")
                },
                false,
            ),
            (
                if thread.unread {
                    vega_i18n::t("ui.mark_as_read")
                } else {
                    vega_i18n::t("ui.mark_as_unread")
                },
                false,
            ),
        ];
        if !thread.is_standalone() {
            actions.extend([
                (vega_i18n::t("ui.open_project_in_finder"), false),
                (vega_i18n::t("ui.copy_project_path"), false),
            ]);
        }
        actions.extend([
            (vega_i18n::t("ui.copy_conversation_id"), false),
            (vega_i18n::t("ui.go_to_settings"), false),
            (vega_i18n::t("ui.delete"), true),
        ]);
        let mut actions: Vec<(String, bool)> = actions
            .into_iter()
            .map(|(label, danger)| (label.to_string(), danger))
            .collect();
        actions.extend(
            self.organization_actions(&thread.id)
                .into_iter()
                .map(|(label, _)| (label, false)),
        );
        let thread_id = thread.id.clone();
        div()
            .id(ElementId::Name(
                format!("{action_prefix}thread-actions-menu-{thread_id}").into(),
            ))
            .w(px(Layout::TASK_MENU_WIDTH))
            // The deferred popup is above sibling rows; its hitbox must stop them too.
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .max_h(self.menu_height)
            .track_scroll(&self.actions_scroll)
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .rounded(px(Layout::MENU_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .p_1()
            .shadow_sm()
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.actions_open.is_some() {
                        this.close_actions();
                        cx.notify();
                    }
                }),
            )
            .children(
                actions
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, danger))| {
                        let thread_id = thread.id.clone();
                        let selected = self.actions_highlight == index;
                        let mut item = div()
                            .id(ElementId::Name(
                                format!("{action_prefix}thread-action-{thread_id}-{index}").into(),
                            ))
                            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                            .flex_shrink_0()
                            .w_full()
                            .flex()
                            .items_center()
                            .px_2()
                            .rounded_md()
                            .text_size(px(Typography::SIDEBAR))
                            .text_color(if danger {
                                colors.danger
                            } else {
                                colors.text_primary
                            })
                            .cursor_pointer()
                            .hover(move |style| style.bg(colors.bg_hover))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    let clear_action_focus = index == 0
                                        && this.focused_thread_action.is_some()
                                        && this.focused_thread_action == this.actions_row;
                                    this.activate_action_index(
                                        &thread_id, archived, index, window, cx,
                                    );
                                    if clear_action_focus {
                                        window.blur(cx);
                                    }
                                }),
                            )
                            .child(label);
                        if selected {
                            item = item.bg(colors.bg_active);
                        }
                        item
                    }),
            )
            .tracked_scroll_edge_fade(
                self.actions_scroll.clone(),
                colors.bg_elevated,
                ScrollFade::compact(),
            )
            .into_any_element()
    }

    /// The inline rename editor mounted in place of the row body: the shared
    /// [`TextInput`] under a `ThreadRename` key context. Enter dispatches
    /// [`ConfirmRename`] (bound to this context in `vega_ui::init`); Esc is
    /// intercepted from the global [`CloseSettings`] action, which the
    /// editor consumes so settings never closes mid-rename.
    fn render_rename_editor(
        &self,
        session: &RenameSession,
        leading_inset: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex_1()
            .min_w_0()
            .pl(px(leading_inset))
            .pr_1()
            .key_context("ThreadRename")
            .track_focus(&session.input.read(cx).focus_handle(cx))
            .on_action(cx.listener(|this, _: &ConfirmRename, _, cx| this.commit_rename(cx)))
            .on_action(cx.listener(|this, _: &CloseSettings, _, cx| this.cancel_rename(cx)))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.cancel_rename(cx)))
            .child(session.input.clone())
            .into_any_element()
    }
}

impl Render for ThreadsBlock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_contextual_focus(window, cx);
        self.menu_height =
            (window.viewport_size().height - px(16.)).max(px(Typography::SIDEBAR_LINE_HEIGHT));
        if self.actions_focus_subscription.is_none() {
            self.actions_focus_subscription =
                Some(
                    cx.on_focus_out(&self.actions_scope_focus, window, |this, _, _, cx| {
                        if this.actions_open.is_some() {
                            this.close_actions();
                            cx.notify();
                        }
                    }),
                );
        }
        let colors = theme(cx).colors;
        if self.organization.is_some() {
            return self.render_organization(window, cx);
        }
        let collapsed = cx.global::<SessionsCollapsed>().0;
        div()
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "tab" && this.editing.is_none() {
                        this.close_actions();
                        if event.keystroke.modifiers.shift {
                            window.focus_prev(cx);
                        } else {
                            window.focus_next(cx);
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
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

fn merge_update(thread: &mut Thread, update: &ThreadUpdate) {
    if let Some(title) = &update.title {
        thread.title = title.clone();
    }
    if let Some(unread) = update.unread {
        thread.unread = unread;
    }
    if let Some(pinned) = update.pinned {
        thread.pinned = pinned;
    }
    if let Some(status) = update.status {
        thread.status = status;
    }
}
