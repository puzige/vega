use super::nsh_view::{self, Action, File, FileAction, Mode, Request, Snapshot};
use super::{ArtifactActivate, ArtifactCard, ArtifactClear, ArtifactGroup};
use crate::nsh::keyboard::{Key, Selection, Step};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Bounds, Context, Element, ElementId, FocusHandle, GlobalElementId,
    InspectorElementId, KeyDownEvent, LayoutId, Pixels, Subscription, Window, div, px,
};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use vega_theme::{Layout, theme};

#[derive(Default)]
pub(super) struct Geometry {
    pub(super) bounds: Option<Bounds<Pixels>>,
    pub(super) clip: Option<Bounds<Pixels>>,
    pub(super) revision: u64,
    reveal: Option<Bounds<Pixels>>,
}

impl Geometry {
    fn mounted_visible(&self) -> bool {
        self.bounds.zip(self.clip).is_some_and(|(bounds, clip)| {
            let visible = bounds.intersect(&clip);
            visible.size.width > px(0.) && visible.size.height > px(0.)
        })
    }
    fn control(&self, rect: [f32; 4]) -> Option<Bounds<Pixels>> {
        if !rect.iter().all(|value| value.is_finite()) || rect[2] <= 0. || rect[3] <= 0. {
            return None;
        }
        let root = self.bounds?;
        Some(Bounds {
            origin: gpui_kit::point(root.origin.x + px(rect[0]), root.origin.y + px(rect[1])),
            size: gpui_kit::size(px(rect[2]), px(rect[3])),
        })
    }
    pub(super) fn menu_trigger(&self, mode: Mode, index: usize) -> Option<Bounds<Pixels>> {
        let width = f32::from(self.bounds?.size.width);
        let rect = nsh_view::menu_trigger_rect(mode, index, width);
        self.visible(rect).then(|| self.control(rect)).flatten()
    }
    fn visible(&self, rect: [f32; 4]) -> bool {
        let Some(control) = self.control(rect) else {
            return false;
        };
        let Some(clip) = self
            .clip
            .zip(self.bounds)
            .map(|(clip, root)| clip.intersect(&root))
        else {
            return false;
        };
        let right = control.origin.x + control.size.width;
        let bottom = control.origin.y + control.size.height;
        control.origin.x >= clip.origin.x
            && control.origin.y >= clip.origin.y
            && right <= clip.origin.x + clip.size.width
            && bottom <= clip.origin.y + clip.size.height
    }
}

struct MenuPresentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    index: usize,
    lease: Weak<()>,
    geometry: Rc<RefCell<Geometry>>,
}

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    menu: Option<MenuPresentation>,
    id: ProjectionId,
    mount: ProjectionId,
    lease: Weak<()>,
    geometry: Rc<RefCell<Geometry>>,
    focus: FocusHandle,
    keyboard_session: ProjectionId,
    keyboard: Option<Selection<Action>>,
    shown: Snapshot,
    displayed: Option<(Snapshot, Action, [f32; 4], u64)>,
    selection: ProjectionId,
    _subscriptions: Vec<Subscription>,
}

impl Presentation {
    fn geometry_for(&self, action: Action) -> Rc<RefCell<Geometry>> {
        if self.shown.uses_menu_viewport(action)
            && let Some(menu) = &self.menu
        {
            menu.geometry.clone()
        } else {
            self.geometry.clone()
        }
    }
    fn menu_trigger(&self) -> Option<Bounds<Pixels>> {
        self.lease.upgrade()?;
        let index = self.shown.menu_index()?;
        self.geometry.borrow().menu_trigger(self.shown.mode, index)
    }
    fn menu_visible(&self) -> bool {
        self.menu_trigger().is_some()
    }
}

