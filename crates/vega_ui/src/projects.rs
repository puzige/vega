//! Projects view (A1-03, temporary mount): add-project via the platform
//! folder picker, the registered-project list with name/branch display,
//! removal, and a name / recently-opened sort toggle.
//!
//! T10 placement note: this view lives **outside** the T09 sidebar for now —
//! it is reached from a temporary "项目管理（临时）" entry under the empty
//! state and replaces the content column while open. T12 moves the project
//! list into the sidebar; only this temporary page is retired then.
//!
//! Store access follows the settings-view pattern: the app binary installs
//! [`ProjectsStore`] (a [`Global`] carrying the opened `Store`) at startup
//! via [`init`], and this view reads it through the global. Project data
//! lives entirely in SQLite (`$HOME/.vega/vega.db`), so registration survives
//! a restart. Errors render as an inline danger bar (ui-spec §4.6: no error
//! modals).

use std::path::{Path, PathBuf};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Global, MouseButton, MouseUpEvent, PathPromptOptions, Window,
    actions, div, px, relative,
};
use vega_store::Store;
use vega_store::git_detect;
use vega_store::projects::{self, Project, ProjectSort, ProjectsError};
use vega_theme::{Typography, theme};

actions!(vega_projects, [OpenProjects, CloseProjects]);

/// Whether the projects view currently replaces the content column.
///
/// Toggled by the app-level [`OpenProjects`]/[`CloseProjects`] handlers.
pub struct ProjectsOpen(pub bool);

impl Global for ProjectsOpen {}

/// The app store handle, installed once at startup by [`init`].
///
/// An init failure is carried as the `Err` half instead of aborting startup;
/// the view renders it as an inline danger bar.
pub struct ProjectsStore(pub Result<Store, String>);

impl Global for ProjectsStore {}

/// The projects view. Holds a cache of the project list plus UI state, so it
/// must be cached by the parent across re-renders (like the settings view).
pub struct ProjectsView {
    /// Cached rows, refreshed from the store after every mutation.
    projects: Vec<Project>,
    /// Current sort order (最小实现：名称 / 最近打开；拖拽排序后置)。
    sort: ProjectSort,
    /// Inline error message (ui-spec §4.6); empty until a failure occurs.
    error: Option<String>,
}

/// Opens the app store at `$HOME/.vega/vega.db` (creating the directory and
/// applying pending migrations) and installs it as the [`ProjectsStore`]
/// global. Call once at app startup.
pub fn init(cx: &mut App) {
    let store = open_default_store();
    cx.set_global(ProjectsStore(store));
}

/// Opens and migrates `$HOME/.vega/vega.db`.
///
/// The path mirrors the config convention (`$HOME/.vega/`); failures come
/// back as ready-to-render messages.
fn open_default_store() -> Result<Store, String> {
    let home =
        std::env::var("HOME").map_err(|_| "未能确定用户主目录（HOME 未设置）".to_string())?;
    let dir = PathBuf::from(home).join(".vega");
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("创建 {} 失败：{error}", dir.display()))?;
    let store = Store::open(dir.join("vega.db"))
        .map_err(|error| format!("打开 {} 失败：{error}", dir.join("vega.db").display()))?;
    store
        .migrate()
        .map_err(|error| format!("数据库迁移失败：{error}"))?;
    Ok(store)
}

