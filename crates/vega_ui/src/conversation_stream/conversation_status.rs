use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{
    AnyElement, App, Bounds, ContentMask, Context, Element, ElementId, Font, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Render, Rgba, Subscription, WeakEntity,
    Window, div, prelude::*, px,
};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography, theme};

const EMPTY_HEIGHT: f32 = 72.;
const EMPTY_BOTTOM_MARGIN: f32 = 24.;
const EMPTY_GAP: f32 = 8.;
const LOCATION_PADDING_X: f32 = 8.;
const LOCATION_PADDING_Y: f32 = 4.;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Empty,
    Location,
    OmittedThinking(u64, usize),
    ReadOnlyPermission(u64),
    Backend,
    Notice,
    UserEllipsis,
    Compaction,
}

impl Kind {
    fn key(self) -> gpui_kit::SharedString {
        match self {
            Self::Empty => "nsh-conversation-empty".into(),
            Self::Location => "nsh-conversation-location".into(),
            Self::OmittedThinking(group, segment) => {
                format!("nsh-omitted-thinking-{group}-{segment}").into()
            }
            Self::ReadOnlyPermission(index) => format!("nsh-read-only-permission-{index}").into(),
            Self::Backend => "nsh-composer-backend-status".into(),
            Self::Notice => "nsh-conversation-notice".into(),
            Self::UserEllipsis => "nsh-user-message-ellipsis".into(),
            Self::Compaction => "nsh-context-compaction".into(),
        }
    }
}

#[derive(Clone, PartialEq)]
struct Label {
    text: String,
    color: Rgba,
    font: Font,
    size: f32,
    line_height: f32,
}

impl Label {
    fn new(text: String, color: Rgba, size: f32, title: bool, window: &Window) -> Self {
        let mut style = window.text_style();
        style.font_size = px(size).into();
        if title {
            style.font_weight = Typography::EMPTY_STATE_TITLE_WEIGHT;
        }
        Self {
            text,
            color,
            font: style.font(),
            size,
            line_height: f32::from(style.line_height_in_pixels(window.rem_size())),
        }
    }
}

#[derive(Clone, PartialEq)]
struct Input {
    kind: Kind,
    labels: Vec<Label>,
}

impl Input {
    fn initial_height(&self) -> f32 {
        match self.kind {
            Kind::Empty => EMPTY_HEIGHT,
            Kind::Location => self.labels[0].line_height + LOCATION_PADDING_Y * 2.,
            Kind::OmittedThinking(_, _) => self.labels[0].line_height + Layout::PROCESS_ITEM_GAP,
            Kind::ReadOnlyPermission(_) | Kind::Notice | Kind::UserEllipsis => {
                self.labels[0].line_height
            }
            Kind::Backend => Layout::TITLEBAR_CONTROL_SIZE,
            Kind::Compaction => Layout::CONVERSATION_TOOL_HEIGHT,
        }
    }
}

pub(super) fn empty(window: &mut Window, cx: &mut App) -> AnyElement {
    let colors = theme(cx).colors;
    element(
        Input {
            kind: Kind::Empty,
            labels: vec![
                Label::new(
                    vega_i18n::t("ui.what_would_you_like_to_do_today").into(),
                    colors.text_primary,
                    Typography::EMPTY_STATE_TITLE,
                    true,
                    window,
                ),
                Label::new(
                    vega_i18n::t("ui.enter_a_task_to_start_or_use_to").into(),
                    colors.text_secondary,
                    Typography::METADATA,
                    false,
                    window,
                ),
            ],
        },
        window,
        cx,
    )
}

pub(super) fn location(
    label: String,
    color: Rgba,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    element(
        Input {
            kind: Kind::Location,
            labels: vec![Label::new(
                label,
                color,
                Typography::METADATA,
                false,
                window,
            )],
        },
        window,
        cx,
    )
}

pub(super) fn notice(message: String, color: Rgba) -> AnyElement {
    passive_text(message, color, Kind::Notice)
}

pub(super) fn user_ellipsis(color: Rgba) -> AnyElement {
    passive_text("…".into(), color, Kind::UserEllipsis)
}

pub(super) fn compaction(message: String, color: Rgba) -> AnyElement {
    passive_text(message, color, Kind::Compaction)
}

fn passive_text(message: String, color: Rgba, kind: Kind) -> AnyElement {
    div()
        .id(kind.key())
        .w_full()
        .min_w_0()
        .child(
            gpui_kit::container_query(move |_, window, cx| {
                let compacting = kind == Kind::Compaction;
                let size = if compacting || kind == Kind::UserEllipsis {
                    Typography::CONVERSATION
                } else {
                    Typography::BODY
                };
                let mut label = Label::new(message.clone(), color, size, false, window);
                label.line_height = size
                    * if compacting {
                        Typography::CONVERSATION_LINE_HEIGHT
                    } else if kind == Kind::UserEllipsis {
                        Typography::CONVERSATION_USER_LINE_HEIGHT
                    } else {
                        Typography::BODY_LINE_HEIGHT
                    };
                element(
                    Input {
                        kind,
                        labels: vec![label],
                    },
                    window,
                    cx,
                )
            })
            .w_full(),
        )
        .into_any_element()
}

