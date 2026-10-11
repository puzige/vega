use std::{cell::Cell, rc::Rc, time::Duration};

use gpui_kit::{
    AnyElement, App, Bounds, Div, DivFrameState, Element, ElementId, GlobalElementId, Hitbox,
    InspectorElementId, InteractiveElement, Interactivity, IntoElement, LayoutId, ParentElement,
    Pixels, Rgba, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window,
    base::motion::{Transition, transition},
};
use vega_theme::{Motion, ThemeColors};

mod platform;
pub use platform::{install_system_motion, observe_system_motion, refresh_system_motion};

pub trait InteractionExt: Sized {
    fn interactive(self, colors: ThemeColors, enabled: bool, selected: bool) -> Interaction;

    fn interaction_surface(
        self,
        rest: Rgba,
        hover: Rgba,
        pressed: Rgba,
        enabled: bool,
    ) -> Interaction;
}

impl InteractionExt for Stateful<Div> {
    fn interactive(self, colors: ThemeColors, enabled: bool, selected: bool) -> Interaction {
        let rest = if selected {
            colors.bg_active
        } else {
            Rgba {
                a: 0.,
                ..colors.bg_hover
            }
        };
        let hover = if selected {
            colors.bg_active
        } else {
            colors.bg_hover
        };
        self.interaction_surface(rest, hover, colors.bg_active, enabled)
    }

    fn interaction_surface(
        mut self,
        rest: Rgba,
        hover: Rgba,
        pressed: Rgba,
        enabled: bool,
    ) -> Interaction {
        self.style().background = None;
        Interaction {
            base: self.hover(|style| style),
            rest,
            hover,
            pressed,
            enabled,
            capture_press: false,
        }
    }
}

pub struct Interaction {
    base: Stateful<Div>,
    rest: Rgba,
    hover: Rgba,
    pressed: Rgba,
    enabled: bool,
    capture_press: bool,
}

impl Interaction {
    pub fn capture_native_button_press(mut self) -> Self {
        self.capture_press = true;
        self
    }
}

impl Styled for Interaction {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for Interaction {
    fn extend(&mut self, children: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(children);
    }
}

impl InteractiveElement for Interaction {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Interaction {}

impl IntoElement for Interaction {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Interaction {
    type RequestLayoutState = DivFrameState;
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        Element::id(&self.base)
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
        prepaint: &mut Self::PrepaintState,
        builder: &mut gpui_kit::A11ySubtreeBuilder,
    ) {
        Element::a11y_synthetic_children(&mut self.base, prepaint, builder);
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
        self.base
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
        let hovered = self.enabled
            && hitbox
                .as_ref()
                .is_some_and(|hitbox| hitbox.is_hovered(window));
        let hover_state = window.use_keyed_state("interaction-hover-source", cx, |_, _| false);
        if hovered != *hover_state.read(cx) {
            if hovered {
                refresh_system_motion(cx);
            }
            hover_state.update(cx, |state, _| *state = hovered);
        }
        let explicit_fill = self.base.style().background.take();
        let explicit_background = explicit_fill.as_ref().and_then(|fill| fill.color());
        let surface = (
            self.rest,
            self.hover,
            self.pressed,
            self.enabled,
            explicit_background,
        );
        let surface_state = window.use_keyed_state("interaction-surface", cx, |_, _| surface);
        let semantic_changed = *surface_state.read(cx) != surface;
        if semantic_changed {
            surface_state.update(cx, |state, _| *state = surface);
        }
        let explicit_solid = explicit_background.and_then(|background| background.as_solid());
        let rest = explicit_solid.unwrap_or_else(|| self.rest.into());
        let static_background = explicit_background.filter(|_| explicit_solid.is_none());
        let target = if hovered { self.hover.into() } else { rest };
        let background = transition(
            "interaction-background",
            target,
            Transition::new(
                if self.enabled
                    && rest != self.hover.into()
                    && !semantic_changed
                    && static_background.is_none()
                {
                    Duration::from_millis(Motion::INTERACTION_DURATION_MS)
                } else {
                    Duration::ZERO
                },
            ),
            window,
            cx,
        );
        let pressed = if self.capture_press {
            let state = window.use_keyed_state("interaction-native-pressed", cx, |_, _| {
                Rc::new(Cell::new(false))
            });
            let state = state.read(cx).clone();
            if !self.enabled || !window.is_window_active() {
                state.set(false);
            }
            if self.enabled {
                let down = state.clone();
                self.base
                    .interactivity()
                    .capture_any_mouse_down(move |event, window, _| {
                        if event.button == gpui_kit::MouseButton::Left && !down.replace(true) {
                            window.refresh();
                        }
                    });
                let up = state.clone();
                self.base
                    .interactivity()
                    .capture_any_mouse_up(move |event, window, _| {
                        if event.button == gpui_kit::MouseButton::Left && up.replace(false) {
                            window.refresh();
                        }
                    });
                let outside = state.clone();
                self.base.interactivity().on_mouse_up_out(
                    gpui_kit::MouseButton::Left,
                    move |_, window, _| {
                        if outside.replace(false) {
                            window.refresh();
                        }
                    },
                );
            }
            hovered && state.get()
        } else {
            hovered && self.base.interactivity().active.unwrap_or(false)
        };
        self.base.style().background = Some(if pressed {
            self.pressed.into()
        } else if !hovered {
            static_background
                .map(Into::into)
                .unwrap_or_else(|| background.into())
        } else {
            background.into()
        });
        self.base
            .paint(id, inspector, bounds, layout, hitbox, window, cx);
        self.base.style().background = explicit_fill;
    }
}

#[cfg(test)]
pub(crate) mod tests;
