use super::*;
use crate::settings::helpers::{section_title, settings_field, settings_group};
use crate::text_input::TextInput;
use gpui_kit::{
    Entity, Focusable as _, InputEvent, MouseButton, MouseDownEvent, MouseUpEvent, ScrollDelta,
    ScrollHandle, ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext, point, size,
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

const LONG: &str = "Default permission and model settings remain available when the window becomes narrow. Long labels and descriptions must wrap without clipping the next field.";
const SHORT: &str = "Default model";
const LABEL: &str = "Default permission and model configuration for new conversations";

type Rect = Rc<RefCell<Option<Bounds<Pixels>>>>;

struct Probe {
    child: AnyElement,
    bounds: Rect,
}

impl IntoElement for Probe {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Probe {
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
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        *self.bounds.borrow_mut() = Some(bounds);
        self.child.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

fn probe(child: impl IntoElement, bounds: &Rect) -> Probe {
    Probe {
        child: child.into_any_element(),
        bounds: bounds.clone(),
    }
}

struct Root {
    width: f32,
    description: String,
    bold: bool,
    inputs: [Entity<TextInput>; 2],
    group: Rect,
    rows: [Rect; 2],
    controls: [Rect; 2],
    scroll: ScrollHandle,
    wheels: usize,
    clicks: usize,
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = vega_theme::theme(cx).colors;
        let controls = self
            .inputs
            .each_ref()
            .map(|input| TextInput::render_nsh_input(input.clone(), window, cx));
        let [first, second] = controls;
        let first = settings_field(
            "settings-defaults-permission-field",
            LABEL,
            Some(self.description.clone()),
            probe(first, &self.controls[0]),
            colors,
            window,
            cx,
        );
        let second = settings_field(
            "settings-defaults-model-field",
            SHORT,
            None,
            probe(second, &self.controls[1]),
            colors,
            window,
            cx,
        );
        let title = section_title("Defaults", colors.text_primary, window, cx);
        let group = settings_group(colors, window, cx)
            .w(px(self.width))
            .child(title)
            .child(probe(first, &self.rows[0]))
            .child(probe(second, &self.rows[1]));
        div()
            .id("mounted-settings-parent")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|this, _, _, _| this.wheels += 1))
            .on_click(cx.listener(|this, _, _, _| this.clicks += 1))
            .font_weight(if self.bold {
                gpui_kit::FontWeight::BOLD
            } else {
                gpui_kit::FontWeight::NORMAL
            })
            .child(
                div()
                    .relative()
                    .w(px(740.))
                    .h(px(2200.))
                    .child(background(
                        "settings-page-background",
                        colors.bg_base,
                        window,
                        cx,
                    ))
                    .child(background(
                        "settings-content-background",
                        colors.bg_base,
                        window,
                        cx,
                    ))
                    .child(probe(group, &self.group)),
            )
    }
}

struct MeasuredLayout {
    host: Entity<Host>,
    id: ProjectionId,
    width: f32,
    height: f32,
    ready: bool,
}

struct Fixture {
    visual: VisualTestContext,
    layout_current: bool,
    root: Entity<Root>,
    hosts: Rc<RefCell<Vec<WeakEntity<Host>>>>,
    _observer: Subscription,
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        cx.update(|cx| {
            assert!(
                !cx.text_system().all_font_names().is_empty(),
                "actual platform fonts required"
            );
            cx.set_global(vega_theme::Theme::light());
        });
        let hosts = Rc::new(RefCell::new(Vec::new()));
        let observed = hosts.clone();
        let observer = cx.update(|cx| {
            cx.observe_new::<Host>(move |_, _, cx| observed.borrow_mut().push(cx.weak_entity()))
        });
        let inputs = [
            cx.new(|cx| TextInput::new(cx, "First setting", false)),
            cx.new(|cx| TextInput::new(cx, "Second setting", false)),
        ];
        let window = cx.open_window(size(px(760.), px(800.)), move |_, _| Root {
            width: 700.,
            description: LONG.into(),
            bold: false,
            inputs,
            group: Default::default(),
            rows: Default::default(),
            controls: Default::default(),
            scroll: ScrollHandle::new(),
            wheels: 0,
            clicks: 0,
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let root = visual.update(|window, _| {
            window.activate_window();
            window
                .root::<Root>()
                .flatten()
                .expect("settings fixture root")
        });
        let mut this = Self {
            visual,
            layout_current: false,
            root,
            hosts,
            _observer: observer,
        };
        this.ready();
        this
    }

    fn drain(&mut self) {
        let started = Instant::now();
        for tick in 0..512 {
            let worked = self.visual.cx.background_executor.tick();
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "settings scheduler deadline at tick {tick}"
            );
            if !worked {
                return;
            }
        }
        panic!("settings scheduler did not quiesce within 512 ticks");
    }

