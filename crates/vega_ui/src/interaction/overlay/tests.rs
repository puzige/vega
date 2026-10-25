use super::*;
use gpui_kit::{prelude::*, test::TestWindowExt, *};
use vega_theme::Theme;

struct Host {
    entry: OverlayEntry,
    shown: bool,
    visible: bool,
    bypass: bool,
    paints: Rc<RefCell<Vec<f32>>>,
    frames: Rc<std::cell::Cell<usize>>,
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.entry.sync(self.shown, self.visible, window, cx);
        let paints = self.paints.clone();
        let frames = self.frames.clone();
        div()
            .size_full()
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, _, _| {
                        frames.set(frames.get() + 1);
                    },
                )
                .absolute()
                .size(px(1.)),
            )
            .children(self.shown.then(|| {
                deferred(
                    self.entry.surface(
                        div()
                            .id("surface")
                            .absolute()
                            .top(px(40.))
                            .left(px(30.))
                            .size(px(80.))
                            .bg(Theme::dark().colors.bg_elevated)
                            .child(
                                canvas(
                                    |_, _, _| (),
                                    move |bounds, _, window, _| {
                                        window.paint_quad(fill(
                                            bounds,
                                            Theme::dark().colors.brand_primary,
                                        ));
                                        paints.borrow_mut().push(1.);
                                    },
                                )
                                .size_full(),
                            ),
                        self.bypass,
                    ),
                )
                .with_priority(2)
            }))
    }
}

fn host(cx: &mut TestAppContext) -> WindowHandle<Host> {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(Theme::dark());
        cx.set_reduce_motion(false);
    });
    let window = cx.add_window(|_, _| Host {
        entry: OverlayEntry::default(),
        shown: false,
        visible: true,
        bypass: false,
        paints: Rc::default(),
        frames: Rc::default(),
    });
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| assert!(window.is_window_active()))
        .unwrap();
    window
}

fn frame<V: Render + 'static>(window: WindowHandle<V>, cx: &mut TestAppContext) {
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
}

fn alpha(window: WindowHandle<Host>, cx: &mut TestAppContext) -> f32 {
    window
        .update(cx, |host, _, _| host.entry.0.borrow().last_alpha.unwrap())
        .unwrap()
}

fn show(window: WindowHandle<Host>, cx: &mut TestAppContext, open: bool) {
    window
        .update(cx, |host, _, cx| {
            host.shown = open;
            if open {
                host.entry.open(cx);
            } else {
                host.entry.close(cx);
            }
            cx.notify();
        })
        .unwrap();
}

fn advance(cx: &mut TestAppContext, ms: u64) {
    cx.background_executor
        .advance_clock(Duration::from_millis(ms));
}

fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
}

