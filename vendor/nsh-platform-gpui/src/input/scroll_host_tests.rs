use std::{rc::Rc, sync::Arc};

use gpui::{ScrollDelta, ScrollWheelEvent, TextSystem, TouchPhase, point, px};
use nsh_core::{AttributedText, MountItem, Points, ResolvedTextStyle, TextEngine, ViewKind};

use super::{GesturePhase, ScrollOutcome, WheelEvent, compose_remainder};
use crate::{
    events::scroll_bridge::{self, ScrollRemainder, ScrollRoute},
    geom::rect,
    reading::{self, ReadingGeometry},
    scroll_scope::{NativeScope, ScrollScope},
    surface::Surface,
    text::GpuiTextEngine,
    view::NshView,
};

struct Fixture {
    surface: Rc<Surface>,
    outer: NshView,
    inner: NshView,
    horizontal: NshView,
    text: NshView,
    sibling: NshView,
}

impl Fixture {
    fn new(system: Arc<TextSystem>) -> Self {
        assert!(!system.all_font_names().is_empty(), "real fonts required");
        let engine = GpuiTextEngine::new(system, Arc::from(".SystemUIFont"));
        let mut content = AttributedText {
            selectable: Some(true),
            ..Default::default()
        };
        content.push(
            &"Real mounted proportional אבגד 😀 scroll ownership text wraps. ".repeat(12),
            ResolvedTextStyle {
                font_size: Points(18.0),
                ..Default::default()
            },
        );
        let content = Arc::new(content);
        let prepared = engine.prepare(&content);
        let size = engine.measure(&prepared, Some(Points(100.0)));
        let layout = engine.layout(&prepared, size);
        assert!(!engine.lines(&layout).is_empty(), "real layout required");
        let surface = Surface::new(nsh_engine::Environment::default());
        surface.resized(400.0, 240.0, 1.0);
        for (id, kind, parent) in [
            (2, ViewKind::Scroll, nsh_core::ROOT_ID),
            (3, ViewKind::Scroll, 2),
            (4, ViewKind::Scroll, 3),
            (5, ViewKind::Text, 4),
            (6, ViewKind::Scroll, nsh_core::ROOT_ID),
        ] {
            surface
                .mounter
                .borrow_mut()
                .apply(&surface, &MountItem::Create { id, kind });
            surface.mounter.borrow_mut().apply(
                &surface,
                &MountItem::Insert {
                    parent,
                    child: id,
                    before: None,
                },
            );
        }
        let outer = surface.view(2).expect("mounted outer scroll");
        let inner = surface.view(3).expect("mounted inner scroll");
        let horizontal = surface.view(4).expect("mounted horizontal scroll");
        let text = surface.view(5).expect("mounted text");
        let sibling = surface.view(6).expect("mounted sibling scroll");
        for (view, frame, width, height, horizontal) in [
            (&outer, rect(0.0, 0.0, 180.0, 120.0), 180.0, 160.0, false),
            (&inner, rect(0.0, 0.0, 140.0, 100.0), 140.0, 160.0, false),
            (&horizontal, rect(0.0, 0.0, 120.0, 80.0), 150.0, 80.0, true),
            (
                &sibling,
                rect(240.0, 0.0, 120.0, 100.0),
                120.0,
                500.0,
                false,
            ),
        ] {
            let mut state = view.state_mut();
            state.frame = frame;
            let scroll = state.scroll.as_mut().expect("actual core scroll state");
            scroll.content = nsh_core::Size::new(Points(width), Points(height));
            scroll.behavior.enabled = true;
            scroll.horizontal = horizontal;
        }
        text.state_mut().frame = rect(0.0, 0.0, 100.0, size.height.0);
        surface.mounter.borrow_mut().apply(
            &surface,
            &MountItem::Text {
                id: 5,
                text: content,
                layout,
                content: rect(0.0, 0.0, size.width.0, size.height.0),
            },
        );
        text.changed();
        surface.drawing();
        Self {
            surface,
            outer,
            inner,
            horizontal,
            text,
            sibling,
        }
    }

    fn geometry(&self) -> ReadingGeometry {
        ReadingGeometry::capture(
            &self.surface,
            &reading::snapshot(&self.surface, &self.text).expect("current mounted snapshot"),
        )
        .expect("real current reading geometry")
    }

