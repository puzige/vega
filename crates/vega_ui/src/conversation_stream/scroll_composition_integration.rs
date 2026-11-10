use gpui_kit::{prelude::*, *};
use nsh_platform_gpui::AppView;
use std::{
    rc::Rc,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(10);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_DRAIN_TICKS: usize = 512;
const EPSILON: f32 = 0.5;

#[derive(Clone, Copy, Debug)]
pub enum BodyKind {
    NoTarget,
    Vertical,
    Horizontal,
}

pub struct BodyObservation {
    pub surface: Entity<AppView>,
    pub viewport: Bounds<Pixels>,
    pub offset: Point<Pixels>,
    pub maximum: Point<Pixels>,
    pub error: Option<String>,
}

pub type BodyRenderer = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
pub type BodyObserver = Rc<dyn Fn(&Window, &App) -> Option<BodyObservation>>;
pub type BodyPositioner = Rc<dyn Fn(Point<Pixels>, &mut Window, &mut App)>;
pub type BodyStopper = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct BodyMount {
    pub(crate) stream: WeakEntity<super::ConversationStream>,
    pub render: BodyRenderer,
    pub observe: BodyObserver,
    pub scroll_to: BodyPositioner,
    pub stop: BodyStopper,
}

pub type BodyFactory = fn(BodyKind, usize, ListState, &mut Window, &mut App) -> BodyMount;

struct FixtureRoot {
    list: ListState,
    bodies: Option<[Rc<BodyMount>; 2]>,
    scope: nsh_platform_gpui::ScrollScope,
}

pub(crate) fn owns_stream(
    stream: &WeakEntity<super::ConversationStream>,
    window: &Window,
    cx: &App,
) -> bool {
    let Some(stream) = stream.upgrade() else {
        return false;
    };
    window.is_window_active()
        && window.root::<FixtureRoot>().flatten().is_some_and(|root| {
            root.read(cx)
                .bodies
                .as_ref()
                .is_some_and(|bodies| bodies.iter().any(|body| body.stream == stream.downgrade()))
        })
}

impl Render for FixtureRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let bodies = self.bodies.clone();
        let owners = bodies
            .iter()
            .flatten()
            .map(|body| body.stream.clone())
            .collect();
        let child = list(self.list.clone(), move |index, window, cx| {
            assert_eq!(index, 0);
            let mut row = div().w_full().h(px(4096.)).flex().flex_col();
            if let Some(bodies) = &bodies {
                row = row
                    .child(div().h(px(96.)).flex_shrink_0())
                    .child(
                        div()
                            .h(px(128.))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .child((bodies[0].render)(window, cx)),
                    )
                    .child(div().h(px(32.)).flex_shrink_0())
                    .child(
                        div()
                            .h(px(128.))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .child((bodies[1].render)(window, cx)),
                    );
            }
            row.into_any_element()
        })
        .size_full()
        .into_any_element();
        let child = super::selection_menu::mounted::ReadingList::with_owners(owners, child)
            .into_any_element();
        crate::nsh::scroll::scoped(self.scope.clone(), child)
    }
}

struct Observation {
    bodies: [BodyObservation; 2],
    outer: f32,
}

struct Fixture {
    case: &'static str,
    visual: VisualTestContext,
    root: Entity<FixtureRoot>,
    list: ListState,
    bodies: [Rc<BodyMount>; 2],
    stopped: bool,
}

fn close(actual: f32, expected: f32) -> bool {
    actual.is_finite() && expected.is_finite() && (actual - expected).abs() <= EPSILON
}

fn assert_point(actual: Point<Pixels>, expected: Point<Pixels>, reason: &str) {
    assert!(
        close(actual.x.into(), expected.x.into()) && close(actual.y.into(), expected.y.into()),
        "{reason}: actual={actual:?} expected={expected:?}"
    );
}

