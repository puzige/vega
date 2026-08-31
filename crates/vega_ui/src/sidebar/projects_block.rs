use super::*;

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
    pub(crate) projects: Vec<Project>,
    /// Sort order — in-memory only (T12 ruling: 不持久化).
    pub(crate) sort: ProjectSort,
    /// Inline error message (ui-spec §4.6); empty until a failure occurs.
    pub(crate) error: Option<String>,
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
