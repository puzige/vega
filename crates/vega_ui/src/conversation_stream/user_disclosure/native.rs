use super::{DisclosureStamp, MessageCopy};
use crate::conversation_stream::model::StreamEntryIdentity;
use crate::conversation_stream::selection_menu::ReadingOwner;
use crate::conversation_stream::{ConversationStream, StreamEntry};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use vega_theme::{Layout, ThemeColors, Typography};

mod pixels;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Intent {
    Activate(bool),
    Focus,
}

#[derive(Clone, PartialEq, Eq)]
struct Source {
    identity: StreamEntryIdentity,
    stamp: DisclosureStamp,
}

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    list: Bounds<Pixels>,
    viewport: Size<Pixels>,
    foreground: Pixels,
    scroll: (usize, Pixels),
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
    epoch: ProjectionId,
    mount: ProjectionId,
    frame: ProjectionId,
    source: Source,
    geometry: Option<Geometry>,
    colors: ThemeColors,
    label: String,
    focused: bool,
    activation: Option<Intent>,
    revision: Arc<AtomicU64>,
}

struct Input {
    owner: WeakEntity<ConversationStream>,
    copy: MessageCopy,
    source: Source,
    colors: ThemeColors,
    focus: FocusHandle,
    language: &'static str,
    appearance: vega_theme::Appearance,
}

impl Input {
    fn source_index(&self, stream: &ConversationStream) -> Option<usize> {
        let index = stream
            .entry_identities
            .iter()
            .position(|identity| identity == &self.source.identity)?;
        match stream.entries.get(index)? {
            StreamEntry::User { copy, .. }
                if self.source.stamp.matches(copy)
                    && self.source.stamp.matches(&self.copy)
                    && Rc::ptr_eq(&copy.user_disclosure, &self.copy.user_disclosure) =>
            {
                Some(index)
            }
            _ => None,
        }
    }

    fn current(
        &self,
        reading: &ReadingOwner,
        interactive: bool,
        window: &Window,
        cx: &App,
    ) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        let stream = owner.read(cx);
        let Some(index) = self.source_index(stream) else {
            return false;
        };
        let state = self.copy.user_disclosure.borrow();
        state.generation == self.source.stamp.generation
            && state
                .preview
                .as_ref()
                .is_some_and(|preview| preview.collapsible)
            && !stream.ui_presentations_suspended
            && self.language == vega_i18n::language()
            && self.appearance == vega_theme::theme(cx).appearance
            && (!interactive
                || (window.is_visible()
                    && reading.root_current(window, cx)
                    && (crate::conversation_stream::selection_menu::mounted::list_bounds(
                        &self.owner,
                        window,
                        cx,
                    )
                    .is_some()
                        || stream.list.bounds_for_item(index).is_some_and(|bounds| {
                            !bounds.intersect(&stream.list.viewport_bounds()).is_empty()
                        }))))
            && cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| {
                    route
                        .0
                        .as_ref()
                        .is_some_and(|thread| thread.id == stream.thread.id)
                })
    }

    fn list(&self, window: &Window, cx: &App) -> Option<Bounds<Pixels>> {
        crate::conversation_stream::selection_menu::mounted::list_bounds(&self.owner, window, cx)
            .or_else(|| {
                self.owner
                    .upgrade()
                    .map(|owner| owner.read(cx).list.viewport_bounds())
            })
    }
}

struct Host {
    input: Input,
    reading: ReadingOwner,
    snapshot: Snapshot,
    published: Option<Snapshot>,
    surface: Option<Surface<Snapshot, Request>>,
    lease: Weak<Mount>,
    _subscriptions: Vec<Subscription>,
}

struct Witness {
    revision: u64,
    control: [f32; 4],
    root: [f32; 4],
}

enum Request {
    Activate(Box<Snapshot>, Witness),
}

impl Host {
    fn invalidate(&mut self) {
        self.snapshot.id = ProjectionId::new();
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.activation = None;
    }