impl Fixture {
    fn new(
        cx: &mut TestAppContext,
        case: &'static str,
        kind: BodyKind,
        factory: BodyFactory,
    ) -> Self {
        cx.update(|cx| {
            assert!(
                !cx.text_system().all_font_names().is_empty(),
                "real platform fonts required"
            );
            cx.set_global(vega_theme::Theme::dark());
        });
        let window = cx.open_window(size(px(360.), px(560.)), move |window, cx| {
            let list = ListState::new(1, ListAlignment::Top, px(600.));
            let bodies = [
                Rc::new(factory(kind, 0, list.clone(), window, cx)),
                Rc::new(factory(kind, 1, list.clone(), window, cx)),
            ];
            let outer = list.clone();
            let scope = nsh_platform_gpui::ScrollScope::new(
                nsh_platform_gpui::ScrollRoute::new(),
                move |remaining, window, cx| {
                    crate::nsh::scroll::consume_list(&outer, remaining, window, cx);
                },
            );
            FixtureRoot {
                list,
                bodies: Some(bodies),
                scope,
            }
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let (root, list, bodies) = visual.update(|window, cx| {
            window.activate_window();
            let root = window
                .root::<FixtureRoot>()
                .flatten()
                .expect("scroll composition root");
            let state = root.read(cx);
            (
                root.clone(),
                state.list.clone(),
                state.bodies.clone().expect("production mounts"),
            )
        });
        let mut this = Self {
            case,
            visual,
            root,
            list,
            bodies,
            stopped: false,
        };
        this.wait(
            "two live production body Surfaces in original List",
            |this| this.observe().is_some(),
        );
        let observed = this.sample();
        assert_ne!(observed.bodies[0].surface, observed.bodies[1].surface);
        assert!(this.list.max_offset_for_scrollbar().y > px(1024.));
        for body in &observed.bodies {
            assert_point(
                body.offset,
                point(px(0.), px(0.)),
                "initial actual inner offset",
            );
            match kind {
                BodyKind::NoTarget => {
                    assert_point(body.maximum, point(px(0.), px(0.)), "no scroll target")
                }
                BodyKind::Vertical => {
                    assert!(body.maximum.y >= px(512.));
                    assert!(close(body.maximum.x.into(), 0.));
                }
                BodyKind::Horizontal => {
                    assert!(body.maximum.x >= px(256.));
                    assert!(close(body.maximum.y.into(), 0.));
                }
            }
        }
        this
    }

    fn drain(&mut self, reason: &str) {
        let started = Instant::now();
        for tick in 0..MAX_DRAIN_TICKS {
            let worked = self.visual.cx.background_executor.tick();
            assert!(
                started.elapsed() < DRAIN_TIMEOUT,
                "scheduler drain deadline: case={} stage={reason} tick={tick} queued={:?}",
                self.case,
                self.visual.cx.dispatcher.scheduler().pending_task_counts()
            );
            if !worked {
                return;
            }
        }
        panic!(
            "scheduler did not quiesce: case={} stage={reason} ticks={MAX_DRAIN_TICKS} queued={:?}",
            self.case,
            self.visual.cx.dispatcher.scheduler().pending_task_counts()
        );
    }

    fn frame(&mut self) {
        self.drain("before draw");
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.drain("after draw");
    }

    fn step(&mut self, duration: Duration) {
        std::thread::sleep(duration);
        self.visual.cx.background_executor.advance_clock(duration);
        self.frame();
    }

    fn wait(&mut self, reason: &str, mut ready: impl FnMut(&mut Self) -> bool) {
        let started = Instant::now();
        loop {
            self.frame();
            if ready(self) {
                return;
            }
            assert!(
                started.elapsed() < TIMEOUT,
                "case={} timeout: {reason}",
                self.case
            );
            self.step(Duration::from_millis(10));
        }
    }

    fn observe(&mut self) -> Option<Observation> {
        self.visual.update(|window, cx| {
            assert!(
                window.is_window_active(),
                "fixture must retain active window authority"
            );
            let bodies = [
                (self.bodies[0].observe)(window, cx)?,
                (self.bodies[1].observe)(window, cx)?,
            ];
            for body in &bodies {
                assert!(
                    body.error.is_none(),
                    "production Surface error: {:?}",
                    body.error
                );
                for value in [
                    body.viewport.origin.x,
                    body.viewport.origin.y,
                    body.viewport.size.width,
                    body.viewport.size.height,
                    body.offset.x,
                    body.offset.y,
                    body.maximum.x,
                    body.maximum.y,
                ] {
                    assert!(f32::from(value).is_finite(), "nonfinite native observation");
                }
                assert!(body.viewport.size.width > px(0.) && body.viewport.size.height > px(0.));
                assert!(body.maximum.x >= px(0.) && body.maximum.y >= px(0.));
                assert!(
                    body.offset.x >= px(-EPSILON) && body.offset.x <= body.maximum.x + px(EPSILON)
                );
                assert!(
                    body.offset.y >= px(-EPSILON) && body.offset.y <= body.maximum.y + px(EPSILON)
                );
                let viewport = self.list.viewport_bounds();
                assert!(
                    body.viewport.left() >= viewport.left()
                        && body.viewport.right() <= viewport.right()
                );
                assert!(
                    body.viewport.top() >= viewport.top()
                        && body.viewport.bottom() <= viewport.bottom()
                );
            }
            Some(Observation {
                bodies,
                outer: -f32::from(self.list.scroll_px_offset_for_scrollbar().y),
            })
        })
    }

    fn sample(&mut self) -> Observation {
        self.observe()
            .expect("current measured production body observation")
    }

    fn point(&mut self, slot: usize) -> Point<Pixels> {
        let body = &self.sample().bodies[slot];
        body.viewport.origin
            + point(
                body.viewport.size.width / 2.,
                body.viewport.size.height / 2.,
            )
    }

    fn input<E: InputEvent>(&mut self, event: E) {
        self.visual.update(|window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
        self.drain("native input dispatch");
        self.frame();
    }

    fn wheel(&mut self, slot: usize, x: f32, y: f32, phase: TouchPhase) {
        let position = self.point(slot);
        self.input(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(-x), px(-y))),
            touch_phase: phase,
            ..Default::default()
        });
    }

