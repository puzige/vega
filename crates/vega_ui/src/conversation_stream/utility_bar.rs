//! R49 composer utility bar: the two-chip strip (folder / branch) that sits
//! directly above the composer card on the new-task page.
//!
//! The bar is a new-task-page affordance only — it renders on an empty draft
//! even before a project is chosen, or on the existing matching-project
//! empty-session route. It reuses the existing
//! data sources: the composer's own project label (projected from
//! `sidebar.project_label` by the app layer) and the sidebar's project rows
//! (`vega_store::projects`), so no second project pipeline exists.

use super::*;
use crate::menu_list;
use crate::sidebar::{SelectedProject, VegaStore};
use crate::text_input::TextInput;

/// R68 R14: the horizontal room the composer's own chrome takes up before the
/// project chip's **left** edge — the composer column's content padding, the
/// utility bar's own inset inside that column, and the chip's inset inside the
/// bar. All three are frozen [`Layout`] constants, so the sum is exact in the
/// one case that matters: a window too narrow for the column to be centred
/// (below `COMPOSER_MAX_WIDTH + 2 * CONTENT_PADDING`) makes the column fill the
/// window and the chip really does start here. On a wider window the centred
/// column pushes the chip further right, so subtracting this sum under-states
/// the room the popup has — a conservative bound, never an optimistic one.
const UTILITY_CHIP_LEFT_INSET: f32 = Layout::CONTENT_PADDING
    + Layout::COMPOSER_UTILITY_BAR_INSET
    + Layout::COMPOSER_UTILITY_CHIP_INSET;

/// R68 R14: the gap the popup keeps from the window's right edge when the
/// viewport is what bounds it.
const PROJECT_MENU_VIEWPORT_MARGIN: f32 = 8.0;

