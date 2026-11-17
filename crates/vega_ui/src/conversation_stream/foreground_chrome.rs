use crate::nsh::Surface;
use gpui_kit::{prelude::*, *};
use ns::{Accessible as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use vega_theme::theme;

mod context_usage;

pub(super) use context_usage::indicator;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Decoration {
    Fade,
    Base,
    ConversationBase,
    UserBubble,
    ThinkingCardShadow,
    SelectorMenuShadow,
}

#[derive(Clone, PartialEq)]
struct Background {
    color: Rgba,
    fade: bool,
    decoration: Decoration,
    radius: f32,
}

struct BackgroundHost {
    surface: Surface<Background, ()>,
    published: Background,
    _release: Subscription,
}

pub(super) fn decoration(kind: Decoration) -> AnyElement {
    BackgroundElement { kind }.into_any_element()
}

#[derive(gpui_kit::IntoElement)]
struct BackgroundElement {
    kind: Decoration,
}

impl RenderOnce for BackgroundElement {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = theme(cx).colors;
        let state = Background {
            color: if self.kind == Decoration::UserBubble {
                Rgba {
                    a: colors.conversation_bubble.a,
                    ..colors.accent
                }
            } else {
                colors.bg_base
            },
            decoration: self.kind,
            fade: self.kind == Decoration::Fade,
            radius: if self.kind == Decoration::UserBubble {
                vega_theme::Layout::USER_MESSAGE_RADIUS
            } else {
                0.
            },
        };
        let key = match self.kind {
            Decoration::ThinkingCardShadow => "nsh-thinking-card-shadow",
            Decoration::SelectorMenuShadow => "nsh-selector-menu-shadow",
            Decoration::Fade => "nsh-foreground-fade",
            Decoration::Base => "nsh-foreground-base",
            Decoration::ConversationBase => "nsh-conversation-base",
            Decoration::UserBubble => "nsh-user-message-background",
        };
        let host = window.use_keyed_state(key, cx, |window, cx| {
            let (surface, _) =
                Surface::new_transparent(state.clone(), background_pixels, window, cx);
            surface.focus_handle(cx).tab_stop(false);
            BackgroundHost {
                surface,
                published: state.clone(),
                _release: cx.on_release(|host: &mut BackgroundHost, cx| {
                    host.surface.entity().read(cx).stop();
                }),
            }
        });
        host.update(cx, |host, _| {
            if host.published != state {
                host.surface.publish(state.clone());
                host.published = state;
            }
        });
        host.into_any_element()
    }
}

impl Render for BackgroundHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.surface.sync_environment(cx);
        Passive::new(div().size_full().child(self.surface.entity()))
    }
}

pub(super) fn small_popup_shadows() -> Vec<ns::BoxShadow> {
    let mut chrome = div().shadow_sm();
    chrome
        .style()
        .box_shadow
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|shadow| ns::BoxShadow {
            offset_x: ns::px(shadow.offset.x.into()),
            offset_y: ns::px(shadow.offset.y.into()),
            blur: ns::px(f32::from(shadow.blur_radius) * 2.),
            spread: ns::px(shadow.spread_radius.into()),
            color: crate::nsh::color(shadow.color.into()).into(),
            inset: shadow.inset,
        })
        .collect()
}

fn background_pixels(
    state: ns::Signal<Background>,
    _: tokio::sync::mpsc::Sender<()>,
) -> ns::AnyElement {
    let kind = state.get_untracked().decoration;
    if matches!(
        kind,
        Decoration::ThinkingCardShadow | Decoration::SelectorMenuShadow
    ) {
        let inset = ns::px(super::thinking_slider::SHADOW_MARGIN);
        let (radius, shadows) = if kind == Decoration::SelectorMenuShadow {
            (vega_theme::Layout::MENU_RADIUS, small_popup_shadows())
        } else {
            let shadows =
                [(10., 30., -3.), (4., 12., -4.)].map(|(offset, blur, spread)| ns::BoxShadow {
                    offset_x: ns::px(0.),
                    offset_y: ns::px(offset),
                    blur: ns::px(blur),
                    spread: ns::px(spread),
                    color: crate::nsh::color(gpui_kit::rgba(0x0000001a)).into(),
                    inset: false,
                });
            (vega_theme::Layout::COMPOSER_RADIUS, shadows.to_vec())
        };
        return ns::view()
            .size_full()
            .p(inset)
            .accessibility_elements_hidden(true)
            .pointer_events(nsh_core::PointerEvents::None)
            .child(ns::view().flex_1().rounded(ns::px(radius)).shadow(shadows))
            .into_element();
    }
    ns::view()
        .size_full()
        .accessibility_elements_hidden(true)
        .rounded(move || ns::px(state.get().radius))
        .bg(move || {
            let state = state.get();
            crate::nsh::color(if state.fade {
                state.color.opacity(0.)
            } else {
                state.color
            })
        })
        .child(
            ns::view()
                .size_full()
                .bg_linear_to_b()
                .gradient_from(move || crate::nsh::color(state.get().color.opacity(0.)))
                .gradient_to(move || {
                    let state = state.get();
                    crate::nsh::color(if state.fade {
                        state.color
                    } else {
                        state.color.opacity(0.)
                    })
                }),
        )
        .into_element()
}

struct Passive {
    child: AnyElement,
}

impl Passive {
    fn new(child: impl IntoElement) -> Self {
        Self {
            child: child.into_any_element(),
        }
    }
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
        let bounds = bounds.intersect(&window.content_mask().bounds);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.child.paint(window, cx);
        });
    }
}
