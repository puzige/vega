use super::*;
use vega_ui::artifact_card::ArtifactCard;
use vega_ui::icons::{Icon, icon_button};

const MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH: f32 = 610.;
const MIN_BOTTOM_WORKSPACE_WINDOW_HEIGHT: f32 = 480.;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TabKey {
    Diff,
    Terminal(u64),
    File(u64),
    Artifact(ArtifactCardId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceCreateAction {
    Review,
    NewTerminal,
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
        let Some(position) = self.tabs.iter().position(|(tab, _)| tab == key) else {
            return;
        };
        let pane = usize::from(self.tabs[position].1);
        let pane_position = self.tabs[..position]
            .iter()
            .filter(|(_, bottom)| usize::from(*bottom) == pane)
            .count();
        self.tabs.remove(position);
        if self.selected[pane].as_ref() == Some(key) {
            self.reveal_tabs[pane] = true;
            let mut siblings = self
                .tabs
                .iter()
                .filter(|(_, bottom)| usize::from(*bottom) == pane)
                .map(|(tab, _)| tab);
            self.selected[pane] = siblings
                .clone()
                .nth(pane_position)
                .or_else(|| siblings.next_back())
                .cloned();
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
        self.workspace_create_terminal(bottom, true, window, cx);
    }

    fn workspace_create_terminal(
        &mut self,
        bottom: bool,
        activate_terminal: bool,
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
        if activate_terminal {
            self.workspace_focus(usize::from(bottom), window, cx);
        } else {
            self.workspace_focus_composer(window, cx);
        }
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

    /// Whether the current project's most recent terminal tab is actually
    /// rendered in its dock (R44 rendered predicate, maximized included).
    /// Drives the selected surface of the R45 bottom-dock shell slot.
    pub(super) fn workspace_recent_terminal_is_rendered(&self, window: &Window, cx: &App) -> bool {
        self.workspace
            .tabs
            .iter()
            .rev()
            .find(|(key, _)| matches!(key, TabKey::Terminal(_)))
            .is_some_and(|(key, bottom)| {
                self.workspace_terminal_is_rendered(key, *bottom, window, cx)
            })
    }

    /// Whether the bottom dock's selected tab is actually rendered (maximized
    /// counts as rendered, a narrow window below the bottom minimum does not),
    /// regardless of whether the tab is a terminal or workspace content.
    pub(super) fn bottom_workspace_selected_is_rendered(&self, window: &Window) -> bool {
        !self.workspace.hidden[1]
            && self.workspace.selected[1].is_some()
            && (self.workspace.maximized[1]
                || f32::from(window.bounds().size.height) >= MIN_BOTTOM_WORKSPACE_WINDOW_HEIGHT)
    }

    /// Whether the right dock could render its selected tab at the current
    /// responsive width.
    fn right_workspace_renderable(&self, window: &Window, cx: &App) -> bool {
        self.workspace.maximized[0]
            || self.workspace_available_width(window, cx) >= MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH
    }

    /// Whether the right dock is actually rendered at all (R44 rendered
    /// predicate: not hidden, a selected tab present, and the responsive
    /// width guard satisfied or maximized).
    fn right_workspace_rendered(&self, window: &Window, cx: &App) -> bool {
        !self.workspace.hidden[0]
            && self.workspace.selected[0].is_some()
            && self.right_workspace_renderable(window, cx)
    }

    /// Whether the right dock's selected tab is actually rendered and is not
    /// a terminal. A rendered right terminal is owned by the bottom-dock
    /// shell slot, so the right slot must never act on it.
    pub(super) fn right_workspace_rendered_non_terminal(&self, window: &Window, cx: &App) -> bool {
        !self.workspace.selected[0]
            .as_ref()
            .is_some_and(|selected| matches!(selected, TabKey::Terminal(_)))
            && self.right_workspace_rendered(window, cx)
    }

    /// Whether the R45 right-dock shell slot can act at all: hide a rendered
    /// non-terminal selection, restore a hidden non-terminal tab, or open the
    /// Review diff when the dock is idle and renderable. Mirrors the branch
    /// order of [`VegaWindow::workspace_toggle_right`].
    pub(super) fn right_workspace_slot_available(&self, window: &Window, cx: &App) -> bool {
        self.right_workspace_rendered_non_terminal(window, cx)
            || self.hidden_workspace_available(0)
            || (self.shell_project_thread(cx).is_some()
                && !self.right_workspace_rendered(window, cx)
                && self.right_workspace_renderable(window, cx))
    }

    /// One deterministic bottom-dock toggle behind the R45 shell slot and
    /// ⌘J: hide the rendered selection, reveal a hidden selected tab, apply
    /// the R44 terminal reveal/migration, or create the first terminal. Every
    /// branch keeps the R44 focus contract (Composer stays focused).
    pub(crate) fn workspace_toggle_bottom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_workspace_route(cx);
        if self.bottom_workspace_selected_is_rendered(window) {
            self.workspace_hide(1, window, cx);
            return;
        }
        if self.workspace.hidden[1] && self.workspace.selected[1].is_some() {
            self.environment_overlay_open = false;
            self.workspace.hidden[1] = false;
            self.workspace.reveal_tabs[1] = true;
            self.workspace_focus_composer(window, cx);
            cx.notify();
            return;
        }
        if let Some((key, bottom)) = self
            .workspace
            .tabs
            .iter()
            .rev()
            .find(|(key, _)| matches!(key, TabKey::Terminal(_)))
            .cloned()
        {
            // R44 terminal toggle semantics, verbatim: a rendered terminal
            // hides its pane; otherwise the same tab (and PTY) reveals,
            // migrating an unavailable-right terminal to the bottom dock.
            if self.workspace_terminal_is_rendered(&key, bottom, window, cx) {
                self.workspace_hide(usize::from(bottom), window, cx);
            } else {
                self.workspace_reveal_terminal_tab(key, bottom, window, cx);
            }
        } else {
            self.workspace_create_terminal(true, false, window, cx);
        }
        cx.notify();
    }

    /// One deterministic right-dock toggle behind the R45 shell slot: hide a
    /// rendered non-terminal selection, restore a hidden non-terminal tab, or
    /// open the Review diff for the current project thread. A rendered right
    /// terminal is owned by the bottom-dock slot and leaves this no-op.
    pub(crate) fn workspace_toggle_right(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_workspace_route(cx);
        if self.right_workspace_rendered_non_terminal(window, cx) {
            self.workspace_hide(0, window, cx);
            return;
        }
        if self.hidden_workspace_available(0) {
            self.restore_hidden_workspace(0, window, cx);
            return;
        }
        if self.shell_project_thread(cx).is_some()
            && !self.right_workspace_rendered(window, cx)
            && self.right_workspace_renderable(window, cx)
        {
            self.environment_overlay_open = false;
            self.workspace_open_diff(cx);
            cx.notify();
        }
    }

    /// Escape dismissal of the narrow Environment overlay (R45): closes the
    /// card and returns focus to the Composer. The rail modality has no
    /// Escape path, and a closed overlay never consumes the key so scoped
    /// component escapes keep their precedence.
    pub(super) fn dismiss_environment_overlay(
        &mut self,
        _: &vega_ui::DismissEnvironmentOverlay,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.environment_is_wide(window, cx) || !self.environment_overlay_open {
            cx.propagate();
            return;
        }
        self.environment_overlay_open = false;
        self.workspace_focus_composer(window, cx);
        cx.stop_propagation();
        cx.notify();
    }

    /// Idempotently reveal the current project's terminal without activating its PTY.
    fn workspace_reveal_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_workspace_route(cx);
        if let Some((key, bottom)) = self
            .workspace
            .tabs
            .iter()
            .rev()
            .find(|(key, _)| matches!(key, TabKey::Terminal(_)))
            .cloned()
        {
            self.workspace_reveal_terminal_tab(key, bottom, window, cx);
        } else {
            self.workspace_create_terminal(true, false, window, cx);
        }
        cx.notify();
    }

    fn workspace_terminal_is_rendered(
        &self,
        key: &TabKey,
        bottom: bool,
        window: &Window,
        cx: &App,
    ) -> bool {
        let index = usize::from(bottom);
        if self.workspace.hidden[index] || self.workspace.selected[index].as_ref() != Some(key) {
            return false;
        }
        self.workspace.maximized[index]
            || if bottom {
                f32::from(window.bounds().size.height) >= MIN_BOTTOM_WORKSPACE_WINDOW_HEIGHT
            } else {
                self.workspace_available_width(window, cx) >= MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH
            }
    }

    fn workspace_reveal_terminal_tab(
        &mut self,
        key: TabKey,
        bottom: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let unavailable_right = !bottom
            && !self.workspace.maximized[0]
            && self.workspace_available_width(window, cx) < MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH;
        if unavailable_right {
            self.workspace.close(&key);
            self.workspace.tabs.push((key.clone(), true));
            if let TabKey::Terminal(id) = &key
                && let Some(terminal) = self.workspace.terminals.get_mut(id)
            {
                terminal.bottom = true;
            }
            self.workspace.selected[1] = Some(key);
            self.workspace.reveal_tabs[1] = true;
            self.workspace.hidden[1] = false;
            self.workspace.maximized[1] = false;
            self.workspace.menu = false;
        } else {
            self.workspace.open(key);
        }
        self.workspace_focus_composer(window, cx);
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
            self.workspace_focus_composer(window, cx);
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
            self.workspace_focus_composer(window, cx);
        }
        cx.notify();
    }

    fn workspace_creation_actions(&self, cx: &App) -> Vec<WorkspaceCreateAction> {
        let mut actions = Vec::with_capacity(2);
        if self.shell_project_thread(cx).is_some() {
            actions.push(WorkspaceCreateAction::Review);
        }
        if self.shell_project_id(cx).is_some()
            && self.file_backed_store_path(cx).is_some()
            && self.workspace.terminals.len() < 8
        {
            actions.push(WorkspaceCreateAction::NewTerminal);
        }
        actions
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
            && self.workspace_available_width(window, cx) >= MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH
    }

    pub(super) fn hidden_workspace_available(&self, index: usize) -> bool {
        self.workspace.hidden[index]
            && self.workspace.selected[index]
                .as_ref()
                .is_some_and(|selected| !matches!(selected, TabKey::Terminal(_)))
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

    /// The dock index currently rendered as fullscreen chrome (R44), if any.
    /// While a dock is fullscreen the R44 shell unmounts the header row, so
    /// the R46 window-anchored cluster unmounts with it: the maximized pane's
    /// own trailing controls own the window's top-right corner then.
    pub(super) fn workspace_fullscreen_index(&self) -> Option<usize> {
        (0..2).find(|index| {
            self.workspace.maximized[*index]
                && !self.workspace.hidden[*index]
                && self.workspace.selected[*index].is_some()
        })
    }

    /// Whether this dock's header row is rendered inside the window's 46px top
    /// band (R46 §2.1.1). The docked right pane spans the full window height
    /// beside the conversation column, so its header opens the top band; a
    /// maximized dock replaces the whole workspace layout, so its header is in
    /// the top band too. The bottom-docked pane header sits in the bottom band.
    /// Top-band headers must reserve the shell slot cluster's trailing band.
    pub(super) fn workspace_pane_header_in_top_band(&self, bottom: bool) -> bool {
        !bottom || self.workspace_fullscreen_index() == Some(usize::from(bottom))
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
                this.workspace_reveal_terminal(window, cx);
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
            // R46 §2.2: rail and overlay share one top offset, so neither card
            // can paint over the 46px main header band (or the window-anchored
            // shell slots inside it). The R21 16px card inset is preserved
            // below that band instead of from the raw column top.
            .pt(px(
                Layout::MAIN_HEADER_HEIGHT + Layout::ENVIRONMENT_CARD_INSET
            ))
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
        let tabs_selector = if bottom { "bottom-tabs" } else { "right-tabs" };
        let header_selector = if bottom {
            "bottom-workspace-header"
        } else {
            "right-workspace-header"
        };
        let actions_selector = if bottom {
            "bottom-workspace-actions"
        } else {
            "right-workspace-actions"
        };
        let add_selector = if bottom {
            "bottom-workspace-add"
        } else {
            "right-workspace-add"
        };
        let dock_selector = if bottom {
            "bottom-workspace-dock"
        } else {
            "right-workspace-dock"
        };
        let maximize_selector = if bottom {
            "bottom-workspace-maximize"
        } else {
            "right-workspace-maximize"
        };
        let hide_selector = if bottom {
            "bottom-workspace-hide"
        } else {
            "right-workspace-hide"
        };
        let mut strip = div()
            .id(tabs_selector)
            .debug_selector(move || tabs_selector.into())
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
            let tab_selector = SharedString::from(format!("workspace-tab-{key:?}"));
            let close_selector = SharedString::from(format!("workspace-tab-close-{key:?}"));
            strip = strip.child(
                div()
                    .id(tab_selector.clone())
                    .debug_selector(move || tab_selector.to_string())
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
                    .child(
                        icon_button(
                            Icon::Close,
                            format!("关闭 {}", self.workspace_label(&close_key, cx)),
                            colors,
                            cx.listener(move |this, _, window, cx| {
                                this.workspace_close_tab(&close_key, window, cx);
                            }),
                        )
                        .debug_selector(move || close_selector.to_string()),
                    ),
            );
        }
        let mut actions = div()
            .debug_selector(move || actions_selector.into())
            .flex()
            .items_center()
            .gap_1()
            .flex_shrink_0();
        if review_selected {
            actions = actions.child(
                icon_button(
                    Icon::Document,
                    "提交更改",
                    colors,
                    cx.listener(|this, _, _, cx| this.workspace_open_commit(cx)),
                )
                .debug_selector(|| "workspace-review-commit".into()),
            );
        }
        let actions = actions
            .child(
                icon_button(
                    Icon::Plus,
                    "新建工作区标签",
                    colors,
                    cx.listener(move |this, _, window, cx| {
                        this.workspace_toggle_menu(bottom, window, cx)
                    }),
                )
                .debug_selector(move || add_selector.into()),
            )
            .child(
                icon_button(
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
                )
                .debug_selector(move || dock_selector.into()),
            )
            .child(
                icon_button(
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
                )
                .debug_selector(move || maximize_selector.into()),
            );
        // R46 §2.1.1: a pane header that opens the window's 46px top band
        // reserves the window-anchored slot cluster's trailing band, so the
        // pane's own trailing actions (commit / add / dock / maximize) lay out
        // to the left of the slots instead of underneath them. The reserved
        // width is the same token `main-header` uses. The bottom-docked pane
        // header lives in the bottom band and reserves nothing.
        let header = div()
            .debug_selector(move || header_selector.into())
            .flex()
            .items_center()
            .gap_1()
            .h(px(Layout::WORKSPACE_HEADER_HEIGHT))
            .px_1()
            .when(self.workspace_pane_header_in_top_band(bottom), |header| {
                header.pr(px(Layout::SHELL_SLOT_CLUSTER_RESERVE))
            })
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                icon_button(
                    if bottom {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                    "隐藏面板",
                    colors,
                    cx.listener(move |this, _, window, cx| this.workspace_hide(index, window, cx)),
                )
                .debug_selector(move || hide_selector.into()),
            )
            .child(strip)
            .child(actions);
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
            && f32::from(size.height) >= MIN_BOTTOM_WORKSPACE_WINDOW_HEIGHT;
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
        // R46 §2.1.1 keeps one owner for "which dock is fullscreen chrome":
        // the same helper decides the pane header's top-band reservation.
        let fullscreen = self.workspace_fullscreen_index();
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
            layout = layout
                .child(
                    // R45 dismissal backdrop: transparent, painted under the
                    // overlay card but over the rest of the shell. A left
                    // press outside the card closes the overlay and returns
                    // focus to the Composer; the press itself keeps
                    // propagating so the underlying control still works.
                    div()
                        .debug_selector(|| "environment-overlay-backdrop".into())
                        .absolute()
                        .inset_0()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.environment_overlay_open = false;
                                this.workspace_focus_composer(window, cx);
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .w(px(Layout::ENVIRONMENT_RAIL_WIDTH))
                        .h_full()
                        // Keep card presses off the dismissal backdrop so
                        // in-card rows keep their R44 mouse-up activation.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
            let creation_actions = self.workspace_creation_actions(cx);
            let focus = self
                .workspace
                .menu_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            let mut menu = div()
                .id("workspace-add-menu")
                .debug_selector(|| "workspace-add-menu".into())
                .track_focus(&focus)
                .key_context("WorkspaceMenu")
                .on_key_down(cx.listener(|_this, event: &KeyDownEvent, window, cx| {
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
                        _ => {}
                    }
                }))
                .on_action(cx.listener(|this, _: &CloseSettings, window, cx| {
                    this.workspace.menu = false;
                    this.workspace_focus_composer(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }))
                .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.workspace.menu = false;
                    this.workspace_focus_composer(window, cx);
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
                .gap_1();
            for action in creation_actions {
                menu = match action {
                    WorkspaceCreateAction::Review => menu.child(workspace_button(
                        "workspace-add-review",
                        "Review · 工作区变更",
                        colors,
                        cx.listener(|this, _, _, cx| {
                            this.workspace_open_diff(cx);
                            this.workspace.menu = false;
                            cx.notify();
                        }),
                    )),
                    WorkspaceCreateAction::NewTerminal => menu.child(workspace_button(
                        "workspace-add-terminal",
                        "终端 · 新建登录 shell",
                        colors,
                        cx.listener(|this, _, window, cx| {
                            this.workspace_open_terminal(this.workspace.menu_bottom, window, cx);
                        }),
                    )),
                };
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
    selector: &'static str,
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
        .debug_selector(move || selector.into())
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
    use super::{TabKey, VegaWindow, Workspace};
    use crate::tests::{diff_controller_repo, install_diff_window_globals};
    use gpui_kit::prelude::*;
    use gpui_kit::{
        AppContext, Bounds, Focusable, KeyBinding, Modifiers, MouseButton, Pixels, TestAppContext,
        VisualTestContext, WindowBounds, WindowHandle, WindowOptions, point, px, size,
    };
    use vega_theme::{Layout, Typography};
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

    #[test]
    fn r44_closing_selected_tab_chooses_the_nearest_same_pane_sibling() {
        let mut workspace = Workspace {
            tabs: vec![
                (TabKey::Terminal(1), false),
                (TabKey::Terminal(20), true),
                (TabKey::Terminal(2), false),
                (TabKey::Terminal(3), false),
            ],
            selected: [Some(TabKey::Terminal(2)), Some(TabKey::Terminal(20))],
            ..Default::default()
        };

        workspace.close(&TabKey::Terminal(2));
        assert_eq!(workspace.selected[0], Some(TabKey::Terminal(3)));
        assert_eq!(workspace.selected[1], Some(TabKey::Terminal(20)));

        workspace.close(&TabKey::Terminal(3));
        assert_eq!(workspace.selected[0], Some(TabKey::Terminal(1)));
        assert_eq!(workspace.selected[1], Some(TabKey::Terminal(20)));
    }

    fn assert_sidebar_footer_geometry(window: WindowHandle<VegaWindow>, cx: &mut TestAppContext) {
        let sidebar = shell_bounds(window, "sidebar", cx);
        let new_task = shell_bounds(window, "sidebar-new-task", cx);
        let surface = shell_bounds(window, "sidebar-settings-surface", cx);
        let button = shell_bounds(window, "sidebar-settings", cx);
        assert_close(
            surface.left() - sidebar.left(),
            Layout::SIDEBAR_PADDING,
            "painted Settings surface keeps the Sidebar left inset",
        );
        assert_close(
            sidebar.right() - surface.right(),
            Layout::SIDEBAR_PADDING,
            "painted Settings surface keeps the Sidebar right inset",
        );
        assert_close(
            sidebar.bottom() - surface.bottom(),
            Layout::SIDEBAR_PADDING,
            "painted Settings surface keeps the Sidebar bottom inset",
        );
        assert_close(
            surface.size.height,
            Typography::SIDEBAR_LINE_HEIGHT,
            "painted Settings surface height",
        );
        assert_close(
            button.left() - surface.left(),
            0.0,
            "interactive Settings button fills the surface on the left",
        );
        assert_close(
            button.right() - surface.right(),
            0.0,
            "interactive Settings button fills the surface on the right",
        );
        assert_close(
            button.top() - surface.top(),
            0.0,
            "interactive Settings button fills the surface on the top",
        );
        assert_close(
            button.bottom() - surface.bottom(),
            0.0,
            "interactive Settings button fills the surface on the bottom",
        );
        assert_close(
            new_task.left() - sidebar.left(),
            Layout::SIDEBAR_PADDING,
            "New Task keeps the Sidebar left inset",
        );
        assert_close(
            sidebar.right() - new_task.right(),
            Layout::SIDEBAR_PADDING,
            "New Task keeps the Sidebar right inset",
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
        assert_sidebar_footer_geometry(window, cx);
        shell_click(window, "sidebar-settings", cx);
        assert!(
            cx.update(|cx| cx.global::<SettingsOpen>().0),
            "Settings footer keeps its production General Settings route"
        );
        cx.update(|cx| {
            cx.set_global(SettingsOpen(false));
            cx.refresh_windows();
        });
        cx.run_until_parked();
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
        // R46 §2.2 updates this R21 assertion: the rail card keeps its 16px
        // card inset, but that inset now measures from the header band's
        // bottom border instead of the raw column top, so the card can no
        // longer paint over the 46px header row (or the shell slots in it).
        assert_close(
            card.top() - rail.top(),
            Layout::MAIN_HEADER_HEIGHT + Layout::ENVIRONMENT_CARD_INSET,
            "Environment card top inset below the header band",
        );
        assert_close(
            card.top() - header.bottom(),
            Layout::ENVIRONMENT_CARD_INSET,
            "Environment card keeps its R21 16px inset below the header",
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
            "main-header-terminal",
            "main-header-environment",
            "main-header-workspace-right",
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
        assert_sidebar_footer_geometry(window, cx);
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
        assert_sidebar_footer_geometry(window, cx);
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
        let actions = shell_bounds(window, "right-workspace-actions", cx);
        let commit = shell_bounds(window, "workspace-review-commit", cx);
        assert_close(actions.size.width, 108.0, "Review workspace trailing group");
        assert_close(
            commit.left() - actions.left(),
            0.0,
            "Review commit begins the trailing group",
        );
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
        for selector in ["main-header-project", "environment-rail"] {
            assert!(
                shell_absent(window, selector, cx),
                "standalone route must fence {selector}"
            );
        }
        // R45 slot model supersedes conditional header membership: the three
        // shell slots render at every route, and on a standalone route they
        // render disabled, so clicking them changes no workspace state.
        for selector in [
            "main-header-environment",
            "main-header-terminal",
            "main-header-workspace-right",
        ] {
            let _ = shell_bounds(window, selector, cx);
        }
        shell_click(window, "main-header-terminal", cx);
        shell_click(window, "main-header-environment", cx);
        shell_click(window, "main-header-workspace-right", cx);
        assert!(
            root.read_with(cx, |root, _| {
                root.workspace.terminals.is_empty()
                    && root.workspace.selected[1].is_none()
                    && root.workspace.hidden == [false, false]
                    && !root.environment_overlay_open
                    && root.diff_controller.active.is_none()
            }),
            "standalone route must keep the disabled shell slots inert"
        );

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
        // R45: the shell slots survive project-less routes as disabled
        // placeholders whose clicks change no workspace state.
        for selector in [
            "main-header-environment",
            "main-header-terminal",
            "main-header-workspace-right",
        ] {
            let _ = shell_bounds(window, selector, cx);
        }
        shell_click(window, "main-header-terminal", cx);
        shell_click(window, "main-header-environment", cx);
        assert!(
            root.read_with(cx, |root, _| root.workspace.terminals.is_empty()
                && !root.environment_overlay_open),
            "project-less route must keep the disabled shell slots inert"
        );
    }

    fn r45_mount_project_window(
        repo: &tempfile::TempDir,
        label: &str,
        width: f32,
        height: f32,
        cx: &mut TestAppContext,
    ) -> (gpui_kit::Entity<VegaWindow>, WindowHandle<VegaWindow>) {
        let store = vega_store::Store::open(":memory:").expect("owned store");
        store.migrate().expect("owned migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("fixture path"),
            label,
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
                        size(px(width), px(height)),
                    ))),
                    ..Default::default()
                },
                move |_, _| window_root,
            )
            .expect("production root window")
        });
        cx.run_until_parked();
        (root, window)
    }

    fn r45_assert_three_shell_slots(window: WindowHandle<VegaWindow>, cx: &mut TestAppContext) {
        let environment = shell_bounds(window, "main-header-environment", cx);
        let terminal = shell_bounds(window, "main-header-terminal", cx);
        let right = shell_bounds(window, "main-header-workspace-right", cx);
        for (name, bounds) in [
            ("environment", environment),
            ("terminal", terminal),
            ("workspace-right", right),
        ] {
            assert_close(
                bounds.size.width,
                Layout::TITLEBAR_CONTROL_SIZE,
                &format!("{name} slot width"),
            );
            assert_close(
                bounds.size.height,
                Layout::TITLEBAR_CONTROL_SIZE,
                &format!("{name} slot height"),
            );
        }
        assert!(
            environment.left() < terminal.left() && terminal.left() < right.left(),
            "slots keep the environment → terminal → right order"
        );
        // Codex-measured 34px centers = one 28px slot plus one 6px gap.
        assert_close(
            terminal.center().x - environment.center().x,
            34.0,
            "environment→terminal center distance",
        );
        assert_close(
            right.center().x - terminal.center().x,
            34.0,
            "terminal→right center distance",
        );
    }

    /// R46 §2.1: the three slots are a window-level trailing cluster, so every
    /// panel state must leave them at the exact same coordinates. Returns the
    /// three bounds so callers can compare states against each other.
    fn r46_shell_slot_bounds(
        window: WindowHandle<VegaWindow>,
        cx: &mut TestAppContext,
    ) -> [Bounds<Pixels>; 3] {
        [
            shell_bounds(window, "main-header-environment", cx),
            shell_bounds(window, "main-header-terminal", cx),
            shell_bounds(window, "main-header-workspace-right", cx),
        ]
    }

    fn r46_assert_slots_identical(
        expected: &[Bounds<Pixels>; 3],
        actual: &[Bounds<Pixels>; 3],
        state: &str,
    ) {
        for (index, (expected, actual)) in expected.iter().zip(actual.iter()).enumerate() {
            for (axis, expected, actual) in [
                ("left", expected.left(), actual.left()),
                ("right", expected.right(), actual.right()),
                ("top", expected.top(), actual.top()),
                ("bottom", expected.bottom(), actual.bottom()),
            ] {
                assert_close(
                    actual,
                    f32::from(expected),
                    &format!(
                        "slot {} {axis} must not move in the {state} state",
                        index + 1
                    ),
                );
            }
        }
    }

    #[gpui_kit::test]
    async fn r46_slot_cluster_is_window_anchored(cx: &mut TestAppContext) {
        let repo = diff_controller_repo();
        let (root, window) = r45_mount_project_window(&repo, "R46 anchored", 1403., 860., cx);

        // a) No panel: the baseline the five other states must match exactly.
        let baseline = r46_shell_slot_bounds(window, cx);
        r45_assert_three_shell_slots(window, cx);

        // b) Environment rail open. The wide project route renders the 320px
        // rail by default, so closing and reopening it proves the rail's own
        // open/close transition cannot drag the cluster.
        shell_click(window, "main-header-environment", cx);
        assert!(shell_absent(window, "environment-rail", cx));
        r46_assert_slots_identical(
            &baseline,
            &r46_shell_slot_bounds(window, cx),
            "Environment rail closed",
        );
        shell_click(window, "main-header-environment", cx);
        let _ = shell_bounds(window, "environment-rail", cx);
        r46_assert_slots_identical(
            &baseline,
            &r46_shell_slot_bounds(window, cx),
            "Environment rail open",
        );

        // c) Right pane open (Review diff).
        root.update(cx, |root, cx| root.workspace_open_diff(cx));
        cx.run_until_parked();
        let _ = shell_bounds(window, "right-workspace-pane", cx);
        r46_assert_slots_identical(
            &baseline,
            &r46_shell_slot_bounds(window, cx),
            "right pane open",
        );

        // d) Bottom dock open: move the Review tab down, then restore the
        // right pane so the states stay independent.
        window
            .update(cx, |root, window, cx| {
                root.workspace_move_selected(0, window, cx)
            })
            .expect("move the Review tab to the bottom dock");
        cx.run_until_parked();
        let _ = shell_bounds(window, "bottom-workspace-pane", cx);
        assert!(shell_absent(window, "right-workspace-pane", cx));
        r46_assert_slots_identical(
            &baseline,
            &r46_shell_slot_bounds(window, cx),
            "bottom dock open",
        );

        // e) Right and bottom docks open together: the file preview joins the
        // right dock while the Review tab stays docked at the bottom.
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
            })
            .expect("open both docks");
        cx.run_until_parked();
        let _ = shell_bounds(window, "right-workspace-pane", cx);
        let _ = shell_bounds(window, "bottom-workspace-pane", cx);
        r46_assert_slots_identical(
            &baseline,
            &r46_shell_slot_bounds(window, cx),
            "right and bottom docks open",
        );

        // f) Environment overlay: only reachable below the breakpoint, so the
        // narrow window keeps its own baseline and the overlay must not move
        // the cluster relative to that window's closed state. The docks are
        // closed first because a rendered right pane replaces the Environment
        // surface entirely (R21).
        window
            .update(cx, |root, window, cx| {
                root.workspace_hide(0, window, cx);
                root.workspace_hide(1, window, cx);
            })
            .expect("close both docks");
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(1100.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("resize below the Environment breakpoint");
        cx.run_until_parked();
        assert!(shell_absent(window, "environment-rail", cx));
        let narrow_baseline = r46_shell_slot_bounds(window, cx);
        shell_click(window, "main-header-environment", cx);
        let _ = shell_bounds(window, "environment-overlay", cx);
        r46_assert_slots_identical(
            &narrow_baseline,
            &r46_shell_slot_bounds(window, cx),
            "Environment overlay open",
        );
        shell_click(window, "main-header-environment", cx);
        assert!(shell_absent(window, "environment-overlay", cx));
        r46_assert_slots_identical(
            &narrow_baseline,
            &r46_shell_slot_bounds(window, cx),
            "Environment overlay closed",
        );

        // §2.1 narrow clause: the trailing inset is the only geometry input,
        // so the distance from the window's right edge must be size-invariant.
        let trailing = |slots: &[Bounds<Pixels>; 3], width: f32| {
            slots
                .iter()
                .map(|slot| width - f32::from(slot.right()))
                .collect::<Vec<_>>()
        };
        let wide_trailing = trailing(&baseline, 1403.);
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(960.), px(600.)));
                window.bounds_changed(cx);
            })
            .expect("resize to the minimum window size");
        cx.run_until_parked();
        let narrow = r46_shell_slot_bounds(window, cx);
        r45_assert_three_shell_slots(window, cx);
        assert_close(
            narrow[2].right(),
            960.0 - wide_trailing[2],
            "960×600 keeps the 1403×860 trailing inset",
        );
        for (index, (wide, narrow)) in wide_trailing
            .iter()
            .zip(trailing(&narrow, 960.))
            .enumerate()
        {
            assert_close(
                px(narrow),
                *wide,
                &format!("slot {} trailing inset is size-invariant", index + 1),
            );
        }
    }

    #[gpui_kit::test]
    async fn r46_overlay_and_rail_start_below_header_band(cx: &mut TestAppContext) {
        let repo = diff_controller_repo();
        let (root, window) = r45_mount_project_window(&repo, "R46 header band", 1403., 860., cx);
        let _ = root;
        let header = shell_bounds(window, "main-header", cx);
        assert_close(
            header.size.height,
            Layout::MAIN_HEADER_HEIGHT,
            "main header keeps its 46px band",
        );
        let slots = r46_shell_slot_bounds(window, cx);

        // Wide rail: the card hangs below the header band and clears the
        // window-anchored cluster entirely.
        let rail_card = shell_bounds(window, "environment-card", cx);
        assert!(
            rail_card.top() >= header.bottom(),
            "rail card top ({:?}) must not cross the header bottom ({:?})",
            rail_card.top(),
            header.bottom()
        );
        assert!(
            !rail_card.intersects(&slots[2]),
            "rail card must not intersect the shell slots"
        );
        for (index, slot) in slots.iter().enumerate() {
            assert!(
                !rail_card.intersects(slot),
                "rail card must not intersect slot {} in a wide window",
                index + 1
            );
        }

        // Narrow overlay: the card starts below the same band and clears the
        // cluster, even though both are pinned to the window's right edge.
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(1100.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("resize below the Environment breakpoint");
        cx.run_until_parked();
        shell_click(window, "main-header-environment", cx);
        let overlay = shell_bounds(window, "environment-overlay", cx);
        let overlay_card = shell_bounds(window, "environment-overlay-card", cx);
        assert!(
            overlay_card.top() >= header.bottom(),
            "overlay card top ({:?}) must not cross the header bottom ({:?})",
            overlay_card.top(),
            header.bottom()
        );
        assert_close(
            overlay_card.top() - overlay.top(),
            Layout::MAIN_HEADER_HEIGHT + Layout::ENVIRONMENT_CARD_INSET,
            "overlay card keeps the R21 16px inset below the header band",
        );
        let narrow_slots = r46_shell_slot_bounds(window, cx);
        for (index, slot) in narrow_slots.iter().enumerate() {
            assert!(
                !overlay_card.intersects(slot),
                "overlay card must not intersect slot {} in a narrow window",
                index + 1
            );
        }
        // The backdrop keeps its full-window dismissal semantics.
        let backdrop = shell_bounds(window, "environment-overlay-backdrop", cx);
        assert_close(backdrop.top(), 0.0, "overlay backdrop keeps the window top");
        assert_close(
            backdrop.size.height,
            860.0,
            "overlay backdrop keeps the full window height",
        );
    }

    /// R46 §2.1.1: every pane header that occupies the window's top band
    /// reserves the window-anchored cluster's trailing band, so its own
    /// trailing actions lay out to the left of the slots. The bottom band
    /// reserves nothing because the cluster never occupies it.
    #[gpui_kit::test]
    async fn r46_top_band_headers_reserve_the_cluster(cx: &mut TestAppContext) {
        let repo = diff_controller_repo();
        let (root, window) = r45_mount_project_window(&repo, "R46 top band", 1403., 860., cx);
        let _ = root;

        // The docked right pane opens the top band, so its trailing actions
        // must end at or before the cluster's left edge.
        root.update(cx, |root, cx| root.workspace_open_diff(cx));
        cx.run_until_parked();
        let slots = r46_shell_slot_bounds(window, cx);
        let pane_actions = [
            "workspace-review-commit",
            "right-workspace-add",
            "right-workspace-dock",
            "right-workspace-maximize",
        ];
        for selector in pane_actions {
            let action = shell_bounds(window, selector, cx);
            assert!(
                action.right() <= slots[0].left() + px(1.),
                "{selector} right ({:?}) must sit left of the reserved cluster band ({:?})",
                action.right(),
                slots[0].left()
            );
            assert!(
                !slots.iter().any(|slot| slot.intersects(&action)),
                "{selector} must not intersect any shell slot"
            );
        }
        // The reserved band is exactly the shared token, measured from the
        // pane header's own trailing edge.
        let header = shell_bounds(window, "right-workspace-header", cx);
        let actions = shell_bounds(window, "right-workspace-actions", cx);
        assert_close(
            header.right() - actions.right(),
            Layout::SHELL_SLOT_CLUSTER_RESERVE,
            "right pane header reserves the shared cluster token",
        );

        // A maximized pane keeps the cluster mounted and reserves the same
        // band: maximizing must not move the slots (R46 §2.1).
        window
            .update(cx, |root, window, cx| {
                root.workspace.maximized[0] = true;
                root.workspace.reveal_tabs[0] = true;
                cx.notify();
                let _ = window;
            })
            .expect("maximize the right pane");
        cx.run_until_parked();
        let maximized_slots = r46_shell_slot_bounds(window, cx);
        r46_assert_slots_identical(&slots, &maximized_slots, "right pane maximized");
        let maximized_header = shell_bounds(window, "right-workspace-header", cx);
        let maximized_actions = shell_bounds(window, "right-workspace-actions", cx);
        assert!(
            maximized_actions.right() <= maximized_slots[0].left() + px(1.),
            "maximized pane actions right ({:?}) must sit left of the reserved band ({:?})",
            maximized_actions.right(),
            maximized_slots[0].left()
        );
        assert_close(
            maximized_header.right() - maximized_actions.right(),
            Layout::SHELL_SLOT_CLUSTER_RESERVE,
            "maximized pane header reserves the shared cluster token",
        );
        // The maximized header opens the same 46px band as `main-header`.
        assert_close(
            maximized_header.top(),
            0.0,
            "maximized pane header opens the window's top band",
        );

        // The bottom band is not reserved: a bottom-docked pane header keeps
        // its own compact trailing inset and clears the cluster by being in a
        // different band entirely.
        window
            .update(cx, |root, window, cx| {
                root.workspace.maximized[0] = false;
                root.workspace_move_selected(0, window, cx);
            })
            .expect("dock the Review tab to the bottom");
        cx.run_until_parked();
        let bottom_header = shell_bounds(window, "bottom-workspace-header", cx);
        let bottom_actions = shell_bounds(window, "bottom-workspace-actions", cx);
        assert!(
            bottom_header.top() > px(Layout::MAIN_HEADER_HEIGHT),
            "bottom pane header must not open the top band"
        );
        assert_close(
            bottom_header.right() - bottom_actions.right(),
            4.0,
            "bottom band pane keeps its compact trailing inset",
        );
        for (index, slot) in r46_shell_slot_bounds(window, cx).iter().enumerate() {
            assert!(
                !slot.intersects(&bottom_actions),
                "bottom band pane actions must not intersect slot {}",
                index + 1
            );
        }
    }

    #[gpui_kit::test]
    async fn r46_overlay_open_slot_clicks_still_work(cx: &mut TestAppContext) {
        let repo = diff_controller_repo();
        let (root, window) = r45_mount_project_window(&repo, "R46 overlay slots", 1100., 860., cx);
        let _ = shell_bounds(window, "main-header-shell-slots", cx);

        // While the overlay is open its dismissal backdrop covers the whole
        // window; the window-anchored cluster paints above it, so slot 1 must
        // still receive the click and close the surface it owns.
        shell_click(window, "main-header-environment", cx);
        let _ = shell_bounds(window, "environment-overlay", cx);
        assert!(root.read_with(cx, |root, _| root.environment_overlay_open));
        shell_click(window, "main-header-environment", cx);
        assert!(shell_absent(window, "environment-overlay", cx));
        assert!(
            !root.read_with(cx, |root, _| root.environment_overlay_open),
            "slot 1 must close the overlay even though the backdrop is painted"
        );

        // Re-open, then prove slot 2 also receives its press. The mount uses
        // an in-memory store, so the terminal branch reports the production
        // "no file-backed project" error instead of spawning a PTY — a
        // deterministic observable of the handler having run. Had the
        // backdrop swallowed the press it would have dismissed the overlay
        // instead and this error would stay false.
        shell_click(window, "main-header-environment", cx);
        let _ = shell_bounds(window, "environment-overlay", cx);
        assert!(!root.read_with(cx, |root, _| root.workspace.terminal_error));
        shell_click(window, "main-header-terminal", cx);
        assert!(
            root.read_with(cx, |root, _| root.workspace.terminal_error),
            "slot 2 must receive its press while the overlay backdrop is painted"
        );
        assert!(
            root.read_with(cx, |root, _| root.environment_overlay_open),
            "a slot press must not be handled as an outside dismissal"
        );
        shell_click(window, "main-header-environment", cx);
        assert!(shell_absent(window, "environment-overlay", cx));

        // The R45 dismissal contract is unchanged: an outside press still
        // closes the overlay and returns focus to the Composer.
        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("focus the task composer");
        let input = root.read_with(cx, |root, cx| {
            root.stream_view
                .as_ref()
                .expect("conversation")
                .1
                .read(cx)
                .composer_input()
        });
        let input_focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
        shell_click(window, "main-header-environment", cx);
        let _ = shell_bounds(window, "environment-overlay", cx);
        let title = shell_bounds(window, "main-header-title", cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(title.center(), Modifiers::default());
        visual.run_until_parked();
        assert!(shell_absent(window, "environment-overlay", cx));
        assert!(
            window
                .update(cx, |_, window, _| input_focus.is_focused(window))
                .expect("composer focus after outside click"),
            "an outside click must still return focus to the Composer"
        );
    }

    #[gpui_kit::test]
    async fn r45_header_cluster_renders_three_stable_slots(cx: &mut TestAppContext) {
        let repo = diff_controller_repo();
        let (root, window) = r45_mount_project_window(&repo, "R45 slots", 1403., 860., cx);
        let _ = root;
        r45_assert_three_shell_slots(window, cx);

        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(960.), px(600.)));
                window.bounds_changed(cx);
            })
            .expect("resize to the minimum window size");
        cx.run_until_parked();
        r45_assert_three_shell_slots(window, cx);
    }

    #[gpui_kit::test]
    async fn r45_environment_overlay_esc_and_outside_click_close_with_composer_focus(
        cx: &mut TestAppContext,
    ) {
        let repo = diff_controller_repo();
        let (root, window) = r45_mount_project_window(&repo, "R45 overlay", 1100., 860., cx);
        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("focus the task composer");
        let input = root.read_with(cx, |root, cx| {
            root.stream_view
                .as_ref()
                .expect("conversation")
                .1
                .read(cx)
                .composer_input()
        });
        let input_focus = input.read_with(cx, |input, cx| input.focus_handle(cx));

        // Below the breakpoint, slot 1 opens the overlay card.
        shell_click(window, "main-header-environment", cx);
        let _ = shell_bounds(window, "environment-overlay", cx);
        assert!(root.read_with(cx, |root, _| root.environment_overlay_open));

        // Escape closes the overlay and returns focus to the Composer.
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        assert!(shell_absent(window, "environment-overlay", cx));
        assert!(!root.read_with(cx, |root, _| root.environment_overlay_open));
        assert!(
            window
                .update(cx, |_, window, _| input_focus.is_focused(window))
                .expect("composer focus after escape"),
            "escape must return focus to the Composer"
        );

        // Reopening and pressing on a neutral header surface outside the
        // card dismisses it and again returns focus to the Composer.
        shell_click(window, "main-header-environment", cx);
        let _ = shell_bounds(window, "environment-overlay", cx);
        let title = shell_bounds(window, "main-header-title", cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(title.center(), Modifiers::default());
        visual.run_until_parked();
        assert!(shell_absent(window, "environment-overlay", cx));
        assert!(
            window
                .update(cx, |_, window, _| input_focus.is_focused(window))
                .expect("composer focus after outside click"),
            "an outside click must return focus to the Composer"
        );

        // The wide rail modality has no Escape contract.
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(1400.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("resize to a wide viewport");
        cx.run_until_parked();
        let _ = shell_bounds(window, "environment-rail", cx);
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        let _ = shell_bounds(window, "environment-rail", cx);
        assert!(!root.read_with(cx, |root, _| root.environment_collapsed));
    }

    #[gpui_kit::test]
    async fn r45_environment_slot_tracks_rendered_state(cx: &mut TestAppContext) {
        let repo = diff_controller_repo();
        let (root, window) = r45_mount_project_window(&repo, "R45 env slot", 1403., 860., cx);
        // The wide rail is rendered by default, so slot 1 owns a visible
        // surface. The painted bg_active surface itself is covered by the
        // native acceptance screenshot (R45 §4, 04-env-rail.png); these
        // assertions pin the rendered state that drives it.
        let _ = shell_bounds(window, "environment-rail", cx);
        assert!(root.read_with(cx, |root, _| !root.environment_collapsed
            && !root.environment_overlay_open));
        shell_click(window, "main-header-environment", cx);
        assert!(shell_absent(window, "environment-rail", cx));
        assert!(root.read_with(cx, |root, _| root.environment_collapsed));
        shell_click(window, "main-header-environment", cx);
        let _ = shell_bounds(window, "environment-rail", cx);
        assert!(root.read_with(cx, |root, _| !root.environment_collapsed));

        // With the right dock rendered, the Environment surface is replaced:
        // slot 1 renders disabled and clicking it changes nothing.
        root.update(cx, |root, cx| root.workspace_open_diff(cx));
        cx.run_until_parked();
        let _ = shell_bounds(window, "right-workspace-pane", cx);
        let collapsed = root.read_with(cx, |root, _| root.environment_collapsed);
        shell_click(window, "main-header-environment", cx);
        assert_eq!(
            root.read_with(cx, |root, _| root.environment_collapsed),
            collapsed,
            "slot 1 must stay disabled while the right dock replaces the rail"
        );
    }

    #[gpui_kit::test]
    async fn r45_right_toggle_hide_restore_open_diff_priority(cx: &mut TestAppContext) {
        let repo = diff_controller_repo();
        let (root, window) = r45_mount_project_window(&repo, "R45 right slot", 1400., 900., cx);
        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("focus the task composer");
        let input = root.read_with(cx, |root, cx| {
            root.stream_view
                .as_ref()
                .expect("conversation")
                .1
                .read(cx)
                .composer_input()
        });
        let input_focus = input.read_with(cx, |input, cx| input.focus_handle(cx));

        // Branch c: idle dock with Review available → open the diff pane.
        shell_click(window, "main-header-workspace-right", cx);
        let _ = shell_bounds(window, "right-workspace-pane", cx);
        assert!(root.read_with(cx, |root, _| {
            root.workspace.selected[0] == Some(TabKey::Diff) && !root.workspace.hidden[0]
        }));

        // Branch a: rendered non-terminal selection → hide, Composer focus.
        shell_click(window, "main-header-workspace-right", cx);
        assert!(shell_absent(window, "right-workspace-pane", cx));
        assert!(root.read_with(cx, |root, _| root.workspace.hidden[0]));
        assert!(
            window
                .update(cx, |_, window, _| input_focus.is_focused(window))
                .expect("composer focus after hide"),
            "hiding returns focus to the Composer"
        );

        // Branch b: hidden non-terminal tab → restore with pane activation.
        shell_click(window, "main-header-workspace-right", cx);
        let _ = shell_bounds(window, "right-workspace-pane", cx);
        assert!(!root.read_with(cx, |root, _| root.workspace.hidden[0]));
        let diff_focus = root.read_with(cx, |root, cx| {
            root.diff_controller
                .active
                .as_ref()
                .expect("diff route")
                .view
                .read(cx)
                .focus_handle(cx)
        });
        assert!(
            window
                .update(cx, |_, window, _| diff_focus.is_focused(window))
                .expect("diff focus after restore"),
            "generic restore activates the pane content"
        );

        // Branch c again: with the hidden tab closed, the slot opens Review.
        shell_click(window, "main-header-workspace-right", cx);
        assert!(root.read_with(cx, |root, _| root.workspace.hidden[0]));
        root.update(cx, |root, _| {
            root.workspace.close(&TabKey::Diff);
            root.diff_controller.close();
        });
        cx.run_until_parked();
        shell_click(window, "main-header-workspace-right", cx);
        assert!(root.read_with(cx, |root, _| {
            root.diff_controller.active.is_some()
                && root.workspace.selected[0] == Some(TabKey::Diff)
        }));
    }

    #[gpui_kit::test]
    async fn r45_toggle_surfaces_track_rendered_visibility(cx: &mut TestAppContext) {
        let repo = diff_controller_repo();
        let (root, window) = r45_mount_project_window(&repo, "R45 surfaces", 1400., 900., cx);
        // Slot 3's selected surface follows the real rendered predicate.
        root.update(cx, |root, cx| root.workspace_open_diff(cx));
        cx.run_until_parked();
        window
            .update(cx, |root, window, cx| {
                assert!(root.right_workspace_rendered_non_terminal(window, cx));
            })
            .expect("an open right dock counts as rendered");
        shell_click(window, "main-header-workspace-right", cx);
        window
            .update(cx, |root, window, cx| {
                assert!(root.workspace.hidden[0]);
                assert!(!root.right_workspace_rendered_non_terminal(window, cx));
            })
            .expect("a hidden dock is not rendered");

        // Restore, then shrink below the responsive guard with a widened
        // Sidebar: hidden == false alone must not light the slot.
        shell_click(window, "main-header-workspace-right", cx);
        cx.update(|cx| cx.set_global(SidebarWidth(Layout::SIDEBAR_MAX_WIDTH)));
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(960.), px(600.)));
                window.bounds_changed(cx);
            })
            .expect("resize below the right-dock guard");
        cx.run_until_parked();
        window
            .update(cx, |root, window, cx| {
                assert!(!root.workspace.hidden[0]);
                assert!(root.workspace.selected[0].is_some());
                assert!(
                    !root.right_workspace_rendered_non_terminal(window, cx),
                    "an unmounted right dock must not count as rendered"
                );
            })
            .expect("unmounted predicate");

        // Maximized bottom dock: rendered even below the bottom minimum, and
        // slot 2 then takes the hide branch.
        window
            .update(cx, |root, window, cx| {
                window.resize(size(px(1400.), px(900.)));
                window.bounds_changed(cx);
                root.workspace_move_selected(0, window, cx);
            })
            .expect("move Review to the bottom dock");
        cx.run_until_parked();
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(960.), px(400.)));
                window.bounds_changed(cx);
            })
            .expect("resize below the bottom minimum");
        cx.run_until_parked();
        window
            .update(cx, |root, window, _| {
                assert!(!root.bottom_workspace_selected_is_rendered(window));
                // White-box stand-in for the maximize control, which is not
                // mounted while the dock is unrendered at this height.
                root.workspace.maximized[1] = true;
                assert!(root.bottom_workspace_selected_is_rendered(window));
            })
            .expect("maximized bottom dock counts as rendered");
        // A maximized pane unmounts the header row (R44 fullscreen chrome),
        // so the hide branch runs through the production handler here.
        window
            .update(cx, |root, window, cx| {
                root.workspace_toggle_bottom(window, cx);
                assert!(root.workspace.hidden[1]);
            })
            .expect("slot 2 takes the hide branch on a maximized bottom dock");
    }

    #[gpui_kit::test]
    async fn r45_composer_centering_and_geometry(cx: &mut TestAppContext) {
        let repo = diff_controller_repo();
        let (_root, window) = r45_mount_project_window(&repo, "R45 composer", 1403., 860., cx);
        let panel = shell_bounds(window, "main-content-panel", cx);
        let rail = shell_bounds(window, "environment-rail", cx);
        let composer = shell_bounds(window, "composer-shell", cx);
        // Conversation column with the rail open: [panel.left, rail.left].
        assert_close(
            composer.center().x,
            (f32::from(panel.left()) + f32::from(rail.left())) / 2.0,
            "composer centers on the conversation column with the rail open",
        );
        assert_close(
            composer.size.width,
            Layout::COMPOSER_MAX_WIDTH,
            "composer width cap",
        );
        assert!(
            f32::from(composer.size.height) >= Layout::COMPOSER_MIN_HEIGHT,
            "composer keeps its 100px minimum"
        );

        // Closing the rail re-centers the composer on the full column.
        shell_click(window, "main-header-environment", cx);
        let composer = shell_bounds(window, "composer-shell", cx);
        assert_close(
            composer.center().x,
            (f32::from(panel.left()) + f32::from(panel.right())) / 2.0,
            "composer re-centers after the rail closes",
        );
        assert_close(
            composer.size.width,
            Layout::COMPOSER_MAX_WIDTH,
            "composer width cap without the rail",
        );

        // Opening the right dock re-centers and never overlaps the pane.
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
            })
            .expect("open the file preview in the right dock");
        cx.run_until_parked();
        let pane = shell_bounds(window, "right-workspace-pane", cx);
        let composer = shell_bounds(window, "composer-shell", cx);
        assert_close(
            composer.center().x,
            (f32::from(panel.left()) + f32::from(pane.left()) - Layout::SIDEBAR_RESIZE_HIT_AREA)
                / 2.0,
            "composer re-centers beside the right dock",
        );
        assert!(
            composer.right() <= pane.left(),
            "composer must not overlap the right dock"
        );

        // Bottom dock: no vertical overlap either.
        window
            .update(cx, |root, window, cx| {
                root.workspace_move_selected(0, window, cx)
            })
            .expect("move the preview to the bottom dock");
        cx.run_until_parked();
        let bottom = shell_bounds(window, "bottom-workspace-pane", cx);
        let composer = shell_bounds(window, "composer-shell", cx);
        assert!(
            composer.bottom() <= bottom.top(),
            "composer must not overlap the bottom dock"
        );

        // Minimum window: the composer shrinks with the padded column.
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(960.), px(600.)));
                window.bounds_changed(cx);
            })
            .expect("resize to the minimum window");
        cx.run_until_parked();
        let panel = shell_bounds(window, "main-content-panel", cx);
        let composer = shell_bounds(window, "composer-shell", cx);
        assert_close(
            composer.center().x,
            (f32::from(panel.left()) + f32::from(panel.right())) / 2.0,
            "composer centers in the narrow column",
        );
        assert_close(
            composer.size.width,
            f32::from(panel.size.width) - 2.0 * Layout::CONTENT_PADDING,
            "narrow composer fills the padded column",
        );
        assert!(
            f32::from(composer.left() - panel.left()) >= Layout::CONTENT_PADDING,
            "left padding stays at least 16px"
        );
        assert!(
            f32::from(panel.right() - composer.right()) >= Layout::CONTENT_PADDING,
            "right padding stays at least 16px"
        );
        assert!(
            f32::from(composer.size.height) >= Layout::COMPOSER_MIN_HEIGHT,
            "narrow composer keeps its 100px minimum"
        );
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
    use super::{TabKey, VegaWindow, WorkspaceCreateAction};
    use crate::tests::install_diff_window_globals;
    use gpui_kit::Focusable;
    use gpui_kit::{
        AppContext, Bounds, EntityInputHandler, FocusHandle, KeyBinding, Modifiers, TestAppContext,
        VisualTestContext, WindowBounds, WindowHandle, WindowOptions, point, px, size,
    };
    use std::time::{Duration, Instant};
    use vega_theme::Layout;
    use vega_ui::settings::CloseSettings;
    use vega_ui::sidebar::{SelectedProject, SidebarWidth};

    fn assert_pixel_close(actual: gpui_kit::Pixels, expected: f32, label: &str) {
        let actual = f32::from(actual);
        assert!(
            (actual - expected).abs() <= 1.0,
            "{label}: expected {expected}±1px, got {actual}px"
        );
    }

    fn mounted_bounds(
        window: WindowHandle<VegaWindow>,
        selector: &'static str,
        cx: &mut TestAppContext,
    ) -> Bounds<gpui_kit::Pixels> {
        cx.run_until_parked();
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing mounted selector: {selector}"))
    }

    fn click_mounted(
        window: WindowHandle<VegaWindow>,
        selector: &'static str,
        cx: &mut TestAppContext,
    ) {
        let bounds = mounted_bounds(window, selector, cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(bounds.center(), Modifiers::default());
        visual.run_until_parked();
    }

    fn focus_is(
        window: WindowHandle<VegaWindow>,
        focus: FocusHandle,
        cx: &mut TestAppContext,
    ) -> bool {
        window
            .update(cx, |_, window, _| focus.is_focused(window))
            .expect("mounted window focus")
    }

    fn wait_for_marker(path: &std::path::Path, cx: &mut TestAppContext) {
        let until = Instant::now() + Duration::from_secs(8);
        while !path.exists() {
            assert!(
                Instant::now() < until,
                "PTY probe timed out waiting for {}",
                path.display()
            );
            cx.executor().advance_clock(Duration::from_millis(30));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn r45_terminal_window(
        path: &std::path::Path,
        cx: &mut TestAppContext,
    ) -> (
        gpui_kit::Entity<VegaWindow>,
        WindowHandle<VegaWindow>,
        gpui_kit::Entity<vega_ui::text_input::TextInput>,
    ) {
        let store = vega_store::Store::open(path.join("test.db")).expect("store");
        store.migrate().expect("migrations");
        let project =
            vega_store::projects::create(store.conn(), path.to_str().unwrap(), "R45", None)
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
        let input = root.read_with(cx, |root, cx| {
            root.stream_view
                .as_ref()
                .expect("conversation")
                .1
                .read(cx)
                .composer_input()
        });
        (root, window, input)
    }

    #[gpui_kit::test]
    async fn r44_terminal_entry_points_and_creation_menu_preserve_explicit_focus(
        cx: &mut TestAppContext,
    ) {
        const MARKER: &str = "VEGA_R44_TERMINAL_INTERACTION_CHILD";
        let Some(path) = std::env::var_os(MARKER) else {
            let root = tempfile::tempdir().expect("owned terminal home");
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    "window::workspace::terminal_tests::r44_terminal_entry_points_and_creation_menu_preserve_explicit_focus",
                    "--nocapture",
                ])
                .env(MARKER, root.path())
                .env("HOME", root.path())
                .env("ZDOTDIR", root.path())
                .output()
                .expect("isolated test process");
            assert!(
                output.status.success(),
                "owned R44 terminal subprocess failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        };
        let path = std::path::PathBuf::from(path);
        let store = vega_store::Store::open(path.join("test.db")).expect("store");
        store.migrate().expect("migrations");
        let project =
            vega_store::projects::create(store.conn(), path.to_str().unwrap(), "R44", None)
                .expect("project");
        let thread =
            vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm")
                .expect("thread");
        cx.update(|cx| {
            install_diff_window_globals(store, thread, cx);
            cx.bind_keys([KeyBinding::new(
                "escape",
                CloseSettings,
                Some("WorkspaceMenu"),
            )]);
        });
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
        let input = root.read_with(cx, |root, cx| {
            root.stream_view
                .as_ref()
                .expect("conversation")
                .1
                .read(cx)
                .composer_input()
        });
        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("focus task composer");

        click_mounted(window, "main-header-terminal", cx);
        let (first_key, first_view) = root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.terminals.len(), 1);
            let key = root.workspace.selected[1]
                .clone()
                .expect("selected terminal");
            let TabKey::Terminal(id) = key else {
                panic!("terminal key")
            };
            assert!(!root.workspace.hidden[1]);
            let view = root.workspace.terminals[&id].view.clone();
            (TabKey::Terminal(id), view)
        });
        let input_focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
        let terminal_focus = first_view.read_with(cx, |view, cx| view.focus_handle(cx));
        assert!(focus_is(window, input_focus.clone(), cx));
        assert!(!focus_is(window, terminal_focus.clone(), cx));
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

        click_mounted(window, "main-header-terminal", cx);
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("bottom-workspace-pane")
                .is_none(),
            "main-header Terminal hides its visible selected terminal"
        );
        click_mounted(window, "environment-terminal", cx);
        root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.terminals.len(), 1);
            assert_eq!(root.workspace.selected[1], Some(first_key.clone()));
            assert!(!root.workspace.hidden[1]);
            assert_eq!(
                root.workspace
                    .terminals
                    .values()
                    .next()
                    .expect("same terminal")
                    .view
                    .entity_id(),
                first_view.entity_id()
            );
        });
        assert!(focus_is(window, input_focus.clone(), cx));
        click_mounted(window, "environment-terminal", cx);
        root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.terminals.len(), 1);
            assert_eq!(root.workspace.selected[1], Some(first_key.clone()));
            assert!(!root.workspace.hidden[1]);
        });
        assert!(focus_is(window, input_focus.clone(), cx));

        cx.simulate_keystrokes(window.into(), "cmd-j");
        cx.run_until_parked();
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("bottom-workspace-pane")
                .is_none(),
            "global terminal toggle hides the visible selected terminal"
        );
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("main-header-restore-bottom")
                .is_none(),
            "hidden terminal has no duplicate generic restore action"
        );
        assert!(focus_is(window, input_focus.clone(), cx));

        click_mounted(window, "main-header-terminal", cx);
        let _ = mounted_bounds(window, "bottom-workspace-pane", cx);
        root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.selected[1], Some(first_key.clone()));
            assert_eq!(root.workspace.terminals.len(), 1);
        });
        assert!(focus_is(window, input_focus.clone(), cx));
        assert!(!focus_is(window, terminal_focus.clone(), cx));
        cx.simulate_keystrokes(window.into(), "s a f e");
        assert_eq!(
            input.read_with(cx, |input, _| input.text().to_string()),
            "safe"
        );

        let tab_selector = match first_key {
            TabKey::Terminal(1) => "workspace-tab-Terminal(1)",
            TabKey::Terminal(id) => panic!("expected first terminal id 1, got {id}"),
            _ => unreachable!(),
        };
        let tab_bounds = VisualTestContext::from_window(window.into(), cx)
            .debug_bounds(tab_selector)
            .expect("mounted terminal tab");
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(tab_bounds.center(), Modifiers::default());
        visual.run_until_parked();
        assert!(focus_is(window, terminal_focus.clone(), cx));

        window
            .update(cx, |_, window, cx| {
                first_view.update(cx, |view, cx| {
                    view.replace_text_in_range(
                        None,
                        "printf explicit-focus > r44-explicit-focus; printf $$ > r44-pid-before",
                        window,
                        cx,
                    )
                });
            })
            .expect("type explicit PTY probe");
        cx.simulate_keystrokes(window.into(), "enter");
        let marker = path.join("r44-explicit-focus");
        let until = Instant::now() + Duration::from_secs(8);
        while !marker.exists() {
            assert!(Instant::now() < until, "explicit PTY focus probe timed out");
            cx.executor().advance_clock(Duration::from_millis(30));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(
            std::fs::read_to_string(marker).expect("PTY marker"),
            "explicit-focus"
        );
        let pid_before = path.join("r44-pid-before");
        let until = Instant::now() + Duration::from_secs(8);
        while !pid_before.exists() {
            assert!(Instant::now() < until, "PTY identity probe timed out");
            cx.executor().advance_clock(Duration::from_millis(30));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(15));
        }

        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("return to composer before layout actions");
        click_mounted(window, "bottom-workspace-maximize", cx);
        assert!(focus_is(window, terminal_focus.clone(), cx));
        assert!(root.read_with(cx, |root, _| root.workspace.maximized[1]));
        assert_eq!(
            first_view.entity_id(),
            root.read_with(cx, |root, _| {
                let TabKey::Terminal(id) = root.workspace.selected[1]
                    .clone()
                    .expect("maximized terminal")
                else {
                    panic!("terminal key")
                };
                root.workspace.terminals[&id].view.entity_id()
            })
        );
        window
            .update(cx, |_, window, cx| {
                first_view.update(cx, |view, cx| {
                    view.replace_text_in_range(
                        None,
                        "printf maximized-focus > r44-maximized-focus",
                        window,
                        cx,
                    )
                });
            })
            .expect("type maximized PTY focus probe");
        cx.simulate_keystrokes(window.into(), "enter");
        let maximized_marker = path.join("r44-maximized-focus");
        let until = Instant::now() + Duration::from_secs(8);
        while !maximized_marker.exists() {
            assert!(
                Instant::now() < until,
                "maximized PTY focus probe timed out"
            );
            cx.executor().advance_clock(Duration::from_millis(30));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(
            std::fs::read_to_string(maximized_marker).expect("maximized PTY marker"),
            "maximized-focus"
        );
        click_mounted(window, "bottom-workspace-maximize", cx);
        assert!(focus_is(window, terminal_focus.clone(), cx));
        assert!(!root.read_with(cx, |root, _| root.workspace.maximized[1]));
        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("return to composer before dock");
        click_mounted(window, "bottom-workspace-dock", cx);
        assert!(focus_is(window, input_focus.clone(), cx));
        let _ = mounted_bounds(window, "right-workspace-hide", cx);
        assert_eq!(
            first_view.entity_id(),
            root.read_with(cx, |root, _| {
                let TabKey::Terminal(id) =
                    root.workspace.selected[0].clone().expect("right terminal")
                else {
                    panic!("terminal key")
                };
                root.workspace.terminals[&id].view.entity_id()
            })
        );
        cx.update(|cx| {
            cx.set_global(SidebarWidth(Layout::SIDEBAR_MAX_WIDTH));
            cx.refresh_windows();
        });
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(960.), px(600.)));
                window.bounds_changed(cx);
            })
            .expect("resize below persistent right workspace width");
        cx.run_until_parked();
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("right-workspace-pane")
                .is_none(),
            "narrow responsive layout no longer renders the right pane"
        );
        root.read_with(cx, |root, _| {
            assert!(!root.workspace.hidden[0]);
            assert_eq!(root.workspace.selected[0], Some(first_key.clone()));
        });
        cx.simulate_keystrokes(window.into(), "cmd-j");
        cx.run_until_parked();
        let _ = mounted_bounds(window, "bottom-workspace-pane", cx);
        root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.terminals.len(), 1);
            assert_eq!(root.workspace.selected[1], Some(first_key.clone()));
            assert!(
                root.workspace
                    .tabs
                    .iter()
                    .any(|(key, bottom)| key == &first_key && *bottom)
            );
            assert_eq!(
                root.workspace
                    .terminals
                    .values()
                    .next()
                    .expect("same responsive terminal")
                    .view
                    .entity_id(),
                first_view.entity_id()
            );
        });
        assert!(focus_is(window, input_focus.clone(), cx));

        let tab_bounds = VisualTestContext::from_window(window.into(), cx)
            .debug_bounds(tab_selector)
            .expect("responsive terminal tab");
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(tab_bounds.center(), Modifiers::default());
        visual.run_until_parked();
        assert!(focus_is(window, terminal_focus.clone(), cx));
        window
            .update(cx, |_, window, cx| {
                first_view.update(cx, |view, cx| {
                    view.replace_text_in_range(None, "printf $$ > r44-pid-responsive", window, cx)
                });
            })
            .expect("type responsive PTY identity probe");
        cx.simulate_keystrokes(window.into(), "enter");
        let pid_responsive = path.join("r44-pid-responsive");
        let until = Instant::now() + Duration::from_secs(8);
        while !pid_responsive.exists() {
            assert!(
                Instant::now() < until,
                "responsive PTY identity probe timed out"
            );
            cx.executor().advance_clock(Duration::from_millis(30));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(
            std::fs::read_to_string(&pid_before).expect("PID before responsive recovery"),
            std::fs::read_to_string(&pid_responsive).expect("PID after responsive recovery"),
            "narrow right recovery preserves the terminal process"
        );

        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx);
                window.resize(size(px(1404.), px(860.)));
                window.bounds_changed(cx);
            })
            .expect("restore wide window before Environment recovery");
        cx.run_until_parked();
        click_mounted(window, "bottom-workspace-dock", cx);
        let _ = mounted_bounds(window, "right-workspace-pane", cx);
        assert!(focus_is(window, input_focus.clone(), cx));
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(960.), px(600.)));
                window.bounds_changed(cx);
            })
            .expect("resize Environment recovery below right workspace width");
        cx.run_until_parked();
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("right-workspace-pane")
                .is_none()
        );
        click_mounted(window, "main-header-environment", cx);
        let _ = mounted_bounds(window, "environment-overlay", cx);
        click_mounted(window, "environment-terminal", cx);
        let _ = mounted_bounds(window, "bottom-workspace-pane", cx);
        root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.terminals.len(), 1);
            assert_eq!(root.workspace.selected[1], Some(first_key.clone()));
            assert_eq!(
                root.workspace
                    .terminals
                    .values()
                    .next()
                    .expect("same Environment terminal")
                    .view
                    .entity_id(),
                first_view.entity_id()
            );
        });
        assert!(focus_is(window, input_focus.clone(), cx));

        click_mounted(window, "bottom-workspace-hide", cx);
        assert!(root.read_with(cx, |root, _| root.workspace.hidden[1]));
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("main-header-restore-bottom")
                .is_none(),
            "leading hide keeps Terminal as the sole restore path"
        );
        cx.simulate_keystrokes(window.into(), "cmd-j");
        cx.run_until_parked();
        let _ = mounted_bounds(window, "bottom-workspace-pane", cx);
        assert!(focus_is(
            window,
            input.read_with(cx, |input, cx| input.focus_handle(cx)),
            cx
        ));
        let tab_bounds = VisualTestContext::from_window(window.into(), cx)
            .debug_bounds(tab_selector)
            .expect("restored terminal tab");
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.simulate_click(tab_bounds.center(), Modifiers::default());
        visual.run_until_parked();
        window
            .update(cx, |_, window, cx| {
                first_view.update(cx, |view, cx| {
                    view.replace_text_in_range(None, "printf $$ > r44-pid-after", window, cx)
                });
            })
            .expect("type post-layout PTY identity probe");
        cx.simulate_keystrokes(window.into(), "enter");
        let pid_after = path.join("r44-pid-after");
        let until = Instant::now() + Duration::from_secs(8);
        while !pid_after.exists() {
            assert!(
                Instant::now() < until,
                "post-layout PTY identity probe timed out"
            );
            cx.executor().advance_clock(Duration::from_millis(30));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(
            std::fs::read_to_string(pid_before).expect("PID before layout changes"),
            std::fs::read_to_string(pid_after).expect("PID after layout changes"),
            "move, maximize, hide, and reveal preserve the terminal process"
        );

        click_mounted(window, "bottom-workspace-add", cx);
        assert_eq!(
            root.read_with(cx, |root, cx| root.workspace_creation_actions(cx)),
            vec![
                WorkspaceCreateAction::Review,
                WorkspaceCreateAction::NewTerminal,
            ],
            "creation menu has no existing tabs, preview restore, or destructive close-all action"
        );
        let menu = mounted_bounds(window, "workspace-add-menu", cx);
        let _ = mounted_bounds(window, "workspace-add-review", cx);
        let _ = mounted_bounds(window, "workspace-add-terminal", cx);
        assert!(
            f32::from(menu.size.height) <= 88.0,
            "creation menu contains only its two truthful rows"
        );
        cx.simulate_keystrokes(window.into(), "escape");
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("workspace-add-menu")
                .is_none()
        );
        assert!(focus_is(window, input_focus, cx));

        click_mounted(window, "bottom-workspace-add", cx);
        click_mounted(window, "workspace-add-terminal", cx);
        let new_view = root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.terminals.len(), 2);
            let selected = root.workspace.selected[1]
                .as_ref()
                .expect("new terminal selected");
            assert_ne!(selected, &first_key);
            let TabKey::Terminal(id) = selected else {
                panic!("new terminal key")
            };
            root.workspace.terminals[id].view.clone()
        });
        let new_focus = new_view.read_with(cx, |view, cx| view.focus_handle(cx));
        assert!(focus_is(window, new_focus, cx));

        click_mounted(window, "bottom-workspace-add", cx);
        click_mounted(window, "workspace-add-terminal", cx);
        assert_eq!(
            root.read_with(cx, |root, _| root.workspace.selected[1].clone()),
            Some(TabKey::Terminal(3))
        );
        click_mounted(window, "workspace-tab-Terminal(2)", cx);
        click_mounted(window, "workspace-tab-close-Terminal(2)", cx);
        root.read_with(cx, |root, _| {
            assert_eq!(
                root.workspace.selected[1],
                Some(TabKey::Terminal(3)),
                "closing a selected middle tab activates its next sibling"
            );
            assert!(!root.workspace.terminals.contains_key(&2));
            assert_eq!(root.workspace.terminals.len(), 2);
        });
        let _ = mounted_bounds(window, "workspace-tab-Terminal(3)", cx);
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
        cx.run_until_parked();
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            let header = visual
                .debug_bounds("right-workspace-header")
                .expect("right workspace header");
            let hide = visual
                .debug_bounds("right-workspace-hide")
                .expect("leading pane hide control");
            let strip = visual
                .debug_bounds("right-tabs")
                .expect("scrollable tab strip");
            let actions = visual
                .debug_bounds("right-workspace-actions")
                .expect("workspace trailing actions");
            let buttons = [
                visual
                    .debug_bounds("right-workspace-add")
                    .expect("workspace add action"),
                visual
                    .debug_bounds("right-workspace-dock")
                    .expect("workspace dock action"),
                visual
                    .debug_bounds("right-workspace-maximize")
                    .expect("workspace maximize action"),
            ];
            assert_pixel_close(
                header.size.height,
                Layout::WORKSPACE_HEADER_HEIGHT,
                "workspace header height",
            );
            assert!(
                hide.right() <= strip.left(),
                "leading hide control stays outside the scrollable strip"
            );
            assert!(
                strip.right() <= actions.left(),
                "pane actions stay outside the scrollable strip"
            );
            // R46 §2.1.1 updates this assertion: the right pane's header opens
            // the window's top band, so it now reserves the window-anchored
            // slot cluster's trailing band instead of the former 4px `px_1`
            // inset. This is the "absolute x of a right-pane trailing action
            // that now sits left of the reserved band" case §3 names.
            assert_pixel_close(
                header.right() - actions.right(),
                Layout::SHELL_SLOT_CLUSTER_RESERVE,
                "top-band pane actions reserve the shell slot cluster",
            );
            assert_pixel_close(actions.size.width, 80.0, "workspace trailing group");
            for (index, button) in buttons.iter().enumerate() {
                assert_pixel_close(button.size.width, 24.0, "workspace action hitbox");
                assert_pixel_close(button.size.height, 24.0, "workspace action hitbox");
                if let Some(next) = buttons.get(index + 1) {
                    assert_pixel_close(next.left() - button.right(), 4.0, "workspace action gap");
                }
            }
        }
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
                root.workspace_toggle_bottom(window, cx);
                assert!(!root.workspace.hidden[1]);
                let TabKey::Terminal(id) = key else {
                    panic!("terminal tab")
                };
                assert_eq!(root.workspace.terminals[&id].view.entity_id(), entity_id);
                assert!(
                    root.stream_view
                        .as_ref()
                        .expect("conversation")
                        .1
                        .read(cx)
                        .composer_input()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
                assert!(
                    !root.workspace.terminals[&id]
                        .view
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
            })
            .unwrap();
        cx.run_until_parked();
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            let header = visual
                .debug_bounds("bottom-workspace-header")
                .expect("bottom workspace header");
            let strip = visual
                .debug_bounds("bottom-tabs")
                .expect("bottom scrollable tabs");
            let actions = visual
                .debug_bounds("bottom-workspace-actions")
                .expect("bottom trailing actions");
            assert_pixel_close(
                header.size.height,
                Layout::WORKSPACE_HEADER_HEIGHT,
                "bottom workspace header height",
            );
            assert!(
                strip.right() <= actions.left(),
                "bottom actions stay outside the scrollable strip"
            );
            assert_pixel_close(actions.size.width, 80.0, "bottom trailing group");
        }
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

    #[gpui_kit::test]
    async fn r45_bottom_toggle_unified_priority_and_cmd_j_parity(cx: &mut TestAppContext) {
        const MARKER: &str = "VEGA_R45_BOTTOM_TOGGLE_CHILD";
        let Some(path) = std::env::var_os(MARKER) else {
            let root = tempfile::tempdir().expect("owned terminal home");
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    "window::workspace::terminal_tests::r45_bottom_toggle_unified_priority_and_cmd_j_parity",
                    "--nocapture",
                ])
                .env(MARKER, root.path())
                .env("HOME", root.path())
                .env("ZDOTDIR", root.path())
                .output()
                .expect("isolated test process");
            assert!(
                output.status.success(),
                "owned R45 bottom toggle subprocess failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        };
        let path = std::path::PathBuf::from(path);
        let (root, window, input) = r45_terminal_window(&path, cx);
        let input_focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("focus the task composer");

        // Branch d (⌘J): with no tabs, the unified toggle creates the first
        // terminal and keeps the Composer focused.
        cx.simulate_keystrokes(window.into(), "cmd-j");
        cx.run_until_parked();
        let _ = mounted_bounds(window, "bottom-workspace-pane", cx);
        let first_view = root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.terminals.len(), 1);
            let key = root.workspace.selected[1]
                .clone()
                .expect("selected terminal");
            let TabKey::Terminal(id) = &key else {
                panic!("terminal key")
            };
            assert!(!root.workspace.hidden[1]);
            root.workspace.terminals[id].view.clone()
        });
        assert!(focus_is(window, input_focus.clone(), cx));

        // Branch a (slot click): the rendered selection hides.
        click_mounted(window, "main-header-terminal", cx);
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("bottom-workspace-pane")
                .is_none(),
            "slot 2 hides its rendered bottom dock"
        );
        assert!(root.read_with(cx, |root, _| root.workspace.hidden[1]));
        assert!(focus_is(window, input_focus.clone(), cx));

        // Branch b (slot click): the hidden selected tab reveals with the
        // same terminal entity and Composer focus.
        click_mounted(window, "main-header-terminal", cx);
        let _ = mounted_bounds(window, "bottom-workspace-pane", cx);
        root.read_with(cx, |root, _| {
            assert!(!root.workspace.hidden[1]);
            assert_eq!(root.workspace.terminals.len(), 1);
            assert_eq!(
                root.workspace
                    .terminals
                    .values()
                    .next()
                    .expect("same terminal")
                    .view
                    .entity_id(),
                first_view.entity_id()
            );
        });
        assert!(focus_is(window, input_focus.clone(), cx));

        // ⌘J parity for branches a and b.
        cx.simulate_keystrokes(window.into(), "cmd-j");
        cx.run_until_parked();
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("bottom-workspace-pane")
                .is_none(),
            "⌘J hides the rendered bottom dock like slot 2"
        );
        assert!(root.read_with(cx, |root, _| root.workspace.hidden[1]));
        assert!(focus_is(window, input_focus.clone(), cx));
        cx.simulate_keystrokes(window.into(), "cmd-j");
        cx.run_until_parked();
        let _ = mounted_bounds(window, "bottom-workspace-pane", cx);
        assert!(!root.read_with(cx, |root, _| root.workspace.hidden[1]));
        assert!(focus_is(window, input_focus.clone(), cx));

        // Branch c with a terminal present: a hidden bottom file tab reveals
        // with the same entity while the terminal stays untouched.
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
            })
            .expect("open the file preview");
        cx.run_until_parked();
        let file_view = root.read_with(cx, |root, _| {
            root.workspace
                .files
                .values()
                .next()
                .expect("file preview")
                .clone()
        });
        window
            .update(cx, |root, window, cx| {
                root.workspace_move_selected(0, window, cx)
            })
            .expect("move the preview to the bottom dock");
        window
            .update(cx, |root, window, cx| root.workspace_hide(1, window, cx))
            .expect("hide the bottom dock");
        cx.run_until_parked();
        click_mounted(window, "main-header-terminal", cx);
        root.read_with(cx, |root, _| {
            assert!(!root.workspace.hidden[1]);
            assert!(matches!(root.workspace.selected[1], Some(TabKey::File(_))));
            assert_eq!(root.workspace.terminals.len(), 1);
            assert_eq!(
                root.workspace
                    .files
                    .values()
                    .next()
                    .expect("same file")
                    .entity_id(),
                file_view.entity_id()
            );
        });
        assert!(focus_is(window, input_focus.clone(), cx));

        // Branch c without any terminal (⌘J parity): closing the terminal
        // still leaves the reveal branch for the hidden file tab.
        click_mounted(window, "workspace-tab-close-Terminal(1)", cx);
        cx.run_until_parked();
        assert!(root.read_with(cx, |root, _| root.workspace.terminals.is_empty()));
        window
            .update(cx, |root, window, cx| root.workspace_hide(1, window, cx))
            .expect("hide the bottom dock");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "cmd-j");
        cx.run_until_parked();
        root.read_with(cx, |root, _| {
            assert!(!root.workspace.hidden[1]);
            assert!(matches!(root.workspace.selected[1], Some(TabKey::File(_))));
            assert!(root.workspace.terminals.is_empty());
            assert_eq!(
                root.workspace
                    .files
                    .values()
                    .next()
                    .expect("same file")
                    .entity_id(),
                file_view.entity_id()
            );
        });
        assert!(focus_is(window, input_focus.clone(), cx));

        // Branch d (slot click) parity: with every tab closed, the slot
        // creates the next terminal.
        click_mounted(window, "workspace-tab-close-File(1)", cx);
        cx.run_until_parked();
        click_mounted(window, "main-header-terminal", cx);
        let _ = mounted_bounds(window, "bottom-workspace-pane", cx);
        assert_eq!(
            root.read_with(cx, |root, _| root.workspace.terminals.len()),
            1
        );
        assert!(focus_is(window, input_focus, cx));
    }

    #[gpui_kit::test]
    async fn r45_terminal_owns_bottom_and_right_hide_paths(cx: &mut TestAppContext) {
        const MARKER: &str = "VEGA_R45_TERMINAL_OWNERSHIP_CHILD";
        let Some(path) = std::env::var_os(MARKER) else {
            let root = tempfile::tempdir().expect("owned terminal home");
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    "window::workspace::terminal_tests::r45_terminal_owns_bottom_and_right_hide_paths",
                    "--nocapture",
                ])
                .env(MARKER, root.path())
                .env("HOME", root.path())
                .env("ZDOTDIR", root.path())
                .output()
                .expect("isolated test process");
            assert!(
                output.status.success(),
                "owned R45 terminal ownership subprocess failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        };
        let path = std::path::PathBuf::from(path);
        let (root, window, input) = r45_terminal_window(&path, cx);
        let input_focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("focus the task composer");

        // Create the first terminal in the bottom dock and take a PTY probe.
        cx.simulate_keystrokes(window.into(), "cmd-j");
        cx.run_until_parked();
        let _ = mounted_bounds(window, "bottom-workspace-pane", cx);
        let (first_key, first_view) = root.read_with(cx, |root, _| {
            let key = root.workspace.selected[1]
                .clone()
                .expect("selected terminal");
            let TabKey::Terminal(id) = &key else {
                panic!("terminal key")
            };
            (key.clone(), root.workspace.terminals[id].view.clone())
        });
        click_mounted(window, "workspace-tab-Terminal(1)", cx);
        window
            .update(cx, |_, window, cx| {
                first_view.update(cx, |view, cx| {
                    view.replace_text_in_range(None, "printf $$ > r45-own-pid-before", window, cx)
                });
            })
            .expect("type the pre-migration PTY probe");
        cx.simulate_keystrokes(window.into(), "enter");
        let pid_before = path.join("r45-own-pid-before");
        wait_for_marker(&pid_before, cx);
        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("return to the composer");

        // Dock the terminal to the right pane (rendered at this width).
        window
            .update(cx, |root, window, cx| {
                root.workspace_move_selected(1, window, cx)
            })
            .expect("dock the terminal right");
        cx.run_until_parked();
        let _ = mounted_bounds(window, "right-workspace-pane", cx);

        // Slot 3 is disabled while the right dock shows a terminal: the
        // click must not change any state.
        let state = |root: &VegaWindow, _: &gpui_kit::App| {
            (
                root.workspace.hidden,
                root.workspace.selected.clone(),
                root.workspace.terminals.len(),
                root.diff_controller.active.is_none(),
            )
        };
        let before = root.read_with(cx, state);
        click_mounted(window, "main-header-workspace-right", cx);
        assert_eq!(
            before,
            root.read_with(cx, state),
            "slot 3 must stay inert while a right terminal is rendered"
        );
        let _ = mounted_bounds(window, "right-workspace-pane", cx);

        // Slot 2 owns the hide path for the rendered right terminal.
        click_mounted(window, "main-header-terminal", cx);
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("right-workspace-pane")
                .is_none(),
            "slot 2 hides the rendered right terminal"
        );
        assert!(root.read_with(cx, |root, _| root.workspace.hidden[0]));
        assert!(focus_is(window, input_focus.clone(), cx));

        // Re-reveal (wide), then shrink below the responsive guard so the
        // right terminal unmounts while hidden == false.
        click_mounted(window, "main-header-terminal", cx);
        let _ = mounted_bounds(window, "right-workspace-pane", cx);
        assert!(focus_is(window, input_focus.clone(), cx));
        cx.update(|cx| cx.set_global(SidebarWidth(Layout::SIDEBAR_MAX_WIDTH)));
        window
            .update(cx, |_, window, cx| {
                window.resize(size(px(960.), px(600.)));
                window.bounds_changed(cx);
            })
            .expect("resize below the right-dock guard");
        cx.run_until_parked();
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("right-workspace-pane")
                .is_none(),
            "the narrow layout no longer renders the right terminal"
        );
        root.read_with(cx, |root, _| {
            assert!(!root.workspace.hidden[0]);
            assert_eq!(root.workspace.selected[0], Some(first_key.clone()));
        });

        // Slot 3 stays disabled for the unmounted right terminal...
        let before = root.read_with(cx, state);
        click_mounted(window, "main-header-workspace-right", cx);
        assert_eq!(
            before,
            root.read_with(cx, state),
            "slot 3 must stay inert while the right terminal is unmounted"
        );

        // ...and slot 2 performs the R44 migration: same tab, same PTY,
        // bottom dock, Composer focus.
        click_mounted(window, "main-header-terminal", cx);
        let _ = mounted_bounds(window, "bottom-workspace-pane", cx);
        root.read_with(cx, |root, _| {
            assert_eq!(root.workspace.terminals.len(), 1);
            assert_eq!(root.workspace.selected[1], Some(first_key.clone()));
            assert!(
                root.workspace
                    .tabs
                    .iter()
                    .any(|(key, bottom)| key == &first_key && *bottom)
            );
            assert_eq!(
                root.workspace
                    .terminals
                    .values()
                    .next()
                    .expect("same terminal")
                    .view
                    .entity_id(),
                first_view.entity_id()
            );
        });
        assert!(focus_is(window, input_focus.clone(), cx));

        // PTY identity: the migrated terminal is the same process.
        click_mounted(window, "workspace-tab-Terminal(1)", cx);
        window
            .update(cx, |_, window, cx| {
                first_view.update(cx, |view, cx| {
                    view.replace_text_in_range(None, "printf $$ > r45-own-pid-after", window, cx)
                });
            })
            .expect("type the post-migration PTY probe");
        cx.simulate_keystrokes(window.into(), "enter");
        let pid_after = path.join("r45-own-pid-after");
        wait_for_marker(&pid_after, cx);
        assert_eq!(
            std::fs::read_to_string(&pid_before).expect("PID before migration"),
            std::fs::read_to_string(&pid_after).expect("PID after migration"),
            "the R44 migration preserves the terminal process"
        );
    }
}
