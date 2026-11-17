use super::*;
use crate::nsh::Surface;
use ns::{Accessible as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;

#[derive(Clone, PartialEq)]
struct Snapshot {
    rectangles: Vec<[f32; 4]>,
    icon: Option<String>,
    color: Rgba,
}

struct Host {
    surface: Surface<Snapshot, ()>,
    snapshot: Snapshot,
    _release: Subscription,
}

pub(super) fn prepare(
    key: String,
    rectangles: Vec<Bounds<Pixels>>,
    icon: Option<LinkSiteKind>,
    color: Rgba,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let clip = window.content_mask().bounds;
    let rectangles = rectangles
        .into_iter()
        .map(|bounds| bounds.intersect(&clip))
        .filter(|bounds| !bounds.is_empty())
        .collect::<Vec<_>>();
    let first = rectangles.first().copied()?;
    let extent = rectangles.iter().skip(1).fold(first, |extent, bounds| {
        let left = extent.left().min(bounds.left());
        let top = extent.top().min(bounds.top());
        Bounds::new(
            point(left, top),
            size(
                extent.right().max(bounds.right()) - left,
                extent.bottom().max(bounds.bottom()) - top,
            ),
        )
    });
    let snapshot = Snapshot {
        rectangles: rectangles
            .iter()
            .map(|bounds| {
                [
                    f32::from(bounds.left() - extent.left()),
                    f32::from(bounds.top() - extent.top()),
                    bounds.size.width.into(),
                    bounds.size.height.into(),
                ]
            })
            .collect(),
        icon: icon.map(super::native::icon_source),
        color,
    };
    let host = window.use_keyed_state(SharedString::from(key), cx, |window, cx| {
        let (surface, _) = Surface::new_transparent(snapshot.clone(), pixels, window, cx);
        surface.focus_handle(cx).tab_stop(false);
        Host {
            surface,
            snapshot: snapshot.clone(),
            _release: cx.on_release(|host: &mut Host, cx| host.surface.entity().read(cx).stop()),
        }
    });
    let view = host.update(cx, |host, cx| {
        host.surface.sync_environment(cx);
        if host.snapshot != snapshot {
            host.surface.publish(snapshot.clone());
            host.snapshot = snapshot;
        }
        host.surface.entity()
    });
    let mut child = Passive {
        child: div()
            .w(extent.size.width)
            .h(extent.size.height)
            .child(view)
            .into_any_element(),
    }
    .into_any_element();
    child.prepaint_as_root(
        extent.origin,
        size(
            AvailableSpace::Definite(extent.size.width),
            AvailableSpace::Definite(extent.size.height),
        ),
        window,
        cx,
    );
    Some(child)
}

fn pixels(state: ns::Signal<Snapshot>, _: tokio::sync::mpsc::Sender<()>) -> ns::AnyElement {
    ns::view()
        .size_full()
        .relative()
        .accessibility_elements_hidden(true)
        .child(ns::dynamic(move || {
            let snapshot = state.get();
            snapshot
                .rectangles
                .iter()
                .map(|bounds| {
                    let view = ns::view()
                        .absolute()
                        .left(ns::px(bounds[0]))
                        .top(ns::px(bounds[1]))
                        .w(ns::px(bounds[2]))
                        .h(ns::px(bounds[3]));
                    if let Some(icon) = &snapshot.icon {
                        view.child(
                            ns::svg(nsh_core::element::ImageSrc::svg_markup(icon))
                                .size_full()
                                .text_color(crate::nsh::color(snapshot.color)),
                        )
                        .into_element()
                    } else {
                        view.bg(crate::nsh::color(snapshot.color)).into_element()
                    }
                })
                .collect::<Vec<_>>()
        }))
        .into_element()
}

struct Passive {
    child: AnyElement,
}

impl IntoElement for Passive {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Passive {
    type RequestLayoutState = ();
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
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let mask = ContentMask {
            bounds: Bounds {
                origin: window.content_mask().bounds.center(),
                size: Default::default(),
            },
        };
        window.with_content_mask(Some(mask), |window| self.child.prepaint(window, cx));
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let bounds = bounds.intersect(&window.content_mask().bounds);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.child.paint(window, cx)
        });
    }
}

pub(super) fn focus_rectangles(bounds: Bounds<Pixels>, scale: f32) -> Vec<Bounds<Pixels>> {
    let thickness = px(1. / scale);
    vec![
        Bounds::new(bounds.origin, size(bounds.size.width, thickness)),
        Bounds::new(
            point(bounds.left(), bounds.bottom() - thickness),
            size(bounds.size.width, thickness),
        ),
        Bounds::new(bounds.origin, size(thickness, bounds.size.height)),
        Bounds::new(
            point(bounds.right() - thickness, bounds.top()),
            size(thickness, bounds.size.height),
        ),
    ]
}
