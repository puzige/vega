use super::model::StreamEntryIdentity;
use super::selection_menu::ReadingOwner;
use super::{ConversationStream, ForkRequested, MessageCopy};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use vega_theme::{Layout, Motion, ThemeColors};

mod pixels;

pub(super) struct MessageActions<'a> {
    pub(super) id: SharedString,
    pub(super) owner: WeakEntity<ConversationStream>,
    pub(super) identity: Option<StreamEntryIdentity>,
    pub(super) user: bool,
    pub(super) user_bubble: bool,
    pub(super) colors: ThemeColors,
    pub(super) enabled: bool,
    pub(super) timestamp: Option<super::timestamp_format::TimestampDisplay>,
    pub(super) focus: Option<&'a FocusHandle>,
    pub(super) fork: Option<(ForkRequested, WeakEntity<ConversationStream>)>,
    pub(super) fork_pending: Option<&'a str>,
    pub(super) fork_enabled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Copy,
    Fork,
}

#[derive(Clone, PartialEq, Eq)]
struct Source {
    identity: Option<StreamEntryIdentity>,
    copy: Option<(u64, u64)>,
    fork: Option<ForkRequested>,
    pending: Option<String>,
    enabled: bool,
    fork_enabled: bool,
    timestamp: Option<(String, String)>,
    user: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    viewport: Size<Pixels>,
    list: Bounds<Pixels>,
    foreground: Pixels,
    scale: f32,
    window: WindowId,
}

#[derive(Default)]
struct Mount {
    id: ProjectionId,
    geometry: Cell<Option<Geometry>>,
    painted: Cell<Option<Geometry>>,
    displayed: RefCell<Option<ProjectionId>>,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    epoch: ProjectionId,
    mount: ProjectionId,
    source: Source,
    geometry: Option<Geometry>,
    colors: ThemeColors,
    visible: bool,
    focused: Option<Action>,
    receipt: Option<Instant>,
    intent: ProjectionId,
    activation: Option<(Action, bool)>,
    revision: Arc<AtomicU64>,
}

impl Snapshot {
    fn enabled(&self, action: Action) -> bool {
        match action {
            Action::Copy => self.source.enabled && self.source.copy.is_some(),
            Action::Fork => {
                self.source.fork_enabled
                    && self.source.fork.is_some()
                    && self.source.pending.is_none()
            }
        }
    }

    fn pending(&self) -> bool {
        self.source.fork.as_ref().is_some_and(|request| {
            self.source.pending.as_deref() == Some(request.message_id.as_str())
        })
    }
}

struct Input {
    owner: WeakEntity<ConversationStream>,
    copy: Option<MessageCopy>,
    source: Source,
    focus: Option<FocusHandle>,
    colors: ThemeColors,
    appearance: vega_theme::Appearance,
    language: &'static str,
}

impl Input {
    fn current(
        &self,
        reading: &ReadingOwner,
        interactive: bool,
        window: &Window,
        cx: &App,
    ) -> bool {
        if interactive && !reading.root_current(window, cx) {
            return false;
        }
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        let stream = owner.read(cx);
        let Some(identity) = &self.source.identity else {
            return false;
        };
        let Some(index) = stream
            .entry_identities
            .iter()
            .position(|value| value == identity)
        else {
            return false;
        };
        let current_copy = match stream.entries.get(index) {
            Some(super::StreamEntry::User { copy, .. }) => Some(copy),
            _ => stream.action_copy_source_for_entry(index),
        }
        .filter(|copy| copy.has_text());
        let timestamp = stream
            .entry_timestamp(index)
            .map(|value| (value.message, value.accessible));
        stream.message_actions_enabled() == self.source.enabled
            && stream.fork_is_idle(cx) == self.source.fork_enabled
            && stream.fork_pending == self.source.pending
            && stream.fork_request_for_entry(index) == self.source.fork
            && current_copy.map(|copy| (copy.id, copy.content_generation())) == self.source.copy
            && timestamp == self.source.timestamp
            && self.copy.as_ref().is_none_or(|copy| {
                (!interactive || reading.current(copy.id, window, cx))
                    && Some((copy.id, copy.content_generation())) == self.source.copy
                    && copy.has_text()
            })
            && !stream.ui_presentations_suspended
            && (!interactive
                || stream.list.bounds_for_item(index).is_some_and(|bounds| {
                    !bounds.intersect(&stream.list.viewport_bounds()).is_empty()
                }))
            && cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| {
                    route
                        .0
                        .as_ref()
                        .is_some_and(|thread| thread.id == stream.thread.id)
                })
    }
}

