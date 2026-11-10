use super::*;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{
    AccessibleAction, Element, ElementId, GlobalElementId, InspectorElementId, KeyDownEvent,
    LayoutId, RenderOnce, Size, Subscription, WeakEntity,
};
use std::rc::Weak;

mod pixels;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ButtonStyle {
    Icon,
    Label,
    Menu,
}

impl ButtonStyle {
    pub(super) fn height(self) -> f32 {
        if self == Self::Menu {
            Layout::PENDING_MESSAGE_ROW_HEIGHT
        } else {
            Layout::TITLEBAR_CONTROL_SIZE
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TextKind {
    Primary,
    Secondary,
    Reason,
    Error,
    Badge,
}

#[derive(Clone, PartialEq, Eq)]
struct Source {
    thread: Thread,
    input: gpui_kit::EntityId,
    revision: u64,
    messages: Vec<PendingMessage>,
    visible: Vec<String>,
    recovered: Vec<PendingMessage>,
    timestamps: HashMap<String, i64>,
    generation: Option<String>,
    menu: Option<String>,
    error: Option<String>,
    projection_error: Option<String>,
    default: PendingMessageBehavior,
    global_default: PendingMessageBehavior,
    active_agent: Option<(String, usize)>,
    active_response: Option<String>,
    active_sequence: Option<i64>,
    attachment_generation: u64,
    state: Vec<bool>,
    appearance: vega_theme::Appearance,
}

impl Source {
    fn capture(stream: &ConversationStream, cx: &App) -> Self {
        Self {
            thread: stream.thread.clone(),
            input: stream.input.entity_id(),
            revision: stream.input.read(cx).content_revision(),
            messages: stream.pending_queue.messages.clone(),
            visible: stream.rail_pending_ids(),
            recovered: stream.pending_ui.recovered_edit.iter().cloned().collect(),
            timestamps: stream.pending_queue.timestamps.clone(),
            generation: stream.pending_queue.committed_generation.clone(),
            menu: stream.pending_ui.menu.clone(),
            error: stream.pending_ui.error.as_ref().map(|error| error.render()),
            projection_error: stream.pending_queue.error.clone(),
            default: stream.running_message_behavior(cx),
            global_default: ConversationStream::default_running_message_behavior(cx),
            active_agent: stream.active_agent_message.clone(),
            active_response: stream.active_response_id.clone(),
            active_sequence: stream.active_agent_sequence,
            attachment_generation: stream.attachment_generation,
            state: vec![
                stream.pending_queue.paused,
                stream.pending_queue.busy,
                stream.composer_submit_pending,
                stream.composer_resume_pending,
                stream.actions.running,
                stream.actions.is_stopping(),
                stream.input.read(cx).is_composing(),
                stream.input.read(cx).composer_editor_ready(),
                stream.pending_draft_is_empty(cx),
                stream.attachment_import_pending,
                stream.permissions_revoked,
                stream.permission_change_pending(),
                stream.reading_preview,
                stream.draft_route,
                stream.pending_controls_enabled(),
                stream.has_cancellable_queue_wait(),
            ],
            appearance: theme(cx).appearance,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Content {
    Button {
        action: PendingControlAction,
        label: String,
        reason: Option<String>,
        style: ButtonStyle,
        enabled: bool,
        selected: bool,
    },
    Text(String, TextKind),
    Glyph,
    Panel(bool),
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    epoch: ProjectionId,
    source_entity: gpui_kit::EntityId,
    source: Source,
    content: Content,
    glyph: Option<String>,
    focused: bool,
    geometry: Option<Geometry>,
    colors: ThemeColors,
}

impl Snapshot {
    fn current(&self, stream: &ConversationStream, cx: &App) -> bool {
        self.epoch.matches(&stream.pending_ui.epoch) && self.source == Source::capture(stream, cx)
    }

    fn same(&self, other: &Self) -> bool {
        self.source_entity == other.source_entity
            && self.epoch.matches(&other.epoch)
            && self.source == other.source
            && self.content == other.content
            && self.glyph == other.glyph
            && self.focused == other.focused
            && self.geometry == other.geometry
    }

    fn accepts(&self, observed: &Self, displayed: bool, keyboard: bool, focused: bool) -> bool {
        displayed
            && self.id.matches(&observed.id)
            && self.same(observed)
            && observed.action().is_some()
            && (!keyboard || focused)
    }

    fn action(&self) -> Option<&PendingControlAction> {
        match &self.content {
            Content::Button {
                action,
                enabled: true,
                ..
            } => Some(action),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: Bounds<Pixels>,
    visible: Bounds<Pixels>,
    viewport: Size<Pixels>,
    foreground: Pixels,
    menu_anchor: Option<Bounds<Pixels>>,
}

#[derive(Default)]
struct Mount {
    geometry: Cell<Option<Geometry>>,
}

enum Request {
    Displayed(Box<Snapshot>),
    Activate(Box<Snapshot>),
    Height(ProjectionId, f32),
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct Chrome {
    owner: WeakEntity<ConversationStream>,
    key: String,
    content: Content,
    glyph: Option<String>,
    focus: Option<FocusHandle>,
}

impl Chrome {
    pub(super) fn button(owner: WeakEntity<ConversationStream>, button: PendingButton) -> Self {
        Self {
            owner,
            key: button.id,
            content: Content::Button {
                action: button.action,
                label: button.label.to_owned(),
                reason: button.description.map(str::to_owned),
                style: button.style,
                enabled: button.enabled,
                selected: button.selected,
            },
            glyph: crate::icons::nsh_source(button.glyph),
            focus: Some(button.focus),
        }
    }

    pub(super) fn text(
        owner: WeakEntity<ConversationStream>,
        key: impl Into<String>,
        text: String,
        kind: TextKind,
    ) -> Self {
        Self {
            owner,
            key: key.into(),
            content: Content::Text(text, kind),
            glyph: None,
            focus: None,
        }
    }

    pub(super) fn glyph(owner: WeakEntity<ConversationStream>, key: String, icon: Icon) -> Self {
        Self {
            owner,
            key,
            content: Content::Glyph,
            glyph: crate::icons::nsh_source(icon),
            focus: None,
        }
    }

    pub(super) fn panel(owner: WeakEntity<ConversationStream>, key: &str, menu: bool) -> Self {
        Self {
            owner,
            key: key.into(),
            content: Content::Panel(menu),
            glyph: None,
            focus: None,
        }
    }
}

impl RenderOnce for Chrome {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let snapshot = Snapshot {
            id: ProjectionId::new(),
            epoch: owner.read(cx).pending_ui.epoch.clone(),
            source_entity: owner.entity_id(),
            source: Source::capture(owner.read(cx), cx),
            content: self.content,
            glyph: self.glyph,
            focused: self
                .focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window)),
            geometry: None,
            colors: theme(cx).colors,
        };
        let width = if let Content::Button { label, style, .. } = &snapshot.content {
            Some(match style {
                ButtonStyle::Icon => Layout::TITLEBAR_CONTROL_SIZE,
                ButtonStyle::Menu => Layout::TASK_MENU_WIDTH - 8.,
                ButtonStyle::Label => {
                    let label: gpui_kit::SharedString = label.clone().into();
                    let run = window.text_style().to_run(label.len());
                    f32::from(
                        window
                            .text_system()
                            .shape_line(label, px(Typography::BODY), &[run], None)
                            .width,
                    ) + Layout::ICON_SIZE
                        + 16.
                }
            })
        } else {
            None
        };
        let key = format!("nsh-pending-{}-{}", owner.entity_id(), self.key);
        let state = window.use_keyed_state(key, cx, |window, cx| {
            let mut subscriptions = vec![cx.on_release(|host: &mut Host, cx| host.stop(cx))];
            if let Some(focus) = &self.focus {
                subscriptions.push(cx.on_focus(focus, window, |host: &mut Host, window, cx| {
                    host.displayed = false;
                    host.snapshot.id = ProjectionId::new();
                    if let Some(geometry) =
                        host.lease.upgrade().and_then(|lease| lease.geometry.get())
                    {
                        window.request_autoscroll(geometry.bounds);
                    }
                    cx.notify();
                }));
                subscriptions.push(cx.on_blur(focus, window, |host: &mut Host, _, cx| {
                    host.displayed = false;
                    host.snapshot.id = ProjectionId::new();
                    cx.notify();
                }));
            }
            Host {
                owner: self.owner.clone(),
                snapshot: snapshot.clone(),
                surface: None,
                published: None,
                focus: self.focus.clone(),
                lease: Weak::new(),
                displayed: false,
                height: None,
                width,
                _subscriptions: subscriptions,
            }
        });
        state.update(cx, |host, cx| {
            let mut incoming = snapshot;
            incoming.geometry = host.snapshot.geometry;
            if host.snapshot.same(&incoming) {
                incoming.id = host.snapshot.id.clone();
            } else {
                host.displayed = false;
                host.height = None;
                cx.notify();
            }
            host.snapshot = incoming;
            host.focus = self.focus;
            host.width = width;
        });
        state.into_any_element()
    }
}

struct Host {
    owner: WeakEntity<ConversationStream>,
    snapshot: Snapshot,
    surface: Option<Surface<Snapshot, Request>>,
    published: Option<Snapshot>,
    focus: Option<FocusHandle>,
    lease: Weak<Mount>,
    displayed: bool,
    height: Option<f32>,
    width: Option<f32>,
    _subscriptions: Vec<Subscription>,
}

impl Host {
    fn stop(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.published = None;
        self.snapshot.id = ProjectionId::new();
        self.displayed = false;
        self.lease = Weak::new();
    }

    fn valid(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        let Some(geometry) = self.lease.upgrade().and_then(|mount| mount.geometry.get()) else {
            return false;
        };
        owner.entity_id() == observed.source_entity
            && window.is_window_active()
            && self.snapshot.id.matches(&observed.id)
            && self.snapshot.same(observed)
            && observed.current(owner.read(cx), cx)
            && observed.geometry == Some(geometry)
            && geometry.viewport == window.viewport_size()
            && geometry.foreground == owner.read(cx).foreground_height
            && geometry.menu_anchor
                == owner
                    .read(cx)
                    .pending_ui
                    .menu
                    .as_ref()
                    .and_then(|id| owner.read(cx).pending_ui.items.get(id))
                    .and_then(|item| item.bounds.get())
            && geometry.bounds == geometry.visible
            && !geometry.bounds.is_empty()
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
    }

    fn activate(
        &mut self,
        observed: &Snapshot,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.valid(observed, window, cx)
            || !self.snapshot.accepts(
                observed,
                self.displayed,
                keyboard,
                self.focus
                    .as_ref()
                    .is_some_and(|focus| focus.is_focused(window)),
            )
        {
            return;
        }
        let Some(action) = observed.action().cloned() else {
            return;
        };
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        self.displayed = false;
        owner.update(cx, |stream, cx| {
            if observed.current(stream, cx) && !stream.input.read(cx).is_composing() {
                stream.pending_ui.epoch = ProjectionId::new();
                stream.activate_pending_control(&action, window, cx);
            }
        });
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .owner
            .upgrade()
            .is_none_or(|owner| owner.read(cx).input.read(cx).is_composing())
        {
            return;
        }
        if matches!(
            crate::nsh::keyboard::Key::from_event(event),
            crate::nsh::keyboard::Key::Activate
        ) {
            let observed = self.snapshot.clone();
            self.activate(&observed, true, window, cx);
            cx.stop_propagation();
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new_transparent(self.snapshot.clone(), pixels::render, window, cx);
        surface.focus_handle(cx).tab_stop(false);
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |host, window, cx| match request {
                        Request::Displayed(observed) => {
                            if host.valid(&observed, window, cx) {
                                host.displayed = true;
                            }
                        }
                        Request::Activate(observed) => host.activate(&observed, false, window, cx),
                        Request::Height(id, height) => {
                            if id.matches(&host.snapshot.id)
                                && height.is_finite()
                                && height > 0.
                                && host.height.is_none_or(|old| (old - height).abs() > 0.5)
                            {
                                host.height = Some(height);
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        self.surface = Some(surface);
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            self.stop(cx);
            return div().into_any_element();
        };
        if !self.snapshot.current(owner.read(cx), cx) {
            self.stop(cx);
            return div().into_any_element();
        }
        let lease = self.lease.upgrade().unwrap_or_else(|| {
            self.stop(cx);
            Rc::new(Mount::default())
        });
        self.lease = Rc::downgrade(&lease);
        let focused = self
            .focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window));
        if self.snapshot.geometry != lease.geometry.get() || self.snapshot.focused != focused {
            self.snapshot.geometry = lease.geometry.get();
            self.snapshot.focused = focused;
            self.snapshot.id = ProjectionId::new();
            self.displayed = false;
        }
        self.mount(window, cx);
        let Some(surface) = &mut self.surface else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        if self
            .published
            .as_ref()
            .is_none_or(|old| !old.id.matches(&self.snapshot.id))
        {
            surface.publish(self.snapshot.clone());
            self.published = Some(self.snapshot.clone());
        }
        let mut node = div()
            .id("nsh-pending-host")
            .relative()
            .w_full()
            .h_full()
            .child(surface.entity());
        if let Some(width) = self.width {
            node = node.w(px(width));
        }
        match &self.snapshot.content {
            Content::Button { style, .. } => {
                node = node.h(px(self
                    .height
                    .unwrap_or(style.height())
                    .max(style.height())))
            }
            Content::Text(_, TextKind::Error) => {
                node = node.h(px(self.height.unwrap_or(Typography::SIDEBAR_LINE_HEIGHT)))
            }
            _ => {}
        }
        if let (
            Some(focus),
            Content::Button {
                label,
                reason,
                enabled,
                ..
            },
        ) = (&self.focus, &self.snapshot.content)
        {
            let focus = focus.clone().tab_stop(*enabled);
            let description = reason.clone();
            let tooltip = reason.clone().unwrap_or_else(|| label.clone());
            let observed = self.snapshot.clone();
            let click = cx.weak_entity();
            let accessibility = observed.clone();
            let focus_owner = cx.weak_entity();
            node = node
                .role(gpui_kit::Role::Button)
                .aria_label(label.clone())
                .when_some(description, |node, description| {
                    node.aria_description(description)
                })
                .track_focus(&focus)
                .key_context("PendingMessageControl")
                .capture_key_down(cx.listener(Self::key))
                .on_action(
                    cx.listener(|host, _: &ActivatePendingMessageControl, window, cx| {
                        host.activate(&host.snapshot.clone(), true, window, cx);
                        cx.stop_propagation();
                    }),
                )
                .tooltip(move |_, cx| crate::icons::tooltip(tooltip.clone(), cx))
                .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                    let _ = focus_owner.update(cx, |host, cx| {
                        if host.snapshot.id.matches(&observed.id)
                            && observed.action().is_some()
                            && observed.current(owner.read(cx), cx)
                            && let Some(focus) = &host.focus
                        {
                            focus.focus(window, cx);
                        }
                    });
                })
                .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                    let _ = click.update(cx, |host, cx| {
                        host.activate(&accessibility, false, window, cx)
                    });
                });
        }
        Mounted {
            child: node.into_any_element(),
            lease,
            host: cx.weak_entity(),
            owner: self.owner.clone(),
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<Mount>,
    host: WeakEntity<Host>,
    owner: WeakEntity<ConversationStream>,
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
        Some("nsh-pending-mounted".into())
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
            if let Some(owner) = self.owner.upgrade() {
                self.lease.geometry.set(Some(Geometry {
                    bounds,
                    visible: bounds.intersect(&window.content_mask().bounds),
                    viewport: window.viewport_size(),
                    foreground: owner.read(cx).foreground_height,
                    menu_anchor: owner
                        .read(cx)
                        .pending_ui
                        .menu
                        .as_ref()
                        .and_then(|id| owner.read(cx).pending_ui.items.get(id))
                        .and_then(|item| item.bounds.get()),
                }));
            }
        }
        if previous != self.lease.geometry.get() {
            let _ = self.host.update(cx, |host, cx| {
                host.displayed = false;
                host.snapshot.id = ProjectionId::new();
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
    }
}
