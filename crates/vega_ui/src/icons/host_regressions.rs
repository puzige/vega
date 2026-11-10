use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::{prelude::*, *};
use nsh_core::prelude as ns;

use super::{Icon, IconTooltip};

#[derive(Clone, Default)]
pub(super) struct Observation(Arc<Mutex<Observed>>);

impl PartialEq for Observation {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Default)]
struct Observed {
    surfaces: Vec<EntityId>,
    setups: usize,
    projections: usize,
    native_bounds: Option<[f32; 4]>,
    colors: Vec<Rgba>,
    label: String,
    asset: Option<String>,
    shortcut: Option<String>,
    size: f32,
    bounds: Option<Bounds<Pixels>>,
    input_mask: Option<Bounds<Pixels>>,
    paint_clip: Option<Bounds<Pixels>>,
    paints: usize,
    failure: Option<String>,
}

impl Observation {
    fn read(&self) -> Observed {
        self.0.lock().expect("icon observation lock").clone()
    }

    pub(super) fn setup(&self) {
        self.0.lock().expect("icon observation lock").setups += 1;
    }

    pub(super) fn projection(
        &self,
        colors: &[Rgba],
        size: f32,
        label: &str,
        shortcut: Option<&str>,
    ) {
        let mut observed = self.0.lock().expect("icon observation lock");
        observed.projections += 1;
        observed.colors = colors.to_vec();
        observed.size = size;
        observed.label = label.to_owned();
        observed.shortcut = shortcut.map(str::to_owned);
    }

    pub(super) fn asset(&self, source: &str) {
        self.0.lock().expect("icon observation lock").asset = Some(source.to_owned());
    }

    pub(super) fn layout(&self, bounds: &ns::Bounds<ns::Points>) {
        self.0.lock().expect("icon observation lock").native_bounds = Some([
            bounds.origin.x.0,
            bounds.origin.y.0,
            bounds.size.width.0,
            bounds.size.height.0,
        ]);
    }

    pub(super) fn host(&self, id: EntityId, bounds: Bounds<Pixels>, failure: Option<String>) {
        let mut observed = self.0.lock().expect("icon observation lock");
        if observed.surfaces.last() != Some(&id) {
            observed.surfaces.push(id);
        }
        observed.bounds = Some(bounds);
        observed.failure = failure;
    }

    pub(super) fn input(&self, bounds: Bounds<Pixels>) {
        self.0.lock().expect("icon observation lock").input_mask = Some(bounds);
    }

    pub(super) fn paint(&self, bounds: Bounds<Pixels>) {
        let mut observed = self.0.lock().expect("icon observation lock");
        observed.paint_clip = Some(bounds);
        observed.paints += 1;
    }
}

#[derive(Clone)]
pub(super) struct Observations {
    icon: Observation,
    inline: Observation,
    tooltip: Observation,
}

impl Global for Observations {}

pub(super) fn icon_observation(kind: Icon, cx: &App) -> Option<Observation> {
    let observations = cx.try_global::<Observations>()?;
    match kind {
        Icon::Search => Some(observations.icon.clone()),
        Icon::Pin => Some(observations.inline.clone()),
        _ => None,
    }
}

pub(super) fn tooltip_observation(cx: &App) -> Option<Observation> {
    cx.try_global::<Observations>()
        .map(|state| state.tooltip.clone())
}

struct Root {
    tooltip: Entity<IconTooltip>,
    size: f32,
    foreground: Option<Rgba>,
    offset: f32,
    clip_width: f32,
    opacity: f32,
    focus: FocusHandle,
    clicks: usize,
    wheels: usize,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = vega_theme::theme(cx).colors;
        let foreground = self.foreground.unwrap_or(colors.text_secondary);
        div()
            .id("icon-regression-root")
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("icon-regression-clip")
                    .w(px(self.clip_width))
                    .h(px(64.))
                    .ml(px(self.offset))
                    .overflow_hidden()
                    .opacity(self.opacity)
                    .child(
                        super::icon_button_with_id(
                            super::sized_icon(Icon::Search, foreground, self.size),
                            "regression-search",
                            "Search",
                            colors,
                            cx.listener(|root, _, _, cx| {
                                root.clicks += 1;
                                cx.notify();
                            }),
                        )
                        .track_focus(&self.focus),
                    )
                    .on_scroll_wheel(cx.listener(|root, _, _, cx| {
                        root.wheels += 1;
                        cx.notify();
                    })),
            )
            .child(div().id("inline-icon-slot").child(super::sized_icon(
                Icon::Pin,
                foreground,
                self.size,
            )))
            .child(
                div()
                    .id("tooltip-slot")
                    .w(px(280.))
                    .h(px(72.))
                    .flex()
                    .items_start()
                    .overflow_hidden()
                    .opacity(self.opacity)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            root.clicks += 1;
                            cx.notify();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|root, _, _, cx| {
                        root.wheels += 1;
                        cx.notify();
                    }))
                    .child(self.tooltip.clone()),
            )
    }
}

