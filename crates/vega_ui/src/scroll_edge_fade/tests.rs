use super::*;
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, FocusHandle, ListAlignment, ListState, Modifiers, Render, ScrollDelta,
    ScrollWheelEvent, TestAppContext, UniformListScrollHandle, VisualTestContext, WindowHandle,
    div, point, size, uniform_list,
};

#[test]
fn scroll_edge_strength_is_spatial_clamped_and_continuous() {
    for (distance, expected) in [(0., 0.), (1., 0.), (9., 0.5), (17., 1.), (50., 1.)] {
        let edge = FadeEdge::new(px(distance), px(64.), Layout::SCROLL_EDGE_FADE_COMPACT);
        assert_eq!(edge.strength, expected);
        assert_eq!(edge.length, px(16.));
    }
    assert_eq!(FadeEdge::new(px(9.), px(32.), 16.).length, px(8.));
    assert_eq!(FadeEdge::new(px(9.), px(32.), 16.).strength, 1.);
    assert_eq!(FadeEdge::new(px(25.), px(240.), 24.).length, px(24.));
    assert_eq!(FadeEdge::new(px(50.), px(0.), 16.).strength, 0.);
    assert_eq!(FadeEdge::new(px(-50.), px(64.), 16.).strength, 0.);
}

#[test]
fn scroll_edge_quads_partition_corners_and_protect_gutters() {
    let bounds = Bounds::new(point(px(10.), px(20.)), size(px(200.), px(120.)));
    let metrics = ScrollFadeMetrics::new(bounds, point(px(80.), px(80.)), point(px(80.), px(80.)));
    let regions = metrics.regions(ScrollFade::reading().vertical_gutter(px(16.)));
    assert_eq!(regions.len(), 4);
    for (index, region) in regions.iter().enumerate() {
        assert!(region.bounds.right() <= bounds.right() - px(16.));
        for other in regions.iter().skip(index + 1) {
            assert!(region.bounds.intersect(&other.bounds).is_empty());
        }
    }
    let top = metrics.regions(ScrollFade::reading().top_only());
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].bounds.size.height, px(24.));
}

enum Viewport {
    Ordinary,
    Nested,
    Implicit,
    Uniform,
    Variable,
}

struct Harness {
    scroll: ScrollHandle,
    uniform: UniformListScrollHandle,
    list: ListState,
    viewport: Viewport,
    count: usize,
    color: Rgba,
    extent: Option<gpui_kit::Size<Pixels>>,
    border: bool,
    radius: Pixels,
    axis: Option<gpui_kit::Axis>,
    style: ScrollFade,
    clicks: std::rc::Rc<std::cell::Cell<usize>>,
    focus: FocusHandle,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let extent = self
            .extent
            .unwrap_or_else(|| size(px(self.count as f32 * 40.), px(self.count as f32 * 40.)));
        let clicks = self.clicks.clone();
        let focus = self.focus.clone();
        let content = match self.viewport {
            Viewport::Ordinary => div()
                .id("fade-viewport")
                .debug_selector(|| "fade-viewport".into())
                .size_full()
                .overflow_scroll()
                .when(self.axis == Some(gpui_kit::Axis::Vertical), |element| {
                    element.overflow_x_hidden()
                })
                .when(self.axis == Some(gpui_kit::Axis::Horizontal), |element| {
                    element.overflow_y_hidden()
                })
                .track_scroll(&self.scroll)
                .rounded(self.radius)
                .when(self.border, |element| {
                    element
                        .border_1()
                        .border_color(vega_theme::LIGHT.border_subtle)
                })
                .child(
                    div()
                        .relative()
                        .flex_shrink_0()
                        .w(extent.width)
                        .h(extent.height)
                        .child(
                            div()
                                .id("fade-action")
                                .debug_selector(|| "fade-action".into())
                                .absolute()
                                .top(px(100.))
                                .left(px(40.))
                                .w(px(40.))
                                .h(px(16.))
                                .track_focus(&self.focus)
                                .on_mouse_up(gpui_kit::MouseButton::Left, move |_, window, cx| {
                                    clicks.set(clicks.get() + 1);
                                    window.focus(&focus, cx);
                                }),
                        ),
                )
                .tracked_scroll_edge_fade(self.scroll.clone(), self.color, self.style)
                .into_any_element(),
            Viewport::Nested => div()
                .size_full()
                .rounded(self.radius)
                .when(self.border, |frame| {
                    frame
                        .border_1()
                        .border_color(vega_theme::LIGHT.border_subtle)
                })
                .map(|mut frame| {
                    let style = self.style.rounded_parent(frame.style());
                    frame.child(
                        div()
                            .id("nested-fade")
                            .size_full()
                            .when(self.style.bottom_extension > px(0.), |element| {
                                element.h(px(120.) - self.style.bottom_extension)
                            })
                            .overflow_scroll()
                            .track_scroll(&self.scroll)
                            .child(div().w(px(480.)).h(px(480.)).flex_shrink_0())
                            .tracked_scroll_edge_fade(self.scroll.clone(), self.color, style),
                    )
                })
                .into_any_element(),
            Viewport::Implicit => div()
                .id("fade-implicit")
                .size_full()
                .overflow_y_scroll()
                .child(div().w_full().h(px(self.count as f32 * 40.)))
                .scroll_edge_fade(self.color, ScrollFade::compact())
                .into_any_element(),
            Viewport::Uniform => uniform_list("fade-uniform", self.count, |range, _, _| {
                range.map(|_| div().h(px(40.)).into_any_element()).collect()
            })
            .track_scroll(&self.uniform)
            .size_full()
            .tracked_scroll_edge_fade(self.uniform.clone(), self.color, ScrollFade::compact())
            .into_any_element(),
            Viewport::Variable => gpui_kit::list(self.list.clone(), |index, _, _| {
                div()
                    .h(px(if index % 2 == 0 { 24. } else { 56. }))
                    .into_any_element()
            })
            .size_full()
            .tracked_scroll_edge_fade(
                self.list.clone(),
                self.color,
                ScrollFade::compact().vertical_list(),
            )
            .into_any_element(),
        };
        div().size_full().bg(self.color).child(content)
    }
}

