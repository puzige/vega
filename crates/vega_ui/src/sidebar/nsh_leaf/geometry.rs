use super::*;

pub(in crate::sidebar) struct Overlay<E> {
    base: E,
    chrome: AnyElement,
}

pub(super) fn overlay<E>(base: E, chrome: AnyElement) -> Overlay<E> {
    Overlay {
        base,
        chrome: div()
            .relative()
            .size_full()
            .child(chrome)
            .into_any_element(),
    }
}

impl<E: gpui_kit::Styled> gpui_kit::Styled for Overlay<E> {
    fn style(&mut self) -> &mut gpui_kit::StyleRefinement {
        self.base.style()
    }
}

impl<E: gpui_kit::InteractiveElement> gpui_kit::InteractiveElement for Overlay<E> {
    fn interactivity(&mut self) -> &mut gpui_kit::Interactivity {
        self.base.interactivity()
    }
}

impl<E: Element + gpui_kit::Styled + gpui_kit::InteractiveElement>
    gpui_kit::StatefulInteractiveElement for Overlay<E>
{
}

impl<E: Element + gpui_kit::Styled + gpui_kit::ParentElement> gpui_kit::ParentElement
    for Overlay<E>
{
    fn extend(&mut self, children: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(children);
    }
}

impl<E: Element> IntoElement for Overlay<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for Overlay<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.base.id()
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.base.source_location()
    }
    fn a11y_role(&self) -> Option<Role> {
        self.base.a11y_role()
    }
    fn write_a11y_info(&self, node: &mut gpui_kit::accesskit::Node) {
        self.base.write_a11y_info(node);
    }
    fn a11y_synthetic_children(
        &mut self,
        state: &mut Self::PrepaintState,
        builder: &mut gpui_kit::A11ySubtreeBuilder,
    ) {
        self.base.a11y_synthetic_children(state, builder);
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.base.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let state = self
            .base
            .prepaint(id, inspector, bounds, layout, window, cx);
        self.chrome.layout_as_root(
            gpui_kit::size(
                gpui_kit::AvailableSpace::Definite(bounds.size.width),
                gpui_kit::AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        self.chrome.prepaint_at(bounds.origin, window, cx);
        state
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.base
            .paint(id, inspector, bounds, layout, state, window, cx);
        self.chrome.paint(window, cx);
    }
}

type Sampler = Box<dyn Fn(Bounds<Pixels>, &mut Window, &mut App)>;

pub(super) enum HitMask {
    Full,
    Decoration,
    Insets(f32, f32),
}

struct Sample {
    child: AnyElement,
    sampler: Sampler,
    mask: HitMask,
}

pub(super) fn sample(
    child: AnyElement,
    mask: HitMask,
    sampler: impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    Sample {
        child,
        sampler: Box::new(sampler),
        mask,
    }
    .into_any_element()
}

pub(super) fn sample_decoration(
    child: AnyElement,
    sampler: impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    Sample {
        child,
        sampler: Box::new(sampler),
        mask: HitMask::Decoration,
    }
    .into_any_element()
}

impl IntoElement for Sample {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Sample {
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
        (self.sampler)(bounds, window, cx);
        let mask = match self.mask {
            HitMask::Full => None,
            HitMask::Decoration => Some(Bounds::default()),
            HitMask::Insets(left, right) => {
                let mut inner = bounds;
                inner.origin.x += px(left);
                inner.size.width = (inner.size.width - px(left + right)).max(px(0.));
                Some(inner)
            }
        };
        window.with_content_mask(
            mask.map(|bounds| gpui_kit::ContentMask { bounds }),
            |window| self.child.prepaint(window, cx),
        );
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
