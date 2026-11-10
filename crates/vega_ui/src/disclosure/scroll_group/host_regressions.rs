use super::*;
use crate::conversation_stream::{
    ConversationStream, ReadingList,
    scroll_composition_integration::{BodyFactory, BodyKind, BodyMount},
};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_DRAIN_TICKS: usize = 512;
const EPSILON: f32 = 0.5;

struct Root {
    list: ListState,
    scroll: ScrollHandle,
    scope: ScrollScope,
    body: Option<Rc<BodyMount>>,
    mounted: bool,
    observed: Rc<RefCell<Option<Rc<RefCell<State>>>>>,
}

fn owns_stream(stream: &WeakEntity<ConversationStream>, window: &Window, cx: &App) -> bool {
    stream.upgrade().is_some()
        && window.is_window_active()
        && window.root::<Root>().flatten().is_some_and(|root| {
            let root = root.read(cx);
            root.mounted
                && root
                    .body
                    .as_ref()
                    .is_some_and(|body| &body.stream == stream)
        })
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let scroll = self.scroll.clone();
        let body = self.body.clone();
        let owners = body.iter().map(|body| body.stream.clone()).collect();
        let mounted = self.mounted;
        let observed = self.observed.clone();
        let list = list(self.list.clone(), move |_, window, cx| {
            let mut row = div().w_full().h(px(4096.)).flex().flex_col();
            if mounted {
                let mut child = div()
                    .id("original-group-div")
                    .relative()
                    .w_full()
                    .h(px(160.))
                    .overflow_x_hidden()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .flex()
                    .flex_col()
                    .child(listener("group-host".into(), &scroll, window, cx));
                if let Some(body) = &body {
                    child = child.child(
                        div()
                            .h(px(128.))
                            .flex_shrink_0()
                            .child((body.render)(window, cx)),
                    );
                }
                child = child.child(div().h(px(1024.)).flex_shrink_0());
                row = row.child(chrome(
                    "group-host".into(),
                    child.into_any_element(),
                    scroll.clone(),
                    window,
                    cx,
                ));
                *observed.borrow_mut() = Some(state("group-host".into(), &scroll, window, cx));
            }
            row.into_any_element()
        })
        .size_full()
        .into_any_element();
        let child = ReadingList::with_owners(owners, list).into_any_element();
        crate::nsh::scroll::scoped(self.scope.clone(), child)
    }
}

struct Fixture {
    visual: VisualTestContext,
    root: Entity<Root>,
    scroll: ScrollHandle,
    list: ListState,
    body: Option<Rc<BodyMount>>,
}