    fn pointer(&mut self, slot: usize) {
        let position = self.point(slot);
        self.input(MouseMoveEvent {
            position,
            ..Default::default()
        });
    }

    fn position(&mut self, inner: Point<Pixels>, outer: f32) {
        self.visual.update(|window, cx| {
            (self.bodies[0].scroll_to)(inner, window, cx);
            self.list.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(outer),
            });
        });
        self.expect(inner, point(px(0.), px(0.)), outer);
    }

    fn matches(
        observed: &Observation,
        first: Point<Pixels>,
        second: Point<Pixels>,
        outer: f32,
    ) -> bool {
        [(&observed.bodies[0], first), (&observed.bodies[1], second)]
            .iter()
            .all(|(body, target)| {
                close(body.offset.x.into(), target.x.into())
                    && close(body.offset.y.into(), target.y.into())
            })
            && close(observed.outer, outer)
    }

    fn expect(&mut self, first: Point<Pixels>, second: Point<Pixels>, outer: f32) {
        self.wait("exact native inner and original List displacement", |this| {
            let Some(observed) = this.observe() else { return false; };
            eprintln!("[nsh-nestedwheel][{}] inner={:?}/{:?} outer={} expected={first:?}/{second:?}/{outer}",
                this.case, observed.bodies[0].offset, observed.bodies[1].offset, observed.outer);
            Self::matches(&observed, first, second, outer)
        });
    }

    fn unchanged_for(&mut self, duration: Duration) {
        let expected = self.sample();
        let started = Instant::now();
        let mut advanced = Duration::ZERO;
        while advanced < duration {
            assert!(
                started.elapsed() < TIMEOUT,
                "post-gesture observation deadline: case={}",
                self.case
            );
            let step = Duration::from_millis(10).min(duration - advanced);
            self.step(step);
            advanced += step;
            let observed = self.sample();
            assert_point(
                observed.bodies[0].offset,
                expected.bodies[0].offset,
                "owner must not glide twice",
            );
            assert_point(
                observed.bodies[1].offset,
                expected.bodies[1].offset,
                "neighbor must not acquire momentum",
            );
            assert!(
                close(observed.outer, expected.outer),
                "case={} unexpected outer glide: {} -> {}",
                self.case,
                expected.outer,
                observed.outer
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let window = self.visual.window_handle();
        let cleanup = self.visual.cx.update_window(window, |_, window, cx| {
            for body in &self.bodies {
                (body.stop)(window, cx);
            }
            self.root.update(cx, |root, cx| {
                root.scope.cancel();
                root.bodies.take();
                cx.notify();
            });
        });
        if let Err(error) = cleanup {
            if std::thread::panicking() {
                eprintln!("nested scroll fixture cleanup during unwind: {error:#}");
            } else {
                panic!("nested scroll fixture cleanup: {error:#}");
            }
        }
    }
}

pub fn no_target_surface_forwards_wheel_once_to_original_list(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(
        cx,
        "no_target_surface_forwards_wheel_once_to_original_list",
        BodyKind::NoTarget,
        factory,
    );
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    fixture.wheel(0, 0., 48., TouchPhase::Moved);
    fixture.expect(point(px(0.), px(0.)), point(px(0.), px(0.)), 48.);
    fixture.wheel(0, 0., 0., TouchPhase::Ended);
    fixture.unchanged_for(Duration::from_millis(1100));
}

pub fn inner_surface_consumes_wheel_until_actual_edge(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(
        cx,
        "inner_surface_consumes_wheel_until_actual_edge",
        BodyKind::Vertical,
        factory,
    );
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    for distance in [24., 48., 72.] {
        fixture.wheel(0, 0., 24., TouchPhase::Moved);
        fixture.expect(point(px(0.), px(distance)), point(px(0.), px(0.)), 0.);
    }
    fixture.wheel(0, 0., 0., TouchPhase::Cancelled);
    fixture.unchanged_for(Duration::from_millis(1100));
}

pub fn partial_forward_remainder_reaches_original_list_exactly_once(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(
        cx,
        "partial_forward_remainder_reaches_original_list_exactly_once",
        BodyKind::Vertical,
        factory,
    );
    let maximum = fixture.sample().bodies[0].maximum.y;
    fixture.position(point(px(0.), maximum - px(12.)), 0.);
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    fixture.wheel(0, 0., 44., TouchPhase::Moved);
    fixture.expect(point(px(0.), maximum), point(px(0.), px(0.)), 32.);
    fixture.wheel(0, 0., 0., TouchPhase::Cancelled);
    fixture.unchanged_for(Duration::from_millis(1100));
}

pub fn partial_reverse_remainder_reaches_original_list_exactly_once(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(
        cx,
        "partial_reverse_remainder_reaches_original_list_exactly_once",
        BodyKind::Vertical,
        factory,
    );
    fixture.position(point(px(0.), px(12.)), 64.);
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    fixture.wheel(0, 0., -44., TouchPhase::Moved);
    fixture.expect(point(px(0.), px(0.)), point(px(0.), px(0.)), 32.);
    fixture.wheel(0, 0., 0., TouchPhase::Cancelled);
    fixture.unchanged_for(Duration::from_millis(1100));
}

pub fn mixed_axes_consume_horizontal_inner_and_forward_vertical_remainder(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(
        cx,
        "mixed_axes_consume_horizontal_inner_and_forward_vertical_remainder",
        BodyKind::Horizontal,
        factory,
    );
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    fixture.wheel(0, 24., 48., TouchPhase::Moved);
    fixture.expect(point(px(24.), px(0.)), point(px(0.), px(0.)), 48.);
    fixture.wheel(0, 0., 0., TouchPhase::Cancelled);
    fixture.unchanged_for(Duration::from_millis(1100));
}

pub fn mixed_axes_partial_inner_remainder_does_not_replay_vertical_delta(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(
        cx,
        "mixed_axes_partial_inner_remainder_does_not_replay_vertical_delta",
        BodyKind::Horizontal,
        factory,
    );
    let maximum = fixture.sample().bodies[0].maximum.x;
    fixture.position(point(maximum - px(8.), px(0.)), 0.);
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    fixture.wheel(0, 24., 48., TouchPhase::Moved);
    fixture.expect(point(maximum, px(0.)), point(px(0.), px(0.)), 48.);
    fixture.wheel(0, 0., 0., TouchPhase::Cancelled);
    fixture.unchanged_for(Duration::from_millis(1100));
}

pub fn actual_top_and_bottom_edges_transfer_only_outward_delta(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(
        cx,
        "actual_top_and_bottom_edges_transfer_only_outward_delta",
        BodyKind::Vertical,
        factory,
    );
    let maximum = fixture.sample().bodies[0].maximum.y;
    fixture.position(point(px(0.), maximum), 0.);
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    fixture.wheel(0, 0., 32., TouchPhase::Moved);
    fixture.expect(point(px(0.), maximum), point(px(0.), px(0.)), 32.);
    fixture.wheel(0, 0., -16., TouchPhase::Moved);
    fixture.expect(point(px(0.), maximum - px(16.)), point(px(0.), px(0.)), 32.);
    fixture.wheel(0, 0., 0., TouchPhase::Cancelled);
    fixture.position(point(px(0.), px(0.)), 64.);
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    fixture.wheel(0, 0., -32., TouchPhase::Moved);
    fixture.expect(point(px(0.), px(0.)), point(px(0.), px(0.)), 32.);
    fixture.wheel(0, 0., 16., TouchPhase::Moved);
    fixture.expect(point(px(0.), px(16.)), point(px(0.), px(0.)), 32.);
    fixture.wheel(0, 0., 0., TouchPhase::Cancelled);
}

pub fn gesture_owner_survives_pointer_move_until_ended(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(
        cx,
        "gesture_owner_survives_pointer_move_until_ended",
        BodyKind::Vertical,
        factory,
    );
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    fixture.wheel(0, 0., 36., TouchPhase::Moved);
    fixture.expect(point(px(0.), px(36.)), point(px(0.), px(0.)), 0.);
    fixture.pointer(1);
    fixture.wheel(1, 0., 24., TouchPhase::Moved);
    fixture.expect(point(px(0.), px(60.)), point(px(0.), px(0.)), 0.);
    fixture.unchanged_for(Duration::from_millis(200));
    fixture.wheel(1, 0., 0., TouchPhase::Ended);
    fixture.wheel(1, 0., 0., TouchPhase::Started);
    let owner = point(px(0.), px(60.));
    fixture.wheel(1, 0., 24., TouchPhase::Moved);
    fixture.expect(owner, point(px(0.), px(24.)), 0.);
    fixture.wheel(1, 0., 0., TouchPhase::Cancelled);
    fixture.unchanged_for(Duration::from_millis(1100));
}

pub fn terminal_phase_delta_reaches_original_list_once(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    for phase in [TouchPhase::Ended, TouchPhase::Cancelled] {
        let mut fixture = Fixture::new(
            cx,
            "terminal_phase_delta_reaches_original_list_once",
            BodyKind::NoTarget,
            factory,
        );
        fixture.wheel(0, 0., 16., TouchPhase::Started);
        fixture.expect(point(px(0.), px(0.)), point(px(0.), px(0.)), 16.);
        fixture.wheel(0, 0., 24., TouchPhase::Moved);
        fixture.expect(point(px(0.), px(0.)), point(px(0.), px(0.)), 40.);
        fixture.wheel(0, 0., 8., phase);
        fixture.expect(point(px(0.), px(0.)), point(px(0.), px(0.)), 48.);
        fixture.unchanged_for(Duration::from_millis(1100));
    }
}

pub fn cancelled_gesture_releases_owner_without_residual_glide(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(
        cx,
        "cancelled_gesture_releases_owner_without_residual_glide",
        BodyKind::Vertical,
        factory,
    );
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    fixture.wheel(0, 0., 32., TouchPhase::Moved);
    fixture.expect(point(px(0.), px(32.)), point(px(0.), px(0.)), 0.);
    fixture.pointer(1);
    fixture.wheel(1, 0., 0., TouchPhase::Cancelled);
    fixture.unchanged_for(Duration::from_millis(1100));
    fixture.wheel(1, 0., 0., TouchPhase::Started);
    fixture.wheel(1, 0., 24., TouchPhase::Moved);
    fixture.expect(point(px(0.), px(32.)), point(px(0.), px(24.)), 0.);
    fixture.wheel(1, 0., 0., TouchPhase::Cancelled);
    fixture.unchanged_for(Duration::from_millis(1100));
}

pub fn ended_edge_transfer_cannot_start_inner_and_outer_glides(
    cx: &mut TestAppContext,
    factory: BodyFactory,
) {
    let mut fixture = Fixture::new(
        cx,
        "ended_edge_transfer_cannot_start_inner_and_outer_glides",
        BodyKind::Vertical,
        factory,
    );
    let maximum = fixture.sample().bodies[0].maximum.y;
    fixture.position(point(px(0.), maximum - px(8.)), 0.);
    fixture.wheel(0, 0., 0., TouchPhase::Started);
    fixture.wheel(0, 0., 40., TouchPhase::Moved);
    fixture.expect(point(px(0.), maximum), point(px(0.), px(0.)), 32.);
    fixture.wheel(0, 0., 0., TouchPhase::Ended);
    fixture.expect(point(px(0.), maximum), point(px(0.), px(0.)), 32.);
    fixture.unchanged_for(Duration::from_millis(1100));
}
