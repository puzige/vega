use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui_kit::{
    App, Bounds, Element, ElementId, GlobalElementId, Hitbox, InspectorElementId,
    InteractiveElement, IntoElement, LayoutId, Pixels, Rgba, SharedString, Styled, Subscription,
    Window, base::motion::MotionStatus, fill, px,
};
use vega_theme::{Layout, Motion};

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Destination {
    Other(usize),
    ProviderList,
    Provider(String),
    ProviderForm(Option<String>),
    Model(String, String, usize),
}

impl Destination {
    fn has_title(&self) -> bool {
        matches!(self, Self::Provider(_) | Self::Model(..))
    }
}

pub(super) enum ReturnFocus {
    ProviderList {
        provider: Option<String>,
        index: usize,
    },
    Model {
        provider: String,
        model: String,
    },
}

#[derive(Default)]
struct Presentation {
    destination: Option<Destination>,
    narrow: Option<bool>,
    permit: Option<Destination>,
    epoch: u64,
    title_serial: u64,
    seed_pending: bool,
    title_active: bool,
}

impl Presentation {
    fn interrupt(&mut self) {
        self.destination = None;
        self.permit = None;
        self.epoch = self.epoch.wrapping_add(1);
        self.seed_pending = false;
        self.title_active = false;
    }
}

#[derive(Default)]
pub(super) struct SettingsMotion {
    presentation: Rc<RefCell<Presentation>>,
    subscriptions: Option<[Subscription; 2]>,
}

impl SettingsMotion {
    pub(super) fn interrupt(&mut self) {
        self.presentation.borrow_mut().interrupt();
    }

    pub(super) fn committed(&mut self, before: Destination, after: Destination) {
        if before != after {
            self.presentation.borrow_mut().permit = Some(after);
        }
    }
}

pub(super) fn narrow(window: &Window) -> bool {
    let content_width = (f32::from(window.viewport_size().width)
        - Layout::SETTINGS_NAV_WIDTH
        - Layout::SETTINGS_SECTION_GAP * 2.)
        .min(Layout::SETTINGS_CONTENT_MAX_WIDTH);
    content_width
        < Layout::SETTINGS_PROVIDER_LIST_WIDTH
            + Layout::SETTINGS_PROVIDER_COLUMN_GAP
            + Layout::SETTINGS_PROVIDER_DETAIL_MIN_WIDTH
}

impl SettingsView {
    pub(super) fn project_settings_motion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_motion.subscriptions.is_none() {
            let activation = cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.settings_motion.interrupt();
                }
                cx.notify();
            });
            let bounds = cx.observe_window_bounds(window, |this, window, cx| {
                if this.settings_motion.presentation.borrow().narrow != Some(narrow(window)) {
                    this.settings_motion.interrupt();
                    cx.notify();
                }
            });
            self.settings_motion.subscriptions = Some([activation, bounds]);
        }
        let destination = self.settings_motion_destination();
        let narrow = narrow(window);
        let eligible = window.is_window_active()
            && !cx.reduce_motion()
            && cx.try_global::<SettingsOpen>().is_none_or(|open| open.0);
        let mut state = self.settings_motion.presentation.borrow_mut();
        let changed = state.destination.as_ref() != Some(&destination);
        let same_mode = state.narrow == Some(narrow);
        let permitted = state.permit.take().as_ref() == Some(&destination)
            && state.destination.is_some()
            && changed
            && same_mode
            && eligible;
        if !eligible || !same_mode || (changed && !permitted) {
            state.interrupt();
        }
        if permitted {
            state.title_serial = state.title_serial.wrapping_add(1);
            state.seed_pending = narrow && destination.has_title();
            state.title_active = state.seed_pending;
        }
        state.destination = Some(destination);
        state.narrow = Some(narrow);
    }

    pub(super) fn motion_title(
        &self,
        inner: Div,
        owner: gpui_kit::EntityId,
    ) -> impl IntoElement + use<> {
        self.settings_motion_element(inner, "title", Paint::Title, owner)
    }

    pub(super) fn motion_provider_row(
        &self,
        inner: crate::interaction::Interaction,
        provider: &str,
        selected: bool,
        color: Rgba,
        owner: gpui_kit::EntityId,
    ) -> impl IntoElement + use<> {
        self.settings_motion_element(
            inner,
            &format!("provider-fill:{provider:?}"),
            Paint::Fill { selected, color },
            owner,
        )
    }

    pub(super) fn motion_model_marker(
        &self,
        inner: Div,
        identity: (&str, &str, usize),
        selected: bool,
        color: Rgba,
        owner: gpui_kit::EntityId,
    ) -> impl IntoElement + use<> {
        self.settings_motion_element(
            inner,
            &format!("model-marker:{identity:?}"),
            Paint::Marker { selected, color },
            owner,
        )
    }

    fn settings_motion_element<E>(
        &self,
        inner: E,
        property: &str,
        paint: Paint,
        owner: gpui_kit::EntityId,
    ) -> MotionElement<E> {
        let state = self.settings_motion.presentation.borrow();
        let identity = if matches!(paint, Paint::Title) {
            format!("{:?}:{}", state.destination, state.title_serial)
        } else {
            String::new()
        };
        MotionElement {
            inner,
            channel: format!(
                "settings-motion:{owner}:{}:{property}:{identity}",
                state.epoch,
            )
            .into(),
            presentation: self.settings_motion.presentation.clone(),
            epoch: state.epoch,
            title_serial: state.title_serial,
            paint,
        }
    }
}