fn open(cx: &mut TestAppContext, viewport: Viewport, count: usize) -> WindowHandle<Harness> {
    let window = cx.open_window(size(px(160.), px(120.)), |_, cx| Harness {
        scroll: ScrollHandle::new(),
        uniform: UniformListScrollHandle::new(),
        list: ListState::new(count, ListAlignment::Top, px(40.)),
        viewport,
        count,
        color: vega_theme::LIGHT.bg_base,
        extent: None,
        border: false,
        radius: px(0.),
        axis: None,
        style: ScrollFade::compact(),
        clicks: Default::default(),
        focus: cx.focus_handle().tab_stop(true),
    });
    cx.run_until_parked();
    window
}

fn gradients(window: WindowHandle<Harness>, cx: &mut TestAppContext) -> Vec<Bounds<Pixels>> {
    window
        .update(cx, |_, window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .iter()
                .filter(|quad| quad.background.as_solid().is_none())
                .map(|quad| {
                    quad.bounds
                        .intersect(&quad.content_mask.bounds)
                        .map(|pixel| px(pixel.as_f32() / scale))
                })
                .collect()
        })
        .expect("fade window")
}

#[gpui_kit::test]
fn scroll_edge_ordinary_bounds_programmatic_scroll_resize_and_theme(cx: &mut TestAppContext) {
    let window = open(cx, Viewport::Ordinary, 2);
    assert!(gradients(window, cx).is_empty());
    window
        .update(cx, |harness, _, cx| {
            harness.count = 12;
            cx.notify();
        })
        .expect("grow content");
    cx.run_until_parked();
    assert_eq!(gradients(window, cx).len(), 2);
    window
        .update(cx, |harness, _, cx| {
            harness.scroll.set_offset(point(px(-80.), px(-80.)));
            harness.color = vega_theme::DARK.bg_base;
            cx.notify();
        })
        .expect("scroll and theme");
    cx.run_until_parked();
    assert_eq!(gradients(window, cx).len(), 4);
    window
        .update(cx, |_, window, _| {
            let color = vega_theme::DARK.bg_base;
            for (angle, stop) in [
                (180., 16. / 120.),
                (0., 16. / 120.),
                (90., 16. / 160.),
                (270., 16. / 160.),
            ] {
                let expected = linear_gradient(
                    angle,
                    linear_color_stop(color, 0.),
                    linear_color_stop(color.opacity(0.), stop),
                );
                assert!(
                    window
                        .painted_quads()
                        .iter()
                        .any(|quad| quad.background == expected)
                );
            }
        })
        .expect("direction and theme colors");
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(80.), px(48.)));
    visual.run_until_parked();
    drop(visual);
    let resized = gradients(window, cx);
    assert_eq!(resized.len(), 4);
    assert!(
        resized
            .iter()
            .filter(|bounds| bounds.size.width == px(80.))
            .all(|bounds| bounds.size.height == px(12.))
    );
    window
        .update(cx, |harness, _, cx| {
            harness
                .scroll
                .set_offset(point(px(-f32::MAX), px(-f32::MAX)));
            cx.notify();
        })
        .expect("scroll to end");
    cx.run_until_parked();
    assert_eq!(gradients(window, cx).len(), 2);
    window
        .update(cx, |harness, _, cx| {
            harness.count = 1;
            cx.notify();
        })
        .expect("shrink content");
    cx.run_until_parked();
    assert!(gradients(window, cx).is_empty());
}

