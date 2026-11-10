use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

#[cfg(feature = "nsh-reading-integration")]
pub mod mounted_regressions;

#[derive(Clone, PartialEq)]
struct Input {
    day: String,
    suffix: String,
    accessible: String,
    color: Rgba,
    family: String,
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
    fn accepts(&self, snapshot: &Snapshot) -> bool {
        self.id.matches(&snapshot.id)
            && self.width == snapshot.width
            && self.width.is_finite()
            && self.width > 0.
            && self.height.is_finite()
            && self.height > 0.
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct Separator {
    pub(super) key: String,
    pub(super) day: String,
    pub(super) suffix: String,
    pub(super) accessible: String,
    pub(super) color: Rgba,
}

impl RenderOnce for Separator {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let input = Input {
            day: self.day,
            suffix: self.suffix,
            accessible: self.accessible,
            color: self.color,
            family: window.text_style().font_family.to_string(),
        };
        let host = window.use_keyed_state(self.key, cx, |_, cx| Host {
            snapshot: Snapshot {
                id: ProjectionId::new(),
                input: input.clone(),
                width: 0.,
            },
            height: initial_height(),
            ready: false,
            surface: None,
            _subscriptions: vec![cx.on_release(|host: &mut Host, cx| host.stop(cx))],
        });
        host.update(cx, |host, cx| {
            if host.snapshot.input != input {
                host.snapshot.input = input;
                host.stop(cx);
                cx.notify();
            }
        });
        host
    }
}

fn initial_height() -> f32 {
    Layout::CONVERSATION_STAGE_TOP * 2. + Typography::BODY * Typography::BODY_LINE_HEIGHT
}

struct Host {
    snapshot: Snapshot,
    height: f32,
    ready: bool,
    surface: Option<Surface<Snapshot, Measurement>>,
    _subscriptions: Vec<Subscription>,
}

impl Host {
    fn stop(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.snapshot.id = ProjectionId::new();
        self.height = initial_height();
        self.ready = false;
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.surface.is_none() && self.snapshot.width > 0. {
            let (surface, mut reports) =
                Surface::new_transparent(self.snapshot.clone(), render, window, cx);
            surface.focus_handle(cx).tab_stop(false);
            self.surface = Some(surface);
            cx.spawn_in(window, async move |host, cx| {
                while let Some(report) = reports.recv().await {
                    if host
                        .update_in(cx, |host, _, cx| {
                            if report.accepts(&host.snapshot)
                                && (!host.ready || (host.height - report.height).abs() > 0.5)
                            {
                                host.height = report.height;
                                host.ready = true;
                                cx.notify();
                            }
                        })
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
                .id("nsh-timestamp-label")
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .h(px(self.height))
                .role(Role::Label)
                .aria_label(self.snapshot.input.accessible.clone())
                .children(self.surface.as_ref().map(Surface::entity))
                .into_any_element(),
            width: self.snapshot.width,
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
        Some("nsh-timestamp-measured".into())
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
        #[cfg(feature = "nsh-reading-integration")]
        mounted_regressions::record_host(&self.host, bounds, cx);
        let width = f32::from(bounds.size.width);
        if !width.is_finite() || width <= 0. || self.width != width {
            let _ = self.host.update(cx, |host, cx| {
                let width = if width.is_finite() { width.max(0.) } else { 0. };
                if host.snapshot.width != width {
                    host.snapshot.width = width;
                    host.stop(cx);
                    cx.notify();
                }
            });
            return false;
        }
        let mask = ContentMask {
            bounds: Bounds {
                origin: window.content_mask().bounds.center(),
                size: Default::default(),
            },
        };
        #[cfg(feature = "nsh-reading-integration")]
        mounted_regressions::record_mask(&self.host, mask.bounds, cx);
        window.with_content_mask(Some(mask), |window| self.child.prepaint(window, cx));
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

fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Measurement>) -> ns::AnyElement {
    let commands = crate::nsh::OrderedCommands::new(sender);
    ns::dynamic(move || {
        let snapshot = state.get();
        let report = commands.clone();
        let width = snapshot.width;
        let id = snapshot.id.clone();
        let text = |value: String, weight: gpui_kit::FontWeight| {
            let text = ns::text(value.clone())
                .flex_shrink(1.)
                .font_family(snapshot.input.family.clone())
                .font_weight(ns::FontWeight(weight.0 as u16))
                .text_size(ns::px(Typography::BODY))
                .line_height(ns::px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
                .text_color(crate::nsh::color(snapshot.input.color));
            #[cfg(feature = "nsh-reading-integration")]
            let text = {
                let id = snapshot.id.clone();
                let layout_id = id.clone();
                let label = value.clone();
                text.on(
                    nsh_core::EventKind::TextLayout,
                    move |event: &nsh_core::EventPayload| {
                        if let nsh_core::EventPayload::TextLayout(lines) = event {
                            mounted_regressions::record_lines(&id, &label, weight, lines);
                        }
                    },
                )
                .on_layout(move |bounds: &ns::Bounds<ns::Points>| {
                    mounted_regressions::record_text(&layout_id, &value, bounds);
                })
            };
            text
        };
        ns::view()
            .w(ns::px(width))
            .flex_row()
            .justify_center()
            .items_center()
            .flex_shrink_0()
            .py(ns::px(Layout::CONVERSATION_STAGE_TOP))
            .role(nsh_core::AccessibilityRole::Text)
            .label(snapshot.input.accessible.clone())
            .on_layout(move |bounds: &ns::Bounds<ns::Points>| {
                #[cfg(feature = "nsh-reading-integration")]
                mounted_regressions::record_native(&id, bounds);
                report.send(Measurement {
                    id: id.clone(),
                    width,
                    height: bounds.size.height.0,
                });
            })
            .child(text(snapshot.input.day.clone(), FontWeight::MEDIUM))
            .child(text(snapshot.input.suffix.clone(), FontWeight::NORMAL))
            .into_element()
    })
    .into_element()
}

#[cfg(test)]
mod tests;