struct OwnerState {
    source: Option<Source>,
    reading: ReadingOwner,
    suspended: bool,
    foreground: Pixels,
    route: Option<String>,
    appearance: vega_theme::Appearance,
    language: &'static str,
}

impl OwnerState {
    fn capture(
        owner: &Entity<ConversationStream>,
        identity: Option<&StreamEntryIdentity>,
        cx: &App,
    ) -> Self {
        let stream = owner.read(cx);
        let source = identity
            .and_then(|identity| {
                stream
                    .entry_identities
                    .iter()
                    .position(|current| current == identity)
            })
            .map(|index| {
                let entry = stream.entries.get(index);
                let copy = match entry {
                    Some(super::StreamEntry::User { copy, .. }) => Some(copy),
                    _ => stream.action_copy_source_for_entry(index),
                }
                .filter(|copy| copy.has_text());
                Source {
                    identity: stream.entry_identities.get(index).cloned(),
                    copy: copy.map(|copy| (copy.id, copy.content_generation())),
                    fork: stream.fork_request_for_entry(index),
                    pending: stream.fork_pending.clone(),
                    enabled: stream.message_actions_enabled(),
                    fork_enabled: stream.fork_is_idle(cx),
                    timestamp: stream
                        .entry_timestamp(index)
                        .map(|value| (value.message, value.accessible)),
                    user: matches!(
                        entry,
                        Some(
                            super::StreamEntry::User { .. } | super::StreamEntry::UserImages { .. }
                        )
                    ),
                }
            });
        Self {
            source,
            reading: ReadingOwner::message(stream, owner.downgrade(), identity.cloned()),
            suspended: stream.ui_presentations_suspended,
            foreground: stream.foreground_height,
            route: cx
                .try_global::<crate::sidebar::OpenedThread>()
                .and_then(|route| route.0.as_ref().map(|thread| thread.id.clone())),
            appearance: vega_theme::theme(cx).appearance,
            language: vega_i18n::language(),
        }
    }

    fn same_presentation(&self, other: &Self) -> bool {
        self.source == other.source
            && self.reading.same_binding(&other.reading)
            && self.suspended == other.suspended
            && self.foreground == other.foreground
            && self.route == other.route
            && self.appearance == other.appearance
            && self.language == other.language
    }
}

struct Host {
    input: Input,
    owner_state: OwnerState,
    reading: ReadingOwner,
    snapshot: Snapshot,
    published: Option<Snapshot>,
    surface: Option<Surface<Snapshot, Request>>,
    lease: Weak<Mount>,
    hovered: bool,
    copy_focus: FocusHandle,
    fork_focus: FocusHandle,
    receipt_timer: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

struct Witness {
    revision: u64,
    control: [f32; 4],
    root: [f32; 4],
}

enum Request {
    Activate(Box<Snapshot>, Action, Witness),
}

impl Host {
    fn observe_owner(owner: &Entity<ConversationStream>, cx: &mut Context<Self>) -> Subscription {
        cx.observe(owner, |host, owner, cx| {
            let state = OwnerState::capture(&owner, host.input.source.identity.as_ref(), cx);
            if !host.owner_state.same_presentation(&state) {
                host.owner_state = state;
                host.stop(cx);
                cx.notify();
            }
        })
    }

    fn stop(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.published = None;
        self.snapshot.id = ProjectionId::new();
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.activation = None;
        self.snapshot.receipt = None;
        self.receipt_timer = None;
        if let Some(lease) = self.lease.upgrade() {
            lease.painted.set(None);
            *lease.displayed.borrow_mut() = None;
        }
    }

