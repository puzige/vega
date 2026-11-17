use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{
    AnyElement, App, Bounds, ContentMask, Context, Element, ElementId, Font, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Render, Rgba, Subscription, WeakEntity,
    Window, div, prelude::*, px,
};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use tokio::sync::mpsc;
use vega_theme::{Layout, ThemeColors, Typography};

#[derive(Clone, PartialEq)]
enum PixelsInput {
    Background(Rgba),
    Group {
        fill: Rgba,
        border: Rgba,
    },
    Rule(Rgba),
    Label {
        text: String,
        color: Rgba,
        font: Font,
        size: f32,
    },
}

impl PixelsInput {
    fn measured(&self) -> bool {
        matches!(self, Self::Label { .. })
    }

    fn initial_height(&self) -> f32 {
        match self {
            Self::Label { size, .. } => size * Typography::BODY_LINE_HEIGHT,
            _ => 0.,
        }
    }
}

pub(super) fn background(
    key: &'static str,
    color: Rgba,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .size_full()
        .child(host(key, PixelsInput::Background(color), window, cx))
        .into_any_element()
}

pub(super) fn group(colors: ThemeColors, window: &mut Window, cx: &mut App) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .size_full()
        .child(host(
            "settings-defaults-group",
            PixelsInput::Group {
                fill: colors.bg_base,
                border: colors.border_subtle,
            },
            window,
            cx,
        ))
        .into_any_element()
}

pub(super) fn rule(
    key: &'static str,
    color: Rgba,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .w_full()
        .h(px(1.))
        .child(host(key, PixelsInput::Rule(color), window, cx))
        .into_any_element()
}

pub(super) fn label(
    key: &'static str,
    text: String,
    color: Rgba,
    size: f32,
    _window: &mut Window,
    _cx: &mut App,
) -> AnyElement {
    div()
        .id(key)
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .child(Label {
            key,
            text,
            color,
            size,
        })
        .into_any_element()
}

struct Label {
    key: &'static str,
    text: String,
    color: Rgba,
    size: f32,
}

impl IntoElement for Label {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Label {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
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
    ) -> (LayoutId, AnyElement) {
        let mut style = window.text_style();
        style.font_size = px(self.size).into();
        if self.size == Typography::HEADING_BLOCK {
            style.font_weight = Typography::HEADING_BLOCK_WEIGHT;
        }
        let mut child = host(
            self.key,
            PixelsInput::Label {
                text: self.text.clone(),
                color: self.color,
                font: style.font(),
                size: self.size,
            },
            window,
            cx,
        )
        .into_any_element();
        (child.request_layout(window, cx), child)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        child: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        child: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        child.paint(window, cx);
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    input: PixelsInput,
    width: f32,
}

struct Measurement {
    id: ProjectionId,
    width: f32,
    height: f32,
}

struct Host {
    input: PixelsInput,
    id: ProjectionId,
    width: f32,
    height: f32,
    ready: bool,
    surface: Option<Surface<Snapshot, Measurement>>,
    observation: Option<Subscription>,
    _release: Subscription,
}

impl Host {
    fn apply_measurement(&mut self, measurement: Measurement) -> bool {
        if measurement.id.matches(&self.id)
            && measurement.width == self.width
            && measurement.width.is_finite()
            && measurement.width > 0.
            && measurement.height.is_finite()
            && measurement.height > 0.
            && (!self.ready || self.height != measurement.height)
        {
            self.height = measurement.height;
            self.ready = true;
            true
        } else {
            false
        }
    }

