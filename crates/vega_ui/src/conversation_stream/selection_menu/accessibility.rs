use gpui_kit::*;

pub(super) fn disabled<E: Element>(child: E, disabled: bool) -> Disabled<E> {
    Disabled { child, disabled }
}

pub(super) struct Disabled<E> {
    child: E,
    disabled: bool,
}

impl<E: Element> IntoElement for Disabled<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for Disabled<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;
    fn id(&self) -> Option<ElementId> {
        self.child.id()
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.child.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.child
            .prepaint(id, inspector, bounds, state, window, cx)
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child
            .paint(id, inspector, bounds, state, prepaint, window, cx);
    }
    fn a11y_role(&self) -> Option<Role> {
        self.child.a11y_role()
    }
    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.child.write_a11y_info(node);
        if self.disabled {
            node.set_disabled();
        }
    }
    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        self.child.a11y_synthetic_children(prepaint, builder);
    }
}
