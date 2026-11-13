use super::*;
use gpui_kit::BackgroundExecutor;
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};
use vega_conversation::{
    ProjectBranchService,
    types::{ProjectBranchCompletion, ProjectBranchState, ProjectBranchTarget},
};

/// Events emitted by the projects block; the sidebar orchestrator reacts.
pub enum ProjectsBlockEvent {
    /// A project row was clicked and is now the selected project.
    Selected(String),
    /// A project row was removed.
    Removed(String),
    /// Folder registration and its organization reveal have durably committed.
    Registered(String),
    VisitRecorded,
}

impl EventEmitter<ProjectsBlockEvent> for ProjectsBlock {}

pub struct ProjectsBlock {
    /// Cached rows, refreshed from the store after every mutation.
    pub(crate) projects: Vec<Project>,
    pub(crate) sort: ProjectSort,
    /// Inline error message (ui-spec §4.6); empty until a failure occurs.
    pub(crate) error: Option<crate::localized_copy::LocalizedCopy>,
    branches: HashMap<String, (String, ProjectBranchState)>,
    branch_service: Option<ProjectBranchService>,
    branch_generation: u64,
    branch_cursor: usize,
    next_branch_refresh: Instant,
    branch_clock: BackgroundExecutor,
    branch_probe: bool,
    branch_rendered: bool,
    branch_pending: bool,
    branch_started: Option<Instant>,
    branch_selection: Option<String>,
    branch_completion: Option<ProjectBranchCompletion>,
    pub(super) organization_mode: bool,
    registration_pending: bool,
    reload_generation: u64,
    reload_pending: bool,
    reload_requested: bool,
    selection_generation: u64,
    route_generation: u64,
    selection_pending: bool,
    selection_requested: Option<(String, PathBuf)>,
}

