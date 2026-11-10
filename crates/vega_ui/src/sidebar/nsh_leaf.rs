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
    Column,
    Row,
    DeleteConfirmation,
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
    pub thread_title: bool,
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
            thread_title: false,
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
    recovery: bool,
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
        id: ProjectionId,
        width: f32,
        height: f32,
    },
}

type Handler = Rc<dyn Fn(usize, Bounds<Pixels>, &mut Window, &mut App)>;
type Guard = Rc<dyn Fn(&App) -> bool>;

struct Leaf {
    snapshot: Snapshot,
    surface: Option<Surface<Snapshot, Request>>,
    focus: FocusHandle,
    selected: Option<Selection<usize>>,
    lease: Weak<chrome::ChromeLease>,
    handler: Handler,
    guard: Guard,
    guard_valid: bool,
    geometry: gpui_kit::ScrollHandle,
    authority: Authority,
    measured_height: Option<f32>,
    acknowledged: Option<(ProjectionId, f32)>,
    mount: ProjectionId,
    motion_visible: bool,
    menu_scroll: Option<gpui_kit::ScrollHandle>,
    _subscriptions: Vec<Subscription>,
}

impl Leaf {
    fn activate(
        &mut self,
        id: &ProjectionId,
        index: usize,
        double: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .surface
            .as_ref()
            .is_some_and(|surface| surface.errors().borrow().is_none())
            || !self
                .acknowledged
                .as_ref()
                .is_some_and(|(projection, width)| {
                    let native_width = f32::from(self.geometry.bounds().size.width);
                    projection.matches(&self.snapshot.id)
                        && native_width.is_finite()
                        && native_width > 0.
                        && width.is_finite()
                        && *width > 0.
                        && (native_width - width).abs() <= 1.
                })
        {
            return;
        }
        if !self.snapshot.accepts(
            id,
            index,
            self.lease.upgrade().is_some(),
            self.authority == Authority::current(cx),
            (self.guard)(cx),
            cx.has_active_drag(),
        ) {
            return;
        }
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
        window.focus(&self.focus, cx);
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
                            Request::Layout { id, width, height }
                                if this.snapshot.id.matches(&id)
                                    && this.lease.upgrade().is_some()
                                    && this.authority == Authority::current(cx)
                                    && (this.guard)(cx)
                                    && this.surface.as_ref().is_some_and(|surface| {
                                        surface.errors().borrow().is_none()
                                    })
                                    && width.is_finite()
                                    && width > 0.
                                    && height.is_finite()
                                    && height > 0. =>
                            {
                                this.acknowledged = Some((id, width));
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
        if !self.focus.is_focused(window) {
            return;
        }
        if let Some(index) = self
            .selected
            .as_ref()
            .and_then(|selected| selected.action(&self.snapshot.id, &self.snapshot.actions()))
        {
            self.activate(&self.snapshot.id.clone(), index, false, window, cx);
        }
    }

    fn key(&mut self, event: &gpui_kit::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
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
                    &self.snapshot.actions(),
                    backwards,
                ) {
                    Step::Selected(selected) => {
                        self.selected = Some(selected);
                        if let Some(index) = self.selected.as_ref().and_then(|selected| {
                            selected.action(&self.snapshot.id, &self.snapshot.actions())
                        }) {
                            self.reveal(index);
                        }
                        window.focus(&self.focus, cx);
                    }
                    Step::Exit => crate::nsh::keyboard::leave(&self.focus, backwards, window, cx),
                }
                cx.stop_propagation();
                cx.notify();
            }
            Key::Activate if self.focus.is_focused(window) => {
                if let Some(index) = self.selected.as_ref().and_then(|selected| {
                    selected.action(&self.snapshot.id, &self.snapshot.actions())
                }) {
                    self.activate(&self.snapshot.id.clone(), index, false, window, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct Host {
    pub key: gpui_kit::SharedString,
    pub text: Option<String>,
    pub items: Vec<Item>,
    presentation: Presentation,
    pub height: f32,
    pub guard: Guard,
    pub handler: Handler,
    pub menu_scroll: Option<gpui_kit::ScrollHandle>,
    pub focus: Option<FocusHandle>,
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
    Host {
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
        focus: None,
    }
    .into_any_element()
}

pub(super) fn focused_actions(
    key: impl Into<gpui_kit::SharedString>,
    items: Vec<Item>,
    focus: Option<FocusHandle>,
    height: f32,
    guard: impl Fn(&App) -> bool + 'static,
    handler: impl Fn(usize, Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    Host {
        key: key.into(),
        text: None,
        items,
        presentation: Presentation::Row,
        height,
        guard: Rc::new(guard),
        handler: Rc::new(handler),
        menu_scroll: None,
        focus,
    }
    .into_any_element()
}

pub(super) fn menu(
    key: impl Into<gpui_kit::SharedString>,
    text: Option<String>,
    items: Vec<Item>,
    scroll: gpui_kit::ScrollHandle,
    guard: impl Fn(&App) -> bool + 'static,
    handler: impl Fn(usize, Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let height = (items.len() as f32 + if text.is_some() { 2. } else { 0. })
        * Typography::SIDEBAR_LINE_HEIGHT;
    Host {
        key: key.into(),
        text,
        items,
        presentation: Presentation::Column,
        height,
        guard: Rc::new(guard),
        handler: Rc::new(handler),
        menu_scroll: Some(scroll),
        focus: None,
    }
    .into_any_element()
}

fn leaf(host: Host, window: &mut Window, cx: &mut App) -> AnyElement {
    let Host {
        key,
        text,
        items,
        presentation,
        height,
        guard,
        handler,
        menu_scroll,
        focus,
    } = host;
    let focusable = items.iter().any(|item| item.enabled);
    let state = window.use_keyed_state(key.clone(), cx, |window, cx| {
        let focus = focus
            .unwrap_or_else(|| cx.focus_handle())
            .tab_stop(focusable);
        let subscriptions = vec![
            cx.on_focus_in(&focus, window, |this: &mut Leaf, window, cx| {
                if this.focus.contains_focused(window, cx) {
                    if let Step::Selected(selected) = Selection::advance(
                        None,
                        &this.snapshot.id,
                        &this.snapshot.actions(),
                        this.snapshot.presentation != Presentation::DeleteConfirmation
                            && window.modifiers().shift,
                    ) {
                        this.selected = Some(selected);
                    }
                    window.focus(&this.focus, cx);
                }
                cx.notify();
            }),
            cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
            cx.observe_window_activation(window, |_, _, cx| cx.notify()),
            cx.observe_window_visibility(window, |_, _, _, cx| cx.notify()),
        ];
        Leaf {
            snapshot: Snapshot {
                id: ProjectionId::new(),
                language: vega_i18n::language(),
                text: None,
                items: Vec::new(),
                colors: theme(cx).colors,
                selected: None,
                recovery: false,
                presentation,
            },
            surface: None,
            focus,
            selected: None,
            lease: Weak::new(),
            handler: Rc::new(|_, _, _, _| {}),
            guard: Rc::new(|_| false),
            guard_valid: false,
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
    state.update(cx, |this, cx| {
        let entering = this.lease.upgrade().is_none();
        if entering {
            if let Some(surface) = this.surface.take() {
                surface.entity().read(cx).stop();
            }
            this.snapshot.id = ProjectionId::new();
            this.acknowledged = None;
            this.selected = None;
            this.motion_visible = false;
        }
        if this
            .surface
            .as_ref()
            .is_some_and(|surface| surface.errors().borrow().is_some())
        {
            if let Some(surface) = this.surface.take() {
                surface.entity().read(cx).stop();
            }
            this.lease = Weak::new();
            this.snapshot.id = ProjectionId::new();
            this.snapshot.recovery = true;
            this.acknowledged = None;
            this.selected = None;
        }
        let authority = Authority::current(cx);
        let previous_guard = (this.guard)(cx);
        let next_guard = guard(cx);
        let changed = (this.guard_valid && !previous_guard)
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
                    || a.thread_title != b.thread_title
                    || a.hint != b.hint
                    || a.wrap != b.wrap
                    || a.label_style != b.label_style
                    || a.tint != b.tint
            });
        if changed {
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
            &this.snapshot.actions(),
        );
        this.snapshot.selected = this
            .focus
            .is_focused(window)
            .then(|| {
                this.selected.as_ref().and_then(|selected| {
                    selected.action(&this.snapshot.id, &this.snapshot.actions())
                })
            })
            .flatten();
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
            .child(surface.entity());
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
            host = host.child(
                div()
                    .id("nsh-sidebar-active")
                    .absolute()
                    .inset_0()
                    .role(Role::Button)
                    .aria_label(label)
                    .aria_active_descendant()
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if this.focus.is_focused(window)
                                && this.selected.as_ref().and_then(|selected| {
                                    selected.action(&id, &this.snapshot.actions())
                                }) == Some(index)
                            {
                                this.activate(&id, index, false, window, cx);
                            }
                        });
                    }),
            );
        }
        let weak = cx.weak_entity();
        let mount = this.mount.clone();
        Mounted {
            id: key,
            child: geometry::sample(host.into_any_element(), move |bounds, window, cx| {
                let visible = !bounds.intersect(&window.content_mask().bounds).is_empty()
                    && window.is_visible()
                    && window.is_window_active()
                    && !cx.reduce_motion();
                let _ = weak.update(cx, |this, _cx| {
                    if this.mount.matches(&mount)
                        && this.lease.upgrade().is_some()
                        && this.motion_visible != visible
                    {
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
            }),
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
            let id = snapshot.id.clone();
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
                            id: id.clone(),
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
            }
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
                let selected = item.selected || snapshot.selected == Some(index);
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
                    .border_color(crate::nsh::color(if snapshot.selected == Some(index) {
                        colors.accent
                    } else {
                        colors.bg_base.opacity(0.)
                    }))
                    .bg(move || {
                        crate::nsh::color(if selected {
                            colors.bg_active
                        } else if hover.get() && enabled {
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
                if item.thread_title {
                    // The GPUI row owns the leading gutter for drag and context-menu events.
                    button = button.border(ns::px(0.)).pl(ns::px(0.)).pr(ns::px(8.));
                    if snapshot.selected == Some(index) {
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
                            .text_color(crate::nsh::color(colors.text_secondary)),
                    );
                }
                if item.compact {
                    button = button.px(ns::px(0.)).justify_center();
                }
                if snapshot.presentation == Presentation::Row {
                    button = button.flex_1();
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
                    } else if let Some(resting) = item.resting {
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