    fn event(&self, dx: f32, dy: f32, precise: bool) -> WheelEvent {
        WheelEvent {
            x: 15.0,
            y: 15.0,
            dx,
            dy,
            precise,
            modifiers: Default::default(),
            time: self.surface.clock(),
        }
    }

    fn send(&self, dx: f32, dy: f32, phase: GesturePhase) -> ScrollOutcome {
        compose_remainder(&self.surface, self.event(dx, dy, true), Some(phase))
    }

    fn offsets(&self) -> [(f32, f32); 4] {
        [&self.horizontal, &self.inner, &self.outer, &self.sibling].map(offset)
    }

    fn bind_and_claim(&self, route: &ScrollRoute) {
        *self.surface.scroll_route.borrow_mut() = Some(route.clone());
        assert!(self.send(0.0, 0.0, GesturePhase::Began).owned);
        route.claim(&self.surface, &platform_event(TouchPhase::Started));
    }
}

fn offset(view: &NshView) -> (f32, f32) {
    let state = view.state();
    let scroll = state.scroll.as_ref().expect("mounted core scroll");
    (scroll.x.0, scroll.y.0)
}

fn position(view: &NshView, x: f32, y: f32) {
    let mut state = view.state_mut();
    let scroll = state.scroll.as_mut().expect("mounted core scroll");
    scroll.x = Points(x);
    scroll.y = Points(y);
}

fn platform_event(phase: TouchPhase) -> ScrollWheelEvent {
    ScrollWheelEvent {
        position: point(px(15.0), px(15.0)),
        delta: ScrollDelta::Pixels(point(px(0.0), px(0.0))),
        touch_phase: phase,
        ..Default::default()
    }
}

fn assert_revoked(geometry: &ReadingGeometry, fixture: &Fixture) {
    assert!(!geometry.current_for(&fixture.surface));
    assert!(geometry.range_rects(0, 0..1).is_none());
    let before = fixture.offsets();
    assert!(!geometry.scroll_anchor(&fixture.surface, fixture.inner.id(), 1.0));
    assert_eq!(fixture.offsets(), before);
}

#[doc = "Checks real geometry leases across zero-motion phases and composed or latched retirement."]
pub fn composed_zero_motion_boundaries_revoke_geometry_leases(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system);
    let original = fixture.offsets();
    let idle = fixture.geometry();
    assert!(!idle.scroll_anchor(&fixture.surface, fixture.inner.id(), 0.0));
    assert!(idle.current_for(&fixture.surface));
    for phase in [
        GesturePhase::Began,
        GesturePhase::Ended,
        GesturePhase::Cancelled,
    ] {
        let geometry = fixture.geometry();
        let result = fixture.send(0.0, 0.0, phase);
        assert_eq!(result.consumed, (0.0, 0.0));
        assert_eq!(result.remaining, (0.0, 0.0));
        assert!(result.owned);
        assert_eq!(fixture.offsets(), original);
        assert_revoked(&geometry, &fixture);
        fixture.surface.drawing();
    }
    assert!(!fixture.surface.input.reading_anchor_blocked());
    assert_revoked(&idle, &fixture);
    let geometry = fixture.geometry();
    let result = compose_remainder(
        &fixture.surface,
        WheelEvent {
            x: -1000.0,
            y: -1000.0,
            ..fixture.event(0.0, 0.0, true)
        },
        Some(GesturePhase::Began),
    );
    assert!(!result.owned);
    assert_eq!(fixture.offsets(), original);
    assert_revoked(&geometry, &fixture);
    assert!(fixture.send(0.0, 0.0, GesturePhase::Began).owned);
    fixture.surface.drawing();
    let active = fixture.geometry();
    assert!(active.current_for(&fixture.surface));
    assert!(fixture.surface.input.reading_anchor_blocked());
    assert!(!active.scroll_anchor(&fixture.surface, fixture.inner.id(), 1.0));
    assert_eq!(fixture.offsets(), original);
    super::retire_scroll_gesture(&fixture.surface);
    fixture.surface.drawing();
    assert!(!fixture.surface.input.reading_anchor_blocked());
    assert_revoked(&active, &fixture);
    let fresh = fixture.geometry();
    assert!(fresh.scroll_anchor(&fixture.surface, fixture.inner.id(), 1.0));
    assert_eq!(offset(&fixture.inner), (0.0, 1.0));
    assert!(compose_remainder(&fixture.surface, fixture.event(0.0, 1.0, false), None).owned);
    fixture.surface.drawing();
    let latched = fixture.geometry();
    assert!(fixture.surface.input.reading_anchor_blocked());
    let before = fixture.offsets();
    super::retire_scroll_gesture(&fixture.surface);
    fixture.surface.drawing();
    assert!(!fixture.surface.input.reading_anchor_blocked());
    assert_eq!(fixture.offsets(), before);
    assert_revoked(&latched, &fixture);
}

