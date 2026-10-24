use std::{cell::RefCell, ops::Deref, rc::Rc};

use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::{
    AnyElement, App, ElementId, IntoElement, OngoingScroll, Pixels, RenderOnce, Rgba, ScrollHandle,
    ScrollWheelEvent, TouchPhase, Window, div, prelude::*,
};

use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};

use super::code_drag::{CodeDragElement, CodeDragSelection};

#[derive(Clone, Default)]
pub(super) struct CodeScrollHandle {
    handle: ScrollHandle,
    identity: Rc<()>,
}

impl CodeScrollHandle {
    pub(super) fn same_binding(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.identity, &other.identity)
    }
}

impl Deref for CodeScrollHandle {
    type Target = ScrollHandle;

    fn deref(&self) -> &Self::Target {
        &self.handle
    }
}

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
    scroll: Option<CodeScrollHandle>,
    height: Option<Pixels>,
    selection: Option<CodeDragSelection>,
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
            selection: None,
        }
    }

    pub(super) fn scroll(mut self, scroll: CodeScrollHandle) -> Self {
        self.scroll = Some(scroll);
        self
    }

    pub(super) fn selection(mut self, selection: Option<CodeDragSelection>) -> Self {
        self.selection = selection;
        self
    }

    pub(super) fn height(mut self, height: Pixels) -> Self {
        self.height = Some(height);
        self
    }
}

impl RenderOnce for CodeViewport {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let binding = self.scroll.unwrap_or_else(|| {
            window
                .use_keyed_state((self.id.clone(), "scroll-handle"), cx, |_, _| {
                    CodeScrollHandle::default()
                })
                .read(cx)
                .clone()
        });
        let scroll = binding.handle.clone();
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
        #[cfg(test)]
        super::code_drag_tests::observe_scroll_handle(
            window.window_handle().window_id(),
            Some(self.id.clone()),
            scroll.clone(),
            cx,
        );
        let viewport = div()
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
                    let next =
                        (offset.x.clamp(-max, Pixels::ZERO) + delta).clamp(-max, Pixels::ZERO);
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
            )
            .into_any_element();
        let viewport = if let Some(selection) = self.selection {
            CodeDragElement::new(
                (self.id.clone(), "code-drag").into(),
                viewport,
                selection,
                binding,
            )
            .into_any_element()
        } else {
            viewport
        };
        div()
            .id((self.id.clone(), "viewport"))
            .w_full()
            .min_w_0()
            .relative()
            .child(viewport)
            .child(
                Scrollbar::horizontal(&scroll)
                    .id((self.id, "scrollbar"))
                    .mode(mode),
            )
    }
}