pub(super) trait Owner: Sized + 'static {
    fn remove(&mut self, mode: Mode) -> Option<Presentation>;
    fn page(&self, mode: Mode) -> Option<&Presentation>;
    fn page_mut(&mut self, mode: Mode) -> Option<&mut Presentation>;
    fn insert(&mut self, mode: Mode, page: Presentation);
    fn snapshot(&self, mode: Mode, id: ProjectionId, mount: ProjectionId, cx: &App) -> Snapshot;
    fn action(&mut self, action: Action, mode: Mode, cx: &mut Context<Self>);
    fn focus(&self, mode: Mode, cx: &mut Context<Self>) -> FocusHandle;
}

fn file(card: &ArtifactCard, capture: u64) -> File {
    File {
        thread: card.thread_id.clone(),
        project: card.project_id.clone(),
        projection: card.projection.clone(),
        revision: card.revision.clone(),
        preview: card.preview_lines.clone(),
        error: card.inline_error,
        opening: card.opening,
        reading: card.reading_preview,
        menu: card.menu_open,
        capture,
    }
}

fn base(
    mode: Mode,
    id: ProjectionId,
    mount: ProjectionId,
    revision: ProjectionId,
    cx: &App,
) -> Snapshot {
    Snapshot {
        id,
        mount,
        revision,
        mode,
        message: String::new(),
        route: (String::new(), String::new()),
        files: Vec::new(),
        visible: 0,
        expanded: false,
        review_reason: None,
        errors: Vec::new(),
        colors: theme(cx).colors,
        environment: (vega_i18n::language(), theme(cx).appearance),
        selected: None,
        selection: ProjectionId::new(),
        focused: false,
        recovery: false,
        menu_surface: None,
    }
}

impl Owner for ArtifactCard {
    fn remove(&mut self, mode: Mode) -> Option<Presentation> {
        self.presentations.remove(&mode)
    }
    fn page(&self, mode: Mode) -> Option<&Presentation> {
        self.presentations.get(&mode)
    }
    fn page_mut(&mut self, mode: Mode) -> Option<&mut Presentation> {
        self.presentations.get_mut(&mode)
    }
    fn insert(&mut self, mode: Mode, page: Presentation) {
        self.presentations.insert(mode, page);
    }
    fn snapshot(&self, mode: Mode, id: ProjectionId, mount: ProjectionId, cx: &App) -> Snapshot {
        let mut snapshot = base(mode, id, mount, self.revision.clone(), cx);
        snapshot.files.push(file(self, 0));
        snapshot.visible = 1;
        snapshot.route = (self.thread_id.clone(), self.project_id.clone());
        snapshot
    }
    fn action(&mut self, action: Action, _: Mode, cx: &mut Context<Self>) {
        match action {
            Action::File(0, FileAction::Preview) => self.preview(cx),
            Action::File(0, FileAction::Open(target)) => self.open(target, cx),
            Action::File(0, FileAction::Clear) => self.clear(cx),
            Action::File(0, FileAction::Menu) => {
                self.menu_open = !self.menu_open;
                self.revision = ProjectionId::new();
                cx.notify();
            }
            Action::CloseMenu if self.menu_open => {
                self.menu_open = false;
                self.revision = ProjectionId::new();
                cx.notify();
            }
            _ => {}
        }
    }
    fn focus(&self, mode: Mode, cx: &mut Context<Self>) -> FocusHandle {
        if mode == Mode::Compact {
            self.focus.clone()
        } else {
            cx.focus_handle().tab_stop(true)
        }
    }
}

