use crate::nsh::Surface;
use gpui_kit::{
    AnyElement, App, Bounds, ContentMask, Context, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Render, Subscription, Window, div,
    prelude::*,
};
use ns::{Accessible as _, IntoElement as _, Styled as _};
use nsh_core::prelude as ns;
use vega_theme::{Layout, theme};

#[derive(Clone, PartialEq)]
struct Style {
    background: gpui_kit::Rgba,
    border: gpui_kit::Rgba,
}

struct Host {
    surface: Surface<Style, ()>,
    style: Style,
    _release: Subscription,
}

pub(in crate::conversation_stream::composer_actions) fn element(
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let colors = theme(cx).colors;
    let style = Style {
        background: colors.bg_elevated,
        border: colors.border_subtle,
    };
    let host = window.use_keyed_state("nsh-composer-shell", cx, |window, cx| {
        let (surface, _) = Surface::new_transparent(style.clone(), render, window, cx);
        let _ = surface.focus_handle(cx).tab_stop(false);
        Host {
            surface,
            style: style.clone(),
            _release: cx.on_release(|host: &mut Host, cx| host.surface.entity().read(cx).stop()),
        }
    });
    host.update(cx, |host, _| {
        if host.style != style {
            host.style = style.clone();
            host.surface.publish(style);
        }
    });
    host.into_any_element()
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.surface.sync_environment(cx);
        Passive {
            child: div()
                .size_full()
                .child(self.surface.entity())
                .into_any_element(),
        }
    }
}

struct Passive {
    child: AnyElement,
}

impl IntoElement for Passive {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Passive {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some("nsh-composer-shell-passive".into())
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
        (self.child.request_layout(window, cx), ())
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
        let mask = ContentMask {
            bounds: Bounds {
                origin: window.content_mask().bounds.center(),
                size: Default::default(),
            },
        };
        window.with_content_mask(Some(mask), |window| self.child.prepaint(window, cx));
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let clipped = bounds.intersect(&window.content_mask().bounds);
        window.with_content_mask(Some(ContentMask { bounds: clipped }), |window| {
            self.child.paint(window, cx)
        });
    }
}

fn render(state: ns::Signal<Style>, _: tokio::sync::mpsc::Sender<()>) -> ns::AnyElement {
    ns::view()
        .accessible(false)
        .accessibility_elements_hidden(true)
        .pointer_events(nsh_core::PointerEvents::None)
        .w_full()
        .h_full()
        .rounded(ns::px(Layout::COMPOSER_RADIUS))
        .bg(move || crate::nsh::color(state.get().background))
        .border(ns::px(1.))
        .border_color(move || crate::nsh::color(state.get().border))
        .into_element()
}
