use std::cell::Cell;
use std::rc::Rc;

use super::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AppContext, Context, Modifiers, Render, TestAppContext, VisualTestContext, WindowHandle, div,
    point, px, size,
};

struct Harness {
    enabled: bool,
    selected: bool,
    rest: Option<Rgba>,
    background_override: Option<Rgba>,
    clicks: Rc<Cell<usize>>,
    hover_events: Rc<Cell<usize>>,
    tooltip_calls: Rc<Cell<usize>>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        let hover_events = self.hover_events.clone();
        let tooltip_calls = self.tooltip_calls.clone();
        let colors = vega_theme::LIGHT;
        div()
            .size_full()
            .bg(colors.bg_base)
            .p_4()
            .flex()
            .gap_2()
            .child(
                div()
                    .id("interaction-control")
                    .debug_selector(|| "interaction-control".into())
                    .w(px(120.))
                    .h(px(28.))
                    .rounded(px(8.))
                    .map(|control| match self.rest {
                        Some(rest) => control.bg(colors.brand_primary_strong).interaction_surface(
                            rest,
                            colors.bg_hover,
                            colors.bg_active,
                            self.enabled,
                        ),
                        None => control.interactive(colors, self.enabled, self.selected),
                    })
                    .when_some(self.background_override, |control, background| {
                        control.bg(background)
                    })
                    .on_hover(move |_, _, _| hover_events.set(hover_events.get() + 1))
                    .tooltip(move |_, cx| {
                        tooltip_calls.set(tooltip_calls.get() + 1);
                        cx.new(|_| TooltipHarness).into()
                    })
                    .when(self.enabled, |control| {
                        control.on_mouse_up(gpui_kit::MouseButton::Left, move |_, _, _| {
                            clicks.set(clicks.get() + 1)
                        })
                    })
                    .child("Control"),
            )
            .child(
                div()
                    .id("interaction-second")
                    .debug_selector(|| "interaction-second".into())
                    .w(px(120.))
                    .h(px(28.))
                    .interactive(colors, true, false)
                    .child("Second"),
            )
    }
}

fn open(cx: &mut TestAppContext, enabled: bool, selected: bool) -> WindowHandle<Harness> {
    let window = cx.open_window(size(px(300.), px(100.)), |window, cx| {
        observe_system_motion(window, cx);
        Harness {
            enabled,
            selected,
            rest: None,
            background_override: None,
            clicks: Rc::default(),
            hover_events: Rc::default(),
            tooltip_calls: Rc::default(),
        }
    });
    cx.run_until_parked();
    window
}

fn surface(window: WindowHandle<Harness>, cx: &mut TestAppContext) -> Rgba {
    named_surface(window, cx, "interaction-control")
}

fn named_surface(
    window: WindowHandle<Harness>,
    cx: &mut TestAppContext,
    selector: &'static str,
) -> Rgba {
    let bounds = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .expect("control");
    window
        .update(cx, |_, window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .iter()
                .find(|quad| {
                    (quad.bounds.left().as_f32() / scale - bounds.left().as_f32()).abs() < 0.1
                        && (quad.bounds.top().as_f32() / scale - bounds.top().as_f32()).abs() < 0.1
                        && (quad.bounds.size.width.as_f32() / scale - 120.).abs() < 0.1
                })
                .and_then(|quad| quad.background.as_solid())
                .map(Rgba::from)
                .unwrap_or_default()
        })
        .expect("window")
}

fn pointer(window: WindowHandle<Harness>, cx: &mut TestAppContext, inside: bool) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual.debug_bounds("interaction-control").expect("control");
    visual.simulate_mouse_move(
        if inside {
            bounds.center()
        } else {
            point(px(250.), px(80.))
        },
        None,
        Modifiers::default(),
    );
    visual.run_until_parked();
}

fn advance(window: WindowHandle<Harness>, cx: &mut TestAppContext, millis: u64) {
    cx.executor().advance_clock(Duration::from_millis(millis));
    window
        .update(cx, |_, window, cx| window.simulate_next_frame(cx))
        .expect("frame");
    cx.run_until_parked();
}

