pub(crate) mod scroll_group;

pub(crate) use scroll_group::{chrome as group_scroll_chrome, listener as group_scroll_listener};

use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, FocusHandle,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollHandle, SharedString,
    Size, TouchDragEvent, TouchPhase, Window, base::ScrollbarHandle, div, point, prelude::*, px,
    size,
};
use ns::{Accessible as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use vega_theme::{Layout, theme};

pub(crate) fn focus(id: SharedString, window: &mut Window, cx: &mut App) -> FocusHandle {
    window
        .use_keyed_state(id, cx, |_, cx| cx.focus_handle().tab_stop(true))
        .read(cx)
        .clone()
}

#[derive(Clone, Copy, PartialEq)]
struct ChromeSnapshot {
    width: f32,
    height: f32,
    before: f32,
    after: f32,
    thumb_start: f32,
    thumb_length: f32,
    thumb_width: f32,
    visible: bool,
    color: gpui_kit::Rgba,
    thumb: gpui_kit::Rgba,
}

impl ChromeSnapshot {
    fn fade_length(self) -> f32 {
        Layout::SCROLL_EDGE_FADE_COMPACT
            .min(self.height / 4.)
            .max(0.)
    }

    fn strength(self, hidden: f32) -> f32 {
        let length = self.fade_length();
        if length > 0. {
            ((hidden - Layout::SCROLL_EDGE_FADE_EPSILON) / length).clamp(0., 1.)
        } else {
            0.
        }
    }
}

#[derive(Clone, Copy)]
struct ThumbGeometry {
    height: f32,
    extent: f32,
    inset: f32,
    length: f32,
    travel: f32,
}

impl ThumbGeometry {
    fn new(height: f32, extent: f32) -> Self {
        let height = height.max(0.);
        let extent = extent.max(0.);
        let logical = if extent > 0. {
            (height * height / (height + extent)).max(48.).min(height)
        } else {
            height
        };
        let inset = 4_f32.min(logical / 2.);
        Self {
            height,
            extent,
            inset,
            length: logical - inset * 2.,
            travel: height - logical,
        }
    }

    fn start(self, offset: f32) -> f32 {
        self.inset
            + if self.extent > 0. {
                (offset / self.extent).clamp(0., 1.) * self.travel
            } else {
                0.
            }
    }

    fn offset(self, position: f32, grab: f32) -> f32 {
        if self.travel <= 0. {
            return 0.;
        }
        -self.extent * ((position - self.inset - grab) / self.travel).clamp(0., 1.)
    }
}

struct Chrome {
    surface: Surface<ChromeSnapshot, ()>,
    published: ChromeSnapshot,
    lease: Weak<()>,
    generation: ProjectionId,
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    viewport: Size<Pixels>,
    scale: f32,
    offset: Pixels,
    extent: Pixels,
    hovered: bool,
    thumb_hovered: bool,
    drag: Option<f32>,
    last_scroll: Option<Instant>,
    idle_pending: bool,
    idle_generation: ProjectionId,
}

impl Chrome {
    fn current(&self, generation: &ProjectionId, window: &Window) -> bool {
        self.lease.upgrade().is_some()
            && self.generation.matches(generation)
            && self.viewport == window.viewport_size()
            && self.scale == window.scale_factor()
            && !self.clip.is_empty()
            && window.is_visible()
            && window.is_window_active()
            && self.surface.errors().borrow().is_none()
    }

    fn track(&self) -> Bounds<Pixels> {
        Bounds::new(
            point(self.bounds.right() - px(16.), self.bounds.top()),
            size(px(16.), self.bounds.size.height),
        )
    }

    fn geometry(&self) -> ThumbGeometry {
        ThumbGeometry::new(f32::from(self.bounds.size.height), f32::from(self.extent))
    }

    fn thumb(&self) -> Bounds<Pixels> {
        let geometry = self.geometry();
        Bounds::new(
            point(
                self.track().left(),
                self.bounds.top() + px(geometry.start(f32::from(-self.offset))),
            ),
            size(px(16.), px(geometry.length)),
        )
    }

    fn set_drag_offset(&self, position: Pixels, grab: f32, scroll: &ScrollHandle) {
        if self.geometry().travel <= 0. {
            return;
        }
        let mut offset = scroll.offset();
        offset.y = px(self
            .geometry()
            .offset(f32::from(position - self.bounds.top()), grab));
        scroll.set_offset(offset);
    }
}

pub(crate) fn scroll_chrome(
    id: SharedString,
    child: AnyElement,
    scroll: ScrollHandle,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let chrome = window
        .use_keyed_state(id.clone(), cx, |window, cx| {
            let snapshot = ChromeSnapshot {
                width: 0.,
                height: 0.,
                before: 0.,
                after: 0.,
                thumb_start: 0.,
                thumb_length: 0.,
                thumb_width: 6.,
                visible: false,
                color: theme(cx).colors.bg_base,
                thumb: theme(cx).colors.text_primary,
            };
            let (surface, _) = Surface::new_transparent(snapshot, render_chrome, window, cx);
            let _ = surface.focus_handle(cx).tab_stop(false);
            Rc::new(RefCell::new(Chrome {
                surface,
                published: snapshot,
                lease: Weak::new(),
                generation: ProjectionId::new(),
                bounds: Bounds::default(),
                clip: Bounds::default(),
                viewport: window.viewport_size(),
                scale: window.scale_factor(),
                offset: px(0.),
                extent: px(0.),
                hovered: false,
                thumb_hovered: false,
                drag: None,
                last_scroll: None,
                idle_pending: false,
                idle_generation: ProjectionId::new(),
            }))
        })
        .read(cx)
        .clone();
    let retained = chrome.borrow().lease.upgrade();
    let lease = retained.unwrap_or_else(|| {
        let lease = Rc::new(());
        let mut chrome = chrome.borrow_mut();
        chrome.generation = ProjectionId::new();
        chrome.drag = None;
        chrome.hovered = false;
        chrome.thumb_hovered = false;
        chrome.idle_pending = false;
        chrome.idle_generation = ProjectionId::new();
        chrome.lease = Rc::downgrade(&lease);
        lease
    });
    ScrollChrome {
        id: id.into(),
        child,
        scroll,
        chrome,
        lease,
        overlay: div().into_any_element(),
    }
    .into_any_element()
}

struct ScrollChrome {
    id: ElementId,
    child: AnyElement,
    scroll: ScrollHandle,
    chrome: Rc<RefCell<Chrome>>,
    lease: Rc<()>,
    overlay: AnyElement,
}

struct ChromePrepaint {
    clip: Bounds<Pixels>,
    hitbox: Option<Hitbox>,
    generation: ProjectionId,
}

impl IntoElement for ScrollChrome {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ScrollChrome {
    type RequestLayoutState = ();
    type PrepaintState = ChromePrepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
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
    ) -> ChromePrepaint {
        self.child.prepaint(window, cx);
        if let Some(id) = id {
            window.with_element_state::<Rc<()>, _>(id, |_, _| ((), self.lease.clone()));
        }
        let bounds = self.scroll.viewport_bounds();
        let clip = bounds.intersect(&window.content_mask().bounds);
        let extent = self.scroll.max_offset().y.max(px(0.));
        let offset = self.scroll.offset().y.clamp(-extent, px(0.));
        let now = Instant::now();
        let mut chrome = self.chrome.borrow_mut();
        if chrome.bounds != bounds
            || chrome.clip != clip
            || chrome.viewport != window.viewport_size()
            || chrome.scale != window.scale_factor()
            || chrome.extent != extent
        {
            chrome.generation = ProjectionId::new();
            if chrome.drag.take().is_some() {
                self.scroll.end_drag();
            }
            chrome.hovered = false;
            chrome.thumb_hovered = false;
        }
        if !window.is_window_active() || !window.is_visible() {
            if chrome.drag.take().is_some() {
                self.scroll.end_drag();
            }
            chrome.hovered = false;
            chrome.thumb_hovered = false;
        }
        if chrome.offset != offset {
            chrome.last_scroll = Some(now);
        }
        chrome.bounds = bounds;
        chrome.clip = clip;
        chrome.viewport = window.viewport_size();
        chrome.scale = window.scale_factor();
        chrome.offset = offset;
        chrome.extent = extent;
        let geometry = chrome.geometry();
        let visible = extent > px(0.)
            && (!cx.should_auto_hide_scrollbars()
                || chrome.hovered
                || chrome.drag.is_some()
                || chrome.last_scroll.is_some_and(|last| {
                    now.saturating_duration_since(last) < Duration::from_secs(2)
                }));
        let mut thumb = theme(cx).colors.text_primary;
        thumb.a *= if chrome.drag.is_some() || chrome.thumb_hovered {
            0.55
        } else {
            0.35
        };
        let snapshot = ChromeSnapshot {
            width: f32::from(bounds.size.width),
            height: geometry.height,
            before: f32::from(-offset),
            after: f32::from(extent + offset),
            thumb_start: geometry.start(f32::from(-offset)),
            thumb_length: geometry.length,
            thumb_width: if chrome.drag.is_some() || chrome.thumb_hovered {
                8.
            } else {
                6.
            },
            visible,
            color: theme(cx).colors.bg_base,
            thumb,
        };
        chrome.surface.sync_environment(cx);
        if snapshot != chrome.published {
            chrome.surface.publish(snapshot);
            chrome.published = snapshot;
        }
        if visible
            && cx.should_auto_hide_scrollbars()
            && !chrome.hovered
            && chrome.drag.is_none()
            && !chrome.idle_pending
            && let Some(last) = chrome.last_scroll
        {
            let delay = Duration::from_secs(2).saturating_sub(now.saturating_duration_since(last));
            if !delay.is_zero() {
                chrome.idle_pending = true;
                let idle_generation = ProjectionId::new();
                chrome.idle_generation = idle_generation.clone();
                let state = Rc::downgrade(&self.chrome);
                let view = window.current_view();
                window
                    .spawn(cx, async move |cx| {
                        cx.background_executor().timer(delay).await;
                        if let Some(state) = state.upgrade() {
                            let mut state = state.borrow_mut();
                            if !state.idle_generation.matches(&idle_generation) {
                                return;
                            }
                            state.idle_pending = false;
                            if state.lease.upgrade().is_some() {
                                drop(state);
                                let _ = cx.update(|_, cx| cx.notify(view));
                            }
                        }
                    })
                    .detach();
            }
        }
        let generation = chrome.generation.clone();
        let track = chrome.track().intersect(&clip);
        let surface = chrome.surface.entity();
        drop(chrome);
        self.overlay = div()
            .w(bounds.size.width)
            .h(bounds.size.height)
            .child(surface)
            .into_any_element();
        window.with_content_mask(
            Some(ContentMask {
                bounds: Bounds::new(clip.center(), Size::default()),
            }),
            |window| {
                self.overlay.prepaint_as_root(
                    bounds.origin,
                    size(
                        AvailableSpace::Definite(bounds.size.width),
                        AvailableSpace::Definite(bounds.size.height),
                    ),
                    window,
                    cx,
                );
            },
        );
        let hitbox = (extent > px(0.) && !track.is_empty())
            .then(|| window.insert_hitbox(track, HitboxBehavior::Normal));
        ChromePrepaint {
            clip,
            hitbox,
            generation,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        state: &mut ChromePrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
        window.with_content_mask(Some(ContentMask { bounds: state.clip }), |window| {
            self.overlay.paint(window, cx)
        });
        let Some(hitbox) = state.hitbox.clone() else {
            return;
        };
        let view = window.current_view();
        let current = self.chrome.clone();
        let generation = state.generation.clone();
        let scroll = self.scroll.clone();
        let track_hitbox = hitbox.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            let mut chrome = current.borrow_mut();
            if !phase.bubble()
                || event.button != MouseButton::Left
                || !chrome.current(&generation, window)
                || !chrome.published.visible
                || !track_hitbox.is_hovered_at(event.position, window)
            {
                return;
            }
            let thumb = chrome.thumb();
            let grab = if thumb.contains(&event.position) {
                f32::from(event.position.y - thumb.top())
            } else {
                chrome.geometry().length / 2.
            };
            if thumb.contains(&event.position) {
                chrome.drag = Some(grab);
                scroll.start_drag();
            }
            chrome.set_drag_offset(event.position.y, grab, &scroll);
            chrome.last_scroll = Some(Instant::now());
            cx.stop_propagation();
            cx.notify(view);
        });
        let current = self.chrome.clone();
        let generation = state.generation.clone();
        let scroll = self.scroll.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, _, window, cx| {
            let mut chrome = current.borrow_mut();
            if !chrome.current(&generation, window) {
                return;
            }
            let hovered = hitbox.is_hovered_at(event.position, window);
            let thumb_hovered = hovered && chrome.thumb().contains(&event.position);
            let changed = chrome.hovered != hovered || chrome.thumb_hovered != thumb_hovered;
            if chrome.hovered && !hovered {
                chrome.last_scroll = Some(Instant::now());
            }
            chrome.hovered = hovered;
            chrome.thumb_hovered = thumb_hovered;
            if let Some(grab) = chrome.drag.filter(|_| event.dragging()) {
                chrome.set_drag_offset(event.position.y, grab, &scroll);
                cx.stop_propagation();
                cx.notify(view);
            } else if changed {
                cx.notify(view);
            }
        });
        let current = self.chrome.clone();
        let generation = state.generation.clone();
        let scroll = self.scroll.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if !phase.bubble() || event.button != MouseButton::Left {
                return;
            }
            let mut chrome = current.borrow_mut();
            if chrome.generation.matches(&generation) && chrome.drag.take().is_some() {
                scroll.end_drag();
                chrome.last_scroll = Some(Instant::now());
                cx.notify(view);
            }
        });
        let current = self.chrome.clone();
        let generation = state.generation.clone();
        let scroll = self.scroll.clone();
        window.on_mouse_event(move |event: &TouchDragEvent, phase, window, cx| {
            if !phase.bubble() {
                return;
            }
            let mut chrome = current.borrow_mut();
            if !chrome.current(&generation, window) {
                return;
            }
            if event.phase == TouchPhase::Started {
                if !chrome.published.visible
                    || window.default_prevented()
                    || !chrome
                        .thumb()
                        .intersect(&chrome.clip)
                        .contains(&event.start_position)
                {
                    return;
                }
                chrome.drag = Some(f32::from(event.start_position.y - chrome.thumb().top()));
                scroll.start_drag();
            }
            let Some(grab) = chrome.drag else {
                return;
            };
            if event.phase != TouchPhase::Cancelled {
                chrome.set_drag_offset(event.position.y, grab, &scroll);
            }
            if matches!(event.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                chrome.drag = None;
                chrome.last_scroll = Some(Instant::now());
                scroll.end_drag();
            }
            window.prevent_default();
            cx.stop_propagation();
            cx.notify(view);
        });
    }
}

