use super::*;

#[derive(Clone)]
pub(super) enum Control {
    View(SidebarView),
    CollapseAll,
    Menu(OrganizationMenu),
    Archive,
    NewGroup,
    AddProject,
    Collapse(SidebarCollapseTarget),
    More(String),
    Project(String),
    NewStandalone,
    NewProjectTask(String),
    ToggleProject(String),
    ToggleSort,
}
impl ThreadsBlock {
    pub(super) fn control_action(
        &mut self,
        action: Control,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Control::View(view) => {
                self.close_actions();
                if let Some(org) = self.organization.as_mut() {
                    org.menu = None;
                    if let Some(snapshot) = &org.snapshot {
                        let mut preferences = snapshot.preferences.clone();
                        preferences.view = view;
                        self.submit_organization(
                            SidebarOrganizationAction::SetPreferences(preferences),
                            cx,
                        );
                    }
                }
            }
            Control::CollapseAll => {
                self.submit_organization(SidebarOrganizationAction::CollapseAll, cx)
            }
            Control::Menu(menu) => {
                self.close_actions();
                if let Some(org) = self.organization.as_mut() {
                    org.menu = Some(menu);
                    org.menu_index = 0;
                    window.focus(&org.menu_focus, cx);
                }
            }
            Control::Archive => {
                if let Some(org) = self.organization.as_mut() {
                    org.archive = !org.archive;
                }
            }
            Control::NewGroup => self.edit_group(None, window, cx),
            Control::AddProject => {
                if let Some(org) = &self.organization {
                    org.projects.update(cx, ProjectsBlock::open_picker);
                }
            }
            Control::Collapse(target) => self.toggle_organization_collapse(target, cx),
            Control::More(id) => {
                if let Some(org) = self.organization.as_mut() {
                    *org.more.entry(id).or_insert(5) += 5;
                }
            }
            Control::Project(id) => {
                if crate::navigation::allow_task_navigation(None, cx)
                    && let Some(org) = &self.organization
                {
                    org.projects
                        .update(cx, |projects, cx| projects.select_project(&id, cx));
                }
            }
            Control::NewStandalone => self.create_task(None, cx),
            Control::NewProjectTask(id) => self.create_task(Some(id), cx),
            Control::ToggleProject(id) => {
                if crate::navigation::allow_task_navigation(None, cx) {
                    if let Some(org) = &self.organization {
                        org.projects
                            .update(cx, |projects, cx| projects.select_project(&id, cx));
                    }
                    self.toggle_organization_collapse(SidebarCollapseTarget::Project(id), cx);
                }
            }
            Control::ToggleSort => {
                if let Some(snapshot) = self
                    .organization
                    .as_ref()
                    .and_then(|organization| organization.snapshot.as_ref())
                {
                    let mut preferences = snapshot.preferences.clone();
                    preferences.sort = match preferences.sort {
                        SidebarTaskSort::Updated => SidebarTaskSort::Created,
                        SidebarTaskSort::Created => SidebarTaskSort::Updated,
                    };
                    self.submit_organization(
                        SidebarOrganizationAction::SetPreferences(preferences),
                        cx,
                    );
                }
            }
        }
        cx.notify();
    }
    pub(super) fn organization_control(
        &self,
        id: impl Into<String>,
        label: impl Into<String>,
        action: Control,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let id = id.into();
        let label = label.into();
        let aria = match &action {
            Control::CollapseAll => "收起全部".to_string(),
            Control::Menu(OrganizationMenu::Filter) => "排序与归档".to_string(),
            Control::Menu(_) => "组织操作".to_string(),
            _ => label.clone(),
        };
        let keyboard_action = action.clone();
        div()
            .id(ElementId::Name(id.clone().into()))
            .debug_selector(move || id.clone())
            .focusable()
            .tab_stop(true)
            .aria_label(aria)
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .px_2()
            .rounded_md()
            .flex()
            .items_center()
            .text_size(px(Typography::SIDEBAR))
            .text_color(if selected {
                colors.text_primary
            } else {
                colors.text_secondary
            })
            .when(selected, |s| {
                s.bg(colors.bg_active).text_color(colors.brand_primary)
            })
            .cursor_pointer()
            .hover(move |s| s.bg(colors.bg_hover))
            .focus_visible(move |s| {
                s.bg(colors.bg_hover)
                    .border_1()
                    .border_color(colors.border_subtle)
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    if cx.has_active_drag() {
                        return;
                    }
                    cx.stop_propagation();
                    this.control_action(action.clone(), window, cx);
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                        cx.stop_propagation();
                        this.control_action(keyboard_action.clone(), window, cx);
                    }
                }),
            )
            .child(label)
            .into_any_element()
    }
    pub(in crate::sidebar::threads_block) fn render_organization(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let Some(org) = &self.organization else {
            return div().into_any_element();
        };
        let Some(snapshot) = org.snapshot.clone() else {
            return div()
                .id("sidebar-organization")
                .flex()
                .flex_col()
                .gap_1()
                .child(self.section_label("正在载入", cx))
                .into_any_element();
        };
        let show_archived = org.archive;
        let menu_open = org.menu.is_some();
        let mut body = div()
            .id("sidebar-organization")
            .flex()
            .flex_col()
            .gap_2()
            .flex_1()
            .min_h_0();
        body = body.child(self.render_sessions_pi(&snapshot, show_archived, cx));
        body = body.child(self.render_projects_pi(&snapshot, show_archived, cx));
        body = body.children(menu_open.then(|| self.render_organization_menu(cx)));
        body = body.children(
            org.projects
                .read(cx)
                .error
                .clone()
                .map(|message| error_bar(message, &colors)),
        );
        body = body.children(
            self.error
                .clone()
                .map(|message| error_bar(message, &colors)),
        );
        body.into_any_element()
    }

    pub(super) fn vector_control(
        &self,
        id: impl Into<String>,
        label: impl Into<String>,
        kind: crate::icons::Icon,
        action: Control,
        hidden_until_hover: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let id = id.into();
        let label = label.into();
        let keyboard_action = action.clone();
        div()
            .id(ElementId::Name(id.clone().into()))
            .debug_selector(move || id.clone())
            .aria_label(label.clone())
            .focusable()
            .tab_stop(true)
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .when(hidden_until_hover, |style| style.opacity(0.))
            .hover(move |style| style.opacity(1.).bg(colors.bg_hover))
            .focus_visible(move |style| {
                style
                    .opacity(1.)
                    .bg(colors.bg_active)
                    .border_1()
                    .border_color(colors.border_subtle)
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.control_action(action.clone(), window, cx);
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        this.control_action(keyboard_action.clone(), window, cx);
                    }
                }),
            )
            .child(crate::icons::icon(kind, colors.text_secondary))
            .into_any_element()
    }

    fn render_sessions_pi(
        &self,
        snapshot: &SidebarOrganizationSnapshot,
        show_archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let mut standalone: Vec<_> = self
            .threads
            .iter()
            .filter(|thread| thread.is_standalone())
            .cloned()
            .collect();
        if show_archived {
            standalone.extend(
                self.archived
                    .iter()
                    .filter(|thread| thread.is_standalone())
                    .cloned(),
            );
        }
        standalone = super::projections::sorted_threads(&standalone, snapshot.preferences.sort);
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(28.))
            .child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child("SESSIONS"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(self.vector_control(
                        "organization-session-sort",
                        "任务排序与归档",
                        crate::icons::Icon::ArrowUpDown,
                        Control::Menu(OrganizationMenu::Filter),
                        false,
                        cx,
                    ))
                    .child(self.vector_control(
                        "organization-new-session",
                        "新建独立任务",
                        crate::icons::Icon::Plus,
                        Control::NewStandalone,
                        false,
                        cx,
                    )),
            );
        let opened_id = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone());
        let empty = standalone.is_empty();
        let rows = standalone.into_iter().map(|thread| {
            let archived = thread.status == ThreadStatus::Archived;
            self.render_pi_row(
                &thread,
                &opened_id,
                archived,
                "standalone-thread-row-",
                true,
                cx,
            )
        });
        let empty = empty.then(|| {
            div()
                .id("organization-sessions-empty")
                .debug_selector(|| "organization-sessions-empty".into())
                .h(px(30.))
                .px_3()
                .flex()
                .items_center()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_tertiary)
                .child("暂无独立任务")
        });
        div()
            .id("organization-sessions")
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(header)
            .child(
                div()
                    .id("organization-sessions-scroll")
                    .max_h(px(30.0 * 5.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(empty)
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_projects_pi(
        &self,
        snapshot: &SidebarOrganizationSnapshot,
        show_archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(28.))
            .child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child("PROJECTS"),
            )
            .child(self.vector_control(
                "organization-add-project",
                "添加项目文件夹",
                crate::icons::Icon::FolderPlus,
                Control::AddProject,
                false,
                cx,
            ));
        let mut ordered: Vec<_> = snapshot
            .project_order
            .iter()
            .filter_map(|id| snapshot.projects.iter().find(|project| &project.id == id))
            .collect();
        ordered.extend(
            snapshot
                .projects
                .iter()
                .filter(|project| !snapshot.project_order.contains(&project.id)),
        );
        let mut rows = div()
            .id("organization-projects-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        for project in ordered {
            rows = rows.child(self.render_pi_project(project, snapshot, show_archived, cx));
        }
        div()
            .id("organization-projects")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(header)
            .child(rows)
            .into_any_element()
    }

    fn render_pi_project(
        &self,
        project: &SidebarProject,
        snapshot: &SidebarOrganizationSnapshot,
        show_archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let target = SidebarCollapseTarget::Project(project.id.clone());
        let collapsed = snapshot.collapsed.contains(&target);
        let project_id = project.id.clone();
        let project_for_toggle = project.id.clone();
        let project_for_add = project.id.clone();
        let project_for_menu = project.id.clone();
        let selected = cx.global::<SelectedProject>().0.as_deref() == Some(project.id.as_str());
        let actions_visible =
            selected || self.hovered_project.as_deref() == Some(project.id.as_str());
        let project_for_hover = project.id.clone();
        let row = div()
            .id(ElementId::Name(
                format!("project-header-{}", project.id).into(),
            ))
            .debug_selector({
                let id = project.id.clone();
                move || format!("project-header-{id}")
            })
            .h(px(32.))
            .flex()
            .items_center()
            .gap_1()
            .rounded_md()
            .focusable()
            .tab_stop(true)
            .cursor_pointer()
            .when(selected, |row| row.bg(colors.bg_active))
            .hover(move |style| {
                if selected {
                    style.bg(colors.brand_soft)
                } else {
                    style.bg(colors.bg_hover)
                }
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.set_hovered_project(&project_for_hover, *hovered, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.control_action(
                        Control::ToggleProject(project_for_toggle.clone()),
                        window,
                        cx,
                    );
                }),
            )
            .child(crate::icons::icon(
                if collapsed {
                    crate::icons::Icon::ChevronRight
                } else {
                    crate::icons::Icon::ChevronDown
                },
                colors.text_tertiary,
            ))
            .child(crate::icons::icon(
                crate::icons::Icon::Folder,
                if selected {
                    colors.brand_primary
                } else {
                    colors.text_secondary
                },
            ))
            .child(
                div()
                    .id(ElementId::Name(
                        format!("organization-project-{}", project.id).into(),
                    ))
                    .debug_selector({
                        let id = project.id.clone();
                        move || format!("organization-project-{id}")
                    })
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if selected {
                        colors.brand_primary
                    } else {
                        colors.text_primary
                    })
                    .child(project.name.clone()),
            )
            .child(self.vector_control(
                format!("project-add-{}", project.id),
                format!("在 {} 中新建任务", project.name),
                crate::icons::Icon::Plus,
                Control::NewProjectTask(project_for_add),
                !actions_visible,
                cx,
            ))
            .child(self.vector_control(
                format!("project-more-{}", project.id),
                format!("{} 项目操作", project.name),
                crate::icons::Icon::More,
                Control::Menu(OrganizationMenu::Project(project_for_menu)),
                !actions_visible,
                cx,
            ));
        let mut section = div()
            .id(ElementId::Name(
                format!("project-section-{}", project_id).into(),
            ))
            .flex()
            .flex_col()
            .child(row);
        if !collapsed {
            let mut tasks: Vec<_> = self
                .threads
                .iter()
                .filter(|thread| thread.project_id == project.id)
                .cloned()
                .collect();
            if show_archived {
                tasks.extend(
                    self.archived
                        .iter()
                        .filter(|thread| thread.project_id == project.id)
                        .cloned(),
                );
            }
            tasks = super::projections::sorted_threads(&tasks, snapshot.preferences.sort);
            let opened_id = cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.clone());
            if tasks.is_empty() {
                section = section.child(self.section_label("暂无任务", cx));
            } else {
                section = section.children(tasks.iter().map(|thread| {
                    self.render_pi_row(
                        thread,
                        &opened_id,
                        thread.status == ThreadStatus::Archived,
                        "project-thread-row-",
                        true,
                        cx,
                    )
                }));
            }
        }
        section.into_any_element()
    }
}
