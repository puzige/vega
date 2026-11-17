use super::super::{StreamEntry, model::StreamEntryIdentity, selection_menu::ReadingOwner};
use super::{ConversationStream, CopySource};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use vega_theme::{Motion, ThemeColors};

mod pixels;

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    list: Bounds<Pixels>,
    viewport: Size<Pixels>,
    foreground: Pixels,
    scale: f32,
    window: WindowId,
}

#[derive(Default)]
struct Mount {
    id: ProjectionId,
    frame: RefCell<ProjectionId>,
    geometry: Cell<Option<Geometry>>,
    painted: Cell<Option<Geometry>>,
    displayed: RefCell<Option<ProjectionId>>,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    intent: ProjectionId,
    mount: ProjectionId,
    frame: ProjectionId,
    epoch: ProjectionId,
    generation: u64,
    content_generation: u64,
    geometry: Option<Geometry>,
    label: &'static str,
    colors: ThemeColors,
    enabled: bool,
    focused: bool,
    hovered: bool,
    pressed: bool,
    receipt: Option<Instant>,
    activation: Option<bool>,
    revision: Arc<AtomicU64>,
}

struct Witness {
    revision: u64,
    control: [f32; 4],
    root: [f32; 4],
}

enum Request {
    Activate(Box<Snapshot>, Witness),
}

struct Binding {
    stream: WeakEntity<ConversationStream>,
    reading: ReadingOwner,
    identity: StreamEntryIdentity,
    source_id: u64,
    code: Option<(u64, usize)>,
    content_generation: u64,
}

impl Binding {
    fn current(&self, source: &CopySource, generation: u64, window: &Window, cx: &App) -> bool {
        let Some(owner) = self.stream.upgrade() else {
            return false;
        };
        let stream = owner.read(cx);
        let Some(index) = stream
            .entry_identities
            .iter()
            .position(|id| id == &self.identity)
        else {
            return false;
        };
        if !self.reading.root_current(window, cx)
            || !stream.message_actions_enabled()
            || stream.ui_presentations_suspended
            || source.generation() != generation
            || !cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| {
                    route
                        .0
                        .as_ref()
                        .is_some_and(|thread| thread.id == stream.thread.id)
                })
        {
            return false;
        }
        let Some(
            StreamEntry::User { copy, model, .. } | StreamEntry::Assistant { copy, model, .. },
        ) = stream.entries.get(index)
        else {
            return false;
        };
        if copy.id != self.source_id || copy.content_generation() != self.content_generation {
            return false;
        }
        match source {
            CopySource::Code(code) => {
                !code.is_empty()
                    && model
                        .committed_lines
                        .iter()
                        .chain(&model.pending_lines)
                        .any(|line| {
                            line.code_block.as_ref().is_some_and(|block| {
                                Some(block.identity) == self.code && Arc::ptr_eq(&block.code, code)
                            })
                        })
            }
        }
    }

    fn list(&self, window: &Window, cx: &App) -> Option<Bounds<Pixels>> {
        super::super::selection_menu::mounted::list_bounds(&self.stream, window, cx).or_else(|| {
            self.stream
                .upgrade()
                .map(|owner| owner.read(cx).list.viewport_bounds())
        })
    }
}

type MountedSnapshot = Option<(WeakEntity<Host>, Snapshot)>;

#[derive(Clone, Default)]
pub(super) struct Route(Rc<RefCell<MountedSnapshot>>, Rc<RefCell<Option<Hitbox>>>);