impl Owner for ArtifactGroup {
    fn remove(&mut self, _: Mode) -> Option<Presentation> {
        self.presentation.take()
    }
    fn page(&self, _: Mode) -> Option<&Presentation> {
        self.presentation.as_ref()
    }
    fn page_mut(&mut self, _: Mode) -> Option<&mut Presentation> {
        self.presentation.as_mut()
    }
    fn insert(&mut self, _: Mode, page: Presentation) {
        self.presentation = Some(page);
    }
    fn snapshot(&self, mode: Mode, id: ProjectionId, mount: ProjectionId, cx: &App) -> Snapshot {
        let mut snapshot = base(mode, id, mount, self.revision.clone(), cx);
        snapshot.message = self.message_id.clone();
        snapshot.route = (
            self.review_request.thread_id.clone(),
            self.review_request.project_id.clone(),
        );
        snapshot.files = self
            .files
            .iter()
            .map(|owned| file(owned.card.read(cx), owned.capture_order))
            .collect();
        snapshot.visible = self.visible_files();
        snapshot.expanded = self.expanded;
        snapshot.review_reason = self.review_disabled_reason_key;
        snapshot.errors = self
            .capture_errors
            .iter()
            .map(|(_, code)| *code)
            .chain(self.file_errors.iter().map(|(_, code)| *code))
            .collect();
        snapshot
    }
    fn action(&mut self, action: Action, mode: Mode, cx: &mut Context<Self>) {
        match action {
            Action::Review => self.open_review(cx),
            Action::Toggle => self.toggle_expanded(cx),
            Action::File(index, command) => {
                let cards = self
                    .files
                    .iter()
                    .map(|file| file.card.clone())
                    .collect::<Vec<_>>();
                if command == FileAction::Menu {
                    for (other, card) in cards.iter().enumerate() {
                        if other != index {
                            card.update(cx, |card, cx| card.action(Action::CloseMenu, mode, cx));
                        }
                    }
                }
                if let Some(card) = cards.get(index) {
                    card.update(cx, |card, cx| {
                        card.action(Action::File(0, command), mode, cx)
                    });
                }
                self.revision = ProjectionId::new();
                cx.notify();
            }
            Action::CloseMenu => {
                for card in self
                    .files
                    .iter()
                    .map(|file| file.card.clone())
                    .collect::<Vec<_>>()
                {
                    card.update(cx, |card, cx| card.action(Action::CloseMenu, mode, cx));
                }
                self.revision = ProjectionId::new();
                cx.notify();
            }
            _ => {}
        }
    }
    fn focus(&self, _: Mode, cx: &mut Context<Self>) -> FocusHandle {
        cx.focus_handle().tab_stop(true)
    }
}

fn current<T: Owner>(owner: &T, mode: Mode, cx: &App) -> Option<Snapshot> {
    let page = owner.page(mode)?;
    let mut current = owner.snapshot(mode, page.id.clone(), page.mount.clone(), cx);
    current.recovery = page.shown.recovery;
    current.menu_surface = page.menu.as_ref().map(|menu| menu.id.clone());
    current.selected = page
        .keyboard
        .as_ref()
        .and_then(|selected| selected.action(&page.keyboard_session, &current.enabled()));
    current.selection = page.selection.clone();
    current.focused = page.shown.focused;
    Some(current)
}

fn accepts<T: Owner>(owner: &T, mode: Mode, observed: &Snapshot, action: Action, cx: &App) -> bool {
    owner.page(mode).is_some_and(|page| {
        page.lease.upgrade().is_some()
            && page.geometry.borrow().mounted_visible()
            && page.surface.errors().borrow().is_none()
            && page.shown.same(observed)
            && (!observed.uses_menu_viewport(action)
                || page.menu.as_ref().is_some_and(|menu| {
                    menu.lease.upgrade().is_some()
                        && menu.surface.errors().borrow().is_none()
                        && menu.geometry.borrow().mounted_visible()
                        && page.menu_visible()
                }))
    }) && current(owner, mode, cx).is_some_and(|current| current.accepts(observed, action))
}

pub(super) fn measured_activation(
    current: &Snapshot,
    observed: &Snapshot,
    action: Action,
    displayed: Option<&(Snapshot, Action, [f32; 4], u64)>,
    geometry: &Geometry,
    focused: bool,
) -> bool {
    focused
        && current.selection_matches(observed, action)
        && displayed.is_some_and(|(measured, target, bounds, revision)| {
            *target == action
                && observed.selection_matches(measured, action)
                && geometry.revision == *revision
                && geometry.visible(*bounds)
        })
}