impl ProjectsBlock {
    /// Creates the block and loads the project list.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            projects: Vec::new(),
            // 侧边栏默认「最近打开」：与初始选中项目（latest_project 语义）一致。
            sort: ProjectSort::RecentlyOpened,
            error: None,
            branches: HashMap::new(),
            branch_service: ProjectBranchService::new().ok(),
            branch_generation: 0,
            branch_cursor: 0,
            next_branch_refresh: cx.background_executor().now(),
            branch_clock: cx.background_executor().clone(),
            branch_probe: false,
            branch_rendered: false,
            branch_pending: false,
            branch_started: None,
            branch_selection: None,
            branch_completion: None,
            organization_mode: false,
            registration_pending: false,
            reload_generation: 0,
            reload_pending: false,
            reload_requested: false,
            selection_generation: 0,
            route_generation: 0,
            selection_pending: false,
            selection_requested: None,
        };
        cx.observe_global::<SelectedProject>(|this, _| {
            this.route_generation = this.route_generation.wrapping_add(1)
        })
        .detach();
        cx.observe_global::<OpenedThread>(|this, _| {
            this.route_generation = this.route_generation.wrapping_add(1)
        })
        .detach();
        cx.observe_global::<SettingsOpen>(|this, _| {
            this.route_generation = this.route_generation.wrapping_add(1)
        })
        .detach();
        view.reload(cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                // The organized sidebar renders project names without branch suffixes.
                // Its project controller stays alive for actions, but has no branch rows to probe.
                let poll = this.update(cx, |this, cx| {
                    if this.organization_mode {
                        return false;
                    }
                    this.poll_branches(cx);
                    true
                });
                if !matches!(poll, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        view
    }

    /// Re-reads the project list in the current sort order.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.invalidate_background_reload();
        self.invalidate_branches();
        let sort = self.sort;
        match with_store(cx, |store| {
            projects::list(store.conn(), sort).map_err(|error| {
                crate::localized_copy::LocalizedCopy::with_args(
                    "ui.could_not_load_projects_error",
                    vec![("error", (error).to_string().into())],
                )
            })
        }) {
            Ok(projects) => {
                self.projects = projects;
            }
            Err(message) => self.error = Some(message),
        }
        cx.notify();
    }

    fn invalidate_background_reload(&mut self) {
        self.reload_generation = self.reload_generation.wrapping_add(1);
        self.reload_requested = false;
    }

    pub(super) fn reload_background(&mut self, cx: &mut Context<Self>) {
        self.reload_generation = self.reload_generation.wrapping_add(1);
        self.reload_requested = true;
        self.start_background_reload(cx);
    }

    fn start_background_reload(&mut self, cx: &mut Context<Self>) {
        if self.reload_pending || !self.reload_requested {
            return;
        }
        self.reload_requested = false;
        self.reload_pending = true;
        let generation = self.reload_generation;
        let sort = self.sort;
        let database = with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
            .ok()
            .flatten();
        let worker_database = database.clone();
        let worker = cx.background_executor().spawn(async move {
            let database = worker_database.ok_or_else(|| {
                crate::localized_copy::LocalizedCopy::new(
                    "ui.project_storage_requires_a_file_backed_database",
                )
            })?;
            let store = Store::open_existing(database)
                .map_err(crate::localized_copy::LocalizedCopy::storage)?;
            projects::list(store.conn(), sort).map_err(|error| {
                crate::localized_copy::LocalizedCopy::with_args(
                    "ui.could_not_load_projects_error",
                    vec![("error", error.to_string().into())],
                )
            })
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.reload_pending = false;
                let current_database =
                    with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                        .ok()
                        .flatten();
                if this.reload_generation == generation
                    && this.sort == sort
                    && current_database == database
                    && !this.registration_pending
                {
                    match result {
                        Ok(projects) => {
                            this.projects = projects;
                            this.invalidate_branches();
                        }
                        Err(error) if this.error.is_none() => this.error = Some(error),
                        Err(_) => {}
                    }
                    cx.notify();
                }
                this.start_background_reload(cx);
            });
        })
        .detach();
    }

    fn invalidate_branches(&mut self) {
        self.branches.clear();
        if let Some(generation) = self.branch_generation.checked_add(1) {
            self.branch_generation = generation;
        } else {
            self.branch_service = None;
        }
        if let Some(service) = &self.branch_service {
            service.invalidate(self.branch_generation);
        }
        self.branch_pending = false;
        self.branch_started = None;
        self.branch_completion = None;
        self.branch_probe = false;
        self.branch_cursor = 0;
        self.next_branch_refresh = self.branch_clock.now();
    }

    #[allow(dead_code)]
    pub(super) fn organization_branch(&mut self, project_id: &str) -> Option<String> {
        if self.branch_probe {
            self.branch_rendered = true;
        }
        self.projects
            .iter()
            .find(|project| project.id == project_id)
            .and_then(|project| self.branch_suffix(project))
            .map(str::to_owned)
    }

    fn branch_suffix(&self, project: &Project) -> Option<&str> {
        self.branches
            .get(&project.id)
            .filter(|(path, _)| path == &project.path)
            .and_then(|(_, state)| state.suffix())
    }

    fn poll_branches(&mut self, cx: &mut Context<Self>) {
        let selection = cx
            .try_global::<SelectedProject>()
            .and_then(|value| value.0.clone());
        if selection != self.branch_selection {
            self.branch_selection = selection;
            self.invalidate_branches();
        }
        let visible = (self.organization_mode
            || !cx.try_global::<ProjectsCollapsed>().is_some_and(|v| v.0))
            && !cx.try_global::<SidebarCollapsed>().is_some_and(|v| v.0)
            && !cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|v| v.0);
        if !visible || self.error.is_some() {
            if self.branch_pending || !self.branches.is_empty() {
                self.invalidate_branches();
            }
            self.branch_probe = false;
            self.branch_rendered = false;
            return;
        }
        if self.branch_started.is_some_and(|start| {
            self.branch_clock.now().duration_since(start) >= Duration::from_secs(1)
        }) {
            // A stalled filesystem cannot keep a previously displayed branch indefinitely.
            self.invalidate_branches();
            self.next_branch_refresh = self.branch_clock.now() + Duration::from_secs(2);
            cx.notify();
            return;
        }
        if let Some(completion) = self
            .branch_service
            .as_ref()
            .and_then(|s| s.take_completion())
            && completion.generation == self.branch_generation
        {
            self.branch_completion = Some(completion);
            self.branch_probe = true;
            self.branch_rendered = false;
            cx.notify();
            return;
        }
        if self.branch_probe {
            self.branch_probe = false;
            if !self.branch_rendered {
                if self.branch_pending || !self.branches.is_empty() {
                    self.invalidate_branches();
                }
                self.next_branch_refresh = self.branch_clock.now() + Duration::from_secs(2);
                return;
            }
            if let Some(completion) = self.branch_completion.take() {
                self.branch_pending = false;
                self.branch_started = None;
                if completion.generation == self.branch_generation {
                    for row in completion.rows {
                        if self.projects.iter().any(|project| {
                            project.id == row.target.project_id
                                && project.path == row.target.registered_path
                        }) {
                            self.branches.insert(
                                row.target.project_id,
                                (row.target.registered_path, row.state),
                            );
                        }
                    }
                    cx.notify();
                }
            } else {
                self.start_branch_refresh();
            }
        } else if !self.branch_pending && self.branch_clock.now() >= self.next_branch_refresh {
            // Probe actual mounted visibility; render only records a flag, never IO.
            self.branch_rendered = false;
            self.branch_probe = true;
            cx.notify();
        }
    }

    fn start_branch_refresh(&mut self) {
        self.next_branch_refresh = self.branch_clock.now() + Duration::from_secs(2);
        let Some(generation) = self.branch_generation.checked_add(1) else {
            self.branches.clear();
            self.branch_service = None;
            return;
        };
        self.branch_generation = generation;
        if self.projects.is_empty() {
            return;
        }
        let targets = self
            .projects
            .iter()
            .cycle()
            .skip(self.branch_cursor)
            .take(self.projects.len().min(128))
            .map(|project| ProjectBranchTarget {
                project_id: project.id.clone(),
                registered_path: project.path.clone(),
            })
            .collect();
        self.branch_cursor =
            (self.branch_cursor + self.projects.len().min(128)) % self.projects.len();
        if let Some(service) = &self.branch_service {
            service.request(generation, targets);
            self.branch_pending = true;
            self.branch_started = Some(self.branch_clock.now());
        }
    }

    pub(super) fn select_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if !self.projects.iter().any(|project| project.id == project_id) {
            return;
        }
        self.invalidate_background_reload();
        self.selection_generation = self.selection_generation.wrapping_add(1);
        self.error = None;
        cx.set_global(SelectedProject(Some(project_id.to_string())));
        cx.emit(ProjectsBlockEvent::Selected(project_id.to_string()));
        let database = with_store(cx, |store| {
            store.database_path().map(Path::to_path_buf).ok_or_else(|| {
                crate::localized_copy::LocalizedCopy::new(
                    "ui.project_storage_requires_a_file_backed_database",
                )
            })
        });
        match database {
            Ok(database) => {
                self.selection_requested = Some((project_id.to_string(), database));
                self.start_selection_write(cx);
            }
            Err(error) => {
                self.selection_requested = None;
                self.error = Some(error);
            }
        }
        cx.notify();
    }

    fn start_selection_write(&mut self, cx: &mut Context<Self>) {
        if self.selection_pending {
            return;
        }
        let Some((project_id, database)) = self.selection_requested.take() else {
            return;
        };
        let current_database =
            with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                .ok()
                .flatten();
        if current_database.as_ref() != Some(&database)
            || !self.projects.iter().any(|project| project.id == project_id)
        {
            return;
        }
        self.selection_pending = true;
        let generation = self.selection_generation;
        let worker_project = project_id.clone();
        let worker_database = database.clone();
        let worker = cx.background_executor().spawn(async move {
            vega_conversation::sidebar_organization::record_project_visit(
                &worker_database,
                &worker_project,
            )
            .map_err(|error| {
                crate::localized_copy::LocalizedCopy::with_args(
                    "ui.could_not_update_project_status_error",
                    vec![("error", error.to_string().into())],
                )
            })
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.selection_pending = false;
                let current_database =
                    with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                        .ok()
                        .flatten();
                if this.selection_generation == generation
                    && current_database.as_ref() == Some(&database)
                    && cx.global::<SelectedProject>().0.as_deref() == Some(project_id.as_str())
                    && this.projects.iter().any(|project| project.id == project_id)
                {
                    match result {
                        Ok(()) => {
                            this.reload_background(cx);
                            cx.emit(ProjectsBlockEvent::VisitRecorded);
                        }
                        Err(error) => this.error = Some(error),
                    }
                    cx.notify();
                }
                this.start_selection_write(cx);
            });
        })
        .detach();
    }

    fn current_route(&self, cx: &App) -> (Option<String>, Option<String>, bool) {
        (
            cx.global::<SelectedProject>().0.clone(),
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.clone()),
            cx.try_global::<SettingsOpen>()
                .is_some_and(|settings| settings.0),
        )
    }

    pub(super) fn remove_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.invalidate_background_reload();
        if task_mutation_busy(cx) {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.a_task_is_being_saved_try_removing_the",
            ));
            cx.notify();
            return;
        }
        if project_worker_is_active(project_id, cx) {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.project_tasks_are_still_running_stop_them_and",
            ));
            cx.notify();
            return;
        }
        if !crate::navigation::allow_task_navigation(None, cx) {
            return;
        }
        let database = with_store(cx, |store| {
            store.database_path().map(Path::to_path_buf).ok_or_else(|| {
                crate::localized_copy::LocalizedCopy::new(
                    "ui.project_storage_requires_a_file_backed_database",
                )
            })
        });
        let Ok(database) = database else {
            self.error = database.err();
            cx.notify();
            return;
        };
        let Some(removal) = begin_project_removal(project_id, cx) else {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.project_tasks_are_still_running_stop_them_and",
            ));
            cx.notify();
            return;
        };
        let id = project_id.to_string();
        let worker_id = id.clone();
        let worker_database = database.clone();
        let route_generation = self.route_generation;
        let route = self.current_route(cx);
        crate::navigation::begin_task_mutation(cx);
        let worker = cx.background_executor().spawn(async move {
            let result = vega_conversation::sidebar_organization::remove_project(
                &worker_database,
                &worker_id,
            )
            .map_err(crate::localized_copy::LocalizedCopy::from);
            if result.is_ok() {
                removal.commit();
            }
            result
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            cx.update(crate::navigation::finish_task_mutation);
            let _ = this.update(cx, |this, cx| {
                let current_database =
                    with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                        .ok()
                        .flatten();
                if current_database.as_ref() != Some(&database) {
                    return;
                }
                match result {
                    Ok(removed) => {
                        this.projects.retain(|project| project.id != id);
                        this.invalidate_branches();
                        if this.route_generation == route_generation
                            && this.current_route(cx) == route
                        {
                            this.error = None;
                        }
                        if removed {
                            cx.emit(ProjectsBlockEvent::Removed(id));
                        }
                        this.reload_background(cx);
                    }
                    Err(error)
                        if this.route_generation == route_generation
                            && this.current_route(cx) == route =>
                    {
                        this.error = Some(error)
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn open_picker(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(vega_i18n::t("ui.choose_a_folder_to_register_as_a_project").into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = match receiver.await {
                Ok(Ok(Some(paths))) => paths,
                // 用户取消（None）或通道关闭（应用退出）都静默结束。
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    let message = crate::localized_copy::LocalizedCopy::with_args(
                        "ui.folder_selection_failed_error",
                        vec![("error", (error).to_string().into())],
                    );
                    this.update(cx, |this, cx| {
                        this.invalidate_background_reload();
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

    /// Register and reveal through the durable service on the background executor.
    pub(super) fn register_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.invalidate_background_reload();
        if self.registration_pending
            || cx
                .try_global::<crate::navigation::TaskMutationState>()
                .is_some_and(|state| state.pending > 0)
        {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.the_project_is_being_saved_try_reopening_the",
            ));
            cx.notify();
            return;
        }
        if !crate::navigation::allow_task_navigation(None, cx) {
            return;
        }
        let database = with_store(cx, |store| {
            store.database_path().map(Path::to_path_buf).ok_or_else(|| {
                crate::localized_copy::LocalizedCopy::new(
                    "ui.project_storage_requires_a_file_backed_database",
                )
            })
        });
        let Ok(database) = database else {
            self.error = database.err();
            cx.notify();
            return;
        };
        let origin = (
            cx.global::<SelectedProject>().0.clone(),
            cx.try_global::<OpenedThread>()
                .and_then(|v| v.0.as_ref().map(|t| t.id.clone())),
            cx.try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|v| v.0),
        );
        self.registration_pending = true;
        crate::navigation::begin_task_mutation(cx);
        let epoch = cx.global::<crate::navigation::TaskMutationState>().epoch;
        let owner_database = database.clone();
        let sort = self.sort;
        let path = path.to_path_buf();
        let worker = cx.background_executor().spawn(async move {
            let branch = git_detect::detect_git(&path);
            let store =
                Store::open(database).map_err(crate::localized_copy::LocalizedCopy::storage)?;
            let (id, _) = vega_conversation::sidebar_organization::register_and_reveal_project(
                &store,
                &path,
                branch.as_deref(),
            )
            .map_err(crate::localized_copy::LocalizedCopy::from)?;
            let rows = projects::list(store.conn(), sort)
                .map_err(crate::localized_copy::LocalizedCopy::storage)?;
            Ok::<_, crate::localized_copy::LocalizedCopy>((id, rows))
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let owner_matches = cx.update(|cx| {
                let database_matches =
                    with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                        .ok()
                        .flatten()
                        .as_ref()
                        == Some(&owner_database);
                let matches = database_matches
                    && cx.global::<crate::navigation::TaskMutationState>().epoch == epoch;
                if database_matches {
                    crate::navigation::finish_task_mutation(cx);
                }
                matches
            });
            this.update(cx, |this, cx| {
                this.registration_pending = false;
                this.invalidate_background_reload();
                if !owner_matches {
                    cx.notify();
                    return;
                }
                let current = (
                    cx.global::<SelectedProject>().0.clone(),
                    cx.try_global::<OpenedThread>()
                        .and_then(|v| v.0.as_ref().map(|t| t.id.clone())),
                    cx.try_global::<crate::settings::SettingsOpen>()
                        .is_some_and(|v| v.0),
                );
                match result {
                    Ok((id, rows)) => {
                        this.projects = rows;
                        this.invalidate_branches();
                        this.error = None;
                        if owner_matches && origin == current {
                            cx.set_global(SelectedProject(Some(id.clone())));
                            cx.set_global(crate::settings::SettingsOpen(false));
                            show_persisted(cx);
                            cx.emit(ProjectsBlockEvent::Registered(id));
                        }
                    }
                    Err(error) => {
                        this.error = Some(crate::localized_copy::LocalizedCopy::with_args(
                            "ui.could_not_open_the_folder_reopen_it_to",
                            vec![("error", error)],
                        ))
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Block header: collapsible title (chevron shows the state) + [+].
    fn render_header(&self, cx: &mut Context<Self>, _colors: &ThemeColors) -> AnyElement {
        let collapsed = cx.global::<ProjectsCollapsed>().0;
        let weak = cx.weak_entity();
        nsh_leaf::host(
            "sidebar-projects-header",
            None,
            vec![
                nsh_leaf::Item::action(
                    vega_i18n::t("sidebar.projects"),
                    Some(if collapsed {
                        crate::icons::Icon::ChevronRight
                    } else {
                        crate::icons::Icon::ChevronDown
                    }),
                ),
                nsh_leaf::Item::action(
                    vega_i18n::t("ui.add_project_folder"),
                    Some(crate::icons::Icon::FolderPlus),
                ),
            ],
            true,
            Typography::SIDEBAR_LINE_HEIGHT,
            move |cx| cx.global::<ProjectsCollapsed>().0 == collapsed,
            move |index, _, _, cx| {
                if index == 0 {
                    toggle_projects_block(cx);
                } else {
                    let _ = weak.update(cx, |this, cx| this.open_picker(cx));
                }
            },
        )
    }

    fn render_body(&self, cx: &mut Context<Self>, colors: &ThemeColors) -> AnyElement {
        let selected = cx.global::<SelectedProject>().0.clone();
        div()
            .flex()
            .flex_col()
            .child(self.render_sort_row(cx, colors))
            .children(self.projects.is_empty().then(|| {
                nsh_leaf::host(
                    "sidebar-projects-empty",
                    Some(vega_i18n::t("ui.click_to_add_a_folder").to_owned()),
                    Vec::new(),
                    false,
                    Typography::SIDEBAR_LINE_HEIGHT,
                    |_| true,
                    |_, _, _, _| {},
                )
            }))
            .children(self.projects.iter().map(|project| {
                let project_id = project.id.clone();
                let observed = project.clone();
                let generation = self.reload_generation;
                let weak = cx.weak_entity();
                let target = weak.clone();
                let mut label =
                    nsh_leaf::Item::action(project.name.clone(), Some(crate::icons::Icon::Folder));
                label.selected = selected.as_deref() == Some(project.id.as_str());
                label.hint = self.branch_suffix(project).map(str::to_owned);
                let mut remove = nsh_leaf::Item::action(
                    vega_i18n::t("ui.remove_project"),
                    Some(crate::icons::Icon::Close),
                );
                remove.compact = true;
                remove.danger = true;
                nsh_leaf::host(
                    format!("sidebar-project-{}", project.id),
                    None,
                    vec![label, remove],
                    true,
                    Typography::SIDEBAR_LINE_HEIGHT,
                    move |cx| {
                        weak.upgrade().is_some_and(|entity| {
                            let this = entity.read(cx);
                            this.reload_generation == generation
                                && this.projects.contains(&observed)
                                && !this.registration_pending
                                && !this.selection_pending
                        })
                    },
                    move |index, _, _, cx| {
                        let _ = target.update(cx, |this, cx| {
                            if index == 0 {
                                this.select_project(&project_id, cx);
                            } else {
                                this.remove_project(&project_id, cx);
                            }
                        });
                    },
                )
            }))
            .into_any_element()
    }

    fn render_sort_row(&self, cx: &mut Context<Self>, _colors: &ThemeColors) -> AnyElement {
        let observed = self.sort;
        let weak = cx.weak_entity();
        let target = weak.clone();
        let items = [
            (ProjectSort::Name, vega_i18n::t("ui.name")),
            (
                ProjectSort::RecentlyOpened,
                vega_i18n::t("ui.recently_opened"),
            ),
        ]
        .into_iter()
        .map(|(sort, label)| {
            let mut item = nsh_leaf::Item::action(label, None);
            item.selected = observed == sort;
            item
        })
        .collect();
        nsh_leaf::host(
            "sidebar-project-sort",
            None,
            items,
            true,
            Typography::SIDEBAR_LINE_HEIGHT,
            move |cx| {
                weak.upgrade()
                    .is_some_and(|entity| entity.read(cx).sort == observed)
            },
            move |index, _, _, cx| {
                let _ = target.update(cx, |this, cx| {
                    this.sort = if index == 0 {
                        ProjectSort::Name
                    } else {
                        ProjectSort::RecentlyOpened
                    };
                    this.reload(cx);
                });
            },
        )
    }
}

impl Render for ProjectsBlock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let collapsed = cx.global::<ProjectsCollapsed>().0;
        if self.branch_probe && !collapsed {
            self.branch_rendered = true;
        }
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
