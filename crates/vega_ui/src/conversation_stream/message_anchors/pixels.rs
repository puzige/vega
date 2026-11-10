use crate::nsh::Surface;
use gpui_kit::{prelude::*, *};
use ns::{Accessible as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use std::hash::{DefaultHasher, Hash, Hasher};
use vega_theme::{Layout, ThemeColors, Typography};

#[derive(Clone, Debug, PartialEq)]
enum Content {
    Mark { width: f32, color: Rgba },
    Preview { label: String, colors: [Rgba; 3] },
}

#[derive(Clone, PartialEq)]
struct Snapshot {
    content: Content,
    family: String,
    weight: u16,
    italic: bool,
    line_height: f32,
    radius: f32,
    padding: f32,
}

struct Host {
    surface: Surface<Snapshot, ()>,
    published: Snapshot,
    _release: Subscription,
}

pub(super) fn mark(selected: bool, colors: ThemeColors) -> AnyElement {
    Leaf {
        content: Content::Mark {
            width: if selected {
                Layout::MESSAGE_ANCHOR_ACTIVE_SIZE
            } else {
                Layout::CONVERSATION_MARKER_WIDTH
            },
            color: if selected {
                colors.text_secondary
            } else {
                colors.conversation_marker
            },
        },
    }
    .into_any_element()
}

pub(super) fn preview(label: String, colors: ThemeColors) -> AnyElement {
    Leaf {
        content: Content::Preview {
            label,
            colors: [
                colors.bg_elevated,
                colors.border_subtle,
                colors.text_primary,
            ],
        },
    }
    .into_any_element()
}

struct Leaf {
    content: Content,
}

struct Measurement {
    _element: AnyElement,
    snapshot: Snapshot,
}

impl IntoElement for Leaf {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Leaf {
    type RequestLayoutState = Measurement;
    type PrepaintState = AnyElement;

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
    ) -> (LayoutId, Measurement) {
        let style = window.text_style();
        let rem = window.rem_size();
        let snapshot = Snapshot {
            content: self.content.clone(),
            family: style.font_family.to_string(),
            weight: style.font_weight.0 as u16,
            italic: style.font_style == FontStyle::Italic,
            line_height: style
                .line_height
                .to_pixels(px(Typography::METADATA).into(), rem)
                .into(),
            radius: (rem * 0.375).into(),
            padding: (rem * 0.5).into(),
        };
        let mut element = match &self.content {
            Content::Mark { width, .. } => div()
                .w(px(*width))
                .h(px(Layout::MESSAGE_ANCHOR_TRACK_WIDTH))
                .into_any_element(),
            Content::Preview { label, .. } => div()
                .w_full()
                .max_h(px(Layout::MESSAGE_ANCHOR_PREVIEW_HEIGHT))
                .py_2()
                .flex()
                .items_center()
                .px_2()
                .border_1()
                .text_size(px(Typography::METADATA))
                .child(div().min_w_0().line_clamp(3).child(label.clone()))
                .into_any_element(),
        };
        let layout = element.request_layout(window, cx);
        (
            layout,
            Measurement {
                _element: element,
                snapshot,
            },
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        measurement: &mut Measurement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let snapshot = measurement.snapshot.clone();
        let host = window.use_keyed_state("nsh-message-anchor-pixels", cx, |window, cx| {
            let (surface, _) = Surface::new_transparent(snapshot.clone(), pixels, window, cx);
            surface.focus_handle(cx).tab_stop(false);
            Host {
                surface,
                published: snapshot.clone(),
                _release: cx.on_release(|host: &mut Host, cx| {
                    host.surface.entity().read(cx).stop();
                }),
            }
        });
        let surface = host.update(cx, |host, cx| {
            host.surface.sync_environment(cx);
            if host.published != snapshot {
                host.surface.publish(snapshot.clone());
                host.published = snapshot;
            }
            host.surface.entity()
        });
        let mut paint = DefaultHasher::new();
        bounds
            .intersect(&window.content_mask().bounds)
            .hash(&mut paint);
        let mut child = div()
            .id(("nsh-message-anchor-paint", paint.finish()))
            .w(bounds.size.width)
            .h(bounds.size.height)
            .child(surface)
            .into_any_element();
        child.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        let mask = ContentMask {
            bounds: Bounds {
                origin: window.content_mask().bounds.center(),
                size: Default::default(),
            },
        };
        window.with_content_mask(Some(mask), |window| {
            child.prepaint_at(bounds.origin, window, cx);
        });
        child
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _measurement: &mut Measurement,
        child: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        let bounds = bounds.intersect(&window.content_mask().bounds);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            child.paint(window, cx);
        });
    }
}

fn preview_shadows() -> Vec<ns::BoxShadow> {
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

fn pixels(state: ns::Signal<Snapshot>, _: tokio::sync::mpsc::Sender<()>) -> ns::AnyElement {
    let root = ns::view()
        .size_full()
        .accessible(false)
        .accessibility_elements_hidden(true);
    root.child(ns::dynamic(move || {
        let snapshot = state.get();
        match snapshot.content {
            Content::Mark { color, .. } => ns::view()
                .size_full()
                .bg(crate::nsh::color(color))
                .into_element(),
            Content::Preview { label, colors } => {
                let text = ns::text(label)
                    .w_full()
                    .min_w(ns::px(0.))
                    .number_of_lines(3u32)
                    .text_size(ns::px(Typography::METADATA))
                    .line_height(ns::px(snapshot.line_height))
                    .font_family(snapshot.family)
                    .font_weight(ns::FontWeight(snapshot.weight))
                    .text_color(crate::nsh::color(colors[2]));
                let text = if snapshot.italic { text.italic() } else { text };
                let shadows = preview_shadows();
                ns::view()
                    .size_full()
                    .flex()
                    .items_center()
                    .p(ns::px(snapshot.padding))
                    .rounded(ns::px(snapshot.radius))
                    .border(ns::px(1.))
                    .border_color(crate::nsh::color(colors[1]))
                    .bg(crate::nsh::color(colors[0]))
                    .shadow(shadows)
                    .child(text)
                    .into_element()
            }
        }
    }))
    .into_element()
}
