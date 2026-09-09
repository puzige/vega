use super::*;
use vega_ui::artifact_card::ArtifactCard;
use vega_ui::icons::{Icon, icon_button};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TabKey {
    Diff,
    Terminal(u64),
    File(u64),
    Artifact(ArtifactCardId),
}

struct TerminalTab {
    project_id: String,
    view: Entity<vega_ui::terminal::TerminalView>,
    bottom: bool,
}

#[derive(Default)]
pub(super) struct Workspace {
    route: Option<String>,
    project_route: Option<String>,
    terminals: std::collections::BTreeMap<u64, TerminalTab>,
    next_terminal: u64,
    files: std::collections::BTreeMap<u64, Entity<vega_ui::file_preview::FilePreview>>,
    next_file: u64,
    terminal_error: bool,
    pub(super) tabs: Vec<(TabKey, bool)>,
    selected: [Option<TabKey>; 2],
    tab_scroll: [ScrollHandle; 2],
    reveal_tabs: [bool; 2],
    window_size: Option<Size<Pixels>>,
    hidden: [bool; 2],
    menu: bool,
    menu_bottom: bool,
    menu_focus: Option<FocusHandle>,
    maximized: [bool; 2],
    width: Option<f32>,
    height: Option<f32>,
    dragging: Option<bool>,
    pub(super) composer_focus_pending: bool,
}

impl Workspace {
    pub(super) fn open(&mut self, key: TabKey) {
        let bottom = self
            .tabs
            .iter()
            .find(|(tab, _)| tab == &key)
            .is_some_and(|(_, bottom)| *bottom);
        if !self.tabs.iter().any(|(tab, _)| tab == &key) {
            self.tabs.push((key.clone(), bottom));
        }
        self.selected[usize::from(bottom)] = Some(key);
        self.reveal_tabs[usize::from(bottom)] = true;
        self.hidden[usize::from(bottom)] = false;
        self.menu = false;
    }

    pub(super) fn close(&mut self, key: &TabKey) {
        self.tabs.retain(|(tab, _)| tab != key);
        for index in 0..2 {
            if self.selected[index].as_ref() == Some(key) {
                self.reveal_tabs[index] = true;
                self.selected[index] = self
                    .tabs
                    .iter()
                    .find(|(_, bottom)| usize::from(*bottom) == index)
                    .map(|(tab, _)| tab.clone());
            }
        }
    }
}

