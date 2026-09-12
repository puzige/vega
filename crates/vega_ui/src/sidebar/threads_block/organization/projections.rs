use super::render::Control;
use super::*;
impl ThreadsBlock {
    pub(super) fn render_group_projection(
        &self,
        snapshot: &SidebarOrganizationSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = div().flex().flex_col();
        let preferences = snapshot.preferences.clone();
        let colors = theme(cx).colors;

        for group in &snapshot.groups {
            let target = SidebarCollapseTarget::Group(group.id.clone());
            let collapsed = snapshot.collapsed.contains(&target);
            let group_id = group.id.clone();
            let before = group.id.clone();
            let context_group = group.id.clone();
            let drag = OrganizationDrag {
                kind: DragKind::Group(group.id.clone()),
                label: group.name.clone(),
            };
            let header = div()
                .id(ElementId::Name(format!("group-header-{}", group.id).into()))
                .debug_selector({
                    let id = group.id.clone();
                    move || format!("group-header-{id}")
                })
                .flex()
                .items_center()
                .drag_over::<OrganizationDrag>(move |s, _, _, _| s.bg(colors.bg_active))
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.control_action(
                            Control::Menu(OrganizationMenu::Group(context_group.clone())),
                            window,
                            cx,
                        );
                    }),
                )
                .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                .on_drop(cx.listener(move |this, drag: &OrganizationDrag, _, cx| {
                    let action = match &drag.kind {
                        DragKind::Thread(id) => Some(SidebarOrganizationAction::MoveThread {
                            thread_id: id.clone(),
                            group_id: Some(group_id.clone()),
                            before_id: None,
                        }),
                        DragKind::Group(id) if id != &before => {
                            Some(SidebarOrganizationAction::MoveGroup {
                                group_id: id.clone(),
                                before_id: Some(before.clone()),
                            })
                        }
                        _ => None,
                    };
                    if let Some(action) = action {
                        this.submit_organization(action, cx);
                    }
                    cx.stop_propagation();
                }))
                .child(
                    div()
                        .text_color(group_color(group.color, &colors))
                        .child("●"),
                )
                .child(self.organization_control(
                    format!("group-collapse-{}", group.id),
                    "展开或收起分组",
                    Control::Collapse(target),
                    false,
                    cx,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(Typography::SIDEBAR))
                        .text_color(colors.text_primary)
                        .child(group.name.clone()),
                )
                .child(self.organization_control(
                    format!("group-menu-{}", group.id),
                    "更多分组操作",
                    Control::Menu(OrganizationMenu::Group(group.id.clone())),
                    false,
                    cx,
                ));
            body = body.child(header);
            if !collapsed {
                let rows: Vec<_> = snapshot
                    .memberships
                    .iter()
                    .filter(|m| m.group_id == group.id)
                    .filter_map(|m| self.threads.iter().find(|t| t.id == m.thread_id))
                    .cloned()
                    .collect();
                if rows.is_empty() {
                    body = body.child(self.group_drop_empty(group.id.clone(), cx));
                }
                for thread in rows {
                    body = body.child(self.organization_thread(
                        &thread,
                        Some(group.id.clone()),
                        true,
                        false,
                        true,
                        cx,
                    ));
                }
            }
        }
        let target = SidebarCollapseTarget::Ungrouped;
        let ungrouped = div()
            .id("organization-ungrouped-drop")
            .min_h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .drag_over::<OrganizationDrag>(move |s, _, _, _| s.bg(colors.bg_active))
            .on_drop(cx.listener(|this, drag: &OrganizationDrag, _, cx| {
                if let DragKind::Thread(id) = &drag.kind {
                    this.submit_organization(
                        SidebarOrganizationAction::MoveThread {
                            thread_id: id.clone(),
                            group_id: None,
                            before_id: None,
                        },
                        cx,
                    );
                }
                cx.stop_propagation();
            }));
        // Both the heading and its expanded rows belong to the same drop target.
        let mut rows = ungrouped.flex().flex_col().child(self.organization_control(
            "organization-ungrouped",
            "未分组",
            Control::Collapse(target.clone()),
            false,
            cx,
        ));
        if !snapshot.collapsed.contains(&target) {
            for thread in sorted_threads(&self.threads, preferences.sort)
                .into_iter()
                .filter(|t| !snapshot.memberships.iter().any(|m| m.thread_id == t.id))
            {
                rows = rows.child(self.organization_thread(&thread, None, true, false, true, cx));
            }
        }
        body = body.child(rows);
        body.into_any_element()
    }
    pub(super) fn render_project_projection(
        &self,
        snapshot: &SidebarOrganizationSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = div().flex().flex_col();
        let preferences = snapshot.preferences.clone();
        let colors = theme(cx).colors;

        body = body.child(self.organization_control(
            "organization-add-project",
            "+ 添加项目",
            Control::AddProject,
            false,
            cx,
        ));
        let mut ordered: Vec<_> = snapshot
            .project_order
            .iter()
            .filter_map(|id| snapshot.projects.iter().find(|p| &p.id == id))
            .collect();
        ordered.extend(
            snapshot
                .projects
                .iter()
                .filter(|p| !snapshot.project_order.contains(&p.id)),
        );
        for project in ordered {
            let reveal = self
                .organization
                .as_ref()
                .and_then(|org| org.reveal_project.as_ref())
                .filter(|(id, _)| id == &project.id)
                .map(|(_, requested)| requested.clone());
            let target = SidebarCollapseTarget::Project(project.id.clone());
            let collapsed = snapshot.collapsed.contains(&target);
            let branch = self.organization.as_ref().and_then(|o| {
                o.projects
                    .update(cx, |p, _| p.organization_branch(&project.id))
            });
            let before = project.id.clone();
            let context_project = project.id.clone();
            let toggle_project = project.id.clone();
            let keyboard_project = project.id.clone();
            let selected = project_row_is_active(&project.id, cx);
            let drag = OrganizationDrag {
                kind: DragKind::Project(project.id.clone()),
                label: project.name.clone(),
            };
            let header = div()
                .on_children_prepainted(move |bounds, window, _| {
                    if let Some(requested) = &reveal
                        && requested.get()
                        && let Some(bounds) = bounds.first()
                    {
                        requested.set(false);
                        window.request_autoscroll(*bounds);
                    }
                })
                .id(ElementId::Name(
                    format!("project-header-{}", project.id).into(),
                ))
                .debug_selector({
                    let id = project.id.clone();
                    move || format!("project-header-{id}")
                })
                .flex()
                .items_center()
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.control_action(
                            Control::Menu(OrganizationMenu::Project(context_project.clone())),
                            window,
                            cx,
                        );
                    }),
                )
                .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                .drag_over::<OrganizationDrag>(move |s, _, _, _| s.bg(colors.bg_active))
                .on_drop(cx.listener(move |this, drag: &OrganizationDrag, _, cx| {
                    if let DragKind::Project(id) = &drag.kind
                        && id != &before
                    {
                        this.submit_organization(
                            SidebarOrganizationAction::MoveProject {
                                project_id: id.clone(),
                                before_id: Some(before.clone()),
                            },
                            cx,
                        );
                    }
                    cx.stop_propagation();
                }))
                .child(
                    div()
                        .id(ElementId::Name(
                            format!("organization-project-{}", project.id).into(),
                        ))
                        .debug_selector({
                            let id = project.id.clone();
                            move || format!("organization-project-{id}")
                        })
                        .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                        .px_2()
                        .rounded_lg()
                        .flex()
                        .items_center()
                        .gap_2()
                        .flex_1()
                        .min_w_0()
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
                        .hover(move |row| {
                            row.bg(if selected {
                                colors.bg_active
                            } else {
                                colors.bg_hover
                            })
                        })
                        .focus_visible(move |row| {
                            row.bg(if selected {
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
                                this.control_action(
                                    Control::ToggleProject(toggle_project.clone()),
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .on_key_down(cx.listener(
                            move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    cx.stop_propagation();
                                    this.control_action(
                                        Control::ToggleProject(keyboard_project.clone()),
                                        window,
                                        cx,
                                    );
                                }
                            },
                        ))
                        .child(crate::icons::icon(
                            if collapsed {
                                crate::icons::Icon::Folder
                            } else {
                                crate::icons::Icon::FolderOpen
                            },
                            colors.text_secondary,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(Typography::SIDEBAR))
                                .text_color(colors.text_primary)
                                .child(project.name.clone()),
                        )
                        .children(branch.map(|branch| {
                            div()
                                .w(px(Layout::SIDEBAR_PROJECT_METADATA_WIDTH))
                                .flex_shrink_0()
                                .truncate()
                                .text_size(px(Typography::METADATA))
                                .text_color(colors.text_tertiary)
                                .child(branch)
                        })),
                )
                .child(self.vector_control(
                    format!("project-menu-{}", project.id),
                    "更多项目操作",
                    crate::icons::Icon::More,
                    Control::Menu(OrganizationMenu::Project(project.id.clone())),
                    false,
                    cx,
                ));
            body = body.child(header);
            if !collapsed {
                let rows: Vec<_> = sorted_threads(&self.threads, preferences.sort)
                    .into_iter()
                    .filter(|t| t.project_id == project.id)
                    .collect();
                let count = self
                    .organization
                    .as_ref()
                    .and_then(|o| o.more.get(&project.id).copied())
                    .unwrap_or(5);
                if rows.is_empty() {
                    body = body.child(self.section_label("暂无任务", cx));
                }
                for thread in rows.iter().take(count) {
                    body =
                        body.child(self.organization_thread(thread, None, false, false, false, cx));
                }
                if rows.len() > count {
                    body = body.child(self.organization_control(
                        format!("project-more-{}", project.id),
                        "显示更多",
                        Control::More(project.id.clone()),
                        false,
                        cx,
                    ));
                }
            }
        }
        body.into_any_element()
    }
    pub(super) fn render_timeline_projection(
        &self,
        snapshot: &SidebarOrganizationSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = div().flex().flex_col();
        let preferences = snapshot.preferences.clone();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let rows = sorted_threads(&self.threads, preferences.sort);
        if rows.is_empty() {
            body = body.child(self.section_label("暂无活跃任务", cx));
        }
        let sections = [
            (None, "置顶"),
            (Some(SidebarTimelineBucket::Today), "今天"),
            (Some(SidebarTimelineBucket::Yesterday), "昨天"),
            (Some(SidebarTimelineBucket::Last7Days), "近7天"),
            (Some(SidebarTimelineBucket::Last30Days), "近30天"),
            (Some(SidebarTimelineBucket::Earlier), "更早"),
        ];
        for (bucket, label) in sections {
            let rows: Vec<_> = rows
                .iter()
                .filter(|t| {
                    if let Some(bucket) = bucket {
                        !t.pinned
                            && service::local_calendar_bucket(task_time(t, preferences.sort), now)
                                .ok()
                                == Some(bucket)
                    } else {
                        t.pinned
                    }
                })
                .collect();
            if rows.is_empty() {
                continue;
            }
            let target = bucket
                .map(SidebarCollapseTarget::Timeline)
                .unwrap_or(SidebarCollapseTarget::Pinned);
            body = body.child(self.organization_control(
                format!("timeline-{bucket:?}"),
                label,
                Control::Collapse(target.clone()),
                false,
                cx,
            ));
            if !snapshot.collapsed.contains(&target) {
                for thread in rows {
                    body =
                        body.child(self.organization_thread(thread, None, true, false, false, cx));
                }
            }
        }
        body.into_any_element()
    }
    pub(super) fn section_label(&self, text: &str, cx: &App) -> AnyElement {
        div()
            .px_2()
            .py_1()
            .text_size(px(Typography::METADATA))
            .font_weight(Typography::HEADING_CARD_WEIGHT)
            .text_color(theme(cx).colors.text_tertiary)
            .child(text.to_string())
            .into_any_element()
    }
    fn group_drop_empty(&self, group_id: String, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id(ElementId::Name(format!("group-empty-{group_id}").into()))
            .debug_selector({
                let id = group_id.clone();
                move || format!("group-empty-{id}")
            })
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .px_3()
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.text_tertiary)
            .child("拖入任务，或通过任务菜单移入")
            .drag_over::<OrganizationDrag>(move |s, _, _, _| s.bg(colors.bg_active))
            .on_drop(cx.listener(move |this, drag: &OrganizationDrag, _, cx| {
                if let DragKind::Thread(id) = &drag.kind {
                    this.submit_organization(
                        SidebarOrganizationAction::MoveThread {
                            thread_id: id.clone(),
                            group_id: Some(group_id.clone()),
                            before_id: None,
                        },
                        cx,
                    );
                }
                cx.stop_propagation();
            }))
            .into_any_element()
    }
    pub(super) fn organization_thread(
        &self,
        thread: &Thread,
        group_id: Option<String>,
        show_project: bool,
        archived: bool,
        actions_enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !actions_enabled {
            return self.render_project_thread(thread, show_project, cx);
        }
        let colors = theme(cx).colors;
        let opened = cx.global::<OpenedThread>().0.as_ref().map(|t| t.id.clone());
        let before = thread.id.clone();
        let drag = OrganizationDrag {
            kind: DragKind::Thread(thread.id.clone()),
            label: thread_title(thread),
        };
        let mut row = div()
            .id(ElementId::Name(
                format!("organized-task-{}", thread.id).into(),
            ))
            .flex()
            .flex_col()
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            // An ungrouped row must leave the drag payload to its enclosing
            // ungrouped section: GPUI consumes it before invoking on_drop.
            .when_some(group_id, |row, group_id| {
                row.drag_over::<OrganizationDrag>(move |s, _, _, _| s.bg(colors.bg_active))
                    .on_drop(cx.listener(move |this, drag: &OrganizationDrag, _, cx| {
                        if let DragKind::Thread(id) = &drag.kind
                            && id != &before
                        {
                            this.submit_organization(
                                SidebarOrganizationAction::MoveThread {
                                    thread_id: id.clone(),
                                    group_id: Some(group_id.clone()),
                                    before_id: Some(before.clone()),
                                },
                                cx,
                            );
                        }
                        cx.stop_propagation();
                    }))
            })
            .child(self.render_row(
                thread,
                &opened,
                archived,
                if show_project {
                    "thread-row-"
                } else {
                    "project-thread-row-"
                },
                actions_enabled,
                cx,
            ));
        if show_project
            && let Some(project) = self
                .organization
                .as_ref()
                .and_then(|o| o.snapshot.as_ref())
                .and_then(|s| s.projects.iter().find(|p| p.id == thread.project_id))
        {
            row = row.child(
                div()
                    .pl(px(Layout::SIDEBAR_NAV_CONTENT_INSET))
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child(project.name.clone()),
            );
        }
        row.into_any_element()
    }

    /// Project and timeline projections intentionally use a compact, passive
    /// row. The full session section owns drag and action-menu interactions;
    /// duplicating those focusable controls in the second projection would
    /// create two hit targets for the same thread.
    pub(super) fn render_project_thread(
        &self,
        thread: &Thread,
        show_project: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let opened = cx.global::<OpenedThread>().0.as_ref().map(|t| t.id.clone());
        let selected = opened.as_deref() == Some(thread.id.as_str());
        let thread_id = thread.id.clone();
        let mut row = div()
            .id(ElementId::Name(
                format!("project-thread-row-{thread_id}").into(),
            ))
            .debug_selector({
                let id = thread_id.clone();
                move || format!("project-thread-row-{id}")
            })
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .flex()
            .items_center()
            .gap_1()
            .rounded_lg()
            // R48: no leading icon, so the row uses the ladder's text column
            // directly (base + SIDEBAR_ROW_INSET).
            .pl(px(Layout::SIDEBAR_ROW_INSET))
            .pr_3()
            .cursor_pointer()
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.text_primary)
            .when(selected, |row| row.bg(colors.bg_active))
            .hover(move |style| {
                style.bg(if selected {
                    colors.bg_active
                } else {
                    colors.bg_hover
                })
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.open_thread(&thread_id, cx)),
            )
            .children(thread.pinned.then(|| {
                div()
                    .flex_shrink_0()
                    .child(crate::icons::icon(crate::icons::Icon::Pin, colors.accent))
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(thread.unread, |title| {
                        title.font_weight(Typography::HEADING_CARD_WEIGHT)
                    })
                    .child(thread_title(thread)),
            )
            .children(
                thread
                    .unread
                    .then(|| div().size(px(6.)).rounded_full().bg(colors.accent)),
            );
        if show_project
            && let Some(project) = self
                .organization
                .as_ref()
                .and_then(|org| org.snapshot.as_ref())
                .and_then(|snapshot| snapshot.projects.iter().find(|p| p.id == thread.project_id))
        {
            row = row.child(
                div()
                    .w(px(Layout::SIDEBAR_PROJECT_METADATA_WIDTH))
                    .flex_shrink_0()
                    .truncate()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child(project.name.clone()),
            );
        }
        row.into_any_element()
    }
}
fn task_time(thread: &Thread, sort: SidebarTaskSort) -> i64 {
    match sort {
        SidebarTaskSort::Updated => thread.updated_at,
        SidebarTaskSort::Created => thread.created_at,
    }
}
pub(super) fn sorted_threads(threads: &[Thread], sort: SidebarTaskSort) -> Vec<Thread> {
    let mut threads = threads.to_vec();
    threads.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| task_time(b, sort).cmp(&task_time(a, sort)))
            .then_with(|| a.id.cmp(&b.id))
    });
    threads
}
fn group_color(color: SidebarGroupColor, colors: &ThemeColors) -> gpui_kit::Rgba {
    colors.sidebar_group_colors[match color {
        SidebarGroupColor::Gray => 0,
        SidebarGroupColor::Red => 1,
        SidebarGroupColor::Orange => 2,
        SidebarGroupColor::Yellow => 3,
        SidebarGroupColor::Green => 4,
        SidebarGroupColor::Blue => 5,
        SidebarGroupColor::Purple => 6,
    }]
}
