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
            .rounded_lg()
            .flex()
            .items_center()
            .text_size(px(Typography::SIDEBAR))
            .text_color(if selected {
                colors.text_primary
            } else {
                colors.text_secondary
            })
            .when(selected, |s| {
                s.bg(colors.bg_active).text_color(colors.text_primary)
            })
            .cursor_pointer()
            .hover(move |s| {
                s.bg(if selected {
                    colors.bg_active
                } else {
                    colors.bg_hover
                })
            })
            .focus_visible(move |s| {
                s.bg(if selected {
                    colors.bg_active
                } else {
                    colors.bg_hover
                })
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
            .debug_selector(|| "sidebar-organization".into())
            .flex()
            .flex_col()
            .gap_2();
        body = body.children(self.render_pinned_pi(&snapshot, show_archived, cx));
        body = body.child(self.render_projects_pi(&snapshot, show_archived, cx));
        body = body.child(self.render_recents_pi(&snapshot, show_archived, cx));
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

    fn eligible_threads(&self, show_archived: bool) -> Vec<Thread> {
        let mut threads = self.threads.clone();
        if show_archived {
            threads.extend(self.archived.iter().cloned());
        }
        threads
    }

    fn render_pinned_pi(
        &self,
        snapshot: &SidebarOrganizationSnapshot,
        show_archived: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let colors = theme(cx).colors;
        let label = "Pinned";
        let mut pinned: Vec<_> = self
            .eligible_threads(show_archived)
            .into_iter()
            .filter(|thread| thread.pinned)
            .collect();
        pinned = super::projections::sorted_threads(&pinned, snapshot.preferences.sort);
        if pinned.is_empty() {
            return None;
        }
        let opened_id = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone());
        Some(
            div()
                .id("organization-pinned")
                .debug_selector(|| "organization-section-pinned".into())
                .flex()
                .flex_col()
                .flex_shrink_0()
                .mb_1()
                .child(
                    div()
                        .debug_selector(|| "organization-header-pinned".into())
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .child(
                            div()
                                .debug_selector(move || {
                                    format!("organization-section-label-{label}")
                                })
                                .child(label),
                        ),
                )
                .child(
                    div()
                        .id("organization-pinned-scroll")
                        .debug_selector(|| "organization-pinned-scroll".into())
                        // Include the row surface outset inside the scroll clip.
                        .ml(px(-8.0))
                        .pl_2()
                        .max_h(px(Typography::SIDEBAR_LINE_HEIGHT * 5.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .children(pinned.iter().map(|thread| {
                            let archived = thread.status == ThreadStatus::Archived;
                            self.render_pinned_row(
                                thread,
                                &opened_id,
                                archived,
                                "pinned-thread-row-",
                                cx,
                            )
                        })),
                )
                .into_any_element(),
        )
    }

    fn render_recents_pi(
        &self,
        snapshot: &SidebarOrganizationSnapshot,
        show_archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let label = "Recents";
        let mut standalone: Vec<_> = self
            .threads
            .iter()
            .filter(|thread| thread.is_standalone() && !thread.pinned)
            .cloned()
            .collect();
        if show_archived {
            standalone.extend(
                self.archived
                    .iter()
                    .filter(|thread| thread.is_standalone() && !thread.pinned)
                    .cloned(),
            );
        }
        standalone = super::projections::sorted_threads(&standalone, snapshot.preferences.sort);
        let header_actions_visible = self.hovered_section == Some(OrganizationSection::Recents)
            || self.focused_section == Some(OrganizationSection::Recents)
            || self.organization.as_ref().is_some_and(|organization| {
                matches!(organization.menu, Some(OrganizationMenu::Filter))
            });
        let header = div()
            .id("organization-recents-header")
            .debug_selector(|| "organization-header-recents".into())
            .track_focus(&self.recents_header_focus)
            .flex()
            .items_center()
            .justify_between()
            .h(px(28.))
            .child(
                div()
                    .debug_selector(move || format!("organization-section-label-{label}"))
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child(label),
            )
            .child(
                div()
                    .debug_selector(move || {
                        format!(
                            "organization-recents-actions-{}",
                            if header_actions_visible {
                                "visible"
                            } else {
                                "rest"
                            }
                        )
                    })
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(self.vector_control(
                        "organization-session-sort",
                        "任务排序与归档",
                        crate::icons::Icon::ArrowUpDown,
                        Control::Menu(OrganizationMenu::Filter),
                        !header_actions_visible,
                        cx,
                    ))
                    .child(self.vector_control(
                        "organization-new-session",
                        "新建独立任务",
                        crate::icons::Icon::Plus,
                        Control::NewStandalone,
                        !header_actions_visible,
                        cx,
                    )),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.set_hovered_section(OrganizationSection::Recents, *hovered, cx);
            }));
        let opened_id = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone());
        let empty = standalone.is_empty();
        let has_hidden = standalone.len() > 10;
        let visible_count = if self.recents_expanded {
            standalone.len()
        } else {
            10
        };
        let rows = standalone.into_iter().take(visible_count).map(|thread| {
            let archived = thread.status == ThreadStatus::Archived;
            self.render_recent_row(
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
                .id("organization-recents-empty")
                .debug_selector(|| "organization-recents-empty".into())
                .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                .px_3()
                .flex()
                .items_center()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_tertiary)
                .child("暂无最近任务")
        });
        div()
            .id("organization-recents")
            .debug_selector(|| "organization-section-recents".into())
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(header)
            .child(
                div()
                    .id("organization-recents-scroll")
                    .flex()
                    .flex_col()
                    .children(empty)
                    .children(rows)
                    .children(has_hidden.then(|| {
                        self.render_progressive_control(
                            OrganizationSection::Recents,
                            self.recents_expanded,
                            cx,
                        )
                    })),
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
        let label = "Projects";
        let header_actions_visible = self.hovered_section == Some(OrganizationSection::Projects)
            || self.focused_section == Some(OrganizationSection::Projects);
        let header = div()
            .id("organization-projects-header")
            .debug_selector(|| "organization-header-projects".into())
            .track_focus(&self.projects_header_focus)
            .flex()
            .items_center()
            .justify_between()
            .h(px(28.))
            .child(
                div()
                    .debug_selector(move || format!("organization-section-label-{label}"))
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child(label),
            )
            .child(
                div()
                    .debug_selector(move || {
                        format!(
                            "organization-projects-actions-{}",
                            if header_actions_visible {
                                "visible"
                            } else {
                                "rest"
                            }
                        )
                    })
                    .child(self.vector_control(
                        "organization-add-project",
                        "添加项目文件夹",
                        crate::icons::Icon::FolderPlus,
                        Control::AddProject,
                        !header_actions_visible,
                        cx,
                    )),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.set_hovered_section(OrganizationSection::Projects, *hovered, cx);
            }));
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
        let has_hidden = ordered.len() > 5;
        let visible_count = if self.projects_expanded {
            ordered.len()
        } else {
            5
        };
        let mut rows = div().id("organization-projects-scroll").flex().flex_col();
        for project in ordered.into_iter().take(visible_count) {
            rows = rows.child(self.render_pi_project(project, snapshot, show_archived, cx));
        }
        rows = rows.children(has_hidden.then(|| {
            self.render_progressive_control(
                OrganizationSection::Projects,
                self.projects_expanded,
                cx,
            )
        }));
        div()
            .id("organization-projects")
            .debug_selector(|| "organization-section-projects".into())
            .flex()
            .flex_col()
            .child(header)
            .child(rows)
            .into_any_element()
    }

    fn render_progressive_control(
        &self,
        section: OrganizationSection,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let section_name = match section {
            OrganizationSection::Projects => "projects",
            OrganizationSection::Recents => "recents",
        };
        let state_name = if expanded { "less" } else { "more" };
        let label = if expanded { "Show Less" } else { "Show More" };
        let id = format!("organization-{section_name}-show-{state_name}");
        let label_id = format!("organization-{section_name}-progressive-label");
        let keyboard_section = section;
        let focus = match section {
            OrganizationSection::Projects => &self.projects_progressive_focus,
            OrganizationSection::Recents => &self.recents_progressive_focus,
        };
        div()
            .id(ElementId::Name(id.clone().into()))
            .debug_selector(move || id.clone())
            .track_focus(focus)
            .focusable()
            .tab_stop(true)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .pl(px(Layout::SIDEBAR_NAV_CONTENT_INSET))
            .rounded_lg()
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.text_tertiary)
            .hover(move |style| style.bg(colors.bg_hover).text_color(colors.text_secondary))
            .focus_visible(move |style| {
                style
                    .bg(colors.bg_active)
                    .text_color(colors.text_primary)
                    .border_1()
                    .border_color(colors.border_subtle)
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_progressive_section(section, cx);
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        this.toggle_progressive_section(keyboard_section, cx);
                    }
                }),
            )
            .child(div().debug_selector(move || label_id.clone()).child(label))
            .into_any_element()
    }

    fn toggle_progressive_section(&mut self, section: OrganizationSection, cx: &mut Context<Self>) {
        match section {
            OrganizationSection::Projects => self.projects_expanded = !self.projects_expanded,
            OrganizationSection::Recents => self.recents_expanded = !self.recents_expanded,
        }
        cx.notify();
    }

    fn render_project_progressive_control(
        &self,
        project_id: &str,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let state_name = if expanded { "less" } else { "more" };
        let label = if expanded { "Show Less" } else { "Show More" };
        let id = format!("project-{project_id}-show-{state_name}");
        let label_id = format!("project-{project_id}-progressive-label");
        let mouse_project_id = project_id.to_owned();
        let keyboard_project_id = project_id.to_owned();
        div()
            .id(ElementId::Name(id.clone().into()))
            .debug_selector(move || id.clone())
            .focusable()
            .tab_stop(true)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .pl(px(Layout::SIDEBAR_NAV_CONTENT_INSET))
            .rounded_lg()
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.text_tertiary)
            .hover(move |style| style.bg(colors.bg_hover).text_color(colors.text_secondary))
            .focus_visible(move |style| {
                style
                    .bg(colors.bg_active)
                    .text_color(colors.text_primary)
                    .border_1()
                    .border_color(colors.border_subtle)
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle_project_progressive(&mouse_project_id, cx);
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        this.toggle_project_progressive(&keyboard_project_id, cx);
                    }
                }),
            )
            .child(div().debug_selector(move || label_id.clone()).child(label))
            .into_any_element()
    }

    fn toggle_project_progressive(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if let Some(organization) = self.organization.as_mut()
            && !organization.project_threads_expanded.remove(project_id)
        {
            organization
                .project_threads_expanded
                .insert(project_id.to_owned());
        }
        cx.notify();
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
        let project_for_keyboard = project.id.clone();
        let project_for_add = project.id.clone();
        let project_for_menu = project.id.clone();
        let selected = project_row_is_active(&project.id, cx);
        let actions_visible = self.hovered_project.as_deref() == Some(project.id.as_str())
            || self.focused_project.as_deref() == Some(project.id.as_str())
            || self.organization.as_ref().is_some_and(|organization| {
                matches!(
                    organization.menu.as_ref(),
                    Some(OrganizationMenu::Project(id)) if id == &project.id
                )
            });
        let project_for_hover = project.id.clone();
        let row = div()
            .id(ElementId::Name(
                format!("project-header-{}", project.id).into(),
            ))
            .debug_selector({
                let id = project.id.clone();
                move || format!("project-header-{id}")
            })
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .rounded_lg()
            .focusable()
            .tab_stop(true)
            .role(gpui_kit::Role::Button)
            .aria_label(format!(
                "{}，{}",
                project.name,
                if collapsed { "已收起" } else { "已展开" }
            ))
            .aria_expanded(!collapsed)
            .cursor_pointer()
            .when(selected, |row| row.bg(colors.bg_active))
            .hover(move |style| {
                style.bg(if selected {
                    colors.bg_active
                } else {
                    colors.bg_hover
                })
            })
            .focus_visible(move |style| {
                style
                    .bg(if selected {
                        colors.bg_active
                    } else {
                        colors.bg_hover
                    })
                    .border_1()
                    .border_color(colors.border_subtle)
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
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        this.control_action(
                            Control::ToggleProject(project_for_keyboard.clone()),
                            window,
                            cx,
                        );
                    }
                }),
            )
            .child(
                div()
                    .id(ElementId::Name(
                        format!(
                            "project-folder-{}-{}",
                            project.id,
                            if collapsed { "closed" } else { "open" }
                        )
                        .into(),
                    ))
                    .debug_selector({
                        let id = project.id.clone();
                        move || {
                            format!(
                                "project-folder-{id}-{}",
                                if collapsed { "closed" } else { "open" }
                            )
                        }
                    })
                    .size(px(16.))
                    .flex_shrink_0()
                    .child(crate::icons::icon(
                        if collapsed {
                            crate::icons::Icon::Folder
                        } else {
                            crate::icons::Icon::FolderOpen
                        },
                        colors.text_secondary,
                    )),
            )
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
                    .text_color(colors.text_primary)
                    .child(project.name.clone()),
            )
            .child(
                div()
                    .when_some(self.project_focuses.get(&project.id), |actions, focus| {
                        actions.track_focus(focus)
                    })
                    .debug_selector({
                        let id = project.id.clone();
                        let state = if actions_visible { "visible" } else { "rest" };
                        move || format!("project-actions-{id}-{state}")
                    })
                    .flex()
                    .items_center()
                    .gap_1()
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
                    )),
            );
        let mut section = div()
            .id(ElementId::Name(
                format!("project-section-{}", project_id).into(),
            ))
            .debug_selector({
                let project_id = project_id.clone();
                move || {
                    format!(
                        "project-persistent-surface-{project_id}-{}",
                        if selected { "active" } else { "rest" }
                    )
                }
            })
            .flex()
            .flex_col()
            .child(row);
        if !collapsed {
            let mut tasks: Vec<_> = self
                .threads
                .iter()
                .filter(|thread| thread.project_id == project.id)
                .filter(|thread| !thread.pinned)
                .cloned()
                .collect();
            if show_archived {
                tasks.extend(
                    self.archived
                        .iter()
                        .filter(|thread| thread.project_id == project.id)
                        .filter(|thread| !thread.pinned)
                        .cloned(),
                );
            }
            tasks = super::projections::sorted_threads(&tasks, snapshot.preferences.sort);
            let expanded = self.organization.as_ref().is_some_and(|organization| {
                organization.project_threads_expanded.contains(&project.id)
            });
            let has_hidden = tasks.len() > 5;
            let visible_count = if expanded { tasks.len() } else { 5 };
            let has_pinned = self
                .eligible_threads(show_archived)
                .iter()
                .any(|thread| thread.project_id == project.id && thread.pinned);
            let opened_id = cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.clone());
            if tasks.is_empty() && !has_pinned {
                section = section.child(self.section_label("暂无任务", cx));
            } else {
                section = section.children(tasks.iter().take(visible_count).map(|thread| {
                    self.render_pi_row(
                        thread,
                        &opened_id,
                        thread.status == ThreadStatus::Archived,
                        "project-thread-row-",
                        true,
                        cx,
                    )
                }));
                if has_hidden {
                    section = section.child(self.render_project_progressive_control(
                        &project.id,
                        expanded,
                        cx,
                    ));
                }
            }
        }
        section.into_any_element()
    }
}
