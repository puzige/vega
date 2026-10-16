use super::*;
use vega_ui::artifact_card::ArtifactCard;
use vega_ui::icons::{Icon, icon_button};

const MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH: f32 = 610.;
const MIN_BOTTOM_WORKSPACE_WINDOW_HEIGHT: f32 = 480.;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum TabKey {
    Diff,
    Image,
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
    tab_focuses: std::collections::HashMap<TabKey, FocusHandle>,
    maximized: [bool; 2],
    width: Option<f32>,
    height: Option<f32>,
    dragging: Option<bool>,
    pub(super) composer_focus_pending: bool,
}

impl VegaWindow {
    pub(super) fn show_image_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_workspace_route(cx);
        self.workspace.close(&TabKey::Image);
        self.workspace.open(TabKey::Image);
        if self.workspace_available_width(window, cx) < MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH {
            self.image_preview
                .previous_maximized
                .get_or_insert(self.workspace.maximized);
            self.workspace.maximized = [true, false];
        } else if self.workspace.maximized[1] {
            self.image_preview
                .previous_maximized
                .get_or_insert(self.workspace.maximized);
            self.workspace.maximized = [false, false];
        }
        self.workspace_focus(0, window, cx);
        cx.notify();
    }

    pub(super) fn remove_image_workspace(&mut self) {
        self.workspace.close(&TabKey::Image);
        if let Some(previous) = self.image_preview.previous_maximized.take() {
            self.workspace.maximized = previous;
        }
    }

    pub(super) fn restore_image_workspace_size(&mut self, window: &Window, cx: &App) {
        if self.workspace_available_width(window, cx) >= MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH {
            if self
                .image_preview
                .previous_maximized
                .is_some_and(|previous| previous[1])
            {
                self.workspace.maximized = [false, false];
            } else if let Some(previous) = self.image_preview.previous_maximized.take() {
                self.workspace.maximized = previous;
            }
        } else if self.workspace.selected[0] == Some(TabKey::Image) && !self.workspace.hidden[0] {
            self.image_preview
                .previous_maximized
                .get_or_insert(self.workspace.maximized);
            self.workspace.maximized = [true, false];
        }
    }

    pub(super) fn workspace_has_terminals(&self) -> bool {
        !self.workspace.terminals.is_empty()
    }
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
        self.tab_focuses.remove(key);
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
        if crate::updater::installing() {
            return;
        }
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
            let target = vega_conversation::types::TerminalTarget::Project {
                database_path,
                project_id: project_id.clone(),
            };
            #[cfg(not(test))]
            {
                vega_ui::terminal::TerminalView::for_target(target, cx)
            }
            #[cfg(test)]
            {
                vega_ui::terminal::TerminalView::for_test_target(target, cx)
            }
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
            .any(|(key, _)| matches!(key, TabKey::Artifact(_) | TabKey::File(_) | TabKey::Image))
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
            .find(|(key, _)| matches!(key, TabKey::Artifact(_) | TabKey::File(_) | TabKey::Image))
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
            TabKey::Image => {
                self.close_image_preview(window, cx);
                return;
            }
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

    pub(super) fn workspace_focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, stream)) = &self.stream_view {
            stream.update(cx, |stream, cx| stream.focus_composer(window, cx));
        }
    }

    fn workspace_label(&self, key: &TabKey, cx: &App) -> String {
        match key {
            TabKey::Image => self
                .image_preview
                .active
                .as_ref()
                .map(|active| active.image.read(cx).name().to_owned())
                .unwrap_or_default(),
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

    fn workspace_tab_icon(key: &TabKey) -> Icon {
        match key {
            TabKey::Image => Icon::Camera,
            TabKey::Diff | TabKey::File(_) => Icon::Document,
            TabKey::Terminal(_) => Icon::Terminal,
            TabKey::Artifact(_) => Icon::Summary,
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
                DiffFocusIntent::PreserveCurrent,
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
            Some(TabKey::Image) => self
                .image_preview
                .active
                .as_ref()
                .map(|active| active.image.read(cx).focus_handle(cx)),
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

    fn workspace_activate_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus_intent = if self.workspace.selected[index] == Some(TabKey::Diff) {
            DiffFocusIntent::ExplicitDiffTab
        } else {
            DiffFocusIntent::PreserveCurrent
        };
        if let Some(active) = self.diff_controller.active.as_mut() {
            active.focus_intent = focus_intent;
        }
        self.workspace_focus(index, window, cx);
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
            // R69 R6: every affordance this accessor gates (the Environment
            // card's Changes/Review row, the right dock's Review slot, the
            // workspace creation menu's Review action) opens a task-scoped
            // workspace route. The draft has no durable row, so the same fence
            // that keeps `ensure_branch_route` and `ensure_artifact_route` from
            // beginning on it applies here — project context still renders
            // (the Environment folder row via `shell_project_label`, and the
            // R49 utility bar via its own binding), but the git route is not
            // offered until first submit materializes the draft.
            .filter(|thread| !self.is_draft_route(&thread.id))
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
        if self.workspace.selected[index] == Some(TabKey::Diff) {
            if let Some(active) = self.diff_controller.active.as_mut() {
                active.focus_intent = DiffFocusIntent::PreserveCurrent;
            }
        } else {
            self.workspace_focus(index, window, cx);
        }
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
    /// A fullscreen dock replaces the whole workspace layout, so its own
    /// header row opens the window's top band (see
    /// [`Self::workspace_pane_header_in_top_band`]); the R46 window-anchored
    /// cluster stays mounted and that header reserves its trailing band.
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
    /// Top-band headers must reserve the shell slot cluster's trailing band
    /// plus the R47 ownership gutter.
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
            let tab_color = if active {
                colors.text_primary
            } else {
                colors.text_secondary
            };
            let tab_icon = Self::workspace_tab_icon(&key);
            let tab_group = SharedString::from(format!("workspace-tab-group-{key:?}"));
            let tab_selector = SharedString::from(format!("workspace-tab-{key:?}"));
            let close_selector = SharedString::from(format!("workspace-tab-close-{key:?}"));
            let tab_focus = self
                .workspace
                .tab_focuses
                .entry(key.clone())
                .or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            let close = icon_button(
                Icon::Close,
                format!("关闭 {}", self.workspace_label(&close_key, cx)),
                colors,
                cx.listener(move |this, _, window, cx| {
                    this.workspace_close_tab(&close_key, window, cx);
                }),
            )
            .when(!active, |close| {
                close
                    .opacity(0.)
                    .group_hover(tab_group.clone(), |style| style.opacity(1.))
                    .in_focus(|style| style.opacity(1.))
                    .focus_visible(|style| style.opacity(1.))
            })
            .debug_selector(move || close_selector.to_string());
            strip = strip.child(
                div()
                    .id(tab_selector.clone())
                    .debug_selector(move || tab_selector.to_string())
                    .aria_label(label.clone())
                    .flex_shrink_0()
                    .max_w_full()
                    .focusable()
                    .track_focus(&tab_focus)
                    .tab_stop(true)
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.workspace.open(keyboard_key.clone());
                            this.workspace_activate_tab(index, window, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .flex()
                    .items_center()
                    .group(tab_group)
                    .h(px(Layout::TAB_HEIGHT))
                    .gap(px(Layout::TAB_CONTENT_GAP))
                    .px(px(Layout::TAB_HORIZONTAL_INSET))
                    .rounded(px(Layout::TAB_RADIUS))
                    // Reserve the focus ring's 1px border so keyboard focus
                    // cannot change the tab's text or close-control geometry.
                    .border_1()
                    .border_color(colors.bg_base.alpha(0.))
                    .focus_visible(|style| style.border_color(colors.accent))
                    .bg(if active {
                        // R50: the pill sits on the pane's white `bg_base`
                        // surface, so the selected fill must be derived for
                        // that surface (5% ink → #f4f4f4). The flattened
                        // `bg_active` (#ededed) is the same rule already
                        // composited over the sidebar's #f9f9f9 and reads 7
                        // levels too dark here.
                        colors.bg_active_alpha
                    } else {
                        colors.bg_sidebar.opacity(0.)
                    })
                    .hover(move |s| s.bg(colors.bg_hover))
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.workspace.open(key.clone());
                            this.workspace_activate_tab(index, window, cx);
                            cx.notify();
                        }),
                    )
                    .child(vega_ui::icons::icon(tab_icon, tab_color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(150.))
                            .truncate()
                            .text_size(px(Typography::SIDEBAR))
                            .text_color(tab_color)
                            .child(label),
                    )
                    .child(close),
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
                    Icon::DockMove,
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
                        this.image_preview.previous_maximized = None;
                        this.workspace.maximized[index] = !this.workspace.maximized[index];
                        this.workspace.reveal_tabs[index] = true;
                        this.workspace_focus(index, window, cx);
                        cx.notify();
                    }),
                )
                .debug_selector(move || maximize_selector.into()),
            );
        // R46 §2.1.1 / R47 §2.2: a pane header that opens the window's 46px
        // top band reserves the window-anchored slot cluster's trailing band
        // plus the ownership gutter, so the pane's own trailing actions
        // (commit / add / dock / maximize) lay out to the left of the slots —
        // separated by an intentional 32px blank gap — instead of underneath
        // them or flush against them. The bottom-docked pane header lives in
        // the bottom band and keeps its compact `px_1` trailing inset.
        let header = div()
            .debug_selector(move || header_selector.into())
            .flex()
            .items_center()
            .gap_1()
            .h(px(Layout::WORKSPACE_HEADER_HEIGHT))
            .px_1()
            .when(self.workspace_pane_header_in_top_band(bottom), |header| {
                header.pr(px(
                    Layout::SHELL_SLOT_CLUSTER_RESERVE + Layout::SHELL_SLOT_GUTTER
                ))
            })
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
            Some(TabKey::Image) => self
                .image_preview
                .active
                .as_ref()
                .map(|active| active.image.clone().into_any_element()),
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
                        && active.focus_intent == DiffFocusIntent::ExplicitDiffTab
                    {
                        window.focus(&view.read(cx).focus_handle(cx), cx);
                        active.focus_intent = DiffFocusIntent::PreserveCurrent;
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
        self.sync_image_preview(window, cx);
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
    use super::{TabKey, VegaWindow};
    use crate::tests::{diff_controller_repo, install_diff_window_globals};
    use gpui_kit::prelude::*;
    use gpui_kit::{
        AppContext, Bounds, FocusHandle, Focusable, KeyBinding, Modifiers, Pixels, TestAppContext,
        VisualTestContext, WindowHandle,
    };
    use vega_ui::diff_view::DiffClosed;
    use vega_ui::settings::{CloseSettings, SettingsOpen, SettingsView};
    use vega_ui::sidebar::{OpenedThread, PendingDeleteConfirm};

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

    fn focus_is(
        window: WindowHandle<VegaWindow>,
        focus: &FocusHandle,
        cx: &mut TestAppContext,
    ) -> bool {
        window
            .update(cx, |_, window, _| focus.is_focused(window))
            .expect("workspace focus window")
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
        let composer_focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("focus the Composer");
        shell_click(window, "main-header-workspace-right", cx);
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
        let diff_focus = view.read_with(cx, |view, cx| view.focus_handle(cx));
        assert!(
            focus_is(window, &composer_focus, cx),
            "global Review reveal keeps Composer focus"
        );
        shell_click(window, "workspace-tab-Diff", cx);
        assert!(
            focus_is(window, &diff_focus, cx),
            "explicit Diff tab activation focuses Diff"
        );
        let diff_tab_focus = root.read_with(cx, |root, _| {
            root.workspace
                .tab_focuses
                .get(&TabKey::Diff)
                .cloned()
                .expect("Diff tab focus handle")
        });
        window
            .update(cx, |_, window, cx| window.focus(&diff_tab_focus, cx))
            .expect("focus the Diff tab");
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert!(
            focus_is(window, &diff_focus, cx),
            "keyboard Diff tab activation focuses Diff"
        );
        window
            .update(cx, |root, window, cx| {
                root.workspace_focus_composer(window, cx)
            })
            .expect("refocus the Composer");
        root.update(cx, |root, cx| root.workspace_open_diff(cx));
        cx.run_until_parked();
        assert!(
            focus_is(window, &composer_focus, cx),
            "repeated global Review reveal keeps Composer focus"
        );
        shell_click(window, "main-header-workspace-right", cx);
        assert!(root.read_with(cx, |root, _| root.workspace.hidden[0]));
        shell_click(window, "main-header-workspace-right", cx);
        assert!(root.read_with(cx, |root, _| !root.workspace.hidden[0]
            && root.workspace.selected[0] == Some(TabKey::Diff)));
        assert!(
            focus_is(window, &composer_focus, cx),
            "global Review restore keeps Composer focus"
        );
        shell_click(window, "workspace-tab-Diff", cx);
        assert!(focus_is(window, &diff_focus, cx));
        root.update(cx, |root, cx| root.workspace_open_diff(cx));
        cx.run_until_parked();
        assert!(
            focus_is(window, &diff_focus, cx),
            "repeated global Review reveal preserves explicit Diff focus"
        );
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

#[cfg(test)]
mod terminal_tests {
    use super::{TabKey, VegaWindow};
    use crate::tests::install_diff_window_globals;
    use gpui_kit::Focusable;
    use gpui_kit::{AppContext, TestAppContext, VisualTestContext};
    use vega_theme::Layout;
    use vega_ui::sidebar::SelectedProject;

    struct NonrepositoryGuards {
        _guards: Vec<vega_conversation::GitTestCommandGuard>,
    }
    impl gpui_kit::Global for NonrepositoryGuards {}

    fn register_nonrepository(paths: &[&std::path::Path], cx: &mut TestAppContext) {
        let guards = paths
            .iter()
            .map(|path| {
                vega_conversation::register_git_test_executor(
                    path,
                    std::sync::Arc::new(|command, _, _| {
                        let args = command
                            .get_args()
                            .map(|arg| arg.to_string_lossy().into_owned())
                            .collect::<Vec<_>>();
                        assert!(
                            args.ends_with(&["rev-parse".into(), "--show-toplevel".into()]),
                            "unexpected nonrepository command: {args:?}"
                        );
                        Err(vega_conversation::types::GitWorkspaceError::for_test(
                            vega_conversation::types::GitWorkspaceErrorCode::NotRepository,
                        ))
                    }),
                )
                .unwrap()
            })
            .collect();
        cx.update(|cx| cx.set_global(NonrepositoryGuards { _guards: guards }));
    }

    fn assert_pixel_close(actual: gpui_kit::Pixels, expected: f32, label: &str) {
        let actual = f32::from(actual);
        assert!(
            (actual - expected).abs() <= 1.0,
            "{label}: expected {expected}±1px, got {actual}px"
        );
    }

    #[gpui_kit::test]
    async fn terminal_workspace_production_handlers_preserve_docking_and_project_isolation(
        cx: &mut TestAppContext,
    ) {
        let owned = tempfile::tempdir().expect("owned terminal fixture");
        let path = owned.path().to_path_buf();

        let store = vega_store::Store::open(path.join("test.db")).unwrap();
        store.migrate().unwrap();
        let first =
            vega_store::projects::create(store.conn(), path.to_str().unwrap(), "first", None)
                .unwrap();
        let other = path.join("other");
        std::fs::create_dir(&other).unwrap();
        register_nonrepository(&[&path, &other], cx);
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
            // that now sits left of the reserved band" case §3 names. R47 §2.2
            // extends the reservation with the ownership gutter.
            assert_pixel_close(
                header.right() - actions.right(),
                Layout::SHELL_SLOT_CLUSTER_RESERVE + Layout::SHELL_SLOT_GUTTER,
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
    }
}