#[doc = "Checks retained precise owners and mouse latch expiry over mounted sibling scroll views."]
pub fn composed_owner_and_mouse_latch_keep_original_mounted_path(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system.clone());
    assert!(fixture.send(0.0, 0.0, GesturePhase::Began).owned);
    let original_target = fixture
        .surface
        .input
        .composition
        .borrow()
        .as_ref()
        .expect("owner")
        .target
        .clone();
    assert_eq!(original_target, fixture.text);
    let result = compose_remainder(
        &fixture.surface,
        WheelEvent {
            x: 255.0,
            ..fixture.event(0.0, 75.0, true)
        },
        Some(GesturePhase::Moved),
    );
    assert_eq!(result.consumed, (0.0, 75.0));
    assert_eq!(result.remaining, (0.0, 0.0));
    assert!(result.owned);
    assert_eq!(offset(&fixture.inner), (0.0, 60.0));
    assert_eq!(offset(&fixture.outer), (0.0, 15.0));
    assert_eq!(offset(&fixture.sibling), (0.0, 0.0));
    assert_eq!(
        fixture
            .surface
            .input
            .composition
            .borrow()
            .as_ref()
            .expect("retained owner")
            .target,
        original_target
    );
    fixture.send(0.0, 0.0, GesturePhase::Cancelled);

    let mouse = Fixture::new(system);
    let first = mouse.event(0.0, 5.0, false);
    assert!(compose_remainder(&mouse.surface, first, None).owned);
    let next = WheelEvent {
        x: 255.0,
        time: first.time + 0.01,
        ..first
    };
    assert!(compose_remainder(&mouse.surface, next, None).owned);
    assert_eq!(offset(&mouse.inner), (0.0, 10.0));
    assert_eq!(offset(&mouse.sibling), (0.0, 0.0));
    assert!(
        compose_remainder(
            &mouse.surface,
            WheelEvent {
                time: next.time + nsh_core::input::WHEEL_LATCH / 1000.0 + 0.01,
                ..next
            },
            None,
        )
        .owned
    );
    assert_eq!(offset(&mouse.inner), (0.0, 10.0));
    assert_eq!(offset(&mouse.sibling), (0.0, 5.0));
    super::retire_scroll_gesture(&mouse.surface);
}

#[doc = "Checks route cancellation, detach, native scope identity replacement, and drop retirement."]
pub fn composed_route_and_native_scope_retirement_revoke_authority(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system);
    let route = ScrollRoute::new();
    fixture.bind_and_claim(&route);
    assert!(route.owner(&platform_event(TouchPhase::Moved)).is_some());
    fixture.surface.drawing();
    let geometry = fixture.geometry();
    route.cancel();
    assert!(route.owner(&platform_event(TouchPhase::Moved)).is_none());
    assert!(!fixture.surface.input.reading_anchor_blocked());
    fixture.surface.drawing();
    assert_revoked(&geometry, &fixture);

    fixture.bind_and_claim(&route);
    fixture.horizontal.unparent();
    assert!(route.owner(&platform_event(TouchPhase::Moved)).is_none());
    fixture.horizontal.insert_before(&fixture.inner, None);
    fixture.surface.drawing();
    assert!(
        !fixture
            .surface
            .input
            .owns_scroll_gesture(fixture.surface.clock())
    );
    assert!(!fixture.surface.input.reading_anchor_blocked());
    assert!(route.owner(&platform_event(TouchPhase::Moved)).is_none());
    let first_parent = ScrollScope::new(ScrollRoute::new(), |_, _, _| {});
    let replacement_parent = ScrollScope::new(ScrollRoute::new(), |_, _, _| {});
    let native =
        first_parent.with(|| NativeScope::new(&fixture.surface, fixture.horizontal.clone()));
    assert!(native.matches(&fixture.surface, &fixture.horizontal, Some(&first_parent)));
    assert!(native.matches(
        &fixture.surface,
        &fixture.horizontal,
        Some(&first_parent.clone())
    ));
    assert!(!native.matches(
        &fixture.surface,
        &fixture.horizontal,
        Some(&replacement_parent)
    ));
    assert!(!native.matches(&fixture.surface, &fixture.sibling, Some(&first_parent)));
    fixture.bind_and_claim(&native.scope.route);
    fixture.surface.drawing();
    let geometry = fixture.geometry();
    let native_route = native.scope.route.clone();
    drop(native);
    assert!(
        native_route
            .owner(&platform_event(TouchPhase::Moved))
            .is_none()
    );
    assert!(!fixture.surface.input.reading_anchor_blocked());
    fixture.surface.drawing();
    assert_revoked(&geometry, &fixture);
    let rebound =
        replacement_parent.with(|| NativeScope::new(&fixture.surface, fixture.horizontal.clone()));
    assert!(rebound.matches(
        &fixture.surface,
        &fixture.horizontal,
        Some(&replacement_parent)
    ));
    assert!(!rebound.matches(&fixture.surface, &fixture.horizontal, Some(&first_parent)));
    fixture.bind_and_claim(&rebound.scope.route);
    assert!(
        rebound
            .scope
            .route
            .owner(&platform_event(TouchPhase::Moved))
            .is_some()
    );
    drop(rebound);
    assert!(!fixture.surface.input.reading_anchor_blocked());
}

