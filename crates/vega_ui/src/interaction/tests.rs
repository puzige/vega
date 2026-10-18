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

struct TooltipHarness;

impl Render for TooltipHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("Control tooltip")
    }
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
