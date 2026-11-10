use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SHORT: &str = "User · Ready";
const LONG: &str = "Assistant · A long message preview preserves its complete accessible name while the visible text wraps and is limited to three lines. This final sentence must not fit into the narrow preview.";
const MIXED: &str = "用户 · 中文消息定位与预览保持完整标签 🧑🏽‍💻🌏🚀。窄窗口中的多行文本必须自然换行并在第三行截断，不改变消息内容和导航身份。继续阅读更多内容以验证真实平台字体与表情符号布局。";

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
    stops: usize,
    content: Option<Content>,
    font: Option<(String, u16, bool, f32)>,
    chrome: Option<(f32, f32)>,
    shadows: Vec<ns::BoxShadow>,
    native: Option<[f32; 4]>,
    text_bounds: Option<[f32; 4]>,
    lines: Vec<nsh_core::TextLine>,
    lines_label: String,
    bounds: Option<Bounds<Pixels>>,
    input_mask: Option<Bounds<Pixels>>,
    clip: Option<Bounds<Pixels>>,
    paints: usize,
    failure: Option<String>,
}

fn rect(bounds: &ns::Bounds<ns::Points>) -> [f32; 4] {
    [
        bounds.origin.x.0,
        bounds.origin.y.0,
        bounds.size.width.0,
        bounds.size.height.0,
    ]
}

impl Observation {
    fn read(&self) -> Observed {
        self.0.lock().expect("anchor observation lock").clone()
    }

    pub(super) fn setup(&self) {
        self.0.lock().expect("anchor observation lock").setups += 1;
    }

    pub(super) fn stopped(&self) {
        self.0.lock().expect("anchor observation lock").stops += 1;
    }

    pub(super) fn projection(&self, snapshot: &Snapshot) {
        let mut state = self.0.lock().expect("anchor observation lock");
        state.projections += 1;
        state.content = Some(snapshot.content.clone());
        state.font = Some((
            snapshot.family.clone(),
            snapshot.weight,
            snapshot.italic,
            snapshot.line_height,
        ));
        state.chrome = Some((snapshot.radius, snapshot.padding));
    }

    pub(super) fn shadows(&self, shadows: &[ns::BoxShadow]) {
        self.0.lock().expect("anchor observation lock").shadows = shadows.to_vec();
    }

    pub(super) fn layout(&self, bounds: &ns::Bounds<ns::Points>) {
        self.0.lock().expect("anchor observation lock").native = Some(rect(bounds));
    }

    pub(super) fn text_layout(&self, bounds: &ns::Bounds<ns::Points>) {
        self.0.lock().expect("anchor observation lock").text_bounds = Some(rect(bounds));
    }

    pub(super) fn lines(&self, label: &str, lines: &[nsh_core::TextLine]) {
        let mut state = self.0.lock().expect("anchor observation lock");
        state.lines_label = label.to_owned();
        state.lines = lines.to_vec();
    }

    pub(super) fn host(&self, id: EntityId, bounds: Bounds<Pixels>, failure: Option<String>) {
        let mut state = self.0.lock().expect("anchor observation lock");
        if state.surfaces.last() != Some(&id) {
            state.surfaces.push(id);
        }
        state.bounds = Some(bounds);
        state.failure = failure;
    }

    pub(super) fn input(&self, mask: Bounds<Pixels>) {
        self.0.lock().expect("anchor observation lock").input_mask = Some(mask);
    }

    pub(super) fn paint(&self, clip: Bounds<Pixels>) {
        let mut state = self.0.lock().expect("anchor observation lock");
        state.clip = Some(clip);
        state.paints += 1;
    }
}

#[derive(Clone)]
struct Observations {
    mark: Observation,
    preview: Observation,
}

impl Global for Observations {}

pub(super) fn observation(content: &Content, cx: &App) -> Option<Observation> {
    cx.try_global::<Observations>().map(|state| match content {
        Content::Mark { .. } => state.mark.clone(),
        Content::Preview { .. } => state.preview.clone(),
    })
}

