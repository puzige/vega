#[cfg(test)]
mod tests;

use crate::nsh::ProjectionId;
use gpui_kit::{
    AnyElement, App, Bounds, Element, ElementId, EntityId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, WeakEntity, Window,
};
use std::{cell::RefCell, rc::Rc};

use super::super::ToolCard;

#[derive(Clone)]
pub(super) struct Authority {
    pub(super) id: ProjectionId,
    pub(super) width: f32,
    scale: f32,
}

impl Default for Authority {
    fn default() -> Self {
        Self {
            id: ProjectionId::new(),
            width: 0.,
            scale: 0.,
        }
    }
}

impl Authority {
    pub(super) fn matches(&self, other: &Self) -> bool {
        self.id.matches(&other.id) && self.width == other.width && self.scale == other.scale
    }

    pub(super) fn accepts_size(&self, size: [f32; 2]) -> bool {
        self.width.is_finite()
            && self.width > 0.
            && self.scale.is_finite()
            && self.scale > 0.
            && size.iter().all(|value| value.is_finite() && *value > 0.)
            && (self.width - size[0]).abs() <= 0.5
    }

    fn update(&mut self, width: f32, scale: f32) {
        if self.width != width || self.scale != scale {
            self.id = ProjectionId::new();
            self.width = width;
            self.scale = scale;
        }
    }
}

pub(super) struct Mounted {
    pub(super) child: AnyElement,
    pub(super) card: WeakEntity<ToolCard>,
    pub(super) entity_id: EntityId,
    pub(super) authority: Rc<RefCell<Authority>>,
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
        Some(("tool-card-layout", self.entity_id).into())
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
        self.authority
            .borrow_mut()
            .update(f32::from(bounds.size.width), window.scale_factor());
        let _ = self.card.update(cx, |card, cx| {
            if card.presentation.as_ref().is_some_and(|page| {
                page.mount
                    .upgrade()
                    .is_some_and(|mount| Rc::ptr_eq(&mount, &self.authority))
            }) {
                card.publish_nsh(cx);
            }
        });
        if let Some(id) = id {
            window.with_element_state::<Rc<RefCell<Authority>>, _>(id, |_, _| {
                ((), self.authority.clone())
            });
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