pub(super) fn backend(label: String, window: &mut Window, cx: &mut App) -> AnyElement {
    element(
        Input {
            kind: Kind::Backend,
            labels: vec![Label::new(
                label,
                theme(cx).colors.text_secondary,
                Typography::BODY,
                false,
                window,
            )],
        },
        window,
        cx,
    )
}

pub(super) fn read_only_permission(
    index: u64,
    text: String,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    element(
        Input {
            kind: Kind::ReadOnlyPermission(index),
            labels: vec![Label::new(
                text,
                theme(cx).colors.text_secondary,
                Typography::BODY,
                false,
                window,
            )],
        },
        window,
        cx,
    )
}

pub(super) fn omitted_thinking(
    group: u64,
    segment: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    element(
        Input {
            kind: Kind::OmittedThinking(group, segment),
            labels: vec![Label::new(
                vega_i18n::t("ui.earlier_thinking_omitted").into(),
                theme(cx).colors.text_tertiary,
                Typography::METADATA,
                false,
                window,
            )],
        },
        window,
        cx,
    )
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    input: Input,
    width: f32,
}

struct Measurement {
    id: ProjectionId,
    width: f32,
    height: f32,
}

impl Measurement {
    fn accepts(&self, id: &ProjectionId, width: f32) -> bool {
        self.id.matches(id)
            && self.width == width
            && self.width.is_finite()
            && self.width > 0.
            && self.height.is_finite()
            && self.height > 0.
    }
}

struct Host {
    input: Input,
    id: ProjectionId,
    width: f32,
    height: f32,
    ready: bool,
    surface: Option<Surface<Snapshot, Measurement>>,
    _subscriptions: Vec<Subscription>,
}

impl Host {
    fn reset(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.id = ProjectionId::new();
        self.height = self.input.initial_height();
        self.ready = false;
    }

    fn measured(&mut self, measurement: Measurement, cx: &mut Context<Self>) {
        if measurement.accepts(&self.id, self.width) {
            let height = match self.input.kind {
                Kind::Empty => measurement.height.max(EMPTY_HEIGHT),
                Kind::Backend => Layout::TITLEBAR_CONTROL_SIZE,
                Kind::Compaction => measurement.height.max(Layout::CONVERSATION_TOOL_HEIGHT),
                Kind::Location
                | Kind::OmittedThinking(_, _)
                | Kind::ReadOnlyPermission(_)
                | Kind::Notice
                | Kind::UserEllipsis => measurement.height,
            };
            if !self.ready || self.height != height {
                self.height = height;
                self.ready = true;
                cx.notify();
            }
        }
    }
}