impl VegaWindow {
    pub(super) fn sync_workspace_route(&mut self, cx: &App) {
        let route = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone());
        let project_route = cx.global::<vega_ui::sidebar::SelectedProject>().0.clone();
        if self.workspace.route != route || self.workspace.project_route != project_route {
            self.environment_overlay_open = false;
            let same_project = self.workspace.project_route == project_route;
            let terminals = std::mem::take(&mut self.workspace.terminals);
            let terminal_tabs: Vec<_> = terminals
                .iter()
                .filter(|(_, terminal)| Some(&terminal.project_id) == project_route.as_ref())
                .map(|(id, terminal)| (TabKey::Terminal(*id), terminal.bottom))
                .collect();
            let selected = std::array::from_fn(|index| {
                self.workspace.selected[index]
                    .as_ref()
                    .filter(|key| terminal_tabs.iter().any(|(tab, _)| tab == *key))
                    .cloned()
                    .or_else(|| {
                        terminal_tabs
                            .iter()
                            .rev()
                            .find(|(_, bottom)| usize::from(*bottom) == index)
                            .map(|(key, _)| key.clone())
                    })
            });
            self.workspace = Workspace {
                route,
                project_route,
                terminals,
                tabs: terminal_tabs,
                selected,
                reveal_tabs: [true; 2],
                next_terminal: self.workspace.next_terminal,
                width: self.workspace.width,
                height: self.workspace.height,
                hidden: if same_project {
                    self.workspace.hidden
                } else {
                    [false; 2]
                },
                ..Default::default()
            };
        }
        if self
            .diff_controller
            .active
            .as_ref()
            .is_some_and(|active| Self::diff_route_is_current(&active.identity, cx))
            && !self
                .workspace
                .tabs
                .iter()
                .any(|(key, _)| *key == TabKey::Diff)
        {
            self.workspace.open(TabKey::Diff);
        }
    }

    pub(super) fn workspace_restore_review(&mut self, cx: &mut Context<Self>) {
        let hidden = self.workspace.hidden;
        self.workspace_open_diff(cx);
        self.workspace.hidden = hidden;
    }

    /// Open an explicitly requested human terminal in the trusted selected project.
    pub(crate) fn workspace_open_terminal(
        &mut self,
        bottom: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_workspace_route(cx);
        let project_id = cx.global::<vega_ui::sidebar::SelectedProject>().0.clone();
        let target = project_id.clone().zip(self.file_backed_store_path(cx));
        let Some((project_id, database_path)) = target else {
            self.workspace.terminal_error = true;
            cx.notify();
            return;
        };
        if self.workspace.terminals.len() >= 8 {
            self.workspace.terminal_error = true;
            cx.notify();
            return;
        }
        self.workspace.next_terminal = self.workspace.next_terminal.saturating_add(1);
        let id = self.workspace.next_terminal;
        let view = cx.new(|cx| {
            vega_ui::terminal::TerminalView::for_target(
                vega_conversation::types::TerminalTarget::Project {
                    database_path,
                    project_id: project_id.clone(),
                },
                cx,
            )
        });
        self.workspace.terminals.insert(
            id,
            TerminalTab {
                project_id,
                view,
                bottom,
            },
        );
        self.workspace.tabs.push((TabKey::Terminal(id), bottom));
        self.workspace.open(TabKey::Terminal(id));
        self.workspace.terminal_error = false;
        self.workspace_focus(usize::from(bottom), window, cx);
        cx.notify();
    }

    /// Show a prevalidated read-only file projection supplied by the palette controller.
    pub(crate) fn workspace_open_file(
        &mut self,
        view: Entity<vega_ui::file_preview::FilePreview>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_workspace_route(cx);
        let title = view.read(cx).title().to_string();
        let id = self
            .workspace
            .files
            .iter()
            .find(|(_, old)| old.read(cx).title() == title)
            .map(|(id, _)| *id)
            .unwrap_or_else(|| {
                self.workspace.next_file = self.workspace.next_file.saturating_add(1);
                self.workspace.next_file
            });
        self.workspace.files.insert(id, view);
        self.workspace.open(TabKey::File(id));
        let index = self
            .workspace
            .tabs
            .iter()
            .find(|(key, _)| *key == TabKey::File(id))
            .map_or(0, |(_, bottom)| usize::from(*bottom));
        self.workspace_focus(index, window, cx);
        cx.notify();
    }

    /// Whether an existing artifact/file preview can be restored without fabricating content.
    pub(crate) fn workspace_has_preview(&self) -> bool {
        self.workspace
            .tabs
            .iter()
            .any(|(key, _)| matches!(key, TabKey::Artifact(_) | TabKey::File(_)))
    }

    /// Restore/hide an existing project terminal, or explicitly create its first one.
    pub(crate) fn workspace_toggle_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let terminal = self
            .workspace
            .tabs
            .iter()
            .rev()
            .find(|(key, _)| matches!(key, TabKey::Terminal(_)))
            .cloned();
        if let Some((key, bottom)) = terminal {
            let index = usize::from(bottom);
            if !self.workspace.hidden[index] && self.workspace.selected[index] == Some(key.clone())
            {
                self.workspace_hide(index, window, cx);
            } else {
                self.workspace.open(key);
                self.workspace_focus(index, window, cx);
            }
        } else {
            self.workspace_open_terminal(true, window, cx);
        }
        cx.notify();
    }

    /// Restore an existing preview or show the workspace menu when no preview exists.
    pub(crate) fn workspace_open_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((key, bottom)) = self
            .workspace
            .tabs
            .iter()
            .rev()
            .find(|(key, _)| matches!(key, TabKey::Artifact(_) | TabKey::File(_)))
            .cloned()
        {
            if let TabKey::File(id) = key
                && let Some(view) = self.workspace.files.get(&id).cloned()
            {
                self.workspace_open_file(view, window, cx);
                return;
            }
            self.workspace.open(key);
            self.workspace_focus(usize::from(bottom), window, cx);
        } else {
            self.workspace_toggle_menu(false, window, cx);
        }
        cx.notify();
    }

    fn workspace_close_tab(&mut self, key: &TabKey, window: &mut Window, cx: &mut Context<Self>) {
        let index = self
            .workspace
            .tabs
            .iter()
            .find(|(tab, _)| tab == key)
            .map_or(0, |(_, bottom)| usize::from(*bottom));
        self.workspace.close(key);
        match key {
            TabKey::Terminal(id) => {
                self.workspace.terminals.remove(id);
            }
            TabKey::File(id) => {
                self.workspace.files.remove(id);
            }
            TabKey::Diff => self.diff_controller.close(),
            TabKey::Artifact(_) => {}
        }
        self.workspace_focus(index, window, cx);
        cx.notify();
    }

    fn workspace_move_selected(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(key) = self.workspace.selected[index].clone() {
            self.workspace.close(&key);
            self.workspace.tabs.push((key.clone(), index == 0));
            if let TabKey::Terminal(id) = &key
                && let Some(terminal) = self.workspace.terminals.get_mut(id)
            {
                terminal.bottom = index == 0;
            }
            self.workspace.selected[1 - index] = Some(key);
            self.workspace.reveal_tabs[1 - index] = true;
            self.workspace.hidden[1 - index] = false;
            self.workspace.maximized[index] = false;
            self.workspace_focus(1 - index, window, cx);
        }
        cx.notify();
    }

    fn workspace_hide(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.hidden[index] = true;
        self.workspace.maximized[index] = false;
        self.workspace_focus_composer(window, cx);
        cx.notify();
    }

    fn workspace_focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, stream)) = &self.stream_view {
            stream.update(cx, |stream, cx| stream.focus_composer(window, cx));
        }
    }

    fn workspace_label(&self, key: &TabKey, cx: &App) -> String {
        match key {
            TabKey::Diff => "Review".into(),
            TabKey::Terminal(id) => format!("终端 {id}"),
            TabKey::File(id) => self
                .workspace
                .files
                .get(id)
                .map(|view| view.read(cx).title().to_string())
                .unwrap_or_else(|| "文件".into()),
            TabKey::Artifact(id) => self
                .artifact_controller
                .active
                .as_ref()
                .and_then(|active| active.cards.get(id))
                .map(|card| card.read(cx).projection().label.clone())
                .unwrap_or_else(|| "Preview".into()),
        }
    }

    pub(crate) fn workspace_open_diff(&mut self, cx: &mut Context<Self>) {
        if let (Some(thread), Some((_, stream))) = (
            cx.global::<OpenedThread>().0.clone(),
            self.stream_view.clone(),
        ) {
            if thread.is_standalone() {
                return;
            }
            self.open_workspace_diff(
                stream,
                &OpenWorkspaceDiffRequested {
                    thread_id: thread.id,
                    project_id: thread.project_id,
                },
                cx,
            );
        }
    }

    /// Keeps the trusted commit route reachable from the selected Review
    /// workspace without adding speculative commit state to the R19 main
    /// header or Environment card.
    fn workspace_open_commit(&mut self, cx: &mut Context<Self>) {
        if let (Some(thread), Some((thread_id, stream))) = (
            cx.global::<OpenedThread>().0.clone(),
            self.stream_view.clone(),
        ) {
            if thread.is_standalone() || thread_id != thread.id {
                return;
            }
            self.open_commit_panel(
                stream,
                &OpenCommitPanelRequested {
                    thread_id: thread.id,
                    project_id: thread.project_id,
                },
                cx,
            );
        }
    }

    fn workspace_toggle_menu(&mut self, bottom: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.menu = !self.workspace.menu;
        self.workspace.menu_bottom = bottom;
        let focus = self
            .workspace
            .menu_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        if self.workspace.menu {
            window.focus(&focus, cx);
        } else {
            self.workspace_focus(usize::from(bottom), window, cx);
        }
        cx.notify();
    }

    fn workspace_focus(&self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let focus = match self.workspace.selected[index].as_ref() {
            Some(TabKey::File(id)) => self
                .workspace
                .files
                .get(id)
                .map(|view| view.read(cx).focus_handle(cx)),
            Some(TabKey::Terminal(id)) => self
                .workspace
                .terminals
                .get(id)
                .map(|tab| tab.view.read(cx).focus_handle(cx)),
            Some(TabKey::Diff) => self
                .diff_controller
                .active
                .as_ref()
                .map(|active| active.view.read(cx).focus_handle(cx)),
            Some(TabKey::Artifact(id)) => self
                .artifact_controller
                .active
                .as_ref()
                .and_then(|active| active.cards.get(id))
                .map(|card| card.read(cx).focus_handle(cx)),
            None => None,
        };
        if let Some(focus) = focus {
            window.focus(&focus, cx);
        } else {
            self.workspace_focus_composer(window, cx);
        }
    }

    /// Returns the selected project only when it is also the current task's
    /// durable project binding. This is the shell's route fence for every
    /// project-only header and Environment action.
    pub(super) fn shell_project_id(&self, cx: &App) -> Option<String> {
        let selected = cx.global::<vega_ui::sidebar::SelectedProject>().0.clone()?;
        match cx.global::<OpenedThread>().0.as_ref() {
            Some(thread) if thread.project_binding() == Some(selected.as_str()) => Some(selected),
            Some(_) => None,
            None => Some(selected),
        }
    }

    pub(super) fn shell_project_label(&self, cx: &App) -> Option<String> {
        let project_id = self.shell_project_id(cx)?;
        self.sidebar.read(cx).project_label(&project_id, cx)
    }

    pub(super) fn shell_project_thread(&self, cx: &App) -> Option<Thread> {
        let project_id = self.shell_project_id(cx)?;
        cx.global::<OpenedThread>()
            .0
            .as_ref()
            .filter(|thread| thread.project_binding() == Some(project_id.as_str()))
            .cloned()
    }

    fn workspace_available_width(&self, window: &Window, cx: &App) -> f32 {
        let sidebar = if !cx.global::<SidebarCollapsed>().0 && !self.auto_collapsed(window, cx) {
            vega_ui::sidebar::width(cx) + Layout::SIDEBAR_RESIZE_HIT_AREA
        } else {
            0.
        };
        f32::from(window.bounds().size.width) - sidebar - Layout::MAIN_CONTENT_GAP * 2.0
    }

    pub(super) fn persistent_right_workspace_visible(&self, window: &Window, cx: &App) -> bool {
        !self.workspace.hidden[0]
            && self.workspace.selected[0].is_some()
            && self.workspace_available_width(window, cx) >= 610.
    }

    pub(super) fn hidden_workspace_available(&self, index: usize) -> bool {
        self.workspace.selected[index].is_some() && self.workspace.hidden[index]
    }

    pub(super) fn restore_hidden_workspace(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.hidden_workspace_available(index) {
            return;
        }
        self.workspace.hidden[index] = false;
        self.workspace.reveal_tabs[index] = true;
        self.environment_overlay_open = false;
        self.workspace_focus(index, window, cx);
        cx.notify();
    }

    pub(super) fn environment_breakpoint(&self, window: &Window, cx: &App) -> f32 {
        let sidebar = if !cx.global::<SidebarCollapsed>().0 && !self.auto_collapsed(window, cx) {
            vega_ui::sidebar::width(cx)
        } else {
            0.0
        };
        Layout::ENVIRONMENT_BREAKPOINT + sidebar - Layout::SIDEBAR_WIDTH
    }

    pub(super) fn environment_is_wide(&self, window: &Window, cx: &App) -> bool {
        f32::from(window.viewport_size().width) >= self.environment_breakpoint(window, cx)
    }

    pub(super) fn toggle_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shell_project_id(cx).is_none()
            || self.persistent_right_workspace_visible(window, cx)
        {
            return;
        }
        if self.environment_is_wide(window, cx) {
            self.environment_collapsed = !self.environment_collapsed;
            self.environment_overlay_open = false;
        } else {
            self.environment_overlay_open = !self.environment_overlay_open;
        }
        cx.notify();
    }

    fn render_environment(&mut self, overlay: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let Some(_) = self.shell_project_id(cx) else {
            return div().into_any_element();
        };
        let project_label = self.shell_project_label(cx);
        let thread = self.shell_project_thread(cx);
        let branch_selector = thread.as_ref().and_then(|thread| {
            self.stream_view
                .as_ref()
                .filter(|(thread_id, _)| thread_id == &thread.id)
                .map(|(_, stream)| stream.read(cx).branch_selector())
        });
        if let Some(selector) = &branch_selector {
            selector.update(cx, |selector, _| selector.set_menu_below(true));
        }
        let close = icon_button(
            Icon::Close,
            "关闭 Environment",
            colors,
            cx.listener(move |this, _, _, cx| {
                if overlay {
                    this.environment_overlay_open = false;
                } else {
                    this.environment_collapsed = true;
                }
                cx.notify();
            }),
        )
        .debug_selector(|| "environment-close".into());
        let mut card = div()
            .id(if overlay {
                "environment-overlay-card"
            } else {
                "environment-card"
            })
            .debug_selector(move || {
                if overlay {
                    "environment-overlay-card".into()
                } else {
                    "environment-card".into()
                }
            })
            .w(px(Layout::ENVIRONMENT_CARD_WIDTH))
            .max_w_full()
            .max_h_full()
            .p_3()
            .rounded(px(Layout::ENVIRONMENT_CARD_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .shadow_sm()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_secondary)
                    .child("Environment")
                    .child(close),
            );
        if let Some(label) = project_label {
            card = card.child(
                div()
                    .debug_selector(|| "environment-project".into())
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_primary)
                    .child(vega_ui::icons::icon(Icon::Folder, colors.brand_primary))
                    .child(div().min_w_0().flex_1().truncate().child(label)),
            );
        }
        if let Some(selector) = branch_selector {
            card = card.child(
                div()
                    .debug_selector(|| "environment-branch".into())
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(vega_ui::icons::icon(
                        Icon::ArrowUpDown,
                        colors.text_secondary,
                    ))
                    .child(div().min_w_0().flex_1().child(selector)),
            );
        }
        if thread.is_some() {
            card = card.child(environment_action(
                "environment-review",
                "Changes / Review",
                Icon::Split,
                colors,
                cx.listener(|this, _, _, cx| {
                    this.environment_overlay_open = false;
                    this.workspace_open_diff(cx);
                    cx.notify();
                }),
            ));
        }
        card = card.child(environment_action(
            "environment-terminal",
            "Local terminal",
            Icon::Terminal,
            colors,
            cx.listener(|this, _, window, cx| {
                this.environment_overlay_open = false;
                this.workspace_toggle_terminal(window, cx);
            }),
        ));
        div()
            .id(if overlay {
                "environment-overlay"
            } else {
                "environment-rail"
            })
            .debug_selector(move || {
                if overlay {
                    "environment-overlay".into()
                } else {
                    "environment-rail".into()
                }
            })
            .w(px(Layout::ENVIRONMENT_RAIL_WIDTH))
            .h_full()
            .flex_shrink_0()
            .pt(px(Layout::ENVIRONMENT_CARD_INSET))
            .pr(px(Layout::ENVIRONMENT_CARD_INSET))
            .child(card)
            .into_any_element()
    }

    fn render_workspace_pane(
        &mut self,
        bottom: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let index = usize::from(bottom);
        let selected = self.workspace.selected[index].clone();
        let review_selected = selected == Some(TabKey::Diff);
        let tabs = self
            .workspace
            .tabs
            .iter()
            .filter(|(_, dock)| *dock == bottom)
            .cloned()
            .collect::<Vec<_>>();
        if self.workspace.reveal_tabs[index] {
            if let Some(position) = tabs
                .iter()
                .position(|(key, _)| Some(key) == selected.as_ref())
            {
                self.workspace.tab_scroll[index].scroll_to_item(position);
            }
            self.workspace.reveal_tabs[index] = false;
        }
        let mut strip = div()
            .id(if bottom { "bottom-tabs" } else { "right-tabs" })
            .flex()
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.workspace.tab_scroll[index])
            .gap_1();
        for (key, _) in tabs {
            let label = self.workspace_label(&key, cx);
            let close_key = key.clone();
            let keyboard_key = key.clone();
            let active = selected.as_ref() == Some(&key);
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("workspace-tab-{key:?}")))
                    .aria_label(label.clone())
                    .flex_shrink_0()
                    .max_w_full()
                    .focusable()
                    .tab_stop(true)
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.workspace.open(keyboard_key.clone());
                            this.workspace_focus(index, window, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(if active {
                        colors.bg_active
                    } else {
                        colors.bg_sidebar
                    })
                    .hover(move |s| s.bg(colors.bg_hover))
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.workspace.open(key.clone());
                            this.workspace_focus(index, window, cx);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(150.))
                            .truncate()
                            .child(label),
                    )
                    .child(icon_button(
                        Icon::Close,
                        format!("关闭 {}", self.workspace_label(&close_key, cx)),
                        colors,
                        cx.listener(move |this, _, window, cx| {
                            this.workspace_close_tab(&close_key, window, cx);
                        }),
                    )),
            );
        }
        let header = div()
            .flex()
            .items_center()
            .gap_1()
            .h(px(Layout::WORKSPACE_HEADER_HEIGHT))
            .px_1()
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(icon_button(
                Icon::ChevronDown,
                "所有工作区标签",
                colors,
                cx.listener(move |this, _, window, cx| {
                    this.workspace_toggle_menu(bottom, window, cx)
                }),
            ))
            .child(strip)
            .when(review_selected, |header| {
                header.child(
                    icon_button(
                        Icon::Document,
                        "提交更改",
                        colors,
                        cx.listener(|this, _, _, cx| this.workspace_open_commit(cx)),
                    )
                    .debug_selector(|| "workspace-review-commit".into()),
                )
            })
            .child(icon_button(
                Icon::Plus,
                "打开工作区标签",
                colors,
                cx.listener(move |this, _, window, cx| {
                    this.workspace_toggle_menu(bottom, window, cx)
                }),
            ))
            .child(icon_button(
                if bottom {
                    Icon::DockRight
                } else {
                    Icon::DockBottom
                },
                if bottom {
                    "移到右侧"
                } else {
                    "移到底部"
                },
                colors,
                cx.listener(move |this, _, window, cx| {
                    this.workspace_move_selected(index, window, cx);
                }),
            ))
            .child(icon_button(
                Icon::Maximize,
                if self.workspace.maximized[index] {
                    "还原工作区"
                } else {
                    "最大化工作区"
                },
                colors,
                cx.listener(move |this, _, window, cx| {
                    this.workspace.maximized[index] = !this.workspace.maximized[index];
                    this.workspace.reveal_tabs[index] = true;
                    this.workspace_focus(index, window, cx);
                    cx.notify();
                }),
            ))
            .child(icon_button(
                Icon::Minimize,
                "隐藏面板",
                colors,
                cx.listener(move |this, _, window, cx| {
                    this.workspace_hide(index, window, cx);
                }),
            ));
        let body = match selected {
            Some(TabKey::File(id)) => self
                .workspace
                .files
                .get(&id)
                .map(|view| view.clone().into_any_element()),
            Some(TabKey::Terminal(id)) => self
                .workspace
                .terminals
                .get(&id)
                .map(|tab| tab.view.clone().into_any_element()),
            Some(TabKey::Diff) => cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .and_then(|thread| self.diff_controller.visible_view(thread))
                .map(|view| {
                    if let Some(active) = self.diff_controller.active.as_mut()
                        && active.focus_pending
                    {
                        window.focus(&view.read(cx).focus_handle(cx), cx);
                        active.focus_pending = false;
                    }
                    view.into_any_element()
                }),
            Some(TabKey::Artifact(id)) => self
                .artifact_controller
                .active
                .as_ref()
                .and_then(|active| active.cards.get(&id))
                .cloned()
                .map(|card| {
                    let count = card.read(cx).row_count();
                    uniform_list("workspace-preview", count, move |range, window, cx| {
                        range
                            .map(|row| ArtifactCard::render_row(card.clone(), row, window, cx))
                            .collect()
                    })
                    .size_full()
                    .into_any_element()
                }),
            None => None,
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .min_w_0()
            .bg(colors.bg_base)
            .on_key_down(|event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            })
            .child(header)
            .child(div().flex_1().min_h_0().overflow_hidden().children(body))
            .into_any_element()
    }

    pub(super) fn render_workspace(
        &mut self,
        conversation: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.workspace.composer_focus_pending {
            self.workspace_focus_composer(window, cx);
            self.workspace.composer_focus_pending = false;
        }
        let colors = theme(cx).colors;
        let size = window.bounds().size;
        if self.workspace.window_size != Some(size) {
            self.workspace.window_size = Some(size);
            self.workspace.reveal_tabs = [true; 2];
        }
        let available = self.workspace_available_width(window, cx);
        let right = self.persistent_right_workspace_visible(window, cx);
        let environment_wide = self.environment_is_wide(window, cx);
        let bottom = !self.workspace.hidden[1]
            && self.workspace.selected[1].is_some()
            && f32::from(size.height) >= 480.;
        let width = if self.workspace.maximized[0] {
            available - 300.
        } else {
            self.workspace.width.unwrap_or(available * 0.43)
        }
        .clamp(270., (available - 300.).max(270.));
        let height = if self.workspace.maximized[1] {
            f32::from(size.height) - 240.
        } else {
            self.workspace
                .height
                .unwrap_or(Layout::BOTTOM_WORKSPACE_HEIGHT)
        }
        .clamp(150., (f32::from(size.height) - 240.).max(150.));
        let fullscreen = (0..2).find(|index| {
            self.workspace.maximized[*index]
                && !self.workspace.hidden[*index]
                && self.workspace.selected[*index].is_some()
        });
        if right {
            self.environment_overlay_open = false;
        }
        if environment_wide {
            self.environment_overlay_open = false;
        }
        let environment_rail = fullscreen.is_none()
            && !right
            && self.shell_project_id(cx).is_some()
            && environment_wide
            && !self.environment_collapsed;
        let environment_overlay = fullscreen.is_none()
            && !right
            && self.shell_project_id(cx).is_some()
            && !environment_wide
            && self.environment_overlay_open;
        if let Some((_, stream)) = &self.stream_view {
            stream.update(cx, |stream, cx| {
                stream.set_workspace_width(
                    if right {
                        available - width - 5.
                    } else if environment_rail {
                        available - Layout::ENVIRONMENT_RAIL_WIDTH
                    } else {
                        available
                    },
                    cx,
                )
            });
        }
        let mut row = div().flex().flex_1().min_h_0().min_w_0().child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_hidden()
                .child(conversation),
        );
        if right && fullscreen.is_none() {
            row = row
                .child(
                    div()
                        .w(px(5.))
                        .h_full()
                        .cursor_col_resize()
                        .flex()
                        .justify_center()
                        .child(div().w(px(1.)).h_full().bg(colors.border_subtle))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.workspace.dragging = Some(false);
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .debug_selector(|| "right-workspace-pane".into())
                        .w(px(width))
                        .h_full()
                        .flex_shrink_0()
                        .child(self.render_workspace_pane(false, window, cx)),
                );
        } else if environment_rail {
            row = row.child(self.render_environment(false, cx));
        }
        let mut layout = div()
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .when(fullscreen.is_none(), |layout| layout.child(row));
        if let Some(index) = fullscreen {
            layout = layout.child(self.render_workspace_pane(index == 1, window, cx));
        }
        if bottom && fullscreen.is_none() {
            layout = layout
                .child(
                    div()
                        .h(px(5.))
                        .w_full()
                        .cursor_row_resize()
                        .flex()
                        .items_center()
                        .child(div().h(px(1.)).w_full().bg(colors.border_subtle))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.workspace.dragging = Some(true);
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .debug_selector(|| "bottom-workspace-pane".into())
                        .h(px(height))
                        .flex_shrink_0()
                        .child(self.render_workspace_pane(true, window, cx)),
                );
        }
        if environment_overlay {
            layout = layout.child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .w(px(Layout::ENVIRONMENT_RAIL_WIDTH))
                    .h_full()
                    .child(self.render_environment(true, cx)),
            );
        }
        if self.workspace.terminal_error {
            layout = layout.child(
                div()
                    .absolute()
                    .top(px(40.))
                    .right(px(8.))
                    .text_color(colors.danger)
                    .child("终端不可用：请选择项目，或关闭部分会话后重试"),
            );
        }
        if self.workspace.menu {
            let focus = self
                .workspace
                .menu_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            let mut menu = div()
                .id("workspace-add-menu")
                .track_focus(&focus)
                .key_context("WorkspaceMenu")
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "tab" | "down" => {
                            if event.keystroke.modifiers.shift {
                                window.focus_prev(cx);
                            } else {
                                window.focus_next(cx);
                            }
                            cx.stop_propagation();
                        }
                        "up" => {
                            window.focus_prev(cx);
                            cx.stop_propagation();
                        }
                        "enter"
                            if this
                                .workspace
                                .menu_focus
                                .as_ref()
                                .is_some_and(|focus| focus.is_focused(window)) =>
                        {
                            this.workspace_open_diff(cx);
                            cx.stop_propagation();
                            cx.notify();
                        }
                        _ => {}
                    }
                }))
                .on_action(cx.listener(|this, _: &CloseSettings, window, cx| {
                    this.workspace.menu = false;
                    this.workspace_focus(usize::from(this.workspace.menu_bottom), window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }))
                .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.workspace.menu = false;
                    this.workspace_focus(usize::from(this.workspace.menu_bottom), window, cx);
                    cx.notify();
                }))
                .absolute()
                .when(
                    !self.workspace.menu_bottom || fullscreen.is_some(),
                    |menu| menu.top(px(38.)),
                )
                .when(self.workspace.menu_bottom && fullscreen.is_none(), |menu| {
                    menu.bottom(px((height - 40.).max(0.)))
                })
                .max_h(px(280.))
                .overflow_y_scroll()
                .right(px(8.))
                .w(px(Layout::TASK_MENU_WIDTH.min(Layout::MENU_MAX_WIDTH)))
                .p_2()
                .rounded(px(Layout::MENU_RADIUS))
                .border_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_elevated)
                .shadow_sm()
                .flex()
                .flex_col()
                .gap_1()
                .child(workspace_button(
                    "Review · 工作区变更",
                    colors,
                    cx.listener(|this, _, _, cx| {
                        this.workspace_open_diff(cx);
                        this.workspace.menu = false;
                        cx.notify();
                    }),
                ));
            menu = menu.child(workspace_button(
                "终端 · 新建登录 shell",
                colors,
                cx.listener(|this, _, window, cx| {
                    this.workspace_open_terminal(this.workspace.menu_bottom, window, cx);
                }),
            ));
            if self.workspace_has_preview() {
                menu = menu.child(workspace_button(
                    "预览 · 恢复已打开内容",
                    colors,
                    cx.listener(|this, _, window, cx| this.workspace_open_preview(window, cx)),
                ));
            }
            if !self.workspace.terminals.is_empty() {
                menu = menu.child(workspace_button(
                    "关闭所有终端（含其他项目）",
                    colors,
                    cx.listener(|this, _, window, cx| {
                        let keys = this
                            .workspace
                            .tabs
                            .iter()
                            .filter(|(key, _)| matches!(key, TabKey::Terminal(_)))
                            .map(|(key, _)| key.clone())
                            .collect::<Vec<_>>();
                        for key in keys {
                            this.workspace_close_tab(&key, window, cx);
                        }
                        this.workspace.terminals.clear();
                        this.workspace.terminal_error = false;
                        this.workspace.menu = false;
                        cx.notify();
                    }),
                ));
            }
            for (key, bottom) in self.workspace.tabs.clone() {
                let label = format!(
                    "{} · {}",
                    self.workspace_label(&key, cx),
                    if bottom { "底部" } else { "右侧" }
                );
                menu = menu.child(workspace_button(
                    label,
                    colors,
                    cx.listener(move |this, _, window, cx| {
                        this.workspace.open(key.clone());
                        this.workspace_focus(usize::from(bottom), window, cx);
                        cx.notify();
                    }),
                ));
            }
            let cards = self
                .artifact_controller
                .active
                .as_ref()
                .map(|active| active.cards.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            for card in cards {
                if card.read(cx).projection().preview_available {
                    let label = card.read(cx).projection().label.clone();
                    menu = menu.child(workspace_button(
                        label,
                        colors,
                        cx.listener(move |this, _, _, cx| {
                            card.update(cx, ArtifactCard::preview);
                            this.workspace.menu = false;
                            cx.notify();
                        }),
                    ));
                }
            }
            layout = layout.child(menu);
        }
        layout
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if let Some(bottom) = this.workspace.dragging {
                    if !event
                        .pressed_button
                        .is_some_and(|button| button == MouseButton::Left)
                    {
                        this.workspace.dragging = None;
                        return;
                    }
                    let size = window.bounds().size;
                    this.workspace.reveal_tabs = [true; 2];
                    if bottom {
                        this.workspace.height = Some(f32::from(size.height - event.position.y));
                    } else {
                        this.workspace.width = Some(f32::from(size.width - event.position.x));
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.workspace.dragging = None;
                }),
            )
            .into_any_element()
    }
}