#[gpui_kit::test]
fn scroll_edge_exact_fit_epsilon_and_partial_strength_use_current_layout(cx: &mut TestAppContext) {
    let window = open(cx, Viewport::Ordinary, 0);
    assert!(gradients(window, cx).is_empty());
    for (height, expected) in [(120., 0), (121., 0), (129., 1)] {
        window
            .update(cx, |harness, _, cx| {
                harness.extent = Some(size(px(160.), px(height)));
                cx.notify();
            })
            .expect("update actual extent");
        cx.run_until_parked();
        assert_eq!(gradients(window, cx).len(), expected);
    }
    window
        .update(cx, |_, window, _| {
            let color = vega_theme::LIGHT.bg_base;
            let expected = linear_gradient(
                0.,
                linear_color_stop(color.opacity(0.5), 0.),
                linear_color_stop(color.opacity(0.), 16. / 120.),
            );
            assert!(
                window
                    .painted_quads()
                    .iter()
                    .any(|quad| quad.background == expected)
            );
        })
        .expect("nine hidden pixels produces half strength");
}

#[gpui_kit::test]
fn scroll_edge_rounded_gradient_uses_full_quad_and_excludes_border_and_gutters(
    cx: &mut TestAppContext,
) {
    let window = open(cx, Viewport::Ordinary, 12);
    window
        .update(cx, |harness, _, cx| {
            harness.border = true;
            harness.radius = px(12.);
            harness.style = ScrollFade {
                vertical_gutter: px(16.),
                horizontal_gutter: px(16.),
                ..ScrollFade::compact()
            };
            harness.scroll.set_offset(point(px(-80.), px(-80.)));
            cx.notify();
        })
        .expect("rounded viewport with gutters");
    cx.run_until_parked();
    window
        .update(cx, |harness, window, _| {
            let scale = window.scale_factor();
            let outer = Bounds::new(point(px(1.), px(1.)), size(px(142.), px(102.))).scale(scale);
            let quads = window.painted_quads();
            let fades = quads
                .iter()
                .filter(|quad| quad.background.as_solid().is_none())
                .collect::<Vec<_>>();
            assert_eq!(fades.len(), 4);
            for quad in &fades {
                assert_eq!(quad.bounds, outer);
                assert_eq!(quad.corner_radii.top_left.as_f32() / scale, 11.);
                assert_eq!(quad.corner_radii.top_right.as_f32(), 0.);
                assert_eq!(quad.corner_radii.bottom_left.as_f32(), 0.);
                assert!(quad.content_mask.bounds.left() >= outer.left());
                assert!(quad.content_mask.bounds.right() <= outer.right());
                assert!(quad.content_mask.bounds.bottom() <= outer.bottom());
            }
            for (angle, stop) in [(270., 16. / 142.), (0., 16. / 102.)] {
                let expected = linear_gradient(
                    angle,
                    linear_color_stop(harness.color, 0.),
                    linear_color_stop(harness.color.opacity(0.), stop),
                );
                let quad = fades
                    .iter()
                    .find(|quad| quad.background == expected)
                    .expect("gutter-adjusted gradient");
                if angle == 270. {
                    assert_eq!(quad.content_mask.bounds.right(), outer.right());
                    assert_eq!(quad.content_mask.bounds.size.width.as_f32() / scale, 16.);
                } else {
                    assert_eq!(quad.content_mask.bounds.bottom(), outer.bottom());
                    assert_eq!(quad.content_mask.bounds.size.height.as_f32() / scale, 16.);
                }
            }
        })
        .expect("actual rounded quad and clipping");
    window
        .update(cx, |harness, _, cx| {
            harness.border = false;
            harness.style = ScrollFade::compact();
            cx.notify();
        })
        .expect("rounded code surface without a border");
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| {
            let scale = window.scale_factor();
            let quads = window.painted_quads();
            let fades = quads
                .iter()
                .filter(|quad| quad.background.as_solid().is_none())
                .collect::<Vec<_>>();
            assert_eq!(fades.len(), 4);
            assert!(
                fades
                    .iter()
                    .all(|quad| quad.corner_radii.top_left.as_f32() / scale == 12.)
            );
            assert!(
                fades
                    .iter()
                    .any(|quad| quad.content_mask.bounds.size.width.as_f32() / scale == 16.)
            );
        })
        .expect("a 16px fade retains the actual 12px surface radius");
}

