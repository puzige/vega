use super::*;
use crate::menu_list;
use crate::sidebar::{SelectedProject, VegaStore};
use crate::text_input::TextInput;

const UTILITY_CHIP_LEFT_INSET: f32 = Layout::CONTENT_PADDING
    + Layout::COMPOSER_UTILITY_BAR_INSET
    + Layout::COMPOSER_UTILITY_CHIP_INSET;

const PROJECT_MENU_VIEWPORT_MARGIN: f32 = 8.0;

fn project_menu_width(viewport: Pixels) -> Pixels {
    let room = viewport - px(UTILITY_CHIP_LEFT_INSET + PROJECT_MENU_VIEWPORT_MARGIN);
    px(Layout::MENU_MAX_WIDTH).min(room.max(px(1.0)))
}

impl ConversationStream {
    /// Whether the utility bar renders: a window-owned draft can choose its
    /// project here, while the older committed empty-session route still
    /// requires its binding to match the shared selection.
    ///
    /// This is a real render-visibility predicate. The bar is never mounted
    /// hidden and never occupies zero height.
    pub(crate) fn utility_bar_visible(&self, cx: &App) -> bool {
        if !self.entries.is_empty() {
            return false;
        }
        if self.draft_route {
            return true;
        }
        let Some(binding) = self.thread.project_binding() else {
            return false;
        };
        cx.try_global::<SelectedProject>()
            .and_then(|selected| selected.0.as_deref())
            == Some(binding)
    }

