use gpui_kit::{prelude::*, *};

const PROBE_ALPHA: f32 = 1. / 65_536.;

#[derive(Clone, Debug)]
pub struct PaintedText {
    pub entity: EntityId,
    pub text: SharedString,
    pub bounds: Bounds<Pixels>,
    pub clip: Bounds<Pixels>,
    pub color: Hsla,
    probe: Rgba,
}

impl PaintedText {
    pub fn inherited_opacity(&self, window: &Window) -> Option<f32> {
        let matches: Vec<_> = window
            .painted_quads()
            .into_iter()
            .filter_map(|quad| quad.background.as_solid().map(Rgba::from))
            .filter(|color| {
                (color.r - self.probe.r).abs() < 0.0001
                    && (color.g - self.probe.g).abs() < 0.0001
                    && (color.b - self.probe.b).abs() < 0.0001
                    && color.a <= PROBE_ALPHA * 1.01
            })
            .collect();
        match matches.as_slice() {
            [color] => Some(color.a / PROBE_ALPHA),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct PaintLog(pub Vec<PaintedText>);

impl Global for PaintLog {}

pub struct ObservedText<E> {
    inner: E,
    text: SharedString,
    entity: Option<EntityId>,
}

pub fn current_text(value: impl Into<SharedString>) -> ObservedText<SharedString> {
    let text = value.into();
    ObservedText {
        inner: text.clone(),
        text,
        entity: None,
    }
}

pub fn text(value: impl Into<SharedString>, entity: EntityId) -> ObservedText<SharedString> {
    let text = value.into();
    ObservedText {
        inner: text.clone(),
        text,
        entity: Some(entity),
    }
}

pub fn current_element<E: IntoElement>(
    inner: E,
    text: impl Into<SharedString>,
) -> ObservedText<E::Element> {
    ObservedText {
        inner: inner.into_element(),
        text: text.into(),
        entity: None,
    }
}

impl<E: Element> IntoElement for ObservedText<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for ObservedText<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn a11y_role(&self) -> Option<Role> {
        self.inner.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.inner.write_a11y_info(node);
    }

    fn a11y_synthetic_children(
        &mut self,
        state: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        self.inner.a11y_synthetic_children(state, builder);
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
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
        self.inner
            .prepaint(id, inspector, bounds, layout, window, cx)
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
        self.inner
            .paint(id, inspector, bounds, layout, state, window, cx);
        record_text(
            self.entity.unwrap_or_else(|| window.current_view()),
            self.text.clone(),
            bounds,
            window,
            cx,
        );
    }
}

pub fn record_text(
    entity: EntityId,
    text: SharedString,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let clip = window.content_mask().bounds;
    if !bounds.intersects(&clip) || !cx.has_global::<PaintLog>() {
        return;
    }
    let index = cx.global::<PaintLog>().0.len() as u32;
    let probe = rgb(0x570000 | (index & 0xffff)).opacity(PROBE_ALPHA);
    let visible = bounds.intersect(&clip);
    window.paint_quad(fill(
        Bounds::new(visible.origin, size(px(1.), px(1.))),
        probe,
    ));
    cx.global_mut::<PaintLog>().0.push(PaintedText {
        entity,
        text,
        bounds,
        clip,
        color: window.text_style().color,
        probe,
    });
}
