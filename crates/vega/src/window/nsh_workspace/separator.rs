use super::*;
use ns::{Accessible as _, IntoElement as _, Styled as _};
use nsh_core::prelude as ns;

pub(in crate::window) fn drag_region(
    key: &'static str,
    vertical: bool,
    window: &mut Window,
    cx: &mut App,
    on_down: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let region = div().flex();
    let region = if vertical {
        region
            .w(px(5.))
            .h_full()
            .cursor_col_resize()
            .justify_center()
    } else {
        region.h(px(5.)).w_full().cursor_row_resize().items_center()
    };
    region
        .child(render(key, vertical, window, cx))
        .on_mouse_down(MouseButton::Left, on_down)
}

pub(in crate::window) fn render(
    key: &'static str,
    vertical: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let color = theme(cx).colors.border_subtle;
    let host = host(key, color, window, cx);
    if vertical {
        div().w(px(1.)).h_full().child(host).into_any_element()
    } else {
        div().h(px(1.)).w_full().child(host).into_any_element()
    }
}

pub(in crate::window) fn background(
    key: &'static str,
    color: Rgba,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .child(host(key, color, window, cx))
        .into_any_element()
}

fn host(key: &'static str, color: Rgba, window: &mut Window, cx: &mut App) -> Entity<Host> {
    let host = window.use_keyed_state(key, cx, |window, cx| {
        let (surface, _) = Surface::new_transparent(color, pixels, window, cx);
        surface.focus_handle(cx).tab_stop(false);
        Host {
            surface,
            color,
            _release: cx.on_release(|host: &mut Host, cx| {
                host.surface.entity().read(cx).stop();
            }),
        }
    });
    host.update(cx, |host, _cx| {
        if host.color != color {
            host.color = color;
            host.surface.publish(color);
        }
    });
    host
}

struct Host {
    surface: Surface<Rgba, ()>,
    color: Rgba,
    _release: Subscription,
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

fn pixels(color: ns::Signal<Rgba>, _: tokio::sync::mpsc::Sender<()>) -> ns::AnyElement {
    ns::view()
        .size_full()
        .accessibility_elements_hidden(true)
        .bg(move || vega_ui::nsh::color(color.get()))
        .into_element()
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
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
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
        let bounds = bounds.intersect(&window.content_mask().bounds);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.child.paint(window, cx);
        });
    }
}
