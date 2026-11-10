use super::separator::mounted_observations::{Observations, Observed};
use super::*;
use std::time::{Duration, Instant};

const LINE: &str = "mounted-workspace-separator";
const BASE: &str = "mounted-workspace-background";

#[derive(Default, Debug, PartialEq, Eq)]
struct Events {
    drag_down: usize,
    parent_down: usize,
    parent_up: usize,
    parent_move: usize,
    parent_wheel: usize,
}

struct Root {
    vertical: bool,
    mounted: bool,
    extent: f32,
    clip: f32,
    events: Events,
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .relative()
            .overflow_hidden()
            .w(px(if self.vertical {
                self.clip
            } else {
                self.extent
            }))
            .h(px(if self.vertical {
                self.extent
            } else {
                self.clip
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|root, _, _, _| root.events.parent_down += 1),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|root, _, _, _| root.events.parent_up += 1),
            )
            .on_mouse_move(cx.listener(|root, _, _, _| root.events.parent_move += 1))
            .on_scroll_wheel(cx.listener(|root, _, _, _| root.events.parent_wheel += 1));
        if self.mounted {
            let on_down = cx.listener(|root: &mut Self, _, _, _| root.events.drag_down += 1);
            root = root
                .child(super::background(
                    BASE,
                    theme(cx).colors.bg_base,
                    window,
                    cx,
                ))
                .child(
                    super::drag_region(LINE, self.vertical, window, cx, on_down)
                        .debug_selector(|| "workspace-drag-region".into()),
                );
        }
        root
    }
}

struct Fixture {
    visual: VisualTestContext,
    root: Entity<Root>,
    observations: Observations,
}