    fn valid(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        let Some(lease) = self.lease.upgrade() else {
            return false;
        };
        let Some(geometry) = lease.geometry.get() else {
            return false;
        };
        let Some(owner) = self.input.owner.upgrade() else {
            return false;
        };
        let stream = owner.read(cx);
        self.snapshot.id.matches(&observed.id)
            && self.snapshot.intent.matches(&observed.intent)
            && self.snapshot.source == observed.source
            && self.snapshot.epoch.matches(&observed.epoch)
            && observed.epoch.matches(&stream.reading_navigation_epoch)
            && observed.mount.matches(&lease.id)
            && observed.geometry == Some(geometry)
            && lease.painted.get() == Some(geometry)
            && lease
                .displayed
                .borrow()
                .as_ref()
                .is_some_and(|displayed| displayed.matches(&observed.id))
            && self.published.as_ref().is_some_and(|published| {
                published.id.matches(&observed.id)
                    && published.intent.matches(&observed.intent)
                    && published.mount.matches(&lease.id)
                    && published.source == observed.source
                    && published.geometry == observed.geometry
            })
            && geometry.viewport == window.viewport_size()
            && geometry.scale == window.scale_factor()
            && geometry.window == window.window_handle().window_id()
            && geometry.foreground == stream.foreground_height
            && geometry.list == stream.list.viewport_bounds()
            && !geometry.clip.is_empty()
            && self.input.current(&self.reading, true, window, cx)
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
    }

    fn request(
        &mut self,
        observed: &Snapshot,
        action: Action,
        keyboard: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !observed.enabled(action) || !self.valid(observed, window, cx) {
            return;
        }
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.activation = Some((action, keyboard));
        cx.notify();
    }

