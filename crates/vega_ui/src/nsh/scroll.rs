#[cfg(feature = "nsh-reading-integration")]
use gpui_kit::prelude::*;
use gpui_kit::*;

#[cfg(feature = "nsh-reading-integration")]
pub(crate) fn scoped(scope: nsh_platform_gpui::ScrollScope, child: AnyElement) -> AnyElement {
    Scoped { scope, child }.into_any_element()
}

#[cfg(feature = "nsh-reading-integration")]
struct Scoped {
    scope: nsh_platform_gpui::ScrollScope,
    child: AnyElement,
}

#[cfg(feature = "nsh-reading-integration")]
impl IntoElement for Scoped {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

#[cfg(feature = "nsh-reading-integration")]
impl Element for Scoped {
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
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.scope
            .with(|| self.child.prepaint_at(bounds.origin, window, cx));
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
        self.scope.with(|| self.child.paint(window, cx));
    }
}

use nsh_platform_gpui::ScrollRemainder;

pub(crate) fn consume_list(
    list: &ListState,
    remaining: ScrollRemainder,
    window: &mut Window,
    _cx: &mut App,
) -> bool {
    let delta = remaining.delta.pixel_delta(px(20.)).y;
    let before = -list.scroll_px_offset_for_scrollbar().y;
    let maximum = list.max_offset_for_scrollbar().y.max(px(0.));
    if !f32::from(delta).is_finite()
        || !f32::from(before).is_finite()
        || !f32::from(maximum).is_finite()
        || list.viewport_bounds().is_empty()
    {
        return false;
    }
    let target = (before - delta).max(px(0.)).min(maximum);
    if target == before {
        return false;
    }
    list.scroll_by(target - before);
    window.refresh();
    true
}
