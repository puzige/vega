use super::nsh_leaf::chrome::ScrollFadeExt;
use super::*;
use gpui_kit::MouseMoveEvent;
use vega_conversation::types::ThreadUpdate;
mod organization;
mod pin;
mod reload;
mod rename;
#[cfg(feature = "nsh-reading-integration")]
mod rename_tests;
use organization::{Organization, OrganizationSection};
#[cfg(feature = "nsh-reading-integration")]
pub use rename_tests::mounted_rename_native_ack_ime_cancel_rebind_and_persistence;

/// Events emitted by the session block.
pub enum ThreadsBlockEvent {
    /// A thread row was opened; the content column switched to it.
    Opened {
        thread_id: String,
        started: std::time::Instant,
    },
    OpeningChanged,
    ForkCreated(String),
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
    selected_row: Option<(String, String)>,
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
    row_pending: Option<String>,
    visit_pending: bool,
    visit_loading: Option<(String, std::time::Instant)>,
    deferred_visit: Option<(String, u64, Option<String>)>,
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
    binding: rename::RenameBinding,
    _shortcut_subscription: Subscription,
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
            selected_row: None,
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
            row_pending: None,
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

    fn thread_row_is_selected(
        &self,
        thread: &Thread,
        opened_id: &Option<String>,
        row_key: &str,
    ) -> bool {
        if opened_id.as_deref() != Some(thread.id.as_str()) {
            return false;
        }
        if let Some((selected_thread, selected_row)) = &self.selected_row
            && selected_thread == &thread.id
        {
            return selected_row == row_key;
        }
        let prefix = match self.organization.as_ref() {
            None => "thread-row-",
            Some(_) if thread.pinned => "pinned-thread-row-",
            Some(organization)
                if !thread.is_standalone()
                    && organization.snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.preferences.hide_project_threads_from_recents
                    }) =>
            {
                "project-thread-row-"
            }
            Some(_) => "standalone-thread-row-",
        };
        row_key == format!("{prefix}{}", thread.id)
    }

    fn open_thread(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        self.open_thread_from_row(thread_id, None, cx);
    }

    fn open_thread_from_row(
        &mut self,
        thread_id: &str,
        row_key: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if cx.has_active_drag() {
            return;
        }
        self.observe_visit_route(cx);
        if self.visit_pending {
            if crate::navigation::allow_task_navigation(Some(thread_id), cx) {
                self.deferred_visit = Some((
                    thread_id.to_owned(),
                    self.visit_navigation_generation,
                    row_key,
                ));
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
                if let Some((target, generation, target_row)) = this.deferred_visit.take() {
                    if generation == this.visit_navigation_generation {
                        this.open_thread_from_row(&target, target_row, cx);
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
                        this.selected_row = row_key.map(|key| (thread_id.clone(), key));
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
    fn set_thread_status(&mut self, thread_id: &str, status: ThreadStatus, cx: &mut Context<Self>) {
        self.persist_row_update(
            thread_id,
            ThreadUpdate {
                status: Some(status),
                ..Default::default()
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

    fn start_rename(
        &mut self,
        thread: &Thread,
        row_key: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_actions();
        let input = if let Some(session) = self
            .editing
            .as_ref()
            .filter(|session| session.thread_id == thread.id)
        {
            session.input.clone()
        } else {
            let input = cx.new(|cx| {
                TextInput::new(cx, "", false).with_placeholder_key("ui.conversation_title")
            });
            input.update(cx, |input, cx| input.set_text(&thread.title, cx));
            input
        };
        self.bind_rename(thread.id.clone(), row_key, input, window, cx);
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
        if self.organization.as_ref().is_none_or(|org| org.focus_dirty) {
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
            if let Some(org) = self.organization.as_mut() {
                org.focus_dirty = false;
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
        let count = (base_count
            + usize::from(Self::supports_fork(thread))
            + self.organization_actions(thread_id).len()) as isize;
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.actions_open.is_some() {
            let focus = self
                .actions_row
                .as_ref()
                .and_then(|row| self.thread_action_focuses.get(row))
                .cloned();
            self.close_actions();
            if let Some(focus) = focus {
                window.focus(&focus, cx);
            }
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
            self.persist_row_update(
                &thread.id,
                ThreadUpdate {
                    unread: Some(unread),
                    ..Default::default()
                },
                cx,
            );
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
                        if let Some(org) = this.organization.as_mut() {
                            org.task_projection = None;
                            org.focus_dirty = true;
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

    fn supports_fork(thread: &Thread) -> bool {
        thread.backend == vega_conversation::types::TaskBackend::Native
            && !thread.is_standalone()
            && thread.parent_thread_id.is_none()
    }

    fn fork_unavailable(&self, thread: &Thread, cx: &App) -> Option<&'static str> {
        if self.action_pending || task_mutation_busy(cx) || thread_is_running(&thread.id, cx) {
            return Some("ui.fork_busy");
        }
        Self::fork_cutoff_unavailable(
            cx.try_global::<VegaStore>()
                .and_then(|store| store.0.as_ref().ok()),
            &thread.id,
        )
    }

    fn fork_cutoff_unavailable(store: Option<&Store>, thread_id: &str) -> Option<&'static str> {
        let Some(store) = store else {
            return Some("ui.fork_failed");
        };
        match vega_store::thread_fork::latest_fork_cutoff(store.conn(), thread_id) {
            Ok(Some(_)) => None,
            Ok(None) => Some("ui.fork_no_completed_reply"),
            Err(_) => Some("ui.fork_failed"),
        }
    }

    fn fork_latest(&mut self, thread: &Thread, cx: &mut Context<Self>) {
        if !Self::supports_fork(thread) {
            return;
        }
        if let Some(reason) = self.fork_unavailable(thread, cx) {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(reason));
            cx.notify();
            return;
        }
        let database = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .ok()
            .and_then(|store| store.database_path())
            .map(Path::to_path_buf);
        let Some(database) = database else {
            self.error = Some(crate::localized_copy::LocalizedCopy::new("ui.fork_failed"));
            cx.notify();
            return;
        };
        self.observe_visit_route(cx);
        let generation = self.visit_navigation_generation;
        let route = Self::current_visit_route(cx);
        let source = thread.id.clone();
        self.action_pending = true;
        self.error = None;
        self.start_visit_loading(&source, cx);
        crate::navigation::begin_task_mutation(cx);
        let worker = cx.background_executor().spawn(async move {
            let store = Store::open_existing(database).map_err(|_| ())?;
            let _admission =
                vega_store::thread_admission::try_acquire(store.database_identity(), &source)
                    .map_err(|_| ())?
                    .ok_or(())?;
            conversation::fork_latest_thread(&store, &source).map_err(|_| ())
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            cx.update(crate::navigation::finish_task_mutation);
            let _ = this.update(cx, |this, cx| {
                this.action_pending = false;
                this.clear_visit_loading(cx);
                this.observe_visit_route(cx);
                let current = generation == this.visit_navigation_generation
                    && route == Self::current_visit_route(cx);
                this.reload(cx);
                match result {
                    Ok(thread) if current => {
                        cx.emit(ThreadsBlockEvent::ForkCreated(thread.id.clone()));
                        this.open_thread(&thread.id, cx);
                    }
                    Ok(_) => {}
                    Err(()) => {
                        this.error =
                            Some(crate::localized_copy::LocalizedCopy::new("ui.fork_failed"));
                    }
                }
                cx.refresh_windows();
            });
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
                let base_count = if project_bound { 9 } else { 7 };
                let has_fork = Self::supports_fork(&thread);
                if has_fork && index == base_count {
                    self.fork_latest(&thread, cx);
                    return;
                }
                let organization_offset = base_count + usize::from(has_fork);
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
    fn render_header(&self, cx: &mut Context<Self>, _colors: &ThemeColors) -> AnyElement {
        let collapsed = cx.global::<SessionsCollapsed>().0;
        nsh_leaf::host(
            "sidebar-sessions-header",
            None,
            vec![nsh_leaf::Item::action(
                vega_i18n::t("ui.sessions"),
                Some(if collapsed {
                    crate::icons::Icon::ChevronRight
                } else {
                    crate::icons::Icon::ChevronDown
                }),
            )],
            false,
            Typography::SIDEBAR_LINE_HEIGHT,
            move |cx| cx.global::<SessionsCollapsed>().0 == collapsed,
            |_, _, _, cx| toggle_sessions_block(cx),
        )
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
            nsh_leaf::host(
                format!("sidebar-guidance-{message}"),
                Some(message.to_owned()),
                Vec::new(),
                false,
                Typography::SIDEBAR_LINE_HEIGHT,
                |_| true,
                |_, _, _, _| {},
            )
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
    fn render_archive_header(&self, cx: &mut Context<Self>, _colors: &ThemeColors) -> AnyElement {
        let expanded = self.archive_expanded;
        let count = self.archived.len();
        let weak = cx.weak_entity();
        let target = weak.clone();
        nsh_leaf::host(
            "sidebar-archive-header",
            None,
            vec![nsh_leaf::Item::action(
                vega_i18n::t_args("ui.archived_count", &[("count", &count.to_string())]),
                Some(if expanded {
                    crate::icons::Icon::ChevronDown
                } else {
                    crate::icons::Icon::ChevronRight
                }),
            )],
            false,
            Typography::SIDEBAR_LINE_HEIGHT,
            move |cx| {
                weak.upgrade().is_some_and(|entity| {
                    let this = entity.read(cx);
                    this.archive_expanded == expanded && this.archived.len() == count
                })
            },
            move |_, _, _, cx| {
                let _ = target.update(cx, |this, cx| {
                    this.archive_expanded = !expanded;
                    cx.notify();
                });
            },
        )
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
        let row_key = format!("{selector_prefix}{}", thread.id);
        let selected = self.thread_row_is_selected(thread, opened_id, &row_key);
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
        let accessibility_label = if thread_is_awaiting_answer(&thread.id, cx) {
            format!(
                "{}, {}",
                thread.title,
                vega_i18n::t("ui.waiting_for_your_answer")
            )
        } else if thread_is_running(&thread.id, cx) {
            vega_i18n::t_args("ui.title_running", &[("title", &thread.title)])
        } else {
            thread.title.clone()
        };
        let mut row = div()
            .id(ElementId::Name(
                format!("{selector_prefix}{thread_id}").into(),
            ))
            .aria_label(accessibility_label)
            .when(thread_is_awaiting_answer(&thread.id, cx), |row| {
                row.tooltip(|_, cx| {
                    crate::icons::tooltip(vega_i18n::t("ui.waiting_for_your_answer"), cx)
                })
            })
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
            );
        if let Some(session) = editing_session {
            row = row.child(rename::mount(session, rename_inset, cx));
        } else {
            let observed = thread.clone();
            let weak = cx.weak_entity();
            let target = weak.clone();
            let projection = self
                .organization
                .as_ref()
                .and_then(|org| org.snapshot.clone());
            let generation = self.project_generation;
            let mut item = nsh_leaf::Item::action(
                thread_title(thread),
                (show_pin_indicator && thread.pinned).then_some(crate::icons::Icon::Pin),
            );
            item.selected = selected;
            item.emphasis = thread.unread;
            item.double_action = Some(1);
            row = row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl(px(content_inset))
                    .child(nsh_leaf::host(
                        format!("{row_key}-title"),
                        None,
                        vec![item],
                        false,
                        row_height,
                        move |cx| {
                            weak.upgrade().is_some_and(|entity| {
                                let this = entity.read(cx);
                                this.project_generation == generation
                                    && this.editing.is_none()
                                    && this
                                        .threads
                                        .iter()
                                        .chain(this.archived.iter())
                                        .any(|thread| thread == &observed)
                                    && this
                                        .organization
                                        .as_ref()
                                        .and_then(|org| org.snapshot.clone())
                                        == projection
                            })
                        },
                        move |index, _, window, cx| {
                            let _ = target.update(cx, |this, cx| {
                                if index == 1 {
                                    this.start_rename(
                                        &row_thread,
                                        rename_row_key.clone(),
                                        window,
                                        cx,
                                    );
                                } else {
                                    this.open_thread_from_row(
                                        &row_thread.id,
                                        Some(rename_row_key.clone()),
                                        cx,
                                    );
                                }
                            });
                        },
                    )),
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
        let thread_id = thread.id.clone();
        let row_key = format!("{selector_prefix}{thread_id}");
        let action_prefix = selector_prefix
            .strip_suffix("thread-row-")
            .unwrap_or(selector_prefix);
        let menu_open = self.actions_row.as_deref() == Some(row_key.as_str());
        let pending = self.row_pending.is_some() || self.action_pending || task_mutation_busy(cx);
        let running = thread_is_running(&thread.id, cx);
        let waiting = thread_is_awaiting_answer(&thread.id, cx);
        let mut items = Vec::new();
        let quick = actions_visible
            && !menu_open
            && self.focused_thread_action.as_deref() != Some(row_key.as_str());
        if quick {
            for (label, icon) in [
                (
                    if thread.pinned {
                        vega_i18n::t("ui.unpin")
                    } else {
                        vega_i18n::t("ui.pin")
                    },
                    crate::icons::Icon::Pin,
                ),
                (
                    if archived {
                        vega_i18n::t("ui.restore_task")
                    } else {
                        vega_i18n::t("ui.archive_task")
                    },
                    crate::icons::Icon::Archive,
                ),
            ] {
                let mut item = nsh_leaf::Item::action(label, Some(icon));
                item.enabled = !pending;
                item.compact = true;
                items.push(item);
            }
        } else {
            let mut item = nsh_leaf::Item::action(
                vega_i18n::t("ui.conversation_actions"),
                Some(crate::icons::Icon::More),
            );
            item.enabled = !pending;
            item.compact = true;
            item.quiet = !menu_open;
            item.running =
                !waiting && (running || self.row_pending.as_deref() == Some(thread.id.as_str()));
            item.resting = Some(if waiting {
                "Ⅱ".to_owned()
            } else if thread.unread {
                "●".to_owned()
            } else if show_timestamp_at_rest {
                relative_time(thread.updated_at)
            } else {
                String::new()
            });
            items.push(item);
        }
        let pinned = thread.pinned;
        let observed = thread.clone();
        let projection = self
            .organization
            .as_ref()
            .and_then(|org| org.snapshot.clone());
        let weak = cx.weak_entity();
        let target = weak.clone();
        let observed_row = row_key.clone();
        let open = self.actions_row.clone();
        let content = nsh_leaf::focused_actions(
            format!("{row_key}-actions-nsh"),
            items,
            self.thread_action_focuses.get(&row_key).cloned(),
            Layout::SIDEBAR_ACTION_SIZE,
            move |cx| {
                weak.upgrade().is_some_and(|entity| {
                    let this = entity.read(cx);
                    this.editing.is_none()
                        && this.row_pending.is_none()
                        && !this.action_pending
                        && this.actions_row == open
                        && this
                            .threads
                            .iter()
                            .chain(this.archived.iter())
                            .any(|thread| thread == &observed)
                        && this
                            .organization
                            .as_ref()
                            .and_then(|org| org.snapshot.clone())
                            == projection
                })
            },
            move |index, _, window, cx| {
                let _ = target.update(cx, |this, cx| match (quick, index) {
                    (true, 0) => this.toggle_pin(&thread_id, pinned, cx),
                    (true, 1) => this.set_thread_status(
                        &thread_id,
                        if archived {
                            ThreadStatus::Active
                        } else {
                            ThreadStatus::Archived
                        },
                        cx,
                    ),
                    _ => this.toggle_actions(&thread_id, &observed_row, window, cx),
                });
            },
        );
        let mut group = div()
            .relative()
            .w(px(Layout::SIDEBAR_ACTIONS_WIDTH))
            .mr(px(Layout::SIDEBAR_ACTIONS_INSET))
            .h(px(Layout::SIDEBAR_ACTION_SIZE))
            .flex_shrink_0()
            .child(content);
        if menu_open {
            group = group
                .track_focus(&self.actions_scope_focus)
                .key_context("ThreadActionsMenu")
                .on_action(cx.listener(Self::previous_action))
                .on_action(cx.listener(Self::next_action))
                .on_action(cx.listener(Self::activate_action))
                .on_action(cx.listener(Self::close_actions_action))
                .child(
                    div().absolute().top_0().right_0().child(
                        anchored()
                            .anchor(Anchor::TopRight)
                            .position_mode(AnchoredPositionMode::Local)
                            .position(point(px(0.), px(Layout::SIDEBAR_ACTION_SIZE)))
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                deferred(self.render_action_menu(
                                    thread,
                                    archived,
                                    action_prefix,
                                    cx,
                                ))
                                .with_priority(2),
                            ),
                    ),
                );
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
        let fork_index = Self::supports_fork(thread).then_some(actions.len());
        let fork_unavailable = self.fork_unavailable(thread, cx);
        if fork_index.is_some() {
            actions.push((vega_i18n::t("ui.branch_in_new_chat"), false));
        }
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
        let content = div()
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
            .p(px(5.))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.actions_open.is_some() {
                        this.close_actions();
                        cx.notify();
                    }
                }),
            )
            .child({
                let observed = thread.clone();
                let row = self.actions_row.clone();
                let projection = self
                    .organization
                    .as_ref()
                    .and_then(|org| org.snapshot.clone());
                let weak = cx.weak_entity();
                let target = weak.clone();
                let items = actions
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, danger))| {
                        let mut item = nsh_leaf::Item::action(label, None);
                        item.danger = danger;
                        if fork_index == Some(index)
                            && let Some(reason) = fork_unavailable
                        {
                            item.enabled = false;
                            item.wrap = true;
                            item.label = format!("{} — {}", item.label, vega_i18n::t(reason));
                        }
                        item
                    })
                    .collect::<Vec<_>>();
                nsh_leaf::menu(
                    format!("{action_prefix}thread-menu-nsh-{thread_id}"),
                    None,
                    items,
                    self.actions_scroll.clone(),
                    move |cx| {
                        weak.upgrade().is_some_and(|entity| {
                            let this = entity.read(cx);
                            this.actions_row == row
                                && this.actions_open.as_deref() == Some(observed.id.as_str())
                                && this
                                    .threads
                                    .iter()
                                    .chain(this.archived.iter())
                                    .any(|thread| thread == &observed)
                                && this
                                    .organization
                                    .as_ref()
                                    .and_then(|org| org.snapshot.clone())
                                    == projection
                                && !this.action_pending
                                && this.editing.is_none()
                        })
                    },
                    move |index, _, window, cx| {
                        let _ = target.update(cx, |this, cx| {
                            this.activate_action_index(&thread_id, archived, index, window, cx)
                        });
                    },
                )
            })
            .sidebar_scroll_fade(
                format!("{action_prefix}thread-menu-fade-{}", thread.id),
                self.actions_scroll.clone(),
                true,
            )
            .into_any_element();
        nsh_leaf::chrome::popup(
            format!("{action_prefix}thread-menu-chrome-{}", thread.id),
            content,
        )
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