/// R68 R13/R14: the project popup's width.
///
/// R13: it is [`Layout::MENU_MAX_WIDTH`] (350) and **not** a function of the
/// chip's width. Before R68 the menu carried `.w(350).max_w_full()` and its
/// containing block was the folder chip, whose width follows the project name;
/// `max_w_full` therefore clamped the popup to the chip (measured 40px in the
/// harness, ~183px natively), which is what truncated every project name
/// (`r13-alp…`). The reference implementation's `contentWidth` map gives the
/// workspace dropdown `min-w-[260px]`, and the Codex screenshot measures 261
/// logical px, so a fixed 350 is the right shape here — Vega's own model list
/// uses the same constant.
///
/// R14: it still never overflows the window. When the viewport cannot host
/// 350px to the right of the chip, the popup shrinks to the room that is left
/// rather than running off the edge.
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

    pub(crate) fn composer_branch_entry_visible(&self) -> bool {
        !self.draft_route && !self.entries.is_empty() && self.thread.project_binding().is_some()
    }

    /// The utility bar layer above the composer card. It is
    /// [`Layout::COMPOSER_UTILITY_BAR_INSET`] narrower than the card on each
    /// side and centered on the card's axis; its bottom edge is flush against
    /// the card's top edge. The parent stacks bar then card (zero overlap, no
    /// negative margin), which is what makes the bar read as one layer tucked
    /// under the card.
    ///
    /// R68 R14: `window` is threaded through only so the project popup can
    /// bound itself against the viewport (see [`project_menu_width`]); the
    /// bar's own geometry reads no window state.
    pub(crate) fn render_composer_utility_bar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                    .child(self.render_utility_project_chip(window, cx))
                    .when(!self.thread.is_standalone(), |bar| {
                        bar.child(self.render_utility_branch_chip())
                    }),
            )
            .into_any_element()
    }

    /// The folder chip: 16px folder icon + project name, with a transparent
    /// rest state and the shared R2 overlay while hovered or open. Clicking
    /// opens the project menu above the chip.
    ///
    /// R68 R3: the chip claims the mouse-down in the **capture** phase. The
    /// popup's outside-click handler (R1) also runs in capture, and the chip's
    /// own open/close toggle runs on mouse-**up**; a bubble-phase
    /// `stop_propagation` would therefore run *after* the popup had already
    /// closed itself, and the subsequent mouse-up would immediately reopen it,
    /// making the chip unable to close its own popup. Declaring the gesture in
    /// capture is what makes "click the chip again" close (R68 §2).
    fn render_utility_project_chip(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let label = if self.thread.is_standalone() {
            "选择项目".to_string()
        } else if self.project_label.is_empty() {
            "项目".to_string()
        } else {
            self.project_label.clone()
        };
        div()
            // R68/R64: keep the popup's legacy 32px anchor box while the
            // interactive chip itself is the new 28px capsule.  Centering the
            // capsule in this unchanged box preserves the frozen menu bounds
            // and its outside-click geometry.
            .relative()
            .h(px(Typography::SIDEBAR_LINE_HEIGHT))
            .flex()
            .items_center()
            .child(
                div()
                    .id("composer-utility-project")
                    .debug_selector(|| "composer-utility-project-chip".into())
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
                    .tooltip(move |_, cx| crate::icons::tooltip("切换项目", cx))
                    .child(
                        div()
                            .debug_selector(|| "composer-utility-project-icon".into())
                            .flex_shrink_0()
                            .child(crate::icons::icon(
                                crate::icons::Icon::Folder,
                                colors.text_secondary,
                            )),
                    )
                    .child(div().min_w_0().max_w(px(180.0)).truncate().child(label)),
            )
            .when(self.utility_projects_open, |chip| {
                chip.child(self.render_utility_projects_menu(window, cx))
            })
            .into_any_element()
    }

    /// The lightweight project menu above the folder chip (R49 §2.5): the same
    /// rows the sidebar's project block lists, projected from the same store
    /// query. Selection writes the shared [`SelectedProject`] global and
    /// refreshes the windows. The window root rebinds only its unmaterialized
    /// draft; a durable task keeps its stored project identity.
    ///
    /// The popup opens **upward**. The composer sits at the window's bottom
    /// edge, so a downward popup would leave the viewport; this is the minimal
    /// composer-context adjustment the R49 residual notes anticipated.
    ///
    /// R62 R10 adds the list structure the reference implementation has and
    /// Vega lacked: a search field on top, a folder icon per row, a checkmark
    /// plus a light-grey rounded surface on the current selection, a separator
    /// before the trailing actions, and the two action rows (`+ 新建项目` /
    /// `× 不关联项目`).
    ///
    /// R62 R11 keeps the semantics: filtering only hides rows from this frame;
    /// clicking any visible row still writes the same `SelectedProject` and
    /// refreshes, exactly as before. `不关联项目` is a real action — it writes
    /// the same global with `None`, which is the state the sidebar's own
    /// project-removal path already produces. `新建项目` has no Vega
    /// implementation reachable from this surface (folder registration lives in
    /// `ProjectsBlock::open_picker`, which owns its worker and its own error
    /// handling), so it renders **disabled** rather than pretending to work;
    /// this is reported as the R62 M7 finding.
    ///
    /// R68 R1/R2/R4: the popup closes when the pointer goes down **outside**
    /// it. `on_mouse_down_out` fires in the capture phase and only when the
    /// pointer is outside the element's own bounds, so a click anywhere on the
    /// popup — the search field, a row, the trailing actions — leaves it open
    /// (R4) and the handler never has to re-derive "inside" itself. The close
    /// goes through this popup's existing path: `utility_projects_open = false`
    /// plus `cx.notify()` (R2), exactly what the row-selection and detach
    /// handlers already do. R5: it touches nothing but this popup's own flag —
    /// the branch popup's state lives in `BranchSelector`, not here.
    ///
    /// R68 R13/R14: the width is [`Layout::MENU_MAX_WIDTH`], bounded by the
    /// viewport but **not** by the chip (see [`project_menu_width`]). The
    /// `left_0()` anchoring, the `bottom(relative(1.0))` relationship and the
    /// R64 `deferred` wrap below are untouched (R15).
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
                let selector_id = project_id.clone();
                let check_id = format!("composer-utility-project-row-{project_id}-check");
                menu_list::row_container(
                    ("composer-utility-project-row", index),
                    is_selected,
                    true,
                    colors,
                )
                .debug_selector(move || format!("composer-utility-project-row-{selector_id}"))
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
                .child(menu_list::selection_marker(
                    is_selected,
                    move || check_id.clone(),
                    colors,
                ))
            });
        let mut menu = div()
            .debug_selector(|| "composer-utility-project-menu".into())
            // R68 R1/R4: the outside-click close runs in **capture** and only
            // when the pointer is outside the popup's bounds.
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                if this.utility_projects_open {
                    this.utility_projects_open = false;
                    cx.notify();
                }
            }))
            // The pre-R68 bubble-phase claim is kept: a click **on** the popup
            // must not reach the composer's own outside-click handler behind
            // it. It is not the dismissal mechanism (R1) and it cannot block
            // the capture handler above — different phase, and gated on the
            // pointer being inside.
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
                "composer-utility-project-search",
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
                        "点击 [+] 添加文件夹"
                    } else {
                        "没有匹配项目"
                    }),
            );
        } else {
            menu = menu.children(rows);
        }
        // R64 R1/R3: deferred paint, priority 2 — the composer card's border is
        // painted after its children (`Style::paint`), so a popup mounted inside
        // the card is crossed by it. `deferred` keeps this layer's layout in the
        // current tree and moves only its painting after the ancestors.
        gpui_kit::deferred(
            menu.child(menu_list::separator(colors))
                .child(menu_list::action_row(
                    "composer-utility-project-new",
                    crate::icons::Icon::Plus,
                    "新建项目".into(),
                    // R62 R11/M7: no Vega implementation path exists from this
                    // surface, so the row renders disabled instead of silently
                    // doing nothing.
                    false,
                    "暂不支持：请在侧栏使用 [+ 添加项目] 注册文件夹",
                    colors,
                    |_, _, _| {},
                ))
                .child(menu_list::action_row(
                    "composer-utility-project-detach",
                    crate::icons::Icon::Close,
                    "不关联项目".into(),
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

    pub(crate) fn render_composer_branch_entry(&self) -> AnyElement {
        div()
            .id("composer-footer-branch")
            .debug_selector(|| "composer-footer-branch-chip".into())
            .flex()
            .items_center()
            .tooltip(|_, cx| crate::icons::tooltip("切换分支", cx))
            .child(self.branch_selector.clone())
            .into_any_element()
    }

    /// Opens/closes the folder chip's project menu. The list is read through
    /// the same store query the sidebar's project block already uses; it runs
    /// in this handler, never during a frame.
    ///
    /// R62 R10: each open starts from an empty filter, so the field never
    /// hides the list the chip just claimed to show.
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

    /// R62 R10: mirrors the visible filter query into the menu's own state.
    /// It only decides which rows this frame lists — the selection contract is
    /// untouched (R62 R11).
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

    /// R62 R10: the menu's `不关联项目` row. It writes the same shared
    /// selection global with `None` — the exact state the sidebar's
    /// project-removal path already produces — so no second deselect
    /// mechanism exists. A draft remains on the bar with `选择项目`; a durable
    /// empty session remains bound and follows the older R49 visibility fence.
    fn detach_utility_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.set_global(SelectedProject(None));
        self.utility_projects_open = false;
        self.focus_composer(window, cx);
        cx.refresh_windows();
        cx.notify();
    }
}