#[doc = "Checks generation, visibility, and weak Surface lifetime gates on actual route ownership."]
pub fn composed_generation_visibility_and_drop_retire_route_owner(system: Arc<TextSystem>) {
    for generation_changed in [true, false] {
        let fixture = Fixture::new(system.clone());
        let route = ScrollRoute::new();
        fixture.bind_and_claim(&route);
        let original = fixture.offsets();
        if generation_changed {
            fixture
                .surface
                .generation
                .set(fixture.surface.generation.get().wrapping_add(1));
        } else {
            fixture.surface.window_hidden.set(true);
        }
        assert!(route.owner(&platform_event(TouchPhase::Moved)).is_none());
        assert!(!fixture.surface.input.reading_anchor_blocked());
        assert_eq!(fixture.offsets(), original);
        fixture.surface.window_hidden.set(false);
        assert!(route.owner(&platform_event(TouchPhase::Moved)).is_none());
    }
    let route = ScrollRoute::new();
    let fixture = Fixture::new(system);
    fixture.bind_and_claim(&route);
    let weak = Rc::downgrade(&fixture.surface);
    drop(fixture);
    assert!(weak.upgrade().is_none());
    assert!(route.owner(&platform_event(TouchPhase::Moved)).is_none());
    route.cancel();
}

