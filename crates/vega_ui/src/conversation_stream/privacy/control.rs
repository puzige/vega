use super::*;
use crate::nsh::{OrderedCommands, ProjectionId, Surface};
use crate::sidebar::SelectedProject;
use gpui_kit::{
    AccessibleAction, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId,
    RenderOnce, Size, WeakEntity,
};
use ns::{
    Accessible as _, Element as _, Interactive as _, IntoElement as _, ParentElement as _,
    Styled as _,
};
use nsh_core::prelude as ns;
use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Weak as RowWeak};
use tokio::sync::mpsc;

#[derive(Clone, PartialEq, Eq)]
struct Source {
    thread: Thread,
    input: gpui_kit::EntityId,
    revision: u64,
    opened: bool,
    pending: bool,
    confirming: bool,
    review: Option<PrivacyReview>,
    details: Vec<usize>,
    error: Option<String>,
    preview: bool,
    revoked: bool,
    permission_pending: bool,
}

impl Source {
    fn capture(stream: &ConversationStream, cx: &App) -> Self {
        let mut details = stream.privacy.details.iter().copied().collect::<Vec<_>>();
        details.sort_unstable();
        Self {
            thread: stream.thread.clone(),
            input: stream.input.entity_id(),
            revision: stream.input.read(cx).content_revision(),
            opened: stream.privacy.opened,
            pending: stream.privacy.pending,
            confirming: stream.privacy.confirming,
            review: stream.privacy.review.clone(),
            details,
            error: stream.privacy.error.as_ref().map(|error| error.render()),
            preview: stream.reading_preview,
            revoked: stream.permissions_revoked,
            permission_pending: stream.permission_change_pending(),
        }
    }
}

#[derive(Clone)]
pub(super) struct Binding {
    owner: WeakEntity<ConversationStream>,
    source: Source,
    root: super::super::selection_menu::ReadingOwner,
}

impl Binding {
    pub(super) fn capture(stream: &ConversationStream, cx: &Context<ConversationStream>) -> Self {
        let owner = cx.weak_entity();
        Self {
            root: super::super::selection_menu::ReadingOwner::message(stream, owner.clone(), None),
            source: Source::capture(stream, cx),
            owner,
        }
    }

    fn same(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.source == other.source
            && self.root.same_binding(&other.root)
    }

    fn current(&self, window: &Window, cx: &App) -> bool {
        self.root.root_current(window, cx)
            && self
                .owner
                .upgrade()
                .is_some_and(|owner| Self::source_current(&self.source, owner.read(cx), cx))
    }

    fn source_current(source: &Source, stream: &ConversationStream, cx: &App) -> bool {
        source == &Source::capture(stream, cx)
    }
}

type Activate = Rc<dyn Fn(&(), &mut Window, &mut App)>;

#[derive(gpui_kit::IntoElement)]
pub(super) struct Control {
    pub id: SharedString,
    pub text: String,
    pub disabled: bool,
    pub button: bool,
    pub colors: vega_theme::ThemeColors,
    pub foreground: gpui_kit::Rgba,
    pub binding: Option<Binding>,
    pub activate: Option<Activate>,
}

#[derive(Clone, PartialEq, Eq)]
struct Environment {
    project: Option<String>,
    settings: bool,
    language: &'static str,
}

