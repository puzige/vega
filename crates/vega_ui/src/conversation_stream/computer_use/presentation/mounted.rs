use super::{ConversationStream, Geometry, Mount};
use gpui_kit::{prelude::*, *};
use std::rc::Rc;

pub(super) struct Mounted {
    pub(super) child: AnyElement,
    pub(super) lease: Rc<Mount>,
    pub(super) owner: WeakEntity<ConversationStream>,
}

impl IntoElement for Mounted {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Mounted {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some("nsh-cua-notice-mounted".into())
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
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.lease.geometry.set(None);
        if let Some(id) = id {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.lease.clone()));
            if let Some(owner) = self.owner.upgrade() {
                let stream = owner.read(cx);
                let viewport = window.viewport_size();
                let visible = bounds
                    .intersect(&window.content_mask().bounds)
                    .intersect(&Bounds::new(point(px(0.), px(0.)), viewport));
                if !visible.is_empty() {
                    self.lease.geometry.set(Some(Geometry {
                        visible,
                        bounds,
                        viewport,
                        list: stream.list.viewport_bounds(),
                        foreground: stream.foreground_height,
                    }));
                }
            }
        }
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}