struct Root {
    width: f32,
    clip_width: f32,
    offset: f32,
    label: String,
    selected: bool,
    visible: bool,
    weight: FontWeight,
    italic: bool,
    line_height: f32,
    family: SharedString,
    focus: [FocusHandle; 2],
    clicks: usize,
    wheels: usize,
    wheel_y: Pixels,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = vega_theme::theme(cx).colors;
        let mark = div()
            .id("mounted-anchor-mark-parent")
            .role(Role::Button)
            .aria_label("Message position")
            .w(px(28.))
            .h(px(Layout::CONVERSATION_MARKER_PITCH))
            .flex()
            .items_center()
            .justify_center()
            .child(super::mark(self.selected, colors));
        let preview = div()
            .id("mounted-anchor-preview-parent")
            .role(Role::Tooltip)
            .aria_label(self.label.clone())
            .w(px(self.width))
            .flex_shrink_0()
            .child(super::preview(self.label.clone(), colors));
        let mut content = div()
            .id("mounted-anchor-content")
            .flex()
            .flex_col()
            .items_start()
            .w(px(self.width))
            .font_family(self.family.clone())
            .font_weight(self.weight)
            .line_height(px(self.line_height));
        if self.italic {
            content = content.italic();
        }
        let content = content.when(self.visible, |content| content.child(mark).child(preview));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("anchor-focus-first")
                    .role(Role::Button)
                    .aria_label("Before")
                    .tab_stop(true)
                    .track_focus(&self.focus[0])
                    .w(px(24.))
                    .h(px(24.)),
            )
            .child(
                div()
                    .id("mounted-anchor-clip")
                    .ml(px(self.offset))
                    .w(px(self.clip_width))
                    .h(px(140.))
                    .overflow_hidden()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|root, _, _, cx| {
                            root.clicks += 1;
                            cx.notify();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|root, event: &ScrollWheelEvent, _, cx| {
                        root.wheels += 1;
                        root.wheel_y += event.delta.pixel_delta(px(16.)).y;
                        cx.notify();
                    }))
                    .child(content),
            )
            .child(
                div()
                    .id("anchor-focus-second")
                    .role(Role::Button)
                    .aria_label("After")
                    .tab_stop(true)
                    .track_focus(&self.focus[1])
                    .w(px(24.))
                    .h(px(24.)),
            )
    }
}

struct Fixture {
    visual: VisualTestContext,
    root: Entity<Root>,
    observations: Observations,
    hosts: Rc<RefCell<Vec<WeakEntity<Host>>>>,
    _observer: Subscription,
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        let observations = Observations {
            mark: Observation::default(),
            preview: Observation::default(),
        };
        let family = cx.update(|cx| {
            assert!(
                !cx.text_system().all_font_names().is_empty(),
                "actual platform fonts required"
            );
            cx.set_global(vega_theme::Theme::dark());
            cx.set_global(observations.clone());
            TextStyle::default().font_family
        });
        let hosts = Rc::new(RefCell::new(Vec::new()));
        let observed = hosts.clone();
        let observer = cx.update(|cx| {
            cx.observe_new::<Host>(move |_, _, cx| observed.borrow_mut().push(cx.weak_entity()))
        });
        let window = cx.open_window(size(px(400.), px(280.)), move |_, cx| Root {
            width: 280.,
            clip_width: 320.,
            offset: 0.,
            label: SHORT.into(),
            selected: false,
            visible: true,
            weight: FontWeight::NORMAL,
            italic: false,
            line_height: 18.,
            family,
            focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            clicks: 0,
            wheels: 0,
            wheel_y: px(0.),
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let root = visual.update(|window, _| {
            window.activate_window();
            window
                .root::<Root>()
                .flatten()
                .expect("anchor fixture root")
        });
        let mut this = Self {
            visual,
            root,
            observations,
            hosts,
            _observer: observer,
        };
        this.ready(SHORT);
        this
    }

    fn probes(&self) -> [Observation; 2] {
        [
            self.observations.mark.clone(),
            self.observations.preview.clone(),
        ]
    }

    fn drain(&mut self) {
        let started = Instant::now();
        for tick in 0..512 {
            let worked = self.visual.cx.background_executor.tick();
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "anchor scheduler deadline tick={tick}"
            );
            if !worked {
                return;
            }
        }
        panic!("anchor scheduler did not quiesce within 512 ticks");
    }