#[gpui_kit::test]
fn scroll_edge_fade_allows_underlying_click_and_focus(cx: &mut TestAppContext) {
    let window = open(cx, Viewport::Ordinary, 12);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let action = visual
        .debug_bounds("fade-action")
        .expect("action beneath end fade");
    visual.simulate_click(action.center(), Modifiers::default());
    visual.run_until_parked();
    drop(visual);
    window
        .update(cx, |harness, window, _| {
            assert_eq!(harness.clicks.get(), 1);
            assert!(harness.focus.is_focused(window));
        })
        .expect("decoration never captures the hit or focus");
}

#[gpui_kit::test]
fn scroll_edge_hidden_non_scroll_axis_never_gets_fades(cx: &mut TestAppContext) {
    let window = open(cx, Viewport::Ordinary, 12);
    for axis in [gpui_kit::Axis::Vertical, gpui_kit::Axis::Horizontal] {
        window
            .update(cx, |harness, _, cx| {
                harness.axis = Some(axis);
                harness
                    .scroll
                    .set_offset(if axis == gpui_kit::Axis::Vertical {
                        point(px(0.), px(-80.))
                    } else {
                        point(px(-80.), px(0.))
                    });
                cx.notify();
            })
            .expect("one scrollable axis with overflow on both");
        cx.run_until_parked();
        let fades = gradients(window, cx);
        assert_eq!(fades.len(), 2);
        if axis == gpui_kit::Axis::Vertical {
            assert!(
                fades
                    .iter()
                    .all(|bounds| bounds.size.width == px(160.) && bounds.size.height == px(16.))
            );
        } else {
            assert!(
                fades
                    .iter()
                    .all(|bounds| bounds.size.height == px(120.) && bounds.size.width == px(16.))
            );
        }
    }
    window
        .update(cx, |harness, _, cx| {
            harness.axis = None;
            harness.scroll.set_offset(point(px(-80.), px(-80.)));
            cx.notify();
        })
        .expect("restore true two-axis scrolling");
    cx.run_until_parked();
    assert_eq!(gradients(window, cx).len(), 4);
}

#[gpui_kit::test]
fn scroll_edge_nested_parent_rounding_and_overlapping_rail_keep_real_corner_geometry(
    cx: &mut TestAppContext,
) {
    let window = open(cx, Viewport::Nested, 12);
    window
        .update(cx, |harness, _, cx| {
            harness.border = true;
            harness.radius = px(12.);
            harness.scroll.set_offset(point(px(-80.), px(-80.)));
            cx.notify();
        })
        .expect("tool detail in its rounded parent");
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| {
            let scale = window.scale_factor();
            let quads = window.painted_quads();
            let fades = quads
                .iter()
                .filter(|quad| quad.background.as_solid().is_none())
                .collect::<Vec<_>>();
            assert_eq!(fades.len(), 4);
            assert!(fades.iter().all(|quad| quad.bounds
                == Bounds::new(point(px(1.), px(1.)), size(px(158.), px(118.))).scale(scale)));
            assert!(
                fades
                    .iter()
                    .all(|quad| quad.corner_radii.top_left.as_f32() / scale == 11.)
            );
        })
        .expect("inner fade inherits radius12 minus parent border1");
    window
        .update(cx, |harness, _, cx| {
            harness.border = false;
            harness.radius = px(16.);
            harness.style = ScrollFade::compact().bottom_extension(px(8.));
            cx.notify();
        })
        .expect("pending rail viewport ends eight pixels above parent bottom");
    cx.run_until_parked();
    window
        .update(cx, |harness, window, _| {
            let scale = window.scale_factor();
            let quads = window.painted_quads();
            let fades = quads
                .iter()
                .filter(|quad| quad.background.as_solid().is_none())
                .collect::<Vec<_>>();
            assert_eq!(fades.len(), 4);
            assert!(
                fades
                    .iter()
                    .all(|quad| quad.bounds.size.height.as_f32() / scale == 120.)
            );
            assert!(
                fades
                    .iter()
                    .all(|quad| quad.corner_radii.bottom_left.as_f32() / scale == 16.)
            );
            let bottom = linear_gradient(
                0.,
                linear_color_stop(harness.color, 8. / 120.),
                linear_color_stop(harness.color.opacity(0.), 24. / 120.),
            );
            let quad = fades
                .iter()
                .find(|quad| quad.background == bottom)
                .expect("bottom fade maps the actual child edge into parent coordinates");
            assert_eq!(quad.content_mask.bounds.bottom().as_f32() / scale, 112.);
            assert_eq!(quad.content_mask.bounds.size.height.as_f32() / scale, 16.);
        })
        .expect("parent round clip and child gradient coordinates remain distinct");
}

