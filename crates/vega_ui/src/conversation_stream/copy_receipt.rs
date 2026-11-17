use std::sync::Arc;

use gpui_kit::{prelude::*, *};
use vega_theme::{Layout, ThemeColors};

use super::ConversationStream;
use crate::interaction::{Interaction, InteractionExt};

mod native;

#[derive(Clone)]
pub(super) enum CopySource {
    Code(Arc<str>),
}

impl CopySource {
    fn generation(&self) -> u64 {
        match self {
            Self::Code(_) => 0,
        }
    }

    fn same_source(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Code(left), Self::Code(right)) => Arc::ptr_eq(left, right),
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Code(code) => code.to_string(),
        }
    }
}

type CopyOwner = (WeakEntity<ConversationStream>, u64, Option<(u64, usize)>);

pub(super) struct CopyReceiptButton {
    id: SharedString,
    source: CopySource,
    label: &'static str,
    colors: ThemeColors,
    style: Option<Stateful<Interaction>>,
    owner: Option<CopyOwner>,
    body: Option<AnyElement>,
    route: native::Route,
}

impl CopyReceiptButton {
    pub(super) fn new(
        id: SharedString,
        source: CopySource,
        label: &'static str,
        colors: ThemeColors,
        style: impl FnOnce(Interaction) -> Stateful<Interaction>,
    ) -> Self {
        let transparent = Rgba::from(transparent_black());
        let bridge = div()
            .id(id.clone())
            .size(px(Layout::COPY_ACTION_SIZE))
            .flex_shrink_0()
            .interaction_surface(transparent, transparent, transparent, true);
        Self {
            id,
            source,
            label,
            colors,
            style: Some(style(bridge)),
            owner: None,
            body: None,
            route: native::Route::default(),
        }
    }
}

impl IntoElement for CopyReceiptButton {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for CopyReceiptButton {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone().into())
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
        if self.body.is_none() {
            let bridge = self.style.take().map(|bridge| {
                div()
                    .opacity(0.)
                    .child(self.route.bridge(bridge))
                    .into_any_element()
            });
            self.body = Some(
                div()
                    .relative()
                    .flex_shrink_0()
                    .child(div().absolute().inset_0().child(native::Native::new(
                        self.id.clone(),
                        self.source.clone(),
                        self.label,
                        self.colors,
                        self.owner.clone(),
                        self.route.clone(),
                        cx,
                    )))
                    .children(bridge)
                    .into_any_element(),
            );
        }
        match &mut self.body {
            Some(body) => (body.request_layout(window, cx), ()),
            None => (window.request_layout(Style::default(), [], cx), ()),
        }
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
        if let Some(body) = &mut self.body {
            body.prepaint(window, cx);
        }
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
        if let Some(body) = &mut self.body {
            body.paint(window, cx);
        }
    }
}