fn workspace_button(
    label: impl Into<SharedString>,
    colors: ThemeColors,
    activate: impl Fn(&(), &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label = label.into();
    let keyboard_label = label.clone();
    let activate = std::rc::Rc::new(activate);
    let keyboard = activate.clone();
    div()
        .id(label.clone())
        .aria_label(keyboard_label)
        .focusable()
        .tab_stop(true)
        .h(px(Typography::SIDEBAR_LINE_HEIGHT))
        .px_2()
        .rounded_md()
        .flex()
        .items_center()
        .text_size(px(Typography::SIDEBAR))
        .text_color(colors.text_secondary)
        .cursor_pointer()
        .hover(move |style| style.bg(colors.bg_hover))
        .focus(move |style| style.bg(colors.bg_active))
        .on_mouse_up(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            activate(&(), window, cx);
        })
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                keyboard(&(), window, cx);
            }
        })
        .child(label)
}

fn environment_action(
    id: &'static str,
    label: &'static str,
    icon: Icon,
    colors: ThemeColors,
    activate: impl Fn(&(), &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let activate = std::rc::Rc::new(activate);
    let keyboard = activate.clone();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .aria_label(label)
        .focusable()
        .tab_stop(true)
        .h(px(Typography::SIDEBAR_LINE_HEIGHT))
        .px_2()
        .rounded_md()
        .flex()
        .items_center()
        .gap_2()
        .text_size(px(Typography::SIDEBAR))
        .text_color(colors.text_primary)
        .cursor_pointer()
        .hover(move |style| style.bg(colors.bg_hover))
        .focus(move |style| style.bg(colors.bg_active))
        .on_mouse_up(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            activate(&(), window, cx);
        })
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                keyboard(&(), window, cx);
            }
        })
        .child(vega_ui::icons::icon(icon, colors.text_secondary))
        .child(div().min_w_0().flex_1().truncate().child(label))
}