#[derive(Clone, Copy)]
enum Paint {
    Title,
    Fill { selected: bool, color: Rgba },
    Marker { selected: bool, color: Rgba },
}

struct MotionElement<E> {
    inner: E,
    channel: SharedString,
    presentation: Rc<RefCell<Presentation>>,
    epoch: u64,
    title_serial: u64,
    paint: Paint,
}

impl<E> IntoElement for MotionElement<E>
where
    E: Element<PrepaintState = Option<Hitbox>> + InteractiveElement + Styled + 'static,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E> Element for MotionElement<E>
where
    E: Element<PrepaintState = Option<Hitbox>> + InteractiveElement + Styled + 'static,
{
    type RequestLayoutState = E::RequestLayoutState;
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
        let visible = style.opacity.unwrap_or(1.) > 0.
            && style.visibility == gpui_kit::Visibility::Visible
            && style.display != gpui_kit::Display::None
            && !bounds.intersect(&window.content_mask().bounds).is_empty();
        let duration = Duration::from_millis(Motion::INTERACTION_DURATION_MS);
        let (enabled, seed) = {
            let mut state = self.presentation.borrow_mut();
            let current = state.epoch == self.epoch;
            match self.paint {
                Paint::Title => {
                    let current = current && state.title_serial == self.title_serial;
                    let enabled = current && state.title_active && state.narrow == Some(true);
                    let seed = enabled && std::mem::take(&mut state.seed_pending);
                    if enabled && (!visible || !window.is_window_active() || cx.reduce_motion()) {
                        state.title_active = false;
                    }
                    (enabled && state.title_active, seed)
                }
                _ => (current && state.narrow == Some(false), false),
            }
        };
        if seed && enabled {
            crate::interaction::decorative_value(
                self.channel.clone(),
                0.,
                Duration::ZERO,
                visible,
                window,
                cx,
            );
        }
        let target = match self.paint {
            Paint::Title => 1.,
            Paint::Fill { selected, .. } | Paint::Marker { selected, .. } => {
                if selected {
                    1.
                } else {
                    0.
                }
            }
        };
        let sample = crate::interaction::decorative_value(
            self.channel.clone(),
            target,
            if enabled { duration } else { Duration::ZERO },
            visible,
            window,
            cx,
        );
        let original_opacity = self.inner.style().opacity;
        let original_border = self.inner.style().border_color;
        match self.paint {
            Paint::Title => {
                if matches!(sample.status, MotionStatus::Finished | MotionStatus::Idle) {
                    self.presentation.borrow_mut().title_active = false;
                }
                self.inner.style().opacity = Some(style.opacity.unwrap_or(1.) * sample.value);
            }
            Paint::Fill { mut color, .. } => {
                color.a *= sample.value;
                if visible {
                    window.paint_quad(
                        fill(bounds, color).corner_radii(px(Layout::SETTINGS_CONTROL_RADIUS)),
                    );
                }
            }
            Paint::Marker { mut color, .. } => {
                color.a *= sample.value;
                self.inner.style().border_color = Some(color.into());
            }
        }
        self.inner
            .paint(id, inspector, bounds, layout, hitbox, window, cx);
        self.inner.style().opacity = original_opacity;
        self.inner.style().border_color = original_border;
    }
}
