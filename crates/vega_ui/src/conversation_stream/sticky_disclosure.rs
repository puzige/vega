use std::{cell::Cell, rc::Rc};

use gpui_kit::{
    AnyElement, App, Bounds, ContentMask, Element, ElementId, FlexDirection, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window, point, px,
};

#[derive(Clone, Default)]
pub(crate) struct StickyState(Rc<Cell<bool>>);

impl StickyState {
    pub(crate) fn pinned(&self) -> bool {
        self.0.get()
    }
}

pub(crate) struct StickySection {
    element: AnyElement,
    region: Option<bool>,
}

impl StickySection {
    pub(crate) fn into_element(self) -> AnyElement {
        self.element
    }
    pub(crate) fn plain(element: AnyElement) -> Self {
        Self {
            element,
            region: None,
        }
    }

    pub(crate) fn content(element: AnyElement, reaches_header: bool) -> Self {
        Self {
            element,
            region: Some(reaches_header),
        }
    }
}

pub(crate) struct StickyDisclosure {
    header: Option<AnyElement>,
    sections: Vec<StickySection>,
    state: StickyState,
    enabled: bool,
    mask: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl StickyDisclosure {
    pub(crate) fn new(
        header: AnyElement,
        sections: Vec<StickySection>,
        state: StickyState,
        enabled: bool,
    ) -> Self {
        let mask = Rc::new(Cell::new(None));
        Self {
            header: Some(
                ClippedHeader {
                    child: header,
                    mask: mask.clone(),
                }
                .into_any_element(),
            ),
            sections,
            state,
            enabled,
            mask,
        }
    }
}

impl IntoElement for StickyDisclosure {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for StickyDisclosure {
    type RequestLayoutState = (Option<LayoutId>, Vec<LayoutId>);
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
        let header = self
            .header
            .as_mut()
            .map(|header| header.request_layout(window, cx));
        let sections = self
            .sections
            .iter_mut()
            .map(|section| section.element.request_layout(window, cx))
            .collect::<Vec<_>>();
        let style = Style {
            display: gpui_kit::Display::Flex,
            flex_direction: FlexDirection::Column,
            ..Style::default()
        };
        let layout = window.request_layout(
            style,
            header.into_iter().chain(sections.iter().copied()),
            cx,
        );
        (layout, (header, sections))
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let viewport = window.content_mask().bounds;
        self.state.0.set(false);
        let placement = layout.0.map(|header| {
            let natural = window.layout_bounds(header);
            let region = self
                .enabled
                .then(|| {
                    self.sections
                        .iter()
                        .zip(&layout.1)
                        .find_map(|(section, layout)| {
                            let reaches_header = section.region?;
                            let mut region = window.layout_bounds(*layout);
                            if reaches_header {
                                let bottom = region.bottom();
                                region.origin.y = natural.top();
                                region.size.height = bottom - natural.top();
                            }
                            (natural.top() < viewport.top()
                                && region.top() <= viewport.top()
                                && region.bottom() > viewport.top())
                            .then_some(region)
                        })
                })
                .flatten();
            if let Some(region) = region {
                let top = viewport.top().min(region.bottom() - natural.size.height);
                self.state.0.set(top > natural.top());
                (top - natural.top(), viewport.intersect(&region))
            } else {
                (px(0.), viewport)
            }
        });
        for section in &mut self.sections {
            section.element.prepaint(window, cx);
        }
        if let Some((offset, mask)) = placement
            && let Some(header) = self.header.as_mut()
        {
            self.mask.set(Some(mask));
            window.with_element_offset(point(px(0.), offset), |window| {
                header.prepaint(window, cx);
            });
        }
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
        for section in &mut self.sections {
            section.element.paint(window, cx);
        }
        if let Some(header) = self.header.as_mut() {
            header.paint(window, cx);
        }
    }
}

struct ClippedHeader {
    child: AnyElement,
    mask: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl IntoElement for ClippedHeader {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for ClippedHeader {
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
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let mask = self.mask.get().map(|bounds| ContentMask { bounds });
        window.with_content_mask(mask, |window| {
            self.child.prepaint(window, cx);
        });
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
        let mask = self.mask.get().map(|bounds| ContentMask { bounds });
        window.with_content_mask(mask, |window| self.child.paint(window, cx));
    }
}