    fn measured_layout(&mut self) -> Vec<MeasuredLayout> {
        let hosts = self.live();
        self.visual.update(|_, cx| {
            hosts
                .into_iter()
                .filter_map(|host| {
                    let state = host.read(cx);
                    state.input.measured().then(|| MeasuredLayout {
                        host: host.clone(),
                        id: state.id.clone(),
                        width: state.width,
                        height: state.height,
                        ready: state.ready,
                    })
                })
                .collect()
        })
    }

    fn frame(&mut self) {
        self.drain();
        let before = self.measured_layout();
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.drain();
        let after = self.measured_layout();
        self.layout_current = before.len() == after.len()
            && before.iter().zip(&after).all(|(before, after)| {
                before.host.entity_id() == after.host.entity_id()
                    && before.id.matches(&after.id)
                    && before.ready
                    && after.ready
                    && before.width == after.width
                    && before.height == after.height
            });
    }

    fn diagnose_layout(&mut self, stage: &str) {
        let hosts = self.live();
        let current = self.layout_current;
        self.visual.update(|_, cx| {
            let root = self.root.read(cx);
            let labels: Vec<_> = hosts
                .iter()
                .filter_map(|host| {
                    let host = host.read(cx);
                    match &host.input {
                        PixelsInput::Label { text, .. } => {
                            Some((text.clone(), host.width, host.height, host.ready))
                        }
                        _ => None,
                    }
                })
                .collect();
            eprintln!(
                "[settings-layout] stage={stage} drawn_measurements_current={current} group_width={} group={:?} rows={:?} controls={:?} labels(text,width,height,ready)={labels:?}; expected=current measurements drawn, narrow control below natural label height, next row clear",
                root.width,
                root.group.borrow(),
                root.rows.each_ref().map(|bounds| *bounds.borrow()),
                root.controls.each_ref().map(|bounds| *bounds.borrow()),
            );
        });
    }