#[gpui_kit::test]
fn issue296_interaction_enters_leaves_and_reverses_without_layout_shift(cx: &mut TestAppContext) {
    let window = open(cx, true, false);
    let bounds =
        VisualTestContext::from_window(window.into(), cx).debug_bounds("interaction-control");
    pointer(window, cx, false);
    pointer(window, cx, true);
    advance(window, cx, 40);
    let entering = surface(window, cx);
    assert!(
        entering.a > 0. && entering.a < 1.,
        "hover must have a real intermediate frame: {entering:?}"
    );
    pointer(window, cx, false);
    let reversed = surface(window, cx);
    assert!(
        (reversed.a - entering.a).abs() < 0.03,
        "reversal starts from the current value"
    );
    advance(window, cx, 35);
    let leaving = surface(window, cx);
    assert!(
        leaving.a > 0. && leaving.a < entering.a,
        "leave must interpolate back"
    );
    advance(window, cx, 120);
    assert_eq!(surface(window, cx).a, 0.);
    assert_eq!(
        VisualTestContext::from_window(window.into(), cx).debug_bounds("interaction-control"),
        bounds
    );
    window
        .update(cx, |_, window, cx| window.simulate_next_frame(cx))
        .expect("drain frame");
    cx.run_until_parked();
    assert_eq!(
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .expect("pending frames"),
        0
    );
    assert!(
        window
            .update(cx, |view, _, _| view.hover_events.get())
            .expect("hover state")
            >= 2
    );
}

#[gpui_kit::test]
fn issue296_interaction_reduced_motion_and_disabled_are_static(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let window = open(cx, true, false);
    pointer(window, cx, true);
    assert_eq!(surface(window, cx), vega_theme::LIGHT.bg_hover);
    pointer(window, cx, false);
    assert_eq!(surface(window, cx).a, 0.);
    window
        .update(cx, |_, window, cx| window.simulate_next_frame(cx))
        .expect("drain static frame");
    cx.run_until_parked();
    assert_eq!(
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .expect("static pending frames"),
        0
    );
    let disabled = open(cx, false, false);
    pointer(disabled, cx, true);
    assert_eq!(surface(disabled, cx).a, 0.);
    let mut visual = VisualTestContext::from_window(disabled.into(), cx);
    let bounds = visual.debug_bounds("interaction-control").expect("control");
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    assert_eq!(
        disabled
            .update(&mut visual, |view, _, _| view.clicks.get())
            .expect("click state"),
        0
    );
}

#[gpui_kit::test]
fn issue296_interaction_selected_surface_and_activation_are_preserved(cx: &mut TestAppContext) {
    let window = open(cx, true, true);
    pointer(window, cx, true);
    advance(window, cx, 60);
    assert_eq!(surface(window, cx), vega_theme::LIGHT.bg_active);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let bounds = visual.debug_bounds("interaction-control").expect("control");
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    assert_eq!(
        window
            .update(&mut visual, |view, _, _| view.clicks.get())
            .expect("click state"),
        1
    );
}

struct TooltipHarness;

impl Render for TooltipHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("Control tooltip")
    }
}

#[gpui_kit::test]
fn issue296_interaction_controls_have_independent_motion_state(cx: &mut TestAppContext) {
    let window = open(cx, true, false);
    pointer(window, cx, true);
    advance(window, cx, 40);
    let first = surface(window, cx).a;
    assert!(first > 0. && first < 1.);
    assert_eq!(named_surface(window, cx, "interaction-second").a, 0.);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let second = visual
        .debug_bounds("interaction-second")
        .expect("second control");
    visual.simulate_mouse_move(second.center(), None, Modifiers::default());
    visual.run_until_parked();
    advance(window, cx, 35);
    let first_leaving = surface(window, cx).a;
    let second_entering = named_surface(window, cx, "interaction-second").a;
    assert!(first_leaving > 0. && first_leaving < first);
    assert!(second_entering > 0. && second_entering < 1.);
    assert!((first_leaving - second_entering).abs() > 0.1);
    advance(window, cx, 120);
    assert_eq!(surface(window, cx).a, 0.);
    assert_eq!(
        named_surface(window, cx, "interaction-second"),
        vega_theme::LIGHT.bg_hover
    );
}

