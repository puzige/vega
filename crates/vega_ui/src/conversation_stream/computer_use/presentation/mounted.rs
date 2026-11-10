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
    type PrepaintState = (Bounds<Pixels>, Bounds<Pixels>, Size<Pixels>);

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
    ) -> Self::PrepaintState {
        self.lease.geometry.set(None);
        let mask = window.content_mask().bounds;
        let viewport = window.viewport_size();
        if let Some(id) = id {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.lease.clone()));
        }
        self.child.prepaint(window, cx);
        (bounds, mask, viewport)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (bounds, mask, viewport) = *state;
        if id.is_some()
            && let Some(owner) = self.owner.upgrade()
        {
            let stream = owner.read(cx);
            let visible = bounds
                .intersect(&mask)
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
        self.child.paint(window, cx);
    }
}
