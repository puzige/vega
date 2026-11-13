use super::*;
use crate::nsh::{OrderedCommands, ProjectionId, Surface};
use crate::sidebar::SelectedProject;
use gpui_kit::{
    AccessibleAction, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId,
    RenderOnce, SharedString, Size, WeakEntity,
};
use ns::{
    Accessible as _, Element as _, Interactive as _, IntoElement as _, ParentElement as _,
    Styled as _,
};
use nsh_core::prelude as ns;
use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak as RowWeak};
use tokio::sync::mpsc;

#[derive(gpui_kit::IntoElement)]
pub(in crate::conversation_stream) struct Control {
    pub id: SharedString,
    pub text: String,
    pub label: String,
    pub button: bool,
    pub details: bool,
    pub glyph: Option<String>,
    pub truncate: bool,
    pub colors: vega_theme::ThemeColors,
    pub foreground: gpui_kit::Rgba,
    pub binding: Binding,
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
    label: String,
    disabled: bool,
    button: bool,
    details: bool,
    glyph: Option<String>,
    truncate: bool,
    scroll: f32,
    scroll_limit: f32,
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
        && left.text_primary == right.text_primary
        && left.danger == right.danger
        && left.warning == right.warning
        && left.bg_elevated == right.bg_elevated
        && left.border_subtle == right.border_subtle
}

struct Row {
    observation: AtomicU64,
}

enum Request {
    Displayed(ProjectionId, ns::Bounds<ns::Points>, RowWeak<Row>, u64),
    Activate(ProjectionId, RowWeak<Row>, u64),
    Scrolled(ProjectionId, f32, f32),
}

struct Displayed {
    bounds: ns::Bounds<ns::Points>,
    row: RowWeak<Row>,
    observation: u64,
}

