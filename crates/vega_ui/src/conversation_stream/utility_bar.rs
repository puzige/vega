//! R49 composer utility bar: the two-chip strip (folder / branch) that sits
//! directly above the composer card on the new-task page.
//!
//! The bar is a new-task-page affordance only — it renders while the current
//! route still resolves a project context and the conversation has no messages
//! yet, exactly like Codex's `ComposerHomeUtilityBar`. It reuses the existing
//! data sources: the composer's own project label (projected from
//! `sidebar.project_label` by the app layer) and the sidebar's project rows
//! (`vega_store::projects`), so no second project pipeline exists.

use super::*;
use crate::sidebar::{SelectedProject, VegaStore};

impl ConversationStream {
    /// Whether the R49 utility bar renders at all: the route resolves a
    /// project context (the opened task is bound to the currently selected
    /// project) **and** the conversation has no messages yet.
    ///
    /// This is a real render-visibility predicate. The bar is never mounted
    /// hidden and never occupies zero height.
    pub(crate) fn utility_bar_visible(&self, cx: &App) -> bool {
        if !self.entries.is_empty() {
            return false;
        }
        let Some(binding) = self.thread.project_binding() else {
            return false;
        };
        cx.try_global::<SelectedProject>()
            .and_then(|selected| selected.0.as_deref())
            == Some(binding)
    }

    /// The utility bar layer above the composer card. It is
    /// [`Layout::COMPOSER_UTILITY_BAR_INSET`] narrower than the card on each
    /// side and centered on the card's axis; its bottom edge is flush against
    /// the card's top edge. The parent stacks bar then card (zero overlap, no
    /// negative margin), which is what makes the bar read as one layer tucked
    /// under the card.
    pub(crate) fn render_composer_utility_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            // Same width cap and centering as `composer-shell`; the bar itself
            // takes the card width minus the frozen inset on both sides.
            .debug_selector(|| "composer-utility-bar-row".into())
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .flex()
            .child(
                div()
                    .debug_selector(|| "composer-utility-bar".into())
                    .flex_1()
                    .mx(px(Layout::COMPOSER_UTILITY_BAR_INSET))
                    .h(px(Layout::COMPOSER_UTILITY_BAR_HEIGHT))
                    .flex()
                    .items_center()
                    .gap(px(Layout::COMPOSER_UTILITY_CHIP_GAP))
                    .pl(px(Layout::COMPOSER_UTILITY_CHIP_INSET))
                    .bg(colors.bg_sidebar)
                    .rounded_t(px(Layout::COMPOSER_UTILITY_BAR_RADIUS))
                    .child(self.render_utility_project_chip(cx))
                    .child(self.render_utility_branch_chip()),
            )
            .into_any_element()
    }

    /// The folder chip: 16px folder icon + project name, with no border, no
    /// background and no pill radius. Hover adds the shared `bg_hover`
    /// surface; clicking opens the project menu above the chip.
    fn render_utility_project_chip(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let label = if self.project_label.is_empty() {
            "项目".to_string()
        } else {
            self.project_label.clone()
        };
        div()
            .relative()
            .child(
                div()
                    .id("composer-utility-project")
                    .debug_selector(|| "composer-utility-project-chip".into())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_primary)
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.bg_hover).rounded_md())
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(Self::toggle_utility_projects),
                    )
                    .tooltip(move |_, cx| crate::icons::tooltip("切换项目", cx))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Folder,
                        colors.text_secondary,
                    ))
                    .child(div().min_w_0().max_w(px(180.0)).truncate().child(label)),
            )
            .when(self.utility_projects_open, |chip| {
                chip.child(self.render_utility_projects_menu(cx))
            })
            .into_any_element()
    }

    /// The lightweight project menu above the folder chip (R49 §2.5): the same
    /// rows the sidebar's project block lists, projected from the same store
    /// query. Selection writes the shared [`SelectedProject`] global and
    /// refreshes the windows; it never re-binds the durable task.
    ///
    /// The popup opens **upward**. The composer sits at the window's bottom
    /// edge, so a downward popup would leave the viewport; this is the minimal
    /// composer-context adjustment the R49 residual notes anticipated.
    fn render_utility_projects_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let selected = cx.global::<SelectedProject>().0.clone();
        let rows = self
            .utility_projects
            .iter()
            .enumerate()
            .map(|(index, (project_id, name))| {
                let is_selected = selected.as_deref() == Some(project_id.as_str());
                let activate_id = project_id.clone();
                let selector_id = project_id.clone();
                div()
                    .id(("composer-utility-project-row", index))
                    .debug_selector(move || format!("composer-utility-project-row-{selector_id}"))
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .px_2()
                    .flex()
                    .items_center()
                    .truncate()
                    .text_size(px(Typography::SIDEBAR))
                    .when(is_selected, |row| row.bg(colors.bg_active))
                    .text_color(if is_selected {
                        colors.brand_primary
                    } else {
                        colors.text_secondary
                    })
                    .cursor_pointer()
                    .when(!is_selected, move |row| {
                        row.hover(move |style| style.bg(colors.bg_hover))
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                            this.select_utility_project(&activate_id, cx);
                        }),
                    )
                    .child(name.clone())
            });
        div()
            .debug_selector(|| "composer-utility-project-menu".into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .bottom(gpui_kit::relative(1.0))
            .left_0()
            .w(px(Layout::MENU_MAX_WIDTH))
            .max_w_full()
            .occlude()
            .flex()
            .flex_col()
            .rounded(px(Layout::MENU_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .text_color(colors.text_primary)
            .shadow_sm()
            .children(rows)
            .when(self.utility_projects.is_empty(), |menu| {
                menu.child(
                    div()
                        .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                        .px_2()
                        .flex()
                        .items_center()
                        .text_size(px(Typography::SIDEBAR))
                        .text_color(colors.text_tertiary)
                        .child("点击 [+] 添加文件夹"),
                )
            })
            .into_any_element()
    }

    /// The branch chip. The selector entity owns its own trigger chrome (see
    /// [`crate::branch_selector::BranchSelector::set_chip_chrome`], enabled at
    /// construction because the composer is the selector's only mount point
    /// since R49 moved it out of the Environment card) and keeps hiding itself
    /// on non-Git folders.
    fn render_utility_branch_chip(&self) -> AnyElement {
        div()
            .id("composer-utility-branch")
            .debug_selector(|| "composer-utility-branch-chip".into())
            .flex()
            .items_center()
            .tooltip(|_, cx| crate::icons::tooltip("切换分支", cx))
            .child(self.branch_selector.clone())
            .into_any_element()
    }

    /// Opens/closes the folder chip's project menu. The list is read through
    /// the same store query the sidebar's project block already uses; it runs
    /// in this handler, never during a frame.
    pub(crate) fn toggle_utility_projects(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = !self.utility_projects_open;
        self.close_composer_popovers(cx);
        self.utility_projects_open = open;
        if open {
            self.utility_projects = Self::load_utility_projects(cx);
        }
        cx.notify();
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
    fn select_utility_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        cx.set_global(SelectedProject(Some(project_id.to_string())));
        self.utility_projects_open = false;
        cx.refresh_windows();
        cx.notify();
    }
}