#[cfg(test)]
mod tests {
    use super::{TabKey, VegaWindow};
    use crate::tests::{diff_controller_repo, install_diff_window_globals};
    use gpui_kit::prelude::*;
    use gpui_kit::{
        AppContext, Bounds, KeyBinding, Modifiers, MouseButton, Pixels, TestAppContext,
        VisualTestContext, WindowBounds, WindowHandle, WindowOptions, point, px, size,
    };
    use vega_theme::Layout;
    use vega_ui::diff_view::DiffClosed;
    use vega_ui::settings::{CloseSettings, SettingsOpen, SettingsView};
    use vega_ui::sidebar::{
        OpenedThread, PendingDeleteConfirm, SelectedProject, SidebarCollapsed, SidebarWidth,
    };

    fn shell_bounds(
        window: WindowHandle<VegaWindow>,
        selector: &'static str,
        cx: &mut TestAppContext,
    ) -> Bounds<Pixels> {
        cx.run_until_parked();
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing R19 shell selector: {selector}"))
    }

    fn shell_absent(
        window: WindowHandle<VegaWindow>,
        selector: &'static str,
        cx: &mut TestAppContext,
    ) -> bool {
        cx.run_until_parked();
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds(selector)
            .is_none()
    }

    fn shell_click(
        window: WindowHandle<VegaWindow>,
        selector: &'static str,
        cx: &mut TestAppContext,
    ) {
        let bounds = shell_bounds(window, selector, cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(bounds.center(), Modifiers::default());
        visual.run_until_parked();
    }

    fn assert_close(actual: Pixels, expected: f32, label: &str) {
        let actual = f32::from(actual);
        assert!(
            (actual - expected).abs() <= 1.0,
            "{label}: expected {expected}±1px, got {actual}px"
        );
    }

    #[gpui_kit::test]
    async fn r21_shell_mounts_resizable_sidebar_and_exact_environment_boundaries(
        cx: &mut TestAppContext,
    ) {
        let repo = diff_controller_repo();
        let store = vega_store::Store::open(":memory:").expect("owned store");
        store.migrate().expect("owned migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("fixture path"),
            "R19 Project",
            None,
        )
        .expect("project");
        let thread =
            vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm")
                .expect("thread");
        vega_conversation::threads::rename_thread(&store, &thread.id, "R19 task")
            .expect("task title");
        let standalone =
            vega_conversation::threads::create_standalone_thread(&store, "mock", "confirm")
                .expect("standalone task");
        let project_id = project.id.clone();
        cx.update(|cx| install_diff_window_globals(store, thread, cx));
        let root = cx.new(VegaWindow::new);
        let window_root = root.clone();
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1403.), px(860.)),
                    ))),
                    ..Default::default()
                },
                move |_, _| window_root,
            )
            .expect("production root window")
        });
        cx.run_until_parked();

        let sidebar = shell_bounds(window, "sidebar", cx);
        let resizer = shell_bounds(window, "sidebar-resize-handle", cx);
        let panel = shell_bounds(window, "main-content-panel", cx);
        let header = shell_bounds(window, "main-header", cx);
        let rail = shell_bounds(window, "environment-rail", cx);
        let card = shell_bounds(window, "environment-card", cx);
        let composer = shell_bounds(window, "composer-shell", cx);
        let conversation = shell_bounds(window, "conversation-column", cx);
        assert_close(sidebar.size.width, Layout::SIDEBAR_WIDTH, "sidebar width");
        assert_close(
            resizer.size.width,
            Layout::SIDEBAR_RESIZE_HIT_AREA,
            "Sidebar resize target width",
        );
        assert_close(
            resizer.left() - sidebar.right(),
            0.0,
            "resize target follows Sidebar",
        );
        assert_close(
            panel.left() - resizer.right(),
            Layout::MAIN_CONTENT_GAP,
            "flat main split",
        );
        assert_close(
            header.size.height,
            Layout::MAIN_HEADER_HEIGHT,
            "header height",
        );
        assert_close(
            rail.size.width,
            Layout::ENVIRONMENT_RAIL_WIDTH,
            "Environment rail width",
        );
        assert_close(
            card.top() - rail.top(),
            Layout::ENVIRONMENT_CARD_INSET,
            "Environment card top inset",
        );
        assert_close(
            rail.right() - card.right(),
            Layout::ENVIRONMENT_CARD_INSET,
            "Environment card right inset",
        );
        assert_close(
            card.size.width,
            Layout::ENVIRONMENT_CARD_WIDTH,
            "Environment card width",
        );
        assert_close(
            composer.size.width,
            Layout::COMPOSER_MAX_WIDTH,
            "composer width",
        );
        assert!(
            f32::from(composer.size.height) >= Layout::COMPOSER_MIN_HEIGHT,
            "composer keeps its 100px minimum"
        );
        assert!(
            f32::from(conversation.size.width) <= Layout::CONTENT_MAX_WIDTH + 1.0,
            "conversation keeps its readable-column cap"
        );
        for selector in [
            "main-header-project",
            "main-header-review",
            "main-header-terminal",
            "main-header-environment",
            "environment-project",
            "environment-branch",
            "environment-review",
            "environment-terminal",
        ] {
            let _ = shell_bounds(window, selector, cx);
        }

        shell_click(window, "environment-close", cx);
        assert!(shell_absent(window, "environment-rail", cx));
        assert!(root.read_with(cx, |root, _| root.environment_collapsed
            && !root.environment_overlay_open));

        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(1229.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("resize to 1229px below default Environment breakpoint");
        cx.run_until_parked();
        assert!(shell_absent(window, "environment-rail", cx));
        shell_click(window, "main-header-environment", cx);
        assert_close(
            shell_bounds(window, "environment-overlay", cx).size.width,
            Layout::ENVIRONMENT_RAIL_WIDTH,
            "Environment overlay width",
        );
        assert!(root.read_with(cx, |root, _| root.environment_collapsed
            && root.environment_overlay_open));

        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(1230.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("resize to exact default Environment breakpoint");
        cx.run_until_parked();
        assert!(shell_absent(window, "environment-rail", cx));
        assert!(root.read_with(cx, |root, _| root.environment_collapsed
            && !root.environment_overlay_open));
        shell_click(window, "main-header-environment", cx);
        let _ = shell_bounds(window, "environment-rail", cx);
        assert!(root.read_with(cx, |root, _| !root.environment_collapsed
            && !root.environment_overlay_open));

        cx.update(|cx| {
            cx.set_global(SidebarWidth(Layout::SIDEBAR_MAX_WIDTH));
            cx.refresh_windows();
        });
        cx.run_until_parked();
        assert_close(
            shell_bounds(window, "sidebar", cx).size.width,
            Layout::SIDEBAR_MAX_WIDTH,
            "maximum Sidebar width",
        );
        assert!(
            shell_absent(window, "environment-rail", cx),
            "1230px is narrow after the Sidebar grows"
        );
        assert!(root.read_with(cx, |root, _| !root.environment_collapsed));
        shell_click(window, "main-header-environment", cx);
        let _ = shell_bounds(window, "environment-overlay", cx);
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(1290.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("resize to 1290px below maximum-width breakpoint");
        let _ = shell_bounds(window, "environment-overlay", cx);
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(1291.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("resize to exact maximum-width breakpoint");
        let _ = shell_bounds(window, "environment-rail", cx);
        assert!(shell_absent(window, "environment-overlay", cx));

        cx.update(|cx| {
            cx.set_global(SidebarWidth(Layout::SIDEBAR_MIN_WIDTH));
            cx.refresh_windows();
        });
        cx.run_until_parked();
        assert_close(
            shell_bounds(window, "sidebar", cx).size.width,
            Layout::SIDEBAR_MIN_WIDTH,
            "minimum Sidebar width",
        );
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(1165.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("resize below minimum-width breakpoint");
        assert!(shell_absent(window, "environment-rail", cx));
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(1166.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("resize to exact minimum-width breakpoint");
        let _ = shell_bounds(window, "environment-rail", cx);

        cx.update(|cx| {
            cx.set_global(SidebarCollapsed(true));
            cx.refresh_windows();
        });
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(960.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("resize with collapsed Sidebar");
        cx.run_until_parked();
        assert!(shell_absent(window, "sidebar", cx));
        let _ = shell_bounds(window, "environment-rail", cx);

        cx.update(|cx| {
            cx.set_global(SidebarWidth(Layout::SIDEBAR_WIDTH));
            cx.set_global(SidebarCollapsed(false));
            cx.refresh_windows();
        });
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(1403.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("restore screenshot-parity viewport");
        cx.run_until_parked();
        let _ = shell_bounds(window, "environment-rail", cx);

        shell_click(window, "environment-review", cx);
        assert!(
            root.read_with(cx, |root, _| root.diff_controller.active.is_some()
                && root.workspace.selected[0] == Some(TabKey::Diff))
        );
        let _ = shell_bounds(window, "right-workspace-pane", cx);
        shell_click(window, "workspace-review-commit", cx);
        assert!(root.read_with(cx, |root, _| root.commit_controller.active.is_some()));
        assert!(shell_absent(window, "environment-rail", cx));

        cx.update(|cx| {
            cx.set_global(SidebarCollapsed(true));
            cx.refresh_windows();
        });
        cx.run_until_parked();
        assert!(shell_absent(window, "sidebar", cx));
        let _ = shell_bounds(window, "navigation-back", cx);

        cx.update(|cx| {
            cx.set_global(SelectedProject(Some(project_id.clone())));
            cx.set_global(OpenedThread(Some(standalone.clone())));
            cx.refresh_windows();
        });
        cx.run_until_parked();
        let _ = shell_bounds(window, "main-header-title", cx);
        for selector in [
            "main-header-project",
            "main-header-review",
            "main-header-terminal",
            "main-header-environment",
            "environment-rail",
        ] {
            assert!(
                shell_absent(window, selector, cx),
                "standalone route must fence {selector}"
            );
        }

        cx.update(|cx| {
            cx.set_global(SidebarCollapsed(false));
            cx.set_global(SelectedProject(Some(project_id.clone())));
            cx.set_global(OpenedThread(None));
            cx.refresh_windows();
        });
        cx.run_until_parked();
        let _ = shell_bounds(window, "environment-project", cx);
        let _ = shell_bounds(window, "environment-terminal", cx);
        assert!(shell_absent(window, "environment-branch", cx));
        assert!(shell_absent(window, "environment-review", cx));
        assert!(shell_absent(window, "main-header-review", cx));

        cx.update(|cx| {
            cx.set_global(SelectedProject(None));
            cx.refresh_windows();
        });
        cx.run_until_parked();
        assert!(shell_absent(window, "environment-rail", cx));
        assert!(shell_absent(window, "main-header-terminal", cx));
        assert!(shell_absent(window, "main-header-environment", cx));
    }

    #[cfg(unix)]
    #[gpui_kit::test]
    async fn r21_sidebar_drag_clamps_persists_and_preserves_independent_choices(
        cx: &mut TestAppContext,
    ) {
        const MARKER: &str = "VEGA_R21_SIDEBAR_DRAG_CHILD";
        let Some(path) = std::env::var_os(MARKER) else {
            let owned = tempfile::tempdir().expect("owned Sidebar config root");
            let config_root = owned.path().join("config");
            let output = std::process::Command::new(
                std::env::current_exe().expect("current Vega test binary"),
            )
            .args([
                "--exact",
                "window::workspace::tests::r21_sidebar_drag_clamps_persists_and_preserves_independent_choices",
                "--nocapture",
            ])
            .env(MARKER, owned.path())
            .env("HOME", owned.path())
            .env("XDG_CONFIG_HOME", config_root)
            .output()
            .expect("isolated Sidebar drag test process");
            assert!(
                output.status.success(),
                "owned R21 Sidebar drag subprocess failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        };
        let path = std::path::PathBuf::from(path);
        let repo = diff_controller_repo();
        let store = vega_store::Store::open(path.join("test.db")).expect("owned store");
        store.migrate().expect("owned migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("fixture path"),
            "R21 drag",
            None,
        )
        .expect("project");
        let thread =
            vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm")
                .expect("thread");
        cx.update(|cx| install_diff_window_globals(store, thread, cx));
        let root = cx.new(VegaWindow::new);
        let window_root = root.clone();
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1403.), px(860.)),
                    ))),
                    ..Default::default()
                },
                move |_, _| window_root,
            )
            .expect("production root window")
        });
        cx.run_until_parked();
        let environment_choice = root.read_with(cx, |root, _| {
            (root.environment_collapsed, root.environment_overlay_open)
        });

        let drag_to = |target: f32, cx: &mut TestAppContext| {
            let handle = shell_bounds(window, "sidebar-resize-handle", cx);
            let destination = point(px(target), handle.center().y);
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            visual.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
            visual.simulate_mouse_move(destination, Some(MouseButton::Left), Modifiers::default());
            visual.simulate_mouse_up(destination, MouseButton::Left, Modifiers::default());
            visual.run_until_parked();
        };

        drag_to(999.0, cx);
        assert_close(
            shell_bounds(window, "sidebar", cx).size.width,
            Layout::SIDEBAR_MAX_WIDTH,
            "dragged maximum Sidebar width",
        );
        assert_eq!(
            vega_store::config::load()
                .expect("persisted maximum Sidebar width")
                .ui
                .sidebar_width,
            Layout::SIDEBAR_MAX_WIDTH
        );
        assert!(!cx.update(|cx| cx.global::<SidebarCollapsed>().0));
        assert_eq!(
            root.read_with(cx, |root, _| {
                (root.environment_collapsed, root.environment_overlay_open)
            }),
            environment_choice
        );

        drag_to(100.0, cx);
        assert_close(
            shell_bounds(window, "sidebar", cx).size.width,
            Layout::SIDEBAR_MIN_WIDTH,
            "dragged minimum Sidebar width",
        );
        assert_eq!(
            vega_store::config::load()
                .expect("persisted minimum Sidebar width")
                .ui
                .sidebar_width,
            Layout::SIDEBAR_MIN_WIDTH
        );
        assert!(!cx.update(|cx| cx.global::<SidebarCollapsed>().0));
        assert_eq!(
            root.read_with(cx, |root, _| {
                (root.environment_collapsed, root.environment_overlay_open)
            }),
            environment_choice
        );
    }

    #[gpui_kit::test]
    async fn workspace_root_preserves_draft_reopens_review_and_fences_task_switch(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let repo = diff_controller_repo();
        let store = vega_store::Store::open(":memory:").expect("owned store");
        store.migrate().expect("owned migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("fixture path"),
            "workspace",
            None,
        )
        .expect("project");
        let thread =
            vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm")
                .expect("thread");
        let second =
            vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm")
                .expect("second thread");
        cx.update(|cx| {
            install_diff_window_globals(store, thread, cx);
            cx.bind_keys([
                KeyBinding::new("escape", CloseSettings, Some("VegaWindow")),
                KeyBinding::new("escape", CloseSettings, Some("WorkspaceMenu")),
            ]);
            cx.on_action(|_: &CloseSettings, cx| {
                if cx.global::<PendingDeleteConfirm>().0.is_some() {
                    cx.set_global(PendingDeleteConfirm(None));
                } else {
                    cx.set_global(SettingsOpen(false));
                }
                cx.refresh_windows();
            });
        });
        let root = cx.new(VegaWindow::new);
        let window_root = root.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, _| window_root)
                .expect("production root window")
        });
        cx.run_until_parked();
        let stream = root.read_with(cx, |root, _| {
            root.stream_view.as_ref().expect("conversation").1.clone()
        });
        let input = stream.read_with(cx, |stream, _| stream.composer_input());
        input.update(cx, |input, cx| {
            input.set_text("retained workspace draft", cx)
        });
        root.update(cx, |root, cx| root.workspace_open_diff(cx));
        let view = root.read_with(cx, |root, _| {
            root.diff_controller
                .active
                .as_ref()
                .expect("production diff route")
                .view
                .clone()
        });
        for _ in 0..400 {
            cx.executor()
                .advance_clock(crate::diff_controller::DIFF_RESULT_POLL);
            cx.run_until_parked();
            if view.read_with(cx, |view, _| !view.is_refreshing()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            view.read_with(cx, |view, _| view.generation().is_some()
                && view.row_count() > 0),
            "real Git data reached production Review"
        );
        assert!(root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(_, entity)| *entity == stream)
                && root.workspace.selected[0] == Some(TabKey::Diff)
        }));
        window
            .update(cx, |root, window, cx| {
                root.workspace_toggle_menu(false, window, cx)
            })
            .expect("open menu");
        cx.simulate_keystrokes(window.into(), "escape");
        assert!(
            root.read_with(cx, |root, _| !root.workspace.menu
                && root.diff_controller.active.is_some()),
            "local menu Escape must not close Review or hit the live global fallback"
        );
        window
            .update(cx, |root, window, cx| {
                root.workspace_move_selected(0, window, cx)
            })
            .expect("move Review bottom");
        cx.run_until_parked();
        assert!(root.read_with(cx, |root, _| root.workspace.selected[1]
            == Some(TabKey::Diff)
            && root.workspace.selected[0].is_none()));
        window
            .update(cx, |root, window, cx| root.workspace_hide(1, window, cx))
            .expect("hide bottom");
        cx.run_until_parked();
        assert!(root.read_with(cx, |root, _| {
            root.workspace.hidden[1]
                && root
                    .diff_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.view == view)
        }));
        root.update(cx, |root, cx| root.workspace_open_diff(cx));
        cx.run_until_parked();
        assert!(
            root.read_with(cx, |root, _| !root.workspace.hidden[1]
                && root
                    .diff_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.view == view)),
            "reopen hidden tab reuses real view and controller"
        );
        window
            .update(cx, |root, window, cx| {
                root.workspace_move_selected(1, window, cx)
            })
            .expect("move Review right");
        root.update(cx, |root, cx| {
            root.workspace.hidden[0] = true;
            root.settings_view = Some(cx.new(|cx| {
                SettingsView::from_config(vega_store::config::AppConfig::default(), None, cx)
            }));
            cx.set_global(SettingsOpen(true));
        });
        cx.run_until_parked();
        assert!(root.read_with(cx, |root, _| root.diff_controller.active.is_none()));
        cx.update(|cx| {
            cx.set_global(SettingsOpen(false));
            cx.refresh_windows();
        });
        cx.run_until_parked();
        assert!(
            root.read_with(cx, |root, _| root.diff_controller.active.is_some()),
            "returning from Settings must not leave an orphan tab"
        );
        assert_eq!(
            input.read_with(cx, |input, _| input.text().to_string()),
            "retained workspace draft"
        );
        root.update(cx, |root, cx| {
            let active = root.diff_controller.active.as_ref().expect("reopened diff");
            let request = DiffClosed {
                thread_id: active.identity.thread_id.clone(),
                project_id: active.identity.project_id.clone(),
            };
            root.close_workspace_diff(active.view.clone(), &request, cx);
        });
        cx.run_until_parked();
        assert!(root.read_with(cx, |root, _| {
            root.diff_controller.active.is_none()
                && !root
                    .workspace
                    .tabs
                    .iter()
                    .any(|(key, _)| *key == TabKey::Diff)
        }));
        window
            .update(cx, |root, window, cx| {
                root.workspace_toggle_menu(false, window, cx)
            })
            .expect("reopen add menu");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "tab enter");
        cx.run_until_parked();
        assert!(
            root.read_with(cx, |root, _| root.diff_controller.active.is_some()
                && !root.workspace.menu),
            "keyboard menu selection must reopen production Review"
        );
        cx.update(|cx| cx.set_global(OpenedThread(Some(second))));
        cx.run_until_parked();
        assert!(
            root.read_with(cx, |root, _| root.diff_controller.active.is_none()
                && root.workspace.tabs.is_empty()),
            "new task must not show previous task workspace data"
        );
    }
}

