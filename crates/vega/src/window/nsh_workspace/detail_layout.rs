use super::*;

pub(super) fn detail_heights(body: f32, has_stream: bool) -> (f32, f32) {
    let body = if body.is_finite() { body.max(0.) } else { 0. };
    if has_stream {
        let chrome = (body * 0.5).min(220.);
        (chrome, body - chrome)
    } else {
        (body, 0.)
    }
}

pub(in crate::window) fn detail_body(chrome: AnyElement, stream: Option<AnyElement>) -> AnyElement {
    DetailBody {
        chrome: Some(chrome),
        stream,
    }
    .into_any_element()
}

struct DetailBody {
    chrome: Option<AnyElement>,
    stream: Option<AnyElement>,
}

impl IntoElement for DetailBody {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for DetailBody {
    type RequestLayoutState = ();
    type PrepaintState = AnyElement;

    fn id(&self) -> Option<ElementId> {
        Some("subagent-detail-body".into())
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
        let mut host = div().size_full().min_h_0().min_w_0().into_any_element();
        (host.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let (chrome_height, stream_height) =
            detail_heights(f32::from(bounds.size.height), self.stream.is_some());
        let mut body = div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .h(px(chrome_height))
                    .w_full()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .children(self.chrome.take()),
            )
            .children(self.stream.take().map(|stream| {
                div()
                    .h(px(stream_height))
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(stream)
            }))
            .into_any_element();
        body.layout_as_root(
            size(
                gpui_kit::AvailableSpace::Definite(bounds.size.width),
                gpui_kit::AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        body.prepaint_at(bounds.origin, window, cx);
        body
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        body: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        body.paint(window, cx);
    }
}