struct Fixture {
    visual: VisualTestContext,
    root: Entity<Root>,
    observations: Observations,
}

impl Fixture {
    fn new(cx: &mut TestAppContext, shortcut: bool) -> Self {
        let observations = Observations {
            icon: Observation::default(),
            inline: Observation::default(),
            tooltip: Observation::default(),
        };
        cx.update(|cx| {
            assert!(!cx.text_system().all_font_names().is_empty());
            cx.set_global(vega_theme::Theme::dark());
            cx.set_global(observations.clone());
        });
        let window = cx.open_window(size(px(360.), px(240.)), move |_, cx| Root {
            tooltip: cx.new(|_| {
                IconTooltip(
                    "Return to bottom".into(),
                    shortcut.then(|| "Ctrl+End".into()),
                )
            }),
            size: 16.,
            foreground: None,
            offset: 0.,
            clip_width: 24.,
            opacity: 1.,
            focus: cx.focus_handle(),
            clicks: 0,
            wheels: 0,
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let root = visual.update(|window, _| {
            window.activate_window();
            window.root::<Root>().flatten().expect("icon fixture root")
        });
        let mut fixture = Self {
            visual,
            root,
            observations,
        };
        fixture.wait("initial native layout", |fixture| {
            fixture.probes().iter().all(|probe| {
                let observed = probe.read();
                observed.native_bounds.is_some() && observed.projections > 0
            })
        });
        fixture
    }

    fn probes(&self) -> [Observation; 3] {
        [
            self.observations.icon.clone(),
            self.observations.inline.clone(),
            self.observations.tooltip.clone(),
        ]
    }

    fn drain(&mut self) {
        let start = Instant::now();
        for tick in 0..512 {
            let worked = self.visual.cx.background_executor.tick();
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "icon scheduler deadline tick={tick}"
            );
            if !worked {
                return;
            }
        }
        panic!("icon scheduler did not quiesce in 512 ticks");
    }

    fn frame(&mut self) {
        self.drain();
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.drain();
    }

    fn wait(&mut self, stage: &str, mut ready: impl FnMut(&Self) -> bool) {
        let start = Instant::now();
        loop {
            self.frame();
            if ready(self) {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "icon stage: {stage}"
            );
            std::thread::sleep(Duration::from_millis(10));
            self.visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
    }

    fn change(&mut self, update: impl FnOnce(&mut Root, &mut Context<Root>)) {
        self.root.update(&mut self.visual.cx, |root, cx| {
            update(root, cx);
            cx.notify();
        });
    }

    fn click(&mut self, position: Point<Pixels>) {
        self.input(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        self.input(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
        });
    }

    fn input(&mut self, event: impl InputEvent) {
        self.visual.update(|window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
        self.drain();
    }

    fn retained(&self) {
        let probes = self.probes().map(|probe| probe.read());
        for probe in &probes {
            assert_eq!(probe.surfaces.len(), 1, "Surface was recreated");
            assert_eq!(probe.setups, 1, "NSH renderer/root setup repeated");
            assert!(probe.projections > 0);
            assert!(probe.paints > 0);
            assert!(probe.failure.is_none(), "native error: {:?}", probe.failure);
            let mask = probe.input_mask.expect("passive prepaint mask");
            assert_eq!(mask.size, size(px(0.), px(0.)));
        }
        assert_ne!(probes[0].surfaces[0], probes[1].surfaces[0]);
        assert_ne!(probes[0].surfaces[0], probes[2].surfaces[0]);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.visual.update(|window, cx| {
            window.remove_window();
            cx.remove_global::<Observations>();
            cx.set_global(vega_theme::Theme::dark());
        });
    }
}

pub fn shared_icons_retain_native_surface_across_frames_and_theme(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx, false);
    assert_eq!(
        fixture.observations.icon.read().asset,
        super::nsh_source(Icon::Search)
    );
    assert_eq!(
        fixture.observations.inline.read().asset,
        Some(String::from_utf8(super::PIN_SVG.to_vec()).expect("pin SVG UTF-8"))
    );
    let projections = fixture.probes().map(|probe| probe.read().projections);
    for _ in 0..8 {
        fixture.frame();
    }
    fixture.retained();
    assert_eq!(
        fixture.probes().map(|probe| probe.read().projections),
        projections
    );
    let foreground = rgba(0x31a7c480);
    fixture.change(|root, _| {
        root.size = 24.;
        root.foreground = Some(foreground);
    });
    fixture.wait("native size and exact RGBA", |fixture| {
        [&fixture.observations.icon, &fixture.observations.inline]
            .iter()
            .all(|probe| {
                let observed = probe.read();
                observed.size == 24.
                    && observed.colors == [foreground]
                    && observed
                        .native_bounds
                        .is_some_and(|bounds| bounds[2] == 24. && bounds[3] == 24.)
            })
    });
    fixture
        .visual
        .update(|_, cx| cx.set_global(vega_theme::Theme::light()));
    fixture.change(|root, cx| {
        root.foreground = None;
        root.tooltip.update(cx, |_, cx| cx.notify());
    });
    let colors = vega_theme::Theme::light().colors;
    fixture.wait("light theme publication", |fixture| {
        fixture.observations.icon.read().colors == [colors.text_secondary]
            && fixture.observations.tooltip.read().colors.first() == Some(&colors.bg_elevated)
    });
    fixture.retained();
}

pub fn shared_icon_clip_motion_and_opacity_preserve_parent_input(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx, true);
    let old_bounds = fixture
        .observations
        .icon
        .read()
        .bounds
        .expect("icon bounds");
    fixture.change(|root, _| {
        root.offset = 12.;
        root.clip_width = 12.;
        root.opacity = 0.35;
    });
    fixture.frame();
    let observed = fixture.observations.icon.read();
    let bounds = observed.bounds.expect("moved icon bounds");
    let clip = observed.paint_clip.expect("actual icon paint clip");
    assert_eq!(bounds.origin.x - old_bounds.origin.x, px(12.));
    assert!(clip.size.width > px(0.) && clip.size.width < bounds.size.width);
    let position = clip.center();
    fixture.click(position);
    fixture.input(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.), px(-16.))),
        touch_phase: TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    fixture.frame();
    fixture.visual.update(|_, cx| {
        assert_eq!(fixture.root.read(cx).clicks, 1);
        assert_eq!(fixture.root.read(cx).wheels, 1);
    });
    fixture.visual.update(|window, cx| {
        let focus = fixture.root.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    fixture.frame();
    for key in ["enter", "space"] {
        fixture.input(KeyDownEvent {
            keystroke: Keystroke::parse(key).expect("fixture activation key"),
            is_held: false,
            prefer_character_input: false,
        });
    }
    fixture.frame();
    fixture.visual.update(|window, cx| {
        assert_eq!(fixture.root.read(cx).clicks, 3);
        assert!(fixture.root.read(cx).focus.is_focused(window));
    });
    fixture.change(|root, _| {
        root.clip_width = 24.;
        root.opacity = 1.;
    });
    fixture.frame();
    assert_eq!(
        fixture
            .observations
            .icon
            .read()
            .paint_clip
            .expect("expanded clip")
            .size
            .width,
        px(16.)
    );
    fixture.retained();
}

pub fn shared_tooltip_shortcut_toggle_retains_native_layout_and_accessibility(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, false);
    let plain = fixture
        .observations
        .tooltip
        .read()
        .bounds
        .expect("plain tooltip bounds");
    fixture.change(|root, cx| {
        root.tooltip.update(cx, |tooltip, cx| {
            tooltip.1 = Some("Ctrl+End".into());
            cx.notify();
        });
    });
    fixture.wait("shortcut native publication", |fixture| {
        let observed = fixture.observations.tooltip.read();
        observed.shortcut.as_deref() == Some("Ctrl+End")
            && observed
                .bounds
                .zip(observed.native_bounds)
                .is_some_and(|(host, native)| {
                    (f32::from(host.size.width) - native[2]).abs() <= 0.5
                        && (f32::from(host.size.height) - native[3]).abs() <= 0.5
                })
    });
    let with_shortcut = fixture.observations.tooltip.read();
    assert!(with_shortcut.bounds.expect("shortcut bounds").size.width > plain.size.width);
    assert_eq!(with_shortcut.label, "Return to bottom");
    super::tooltip::assert_accessibility("Return to bottom".into(), Some("Ctrl+End".into()));
    let position = with_shortcut.bounds.expect("tooltip bounds").center();
    fixture.click(position);
    fixture.input(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.), px(-16.))),
        touch_phase: TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    fixture.frame();
    fixture.visual.update(|_, cx| {
        assert_eq!(fixture.root.read(cx).clicks, 1);
        assert_eq!(fixture.root.read(cx).wheels, 1);
    });
    fixture.change(|root, cx| {
        root.tooltip.update(cx, |tooltip, cx| {
            tooltip.1 = None;
            cx.notify();
        });
    });
    fixture.wait("shortcut removed from native publication", |fixture| {
        let observed = fixture.observations.tooltip.read();
        observed.shortcut.is_none()
            && observed
                .bounds
                .zip(observed.native_bounds)
                .is_some_and(|(host, native)| {
                    (f32::from(host.size.width) - native[2]).abs() <= 0.5
                        && (f32::from(host.size.height) - native[3]).abs() <= 0.5
                })
    });
    assert_eq!(
        fixture
            .observations
            .tooltip
            .read()
            .bounds
            .expect("plain bounds restored")
            .size,
        plain.size
    );
    super::tooltip::assert_accessibility("Return to bottom".into(), None);
    for _ in 0..8 {
        fixture.frame();
    }
    fixture.retained();
}