fn element(input: Input, window: &mut Window, cx: &mut App) -> AnyElement {
    let host = window.use_keyed_state(input.kind.key(), cx, |_, cx| Host {
        height: input.initial_height(),
        input: input.clone(),
        id: ProjectionId::new(),
        width: 0.,
        ready: false,
        surface: None,
        _subscriptions: vec![cx.on_release(|host: &mut Host, cx| {
            if let Some(surface) = host.surface.take() {
                surface.entity().read(cx).stop();
            }
        })],
    });
    host.update(cx, |host, cx| {
        if host.input != input {
            host.input = input;
            host.reset(cx);
            cx.notify();
        }
    });
    host.into_any_element()
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.surface.is_none() && self.width.is_finite() && self.width > 0. {
            let (surface, mut measurements) = Surface::new_transparent(
                Snapshot {
                    id: self.id.clone(),
                    input: self.input.clone(),
                    width: self.width,
                },
                render,
                window,
                cx,
            );
            let _ = surface.focus_handle(cx).tab_stop(false);
            self.surface = Some(surface);
            cx.spawn_in(window, async move |host, cx| {
                while let Some(measurement) = measurements.recv().await {
                    if host
                        .update_in(cx, |host, _, cx| host.measured(measurement, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        if let Some(surface) = &mut self.surface {
            surface.sync_environment(cx);
        }
        Measured {
            child: div()
                .id(self.input.kind.key())
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .h(px(self.height))
                .when(self.input.kind == Kind::Empty, |view| {
                    view.mb(px(EMPTY_BOTTOM_MARGIN))
                })
                .children(self.surface.as_ref().map(Surface::entity))
                .into_any_element(),
            width: self.width,
            ready: self.ready,
            host: cx.weak_entity(),
        }
    }
}

struct Measured {
    child: AnyElement,
    width: f32,
    ready: bool,
    host: WeakEntity<Host>,
}

impl IntoElement for Measured {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Measured {
    type RequestLayoutState = ();
    type PrepaintState = bool;

    fn id(&self) -> Option<ElementId> {
        Some("nsh-conversation-status-measured".into())
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
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let width = f32::from(bounds.size.width);
        if !width.is_finite() || width <= 0. || self.width != width {
            let _ = self.host.update(cx, |host, cx| {
                if host.width != width {
                    host.width = if width.is_finite() { width.max(0.) } else { 0. };
                    host.reset(cx);
                    cx.notify();
                }
            });
            return false;
        }
        let input_mask = ContentMask {
            bounds: Bounds {
                origin: window.content_mask().bounds.center(),
                size: Default::default(),
            },
        };
        window.with_content_mask(Some(input_mask), |window| {
            self.child.prepaint(window, cx);
        });
        self.ready
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        ready: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if *ready {
            let bounds = bounds.intersect(&window.content_mask().bounds);
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                self.child.paint(window, cx);
            });
        }
    }
}

fn render(state: ns::Signal<Snapshot>, measurements: mpsc::Sender<Measurement>) -> ns::AnyElement {
    ns::view()
        .w_full()
        .h_full()
        .child(ns::dynamic(move || {
            content(state.get(), measurements.clone())
        }))
        .into_element()
}

fn content(snapshot: Snapshot, measurements: mpsc::Sender<Measurement>) -> ns::AnyElement {
    let id = snapshot.id;
    let width = snapshot.width;
    let mut content = ns::view()
        .w(ns::px(width))
        .flex_col()
        .flex_shrink_0()
        .on_layout(move |bounds: &ns::Bounds<ns::Points>| {
            let _ = measurements.try_send(Measurement {
                id: id.clone(),
                width,
                height: bounds.size.height.0,
            });
        });
    if snapshot.input.kind == Kind::Compaction {
        let line_height = snapshot.input.labels[0].line_height;
        let color = snapshot.input.labels[0].color;
        let padding = (Layout::CONVERSATION_TOOL_HEIGHT - line_height) / 2.;
        let icon = ns::svg(nsh_core::element::ImageSrc::svg_markup(
            &String::from_utf8_lossy(crate::icons::TEXT_SELECT_SVG),
        ))
        .size(ns::px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .accessibility_elements_hidden(true)
        .text_color(crate::nsh::color(color));
        return content
            .flex_row()
            .items_start()
            .gap(ns::px(Layout::CONVERSATION_INLINE_GAP))
            .min_h(ns::px(Layout::CONVERSATION_TOOL_HEIGHT))
            .py(ns::px(padding))
            .child(
                ns::view()
                    .w(ns::px(Layout::ICON_SIZE))
                    .h(ns::px(line_height))
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .child(icon),
            )
            .child(
                ns::view().flex_1().min_w(ns::px(0.)).children(
                    snapshot
                        .input
                        .labels
                        .into_iter()
                        .map(|label| native_label(label, false)),
                ),
            )
            .into_element();
    }
    content = match snapshot.input.kind {
        Kind::Empty => content
            .min_h(ns::px(EMPTY_HEIGHT))
            .items_center()
            .gap(ns::px(EMPTY_GAP)),
        Kind::Location => content
            .px(ns::px(LOCATION_PADDING_X))
            .py(ns::px(LOCATION_PADDING_Y)),
        Kind::OmittedThinking(_, _) => content.pt(ns::px(Layout::PROCESS_ITEM_GAP)),
        Kind::ReadOnlyPermission(_) | Kind::Notice | Kind::UserEllipsis | Kind::Compaction => {
            content
        }
        Kind::Backend => content
            .h(ns::px(Layout::TITLEBAR_CONTROL_SIZE))
            .justify_center(),
    };
    let backend = snapshot.input.kind == Kind::Backend;
    content
        .children(snapshot.input.labels.into_iter().map(|label| {
            let hint = label.text.clone();
            let text = native_label(label, snapshot.input.kind == Kind::Empty);
            if backend {
                ns::view()
                    .w_full()
                    .child(text)
                    .tooltip(ns::text(hint))
                    .into_element()
            } else {
                text
            }
        }))
        .into_element()
}

fn native_label(label: Label, centered: bool) -> ns::AnyElement {
    let mut families = vec![label.font.family.to_string()];
    if let Some(fallbacks) = &label.font.fallbacks {
        families.extend(fallbacks.fallback_list().iter().cloned());
    }
    let mut text = ns::text(label.text.clone())
        .label(label.text)
        .w_full()
        .flex_shrink_0()
        .text_size(ns::px(label.size))
        .line_height(ns::px(label.line_height))
        .text_color(crate::nsh::color(label.color))
        .font_weight(ns::FontWeight(label.font.weight.0 as u16))
        .font_family(
            families
                .iter()
                .map(|family| format!("\"{family}\""))
                .collect::<Vec<_>>()
                .join(", "),
        );
    text = if centered {
        text.text_center()
    } else {
        text.text_left()
    };
    if label.font.style != gpui_kit::FontStyle::Normal {
        text = text.italic();
    }
    for (tag, value) in label.font.features.tag_value_list() {
        if *value != 1 {
            continue;
        }
        text = match tag.as_str() {
            "smcp" => text.small_caps(),
            "onum" => text.oldstyle_nums(),
            "lnum" => text.lining_nums(),
            "tnum" => text.tabular_nums(),
            "pnum" => text.proportional_nums(),
            _ => text,
        };
    }
    text.into_element()
}

#[cfg(test)]
mod tests;
