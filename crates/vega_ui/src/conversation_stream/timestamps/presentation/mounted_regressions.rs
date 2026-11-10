use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DAY: &str = "Wednesday September twenty third two thousand twenty six";
const SUFFIX: &str = " at 11:59 PM";

#[derive(Clone, Default)]
struct Observed {
    id: ProjectionId,
    host: Option<Bounds<Pixels>>,
    mask: Option<Bounds<Pixels>>,
    native: Option<[f32; 4]>,
    texts: Vec<(String, [f32; 4])>,
    lines: Vec<(String, FontWeight, Vec<nsh_core::TextLine>)>,
}

static OBSERVED: Mutex<Vec<Observed>> = Mutex::new(Vec::new());

fn change(id: &ProjectionId, run: impl FnOnce(&mut Observed)) {
    let mut observations = OBSERVED.lock().expect("timestamp observations");
    let index = observations.iter().position(|state| state.id.matches(id));
    let index = index.unwrap_or_else(|| {
        observations.push(Observed {
            id: id.clone(),
            ..Default::default()
        });
        observations.len() - 1
    });
    run(&mut observations[index]);
}

fn rect(bounds: &ns::Bounds<ns::Points>) -> [f32; 4] {
    [
        bounds.origin.x.0,
        bounds.origin.y.0,
        bounds.size.width.0,
        bounds.size.height.0,
    ]
}

pub(super) fn record_host(host: &WeakEntity<Host>, bounds: Bounds<Pixels>, cx: &App) {
    if let Some(host) = host.upgrade() {
        change(&host.read(cx).snapshot.id, |state| {
            state.host = Some(bounds)
        });
    }
}

pub(super) fn record_mask(host: &WeakEntity<Host>, bounds: Bounds<Pixels>, cx: &App) {
    if let Some(host) = host.upgrade() {
        change(&host.read(cx).snapshot.id, |state| {
            state.mask = Some(bounds)
        });
    }
}

pub(super) fn record_native(id: &ProjectionId, bounds: &ns::Bounds<ns::Points>) {
    change(id, |state| state.native = Some(rect(bounds)));
}

pub(super) fn record_text(id: &ProjectionId, value: &str, bounds: &ns::Bounds<ns::Points>) {
    change(id, |state| {
        state.texts.retain(|(text, _)| text != value);
        state.texts.push((value.into(), rect(bounds)));
    });
}

pub(super) fn record_lines(
    id: &ProjectionId,
    value: &str,
    weight: FontWeight,
    lines: &[nsh_core::TextLine],
) {
    change(id, |state| {
        state.lines.retain(|(text, _, _)| text != value);
        state.lines.push((value.into(), weight, lines.to_vec()));
    });
}

struct Root {
    width: f32,
    focus: [FocusHandle; 2],
    clicks: usize,
    wheels: usize,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("timestamp-before")
                    .w(px(24.))
                    .h(px(24.))
                    .track_focus(&self.focus[0])
                    .tab_stop(true),
            )
            .child(
                div()
                    .w(px(self.width))
                    .flex()
                    .flex_col()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|root, _, _, _| root.clicks += 1),
                    )
                    .on_scroll_wheel(cx.listener(|root, _, _, _| root.wheels += 1))
                    .child(Separator {
                        key: "mounted-timestamp-separator".into(),
                        day: DAY.into(),
                        suffix: SUFFIX.into(),
                        accessible: format!("{DAY}{SUFFIX}"),
                        color: vega_theme::theme(cx).colors.text_tertiary,
                    }),
            )
            .child(
                div()
                    .id("timestamp-after")
                    .w(px(24.))
                    .h(px(24.))
                    .track_focus(&self.focus[1])
                    .tab_stop(true),
            )
    }
}

struct Fixture {
    visual: VisualTestContext,
    root: Entity<Root>,
    hosts: Rc<RefCell<Vec<WeakEntity<Host>>>>,
    _observer: Subscription,
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        OBSERVED
            .lock()
            .expect("clear timestamp observations")
            .clear();
        cx.update(|cx| {
            assert!(!cx.text_system().all_font_names().is_empty());
            cx.set_global(vega_theme::Theme::dark());
        });
        let hosts = Rc::new(RefCell::new(Vec::new()));
        let observed = hosts.clone();
        let observer = cx.update(|cx| {
            cx.observe_new::<Host>(move |_, _, cx| observed.borrow_mut().push(cx.weak_entity()))
        });
        let window = cx.open_window(size(px(1000.), px(760.)), |_, cx| Root {
            width: 920.,
            focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            clicks: 0,
            wheels: 0,
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let root = visual.update(|window, _| {
            window.activate_window();
            window
                .root::<Root>()
                .flatten()
                .expect("timestamp fixture root")
        });
        Self {
            visual,
            root,
            hosts,
            _observer: observer,
        }
    }

    fn drain(&mut self) {
        let started = Instant::now();
        for tick in 0..512 {
            let worked = self.visual.cx.background_executor.tick();
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "timestamp scheduler deadline tick={tick}"
            );
            if !worked {
                return;
            }
        }
        panic!("timestamp scheduler did not quiesce in 512 ticks");
    }

    fn frame(&mut self) {
        self.drain();
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.drain();
    }

    fn current(&mut self) -> Entity<Host> {
        let hosts = self.hosts.borrow().clone();
        self.visual.update(|_, cx| {
            hosts
                .into_iter()
                .rev()
                .filter_map(|host| host.upgrade())
                .find(|host| host.read(cx).snapshot.input.day == DAY)
                .expect("production timestamp host")
        })
    }

