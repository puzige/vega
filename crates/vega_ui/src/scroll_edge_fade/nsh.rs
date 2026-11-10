#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use super::{FadeRegion, ScrollFade, ScrollFadeMetrics};
use crate::nsh::Surface;
use gpui_kit::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Corners, IntoElement, ParentElement,
    Pixels, Rgba, Size, Styled, Subscription, Window, div, px, size,
};
use ns::{Accessible as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use nsh_platform_gpui::NativeComponent;
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Debug, PartialEq)]
struct Gradient {
    bounds: Bounds<Pixels>,
    radii: Corners<Pixels>,
    color: Rgba,
    angle: f32,
    start: f32,
    stop: f32,
}

impl Gradient {
    fn new(
        metrics: ScrollFadeMetrics,
        style: ScrollFade,
        color: Rgba,
        region: &FadeRegion,
    ) -> Self {
        let mut outer = metrics.bounds;
        outer.origin += gpui_kit::point(style.inset, style.inset);
        outer.size.width =
            (outer.size.width - style.inset * 2. - style.vertical_gutter).max(px(0.));
        outer.size.height =
            (outer.size.height - style.inset * 2. - style.horizontal_gutter).max(px(0.));
        outer.size.height += style.bottom_extension;
        let extension = if region.angle == 0. {
            style.bottom_extension
        } else {
            px(0.)
        };
        let (length, viewport_length) = if region.angle == 0. || region.angle == 180. {
            (region.bounds.size.height, outer.size.height)
        } else {
            (region.bounds.size.width, outer.size.width)
        };
        let mut radii = Corners::all(style.radius).clamp_radii_for_quad_size(outer.size);
        if style.vertical_gutter > px(0.) {
            radii.top_right = px(0.);
            radii.bottom_right = px(0.);
        }
        if style.horizontal_gutter > px(0.) {
            radii.bottom_left = px(0.);
            radii.bottom_right = px(0.);
        }
        Self {
            bounds: outer,
            radii,
            color: color.opacity(region.strength),
            angle: region.angle,
            start: f32::from(extension) / f32::from(viewport_length),
            stop: f32::from(extension + length) / f32::from(viewport_length),
        }
    }

    fn valid(&self) -> bool {
        let bounds = self.bounds;
        !bounds.is_empty()
            && [
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
                self.start,
                self.stop,
                self.color.r,
                self.color.g,
                self.color.b,
                self.color.a,
            ]
            .into_iter()
            .all(f32::is_finite)
    }
}

#[derive(Clone, PartialEq)]
struct Snapshot {
    gradient: Gradient,
    clip: Bounds<Pixels>,
    viewport: Size<Pixels>,
    scale: f32,
}

#[derive(Clone)]
struct Presentation {
    snapshot: Snapshot,
    revision: u64,
}

struct Receipt(Rc<Cell<u64>>);

impl NativeComponent for Receipt {
    fn set_prop(
        &mut self,
        name: &str,
        value: Option<&nsh_core::JsonValue>,
        _: &mut Window,
        _: &mut App,
    ) {
        if name == "revision"
            && let Some(revision) = value.and_then(nsh_core::JsonValue::as_u64)
        {
            self.0.set(revision);
        }
    }
}

struct Edge {
    surface: Surface<Presentation, ()>,
    published: Presentation,
    receipt: Rc<Cell<u64>>,
}

struct Host {
    edges: [Option<Edge>; 4],
    _release: Subscription,
}

struct PreparedEdge {
    element: AnyElement,
    gradient: Gradient,
    revision: u64,
    receipt: Rc<Cell<u64>>,
}

pub struct PreparedScrollFade {
    edges: Vec<PreparedEdge>,
    color: Rgba,
    clip: Bounds<Pixels>,
    viewport: Size<Pixels>,
    scale: f32,
}

