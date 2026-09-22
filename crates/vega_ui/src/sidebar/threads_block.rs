use super::*;
use vega_conversation::types::ThreadUpdate;
mod organization;
use organization::{Organization, OrganizationSection};

/// Events emitted by the session block.
pub enum ThreadsBlockEvent {
    /// A thread row was opened; the content column switched to it.
    Opened,
}

impl EventEmitter<ThreadsBlockEvent> for ThreadsBlock {}

/// Shared task rows/actions. The production organization projection renders
/// pinned tasks, project folders, and recent standalone tasks exactly once;
/// standalone legacy consumers retain the selected-project block below.
///
/// The 「会话」 block: the selected project's threads, pinned group first,
/// then `updated_at` desc (store ordering, ui-spec §4.1 置顶组优先). Rows =
/// truncated title; the selected row gets
/// `bg_active`; unread rows render medium weight + a dot (the field stays 0
/// until S3 produces unread state).
///
/// T13 (A1-05) session management: the main list reads `status = active`
/// only; archived threads hide here and surface in the 「已归档 (N)」
/// collapsed section at the bottom of the block (展开可查看，行上有「恢复」).
/// Hovering a row reveals its compact action trigger (裁决①：置顶 /
/// 归档或恢复 / 删除); the trigger is always keyboard reachable, while its
/// low-frequency actions are collected in a focusable menu. Double-clicking a
/// row enters inline renaming via the shared [`TextInput`] (Enter submits, Esc
/// cancels, an empty title cancels — the keyboard path itself is
/// manual-acceptance, see [`resolve_rename`]).
/// No project selected → guidance copy. Collapse state persists in config
/// (`ui.sessions_collapsed`); the archive expansion is in-memory only.
pub struct ThreadsBlock {
    /// Cached active rows (all projects when organization is enabled).
    pub(crate) threads: Vec<Thread>,
    /// Cached archived rows (shown in the 「已归档」 collapsed section).
    pub(crate) archived: Vec<Thread>,
    /// Project id the cache was loaded for (`None` → guidance copy).
    pub(crate) loaded_project: Option<String>,
    /// Thread id currently under the mouse; drives the row hover background.
    pub(crate) hovered: Option<String>,
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
    /// In-memory progressive-list state. Projects keeps the R29
    /// `Show More / Show Less` toggle; Recents no longer has a control — it
    /// grows its render window as the outer Sidebar scroller reaches the
    /// bottom (Issue #57).
    pub(crate) projects_expanded: bool,
    pub(crate) projects_progressive_focus: FocusHandle,
    /// Issue #57: how many Recents rows the current render window contains.
    /// Starts at [`organization::RECENTS_PAGE`] and only grows (never shrinks)
    /// for the lifetime of this block.
    pub(crate) recents_visible: usize,
    /// Issue #57: the number of eligible Recents rows observed by the last
    /// render. Written during render; read by [`Self::grow_recents`] so the
    /// outer scroller never asks for more than exists.
    pub(crate) recents_total: usize,
    /// Thread id whose compact low-frequency action menu is open.
    pub(crate) actions_open: Option<String>,
    /// Highlighted action inside the open menu (arrow keys move it).
    pub(crate) actions_highlight: usize,
    /// Scope handle for the open action menu; leaving the row closes it.
    pub(crate) actions_scope_focus: FocusHandle,
    /// One focus-out subscription for the action-menu scope.
    pub(crate) actions_focus_subscription: Option<Subscription>,
    /// Whether the 「已归档 (N)」 section is expanded (in-memory, T13 卡允许
    /// 不做折叠记忆).
    pub(crate) archive_expanded: bool,
    /// Active inline rename session (`None` = not renaming).
    pub(crate) editing: Option<RenameSession>,
    /// Inline error message (ui-spec §4.6).
    pub(crate) error: Option<String>,
    action_pending: bool,
    project_generation: u64,
    actions_scroll: gpui_kit::ScrollHandle,
    menu_height: gpui_kit::Pixels,
    organization: Option<Organization>,
}

