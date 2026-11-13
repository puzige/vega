use super::*;
use crate::icons::{Icon, nsh_source};
use crate::nsh::keyboard::{Key, Selection, Step};
use crate::nsh::{OrderedCommands, ProjectionId, Surface};
use gpui_kit::{
    AccessibleAction, Bounds, Element, GlobalElementId, InspectorElementId, LayoutId, Pixels, Role,
};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use std::rc::{Rc, Weak};
use tokio::sync::mpsc;

pub(super) mod chrome;
pub(super) use confirmation::delete_confirmation;
mod confirmation;
mod geometry;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Presentation {
    ThreadRow,
    Column,
    TaskMenu,
    Row,
    DeleteConfirmation,
}

const TASK_MENU_GAP: f32 = 4.;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct ThreadTitle {
    pub inset: f32,
    pub highlighted: bool,
}

fn action_slot_width(count: usize) -> f32 {
    (Layout::SIDEBAR_ACTIONS_WIDTH - Layout::SIDEBAR_ACTION_GAP * (count - 1) as f32) / count as f32
}

fn trailing_inset(presentation: Presentation) -> f32 {
    if presentation == Presentation::ThreadRow {
        Layout::SIDEBAR_ACTIONS_INSET
    } else {
        Layout::SIDEBAR_ACTIONS_INSET + Layout::SIDEBAR_ACTIONS_WIDTH
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct Item {
    pub label: String,
    pub icon: Option<Icon>,
    pub enabled: bool,
    pub danger: bool,
    pub selected: bool,
    pub compact: bool,
    pub double_action: Option<usize>,
    pub quiet: bool,
    pub resting: Option<String>,
    pub running: bool,
    motion: bool,
    pub emphasis: bool,
    pub thread_title: Option<ThreadTitle>,
    pub hint: Option<String>,
    pub wrap: bool,
    label_style: Option<LabelStyle>,
    pub tint: Option<gpui_kit::Rgba>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LabelStyle {
    Heading,
    Detail,
}

impl Item {
    pub(super) fn label(text: impl Into<String>, heading: bool) -> Self {
        let mut item = Self::action(text, None);
        item.enabled = false;
        item.label_style = Some(if heading {
            LabelStyle::Heading
        } else {
            LabelStyle::Detail
        });
        item
    }

    pub(super) fn action(label: impl Into<String>, icon: Option<Icon>) -> Self {
        Self {
            label: label.into(),
            icon,
            enabled: true,
            danger: false,
            selected: false,
            compact: false,
            double_action: None,
            quiet: false,
            resting: None,
            running: false,
            motion: false,
            emphasis: false,
            thread_title: None,
            hint: None,
            wrap: false,
            label_style: None,
            tint: None,
        }
    }

    pub(super) fn with_hint(mut self, hint: &str) -> Self {
        self.hint = Some(hint.to_owned());
        self
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Authority {
    route: (Option<Thread>, Option<String>, bool, bool),
    mutation: crate::navigation::TaskMutationState,
    draft: Option<(crate::navigation::DraftNavigationGuard, String, bool)>,
    database: Option<std::path::PathBuf>,
}

impl Authority {
    fn current(cx: &App) -> Self {
        Self {
            route: (
                cx.try_global::<OpenedThread>()
                    .and_then(|value| value.0.clone()),
                cx.try_global::<SelectedProject>()
                    .and_then(|value| value.0.clone()),
                cx.try_global::<SettingsOpen>().is_some_and(|value| value.0),
                cx.try_global::<SidebarCollapsed>()
                    .is_some_and(|value| value.0),
            ),
            mutation: cx
                .try_global::<crate::navigation::TaskMutationState>()
                .copied()
                .unwrap_or_default(),
            draft: cx
                .try_global::<crate::navigation::DraftNavigationGuard>()
                .cloned()
                .map(|guard| {
                    let text = guard
                        .input
                        .upgrade()
                        .map(|input| input.read(cx).text().to_string())
                        .unwrap_or_default();
                    let composing = guard
                        .input
                        .upgrade()
                        .is_some_and(|input| input.read(cx).is_composing());
                    (guard, text, composing)
                }),
            database: with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                .ok()
                .flatten(),
        }
    }
}

#[derive(Clone, PartialEq)]
struct Snapshot {
    id: ProjectionId,
    language: &'static str,
    text: Option<String>,
    items: Vec<Item>,
    colors: ThemeColors,
    selected: Option<usize>,
    keyboard_focus_visible: bool,
    recovery: bool,
    // Keep each native button actionable while its offscreen text awaits first paint.
    row_was_visible: bool,
    presentation: Presentation,
}

impl Snapshot {
    fn accepts(
        &self,
        id: &ProjectionId,
        index: usize,
        mounted: bool,
        authority: bool,
        controller: bool,
        dragging: bool,
    ) -> bool {
        mounted
            && self.id.matches(id)
            && authority
            && controller
            && !dragging
            && self.actions().contains(&index)
    }

    fn actions(&self) -> Vec<usize> {
        if self.recovery {
            return vec![0];
        }
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.enabled.then_some(index))
            .collect()
    }
}

enum Request {
    Activate {
        id: ProjectionId,
        index: usize,
        double: bool,
    },
    Layout {
        frame: Box<Snapshot>,
        width: f32,
        height: f32,
    },
}

type Handler = Rc<dyn Fn(usize, Bounds<Pixels>, &mut Window, &mut App)>;
type Guard = Rc<dyn Fn(&App) -> bool>;

struct Leaf {
    on_screen: bool,
    waiting_for_row_layout: bool,
    painted_at: Option<(gpui_kit::Size<gpui_kit::Pixels>, f32, f32)>,
    parked: Weak<chrome::ChromeLease>,
    snapshot: Snapshot,
    surface: Option<Surface<Snapshot, Request>>,
    focus: FocusHandle,
    actions_focus: Option<FocusHandle>,
    selected: Option<Selection<usize>>,
    pending_menu_key: Option<Selection<usize>>,
    lease: Weak<chrome::ChromeLease>,
    handler: Handler,
    guard: Guard,
    guard_valid: bool,
    action_guard_valid: bool,
    action_guard: Option<Guard>,
    geometry: gpui_kit::ScrollHandle,
    authority: Authority,
    measured_height: Option<f32>,
    acknowledged: Option<(Box<Snapshot>, f32)>,
    mount: ProjectionId,
    motion_visible: bool,
    menu_scroll: Option<gpui_kit::ScrollHandle>,
    _subscriptions: Vec<Subscription>,
}

impl Leaf {
    fn focus_for(&self, index: usize) -> &FocusHandle {
        if index > 0
            && let Some(focus) = &self.actions_focus
        {
            return focus;
        }
        &self.focus
    }

    fn focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
            || self
                .actions_focus
                .as_ref()
                .is_some_and(|f| f.is_focused(window))
    }

    fn scope_actions(&self, window: &Window) -> Vec<usize> {
        let mut actions = self.snapshot.actions();
        if let Some(secondary) = &self.actions_focus {
            actions.retain(|i| (*i > 0) == secondary.is_focused(window));
        }
        actions
    }

    fn action_is_current(&self, id: &ProjectionId, index: usize, cx: &App) -> bool {
        self.surface
            .as_ref()
            .is_some_and(|s| s.errors().borrow().is_none())
            && self.snapshot.accepts(
                id,
                index,
                self.lease.upgrade().is_some(),
                self.authority == Authority::current(cx),
                (self.guard)(cx)
                    && (index == 0 || self.action_guard.as_ref().is_none_or(|g| g(cx))),
                cx.has_active_drag(),
            )
    }

    fn activate(
        &mut self,
        id: &ProjectionId,
        index: usize,
        double: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.action_is_current(id, index, cx)
            || !self
                .acknowledged
                .as_ref()
                .is_some_and(|(projection, width)| {
                    let native_width = f32::from(self.geometry.bounds().size.width);
                    projection.id.matches(&self.snapshot.id)
                        && native_width.is_finite()
                        && native_width > 0.
                        && width.is_finite()
                        && *width > 0.
                        && (native_width - width).abs() <= 1.
                })
        {
            return;
        }
        self.pending_menu_key = None;
        if self.snapshot.recovery {
            if let Some(surface) = self.surface.take() {
                surface.entity().read(cx).stop();
            }
            self.lease = Weak::new();
            self.snapshot.id = ProjectionId::new();
            self.snapshot.recovery = false;
            self.acknowledged = None;
            self.selected = None;
            cx.notify();
            return;
        }
        window.focus(self.focus_for(index), cx);
        self.selected = Selection::reconcile(None, &self.snapshot.id, &[index]);
        let action = if double {
            self.snapshot.items[index].double_action.unwrap_or(index)
        } else {
            index
        };
        (self.handler)(action, self.geometry.bounds(), window, cx);
        cx.notify();
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new_transparent(self.snapshot.clone(), render, window, cx);
        let mut errors = surface.errors();
        self.mount = ProjectionId::new();
        let mount = self.mount.clone();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        if !this.mount.matches(&mount) {
                            return;
                        }
                        match request {
                            Request::Activate { id, index, double } => {
                                this.activate(&id, index, double, window, cx)
                            }
                            Request::Layout {
                                frame,
                                width,
                                height,
                            } if this.snapshot.id.matches(&frame.id)
                                && this.lease.upgrade().is_some()
                                && this.authority == Authority::current(cx)
                                && (this.guard)(cx)
                                && this
                                    .surface
                                    .as_ref()
                                    .is_some_and(|surface| surface.errors().borrow().is_none())
                                && width.is_finite()
                                && width > 0.
                                && height.is_finite()
                                && height > 0. =>
                            {
                                let id = frame.id.clone();
                                if this.waiting_for_row_layout {
                                    if *frame != this.snapshot {
                                        return;
                                    }
                                    if (f32::from(this.geometry.bounds().size.width) - width).abs()
                                        > 1.
                                    {
                                        this.waiting_for_row_layout = false;
                                        if let Some(surface) = this.surface.take() {
                                            surface.entity().read(cx).stop();
                                        }
                                        this.lease = Weak::new();
                                        this.mount = ProjectionId::new();
                                        this.acknowledged = None;
                                        window.refresh();
                                        return;
                                    }
                                }
                                this.acknowledged = Some((frame, width));
                                if std::mem::take(&mut this.waiting_for_row_layout) {
                                    window.refresh();
                                }
                                if let Some(key) = this.pending_menu_key.take() {
                                    let actions = this.snapshot.actions();
                                    if let Some(index) = key.action(&id, &actions)
                                        && this
                                            .selected
                                            .as_ref()
                                            .and_then(|s| s.action(&id, &actions))
                                            == Some(index)
                                        && this.focus.is_focused(window)
                                        && window.is_window_active()
                                        && window.is_visible()
                                    {
                                        this.activate(&id, index, false, window, cx);
                                    }
                                }
                                if this
                                    .measured_height
                                    .is_none_or(|previous| (previous - height).abs() > 0.5)
                                {
                                    this.measured_height = Some(height);
                                    cx.notify();
                                }
                            }
                            Request::Layout { .. } => {}
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let mount = self.mount.clone();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |this, cx| {
                        if this.mount.matches(&mount) {
                            this.acknowledged = None;
                            cx.notify();
                        }
                    });
                    break;
                }
            }
        })
        .detach();
        self.surface = Some(surface);
    }

    fn reveal(&self, index: usize) {
        let Some(scroll) = &self.menu_scroll else {
            return;
        };
        let offset = scroll.offset();
        let top = px(
            (index as f32 + if self.snapshot.text.is_some() { 2. } else { 0. })
                * Typography::SIDEBAR_LINE_HEIGHT,
        );
        let top = if self.snapshot.presentation == Presentation::TaskMenu {
            top + px(index as f32 * TASK_MENU_GAP + 5.)
        } else {
            top
        };
        let bottom = top + px(Typography::SIDEBAR_LINE_HEIGHT);
        let viewport = scroll.bounds().size.height;
        let y = if top + offset.y < px(0.) {
            -top
        } else if bottom + offset.y > viewport {
            viewport - bottom
        } else {
            offset.y
        };
        scroll.set_offset(point(offset.x, y));
    }

    fn step_menu(&mut self, backwards: bool, cx: &mut Context<Self>) {
        self.pending_menu_key = None;
        let actions = self.snapshot.actions();
        if actions.is_empty() {
            return;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|selected| selected.action(&self.snapshot.id, &actions))
            .and_then(|index| actions.iter().position(|action| *action == index))
            .unwrap_or(0);
        let next = if backwards {
            (current + actions.len() - 1) % actions.len()
        } else {
            (current + 1) % actions.len()
        };
        self.selected = Selection::reconcile(None, &self.snapshot.id, &[actions[next]]);
        self.reveal(actions[next]);
        cx.notify();
    }

    fn activate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focused(window) {
            return;
        }
        let id = self.snapshot.id.clone();
        let Some(index) = self
            .selected
            .as_ref()
            .and_then(|s| s.action(&id, &self.scope_actions(window)))
        else {
            return;
        };
        if self.snapshot.presentation == Presentation::TaskMenu
            && !self.snapshot.recovery
            && self.acknowledged.is_none()
            && window.is_visible()
            && window.is_window_active()
            && self.action_is_current(&id, index, cx)
        {
            self.pending_menu_key = self.selected.clone();
            return;
        }
        self.activate(&id, index, false, window, cx);
    }

    fn key(&mut self, event: &gpui_kit::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_menu_key = None;
        let modifiers = event.keystroke.modifiers;
        if self.menu_scroll.is_some()
            && !event.prefer_character_input
            && !modifiers.control
            && !modifiers.alt
            && !modifiers.platform
            && !modifiers.function
        {
            let actions = self.snapshot.actions();
            let key = event.keystroke.key.as_str();
            let selected = self
                .selected
                .as_ref()
                .and_then(|selected| selected.action(&self.snapshot.id, &actions));
            let current = selected
                .and_then(|index| actions.iter().position(|action| *action == index))
                .unwrap_or(0);
            let next = match key {
                "up" if !actions.is_empty() => Some((current + actions.len() - 1) % actions.len()),
                "down" if !actions.is_empty() => Some((current + 1) % actions.len()),
                "home" if !actions.is_empty() => Some(0),
                "end" if !actions.is_empty() => Some(actions.len() - 1),
                _ => None,
            };
            if let Some(next) = next {
                self.selected = Selection::reconcile(None, &self.snapshot.id, &[actions[next]]);
                self.reveal(actions[next]);
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        match Key::from_event(event) {
            Key::Next(backwards)
                if self.menu_scroll.is_some()
                    || self.snapshot.presentation == Presentation::DeleteConfirmation =>
            {
                self.step_menu(backwards, cx);
                cx.stop_propagation();
            }
            Key::Next(backwards) => {
                match Selection::advance(
                    self.selected.as_ref(),
                    &self.snapshot.id,
                    &self.scope_actions(window),
                    backwards,
                ) {
                    Step::Selected(selected) => {
                        self.selected = Some(selected);
                        if let Some(index) = self.selected.as_ref().and_then(|selected| {
                            selected.action(&self.snapshot.id, &self.snapshot.actions())
                        }) {
                            self.reveal(index);
                        }
                        if let Some(index) = self
                            .selected
                            .as_ref()
                            .and_then(|s| s.action(&self.snapshot.id, &self.snapshot.actions()))
                        {
                            window.focus(self.focus_for(index), cx);
                        }
                    }
                    Step::Exit => {
                        let handle = self
                            .actions_focus
                            .as_ref()
                            .filter(|f| f.is_focused(window))
                            .unwrap_or(&self.focus);
                        if let Some(actions) = &self.actions_focus
                            && ((actions.is_focused(window) && backwards)
                                || (self.focus.is_focused(window) && !backwards))
                            && self
                                .snapshot
                                .actions()
                                .iter()
                                .any(|i| (*i == 0) == backwards)
                        {
                            let next = if backwards { &self.focus } else { actions };
                            window.focus(next, cx);
                            if let Step::Selected(selected) = Selection::advance(
                                None,
                                &self.snapshot.id,
                                &self.scope_actions(window),
                                backwards,
                            ) {
                                self.selected = Some(selected);
                            }
                        } else {
                            crate::nsh::keyboard::leave(handle, backwards, window, cx);
                        }
                    }
                }
                cx.stop_propagation();
                cx.notify();
            }
            Key::Activate if self.focused(window) => {
                self.activate_selected(window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct Host {
    rows: Option<RowMap>,
    pub key: gpui_kit::SharedString,
    pub text: Option<String>,
    pub items: Vec<Item>,
    presentation: Presentation,
    pub height: f32,
    pub guard: Guard,
    pub handler: Handler,
    pub menu_scroll: Option<gpui_kit::ScrollHandle>,
    pub focus: Option<FocusHandle>,
    action_guard: Option<Guard>,
}

impl Host {
    pub(super) fn retain_across_settings(mut self, rows: RowMap) -> Self {
        self.rows = Some(rows);
        self
    }
}

impl gpui_kit::RenderOnce for Host {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        leaf(self, window, cx)
    }
}

pub(super) fn host(
    key: impl Into<gpui_kit::SharedString>,
    text: Option<String>,
    items: Vec<Item>,
    horizontal: bool,
    height: f32,
    guard: impl Fn(&App) -> bool + 'static,
    handler: impl Fn(usize, Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    host_parts(key, text, items, horizontal, height, guard, handler).into_any_element()
}

pub(super) fn host_parts(
    key: impl Into<gpui_kit::SharedString>,
    text: Option<String>,
    items: Vec<Item>,
    horizontal: bool,
    height: f32,
    guard: impl Fn(&App) -> bool + 'static,
    handler: impl Fn(usize, Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> Host {
    Host {
        rows: None,
        key: key.into(),
        text,
        items,
        presentation: if horizontal {
            Presentation::Row
        } else {
            Presentation::Column
        },
        height,
        guard: Rc::new(guard),
        handler: Rc::new(handler),
        menu_scroll: None,
        action_guard: None,
        focus: None,
    }
}

pub(super) fn focused_actions(
    key: impl Into<gpui_kit::SharedString>,
    items: Vec<Item>,
    focus: Option<FocusHandle>,
    height: f32,
    guard: impl Fn(&App) -> bool + 'static,
    handler: impl Fn(usize, Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> Host {
    Host {
        rows: None,
        key: key.into(),
        text: None,
        items,
        presentation: Presentation::Row,
        height,
        guard: Rc::new(guard),
        handler: Rc::new(handler),
        menu_scroll: None,
        action_guard: None,
        focus,
    }
}

pub(super) fn thread_row(mut title: Host, mut actions: Host, rows: RowMap) -> Host {
    let rename = actions.items.len() + 1;
    title.items[0].double_action = Some(rename);
    title.items.append(&mut actions.items);
    let title_handler = title.handler;
    let action_handler = actions.handler;
    title.action_guard = Some(actions.guard);
    title.focus = actions.focus;
    title.presentation = Presentation::ThreadRow;
    title.rows = Some(rows);
    title.handler = Rc::new(move |index, bounds, window, cx| {
        if index == 0 || index == rename {
            title_handler(usize::from(index == rename), bounds, window, cx);
        } else {
            action_handler(index - 1, bounds, window, cx);
        }
    });
    title
}

pub(super) fn menu(
    key: impl Into<gpui_kit::SharedString>,
    text: Option<String>,
    items: Vec<Item>,
    scroll: gpui_kit::ScrollHandle,
    guard: impl Fn(&App) -> bool + 'static,
    handler: impl Fn(usize, Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let task_menu = text.is_none();
    let height = (items.len() as f32 + if text.is_some() { 2. } else { 0. })
        * Typography::SIDEBAR_LINE_HEIGHT;
    let height = if task_menu {
        height + items.len().saturating_sub(1) as f32 * TASK_MENU_GAP
    } else {
        height
    };
    Host {
        rows: None,
        key: key.into(),
        text,
        items,
        presentation: if task_menu {
            Presentation::TaskMenu
        } else {
            Presentation::Column
        },
        height,
        guard: Rc::new(guard),
        handler: Rc::new(handler),
        menu_scroll: Some(scroll),
        action_guard: None,
        focus: None,
    }
    .into_any_element()
}

#[derive(Clone, Default)]
pub(super) struct RowMap(
    Rc<std::cell::RefCell<HashMap<gpui_kit::SharedString, gpui_kit::WeakEntity<Leaf>>>>,
);

pub struct SettingsRows {
    _rows: Vec<(Entity<Leaf>, Rc<chrome::ChromeLease>)>,
}

impl RowMap {
    pub(super) fn park(&self, window: &Window, cx: &mut App) -> SettingsRows {
        let mut held = Vec::new();
        self.0.borrow_mut().retain(|_, v| v.upgrade().is_some());
        for state in self.0.borrow().values().filter_map(|weak| weak.upgrade()) {
            let lease = state.update(cx, |this, _| {
                let lease = this.lease.upgrade()?;
                let confirmed = this.acknowledged.as_ref().is_some_and(|(frame, width)| {
                    **frame == this.snapshot
                        && (f32::from(this.geometry.bounds().size.width) - width).abs() <= 1.
                });
                if this.snapshot.items.iter().any(|v| v.running)
                    || !this.on_screen
                    || (this.snapshot.presentation == Presentation::ThreadRow
                        && !this.snapshot.row_was_visible)
                    || this.snapshot.recovery
                    || this.pending_menu_key.is_some()
                    || this
                        .geometry
                        .bounds()
                        .intersect(&Bounds::new(Default::default(), window.viewport_size()))
                        .is_empty()
                {
                    return None;
                }
                this.waiting_for_row_layout = !confirmed || this.snapshot.selected.is_some();
                this.parked = Rc::downgrade(&lease);
                this.lease = Weak::new();
                this.pending_menu_key = None;
                this.snapshot.id = ProjectionId::new();
                this.acknowledged = None;
                Some(lease)
            });
            if let Some(lease) = lease {
                held.push((state, lease));
            }
        }
        SettingsRows { _rows: held }
    }
}

fn leaf(host: Host, window: &mut Window, cx: &mut App) -> AnyElement {
    let Host {
        rows,
        key,
        text,
        items,
        presentation,
        height,
        guard,
        handler,
        menu_scroll,
        action_guard,
        focus,
    } = host;
    let focusable = items.iter().any(|item| item.enabled);
    let holder = window.use_keyed_state(key.clone(), cx, |window, cx| {
        if let Some(previous) = rows
            .as_ref()
            .and_then(|rows| rows.0.borrow().get(&key).and_then(|w| w.upgrade()))
        {
            let old = previous.read(cx);
            if let Some(parked) = old.parked.upgrade()
                && old.lease.upgrade().is_none()
                && old.surface.is_some()
                && old.snapshot.language == vega_i18n::language()
                && old.snapshot.colors == theme(cx).colors
                && {
                    let now = Authority::current(cx);
                    old.authority.route.0.as_ref().map(|t| &t.id)
                        == now.route.0.as_ref().map(|t| &t.id)
                        && old.authority.route.1 == now.route.1
                        && old.authority.route.2 == now.route.2
                        && old.authority.route.3 == now.route.3
                        && old.authority.database == now.database
                }
                && old.painted_at
                    == Some((
                        window.viewport_size(),
                        window.scale_factor(),
                        super::width(cx),
                    ))
                && old.actions_focus == focus
                && guard(cx)
                && old
                    .surface
                    .as_ref()
                    .is_some_and(|s| s.errors().borrow().is_none())
            {
                // A warmed engine may have a different enabled state, text or selection.
                // Keep its native pixels and AX tree detached until the new layout arrives.
                let wait = old.waiting_for_row_layout
                    || old.snapshot.text != text
                    || old.snapshot.presentation != presentation
                    || old.snapshot.items.len() != items.len()
                    || old.snapshot.items.iter().zip(&items).any(|(a, b)| {
                        let mut next = b.clone();
                        next.motion = a.motion;
                        *a != next
                    });
                parked.transferred();
                previous.update(cx, |this, _| this.waiting_for_row_layout = wait);
                let subscription = cx.observe(&previous, |_, _, cx| cx.notify());
                return (previous, true, subscription);
            }
        }
        let state = cx.new(|cx| {
            let (focus, actions_focus) = if presentation == Presentation::ThreadRow {
                (None, focus)
            } else {
                (focus, None)
            };
            let focus = focus
                .unwrap_or_else(|| cx.focus_handle())
                .tab_stop(focusable);
            let mut subscriptions = vec![
                cx.on_focus_in(&focus, window, |this: &mut Leaf, window, cx| {
                    if this.focus.contains_focused(window, cx)
                        && !this
                            .actions_focus
                            .as_ref()
                            .is_some_and(|f| f.is_focused(window))
                    {
                        if let Step::Selected(selected) = Selection::advance(
                            None,
                            &this.snapshot.id,
                            &this.scope_actions(window),
                            this.snapshot.presentation != Presentation::DeleteConfirmation
                                && window.modifiers().shift,
                        ) {
                            this.selected = Some(selected);
                        }
                        window.focus(&this.focus, cx);
                    }
                    cx.notify();
                }),
                cx.on_focus_out(&focus, window, |this, _, _, cx| {
                    this.pending_menu_key = None;
                    cx.notify()
                }),
                cx.observe_window_activation(window, |this, _, cx| {
                    this.pending_menu_key = None;
                    cx.notify()
                }),
                cx.observe_window_visibility(window, |this, _, _, cx| {
                    this.pending_menu_key = None;
                    cx.notify()
                }),
            ];
            if let Some(rows) = &rows {
                let registry = Rc::downgrade(&rows.0);
                let registered_key = key.clone();
                let id = cx.entity_id();
                subscriptions.push(cx.on_release(move |_: &mut Leaf, _| {
                    if let Some(registry) = registry.upgrade() {
                        let mut entries = registry.borrow_mut();
                        if entries
                            .get(&registered_key)
                            .is_some_and(|entry| entry.entity_id() == id)
                        {
                            entries.remove(&registered_key);
                        }
                    }
                }));
            }
            if let Some(handle) = &actions_focus {
                subscriptions.push(cx.on_focus_in(
                    handle,
                    window,
                    |this: &mut Leaf, window, cx| {
                        if let Step::Selected(selected) = Selection::advance(
                            None,
                            &this.snapshot.id,
                            &this.scope_actions(window),
                            window.modifiers().shift,
                        ) {
                            this.selected = Some(selected);
                        }
                        cx.notify();
                    },
                ));
            }
            Leaf {
                on_screen: false,
                waiting_for_row_layout: false,
                painted_at: None,
                parked: Weak::new(),
                snapshot: Snapshot {
                    id: ProjectionId::new(),
                    language: vega_i18n::language(),
                    text: None,
                    items: Vec::new(),
                    colors: theme(cx).colors,
                    selected: None,
                    keyboard_focus_visible: false,
                    recovery: false,
                    row_was_visible: false,
                    presentation,
                },
                surface: None,
                actions_focus,
                focus,
                selected: None,
                pending_menu_key: None,
                lease: Weak::new(),
                handler: Rc::new(|_, _, _, _| {}),
                guard: Rc::new(|_| false),
                guard_valid: false,
                action_guard: None,
                action_guard_valid: false,
                geometry: gpui_kit::ScrollHandle::new(),
                authority: Authority::current(cx),
                measured_height: None,
                acknowledged: None,
                mount: ProjectionId::new(),
                motion_visible: false,
                menu_scroll: menu_scroll.clone(),
                _subscriptions: subscriptions,
            }
        });
        if let Some(rows) = &rows {
            let mut rows = rows.0.borrow_mut();
            rows.insert(key.clone(), state.downgrade());
        }
        let subscription = cx.observe(&state, |_, _, cx| cx.notify());
        (state, false, subscription)
    });
    let (state, resumed) = holder.update(cx, |(state, resumed, _), _| {
        (state.clone(), std::mem::take(resumed))
    });
    state.update(cx, |this, cx| {
        let entering = this.lease.upgrade().is_none();
        if entering {
            if !resumed {
                this.waiting_for_row_layout = false;
                if let Some(surface) = this.surface.take() {
                    surface.entity().read(cx).stop();
                }
            }
            this.pending_menu_key = None;
            this.snapshot.id = ProjectionId::new();
            this.acknowledged = None;
            this.selected = None;
            this.motion_visible = false;
            this.snapshot.row_was_visible = resumed;
        }
        if this
            .surface
            .as_ref()
            .is_some_and(|surface| surface.errors().borrow().is_some())
        {
            if let Some(surface) = this.surface.take() {
                surface.entity().read(cx).stop();
            }
            this.waiting_for_row_layout = false;
            this.lease = Weak::new();
            this.pending_menu_key = None;
            this.snapshot.id = ProjectionId::new();
            this.snapshot.recovery = true;
            if this
                .actions_focus
                .as_ref()
                .is_some_and(|f| f.is_focused(window))
            {
                window.focus(&this.focus, cx);
            }
            this.acknowledged = None;
            this.selected = None;
        }
        let authority = Authority::current(cx);
        let previous_guard = (this.guard)(cx);
        let next_guard = guard(cx);
        let actions_ready = action_guard.as_ref().is_none_or(|g| g(cx));
        let actions_expired =
            this.action_guard_valid && this.action_guard.as_ref().is_some_and(|g| !g(cx));
        let changed = actions_expired
            || this.action_guard_valid != actions_ready
            || (this.guard_valid && !previous_guard)
            || this.guard_valid != next_guard
            || this.authority != authority
            || this.snapshot.text != text
            || this.snapshot.presentation != presentation
            || this.snapshot.items.len() != items.len()
            || this.snapshot.items.iter().zip(&items).any(|(a, b)| {
                a.label != b.label
                    || a.icon != b.icon
                    || a.enabled != b.enabled
                    || a.danger != b.danger
                    || a.selected != b.selected
                    || a.compact != b.compact
                    || a.double_action != b.double_action
                    || a.quiet != b.quiet
                    || a.resting != b.resting
                    || a.running != b.running
                    || a.emphasis != b.emphasis
                    || a.thread_title.map(|t| t.inset) != b.thread_title.map(|t| t.inset)
                    || a.hint != b.hint
                    || a.wrap != b.wrap
                    || a.label_style != b.label_style
                    || a.tint != b.tint
            });
        if changed {
            this.pending_menu_key = None;
            this.snapshot.id = ProjectionId::new();
            this.selected = None;
            this.measured_height = None;
            this.acknowledged = None;
        }
        this.authority = authority;
        this.snapshot.text = text;
        this.snapshot.items = items
            .into_iter()
            .map(|mut item| {
                item.motion = this.motion_visible;
                item
            })
            .collect();
        this.snapshot.language = vega_i18n::language();
        this.snapshot.colors = theme(cx).colors;
        this.snapshot.presentation = presentation;
        this.handler = handler;
        this.action_guard = action_guard;
        this.action_guard_valid = actions_ready;
        this.guard = guard;
        this.guard_valid = next_guard;
        this.menu_scroll = menu_scroll;
        if entering
            && (this.menu_scroll.is_some() || presentation == Presentation::DeleteConfirmation)
        {
            window.focus(&this.focus, cx);
        }
        this.focus = this
            .focus
            .clone()
            .tab_stop(!this.snapshot.actions().is_empty());
        this.selected = Selection::reconcile(
            this.selected.as_ref(),
            &this.snapshot.id,
            &this.scope_actions(window),
        );
        this.snapshot.selected = this
            .focused(window)
            .then(|| {
                this.selected.as_ref().and_then(|selected| {
                    selected.action(&this.snapshot.id, &this.snapshot.actions())
                })
            })
            .flatten();
        this.snapshot.keyboard_focus_visible =
            this.snapshot.selected.is_some() && window.last_input_was_keyboard();
        this.mount(window, cx);
        let Some(surface) = &mut this.surface else {
            return div().into_any_element();
        };
        let lease = this
            .lease
            .upgrade()
            .unwrap_or_else(|| Rc::new(chrome::ChromeLease::new(surface, cx)));
        this.lease = Rc::downgrade(&lease);
        surface.sync_environment(cx);
        surface.publish_if_changed(this.snapshot.clone());
        let mut host = div()
            .id(key.clone())
            .relative()
            .w_full()
            .h(px(this.measured_height.unwrap_or(height).max(height)))
            .track_scroll(&this.geometry)
            .track_focus(&this.focus)
            .role(Role::Group)
            .capture_key_down(cx.listener(Leaf::key))
            .when(!this.waiting_for_row_layout, |host| {
                host.child(surface.entity())
            });
        let mut action_area = this
            .actions_focus
            .as_ref()
            .filter(|_| !this.snapshot.recovery)
            .map(|focus| {
                div()
                    .id("nsh-thread-action-area")
                    .role(Role::Group)
                    .absolute()
                    .right(px(Layout::SIDEBAR_ACTIONS_INSET))
                    .top(px((height - Layout::SIDEBAR_ACTION_SIZE) / 2.))
                    .w(px(Layout::SIDEBAR_ACTIONS_WIDTH))
                    .h(px(Layout::SIDEBAR_ACTION_SIZE))
                    .track_focus(
                        &focus
                            .clone()
                            .tab_stop(this.snapshot.actions().iter().any(|i| *i > 0)),
                    )
                    .key_context("ThreadActionTrigger")
                    .on_action(cx.listener(|this, _: &OpenThreadActions, window, cx| {
                        this.activate_selected(window, cx);
                        cx.stop_propagation();
                    }))
            });
        if presentation == Presentation::DeleteConfirmation {
            host = host
                .key_context("NshDeleteConfirm")
                .role(Role::AlertDialog)
                .aria_label(vega_i18n::t("ui.delete_conversation").to_owned())
                .on_action(cx.listener(|this, _: &CloseSettings, window, cx| {
                    if (this.guard)(cx) {
                        PendingDeleteConfirm::cancel(window, cx);
                    }
                    cx.stop_propagation();
                }));
        } else if this.menu_scroll.is_some() {
            host = host
                .key_context("ThreadActionsMenu")
                .on_action(cx.listener(|this, _: &NextThreadAction, _, cx| {
                    this.step_menu(false, cx);
                    cx.stop_propagation();
                }))
                .on_action(cx.listener(|this, _: &PreviousThreadAction, _, cx| {
                    this.step_menu(true, cx);
                    cx.stop_propagation();
                }))
                .on_action(cx.listener(|this, _: &ActivateThreadAction, window, cx| {
                    this.activate_selected(window, cx);
                    cx.stop_propagation();
                }));
        } else if key.contains("actions-nsh") {
            host = host
                .key_context("ThreadActionTrigger")
                .on_action(cx.listener(|this, _: &OpenThreadActions, window, cx| {
                    this.activate_selected(window, cx);
                    cx.stop_propagation();
                }));
        }
        if let Some(index) = this.snapshot.selected {
            let id = this.snapshot.id.clone();
            let label = if this.snapshot.recovery {
                vega_i18n::t("ui.reload_view").to_string()
            } else {
                this.snapshot.items[index].label.clone()
            };
            let weak = cx.weak_entity();
            let mut active = div().id("nsh-sidebar-active").absolute().inset_0();
            if !this.snapshot.recovery
                && let Some(title) = this
                    .snapshot
                    .items
                    .first()
                    .and_then(|item| item.thread_title)
            {
                if index == 0 {
                    active = active.left(px(title.inset)).right(px(
                        Layout::SIDEBAR_ACTIONS_WIDTH + Layout::SIDEBAR_ACTIONS_INSET
                    ));
                } else {
                    let count = this.snapshot.items.len() - 1;
                    let width = action_slot_width(count);
                    let offset = (width + Layout::SIDEBAR_ACTION_GAP) * (count - index) as f32;
                    active = div()
                        .id("nsh-sidebar-active")
                        .absolute()
                        .right(px(offset))
                        .top_0()
                        .w(px(width))
                        .h(px(Layout::SIDEBAR_ACTION_SIZE));
                }
            }
            let active = active
                .role(Role::Button)
                .aria_label(label)
                .aria_active_descendant()
                .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        if this.focused(window)
                            && this.selected.as_ref().and_then(|selected| {
                                selected.action(&id, &this.scope_actions(window))
                            }) == Some(index)
                        {
                            this.activate(&id, index, false, window, cx);
                        }
                    });
                });
            if index > 0
                && let Some(area) = action_area.take()
            {
                action_area = Some(area.child(active));
            } else {
                host = host.child(active);
            }
        }
        host = host.children(action_area);
        let weak = cx.weak_entity();
        let mount = this.mount.clone();
        let hit_mask = this
            .snapshot
            .items
            .first()
            .and_then(|item| item.thread_title)
            .filter(|_| !this.snapshot.recovery)
            .map_or(geometry::HitMask::Full, |title| {
                geometry::HitMask::Insets(title.inset, trailing_inset(presentation))
            });
        Mounted {
            id: key,
            child: geometry::sample(
                host.into_any_element(),
                hit_mask,
                move |bounds, window, cx| {
                    let on_screen = !bounds.intersect(&window.content_mask().bounds).is_empty();
                    let visible = on_screen
                        && window.is_visible()
                        && window.is_window_active()
                        && !cx.reduce_motion();
                    let _ = weak.update(cx, |this, _cx| {
                        if this.mount.matches(&mount) && this.lease.upgrade().is_some() {
                            this.on_screen = on_screen;
                            this.painted_at = Some((
                                window.viewport_size(),
                                window.scale_factor(),
                                super::width(_cx),
                            ));
                        }
                        let first_paint = this.snapshot.presentation == Presentation::ThreadRow
                            && on_screen
                            && !this.snapshot.row_was_visible;
                        if this.mount.matches(&mount)
                            && this.lease.upgrade().is_some()
                            && (first_paint || this.motion_visible != visible)
                        {
                            this.snapshot.row_was_visible |= first_paint;
                            this.motion_visible = visible;
                            for item in &mut this.snapshot.items {
                                item.motion = visible;
                            }
                            if let Some(surface) = &this.surface
                                && surface.errors().borrow().is_none()
                            {
                                surface.publish_if_changed(this.snapshot.clone());
                            }
                        }
                    });
                },
            ),
            lease,
        }
        .into_any_element()
    })
}

struct Mounted<L = ()> {
    id: gpui_kit::SharedString,
    child: AnyElement,
    lease: Rc<L>,
}
impl<L: 'static> IntoElement for Mounted<L> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl<L: 'static> Element for Mounted<L> {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone().into())
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
            window.with_element_state::<Rc<L>, _>(id, |_, _| ((), self.lease.clone()));
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

fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    let failed = commands.failed;
    ns::view()
        .w_full()
        .h_full()
        .accessible(false)
        .child(ns::dynamic(move || {
            let snapshot = state.get();
            let colors = snapshot.colors;
            let layout = commands.clone();
            let frame = Box::new(snapshot.clone());
            let measured = ns::signal((0.0f32, 0.0f32));
            let mut content = ns::view().w_full().flex_col().on_layout(
                move |bounds: &nsh_core::Bounds<nsh_core::Points>| {
                    let width = bounds.size.width.0;
                    let height = bounds.size.height.0;
                    let previous = measured.get_untracked();
                    if width.is_finite()
                        && width > 0.
                        && height.is_finite()
                        && height > 0.
                        && ((previous.0 - width).abs() > 0.5 || (previous.1 - height).abs() > 0.5)
                    {
                        measured.set((width, height));
                        layout.send(Request::Layout {
                            frame: frame.clone(),
                            width,
                            height,
                        });
                    }
                },
            );
            if snapshot.presentation == Presentation::DeleteConfirmation && !snapshot.recovery {
                return confirmation::render(content, &snapshot, commands.clone());
            }
            if let Some(text) = &snapshot.text {
                content = content.child(
                    ns::text(text.clone())
                        .mb(ns::px(4.))
                        .text_size(ns::px(Typography::SIDEBAR))
                        .text_color(crate::nsh::color(colors.text_primary)),
                );
            }
            let items = if snapshot.recovery {
                vec![Item::action(
                    vega_i18n::t("ui.reload_view"),
                    Some(Icon::Refresh),
                )]
            } else {
                snapshot.items.clone()
            };
            let mut actions = ns::view().w_full().flex_col();
            if snapshot.presentation == Presentation::Row {
                actions = actions.flex_row();
            } else if snapshot.presentation == Presentation::ThreadRow {
                actions = actions.flex_row().items_center();
            } else if snapshot.presentation == Presentation::TaskMenu {
                actions = actions.gap(ns::px(TASK_MENU_GAP));
            }
            if let Some((item, title)) = items
                .first()
                .and_then(|item| item.thread_title.map(|title| (item, title)))
            {
                let bg = if item.selected {
                    colors.bg_active
                } else if title.highlighted {
                    colors.bg_hover
                } else {
                    colors.bg_base.opacity(0.)
                };
                actions = actions
                    .pl(ns::px(title.inset))
                    .pr(ns::px(trailing_inset(snapshot.presentation)))
                    .rounded(ns::px(8.))
                    .bg(crate::nsh::color(bg))
                    .active(|style| style.bg(crate::nsh::color(colors.bg_active)));
            }
            let action_count = items.len().saturating_sub(1);
            for (index, item) in items.into_iter().enumerate() {
                if let Some(style) = item.label_style {
                    let heading = style == LabelStyle::Heading;
                    let mut label = ns::text(item.label.clone())
                        .label(item.label)
                        .w_full()
                        .truncate()
                        .text_size(ns::px(if heading {
                            Typography::SIDEBAR_SECTION
                        } else {
                            Typography::SIDEBAR
                        }))
                        .text_color(crate::nsh::color(colors.text_tertiary));
                    if heading {
                        label = label.font_weight(ns::FontWeight(
                            Typography::SIDEBAR_SECTION_WEIGHT.0 as u16,
                        ));
                    }
                    actions = actions.child(
                        ns::view()
                            .w_full()
                            .h(ns::px(if heading {
                                28.
                            } else {
                                Typography::SIDEBAR_LINE_HEIGHT
                            }))
                            .justify_center()
                            .child(label),
                    );
                    continue;
                }
                let sender = commands.clone();
                let id = snapshot.id.clone();
                let hover = ns::signal(false);
                let enabled = item.enabled;
                let selected = (item.selected && item.thread_title.is_none())
                    || snapshot.selected == Some(index);
                let title_button = item.thread_title.is_some();
                let quiet = item.quiet && snapshot.selected.is_none();
                let mut button = ns::view()
                    .min_h(ns::px(if item.compact {
                        Layout::SIDEBAR_ACTION_SIZE
                    } else {
                        Typography::SIDEBAR_LINE_HEIGHT
                    }))
                    .flex_row()
                    .items_center()
                    .gap(ns::px(8.))
                    .px(ns::px(Layout::SIDEBAR_LABEL_INSET))
                    .rounded(ns::px(6.))
                    .role(nsh_core::AccessibilityRole::Button)
                    .label(item.label.clone())
                    .disabled(!enabled)
                    .accessibility_state(nsh_core::AccessibilityState {
                        disabled: !enabled,
                        ..Default::default()
                    })
                    .border(ns::px(1.))
                    .border_color(crate::nsh::color(
                        if snapshot.keyboard_focus_visible && snapshot.selected == Some(index) {
                            colors.accent
                        } else {
                            colors.bg_base.opacity(0.)
                        },
                    ))
                    .bg(move || {
                        crate::nsh::color(if selected {
                            colors.bg_active
                        } else if !title_button && hover.get() && enabled {
                            colors.bg_hover
                        } else {
                            colors.bg_base.opacity(0.)
                        })
                    })
                    .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
                    .on_mouse_leave(move |_: &nsh_core::MouseEvent| hover.set(false))
                    .on_press(move |event: &nsh_core::PressEvent| {
                        if enabled {
                            sender.send(Request::Activate {
                                id: id.clone(),
                                index,
                                double: event.clicks >= 2,
                            });
                        }
                    });
                if snapshot.presentation == Presentation::TaskMenu {
                    button = button.px(ns::px(7.));
                }
                if item.thread_title.is_some() {
                    // The GPUI row owns the leading gutter for drag and context-menu events.
                    button = button.border(ns::px(0.)).pl(ns::px(0.)).pr(ns::px(8.));
                    if snapshot.selected == Some(index) && snapshot.keyboard_focus_visible {
                        button = button
                            .outline(ns::px(1.))
                            .outline_offset(ns::px(-1.))
                            .outline_color(crate::nsh::color(colors.accent));
                    }
                }
                if let Some(icon) = item.icon.filter(|_| !quiet) {
                    let source = nsh_source(icon).unwrap_or_default();
                    button = button.child(
                        ns::svg(nsh_core::element::ImageSrc::svg_markup(&source))
                            .w(ns::px(Layout::ICON_SIZE))
                            .h(ns::px(Layout::ICON_SIZE))
                            .text_color(crate::nsh::color(
                                if snapshot.presentation == Presentation::ThreadRow && item.compact
                                {
                                    item.tint.unwrap_or(colors.text_secondary)
                                } else {
                                    colors.text_secondary
                                },
                            )),
                    );
                }
                if item.compact {
                    button = button.px(ns::px(0.)).justify_center();
                }
                if snapshot.presentation == Presentation::Row {
                    button = button.flex_1();
                } else if snapshot.presentation == Presentation::ThreadRow {
                    button = if index == 0 {
                        button.flex_1().min_w(ns::px(0.))
                    } else {
                        button
                            .w(ns::px(action_slot_width(action_count)))
                            .h(ns::px(Layout::SIDEBAR_ACTION_SIZE))
                            .ml(ns::px(if index > 1 {
                                Layout::SIDEBAR_ACTION_GAP
                            } else {
                                0.
                            }))
                    };
                }
                if snapshot.presentation == Presentation::ThreadRow
                    && !snapshot.row_was_visible
                    && !snapshot.recovery
                {
                    actions = actions.child(button);
                    continue;
                }
                if quiet {
                    if item.running {
                        let phase = ns::signal(0.0f32);
                        button = button.child(
                            ns::view()
                                .size(ns::px(Layout::ICON_SIZE))
                                .items_center()
                                .justify_center()
                                .child(ns::when(
                                    move || {
                                        item.motion
                                            && ns::lifecycle()
                                                == nsh_core::platform::Lifecycle::Active
                                            && !ns::reduce_motion()
                                    },
                                    move || {
                                        let epoch = std::time::Instant::now();
                                        phase.set(0.);
                                        ns::spawn(async move {
                                            loop {
                                                ns::next_frame().await;
                                                phase.set(
                                                    epoch.elapsed().as_secs_f32() * 360. * 1000.
                                                        / vega_theme::Motion::THINKING_SHIMMER_MS
                                                            as f32,
                                                );
                                            }
                                        });
                                        ns::view()
                                    },
                                ))
                                .child(
                                    ns::svg(nsh_core::element::ImageSrc::svg_markup(
                                        &nsh_source(Icon::Refresh).unwrap_or_default(),
                                    ))
                                    .size(ns::px(Layout::ICON_SIZE))
                                    .rotate(move || {
                                        ns::deg(
                                            if item.motion
                                                && ns::lifecycle()
                                                    == nsh_core::platform::Lifecycle::Active
                                                && !ns::reduce_motion()
                                            {
                                                phase.get()
                                            } else {
                                                0.
                                            },
                                        )
                                    })
                                    .text_color(crate::nsh::color(colors.text_secondary)),
                                ),
                        );
                    } else if let Some(resting) = item.resting.filter(|text| !text.is_empty()) {
                        button = button.child(
                            ns::text(resting)
                                .text_size(ns::px(Typography::METADATA))
                                .text_color(crate::nsh::color(colors.text_secondary)),
                        );
                    }
                } else if !item.compact {
                    let mut label = ns::text(item.label.clone())
                        .text_size(ns::px(Typography::SIDEBAR))
                        .flex_1()
                        .min_w(ns::px(0.));
                    if !item.wrap {
                        label = label.truncate();
                    }
                    if item.emphasis {
                        label = label.font_weight(nsh_core::element::FontWeight(
                            Typography::HEADING_CARD_WEIGHT.0 as u16,
                        ));
                    }
                    button = button.child(label.text_color(crate::nsh::color(if item.danger {
                        colors.danger
                    } else if !enabled {
                        item.tint.unwrap_or(colors.text_tertiary)
                    } else {
                        colors.text_primary
                    })));
                } else {
                    button = button.px(ns::px(0.)).justify_center().tooltip(
                        ns::view()
                            .px_2()
                            .py_1()
                            .bg(crate::nsh::color(colors.bg_elevated))
                            .child(
                                ns::text(item.label)
                                    .text_color(crate::nsh::color(colors.text_primary)),
                            ),
                    );
                }
                if let Some(hint) = item.hint {
                    button = button.hint(hint.clone()).child(
                        ns::text(hint)
                            .text_size(ns::px(Typography::METADATA))
                            .text_color(crate::nsh::color(colors.text_tertiary)),
                    );
                }
                actions = actions.child(button);
            }
            content = content.child(actions);
            content
                .child(ns::dynamic(move || {
                    if failed.get() {
                        ns::text(vega_i18n::t("ui.reload_view"))
                            .mt(ns::px(4.))
                            .text_color(crate::nsh::color(colors.danger))
                            .into_element()
                    } else {
                        ns::view().into_element()
                    }
                }))
                .into_element()
        }))
        .into_element()
}