    fn wait(&mut self, stage: &str, mut ready: impl FnMut(&mut Self) -> bool) {
        let started = Instant::now();
        loop {
            self.frame();
            if ready(self) {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "settings stage timeout: {stage}"
            );
            std::thread::sleep(Duration::from_millis(10));
            self.visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
    }

    fn live(&self) -> Vec<Entity<Host>> {
        self.hosts
            .borrow()
            .iter()
            .filter_map(WeakEntity::upgrade)
            .collect()
    }

    fn ready(&mut self) {
        let mut observations = 0;
        self.wait("production labels and native controls ready", |this| {
            observations += 1;
            if observations <= 3 {
                this.diagnose_layout("readiness observation");
            }
            let hosts = this.live();
            this.layout_current
                && this.visual.update(|_, cx| {
                    let labels = hosts
                        .iter()
                        .filter(|host| host.read(cx).input.measured())
                        .count();
                    hosts.len() >= 9
                        && labels == 4
                        && hosts.iter().all(|host| {
                            let host = host.read(cx);
                            if let Some(surface) = &host.surface {
                                assert!(
                                    surface.errors().borrow().is_none(),
                                    "settings Surface failed"
                                );
                            }
                            host.ready && host.surface.is_some()
                        })
                        && this
                            .root
                            .read(cx)
                            .inputs
                            .iter()
                            .all(|input| input.read(cx).composer_editor_ready())
                })
        });
        self.diagnose_layout("ready: current measurements drawn");
    }

    fn label(&mut self, text: &str) -> Entity<Host> {
        let hosts = self.live();
        self.visual.update(|_, cx| {
            hosts.into_iter().find(|host| matches!(&host.read(cx).input, PixelsInput::Label { text: actual, .. } if actual == text)).expect("actual production label")
        })
    }

    fn mutate(&mut self, apply: impl FnOnce(&mut Root, &mut App)) {
        self.visual.update(|_, cx| {
            self.root.update(cx, |root, cx| {
                apply(root, cx);
                cx.notify();
            })
        });
        self.ready();
    }

    fn input<E: InputEvent>(&mut self, event: E) {
        self.visual.update(|window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
        self.frame();
    }

    fn assert_passive_unfocused(&mut self) {
        let hosts = self.live();
        self.visual.update(|window, cx| {
            for host in hosts {
                assert!(
                    !host
                        .read(cx)
                        .surface
                        .as_ref()
                        .unwrap()
                        .focus_handle(cx)
                        .contains_focused(window, cx),
                    "passive settings Surface acquired focus"
                );
            }
        });
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let hosts = self.live();
        let window = self.visual.window_handle();
        let cleanup = self.visual.cx.update_window(window, |_, window, cx| {
            for host in hosts {
                if let Some(surface) = &host.read(cx).surface {
                    surface.entity().read(cx).stop();
                }
            }
            let inputs = self.root.read(cx).inputs.clone();
            for input in inputs {
                input.update(cx, |input, cx| input.suspend_nsh_composer(cx));
            }
            window.remove_window();
        });
        if let Err(error) = cleanup {
            if std::thread::panicking() {
                eprintln!("settings fixture cleanup during unwind: {error:#}");
            } else {
                panic!("settings fixture cleanup: {error:#}");
            }
        }
    }
}

pub fn narrow_fields_wrap_labels_and_preserve_natural_height(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let label = fixture.label(LABEL);
    let description = fixture.label(LONG);
    fixture.visual.update(|_, cx| {
        let host = label.read(cx);
        assert!(
            host.height > host.input.initial_height() * 1.5,
            "production field label must wrap"
        );
        let root = fixture.root.read(cx);
        assert!(
            root.controls[0].borrow().unwrap().left()
                > root.rows[0].borrow().unwrap().left() + px(Layout::SETTINGS_LABEL_WIDTH)
        );
    });
    let wide_height = fixture.visual.update(|_, cx| description.read(cx).height);
    fixture.mutate(|root, _| root.width = 180.);
    fixture.visual.update(|_, cx| {
        let host = description.read(cx);
        assert!(host.width < Layout::SETTINGS_LABEL_WIDTH);
        assert!(
            host.height > wide_height,
            "narrow label must grow naturally"
        );
        let root = fixture.root.read(cx);
        let row = root.rows[0].borrow().unwrap();
        let next = root.rows[1].borrow().unwrap();
        let control = root.controls[0].borrow().unwrap();
        assert!(
            control.top() >= row.top() + px(host.height),
            "control must wrap below natural label height"
        );
        assert!(control.right() <= row.right() + px(0.5));
        assert!(
            next.top() >= row.bottom() - px(0.5),
            "natural row height must keep the next setting clear"
        );
        assert!(row.bottom() >= control.bottom());
    });
    fixture.mutate(|root, _| root.width = 700.);
    fixture.visual.update(|_, cx| {
        assert!(
            (description.read(cx).height - wide_height).abs() <= 0.5,
            "restored width must not retain narrow height"
        )
    });
}

pub fn changed_content_font_theme_and_width_reject_retired_measurements(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let label = fixture.label(LONG);
    for change in 0..4 {
        let (id, width, height, old_surface) = fixture.visual.update(|_, cx| {
            let host = label.read(cx);
            (
                host.id.clone(),
                host.width,
                host.height,
                host.surface.as_ref().unwrap().entity(),
            )
        });
        fixture.mutate(|root, cx| match change {
            0 => root.description = SHORT.into(),
            1 => root.bold = true,
            2 => cx.set_global(vega_theme::Theme::dark()),
            _ => root.width = 180.,
        });
        fixture.visual.update(|_, cx| {
            label.update(cx, |host, _| {
                assert!(
                    !id.matches(&host.id),
                    "changed rendering input must retire projection"
                );
                assert_ne!(
                    old_surface.entity_id(),
                    host.surface.as_ref().unwrap().entity().entity_id()
                );
                assert!(host.ready);
                let current = host.height;
                assert!(!host.apply_measurement(Measurement {
                    id,
                    width,
                    height: height + 1000.
                }));
                assert_eq!(
                    host.height, current,
                    "retired report changed current measured height"
                );
                assert!(!host.apply_measurement(Measurement {
                    id: host.id.clone(),
                    width: host.width + 1.,
                    height: current + 1000.
                }));
                for invalid in [f32::NAN, f32::INFINITY, 0., -1.] {
                    assert!(!host.apply_measurement(Measurement {
                        id: host.id.clone(),
                        width: host.width,
                        height: invalid
                    }));
                    assert_eq!(host.height, current);
                }
            })
        });
        if change == 0 {
            fixture.visual.update(|_, cx| {
                assert!(
                    label.read(cx).height < height,
                    "shorter content must shed the old natural height"
                )
            });
        }
    }
}

pub fn passive_pixels_forward_parent_input_once_and_skip_cross_surface_focus(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx);
    let inputs = fixture
        .visual
        .update(|_, cx| fixture.root.read(cx).inputs.clone());
    fixture
        .visual
        .update(|window, cx| window.focus(&inputs[0].focus_handle(cx), cx));
    fixture.wait("first native control focus", |this| {
        this.visual.update(|window, cx| {
            inputs[0].read(cx).composer_contains_focus(window, cx)
                && !inputs[0].focus_handle(cx).is_focused(window)
        })
    });
    let focus = fixture.visual.update(|window, cx| {
        assert!(inputs[0].read(cx).composer_editor_ready());
        window.focused(cx).expect("native control focus")
    });
    for target in 0..3 {
        let position = fixture.visual.update(|_, cx| {
            let root = fixture.root.read(cx);
            match target {
                0 => {
                    let row = root.rows[0].borrow().unwrap();
                    row.origin + point(px(20.), px(Layout::SETTINGS_ROW_PADDING_Y + 10.))
                }
                1 => {
                    let group = root.group.borrow().unwrap();
                    point(group.left() + px(4.), group.top() + px(100.))
                }
                _ => point(px(720.), px(700.)),
            }
        });
        fixture.input(MouseDownEvent {
            button: MouseButton::Left,
            position,
            click_count: 1,
            ..Default::default()
        });
        fixture.input(MouseUpEvent {
            button: MouseButton::Left,
            position,
            click_count: 1,
            ..Default::default()
        });
        fixture.visual.update(|window, cx| {
            assert_eq!(
                fixture.root.read(cx).clicks,
                target + 1,
                "parent receives exactly one passive click"
            );
            assert!(focus.is_focused(window), "passive click stole native focus");
        });
        let before = fixture
            .visual
            .update(|_, cx| fixture.root.read(cx).scroll.offset().y);
        fixture.input(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(-12.))),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        });
        fixture.visual.update(|_, cx| {
            let root = fixture.root.read(cx);
            assert_eq!(
                root.wheels,
                target + 1,
                "parent receives exactly one passive wheel event"
            );
            assert!(
                (f32::from(root.scroll.offset().y - before) + 12.).abs() <= 0.5,
                "parent must consume wheel exactly once"
            );
        });
        fixture.assert_passive_unfocused();
    }
    fixture.visual.update(|window, cx| window.focus_next(cx));
    fixture.wait(
        "Tab skips passive Surfaces to second native control",
        |this| {
            this.visual.update(|window, cx| {
                inputs[1].read(cx).composer_contains_focus(window, cx)
                    && !inputs[1].focus_handle(cx).is_focused(window)
            })
        },
    );
    fixture.assert_passive_unfocused();
    fixture.visual.update(|window, cx| window.focus_prev(cx));
    fixture.wait("reverse Tab returns to first native control", |this| {
        this.visual.update(|window, cx| {
            inputs[0].read(cx).composer_contains_focus(window, cx)
                && !inputs[0].focus_handle(cx).is_focused(window)
        })
    });
    fixture.assert_passive_unfocused();
    fixture.visual.update(|_, cx| {
        assert_eq!(inputs[0].read(cx).text(), "");
        assert_eq!(inputs[1].read(cx).text(), "");
    });
}