#[gpui_kit::test]
fn issue296_interaction_preserves_tooltip_and_hover_callback(cx: &mut TestAppContext) {
    let window = open(cx, true, false);
    pointer(window, cx, true);
    advance(window, cx, 800);
    assert!(
        window
            .update(cx, |view, _, _| view.tooltip_calls.get())
            .expect("tooltip count")
            > 0
    );
    assert!(
        window
            .update(cx, |view, _, _| view.hover_events.get())
            .expect("hover count")
            > 0
    );
    pointer(window, cx, false);
    advance(window, cx, 120);
    assert_eq!(surface(window, cx).a, 0.);
}

#[gpui_kit::test]
fn issue296_interaction_press_release_and_drag_out_clear_immediately(cx: &mut TestAppContext) {
    let window = open(cx, true, false);
    pointer(window, cx, true);
    advance(window, cx, 120);
    let bounds = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("interaction-control")
        .expect("control");
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_down(
        bounds.center(),
        gpui_kit::MouseButton::Left,
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert_eq!(surface(window, cx), vega_theme::LIGHT.bg_active);
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_up(
        bounds.center(),
        gpui_kit::MouseButton::Left,
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert_eq!(surface(window, cx), vega_theme::LIGHT.bg_hover);
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_down(
        bounds.center(),
        gpui_kit::MouseButton::Left,
        Modifiers::default(),
    );
    cx.run_until_parked();
    let outside = point(px(280.), px(80.));
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_move(
        outside,
        Some(gpui_kit::MouseButton::Left),
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert_ne!(surface(window, cx), vega_theme::LIGHT.bg_active);
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_up(
        outside,
        gpui_kit::MouseButton::Left,
        Modifiers::default(),
    );
    cx.run_until_parked();
    advance(window, cx, 120);
    assert_eq!(surface(window, cx).a, 0.);
    assert_eq!(
        VisualTestContext::from_window(window.into(), cx).debug_bounds("interaction-control"),
        Some(bounds)
    );
    assert_eq!(
        window
            .update(cx, |view, _, _| view.clicks.get())
            .expect("click state"),
        1
    );
}

#[gpui_kit::test]
fn issue296_interaction_motion_preference_refreshes_and_failure_is_static(cx: &mut TestAppContext) {
    let preference = Rc::new(Cell::new(Some(true)));
    let calls = Rc::new(Cell::new(0));
    let reader_value = preference.clone();
    let reader_calls = calls.clone();
    cx.update(|cx| {
        platform::install_with_reader(
            cx,
            Rc::new(move || {
                reader_calls.set(reader_calls.get() + 1);
                reader_value.get()
            }),
        )
    });
    assert_eq!(calls.get(), 1);
    assert!(cx.read(|cx| cx.reduce_motion()));
    preference.set(Some(false));
    cx.update(refresh_system_motion);
    assert!(!cx.read(|cx| cx.reduce_motion()));
    let window = open(cx, true, false);
    VisualTestContext::from_window(window.into(), cx).deactivate_window();
    preference.set(Some(true));
    let foreground_calls = calls.get();
    window
        .update(cx, |_, window, _| window.activate_window())
        .expect("activate");
    cx.run_until_parked();
    assert!(calls.get() > foreground_calls);
    assert!(cx.read(|cx| cx.reduce_motion()));
    preference.set(Some(false));
    pointer(window, cx, true);
    assert!(!cx.read(|cx| cx.reduce_motion()));
    pointer(window, cx, false);
    preference.set(Some(true));
    pointer(window, cx, true);
    assert!(cx.read(|cx| cx.reduce_motion()));
    assert_eq!(surface(window, cx), vega_theme::LIGHT.bg_hover);
    pointer(window, cx, false);
    preference.set(None);
    cx.update(|cx| cx.set_reduce_motion(false));
    pointer(window, cx, true);
    assert!(cx.read(|cx| cx.reduce_motion()));
    assert_eq!(surface(window, cx), vega_theme::LIGHT.bg_hover);
    pointer(window, cx, false);
    assert_eq!(surface(window, cx).a, 0.);
}

struct IconHarness(crate::icons::Icon);

impl Render for IconHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .debug_selector(|| "icon-slot".into())
                    .flex()
                    .child(crate::icons::icon(self.0, vega_theme::LIGHT.text_primary)),
            )
            .child(
                div()
                    .debug_selector(|| "icon-label".into())
                    .text_size(px(13.))
                    .line_height(gpui_kit::relative(1.55))
                    .child("Label"),
            )
            .child(
                crate::icons::shell_icon_button(
                    self.0,
                    "icon-control",
                    None,
                    true,
                    false,
                    vega_theme::LIGHT,
                    |_, _, _| {},
                )
                .debug_selector(|| "icon-control".into()),
            )
    }
}