impl ProjectsView {
    /// Creates the view with an empty-name sort and loads the project list.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            projects: Vec::new(),
            sort: ProjectSort::Name,
            error: None,
        };
        view.reload(cx);
        view
    }

    fn on_back(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        // 与设置页返回按钮同构：派发动作，由 app 级处理器统一收口。
        window.dispatch_action(Box::new(CloseProjects), cx);
    }

    /// Opens the platform folder picker and registers the picked folder.
    ///
    /// The picker answers asynchronously (oneshot); the future runs on the
    /// foreground executor, so every store access stays on the main thread.
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
                Err(ProjectsError::PathAlreadyRegistered(registered)) => {
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

    /// Removes the project row (database only; files on disk are never
    /// touched — S2 ruling: no confirmation layer).
    fn remove_project(&mut self, id: &str, cx: &mut Context<Self>) {
        let result = with_store(cx, |store| {
            projects::remove(store.conn(), id).map_err(|error| format!("项目移除失败：{error}"))
        });
        match result {
            Ok(_) => self.error = None,
            Err(message) => self.error = Some(message),
        }
        self.reload(cx);
    }

    /// "Opens" the project: refreshes `last_opened_at`（本临时视图里"选中即
    /// 打开"；真正的打开流程由 T11/T12 接管）。
    fn open_project(&mut self, id: &str, cx: &mut Context<Self>) {
        let result = with_store(cx, |store| {
            projects::touch_last_opened(store.conn(), id)
                .map_err(|error| format!("项目状态更新失败：{error}"))
        });
        match result {
            Ok(_) => self.error = None,
            Err(message) => self.error = Some(message),
        }
        self.reload(cx);
    }

    /// Re-reads the project list in the current sort order.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let result = with_store(cx, |store| {
            projects::list(store.conn(), self.sort)
                .map_err(|error| format!("项目列表加载失败：{error}"))
        });
        match result {
            Ok(projects) => self.projects = projects,
            Err(message) => self.error = Some(message),
        }
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
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
                    .child("项目"),
            )
            .into_any_element()
    }

    /// The registered-project list plus the sort toggle (names / recently
    /// opened) and a hint when nothing is registered yet.
    fn render_project_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_title("已注册项目", colors.text_primary))
                    .child(
                        div().flex().items_center().gap_1().children(
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
                        ),
                    ),
            )
            .children(self.projects.is_empty().then(|| {
                div()
                    .text_color(colors.text_tertiary)
                    .text_size(px(Typography::BODY))
                    .child("暂无项目，点击下方「添加项目」选择文件夹注册")
                    .into_any_element()
            }))
            .children(self.projects.iter().map(|project| {
                let open_id = project.id.clone();
                let remove_id = project.id.clone();
                let branch_label = project
                    .git_default_branch
                    .clone()
                    .unwrap_or_else(|| "非 git 目录".to_string());
                let is_git = project.git_default_branch.is_some();
                let path = project.path.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    // 可点击主体（选中即打开）与移除按钮是兄弟节点，避免
                    // 嵌套命中导致一次点击同时触发两个操作。
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .cursor_pointer()
                            .hover(move |s| s.bg(colors.bg_hover))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                    this.open_project(&open_id, cx);
                                }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(Typography::HEADING_CARD))
                                            .font_weight(Typography::HEADING_CARD_WEIGHT)
                                            .child(project.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .px_1()
                                            .py_1()
                                            .rounded_md()
                                            .text_size(px(Typography::SIDEBAR))
                                            .when(is_git, move |chip| {
                                                chip.bg(colors.bg_hover)
                                                    .text_color(colors.text_secondary)
                                            })
                                            .when(!is_git, move |chip| {
                                                chip.text_color(colors.text_tertiary)
                                            })
                                            .child(branch_label),
                                    ),
                            )
                            .child(
                                div()
                                    .text_color(colors.text_secondary)
                                    .text_size(px(Typography::BODY))
                                    .truncate()
                                    .child(path),
                            ),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(Typography::SIDEBAR))
                            .text_color(colors.text_secondary)
                            .cursor_pointer()
                            .hover(move |s| s.text_color(colors.danger))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                    this.remove_project(&remove_id, cx);
                                }),
                            )
                            .child("移除"),
                    )
            }))
            .into_any_element()
    }

    fn render_add_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("添加项目", colors.text_primary))
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .self_start()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::SIDEBAR))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_add_clicked))
                    .child("选择文件夹…"),
            )
            .into_any_element()
    }
}

impl Render for ProjectsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .id("projects-page")
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .line_height(relative(Typography::BODY_LINE_HEIGHT))
            .overflow_y_scroll()
            .child(
                // Content column per UI spec §1: max 820px, centered, ≥24px
                // side padding.
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w_full()
                    .max_w(px(820.))
                    .mx_auto()
                    .px(px(24.))
                    .py(px(24.))
                    .child(self.render_header(cx))
                    .children(self.error.clone().map(|message| {
                        div()
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
                    .child(self.render_project_list(cx))
                    .child(self.render_add_form(cx)),
            )
    }
}

fn section_title(label: &'static str, color: gpui::Rgba) -> gpui::Div {
    div()
        .text_size(px(Typography::HEADING_BLOCK))
        .font_weight(Typography::HEADING_BLOCK_WEIGHT)
        .text_color(color)
        .child(label)
}

/// Runs a projects-table operation against the global store, mapping any
/// store-level failure into a ready-to-render message.
fn with_store<R>(
    cx: &App,
    operation: impl FnOnce(&Store) -> Result<R, String>,
) -> Result<R, String> {
    match cx.try_global::<ProjectsStore>() {
        Some(ProjectsStore(Ok(store))) => operation(store),
        Some(ProjectsStore(Err(error))) => Err(format!("项目存储不可用：{error}")),
        None => Err("项目存储不可用：应用启动时未完成初始化".to_string()),
    }
}