fn guarded<T: Owner>(
    owner: &T,
    mode: Mode,
    observed: &Snapshot,
    action: Action,
    window: &Window,
    cx: &App,
) -> bool {
    let Some(page) = owner.page(mode) else {
        return false;
    };
    let Some(current) = current(owner, mode, cx) else {
        return false;
    };
    measured_activation(
        &current,
        observed,
        action,
        page.displayed.as_ref(),
        &page.geometry_for(action).borrow(),
        page.focus.is_focused(window),
    )
}

fn select<T: Owner>(owner: &mut T, mode: Mode, action: Action) {
    if let Some(page) = owner.page_mut(mode) {
        page.keyboard = Selection::reconcile(None, &page.keyboard_session, &[action]);
        page.selection = ProjectionId::new();
        page.displayed = None;
    }
}

fn activate<T: Owner>(
    owner: &mut T,
    mode: Mode,
    observed: &Snapshot,
    action: Action,
    window: &mut Window,
    cx: &mut Context<T>,
) -> bool {
    if !accepts(owner, mode, observed, action, cx) {
        return false;
    }
    let focus = owner.page(mode).map(|page| page.focus.clone());
    if action == Action::Reload {
        if let Some(page) = owner.remove(mode) {
            page.surface.entity().read(cx).stop();
            if let Some(menu) = page.menu {
                menu.surface.entity().read(cx).stop();
            }
        }
        cx.notify();
        return true;
    }
    let selected = if action == Action::CloseMenu {
        observed
            .files
            .iter()
            .position(|file| file.menu)
            .map(|index| Action::File(index, FileAction::Menu))
            .unwrap_or(action)
    } else {
        action
    };
    if action != Action::CloseMenu
        && let Some(focus) = focus
    {
        window.focus(&focus, cx);
    }
    select(owner, mode, selected);
    owner.action(action, mode, cx);
    true
}

fn keyboard<T: Owner>(
    owner: &mut T,
    mode: Mode,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    let Some(page) = owner.page(mode) else {
        return;
    };
    if page.lease.upgrade().is_none() || !page.focus.contains_focused(window, cx) {
        return;
    }
    if event.is_held && matches!(event.keystroke.key.as_str(), "enter" | "space" | " ") {
        cx.stop_propagation();
        return;
    }
    let Some(snapshot) = current(owner, mode, cx) else {
        return;
    };
    if !snapshot.same(&page.shown) {
        return;
    }
    let focus = page.focus.clone();
    if event.keystroke.key == "escape" {
        if snapshot.files.iter().any(|file| file.menu) {
            activate(owner, mode, &snapshot, Action::CloseMenu, window, cx);
            cx.stop_propagation();
        } else if mode == Mode::Row(1) {
            activate(
                owner,
                mode,
                &snapshot,
                Action::File(0, FileAction::Clear),
                window,
                cx,
            );
            cx.stop_propagation();
        }
        return;
    }
    match Key::from_event(event) {
        Key::Next(reverse) => {
            let previous = focus
                .is_focused(window)
                .then_some(page.keyboard.as_ref())
                .flatten();
            let next = Selection::advance(
                previous,
                &page.keyboard_session,
                &snapshot.enabled(),
                reverse,
            );
            match next {
                Step::Selected(next) => {
                    if let Some(page) = owner.page_mut(mode) {
                        page.keyboard = Some(next);
                        page.selection = ProjectionId::new();
                        page.displayed = None;
                    }
                    window.focus(&focus, cx);
                }
                Step::Exit => crate::nsh::keyboard::leave(&focus, reverse, window, cx),
            }
            cx.stop_propagation();
            cx.notify();
        }
        Key::Activate if focus.is_focused(window) => {
            if let Some((observed, action, _, _)) = page.displayed.clone()
                && guarded(owner, mode, &observed, action, window, cx)
            {
                activate(owner, mode, &observed, action, window, cx);
            }
            cx.stop_propagation();
        }
        _ => {}
    }
}