#[gpui_kit::test]
fn deferred_surface_paints_children_with_entry_alpha_and_stops_when_finished(
    cx: &mut TestAppContext,
) {
    let window = host(cx);
    show(window, cx, true);
    frame(window, cx);
    near(alpha(window, cx), 0.);
    advance(cx, 60);
    frame(window, cx);
    let middle = alpha(window, cx);
    assert!(middle > 0. && middle < 1.);
    window
        .update(cx, |host, window, _| {
            assert!(!host.paints.borrow().is_empty());
            let color: Hsla = Theme::dark().colors.brand_primary.into();
            let quads = window.painted_quads();
            assert!(
                quads
                    .iter()
                    .filter_map(|quad| quad.background.as_solid())
                    .any(|paint| {
                        (paint.h - color.h).abs() < 0.0001
                            && (paint.s - color.s).abs() < 0.0001
                            && (paint.l - color.l).abs() < 0.0001
                            && (paint.a - color.a * middle).abs() < 0.0001
                    })
            );
            let scale = window.scale_factor();
            assert!(quads.iter().any(|quad| {
                (quad.bounds.origin.x.0 - 30. * scale).abs() < 0.01
                    && (quad.bounds.origin.y.0 - 40. * scale).abs() < 0.01
                    && (quad.bounds.size.width.0 - 80. * scale).abs() < 0.01
            }));
        })
        .unwrap();
    advance(cx, 60);
    frame(window, cx);
    near(alpha(window, cx), 1.);
    window
        .update(cx, |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .unwrap();
    frame(window, cx);
    window
        .update(cx, |_, window, cx| {
            assert_eq!(window.simulate_next_frame(cx), 0)
        })
        .unwrap();
    show(window, cx, false);
    frame(window, cx);
    window
        .update(cx, |_, window, cx| {
            assert!(window.painted_quads().is_empty());
            window.simulate_next_frame(cx);
        })
        .unwrap();
    frame(window, cx);
    window
        .update(cx, |_, window, cx| {
            assert_eq!(window.simulate_next_frame(cx), 0)
        })
        .unwrap();
}

#[gpui_kit::test]
fn reopen_uses_last_real_paint_with_or_without_a_closed_frame_and_includes_deadline(
    cx: &mut TestAppContext,
) {
    let window = host(cx);
    for closed_frame in [false, true] {
        for gap in [0, Motion::OVERLAY_ENTER_MS, Motion::OVERLAY_ENTER_MS + 1] {
            window
                .update(cx, |host, _, _| host.entry.invalidate())
                .unwrap();
            show(window, cx, true);
            frame(window, cx);
            advance(cx, 45);
            frame(window, cx);
            let painted = alpha(window, cx);
            assert!(painted > 0. && painted < 1.);
            advance(cx, 20);
            near(alpha(window, cx), painted);
            show(window, cx, false);
            if closed_frame {
                frame(window, cx);
            }
            advance(cx, gap);
            show(window, cx, true);
            frame(window, cx);
            near(
                alpha(window, cx),
                if gap <= Motion::OVERLAY_ENTER_MS {
                    painted
                } else {
                    0.
                },
            );
            advance(cx, Motion::OVERLAY_ENTER_MS);
            frame(window, cx);
            near(alpha(window, cx), 1.);
        }
    }
}

#[gpui_kit::test]
fn owners_and_invalidated_entries_do_not_borrow_opacity(cx: &mut TestAppContext) {
    let window = host(cx);
    show(window, cx, true);
    frame(window, cx);
    advance(cx, 60);
    frame(window, cx);
    assert!(alpha(window, cx) > 0.);
    window
        .update(cx, |host, _, cx| {
            host.entry = OverlayEntry::default();
            host.entry.open(cx);
        })
        .unwrap();
    frame(window, cx);
    near(alpha(window, cx), 0.);
    advance(cx, 60);
    frame(window, cx);
    window
        .update(cx, |host, _, cx| {
            host.entry.invalidate();
            host.entry.open(cx);
        })
        .unwrap();
    frame(window, cx);
    near(alpha(window, cx), 0.);
}

#[gpui_kit::test]
fn safety_hidden_and_reduce_motion_snap_without_replaying(cx: &mut TestAppContext) {
    let window = host(cx);
    for gate in 0..3 {
        window
            .update(cx, |host, _, cx| {
                host.entry.invalidate();
                host.shown = true;
                host.entry.open(cx);
            })
            .unwrap();
        frame(window, cx);
        advance(cx, 35);
        frame(window, cx);
        assert!(alpha(window, cx) > 0. && alpha(window, cx) < 1.);
        window
            .update(cx, |host, _, cx| match gate {
                0 => host.bypass = true,
                1 => host.visible = false,
                _ => cx.set_reduce_motion(true),
            })
            .unwrap();
        frame(window, cx);
        near(alpha(window, cx), 1.);
        window
            .update(cx, |host, _, cx| {
                host.bypass = false;
                host.visible = true;
                cx.set_reduce_motion(false);
                cx.notify();
            })
            .unwrap();
        frame(window, cx);
        near(alpha(window, cx), 1.);
        window
            .update(cx, |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
        frame(window, cx);
        window
            .update(cx, |_, window, cx| {
                assert_eq!(window.simulate_next_frame(cx), 0)
            })
            .unwrap();
    }
}

#[gpui_kit::test]
fn activation_round_trip_without_an_inactive_paint_does_not_restore_entry(cx: &mut TestAppContext) {
    let window = host(cx);
    show(window, cx, true);
    frame(window, cx);
    advance(cx, 30);
    frame(window, cx);
    let before = window.update(cx, |host, _, _| host.frames.get()).unwrap();
    let other = cx.add_window(|_, _| gpui_kit::Empty);
    other
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |host, window, _| {
            assert!(!window.is_window_active());
            assert_eq!(host.frames.get(), before);
        })
        .unwrap();
    advance(cx, 200);
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |host, window, _| {
            assert!(window.is_window_active());
            assert_eq!(host.frames.get(), before);
        })
        .unwrap();
    frame(window, cx);
    near(alpha(window, cx), 1.);
    window
        .update(cx, |host, _, _| assert!(host.frames.get() > before))
        .unwrap();
}