    pub(crate) fn render_composer_utility_bar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .flex()
            .child(
                div()
                    .flex_1()
                    .mx(px(Layout::COMPOSER_UTILITY_BAR_INSET))
                    .h(px(Layout::COMPOSER_UTILITY_BAR_HEIGHT))
                    .flex()
                    .items_center()
                    .gap(px(Layout::COMPOSER_UTILITY_CHIP_GAP))
                    .pl(px(Layout::COMPOSER_UTILITY_CHIP_INSET))
                    .bg(colors.bg_sidebar)
                    .rounded_t(px(Layout::COMPOSER_UTILITY_BAR_RADIUS))
                    .child(self.render_utility_project_chip(window, cx))
                    .when(!self.thread.is_standalone(), |bar| {
                        bar.child(self.render_utility_branch_chip())
                    })
                    .child(self.render_utility_backend_chip(cx)),
            )
            .into_any_element()
    }

    fn render_utility_backend_chip(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let backend = self.thread.backend;
        let draft = self.draft_route && self.entries.is_empty();
        let label = match backend {
            vega_conversation::types::TaskBackend::Native => "Vega Native",
            vega_conversation::types::TaskBackend::Codex => "Codex ACP",
        };
        div()
            .relative()
            .flex()
            .items_center()
            .child(
                div()
                    .id("composer-utility-backend")
                    .h(px(Layout::COMPOSER_UTILITY_CHIP_HEIGHT))
                    .px(px(Layout::COMPOSER_UTILITY_CHIP_PADDING_X))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_primary)
                    .when(draft, |chip| {
                        chip.cursor_pointer()
                            .hover(move |style| {
                                style.bg(colors.bg_utility_chip_overlay).rounded_full()
                            })
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let open = !this.backend_selector_open;
                                    this.close_composer_popovers(cx);
                                    this.backend_selector_open = open;
                                    cx.notify();
                                }),
                            )
                    })
                    .child(label),
            )
            .when(draft && self.backend_selector_open, |chip| {
                chip.child(self.render_backend_menu(cx))
            })
            .into_any_element()
    }

    fn render_backend_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let options = [
            (vega_conversation::types::TaskBackend::Native, "Vega Native"),
            (vega_conversation::types::TaskBackend::Codex, "Codex ACP"),
        ];
        let mut menu = div()
            .id("composer-utility-backend-menu")
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                if this.backend_selector_open {
                    this.backend_selector_open = false;
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .bottom(gpui_kit::relative(1.0))
            .left_0()
            .w(px(260.0))
            .occlude()
            .flex()
            .flex_col()
            .py_1()
            .rounded(px(Layout::MENU_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .text_color(colors.text_primary)
            .shadow_sm();
        for (index, (backend, label)) in options.into_iter().enumerate() {
            let selected = self.thread.backend == backend;
            menu = menu.child(
                menu_list::row_container(
                    ("composer-utility-backend-option", index as u32),
                    selected,
                    true,
                    colors,
                )
                .text_color(colors.text_primary)
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                        this.request_task_backend(backend, cx);
                    }),
                )
                .child(div().flex_1().min_w_0().child(label)),
            );
        }
        gpui_kit::deferred(menu).with_priority(2).into_any_element()
    }

    fn render_utility_project_chip(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let label = if self.thread.is_standalone() {
            vega_i18n::t("ui.select_project").to_string()
        } else if self.project_label.is_empty() {
            vega_i18n::t("sidebar.projects").to_string()
        } else {
            self.project_label.clone()
        };
        div()
            .relative()
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .flex()
            .items_center()
            .child(
                div()
                    .id("composer-utility-project")
                    .capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
                    .h(px(Layout::COMPOSER_UTILITY_CHIP_HEIGHT))
                    .px(px(Layout::COMPOSER_UTILITY_CHIP_PADDING_X))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_primary)
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.bg_utility_chip_overlay).rounded_full())
                    .when(self.utility_projects_open, |chip| {
                        chip.bg(colors.bg_utility_chip_overlay).rounded_full()
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(Self::toggle_utility_projects),
                    )
                    .tooltip(move |_, cx| {
                        crate::icons::tooltip(vega_i18n::t("ui.switch_project"), cx)
                    })
                    .child(div().flex_shrink_0().child(crate::icons::icon(
                        crate::icons::Icon::Folder,
                        colors.text_secondary,
                    )))
                    .child(div().min_w_0().max_w(px(180.0)).truncate().child(label)),
            )
            .when(self.utility_projects_open, |chip| {
                chip.child(self.render_utility_projects_menu(window, cx))
            })
            .into_any_element()
    }

    fn render_utility_projects_menu(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let selected = cx.global::<SelectedProject>().0.clone();
        let query = self.utility_project_query.trim().to_lowercase();
        let visible: Vec<&(String, String)> = self
            .utility_projects
            .iter()
            .filter(|(_, name)| query.is_empty() || name.to_lowercase().contains(&query))
            .collect();
        let rows = visible
            .iter()
            .enumerate()
            .map(|(index, (project_id, name))| {
                let is_selected = selected.as_deref() == Some(project_id.as_str());
                let activate_id = project_id.clone();
                menu_list::row_container(
                    ("composer-utility-project-row", index),
                    is_selected,
                    true,
                    colors,
                )
                .text_color(if is_selected {
                    colors.text_primary
                } else {
                    colors.text_secondary
                })
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseUpEvent, window, cx| {
                        this.select_utility_project(&activate_id, window, cx);
                    }),
                )
                .child(crate::icons::icon(
                    crate::icons::Icon::Folder,
                    colors.text_secondary,
                ))
                .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                .child(menu_list::selection_marker(is_selected, colors))
            });
        let mut menu = div()
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                if this.utility_projects_open {
                    this.utility_projects_open = false;
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .bottom(gpui_kit::relative(1.0))
            .left_0()
            .w(project_menu_width(window.viewport_size().width))
            .occlude()
            .flex()
            .flex_col()
            .py_1()
            .rounded(px(Layout::MENU_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .text_color(colors.text_primary)
            .shadow_sm()
            .child(menu_list::search_field(
                &self.utility_project_search,
                colors,
            ));
        if visible.is_empty() {
            menu = menu.child(
                div()
                    .h(px(menu_list::MENU_ROW_HEIGHT))
                    .px(px(menu_list::MENU_ROW_PADDING_X))
                    .flex()
                    .items_center()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_tertiary)
                    .child(if self.utility_projects.is_empty() {
                        vega_i18n::t("ui.click_to_add_a_folder")
                    } else {
                        vega_i18n::t("ui.no_matching_projects")
                    }),
            );
        } else {
            menu = menu.children(rows);
        }

        gpui_kit::deferred(
            menu.child(menu_list::separator(colors))
                .child(menu_list::action_row(
                    "composer-utility-project-new",
                    crate::icons::Icon::Plus,
                    vega_i18n::t("ui.new_project").into(),
                    false,
                    vega_i18n::t("ui.not_available_register_a_folder_with_add_project"),
                    colors,
                    |_, _, _| {},
                ))
                .child(menu_list::action_row(
                    "composer-utility-project-detach",
                    crate::icons::Icon::Close,
                    vega_i18n::t("ui.no_project").into(),
                    true,
                    "",
                    colors,
                    cx.listener(|this, _: &MouseUpEvent, window, cx| {
                        this.detach_utility_project(window, cx)
                    }),
                )),
        )
        .with_priority(2)
        .into_any_element()
    }

    fn render_utility_branch_chip(&self) -> AnyElement {
        div()
            .id("composer-utility-branch")
            .flex()
            .items_center()
            .tooltip(|_, cx| crate::icons::tooltip(vega_i18n::t("ui.switch_branch"), cx))
            .child(self.branch_selector.clone())
            .into_any_element()
    }

    pub(crate) fn toggle_utility_projects(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = !self.utility_projects_open;
        // C2: project opening is the sibling boundary in the opposite
        // direction. Use the selector's normal close path so the controller
        // receives `BranchSelectorClosed` and owns any pending cleanup; do
        // not fold the branch entity into `close_composer_popovers`, because
        // that helper is also called by the branch-open subscription above.
        if open {
            self.branch_selector.update(cx, |selector, cx| {
                let _ = selector.request_close(cx);
            });
        }
        self.close_composer_popovers(cx);
        self.utility_projects_open = open;
        if open {
            self.utility_projects = Self::load_utility_projects(cx);
            self.utility_project_query.clear();
            self.utility_project_search
                .update(cx, |input, cx| input.clear(cx));
        }
        cx.notify();
    }

    pub(crate) fn sync_utility_project_query(
        &mut self,
        input: &Entity<TextInput>,
        cx: &mut Context<Self>,
    ) {
        let query = input.read(cx).text().to_owned();
        if self.utility_project_query != query {
            self.utility_project_query = query;
            cx.notify();
        }
    }

    /// Reads the sidebar's project rows. A missing or failing store degrades
    /// to an empty list (the menu then shows the sidebar's own empty-state
    /// copy) instead of failing the click.
    fn load_utility_projects(cx: &App) -> Vec<(String, String)> {
        let Some(VegaStore(Ok(store))) = cx.try_global::<VegaStore>() else {
            return Vec::new();
        };
        let sort = vega_store::projects::ProjectSort::RecentlyOpened;
        match vega_store::projects::list(store.conn(), sort) {
            Ok(projects) => projects
                .into_iter()
                .map(|project| (project.id, project.name))
                .collect(),
            Err(error) => {
                tracing::error!(%error, "failed to read projects for the composer utility bar");
                Vec::new()
            }
        }
    }

    /// Applies a project choice from the folder chip's menu: the shared
    /// selection global is rewritten and every window repaints.
    fn select_utility_project(
        &mut self,
        project_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.set_global(SelectedProject(Some(project_id.to_string())));
        self.utility_projects_open = false;
        self.focus_composer(window, cx);
        cx.refresh_windows();
        cx.notify();
    }

    fn detach_utility_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.set_global(SelectedProject(None));
        self.utility_projects_open = false;
        self.focus_composer(window, cx);
        cx.refresh_windows();
        cx.notify();
    }
}
