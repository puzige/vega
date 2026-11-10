use super::{display_layout, frame_colors, should_animate};
use crate::nsh::Surface;
use gpui_kit::{
    AnyElement, App, Bounds, ContentMask, Context, Element, ElementId, Font, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, LineLayout, Pixels, Render, RenderOnce, Rgba,
    Subscription, TextRun, WeakEntity, Window, div, prelude::*, px,
};
use ns::{Accessible as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use std::{sync::Arc, time::Instant};
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

#[derive(Clone, PartialEq)]
struct Input {
    text: String,
    epoch: Option<Instant>,
    base: Rgba,
    highlight: Rgba,
    font: Font,
}

#[derive(Clone, PartialEq)]
struct Snapshot {
    shown: String,
    width: f32,
    label: String,
    colors: Vec<Rgba>,
    font: Font,
}

impl Snapshot {
    fn empty(input: &Input) -> Self {
        Self {
            shown: String::new(),
            width: 0.,
            label: input.text.clone(),
            colors: Vec::new(),
            font: input.font.clone(),
        }
    }
}

pub(super) fn element(
    text: String,
    epoch: Option<Instant>,
    base: Rgba,
    highlight: Rgba,
    window: &Window,
) -> AnyElement {
    NativeRenderOnce(Input {
        text,
        epoch,
        base,
        highlight,
        font: window.text_style().font(),
    })
    .into_any_element()
}

#[derive(gpui_kit::IntoElement)]
struct NativeRenderOnce(Input);

#[derive(Default)]
struct FrameClock {
    sampled: Option<Instant>,
    pending: bool,
}

impl FrameClock {
    fn phase(&self, epoch: Option<Instant>) -> f32 {
        self.sampled.zip(epoch).map_or(0., |(now, epoch)| {
            now.saturating_duration_since(epoch).as_secs_f32() * 1000.
                / vega_theme::Motion::THINKING_SHIMMER_MS as f32
        })
    }

    fn request(&mut self) -> bool {
        if self.pending {
            false
        } else {
            self.pending = true;
            true
        }
    }

    fn delivered(&mut self, now: Instant) -> bool {
        if !self.pending {
            return false;
        }
        self.pending = false;
        self.sampled = Some(now);
        true
    }
}

struct Host {
    input: Input,
    clock: FrameClock,
    surface: Option<Surface<Snapshot, ()>>,
    published: Option<Snapshot>,
    _subscriptions: Vec<Subscription>,
}

impl RenderOnce for NativeRenderOnce {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let host = window.use_keyed_state("nsh-stage-shimmer", cx, |window, cx| Host {
            input: self.0.clone(),
            clock: FrameClock::default(),
            surface: None,
            published: None,
            _subscriptions: vec![
                cx.on_release(|host: &mut Host, cx| {
                    if let Some(surface) = host.surface.take() {
                        surface.entity().read(cx).stop();
                    }
                }),
                cx.observe_window_activation(window, |_, _, cx| cx.notify()),
                cx.observe_window_visibility(window, |_, _, _, cx| cx.notify()),
            ],
        });
        host.update(cx, |host, cx| {
            if host.input != self.0 {
                host.input = self.0;
                let empty = Snapshot::empty(&host.input);
                if let Some(surface) = &host.surface {
                    surface.publish(empty.clone());
                }
                host.published = Some(empty);
                cx.notify();
            }
        });
        host
    }
}