fn request<T: Owner>(
    this: &mut T,
    mode: Mode,
    request: Request,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    match request {
        Request::Act(observed, action) => {
            activate(this, mode, &observed, action, window, cx);
        }
        Request::Accessible(observed, action) => {
            if guarded(this, mode, &observed, action, window, cx) {
                activate(this, mode, &observed, action, window, cx);
            }
        }
        Request::LayoutChanged(observed) => {
            if this
                .page(mode)
                .is_some_and(|page| page.lease.upgrade().is_some() && page.shown.same(&observed))
                && let Some(page) = this.page_mut(mode)
            {
                let mut geometry = page.geometry.borrow_mut();
                geometry.revision = geometry.revision.saturating_add(1);
                page.displayed = None;
                cx.notify();
            }
        }
        Request::Reveal(observed, action, bounds, reply) => {
            let accepted = accepts(this, mode, &observed, action, cx)
                && this
                    .page(mode)
                    .is_some_and(|page| page.focus.is_focused(window))
                && current(this, mode, cx)
                    .is_some_and(|current| current.selection_matches(&observed, action));
            let visible = if accepted && let Some(page) = this.page_mut(mode) {
                let source = page.geometry_for(action);
                let mut geometry = source.borrow_mut();
                if geometry.visible(bounds) {
                    Some(geometry.revision)
                } else {
                    geometry.reveal = geometry.control(bounds);
                    cx.notify();
                    None
                }
            } else {
                None
            };
            let _ = reply.send(visible);
        }
        Request::Displayed(observed, action, bounds, measured_revision) => {
            if accepts(this, mode, &observed, action, cx)
                && current(this, mode, cx)
                    .is_some_and(|current| current.selection_matches(&observed, action))
                && let Some(page) = this.page_mut(mode)
                && page.focus.is_focused(window)
                && page.geometry_for(action).borrow().visible(bounds)
                && page.geometry_for(action).borrow().revision == measured_revision
            {
                let revision = page.geometry_for(action).borrow().revision;
                page.displayed = Some((*observed, action, bounds, revision));
                cx.notify();
            }
        }
    }
}