    fn stop(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.published = None;
        self.invalidate();
        if let Some(lease) = self.lease.upgrade() {
            lease.painted.set(None);
            *lease.displayed.borrow_mut() = None;
            *lease.frame.borrow_mut() = ProjectionId::new();
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
        let scroll = stream.list.logical_scroll_top();
        self.snapshot.id.matches(&observed.id)
            && self.snapshot.intent.matches(&observed.intent)
            && self.snapshot.source == observed.source
            && self.snapshot.epoch.matches(&observed.epoch)
            && observed.epoch.matches(&stream.reading_navigation_epoch)
            && observed.mount.matches(&lease.id)
            && observed.frame.matches(&lease.frame.borrow())
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
                    && published.frame.matches(&observed.frame)
                    && published.source == observed.source
                    && published.geometry == observed.geometry
            })
            && geometry.viewport == window.viewport_size()
            && geometry.scale == window.scale_factor()
            && geometry.window == window.window_handle().window_id()
            && geometry.foreground == stream.foreground_height
            && geometry.scroll == (scroll.item_ix, scroll.offset_in_item)
            && self.input.list(window, cx) == Some(geometry.list)
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
        intent: Intent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !self.valid(observed, window, cx) {
            return;
        }
        self.snapshot.intent = ProjectionId::new();
        self.snapshot.activation = Some(intent);
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
            || (observed.activation == Some(Intent::Activate(true))
                && !self.input.focus.is_focused(window))
            || observed.revision.load(Ordering::Acquire) != witness.revision
            || !observed
                .geometry
                .is_some_and(|geometry| measured_visible(&witness, geometry))
        {
            return;
        }
        if observed.activation == Some(Intent::Focus) {
            self.invalidate();
            window.focus(&self.input.focus, cx);
            cx.notify();
            return;
        }
        let changed = self
            .input
            .owner
            .update(cx, |stream, cx| {
                let Some(index) = self.input.source_index(stream) else {
                    return false;
                };
                if !self.input.source.stamp.toggle(&self.input.copy) {
                    return false;
                }
                stream.invalidate_item(Some(index));
                cx.notify();
                true
            })
            .unwrap_or(false);
        if changed {
            self.invalidate();
            window.refresh();
            cx.notify();
        }
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

fn measured_visible(witness: &Witness, geometry: Geometry) -> bool {
    let control = witness.control;
    let root = witness.root;
    let width = f32::from(geometry.bounds.size.width);
    let height = f32::from(geometry.bounds.size.height);
    pixels::contained(control, root)
        && root[0].abs() < 0.5
        && root[1].abs() < 0.5
        && (root[2] - width).abs() < 0.5
        && (root[3] - height).abs() < 0.5
        && geometry.bounds.origin.x + px(control[0]) >= geometry.clip.left()
        && geometry.bounds.origin.y + px(control[1]) >= geometry.clip.top()
        && geometry.bounds.origin.x + px(control[0] + control[2]) <= geometry.clip.right()
        && geometry.bounds.origin.y + px(control[1] + control[3]) <= geometry.clip.bottom()
}

#[derive(gpui_kit::IntoElement)]
struct Native {
    input: Input,
}

impl RenderOnce for Native {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.input.owner.upgrade() else {
            return div().into_any_element();
        };
        let reading = ReadingOwner::message(
            owner.read(cx),
            self.input.owner.clone(),
            Some(self.input.source.identity.clone()),
        );
        let epoch = owner.read(cx).reading_navigation_epoch.clone();
        let key = format!(
            "nsh-user-disclosure-{}-{}",
            owner.entity_id(),
            self.input.copy.id
        );
        let make_host = |window: &mut Window, cx: &mut Context<Host>| {
            let focus = self.input.focus.clone();
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
                cx.on_focus_in(&focus, window, |host: &mut Host, _, cx| {
                    host.invalidate();
                    cx.notify();
                }),
                cx.on_focus_out(&focus, window, |host: &mut Host, _, _, cx| {
                    host.invalidate();
                    cx.notify();
                }),
                cx.observe(&owner, |host: &mut Host, _, cx| {
                    host.invalidate();
                    cx.notify();
                }),
            ];
            Host {
                snapshot: Snapshot {
                    id: ProjectionId::new(),
                    intent: ProjectionId::new(),
                    epoch: epoch.clone(),
                    mount: ProjectionId::new(),
                    frame: ProjectionId::new(),
                    source: self.input.source.clone(),
                    geometry: None,
                    colors: self.input.colors,
                    label: label(self.input.source.stamp.expanded),
                    focused: false,
                    activation: None,
                    revision: Arc::new(AtomicU64::new(0)),
                },
                input: Input {
                    owner: self.input.owner.clone(),
                    copy: self.input.copy.clone(),
                    source: self.input.source.clone(),
                    colors: self.input.colors,
                    focus: focus.clone(),
                    language: self.input.language,
                    appearance: self.input.appearance,
                },
                reading: reading.clone(),
                published: None,
                surface: None,
                lease: Weak::new(),
                _subscriptions: subscriptions,
            }
        };
        let host = window.with_global_id(key.into(), |id, window| {
            window.with_element_state::<gpui_kit::Entity<Host>, _>(id, |previous, window| {
                let host = previous.unwrap_or_else(|| cx.new(|cx| make_host(window, cx)));
                (host.clone(), host)
            })
        });
        host.update(cx, |host, cx| {
            if host.input.source != self.input.source
                || !Rc::ptr_eq(
                    &host.input.copy.user_disclosure,
                    &self.input.copy.user_disclosure,
                )
                || !host.snapshot.epoch.matches(&epoch)
                || !host.reading.same_binding(&reading)
                || palette(host.input.colors) != palette(self.input.colors)
                || host.input.appearance != self.input.appearance
                || host.input.language != self.input.language
            {
                host.stop(cx);
                host.snapshot.source = self.input.source.clone();
                host.snapshot.epoch = epoch;
                host.snapshot.colors = self.input.colors;
                host.snapshot.label = label(self.input.source.stamp.expanded);
            }
            host.input = self.input;
            host.reading = reading;
        });
        host.into_any_element()
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = px(Typography::CONVERSATION * Typography::CONVERSATION_USER_LINE_HEIGHT);
        if !self.input.current(&self.reading, false, window, cx) {
            self.stop(cx);
            return div().w_full().h(height).into_any_element();
        }
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
        self.snapshot.focused = self.input.focus.is_focused(window);
        self.mount(window, cx);
        let Some(surface) = &mut self.surface else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        if self.published.as_ref().is_none_or(|previous| {
            !previous.id.matches(&self.snapshot.id)
                || !previous.intent.matches(&self.snapshot.intent)
                || previous.focused != self.snapshot.focused
        }) {
            surface.publish(self.snapshot.clone());
            self.published = Some(self.snapshot.clone());
        }
        let observed = self.snapshot.clone();
        let click_observed = observed.clone();
        let keyboard_observed = observed.clone();
        let focused = cx.weak_entity();
        let click = cx.weak_entity();
        let focus = self.input.focus.clone();
        let content = div()
            .id("nsh-user-disclosure-host")
            .relative()
            .w_full()
            .h(height)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(surface.entity())
            .child(
                div()
                    .id("user-disclosure-ax")
                    .absolute()
                    .size(px(0.))
                    .track_focus(&focus)
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(self.snapshot.label.clone())
                    .aria_expanded(self.snapshot.source.stamp.expanded)
                    .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                        let _ = focused.update(cx, |host, cx| {
                            host.request(&observed, Intent::Focus, window, cx);
                        });
                    })
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = click.update(cx, |host, cx| {
                            host.request(&click_observed, Intent::Activate(false), window, cx)
                        });
                    })
                    .on_key_down(cx.listener(move |host, event: &KeyDownEvent, window, cx| {
                        let modifiers = event.keystroke.modifiers;
                        if !event.is_held
                            && !event.prefer_character_input
                            && focus.is_focused(window)
                            && !modifiers.control
                            && !modifiers.alt
                            && !modifiers.shift
                            && !modifiers.platform
                            && !modifiers.function
                            && matches!(event.keystroke.key.as_str(), "enter" | "space" | " ")
                        {
                            host.request(&keyboard_observed, Intent::Activate(true), window, cx);
                            cx.stop_propagation();
                        }
                    })),
            );
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

