use gpui_kit::{base::ScrollbarHandle, prelude::*, *};
use nsh_platform_gpui::{ScrollRemainder, ScrollRoute, ScrollScope};
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    time::Instant,
};

use crate::nsh::ProjectionId;

#[cfg(feature = "nsh-reading-integration")]
pub mod host_regressions;

struct Owner {
    precise: bool,
    horizontal: bool,
    phased: bool,
    ended: bool,
    last: Instant,
}

impl Owner {
    fn holds(&self, event: &ScrollWheelEvent) -> bool {
        let age = self.last.elapsed().as_secs_f64() * 1000.;
        let delta = event.delta.pixel_delta(px(1.));
        if event.delta.precise() {
            self.precise
                && event.touch_phase != TouchPhase::Started
                && ((self.phased && !self.ended) || age < nsh_core::input::TOUCHPAD_IDLE)
        } else {
            !self.precise
                && nsh_core::input::latch_holds(
                    self.horizontal,
                    0.,
                    delta.x.abs() > delta.y.abs(),
                    age,
                )
        }
    }
}

struct State {
    scroll: ScrollHandle,
    parent: Option<ScrollScope>,
    scope: Option<ScrollScope>,
    owner: Option<Owner>,
    generation: ProjectionId,
    mount: Weak<Mount>,
    hitbox: Option<Hitbox>,
    viewport: Size<Pixels>,
    scale: f32,
    visible: bool,
}

impl State {
    fn retire(&mut self) {
        self.owner = None;
        self.generation = ProjectionId::new();
        if let Some(scope) = self.scope.take() {
            scope.cancel();
        }
    }

    fn current(&self, generation: &ProjectionId, window: &Window) -> bool {
        self.mount.upgrade().is_some()
            && self.generation.matches(generation)
            && self.visible
            && self.viewport == window.viewport_size()
            && self.scale == window.scale_factor()
            && window.is_visible()
            && window.is_window_active()
    }
}

struct Mount(Weak<RefCell<State>>);

impl Drop for Mount {
    fn drop(&mut self) {
        if let Some(state) = self.0.upgrade() {
            state.borrow_mut().retire();
        }
    }
}

pub(crate) fn chrome(
    id: SharedString,
    child: AnyElement,
    scroll: ScrollHandle,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = state(id.clone(), &scroll, window, cx);
    let retained = state.borrow().mount.upgrade();
    let mount = retained.unwrap_or_else(|| {
        let mount = Rc::new(Mount(Rc::downgrade(&state)));
        state.borrow_mut().mount = Rc::downgrade(&mount);
        mount
    });
    Group {
        id: format!("{id}-protocol").into(),
        child: super::scroll_chrome(id, child, scroll, window, cx),
        state,
        mount,
    }
    .into_any_element()
}

fn state(
    id: SharedString,
    scroll: &ScrollHandle,
    window: &mut Window,
    cx: &mut App,
) -> Rc<RefCell<State>> {
    let id = SharedString::from(format!("{id}-scroll-state"));
    window
        .use_keyed_state(id, cx, |window, _| {
            Rc::new(RefCell::new(State {
                scroll: scroll.clone(),
                parent: None,
                scope: None,
                owner: None,
                generation: ProjectionId::new(),
                mount: Weak::new(),
                hitbox: None,
                viewport: window.viewport_size(),
                scale: window.scale_factor(),
                visible: false,
            }))
        })
        .read(cx)
        .clone()
}

pub(crate) fn listener(
    id: SharedString,
    scroll: &ScrollHandle,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = state(id, scroll, window, cx);
    canvas(
        |_, _, _| (),
        move |_, (), window, _| {
            let generation = state.borrow().generation.clone();
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if !phase.bubble() {
                    return;
                }
                let eligible = {
                    let state = state.borrow();
                    state.current(&generation, window)
                        && state
                            .hitbox
                            .as_ref()
                            .is_some_and(|hitbox| hitbox.should_handle_scroll(window))
                };
                if eligible {
                    ordinary(&state, event, window, cx);
                }
            });
        },
    )
    .absolute()
    .w_0()
    .h_0()
    .into_any_element()
}

fn finite(remaining: ScrollRemainder) -> bool {
    let delta = remaining.delta.pixel_delta(px(1.));
    [
        f32::from(delta.x),
        f32::from(delta.y),
        remaining.outcome.remaining.0,
        remaining.outcome.remaining.1,
        remaining.outcome.consumed.0,
        remaining.outcome.consumed.1,
        f32::from(remaining.position.x),
        f32::from(remaining.position.y),
    ]
    .into_iter()
    .all(f32::is_finite)
}