impl Fixture {
    fn new(cx: &mut TestAppContext, body: Option<(BodyFactory, BodyKind)>) -> Self {
        cx.update(|cx| cx.set_global(vega_theme::Theme::dark()));
        let window = cx.open_window(size(px(360.), px(560.)), move |window, cx| {
            let list = ListState::new(1, ListAlignment::Top, px(600.));
            let outer = list.clone();
            let body = body.map(|(factory, kind)| {
                let body = factory(kind, 0, list.clone(), window, cx);
                body.stream
                    .upgrade()
                    .expect("production body stream")
                    .update(cx, |stream, cx| {
                        stream.set_reading_navigation_authority(owns_stream, cx);
                    });
                Rc::new(body)
            });
            let scope = ScrollScope::new(ScrollRoute::new(), move |remaining, window, cx| {
                crate::nsh::scroll::consume_list(&outer, remaining, window, cx);
            });
            Root {
                list,
                scroll: ScrollHandle::new(),
                scope,
                body,
                mounted: true,
                observed: Rc::new(RefCell::new(None)),
            }
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let (root, scroll, list, body) = visual.update(|window, cx| {
            window.activate_window();
            let root = window
                .root::<Root>()
                .flatten()
                .expect("group regression root");
            let state = root.read(cx);
            (
                root.clone(),
                state.scroll.clone(),
                state.list.clone(),
                state.body.clone(),
            )
        });
        let mut fixture = Self {
            visual,
            root,
            scroll,
            list,
            body,
        };
        let started = Instant::now();
        loop {
            fixture.frame();
            let ready = fixture.scroll.max_offset().y > px(512.)
                && fixture.list.max_offset_for_scrollbar().y > px(1024.)
                && fixture.visual.update(|window, cx| {
                    fixture
                        .body
                        .as_ref()
                        .is_none_or(|body| (body.observe)(window, cx).is_some())
                });
            if ready {
                break;
            }
            assert!(
                started.elapsed() < TIMEOUT,
                "real group/List/body mount deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
            fixture
                .visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
        fixture
    }

    fn drain(&mut self) {
        let started = Instant::now();
        for tick in 0..MAX_DRAIN_TICKS {
            let worked = self.visual.cx.background_executor.tick();
            assert!(
                started.elapsed() < DRAIN_TIMEOUT,
                "group scheduler deadline tick={tick}"
            );
            if !worked {
                return;
            }
        }
        panic!("group scheduler did not quiesce in {MAX_DRAIN_TICKS} ticks");
    }

    fn frame(&mut self) {
        self.drain();
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.drain();
    }

    fn input(&mut self, delta: ScrollDelta, phase: TouchPhase, outside: bool) {
        let position = if outside {
            point(px(240.), px(420.))
        } else {
            let bounds = self.scroll.viewport_bounds();
            bounds.origin + point(px(24.), px(48.))
        };
        self.visual.update(|window, cx| {
            window.dispatch_event(
                ScrollWheelEvent {
                    delta,
                    position,
                    touch_phase: phase,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
        });
        self.frame();
    }

    fn wheel(&mut self, x: f32, y: f32, phase: TouchPhase, outside: bool) {
        self.input(ScrollDelta::Pixels(point(px(-x), px(-y))), phase, outside);
    }

    fn group(&self) -> f32 {
        -f32::from(self.scroll.offset().y)
    }

    fn outer(&self) -> f32 {
        -f32::from(self.list.scroll_px_offset_for_scrollbar().y)
    }

    fn expect(&self, group: f32, outer: f32) {
        assert!(
            (self.group() - group).abs() <= EPSILON,
            "group actual={} expected={group}",
            self.group()
        );
        assert!(
            (self.outer() - outer).abs() <= EPSILON,
            "outer actual={} expected={outer}",
            self.outer()
        );
    }

    fn cancel(&mut self) {
        self.wheel(0., 0., TouchPhase::Cancelled, false);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let window = self.visual.window_handle();
        let cleanup = self.visual.cx.update_window(window, |_, window, cx| {
            if let Some(body) = &self.body {
                (body.stop)(window, cx);
            }
            self.root.update(cx, |root, cx| {
                root.scope.cancel();
                root.body.take();
                root.mounted = false;
                cx.notify();
            });
        });
        if let Err(error) = cleanup {
            if std::thread::panicking() {
                eprintln!("scroll group fixture cleanup during unwind: {error:#}");
            } else {
                panic!("scroll group fixture cleanup: {error:#}");
            }
        }
    }
}

pub fn ordinary_group_partial_and_mixed_axes_reach_original_list_once(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx, None);
    let maximum = f32::from(fixture.scroll.max_offset().y);
    fixture
        .scroll
        .set_offset(point(px(0.), px(-(maximum - 8.))));
    fixture.frame();
    fixture.wheel(0., 0., TouchPhase::Started, false);
    fixture.wheel(24., 32., TouchPhase::Moved, false);
    fixture.expect(maximum, 24.);
    assert_eq!(fixture.scroll.offset().x, px(0.));
    fixture.wheel(0., -16., TouchPhase::Moved, false);
    fixture.expect(maximum - 16., 24.);
    fixture.cancel();
}

pub fn group_horizontal_fallback_and_line_units_use_actual_offsets(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx, None);
    fixture.wheel(0., 0., TouchPhase::Started, false);
    fixture.wheel(24., 0., TouchPhase::Moved, false);
    fixture.expect(24., 0.);
    fixture.cancel();
    let maximum = f32::from(fixture.scroll.max_offset().y);
    let line = fixture
        .visual
        .update(|window, _| f32::from(window.line_height()));
    assert!(line.is_finite() && line > 0.);
    fixture
        .scroll
        .set_offset(point(px(0.), px(-(maximum - line / 2.))));
    fixture.frame();
    fixture.input(ScrollDelta::Lines(point(0., -2.)), TouchPhase::Moved, false);
    fixture.expect(maximum, 30.);
    fixture.cancel();
}

pub fn group_owner_survives_pointer_change_and_terminal_delta_has_no_glide(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, None);
    fixture.wheel(0., 0., TouchPhase::Started, false);
    fixture.wheel(0., 24., TouchPhase::Moved, true);
    fixture.expect(24., 0.);
    fixture.wheel(0., 8., TouchPhase::Ended, true);
    fixture.expect(32., 0.);
    fixture.wheel(0., 4., TouchPhase::Moved, true);
    fixture.expect(36., 0.);
    fixture.wheel(0., 0., TouchPhase::Cancelled, true);
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(1100) {
        std::thread::sleep(Duration::from_millis(25));
        fixture
            .visual
            .cx
            .background_executor
            .advance_clock(Duration::from_millis(25));
        fixture.frame();
        fixture.expect(36., 0.);
    }
    fixture.wheel(0., 16., TouchPhase::Moved, true);
    fixture.expect(36., 16.);
}

pub fn replaced_scope_and_detached_group_revoke_retained_remainder_consumer(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, None);
    fixture.wheel(0., 0., TouchPhase::Started, false);
    let stale = fixture.visual.update(|_, cx| {
        fixture
            .root
            .read(cx)
            .observed
            .borrow()
            .as_ref()
            .expect("actual rendered group state")
            .borrow()
            .scope
            .clone()
            .expect("real mounted group scope")
    });
    fixture.visual.update(|_, cx| {
        fixture.root.update(cx, |root, cx| {
            let list = root.list.clone();
            root.scope = ScrollScope::new(ScrollRoute::new(), move |remaining, window, cx| {
                crate::nsh::scroll::consume_list(&list, remaining, window, cx);
            });
            cx.notify();
        })
    });
    fixture.frame();
    let pending = ScrollRemainder::from_event(&ScrollWheelEvent {
        delta: ScrollDelta::Pixels(point(px(0.), px(-32.))),
        touch_phase: TouchPhase::Moved,
        ..Default::default()
    });
    fixture
        .visual
        .update(|window, cx| stale.consume(pending, window, cx));
    fixture.expect(0., 0.);
    fixture.wheel(0., 16., TouchPhase::Moved, true);
    fixture.expect(0., 16.);
    let detached = fixture.visual.update(|_, cx| {
        fixture
            .root
            .read(cx)
            .observed
            .borrow()
            .as_ref()
            .expect("actual rendered group state")
            .borrow()
            .scope
            .clone()
            .expect("replacement mounted group scope")
    });
    fixture.visual.update(|_, cx| {
        fixture.root.update(cx, |root, cx| {
            root.mounted = false;
            cx.notify();
        })
    });
    fixture.frame();
    fixture
        .visual
        .update(|window, cx| detached.consume(pending, window, cx));
    fixture.expect(0., 16.);
}

pub fn real_body_then_group_then_original_list_preserve_partial_remainder(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(cx, Some((factory, BodyKind::Vertical)));
    let body = fixture.body.clone().expect("production body");
    let observed = fixture
        .visual
        .update(|window, cx| (body.observe)(window, cx).expect("real body geometry"));
    assert!(observed.maximum.y > px(32.));
    fixture.visual.update(|window, cx| {
        (body.scroll_to)(point(px(0.), observed.maximum.y - px(8.)), window, cx);
    });
    fixture.frame();
    let before = fixture
        .visual
        .update(|window, cx| (body.observe)(window, cx).expect("current body geometry"));
    let group_max = f32::from(fixture.scroll.max_offset().y);
    fixture.wheel(0., 0., TouchPhase::Started, false);
    fixture.wheel(0., 32., TouchPhase::Moved, false);
    let after = fixture
        .visual
        .update(|window, cx| (body.observe)(window, cx).expect("retained body geometry"));
    assert!((f32::from(after.offset.y - before.offset.y) - 8.).abs() <= EPSILON);
    fixture.expect(24., 0.);
    fixture.wheel(0., group_max + 8., TouchPhase::Moved, false);
    fixture.expect(group_max, 32.);
    fixture.cancel();
}
