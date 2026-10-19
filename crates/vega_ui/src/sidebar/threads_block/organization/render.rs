use super::*;
use crate::scroll_edge_fade::{ScrollFade, ScrollableFadeExt};
use gpui_kit::{Bounds, Pixels, Point};
use std::{cell::Cell, rc::Rc};

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
}
impl ThreadsBlock {
    pub(super) fn open_organization_menu(
        &mut self,
        menu: OrganizationMenu,
        position: Point<Pixels>,
        anchor: Anchor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_actions();
        if let Some(org) = self.organization.as_mut() {
            org.menu = Some(menu);
            org.menu_anchor = Some((position, anchor));
            org.menu_index = 0;
            window.focus(&org.menu_focus, cx);
        }
        cx.notify();
    }

    fn control_action_from_button(
        &mut self,
        action: Control,
        bounds: Option<Bounds<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Control::Menu(menu) => {
                if let Some(bounds) = bounds {
                    self.open_organization_menu(
                        menu,
                        bounds.bottom_right(),
                        Anchor::TopRight,
                        window,
                        cx,
                    );
                }
            }
            action => self.control_action(action, window, cx),
        }
    }

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
                    org.menu_anchor = None;
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
            Control::Menu(_) => return,
            Control::Archive => {
                let show_archived = if let Some(organization) = self.organization.as_mut() {
                    organization.archive = !organization.archive;
                    organization.archive
                } else {
                    false
                };
                let projects: Vec<_> =
                    self.organization
                        .as_ref()
                        .and_then(|organization| organization.snapshot.as_ref())
                        .map(|snapshot| {
                            let mut projects: Vec<_> = snapshot
                                .project_order
                                .iter()
                                .filter_map(|id| {
                                    snapshot.projects.iter().find(|project| &project.id == id)
                                })
                                .collect();
                            projects.extend(
                                snapshot.projects.iter().filter(|project| {
                                    !snapshot.project_order.contains(&project.id)
                                }),
                            );
                            projects.into_iter().cloned().collect()
                        })
                        .unwrap_or_default();
                let project_refs: Vec<_> = projects.iter().collect();
                self.sync_project_task_windows(&project_refs, show_archived);
            }
            Control::NewGroup => self.edit_group(None, window, cx),
            Control::AddProject => {
                if let Some(org) = &self.organization {
                    org.projects.update(cx, ProjectsBlock::open_picker);
                }
            }
            Control::Collapse(target) => self.toggle_organization_collapse(target, cx),
            Control::More(id) => {
                if let Some(organization) = self.organization.as_mut() {
                    organization.grow_project_task_page(&id);
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
            Control::Menu(_) => "组织操作".to_string(),
            _ => label.clone(),
        };
        let keyboard_action = action.clone();
        let button_bounds =
            matches!(&action, Control::Menu(_)).then(|| Rc::new(Cell::new(None::<Bounds<Pixels>>)));
        let mouse_bounds = button_bounds.clone();
        let keyboard_bounds = button_bounds.clone();
        let control = div()
            .id(ElementId::Name(id.clone().into()))
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
            .cursor_pointer()
            .interactive(colors, true, selected)
            .focus_visible(move |style| {
                style
                    .bg(if selected {
                        colors.bg_active
                    } else {
                        colors.bg_hover
                    })
                    .shadow(vec![gpui_kit::BoxShadow {
                        color: colors.accent.into(),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(0.),
                        spread_radius: px(1.),
                        inset: true,
                    }])
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    if cx.has_active_drag() {
                        return;
                    }
                    cx.stop_propagation();
                    this.control_action_from_button(
                        action.clone(),
                        mouse_bounds.as_ref().and_then(|bounds| bounds.get()),
                        window,
                        cx,
                    );
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                        cx.stop_propagation();
                        this.control_action_from_button(
                            keyboard_action.clone(),
                            keyboard_bounds.as_ref().and_then(|bounds| bounds.get()),
                            window,
                            cx,
                        );
                    }
                }),
            )
            .child(label)
            .when_some(button_bounds, |control, bounds| {
                let button_geometry = gpui_kit::ScrollHandle::new();
                control.relative().track_scroll(&button_geometry).child(
                    gpui_kit::canvas(
                        move |_, _, _| bounds.set(Some(button_geometry.bounds())),
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
            });
        control.into_any_element()
    }
    pub(in crate::sidebar::threads_block) fn render_organization(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        // Every value the sections need is copied out before the first
        // `&mut self` section renderer runs, so no borrow of
        // `self.organization` is held across it. `render_recents_pi` is
        // `&mut self` because lazy loading records the live total.
        let Some((snapshot, show_archived, menu_open, projects_error)) =
            self.organization.as_ref().map(|org| {
                (
                    org.snapshot.clone().map(|mut snapshot| {
                        if let Some((_, order)) = &org.reorder.pointer_order {
                            snapshot.preferences.manual_thread_order = order.clone();
                        }
                        snapshot
                    }),
                    org.archive,
                    org.menu.is_some(),
                    org.projects.read(cx).error.clone(),
                )
            })
        else {
            return div().into_any_element();
        };
        let Some(snapshot) = snapshot else {
            return div()
                .id("sidebar-organization")
                .flex()
                .flex_col()
                .gap_1()
                .child(self.section_label("正在载入", cx))
                .into_any_element();
        };
        self.prepare_reorder();
        let mut body = div()
            .id("sidebar-organization")
            .when_some(
                self.organization.as_ref().filter(|org| org.reorder.gesture),
                |body, org| body.track_focus(&org.reorder.focus),
            )
            .capture_any_mouse_up(cx.listener(Self::release_reorder))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::release_reorder))
            .capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                this.reset_reorder(window, cx);
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered {
                    this.clear_reorder_projection(cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &CloseSettings, window, cx| {
                this.clear_reorder_projection(cx);
                this.cancel_reorder(window, cx);
            }))
            .capture_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    this.clear_reorder_projection(cx);
                    if event.keystroke.key == "escape" {
                        this.cancel_reorder(window, cx);
                    }
                }),
            )
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    let modifiers = &event.keystroke.modifiers;
                    if event.keystroke.key != "tab"
                        || modifiers.control
                        || modifiers.alt
                        || modifiers.platform
                        || modifiers.function
                        || this.actions_open.is_some()
                        || this.editing.is_some()
                        || cx.has_active_drag()
                        || this.organization.as_ref().is_none_or(|org| {
                            org.menu.is_some() || org.editor.is_some() || org.reorder.gesture
                        })
                    {
                        return;
                    }
                    if modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }),
            )
            .flex()
            .flex_col()
            .gap(px(Layout::SIDEBAR_SECTION_GAP));
        body = body.children(self.render_pinned_pi(&snapshot, show_archived, cx));
        body = body.child(self.render_projects_pi(&snapshot, show_archived, cx));
        body = body.child(self.render_recents_pi(&snapshot, show_archived, cx));
        body = body.children(menu_open.then(|| self.render_organization_menu(cx)));
        body = body.children(projects_error.map(|message| error_bar(message, &colors)));
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
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let colors = theme(cx).colors;
        let id = id.into();
        let label = label.into();
        let selected = matches!(action, Control::Archive)
            && self.organization.as_ref().is_some_and(|org| org.archive);
        let button_bounds =
            matches!(&action, Control::Menu(_)).then(|| Rc::new(Cell::new(None::<Bounds<Pixels>>)));
        let mouse_bounds = button_bounds.clone();
        let keyboard_bounds = button_bounds.clone();
        let control = div()
            .id(ElementId::Name(id.clone().into()))
            .aria_label(label.clone())
            .focusable()
            .tab_stop(true)
            .size(px(Layout::SIDEBAR_ACTION_SIZE))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .when(selected, |style| style.bg(colors.bg_active))
            .when(hidden_until_hover && !selected, |style| style.opacity(0.))
            .hover(move |style| {
                style.opacity(1.).bg(if selected {
                    colors.bg_active
                } else {
                    colors.bg_hover
                })
            })
            .focus_visible(move |style| {
                style
                    .opacity(1.)
                    .bg(colors.bg_active)
                    .border_1()
                    .border_color(colors.border_subtle)
            });
        let control = if matches!(action, Control::Archive) {
            control
                .role(gpui_kit::Role::Button)
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.control_action(Control::Archive, window, cx);
                }))
        } else {
            let keyboard_action = action.clone();
            control
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.control_action_from_button(
                            action.clone(),
                            mouse_bounds.as_ref().and_then(|bounds| bounds.get()),
                            window,
                            cx,
                        );
                    }),
                )
                .on_key_down(cx.listener(
                    move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            this.control_action_from_button(
                                keyboard_action.clone(),
                                keyboard_bounds.as_ref().and_then(|bounds| bounds.get()),
                                window,
                                cx,
                            );
                        }
                    },
                ))
        }
        .when_some(button_bounds, |control, bounds| {
            let button_geometry = gpui_kit::ScrollHandle::new();
            control.relative().track_scroll(&button_geometry).child(
                gpui_kit::canvas(
                    move |_, _, _| bounds.set(Some(button_geometry.bounds())),
                    |_, (), _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
        });
        control.child(crate::icons::icon(kind, colors.text_secondary))
    }

    pub(super) fn archive_control(
        &self,
        hidden_until_hover: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        self.vector_control(
            "organization-archive",
            if self.organization.as_ref().is_some_and(|org| org.archive) {
                "隐藏已归档"
            } else {
                "显示已归档"
            },
            crate::icons::Icon::Archive,
            Control::Archive,
            hidden_until_hover,
            cx,
        )
    }

    pub(super) fn eligible_threads(&self, show_archived: bool) -> Vec<Thread> {
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
        let label = "已固定";
        let mut pinned: Vec<_> = self
            .eligible_threads(show_archived)
            .into_iter()
            .filter(|thread| thread.pinned)
            .collect();
        pinned = super::projections::sorted_threads(&pinned, &snapshot.preferences);
        if pinned.is_empty() {
            return None;
        }
        let opened_id = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone());
        Some(
            observe_sidebar!(div().id("organization-pinned"))
                .flex()
                .flex_col()
                .flex_shrink_0()
                .child(
                    div()
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .child(div().pl(px(Layout::SIDEBAR_LABEL_INSET)).child(label)),
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
                        }))
                        .scroll_edge_fade(colors.bg_sidebar, ScrollFade::compact()),
                )
                .into_any_element(),
        )
    }

    fn render_recents_pi(
        &mut self,
        snapshot: &SidebarOrganizationSnapshot,
        show_archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let label = "最近任务";
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
        standalone = super::projections::sorted_threads(&standalone, &snapshot.preferences);
        let header_actions_visible = self.hovered_section == Some(OrganizationSection::Recents)
            || self.focused_section == Some(OrganizationSection::Recents);
        let header = div()
            .id("organization-recents-header")
            .track_focus(&self.recents_header_focus)
            .flex()
            .items_center()
            .justify_between()
            .h(px(28.))
            .child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .pl(px(Layout::SIDEBAR_LABEL_INSET))
                    .child(label),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(self.archive_control(!header_actions_visible, cx))
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

        self.recents_total = standalone.len();
        let rows = standalone
            .into_iter()
            .take(self.recents_visible)
            .map(|thread| {
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
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_projects_pi(
        &mut self,
        snapshot: &SidebarOrganizationSnapshot,
        show_archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let label = "项目";
        let header_actions_visible = self.hovered_section == Some(OrganizationSection::Projects)
            || self.focused_section == Some(OrganizationSection::Projects);
        let header = div()
            .id("organization-projects-header")
            .track_focus(&self.projects_header_focus)
            .flex()
            .items_center()
            .justify_between()
            .h(px(28.))
            .child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .pl(px(Layout::SIDEBAR_LABEL_INSET))
                    .child(label),
            )
            .child(div().child(self.vector_control(
                "organization-add-project",
                "添加项目文件夹",
                crate::icons::Icon::FolderPlus,
                Control::AddProject,
                !header_actions_visible,
                cx,
            )))
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
        let all_projects: Vec<_> = ordered.into_iter().collect();
        self.sync_project_task_windows(&all_projects, show_archived);
        let mut rows = div().id("organization-projects-scroll").flex().flex_col();
        for project in all_projects.into_iter().take(visible_count) {
            rows = rows.child(self.render_pi_project(project, snapshot, show_archived, cx));
        }
        rows = rows.children(
            has_hidden.then(|| self.render_progressive_control(self.projects_expanded, cx)),
        );
        observe_sidebar!(div().id("organization-projects"))
            .flex()
            .flex_col()
            .child(header)
            .child(rows)
            .into_any_element()
    }

    fn render_progressive_control(&self, expanded: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let state_name = if expanded { "less" } else { "more" };
        let label = if expanded { "收起" } else { "展开更多" };
        let id = format!("organization-projects-show-{state_name}");
        let focus = &self.projects_progressive_focus;
        div()
            .id(ElementId::Name(id.clone().into()))
            .track_focus(focus)
            .focusable()
            .tab_stop(true)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .pl(px(Layout::SIDEBAR_ROW_INSET))
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
                    this.toggle_progressive_section(cx);
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        this.toggle_progressive_section(cx);
                    }
                }),
            )
            .child(div().child(label))
            .into_any_element()
    }

    fn toggle_progressive_section(&mut self, cx: &mut Context<Self>) {
        self.projects_expanded = !self.projects_expanded;
        cx.notify();
    }

    fn render_project_progressive_control(
        &self,
        project_id: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let label = "展开更多";
        let id = format!("project-{project_id}-show-more");
        let mouse_project_id = project_id.to_owned();
        let keyboard_project_id = project_id.to_owned();
        div()
            .id(ElementId::Name(id.clone().into()))
            .focusable()
            .tab_stop(true)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .pl(px(Layout::SIDEBAR_ROW_INSET))
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
                cx.listener(move |this, _, window, cx| {
                    if cx.has_active_drag() {
                        return;
                    }
                    cx.stop_propagation();
                    this.control_action(Control::More(mouse_project_id.clone()), window, cx);
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        this.control_action(Control::More(keyboard_project_id.clone()), window, cx);
                    }
                }),
            )
            .child(div().child(label))
            .into_any_element()
    }

    fn render_pi_project(
        &mut self,
        project: &SidebarProject,
        snapshot: &SidebarOrganizationSnapshot,
        show_archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let target = SidebarCollapseTarget::Project(project.id.clone());
        let collapsed = snapshot.collapsed.contains(&target);
        let project_id = project.id.clone();
        let project_for_hover = project.id.clone();
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
        let row = div()
            .id(ElementId::Name(
                format!("project-header-{}", project.id).into(),
            ))
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .flex()
            .items_center()
            .gap_2()
            .pr_2()
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
            .interactive(colors, true, selected)
            .focus_visible(move |style| {
                style
                    .bg(if selected {
                        colors.bg_active
                    } else {
                        colors.bg_hover
                    })
                    .shadow(vec![gpui_kit::BoxShadow {
                        color: colors.accent.into(),
                        offset: point(px(0.), px(0.)),
                        blur_radius: px(0.),
                        spread_radius: px(1.),
                        inset: true,
                    }])
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
            .flex()
            .flex_col()
            .child(self.render_reorder_row(
                ReorderItem::Project(project.id.clone()),
                project.name.clone(),
                row,
                cx,
            ));
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
            tasks = super::projections::sorted_threads(&tasks, &snapshot.preferences);
            let (visible_count, has_hidden) = if let Some(organization) = self.organization.as_mut()
            {
                organization
                    .project_threads_total
                    .insert(project.id.clone(), tasks.len());
                let visible_window = *organization
                    .project_threads_visible
                    .entry(project.id.clone())
                    .or_insert(super::PROJECT_TASKS_PAGE);
                (
                    visible_window.min(tasks.len()),
                    organization.has_project_task_page(&project.id),
                )
            } else {
                (
                    super::PROJECT_TASKS_PAGE.min(tasks.len()),
                    tasks.len() > super::PROJECT_TASKS_PAGE,
                )
            };
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
                    section =
                        section.child(self.render_project_progressive_control(&project.id, cx));
                }
            }
        }
        section.into_any_element()
    }
}