    fn frame(&mut self) {
        self.drain();
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.drain();
    }

    fn diagnostics(&self) -> String {
        self.probes()
            .into_iter()
            .enumerate()
            .map(|(index, probe)| {
                let state = probe.read();
                format!(
                    "leaf={index} surfaces={:?} setups={} projections={} stops={} paints={} content={:?} font={:?} chrome={:?} host={:?} native={:?} text={:?} lines_label={:?} line_count={} lines={:?} input={:?} clip={:?} failure={:?}",
                    state.surfaces,
                    state.setups,
                    state.projections,
                    state.stops,
                    state.paints,
                    state.content,
                    state.font,
                    state.chrome,
                    state.bounds,
                    state.native,
                    state.text_bounds,
                    state.lines_label,
                    state.lines.len(),
                    state.lines.iter().take(3).collect::<Vec<_>>(),
                    state.input_mask,
                    state.clip,
                    state.failure,
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
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
                "anchor stage timeout: {stage}; {}",
                self.diagnostics()
            );
            std::thread::sleep(Duration::from_millis(10));
            self.visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
    }

    fn ready(&mut self, label: &str) {
        self.wait(
            &format!("actual native layout and text lines for {label:?}"),
            |this| {
                let preview = this.observations.preview.read();
                preview.lines_label == label
                    && !preview.lines.is_empty()
                    && this.probes().iter().all(|probe| {
                        let state = probe.read();
                        assert!(state.failure.is_none(), "native error: {:?}", state.failure);
                        state
                            .bounds
                            .zip(state.native)
                            .is_some_and(|(bounds, native)| {
                                (f32::from(bounds.size.width) - native[2]).abs() <= 0.5
                                    && (f32::from(bounds.size.height) - native[3]).abs() <= 0.5
                            })
                    })
            },
        );
        self.frame();
    }

    fn change(&mut self, change: impl FnOnce(&mut Root)) {
        {
            let mut state = self
                .observations
                .preview
                .0
                .lock()
                .expect("anchor observation lock");
            state.lines.clear();
            state.lines_label.clear();
        }
        self.root.update(&mut self.visual.cx, |root, cx| {
            change(root);
            cx.notify();
        });
    }

    fn live(&self) -> Vec<Entity<Host>> {
        self.hosts
            .borrow()
            .iter()
            .filter_map(WeakEntity::upgrade)
            .collect()
    }

    fn retained(&mut self) {
        assert_eq!(self.live().len(), 2);
        for probe in self.probes() {
            let state = probe.read();
            assert_eq!(state.surfaces.len(), 1, "leaf Surface recreated");
            assert_eq!(state.setups, 1, "native renderer setup repeated");
            assert_eq!(state.stops, 0);
            assert!(state.projections > 0 && state.paints > 0);
            assert!(state.failure.is_none());
            assert_eq!(
                state.input_mask.expect("passive mask").size,
                size(px(0.), px(0.))
            );
        }
        let hosts = self.live();
        self.visual.update(|window, cx| {
            for host in hosts {
                assert!(!host.read(cx).surface.focus_handle(cx).is_focused(window));
            }
        });
    }

    fn assert_focus(&mut self, expected: usize, stage: &str) {
        let hosts = self.live();
        self.visual.update(|window, cx| {
            let focus = &self.root.read(cx).focus;
            assert!(
                focus[expected].is_focused(window),
                "{stage} did not reach fixture focus[{expected}]; actual={:?} fixture={focus:?} fixture_tab_stops={:?} surfaces={:?}",
                window.focused(cx),
                focus.each_ref().map(|handle| handle.tab_stop),
                hosts
                    .iter()
                    .take(2)
                    .map(|host| {
                        let handle = host.read(cx).surface.focus_handle(cx);
                        let tab_stop = handle.tab_stop;
                        let focused = handle.is_focused(window);
                        (host.entity_id(), handle, tab_stop, focused)
                    })
                    .collect::<Vec<_>>()
            );
        });
    }

    fn input(&mut self, event: impl InputEvent) {
        self.visual.update(|window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
        self.drain();
    }

    fn click(&mut self, position: Point<Pixels>) {
        self.input(MouseDownEvent {
            position,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        });
        self.input(MouseUpEvent {
            position,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        });
    }

    fn assert_production_accessible_label(&mut self, label: &str) {
        let label = label.to_owned();
        self.visual.update(|_, cx| {
            let thread = vega_conversation::threads::draft_thread(None, "fixture", "confirm");
            let stream = cx.new(|cx| super::super::super::ConversationStream::new(thread, cx));
            stream.update(cx, |_, cx| {
                let mut element = super::super::render_message_anchor_preview(
                    "mounted-anchor".into(),
                    label.clone(),
                    132.,
                    vega_theme::theme(cx).colors,
                    cx,
                );
                let host = element
                    .downcast_mut::<Stateful<Div>>()
                    .expect("actual production preview wrapper");
                let mut node = accesskit::Node::new(Role::GenericContainer);
                host.write_a11y_info(&mut node);
                assert_eq!(
                    node.label(),
                    Some(label.as_str()),
                    "full production AX label was truncated"
                );
            });
        });
    }

    fn assert_mark_quad(&mut self, width: f32, color: Rgba) {
        self.visual.update(|window, _| {
            let scale = window.scale_factor();
            assert!(
                window.painted_quads().iter().any(|quad| {
                    (quad.bounds.size.width.as_f32() - width * scale).abs() <= 0.5
                        && (quad.bounds.size.height.as_f32() - 2. * scale).abs() <= 0.5
                        && quad.background.as_solid().is_some_and(|actual| {
                            let actual = actual.to_rgb();
                            (actual.r - color.r).abs() <= 0.001
                                && (actual.g - color.g).abs() <= 0.001
                                && (actual.b - color.b).abs() <= 0.001
                                && (actual.a - color.a).abs() <= 0.001
                        })
                }),
                "native mark did not emit the expected painted quad"
            );
        });
    }

    fn assert_text_fits(&self, count: usize) {
        let state = self.observations.preview.read();
        let bounds = state.bounds.expect("preview host");
        let text = state.text_bounds.expect("native text frame");
        assert_eq!(state.lines.len(), count, "real native line count");
        assert!(bounds.size.height > px(0.) && bounds.size.height <= px(72.));
        let bottom = state
            .lines
            .iter()
            .map(|line| line.y.0 + line.height.0)
            .fold(0., f32::max);
        assert!(
            bottom <= text[3] + 0.5,
            "native lines clipped by allocated text frame: bottom={bottom} frame={text:?}"
        );
        assert!(
            text[1] + text[3] <= f32::from(bounds.size.height) - 9. + 0.5,
            "text overlaps bottom padding/border"
        );
        assert!((text[2] - (f32::from(bounds.size.width) - 18.)).abs() <= 0.5);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let hosts = self.live();
        let window = self.visual.window_handle();
        let cleanup = self.visual.cx.update_window(window, |_, window, cx| {
            for host in hosts {
                host.read(cx).surface.entity().read(cx).stop();
            }
            window.remove_window();
        });
        if let Err(error) = cleanup {
            if std::thread::panicking() {
                eprintln!("anchor fixture cleanup during unwind: {error:#}");
            } else {
                panic!("anchor fixture cleanup: {error:#}");
            }
        }
    }
}

pub fn short_preview_uses_actual_natural_height_and_original_shadow(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    fixture.assert_text_fits(1);
    let state = fixture.observations.preview.read();
    assert_eq!(state.lines[0].text, SHORT);
    assert!((f32::from(state.bounds.expect("short host").size.height) - 36.).abs() <= 0.5);
    assert_eq!(state.chrome, Some((6., 8.)));
    let font = state.font.expect("captured font");
    assert!(!font.0.is_empty());
    assert_eq!((font.1, font.2, font.3), (400, false, 18.));
    assert_eq!(state.shadows.len(), 2);
    for (shadow, blur, spread) in [(&state.shadows[0], 6., 0.), (&state.shadows[1], 4., -1.)] {
        assert_eq!(
            (
                shadow.offset_x.0,
                shadow.offset_y.0,
                shadow.blur.0,
                shadow.spread.0
            ),
            (0., 1., blur, spread)
        );
        assert_eq!(
            shadow.color,
            crate::nsh::color(hsla(0., 0., 0., 0.1).into()).into()
        );
        assert!(!shadow.inset);
    }
    fixture.assert_production_accessible_label(SHORT);
    fixture.retained();
}

pub fn narrow_long_cjk_and_emoji_preview_clamps_three_native_lines(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let short_height = fixture
        .observations
        .preview
        .read()
        .bounds
        .expect("short host")
        .size
        .height;
    for label in [LONG, MIXED] {
        fixture.change(|root| {
            root.width = 132.;
            root.label = label.into();
        });
        fixture.ready(label);
        fixture.assert_text_fits(3);
        fixture.assert_production_accessible_label(label);
        let state = fixture.observations.preview.read();
        let bounds = state.bounds.expect("narrow host");
        assert_eq!(bounds.size.width, px(132.));
        assert!(bounds.size.height > short_height);
        assert!((f32::from(bounds.size.height) - 72.).abs() <= 0.5);
        assert!(
            state.lines.last().expect("last visible native line").end
                < label.encode_utf16().count() as u32,
            "native layout did not truncate content"
        );
        assert_eq!(
            state.content,
            Some(Content::Preview {
                label: label.into(),
                colors: {
                    let colors = vega_theme::Theme::dark().colors;
                    [
                        colors.bg_elevated,
                        colors.border_subtle,
                        colors.text_primary,
                    ]
                }
            })
        );
    }
    fixture.change(|root| {
        root.label = SHORT.into();
        root.width = 280.;
    });
    fixture.ready(SHORT);
    fixture.assert_text_fits(1);
    assert_eq!(
        fixture
            .observations
            .preview
            .read()
            .bounds
            .expect("restored short host")
            .size
            .height,
        short_height
    );
    fixture.retained();
}

pub fn mark_states_keep_native_dimensions_and_semantic_colors(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    for selected in [false, true, false] {
        fixture.change(|root| root.selected = selected);
        let width = if selected { 16. } else { 6. };
        let colors = vega_theme::Theme::dark().colors;
        let color = if selected {
            colors.text_secondary
        } else {
            colors.conversation_marker
        };
        fixture.wait("selected/rest native mark", |this| {
            let state = this.observations.mark.read();
            state.content == Some(Content::Mark { width, color })
                && state
                    .native
                    .is_some_and(|rect| rect[2] == width && rect[3] == 2.)
        });
        let state = fixture.observations.mark.read();
        assert_eq!(
            state.bounds.expect("mark host").size,
            size(px(width), px(2.))
        );
        assert_eq!(state.bounds.expect("mark host").center().x, px(14.));
        fixture.assert_mark_quad(width, color);
    }
    fixture.retained();
}

pub fn stable_frames_and_changed_font_theme_content_width_retain_surfaces(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let projections = fixture.probes().map(|probe| probe.read().projections);
    for _ in 0..8 {
        fixture.frame();
    }
    assert_eq!(
        fixture.probes().map(|probe| probe.read().projections),
        projections
    );
    fixture
        .visual
        .update(|_, cx| cx.set_global(vega_theme::Theme::light()));
    let family: SharedString = fixture.visual.update(|_, cx| {
        cx.text_system()
            .all_font_names()
            .into_iter()
            .next()
            .expect("installed platform font")
            .into()
    });
    fixture.change(|root| {
        root.family = family.clone();
        root.label = LONG.into();
        root.width = 184.;
        root.weight = FontWeight::BOLD;
        root.italic = true;
        root.line_height = 16.;
    });
    fixture.ready(LONG);
    fixture.wait("updated actual projection", |this| {
        let state = this.observations.preview.read();
        let colors = vega_theme::Theme::light().colors;
        state.content
            == Some(Content::Preview {
                label: LONG.into(),
                colors: [
                    colors.bg_elevated,
                    colors.border_subtle,
                    colors.text_primary,
                ],
            })
            && state
                .font
                .as_ref()
                .is_some_and(|(actual_family, weight, italic, height)| {
                    actual_family.as_str() == family.as_ref()
                        && *weight == 700
                        && *italic
                        && *height == 16.
                })
            && state.native.is_some_and(|bounds| bounds[2] == 184.)
    });
    fixture.assert_text_fits(3);
    assert!(fixture.observations.preview.read().projections > projections[1]);
    fixture.retained();
}

pub fn clipped_passive_leaves_forward_input_once_and_skip_tab_focus(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let old = fixture
        .observations
        .preview
        .read()
        .bounds
        .expect("initial preview");
    fixture.change(|root| {
        root.offset = 12.;
        root.clip_width = 100.;
    });
    fixture.frame();
    let state = fixture.observations.preview.read();
    let bounds = state.bounds.expect("moved host");
    let clip = state.clip.expect("paint clip");
    assert_eq!(bounds.origin.x - old.origin.x, px(12.));
    assert_eq!(clip.size.width, px(100.));
    assert!(clip.size.width < bounds.size.width);
    let focus = fixture.visual.update(|window, cx| {
        let focus = fixture.root.read(cx).focus.clone();
        window.focus(&focus[0], cx);
        focus
    });
    fixture.frame();
    for (index, probe) in fixture.probes().into_iter().enumerate() {
        let position = probe.read().clip.expect("leaf clip").center();
        fixture.click(position);
        fixture.input(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(-12.))),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        });
        fixture.frame();
        fixture.visual.update(|window, cx| {
            let root = fixture.root.read(cx);
            assert_eq!(root.clicks, index + 1);
            assert_eq!(root.wheels, index + 1);
            assert_eq!(root.wheel_y, px(-12. * (index + 1) as f32));
            assert!(focus[0].is_focused(window), "passive leaf stole focus");
        });
    }
    fixture.visual.update(|window, cx| window.focus_next(cx));
    fixture.frame();
    fixture.assert_focus(1, "Tab");
    fixture.visual.update(|window, cx| window.focus_prev(cx));
    fixture.frame();
    fixture.assert_focus(0, "reverse Tab");
    fixture.change(|root| root.clip_width = 320.);
    fixture.frame();
    assert_eq!(
        fixture
            .observations
            .preview
            .read()
            .clip
            .expect("expanded clip")
            .size
            .width,
        px(280.)
    );
    fixture.retained();
}

pub fn unmount_stops_both_surfaces_and_remount_creates_new_identity(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let before = fixture.probes().map(|probe| probe.read().surfaces[0]);
    let old_hosts = fixture.hosts.borrow().clone();
    fixture.change(|root| root.visible = false);
    fixture.wait("unmount release and stop", |this| {
        old_hosts.iter().all(|host| host.upgrade().is_none())
            && this.probes().iter().all(|probe| probe.read().stops == 1)
    });
    assert!(fixture.live().is_empty());
    fixture.change(|root| root.visible = true);
    fixture.ready(SHORT);
    for (index, probe) in fixture.probes().into_iter().enumerate() {
        let state = probe.read();
        assert_eq!(state.setups, 2);
        assert_eq!(state.stops, 1);
        assert_eq!(state.surfaces.len(), 2);
        assert_ne!(state.surfaces[1], before[index]);
        assert!(state.failure.is_none());
    }
    assert_eq!(fixture.live().len(), 2);
}
