use super::*;
impl ThreadsBlock {
    fn organization_menu_items(&self) -> Vec<(String, MenuCommand)> {
        let Some(org) = &self.organization else {
            return Vec::new();
        };
        let Some(snapshot) = &org.snapshot else {
            return Vec::new();
        };
        let Some(menu) = &org.menu else {
            return Vec::new();
        };
        let mut items = Vec::new();
        match menu {
            OrganizationMenu::Filter => {
                for (sort, label) in [
                    (SidebarTaskSort::Updated, "更新时间"),
                    (SidebarTaskSort::Created, "创建时间"),
                ] {
                    let mut preferences = snapshot.preferences.clone();
                    preferences.sort = sort;
                    items.push((
                        format!(
                            "{} {label}",
                            if snapshot.preferences.sort == sort {
                                "✓"
                            } else {
                                "  "
                            }
                        ),
                        MenuCommand::Apply(SidebarOrganizationAction::SetPreferences(preferences)),
                    ));
                }
                items.push((
                    format!(
                        "{} {}",
                        if org.archive { "✓" } else { "  " },
                        if org.archive {
                            "隐藏已归档"
                        } else {
                            "显示已归档"
                        },
                    ),
                    MenuCommand::ToggleArchive,
                ));
            }
            OrganizationMenu::Group(id) => {
                items.push(("新建任务".into(), MenuCommand::NewTask(id.clone())));
                items.push(("重命名".into(), MenuCommand::Rename(id.clone())));
                for (color, label) in [
                    (SidebarGroupColor::Gray, "灰"),
                    (SidebarGroupColor::Red, "红"),
                    (SidebarGroupColor::Orange, "橙"),
                    (SidebarGroupColor::Yellow, "黄"),
                    (SidebarGroupColor::Green, "绿"),
                    (SidebarGroupColor::Blue, "蓝"),
                    (SidebarGroupColor::Purple, "紫"),
                ] {
                    let selected = snapshot
                        .groups
                        .iter()
                        .find(|g| &g.id == id)
                        .is_some_and(|g| g.color == color);
                    items.push((
                        format!("{} 更改颜色 · {label}", if selected { "✓" } else { "  " }),
                        MenuCommand::Apply(SidebarOrganizationAction::SetGroupColor {
                            group_id: id.clone(),
                            color,
                        }),
                    ));
                }
                if let Some(index) = snapshot.groups.iter().position(|g| &g.id == id) {
                    if index > 0 {
                        items.push((
                            "分组上移".into(),
                            MenuCommand::Apply(SidebarOrganizationAction::MoveGroup {
                                group_id: id.clone(),
                                before_id: Some(snapshot.groups[index - 1].id.clone()),
                            }),
                        ));
                    }
                    if index + 1 < snapshot.groups.len() {
                        items.push((
                            "分组下移".into(),
                            MenuCommand::Apply(SidebarOrganizationAction::MoveGroup {
                                group_id: id.clone(),
                                before_id: snapshot.groups.get(index + 2).map(|g| g.id.clone()),
                            }),
                        ));
                    }
                }
                items.push((
                    "取消分组并删除".into(),
                    MenuCommand::Apply(SidebarOrganizationAction::DissolveGroup {
                        group_id: id.clone(),
                    }),
                ));
            }
            OrganizationMenu::Project(id) => {
                let mut ordered: Vec<_> = snapshot.project_order.clone();
                ordered.extend(
                    snapshot
                        .projects
                        .iter()
                        .filter(|p| !snapshot.project_order.contains(&p.id))
                        .map(|p| p.id.clone()),
                );
                if let Some(index) = ordered.iter().position(|p| p == id) {
                    if index > 0 {
                        items.push((
                            "项目上移".into(),
                            MenuCommand::Apply(SidebarOrganizationAction::MoveProject {
                                project_id: id.clone(),
                                before_id: Some(ordered[index - 1].clone()),
                            }),
                        ));
                    }
                    if index + 1 < ordered.len() {
                        items.push((
                            "项目下移".into(),
                            MenuCommand::Apply(SidebarOrganizationAction::MoveProject {
                                project_id: id.clone(),
                                before_id: ordered.get(index + 2).cloned(),
                            }),
                        ));
                    }
                }
                items.push((
                    "移除项目（保留文件）".into(),
                    MenuCommand::RemoveProject(id.clone()),
                ));
            }
        }
        items
    }
    fn activate_organization_menu(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, command)) = self.organization_menu_items().get(index).cloned() else {
            return;
        };
        if let Some(org) = self.organization.as_mut() {
            org.menu = None;
        }
        match command {
            MenuCommand::Apply(action) => self.submit_organization(action, cx),
            MenuCommand::Rename(id) => self.edit_group(Some(id), window, cx),
            MenuCommand::NewTask(id) => self.new_group_task(id, cx),
            MenuCommand::RemoveProject(id) => {
                if crate::navigation::allow_task_navigation(None, cx)
                    && let Some(org) = &self.organization
                {
                    org.projects.update(cx, |p, cx| p.remove_project(&id, cx));
                }
            }
            MenuCommand::ToggleArchive => {
                if let Some(org) = self.organization.as_mut() {
                    org.archive = !org.archive;
                }
            }
        }
        cx.notify();
    }
    pub(super) fn render_organization_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let Some(org) = &self.organization else {
            return div().into_any_element();
        };
        let items = self.organization_menu_items();
        let title = match org.menu {
            Some(OrganizationMenu::Filter) => "排序与归档",
            Some(OrganizationMenu::Group(_)) => "分组操作",
            _ => "项目操作",
        };
        let menu = div()
            .id("organization-menu")
            .debug_selector(|| "organization-menu".into())
            .track_focus(&org.menu_focus)
            .occlude()
            .w(px(Layout::TASK_MENU_WIDTH))
            .max_h(self.menu_height)
            .track_scroll(&org.menu_scroll)
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .shadow_md()
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(org) = this.organization.as_mut() {
                        org.menu = None;
                    }
                    cx.notify();
                }),
            )
            // VegaWindow binds Escape to CloseSettings before raw key_down.
            // Handle the action in this focused menu so it cannot reach the route.
            .on_action(cx.listener(|this, _: &CloseSettings, _, cx| {
                if let Some(org) = this.organization.as_mut() {
                    org.menu = None;
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    let count = this.organization_menu_items().len();
                    if count == 0 {
                        return;
                    }
                    let Some(org) = this.organization.as_mut() else {
                        return;
                    };
                    match event.keystroke.key.as_str() {
                        "up" => org.menu_index = (org.menu_index + count - 1) % count,
                        "down" => org.menu_index = (org.menu_index + 1) % count,
                        "tab" => {
                            org.menu_index = if event.keystroke.modifiers.shift {
                                (org.menu_index + count - 1) % count
                            } else {
                                (org.menu_index + 1) % count
                            }
                        }
                        "escape" => org.menu = None,
                        "enter" | "space" => {
                            let index = org.menu_index;
                            this.activate_organization_menu(index, window, cx);
                        }
                        _ => return,
                    }
                    if let Some(org) = this.organization.as_ref() {
                        org.menu_scroll.scroll_to_item(org.menu_index + 1);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child(title),
            )
            .children(items.into_iter().enumerate().map(|(index, (label, _))| {
                div()
                    .id(ElementId::Name(format!("organization-menu-{index}").into()))
                    .debug_selector(move || format!("organization-menu-{index}"))
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px_2()
                    .rounded_md()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if label.contains("取消分组并删除") {
                        colors.danger
                    } else {
                        colors.text_primary
                    })
                    .when(org.menu_index == index, |s| s.bg(colors.bg_active))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.activate_organization_menu(index, window, cx);
                        }),
                    )
                    .child(label)
            }));
        div()
            .absolute()
            .top_0()
            .right_0()
            .w_full()
            .h_full()
            .child(
                anchored()
                    .anchor(Anchor::TopRight)
                    .position_mode(AnchoredPositionMode::Local)
                    .position(point(
                        px(Layout::SIDEBAR_WIDTH - 16.),
                        px(Typography::SIDEBAR_LINE_HEIGHT),
                    ))
                    .snap_to_window_with_margin(px(8.))
                    .child(deferred(menu).with_priority(3)),
            )
            .into_any_element()
    }
}
