use std::{cell::RefCell, sync::Arc};

use gpui_kit::base::{TextSelectionRun, TextSelectionScopeId};
use gpui_kit::{
    AnyElement, App, Bounds, Element, ElementId, EntityId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window,
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Observation {
    Invalidate(EntityId, usize),
    Remeasure(EntityId, Option<usize>),
    Layout(EntityId, usize),
    Bounds(EntityId, usize, Bounds<Pixels>),
}

#[derive(Clone)]
pub(crate) struct SelectionPaint {
    pub(crate) copy_id: u64,
    pub(crate) handle_id: EntityId,
    pub(crate) scope: TextSelectionScopeId,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) content_mask: Bounds<Pixels>,
    pub(crate) content: Arc<str>,
    pub(crate) runs: Vec<TextSelectionRun>,
}

thread_local! {
    static OBSERVATIONS: RefCell<Vec<Observation>> = const { RefCell::new(Vec::new()) };
    static SELECTION_PAINTS: RefCell<Vec<SelectionPaint>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn record_selection_paint(paint: SelectionPaint) {
    SELECTION_PAINTS.with(|paints| paints.borrow_mut().push(paint));
}

pub(crate) fn take_selection_paints() -> Vec<SelectionPaint> {
    SELECTION_PAINTS.with(|paints| std::mem::take(&mut *paints.borrow_mut()))
}

pub(crate) fn record(observation: Observation) {
    OBSERVATIONS.with(|events| events.borrow_mut().push(observation));
}

pub(crate) fn take() -> Vec<Observation> {
    OBSERVATIONS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

pub(crate) fn observe(owner: EntityId, index: usize, inner: AnyElement) -> AnyElement {
    Item {
        owner,
        index,
        inner,
    }
    .into_any_element()
}

struct Item {
    owner: EntityId,
    index: usize,
    inner: AnyElement,
}

impl IntoElement for Item {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Item {
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
    ) -> (LayoutId, Self::RequestLayoutState) {
        record(Observation::Layout(self.owner, self.index));
        (self.inner.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        record(Observation::Bounds(self.owner, self.index, bounds));
        self.inner.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.paint(window, cx);
    }
}