impl Environment {
    fn capture(cx: &App) -> Self {
        Self {
            project: cx
                .try_global::<SelectedProject>()
                .and_then(|selected| selected.0.clone()),
            settings: cx
                .try_global::<SettingsOpen>()
                .is_some_and(|settings| settings.0),
            language: vega_i18n::language(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    viewport: Size<Pixels>,
    scale: f32,
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

#[derive(Clone)]
struct PixelsState {
    id: ProjectionId,
    text: String,
    disabled: bool,
    button: bool,
    focused: bool,
    colors: vega_theme::ThemeColors,
    foreground: gpui_kit::Rgba,
}

fn same_palette(left: vega_theme::ThemeColors, right: vega_theme::ThemeColors) -> bool {
    left.bg_base == right.bg_base
        && left.bg_hover == right.bg_hover
        && left.bg_active == right.bg_active
        && left.accent == right.accent
        && left.text_secondary == right.text_secondary
        && left.text_tertiary == right.text_tertiary
}

enum Request {
    Displayed(ProjectionId, ns::Bounds<ns::Points>, RowWeak<()>),
    Activate(ProjectionId, RowWeak<()>),
}

struct Displayed {
    bounds: ns::Bounds<ns::Points>,
    row: RowWeak<()>,
}

struct Host {
    snapshot: PixelsState,
    binding: Option<Binding>,
    environment: Environment,
    activate: Option<Activate>,
    surface: Option<Surface<PixelsState, Request>>,
    displayed: Option<Displayed>,
    lease: Weak<Mount>,
    focus: gpui_kit::FocusHandle,
    height: f32,
    geometry: Option<Geometry>,
    published: Option<ProjectionId>,
    recovery: bool,
    _subscriptions: Vec<gpui_kit::Subscription>,
}

impl RenderOnce for Control {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let environment = Environment::capture(cx);
        let host = window.use_keyed_state(self.id, cx, |window, cx| {
            let focus = cx.focus_handle().tab_stop(self.button && !self.disabled);
            let mut subscriptions = vec![
                cx.on_focus_in(&focus, window, |host: &mut Host, window, cx| {
                    if let Some(geometry) = host.geometry {
                        window.request_autoscroll(geometry.bounds);
                    }
                    host.invalidate();
                    cx.notify();
                }),
                cx.on_focus_out(&focus, window, |host: &mut Host, _, _, cx| {
                    host.invalidate();
                    cx.notify();
                }),
                cx.observe_window_activation(window, |host: &mut Host, _, cx| {
                    host.invalidate();
                    cx.notify();
                }),
                cx.observe_window_visibility(window, |host: &mut Host, _, _, cx| {
                    host.invalidate();
                    cx.notify();
                }),
                cx.on_release(|host: &mut Host, cx| host.stop(cx)),
            ];
            if let Some(binding) = &self.binding
                && let Some(owner) = binding.owner.upgrade()
            {
                subscriptions.push(cx.observe(&owner, |_: &mut Host, _, cx| cx.notify()));
            }
            Host {
                snapshot: PixelsState {
                    id: ProjectionId::new(),
                    text: self.text.clone(),
                    disabled: self.disabled,
                    button: self.button,
                    focused: false,
                    colors: self.colors,
                    foreground: self.foreground,
                },
                binding: self.binding.clone(),
                environment: environment.clone(),
                activate: self.activate.clone(),
                surface: None,
                displayed: None,
                lease: Weak::new(),
                focus,
                height: Layout::SETTINGS_CONTROL_MIN_HEIGHT,
                geometry: None,
                published: None,
                recovery: false,
                _subscriptions: subscriptions,
            }
        });
        host.update(cx, |host, cx| {
            let binding_changed = match (&host.binding, &self.binding) {
                (Some(old), Some(new)) => !old.same(new),
                (None, None) => false,
                _ => true,
            };
            if binding_changed
                || host.environment != environment
                || host.snapshot.text != self.text
                || host.snapshot.disabled != self.disabled
                || host.snapshot.button != self.button
                || host.snapshot.foreground != self.foreground
                || !same_palette(host.snapshot.colors, self.colors)
            {
                host.invalidate();
                host.recovery = false;
            }
            host.binding = self.binding;
            host.environment = environment;
            host.activate = self.activate;
            host.snapshot.text = self.text;
            host.snapshot.disabled = self.disabled;
            host.snapshot.button = self.button;
            host.snapshot.colors = self.colors;
            host.snapshot.foreground = self.foreground;
            if host.lease.upgrade().is_none() {
                host.stop(cx);
                host.invalidate();
            }
        });
        host.into_any_element()
    }
}

impl Host {
    fn stop(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.displayed = None;
        self.published = None;
        self.lease = Weak::new();
        self.geometry = None;
    }

    fn invalidate(&mut self) {
        self.snapshot.id = ProjectionId::new();
        self.displayed = None;
    }

    fn live(&self, id: &ProjectionId, window: &Window, cx: &App) -> bool {
        self.snapshot.id.matches(id)
            && self.environment == Environment::capture(cx)
            && window.is_visible()
            && window.is_window_active()
            && self
                .binding
                .as_ref()
                .is_some_and(|binding| binding.current(window, cx))
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
            && self
                .lease
                .upgrade()
                .and_then(|lease| lease.geometry.get())
                .is_some_and(|geometry| {
                    Some(geometry) == self.geometry
                        && geometry.viewport == window.viewport_size()
                        && geometry.scale == window.scale_factor()
                        && !geometry.clip.is_empty()
                })
    }

    fn native_bounds(&self, displayed: &Displayed) -> Option<Bounds<Pixels>> {
        let geometry = self.geometry?;
        let bounds = Bounds {
            origin: gpui_kit::point(
                geometry.bounds.origin.x + px(displayed.bounds.origin.x.0),
                geometry.bounds.origin.y + px(displayed.bounds.origin.y.0),
            ),
            size: gpui_kit::size(
                px(displayed.bounds.size.width.0),
                px(displayed.bounds.size.height.0),
            ),
        };
        let finite = [
            displayed.bounds.origin.x.0,
            displayed.bounds.origin.y.0,
            displayed.bounds.size.width.0,
            displayed.bounds.size.height.0,
        ]
        .iter()
        .all(|value| value.is_finite());
        (finite && !bounds.is_empty() && bounds.intersect(&geometry.clip) == bounds)
            .then_some(bounds)
    }

    fn actionable(&self, id: &ProjectionId, row: &RowWeak<()>, window: &Window, cx: &App) -> bool {
        self.live(id, window, cx)
            && (!self.snapshot.disabled || self.recovery)
            && self.displayed.as_ref().is_some_and(|displayed| {
                row.upgrade().is_some()
                    && RowWeak::ptr_eq(&displayed.row, row)
                    && self.native_bounds(displayed).is_some()
            })
    }

    fn invoke(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recovery {
            self.stop(cx);
            self.recovery = false;
            self.invalidate();
            cx.notify();
        } else if let Some(activate) = self.activate.clone() {
            self.invalidate();
            cx.notify();
            window.focus(&self.focus, cx);
            activate(&(), window, cx);
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let mut snapshot = self.snapshot.clone();
        if self.recovery {
            snapshot.text = vega_i18n::t("ui.reload_view").to_owned();
            snapshot.disabled = false;
            snapshot.button = true;
        }
        let (surface, mut requests) = Surface::new_transparent(snapshot, pixels, window, cx);
        surface.focus_handle(cx).tab_stop(false);
        let mut errors = surface.errors();
        let failed_view = surface.entity().entity_id();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |host, window, cx| match request {
                        Request::Displayed(id, bounds, row)
                            if host.live(&id, window, cx) && row.upgrade().is_some() =>
                        {
                            let height = bounds.size.height.0;
                            if height.is_finite() && height > 0. {
                                if (host.height - height).abs() > 0.5 {
                                    host.height = height;
                                    host.invalidate();
                                } else {
                                    host.displayed = Some(Displayed { bounds, row });
                                }
                                cx.notify();
                            }
                        }
                        Request::Activate(id, row) if host.actionable(&id, &row, window, cx) => {
                            host.invoke(window, cx)
                        }
                        _ => {}
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
                    let _ = this.update(cx, |host, cx| {
                        if host
                            .surface
                            .as_ref()
                            .is_none_or(|surface| surface.entity().entity_id() != failed_view)
                        {
                            return;
                        }
                        host.stop(cx);
                        host.recovery = true;
                        host.invalidate();
                        cx.notify();
                    });
                    break;
                }
            }
        })
        .detach();
        self.surface = Some(surface);
        self.published = None;
    }

    fn key(&mut self, event: &gpui_kit::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if !self.focus.is_focused(window)
            || event.prefer_character_input
            || event.is_held
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
            || modifiers.shift
            || !matches!(event.keystroke.key.as_str(), "enter" | "space")
        {
            return;
        }
        let Some(displayed) = &self.displayed else {
            return;
        };
        if self.actionable(&self.snapshot.id, &displayed.row, window, cx) {
            cx.stop_propagation();
            self.invoke(window, cx);
        }
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let geometry = self.lease.upgrade().and_then(|lease| lease.geometry.get());
        if self.geometry != geometry || self.snapshot.focused != self.focus.is_focused(window) {
            self.geometry = geometry;
            self.snapshot.focused = self.focus.is_focused(window);
            self.invalidate();
        }
        if self.lease.upgrade().is_none() && self.surface.is_some() {
            self.stop(cx);
            self.invalidate();
        }
        self.mount(window, cx);
        let Some(surface) = &mut self.surface else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        if self
            .published
            .as_ref()
            .is_none_or(|id| !id.matches(&self.snapshot.id))
        {
            let mut snapshot = self.snapshot.clone();
            if self.recovery {
                snapshot.text = vega_i18n::t("ui.reload_view").to_owned();
                snapshot.disabled = false;
                snapshot.button = true;
            }
            surface.publish(snapshot);
            self.published = Some(self.snapshot.id.clone());
        }
        let lease = self.lease.upgrade().unwrap_or_else(|| {
            Rc::new(Mount {
                geometry: Cell::new(None),
                view: surface.entity().downgrade(),
                app: cx.to_async(),
            })
        });
        self.lease = Rc::downgrade(&lease);
        let enabled = self.recovery || (self.snapshot.button && !self.snapshot.disabled);
        let disabled = self.snapshot.button && self.snapshot.disabled && !self.recovery;
        self.focus.clone().tab_stop(enabled);
        let mut child = div()
            .relative()
            .w_full()
            .h(px(self.height))
            .flex_shrink_0()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::key))
            .child(surface.entity());
        if let Some(displayed) = &self.displayed
            && displayed.row.upgrade().is_some()
            && let Some(bounds) = self.native_bounds(displayed)
            && self.live(&self.snapshot.id, window, cx)
        {
            let origin = self
                .geometry
                .map(|geometry| geometry.bounds.origin)
                .unwrap_or_default();
            let label = if self.recovery {
                vega_i18n::t("ui.reload_view").to_owned()
            } else {
                self.snapshot.text.clone()
            };
            let mut proxy = div()
                .id("privacy-leaf-ax")
                .absolute()
                .left(bounds.origin.x - origin.x)
                .top(bounds.origin.y - origin.y)
                .w(bounds.size.width)
                .h(bounds.size.height)
                .role(if self.snapshot.button || self.recovery {
                    gpui_kit::Role::Button
                } else {
                    gpui_kit::Role::Label
                })
                .aria_label(label)
                .a11y_synthetic_children(move |tree| {
                    if disabled {
                        tree.parent_node().set_disabled();
                    }
                });
            if enabled {
                let id = self.snapshot.id.clone();
                let row = displayed.row.clone();
                let focus_id = id.clone();
                let focus_row = row.clone();
                let weak = cx.weak_entity();
                let focus_weak = weak.clone();
                proxy = proxy
                    .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                        let _ = focus_weak.update(cx, |host, cx| {
                            if host.actionable(&focus_id, &focus_row, window, cx) {
                                window.focus(&host.focus, cx);
                            }
                        });
                    })
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |host, cx| {
                            if host.actionable(&id, &row, window, cx) {
                                host.invoke(window, cx);
                            }
                        });
                    });
            }
            child = child.child(proxy);
        }
        Mounted {
            child: child.into_any_element(),
            lease,
            host: cx.weak_entity(),
        }
        .into_any_element()
    }
}