fn render_chrome(state: ns::Signal<ChromeSnapshot>, _: mpsc::Sender<()>) -> ns::AnyElement {
    ns::view()
        .size_full()
        .relative()
        .accessibility_elements_hidden(true)
        .child(fade(state, true))
        .child(fade(state, false))
        .child(
            ns::view()
                .absolute()
                .right(ns::px(4.))
                .top(move || ns::px(state.get().thumb_start))
                .w(move || ns::px(state.get().thumb_width))
                .h(move || ns::px(state.get().thumb_length))
                .opacity(move || if state.get().visible { 1_f32 } else { 0_f32 })
                .bg(move || crate::nsh::color(state.get().thumb)),
        )
        .into_element()
}

fn fade(state: ns::Signal<ChromeSnapshot>, top: bool) -> ns::AnyElement {
    let edge = ns::view()
        .absolute()
        .left_0()
        .w(move || ns::px((state.get().width - Layout::DISCLOSURE_SCROLLBAR_GUTTER).max(0.)))
        .h(move || ns::px(state.get().fade_length()))
        .opacity(move || {
            let snapshot = state.get();
            snapshot.strength(if top { snapshot.before } else { snapshot.after })
        })
        .bg_linear(ns::deg(if top { 180. } else { 0. }))
        .gradient_from(move || crate::nsh::color(state.get().color).to_hsla())
        .gradient_to(move || {
            crate::nsh::color(gpui_kit::Rgba {
                a: 0.,
                ..state.get().color
            })
            .to_hsla()
        });
    if top {
        edge.top_0().into_element()
    } else {
        edge.bottom_0().into_element()
    }
}