fn shape(text: &str, font: &Font, window: &Window) -> Arc<LineLayout> {
    window.text_system().layout_line(
        text,
        px(Typography::CONVERSATION),
        &[TextRun {
            len: text.len(),
            font: font.clone(),
            color: gpui_kit::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let full = shape(&self.input.text, &self.input.font, window);
        if self.surface.is_none() {
            let initial = Snapshot::empty(&self.input);
            let (surface, _) = Surface::new_transparent(initial.clone(), render, window, cx);
            let _ = surface.focus_handle(cx).tab_stop(false);
            self.published = Some(initial);
            self.surface = Some(surface);
        }
        let Some(surface) = self.surface.as_mut() else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        Measured {
            child: div()
                .id("thinking-shimmer")
                .w(full.width)
                .min_w_0()
                .max_w_full()
                .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
                .overflow_hidden()
                .aria_label(self.input.text.clone())
                .child(surface.entity())
                .into_any_element(),
            host: cx.weak_entity(),
            input: self.input.clone(),
            phase: self.clock.phase(self.input.epoch),
            full,
        }
        .into_any_element()
    }
}

struct Measured {
    child: AnyElement,
    host: WeakEntity<Host>,
    input: Input,
    phase: f32,
    full: Arc<LineLayout>,
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
        Some("nsh-shimmer-measured".into())
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
        let visible =
            window.is_visible() && !bounds.intersect(&window.content_mask().bounds).is_empty();
        let display = display_layout(
            &self.input.text,
            self.full.clone(),
            bounds.size.width,
            |candidate| shape(candidate, &self.input.font, window),
        );
        let moving = self.input.epoch.is_some()
            && should_animate(
                display.as_ref().map(|(_, layout)| layout.as_ref()),
                visible,
                window.is_window_active(),
                cx.reduce_motion(),
            );
        let mut snapshot = Snapshot::empty(&self.input);
        if let Some((shown, layout)) = display {
            snapshot.colors = frame_colors(
                &shown,
                &layout,
                self.full.width,
                self.phase,
                self.input.base,
                self.input.highlight,
                moving,
            );
            snapshot.width = f32::from(layout.width) + 1.;
            snapshot.shown = shown;
        }
        let _ = self.host.update(cx, |host, _| {
            if host.published.as_ref() != Some(&snapshot) {
                if let Some(surface) = &host.surface {
                    surface.publish(snapshot.clone());
                }
                host.published = Some(snapshot);
            }
        });
        let input_mask = ContentMask {
            bounds: Bounds {
                origin: window.content_mask().bounds.center(),
                size: Default::default(),
            },
        };
        window.with_content_mask(Some(input_mask), |window| {
            self.child.prepaint(window, cx);
        });
        moving
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        moving: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let clipped = bounds.intersect(&window.content_mask().bounds);
        window.with_content_mask(Some(ContentMask { bounds: clipped }), |window| {
            self.child.paint(window, cx);
        });
        if *moving
            && self
                .host
                .update(cx, |host, _| host.clock.request())
                .unwrap_or(false)
        {
            let host = self.host.clone();
            window.on_next_frame(move |_, cx| {
                let _ = host.update(cx, |host, cx| {
                    if host.clock.delivered(Instant::now()) {
                        cx.notify();
                    }
                });
            });
        }
    }
}

fn render(state: ns::Signal<Snapshot>, _: mpsc::Sender<()>) -> ns::AnyElement {
    ns::view()
        .w_full()
        .h(ns::px(Layout::CONVERSATION_TOOL_HEIGHT))
        .overflow_hidden()
        .label(move || state.get().label)
        .child(ns::dynamic(move || label(state.get())))
        .into_element()
}

fn label(snapshot: Snapshot) -> ns::AnyElement {
    let mut text = ns::text("")
        .w(ns::px(snapshot.width))
        .flex_shrink_0()
        .text_size(ns::px(Typography::CONVERSATION))
        .line_height(ns::px(Layout::CONVERSATION_TOOL_HEIGHT))
        .font_weight(ns::FontWeight(snapshot.font.weight.0 as u16))
        .number_of_lines(1u32)
        .text_left()
        .text_overflow(nsh_core::EllipsizeMode::Clip);
    let mut families = vec![snapshot.font.family.to_string()];
    if let Some(fallbacks) = &snapshot.font.fallbacks {
        families.extend(fallbacks.fallback_list().iter().cloned());
    }
    text = text.font_family(
        families
            .iter()
            .map(|family| format!("\"{family}\""))
            .collect::<Vec<_>>()
            .join(", "),
    );
    if snapshot.font.style != gpui_kit::FontStyle::Normal {
        text = text.italic();
    }
    for (tag, value) in snapshot.font.features.tag_value_list() {
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
    text.children(
        snapshot
            .shown
            .chars()
            .zip(snapshot.colors)
            .map(|(ch, color)| ns::text(ch.to_string()).text_color(crate::nsh::color(color))),
    )
    .into_element()
}

#[cfg(test)]
mod tests;
