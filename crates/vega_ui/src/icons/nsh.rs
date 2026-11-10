use std::hash::{DefaultHasher, Hash, Hasher};

use gpui_kit::{prelude::*, *};
use ns::{Accessible as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;

use super::{Icon, nsh_source};
use crate::nsh::Surface;

#[derive(Clone, PartialEq)]
struct Glyph {
    kind: Icon,
    color: Rgba,
    size: f32,
    #[cfg(feature = "nsh-reading-integration")]
    observation: Option<super::host_regressions::Observation>,
}

struct Host {
    surface: Surface<Glyph, ()>,
    published: Glyph,
    _release: Subscription,
}

pub(super) fn element(kind: Icon, color: Rgba, size: f32) -> AnyElement {
    NativeIcon {
        glyph: Glyph {
            kind,
            color,
            size,
            #[cfg(feature = "nsh-reading-integration")]
            observation: None,
        },
    }
    .into_any_element()
}

struct NativeIcon {
    glyph: Glyph,
}

impl IntoElement for NativeIcon {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for NativeIcon {
    type RequestLayoutState = LayoutId;
    type PrepaintState = AnyElement;

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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout = window.request_layout(
            Style {
                size: size(px(self.glyph.size).into(), px(self.glyph.size).into()),
                flex_shrink: 0.,
                ..Default::default()
            },
            [],
            cx,
        );
        (layout, layout)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let mut key = DefaultHasher::new();
        layout.hash(&mut key);
        #[cfg(feature = "nsh-reading-integration")]
        {
            self.glyph.observation = super::host_regressions::icon_observation(self.glyph.kind, cx);
        }
        let glyph = self.glyph.clone();
        let host = window.use_keyed_state(("nsh-icon-surface", key.finish()), cx, |window, cx| {
            let (surface, _) = Surface::new_transparent(glyph.clone(), pixels, window, cx);
            surface.focus_handle(cx).tab_stop(false);
            Host {
                surface,
                published: glyph.clone(),
                _release: cx.on_release(|host: &mut Host, cx| {
                    host.surface.entity().read(cx).stop();
                }),
            }
        });
        let surface = host.update(cx, |host, cx| {
            host.surface.sync_environment(cx);
            if host.published != glyph {
                host.surface.publish(glyph.clone());
                host.published = glyph;
            }
            #[cfg(feature = "nsh-reading-integration")]
            if let Some(observation) = &host.published.observation {
                observation.host(
                    host.surface.entity().entity_id(),
                    bounds,
                    host.surface.errors().borrow().clone(),
                );
            }
            host.surface.entity()
        });
        let mut child = div()
            .id(paint_id(bounds, window))
            .size(px(self.glyph.size))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(surface)
            .into_any_element();
        child.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        let mask = ContentMask {
            bounds: Bounds {
                origin: window.content_mask().bounds.center(),
                size: Default::default(),
            },
        };
        #[cfg(feature = "nsh-reading-integration")]
        if let Some(observation) = &self.glyph.observation {
            observation.input(mask.bounds);
        }
        window.with_content_mask(Some(mask), |window| {
            child.prepaint_at(bounds.origin, window, cx);
        });
        child
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        child: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let bounds = bounds.intersect(&window.content_mask().bounds);
        #[cfg(feature = "nsh-reading-integration")]
        if let Some(observation) = &self.glyph.observation {
            observation.paint(bounds);
        }
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            child.paint(window, cx);
        });
    }
}

pub(super) fn paint_id(bounds: Bounds<Pixels>, window: &Window) -> ElementId {
    let mut key = DefaultHasher::new();
    bounds
        .intersect(&window.content_mask().bounds)
        .hash(&mut key);
    ("nsh-passive-paint", key.finish()).into()
}

fn pixels(state: ns::Signal<Glyph>, _: tokio::sync::mpsc::Sender<()>) -> ns::AnyElement {
    #[cfg(feature = "nsh-reading-integration")]
    if let Some(observation) = &state.get_untracked().observation {
        observation.setup();
    }
    let root = ns::view().size_full().accessibility_elements_hidden(true);
    #[cfg(feature = "nsh-reading-integration")]
    let root = {
        use ns::Interactive as _;
        let observation = state.get_untracked().observation;
        root.on_layout(move |bounds: &ns::Bounds<ns::Points>| {
            if let Some(observation) = &observation {
                observation.layout(bounds);
            }
        })
    };
    root.child(ns::dynamic(move || {
        let glyph = state.get();
        #[cfg(feature = "nsh-reading-integration")]
        if let Some(observation) = &glyph.observation {
            observation.projection(&[glyph.color], glyph.size, "", None);
        }
        let Some(source) = nsh_source(glyph.kind) else {
            return ns::view().into_element();
        };
        #[cfg(feature = "nsh-reading-integration")]
        if let Some(observation) = &glyph.observation {
            observation.asset(&source);
        }
        ns::svg(nsh_core::element::ImageSrc::svg_markup(&source))
            .size(ns::px(glyph.size))
            .flex_shrink_0()
            .text_color(crate::nsh::color(glyph.color))
            .tint_color(crate::nsh::color(glyph.color))
            .into_element()
    }))
    .into_element()
}
