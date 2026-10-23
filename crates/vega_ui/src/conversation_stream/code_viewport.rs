use std::{cell::RefCell, rc::Rc};

use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::{
    AnyElement, App, ElementId, IntoElement, OngoingScroll, Pixels, RenderOnce, Rgba, ScrollHandle,
    ScrollWheelEvent, TouchPhase, Window, div, prelude::*,
};

use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};

#[cfg(test)]
#[path = "code_viewport_tests.rs"]
pub(super) mod tests;

#[derive(Default)]
struct CodeScrollGesture {
    ongoing: OngoingScroll,
    horizontal: bool,
}

impl CodeScrollGesture {
    fn horizontal_delta(
        &mut self,
        event: &ScrollWheelEvent,
        line_height: Pixels,
    ) -> Option<Pixels> {
        let mut delta = event.delta.pixel_delta(line_height);
        if !event.delta.precise() {
            *self = Self::default();
            return (delta.x.abs() > delta.y.abs()).then_some(delta.x);
        }
        if event.touch_phase == TouchPhase::Started {
            self.horizontal = false;
        }
        let was_horizontal = self.horizontal;
        self.ongoing.filter(&mut delta, event.touch_phase);
        if matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.horizontal = false;
            return was_horizontal.then_some(delta.x);
        }
        if delta.x != Pixels::ZERO || delta.y != Pixels::ZERO {
            self.horizontal = delta.x.abs() > delta.y.abs();
        }
        self.horizontal.then_some(delta.x)
    }
}

#[derive(IntoElement)]
pub(super) struct CodeViewport {
    id: ElementId,
    content: AnyElement,
    background: Rgba,
    scroll: Option<ScrollHandle>,
    height: Option<Pixels>,
}

impl CodeViewport {
    pub(super) fn new(
        id: impl Into<ElementId>,
        content: impl IntoElement,
        background: Rgba,
    ) -> Self {
        Self {
            id: id.into(),
            content: content.into_any_element(),
            background,
            scroll: None,
            height: None,
        }
    }

    pub(super) fn scroll(mut self, scroll: ScrollHandle) -> Self {
        self.scroll = Some(scroll);
        self
    }

    pub(super) fn height(mut self, height: Pixels) -> Self {
        self.height = Some(height);
        self
    }
}

impl RenderOnce for CodeViewport {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scroll = self.scroll.unwrap_or_else(|| {
            window
                .use_keyed_state((self.id.clone(), "scroll-handle"), cx, |_, _| {
                    ScrollHandle::new()
                })
                .read(cx)
                .clone()
        });
        #[cfg(test)]
        tests::observe(self.id.clone(), scroll.clone(), window, cx);
        let gesture = window
            .use_keyed_state((self.id.clone(), "scroll-gesture"), cx, |_, _| {
                Rc::new(RefCell::new(CodeScrollGesture::default()))
            })
            .read(cx)
            .clone();
        let wheel_scroll = scroll.clone();
        let mode = if cx.should_auto_hide_scrollbars() {
            ScrollbarMode::Hover
        } else {
            ScrollbarMode::Always
        };
        div()
            .id((self.id.clone(), "viewport"))
            .w_full()
            .min_w_0()
            .relative()
            .child(
                div()
                    .id(self.id.clone())
                    .w_full()
                    .min_w_0()
                    .when_some(self.height, |body, height| body.h(height))
                    .overflow_hidden()
                    .track_scroll(&scroll)
                    .on_scroll_wheel(move |event, window, cx| {
                        let delta = gesture
                            .borrow_mut()
                            .horizontal_delta(event, window.line_height());
                        if let Some(delta) = delta {
                            let mut offset = wheel_scroll.offset();
                            let max = wheel_scroll.max_offset().x;
                            let next = (offset.x.clamp(-max, Pixels::ZERO) + delta)
                                .clamp(-max, Pixels::ZERO);
                            if next != offset.x {
                                offset.x = next;
                                wheel_scroll.set_offset(offset);
                                window.refresh();
                            }
                            cx.stop_propagation();
                        }
                    })
                    .flex()
                    .child(self.content)
                    .tracked_scroll_edge_fade(
                        scroll.clone(),
                        self.background,
                        ScrollFade::compact().horizontal_viewport(),
                    ),
            )
            .child(
                Scrollbar::horizontal(&scroll)
                    .id((self.id, "scrollbar"))
                    .mode(mode),
            )
    }
}
