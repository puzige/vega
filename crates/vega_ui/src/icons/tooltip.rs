use gpui_kit::{prelude::*, *};
use ns::{Accessible as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;

use crate::nsh::{Surface, color};
use vega_theme::Typography;

#[derive(Clone, PartialEq)]
struct Snapshot {
    label: String,
    shortcut: Option<String>,
    label_bounds: [f32; 4],
    shortcut_bounds: Option<[f32; 4]>,
    shortcut_text_bounds: Option<[f32; 4]>,
    colors: [Rgba; 5],
    family: String,
    weight: u16,
    italic: bool,
    line_height: f32,
    radius: f32,
}

struct Host {
    surface: Surface<Snapshot, ()>,
    published: Snapshot,
    _release: Subscription,
}

pub(super) fn element(
    label: SharedString,
    shortcut: Option<SharedString>,
    colors: vega_theme::ThemeColors,
) -> AnyElement {
    chrome(&label, shortcut.as_ref())
        .child(Body {
            label,
            shortcut,
            colors: [
                colors.bg_elevated,
                colors.border_subtle,
                colors.text_primary,
                colors.bg_hover,
                colors.text_secondary,
            ],
        })
        .into_any_element()
}

fn chrome(label: &SharedString, shortcut: Option<&SharedString>) -> Stateful<Div> {
    let accessible_label = match shortcut {
        Some(shortcut) => format!("{label} {shortcut}"),
        None => label.to_string(),
    };
    div()
        .id("nsh-icon-tooltip")
        .role(Role::Tooltip)
        .aria_label(accessible_label)
}

struct Body {
    label: SharedString,
    shortcut: Option<SharedString>,
    colors: [Rgba; 5],
}

struct Measurement {
    _elements: Vec<AnyElement>,
    label: LayoutId,
    shortcut: Option<(LayoutId, LayoutId)>,
    family: String,
    weight: u16,
    italic: bool,
    line_height: f32,
    radius: f32,
}

impl IntoElement for Body {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Body {
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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = window.text_style();
        let rem = window.rem_size();
        let mut label = div()
            .text_size(px(Typography::METADATA))
            .child(self.label.clone())
            .into_any_element();
        let label_id = label.request_layout(window, cx);
        let mut children = vec![label_id];
        let mut elements = vec![label];
        let shortcut = self.shortcut.clone().map(|shortcut| {
            let mut text = div()
                .text_size(px(Typography::METADATA))
                .child(shortcut)
                .into_any_element();
            let text_id = text.request_layout(window, cx);
            elements.push(text);
            let chip = window.request_layout(
                Style {
                    padding: Edges {
                        left: (rem * 0.25).into(),
                        right: (rem * 0.25).into(),
                        ..Default::default()
                    },
                    border_widths: Edges::all(px(1.).into()),
                    ..Default::default()
                },
                [text_id],
                cx,
            );
            children.push(chip);
            (chip, text_id)
        });
        let root = window.request_layout(
            Style {
                display: Display::Flex,
                align_items: Some(AlignItems::Center),
                gap: size((rem * 0.25).into(), (rem * 0.25).into()),
                padding: Edges {
                    left: (rem * 0.5).into(),
                    right: (rem * 0.5).into(),
                    top: (rem * 0.25).into(),
                    bottom: (rem * 0.25).into(),
                },
                border_widths: Edges::all(px(1.).into()),
                ..Default::default()
            },
            children,
            cx,
        );
        (
            root,
            Measurement {
                _elements: elements,
                label: label_id,
                shortcut,
                family: style.font_family.to_string(),
                weight: style.font_weight.0 as u16,
                italic: style.font_style == FontStyle::Italic,
                line_height: style
                    .line_height
                    .to_pixels(px(Typography::METADATA).into(), rem)
                    .into(),
                radius: (rem * 0.375).into(),
            },
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        measurement: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let rect = |id, window: &mut Window| {
            let rect = window.layout_bounds(id);
            [
                f32::from(rect.origin.x - bounds.origin.x),
                f32::from(rect.origin.y - bounds.origin.y),
                f32::from(rect.size.width),
                f32::from(rect.size.height),
            ]
        };
        let snapshot = Snapshot {
            label: self.label.to_string(),
            shortcut: self.shortcut.as_ref().map(ToString::to_string),
            label_bounds: rect(measurement.label, window),
            shortcut_bounds: measurement.shortcut.map(|(chip, _)| rect(chip, window)),
            shortcut_text_bounds: measurement.shortcut.map(|(_, text)| rect(text, window)),
            colors: self.colors,
            family: measurement.family.clone(),
            weight: measurement.weight,
            italic: measurement.italic,
            line_height: measurement.line_height,
            radius: measurement.radius,
        };
        let host = window.use_keyed_state("nsh-icon-tooltip-surface", cx, |window, cx| {
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
        let mut child = div()
            .id(super::nsh::paint_id(bounds, window))
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
        _: &mut Self::RequestLayoutState,
        child: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let bounds = bounds.intersect(&window.content_mask().bounds);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            child.paint(window, cx);
        });
    }
}

fn text(value: String, rect: [f32; 4], snapshot: &Snapshot, foreground: Rgba) -> ns::AnyElement {
    let label = ns::text(value)
        .absolute()
        .left(ns::px(rect[0]))
        .top(ns::px(rect[1]))
        .w(ns::px(rect[2]))
        .h(ns::px(rect[3]))
        .font_family(snapshot.family.clone())
        .font_weight(ns::FontWeight(snapshot.weight))
        .text_size(ns::px(Typography::METADATA))
        .line_height(ns::px(snapshot.line_height))
        .text_color(color(foreground));
    if snapshot.italic {
        label.italic().into_element()
    } else {
        label.into_element()
    }
}

fn pixels(state: ns::Signal<Snapshot>, _: tokio::sync::mpsc::Sender<()>) -> ns::AnyElement {
    let root = ns::view()
        .size_full()
        .accessible(false)
        .accessibility_elements_hidden(true);
    root.child(ns::dynamic(move || {
        let snapshot = state.get();
        let [
            background,
            border,
            foreground,
            chip_background,
            chip_foreground,
        ] = snapshot.colors;
        let mut body = ns::view().size_full().child(
            ns::view()
                .absolute()
                .left(ns::px(0.))
                .top(ns::px(0.))
                .size_full()
                .rounded(ns::px(snapshot.radius))
                .border(ns::px(1.))
                .border_color(color(border))
                .bg(color(background)),
        );
        if let (Some(shortcut), Some(rect), Some(text_rect)) = (
            snapshot.shortcut.clone(),
            snapshot.shortcut_bounds,
            snapshot.shortcut_text_bounds,
        ) {
            body = body
                .child(
                    ns::view()
                        .absolute()
                        .left(ns::px(rect[0]))
                        .top(ns::px(rect[1]))
                        .w(ns::px(rect[2]))
                        .h(ns::px(rect[3]))
                        .rounded(ns::px(snapshot.radius))
                        .border(ns::px(1.))
                        .border_color(color(border))
                        .bg(color(chip_background)),
                )
                .child(text(shortcut, text_rect, &snapshot, chip_foreground));
        }
        body.child(text(
            snapshot.label.clone(),
            snapshot.label_bounds,
            &snapshot,
            foreground,
        ))
        .into_element()
    }))
    .into_element()
}
