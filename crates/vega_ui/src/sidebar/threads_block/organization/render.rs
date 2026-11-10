use super::*;
use gpui_kit::{Bounds, Pixels, Point};

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
                    organization.task_projection = None;
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
            Control::NewStandalone => self.create_task(None, cx),
            Control::NewProjectTask(id) => self.create_task(Some(id), cx),
            Control::ToggleProject(id) => {
                self.toggle_organization_collapse(SidebarCollapseTarget::Project(id), cx);
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
        let label = match &action {
            Control::CollapseAll => vega_i18n::t("ui.collapse_all").to_owned(),
            Control::Menu(_) => vega_i18n::t("ui.organization_actions").to_owned(),
            _ => label.into(),
        };
        let mut item = nsh_leaf::Item::action(label, None);
        item.selected = selected;
        self.nsh_control(id.into(), item, action, false, cx)
    }

    fn nsh_control(
        &self,
        id: String,
        item: nsh_leaf::Item,
        action: Control,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let observed = self
            .organization
            .as_ref()
            .and_then(|org| org.snapshot.clone());
        let generation = self
            .organization
            .as_ref()
            .map_or(0, |org| org.presentation_generation);
        let archive = self.organization.as_ref().is_some_and(|org| org.archive);
        let revision = observed.as_ref().map_or(0, |snapshot| snapshot.revision);
        let weak = cx.weak_entity();
        let target = weak.clone();
        let mut item = item;
        item.compact = compact;
        nsh_leaf::host(
            format!("{id}-{revision}-{generation}-{archive}"),
            None,
            vec![item],
            false,
            if compact {
                Layout::SIDEBAR_ACTION_SIZE
            } else {
                Typography::SIDEBAR_LINE_HEIGHT
            },
            move |cx| {
                weak.upgrade().is_some_and(|entity| {
                    let this = entity.read(cx);
                    this.editing.is_none()
                        && !this.action_pending
                        && this.organization.as_ref().is_some_and(|org| {
                            org.snapshot == observed
                                && org.presentation_generation == generation
                                && org.archive == archive
                                && org.pending != Some(PendingRequest::Write)
                                && org.editor.is_none()
                                && !org.reorder.gesture
                        })
                })
            },
            move |_, bounds, window, cx| {
                let _ = target.update(cx, |this, cx| {
                    this.control_action_from_button(action.clone(), Some(bounds), window, cx)
                });
            },
        )
    }

    pub(in crate::sidebar::threads_block) fn render_organization(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        if let Some(org) = &self.organization {
            org.next_time_change.set(i64::MAX);
        }
        let Some((snapshot, show_archived, menu_open)) = self.organization.as_ref().map(|org| {
            (
                org.snapshot.clone().map(|mut snapshot| {
                    if let Some((_, order)) = &org.reorder.pointer_order {
                        std::rc::Rc::make_mut(&mut snapshot)
                            .preferences
                            .manual_thread_order = order.clone();
                    }
                    snapshot
                }),
                org.archive,
                org.menu.is_some(),
            )
        }) else {
            return div().into_any_element();
        };
        let Some(snapshot) = snapshot else {
            return div()
                .id("sidebar-organization")
                .flex()
                .flex_col()
                .gap_1()
                .child(self.section_label(vega_i18n::t("ui.loading_335"), cx))
                .into_any_element();
        };
        self.prepare_task_projection(&snapshot);
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
        body = body.children(
            self.error
                .clone()
                .map(|message| error_bar(message, &colors)),
        );
        if let Some(org) = &self.organization {
            for (_, error) in &org.collapse.errors {
                body = body.child(error_bar(
                    crate::localized_copy::LocalizedCopy::with_args(
                        "ui.collapse_save_failed_retry",
                        vec![("error", error.clone())],
                    ),
                    &colors,
                ));
            }
        }
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
        let selected = matches!(action, Control::Archive)
            && self.organization.as_ref().is_some_and(|org| org.archive);
        let mut item = nsh_leaf::Item::action(label.into(), Some(kind));
        item.selected = selected;
        item.quiet = hidden_until_hover && !selected;
        let id = id.into();
        let content = self.nsh_control(id.clone(), item, action, true, cx);
        div()
            .id(gpui_kit::SharedString::from(format!("{id}-host")))
            .size(px(Layout::SIDEBAR_ACTION_SIZE))
            .flex_shrink_0()
            .child(content)
    }

    pub(super) fn archive_control(
        &self,
        hidden_until_hover: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        self.vector_control(
            "organization-archive",
            if self.organization.as_ref().is_some_and(|org| org.archive) {
                vega_i18n::t("ui.hide_archived")
            } else {
                vega_i18n::t("ui.show_archived")
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
        let label = vega_i18n::t("sidebar.pinned");
        let _ = (snapshot, show_archived);
        let projection = self.task_projection();
        let pinned = &projection.pinned;
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
                .flex()
                .flex_col()
                .flex_shrink_0()
                .child(
                    div()
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .text_size(px(Typography::SIDEBAR_SECTION))
                        .font_weight(Typography::SIDEBAR_SECTION_WEIGHT)
                        .text_color(colors.text_tertiary)
                        .pl(px(Layout::SIDEBAR_LABEL_INSET))
                        .child(self.section_label(label, cx)),
                )
                .child(
                    div()
                        .id("organization-pinned-scroll")
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
        &mut self,
        snapshot: &SidebarOrganizationSnapshot,
        show_archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let label = vega_i18n::t("sidebar.recents");
        let _ = (snapshot, show_archived);
        let projection = self.task_projection();
        let recents = &projection.recents;
        let header_actions_visible = self.hovered_section == Some(OrganizationSection::Recents)
            || self.focused_section == Some(OrganizationSection::Recents)
            || self
                .organization
                .as_ref()
                .is_some_and(|org| matches!(org.menu, Some(OrganizationMenu::Recents)));
        let header = div()
            .id("organization-recents-header")
            .track_focus(&self.recents_header_focus)
            .flex()
            .items_center()
            .justify_between()
            .h(px(28.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(Typography::SIDEBAR_SECTION))
                    .font_weight(Typography::SIDEBAR_SECTION_WEIGHT)
                    .text_color(colors.text_tertiary)
                    .pl(px(Layout::SIDEBAR_LABEL_INSET))
                    .child(self.section_label(label, cx)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(self.vector_control(
                        "organization-recents-menu",
                        vega_i18n::t("sidebar.recents_options"),
                        crate::icons::Icon::More,
                        Control::Menu(OrganizationMenu::Recents),
                        !header_actions_visible,
                        cx,
                    ))
                    .child(self.archive_control(!header_actions_visible, cx))
                    .child(self.vector_control(
                        "organization-new-session",
                        vega_i18n::t("ui.new_standalone_task"),
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
        let empty = recents.is_empty();

        self.recents_total = recents.len();
        let rows: Vec<_> = recents
            .iter()
            .take(self.recents_visible)
            .map(|thread| {
                let archived = thread.status == ThreadStatus::Archived;
                self.render_recent_row(
                    thread,
                    &opened_id,
                    archived,
                    "standalone-thread-row-",
                    true,
                    cx,
                )
            })
            .collect();
        let empty = empty.then(|| {
            div()
                .id("organization-recents-empty")
                .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                .px_3()
                .flex()
                .items_center()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_tertiary)
                .child(self.section_label(vega_i18n::t("ui.no_recent_tasks"), cx))
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
        let label = vega_i18n::t("sidebar.projects");
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
                    .flex_1()
                    .min_w_0()
                    .text_size(px(Typography::SIDEBAR_SECTION))
                    .font_weight(Typography::SIDEBAR_SECTION_WEIGHT)
                    .text_color(colors.text_tertiary)
                    .pl(px(Layout::SIDEBAR_LABEL_INSET))
                    .child(self.section_label(label, cx)),
            )
            .child(div().child(self.vector_control(
                "organization-add-project",
                vega_i18n::t("ui.add_project_folder"),
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
        let mut rows = div().id("organization-projects-scroll").flex().flex_col();
        for project in all_projects.into_iter().take(visible_count) {
            rows = rows.child(self.render_pi_project(project, snapshot, show_archived, cx));
        }
        rows = rows.children(
            has_hidden.then(|| self.render_progressive_control(self.projects_expanded, cx)),
        );
        div()
            .id("organization-projects")
            .flex()
            .flex_col()
            .child(header)
            .child(rows)
            .into_any_element()
    }

    fn render_progressive_control(&self, expanded: bool, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let target = weak.clone();
        nsh_leaf::host(
            format!("organization-projects-show-{expanded}"),
            None,
            vec![nsh_leaf::Item::action(
                if expanded {
                    vega_i18n::t("ui.collapse")
                } else {
                    vega_i18n::t("ui.show_more")
                },
                None,
            )],
            false,
            Typography::SIDEBAR_LINE_HEIGHT,
            move |cx| {
                weak.upgrade()
                    .is_some_and(|entity| entity.read(cx).projects_expanded == expanded)
            },
            move |_, _, _, cx| {
                let _ = target.update(cx, |this, cx| this.toggle_progressive_section(cx));
            },
        )
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
        self.nsh_control(
            format!("project-show-more-{project_id}"),
            nsh_leaf::Item::action(vega_i18n::t("ui.show_more"), None),
            Control::More(project_id.to_owned()),
            false,
            cx,
        )
    }

    fn render_pi_project(
        &mut self,
        project: &SidebarProject,
        snapshot: &SidebarOrganizationSnapshot,
        show_archived: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = SidebarCollapseTarget::Project(project.id.clone());
        let collapsed = snapshot.collapsed.contains(&target);
        let project_id = project.id.clone();
        let project_for_hover = project.id.clone();
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
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.set_hovered_project(&project_for_hover, *hovered, cx);
            }))
            .child({
                let mut item = nsh_leaf::Item::action(
                    project.name.clone(),
                    Some(if collapsed {
                        crate::icons::Icon::Folder
                    } else {
                        crate::icons::Icon::FolderOpen
                    }),
                );
                item.selected = selected;
                div().flex_1().min_w_0().child(self.nsh_control(
                    format!("project-label-{}", project.id),
                    item,
                    Control::ToggleProject(project.id.clone()),
                    false,
                    cx,
                ))
            })
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
                        vega_i18n::t_args("ui.new_task_in_project", &[("project", &project.name)]),
                        crate::icons::Icon::Plus,
                        Control::NewProjectTask(project.id.clone()),
                        !actions_visible,
                        cx,
                    ))
                    .child(self.vector_control(
                        format!("project-more-{}", project.id),
                        vega_i18n::t_args(
                            "ui.project_project_actions",
                            &[("project", &project.name)],
                        ),
                        crate::icons::Icon::More,
                        Control::Menu(OrganizationMenu::Project(project.id.clone())),
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
            let _ = show_archived;
            let projection = self.task_projection();
            let tasks = projection
                .projects
                .get(&project.id)
                .map(Vec::as_slice)
                .unwrap_or_default();
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
            let has_pinned = projection
                .pinned
                .iter()
                .any(|thread| thread.project_id == project.id);
            let opened_id = cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.clone());
            if tasks.is_empty() && !has_pinned {
                section = section.child(self.section_label(vega_i18n::t("ui.no_tasks"), cx));
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
