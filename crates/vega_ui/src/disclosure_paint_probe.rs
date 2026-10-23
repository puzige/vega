use std::cell::RefCell;

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
        }))
    }))
}

pub(crate) fn current_scope() -> Option<Scope> {
    OWNER.with(|owner| owner.borrow().clone())
}

pub(crate) fn take() -> Vec<GlyphPaint> {
    PAINTS.with(|paints| std::mem::take(&mut *paints.borrow_mut()))
}

pub(crate) fn record_paint(paint: GlyphPaint, cx: &App) {
    let bytes = cx
        .asset_source()
        .load(paint.asset_path.as_ref())
        .expect("WIRING: actual Kit SVG asset failed to load")
        .expect("WIRING: actual Kit SVG asset is absent");
    cx.svg_renderer()
        .render_single_frame(&bytes, 1.)
        .expect("WIRING: actual Kit SVG must be renderable");
    PAINTS.with(|paints| paints.borrow_mut().push(paint));
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
        self.inner.paint(window, cx);
        record_paint(
            GlyphPaint {
                scope: self.scope.clone(),
                asset: self.asset,
                asset_path: self.asset_path.clone(),
                rotation: self.rotation,
                bounds,
                opacity: 1.,
            },
            cx,
        );
    }
}