#[gpui_kit::test]
fn scroll_edge_virtual_handles_preserve_real_geometry_and_offset(cx: &mut TestAppContext) {
    for viewport in [Viewport::Uniform, Viewport::Variable] {
        let window = open(cx, viewport, 120);
        assert_eq!(gradients(window, cx).len(), 1);
        window
            .update(cx, |harness, _, _| {
                if matches!(harness.viewport, Viewport::Variable) {
                    assert!(harness.list.bounds_for_item(119).is_none());
                }
            })
            .expect("tail remains unmaterialized at the start");
        window
            .update(cx, |harness, _, cx| {
                match harness.viewport {
                    Viewport::Uniform => {
                        ScrollbarHandle::set_offset(&harness.uniform, point(px(0.), px(-80.)))
                    }
                    Viewport::Variable => harness.list.scroll_by(px(80.)),
                    _ => unreachable!(),
                }
                cx.notify();
            })
            .expect("scroll virtual viewport");
        cx.run_until_parked();
        assert_eq!(gradients(window, cx).len(), 2);
        window
            .update(cx, |harness, _, _| {
                assert_eq!(
                    match harness.viewport {
                        Viewport::Uniform => ScrollbarHandle::offset(&harness.uniform).y,
                        Viewport::Variable => ScrollbarHandle::offset(&harness.list).y,
                        _ => unreachable!(),
                    },
                    px(-80.)
                );
            })
            .expect("offset remains user controlled");
        window
            .update(cx, |harness, _, cx| {
                match harness.viewport {
                    Viewport::Uniform => harness.uniform.scroll_to_bottom(),
                    Viewport::Variable => harness.list.scroll_to_end(),
                    _ => unreachable!(),
                }
                cx.notify();
            })
            .expect("actual virtual end");
        cx.run_until_parked();
        assert_eq!(gradients(window, cx).len(), 1);
        window
            .update(cx, |harness, window, _| {
                let metrics = match harness.viewport {
                    Viewport::Uniform => ScrollFadeMetrics::from_handle(&harness.uniform),
                    Viewport::Variable => ScrollFadeMetrics::from_handle(&harness.list),
                    _ => unreachable!(),
                };
                assert!(metrics.start.y > px(0.));
                assert!(metrics.end.y <= px(Layout::SCROLL_EDGE_FADE_EPSILON));
                let expected = linear_gradient(
                    180.,
                    linear_color_stop(harness.color, 0.),
                    linear_color_stop(harness.color.opacity(0.), 16. / 120.),
                );
                assert!(
                    window
                        .painted_quads()
                        .iter()
                        .any(|quad| quad.background == expected)
                );
            })
            .expect("estimated range resolves to the true endpoint");
        window
            .update(cx, |harness, _, cx| {
                harness.count = 1;
                if matches!(harness.viewport, Viewport::Variable) {
                    harness.list.reset(1);
                }
                cx.notify();
            })
            .expect("short virtual content");
        cx.run_until_parked();
        assert!(gradients(window, cx).is_empty());
    }
}

#[gpui_kit::test]
fn scroll_edge_implicit_handle_survives_rerender_and_wheel_reaches_viewport(
    cx: &mut TestAppContext,
) {
    let window = open(cx, Viewport::Implicit, 12);
    assert_eq!(gradients(window, cx).len(), 1);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_event(ScrollWheelEvent {
        position: point(px(80.), px(60.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-80.))),
        modifiers: Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    visual.run_until_parked();
    drop(visual);
    assert_eq!(gradients(window, cx).len(), 2);
    window.update(cx, |_, _, cx| cx.notify()).expect("rerender");
    cx.run_until_parked();
    assert_eq!(gradients(window, cx).len(), 2);
}
