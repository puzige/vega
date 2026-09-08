use super::*;
use std::{
    collections::HashMap,
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
    pub(crate) projects: Vec<Project>,
    /// Sort order — in-memory only (T12 ruling: 不持久化).
    pub(crate) sort: ProjectSort,
    /// Inline error message (ui-spec §4.6); empty until a failure occurs.
    pub(crate) error: Option<String>,
    branches: HashMap<String, (String, ProjectBranchState)>,
    branch_service: Option<ProjectBranchService>,
    branch_generation: u64,
    branch_cursor: usize,
    next_branch_refresh: Instant,
    branch_probe: bool,
    branch_rendered: bool,
    branch_pending: bool,
    branch_started: Option<Instant>,
    branch_selection: Option<String>,
    branch_completion: Option<ProjectBranchCompletion>,
    pub(super) organization_mode: bool,
    registration_pending: bool,
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
            next_branch_refresh: Instant::now(),
            branch_probe: false,
            branch_rendered: false,
            branch_pending: false,
            branch_started: None,
            branch_selection: None,
            branch_completion: None,
            organization_mode: false,
            registration_pending: false,
        };
        view.reload(cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this.update(cx, |this, cx| this.poll_branches(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        view
    }

    /// Re-reads the project list in the current sort order.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.invalidate_branches();
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
        self.next_branch_refresh = Instant::now();
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
        if self
            .branch_started
            .is_some_and(|start| start.elapsed() >= Duration::from_secs(1))
        {
            // A stalled filesystem cannot keep a previously displayed branch indefinitely.
            self.invalidate_branches();
            self.next_branch_refresh = Instant::now() + Duration::from_secs(2);
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
                self.next_branch_refresh = Instant::now() + Duration::from_secs(2);
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
        } else if !self.branch_pending && Instant::now() >= self.next_branch_refresh {
            // Probe actual mounted visibility; render only records a flag, never IO.
            self.branch_rendered = false;
            self.branch_probe = true;
            cx.notify();
        }
    }

    fn start_branch_refresh(&mut self) {
        self.next_branch_refresh = Instant::now() + Duration::from_secs(2);
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
            self.branch_started = Some(Instant::now());
        }
    }

    /// Click = select: touch `last_opened_at`, cache the selection in the
    /// global, then let the orchestrator resync the session block.
    pub(super) fn select_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
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
    pub(super) fn remove_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
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
    pub(crate) fn open_picker(&mut self, cx: &mut Context<Self>) {
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

    fn on_add_clicked(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_picker(cx);
    }

    /// Register and reveal through the durable service on the background executor.
    pub(super) fn register_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.registration_pending
            || cx
                .try_global::<crate::navigation::TaskMutationState>()
                .is_some_and(|state| state.pending > 0)
        {
            self.error = Some("项目正在保存，请稍后重新打开文件夹".into());
            cx.notify();
            return;
        }
        if !crate::navigation::allow_task_navigation(None, cx) {
            return;
        }
        let database = with_store(cx, |store| {
            store
                .database_path()
                .map(Path::to_path_buf)
                .ok_or_else(|| "项目存储需要文件数据库".to_string())
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
            let store = Store::open(database).map_err(|e| e.to_string())?;
            let (id, _) = vega_conversation::sidebar_organization::register_and_reveal_project(
                &store,
                &path,
                branch.as_deref(),
            )
            .map_err(|e| e.to_string())?;
            let rows = projects::list(store.conn(), sort).map_err(|e| e.to_string())?;
            Ok::<_, String>((id, rows))
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
                        this.error = Some(format!("打开文件夹失败，请重新打开重试：{error}"))
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
                            .text_size(px(Typography::METADATA))
                            .font_weight(Typography::HEADING_CARD_WEIGHT)
                            .text_color(colors.text_secondary)
                            .child("PROJECTS"),
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
                    .child(crate::icons::icon(
                        crate::icons::Icon::FolderPlus,
                        colors.text_secondary,
                    )),
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
                let branch = self.branch_suffix(project).map(str::to_owned);
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
                            .child(crate::icons::icon(
                                crate::icons::Icon::Close,
                                colors.text_secondary,
                            )),
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

#[cfg(test)]
mod tests;
