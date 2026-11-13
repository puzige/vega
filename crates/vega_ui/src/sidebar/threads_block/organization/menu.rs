use super::*;
use crate::sidebar::nsh_leaf::chrome::ScrollFadeExt;
impl ThreadsBlock {
    pub(super) fn organization_menu_items(&self) -> Vec<(String, MenuCommand)> {
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
            OrganizationMenu::Recents => {
                let mut preferences = snapshot.preferences.clone();
                preferences.hide_project_threads_from_recents =
                    !preferences.hide_project_threads_from_recents;
                items.push((
                    vega_i18n::t_args(
                        "sidebar.show_project_conversations_selected",
                        &[(
                            "selected",
                            if snapshot.preferences.hide_project_threads_from_recents {
                                "  "
                            } else {
                                "✓"
                            },
                        )],
                    ),
                    MenuCommand::Apply(SidebarOrganizationAction::SetPreferences(preferences)),
                ));
            }
            OrganizationMenu::Group(id) => {
                items.push((
                    vega_i18n::t("ui.new_task").into(),
                    MenuCommand::NewTask(id.clone()),
                ));
                items.push((
                    vega_i18n::t("ui.rename").into(),
                    MenuCommand::Rename(id.clone()),
                ));
                for (color, label) in [
                    (SidebarGroupColor::Gray, vega_i18n::t("ui.gray")),
                    (SidebarGroupColor::Red, vega_i18n::t("ui.red")),
                    (SidebarGroupColor::Orange, vega_i18n::t("ui.orange")),
                    (SidebarGroupColor::Yellow, vega_i18n::t("ui.yellow")),
                    (SidebarGroupColor::Green, vega_i18n::t("ui.green")),
                    (SidebarGroupColor::Blue, vega_i18n::t("ui.blue")),
                    (SidebarGroupColor::Purple, vega_i18n::t("ui.purple")),
                ] {
                    let selected = snapshot
                        .groups
                        .iter()
                        .find(|g| &g.id == id)
                        .is_some_and(|g| g.color == color);
                    items.push((
                        vega_i18n::t_args(
                            "ui.selected_change_color_label",
                            &[
                                ("selected", if selected { "✓" } else { "  " }),
                                ("label", label),
                            ],
                        ),
                        MenuCommand::Apply(SidebarOrganizationAction::SetGroupColor {
                            group_id: id.clone(),
                            color,
                        }),
                    ));
                }
                if let Some(index) = snapshot.groups.iter().position(|g| &g.id == id) {
                    if index > 0 {
                        items.push((
                            vega_i18n::t("ui.move_group_up").into(),
                            MenuCommand::Apply(SidebarOrganizationAction::MoveGroup {
                                group_id: id.clone(),
                                before_id: Some(snapshot.groups[index - 1].id.clone()),
                            }),
                        ));
                    }
                    if index + 1 < snapshot.groups.len() {
                        items.push((
                            vega_i18n::t("ui.move_group_down").into(),
                            MenuCommand::Apply(SidebarOrganizationAction::MoveGroup {
                                group_id: id.clone(),
                                before_id: snapshot.groups.get(index + 2).map(|g| g.id.clone()),
                            }),
                        ));
                    }
                }
                items.push((
                    vega_i18n::t("ui.ungroup_and_delete").into(),
                    MenuCommand::Apply(SidebarOrganizationAction::DissolveGroup {
                        group_id: id.clone(),
                    }),
                ));
            }
            OrganizationMenu::Project(id) => {
                items.push((
                    vega_i18n::t("ui.remove_project").into(),
                    MenuCommand::RemoveProject(id.clone()),
                ));
                items.extend(
                    self.reorder_menu_actions(&ReorderItem::Project(id.clone()))
                        .into_iter()
                        .map(|(label, action)| (label, MenuCommand::Apply(action))),
                );
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
            org.menu_anchor = None;
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
        }
        cx.notify();
    }
    pub(super) fn render_organization_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(org) = &self.organization else {
            return div().into_any_element();
        };
        let Some((position, anchor)) = org.menu_anchor else {
            return div().into_any_element();
        };
        let items = self.organization_menu_items();
        let title = match org.menu {
            Some(OrganizationMenu::Recents) => vega_i18n::t("sidebar.recents"),
            Some(OrganizationMenu::Group(_)) => vega_i18n::t("ui.group_actions"),
            _ => vega_i18n::t("ui.project_actions"),
        };
        let menu = div()
            .id("organization-menu")
            .track_focus(&org.menu_focus)
            .occlude()
            .w(px(Layout::TASK_MENU_WIDTH))
            .max_h(self.menu_height)
            .track_scroll(&org.menu_scroll)
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .p(px(5.))
            .rounded(px(Layout::MENU_RADIUS))
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
                        org.menu_anchor = None;
                    }
                    cx.notify();
                }),
            )
            .sidebar_scroll_fade(
                format!("organization-menu-fade-{}", org.presentation_generation),
                org.menu_scroll.clone(),
                true,
            )
            // VegaWindow binds Escape to CloseSettings before raw key_down.
            // Handle the action in this focused menu so it cannot reach the route.
            .on_action(cx.listener(|this, _: &CloseSettings, _, cx| {
                if let Some(org) = this.organization.as_mut() {
                    org.menu = None;
                    org.menu_anchor = None;
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
                        "escape" => {
                            org.menu = None;
                            org.menu_anchor = None;
                        }
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
            .child({
                let observed = org.snapshot.clone();
                let generation = org.presentation_generation;
                let observed_menu = org.menu.clone();
                let weak = cx.weak_entity();
                let target = weak.clone();
                let items = items
                    .into_iter()
                    .map(|(label, command)| {
                        let mut item = nsh_leaf::Item::action(label, None);
                        item.danger = matches!(
                            command,
                            MenuCommand::Apply(SidebarOrganizationAction::DissolveGroup { .. })
                        );
                        item
                    })
                    .collect::<Vec<_>>();
                nsh_leaf::menu(
                    format!("organization-menu-nsh-{generation}"),
                    Some(title.to_owned()),
                    items,
                    org.menu_scroll.clone(),
                    move |cx| {
                        weak.upgrade().is_some_and(|entity| {
                            entity.read(cx).organization.as_ref().is_some_and(|org| {
                                org.snapshot == observed
                                    && org.presentation_generation == generation
                                    && org.menu == observed_menu
                                    && org.pending != Some(PendingRequest::Write)
                                    && org.editor.is_none()
                            })
                        })
                    },
                    move |index, _, window, cx| {
                        let _ = target.update(cx, |this, cx| {
                            this.activate_organization_menu(index, window, cx)
                        });
                    },
                )
            });
        div()
            .absolute()
            .top_0()
            .right_0()
            .w_full()
            .h_full()
            .child(
                anchored()
                    .anchor(anchor)
                    .position_mode(AnchoredPositionMode::Window)
                    .position(position)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        deferred(nsh_leaf::chrome::popup(
                            format!("organization-menu-chrome-{}", org.presentation_generation),
                            menu.into_any_element(),
                        ))
                        .with_priority(3),
                    ),
            )
            .into_any_element()
    }
}
