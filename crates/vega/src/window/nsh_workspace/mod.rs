use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use vega_ui::nsh::{ProjectionId, Surface};

pub(in crate::window) mod controller;
mod detail_layout;
mod separator;
mod view;

pub(in crate::window) use separator::{background, drag_region, render as separator};

pub(in crate::window) use detail_layout::detail_body;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::window) enum Kind {
    Header(usize),
    Environment(bool),
    Menu,
    Error,
    Subagents,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::window) enum Mode {
    Header,
    Environment,
    Menu,
    Error,
    Panel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::window) enum Action {
    Select(TabKey),
    Close(TabKey),
    Hide,
    Move,
    Maximize,
    Commit,
    Menu,
    Create(WorkspaceCreateAction),
    CloseEnvironment,
    Review,
    Terminal,
    Subagents,
    RetryAgents,
    Detail(String),
    Back,
    Open(String),
    Stop(String, u64),
    Reload,
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::window) struct Control {
    action: Action,
    label: String,
    icon: Option<Icon>,
    icon_only: bool,
    chosen: bool,
}

impl Control {
    pub(in crate::window) fn new(
        action: Action,
        label: impl Into<String>,
        icon: Option<Icon>,
    ) -> Self {
        Self {
            action,
            label: label.into(),
            icon,
            icon_only: false,
            chosen: false,
        }
    }
    fn icon(action: Action, label: impl Into<String>, icon: Icon) -> Self {
        Self {
            icon_only: true,
            ..Self::new(action, label, Some(icon))
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Tab {
    open: Control,
    close: Control,
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::window) struct Line {
    text: String,
    error: bool,
}

impl Line {
    pub(in crate::window) fn new(text: impl Into<String>, error: bool) -> Self {
        Self {
            text: text.into(),
            error,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(in crate::window) struct AgentIdentity {
    pub(in crate::window) child: String,
    pub(in crate::window) parent: String,
    pub(in crate::window) parent_message: String,
    pub(in crate::window) message: String,
    pub(in crate::window) generation: Option<u64>,
    pub(in crate::window) operation: Option<String>,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(in crate::window) struct PanelAuthority {
    pub(in crate::window) parent: Option<String>,
    pub(in crate::window) epoch: u64,
    pub(in crate::window) selection: u64,
    pub(in crate::window) revision: u64,
    pub(in crate::window) history_epoch: u64,
    pub(in crate::window) detail: Option<String>,
    pub(in crate::window) agents: Vec<AgentIdentity>,
}

#[derive(Clone, PartialEq, Eq)]
struct Authority {
    route: Option<String>,
    project: Option<String>,
    tabs: Vec<(TabKey, bool)>,
    selected: [Option<TabKey>; 2],
    hidden: [bool; 2],
    maximized: [bool; 2],
    menu: bool,
    menu_bottom: bool,
    environment: (bool, bool),
    panel: Option<PanelAuthority>,
}

#[derive(Clone)]
pub(in crate::window) struct Snapshot {
    id: ProjectionId,
    mount: ProjectionId,
    kind: Kind,
    mode: Mode,
    authority: Authority,
    tabs: Vec<Tab>,
    pub(in crate::window) controls: Vec<Control>,
    pub(in crate::window) lines: Vec<Line>,
    colors: ThemeColors,
    environment: (&'static str, vega_theme::Appearance),
    selected: Option<Action>,
    focused: bool,
    accessible: Option<ProjectionId>,
    recovery: bool,
}

impl Snapshot {
    fn same(&self, other: &Self) -> bool {
        self.same_content(other)
            && match (&self.accessible, &other.accessible) {
                (None, None) => true,
                (Some(left), Some(right)) => left.matches(right),
                _ => false,
            }
    }

    fn same_content(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.mount.matches(&other.mount)
            && self.kind == other.kind
            && self.mode == other.mode
            && self.authority == other.authority
            && self.tabs == other.tabs
            && self.controls == other.controls
            && self.lines == other.lines
            && self.environment == other.environment
            && self.recovery == other.recovery
    }

    fn accessible_request(&self, observed: &Self, action: &Action) -> Option<ProjectionId> {
        self.accepts(observed, action).then(ProjectionId::new)
    }

    fn measured_revision(
        &self,
        observed: &Self,
        action: &Action,
        selected: Option<&Action>,
        geometry: &Geometry,
        rect: [f32; 4],
    ) -> Option<u64> {
        (self.accepts(observed, action) && selected == Some(action) && geometry.visible(rect))
            .then_some(geometry.revision)
    }

    fn accepts(&self, observed: &Self, action: &Action) -> bool {
        self.same(observed) && self.enabled().contains(action)
    }

    fn enabled(&self) -> Vec<Action> {
        if self.recovery {
            return vec![Action::Reload];
        }
        let mut actions = Vec::new();
        if self.mode == Mode::Header {
            actions.extend(self.controls.first().map(|control| control.action.clone()));
            for tab in &self.tabs {
                actions.push(tab.open.action.clone());
                actions.push(tab.close.action.clone());
            }
            actions.extend(
                self.controls
                    .iter()
                    .skip(1)
                    .map(|control| control.action.clone()),
            );
        } else {
            actions.extend(self.controls.iter().map(|control| control.action.clone()));
        }
        actions
    }
    fn control(&self, action: &Action) -> Option<Control> {
        if self.recovery && *action == Action::Reload {
            return Some(Control::new(
                Action::Reload,
                vega_i18n::t("app.workspace.retry"),
                None,
            ));
        }
        self.controls
            .iter()
            .chain(self.tabs.iter().flat_map(|tab| [&tab.open, &tab.close]))
            .find(|control| &control.action == action)
            .cloned()
    }
}

pub(in crate::window) enum Request {
    Act(Box<Snapshot>, Action),
    Accessible(Box<Snapshot>, Action),
    Measured(
        Box<Snapshot>,
        Action,
        [f32; 4],
        tokio::sync::oneshot::Sender<Option<u64>>,
    ),
    Displayed(
        Box<Snapshot>,
        Action,
        [f32; 4],
        u64,
        tokio::sync::oneshot::Sender<bool>,
    ),
    Scrolled(ProjectionId),
}

#[derive(Default)]
struct Geometry {
    revision: u64,
    bounds: Option<Bounds<Pixels>>,
    clip: Option<Bounds<Pixels>>,
}

impl Geometry {
    fn visible(&self, rect: [f32; 4]) -> bool {
        if !rect.iter().all(|value| value.is_finite()) || rect[2] <= 0. || rect[3] <= 0. {
            return false;
        }
        let Some(root) = self.bounds else {
            return false;
        };
        let Some(clip) = self.clip.map(|clip| root.intersect(&clip)) else {
            return false;
        };
        rect[0] >= -0.5
            && rect[1] >= -0.5
            && root.origin.x + px(rect[0]) >= clip.origin.x - px(0.5)
            && root.origin.y + px(rect[1]) >= clip.origin.y - px(0.5)
            && root.origin.x + px(rect[0] + rect[2]) <= clip.origin.x + clip.size.width + px(0.5)
            && root.origin.y + px(rect[1] + rect[3]) <= clip.origin.y + clip.size.height + px(0.5)
    }
}

pub(in crate::window) struct Page {
    surface: Surface<Snapshot, Request>,
    recovery: Option<Surface<Snapshot, Request>>,
    id: ProjectionId,
    mount: ProjectionId,
    focus: FocusHandle,
    selected: Option<Action>,
    accessible: Option<ProjectionId>,
    shown: Snapshot,
    displayed: Option<(Snapshot, Action, [f32; 4])>,
    lease: Weak<()>,
    geometry: Rc<RefCell<Geometry>>,
    _subscriptions: Vec<Subscription>,
}

pub(in crate::window) type Pages = HashMap<Kind, Page>;

pub(in crate::window) fn reveal_delta(
    start: f32,
    length: f32,
    clip: f32,
    available: f32,
) -> Option<f32> {
    if ![start, length, clip, available]
        .iter()
        .all(|value| value.is_finite())
        || length <= 0.
        || available <= 0.
        || length > available + 0.5
    {
        return None;
    }
    Some(if start < clip {
        start - clip
    } else {
        (start + length - clip - available).max(0.)
    })
}

impl Workspace {
    pub(super) fn invalidate_nsh(&mut self) {
        for page in self.nsh.values_mut() {
            page.mount = ProjectionId::new();
            page.displayed = None;
            page.accessible = None;
        }
    }
}

impl VegaWindow {
    pub(in crate::window) fn set_workspace_presentation_visible(
        &mut self,
        visible: bool,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.nsh_visible == visible {
            return;
        }
        self.workspace.nsh_visible = visible;
        if !visible {
            for page in self.workspace.nsh.values_mut() {
                page.mount = ProjectionId::new();
                page.displayed = None;
                page.selected = None;
                page.accessible = None;
            }
            self.set_subagent_preview_visible(false, 0., cx);
        }
    }

    fn nsh_pane_visible(&self, index: usize, window: &Window, cx: &App) -> bool {
        self.workspace.panes().visible(
            index,
            if index == 0 {
                self.workspace_available_width(window, cx)
            } else {
                f32::from(window.bounds().size.height)
            },
        )
    }

    fn nsh_kind_visible(&self, kind: Kind, window: &Window, cx: &App) -> bool {
        if !self.workspace.nsh_visible
            || cx.global::<SettingsOpen>().0
            || self.workspace.route
                != cx
                    .global::<OpenedThread>()
                    .0
                    .as_ref()
                    .map(|thread| thread.id.clone())
            || self.workspace.project_route != cx.global::<SelectedProject>().0
        {
            return false;
        }
        match kind {
            Kind::Header(index) => self.nsh_pane_visible(index, window, cx),
            Kind::Environment(overlay) => {
                self.workspace_fullscreen_index().is_none()
                    && !self.persistent_right_workspace_visible(window, cx)
                    && self.environment_available(cx)
                    && if overlay {
                        !self.environment_is_wide(window, cx) && self.environment_overlay_open
                    } else {
                        self.environment_is_wide(window, cx) && !self.environment_collapsed
                    }
            }
            Kind::Menu => {
                self.workspace.menu
                    && self.nsh_pane_visible(usize::from(self.workspace.menu_bottom), window, cx)
            }
            Kind::Error => self.workspace.terminal_error,
            Kind::Subagents => (0..2).any(|index| {
                self.workspace.selected[index] == Some(TabKey::Subagents)
                    && self.nsh_pane_visible(index, window, cx)
            }),
        }
    }

    fn nsh_workspace_snapshot(
        &self,
        kind: Kind,
        id: ProjectionId,
        mount: ProjectionId,
        window: &Window,
        cx: &App,
    ) -> Option<Snapshot> {
        if !self.nsh_kind_visible(kind, window, cx) {
            return None;
        }
        let mode = match kind {
            Kind::Header(_) => Mode::Header,
            Kind::Environment(_) => Mode::Environment,
            Kind::Menu => Mode::Menu,
            Kind::Error => Mode::Error,
            Kind::Subagents => Mode::Panel,
        };
        let mut snapshot = Snapshot {
            id,
            mount,
            kind,
            mode,
            authority: Authority {
                route: self.workspace.route.clone(),
                project: self.workspace.project_route.clone(),
                tabs: self.workspace.tabs.clone(),
                selected: self.workspace.selected.clone(),
                hidden: self.workspace.hidden,
                maximized: self.workspace.maximized,
                menu: self.workspace.menu,
                menu_bottom: self.workspace.menu_bottom,
                environment: (self.environment_collapsed, self.environment_overlay_open),
                panel: matches!(kind, Kind::Subagents | Kind::Environment(_) | Kind::Menu)
                    .then(|| self.nsh_subagent_authority()),
            },
            tabs: Vec::new(),
            controls: Vec::new(),
            lines: Vec::new(),
            colors: theme(cx).colors,
            environment: (vega_i18n::language(), theme(cx).appearance),
            selected: None,
            focused: false,
            accessible: None,
            recovery: false,
        };
        match kind {
            Kind::Header(index) => {
                let bottom = index == 1;
                snapshot.controls.push(Control::icon(
                    Action::Hide,
                    vega_i18n::t("app.workspace.hide_panel"),
                    if bottom {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                ));
                for (key, _) in self
                    .workspace
                    .tabs
                    .iter()
                    .filter(|(_, dock)| *dock == bottom)
                {
                    let name = self.workspace_label(key, cx);
                    let mut open = Control::new(
                        Action::Select(key.clone()),
                        name.clone(),
                        Some(Self::workspace_tab_icon(key)),
                    );
                    open.chosen = self.workspace.selected[index].as_ref() == Some(key);
                    snapshot.tabs.push(Tab {
                        open,
                        close: Control::icon(
                            Action::Close(key.clone()),
                            vega_i18n::t_args("app.workspace.close", &[("label", &name)]),
                            Icon::Close,
                        ),
                    });
                }
                if self.workspace.selected[index] == Some(TabKey::Diff) {
                    snapshot.controls.push(Control::icon(
                        Action::Commit,
                        vega_i18n::t("app.workspace.commit"),
                        Icon::Document,
                    ));
                }
                snapshot.controls.push(Control::icon(
                    Action::Menu,
                    vega_i18n::t("app.workspace.new_tab"),
                    Icon::Plus,
                ));
                snapshot.controls.push(Control::icon(
                    Action::Move,
                    vega_i18n::t(if bottom {
                        "app.workspace.move_right"
                    } else {
                        "app.workspace.move_bottom"
                    }),
                    Icon::DockMove,
                ));
                snapshot.controls.push(Control::icon(
                    Action::Maximize,
                    vega_i18n::t(if self.workspace.maximized[index] {
                        "app.workspace.restore"
                    } else {
                        "app.workspace.maximize"
                    }),
                    Icon::Maximize,
                ));
            }
            Kind::Environment(_) => {
                snapshot
                    .lines
                    .push(Line::new(vega_i18n::t("app.workspace.environment"), false));
                if let Some(label) = self.shell_project_label(cx) {
                    snapshot.lines.push(Line::new(label, false));
                }
                snapshot.controls.push(Control::icon(
                    Action::CloseEnvironment,
                    vega_i18n::t("app.workspace.close_environment"),
                    Icon::Close,
                ));
                if self.shell_project_thread(cx).is_some() {
                    snapshot.controls.push(Control::new(
                        Action::Review,
                        vega_i18n::t("app.workspace.changes_review"),
                        Some(Icon::Split),
                    ));
                }
                self.nsh_subagent_overview(&mut snapshot, cx);
                if self.shell_project_id(cx).is_some() {
                    snapshot.controls.push(Control::new(
                        Action::Terminal,
                        vega_i18n::t("app.workspace.local_terminal"),
                        Some(Icon::Terminal),
                    ));
                }
            }
            Kind::Menu => {
                for action in self.workspace_creation_actions(cx) {
                    let (label, icon) = match action {
                        WorkspaceCreateAction::Subagents => ("app.workspace.subagents", Icon::Bot),
                        WorkspaceCreateAction::Review => {
                            ("app.workspace.review_changes", Icon::Split)
                        }
                        WorkspaceCreateAction::NewTerminal => {
                            ("app.workspace.new_shell", Icon::Terminal)
                        }
                    };
                    snapshot.controls.push(Control::new(
                        Action::Create(action),
                        vega_i18n::t(label),
                        Some(icon),
                    ));
                }
            }
            Kind::Error => snapshot.lines.push(Line::new(
                vega_i18n::t("app.workspace.terminal_unavailable"),
                true,
            )),
            Kind::Subagents => self.nsh_subagent_controls(&mut snapshot, cx),
        }
        Some(snapshot)
    }

    fn nsh_workspace_current(&self, kind: Kind, window: &Window, cx: &App) -> Option<Snapshot> {
        let page = self.workspace.nsh.get(&kind)?;
        page.lease.upgrade()?;
        let mut snapshot =
            self.nsh_workspace_snapshot(kind, page.id.clone(), page.mount.clone(), window, cx)?;
        snapshot.recovery = page.recovery.is_some();
        snapshot.selected = page.selected.clone();
        snapshot.focused = page.focus.is_focused(window);
        snapshot.accessible = page.accessible.clone();
        Some(snapshot)
    }

    fn nsh_workspace_accepts(
        &self,
        observed: &Snapshot,
        action: &Action,
        window: &Window,
        cx: &App,
    ) -> bool {
        self.nsh_workspace_current(observed.kind, window, cx)
            .is_some_and(|current| current.accepts(observed, action))
    }

    fn nsh_workspace_guarded(
        &self,
        observed: &Snapshot,
        action: &Action,
        window: &Window,
        cx: &App,
    ) -> bool {
        self.nsh_workspace_accepts(observed, action, window, cx)
            && self.workspace.nsh.get(&observed.kind).is_some_and(|page| {
                page.focus.is_focused(window)
                    && page.selected.as_ref() == Some(action)
                    && page
                        .displayed
                        .as_ref()
                        .is_some_and(|(shown, displayed, rect)| {
                            shown.same(observed)
                                && displayed == action
                                && page.geometry.borrow().visible(*rect)
                        })
            })
    }

    fn nsh_workspace_action(
        &mut self,
        observed: &Snapshot,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.nsh_workspace_accepts(observed, &action, window, cx) {
            return;
        }
        if action == Action::Reload {
            if let Some(page) = self.workspace.nsh.remove(&observed.kind) {
                page.surface.entity().read(cx).stop();
                if let Some(recovery) = page.recovery {
                    recovery.entity().read(cx).stop();
                }
            }
            cx.notify();
            return;
        }
        if let Some(page) = self.workspace.nsh.get_mut(&observed.kind) {
            page.selected = Some(action.clone());
            page.displayed = None;
            page.accessible = None;
        }
        match action {
            Action::Select(key) => {
                if let Kind::Header(index) = observed.kind {
                    self.workspace.open(key);
                    self.workspace_activate_tab(index, window, cx);
                }
            }
            Action::Close(key) => self.workspace_close_tab(&key, window, cx),
            Action::Hide => {
                if let Kind::Header(index) = observed.kind {
                    self.workspace_hide(index, window, cx);
                }
            }
            Action::Move => {
                if let Kind::Header(index) = observed.kind {
                    self.workspace_move_selected(index, window, cx);
                }
            }
            Action::Maximize => {
                if let Kind::Header(index) = observed.kind {
                    self.image_preview.previous_maximized = None;
                    self.nsh_subagent_clear_maximized(index);
                    self.workspace.maximized[index] = !self.workspace.maximized[index];
                    self.workspace.reveal_tabs[index] = true;
                    self.workspace_focus(index, window, cx);
                }
            }
            Action::Commit => self.workspace_open_commit(cx),
            Action::Menu => {
                if let Kind::Header(index) = observed.kind {
                    self.workspace_toggle_menu(index == 1, window, cx);
                }
            }
            Action::Create(action) => {
                let bottom = self.workspace.menu_bottom;
                self.workspace.menu = false;
                match action {
                    WorkspaceCreateAction::Subagents => self.open_subagent_panel(window, cx),
                    WorkspaceCreateAction::Review => self.workspace_open_diff(cx),
                    WorkspaceCreateAction::NewTerminal => {
                        self.workspace_open_terminal(bottom, window, cx)
                    }
                }
            }
            Action::CloseEnvironment => {
                if let Kind::Environment(overlay) = observed.kind {
                    if overlay {
                        self.environment_overlay_open = false;
                    } else {
                        self.environment_collapsed = true;
                    }
                    self.workspace_focus_composer(window, cx);
                }
            }
            Action::Review => {
                self.environment_overlay_open = false;
                self.workspace_open_diff(cx);
            }
            Action::Terminal => {
                self.environment_overlay_open = false;
                self.workspace_reveal_terminal(window, cx);
            }
            Action::Subagents => self.open_subagent_panel(window, cx),
            action => self.nsh_subagent_action(action, window, cx),
        }
        self.workspace.invalidate_nsh();
        cx.notify();
    }

    fn nsh_workspace_keyboard(
        &mut self,
        kind: Kind,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self.nsh_workspace_current(kind, window, cx) else {
            return;
        };
        let Some(page) = self.workspace.nsh.get(&kind) else {
            return;
        };
        if !page.focus.contains_focused(window, cx) || !current.same(&page.shown) {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if event.prefer_character_input
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
        {
            return;
        }
        let focus = page.focus.clone();
        let actions = current.enabled();
        let key = event.keystroke.key.as_str();
        if key == "escape" && (kind == Kind::Menu || matches!(kind, Kind::Environment(true))) {
            self.workspace.invalidate_nsh();
            self.workspace.menu = false;
            if matches!(kind, Kind::Environment(true)) {
                self.environment_overlay_open = false;
            }
            self.workspace_focus_composer(window, cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if key == "tab" || (kind == Kind::Menu && matches!(key, "up" | "down")) {
            let backwards = modifiers.shift || key == "up";
            let position = page
                .selected
                .as_ref()
                .and_then(|action| actions.iter().position(|candidate| candidate == action));
            let next = match (position, backwards) {
                (None, false) => actions.first(),
                (None, true) => actions.last(),
                (Some(index), false) => actions.get(index + 1),
                (Some(index), true) => index.checked_sub(1).and_then(|index| actions.get(index)),
            }
            .cloned();
            if let Some(next) = next {
                if let Some(page) = self.workspace.nsh.get_mut(&kind) {
                    page.selected = Some(next);
                    page.displayed = None;
                    page.accessible = None;
                }
                window.focus(&focus, cx);
            } else {
                window.focus(&focus, cx);
                for _ in 0..2 {
                    if backwards {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    if !focus.contains_focused(window, cx) {
                        break;
                    }
                }
            }
            cx.stop_propagation();
            cx.notify();
        } else if matches!(key, "enter" | "space" | " ") && !modifiers.shift {
            if !event.is_held
                && let Some(action) = current.selected.clone()
                && self.nsh_workspace_guarded(&current, &action, window, cx)
            {
                self.nsh_workspace_action(&current, action, window, cx);
            }
            cx.stop_propagation();
        }
    }

    fn nsh_workspace_receive(
        &mut self,
        kind: Kind,
        mut requests: tokio::sync::mpsc::Receiver<Request>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Act(observed, action) => {
                            this.nsh_workspace_action(&observed, action, window, cx)
                        }
                        Request::Accessible(observed, action) => {
                            let request = this
                                .nsh_workspace_current(kind, window, cx)
                                .and_then(|current| current.accessible_request(&observed, &action));
                            if let Some(request) = request
                                && let Some(page) = this.workspace.nsh.get_mut(&kind)
                            {
                                page.selected = Some(action);
                                page.displayed = None;
                                page.accessible = Some(request);
                                cx.notify();
                            }
                        }
                        Request::Scrolled(id) => {
                            if let Some(page) = this.workspace.nsh.get_mut(&kind)
                                && page.id.matches(&id)
                            {
                                let mut geometry = page.geometry.borrow_mut();
                                geometry.revision = geometry.revision.wrapping_add(1);
                                page.displayed = None;
                                cx.notify();
                            }
                        }
                        Request::Measured(observed, action, rect, reply) => {
                            let revision =
                                this.nsh_workspace_current(kind, window, cx)
                                    .and_then(|current| {
                                        let page = this.workspace.nsh.get(&kind)?;
                                        current.measured_revision(
                                            &observed,
                                            &action,
                                            page.selected.as_ref(),
                                            &page.geometry.borrow(),
                                            rect,
                                        )
                                    });
                            let _ = reply.send(revision);
                        }
                        Request::Displayed(observed, action, rect, revision, reply) => {
                            let accepted = this
                                .nsh_workspace_current(kind, window, cx)
                                .zip(this.workspace.nsh.get(&kind))
                                .is_some_and(|(current, page)| {
                                    current.measured_revision(
                                        &observed,
                                        &action,
                                        page.selected.as_ref(),
                                        &page.geometry.borrow(),
                                        rect,
                                    ) == Some(revision)
                                });
                            if accepted && let Some(page) = this.workspace.nsh.get_mut(&kind) {
                                let accessible = page.accessible.is_some();
                                page.displayed = Some(((*observed).clone(), action.clone(), rect));
                                if accessible {
                                    this.nsh_workspace_action(&observed, action, window, cx);
                                }
                                cx.notify();
                            }
                            let _ = reply.send(accepted);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    pub(in crate::window) fn workspace_menu_pointer_in_header(
        &self,
        position: Point<Pixels>,
    ) -> bool {
        self.workspace
            .nsh
            .get(&Kind::Header(usize::from(self.workspace.menu_bottom)))
            .and_then(|page| page.geometry.borrow().bounds)
            .is_some_and(|bounds| bounds.contains(&position))
    }

    pub(in crate::window) fn render_nsh_workspace(
        &mut self,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.nsh_kind_visible(kind, window, cx) {
            return div().into_any_element();
        }
        if !self.workspace.nsh.contains_key(&kind) {
            let id = ProjectionId::new();
            let mount = ProjectionId::new();
            let Some(initial) =
                self.nsh_workspace_snapshot(kind, id.clone(), mount.clone(), window, cx)
            else {
                return div().into_any_element();
            };
            let (surface, requests) = Surface::new(initial.clone(), view::render, window, cx);
            let focus = cx.focus_handle().tab_stop(true);
            let subscriptions = vec![
                cx.on_focus_in(&focus, window, move |this: &mut Self, window, cx| {
                    let Some(page) = this.workspace.nsh.get_mut(&kind) else {
                        return;
                    };
                    if page.lease.upgrade().is_none() {
                        return;
                    }
                    let enabled = page.shown.enabled();
                    if page
                        .selected
                        .as_ref()
                        .is_none_or(|action| !enabled.contains(action))
                    {
                        page.selected = if window.modifiers().shift {
                            enabled.last()
                        } else {
                            enabled.first()
                        }
                        .cloned();
                    }
                    if !page.focus.is_focused(window) || !page.shown.focused {
                        page.displayed = None;
                    }
                    let focus = page.focus.clone();
                    if !focus.is_focused(window) {
                        window.focus(&focus, cx);
                    }
                    cx.notify();
                }),
                cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
            ];
            let mut errors = surface.errors();
            cx.spawn(async move |this, cx| {
                while errors.changed().await.is_ok() {
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
            .detach();
            self.workspace.nsh.insert(
                kind,
                Page {
                    surface,
                    recovery: None,
                    id,
                    mount,
                    focus,
                    selected: None,
                    accessible: None,
                    shown: initial,
                    displayed: None,
                    lease: Weak::new(),
                    geometry: Rc::new(RefCell::new(Geometry::default())),
                    _subscriptions: subscriptions,
                },
            );
            self.nsh_workspace_receive(kind, requests, window, cx);
        }
        let Some(page) = self.workspace.nsh.get(&kind) else {
            return div().into_any_element();
        };
        let lease = page.lease.upgrade().unwrap_or_else(|| Rc::new(()));
        let remounted = page.lease.upgrade().is_none();
        let mount = if remounted {
            ProjectionId::new()
        } else {
            page.mount.clone()
        };
        let Some(mut snapshot) =
            self.nsh_workspace_snapshot(kind, page.id.clone(), mount.clone(), window, cx)
        else {
            return div().into_any_element();
        };
        let failed = page.surface.errors().borrow().is_some();
        if failed && page.recovery.is_none() {
            snapshot.recovery = true;
            let (recovery, requests) = Surface::new(snapshot.clone(), view::recovery, window, cx);
            if let Some(page) = self.workspace.nsh.get_mut(&kind) {
                page.recovery = Some(recovery);
                page.selected = Some(Action::Reload);
                page.displayed = None;
            }
            self.nsh_workspace_receive(kind, requests, window, cx);
        }
        let Some(page) = self.workspace.nsh.get_mut(&kind) else {
            return div().into_any_element();
        };
        let focus_requested = (kind == Kind::Menu
            && self
                .workspace
                .menu_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window)))
            || (kind == Kind::Subagents && self.subagent_panel.focus.is_focused(window));
        if focus_requested {
            window.focus(&page.focus, cx);
        }
        snapshot.recovery = page.recovery.is_some();
        snapshot.accessible = page.accessible.clone();
        if remounted || !snapshot.same_content(&page.shown) {
            page.accessible = None;
            snapshot.accessible = None;
        }
        if remounted || !snapshot.same(&page.shown) {
            page.displayed = None;
        }
        if page
            .selected
            .as_ref()
            .is_some_and(|action| !snapshot.enabled().contains(action))
        {
            page.selected = None;
        }
        if let Kind::Header(index) = kind
            && self.workspace.reveal_tabs[index]
        {
            if page.accessible.is_none()
                && let Some(key) = self.workspace.selected[index].clone()
            {
                page.selected = Some(Action::Select(key));
            }
            self.workspace.reveal_tabs[index] = false;
        }
        snapshot.selected = page.selected.clone();
        snapshot.focused = page.focus.is_focused(window);
        page.mount = mount;
        page.lease = Rc::downgrade(&lease);
        page.shown = snapshot.clone();
        let active = page.recovery.as_mut().unwrap_or(&mut page.surface);
        active.sync_environment(cx);
        active.publish(snapshot.clone());
        let _ = active.focus_handle(cx).tab_stop(false);
        let focus = page.focus.clone();
        let geometry = page.geometry.clone();
        let page_id = page.id.clone();
        let weak = cx.weak_entity();
        let mut host = div()
            .id(SharedString::from(format!("nsh-workspace-{kind:?}")))
            .relative()
            .size_full()
            .min_w_0()
            .track_focus(&focus)
            .tab_stop(!snapshot.enabled().is_empty())
            .on_key_down(cx.listener(move |this, event, window, cx| {
                this.nsh_workspace_keyboard(kind, event, window, cx)
            }))
            .child(active.entity())
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let clip = window.content_mask().bounds;
                        let mut geometry = geometry.borrow_mut();
                        let changed =
                            geometry.bounds != Some(bounds) || geometry.clip != Some(clip);
                        if changed {
                            geometry.revision = geometry.revision.wrapping_add(1);
                        }
                        geometry.bounds = Some(bounds);
                        geometry.clip = Some(clip);
                        if changed {
                            let _ = weak.update(cx, |this, cx| {
                                if let Some(page) = this.workspace.nsh.get_mut(&kind)
                                    && page.id.matches(&page_id)
                                {
                                    page.displayed = None;
                                    page.mount = ProjectionId::new();
                                    page.accessible = None;
                                    cx.notify();
                                }
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        if let Some(action) = snapshot.selected.clone()
            && self.nsh_workspace_guarded(&snapshot, &action, window, cx)
            && let Some(control) = snapshot.control(&action)
        {
            let observed = snapshot.clone();
            let weak = cx.weak_entity();
            host = host.child(
                div()
                    .id("workspace-active-control")
                    .absolute()
                    .inset_0()
                    .role(Role::Button)
                    .aria_label(control.label)
                    .aria_active_descendant()
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if this.nsh_workspace_guarded(&observed, &action, window, cx) {
                                this.nsh_workspace_action(&observed, action.clone(), window, cx);
                            }
                        });
                    }),
            );
        }
        Mounted {
            child: host.into_any_element(),
            lease,
            kind,
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<()>,
    kind: Kind,
}
impl IntoElement for Mounted {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Mounted {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some(SharedString::from(format!("workspace-mounted-{:?}", self.kind)).into())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(id) = id {
            window.with_element_state::<Rc<()>, _>(id, |_, _| ((), self.lease.clone()));
        }
        self.child.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}