fn receive<T: Owner>(
    mode: Mode,
    mut requests: tokio::sync::mpsc::Receiver<Request>,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    cx.spawn_in(window, async move |this, cx| {
        while let Some(next) = requests.recv().await {
            if this
                .update_in(cx, |this, window, cx| request(this, mode, next, window, cx))
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

pub(super) fn render<T: Owner>(
    owner: &mut T,
    mode: Mode,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let failed = owner.page(mode).is_some_and(|page| {
        page.surface.errors().borrow().is_some()
            || page
                .menu
                .as_ref()
                .is_some_and(|menu| menu.surface.errors().borrow().is_some())
    });
    if failed && let Some(page) = owner.remove(mode) {
        page.surface.entity().read(cx).stop();
        if let Some(menu) = page.menu {
            menu.surface.entity().read(cx).stop();
        }
    }
    if owner.page(mode).is_none() {
        let id = ProjectionId::new();
        let mount = ProjectionId::new();
        let mut initial = owner.snapshot(mode, id.clone(), mount.clone(), cx);
        initial.recovery = failed;
        let (surface, requests) = Surface::new(initial.clone(), nsh_view::render, window, cx);
        let mut errors = surface.errors();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |_, cx| cx.notify());
                    break;
                }
            }
        })
        .detach();
        let focus = owner.focus(mode, cx);
        let subscriptions = vec![
            cx.on_focus_in(&focus, window, move |this: &mut T, window, cx| {
                let Some(page) = this.page(mode) else {
                    return;
                };
                if page.lease.upgrade().is_none() || !page.focus.contains_focused(window, cx) {
                    return;
                }
                let focus = page.focus.clone();
                let descendant = !focus.is_focused(window);
                let next = if descendant || !page.shown.focused || page.keyboard.is_none() {
                    Selection::advance(
                        None,
                        &page.keyboard_session,
                        &page.shown.enabled(),
                        window.modifiers().shift,
                    )
                } else {
                    return;
                };
                if let Step::Selected(next) = next
                    && let Some(page) = this.page_mut(mode)
                {
                    page.keyboard = Some(next);
                    page.selection = ProjectionId::new();
                    page.displayed = None;
                }
                window.focus(&focus, cx);
                cx.notify();
            }),
            cx.on_focus_out(&focus, window, move |this: &mut T, _, window, cx| {
                let outside = this
                    .page(mode)
                    .is_some_and(|page| !page.focus.contains_focused(window, cx));
                if outside {
                    this.action(Action::CloseMenu, mode, cx);
                    if let Some(page) = this.page_mut(mode) {
                        page.displayed = None;
                    }
                }
                cx.notify();
            }),
        ];
        owner.insert(
            mode,
            Presentation {
                surface,
                menu: None,
                id,
                mount,
                lease: Weak::new(),
                geometry: Rc::new(RefCell::new(Geometry::default())),
                focus,
                keyboard_session: ProjectionId::new(),
                keyboard: None,
                shown: initial,
                displayed: None,
                selection: ProjectionId::new(),
                _subscriptions: subscriptions,
            },
        );
        receive(mode, requests, window, cx);
    }
    let Some(page) = owner.page(mode) else {
        return div().into_any_element();
    };
    let lease = page.lease.upgrade().unwrap_or_else(|| Rc::new(()));
    let remounted = page.lease.upgrade().is_none();
    let id = page.id.clone();
    let mount = if remounted {
        ProjectionId::new()
    } else {
        page.mount.clone()
    };
    let mut snapshot = owner.snapshot(mode, id, mount.clone(), cx);
    snapshot.recovery = page.shown.recovery;
    let desired_menu = snapshot.menu_index();
    let replace_menu = page.menu.as_ref().map(|menu| menu.index) != desired_menu
        || remounted
        || page
            .menu
            .as_ref()
            .is_some_and(|menu| menu.lease.upgrade().is_none());
    if replace_menu
        && let Some(page) = owner.page_mut(mode)
        && let Some(menu) = page.menu.take()
    {
        menu.surface.entity().read(cx).stop();
    }
    if let Some(index) = desired_menu
        && owner.page(mode).is_some_and(|page| page.menu.is_none())
    {
        let menu_id = ProjectionId::new();
        snapshot.menu_surface = Some(menu_id.clone());
        let (surface, requests) = Surface::new(snapshot.clone(), nsh_view::render_menu, window, cx);
        let mut errors = surface.errors();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |_, cx| cx.notify());
                    break;
                }
            }
        })
        .detach();
        if let Some(page) = owner.page_mut(mode) {
            page.menu = Some(MenuPresentation {
                surface,
                id: menu_id,
                index,
                lease: Weak::new(),
                geometry: Rc::new(RefCell::new(Geometry::default())),
            });
        }
        receive(mode, requests, window, cx);
    }
    let Some(page) = owner.page_mut(mode) else {
        return div().into_any_element();
    };
    snapshot.menu_surface = page.menu.as_ref().map(|menu| menu.id.clone());
    if remounted || !snapshot.same(&page.shown) {
        let previous = if remounted {
            None
        } else {
            page.keyboard
                .as_ref()
                .and_then(|selection| selection.action(&page.keyboard_session, &snapshot.enabled()))
        };
        page.keyboard_session = ProjectionId::new();
        page.keyboard = previous
            .and_then(|action| Selection::reconcile(None, &page.keyboard_session, &[action]));
        page.selection = ProjectionId::new();
        page.displayed = None;
    }
    page.mount = mount;
    page.lease = Rc::downgrade(&lease);
    page.keyboard = Selection::reconcile(
        page.keyboard.as_ref(),
        &page.keyboard_session,
        &snapshot.enabled(),
    );
    snapshot.focused = page.focus.is_focused(window);
    snapshot.selected = if snapshot.focused {
        page.keyboard
            .as_ref()
            .and_then(|selection| selection.action(&page.keyboard_session, &snapshot.enabled()))
    } else {
        None
    };
    snapshot.selection = page.selection.clone();
    page.shown = snapshot.clone();
    page.surface.sync_environment(cx);
    page.surface.publish(snapshot.clone());
    let enabled = !snapshot.enabled().is_empty();
    let _ = page.surface.focus_handle(cx).tab_stop(enabled);
    page.focus = page.focus.clone().tab_stop(enabled);
    let focus = page.focus.clone();
    let height = snapshot.height();
    let geometry = page.geometry.clone();
    let reveal_geometry = geometry.clone();
    let geometry_owner = cx.weak_entity();
    let mut host = div()
        .id(format!("artifact-nsh-{mode:?}"))
        .relative()
        .w_full()
        .min_w_0()
        .h(px(height))
        .track_focus(&focus)
        .tab_stop(!snapshot.enabled().is_empty())
        .role(gpui_kit::Role::Group)
        .key_context("ArtifactCard")
        .capture_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            keyboard(this, mode, event, window, cx)
        }))
        .on_action(cx.listener(move |this, _: &ArtifactActivate, window, cx| {
            let Some(page) = this.page(mode) else {
                return;
            };
            let Some((observed, action, _, _)) = page.displayed.clone() else {
                return;
            };
            if guarded(this, mode, &observed, action, window, cx) {
                activate(this, mode, &observed, action, window, cx);
            }
        }))
        .on_action(cx.listener(move |this, _: &ArtifactClear, window, cx| {
            if !this
                .page(mode)
                .is_some_and(|page| page.focus.contains_focused(window, cx))
            {
                return;
            }
            if let Some(observed) = current(this, mode, cx) {
                let action = if observed.files.iter().any(|file| file.menu) {
                    Action::CloseMenu
                } else {
                    Action::File(0, FileAction::Clear)
                };
                activate(this, mode, &observed, action, window, cx);
            }
        }))
        .child(page.surface.entity())
        .child(
            gpui_kit::canvas(
                move |bounds, window, cx| {
                    let clip = window.content_mask().bounds;
                    let (changed, reveal) = {
                        let mut geometry = reveal_geometry.borrow_mut();
                        let changed =
                            geometry.bounds != Some(bounds) || geometry.clip != Some(clip);
                        if changed {
                            geometry.revision = geometry.revision.saturating_add(1);
                            geometry.bounds = Some(bounds);
                            geometry.clip = Some(clip);
                        }
                        (changed, geometry.reveal.take())
                    };
                    if changed {
                        let _ = geometry_owner.update(cx, |this, cx| {
                            let hidden_menu = this
                                .page(mode)
                                .is_some_and(|page| page.menu.is_some() && !page.menu_visible());
                            if hidden_menu {
                                this.action(Action::CloseMenu, mode, cx);
                            }
                            if let Some(page) = this.page_mut(mode) {
                                page.displayed = None;
                                page.selection = ProjectionId::new();
                                cx.notify();
                            }
                        });
                    }
                    if let Some(reveal) = reveal {
                        window.request_autoscroll(reveal);
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );
    if snapshot.focused
        && let Some(action) = snapshot.selected
        && measured_activation(
            &snapshot,
            &snapshot,
            action,
            page.displayed.as_ref(),
            &page.geometry_for(action).borrow(),
            snapshot.focused,
        )
        && let Some(control) = snapshot
            .controls()
            .into_iter()
            .find(|control| control.action == action && control.enabled)
    {
        let observed = snapshot.clone();
        let weak = cx.weak_entity();
        host = host.child(
            div()
                .id("artifact-active-control")
                .absolute()
                .inset_0()
                .role(gpui_kit::Role::Button)
                .aria_label(control.label)
                .aria_description(control.hint)
                .aria_active_descendant()
                .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        if guarded(this, mode, &observed, action, window, cx) {
                            activate(this, mode, &observed, action, window, cx);
                        }
                    });
                }),
        );
    }
    if page.menu_visible()
        && let Some(top) = snapshot.menu_top()
        && let Some(menu) = page.menu.as_mut()
    {
        menu.surface.sync_environment(cx);
        menu.surface.publish(snapshot.clone());
        let _ = menu.surface.focus_handle(cx).tab_stop(enabled);
        let menu_lease = menu.lease.upgrade().unwrap_or_else(|| Rc::new(()));
        menu.lease = Rc::downgrade(&menu_lease);
        let menu_geometry = menu.geometry.clone();
        let menu_id = menu.id.clone();
        let menu_owner = cx.weak_entity();
        let outside_owner = cx.weak_entity();
        let outside = snapshot.clone();
        let popup = div()
            .id("artifact-menu-surface")
            .relative()
            .w(px(Layout::TASK_MENU_WIDTH))
            .h(px(nsh_view::menu_height()))
            .on_mouse_down_out(move |event, window, cx| {
                let _ = outside_owner.update(cx, |this, cx| {
                    let trigger = this.page(mode).is_some_and(|page| {
                        page.shown.accepts(&outside, Action::CloseMenu)
                            && page
                                .menu_trigger()
                                .is_some_and(|bounds| bounds.contains(&event.position))
                    });
                    if !trigger {
                        activate(this, mode, &outside, Action::CloseMenu, window, cx);
                    }
                });
            })
            .child(menu.surface.entity())
            .child(
                gpui_kit::canvas(
                    move |bounds, window, cx| {
                        let clip = window.content_mask().bounds;
                        let changed = {
                            let mut geometry = menu_geometry.borrow_mut();
                            let changed =
                                geometry.bounds != Some(bounds) || geometry.clip != Some(clip);
                            if changed {
                                geometry.revision = geometry.revision.saturating_add(1);
                                geometry.bounds = Some(bounds);
                                geometry.clip = Some(clip);
                            }
                            changed
                        };
                        if changed {
                            let _ = menu_owner.update(cx, |this, cx| {
                                if let Some(page) = this.page_mut(mode)
                                    && page
                                        .menu
                                        .as_ref()
                                        .is_some_and(|menu| menu.id.matches(&menu_id))
                                {
                                    page.displayed = None;
                                    page.selection = ProjectionId::new();
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
        host = host.child(
            div()
                .absolute()
                .top(px(top + Layout::ARTIFACT_ROW_HEIGHT))
                .right_0()
                .child(
                    gpui_kit::anchored()
                        .anchor(gpui_kit::Anchor::TopRight)
                        .position_mode(gpui_kit::AnchoredPositionMode::Local)
                        .position(gpui_kit::point(px(0.), px(0.)))
                        .snap_to_window_with_margin(px(Layout::TOOLBAR_TRAILING_INSET))
                        .child(
                            gpui_kit::deferred(Mounted {
                                child: popup.into_any_element(),
                                lease: menu_lease,
                            })
                            .with_priority(2),
                        ),
                ),
        );
    }
    Mounted {
        child: host.into_any_element(),
        lease,
    }
    .into_any_element()
}

struct Mounted {
    child: AnyElement,
    lease: Rc<()>,
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
        Some("artifact-mounted".into())
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
