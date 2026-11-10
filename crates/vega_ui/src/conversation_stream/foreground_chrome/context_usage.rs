use super::super::{
    ConversationStream, context_usage::ContextUsageDisplay, selection_menu::ReadingOwner,
};
use super::Passive;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use ns::{Accessible as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use std::{
    cell::Cell,
    rc::{Rc, Weak},
};
use vega_theme::{Layout, Typography, theme};

#[derive(Clone, PartialEq)]
struct Palette {
    track: Rgba,
    progress: Rgba,
    hover: Rgba,
    tooltip_bg: Rgba,
    tooltip_border: Rgba,
    secondary: Rgba,
    primary: Rgba,
    focus: Rgba,
}

#[derive(Clone, PartialEq)]
struct PixelsState {
    palette: Palette,
    fraction: f32,
    hovered: bool,
    focused: bool,
    lines: [String; 3],
    width: f32,
    font_family: String,
}

#[derive(Clone)]
struct Source {
    root: ReadingOwner,
    display: ContextUsageDisplay,
    language: &'static str,
}

impl Source {
    fn capture(stream: &ConversationStream, owner: WeakEntity<ConversationStream>) -> Self {
        Self {
            root: ReadingOwner::message(stream, owner, None),
            display: stream.context_usage_display(),
            language: vega_i18n::language(),
        }
    }

    fn matches(&self, other: &Self) -> bool {
        self.root.same_binding(&other.root)
            && self.display == other.display
            && self.language == other.language
    }

    fn bound(&self, owner: &WeakEntity<ConversationStream>, window: &Window, cx: &App) -> bool {
        owner.upgrade().is_some_and(|entity| {
            let stream = entity.read(cx);
            stream.context_usage_visible()
                && self.matches(&Self::capture(stream, owner.clone()))
                && stream
                    .resume_tail_authority
                    .is_some_and(|authority| authority(owner, window, cx))
        })
    }

    fn current(&self, owner: &WeakEntity<ConversationStream>, window: &Window, cx: &App) -> bool {
        self.bound(owner, window, cx) && self.root.root_current(window, cx) && window.is_visible()
    }
}

#[derive(Default)]
struct Reveal {
    hovered: bool,
    focused: bool,
    hover_dismissed: bool,
    focus_dismissed: bool,
}

impl Reveal {
    fn visible(&self) -> bool {
        (self.hovered && !self.hover_dismissed) || (self.focused && !self.focus_dismissed)
    }

    fn hover(&mut self, value: bool) -> bool {
        if self.hovered == value {
            return false;
        }
        self.hovered = value;
        self.hover_dismissed = false;
        true
    }

    fn focus(&mut self, value: bool) -> bool {
        if self.focused == value {
            return false;
        }
        self.focused = value;
        self.focus_dismissed = false;
        true
    }

    fn dismiss(&mut self) {
        self.hover_dismissed = self.hovered;
        self.focus_dismissed = self.focused;
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    viewport: Size<Pixels>,
    scale: f32,
}

impl Geometry {
    fn current(&self, window: &Window) -> bool {
        self.viewport == window.viewport_size()
            && self.scale == window.scale_factor()
            && self.clip.size.width > px(0.)
            && self.clip.size.height > px(0.)
    }
}

#[derive(Default)]
struct Lease {
    geometry: Cell<Option<Geometry>>,
}

struct Host {
    owner: WeakEntity<ConversationStream>,
    source: Source,
    id: ProjectionId,
    focus: FocusHandle,
    reveal: Reveal,
    lease: Weak<Lease>,
    ring: Surface<PixelsState, ()>,
    tooltip: Surface<PixelsState, ()>,
    published: PixelsState,
    _subscriptions: Vec<Subscription>,
}

pub(in super::super) fn indicator(owner: WeakEntity<ConversationStream>) -> AnyElement {
    Indicator { owner }.into_any_element()
}

#[derive(gpui_kit::IntoElement)]
struct Indicator {
    owner: WeakEntity<ConversationStream>,
}

impl RenderOnce for Indicator {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let source = Source::capture(owner.read(cx), self.owner.clone());
        if !source.bound(&self.owner, window, cx) {
            return div().into_any_element();
        }
        let state = pixels_state(&source.display, false, false, window, cx);
        let key = format!("nsh-context-usage-{}", owner.entity_id());
        let host = window.use_keyed_state(key, cx, |window, cx| {
            let focus = owner.read(cx).context_usage_focus.clone();
            let (ring, _) = Surface::new_transparent(state.clone(), ring_pixels, window, cx);
            let (tooltip, _) = Surface::new_transparent(state.clone(), tooltip_pixels, window, cx);
            ring.focus_handle(cx).tab_stop(false);
            tooltip.focus_handle(cx).tab_stop(false);
            let subscriptions = vec![
                cx.on_release(|host: &mut Host, cx| {
                    host.ring.entity().read(cx).stop();
                    host.tooltip.entity().read(cx).stop();
                }),
                cx.on_focus(&focus, window, |host: &mut Host, window, cx| {
                    if host.current(window, cx) && host.reveal.focus(true) {
                        cx.notify();
                    }
                }),
                cx.on_blur(&focus, window, |host: &mut Host, _, cx| {
                    if host.reveal.focus(false) {
                        cx.notify();
                    }
                }),
                cx.observe_window_activation(window, |host: &mut Host, _, cx| {
                    host.reveal = Reveal::default();
                    cx.notify();
                }),
                cx.observe_window_visibility(window, |host: &mut Host, _, _, cx| {
                    host.reveal = Reveal::default();
                    cx.notify();
                }),
                cx.observe(&owner, |_: &mut Host, _, cx| cx.notify()),
            ];
            Host {
                owner: self.owner.clone(),
                source: source.clone(),
                id: ProjectionId::new(),
                focus,
                reveal: Reveal::default(),
                lease: Weak::new(),
                ring,
                tooltip,
                published: state.clone(),
                _subscriptions: subscriptions,
            }
        });
        host.update(cx, |host, _| {
            if !host.source.matches(&source) {
                host.id = ProjectionId::new();
                host.reveal = Reveal::default();
                host.lease = Weak::new();
            }
            host.source = source;
        });
        host.into_any_element()
    }
}

impl Host {
    fn current(&self, window: &Window, cx: &App) -> bool {
        self.source.current(&self.owner, window, cx)
            && self.ring.errors().borrow().is_none()
            && self.tooltip.errors().borrow().is_none()
            && self
                .lease
                .upgrade()
                .and_then(|lease| lease.geometry.get())
                .is_some_and(|geometry| geometry.current(window))
    }

    fn accepts(&self, id: &ProjectionId, lease: &Rc<Lease>, window: &Window, cx: &App) -> bool {
        self.id.matches(id)
            && self.current(window, cx)
            && self
                .lease
                .upgrade()
                .is_some_and(|current| Rc::ptr_eq(&current, lease))
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.source.bound(&self.owner, window, cx) {
            self.reveal = Reveal::default();
            self.lease = Weak::new();
            return div().into_any_element();
        }
        let lease = self.lease.upgrade().unwrap_or_else(|| {
            self.reveal = Reveal::default();
            let lease = Rc::new(Lease::default());
            self.lease = Rc::downgrade(&lease);
            lease
        });
        if !self.source.current(&self.owner, window, cx) {
            self.reveal = Reveal::default();
        } else {
            self.reveal.focus(self.focus.is_focused(window));
        }
        if self.reveal.hovered
            && self
                .owner
                .upgrade()
                .is_some_and(|owner| !owner.read(cx).context_usage_trigger_hovered)
        {
            self.reveal.dismiss();
        }
        let state = pixels_state(
            &self.source.display,
            self.reveal.hovered,
            self.reveal.focused,
            window,
            cx,
        );
        self.ring.sync_environment(cx);
        self.tooltip.sync_environment(cx);
        if self.published != state {
            self.ring.publish(state.clone());
            self.tooltip.publish(state.clone());
            self.published = state.clone();
        }
        let hovered_id = self.id.clone();
        let hovered_lease = lease.clone();
        let mut child = div()
            .id("nsh-context-usage-proxy")
            .relative()
            .flex_shrink_0()
            .size(px(Layout::CONTEXT_USAGE_SIZE))
            .focusable()
            .track_focus(&self.focus)
            .tab_stop(true)
            .aria_label(self.source.display.accessibility_label())
            .on_hover(cx.listener(move |host, hovered: &bool, window, cx| {
                if host.accepts(&hovered_id, &hovered_lease, window, cx)
                    && host.reveal.hover(*hovered)
                {
                    let _ = host.owner.update(cx, |stream, _| {
                        stream.context_usage_trigger_hovered = *hovered;
                    });
                    cx.notify();
                }
            }))
            .child(Passive::new(
                div().absolute().inset_0().child(self.ring.entity()),
            ));
        if self.reveal.visible() {
            let tooltip = Guarded {
                child: Passive::new(div().size_full().child(self.tooltip.entity()))
                    .into_any_element(),
                host: cx.weak_entity(),
                id: self.id.clone(),
                lease: lease.clone(),
            };
            child = child.child(
                div().absolute().top_0().left_0().size(px(0.)).child(
                    div().relative().child(
                        anchored()
                            .anchor(Anchor::BottomLeft)
                            .position_mode(AnchoredPositionMode::Local)
                            .position(point(
                                (px(Layout::CONTEXT_USAGE_SIZE) - px(state.width)) / 2.,
                                px(-Layout::CONTEXT_USAGE_GAP),
                            ))
                            .snap_to_window()
                            .child(
                                deferred(
                                    div()
                                        .w(px(state.width))
                                        .h(px(tooltip_height()))
                                        .child(tooltip),
                                )
                                .with_priority(2),
                            ),
                    ),
                ),
            );
        }
        Mounted {
            child: child.into_any_element(),
            host: cx.weak_entity(),
            id: self.id.clone(),
            lease,
        }
        .into_any_element()
    }
}

fn tooltip_height() -> f32 {
    3. * Layout::CONTEXT_USAGE_LINE_HEIGHT
        + 2. * (Layout::CONTEXT_USAGE_VERTICAL_PADDING + Layout::CONTEXT_USAGE_BORDER)
}

fn pixels_state(
    display: &ContextUsageDisplay,
    hovered: bool,
    focused: bool,
    window: &Window,
    cx: &App,
) -> PixelsState {
    let colors = theme(cx).colors;
    let lines = [
        vega_i18n::t("ui.context_window").to_string(),
        display.tooltip_status(),
        display.tooltip_usage(),
    ];
    let text_width = lines
        .iter()
        .map(|line| {
            window
                .text_system()
                .shape_line(
                    line.clone().into(),
                    px(Typography::BODY),
                    &[window.text_style().to_run(line.len())],
                    None,
                )
                .width()
        })
        .max()
        .unwrap_or_default();
    PixelsState {
        palette: Palette {
            track: colors.border_subtle,
            progress: colors.accent,
            hover: colors.bg_hover,
            tooltip_bg: colors.context_tooltip_bg,
            tooltip_border: colors.context_tooltip_border,
            secondary: colors.context_tooltip_secondary,
            primary: colors.context_tooltip_primary,
            focus: colors.accent,
        },
        fraction: display.fill_fraction,
        font_family: window.text_style().font_family.to_string(),
        hovered,
        focused,
        lines,
        width: f32::from(
            (text_width + px(2. * (Layout::CONTEXT_USAGE_PADDING + Layout::CONTEXT_USAGE_BORDER)))
                .ceil()
                .max(px(Layout::CONTEXT_USAGE_MIN_WIDTH)),
        ),
    }
}

fn ring_pixels(state: ns::Signal<PixelsState>, _: tokio::sync::mpsc::Sender<()>) -> ns::AnyElement {
    ns::view()
        .size_full()
        .accessibility_elements_hidden(true)
        .rounded_full()
        .bg(move || {
            let state = state.get();
            crate::nsh::color(if state.hovered || state.focused {
                state.palette.hover
            } else {
                state.palette.hover.opacity(0.)
            })
        })
        .border(ns::px(1.))
        .border_color(move || {
            let state = state.get();
            crate::nsh::color(if state.focused {
                state.palette.focus
            } else {
                state.palette.focus.opacity(0.)
            })
        })
        .items_center()
        .justify_center()
        .child(
            ns::canvas(move || {
                let state = state.get();
                let start = -std::f32::consts::FRAC_PI_2;
                let painter = ns::Painter::new()
                    .line_width(2.)
                    .stroke_color(crate::nsh::color(state.palette.track).to_hsla())
                    .begin_path()
                    .arc(8., 8., 6., start, start + std::f32::consts::TAU)
                    .stroke();
                if state.fraction <= 0. {
                    painter
                } else {
                    painter
                        .stroke_color(crate::nsh::color(state.palette.progress).to_hsla())
                        .begin_path()
                        .arc(
                            8.,
                            8.,
                            6.,
                            start,
                            start + std::f32::consts::TAU * state.fraction,
                        )
                        .stroke()
                }
            })
            .w(ns::px(Layout::ICON_SIZE))
            .h(ns::px(Layout::ICON_SIZE)),
        )
        .into_element()
}

fn tooltip_pixels(
    state: ns::Signal<PixelsState>,
    _: tokio::sync::mpsc::Sender<()>,
) -> ns::AnyElement {
    ns::view()
        .size_full()
        .accessibility_elements_hidden(true)
        .px(ns::px(Layout::CONTEXT_USAGE_PADDING))
        .py(ns::px(Layout::CONTEXT_USAGE_VERTICAL_PADDING))
        .rounded(ns::px(Layout::CONTEXT_USAGE_RADIUS))
        .border(ns::px(Layout::CONTEXT_USAGE_BORDER))
        .border_color(move || crate::nsh::color(state.get().palette.tooltip_border))
        .bg(move || crate::nsh::color(state.get().palette.tooltip_bg))
        .shadow_md()
        .flex_col()
        .items_center()
        .text_center()
        .font_family(move || state.get().font_family.clone())
        .text_size(ns::px(Typography::BODY))
        .line_height(ns::px(Layout::CONTEXT_USAGE_LINE_HEIGHT))
        .child(
            ns::text(move || state.get().lines[0].clone())
                .text_color(move || crate::nsh::color(state.get().palette.secondary)),
        )
        .child(
            ns::text(move || state.get().lines[1].clone())
                .text_color(move || crate::nsh::color(state.get().palette.primary)),
        )
        .child(
            ns::text(move || state.get().lines[2].clone())
                .text_color(move || crate::nsh::color(state.get().palette.primary)),
        )
        .into_element()
}

struct Mounted {
    child: AnyElement,
    host: WeakEntity<Host>,
    id: ProjectionId,
    lease: Rc<Lease>,
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
        Some("nsh-context-usage-mount".into())
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
        self.lease.geometry.set(Some(Geometry {
            bounds,
            clip: bounds.intersect(&window.content_mask().bounds),
            viewport: window.viewport_size(),
            scale: window.scale_factor(),
        }));
        if let Some(id) = id {
            window.with_element_state::<Rc<Lease>, _>(id, |_, _| ((), self.lease.clone()));
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
        let host = self.host.clone();
        let id = self.id.clone();
        let lease = self.lease.clone();
        window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
            if phase.bubble() && event.keystroke.key == "escape" {
                let _ = host.update(cx, |host, cx| {
                    if host.accepts(&id, &lease, window, cx)
                        && host.reveal.visible()
                        && host
                            .owner
                            .upgrade()
                            .is_some_and(|owner| !owner.read(cx).input.read(cx).is_composing())
                    {
                        host.reveal.dismiss();
                        let _ = host
                            .owner
                            .update(cx, |stream, _| stream.context_usage_trigger_hovered = false);
                        cx.notify();
                        cx.stop_propagation();
                    }
                });
            }
        });
    }
}

struct Guarded {
    child: AnyElement,
    host: WeakEntity<Host>,
    id: ProjectionId,
    lease: Rc<Lease>,
}

impl Guarded {
    fn current(&self, window: &Window, cx: &App) -> bool {
        self.host.upgrade().is_some_and(|host| {
            let host = host.read(cx);
            host.accepts(&self.id, &self.lease, window, cx) && host.reveal.visible()
        })
    }
}

impl IntoElement for Guarded {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Guarded {
    type RequestLayoutState = ();
    type PrepaintState = bool;
    fn id(&self) -> Option<ElementId> {
        None
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
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let current = self.current(window, cx);
        if current {
            self.child.prepaint(window, cx);
        }
        current
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if *prepaint && self.current(window, cx) {
            self.child.paint(window, cx);
        }
    }
}

#[cfg(test)]
mod tests;