impl ScrollFadeMetrics {
    pub(crate) fn prepaint(
        self,
        color: Rgba,
        style: ScrollFade,
        window: &mut Window,
        cx: &mut App,
    ) -> PreparedScrollFade {
        let clip = window.content_mask().bounds;
        let mut prepared = PreparedScrollFade {
            edges: Vec::new(),
            color,
            clip,
            viewport: window.viewport_size(),
            scale: window.scale_factor(),
        };
        let regions = self.regions(style);
        if regions.is_empty() || clip.is_empty() {
            return prepared;
        }
        let host = window.use_keyed_state("nsh-scroll-edge-fade", cx, |_, cx| Host {
            edges: std::array::from_fn(|_| None),
            _release: cx.on_release(|host: &mut Host, cx| {
                for edge in host.edges.iter().flatten() {
                    edge.surface.entity().read(cx).stop();
                }
            }),
        });
        for region in regions {
            let gradient = Gradient::new(self, style, color, &region);
            if !gradient.valid() || gradient.bounds.intersect(&clip).is_empty() {
                continue;
            }
            let index = match region.angle {
                180. => 0,
                0. => 1,
                90. => 2,
                _ => 3,
            };
            let snapshot = Snapshot {
                gradient: gradient.clone(),
                clip,
                viewport: prepared.viewport,
                scale: prepared.scale,
            };
            let (surface, revision, receipt) = host.update(cx, |host, cx| {
                let edge = host.edges[index].get_or_insert_with(|| {
                    let published = Presentation {
                        snapshot: snapshot.clone(),
                        revision: 1,
                    };
                    let (surface, _) =
                        Surface::new_transparent(published.clone(), pixels, window, cx);
                    surface.focus_handle(cx).tab_stop(false);
                    let receipt = Rc::new(Cell::new(0));
                    let mounted = receipt.clone();
                    surface.entity().read(cx).register_component(
                        "scroll-fade-receipt",
                        Rc::new(move |_| Box::new(Receipt(mounted.clone()))),
                    );
                    Edge {
                        surface,
                        published,
                        receipt,
                    }
                });
                edge.surface.sync_environment(cx);
                if edge.published.snapshot != snapshot {
                    edge.published = Presentation {
                        snapshot,
                        revision: edge.published.revision.wrapping_add(1).max(1),
                    };
                    edge.surface.publish(edge.published.clone());
                }
                (
                    edge.surface.entity(),
                    edge.published.revision,
                    edge.receipt.clone(),
                )
            });
            let mut element = div()
                .w(gradient.bounds.size.width)
                .h(gradient.bounds.size.height)
                .child(surface)
                .into_any_element();
            window.with_content_mask(
                Some(ContentMask {
                    bounds: Bounds::new(clip.center(), Size::default()),
                }),
                |window| {
                    element.prepaint_as_root(
                        gradient.bounds.origin,
                        size(
                            AvailableSpace::Definite(gradient.bounds.size.width),
                            AvailableSpace::Definite(gradient.bounds.size.height),
                        ),
                        window,
                        cx,
                    );
                },
            );
            prepared.edges.push(PreparedEdge {
                element,
                gradient,
                revision,
                receipt,
            });
        }
        prepared
    }
}

impl PreparedScrollFade {
    pub(crate) fn paint(
        &mut self,
        metrics: ScrollFadeMetrics,
        style: ScrollFade,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.viewport != window.viewport_size()
            || self.scale != window.scale_factor()
            || self.clip != window.content_mask().bounds
        {
            return;
        }
        for region in metrics.regions(style) {
            let gradient = Gradient::new(metrics, style, self.color, &region);
            let Some(edge) = self.edges.iter_mut().find(|edge| edge.gradient == gradient) else {
                if !region.bounds.intersect(&self.clip).is_empty() {
                    window.request_animation_frame();
                }
                continue;
            };
            let bounds = region.bounds.intersect(&self.clip);
            if !bounds.is_empty() && edge.receipt.get() == edge.revision {
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    edge.element.paint(window, cx);
                });
            }
        }
    }
}

fn pixels(state: ns::Signal<Presentation>, _: tokio::sync::mpsc::Sender<()>) -> ns::AnyElement {
    ns::view()
        .size_full()
        .accessibility_elements_hidden(true)
        .rounded_tl(move || ns::px(f32::from(state.get().snapshot.gradient.radii.top_left)))
        .rounded_tr(move || ns::px(f32::from(state.get().snapshot.gradient.radii.top_right)))
        .rounded_bl(move || ns::px(f32::from(state.get().snapshot.gradient.radii.bottom_left)))
        .rounded_br(move || ns::px(f32::from(state.get().snapshot.gradient.radii.bottom_right)))
        .bg_linear(move || ns::deg(state.get().snapshot.gradient.angle))
        .gradient_from(move || crate::nsh::color(state.get().snapshot.gradient.color))
        .gradient_to(move || crate::nsh::color(state.get().snapshot.gradient.color.opacity(0.)))
        .gradient_from_position(move || ns::percentage(state.get().snapshot.gradient.start))
        .gradient_to_position(move || ns::percentage(state.get().snapshot.gradient.stop))
        .child(
            ns::native("scroll-fade-receipt")
                .absolute()
                .w(ns::px(0.))
                .h(ns::px(0.))
                .accessibility_elements_hidden(true)
                .prop("revision", move || {
                    nsh_core::JsonValue::from(state.get().revision)
                }),
        )
        .into_element()
}
