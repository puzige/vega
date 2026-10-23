use gpui_kit::base::{ScrollableMask, Scrollbar, ScrollbarMode};
use gpui_kit::{
    AnyElement, App, Axis, ElementId, IntoElement, Pixels, RenderOnce, Rgba, ScrollHandle, Window,
    div, prelude::*,
};

use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};

#[cfg(test)]
#[path = "code_viewport_tests.rs"]
pub(super) mod tests;

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
                    .flex()
                    .child(self.content)
                    .tracked_scroll_edge_fade(
                        scroll.clone(),
                        self.background,
                        ScrollFade::compact().horizontal_viewport(),
                    ),
            )
            .child(ScrollableMask::new(Axis::Horizontal, &scroll).id(self.id.clone()))
            .child(
                Scrollbar::horizontal(&scroll)
                    .id((self.id, "scrollbar"))
                    .mode(mode),
            )
    }
}
