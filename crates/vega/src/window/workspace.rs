use super::*;
use vega_ui::artifact_card::ArtifactCard;
use vega_ui::icons::{Icon, icon_button};
use vega_ui::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade, ScrollableFadeExt};

const MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH: f32 = 610.;
const MIN_BOTTOM_WORKSPACE_WINDOW_HEIGHT: f32 = 480.;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum TabKey {
    Subagents,
    Diff,
    Image,
    Terminal(u64),
    File(u64),
    Artifact(ArtifactCardId),
    Automation(vega_ui::automation_editor::AutomationEditorKey),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceCreateAction {
    Subagents,
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
    automation_previous_maximized: Option<[bool; 2]>,
    tab_scroll: [ScrollHandle; 2],
    artifact_scrolls: std::collections::HashMap<ArtifactCardId, gpui_kit::UniformListScrollHandle>,
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
    pub(super) fn show_subagent_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_workspace_route(cx);
        self.workspace.open(TabKey::Subagents);
        self.workspace.menu = false;
        self.environment_overlay_open = false;
        self.sync_subagent_workspace_size(window, cx);
        window.focus(&self.subagent_panel.focus, cx);
        cx.notify();
    }

    pub(super) fn sync_subagent_workspace_size(&mut self, window: &Window, cx: &App) {
        let selected =
            self.workspace.selected[0] == Some(TabKey::Subagents) && !self.workspace.hidden[0];
        if selected
            && self.workspace_available_width(window, cx) < MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH
        {
            self.subagent_panel
                .previous_maximized
                .get_or_insert(self.workspace.maximized);
            self.workspace.maximized = [true, false];
        } else if let Some(previous) = self.subagent_panel.previous_maximized.take() {
            self.workspace.maximized = previous;
        }
    }

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
        if let TabKey::Artifact(id) = key {
            self.artifact_scrolls.remove(id);
        }
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
        self.workspace.tabs.iter().any(|(key, _)| {
            matches!(
                key,
                TabKey::Artifact(_) | TabKey::File(_) | TabKey::Image | TabKey::Subagents
            )
        })
    }

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

    pub(super) fn right_workspace_slot_available(&self, window: &Window, cx: &App) -> bool {
        self.right_workspace_rendered_non_terminal(window, cx)
            || self.hidden_workspace_available(0)
            || (self.shell_project_thread(cx).is_some()
                && !self.right_workspace_rendered(window, cx)
                && self.right_workspace_renderable(window, cx))
    }

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
            .find(|(key, _)| {
                matches!(
                    key,
                    TabKey::Artifact(_) | TabKey::File(_) | TabKey::Image | TabKey::Subagents
                )
            })
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

    pub(super) fn show_automation_workspace(
        &mut self,
        key: vega_ui::automation_editor::AutomationEditorKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_workspace_route(cx);
        self.workspace.open(TabKey::Automation(key.clone()));
        self.sync_automation_workspace_size(window, cx);
        let index = self
            .workspace
            .tabs
            .iter()
            .find(|(tab, _)| matches!(tab, TabKey::Automation(open) if open == &key))
            .map_or(0, |(_, bottom)| usize::from(*bottom));
        self.workspace_focus(index, window, cx);
        self.sync_automation_editor_visibility(window, cx);
        cx.notify();
    }

    pub(super) fn sync_automation_workspace_size(&mut self, window: &Window, cx: &App) {
        let selected = matches!(self.workspace.selected[0], Some(TabKey::Automation(_)))
            && !self.workspace.hidden[0];
        if selected
            && self.workspace_available_width(window, cx) < MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH
        {
            self.workspace
                .automation_previous_maximized
                .get_or_insert(self.workspace.maximized);
            self.workspace.maximized = [true, false];
        } else if let Some(previous) = self.workspace.automation_previous_maximized.take() {
            self.workspace.maximized = previous;
        }
    }

    pub(super) fn visible_automation_editors(
        &self,
        window: &Window,
        cx: &App,
    ) -> Vec<&vega_ui::automation_editor::AutomationEditorKey> {
        let mut result = Vec::new();
        if cx.global::<SettingsOpen>().0 {
            return result;
        }
        for index in 0..2 {
            if self
                .workspace_fullscreen_index()
                .is_some_and(|visible| visible != index)
            {
                continue;
            }
            let visible = if index == 0 {
                self.right_workspace_rendered(window, cx)
            } else {
                self.bottom_workspace_selected_is_rendered(window)
            };
            if visible
                && let Some(TabKey::Automation(key)) = &self.workspace.selected[index]
                && self.workspace.route.as_ref() == Some(&key.thread_id)
            {
                result.push(key);
            }
        }
        result
    }

    pub(super) fn close_automation_workspace(
        &mut self,
        key: &vega_ui::automation_editor::AutomationEditorKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace_close_tab(&TabKey::Automation(key.clone()), window, cx);
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
            TabKey::Subagents => {
                self.clear_subagent_detail(cx);
                if let Some(previous) = self.subagent_panel.previous_maximized.take() {
                    self.workspace.maximized = previous;
                }
            }
            TabKey::Terminal(id) => {
                self.workspace.terminals.remove(id);
            }
            TabKey::File(id) => {
                self.workspace.files.remove(id);
            }
            TabKey::Diff => self.diff_controller.close(),
            TabKey::Artifact(_) => {}
            TabKey::Automation(_) => {
                self.sync_automation_workspace_size(window, cx);
                self.sync_automation_editor_visibility(window, cx);
                self.workspace_focus_composer(window, cx);
                cx.notify();
                return;
            }
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
        self.sync_automation_workspace_size(window, cx);
        self.sync_automation_editor_visibility(window, cx);
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
            TabKey::Subagents => "Subagents".into(),
            TabKey::Image => self
                .image_preview
                .active
                .as_ref()
                .map(|active| active.image.read(cx).name().to_owned())
                .unwrap_or_default(),
            TabKey::Diff => "Review".into(),
            TabKey::Automation(key) => if key.kind == AutomationKind::Goal {
                "编辑目标"
            } else {
                "编辑循环"
            }
            .into(),
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
            TabKey::Subagents => Icon::Bot,
            TabKey::Image => Icon::Camera,
            TabKey::Diff | TabKey::File(_) => Icon::Document,
            TabKey::Terminal(_) => Icon::Terminal,
            TabKey::Artifact(_) => Icon::Summary,
            TabKey::Automation(key) => {
                if key.kind == AutomationKind::Goal {
                    Icon::Goal
                } else {
                    Icon::Refresh
                }
            }
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
        if self.has_subagents(cx) {
            actions.push(WorkspaceCreateAction::Subagents);
        }
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
        self.sync_automation_editor_visibility(window, cx);
        let focus = match self.workspace.selected[index].as_ref() {
            Some(TabKey::Subagents) => Some(self.subagent_panel.focus.clone()),
            Some(TabKey::Automation(key)) => self
                .automation_editors
                .get(key)
                .map(|view| view.read(cx).focus_handle(cx)),
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

    pub(super) fn workspace_fullscreen_index(&self) -> Option<usize> {
        (0..2).find(|index| {
            self.workspace.maximized[*index]
                && !self.workspace.hidden[*index]
                && self.workspace.selected[*index].is_some()
        })
    }

    pub(super) fn workspace_pane_header_in_top_band(&self, bottom: bool) -> bool {
        !bottom || self.workspace_fullscreen_index() == Some(usize::from(bottom))
    }

    pub(super) fn toggle_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.environment_available(cx) || self.persistent_right_workspace_visible(window, cx) {
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
        if !self.environment_available(cx) {
            return div().into_any_element();
        }
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
        card = card.children(self.render_subagent_overview(cx));
        if self.shell_project_id(cx).is_some() {
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
        }
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
            .gap_1()
            .tracked_scroll_edge_fade(
                self.workspace.tab_scroll[index].clone(),
                colors.bg_base,
                ScrollFade::compact(),
            );
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
                        if this.workspace.selected[index] == Some(TabKey::Subagents) {
                            this.subagent_panel.previous_maximized = None;
                        }
                        this.workspace.maximized[index] = !this.workspace.maximized[index];
                        this.workspace.reveal_tabs[index] = true;
                        this.workspace_focus(index, window, cx);
                        cx.notify();
                    }),
                )
                .debug_selector(move || maximize_selector.into()),
            );

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
            Some(TabKey::Subagents) => Some(self.render_subagent_panel(window, cx)),
            Some(TabKey::Automation(key)) => self
                .automation_editors
                .get(&key)
                .map(|view| view.clone().into_any_element()),
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
                    let scroll = self
                        .workspace
                        .artifact_scrolls
                        .entry(id)
                        .or_default()
                        .clone();
                    uniform_list("workspace-preview", count, move |range, window, cx| {
                        range
                            .map(|row| ArtifactCard::render_row(card.clone(), row, window, cx))
                            .collect()
                    })
                    .size_full()
                    .track_scroll(&scroll)
                    .tracked_scroll_edge_fade(scroll, colors.bg_base, ScrollFade::reading())
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

        let fullscreen = self.workspace_fullscreen_index();
        let subagent_pane = (0..2).find(|index| {
            self.workspace.selected[*index] == Some(TabKey::Subagents)
                && !self.workspace.hidden[*index]
                && (fullscreen == Some(*index)
                    || (fullscreen.is_none() && if *index == 0 { right } else { bottom }))
        });
        self.set_subagent_preview_visible(
            subagent_pane.is_some(),
            if fullscreen.is_some() || subagent_pane == Some(1) {
                available
            } else {
                width
            },
            cx,
        );
        if right {
            self.environment_overlay_open = false;
        }
        if environment_wide {
            self.environment_overlay_open = false;
        }
        let environment_rail = fullscreen.is_none()
            && !right
            && self.environment_available(cx)
            && environment_wide
            && !self.environment_collapsed;
        let environment_overlay = fullscreen.is_none()
            && !right
            && self.environment_available(cx)
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
                .gap_1()
                .scroll_edge_fade(colors.bg_elevated, ScrollFade::compact());
            for action in creation_actions {
                menu = match action {
                    WorkspaceCreateAction::Subagents => menu.child(workspace_button(
                        "workspace-add-subagents",
                        "Subagents",
                        colors,
                        cx.listener(|this, _, window, cx| this.open_subagent_panel(window, cx)),
                    )),
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