/// An inline rename in progress: the thread being renamed plus the shared
/// [`TextInput`] entity pre-filled with its current title.
pub(crate) struct RenameSession {
    pub(crate) thread_id: String,
    pub(crate) input: Entity<TextInput>,
}

impl ThreadsBlock {
    /// Creates the block and loads the selected project's thread list.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            threads: Vec::new(),
            archived: Vec::new(),
            loaded_project: None,
            hovered: None,
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
            actions_highlight: 0,
            actions_scope_focus: cx.focus_handle(),
            actions_focus_subscription: None,
            archive_expanded: false,
            editing: None,
            error: None,
            action_pending: false,
            project_generation: 0,
            actions_scroll: gpui_kit::ScrollHandle::new(),
            menu_height: px(360.),
            organization: None,
        };
        view.reload(cx);
        view
    }

    /// Issue #57: appends one page to the Recents render window when the
    /// outer Sidebar scroller has reached the bottom and more eligible rows
    /// exist. Returns whether the window changed; a change notifies so the
    /// next frame paints the appended rows.
    ///
    /// The window only ever grows, so already-painted rows keep their
    /// positions and the stable sort is never re-ordered.
    pub(crate) fn grow_recents(&mut self, cx: &mut Context<Self>) -> bool {
        if self.recents_visible >= self.recents_total {
            return false;
        }
        self.recents_visible = self
            .recents_visible
            .saturating_add(organization::RECENTS_PAGE)
            .min(self.recents_total);
        cx.notify();
        true
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
            self.actions_open = None;
            self.actions_highlight = 0;
        }
        cx.notify();
    }

    /// R69 R14: navigates to the home draft route for the requested project
    /// binding instead of eagerly INSERTing a row. `None` is a genuine
    /// standalone task and is used by the SESSIONS plus button.
    ///
    /// The draft is materialized by the window on first submit (R8), which is
    /// what stops this entry point from piling up empty `未命名任务` rows. The
    /// store is still read here for the project's display name so the route
    /// carries the same selection the eager path used to install.
    pub(crate) fn create_task(&mut self, project_id: Option<String>, cx: &mut Context<Self>) {
        if task_mutation_busy(cx) {
            self.error = Some("任务正在保存，请稍后重试".into());
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
                    .map_err(|error| error.to_string())
                    .map(|row| row.is_some())
            });
            match exists {
                Ok(true) => {}
                Ok(false) => {
                    self.error = Some("项目不存在，无法创建任务".into());
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
        cx.emit(ThreadsBlockEvent::Opened);
        cx.refresh_windows();
    }

    /// Click = open: bumps `threads.updated_at` + the owning project's
    /// `last_opened_at` (single transaction) and switches the content column
    /// via the [`OpenedThread`] global.
    fn open_thread(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        if cx.has_active_drag() {
            return;
        }
        if task_mutation_busy(cx) {
            self.error = Some("任务正在保存，请稍后重试".into());
            cx.notify();
            return;
        }
        if !crate::navigation::allow_task_navigation(Some(thread_id), cx) {
            return;
        }
        self.close_actions();
        crate::navigation::begin_task_mutation(cx);
        let result = with_store(cx, |store| {
            conversation::visit_thread(store, thread_id).map_err(|error| error.to_string())
        });
        crate::navigation::finish_task_mutation(cx);
        match result {
            Ok(opened) => {
                self.error = None;
                cx.set_global(SelectedProject(opened.project_binding().map(str::to_owned)));
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
        if task_mutation_busy(cx) {
            self.error = Some("任务正在保存，请稍后重试".into());
            cx.notify();
            return;
        }
        crate::navigation::begin_task_mutation(cx);
        let result = with_store(cx, operation);
        crate::navigation::finish_task_mutation(cx);
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
        self.close_actions();
        self.editing = None;
        cx.set_global(PendingDeleteConfirm(Some(thread.clone())));
        cx.refresh_windows();
    }

    /// Double-click = rename: builds the inline editor pre-filled with the
    /// current title and moves focus into it. (合成键盘事件送不进 GPUI：键入
    /// 与 Enter/Esc 提交路径为人工验收；提交/取消的纯逻辑见 [`resolve_rename`]。)
    fn start_rename(&mut self, thread: &Thread, window: &mut Window, cx: &mut Context<Self>) {
        self.close_actions();
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
        let thread_ids: HashSet<_> = self
            .threads
            .iter()
            .chain(self.archived.iter())
            .map(|thread| thread.id.clone())
            .collect();
        self.thread_action_focuses
            .retain(|id, _| thread_ids.contains(id));
        for thread in self.threads.iter().chain(self.archived.iter()) {
            self.thread_action_focuses
                .entry(thread.id.clone())
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
    fn toggle_actions(&mut self, thread_id: &str, _: &mut Window, cx: &mut Context<Self>) {
        if cx.has_active_drag() {
            return;
        }
        if self.actions_open.as_deref() == Some(thread_id) {
            self.close_actions();
        } else {
            self.actions_open = Some(thread_id.to_string());
            self.actions_highlight = 0;
        }
        cx.notify();
    }

    fn close_actions(&mut self) {
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
            self.error = Some("任务操作正在保存，请稍后重试".into());
            cx.notify();
            return;
        }
        let generation = self.project_generation;
        let id = thread.id.clone();
        let project = thread.project_id.clone();
        let database = with_store(cx, |store| {
            store
                .database_path()
                .map(Path::to_path_buf)
                .ok_or_else(|| "任务存储需要文件数据库".to_string())
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
            let store = Store::open(database).map_err(|e| e.to_string())?;
            let renamed_at = if let Some(title) = &update.title {
                Some(
                    conversation::rename_thread(&store, &id, title)
                        .map_err(|e| e.to_string())?
                        .updated_at,
                )
            } else {
                conversation::update_thread(&store, &id, &update).map_err(|e| e.to_string())?;
                None
            };
            Ok::<_, String>((id, project, renamed_at))
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
            self.error = Some("任务操作正在保存，请稍后重试".into());
            cx.notify();
            return;
        }
        let generation = self.project_generation;
        let id = thread.id.clone();
        let project = thread.project_id.clone();
        let target_id = id.clone();
        let route = cx.global::<OpenedThread>().0.as_ref().map(|t| t.id.clone());
        let database = with_store(cx, |store| {
            store
                .database_path()
                .map(Path::to_path_buf)
                .ok_or_else(|| "任务存储需要文件数据库".to_string())
        });
        let Ok(database) = database else {
            self.error = database.err();
            cx.notify();
            return;
        };
        self.action_pending = true;
        let worker = cx.background_executor().spawn(async move {
            let store = Store::open_read_only(database).map_err(|e| e.to_string())?;
            conversation::task_project_path(&store, &id).map_err(|e| e.to_string())
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
        self.close_actions();
        let project_bound = !thread.is_standalone();
        match (project_bound, index) {
            (_, 0) => self.toggle_pin(thread_id, thread.pinned, cx),
            (_, 1) => self.start_rename(&thread, window, cx),
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
                            .child("SESSIONS"),
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
                    .child(format!("已归档 ({})", self.archived.len())),
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

    /// One session row per ui-spec §4.1: [pin mark][title] [dot] plus a compact
    /// action trigger. The selected row gets
    /// `bg_active`; hovering a non-editing row gets `bg_hover` and reveals the
    /// trigger for its action menu (置顶 / 归档或恢复 / 删除；行内编辑行除外).
    ///
    /// The clickable body and the right side are sibling nodes — clicks on
    /// the action buttons must not re-trigger open (T10 经验：兄弟节点避免
    /// 嵌套命中).
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
            // R48: a project child row has no leading icon, so it uses the
            // ladder's text column directly (base + SIDEBAR_ROW_INSET), landing
            // on the same column as its project row's name.
            Layout::SIDEBAR_ROW_INSET,
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
            0.0,
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
            8.0,
            8.0,
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
        row_height: f32,
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
        let actions_visible = row_shows_actions(hovered, editing_this_row)
            || self.actions_open.as_deref() == Some(thread.id.as_str())
            || self.focused_thread_action.as_deref() == Some(thread.id.as_str());
        let thread_id = thread.id.clone();
        let row_thread = thread.clone();
        let mut row = div()
            .id(ElementId::Name(
                format!("{selector_prefix}{thread_id}").into(),
            ))
            .debug_selector({
                let id = thread_id.clone();
                let selector_prefix = selector_prefix.to_owned();
                move || format!("{selector_prefix}{id}")
            })
            .h(px(row_height))
            .ml(px(-surface_leading_outset))
            .flex()
            .items_center()
            .rounded_lg()
            .overflow_hidden()
            .text_size(px(Typography::SIDEBAR))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.set_hovered(&thread_id, *hovered, cx);
            }))
            .when(
                selected || (actions_visible && !editing_this_row),
                move |row| {
                    row.bg(if selected {
                        colors.bg_active
                    } else {
                        colors.bg_hover
                    })
                },
            );
        if let Some(session) = editing_session {
            row = row.child(self.render_rename_editor(session, cx));
        } else {
            row = row
                // 可点击主体与右侧操作是兄弟节点：双击进入行内编辑，单击
                // 打开会话（双击序列的第一次单击仍会先打开，属预期）。
                .child(
                    div()
                        .debug_selector({
                            let id = thread.id.clone();
                            let selector_prefix = selector_prefix.to_owned();
                            move || {
                                format!(
                                    "{selector_prefix}surface-{id}-{}",
                                    if selected { "active" } else { "rest" }
                                )
                            }
                        })
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
                                    this.start_rename(&row_thread, window, cx);
                                } else {
                                    this.open_thread(&row_thread.id, cx);
                                }
                            }),
                        )
                        .children((show_pin_indicator && thread.pinned).then(|| {
                            // 置顶小标记：token 色着色（裁决③）。
                            div()
                                .debug_selector({
                                    let id = thread.id.clone();
                                    let selector_prefix = selector_prefix.to_owned();
                                    move || format!("{selector_prefix}pin-{id}")
                                })
                                .flex_shrink_0()
                                .text_color(colors.accent)
                                .child(crate::icons::icon(crate::icons::Icon::Pin, colors.accent))
                        }))
                        .child(
                            div()
                                .id(ElementId::Name(
                                    format!("{selector_prefix}title-{}", thread.id).into(),
                                ))
                                .debug_selector({
                                    let id = thread.id.clone();
                                    let selector_prefix = selector_prefix.to_owned();
                                    move || format!("{selector_prefix}title-{id}")
                                })
                                .flex_1()
                                .min_w_0()
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
                row.child(div().w(px(Layout::SIDEBAR_ACTIONS_WIDTH)).flex_shrink_0())
            };
        }
        row.into_any_element()
    }

    /// The fixed-width row tail. Production organization rows are quiet at
    /// rest; hover (or keyboard focus) reveals one compact action trigger. The
    /// actual operations are rendered in a deferred anchored menu so the row
    /// and sidebar scroll masks cannot clip the popup.
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
        let action_prefix = selector_prefix
            .strip_suffix("thread-row-")
            .unwrap_or(selector_prefix);
        let menu_open = self.actions_open.as_deref() == Some(thread.id.as_str());
        let mut trigger = div()
            .id(ElementId::Name(
                format!("{action_prefix}thread-actions-{thread_id}").into(),
            ))
            .debug_selector({
                let id = thread_id.clone();
                let action_prefix = action_prefix.to_owned();
                move || format!("{action_prefix}thread-actions-{id}")
            })
            .relative()
            .w(px(28.))
            .h(px(28.))
            .rounded_md()
            .text_size(px(Typography::HEADING_BLOCK))
            .text_color(colors.text_secondary)
            .aria_label("会话操作")
            .focusable()
            .tab_stop(true)
            .when_some(
                self.organization
                    .as_ref()
                    .and_then(|_| self.thread_action_focuses.get(&thread.id)),
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
                    move |this, _: &OpenThreadActions, window, cx| {
                        this.toggle_actions(&thread_id, window, cx);
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
            // A mouse click focuses the trigger automatically. When the menu
            // is already open, consume a second click on the trigger so the
            // outside-dismiss listener cannot reopen it in the bubble phase.
            .capture_any_mouse_up(cx.listener({
                let thread_id = thread.id.clone();
                move |this, event: &MouseUpEvent, _, cx| {
                    if event.button == MouseButton::Left
                        && this.actions_open.as_deref() == Some(thread_id.as_str())
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
                    move |this, _: &MouseUpEvent, window, cx| {
                        this.toggle_actions(&thread_id, window, cx);
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

        let mut group = div()
            .debug_selector({
                let id = thread.id.clone();
                let state = if actions_visible { "visible" } else { "rest" };
                move || format!("thread-actions-state-{id}-{state}")
            })
            .relative()
            .w(px(Layout::SIDEBAR_ACTIONS_WIDTH))
            .flex()
            .items_center()
            .justify_end()
            .flex_shrink_0()
            .pr_1();
        if menu_open {
            group = group.track_focus(&self.actions_scope_focus);
        }
        if !actions_visible && show_timestamp_at_rest {
            group = group.child(
                div()
                    .debug_selector({
                        let id = thread.id.clone();
                        move || format!("thread-timestamp-{id}")
                    })
                    .flex_1()
                    .min_w_0()
                    .text_right()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(relative_time(thread.updated_at)),
            );
        }
        // Keep the trigger mounted even at rest so Tab can reach every row's
        // action entry. It is visually quiet until hover/focus/open.
        if !actions_visible {
            trigger = trigger.opacity(0.);
        }
        group = group.child(trigger);
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
                    "取消置顶"
                } else {
                    "置顶"
                },
                false,
            ),
            ("重命名", false),
            (if archived { "恢复" } else { "归档" }, false),
            (
                if thread.unread {
                    "标记为已读"
                } else {
                    "标记为未读"
                },
                false,
            ),
        ];
        if !thread.is_standalone() {
            actions.extend([("在 Finder 中打开项目", false), ("复制项目路径", false)]);
        }
        actions.extend([("复制会话 ID", false), ("前往设置", false), ("删除", true)]);
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
                            .debug_selector({
                                let id = thread_id.clone();
                                let action_prefix = action_prefix.to_owned();
                                move || format!("{action_prefix}thread-action-{id}-{index}")
                            })
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
                                    this.activate_action_index(
                                        &thread_id, archived, index, window, cx,
                                    );
                                }),
                            )
                            .child(label);
                        if selected {
                            item = item.bg(colors.bg_active);
                        }
                        item
                    }),
            )
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

