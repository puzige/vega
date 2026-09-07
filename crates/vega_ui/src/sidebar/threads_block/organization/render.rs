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
            Control::Menu(OrganizationMenu::Filter) => "视图与排序".to_string(),
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
            .when(selected, |s| s.bg(colors.bg_active))
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
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
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
        let snapshot = org.snapshot.clone();
        let preferences = snapshot
            .as_ref()
            .map(|s| s.preferences.clone())
            .unwrap_or_default();
        let archive = org.archive;
        let project_error = org.projects.read(cx).error.clone();
        let mut body = div()
            .id("sidebar-organization")
            .flex()
            .flex_col()
            .gap_1()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab"
                    && this.editing.is_none()
                    && this
                        .organization
                        .as_ref()
                        .is_none_or(|o| o.editor.is_none())
                {
                    this.close_actions();
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(self.organization_control(
                                "organization-groups",
                                "SESSIONS",
                                Control::View(SidebarView::Groups),
                                preferences.view == SidebarView::Groups,
                                cx,
                            ))
                            .child(self.organization_control(
                                "organization-projects",
                                "PROJECTS",
                                Control::View(SidebarView::Projects),
                                preferences.view == SidebarView::Projects,
                                cx,
                            )),
                    )
                    .child(div().flex_1())
                    .child(self.organization_control(
                        "organization-collapse-all",
                        "⌃",
                        Control::CollapseAll,
                        false,
                        cx,
                    ))
                    .child(self.organization_control(
                        "organization-filter",
                        "≡",
                        Control::Menu(OrganizationMenu::Filter),
                        false,
                        cx,
                    ))
                    .child(self.organization_control(
                        "organization-archive",
                        "归档",
                        Control::Archive,
                        archive,
                        cx,
                    ))
                    .children(
                        self.organization
                            .as_ref()
                            .is_some_and(|o| o.menu.is_some())
                            .then(|| self.render_organization_menu(cx)),
                    ),
            )
            .children(project_error.map(|message| error_bar(message, &colors)))
            .children(
                self.error
                    .clone()
                    .map(|message| error_bar(message, &colors)),
            );
        if let Some(editor) = self.organization.as_ref().and_then(|o| o.editor.as_ref()) {
            body = body.child(
                div()
                    .id("organization-group-editor")
                    .key_context("ThreadRename")
                    .track_focus(&editor.input.read(cx).focus_handle(cx))
                    .on_action(cx.listener(|this, _: &ConfirmRename, _, cx| this.commit_group(cx)))
                    .on_action(cx.listener(|this, _: &CloseSettings, _, cx| {
                        if let Some(org) = this.organization.as_mut()
                            && !org.pending
                        {
                            org.editor = None;
                            this.error = None;
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(editor.input.clone()),
            );
        }
        let Some(snapshot) = snapshot else {
            return body.child("正在载入…").into_any_element();
        };
        if archive {
            body = body.child(self.section_label("已归档", cx));
            if self.archived.is_empty() {
                body = body.child(self.section_label("暂无已归档任务", cx));
            }
            for thread in self.archived.clone() {
                body = body.child(self.organization_thread(&thread, None, true, true, cx));
            }
        } else if preferences.view == SidebarView::Groups {
            body = body.child(self.render_group_projection(&snapshot, cx));
        } else if preferences.project_view == SidebarProjectView::ByProject {
            body = body.child(self.render_project_projection(&snapshot, cx));
        } else {
            body = body.child(self.render_timeline_projection(&snapshot, cx));
        }
        body.into_any_element()
    }
}