struct Host {
    snapshot: PixelsState,
    binding: Option<Binding>,
    environment: Environment,
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
        let disabled = !self.binding.enabled;
        let key: SharedString = format!(
            "{}-{:?}-{:?}",
            self.id,
            self.binding.owner.entity_id(),
            self.binding.focus
        )
        .into();
        let binding = Some(self.binding);
        let host = window.use_keyed_state(key, cx, |window, cx| {
            let focus = binding
                .as_ref()
                .filter(|_| self.button || self.details)
                .and_then(|binding| binding.focus.clone())
                .unwrap_or_else(|| cx.focus_handle().tab_stop(false));
            let subscriptions = vec![
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
            Host {
                snapshot: PixelsState {
                    id: ProjectionId::new(),
                    text: self.text.clone(),
                    label: self.label.clone(),
                    disabled,
                    button: self.button,
                    details: self.details,
                    glyph: self.glyph.clone(),
                    truncate: self.truncate,
                    scroll: 0.,
                    scroll_limit: 0.,
                    focused: false,
                    colors: self.colors,
                    foreground: self.foreground,
                },
                binding: binding.clone(),
                environment: environment.clone(),
                surface: None,
                displayed: None,
                lease: Weak::new(),
                focus,
                height: if self.details {
                    (f32::from(window.viewport_size().height) - 240.).clamp(96., 360.)
                } else {
                    Layout::SETTINGS_CONTROL_MIN_HEIGHT
                },
                geometry: None,
                published: None,
                recovery: false,
                _subscriptions: subscriptions,
            }
        });
        host.update(cx, |host, cx| {
            let binding_changed = match (&host.binding, &binding) {
                (Some(old), Some(new)) => !old.same(new),
                (None, None) => false,
                _ => true,
            };
            if binding_changed
                || host.environment != environment
                || host.snapshot.text != self.text
                || host.snapshot.label != self.label
                || host.snapshot.disabled != disabled
                || host.snapshot.button != self.button
                || host.snapshot.details != self.details
                || host.snapshot.glyph != self.glyph
                || host.snapshot.truncate != self.truncate
                || host.snapshot.foreground != self.foreground
                || !same_palette(host.snapshot.colors, self.colors)
            {
                host.invalidate();
                host.recovery = false;
                cx.notify();
            }
            host.binding = binding;
            host.environment = environment;
            host.snapshot.text = self.text;
            host.snapshot.label = self.label;
            host.snapshot.disabled = disabled;
            if binding_changed {
                host.snapshot.scroll = 0.;
                host.snapshot.scroll_limit = 0.;
            }
            host.snapshot.button = self.button;
            host.snapshot.details = self.details;
            host.snapshot.glyph = self.glyph;
            host.snapshot.truncate = self.truncate;
            if self.details {
                host.height = (f32::from(window.viewport_size().height) - 240.).clamp(96., 360.);
            }
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

    fn actionable(&self, id: &ProjectionId, row: &RowWeak<Row>, window: &Window, cx: &App) -> bool {
        self.live(id, window, cx)
            && (self.snapshot.button || self.snapshot.details || self.recovery)
            && (!self.snapshot.disabled || self.recovery)
            && self.displayed.as_ref().is_some_and(|displayed| {
                row.upgrade().is_some_and(|row| {
                    row.observation.load(Ordering::Acquire) == displayed.observation
                }) && RowWeak::ptr_eq(&displayed.row, row)
                    && self.native_bounds(displayed).is_some()
            })
    }

    fn invoke(&mut self, keyboard: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.recovery {
            self.stop(cx);
            self.recovery = false;
            self.invalidate();
            cx.notify();
        } else if self.snapshot.details {
            window.focus(&self.focus, cx);
        } else if let Some(binding) = self.binding.clone()
            && self.snapshot.button
        {
            self.invalidate();
            cx.notify();
            if !keyboard {
                window.focus(&self.focus, cx);
            }
            binding.dispatch(Navigation::Activate, window, cx);
        }
    }

    fn neighbor(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window)
            || self.recovery
            || self
                .binding
                .as_ref()
                .is_none_or(|binding| binding.composing(cx))
        {
            return;
        }
        let Some(displayed) = &self.displayed else {
            return;
        };
        if self.actionable(&self.snapshot.id, &displayed.row, window, cx)
            && let Some(binding) = &self.binding
        {
            binding.dispatch(Navigation::Neighbor { backwards }, window, cx);
            cx.stop_propagation();
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let mut snapshot = self.snapshot.clone();
        if self.recovery {
            snapshot.text = vega_i18n::t("ui.reload_view").to_owned();
            snapshot.label = snapshot.text.clone();
            snapshot.disabled = false;
            snapshot.button = true;
            snapshot.details = false;
        }
        let (surface, mut requests) = Surface::new_transparent(snapshot, pixels, window, cx);
        surface.focus_handle(cx).tab_stop(false);
        let mut errors = surface.errors();
        let failed_view = surface.entity().entity_id();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |host, window, cx| match request {
                        Request::Displayed(id, bounds, row, observation)
                            if host.live(&id, window, cx)
                                && row.upgrade().is_some_and(|row| {
                                    row.observation.load(Ordering::Acquire) == observation
                                }) =>
                        {
                            let height = bounds.size.height.0;
                            if height.is_finite()
                                && height > 0.
                                && host.displayed.as_ref().is_none_or(|displayed| {
                                    !RowWeak::ptr_eq(&displayed.row, &row)
                                        || observation >= displayed.observation
                                })
                            {
                                if !host.snapshot.details && (host.height - height).abs() > 0.5 {
                                    host.height = height;
                                    host.invalidate();
                                    cx.notify();
                                } else if host.displayed.as_ref().is_none_or(|displayed| {
                                    displayed.bounds != bounds
                                        || displayed.observation != observation
                                        || !RowWeak::ptr_eq(&displayed.row, &row)
                                }) {
                                    host.displayed = Some(Displayed {
                                        bounds,
                                        row,
                                        observation,
                                    });
                                    cx.notify();
                                }
                            }
                        }
                        Request::Activate(id, row, observation)
                            if host.actionable(&id, &row, window, cx)
                                && host.displayed.as_ref().is_some_and(|displayed| {
                                    displayed.observation == observation
                                }) =>
                        {
                            host.invoke(false, window, cx)
                        }
                        Request::Scrolled(id, offset, limit)
                            if host.live(&id, window, cx)
                                && offset.is_finite()
                                && limit.is_finite() =>
                        {
                            host.snapshot.scroll_limit = limit.max(0.);
                            host.snapshot.scroll = offset.clamp(0., host.snapshot.scroll_limit);
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
        if self.focus.is_focused(window)
            && !event.prefer_character_input
            && !event.is_held
            && !modifiers.control
            && !modifiers.alt
            && !modifiers.platform
            && !modifiers.function
            && self
                .binding
                .as_ref()
                .is_some_and(|binding| !binding.composing(cx))
            && let Some(displayed) = &self.displayed
            && self.actionable(&self.snapshot.id, &displayed.row, window, cx)
        {
            match event.keystroke.key.as_str() {
                "tab" => {
                    self.neighbor(modifiers.shift, window, cx);
                    return;
                }
                "escape" if !modifiers.shift => {
                    if let Some(binding) = &self.binding {
                        binding.dispatch(Navigation::Escape, window, cx);
                        cx.stop_propagation();
                    }
                    return;
                }
                key if self.snapshot.details => {
                    let next = match key {
                        "up" => self.snapshot.scroll - 32.,
                        "down" => self.snapshot.scroll + 32.,
                        "pageup" => self.snapshot.scroll - 180.,
                        "pagedown" => self.snapshot.scroll + 180.,
                        "space" | " " if modifiers.shift => self.snapshot.scroll - 180.,
                        "space" | " " => self.snapshot.scroll + 180.,
                        "home" => 0.,
                        "end" => self.snapshot.scroll_limit,
                        _ => return,
                    };
                    self.snapshot.scroll = next.clamp(0., self.snapshot.scroll_limit);
                    self.invalidate();
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }
        if !self.focus.is_focused(window)
            || self
                .binding
                .as_ref()
                .is_none_or(|binding| binding.composing(cx))
            || event.prefer_character_input
            || event.is_held
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
            || modifiers.shift
            || !matches!(event.keystroke.key.as_str(), "enter" | "space" | " ")
        {
            return;
        }
        let Some(displayed) = &self.displayed else {
            return;
        };
        if self.actionable(&self.snapshot.id, &displayed.row, window, cx) {
            cx.stop_propagation();
            self.invoke(true, window, cx);
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
                snapshot.label = snapshot.text.clone();
                snapshot.disabled = false;
                snapshot.button = true;
                snapshot.details = false;
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
        let enabled = self.recovery
            || ((self.snapshot.button || self.snapshot.details) && !self.snapshot.disabled);
        let disabled = (self.snapshot.button || self.snapshot.details)
            && self.snapshot.disabled
            && !self.recovery;
        self.focus.clone().tab_stop(enabled);
        let mut child = div()
            .relative()
            .w_full()
            .h(px(self.height))
            .flex_shrink_0()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::key))
            .on_action(cx.listener(|host, _: &NextComposerControl, window, cx| {
                host.neighbor(false, window, cx)
            }))
            .on_action(
                cx.listener(|host, _: &PreviousComposerControl, window, cx| {
                    host.neighbor(true, window, cx)
                }),
            )
            .child(surface.entity());
        if let Some(displayed) = &self.displayed
            && displayed
                .row
                .upgrade()
                .is_some_and(|row| row.observation.load(Ordering::Acquire) == displayed.observation)
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
                self.snapshot.label.clone()
            };
            let mut proxy = div()
                .id("automation-feedback-leaf-ax")
                .absolute()
                .left(bounds.origin.x - origin.x)
                .top(bounds.origin.y - origin.y)
                .w(bounds.size.width)
                .h(bounds.size.height)
                .role(if self.snapshot.button || self.recovery {
                    gpui_kit::Role::Button
                } else if self.snapshot.details {
                    gpui_kit::Role::Group
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
                let observation = displayed.observation;
                let id = self.snapshot.id.clone();
                let row = displayed.row.clone();
                let focus_id = id.clone();
                let focus_row = row.clone();
                let weak = cx.weak_entity();
                let focus_weak = weak.clone();
                proxy =
                    proxy
                        .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                            let _ = focus_weak.update(cx, |host, cx| {
                                if host.actionable(&focus_id, &focus_row, window, cx)
                                    && host.displayed.as_ref().is_some_and(|displayed| {
                                        displayed.observation == observation
                                    })
                                {
                                    window.focus(&host.focus, cx);
                                }
                            });
                        })
                        .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                            let _ = weak.update(cx, |host, cx| {
                                if host.actionable(&id, &row, window, cx)
                                    && host.displayed.as_ref().is_some_and(|displayed| {
                                        displayed.observation == observation
                                    })
                                {
                                    host.invoke(false, window, cx);
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
    use ns::Scrollable as _;
    let commands = OrderedCommands::new(sender);
    ns::dynamic(move || {
        let snapshot = state.get();
        let target = ns::element_ref();
        let row = Arc::new(Row {
            observation: AtomicU64::new(0),
        });
        let row_lease = Arc::downgrade(&row);
        nsh_core::element::cleanup(move || drop(row));
        let owner = nsh_core::element::Owner::current();
        let revision = ns::signal(0u64);
        let measuring = commands.clone();
        let measured_id = snapshot.id.clone();
        let measured_row = row_lease.clone();
        let layout_row = row_lease.clone();
        let measure = Rc::new(move |activate: bool| {
            let id = measured_id.clone();
            let lease = measured_row.clone();
            let sender = measuring.clone();
            let observation = revision.get_untracked();
            owner.run(|| {
                ns::spawn(async move {
                    if let Some(measurement) = target.measure().await
                        && owner.is_live()
                        && revision.get_untracked() == observation
                    {
                        sender.send(Request::Displayed(
                            id.clone(),
                            ns::Bounds {
                                origin: measurement.page,
                                size: measurement.frame.size,
                            },
                            lease.clone(),
                            observation,
                        ));
                        if activate {
                            sender.send(Request::Activate(id, lease, observation));
                        }
                    }
                })
            });
        });
        let layout_measure = measure.clone();
        let press_measure = measure.clone();
        let hover = ns::signal(false);
        let down = ns::signal(false);
        let enabled = (snapshot.button || snapshot.details) && !snapshot.disabled;
        let colors = snapshot.colors;
        let root = ns::view()
            .element_ref(&target)
            .w_full()
            .min_h(ns::px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
            .flex_col()
            .justify_center()
            .px(ns::px(if snapshot.button && snapshot.text.is_empty() {
                Layout::AUTOMATION_GAP
            } else if snapshot.button {
                8.
            } else {
                0.
            }))
            .py(ns::px(if snapshot.button { 4. } else { 0. }))
            .rounded(ns::px(Layout::SETTINGS_CONTROL_RADIUS))
            .border(ns::px(1.))
            .border_color(crate::nsh::color(if snapshot.focused {
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
                    colors.bg_elevated
                })
            })
            .role(if snapshot.button {
                nsh_core::AccessibilityRole::Button
            } else {
                nsh_core::AccessibilityRole::Text
            })
            .label(snapshot.label.clone())
            .disabled(snapshot.disabled)
            .accessibility_elements_hidden(true)
            .on_layout(move |_: &ns::Bounds<ns::Points>| {
                revision.update(|value| *value = value.saturating_add(1));
                if let Some(row) = layout_row.upgrade() {
                    row.observation
                        .store(revision.get_untracked(), Ordering::Release);
                }
                layout_measure(false);
            })
            .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
            .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
                hover.set(false);
                down.set(false);
            })
            .on_press_down(move || down.set(true))
            .on_press_up(move || down.set(false))
            .on_press(move || {
                if enabled {
                    press_measure(true);
                }
            })
            .on_accessibility_action(move |name: &str| {
                if enabled && name == "activate" {
                    measure(true);
                }
            });
        if snapshot.details {
            let scroll_commands = commands.clone();
            let scroll_id = snapshot.id.clone();
            let scrolling = ns::element_ref();
            let content = ns::element_ref();
            let range_commands = commands.clone();
            let range_id = snapshot.id.clone();
            let range = move || {
                let commands = range_commands.clone();
                let id = range_id.clone();
                let observation = revision.get_untracked();
                owner.run(|| {
                    ns::spawn(async move {
                        if let Some(viewport) = scrolling.measure().await
                            && let Some(body) = content.measure().await
                            && owner.is_live()
                            && revision.get_untracked() == observation
                        {
                            commands.send(Request::Scrolled(
                                id,
                                snapshot.scroll,
                                body.frame.size.height.0 - viewport.frame.size.height.0,
                            ));
                        }
                    })
                });
            };
            root.h_full()
                .child(
                    ns::scroll()
                        .element_ref(&scrolling)
                        .w_full()
                        .h_full()
                        .bounces(false)
                        .content_offset(nsh_core::geometry::point(
                            ns::px(0.),
                            ns::px(snapshot.scroll),
                        ))
                        .on_layout(move |_: &ns::Bounds<ns::Points>| {
                            scrolling.scroll_to_offset(ns::px(snapshot.scroll), false)
                        })
                        .on_scroll(move |event: &ns::ScrollEvent| {
                            scroll_commands.send(Request::Scrolled(
                                scroll_id.clone(),
                                event.offset.y.0,
                                event.content_size.height.0 - event.size.height.0,
                            ));
                        })
                        .child(
                            ns::text(snapshot.text)
                                .element_ref(&content)
                                .w_full()
                                .on_layout(move |_: &ns::Bounds<ns::Points>| range())
                                .text_size(ns::px(Typography::BODY))
                                .text_color(crate::nsh::color(snapshot.foreground)),
                        ),
                )
                .into_element()
        } else {
            let mut body = ns::view()
                .w_full()
                .flex_row()
                .items_center()
                .gap(ns::px(Layout::AUTOMATION_GAP));
            if let Some(glyph) = snapshot.glyph {
                body = body.child(
                    ns::svg(nsh_core::element::ImageSrc::svg_markup(&glyph))
                        .size(ns::px(Layout::AUTOMATION_ICON_SIZE))
                        .flex_shrink_0()
                        .text_color(crate::nsh::color(snapshot.foreground)),
                );
            }
            let foreground = if snapshot.disabled {
                colors.text_tertiary
            } else {
                snapshot.foreground
            };
            if snapshot.text.is_empty() {
                return root.child(body).into_element();
            }
            let mut text = ns::text(snapshot.text)
                .flex_1()
                .min_w(ns::px(0.))
                .text_size(ns::px(Typography::BODY))
                .text_color(crate::nsh::color(foreground));
            if snapshot.truncate {
                text = text.truncate();
            }
            root.child(body.child(text)).into_element()
        }
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
        Some("automation-feedback-nsh-mount".into())
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
