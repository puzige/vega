use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use gpui::{App, DispatchPhase, ScrollDelta, ScrollEvent, TouchPhase, Window};

use crate::{app::AppView, input::ScrollOutcome, surface::Surface};

#[doc = "Single-event remainder preserving original GPUI delta units, phase and pointer context."]
#[derive(Clone, Copy, Debug)]
pub struct ScrollRemainder {
    #[doc = "Measured native consumption and remaining content-point delta."]
    pub outcome: ScrollOutcome,
    #[doc = "Remaining GPUI-signed delta in the original pixel or line units."]
    pub delta: ScrollDelta,
    #[doc = "Original gesture phase, including terminal events with nonzero delta."]
    pub phase: TouchPhase,
    #[doc = "Original pointer location in window logical pixels."]
    pub position: gpui::Point<gpui::Pixels>,
    #[doc = "Original keyboard modifiers."]
    pub modifiers: gpui::Modifiers,
}

impl ScrollRemainder {
    #[doc = "Normalizes an original host event using this backend's native units and Shift axis policy, without dispatching it."]
    pub fn from_event(event: &ScrollEvent) -> Self {
        let (mut dx, mut dy, precise) = super::scroll_delta(event);
        if !precise && dx == 0.0 && event.modifiers.shift { (dx, dy) = (dy, dx); }
        remainder(event, ScrollOutcome { remaining: (dx, dy), ..ScrollOutcome::default() })
    }
}

type Consumer = RefCell<Box<dyn FnMut(ScrollRemainder, &mut Window, &mut App)>>;

struct Owner {
    surface: Weak<Surface>,
    generation: u64,
    last: std::time::Instant,
    precise: bool,
    horizontal: bool,
    ended: bool,
    phased: bool,
}

#[doc = "Retained gesture owner and synchronous single-event remainder delivery route."]
#[derive(Clone, Default)]
pub struct ScrollRoute {
    owner: Rc<RefCell<Option<Owner>>>,
    consumer: Rc<RefCell<Option<Weak<Consumer>>>>,
}

impl ScrollRoute {
    #[doc = "Creates an unbound route without gesture ownership."]
    pub fn new() -> Self {
        Self::default()
    }

    #[doc = "Installs this frame's capture listener and synchronous remainder consumer on the host window."]
    pub fn install(
        &self,
        window: &mut Window,
        consume: impl FnMut(ScrollRemainder, &mut Window, &mut App) + 'static,
    ) {
        let consumer: Rc<Consumer> = Rc::new(RefCell::new(Box::new(consume)));
        *self.consumer.borrow_mut() = Some(Rc::downgrade(&consumer));
        let route = self.clone();
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            let _keep_consumer = &consumer;
            if phase != DispatchPhase::Capture {
                return;
            }
            if !window.is_window_active() {
                route.cancel();
                return;
            }
            if let Some(surface) = route.owner(event) {
                super::dispatch_scroll(&surface, event, window, cx);
            }
        });
    }

    #[doc = "Retires the current owner and its native motion and geometry authority without replay."]
    pub fn cancel(&self) {
        let owner = self.owner.borrow_mut().take();
        if let Some(surface) = owner.and_then(|o| o.surface.upgrade()) {
            crate::input::retire_scroll_gesture(&surface);
        }
    }

    pub(crate) fn deliver(
        &self,
        pending: ScrollRemainder,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let consumer = self.consumer.borrow().as_ref().and_then(Weak::upgrade);
        let Some(consumer) = consumer else {
            return false;
        };
        let Ok(mut consume) = consumer.try_borrow_mut() else {
            return false;
        };
        consume(pending, window, cx);
        true
    }

    pub(crate) fn release(&self, surface: &Surface) {
        let ours = self.owner.borrow().as_ref().is_some_and(|o| {
            o.surface
                .upgrade()
                .is_some_and(|s| s.key() == surface.key())
        });
        if ours {
            self.owner.borrow_mut().take();
        }
    }

    pub(crate) fn owner(&self, event: &ScrollEvent) -> Option<Rc<Surface>> {
        let (dx, dy, precise) = super::scroll_delta(event);
        let mut owner = self.owner.borrow_mut();
        let valid = owner.as_ref().is_some_and(|o| {
            let age = o.last.elapsed().as_secs_f64() * 1000.0;
            let alive = o.surface.upgrade().is_some_and(|s| {
                s.generation.get() == o.generation
                    && !s.window_hidden.get()
                    && s.input.owns_scroll_gesture(s.clock())
                    && s.scroll_route
                        .borrow()
                        .as_ref()
                        .is_some_and(|r| Rc::ptr_eq(&self.owner, &r.owner))
            });
            alive
                && if precise {
                    o.precise
                        && event.touch_phase != TouchPhase::Started
                        && ((o.phased && !o.ended) || age < nsh_core::input::TOUCHPAD_IDLE)
                } else {
                    !o.precise
                        && nsh_core::input::latch_holds(o.horizontal, 0.0, dx.abs() > dy.abs(), age)
                }
        });
        if valid {
            owner.as_ref().and_then(|o| o.surface.upgrade())
        } else {
            let previous = owner.take();
            drop(owner);
            if let Some(surface) = previous.and_then(|o| o.surface.upgrade()) {
                crate::input::retire_scroll_gesture(&surface);
            }
            None
        }
    }

    pub(crate) fn claim(&self, surface: &Rc<Surface>, event: &ScrollEvent) {
        let (dx, dy, precise) = super::scroll_delta(event);
        let previous = self.owner.borrow_mut().take();
        let same = previous.as_ref().is_some_and(|o| {
            o.generation == surface.generation.get()
                && o.surface.upgrade().is_some_and(|s| Rc::ptr_eq(&s, surface))
                && event.touch_phase != TouchPhase::Started
        });
        *self.owner.borrow_mut() = Some(Owner {
            surface: Rc::downgrade(surface),
            generation: surface.generation.get(),
            last: std::time::Instant::now(),
            precise,
            horizontal: dx.abs() > dy.abs(),
            ended: precise
                && (event.touch_phase == TouchPhase::Ended
                    || same && previous.as_ref().is_some_and(|o| o.ended)),
            phased: precise
                && (event.touch_phase == TouchPhase::Started
                    || same && previous.as_ref().is_some_and(|o| o.phased)),
        });
    }
}