#[cfg(test)]
mod task_action_tests {
    use super::*;

    fn fixture(
        cx: &mut gpui_kit::TestAppContext,
    ) -> (tempfile::TempDir, Entity<ThreadsBlock>, Thread, Thread) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("owned.db")).unwrap();
        store.migrate().unwrap();
        store.conn().execute("INSERT INTO projects (id,path,name,created_at,last_opened_at) VALUES ('p',?1,'owned',0,0)", [dir.path().to_str().unwrap()]).unwrap();
        let first = conversation::create_thread(&store, "p", "initial", "confirm").unwrap();
        let other = conversation::create_thread(&store, "p", "other", "confirm").unwrap();
        cx.update(|cx| {
            cx.set_global(VegaStore(Ok(store)));
            cx.set_global(SelectedProject(Some("p".into())));
            cx.set_global(OpenedThread(Some(first.clone())));
        });
        let block = cx.new(ThreadsBlock::new);
        (dir, block, first, other)
    }

    #[gpui_kit::test]
    async fn task_mutation_epoch_fences_real_archive_and_releases_after_dropped_async_owner(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::navigation::{TaskMutationState, begin_task_mutation, finish_task_mutation};
        let (dir, block, first, other) = fixture(cx);
        // Model an accepted N visit: A must not archive its destination while pending.
        cx.update(begin_task_mutation);
        block.update(cx, |block, cx| {
            block.set_thread_status(&other.id, ThreadStatus::Archived, cx)
        });
        let store = Store::open(dir.path().join("owned.db")).unwrap();
        assert_eq!(
            conversation::open_thread(&store, &other.id).unwrap().status,
            ThreadStatus::Active
        );
        block.read_with(cx, |block, _| assert!(block.error.is_some()));
        cx.update(finish_task_mutation);
        let resolved_epoch = cx.update(|cx| *cx.global::<TaskMutationState>());
        block.update(cx, |block, cx| {
            block.set_thread_status(&other.id, ThreadStatus::Archived, cx)
        });
        assert_eq!(
            conversation::open_thread(&store, &other.id).unwrap().status,
            ThreadStatus::Archived
        );
        cx.update(|cx| {
            let state = cx.global::<TaskMutationState>();
            assert_eq!(state.pending, 0);
            assert_ne!(
                *state, resolved_epoch,
                "a previously resolved N target must now fail its fence"
            );
        });
        block.update(cx, |block, cx| {
            block.apply_update(
                &first,
                ThreadUpdate {
                    title: Some("durable after drop".into()),
                    ..Default::default()
                },
                cx,
            )
        });
        cx.update(|cx| assert_eq!(cx.global::<TaskMutationState>().pending, 1));
        drop(block);
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(cx.global::<TaskMutationState>().pending, 0));
        assert_eq!(
            conversation::open_thread(&store, &first.id).unwrap().title,
            "durable after drop"
        );
    }

    #[gpui_kit::test]
    async fn task_menu_keyboard_reaches_unread_and_escape(cx: &mut gpui_kit::TestAppContext) {
        let (dir, block, _, _) = fixture(cx);
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(SessionsCollapsed(false));
            crate::init(cx);
        });
        let target = block.read_with(cx, |block, _| block.threads[0].id.clone());
        let root = block.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, _| root)
                .unwrap()
        });
        cx.run_until_parked();
        // Establish the platform's initial tab stop; subsequent input uses real dispatch.
        window
            .update(cx, |_, window, cx| window.focus_next(cx))
            .unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        block.read_with(cx, |block, _| {
            assert_eq!(block.actions_open.as_deref(), Some(target.as_str()))
        });
        cx.simulate_keystrokes(window.into(), "down down down enter");
        cx.run_until_parked();
        let store = Store::open(dir.path().join("owned.db")).unwrap();
        assert!(conversation::open_thread(&store, &target).unwrap().unread);
        cx.simulate_keystrokes(window.into(), "enter escape");
        cx.run_until_parked();
        block.read_with(cx, |block, _| assert!(block.actions_open.is_none()));
        cx.simulate_keystrokes(window.into(), "tab enter");
        cx.run_until_parked();
        block.read_with(cx, |block, _| {
            assert!(block.actions_open.is_some());
            assert_ne!(block.actions_open.as_deref(), Some(target.as_str()));
        });
    }

    #[gpui_kit::test]
    async fn standalone_task_menu_wraps_seven_actions_and_activates_delete(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (dir, block, _, _) = fixture(cx);
        let store = Store::open(dir.path().join("owned.db")).unwrap();
        let standalone = conversation::create_standalone_thread(&store, "mock", "confirm").unwrap();
        block.update(cx, |block, _| {
            block.threads = vec![standalone.clone()];
            block.archived.clear();
            block.loaded_project = Some("p".into());
        });
        cx.update(|cx| {
            cx.set_global(OpenedThread(Some(standalone.clone())));
            cx.set_global(PendingDeleteConfirm(None));
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(SessionsCollapsed(false));
        });

        let root = block.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, _| root)
                .unwrap()
        });
        cx.run_until_parked();
        block.update(cx, |block, _| {
            block.actions_open = Some(standalone.id.clone());
            block.actions_highlight = 0;
        });
        block.read_with(cx, |block, _| {
            assert_eq!(block.actions_open.as_deref(), Some(standalone.id.as_str()));
            assert_eq!(block.actions_highlight, 0);
        });

        // These are the handlers bound to Up/Down/Enter in ThreadActionsMenu.
        window
            .update(cx, |block, window, cx| {
                block.move_action_highlight(-1, cx);
                assert_eq!(block.actions_highlight, 6);
                block.move_action_highlight(1, cx);
                assert_eq!(block.actions_highlight, 0);
                block.move_action_highlight(-1, cx);
                let index = block.actions_highlight;
                block.activate_action_index(&standalone.id, false, index, window, cx);
            })
            .unwrap();
        block.read_with(cx, |block, _| assert!(block.actions_open.is_none()));
        cx.update(|cx| {
            assert_eq!(
                cx.global::<PendingDeleteConfirm>()
                    .0
                    .as_ref()
                    .map(|thread| &thread.id),
                Some(&standalone.id)
            );
        });
    }

    #[gpui_kit::test]
    async fn durable_unread_ack_preserves_new_model_and_metadata_refresh(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (dir, block, first, _) = fixture(cx);
        block.update(cx, |block, cx| {
            block.apply_update(
                &first,
                ThreadUpdate {
                    unread: Some(true),
                    ..Default::default()
                },
                cx,
            )
        });
        cx.update(|cx| {
            let mut current = cx.global::<OpenedThread>().0.clone().unwrap();
            current.model = "later-model".into();
            cx.set_global(OpenedThread(Some(current)));
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let current = cx.global::<OpenedThread>().0.as_ref().unwrap();
            assert!(current.unread);
            assert_eq!(current.model, "later-model");
        });
        let reopened = Store::open(dir.path().join("owned.db")).unwrap();
        assert!(
            conversation::open_thread(&reopened, &first.id)
                .unwrap()
                .unread
        );
        assert!(
            !conversation::visit_thread(&reopened, &first.id)
                .unwrap()
                .unread
        );
    }

    #[gpui_kit::test]
    async fn rename_noncurrent_task_does_not_navigate_and_failure_retains_input(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (dir, block, first, other) = fixture(cx);
        block.update(cx, |block, cx| {
            let input = cx.new(|cx| TextInput::new(cx, "title", false));
            input.update(cx, |input, cx| input.set_text("renamed", cx));
            block.editing = Some(RenameSession {
                thread_id: other.id.clone(),
                input,
            });
            block.commit_rename(cx);
        });
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(cx.global::<OpenedThread>().0.as_ref().unwrap().id, first.id));
        block.read_with(cx, |block, _| assert!(block.editing.is_none()));
        let store = Store::open(dir.path().join("owned.db")).unwrap();
        assert_eq!(
            conversation::open_thread(&store, &other.id).unwrap().title,
            "renamed"
        );
        store.conn().execute_batch("CREATE TRIGGER reject_rename BEFORE UPDATE OF title ON threads BEGIN SELECT RAISE(ABORT, 'owned failure'); END;").unwrap();
        block.update(cx, |block, cx| {
            let input = cx.new(|cx| TextInput::new(cx, "title", false));
            input.update(cx, |input, cx| input.set_text("retry me", cx));
            block.editing = Some(RenameSession {
                thread_id: other.id.clone(),
                input,
            });
            block.commit_rename(cx);
        });
        cx.run_until_parked();
        block.read_with(cx, |block, cx| {
            assert!(block.error.is_some());
            assert_eq!(
                block.editing.as_ref().unwrap().input.read(cx).text(),
                "retry me"
            );
        });
        assert_eq!(
            conversation::open_thread(&store, &other.id).unwrap().title,
            "renamed"
        );
    }

    #[gpui_kit::test]
    async fn late_task_ack_cannot_replace_new_route(cx: &mut gpui_kit::TestAppContext) {
        let (dir, block, first, other) = fixture(cx);
        block.update(cx, |block, cx| {
            block.apply_update(
                &first,
                ThreadUpdate {
                    unread: Some(true),
                    ..Default::default()
                },
                cx,
            )
        });
        cx.update(|cx| cx.set_global(OpenedThread(Some(other.clone()))));
        cx.run_until_parked();
        cx.update(|cx| {
            let current = cx.global::<OpenedThread>().0.as_ref().unwrap();
            assert_eq!(current.id, other.id);
            assert!(!current.unread);
        });
        let store = Store::open(dir.path().join("owned.db")).unwrap();
        assert!(conversation::open_thread(&store, &first.id).unwrap().unread);
    }
}