#[gpui_kit::test]
fn issue296_interaction_both_icon_paths_center_with_text_and_keep_focus_bounds(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        cx.set_global(vega_theme::Theme::light());
    });
    for kind in [crate::icons::Icon::Search, crate::icons::Icon::Pin] {
        let window = cx.open_window(size(px(240.), px(80.)), |_, _| IconHarness(kind));
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let icon = visual.debug_bounds("icon-slot").expect("icon bounds");
        let label = visual.debug_bounds("icon-label").expect("label bounds");
        let button = visual.debug_bounds("icon-control").expect("button bounds");
        assert_eq!(icon.size, size(px(16.), px(16.)));
        assert!((icon.center().y - label.center().y).abs() <= px(0.5));
        assert!((label.size.height - px(20.15)).abs() <= px(0.5));
        assert_eq!(button.size, size(px(28.), px(28.)));
        visual.update(|window, cx| window.focus_next(cx));
        visual.run_until_parked();
        assert_eq!(visual.debug_bounds("icon-slot"), Some(icon));
        assert_eq!(visual.debug_bounds("icon-label"), Some(label));
        assert_eq!(visual.debug_bounds("icon-control"), Some(button));
    }
}

pub(crate) fn assert_native_control_press<V: Render>(
    window: WindowHandle<V>,
    cx: &mut TestAppContext,
    selector: &'static str,
) {
    window
        .update(cx, |_, window, _| window.activate_window())
        .expect("activate control window");
    cx.run_until_parked();
    fn color<V: Render>(
        window: WindowHandle<V>,
        cx: &mut TestAppContext,
        bounds: Bounds<Pixels>,
    ) -> Rgba {
        window
            .update(cx, |_, window, _| {
                let scale = window.scale_factor();
                window
                    .painted_quads()
                    .iter()
                    .filter(|quad| {
                        (quad.bounds.left().as_f32() / scale - bounds.left().as_f32()).abs() < 0.1
                            && (quad.bounds.top().as_f32() / scale - bounds.top().as_f32()).abs()
                                < 0.1
                            && (quad.bounds.size.width.as_f32() / scale
                                - bounds.size.width.as_f32())
                            .abs()
                                < 0.1
                            && (quad.bounds.size.height.as_f32() / scale
                                - bounds.size.height.as_f32())
                            .abs()
                                < 0.1
                    })
                    .filter_map(|quad| quad.background.as_solid())
                    .rfind(|bg| bg.a > 0.)
                    .map(Rgba::from)
                    .unwrap_or_default()
            })
            .expect("color")
    }
    let bounds = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .expect("native button");
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_move(
        bounds.center(),
        None,
        Modifiers::default(),
    );
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(120));
    window
        .update(cx, |_, window, cx| window.simulate_next_frame(cx))
        .expect("frame");
    cx.run_until_parked();
    assert_eq!(color(window, cx, bounds), vega_theme::LIGHT.bg_hover);
    for release_inside in [true, false] {
        VisualTestContext::from_window(window.into(), cx).simulate_mouse_down(
            bounds.center(),
            gpui_kit::MouseButton::Left,
            Modifiers::default(),
        );
        cx.run_until_parked();
        assert_eq!(
            color(window, cx, bounds),
            vega_theme::LIGHT.bg_active,
            "native button pressed"
        );
        let release = if release_inside {
            bounds.center()
        } else {
            point(px(350.), px(180.))
        };
        if !release_inside {
            VisualTestContext::from_window(window.into(), cx).simulate_mouse_move(
                release,
                Some(gpui_kit::MouseButton::Left),
                Modifiers::default(),
            );
            cx.run_until_parked();
            assert_ne!(
                color(window, cx, bounds),
                vega_theme::LIGHT.bg_active,
                "native drag-out clears pressed"
            );
        }
        VisualTestContext::from_window(window.into(), cx).simulate_mouse_up(
            release,
            gpui_kit::MouseButton::Left,
            Modifiers::default(),
        );
        cx.run_until_parked();
        assert_ne!(
            color(window, cx, bounds),
            vega_theme::LIGHT.bg_active,
            "native release clears pressed"
        );
    }
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_move(
        bounds.center(),
        None,
        Modifiers::default(),
    );
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_down(
        bounds.center(),
        gpui_kit::MouseButton::Left,
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert_eq!(color(window, cx, bounds), vega_theme::LIGHT.bg_active);
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_move(
        point(px(350.), px(180.)),
        Some(gpui_kit::MouseButton::Left),
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert_ne!(color(window, cx, bounds), vega_theme::LIGHT.bg_active);
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_move(
        bounds.center(),
        Some(gpui_kit::MouseButton::Left),
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert_eq!(
        color(window, cx, bounds),
        vega_theme::LIGHT.bg_active,
        "held pointer re-entry restores pressed"
    );
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_up(
        bounds.center(),
        gpui_kit::MouseButton::Left,
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert_ne!(color(window, cx, bounds), vega_theme::LIGHT.bg_active);
    assert_eq!(
        VisualTestContext::from_window(window.into(), cx).debug_bounds(selector),
        Some(bounds)
    );
}

#[gpui_kit::test]
fn issue296_interaction_semantic_changes_snap_and_cancel_hover_motion(cx: &mut TestAppContext) {
    let window = open(cx, false, false);
    pointer(window, cx, false);
    window
        .update(cx, |view, _, cx| {
            view.enabled = true;
            view.rest = Some(vega_theme::LIGHT.accent);
            cx.notify();
        })
        .expect("enable semantic action");
    cx.run_until_parked();
    assert_eq!(
        u32::from(surface(window, cx)),
        u32::from(vega_theme::LIGHT.accent)
    );
    window
        .update(cx, |view, _, cx| {
            view.rest = None;
            cx.notify();
        })
        .expect("clear rest");
    cx.run_until_parked();
    assert_eq!(surface(window, cx).a, 0.);
    pointer(window, cx, true);
    advance(window, cx, 40);
    assert!(surface(window, cx).a > 0. && surface(window, cx).a < 1.);
    window
        .update(cx, |view, _, cx| {
            view.selected = true;
            cx.notify();
        })
        .expect("select during motion");
    cx.run_until_parked();
    assert_eq!(surface(window, cx), vega_theme::LIGHT.bg_active);
    pointer(window, cx, false);
    window
        .update(cx, |view, _, cx| {
            view.selected = false;
            cx.notify();
        })
        .expect("clear selection");
    cx.run_until_parked();
    assert_eq!(surface(window, cx).a, 0.);
    window
        .update(cx, |_, window, cx| window.simulate_next_frame(cx))
        .expect("drain cancelled frame");
    cx.run_until_parked();
    assert_eq!(
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .expect("no pending motion"),
        0
    );
    pointer(window, cx, true);
    advance(window, cx, 40);
    let resumed = surface(window, cx).a;
    assert!(resumed > 0. && resumed < 1.);
    pointer(window, cx, false);
    advance(window, cx, 35);
    assert!(surface(window, cx).a > 0. && surface(window, cx).a < resumed);
}

#[gpui_kit::test]
fn issue296_interaction_explicit_rest_is_immediate_and_survives_hover(cx: &mut TestAppContext) {
    let window = open(cx, true, false);
    pointer(window, cx, false);
    window
        .update(cx, |view, _, cx| {
            view.background_override = Some(vega_theme::LIGHT.accent);
            cx.notify();
        })
        .expect("explicit keyboard surface");
    cx.run_until_parked();
    assert_eq!(
        u32::from(surface(window, cx)),
        u32::from(vega_theme::LIGHT.accent)
    );
    pointer(window, cx, true);
    advance(window, cx, 120);
    assert_eq!(surface(window, cx), vega_theme::LIGHT.bg_hover);
    pointer(window, cx, false);
    advance(window, cx, 120);
    assert_eq!(
        u32::from(surface(window, cx)),
        u32::from(vega_theme::LIGHT.accent)
    );
    window
        .update(cx, |view, _, cx| {
            view.background_override = None;
            cx.notify();
        })
        .expect("remove keyboard surface");
    cx.run_until_parked();
    assert_eq!(surface(window, cx).a, 0.);
}