    fn activate(
        &mut self,
        observed: &Snapshot,
        action: Action,
        witness: Witness,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Witness {
            revision,
            control,
            root,
        } = witness;
        if !self.valid(observed, window, cx)
            || !observed.enabled(action)
            || (observed.activation == Some((action, true))
                && !match action {
                    Action::Copy => self.copy_focus.is_focused(window),
                    Action::Fork => self.fork_focus.is_focused(window),
                })
            || self.snapshot.revision.load(Ordering::Acquire) != revision
            || !pixels::contained(control, root)
            || !observed.geometry.is_some_and(|geometry| {
                let width = f32::from(geometry.bounds.size.width);
                let height = f32::from(geometry.bounds.size.height);
                root[0].abs() < 1.
                    && root[1].abs() < 1.
                    && (root[2] - width).abs() < 1.
                    && (root[3] - height).abs() < 1.
                    && geometry.clip.contains(&point(
                        geometry.bounds.origin.x + px(control[0]),
                        geometry.bounds.origin.y + px(control[1]),
                    ))
                    && geometry.clip.contains(&point(
                        geometry.bounds.origin.x + px(control[0] + control[2] - 0.5),
                        geometry.bounds.origin.y + px(control[1] + control[3] - 0.5),
                    ))
            })
        {
            return;
        }
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.activation = None;
        match action {
            Action::Copy => {
                let Some(copy) = self
                    .input
                    .copy
                    .as_ref()
                    .filter(|copy| copy.has_text() && !copy.display_text().is_empty())
                else {
                    return;
                };
                copy.copy(cx);
                let deadline =
                    cx.background_executor().now() + Duration::from_millis(Motion::COPY_RECEIPT_MS);
                self.snapshot.receipt = Some(deadline);
                self.receipt_timer = Some(cx.spawn_in(window, async move |host, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(Motion::COPY_RECEIPT_MS))
                        .await;
                    let _ = host.update_in(cx, |host, _, cx| {
                        if host.snapshot.receipt == Some(deadline) {
                            host.snapshot.receipt = None;
                            host.receipt_timer = None;
                            cx.notify();
                        }
                    });
                }));
            }
            Action::Fork => {
                if let Some(request) = &observed.source.fork {
                    let _ = self.input.owner.update(cx, |stream, cx| {
                        if !stream.permissions_revoked
                            && !stream.reading_preview
                            && !stream.permission_change_pending()
                        {
                            stream.request_fork(request, cx);
                        }
                    });
                }
            }
        }
        cx.notify();
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new_transparent(self.snapshot.clone(), pixels::render, window, cx);
        surface.focus_handle(cx).tab_stop(false);
        self.surface = Some(surface);
        self.published = Some(self.snapshot.clone());
        cx.spawn_in(window, async move |host, cx| {
            while let Some(Request::Activate(observed, action, witness)) = requests.recv().await {
                if host
                    .update_in(cx, |host, window, cx| {
                        host.activate(&observed, action, witness, window, cx);
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

#[derive(gpui_kit::IntoElement)]
struct Native {
    input: Input,
    route: Rc<RefCell<Option<WeakEntity<Host>>>>,
}

impl RenderOnce for Native {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.input.owner.upgrade() else {
            return div().into_any_element();
        };
        let reading = ReadingOwner::message(
            owner.read(cx),
            self.input.owner.clone(),
            self.input.source.identity.clone(),
        );
        let epoch = owner.read(cx).reading_navigation_epoch.clone();
        let key = format!(
            "nsh-message-footer-{}-{}",
            owner.entity_id(),
            self.input
                .source
                .identity
                .as_ref()
                .map(|identity| identity.key.as_str())
                .unwrap_or("unbound")
        );
        let host = window.use_keyed_state(key, cx, |window, cx| {
            let copy_focus = cx.focus_handle().tab_stop(true);
            let fork_focus = cx.focus_handle().tab_stop(true);
            let subscriptions = vec![
                cx.on_release(|host: &mut Host, cx| host.stop(cx)),
                cx.observe_window_activation(window, |host: &mut Host, _, cx| {
                    host.stop(cx);
                    cx.notify();
                }),
                cx.observe_window_bounds(window, |host: &mut Host, _, cx| {
                    host.stop(cx);
                    cx.notify();
                }),
                cx.on_focus_in(&copy_focus, window, |_: &mut Host, _, cx| cx.notify()),
                cx.on_focus_out(&copy_focus, window, |host: &mut Host, _, _, cx| {
                    host.snapshot.intent = ProjectionId::new();
                    host.snapshot.activation = None;
                    cx.notify();
                }),
                cx.on_focus_in(&fork_focus, window, |_: &mut Host, _, cx| cx.notify()),
                cx.on_focus_out(&fork_focus, window, |host: &mut Host, _, _, cx| {
                    host.snapshot.intent = ProjectionId::new();
                    host.snapshot.activation = None;
                    cx.notify();
                }),
                Host::observe_owner(&owner, cx),
            ];
            Host {
                owner_state: OwnerState::capture(&owner, self.input.source.identity.as_ref(), cx),
                snapshot: Snapshot {
                    id: ProjectionId::new(),
                    epoch: epoch.clone(),
                    mount: ProjectionId::new(),
                    source: self.input.source.clone(),
                    geometry: None,
                    colors: self.input.colors,
                    visible: false,
                    focused: None,
                    receipt: None,
                    intent: ProjectionId::new(),
                    activation: None,
                    revision: Arc::new(AtomicU64::new(0)),
                },
                input: Input {
                    owner: self.input.owner.clone(),
                    copy: self.input.copy.clone(),
                    source: self.input.source.clone(),
                    focus: self.input.focus.clone(),
                    colors: self.input.colors,
                    appearance: self.input.appearance,
                    language: self.input.language,
                },
                reading: reading.clone(),
                published: None,
                surface: None,
                lease: Weak::new(),
                hovered: false,
                copy_focus,
                fork_focus,
                receipt_timer: None,
                _subscriptions: subscriptions,
            }
        });
        host.update(cx, |host, cx| {
            let state = OwnerState::capture(&owner, self.input.source.identity.as_ref(), cx);
            if !host.owner_state.same_presentation(&state)
                || host.input.source != self.input.source
                || !host.snapshot.epoch.matches(&epoch)
                || !host.reading.same_binding(&reading)
                || host.input.appearance != self.input.appearance
                || host.input.language != self.input.language
            {
                host.stop(cx);
                host.snapshot.source = self.input.source.clone();
                host.snapshot.epoch = epoch;
                host.snapshot.colors = self.input.colors;
            }
            host.owner_state = state;
            host.input = self.input;
            host.reading = reading;
        });
        *self.route.borrow_mut() = Some(host.downgrade());
        host.into_any_element()
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.input.current(&self.reading, false, window, cx) {
            self.stop(cx);
            return div()
                .w_full()
                .h(px(Layout::COPY_ACTION_SIZE))
                .into_any_element();
        }
        let lease = self.lease.upgrade().unwrap_or_else(|| {
            self.stop(cx);
            Rc::new(Mount::default())
        });
        self.lease = Rc::downgrade(&lease);
        if !self.snapshot.mount.matches(&lease.id) || self.snapshot.geometry != lease.geometry.get()
        {
            self.snapshot.id = ProjectionId::new();
            self.snapshot.intent = ProjectionId::new();
            self.snapshot.activation = None;
            self.snapshot.mount = lease.id.clone();
            self.snapshot.geometry = lease.geometry.get();
            self.snapshot.revision = Arc::new(AtomicU64::new(0));
        }
        self.snapshot.focused = if self.copy_focus.is_focused(window) {
            Some(Action::Copy)
        } else if self.fork_focus.is_focused(window) {
            Some(Action::Fork)
        } else {
            None
        };
        self.snapshot.visible = self.hovered
            || self.snapshot.pending()
            || self.snapshot.focused.is_some()
            || self
                .input
                .focus
                .as_ref()
                .is_some_and(|focus| focus.contains_focused(window, cx))
            || self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.focus_handle(cx).contains_focused(window, cx));
        self.mount(window, cx);
        let Some(surface) = &mut self.surface else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        let publish = self.published.as_ref().is_none_or(|previous| {
            !previous.id.matches(&self.snapshot.id)
                || !previous.intent.matches(&self.snapshot.intent)
                || previous.visible != self.snapshot.visible
                || previous.focused != self.snapshot.focused
                || previous.receipt != self.snapshot.receipt
        });
        if publish {
            surface.publish(self.snapshot.clone());
            self.published = Some(self.snapshot.clone());
        }
        let mut content = div()
            .id("nsh-message-footer-host")
            .relative()
            .w_full()
            .h(px(Layout::COPY_ACTION_SIZE))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(surface.entity());
        for (action, focus, label) in [
            (
                Action::Copy,
                self.copy_focus.clone(),
                vega_i18n::t(if self.input.source.user {
                    "ui.copy_message"
                } else {
                    "ui.copy_final_reply"
                }),
            ),
            (
                Action::Fork,
                self.fork_focus.clone(),
                vega_i18n::t("ui.branch_in_new_chat"),
            ),
        ] {
            if !self.snapshot.enabled(action) {
                continue;
            }
            let observed = self.snapshot.clone();
            let click_observed = observed.clone();
            let keyboard_observed = observed.clone();
            let click = cx.weak_entity();
            let focused = cx.weak_entity();
            let target = focus.clone();
            content = content.child(
                div()
                    .id(match action {
                        Action::Copy => "message-copy-ax",
                        Action::Fork => "message-fork-ax",
                    })
                    .absolute()
                    .size(px(0.))
                    .track_focus(&focus)
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(label)
                    .when(
                        action == Action::Copy && self.snapshot.receipt.is_some(),
                        |node| node.aria_description(vega_i18n::t("ui.copy_requested")),
                    )
                    .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                        let _ = focused.update(cx, |host, cx| {
                            if host.valid(&observed, window, cx) {
                                window.focus(&target, cx);
                                cx.notify();
                            }
                        });
                    })
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = click.update(cx, |host, cx| {
                            host.request(&click_observed, action, false, window, cx)
                        });
                    })
                    .on_key_down(cx.listener(move |host, event: &KeyDownEvent, window, cx| {
                        let modifiers = event.keystroke.modifiers;
                        if !event.is_held
                            && !event.prefer_character_input
                            && focus.is_focused(window)
                            && !modifiers.control
                            && !modifiers.alt
                            && !modifiers.platform
                            && !modifiers.function
                            && matches!(event.keystroke.key.as_str(), "enter" | "space" | " ")
                        {
                            host.request(&keyboard_observed, action, true, window, cx);
                            cx.stop_propagation();
                        }
                    })),
            );
        }
        if let Some((_, accessible)) = &self.snapshot.source.timestamp {
            content = content.child(
                div()
                    .id("message-time-ax")
                    .absolute()
                    .size(px(0.))
                    .role(Role::Label)
                    .aria_label(accessible.clone()),
            );
        }
        Mounted {
            child: content.into_any_element(),
            lease,
            host: cx.weak_entity(),
            owner: self.input.owner.clone(),
            displayed: self.snapshot.id.clone(),
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<Mount>,
    host: WeakEntity<Host>,
    owner: WeakEntity<ConversationStream>,
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
        Some("nsh-message-footer-mount".into())
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
            if let Some(owner) = self.owner.upgrade()
                && let Some(list) =
                    super::selection_menu::mounted::list_bounds(&self.owner, window, cx)
            {
                let stream = owner.read(cx);
                let clip = bounds
                    .intersect(&window.content_mask().bounds)
                    .intersect(&list);
                if !clip.is_empty() {
                    self.lease.geometry.set(Some(Geometry {
                        bounds,
                        clip,
                        viewport: window.viewport_size(),
                        list,
                        foreground: stream.foreground_height,
                        scale: window.scale_factor(),
                        window: window.window_handle().window_id(),
                    }));
                }
            }
        }
        if previous != self.lease.geometry.get() {
            let _ = self.host.update(cx, |_, cx| cx.notify());
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

pub(super) fn message_with_copy(
    body: AnyElement,
    copy: Option<&MessageCopy>,
    actions: MessageActions<'_>,
    _: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if copy.is_none_or(|copy| !copy.has_text()) && actions.timestamp.is_none() {
        return body;
    }
    let route = Rc::new(RefCell::new(None::<WeakEntity<Host>>));
    let hovered = route.clone();
    let focus = actions.focus.cloned();
    let fork = actions
        .fork
        .filter(|(_, owner)| *owner == actions.owner)
        .map(|(request, _)| request);
    let copy = copy.filter(|copy| copy.has_text()).cloned();
    let source = Source {
        identity: actions.identity,
        copy: copy
            .as_ref()
            .map(|copy| (copy.id, copy.content_generation())),
        fork,
        pending: actions.fork_pending.map(str::to_owned),
        enabled: actions.enabled,
        fork_enabled: actions.fork_enabled,
        timestamp: actions
            .timestamp
            .map(|value| (value.message, value.accessible)),
        user: actions.user,
    };
    div()
        .id(actions.id)
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .relative()
        .flex()
        .flex_col()
        .when(!actions.user_bubble, |node| {
            node.pb(px(Layout::MESSAGE_COPY_FOOTER))
        })
        .when_some(focus, |node, focus| node.track_focus(&focus).tab_stop(true))
        .on_hover(move |value: &bool, _, cx| {
            if let Some(host) = hovered.borrow().as_ref() {
                let _ = host.update(cx, |host, cx| {
                    if host.hovered != *value {
                        host.hovered = *value;
                        cx.notify();
                    }
                });
            }
        })
        .child(body)
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(if actions.user_bubble {
                    Layout::CONVERSATION_USER_BOTTOM - Layout::MESSAGE_COPY_FOOTER
                } else {
                    0.
                }))
                .h(px(Layout::COPY_ACTION_SIZE))
                .child(Native {
                    input: Input {
                        owner: actions.owner,
                        copy,
                        source,
                        focus: actions.focus.cloned(),
                        colors: actions.colors,
                        appearance: vega_theme::theme(cx).appearance,
                        language: vega_i18n::language(),
                    },
                    route,
                }),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui_kit::test]
    fn owner_feedback_converges_without_consuming_receipts_and_real_changes_revoke_projections(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let notifications = Rc::new(Cell::new(0));
        let count = notifications.clone();
        let (owner, host, lease, copy, _feedback) = cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            let thread = vega_conversation::threads::draft_thread(None, "footer", "confirm");
            cx.set_global(crate::sidebar::OpenedThread(Some(thread.clone())));
            let copy = MessageCopy::new("Current message");
            let identity = StreamEntryIdentity {
                key: "footer-message".into(),
                message_id: Some("message".into()),
                pending_id: None,
                echo_kind: None,
                sequence: Some(1),
            };
            let owner = cx.new(|cx| {
                let mut stream = ConversationStream::new(thread, cx);
                stream.entry_identities = vec![identity.clone()];
                stream.entries = vec![super::super::StreamEntry::User {
                    lines: Vec::new(),
                    copy: copy.clone(),
                    model: Default::default(),
                }];
                stream
            });
            let state = OwnerState::capture(&owner, Some(&identity), cx);
            let source = state.source.clone().expect("actual footer source");
            let lease = Rc::new(Mount::default());
            let colors = vega_theme::theme(cx).colors;
            let host = cx.new(|cx| Host {
                input: Input {
                    owner: owner.downgrade(),
                    copy: Some(copy.clone()),
                    source: source.clone(),
                    focus: None,
                    colors,
                    appearance: state.appearance,
                    language: state.language,
                },
                reading: state.reading.clone(),
                owner_state: state,
                snapshot: Snapshot {
                    id: ProjectionId::new(),
                    epoch: owner.read(cx).reading_navigation_epoch.clone(),
                    mount: lease.id.clone(),
                    source,
                    geometry: None,
                    colors,
                    visible: true,
                    focused: Some(Action::Copy),
                    receipt: None,
                    intent: ProjectionId::new(),
                    activation: None,
                    revision: Arc::new(AtomicU64::new(0)),
                },
                published: None,
                surface: None,
                lease: Rc::downgrade(&lease),
                hovered: true,
                copy_focus: cx.focus_handle(),
                fork_focus: cx.focus_handle(),
                receipt_timer: None,
                _subscriptions: vec![Host::observe_owner(&owner, cx)],
            });
            let parent = owner.downgrade();
            let feedback = cx.observe(&host, move |_, cx| {
                count.set(count.get() + 1);
                assert!(count.get() <= 16, "footer owner feedback did not converge");
                if let Some(parent) = parent.upgrade() {
                    cx.notify(parent.entity_id());
                }
            });
            (owner, host, lease, copy, feedback)
        });
        let original = host.update(cx, |host, cx| {
            let geometry = Geometry {
                bounds: Bounds::new(point(px(0.), px(0.)), size(px(160.), px(24.))),
                clip: Bounds::new(point(px(0.), px(0.)), size(px(160.), px(24.))),
                viewport: size(px(820.), px(780.)),
                list: Bounds::new(point(px(0.), px(0.)), size(px(820.), px(780.))),
                foreground: px(0.),
                scale: 1.,
                window: WindowId::from(1),
            };
            host.snapshot.geometry = Some(geometry);
            host.snapshot.receipt = Some(cx.background_executor().now());
            host.snapshot.activation = Some((Action::Copy, true));
            host.published = Some(host.snapshot.clone());
            lease.geometry.set(Some(geometry));
            lease.painted.set(Some(geometry));
            *lease.displayed.borrow_mut() = Some(host.snapshot.id.clone());
            host.snapshot.clone()
        });
        owner.update(cx, |_, cx| cx.notify());
        assert_eq!(notifications.get(), 0);
        host.update(cx, |_, cx| cx.notify());
        assert_eq!(notifications.get(), 1);
        cx.update(|cx| {
            let host = host.read(cx);
            assert!(host.snapshot.id.matches(&original.id));
            assert!(host.snapshot.intent.matches(&original.intent));
            assert!(host.published.as_ref().unwrap().id.matches(&original.id));
            assert_eq!(host.snapshot.receipt, original.receipt);
            assert!(host.snapshot.activation == Some((Action::Copy, true)));
            assert!(host.snapshot.focused == Some(Action::Copy));
            assert!(host.hovered);
            assert!(lease.painted.get() == original.geometry);
            assert!(
                lease
                    .displayed
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .matches(&original.id)
            );
        });
        copy.replace_text("Changed current message");
        owner.update(cx, |_, cx| cx.notify());
        assert_eq!(notifications.get(), 2);
        let changed = cx.update(|cx| {
            let host = host.read(cx);
            assert!(!host.snapshot.id.matches(&original.id));
            assert!(!host.snapshot.intent.matches(&original.intent));
            assert!(host.snapshot.receipt.is_none());
            assert!(host.receipt_timer.is_none());
            assert!(host.snapshot.activation.is_none());
            assert!(host.published.is_none());
            assert!(lease.painted.get().is_none());
            assert!(lease.displayed.borrow().is_none());
            assert!(host.owner_state.source.as_ref().unwrap().copy != original.source.copy);
            host.snapshot.clone()
        });
        owner.update(cx, |_, cx| cx.notify());
        assert_eq!(notifications.get(), 2);
        cx.update(|cx| cx.set_global(crate::sidebar::OpenedThread(None)));
        owner.update(cx, |_, cx| cx.notify());
        assert_eq!(notifications.get(), 3);
        cx.update(|cx| {
            cx.set_global(crate::sidebar::OpenedThread(Some(
                owner.read(cx).thread.clone(),
            )))
        });
        owner.update(cx, |_, cx| cx.notify());
        assert_eq!(notifications.get(), 4);
        owner.update(cx, |stream, cx| {
            stream.reading_preview = true;
            cx.notify();
        });
        assert_eq!(notifications.get(), 5);
        owner.update(cx, |stream, cx| {
            stream.reading_preview = false;
            cx.notify();
        });
        assert_eq!(notifications.get(), 6);
        cx.update(|cx| {
            let host = host.read(cx);
            assert!(!host.snapshot.id.matches(&original.id));
            assert!(!host.snapshot.id.matches(&changed.id));
            assert!(host.owner_state.reading.same_binding(&host.reading));
            assert_eq!(
                host.owner_state.route.as_deref(),
                Some(owner.read(cx).thread.id.as_str())
            );
            assert!(host.snapshot.receipt.is_none());
            assert!(host.published.is_none());
            assert!(host.hovered);
        });
        owner.update(cx, |_, cx| cx.notify());
        assert_eq!(notifications.get(), 6);
        let before_pending = cx.update(|cx| host.read(cx).snapshot.clone());
        owner.update(cx, |stream, cx| {
            stream.fork_pending = Some("message".into());
            cx.notify();
        });
        assert_eq!(notifications.get(), 7);
        let pending = cx.update(|cx| {
            let host = host.read(cx);
            assert_eq!(
                host.owner_state.source.as_ref().unwrap().pending.as_deref(),
                Some("message")
            );
            assert!(!host.snapshot.id.matches(&before_pending.id));
            host.snapshot.clone()
        });
        owner.update(cx, |stream, cx| {
            stream.fork_pending = None;
            cx.notify();
        });
        assert_eq!(notifications.get(), 8);
        cx.update(|cx| {
            let host = host.read(cx);
            assert!(host.owner_state.source.as_ref().unwrap().pending.is_none());
            assert!(!host.snapshot.id.matches(&pending.id));
            assert!(!host.snapshot.id.matches(&before_pending.id));
            assert!(!host.snapshot.intent.matches(&before_pending.intent));
            assert!(host.owner_state.source.as_ref().unwrap().copy != original.source.copy);
        });
        owner.update(cx, |_, cx| cx.notify());
        assert_eq!(notifications.get(), 8);
    }
}