fn consume(
    scroll: &ScrollHandle,
    mut remaining: ScrollRemainder,
    line_height: Pixels,
) -> ScrollRemainder {
    if !finite(remaining) || !f32::from(line_height).is_finite() || line_height <= px(0.) {
        return remaining;
    }
    let delta = remaining.delta.pixel_delta(line_height);
    let before = scroll.offset();
    let maximum = scroll.max_offset().y;
    if ![f32::from(before.x), f32::from(before.y), f32::from(maximum)]
        .into_iter()
        .all(f32::is_finite)
        || maximum < px(0.)
        || before.y < -maximum
        || before.y > px(0.)
        || scroll.viewport_bounds().is_empty()
    {
        return remaining;
    }
    let vertical = delta.y != px(0.);
    let requested = if vertical { delta.y } else { delta.x };
    let target = (before.y + requested).clamp(-maximum, px(0.));
    if target == before.y {
        return remaining;
    }
    scroll.set_offset(point(before.x, target));
    let moved = scroll.offset().y - before.y;
    let fraction = f32::from(moved) / f32::from(requested);
    let shifted = remaining.modifiers.shift
        && matches!(remaining.delta, ScrollDelta::Lines(delta) if delta.x == 0.);
    let axis = usize::from(vertical != shifted);
    let original = if axis == 1 {
        remaining.outcome.remaining.1
    } else {
        remaining.outcome.remaining.0
    };
    let consumed = original * fraction;
    if axis == 1 {
        remaining.outcome.remaining.1 -= consumed;
        remaining.outcome.consumed.1 += consumed;
    } else {
        remaining.outcome.remaining.0 -= consumed;
        remaining.outcome.consumed.0 += consumed;
    }
    remaining.delta = match remaining.delta {
        ScrollDelta::Pixels(mut delta) => {
            if vertical {
                delta.y -= moved;
            } else {
                delta.x -= moved;
            }
            ScrollDelta::Pixels(delta)
        }
        ScrollDelta::Lines(mut delta) => {
            let moved = f32::from(moved) / f32::from(line_height);
            if vertical {
                delta.y -= moved;
            } else {
                delta.x -= moved;
            }
            ScrollDelta::Lines(delta)
        }
    };
    remaining
}

fn forward(
    state: &Rc<RefCell<State>>,
    remaining: ScrollRemainder,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let (scroll, parent) = {
        let state = state.borrow();
        (state.scroll.clone(), state.parent.clone())
    };
    let before = scroll.offset();
    let remaining = consume(&scroll, remaining, window.line_height());
    let moved = scroll.offset() != before;
    if moved {
        window.refresh();
    }
    if let Some(parent) = &parent {
        parent.consume(remaining, window, cx);
    }
    moved || parent.is_some()
}

fn ordinary(
    state: &Rc<RefCell<State>>,
    event: &ScrollWheelEvent,
    window: &mut Window,
    cx: &mut App,
) {
    let delta = event.delta.pixel_delta(window.line_height());
    let remaining = ScrollRemainder::from_event(event);
    if !finite(remaining) {
        state.borrow_mut().owner = None;
        cx.stop_propagation();
        return;
    }
    let terminal = matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled);
    let owned = state.borrow().owner.is_some();
    if !forward(state, remaining, window, cx) && !owned {
        return;
    }
    let mut state = state.borrow_mut();
    if event.touch_phase == TouchPhase::Cancelled {
        state.owner = None;
    } else if !terminal || owned {
        let previous = state.owner.take();
        state.owner = Some(Owner {
            precise: event.delta.precise(),
            horizontal: delta.x.abs() > delta.y.abs(),
            phased: event.touch_phase == TouchPhase::Started
                || previous.as_ref().is_some_and(|owner| owner.phased),
            ended: terminal || previous.as_ref().is_some_and(|owner| owner.ended),
            last: Instant::now(),
        });
    }
    drop(state);
    cx.stop_propagation();
}

struct Group {
    id: ElementId,
    child: AnyElement,
    state: Rc<RefCell<State>>,
    mount: Rc<Mount>,
}

impl IntoElement for Group {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Group {
    type RequestLayoutState = ();
    type PrepaintState = ProjectionId;

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
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> ProjectionId {
        if let Some(id) = id {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.mount.clone()));
        }
        let parent = ScrollScope::current();
        let scope = {
            let mut state = self.state.borrow_mut();
            let same = match (&state.parent, &parent) {
                (Some(previous), Some(parent)) => previous.same(parent),
                (None, None) => true,
                _ => false,
            };
            if !same
                || state.viewport != window.viewport_size()
                || state.scale != window.scale_factor()
                || !window.is_visible()
                || !window.is_window_active()
            {
                state.retire();
            }
            state.parent = parent;
            state.viewport = window.viewport_size();
            state.scale = window.scale_factor();
            state.visible = !bounds.intersect(&window.content_mask().bounds).is_empty();
            if !state.visible {
                state.retire();
            }
            state.hitbox = Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));
            state.scope.clone()
        };
        let scope = scope.unwrap_or_else(|| {
            let weak = Rc::downgrade(&self.state);
            let generation = self.state.borrow().generation.clone();
            let scope = ScrollScope::new(ScrollRoute::new(), move |remaining, window, cx| {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                let current = {
                    let state = state.borrow();
                    state.current(&generation, window)
                };
                if current && finite(remaining) {
                    forward(&state, remaining, window, cx);
                }
            });
            self.state.borrow_mut().scope = Some(scope.clone());
            scope
        });
        scope.with(|| self.child.prepaint(window, cx));
        self.state.borrow().generation.clone()
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        generation: &mut ProjectionId,
        window: &mut Window,
        cx: &mut App,
    ) {
        let current = self.state.clone();
        let generation = generation.clone();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if !phase.capture() {
                return;
            }
            let owned = {
                let mut state = current.borrow_mut();
                if !state.current(&generation, window) {
                    state.retire();
                    false
                } else if state.owner.as_ref().is_some_and(|owner| owner.holds(event)) {
                    true
                } else {
                    state.owner = None;
                    false
                }
            };
            if owned {
                ordinary(&current, event, window, cx);
            }
        });
        let scope = self.state.borrow().scope.clone();
        if let Some(scope) = scope {
            scope.with(|| self.child.paint(window, cx));
        }
    }
}