#[doc = "Checks real clamped offsets, mirrored mixed-axis remainder, ancestor forwarding, and units."]
pub fn composed_real_offsets_preserve_mixed_axis_remainder_and_units(system: Arc<TextSystem>) {
    for pixels in [true, false] {
        let fixture = Fixture::new(system.clone());
        fixture
            .horizontal
            .state_mut()
            .scroll
            .as_mut()
            .expect("scroll")
            .mirrored = true;
        position(&fixture.horizontal, 20.0, 0.0);
        position(&fixture.inner, 0.0, 50.0);
        position(&fixture.outer, 0.0, 30.0);
        let event = ScrollWheelEvent {
            delta: if pixels {
                ScrollDelta::Pixels(point(px(50.0), px(-35.0)))
            } else {
                ScrollDelta::Lines(point(5.0, -3.5))
            },
            ..platform_event(TouchPhase::Moved)
        };
        let pending = scroll_bridge::remainder(
            &event,
            ScrollOutcome {
                remaining: (-50.0, 35.0),
                ..Default::default()
            },
        );
        let original_delta = pending.delta;
        let result = scroll_bridge::consume_from(
            &fixture.surface,
            Some(fixture.horizontal.clone()),
            pending,
        );
        assert_eq!(offset(&fixture.horizontal), (30.0, 0.0));
        assert_eq!(offset(&fixture.inner), (0.0, 60.0));
        assert_eq!(offset(&fixture.outer), (0.0, 40.0));
        assert_eq!(offset(&fixture.sibling), (0.0, 0.0));
        assert_eq!(result.outcome.consumed, (-10.0, 20.0));
        assert_eq!(result.outcome.remaining, (-40.0, 15.0));
        assert!(result.outcome.owned);
        assert!(!result.outcome.editor_claimed);
        match (original_delta, result.delta) {
            (ScrollDelta::Pixels(original), ScrollDelta::Pixels(actual)) => {
                assert_eq!(f32::from(actual.x), f32::from(original.x) * 0.8);
                assert_eq!(f32::from(actual.y), -15.0);
            }
            (ScrollDelta::Lines(original), ScrollDelta::Lines(actual)) => {
                assert_eq!(actual.x, original.x * 0.8);
                assert_eq!(actual.y, original.y * (15.0 / 35.0));
            }
            _ => panic!("original scroll units must be retained"),
        }
        let ancestor = Fixture::new(system.clone());
        let result = scroll_bridge::consume_from(
            &ancestor.surface,
            Some(ancestor.outer.clone()),
            ScrollRemainder {
                position: point(px(255.0), px(15.0)),
                ..result
            },
        );
        assert_eq!(offset(&ancestor.outer), (0.0, 15.0));
        assert_eq!(offset(&ancestor.inner), (0.0, 0.0));
        assert_eq!(offset(&ancestor.sibling), (0.0, 0.0));
        assert_eq!(result.outcome.consumed, (-10.0, 35.0));
        assert_eq!(result.outcome.remaining, (-40.0, 0.0));
        super::retire_scroll_gesture(&fixture.surface);
        super::retire_scroll_gesture(&ancestor.surface);
    }
    let fixture = Fixture::new(system);
    fixture
        .inner
        .state_mut()
        .scroll
        .as_mut()
        .expect("scroll")
        .behavior
        .enabled = false;
    fixture
        .outer
        .state_mut()
        .scroll
        .as_mut()
        .expect("scroll")
        .behavior
        .enabled = false;
    let result = compose_remainder(&fixture.surface, fixture.event(0.0, 45.0, false), None);
    assert_eq!(offset(&fixture.horizontal), (30.0, 0.0));
    assert_eq!(result.consumed, (0.0, 30.0));
    assert_eq!(result.remaining, (0.0, 15.0));
    super::retire_scroll_gesture(&fixture.surface);
}

#[doc = "Checks terminal delta and OS momentum consumption without adding a core-driven fling."]
pub fn composed_terminal_delta_and_system_momentum_do_not_add_glide(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system);
    assert!(fixture.send(0.0, 0.0, GesturePhase::Began).owned);
    assert_eq!(
        fixture.send(0.0, 12.0, GesturePhase::Moved).consumed,
        (0.0, 12.0)
    );
    let end = fixture.send(0.0, 7.0, GesturePhase::Ended);
    assert_eq!(end.consumed, (0.0, 7.0));
    assert_eq!(end.remaining, (0.0, 0.0));
    assert!(end.owned);
    assert_eq!(offset(&fixture.inner), (0.0, 19.0));
    assert!(fixture.surface.input.reading_anchor_blocked());
    let momentum = compose_remainder(
        &fixture.surface,
        WheelEvent {
            x: 255.0,
            ..fixture.event(0.0, 9.0, true)
        },
        Some(GesturePhase::Moved),
    );
    assert_eq!(momentum.consumed, (0.0, 9.0));
    assert!(momentum.owned);
    assert_eq!(offset(&fixture.inner), (0.0, 28.0));
    assert_eq!(offset(&fixture.sibling), (0.0, 0.0));
    let before = fixture.offsets();
    let now = fixture.surface.clock() * 1000.0;
    for delta in [16.0, 250.0, 1000.0] {
        for view in [&fixture.horizontal, &fixture.inner, &fixture.outer] {
            crate::scroll::reading_geometry_scroll_frame(&fixture.surface, view, now + delta);
        }
        assert_eq!(fixture.offsets(), before);
    }
    assert_eq!(
        fixture.send(0.0, 0.0, GesturePhase::Cancelled).consumed,
        (0.0, 0.0)
    );
    assert!(!fixture.surface.input.reading_anchor_blocked());
    for view in [&fixture.horizontal, &fixture.inner, &fixture.outer] {
        crate::scroll::reading_geometry_scroll_frame(&fixture.surface, view, now + 2000.0);
    }
    assert_eq!(fixture.offsets(), before);
    let no_owner_terminal = fixture.send(0.0, 7.0, GesturePhase::Ended);
    assert!(!no_owner_terminal.owned);
    assert_eq!(no_owner_terminal.consumed, (0.0, 0.0));
    assert_eq!(no_owner_terminal.remaining, (0.0, 7.0));
    assert_eq!(fixture.offsets(), before);
}