#[cfg(all(test, unix))]
mod terminal_tests {
    use super::{TabKey, VegaWindow};
    use crate::tests::install_diff_window_globals;
    use gpui_kit::Focusable;
    use gpui_kit::{
        AppContext, Bounds, Modifiers, TestAppContext, VisualTestContext, WindowBounds,
        WindowOptions, point, px, size,
    };
    use vega_theme::Layout;
    use vega_ui::sidebar::SelectedProject;

    #[gpui_kit::test]
    async fn r19_environment_terminal_action_opens_default_bottom_dock(cx: &mut TestAppContext) {
        const MARKER: &str = "VEGA_R19_ENVIRONMENT_TERMINAL_CHILD";
        let Some(path) = std::env::var_os(MARKER) else {
            let root = tempfile::tempdir().expect("owned terminal home");
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    "window::workspace::terminal_tests::r19_environment_terminal_action_opens_default_bottom_dock",
                    "--nocapture",
                ])
                .env(MARKER, root.path())
                .env("HOME", root.path())
                .env("ZDOTDIR", root.path())
                .output()
                .expect("isolated test process");
            assert!(
                output.status.success(),
                "owned R19 terminal subprocess failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        };
        let path = std::path::PathBuf::from(path);
        let store = vega_store::Store::open(path.join("test.db")).expect("store");
        store.migrate().expect("migrations");
        let project =
            vega_store::projects::create(store.conn(), path.to_str().unwrap(), "R19", None)
                .expect("project");
        let thread =
            vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm")
                .expect("thread");
        cx.update(|cx| install_diff_window_globals(store, thread, cx));
        let root = cx.new(VegaWindow::new);
        let entity = root.clone();
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1400.), px(900.)),
                    ))),
                    ..Default::default()
                },
                move |_, _| entity,
            )
            .expect("production root")
        });
        cx.run_until_parked();
        let action = VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("environment-terminal")
            .expect("truthful Local terminal action");
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(action.center(), Modifiers::default());
        visual.run_until_parked();
        root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.terminals.len(), 1);
            assert!(matches!(
                root.workspace.selected[1],
                Some(TabKey::Terminal(_))
            ));
            assert!(!root.workspace.hidden[1]);
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let bottom = visual
            .debug_bounds("bottom-workspace-pane")
            .expect("bottom terminal dock");
        assert!(
            (f32::from(bottom.size.height) - Layout::BOTTOM_WORKSPACE_HEIGHT).abs() <= 1.0,
            "bottom dock uses the frozen 272px default"
        );
        assert!(
            visual.debug_bounds("environment-rail").is_some(),
            "bottom dock remains a sibling of center plus Environment"
        );
    }

    #[gpui_kit::test]
    async fn terminal_workspace_production_handlers_preserve_docking_and_project_isolation(
        cx: &mut TestAppContext,
    ) {
        const MARKER: &str = "VEGA_R11_WORKSPACE_CHILD";
        let Some(path) = std::env::var_os(MARKER) else {
            let root = tempfile::tempdir().unwrap();
            let output=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","window::workspace::terminal_tests::terminal_workspace_production_handlers_preserve_docking_and_project_isolation","--nocapture"])
                .env(MARKER,root.path()).env("HOME",root.path()).env("ZDOTDIR",root.path()).output().unwrap();
            assert!(
                output.status.success(),
                "owned workspace subprocess failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        };
        let path = std::path::PathBuf::from(path);
        let store = vega_store::Store::open(path.join("test.db")).unwrap();
        store.migrate().unwrap();
        let first =
            vega_store::projects::create(store.conn(), path.to_str().unwrap(), "first", None)
                .unwrap();
        let other = path.join("other");
        std::fs::create_dir(&other).unwrap();
        let second =
            vega_store::projects::create(store.conn(), other.to_str().unwrap(), "second", None)
                .unwrap();
        let thread =
            vega_conversation::threads::create_thread(&store, &first.id, "mock", "confirm")
                .unwrap();
        cx.update(|cx| {
            install_diff_window_globals(store, thread, cx);
            cx.set_global(SelectedProject(Some(first.id.clone())));
        });
        let root = cx.new(VegaWindow::new);
        let entity = root.clone();
        let window = cx.update(|cx| {
            cx.open_window(
                gpui_kit::WindowOptions {
                    window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds::new(
                        gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(0.)),
                        gpui_kit::size(gpui_kit::px(960.), gpui_kit::px(600.)),
                    ))),
                    ..Default::default()
                },
                move |_, _| entity,
            )
            .unwrap()
        });
        cx.run_until_parked();
        window
            .update(cx, |root, window, cx| {
                let file = cx.new(|cx| {
                    vega_ui::file_preview::FilePreview::new(
                        vega_conversation::types::PaletteFilePreview {
                            relative_path: "README.md".into(),
                            content: "owned read-only preview".into(),
                        },
                        cx,
                    )
                });
                root.workspace_open_file(file, window, cx);
                root.workspace_open_terminal(false, window, cx)
            })
            .unwrap();
        let (key, entity_id) = root.read_with(cx, |root, _| {
            let key = root.workspace.selected[0].clone().unwrap();
            let TabKey::Terminal(id) = key else {
                panic!("terminal tab")
            };
            (key, root.workspace.terminals[&id].view.entity_id())
        });
        window
            .update(cx, |root, window, cx| {
                root.workspace_move_selected(0, window, cx);
                assert_eq!(root.workspace.selected[1], Some(key.clone()));
                root.workspace_hide(1, window, cx);
                root.workspace_toggle_terminal(window, cx);
                assert!(!root.workspace.hidden[1]);
                let TabKey::Terminal(id) = key else {
                    panic!("terminal tab")
                };
                assert_eq!(root.workspace.terminals[&id].view.entity_id(), entity_id);
                assert!(
                    root.workspace.terminals[&id]
                        .view
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |root, window, cx| {
                root.workspace_move_selected(1, window, cx)
            })
            .unwrap();
        cx.run_until_parked();
        root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.selected[0], Some(key.clone()));
            let position = root
                .workspace
                .tabs
                .iter()
                .filter(|(_, bottom)| !*bottom)
                .position(|(tab, _)| tab == &key)
                .unwrap();
            assert!(position > 0, "existing README tab retains its order");
            let scroll = &root.workspace.tab_scroll[0];
            let viewport = scroll.bounds();
            let tab = scroll.bounds_for_item(position).unwrap();
            assert!(viewport.size.width > gpui_kit::px(0.));
            assert!(
                scroll.offset().x < gpui_kit::px(0.),
                "selected terminal was revealed by horizontal scrolling"
            );
            assert!(
                tab.left() + scroll.offset().x >= viewport.left() - gpui_kit::px(1.),
                "selected label visible"
            );
            assert!(
                tab.right() + scroll.offset().x <= viewport.right() + gpui_kit::px(1.),
                "selected close control visible"
            );
        });
        window
            .update(cx, |root, window, cx| {
                root.workspace_open_terminal(false, window, cx);
                assert_eq!(root.workspace.terminals.len(), 2);
                cx.set_global(SelectedProject(Some(second.id.clone())));
                root.sync_workspace_route(cx);
                assert!(
                    root.workspace
                        .tabs
                        .iter()
                        .all(|(key, _)| !matches!(key, TabKey::Terminal(_)))
                );
                assert_eq!(
                    root.workspace.terminals.len(),
                    2,
                    "hidden project sessions retained"
                );
                cx.set_global(SelectedProject(Some(first.id.clone())));
                root.sync_workspace_route(cx);
                assert!(root.workspace.tabs.iter().any(|(tab, _)| tab == &key));
                let keys = root
                    .workspace
                    .tabs
                    .iter()
                    .filter(|(key, _)| matches!(key, TabKey::Terminal(_)))
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>();
                for key in keys {
                    root.workspace_close_tab(&key, window, cx);
                }
                assert!(root.workspace.terminals.is_empty());
            })
            .unwrap();
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