    fn ready(&mut self) -> Observed {
        let started = Instant::now();
        loop {
            self.frame();
            let host = self.current();
            let state = self.visual.update(|_, cx| {
                let host = host.read(cx);
                let state = OBSERVED
                    .lock()
                    .expect("timestamp observation")
                    .iter()
                    .find(|state| state.id.matches(&host.snapshot.id))
                    .cloned()?;
                let bounds = state.host?;
                let native = state.native?;
                (host.ready
                    && state.lines.len() == 2
                    && state.texts.len() == 2
                    && host
                        .surface
                        .as_ref()
                        .is_some_and(|surface| surface.errors().borrow().is_none())
                    && (host.height - f32::from(bounds.size.height)).abs() <= 0.5
                    && (native[3] - host.height).abs() <= 0.5
                    && (native[2] - f32::from(bounds.size.width)).abs() <= 0.5
                    && state.mask.is_some())
                .then_some(state)
            });
            if let Some(state) = state {
                return state;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "timestamp native/layout convergence timeout"
            );
            std::thread::sleep(Duration::from_millis(10));
            self.visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
    }

    fn input(&mut self, event: impl InputEvent) {
        self.visual.update(|window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
        self.drain();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let hosts = self.hosts.borrow().clone();
        let window = self.visual.window_handle();
        let cleanup = self.visual.cx.update_window(window, |_, window, cx| {
            for host in hosts.into_iter().filter_map(|host| host.upgrade()) {
                host.update(cx, |host, cx| host.stop(cx));
            }
            window.remove_window();
        });
        if let Err(error) = cleanup {
            if std::thread::panicking() {
                eprintln!("timestamp fixture cleanup during unwind: {error:#}");
            } else {
                panic!("timestamp fixture cleanup: {error:#}");
            }
        } else if !std::thread::panicking() {
            self.drain();
        }
    }
}

pub fn timestamp_native_narrow_height_and_passive_input(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let wide = fixture.ready();
    fixture.root.update(&mut fixture.visual.cx, |root, cx| {
        root.width = 240.;
        cx.notify();
    });
    let narrow = fixture.ready();
    assert!(
        narrow.native.expect("narrow native")[3] > wide.native.expect("wide native")[3],
        "long timestamp must use actual wrapped natural height; wide_native={:?} narrow_native={:?} wide_host={:?} narrow_host={:?} wide_texts={:?} narrow_texts={:?} wide_lines={:?} narrow_lines={:?}",
        wide.native,
        narrow.native,
        wide.host,
        narrow.host,
        wide.texts,
        narrow.texts,
        wide.lines
            .iter()
            .map(|(text, _, lines)| (text, lines.len()))
            .collect::<Vec<_>>(),
        narrow
            .lines
            .iter()
            .map(|(text, _, lines)| (text, lines.len()))
            .collect::<Vec<_>>()
    );
    for state in [&wide, &narrow] {
        assert_eq!(
            state.mask.expect("passive NSH input mask").size,
            size(px(0.), px(0.))
        );
        let day = state
            .lines
            .iter()
            .find(|(text, _, _)| text == DAY)
            .expect("native day text");
        let suffix = state
            .lines
            .iter()
            .find(|(text, _, _)| text == SUFFIX)
            .expect("native suffix text");
        assert_eq!(day.1, FontWeight::MEDIUM);
        assert_eq!(suffix.1, FontWeight::NORMAL);
        assert!(
            !day.2.is_empty() && !suffix.2.is_empty(),
            "actual platform shaped text lines"
        );
        let root = state.native.expect("native separator bounds");
        let day_bounds = state
            .texts
            .iter()
            .find(|(text, _)| text == DAY)
            .expect("day layout")
            .1;
        let suffix_bounds = state
            .texts
            .iter()
            .find(|(text, _)| text == SUFFIX)
            .expect("suffix layout")
            .1;
        let height = day_bounds[3].max(suffix_bounds[3]);
        assert!(
            (root[3] - height - Layout::CONVERSATION_STAGE_TOP * 2.).abs() <= 0.5,
            "original vertical padding must remain outside natural text height; native={root:?} day={day_bounds:?} suffix={suffix_bounds:?} expected_padding={}",
            Layout::CONVERSATION_STAGE_TOP * 2.
        );
        let left = day_bounds[0].min(suffix_bounds[0]);
        let right = (day_bounds[0] + day_bounds[2]).max(suffix_bounds[0] + suffix_bounds[2]);
        assert!(
            (left + right - root[0] * 2. - root[2]).abs() <= 1.,
            "timestamp day and suffix must remain centered; native={root:?} day={day_bounds:?} suffix={suffix_bounds:?} left={left} right={right}"
        );
    }
    let position = narrow.host.expect("timestamp GPUI bounds").center();
    fixture.input(MouseDownEvent {
        position,
        button: MouseButton::Left,
        click_count: 1,
        ..Default::default()
    });
    fixture.input(MouseUpEvent {
        position,
        button: MouseButton::Left,
        click_count: 1,
        ..Default::default()
    });
    fixture.input(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.), px(-12.))),
        ..Default::default()
    });
    let root = fixture.root.clone();
    let host = fixture.current();
    fixture.visual.update(|window, cx| {
        let root = root.read(cx);
        assert_eq!(root.clicks, 1, "passive timestamp must not swallow pointer");
        assert_eq!(root.wheels, 1, "passive timestamp must not swallow wheel");
        let focus = root.focus.clone();
        window.focus(&focus[0], cx);
        window.focus_next(cx);
        assert!(
            focus[1].is_focused(window),
            "timestamp must not add a tab stop"
        );
        assert!(
            !host
                .read(cx)
                .surface
                .as_ref()
                .expect("timestamp Surface")
                .focus_handle(cx)
                .is_focused(window)
        );
    });
}