impl Route {
    pub(super) fn bridge(&self, bridge: Stateful<crate::interaction::Interaction>) -> Bridge {
        let down = self.clone();
        let up = self.clone();
        let hover = self.clone();
        let button = bridge
            .capture_any_mouse_down(move |event, window, cx| {
                if event.button != MouseButton::Left
                    || !down
                        .1
                        .borrow()
                        .as_ref()
                        .is_some_and(|hitbox| hitbox.is_hovered_at(event.position, window))
                {
                    return;
                }
                if let Some((host, observed)) = down.0.borrow().clone() {
                    let _ = host.update(cx, |host, cx| {
                        if host.valid(&observed, window, cx)
                            && observed
                                .geometry
                                .is_some_and(|geometry| geometry.clip.contains(&event.position))
                        {
                            host.pointer = Some(observed);
                            host.snapshot.pressed = true;
                            cx.notify();
                        }
                    });
                }
            })
            .capture_any_mouse_up(move |event, window, cx| {
                if event.button != MouseButton::Left {
                    return;
                }
                if let Some((host, _)) = up.0.borrow().clone() {
                    let _ =
                        host.update(cx, |host, cx| {
                            host.snapshot.pressed = false;
                            if let Some(observed) = host.pointer.take()
                                && observed
                                    .geometry
                                    .is_some_and(|geometry| geometry.clip.contains(&event.position))
                                && up.1.borrow().as_ref().is_some_and(|hitbox| {
                                    hitbox.is_hovered_at(event.position, window)
                                })
                            {
                                host.request(&observed, false, window, cx);
                            }
                            cx.notify();
                        });
                }
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(move |hovered, _, cx| {
                if let Some((host, _)) = hover.0.borrow().clone() {
                    let _ = host.update(cx, |host, cx| {
                        host.snapshot.hovered = *hovered;
                        cx.notify();
                    });
                }
            });
        Bridge {
            button,
            hitbox: self.1.clone(),
        }
    }
}

pub(super) struct Bridge {
    button: Stateful<crate::interaction::Interaction>,
    hitbox: Rc<RefCell<Option<Hitbox>>>,
}

impl IntoElement for Bridge {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Bridge {
    type RequestLayoutState =
        <Stateful<crate::interaction::Interaction> as Element>::RequestLayoutState;
    type PrepaintState = <Stateful<crate::interaction::Interaction> as Element>::PrepaintState;
    fn id(&self) -> Option<ElementId> {
        Element::id(&self.button)
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.button.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let hitbox = self
            .button
            .prepaint(id, inspector, bounds, layout, window, cx);
        *self.hitbox.borrow_mut() = hitbox.clone();
        hitbox
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.button
            .paint(id, inspector, bounds, layout, hitbox, window, cx);
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct Native {
    key: SharedString,
    source: CopySource,
    label: &'static str,
    colors: ThemeColors,
    owner: Option<super::CopyOwner>,
    appearance: vega_theme::Appearance,
    language: &'static str,
    route: Route,
}

impl Native {
    pub(super) fn new(
        key: SharedString,
        source: CopySource,
        label: &'static str,
        colors: ThemeColors,
        owner: Option<super::CopyOwner>,
        route: Route,
        cx: &App,
    ) -> Self {
        Self {
            key,
            source,
            label,
            colors,
            owner,
            route,
            appearance: vega_theme::theme(cx).appearance,
            language: vega_i18n::language(),
        }
    }
}

struct Host {
    source: CopySource,
    binding: Option<Binding>,
    snapshot: Snapshot,
    published: Option<Snapshot>,
    surface: Option<Surface<Snapshot, Request>>,
    lease: Weak<Mount>,
    focus: FocusHandle,
    timer: Option<Task<()>>,
    appearance: vega_theme::Appearance,
    language: &'static str,
    route: Route,
    pointer: Option<Snapshot>,
    owner_subscription: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl Host {
    fn invalidate(&mut self) {
        self.snapshot.id = ProjectionId::new();
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.activation = None;
        self.snapshot.pressed = false;
        self.pointer = None;
        if let Some(lease) = self.lease.upgrade() {
            lease.painted.set(None);
            *lease.displayed.borrow_mut() = None;
        }
    }

    fn stop(&mut self, cx: &App) {
        self.invalidate();
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.snapshot.receipt = None;
        self.timer = None;
        self.published = None;
    }

    fn valid(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        let (Some(binding), Some(lease), Some(geometry)) =
            (&self.binding, self.lease.upgrade(), observed.geometry)
        else {
            return false;
        };
        let Some(owner) = binding.stream.upgrade() else {
            return false;
        };
        self.snapshot.id.matches(&observed.id)
            && self.snapshot.intent.matches(&observed.intent)
            && observed
                .epoch
                .matches(&owner.read(cx).reading_navigation_epoch)
            && observed.mount.matches(&lease.id)
            && observed.frame.matches(&lease.frame.borrow())
            && lease.geometry.get() == Some(geometry)
            && lease.painted.get() == Some(geometry)
            && lease
                .displayed
                .borrow()
                .as_ref()
                .is_some_and(|id| id.matches(&observed.id))
            && self.published.as_ref().is_some_and(|published| {
                published.id.matches(&observed.id) && published.intent.matches(&observed.intent)
            })
            && geometry.viewport == window.viewport_size()
            && geometry.scale == window.scale_factor()
            && geometry.window == window.window_handle().window_id()
            && geometry.foreground == owner.read(cx).foreground_height
            && binding.list(window, cx) == Some(geometry.list)
            && !geometry.clip.is_empty()
            && observed.content_generation == binding.content_generation
            && observed.enabled
            && binding.current(&self.source, observed.generation, window, cx)
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
    }

    fn request(
        &mut self,
        observed: &Snapshot,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !self.valid(observed, window, cx) || (keyboard && !self.focus.is_focused(window)) {
            return;
        }
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.activation = Some(keyboard);
        cx.notify();
    }

    fn activate(
        &mut self,
        observed: &Snapshot,
        witness: Witness,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.valid(observed, window, cx)
            || (observed.activation == Some(true) && !self.focus.is_focused(window))
            || witness.revision != self.snapshot.revision.load(Ordering::Acquire)
            || !pixels::contained(witness.control, witness.root)
            || !observed.geometry.is_some_and(|geometry| {
                let [x, y, width, height] = witness.root;
                let [cx, cy, cw, ch] = witness.control;
                x.abs() < 1.
                    && y.abs() < 1.
                    && (width - f32::from(geometry.bounds.size.width)).abs() < 1.
                    && (height - f32::from(geometry.bounds.size.height)).abs() < 1.
                    && geometry.clip.contains(&point(
                        geometry.bounds.origin.x + px(cx),
                        geometry.bounds.origin.y + px(cy),
                    ))
                    && geometry.clip.contains(&point(
                        geometry.bounds.origin.x + px(cx + cw - 0.5),
                        geometry.bounds.origin.y + px(cy + ch - 0.5),
                    ))
            })
        {
            return;
        }
        let text = self.source.text();
        if text.is_empty() {
            return;
        }
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.activation = None;
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let deadline =
            cx.background_executor().now() + Duration::from_millis(Motion::COPY_RECEIPT_MS);
        self.snapshot.receipt = Some(deadline);
        self.timer = Some(cx.spawn_in(window, async move |host, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(Motion::COPY_RECEIPT_MS))
                .await;
            let _ = host.update_in(cx, |host, _, cx| {
                if host.snapshot.receipt == Some(deadline) {
                    host.snapshot.receipt = None;
                    host.timer = None;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new_transparent(self.snapshot.clone(), pixels::render, window, cx);
        surface.focus_handle(cx).tab_stop(false);
        self.published = Some(self.snapshot.clone());
        self.surface = Some(surface);
        cx.spawn_in(window, async move |host, cx| {
            while let Some(Request::Activate(observed, witness)) = requests.recv().await {
                if host
                    .update_in(cx, |host, window, cx| {
                        host.activate(&observed, witness, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
}

impl RenderOnce for Native {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let binding = self.owner.as_ref().and_then(|(stream, source_id, code)| {
            let entity = stream.upgrade()?;
            let owner = entity.read(cx);
            let index = owner.entries.iter().position(|entry| matches!(entry, StreamEntry::User { copy, .. } | StreamEntry::Assistant { copy, .. } if copy.id == *source_id))?;
            let identity = owner.entry_identities.get(index)?.clone();
            let content_generation = match owner.entries.get(index)? {
                StreamEntry::User { copy, .. } | StreamEntry::Assistant { copy, .. } => copy.content_generation(),
                _ => return None,
            };
            Some(Binding { stream: stream.clone(), reading: ReadingOwner::message(owner, stream.clone(), Some(identity.clone())), identity, source_id: *source_id, code: *code, content_generation })
        });
        let epoch = binding
            .as_ref()
            .and_then(|binding| binding.stream.upgrade())
            .map(|owner| owner.read(cx).reading_navigation_epoch.clone())
            .unwrap_or_default();
        let key: SharedString = format!("nsh-copy-receipt-{}", self.key).into();
        let host = window.use_keyed_state(key, cx, |window, cx| {
            let focus = cx.focus_handle().tab_stop(true);
            let subscriptions = vec![
                cx.on_release(|host: &mut Host, cx| host.stop(cx)),
                cx.observe_global::<crate::sidebar::OpenedThread>(|host: &mut Host, cx| {
                    host.stop(cx);
                    cx.notify();
                }),
                cx.observe_global::<crate::settings::SettingsOpen>(|host: &mut Host, cx| {
                    host.stop(cx);
                    cx.notify();
                }),
                cx.observe_global::<crate::sidebar::PendingDeleteConfirm>(|host: &mut Host, cx| {
                    host.stop(cx);
                    cx.notify();
                }),
                cx.observe_window_activation(window, |host: &mut Host, _, cx| {
                    host.stop(cx);
                    cx.notify();
                }),
                cx.observe_window_bounds(window, |host: &mut Host, _, cx| {
                    host.invalidate();
                    cx.notify();
                }),
                cx.on_focus_in(&focus, window, |host: &mut Host, _, cx| {
                    host.snapshot.intent = ProjectionId::new();
                    host.snapshot.activation = None;
                    cx.notify();
                }),
                cx.on_focus_out(&focus, window, |host: &mut Host, _, _, cx| {
                    host.snapshot.intent = ProjectionId::new();
                    host.snapshot.activation = None;
                    cx.notify();
                }),
            ];
            Host {
                source: self.source.clone(),
                binding: None,
                snapshot: Snapshot {
                    id: ProjectionId::new(),
                    intent: ProjectionId::new(),
                    mount: ProjectionId::new(),
                    frame: ProjectionId::new(),
                    epoch: epoch.clone(),
                    generation: self.source.generation(),
                    content_generation: 0,
                    geometry: None,
                    label: self.label,
                    colors: self.colors,
                    enabled: false,
                    focused: false,
                    hovered: false,
                    pressed: false,
                    receipt: None,
                    activation: None,
                    revision: Arc::new(AtomicU64::new(0)),
                },
                published: None,
                surface: None,
                lease: Weak::new(),
                focus,
                timer: None,
                appearance: self.appearance,
                language: self.language,
                route: self.route.clone(),
                pointer: None,
                owner_subscription: None,
                _subscriptions: subscriptions,
            }
        });
        host.update(cx, |host, cx| {
            let same_binding = match (&host.binding, &binding) {
                (Some(left), Some(right)) => {
                    left.stream == right.stream
                        && left.identity == right.identity
                        && left.source_id == right.source_id
                        && left.code == right.code
                        && left.content_generation == right.content_generation
                        && left.reading.same_binding(&right.reading)
                }
                (None, None) => true,
                _ => false,
            };
            if !same_binding
                || !host.source.same_source(&self.source)
                || host.snapshot.generation != self.source.generation()
                || !host.snapshot.epoch.matches(&epoch)
                || host.appearance != self.appearance
                || host.language != self.language
            {
                host.stop(cx);
            }
            if !same_binding {
                host.owner_subscription = binding
                    .as_ref()
                    .and_then(|binding| binding.stream.upgrade())
                    .map(|owner| {
                        cx.observe(&owner, |host: &mut Host, _, cx| {
                            host.invalidate();
                            cx.notify();
                        })
                    });
            }
            host.snapshot.content_generation = binding
                .as_ref()
                .map_or(0, |binding| binding.content_generation);
            host.source = self.source;
            host.binding = binding;
            host.snapshot.epoch = epoch;
            host.snapshot.generation = host.source.generation();
            host.snapshot.colors = self.colors;
            host.snapshot.label = self.label;
            host.appearance = self.appearance;
            host.language = self.language;
        });
        let snapshot = host.read(cx).snapshot.clone();
        *self.route.0.borrow_mut() = Some((host.downgrade(), snapshot));
        host.update(cx, |host, _| host.route = self.route);
        host.into_any_element()
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lease = self.lease.upgrade().unwrap_or_else(|| {
            self.stop(cx);
            Rc::new(Mount::default())
        });
        self.lease = Rc::downgrade(&lease);
        if !self.snapshot.mount.matches(&lease.id)
            || !self.snapshot.frame.matches(&lease.frame.borrow())
            || self.snapshot.geometry != lease.geometry.get()
        {
            self.invalidate();
            self.snapshot.mount = lease.id.clone();
            self.snapshot.frame = lease.frame.borrow().clone();
            self.snapshot.geometry = lease.geometry.get();
            self.snapshot.revision = Arc::new(AtomicU64::new(0));
        }
        self.snapshot.enabled = self.binding.as_ref().is_some_and(|binding| {
            binding.current(&self.source, self.snapshot.generation, window, cx)
        });
        if !self.snapshot.enabled {
            self.snapshot.receipt = None;
            self.timer = None;
        }
        self.snapshot.focused = self.focus.is_focused(window);
        self.focus = self.focus.clone().tab_stop(self.snapshot.enabled);
        self.mount(window, cx);
        let Some(surface) = &mut self.surface else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        let publish = self.published.as_ref().is_none_or(|old| {
            !old.id.matches(&self.snapshot.id)
                || !old.intent.matches(&self.snapshot.intent)
                || old.enabled != self.snapshot.enabled
                || old.focused != self.snapshot.focused
                || old.hovered != self.snapshot.hovered
                || old.pressed != self.snapshot.pressed
                || old.receipt != self.snapshot.receipt
        });
        if publish {
            surface.publish(self.snapshot.clone());
            self.published = Some(self.snapshot.clone());
        }
        let observed = self.snapshot.clone();
        *self.route.0.borrow_mut() = Some((cx.weak_entity(), observed.clone()));
        let ax = observed.clone();
        let keyboard = observed.clone();
        let focus_observed = observed.clone();
        let target = self.focus.clone();
        let keyboard_focus = self.focus.clone();
        let focus_host = cx.weak_entity();
        let ax_host = cx.weak_entity();
        let mut proxy = div()
            .id("nsh-copy-receipt-ax")
            .absolute()
            .size(px(0.))
            .track_focus(&self.focus)
            .tab_stop(self.snapshot.enabled)
            .role(Role::Button)
            .aria_label(self.snapshot.label)
            .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                let _ = focus_host.update(cx, |host, cx| {
                    if host.valid(&focus_observed, window, cx) {
                        window.focus(&target, cx);
                        cx.notify();
                    }
                });
            })
            .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                let _ = ax_host.update(cx, |host, cx| host.request(&ax, false, window, cx));
            })
            .on_key_down(cx.listener(move |host, event: &KeyDownEvent, window, cx| {
                if keyboard_focus.is_focused(window)
                    && matches!(
                        crate::nsh::keyboard::Key::from_event(event),
                        crate::nsh::keyboard::Key::Activate
                    )
                {
                    host.request(&keyboard, true, window, cx);
                    cx.stop_propagation();
                }
            }));
        if self.snapshot.receipt.is_some() {
            proxy = proxy.aria_description(vega_i18n::t("ui.copy_requested"));
        }
        Mounted {
            child: div()
                .w_full()
                .h_full()
                .relative()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(surface.entity())
                .child(proxy)
                .into_any_element(),
            lease,
            host: cx.weak_entity(),
            owner: self.binding.as_ref().map(|binding| binding.stream.clone()),
            displayed: observed.id,
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<Mount>,
    host: WeakEntity<Host>,
    owner: Option<WeakEntity<ConversationStream>>,
    displayed: ProjectionId,
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
        Some("nsh-copy-receipt-mount".into())
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
        self.lease.painted.set(None);
        *self.lease.displayed.borrow_mut() = None;
        if let Some(id) = id {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.lease.clone()));
            if let Some(owner) = self.owner.as_ref().and_then(|owner| owner.upgrade()) {
                let stream = owner.read(cx);
                let list = super::super::selection_menu::mounted::list_bounds(
                    &owner.downgrade(),
                    window,
                    cx,
                )
                .unwrap_or_else(|| stream.list.viewport_bounds());
                let clip = bounds
                    .intersect(&window.content_mask().bounds)
                    .intersect(&list);
                if !clip.is_empty() {
                    self.lease.geometry.set(Some(Geometry {
                        bounds,
                        clip,
                        list,
                        viewport: window.viewport_size(),
                        foreground: stream.foreground_height,
                        scale: window.scale_factor(),
                        window: window.window_handle().window_id(),
                    }));
                }
            }
        }
        if previous != self.lease.geometry.get() {
            *self.lease.frame.borrow_mut() = ProjectionId::new();
            let _ = self.host.update(cx, |host, cx| {
                host.invalidate();
                cx.notify();
            });
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
        self.lease.painted.set(self.lease.geometry.get());
        *self.lease.displayed.borrow_mut() = Some(self.displayed.clone());
    }
}