fn pixels(state: ns::Signal<PixelsState>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    ns::dynamic(move || {
        let snapshot = state.get();
        let target = ns::element_ref();
        let row = Arc::new(());
        let row_lease = Arc::downgrade(&row);
        nsh_core::element::cleanup(move || drop(row));
        let owner = nsh_core::element::Owner::current();
        let presentation = owner;
        let measuring = commands.clone();
        let measured_id = snapshot.id.clone();
        let measured_row = row_lease.clone();
        let measure = move || {
            let id = measured_id.clone();
            let lease = measured_row.clone();
            let sender = measuring.clone();
            owner.run(|| {
                ns::spawn(async move {
                    if let Some(measurement) = target.measure().await
                        && presentation.is_live()
                    {
                        sender.send(Request::Displayed(
                            id,
                            ns::Bounds {
                                origin: measurement.page,
                                size: measurement.frame.size,
                            },
                            lease,
                        ));
                    }
                })
            });
        };
        let hover = ns::signal(false);
        let down = ns::signal(false);
        let focused = snapshot.focused;
        let enabled = snapshot.button && !snapshot.disabled;
        let colors = snapshot.colors;
        let press = commands.clone();
        let id = snapshot.id.clone();
        let press_row = row_lease.clone();
        let action = commands.clone();
        let action_id = id.clone();
        let action_row = row_lease.clone();
        ns::view()
            .element_ref(&target)
            .w_full()
            .min_h(ns::px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
            .flex_col()
            .justify_center()
            .px(ns::px(if snapshot.button { 8. } else { 0. }))
            .py(ns::px(if snapshot.button { 4. } else { 0. }))
            .rounded(ns::px(Layout::SETTINGS_CONTROL_RADIUS))
            .border(ns::px(1.))
            .border_color(crate::nsh::color(if focused {
                colors.accent
            } else {
                colors.bg_base.opacity(0.)
            }))
            .bg(move || {
                crate::nsh::color(if enabled && down.get() {
                    colors.bg_active
                } else if enabled && hover.get() {
                    colors.bg_hover
                } else {
                    colors.bg_base.opacity(0.)
                })
            })
            .role(if snapshot.button {
                nsh_core::AccessibilityRole::Button
            } else {
                nsh_core::AccessibilityRole::Text
            })
            .label(snapshot.text.clone())
            .disabled(snapshot.disabled)
            .accessibility_elements_hidden(true)
            .on_layout(move |_: &ns::Bounds<ns::Points>| measure())
            .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
            .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
                hover.set(false);
                down.set(false);
            })
            .on_press_down(move || down.set(true))
            .on_press_up(move || down.set(false))
            .on_press(move || {
                if enabled {
                    press.send(Request::Activate(id.clone(), press_row.clone()));
                }
            })
            .on_accessibility_action(move |name: &str| {
                if enabled && name == "activate" {
                    action.send(Request::Activate(action_id.clone(), action_row.clone()));
                }
            })
            .child(
                ns::text(snapshot.text)
                    .text_size(ns::px(Typography::BODY))
                    .text_color(crate::nsh::color(snapshot.foreground)),
            )
    })
    .into_element()
}

struct Mounted {
    child: AnyElement,
    lease: Rc<Mount>,
    host: WeakEntity<Host>,
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
        Some("privacy-nsh-mount".into())
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
            let clip = bounds.intersect(&window.content_mask().bounds);
            if !clip.is_empty()
                && [
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                ]
                .iter()
                .all(|value| value.is_finite())
            {
                self.lease.geometry.set(Some(Geometry {
                    bounds,
                    clip,
                    viewport: window.viewport_size(),
                    scale: window.scale_factor(),
                }));
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
    }
}