struct PrepaintClip {
    bounds: Bounds<Pixels>,
    list: Option<Bounds<Pixels>>,
}

impl Element for Mounted {
    type RequestLayoutState = ();
    type PrepaintState = Option<PrepaintClip>;
    fn id(&self) -> Option<ElementId> {
        Some("nsh-user-disclosure-mount".into())
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
    ) -> Self::PrepaintState {
        self.lease.painted.set(None);
        *self.lease.displayed.borrow_mut() = None;
        let clip = id.map(|id| {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.lease.clone()));
            PrepaintClip {
                bounds: bounds.intersect(&window.content_mask().bounds),
                list: crate::conversation_stream::selection_menu::mounted::list_bounds(
                    &self.owner,
                    window,
                    cx,
                ),
            }
        });
        self.child.prepaint(window, cx);
        clip
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        clip: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
        let geometry = clip.as_ref().and_then(|clip| {
            let owner = self.owner.upgrade()?;
            let stream = owner.read(cx);
            let list = clip.list.unwrap_or_else(|| stream.list.viewport_bounds());
            let visible = clip.bounds.intersect(&list);
            if visible.is_empty() {
                return None;
            }
            let scroll = stream.list.logical_scroll_top();
            Some(Geometry {
                bounds,
                clip: visible,
                list,
                viewport: window.viewport_size(),
                foreground: stream.foreground_height,
                scroll: (scroll.item_ix, scroll.offset_in_item),
                scale: window.scale_factor(),
                window: window.window_handle().window_id(),
            })
        });
        if self.lease.geometry.replace(geometry) != geometry {
            *self.lease.frame.borrow_mut() = ProjectionId::new();
            let _ = self.host.update(cx, |host, cx| {
                host.invalidate();
                cx.notify();
            });
        }
        self.lease.painted.set(geometry);
        *self.lease.displayed.borrow_mut() = Some(self.displayed.clone());
    }
}

