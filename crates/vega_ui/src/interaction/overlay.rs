use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::{
    App, AppContext, Bounds, Element, ElementId, Entity, GlobalElementId, Hitbox,
    InspectorElementId, InteractiveElement, IntoElement, LayoutId, Pixels, Styled, Subscription,
    Window, base::motion::MotionStatus,
};
use vega_theme::Motion;

#[derive(Clone, Default)]
pub struct OverlayEntry(Rc<RefCell<EntryState>>);

#[derive(Default)]
struct EntryState {
    open: bool,
    revision: u64,
    last_alpha: Option<f32>,
    closed_at: Option<Instant>,
    pending_seed: Option<f32>,
    settled: bool,
    lifecycle: Option<Entity<OverlayLifecycle>>,
}

struct OverlayLifecycle {
    _activation: Subscription,
}

impl OverlayEntry {
    pub fn open(&self, cx: &App) {
        let mut state = self.0.borrow_mut();
        if state.open {
            return;
        }
        let seed = state
            .closed_at
            .filter(|closed| {
                cx.background_executor()
                    .now()
                    .saturating_duration_since(*closed)
                    <= Duration::from_millis(Motion::OVERLAY_ENTER_MS)
            })
            .and(state.last_alpha);
        if seed.is_none() {
            state.last_alpha = None;
        }
        let seed = seed.unwrap_or(0.);
        state.open = true;
        state.revision = state.revision.wrapping_add(1);
        state.pending_seed = Some(seed);
        state.closed_at = None;
        state.settled = false;
    }

    pub fn close(&self, cx: &App) {
        let mut state = self.0.borrow_mut();
        if state.open {
            state.open = false;
            state.closed_at = Some(cx.background_executor().now());
            state.pending_seed = None;
        }
    }

    pub fn invalidate(&self) {
        let mut state = self.0.borrow_mut();
        state.open = false;
        state.revision = state.revision.wrapping_add(1);
        state.last_alpha = None;
        state.closed_at = None;
        state.pending_seed = None;
        state.settled = false;
    }

    pub fn suspend(&self) {
        Self::interrupt(&mut self.0.borrow_mut());
    }

    pub fn sync(&self, open: bool, visible: bool, window: &mut Window, cx: &mut App) {
        if self.0.borrow().lifecycle.is_none() {
            let weak = Rc::downgrade(&self.0);
            let lifecycle = cx.new(|cx| OverlayLifecycle {
                _activation: cx.observe_window_activation(window, move |_, window, _| {
                    if !window.is_window_active()
                        && let Some(state) = weak.upgrade()
                    {
                        Self::interrupt(&mut state.borrow_mut());
                    }
                }),
            });
            self.0.borrow_mut().lifecycle = Some(lifecycle);
        }
        if open {
            self.open(cx);
        } else {
            self.close(cx);
        }
        if !visible || !window.is_window_active() || cx.reduce_motion() {
            Self::interrupt(&mut self.0.borrow_mut());
        }
    }

    fn interrupt(state: &mut EntryState) {
        state.last_alpha = None;
        if state.open {
            state.settled = true;
            state.pending_seed = Some(1.);
        }
    }

    pub fn surface<E: InteractiveElement>(&self, inner: E, bypass: bool) -> OverlaySurface<E> {
        #[cfg(test)]
        let inner = inner.debug_selector(|| "overlay-entry-surface".into());
        OverlaySurface {
            inner,
            entry: self.clone(),
            revision: self.0.borrow().revision,
            bypass,
        }
    }
}

pub struct OverlaySurface<E> {
    inner: E,
    entry: OverlayEntry,
    revision: u64,
    bypass: bool,
}

impl<E> IntoElement for OverlaySurface<E>
where
    E: Element<PrepaintState = Option<Hitbox>> + InteractiveElement + Styled + 'static,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E> Element for OverlaySurface<E>
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
        let visible = style.visibility == gpui_kit::Visibility::Visible
            && style.display != gpui_kit::Display::None
            && style.opacity.unwrap_or(1.) > 0.
            && !bounds.intersect(&window.content_mask().bounds).is_empty();
        let (enabled, seed) = {
            let mut state = self.entry.0.borrow_mut();
            if self.bypass || !visible || !window.is_window_active() || cx.reduce_motion() {
                OverlayEntry::interrupt(&mut state);
            }
            let current = state.open && state.revision == self.revision;
            (
                current && !state.settled,
                if current {
                    state.pending_seed.take()
                } else {
                    None
                },
            )
        };
        let sample = window.with_element_namespace("overlay-entry", |window| {
            let mut mounted = false;
            window.use_keyed_state("mount", cx, |_, _| {
                mounted = true;
            });
            let seed =
                seed.or_else(|| mounted.then(|| self.entry.0.borrow().last_alpha.unwrap_or(0.)));
            super::decorative::seeded_decorative_value(
                "opacity".into(),
                (1., seed),
                if enabled {
                    Duration::from_millis(Motion::OVERLAY_ENTER_MS)
                } else {
                    Duration::ZERO
                },
                visible,
                window,
                cx,
            )
        });
        let original_opacity = self.inner.style().opacity;
        self.inner.style().opacity = Some(style.opacity.unwrap_or(1.) * sample.value);
        self.inner
            .paint(id, inspector, bounds, layout, hitbox, window, cx);
        self.inner.style().opacity = original_opacity;
        let mut state = self.entry.0.borrow_mut();
        if state.open && state.revision == self.revision {
            if visible {
                state.last_alpha = Some(sample.value);
            }
            if matches!(sample.status, MotionStatus::Finished | MotionStatus::Idle) {
                state.settled = true;
            }
        }
    }
}

#[cfg(test)]
mod tests;