impl AppView {
    #[doc = "Binds a route, retiring prior gesture authority when route identity changes."]
    pub fn set_scroll_route(&self, route: Option<ScrollRoute>) {
        let previous = self.surface().scroll_route.borrow().clone();
        if previous
            .as_ref()
            .zip(route.as_ref())
            .is_some_and(|(a, b)| Rc::ptr_eq(&a.owner, &b.owner))
        {
            return;
        }
        if let Some(previous) = previous {
            previous.release(self.surface());
        }
        crate::input::retire_scroll_gesture(self.surface());
        *self.surface().scroll_route.borrow_mut() = route;
    }

    #[doc = "Takes a pending undelivered remainder once; this does not dispatch an input event."]
    pub fn take_scroll_remainder(&self) -> Option<ScrollRemainder> {
        self.surface().scroll_pending.borrow_mut().take()
    }

    #[doc = "Reports whether this Surface currently retains native gesture ownership."]
    pub fn owns_scroll_gesture(&self) -> bool {
        self.surface()
            .input
            .owns_scroll_gesture(self.surface().clock())
    }

    #[doc = "Consumes a remainder through this Surface's native ancestors and returns only the residual delta."]
    pub fn consume_scroll_remainder(&self, pending: ScrollRemainder) -> ScrollRemainder {
        consume_surface(self.surface(), pending)
    }
}

pub(crate) fn consume_surface(surface: &Rc<Surface>, pending: ScrollRemainder) -> ScrollRemainder {
    consume_from(surface, None, pending)
}

pub(crate) fn consume_from(
    surface: &Rc<Surface>,
    target: Option<crate::view::NshView>,
    mut pending: ScrollRemainder,
) -> ScrollRemainder {
    if surface.window_hidden.get() {
        return pending;
    }
    let (x, y) = super::root_point(surface, pending.position);
    let (dx, dy) = pending.outcome.remaining;
    let precise = matches!(pending.delta, ScrollDelta::Pixels(_));
    let event = crate::input::WheelEvent {
        x,
        y,
        dx,
        dy,
        precise,
        modifiers: crate::keys::modifiers(&pending.modifiers),
        time: surface.clock(),
    };
    let phase = precise.then(|| super::gesture_phase(pending.phase));
    let outcome = if let Some(target) = target {
        crate::input::compose_ancestor(surface, target, event, phase)
    } else {
        crate::input::compose_remainder(surface, event, phase)
    };
    pending.outcome.consumed.0 += outcome.consumed.0;
    pending.outcome.consumed.1 += outcome.consumed.1;
    pending.outcome.remaining = outcome.remaining;
    pending.outcome.owned |= outcome.owned;
    pending.outcome.editor_claimed |= outcome.editor_claimed;
    pending.delta = remaining_delta(pending.delta, pending.modifiers, pending.outcome.remaining);
    pending
}

fn remaining_delta(
    delta: ScrollDelta,
    modifiers: gpui::Modifiers,
    remaining: (f32, f32),
) -> ScrollDelta {
    let (mut x, mut y) = remaining;
    if let ScrollDelta::Lines(original) = delta
        && modifiers.shift
        && original.x == 0.0
    {
        (x, y) = (y, x);
    }
    match delta {
        ScrollDelta::Pixels(_) => ScrollDelta::Pixels(gpui::point(gpui::px(-x), gpui::px(-y))),
        ScrollDelta::Lines(_) => {
            ScrollDelta::Lines(gpui::point(-x / super::LINE, -y / super::LINE))
        }
    }
}

pub(crate) fn remainder(event: &ScrollEvent, outcome: ScrollOutcome) -> ScrollRemainder {
    ScrollRemainder {
        outcome,
        delta: remaining_delta(event.delta, event.modifiers, outcome.remaining),
        phase: event.touch_phase,
        position: event.position,
        modifiers: event.modifiers,
    }
}