fn palette(colors: ThemeColors) -> [Rgba; 3] {
    [
        colors.conversation_user,
        colors.bg_active_alpha,
        colors.bg_hover,
    ]
}

pub(super) fn label(expanded: bool) -> String {
    vega_i18n::t(if expanded {
        "ui.show_less"
    } else {
        "ui.show_more"
    })
    .to_owned()
}

pub(in crate::conversation_stream) fn control(
    copy: &MessageCopy,
    stream: &Option<WeakEntity<ConversationStream>>,
    identity: Option<&StreamEntryIdentity>,
    colors: &ThemeColors,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (Some(owner), Some(identity)) = (stream.as_ref(), identity) else {
        return div().into_any_element();
    };
    let id: SharedString = format!("user-message-disclosure-{}", copy.id).into();
    let focus = crate::disclosure::focus(id, window, cx);
    div()
        .mt(px(Layout::USER_MESSAGE_TOGGLE_GAP))
        .w_full()
        .flex()
        .justify_start()
        .child(Native {
            input: Input {
                owner: owner.clone(),
                copy: copy.clone(),
                source: Source {
                    identity: identity.clone(),
                    stamp: DisclosureStamp::capture(copy),
                },
                colors: *colors,
                focus,
                language: vega_i18n::language(),
                appearance: vega_theme::theme(cx).appearance,
            },
        })
        .into_any_element()
}
