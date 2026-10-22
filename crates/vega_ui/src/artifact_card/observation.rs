use gpui_kit::prelude::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId,
    Pixels, SharedString, TextLayout, TextStyle, Window, WindowId, px,
};
use std::cell::RefCell;

#[derive(Clone)]
pub struct PaintedText {
    pub text: String,
    pub scope: &'static str,
    pub style: Option<TextStyle>,
    pub bounds: Bounds<Pixels>,
    pub ancestors: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct PaintedItem {
    pub token: usize,
    pub kind: &'static str,
    pub owner: Option<String>,
    pub rows: Option<usize>,
    pub bounds: Bounds<Pixels>,
    pub clipped: Bounds<Pixels>,
    pub painted: bool,
}

#[derive(Default)]
struct Frame {
    window: Option<WindowId>,
    text: Vec<PaintedText>,
    items: Vec<PaintedItem>,
    ancestors: Vec<usize>,
}

thread_local! {
    static FRAME: RefCell<Frame> = RefCell::new(Frame::default());
}

pub struct ObservedText(SharedString, &'static str);

pub fn observe_text(text: impl Into<SharedString>) -> ObservedText {
    ObservedText(text.into(), "artifact")
}

pub fn observe_tooltip(text: impl Into<SharedString>) -> ObservedText {
    ObservedText(text.into(), "tooltip")
}

impl IntoElement for ObservedText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for ObservedText {
    type RequestLayoutState = (TextLayout, TextStyle);
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Element::id(&self.0)
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.0.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = window.text_style();
        let (id, layout) = self.0.request_layout(id, inspector, window, cx);
        (id, (layout, style))
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0
            .prepaint(id, inspector, bounds, &mut state.0, window, cx);
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0
            .paint(id, inspector, bounds, &mut state.0, prepaint, window, cx);
        record_text(&state.0, Some(state.1.clone()), self.1, bounds, window);
    }
}

pub fn record_shaped_text(layout: &TextLayout, bounds: Bounds<Pixels>, window: &Window) {
    record_text(layout, None, "assistant", bounds, window);
}

fn clip(bounds: Bounds<Pixels>, window: &Window) -> Bounds<Pixels> {
    bounds
        .intersect(&window.content_mask().bounds)
        .intersect(&Bounds::new(Default::default(), window.viewport_size()))
}

fn record_text(
    layout: &TextLayout,
    style: Option<TextStyle>,
    scope: &'static str,
    bounds: Bounds<Pixels>,
    window: &Window,
) {
    FRAME.with_borrow_mut(|frame| {
        if frame.window == Some(window.window_handle().window_id()) {
            frame.text.push(PaintedText {
                text: layout.text(),
                scope,
                style,
                bounds: clip(bounds, window),
                ancestors: frame.ancestors.clone(),
            });
        }
    });
}

struct ObservedItem {
    child: AnyElement,
    kind: &'static str,
    owner: Option<String>,
    rows: Option<usize>,
}

pub fn observe_stream_item(
    child: AnyElement,
    kind: &'static str,
    owner: Option<String>,
    rows: Option<usize>,
) -> AnyElement {
    ObservedItem {
        child,
        kind,
        owner,
        rows,
    }
    .into_any_element()
}

impl IntoElement for ObservedItem {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for ObservedItem {
    type RequestLayoutState = ();
    type PrepaintState = Option<usize>;

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
    ) -> Option<usize> {
        let _ = self.child.prepaint(window, cx);
        FRAME.with_borrow_mut(|frame| {
            if frame.window != Some(window.window_handle().window_id()) {
                return None;
            }
            let token = frame.items.len();
            frame.items.push(PaintedItem {
                token,
                kind: self.kind,
                owner: self.owner.clone(),
                rows: self.rows,
                bounds,
                clipped: clip(bounds, window),
                painted: false,
            });
            Some(token)
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        token: &mut Option<usize>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(token) = *token {
            FRAME.with_borrow_mut(|frame| {
                frame.items[token].painted = true;
                frame.ancestors.push(token);
            });
        }
        self.child.paint(window, cx);
        if token.is_some() {
            FRAME.with_borrow_mut(|frame| {
                frame.ancestors.pop();
            });
        }
    }
}

pub fn frame(window: &mut Window, cx: &mut App) {
    FRAME.with_borrow_mut(|frame| {
        *frame = Frame {
            window: Some(window.window_handle().window_id()),
            ..Frame::default()
        };
    });
    window.render_frame(cx);
}

pub fn painted() -> Vec<PaintedText> {
    FRAME.with_borrow(|frame| {
        frame
            .text
            .iter()
            .filter(|text| text.bounds.size.width > px(0.) && text.bounds.size.height > px(0.))
            .cloned()
            .collect()
    })
}

pub fn texts() -> Vec<String> {
    painted().into_iter().map(|text| text.text).collect()
}

pub fn items(kind: &str) -> Vec<PaintedItem> {
    FRAME.with_borrow(|frame| {
        frame
            .items
            .iter()
            .filter(|item| item.kind == kind && item.painted)
            .cloned()
            .collect()
    })
}

pub fn group(owner: &str, rows: usize) -> PaintedItem {
    let matching: Vec<_> = items("artifact-group")
        .into_iter()
        .filter(|item| item.owner.as_deref() == Some(owner))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "one rendered group owned by {owner}: {matching:?}"
    );
    let group = matching[0].clone();
    assert_eq!(group.rows, Some(rows));
    assert!(
        group.painted && group.clipped.size.width > px(0.) && group.clipped.size.height > px(0.)
    );
    group
}

pub fn group_painted(owner: &str, rows: usize) -> Vec<PaintedText> {
    let group = group(owner, rows);
    painted()
        .into_iter()
        .filter(|text| text.ancestors.contains(&group.token))
        .collect()
}

pub fn group_texts(owner: &str, rows: usize) -> Vec<String> {
    group_painted(owner, rows)
        .into_iter()
        .map(|text| text.text)
        .collect()
}

pub fn ordered(owner: &str, rows: usize) {
    let group = group(owner, rows);
    let assistant_text = format!("Answer for {owner}.");
    let text = painted()
        .into_iter()
        .find(|text| text.scope == "assistant" && text.text.contains(&assistant_text))
        .expect("real shaped assistant text in this frame");
    let assistant = items("assistant")
        .into_iter()
        .find(|item| text.ancestors.contains(&item.token))
        .expect("assistant text ancestry");
    assert!(assistant.painted && assistant.clipped.size.height > px(0.));
    assert!(
        assistant.bounds.bottom() <= group.bounds.top(),
        "group must follow its real assistant container: {assistant:?}, {group:?}"
    );
    let summary = items("summary")
        .into_iter()
        .find(|item| item.owner.as_deref() == Some(owner))
        .expect("actual Summary layout anchor");
    for segment in items("assistant").into_iter().filter(|item| {
        item.bounds.top() >= assistant.bounds.top() && item.bounds.top() < summary.bounds.top()
    }) {
        assert!(
            segment.bounds.bottom() <= group.bounds.top(),
            "group must follow the final assistant container: {segment:?}, {group:?}"
        );
    }
    assert!(
        group.bounds.bottom() <= summary.bounds.top(),
        "group must precede Summary layout anchor"
    );
}
