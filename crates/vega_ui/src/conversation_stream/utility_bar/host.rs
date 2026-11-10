use super::*;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{
    AccessibleAction, Bounds, Element, ElementId, GlobalElementId, InspectorElementId,
    KeyDownEvent, LayoutId, RenderOnce, Size, Subscription, WeakEntity,
};
use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Background,
    Project,
    Backend,
    Projects,
    Backends,
    Files,
}

impl Kind {
    fn popup(self) -> bool {
        matches!(self, Self::Projects | Self::Backends | Self::Files)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Source {
    thread: Thread,
    selected: Option<String>,
    input: gpui_kit::EntityId,
    revision: u64,
    search: gpui_kit::EntityId,
    query: String,
    projects: Vec<(String, String)>,
    label: String,
    draft: bool,
    menus: [bool; 2],
    state: Vec<bool>,
    files: Vec<String>,
    file_query: String,
    file_highlight: usize,
    file_generation: u64,
    file_state: [bool; 3],
    file_failure: Option<FileIndexFailureCode>,
}

impl Source {
    fn capture(stream: &ConversationStream, cx: &App) -> Self {
        Self {
            thread: stream.thread.clone(),
            selected: cx
                .try_global::<SelectedProject>()
                .and_then(|selected| selected.0.clone()),
            input: stream.input.entity_id(),
            revision: stream.input.read(cx).content_revision(),
            search: stream.utility_project_search.entity_id(),
            query: stream.utility_project_query.clone(),
            projects: stream.utility_projects.clone(),
            label: stream.project_label.clone(),
            draft: stream.draft_route,
            menus: [stream.utility_projects_open, stream.backend_selector_open],
            files: stream.file_selector.candidates().to_vec(),
            file_query: stream.file_selector.query().to_owned(),
            file_highlight: stream.file_selector.highlighted(),
            file_generation: stream.file_index_generation,
            file_state: [
                stream.file_selector.is_open(),
                stream.file_selector_wanted,
                stream.file_index_loading,
            ],
            file_failure: stream.file_index_failure,
            state: vec![
                stream.permissions_revoked,
                stream.permission_change_pending(),
                stream.trusted_action_busy,
                stream.composer_submit_pending,
                stream.composer_resume_pending,
                stream.actions.running,
                stream.actions.is_stopping(),
                stream.has_pending_model_selection(),
                stream.input.read(cx).is_composing(),
                stream.attachment_import_pending,
                stream.context_operation_busy(),
            ],
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Action {
    ToggleProject,
    ToggleBackend,
    Project(String),
    Detach,
    Backend(TaskBackend),
    Reload,
    File(usize),
    RetryFiles,
}

#[derive(Clone)]
pub(super) struct Row {
    pub action: Option<Action>,
    pub label: String,
    pub hint: Option<String>,
    pub icon: Option<crate::icons::Icon>,
    pub checked: Option<bool>,
    pub enabled: bool,
}

#[derive(Default)]
pub(super) struct Draft {
    pub text: String,
    pub composing: bool,
    pub revision: u64,
}

impl Draft {
    fn accepts(&self, revision: u64) -> bool {
        !self.composing && self.revision == revision
    }
}

#[derive(Clone)]
pub(super) struct Snapshot {
    pub id: ProjectionId,
    pub version: ProjectionId,
    epoch: ProjectionId,
    source: Source,
    pub kind: Kind,
    pub rows: Vec<Row>,
    pub selected: Option<usize>,
    pub focused: bool,
    pub failure: bool,
    pub draft: Arc<Mutex<Draft>>,
    pub query_revision: u64,
    pub scroll: Arc<AtomicU64>,
    pub colors: vega_theme::ThemeColors,
    geometry: Option<Geometry>,
}

impl Snapshot {
    fn capture(stream: &ConversationStream, kind: Kind, cx: &App) -> Self {
        let source = Source::capture(stream, cx);
        let mut rows = Vec::new();
        let backend_label = |backend| match backend {
            TaskBackend::Native => "Vega Native",
            TaskBackend::Codex => "Codex ACP",
        };
        match kind {
            Kind::Background => {}
            Kind::Project => rows.push(Row {
                action: Some(Action::ToggleProject),
                label: if stream.thread.is_standalone() {
                    vega_i18n::t("ui.select_project").to_owned()
                } else if stream.project_label.is_empty() {
                    vega_i18n::t("sidebar.projects").to_owned()
                } else {
                    stream.project_label.clone()
                },
                hint: Some(vega_i18n::t("ui.switch_project").to_owned()),
                icon: Some(crate::icons::Icon::Folder),
                checked: None,
                enabled: true,
            }),
            Kind::Backend => rows.push(Row {
                action: Some(Action::ToggleBackend),
                label: backend_label(stream.thread.backend).to_owned(),
                hint: None,
                icon: None,
                checked: None,
                enabled: stream.draft_route && stream.entries.is_empty(),
            }),
            Kind::Projects => {
                let query = stream.utility_project_query.trim().to_lowercase();
                for (id, name) in &stream.utility_projects {
                    if query.is_empty() || name.to_lowercase().contains(&query) {
                        rows.push(Row {
                            action: Some(Action::Project(id.clone())),
                            label: name.clone(),
                            hint: None,
                            icon: Some(crate::icons::Icon::Folder),
                            checked: Some(source.selected.as_deref() == Some(id.as_str())),
                            enabled: true,
                        });
                    }
                }
                if rows.is_empty() {
                    rows.push(Row {
                        action: None,
                        label: vega_i18n::t(if stream.utility_projects.is_empty() {
                            "ui.click_to_add_a_folder"
                        } else {
                            "ui.no_matching_projects"
                        })
                        .to_owned(),
                        hint: None,
                        icon: None,
                        checked: None,
                        enabled: false,
                    });
                }
                rows.push(Row {
                    action: None,
                    label: vega_i18n::t("ui.new_project").to_owned(),
                    hint: Some(
                        vega_i18n::t("ui.not_available_register_a_folder_with_add_project")
                            .to_owned(),
                    ),
                    icon: Some(crate::icons::Icon::Plus),
                    checked: None,
                    enabled: false,
                });
                rows.push(Row {
                    action: Some(Action::Detach),
                    label: vega_i18n::t("ui.no_project").to_owned(),
                    hint: None,
                    icon: Some(crate::icons::Icon::Close),
                    checked: None,
                    enabled: true,
                });
            }
            Kind::Files => {
                if stream.file_index_loading {
                    rows.push(Row {
                        action: None,
                        label: vega_i18n::t("ui.indexing_files").to_owned(),
                        hint: None,
                        icon: None,
                        checked: None,
                        enabled: false,
                    });
                } else if let Some(code) = stream.file_index_failure {
                    rows.push(Row {
                        action: None,
                        label: code.message().to_owned(),
                        hint: None,
                        icon: Some(crate::icons::Icon::Warning),
                        checked: None,
                        enabled: false,
                    });
                    rows.push(Row {
                        action: Some(Action::RetryFiles),
                        label: vega_i18n::t("ui.retry").to_owned(),
                        hint: Some(vega_i18n::t("ui.retry_file_indexing").to_owned()),
                        icon: None,
                        checked: None,
                        enabled: true,
                    });
                } else if !stream.file_selector.is_open() {
                    rows.push(Row {
                        action: None,
                        label: vega_i18n::t("ui.no_matching_files").to_owned(),
                        hint: None,
                        icon: None,
                        checked: None,
                        enabled: false,
                    });
                } else {
                    for (index, path) in stream
                        .file_selector
                        .candidates()
                        .iter()
                        .take(crate::file_selector::FILE_SUGGESTION_LIMIT)
                        .enumerate()
                    {
                        rows.push(Row {
                            action: Some(Action::File(index)),
                            label: path.clone(),
                            hint: None,
                            icon: None,
                            checked: Some(index == stream.file_selector.highlighted()),
                            enabled: true,
                        });
                    }
                }
            }
            Kind::Backends => {
                for backend in [TaskBackend::Native, TaskBackend::Codex] {
                    rows.push(Row {
                        action: Some(Action::Backend(backend)),
                        label: backend_label(backend).to_owned(),
                        hint: None,
                        icon: None,
                        checked: Some(stream.thread.backend == backend),
                        enabled: true,
                    });
                }
            }
        }
        Self {
            id: ProjectionId::new(),
            version: ProjectionId::new(),
            epoch: stream.run_notice_epoch.clone(),
            source,
            kind,
            rows,
            selected: None,
            focused: false,
            failure: false,
            draft: Arc::new(Mutex::new(Draft {
                text: stream.utility_project_query.clone(),
                ..Default::default()
            })),
            query_revision: 0,
            scroll: Arc::new(AtomicU64::new(0)),
            colors: theme(cx).colors,
            geometry: None,
        }
    }

    pub(super) fn source_project_open(&self) -> bool {
        self.source.menus[0]
    }
    pub(super) fn source_backend_open(&self) -> bool {
        self.source.menus[1]
    }

    fn current(&self, stream: &ConversationStream, cx: &App) -> bool {
        self.epoch.matches(&stream.run_notice_epoch)
            && !stream.ui_presentations_suspended
            && !stream.reading_preview
            && (self.kind == Kind::Files || stream.utility_bar_visible(cx))
            && cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|settings| !settings.0)
            && cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| match &route.0 {
                    Some(thread) => thread.id == stream.thread.id,
                    None => stream.draft_route,
                })
            && match self.kind {
                Kind::Projects => stream.utility_projects_open,
                Kind::Backends => stream.backend_selector_open && stream.draft_route,
                Kind::Files => {
                    stream.file_selector.is_open()
                        || stream.file_selector_wanted
                        || stream.file_index_loading
                }
                _ => true,
            }
    }

    fn enabled(&self) -> Vec<usize> {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| (row.enabled && row.action.is_some()).then_some(index))
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    origin: gpui_kit::Point<Pixels>,
    bounds: Bounds<Pixels>,
    viewport: Size<Pixels>,
}

struct Mount {
    geometry: Cell<Option<Geometry>>,
    view: WeakEntity<nsh_platform_gpui::AppView>,
    app: gpui_kit::AsyncApp,
}
impl Drop for Mount {
    fn drop(&mut self) {
        let view = self.view.clone();
        let mut app = self.app.clone();
        self.app
            .foreground_executor()
            .spawn(async move {
                let _ = view.update(&mut app, |view, _| view.stop());
            })
            .detach();
    }
}

pub(super) enum Request {
    Activate(Box<Snapshot>, usize, std::sync::Weak<()>),
    Displayed(
        Box<Snapshot>,
        usize,
        Option<nsh_core::Bounds<nsh_core::Points>>,
        u64,
        std::sync::Weak<()>,
    ),
    Edit(ProjectionId),
}

#[derive(Default)]
struct TriggerRegistry {
    project: Option<WeakEntity<Host>>,
    backend: Option<WeakEntity<Host>>,
}

#[derive(Clone)]
struct Measurement {
    bounds: nsh_core::Bounds<nsh_core::Points>,
    scroll: u64,
    lease: std::sync::Weak<()>,
}

impl Measurement {
    fn current(&self, scroll: u64) -> bool {
        self.scroll == scroll && self.lease.upgrade().is_some()
    }

    fn native_bounds(&self, geometry: Geometry) -> Option<Bounds<Pixels>> {
        let bounds = Bounds {
            origin: gpui_kit::point(
                geometry.origin.x + px(self.bounds.origin.x.0),
                geometry.origin.y + px(self.bounds.origin.y.0),
            ),
            size: gpui_kit::size(px(self.bounds.size.width.0), px(self.bounds.size.height.0)),
        };
        let valid = [
            self.bounds.origin.x.0,
            self.bounds.origin.y.0,
            self.bounds.size.width.0,
            self.bounds.size.height.0,
        ]
        .iter()
        .all(|value| value.is_finite());
        (valid && !bounds.is_empty() && bounds.intersect(&geometry.bounds) == bounds)
            .then_some(bounds)
    }
}

struct Publication {
    snapshot: Snapshot,
    appearance: vega_theme::Appearance,
    language: String,
}

impl Publication {
    fn current(&self, snapshot: &Snapshot, cx: &App) -> bool {
        self.snapshot.id.matches(&snapshot.id)
            && self.snapshot.version.matches(&snapshot.version)
            && self.snapshot.epoch.matches(&snapshot.epoch)
            && self.snapshot.source == snapshot.source
            && self.snapshot.geometry == snapshot.geometry
            && self.snapshot.selected == snapshot.selected
            && self.snapshot.focused == snapshot.focused
            && self.snapshot.failure == snapshot.failure
            && self.snapshot.query_revision == snapshot.query_revision
            && self.appearance == theme(cx).appearance
            && self.language == vega_i18n::language()
    }
}

struct Host {
    owner: WeakEntity<ConversationStream>,
    snapshot: Snapshot,
    surface: Option<Surface<Snapshot, Request>>,
    focus: FocusHandle,
    lease: Weak<Mount>,
    displayed: Vec<Option<Measurement>>,
    published: Option<Publication>,
    _subscriptions: Vec<Subscription>,
    trigger: Option<WeakEntity<Host>>,
}

impl Host {
    fn stop(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.snapshot.id = ProjectionId::new();
        self.snapshot.version = ProjectionId::new();
        self.displayed.clear();
        self.published = None;
        self.lease = Weak::new();
    }

    fn live(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        let Some(geometry) = self.lease.upgrade().and_then(|lease| lease.geometry.get()) else {
            return false;
        };
        self.snapshot.id.matches(&observed.id)
            && self.snapshot.version.matches(&observed.version)
            && geometry
                == observed.geometry.unwrap_or(Geometry {
                    origin: gpui_kit::Point::default(),
                    bounds: Bounds::default(),
                    viewport: window.viewport_size(),
                })
            && geometry.viewport == window.viewport_size()
            && !geometry.bounds.is_empty()
            && window.is_window_active()
            && observed.current(owner.read(cx), cx)
            && observed.source == Source::capture(owner.read(cx), cx)
            && (observed.failure
                || self
                    .surface
                    .as_ref()
                    .is_some_and(|surface| surface.errors().borrow().is_none()))
    }

    fn row_current(
        &self,
        observed: &Snapshot,
        index: usize,
        measured: &Measurement,
        window: &Window,
        cx: &App,
    ) -> bool {
        self.live(observed, window, cx)
            && self.snapshot.selected == observed.selected
            && self.snapshot.focused == observed.focused
            && measured.current(observed.scroll.load(Ordering::Acquire))
            && observed
                .geometry
                .and_then(|geometry| measured.native_bounds(geometry))
                .is_some()
            && self
                .displayed
                .get(index)
                .and_then(|value| value.as_ref())
                .is_some_and(|current| {
                    current.bounds == measured.bounds
                        && current.scroll == measured.scroll
                        && current.lease.ptr_eq(&measured.lease)
                })
            && observed
                .draft
                .lock()
                .is_ok_and(|draft| draft.accepts(observed.query_revision))
    }

    fn focus_row(
        &mut self,
        observed: &Snapshot,
        index: usize,
        measured: &Measurement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.row_current(observed, index, measured, window, cx)
            || !observed
                .rows
                .get(index)
                .is_some_and(|row| row.enabled && row.action.is_some())
            || self.surface.as_ref().is_some_and(|surface| {
                surface.entity().update(cx, |view, cx| {
                    gpui_kit::EntityInputHandler::marked_text_range(view, window, cx).is_some()
                })
            })
        {
            return;
        }
        self.snapshot.selected = Some(index);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn activate(
        &mut self,
        observed: &Snapshot,
        index: usize,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.live(observed, window, cx)
            || !self
                .displayed
                .get(index)
                .and_then(|value| value.as_ref())
                .is_some_and(|measurement| {
                    measurement.current(observed.scroll.load(Ordering::Acquire))
                })
            || (keyboard
                && (!self.focus.is_focused(window) || self.snapshot.selected != Some(index)))
        {
            return;
        }
        if self.surface.as_ref().is_some_and(|surface| {
            surface.entity().update(cx, |view, cx| {
                gpui_kit::EntityInputHandler::marked_text_range(view, window, cx).is_some()
            })
        }) {
            return;
        }
        let Some(row) = observed.rows.get(index).filter(|row| row.enabled) else {
            return;
        };
        let Some(action) = row.action.clone() else {
            return;
        };
        if let Ok(draft) = observed.draft.lock() {
            if !draft.accepts(observed.query_revision) {
                return;
            }
        } else {
            return;
        }
        if action == Action::Reload {
            self.stop(cx);
            self.snapshot.failure = false;
            if let Some(owner) = self.owner.upgrade() {
                self.snapshot.rows = Snapshot::capture(owner.read(cx), self.snapshot.kind, cx).rows;
            }
            cx.notify();
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        self.snapshot.selected = Some(index);
        if !self.snapshot.kind.popup() {
            window.focus(&self.focus, cx);
        }
        owner.update(cx, |stream, cx| {
            if !observed.current(stream, cx) || observed.source != Source::capture(stream, cx) {
                return;
            }
            match action {
                Action::ToggleProject => stream.toggle_utility_projects_menu(cx),
                Action::ToggleBackend => {
                    let open = !stream.backend_selector_open;
                    stream.branch_selector.update(cx, |selector, cx| {
                        let _ = selector.request_close(cx);
                    });
                    stream.close_composer_popovers(cx);
                    stream.backend_selector_open = open;
                    cx.notify();
                }
                Action::Project(id) => stream.select_utility_project(&id, window, cx),
                Action::Detach => stream.detach_utility_project(window, cx),
                Action::Backend(backend) => {
                    stream.request_task_backend(backend, cx);
                    stream.focus_composer(window, cx);
                }
                Action::File(index) => {
                    stream.on_selector_click(index, cx);
                    stream.focus_composer(window, cx);
                }
                Action::RetryFiles => {
                    stream.on_selector_retry_action(&crate::file_selector::RetryFile, window, cx)
                }
                Action::Reload => {}
            }
        });
        self.snapshot.version = ProjectionId::new();
        self.displayed.fill(None);
        cx.notify();
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(owner) = self.owner.upgrade() {
            owner.update(cx, |stream, cx| {
                if self.snapshot.kind == Kind::Files {
                    stream.on_selector_cancel(&crate::file_selector::CancelFile, window, cx);
                } else {
                    stream.utility_projects_open = false;
                    stream.backend_selector_open = false;
                    cx.notify();
                }
            });
        }
        if let Some(trigger) = self.trigger.as_ref().and_then(|trigger| trigger.upgrade()) {
            let focus = trigger.read(cx).focus.clone();
            window.focus(&focus, cx);
        } else if let Some(owner) = self.owner.upgrade() {
            owner.update(cx, |stream, cx| stream.focus_composer(window, cx));
        }
        self.snapshot.version = ProjectionId::new();
        self.displayed.fill(None);
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.live(&self.snapshot, window, cx) {
            return;
        }
        let composing = self
            .snapshot
            .draft
            .lock()
            .map_or(true, |draft| draft.composing);
        let native_composing = self.surface.as_ref().is_some_and(|surface| {
            surface.entity().update(cx, |view, cx| {
                gpui_kit::EntityInputHandler::marked_text_range(view, window, cx).is_some()
            })
        });
        if composing || native_composing || event.prefer_character_input {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return;
        }
        if event.keystroke.key == "escape" && self.snapshot.kind.popup() {
            self.dismiss(window, cx);
            cx.stop_propagation();
            return;
        }
        if self.snapshot.kind == Kind::Files
            && !event.is_held
            && matches!(event.keystroke.key.as_str(), "up" | "down")
        {
            if let Some(owner) = self.owner.upgrade() {
                owner.update(cx, |stream, cx| {
                    if event.keystroke.key == "up" {
                        stream.on_selector_previous(
                            &crate::file_selector::PreviousFile,
                            window,
                            cx,
                        );
                    } else {
                        stream.on_selector_next(&crate::file_selector::NextFile, window, cx);
                    }
                });
            }
            cx.stop_propagation();
            return;
        }
        let enabled = self.snapshot.enabled();
        let input = self.snapshot.kind == Kind::Projects
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.focus_handle(cx).contains_focused(window, cx))
            && !self.focus.is_focused(window);
        if event.keystroke.key == "tab"
            || (!input && matches!(event.keystroke.key.as_str(), "up" | "down"))
        {
            let backwards = modifiers.shift || event.keystroke.key == "up";
            let selected = self
                .snapshot
                .selected
                .and_then(|selected| enabled.iter().position(|index| *index == selected));
            let next = if input {
                if backwards {
                    None
                } else {
                    enabled.first().copied()
                }
            } else {
                match (selected, backwards) {
                    (Some(index), true) => index
                        .checked_sub(1)
                        .and_then(|index| enabled.get(index))
                        .copied(),
                    (Some(index), false) => enabled.get(index + 1).copied(),
                    (None, true) => enabled.last().copied(),
                    (None, false) => enabled.first().copied(),
                }
            };
            if let Some(next) = next {
                self.snapshot.selected = Some(next);
                window.focus(&self.focus, cx);
                self.displayed.fill(None);
                cx.notify();
            } else if self.snapshot.kind == Kind::Projects && backwards && !input {
                if let Some(surface) = &self.surface {
                    surface.focus(window, cx);
                }
                self.snapshot.selected = None;
                cx.notify();
            } else {
                if self.snapshot.kind.popup() {
                    self.dismiss(window, cx);
                    if let Some(trigger) =
                        self.trigger.as_ref().and_then(|trigger| trigger.upgrade())
                    {
                        crate::nsh::keyboard::leave(
                            &trigger.read(cx).focus.clone(),
                            backwards,
                            window,
                            cx,
                        );
                    }
                } else {
                    crate::nsh::keyboard::leave(&self.focus, backwards, window, cx);
                }
            }
            cx.stop_propagation();
        } else if !input
            && !event.is_held
            && !modifiers.shift
            && matches!(event.keystroke.key.as_str(), "enter" | "space" | " ")
        {
            if let Some(index) = self.snapshot.selected {
                self.activate(&self.snapshot.clone(), index, true, window, cx);
            }
            cx.stop_propagation();
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new_transparent(self.snapshot.clone(), view::render, window, cx);
        surface.focus_handle(cx).tab_stop(false);
        let page = self.snapshot.id.clone();
        let mut errors = surface.errors();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Activate(observed, index, lease) => {
                            if this
                                .displayed
                                .get(index)
                                .and_then(|value| value.as_ref())
                                .is_some_and(|measurement| measurement.lease.ptr_eq(&lease))
                            {
                                this.activate(&observed, index, false, window, cx);
                            }
                        }
                        Request::Displayed(observed, index, bounds, scroll, lease) => {
                            if this.live(&observed, window, cx)
                                && this.snapshot.selected == observed.selected
                                && scroll == observed.scroll.load(Ordering::Acquire)
                                && let Some(value) = this.displayed.get_mut(index)
                            {
                                let next =
                                    bounds.filter(|_| lease.upgrade().is_some()).map(|bounds| {
                                        Measurement {
                                            bounds,
                                            scroll,
                                            lease,
                                        }
                                    });
                                let changed = match (value.as_ref(), next.as_ref()) {
                                    (Some(previous), Some(next)) => {
                                        previous.bounds != next.bounds
                                            || previous.scroll != next.scroll
                                            || !previous.lease.ptr_eq(&next.lease)
                                    }
                                    (None, None) => false,
                                    _ => true,
                                };
                                *value = next;
                                if changed {
                                    cx.notify();
                                }
                            }
                        }
                        Request::Edit(id) => {
                            if !this.snapshot.id.matches(&id) || this.lease.upgrade().is_none() {
                                return;
                            }
                            let Some(owner) = this.owner.upgrade() else {
                                return;
                            };
                            let mut source = Source::capture(owner.read(cx), cx);
                            source.query.clone_from(&this.snapshot.source.query);
                            if !this.snapshot.current(owner.read(cx), cx)
                                || this.snapshot.source != source
                                || this.snapshot.source.thread != owner.read(cx).thread
                                || this.snapshot.source.input != owner.read(cx).input.entity_id()
                                || this.snapshot.source.revision
                                    != owner.read(cx).input.read(cx).content_revision()
                                || this
                                    .surface
                                    .as_ref()
                                    .is_none_or(|surface| surface.errors().borrow().is_some())
                            {
                                return;
                            }
                            let Ok(draft) = this.snapshot.draft.lock() else {
                                return;
                            };
                            let text = draft.text.clone();
                            drop(draft);
                            owner.update(cx, |stream, cx| {
                                stream.utility_project_query.clone_from(&text);
                                stream
                                    .utility_project_search
                                    .update(cx, |input, cx| input.set_text(&text, cx));
                                cx.notify();
                            });
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |this, cx| {
                        if this.snapshot.id.matches(&page) {
                            cx.notify();
                        }
                    });
                    break;
                }
            }
        })
        .detach();
        self.surface = Some(surface);
        if self.snapshot.kind == Kind::Backends {
            self.snapshot.selected = self.snapshot.enabled().first().copied();
            window.focus(&self.focus, cx);
        }
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct Files {
    pub owner: WeakEntity<ConversationStream>,
}

impl RenderOnce for Files {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let stream = owner.read(cx);
        if !stream.file_selector.is_open()
            && !stream.file_selector_wanted
            && !stream.file_index_loading
        {
            return div().into_any_element();
        }
        let registry = window.use_keyed_state(
            format!("nsh-file-selector-{}", owner.entity_id()),
            cx,
            |_, _| TriggerRegistry::default(),
        );
        Chrome {
            owner: self.owner,
            kind: Kind::Files,
            registry,
        }
        .into_any_element()
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct Bar {
    pub owner: WeakEntity<ConversationStream>,
}

impl RenderOnce for Bar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let registry = window.use_keyed_state(
            format!("nsh-utility-triggers-{}", owner.entity_id()),
            cx,
            |_, _| TriggerRegistry::default(),
        );
        let stream = owner.read(cx);
        div()
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .child(
                div()
                    .relative()
                    .mx(px(Layout::COMPOSER_UTILITY_BAR_INSET))
                    .h(px(Layout::COMPOSER_UTILITY_BAR_HEIGHT))
                    .child(Chrome {
                        owner: self.owner.clone(),
                        kind: Kind::Background,
                        registry: registry.clone(),
                    })
                    .child(
                        div()
                            .relative()
                            .h_full()
                            .flex()
                            .items_center()
                            .gap(px(Layout::COMPOSER_UTILITY_CHIP_GAP))
                            .pl(px(Layout::COMPOSER_UTILITY_CHIP_INSET))
                            .child(
                                div()
                                    .relative()
                                    .child(Chrome {
                                        owner: self.owner.clone(),
                                        kind: Kind::Project,
                                        registry: registry.clone(),
                                    })
                                    .when(stream.utility_projects_open, |host| {
                                        host.child(Chrome {
                                            owner: self.owner.clone(),
                                            kind: Kind::Projects,
                                            registry: registry.clone(),
                                        })
                                    }),
                            )
                            .when(!stream.thread.is_standalone(), |bar| {
                                bar.child(stream.branch_selector.clone())
                            })
                            .child(
                                div()
                                    .relative()
                                    .child(Chrome {
                                        owner: self.owner.clone(),
                                        kind: Kind::Backend,
                                        registry: registry.clone(),
                                    })
                                    .when(
                                        stream.backend_selector_open && stream.draft_route,
                                        |host| {
                                            host.child(Chrome {
                                                owner: self.owner.clone(),
                                                kind: Kind::Backends,
                                                registry: registry.clone(),
                                            })
                                        },
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[derive(gpui_kit::IntoElement)]
struct Chrome {
    owner: WeakEntity<ConversationStream>,
    kind: Kind,
    registry: Entity<TriggerRegistry>,
}
impl RenderOnce for Chrome {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let snapshot = Snapshot::capture(owner.read(cx), self.kind, cx);
        let key = format!("nsh-utility-{}-{}", owner.entity_id(), self.kind as u8);
        let state = window.use_keyed_state(key, cx, |window, cx| {
            let focus = if self.kind == Kind::Files && snapshot.source.file_failure.is_some() {
                owner.read(cx).file_retry_focus.clone()
            } else {
                cx.focus_handle()
            }
            .tab_stop(!snapshot.enabled().is_empty());
            let subscriptions = vec![
                cx.on_release(|this: &mut Host, cx| this.stop(cx)),
                cx.on_focus_in(&focus, window, |this: &mut Host, window, cx| {
                    if this.focus.contains_focused(window, cx) {
                        if this.snapshot.kind != Kind::Projects {
                            window.focus(&this.focus, cx);
                        }
                        if this.focus.is_focused(window) && this.snapshot.selected.is_none() {
                            this.snapshot.selected = if window.modifiers().shift {
                                this.snapshot.enabled().last().copied()
                            } else {
                                this.snapshot.enabled().first().copied()
                            };
                        }
                        cx.notify();
                    }
                }),
                cx.on_focus_out(&focus, window, |_: &mut Host, _, _, cx| cx.notify()),
            ];
            Host {
                owner: self.owner,
                snapshot: snapshot.clone(),
                surface: None,
                focus,
                lease: Weak::new(),
                displayed: Vec::new(),
                published: None,
                _subscriptions: subscriptions,
                trigger: None,
            }
        });
        let trigger = self.registry.update(cx, |registry, _| match self.kind {
            Kind::Project => {
                registry.project = Some(state.downgrade());
                None
            }
            Kind::Backend => {
                registry.backend = Some(state.downgrade());
                None
            }
            Kind::Projects => registry.project.clone(),
            Kind::Backends => registry.backend.clone(),
            Kind::Background | Kind::Files => None,
        });
        state.update(cx, |host, cx| {
            host.trigger = trigger;
            let changed = host.snapshot.source != snapshot.source
                || !host.snapshot.epoch.matches(&snapshot.epoch);
            if host.lease.upgrade().is_none() {
                host.stop(cx);
                host.snapshot.failure = false;
                host.snapshot.draft = snapshot.draft.clone();
            }
            if changed {
                host.snapshot.version = ProjectionId::new();
                host.displayed.clear();
                host.snapshot.selected = if host.focus.is_focused(window) {
                    if snapshot.kind == Kind::Files && snapshot.source.file_state[0] {
                        Some(snapshot.source.file_highlight)
                    } else {
                        snapshot.enabled().first().copied()
                    }
                } else {
                    None
                };
            }
            host.snapshot.source = snapshot.source;
            host.snapshot.epoch = snapshot.epoch;
            host.snapshot.rows = snapshot.rows;
            host.snapshot.colors = snapshot.colors;
            if let Ok(mut draft) = host.snapshot.draft.lock() {
                if !draft.composing
                    && draft.revision == host.snapshot.query_revision
                    && draft.text != host.snapshot.source.query
                {
                    draft.text = host.snapshot.source.query.clone();
                    draft.revision = draft.revision.wrapping_add(1);
                }
                if !draft.composing && draft.text == host.snapshot.source.query {
                    host.snapshot.query_revision = draft.revision;
                }
            }
        });
        if self.kind == Kind::Background {
            div().absolute().inset_0().child(state).into_any_element()
        } else if self.kind.popup() {
            let width = if self.kind == Kind::Projects {
                project_menu_width(window.viewport_size().width)
            } else if self.kind == Kind::Files {
                px(Layout::MENU_MAX_WIDTH).min((window.viewport_size().width - px(16.)).max(px(1.)))
            } else {
                px(260.).min((window.viewport_size().width - px(16.)).max(px(1.)))
            };
            let rows = state.read(cx).snapshot.rows.len();
            let height = ((rows + usize::from(self.kind == Kind::Projects)) as f32
                * crate::menu_list::MENU_ROW_HEIGHT
                + 8.)
                .min(Layout::COMPOSER_PICKER_MAX_HEIGHT)
                .min((f32::from(window.viewport_size().height) - 16.).max(1.));
            gpui_kit::anchored()
                .anchor(gpui_kit::Anchor::BottomLeft)
                .position_mode(gpui_kit::AnchoredPositionMode::Local)
                .position(gpui_kit::point(
                    px(0.),
                    px(if self.kind == Kind::Files { -8. } else { 0. }),
                ))
                .snap_to_window_with_margin(px(PROJECT_MENU_VIEWPORT_MARGIN))
                .child(
                    gpui_kit::deferred(div().w(width).h(px(height)).occlude().child(state))
                        .with_priority(2),
                )
                .into_any_element()
        } else {
            state.into_any_element()
        }
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self
            .surface
            .as_ref()
            .is_some_and(|surface| surface.errors().borrow().is_some())
        {
            self.stop(cx);
            self.snapshot.failure = true;
        }
        if self.snapshot.failure {
            self.snapshot.rows = vec![Row {
                action: Some(Action::Reload),
                label: vega_i18n::t("ui.reload_view").to_owned(),
                hint: None,
                icon: None,
                checked: None,
                enabled: true,
            }];
        }
        self.snapshot.geometry = self.lease.upgrade().and_then(|lease| lease.geometry.get());
        self.snapshot.focused = self.focus.is_focused(window);
        self.displayed.resize(self.snapshot.rows.len(), None);
        self.mount(window, cx);
        let Some(surface) = &mut self.surface else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        if self
            .published
            .as_ref()
            .is_none_or(|published| !published.current(&self.snapshot, cx))
        {
            surface.publish(self.snapshot.clone());
            self.published = Some(Publication {
                snapshot: self.snapshot.clone(),
                appearance: theme(cx).appearance,
                language: vega_i18n::language().to_owned(),
            });
        }
        let lease = self.lease.upgrade().unwrap_or_else(|| {
            Rc::new(Mount {
                geometry: Cell::new(None),
                view: surface.entity().downgrade(),
                app: cx.to_async(),
            })
        });
        self.lease = Rc::downgrade(&lease);
        let decorative = self.snapshot.kind == Kind::Background && !self.snapshot.failure;
        self.focus
            .clone()
            .tab_stop(!self.snapshot.enabled().is_empty());
        let mut host = div()
            .relative()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::key))
            .child(surface.entity());
        if self.snapshot.kind.popup() || self.snapshot.kind == Kind::Background {
            host = host.size_full();
        } else {
            let label: gpui_kit::SharedString = self
                .snapshot
                .rows
                .first()
                .map(|row| row.label.replace(['\n', '\r'], " "))
                .unwrap_or_default()
                .into();
            let run = window.text_style().to_run(label.len());
            let width = f32::from(
                window
                    .text_system()
                    .shape_line(label, px(Typography::SIDEBAR), &[run], None)
                    .width,
            )
            .min(180.)
                + Layout::COMPOSER_UTILITY_CHIP_PADDING_X * 2.
                + if self.snapshot.kind == Kind::Project {
                    Layout::ICON_SIZE + 8.
                } else {
                    0.
                };
            host = host
                .w(px(width))
                .h(px(Layout::COMPOSER_UTILITY_CHIP_HEIGHT));
        }
        if self.snapshot.kind.popup() {
            host = host
                .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    let trigger_hit = this
                        .trigger
                        .as_ref()
                        .and_then(|trigger| trigger.upgrade())
                        .and_then(|trigger| trigger.read(cx).lease.upgrade())
                        .and_then(|lease| lease.geometry.get())
                        .is_some_and(|geometry| geometry.bounds.contains(&event.position));
                    if !trigger_hit && this.live(&this.snapshot, window, cx) {
                        this.dismiss(window, cx);
                    }
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        }
        for (index, row) in self.snapshot.rows.iter().enumerate() {
            let Some(measured) = self.displayed.get(index).and_then(|value| value.as_ref()) else {
                continue;
            };
            if !self.row_current(&self.snapshot, index, measured, window, cx) {
                continue;
            }
            let Some(geometry) = self.snapshot.geometry else {
                continue;
            };
            let Some(bounds) = measured.native_bounds(geometry) else {
                continue;
            };
            let enabled = row.enabled && row.action.is_some();
            let mut proxy = div()
                .id(format!("utility-row-ax-{index}"))
                .absolute()
                .left(bounds.origin.x - geometry.origin.x)
                .top(bounds.origin.y - geometry.origin.y)
                .w(bounds.size.width)
                .h(bounds.size.height)
                .role(if row.action.is_none() && row.hint.is_none() {
                    gpui_kit::Role::Label
                } else if row.checked.is_some() {
                    gpui_kit::Role::RadioButton
                } else {
                    gpui_kit::Role::Button
                })
                .aria_label(row.label.clone())
                .a11y_synthetic_children(move |tree| {
                    if !enabled {
                        tree.parent_node().set_disabled();
                    }
                });
            if let Some(hint) = &row.hint {
                proxy = proxy.aria_description(hint.clone());
            }
            if let Some(checked) = row.checked {
                proxy = proxy.aria_selected(checked).aria_toggled(if checked {
                    gpui_kit::Toggled::True
                } else {
                    gpui_kit::Toggled::False
                });
            }
            if !self.snapshot.kind.popup() && enabled {
                proxy = proxy.aria_expanded(match self.snapshot.kind {
                    Kind::Project => self.snapshot.source_project_open(),
                    Kind::Backend => self.snapshot.source_backend_open(),
                    _ => false,
                });
            }
            if self.snapshot.focused && self.snapshot.selected == Some(index) {
                proxy = proxy.aria_active_descendant();
            }
            if enabled {
                let focus_owner = cx.weak_entity();
                let focus_observed = self.snapshot.clone();
                let focus_measurement = measured.clone();
                let weak = cx.weak_entity();
                let observed = self.snapshot.clone();
                let measured = measured.clone();
                proxy = proxy
                    .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                        let _ = focus_owner.update(cx, |this, cx| {
                            this.focus_row(&focus_observed, index, &focus_measurement, window, cx);
                        });
                    })
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if this.row_current(&observed, index, &measured, window, cx) {
                                this.activate(&observed, index, false, window, cx);
                            }
                        });
                    });
            }
            host = host.child(proxy);
        }
        Mounted {
            child: host.into_any_element(),
            lease,
            host: cx.weak_entity(),
            decorative,
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<Mount>,
    host: WeakEntity<Host>,
    decorative: bool,
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
        Some("nsh-utility-mount".into())
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
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let previous = self.lease.geometry.replace(None);
        if let Some(id) = id {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.lease.clone()));
            let visible = bounds.intersect(&window.content_mask().bounds);
            if !visible.is_empty()
                && [
                    f32::from(visible.origin.x),
                    f32::from(visible.origin.y),
                    f32::from(visible.size.width),
                    f32::from(visible.size.height),
                ]
                .iter()
                .all(|value| value.is_finite())
            {
                self.lease.geometry.set(Some(Geometry {
                    origin: bounds.origin,
                    bounds: visible,
                    viewport: window.viewport_size(),
                }));
            }
        }
        if previous != self.lease.geometry.get() {
            let _ = self.host.update(cx, |_, cx| cx.notify());
        }
        if self.decorative {
            window.with_content_mask(
                Some(gpui_kit::ContentMask {
                    bounds: Bounds::default(),
                }),
                |window| {
                    self.child.prepaint(window, cx);
                },
            );
        } else {
            self.child.prepaint(window, cx);
        }
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
