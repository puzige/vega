use gpui_kit::{
    App, Bounds, ContentMask, Div, Element, ElementId, GlobalElementId, InspectorElementId,
    InteractiveElement, Interactivity, IntoElement, LayoutId, ParentElement, Pixels, Point,
    Refineable, Rgba, ScrollHandle, Stateful, StatefulInteractiveElement, Style, StyleRefinement,
    Styled, Window, base::ScrollbarHandle, div, fill, linear_color_stop, linear_gradient, point,
    px, size,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use vega_theme::Layout;

#[derive(Clone, Default)]
pub(crate) struct ScrollFadeTopGuards(Rc<RefCell<Vec<Rc<Cell<bool>>>>>);

impl ScrollFadeTopGuards {
    pub(crate) fn observe(
        &self,
        element: gpui_kit::AnyElement,
        pinned: Rc<Cell<bool>>,
    ) -> gpui_kit::AnyElement {
        ScrollEdgeFade {
            base: element,
            handle: None,
            prepare: None,
            top_guards: None,
            observed_pinned: Some((self.clone(), pinned)),
            chrome: None,
            color: Rgba::default(),
            style: ScrollFade::compact(),
        }
        .into_any_element()
    }

    fn record(&self, pinned: Rc<Cell<bool>>) {
        self.0.borrow_mut().push(pinned);
    }

    fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    fn pinned(&self) -> bool {
        self.0.borrow().iter().any(|pinned| pinned.get())
    }
}

#[derive(Clone, Copy)]
pub struct ScrollFade {
    horizontal: f32,
    vertical: f32,
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
    vertical_gutter: Pixels,
    horizontal_gutter: Pixels,
    radius: Pixels,
    inset: Pixels,
    intrinsic_vertical: bool,
    parent_rounding: Option<(gpui_kit::AbsoluteLength, gpui_kit::AbsoluteLength)>,
    bottom_extension: Pixels,
}

impl ScrollFade {
    pub fn compact() -> Self {
        Self {
            horizontal: Layout::SCROLL_EDGE_FADE_COMPACT,
            vertical: Layout::SCROLL_EDGE_FADE_COMPACT,
            top: true,
            bottom: true,
            left: true,
            right: true,
            vertical_gutter: px(0.),
            horizontal_gutter: px(0.),
            radius: px(0.),
            inset: px(0.),
            intrinsic_vertical: false,
            parent_rounding: None,
            bottom_extension: px(0.),
        }
    }

    pub fn reading() -> Self {
        Self {
            vertical: Layout::SCROLL_EDGE_FADE_READING,
            ..Self::compact()
        }
    }

    pub fn top_only(self) -> Self {
        Self {
            bottom: false,
            left: false,
            right: false,
            ..self
        }
    }

    pub fn vertical_gutter(self, gutter: Pixels) -> Self {
        Self {
            vertical_gutter: gutter,
            ..self
        }
    }

    pub fn rounded_parent(mut self, parent: &StyleRefinement) -> Self {
        let mut style = Style::default();
        style.refine(parent);
        self.parent_rounding = Some((style.corner_radii.top_left, style.border_widths.top));
        self
    }

    pub fn bottom_extension(self, extension: Pixels) -> Self {
        Self {
            bottom_extension: extension,
            ..self
        }
    }

    pub fn vertical_list(self) -> Self {
        Self {
            intrinsic_vertical: true,
            left: false,
            right: false,
            ..self
        }
    }

    pub fn enabled(self, enabled: bool) -> Self {
        Self {
            top: self.top && enabled,
            bottom: self.bottom && enabled,
            left: self.left && enabled,
            right: self.right && enabled,
            ..self
        }
    }
}

#[derive(Clone, Copy, Default)]
struct FadeEdge {
    length: Pixels,
    strength: f32,
}

impl FadeEdge {
    fn new(hidden: Pixels, viewport: Pixels, token: f32) -> Self {
        let length = px(token).min(viewport / 4.).max(px(0.));
        let strength = if length > px(0.) {
            ((f32::from(hidden) - Layout::SCROLL_EDGE_FADE_EPSILON) / f32::from(length))
                .clamp(0., 1.)
        } else {
            0.
        };
        Self { length, strength }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScrollFadeMetrics {
    bounds: Bounds<Pixels>,
    pub(crate) start: Point<Pixels>,
    pub(crate) end: Point<Pixels>,
}

struct FadeRegion {
    bounds: Bounds<Pixels>,
    strength: f32,
    angle: f32,
}

impl ScrollFadeMetrics {
    pub(crate) fn new(bounds: Bounds<Pixels>, start: Point<Pixels>, end: Point<Pixels>) -> Self {
        Self { bounds, start, end }
    }

    fn from_handle(handle: &dyn ScrollbarHandle) -> Self {
        let bounds = handle.viewport_bounds();
        let content = handle.content_size();
        let offset = handle.offset();
        let max_x = (content.width - bounds.size.width).max(px(0.));
        let max_y = (content.height - bounds.size.height).max(px(0.));
        let x = (-offset.x).clamp(px(0.), max_x);
        let y = (-offset.y).clamp(px(0.), max_y);
        Self::new(bounds, point(x, y), point(max_x - x, max_y - y))
    }

    fn regions(self, style: ScrollFade) -> Vec<FadeRegion> {
        let mut bounds = self.bounds;
        bounds.origin += point(style.inset, style.inset);
        bounds.size.width = (bounds.size.width - style.inset * 2.).max(px(0.));
        bounds.size.height = (bounds.size.height - style.inset * 2.).max(px(0.));
        bounds.size.width = (bounds.size.width - style.vertical_gutter).max(px(0.));
        bounds.size.height = (bounds.size.height - style.horizontal_gutter).max(px(0.));
        if bounds.is_empty() {
            return Vec::new();
        }
        let top = FadeEdge::new(self.start.y, bounds.size.height, style.vertical);
        let bottom = FadeEdge::new(self.end.y, bounds.size.height, style.vertical);
        let left = FadeEdge::new(self.start.x, bounds.size.width, style.horizontal);
        let right = FadeEdge::new(self.end.x, bounds.size.width, style.horizontal);
        let mut regions = Vec::with_capacity(4);
        let top_length = if style.top && top.strength > 0. {
            regions.push(FadeRegion {
                bounds: Bounds::new(bounds.origin, size(bounds.size.width, top.length)),
                strength: top.strength,
                angle: 180.,
            });
            top.length
        } else {
            px(0.)
        };
        let bottom_length = if style.bottom && bottom.strength > 0. {
            regions.push(FadeRegion {
                bounds: Bounds::new(
                    point(bounds.left(), bounds.bottom() - bottom.length),
                    size(bounds.size.width, bottom.length),
                ),
                strength: bottom.strength,
                angle: 0.,
            });
            bottom.length
        } else {
            px(0.)
        };
        let height = bounds.size.height - top_length - bottom_length;
        for (enabled, edge, x, angle) in [
            (style.left, left, bounds.left(), 90.),
            (style.right, right, bounds.right() - right.length, 270.),
        ] {
            if enabled && edge.strength > 0. {
                regions.push(FadeRegion {
                    bounds: Bounds::new(
                        point(x, bounds.top() + top_length),
                        size(edge.length, height),
                    ),
                    strength: edge.strength,
                    angle,
                });
            }
        }
        regions
    }

    pub(crate) fn paint(self, color: Rgba, style: ScrollFade, window: &mut Window) {
        let mut outer = self.bounds;
        outer.origin += point(style.inset, style.inset);
        outer.size.width =
            (outer.size.width - style.inset * 2. - style.vertical_gutter).max(px(0.));
        outer.size.height =
            (outer.size.height - style.inset * 2. - style.horizontal_gutter).max(px(0.));
        outer.size.height += style.bottom_extension;
        for region in self.regions(style) {
            let start = if region.angle == 0. {
                f32::from(style.bottom_extension) / f32::from(outer.size.height)
            } else {
                0.
            };
            let stop = start
                + if region.angle == 0. || region.angle == 180. {
                    f32::from(region.bounds.size.height) / f32::from(outer.size.height)
                } else {
                    f32::from(region.bounds.size.width) / f32::from(outer.size.width)
                };
            let mut quad = fill(
                outer,
                linear_gradient(
                    region.angle,
                    linear_color_stop(color.opacity(region.strength), start),
                    linear_color_stop(color.opacity(0.), stop),
                ),
            );
            quad.corner_radii =
                gpui_kit::Corners::all(style.radius).clamp_radii_for_quad_size(outer.size);
            if style.vertical_gutter > px(0.) {
                quad.corner_radii.top_right = px(0.);
                quad.corner_radii.bottom_right = px(0.);
            }
            if style.horizontal_gutter > px(0.) {
                quad.corner_radii.bottom_left = px(0.);
                quad.corner_radii.bottom_right = px(0.);
            }
            window.with_content_mask(
                Some(ContentMask {
                    bounds: region.bounds,
                }),
                |window| {
                    window.paint_quad(quad);
                },
            );
        }
    }

    pub(crate) fn protect(mut self, protected: Bounds<Pixels>, style: ScrollFade) -> Self {
        let vertical = FadeEdge::new(px(0.), self.bounds.size.height, style.vertical).length;
        let horizontal = FadeEdge::new(px(0.), self.bounds.size.width, style.horizontal).length;
        if protected.right() > self.bounds.left() && protected.left() < self.bounds.right() {
            if protected.top() < self.bounds.top() + vertical
                && protected.bottom() > self.bounds.top()
            {
                self.start.y = px(0.);
            }
            if protected.bottom() > self.bounds.bottom() - vertical
                && protected.top() < self.bounds.bottom()
            {
                self.end.y = px(0.);
            }
        }
        if protected.bottom() > self.bounds.top() && protected.top() < self.bounds.bottom() {
            if protected.left() < self.bounds.left() + horizontal {
                self.start.x = px(0.);
            }
            if protected.right() > self.bounds.right() - horizontal {
                self.end.x = px(0.);
            }
        }
        self
    }
}

pub trait ScrollEdgeFadeExt: IntoElement + Sized
where
    Self::Element: Styled,
{
    fn tracked_scroll_edge_fade(
        self,
        handle: impl ScrollbarHandle,
        color: Rgba,
        style: ScrollFade,
    ) -> ScrollEdgeFade<Self::Element> {
        ScrollEdgeFade {
            base: self.into_element(),
            handle: Some(Box::new(handle)),
            prepare: None,
            top_guards: None,
            observed_pinned: None,
            chrome: Some(read_chrome::<Self::Element>),
            color,
            style,
        }
    }
}

impl<T: IntoElement> ScrollEdgeFadeExt for T where T::Element: Styled {}

pub trait ScrollableFadeExt {
    fn scroll_edge_fade(self, color: Rgba, style: ScrollFade) -> ScrollEdgeFade<Stateful<Div>>;
}

impl ScrollableFadeExt for Stateful<Div> {
    fn scroll_edge_fade(self, color: Rgba, style: ScrollFade) -> ScrollEdgeFade<Stateful<Div>> {
        ScrollEdgeFade {
            base: self,
            handle: None,
            prepare: Some(|base, handle| {
                let original = std::mem::replace(base, div().id("scroll-edge-fade-placeholder"));
                *base = original.track_scroll(handle);
            }),
            top_guards: None,
            observed_pinned: None,
            chrome: Some(read_chrome::<Stateful<Div>>),
            color,
            style,
        }
    }
}

pub struct ScrollEdgeFade<E> {
    base: E,
    handle: Option<Box<dyn ScrollbarHandle>>,
    prepare: Option<fn(&mut E, &ScrollHandle)>,
    top_guards: Option<ScrollFadeTopGuards>,
    observed_pinned: Option<(ScrollFadeTopGuards, Rc<Cell<bool>>)>,
    chrome: Option<fn(&mut E, &Window, &mut ScrollFade)>,
    color: Rgba,
    style: ScrollFade,
}

impl<E> ScrollEdgeFade<E> {
    pub(crate) fn preserve_pinned_headers(mut self, guards: ScrollFadeTopGuards) -> Self {
        self.top_guards = Some(guards);
        self
    }
}

impl<E: Styled> Styled for ScrollEdgeFade<E> {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl<E: Element + Styled + ParentElement> ParentElement for ScrollEdgeFade<E> {
    fn extend(&mut self, children: impl IntoIterator<Item = gpui_kit::AnyElement>) {
        self.base.extend(children);
    }
}

impl<E: InteractiveElement> InteractiveElement for ScrollEdgeFade<E> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl<E: Element + Styled + InteractiveElement> StatefulInteractiveElement for ScrollEdgeFade<E> {}

impl<E: Element> IntoElement for ScrollEdgeFade<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for ScrollEdgeFade<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = (E::PrepaintState, Bounds<Pixels>, ScrollFade);

    fn id(&self) -> Option<ElementId> {
        self.base.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.base.source_location()
    }

    fn a11y_role(&self) -> Option<gpui_kit::Role> {
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
        self.base.a11y_synthetic_children(&mut state.0, builder);
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
        if let Some(prepare) = self.prepare {
            let state =
                window.use_keyed_state("scroll-edge-fade-handle", cx, |_, _| ScrollHandle::new());
            let handle = state.read(cx).clone();
            prepare(&mut self.base, &handle);
            self.handle = Some(Box::new(handle));
        }
        if let Some(guards) = &self.top_guards {
            guards.clear();
        }
        let mask = window.content_mask().bounds.intersect(&bounds);
        let mut style = self.style;
        if let Some(chrome) = self.chrome {
            chrome(&mut self.base, window, &mut style);
        }
        let state = self
            .base
            .prepaint(id, inspector, bounds, layout, window, cx);
        (state, mask, style)
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
            .paint(id, inspector, bounds, layout, &mut state.0, window, cx);
        if let Some((guards, pinned)) = &self.observed_pinned
            && !state.1.is_empty()
            && bounds.size.height > px(0.)
        {
            guards.record(pinned.clone());
        }
        if let Some(handle) = self.handle.as_deref() {
            let metrics = ScrollFadeMetrics::from_handle(handle);
            let mut style = state.2;
            if self
                .top_guards
                .as_ref()
                .is_some_and(ScrollFadeTopGuards::pinned)
            {
                style.top = false;
            }
            window.with_content_mask(Some(ContentMask { bounds: state.1 }), |window| {
                metrics.paint(self.color, style, window);
            });
        }
    }
}

fn read_chrome<E: Styled>(base: &mut E, window: &Window, style: &mut ScrollFade) {
    let mut element_style = Style::default();
    element_style.refine(base.style());
    let horizontal =
        !style.intrinsic_vertical && element_style.overflow.x == gpui_kit::Overflow::Scroll;
    let vertical =
        style.intrinsic_vertical || element_style.overflow.y == gpui_kit::Overflow::Scroll;
    style.left &= horizontal;
    style.right &= horizontal;
    style.top &= vertical;
    style.bottom &= vertical;
    let widths = element_style.border_widths.to_pixels(window.rem_size());
    let inset = widths
        .top
        .max(widths.bottom)
        .max(widths.left)
        .max(widths.right);
    let radii = element_style.corner_radii.to_pixels(window.rem_size());
    let radius = radii
        .top_left
        .max(radii.top_right)
        .max(radii.bottom_left)
        .max(radii.bottom_right);
    style.inset = inset;
    let inherited = style.parent_rounding.map_or(px(0.), |(radius, border)| {
        (radius.to_pixels(window.rem_size()) - border.to_pixels(window.rem_size())).max(px(0.))
    });
    style.radius = (radius.max(inherited).max(style.radius) - inset).max(px(0.));
}

#[cfg(test)]
mod tests;