    fn reset(&mut self, cx: &App) {
        self.observation = None;
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.id = ProjectionId::new();
        self.height = self.input.initial_height();
        self.ready = !self.input.measured();
    }
}

fn host(
    key: &'static str,
    input: PixelsInput,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::Entity<Host> {
    let host = window.use_keyed_state(key, cx, |_, cx| Host {
        height: input.initial_height(),
        ready: !input.measured(),
        input: input.clone(),
        id: ProjectionId::new(),
        width: 0.,
        surface: None,
        observation: None,
        _release: cx.on_release(|host: &mut Host, cx| {
            if let Some(surface) = host.surface.take() {
                surface.entity().read(cx).stop();
            }
        }),
    });
    host.update(cx, |host, cx| {
        if host.input != input {
            host.input = input;
            host.reset(cx);
            cx.notify();
        }
    });
    host
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = self.input.measured();
        if self.surface.is_none() && (!measured || self.width > 0.) {
            let (surface, mut measurements) = Surface::new_transparent(
                Snapshot {
                    id: self.id.clone(),
                    input: self.input.clone(),
                    width: self.width,
                },
                pixels,
                window,
                cx,
            );
            surface.focus_handle(cx).tab_stop(false);
            self.observation = Some(cx.observe(&surface.entity(), |_, _, cx| cx.notify()));
            self.surface = Some(surface);
            cx.spawn_in(window, async move |host, cx| {
                while let Some(measurement) = measurements.recv().await {
                    if host
                        .update_in(cx, |host, _, cx| {
                            if host.apply_measurement(measurement) {
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        if let Some(surface) = &mut self.surface {
            surface.sync_environment(cx);
        }
        Passive {
            child: div()
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .when(measured, |view| view.h(px(self.height)))
                .when(!measured, |view| view.h_full())
                .children(self.surface.as_ref().map(Surface::entity))
                .into_any_element(),
            measured,
            width: self.width,
            ready: self.ready,
            host: cx.weak_entity(),
        }
    }
}

struct Passive {
    child: AnyElement,
    measured: bool,
    width: f32,
    ready: bool,
    host: WeakEntity<Host>,
}

impl IntoElement for Passive {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Passive {
    type RequestLayoutState = ();
    type PrepaintState = bool;
    fn id(&self) -> Option<ElementId> {
        None
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
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let width = f32::from(bounds.size.width);
        if self.measured && (!width.is_finite() || width <= 0. || self.width != width) {
            let _ = self.host.update(cx, |host, cx| {
                let width = if width.is_finite() { width.max(0.) } else { 0. };
                if host.width != width {
                    host.width = width;
                    host.reset(cx);
                    cx.notify();
                }
            });
            return false;
        }
        let mask = ContentMask {
            bounds: Bounds {
                origin: window.content_mask().bounds.center(),
                size: Default::default(),
            },
        };
        window.with_content_mask(Some(mask), |window| self.child.prepaint(window, cx));
        self.ready
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        ready: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if *ready {
            let bounds = bounds.intersect(&window.content_mask().bounds);
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                self.child.paint(window, cx)
            });
        }
    }
}

fn pixels(state: ns::Signal<Snapshot>, measurements: mpsc::Sender<Measurement>) -> ns::AnyElement {
    let snapshot = state.get_untracked();
    let base = ns::view().size_full().accessibility_elements_hidden(true);
    match snapshot.input {
        PixelsInput::Background(color) | PixelsInput::Rule(color) => {
            base.bg(crate::nsh::color(color)).into_element()
        }
        PixelsInput::Group { fill, border } => base
            .bg(crate::nsh::color(fill))
            .border(ns::px(1.))
            .border_color(crate::nsh::color(border))
            .rounded(ns::px(Layout::PANEL_RADIUS))
            .into_element(),
        PixelsInput::Label {
            text,
            color,
            font,
            size,
        } => {
            let mut families = vec![font.family.to_string()];
            if let Some(fallbacks) = &font.fallbacks {
                families.extend(fallbacks.fallback_list().iter().cloned());
            }
            let mut label = ns::text(text.clone())
                .label(text)
                .w_full()
                .flex_shrink_0()
                .text_size(ns::px(size))
                .line_height(ns::px(size * Typography::BODY_LINE_HEIGHT))
                .text_color(crate::nsh::color(color))
                .font_weight(ns::FontWeight(font.weight.0 as u16))
                .font_family(
                    families
                        .iter()
                        .map(|family| format!("\"{family}\""))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            if font.style != gpui_kit::FontStyle::Normal {
                label = label.italic();
            }
            for (tag, value) in font.features.tag_value_list() {
                if *value != 1 {
                    continue;
                }
                label = match tag.as_str() {
                    "smcp" => label.small_caps(),
                    "onum" => label.oldstyle_nums(),
                    "lnum" => label.lining_nums(),
                    "tnum" => label.tabular_nums(),
                    "pnum" => label.proportional_nums(),
                    _ => label,
                };
            }
            let measurements = measurements.clone();
            ns::view()
                .w(ns::px(snapshot.width))
                .flex_col()
                .flex_shrink_0()
                .on_layout(move |bounds: &ns::Bounds<ns::Points>| {
                    let _ = measurements.try_send(Measurement {
                        id: snapshot.id.clone(),
                        width: snapshot.width,
                        height: bounds.size.height.0,
                    });
                })
                .child(label)
                .into_element()
        }
    }
}