impl Fixture {
    fn new(cx: &mut TestAppContext, vertical: bool) -> Self {
        let observations = Observations::default();
        cx.update(|cx| {
            assert!(!cx.text_system().all_font_names().is_empty());
            cx.set_global(vega_theme::Theme::dark());
            cx.set_global(observations.clone());
        });
        let window = cx.open_window(size(px(240.), px(240.)), move |_, _| Root {
            vertical,
            mounted: true,
            extent: 100.,
            clip: 120.,
            events: Events::default(),
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let root = visual.update(|window, _| {
            window.activate_window();
            window
                .root::<Root>()
                .flatten()
                .expect("workspace fixture root")
        });
        let mut fixture = Self {
            visual,
            root,
            observations,
        };
        fixture.wait("initial native separator and background", |fixture| {
            fixture.painted(LINE, vega_theme::Theme::dark().colors.border_subtle)
                && fixture.painted(BASE, vega_theme::Theme::dark().colors.bg_base)
        });
        fixture
    }

    fn observed(&self, key: &'static str) -> Observed {
        self.observations
            .0
            .borrow()
            .get(key)
            .cloned()
            .expect("mounted observation")
    }

    fn drain(&mut self) {
        let start = Instant::now();
        for _ in 0..512 {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "workspace scheduler deadline"
            );
            if !self.visual.cx.background_executor.tick() {
                return;
            }
        }
        panic!("workspace scheduler did not quiesce in 512 ticks");
    }

    fn frame(&mut self) {
        self.drain();
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.drain();
    }

    fn wait(&mut self, stage: &str, mut ready: impl FnMut(&mut Self) -> bool) {
        let start = Instant::now();
        loop {
            self.frame();
            if ready(self) {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "workspace stage: {stage}"
            );
            std::thread::sleep(Duration::from_millis(10));
            self.visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
    }

    fn change(&mut self, update: impl FnOnce(&mut Root)) {
        self.root.update(&mut self.visual.cx, |root, cx| {
            update(root);
            cx.notify();
        });
    }

    fn input(&mut self, event: impl InputEvent) {
        self.visual.update(|window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
        self.drain();
    }

    fn click(&mut self, position: Point<Pixels>) {
        self.input(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        self.input(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
        });
    }

    fn painted(&mut self, key: &'static str, color: Rgba) -> bool {
        let Some(bounds) = self.observed(key).bounds else {
            return false;
        };
        self.visual.update(|window, _| {
            let scale = window.scale_factor();
            window.painted_quads().iter().any(|quad| {
                quad.bounds == bounds.scale(scale) && quad.background == Background::from(color)
            })
        })
    }

    fn retained(&mut self, ids: [EntityId; 2]) {
        for (key, id) in [LINE, BASE].into_iter().zip(ids) {
            let observed = self.observed(key);
            assert_eq!(observed.surfaces.len(), 1, "Surface recreated for {key}");
            assert_eq!(observed.surfaces[0].entity_id(), id);
            assert!(observed.stops.is_empty());
            assert_eq!(
                observed.input_mask.expect("passive input mask").size,
                size(px(0.), px(0.))
            );
            assert!(observed.surfaces[0].upgrade().is_some(), "retained AppView");
        }
    }

    fn ids(&self) -> [EntityId; 2] {
        [LINE, BASE].map(|key| {
            self.observed(key)
                .surfaces
                .last()
                .expect("Surface identity")
                .entity_id()
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.visual.update(|window, cx| {
            window.remove_window();
            cx.remove_global::<Observations>();
            cx.set_global(vega_theme::Theme::dark());
        });
    }
}

pub fn five_pixel_region_and_background_deliver_pointer_and_wheel_once(cx: &mut TestAppContext) {
    for vertical in [true, false] {
        let mut fixture = Fixture::new(cx, vertical);
        let ids = fixture.ids();
        let region = fixture
            .visual
            .debug_bounds("workspace-drag-region")
            .expect("actual drag region");
        assert_eq!(
            if vertical {
                region.size.width
            } else {
                region.size.height
            },
            px(5.)
        );
        let line = fixture.observed(LINE).bounds.expect("native line host");
        assert_eq!(
            if vertical {
                line.size.width
            } else {
                line.size.height
            },
            px(1.)
        );
        for (index, cross) in [0.25, 2.5, 4.75, 5.25, 40.].into_iter().enumerate() {
            let position = if vertical {
                region.origin + point(px(cross), px(40.))
            } else {
                region.origin + point(px(40.), px(cross))
            };
            fixture.click(position);
            fixture.input(MouseMoveEvent {
                position,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            });
            fixture.input(ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(3.), px(-16.))),
                touch_phase: TouchPhase::Moved,
                modifiers: Modifiers::default(),
            });
            fixture.visual.update(|_, cx| {
                assert_eq!(
                    fixture.root.read(cx).events,
                    Events {
                        drag_down: (index + 1).min(3),
                        parent_down: index + 1,
                        parent_up: index + 1,
                        parent_move: index + 1,
                        parent_wheel: index + 1,
                    }
                );
            });
        }
        fixture.retained(ids);
    }
}

pub fn resize_theme_and_clip_retain_surface_and_native_paint(cx: &mut TestAppContext) {
    for vertical in [true, false] {
        let mut fixture = Fixture::new(cx, vertical);
        let ids = fixture.ids();
        for _ in 0..8 {
            fixture.frame();
        }
        for key in [LINE, BASE] {
            assert_eq!(fixture.observed(key).publications, 0);
        }
        fixture.change(|root| {
            root.extent = 160.;
            root.clip = 2.75;
        });
        fixture
            .visual
            .update(|_, cx| cx.set_global(vega_theme::Theme::light()));
        fixture.wait("resized light native painting", |fixture| {
            let bounds = fixture.observed(LINE).bounds.expect("resized line");
            (if vertical {
                bounds.size.height
            } else {
                bounds.size.width
            }) == px(160.)
                && fixture.painted(LINE, vega_theme::Theme::light().colors.border_subtle)
                && fixture.painted(BASE, vega_theme::Theme::light().colors.bg_base)
        });
        fixture.retained(ids);
        let line = fixture.observed(LINE);
        let bounds = line.bounds.expect("line bounds");
        let clip = line.paint_clip.expect("inherited paint clip");
        assert!(if vertical {
            clip.size.width < bounds.size.width
        } else {
            clip.size.height < bounds.size.height
        });
        fixture.visual.update(|window, _| {
            let expected = clip.scale(window.scale_factor());
            assert!(window.painted_quads().iter().any(|quad| {
                quad.bounds == bounds.scale(window.scale_factor())
                    && quad.background
                        == Background::from(vega_theme::Theme::light().colors.border_subtle)
                    && quad.bounds.intersect(&quad.content_mask.bounds) == expected
            }));
        });
        for key in [LINE, BASE] {
            assert_eq!(fixture.observed(key).publications, 1);
        }
        fixture.change(|root| root.clip = 120.);
        fixture.frame();
        fixture.retained(ids);
        assert_eq!(
            fixture.observed(LINE).paint_clip,
            fixture.observed(LINE).bounds
        );
    }
}

pub fn unmount_stops_and_releases_surface_before_rebuild(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx, true);
    let ids = fixture.ids();
    fixture.change(|root| root.mounted = false);
    fixture.wait("both removed Surface entities released", |fixture| {
        [LINE, BASE].into_iter().all(|key| {
            let observed = fixture.observed(key);
            observed.stops.len() == 1 && observed.surfaces[0].upgrade().is_none()
        })
    });
    for (key, id) in [LINE, BASE].into_iter().zip(ids) {
        assert_eq!(fixture.observed(key).stops, vec![id]);
    }
    fixture.change(|root| root.mounted = true);
    fixture.wait("new native painting after rebuild", |fixture| {
        fixture.observed(LINE).surfaces.len() == 2
            && fixture.observed(BASE).surfaces.len() == 2
            && fixture.painted(LINE, vega_theme::Theme::dark().colors.border_subtle)
            && fixture.painted(BASE, vega_theme::Theme::dark().colors.bg_base)
    });
    let rebuilt = fixture.ids();
    assert_ne!(rebuilt[0], ids[0]);
    assert_ne!(rebuilt[1], ids[1]);
    for key in [LINE, BASE] {
        assert_eq!(fixture.observed(key).stops.len(), 1);
    }
}
