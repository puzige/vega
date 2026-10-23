use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui_kit::{
    AnyElement, App, Bounds, Element, ElementId, FocusHandle, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, SharedString, Window,
};

use crate::icons::Icon;

#[derive(Clone, Debug)]
pub(crate) struct Scope {
    pub(crate) owner: SharedString,
    pub(crate) expanded: bool,
    pub(crate) enabled: bool,
    pub(crate) focus: FocusHandle,
    opacity: Rc<Cell<f32>>,
}

#[derive(Clone, Debug)]
pub(crate) struct GlyphPaint {
    pub(crate) scope: Scope,
    pub(crate) asset: Icon,
    pub(crate) asset_path: SharedString,
    pub(crate) rotation: f32,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) opacity: f32,
}

thread_local! {
    static OWNER: RefCell<Option<Scope>> = const { RefCell::new(None) };
    static PAINTS: RefCell<Vec<GlyphPaint>> = const { RefCell::new(Vec::new()) };
}

pub(crate) struct RestoreScope(Option<Scope>);

impl Drop for RestoreScope {
    fn drop(&mut self) {
        OWNER.with(|owner| *owner.borrow_mut() = self.0.take());
    }
}

pub(crate) fn scope(
    owner: SharedString,
    expanded: bool,
    enabled: bool,
    focus: &FocusHandle,
) -> RestoreScope {
    RestoreScope(OWNER.with(|current| {
        current.replace(Some(Scope {
            owner,
            expanded,
            enabled,
            focus: focus.clone(),
            opacity: Rc::new(Cell::new(0.)),
        }))
    }))
}

pub(crate) fn take() -> Vec<GlyphPaint> {
    PAINTS.with(|paints| std::mem::take(&mut *paints.borrow_mut()))
}

pub(crate) fn observe_untransformed_icon(
    asset: Icon,
    asset_path: SharedString,
    inner: AnyElement,
) -> AnyElement {
    observe_configured_icon(asset, asset_path, 0., inner)
}

pub(crate) fn observe_configured_icon(
    asset: Icon,
    asset_path: SharedString,
    rotation: f32,
    inner: AnyElement,
) -> AnyElement {
    let scope = OWNER.with(|owner| owner.borrow().clone());
    match scope {
        Some(scope) if matches!(asset, Icon::ChevronRight | Icon::ChevronDown) => PaintedIcon {
            inner,
            scope,
            asset,
            asset_path,
            rotation,
        }
        .into_any_element(),
        _ => inner,
    }
}

pub(crate) fn observe_visibility(inner: gpui_kit::Div) -> impl IntoElement {
    Visibility {
        inner,
        scope: OWNER
            .with(|owner| owner.borrow().clone())
            .expect("disclosure scope"),
    }
}

struct Visibility {
    inner: gpui_kit::Div,
    scope: Scope,
}

impl IntoElement for Visibility {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Visibility {
    type RequestLayoutState = gpui_kit::DivFrameState;
    type PrepaintState = Option<gpui_kit::Hitbox>;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        Element::request_layout(&mut self.inner, id, inspector, window, cx)
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
        Element::prepaint(&mut self.inner, id, inspector, bounds, layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        use gpui_kit::InteractiveElement as _;
        let style = self
            .inner
            .interactivity()
            .compute_style(id, hitbox.as_ref(), window, cx);
        self.scope.opacity.set(style.opacity.unwrap_or(1.));
        Element::paint(
            &mut self.inner,
            id,
            inspector,
            bounds,
            layout,
            hitbox,
            window,
            cx,
        );
    }
}

struct PaintedIcon {
    inner: AnyElement,
    scope: Scope,
    asset: Icon,
    asset_path: SharedString,
    rotation: f32,
}

impl IntoElement for PaintedIcon {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PaintedIcon {
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
        (self.inner.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let bytes = cx
            .asset_source()
            .load(self.asset_path.as_ref())
            .expect("WIRING: actual Kit SVG asset failed to load")
            .expect("WIRING: actual Kit SVG asset is absent");
        cx.svg_renderer()
            .render_single_frame(&bytes, 1.)
            .expect("WIRING: actual Kit SVG must be renderable");
        self.inner.paint(window, cx);
        PAINTS.with(|paints| {
            paints.borrow_mut().push(GlyphPaint {
                scope: self.scope.clone(),
                asset: self.asset,
                asset_path: self.asset_path.clone(),
                rotation: self.rotation,
                bounds,
                opacity: self.scope.opacity.get(),
            });
        });
    }
}
