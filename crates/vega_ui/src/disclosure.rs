use std::{f32::consts::FRAC_PI_2, time::Duration};

use gpui_kit::{
    AnyElement, App, Bounds, Element, ElementId, FocusHandle, GlobalElementId, Hitbox,
    InspectorElementId, LayoutId, Pixels, Rgba, SharedString, Svg, Transformation, Window,
    component::{IconName, IconNamed},
    div,
    prelude::*,
    px, svg,
};
use vega_theme::{Layout, Motion, ThemeColors};

pub(crate) fn focus(id: SharedString, window: &mut Window, cx: &mut App) -> FocusHandle {
    window
        .use_keyed_state(id, cx, |_, cx| cx.focus_handle().tab_stop(true))
        .read(cx)
        .clone()
}

pub(crate) fn chevron(
    group: SharedString,
    focus: &FocusHandle,
    expanded: bool,
    enabled: bool,
    colors: ThemeColors,
    window: &Window,
) -> AnyElement {
    div()
        .w(px(Layout::DISCLOSURE_TRAILING_SLOT))
        .h(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(Chevron::new(
            group,
            focus,
            expanded,
            enabled,
            colors.text_tertiary,
            Layout::ICON_SIZE,
            window,
        ))
        .into_any_element()
}

pub(crate) fn inline_chevron(
    group: SharedString,
    focus: &FocusHandle,
    expanded: bool,
    enabled: bool,
    colors: ThemeColors,
    window: &Window,
) -> AnyElement {
    div()
        .size(px(Layout::CONVERSATION_CHEVRON_SIZE))
        .flex_shrink_0()
        .child(Chevron::new(
            group,
            focus,
            expanded,
            enabled,
            colors.conversation_tool,
            Layout::CONVERSATION_CHEVRON_SIZE,
            window,
        ))
        .into_any_element()
}

struct Chevron {
    inner: Svg,
    channel: SharedString,
    target: f32,
    enabled: bool,
}

impl Chevron {
    fn new(
        group: SharedString,
        focus: &FocusHandle,
        expanded: bool,
        enabled: bool,
        color: Rgba,
        size: f32,
        window: &Window,
    ) -> Self {
        Self {
            inner: svg()
                .path(IconName::ChevronRight.path())
                .size(px(size))
                .flex_shrink_0()
                .text_color(color)
                .opacity(
                    if enabled
                        && (expanded
                            || (focus.is_focused(window) && window.last_input_was_keyboard()))
                    {
                        1.
                    } else {
                        0.
                    },
                )
                .when(enabled, |glyph| {
                    glyph.group_hover(group.clone(), |style| style.opacity(1.))
                }),
            channel: format!("{group}-chevron-angle").into(),
            target: if expanded { FRAC_PI_2 } else { 0. },
            enabled,
        }
    }
}

impl IntoElement for Chevron {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Chevron {
    type RequestLayoutState = <Svg as Element>::RequestLayoutState;
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        Element::id(&self.inner)
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn a11y_role(&self) -> Option<gpui_kit::Role> {
        self.inner.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut gpui_kit::accesskit::Node) {
        self.inner.write_a11y_info(node);
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut gpui_kit::A11ySubtreeBuilder,
    ) {
        self.inner.a11y_synthetic_children(prepaint, builder);
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
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let style = self
            .inner
            .interactivity()
            .compute_style(id, hitbox.as_ref(), window, cx);
        let visible = self.enabled
            && style.opacity.unwrap_or(1.) > 0.
            && style.visibility == gpui_kit::Visibility::Visible
            && style.display != gpui_kit::Display::None
            && !bounds.intersect(&window.content_mask().bounds).is_empty();
        let radians = crate::interaction::decorative_value(
            self.channel.clone(),
            self.target,
            Duration::from_millis(Motion::DISCLOSURE_TURN_MS),
            visible,
            window,
            cx,
        )
        .value;
        self.inner = std::mem::replace(&mut self.inner, svg())
            .with_transformation(Transformation::rotate(gpui_kit::Radians(radians)));
        self.inner
            .paint(id, inspector, bounds, layout, hitbox, window, cx);
    }
}
