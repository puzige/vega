use super::CodeDragController;
use crate::conversation_stream::{
    CopySelectedText, MessageCopy, StreamCounters, StreamModel, ThinkingBlock, render_rows,
};
use gpui_kit::base::{TextSelectionLayer, TextSelectionSnapshot};
use gpui_kit::prelude::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    App, Context, ElementId, Entity, FocusHandle, Global, Modifiers, MouseButton, MouseMoveEvent,
    Pixels, PlatformInput, Point, Render, ScrollHandle, SharedString, Task, TestAppContext,
    TextLayout, VisualTestContext, Window, WindowId, div, point, px, size,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
    time::Duration,
};
use vega_markdown::MarkdownStream;
use vega_theme::{Theme, theme};

const CODE: &str = concat!(
    "起点 anchor = 中文与 α；first = 0123456789；second = abcdefghijklmnopqrstuvwxyz；",
    "third = 保留字符、缩进与标点。；fourth = ABCDEFGHIJKLMNOPQRSTUVWXYZ；",
    "fifth = 0123456789_abcdefghijklmnopqrstuvwxyz；sixth = 👩🏽‍💻🚀；",
    "seventh = 连续拖动仍然向右；eighth = 9876543210；tail = 终点。"
);
const SIBLING: &str = concat!(
    "sibling = abcdefghijklmnopqrstuvwxyz_ABCDEFGHIJKLMNOPQRSTUVWXYZ_",
    "0123456789_abcdefghijklmnopqrstuvwxyz_ABCDEFGHIJKLMNOPQRSTUVWXYZ"
);
const ANCHOR: usize = "起点 ".len();

#[derive(Default)]
struct Probe {
    drivers: HashMap<WindowId, HashMap<ElementId, Weak<RefCell<CodeDragController>>>>,
    text: HashMap<WindowId, HashMap<SharedString, TextLayout>>,
}

impl Global for Probe {}

pub(super) fn observe_driver(
    id: ElementId,
    driver: &Rc<RefCell<CodeDragController>>,
    window: &Window,
    cx: &mut App,
) {
    if cx.has_global::<Probe>() {
        cx.global_mut::<Probe>()
            .drivers
            .entry(window.window_handle().window_id())
            .or_default()
            .insert(id, Rc::downgrade(driver));
    }
}

pub(in crate::conversation_stream) fn observe_text(
    text: SharedString,
    layout: TextLayout,
    window: &Window,
    cx: &mut App,
) {
    if cx.has_global::<Probe>() {
        cx.global_mut::<Probe>()
            .text
            .entry(window.window_handle().window_id())
            .or_default()
            .insert(text, layout);
    }
}

#[derive(Clone, Copy)]
enum Surface {
    Answer,
    Thinking,
}

struct Fixture {
    copy: MessageCopy,
    model: StreamModel,
    focus: FocusHandle,
    thinking: Option<Entity<ThinkingBlock>>,
    outer: ScrollHandle,
}

impl Fixture {
    fn new(surface: Surface, cx: &mut Context<Self>) -> Self {
        let source = format!("```text\n{CODE}\n```\n\n```text\n{SIBLING}\n```\n");
        let mut parser = MarkdownStream::new();
        parser.append(&source);
        parser.finish();
        let mut model = StreamModel::default();
        model.sync(&parser.snapshot(), &StreamCounters::default());
        let thinking = matches!(surface, Surface::Thinking).then(|| {
            cx.new(|cx| {
                let mut block = ThinkingBlock::repeated_drag_fixture(cx);
                block.append(&source, false, usize::MAX);
                block.set_expanded(true, cx);
                block
            })
        });
        Self {
            copy: MessageCopy::new(&source),
            model,
            focus: cx.focus_handle(),
            thinking,
            outer: ScrollHandle::new(),
        }
    }
}

impl Render for Fixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let content = if let Some(thinking) = &self.thinking {
            thinking.clone().into_any_element()
        } else {
            render_rows::selectable_markdown_content(
                &self.model,
                &self.copy,
                &self.focus,
                &colors,
                false,
                window,
                cx,
            )
        };
        let copy = self.copy.clone();
        div()
            .size_full()
            .p(px(40.))
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_action(move |_: &CopySelectedText, _, cx| copy.copy_selected_text(cx))
            .child(TextSelectionLayer)
            .child(
                div()
                    .id("repeated-drag-outer")
                    .w(px(360.))
                    .h(px(500.))
                    .overflow_y_scroll()
                    .overflow_x_hidden()
                    .track_scroll(&self.outer)
                    .child(content)
                    .child(div().h(px(900.)).child("Further reading")),
            )
    }
}

struct InputStream {
    position: Rc<Cell<Point<Pixels>>>,
    dispatched: Rc<Cell<usize>>,
    varying: Rc<Cell<bool>>,
    task: Option<Task<()>>,
}

impl InputStream {
    fn stop(&mut self) {
        self.task = None;
    }
}

struct Session {
    visual: VisualTestContext,
    view: Entity<Fixture>,
    window: WindowId,
    anchor: usize,
}

impl Session {
    fn new(cx: &mut TestAppContext, surface: Surface) -> Self {
        cx.update(|cx| {
            if !cx.has_global::<Probe>() {
                gpui_kit::init(cx);
                cx.set_global(Theme::light());
                cx.set_global(Probe::default());
            }
        });
        let window = cx.open_window(size(px(600.), px(680.)), |_, cx| Fixture::new(surface, cx));
        let view = window.root(cx).unwrap();
        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.update(|window, _| window.activate_window());
        let mut session = Self {
            visual,
            view,
            window: window.window_id(),
            anchor: ANCHOR,
        };
        for _ in 0..6 {
            session.frame();
        }
        assert!(session.driver(0).borrow().scroll.max_offset().x > px(0.));
        session
    }

    fn frame(&mut self) {
        self.visual.update(|window, cx| window.render_frame(cx));
        self.visual.run_until_parked();
        self.visual.update(|window, cx| window.render_frame(cx));
    }

    fn advance(&mut self, millis: u64) {
        self.visual
            .executor()
            .advance_clock(Duration::from_millis(millis));
        self.visual.run_until_parked();
        self.frame();
        for driver in self.drivers() {
            let state = driver.borrow();
            let offset = state.scroll.offset();
            assert!(offset.x >= -state.scroll.max_offset().x && offset.x <= px(0.));
            assert_eq!(offset.y, px(0.));
        }
    }

    fn drivers(&self) -> Vec<Rc<RefCell<CodeDragController>>> {
        self.visual.read(|cx| {
            let mut drivers: Vec<_> = cx.global::<Probe>().drivers[&self.window]
                .values()
                .filter_map(Weak::upgrade)
                .filter(|driver| driver.borrow().mounted)
                .collect();
            drivers.sort_by(|left, right| {
                left.borrow()
                    .visible
                    .top()
                    .partial_cmp(&right.borrow().visible.top())
                    .unwrap()
            });
            drivers
        })
    }

    fn driver(&self, index: usize) -> Rc<RefCell<CodeDragController>> {
        self.drivers()[index].clone()
    }

    fn offset(&self) -> Pixels {
        self.driver(0).borrow().scroll.offset().x
    }

    fn outer(&self) -> Point<Pixels> {
        self.visual.read(|cx| self.view.read(cx).outer.offset())
    }

    fn text_point(&self, index: usize) -> Point<Pixels> {
        self.visual.read(|cx| {
            let layout = &cx.global::<Probe>().text[&self.window][CODE];
            layout.position_for_index(index).unwrap() + point(px(0.1), layout.line_height() / 2.)
        })
    }

    fn edge(&self, right: bool) -> Point<Pixels> {
        let visible = self.driver(0).borrow().visible;
        point(
            if right {
                visible.right() - px(4.)
            } else {
                visible.left() + px(4.)
            },
            self.text_point(0).y,
        )
    }

    fn down(&mut self) {
        self.down_at(ANCHOR);
    }

    fn down_at(&mut self, index: usize) {
        self.anchor = index;
        let start = self.text_point(index);
        assert!(self.driver(0).borrow().visible.contains(&start));
        self.visual
            .simulate_mouse_move(start, None, Modifiers::none());
        self.visual
            .simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        self.frame();
    }

    fn moved(&mut self, position: Point<Pixels>) {
        self.visual
            .simulate_mouse_move(position, MouseButton::Left, Modifiers::none());
        self.frame();
    }

    fn repeat(&mut self, period: u64, varying: bool) -> InputStream {
        let position = Rc::new(Cell::new(self.edge(true)));
        let dispatched = Rc::new(Cell::new(0usize));
        let varying = Rc::new(Cell::new(varying));
        let next_position = position.clone();
        let count = dispatched.clone();
        let vary = varying.clone();
        let registered = Rc::new(Cell::new(false));
        let started = registered.clone();
        let now = self.visual.executor().now();
        let task = self.visual.update(|window, cx| {
            window.spawn(cx, async move |cx| {
                loop {
                    let timer = cx
                        .background_executor()
                        .timer(Duration::from_millis(period));
                    started.set(true);
                    timer.await;
                    if cx
                        .update(|window, cx| {
                            let mut position = next_position.get();
                            if vary.get() && count.get().is_multiple_of(2) {
                                position.x += px(1.);
                            }
                            count.set(count.get() + 1);
                            window.dispatch_event(
                                PlatformInput::MouseMove(MouseMoveEvent {
                                    position,
                                    pressed_button: Some(MouseButton::Left),
                                    modifiers: Modifiers::none(),
                                }),
                                cx,
                            );
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
        self.visual.run_until_parked();
        assert!(registered.get());
        assert_eq!(self.visual.executor().now(), now);
        assert_eq!(dispatched.get(), 0);
        self.moved(position.get());
        InputStream {
            position,
            dispatched,
            varying,
            task: Some(task),
        }
    }

    fn snapshot(&self) -> TextSelectionSnapshot {
        let driver = self.driver(0);
        self.visual
            .read(|cx| driver.borrow().selection.handle.snapshot(cx).unwrap())
    }

    fn copied(&mut self) -> String {
        self.visual.dispatch_action(CopySelectedText);
        self.visual.run_until_parked();
        self.visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default()
    }

    fn progress(&mut self, stream: &InputStream, right: bool) {
        let before = self.offset();
        let count = stream.dispatched.get();
        self.advance(32);
        let first = self.offset();
        let first_cursor = self.snapshot().cursor().content_key().unwrap().value();
        let first_count = stream.dispatched.get();
        assert!(stream.task.is_some() && first_count > count);
        self.advance(32);
        let second = self.offset();
        let second_cursor = self.snapshot().cursor().content_key().unwrap().value();
        assert!(stream.task.is_some() && stream.dispatched.get() > first_count);
        assert!(stream.dispatched.get() >= count + 4);
        if right {
            assert!(first < before && second < first);
            assert!(second_cursor > first_cursor);
        } else {
            assert!(first > before && second > first);
            assert!(second_cursor < first_cursor);
        }
        assert_eq!(
            self.snapshot().anchor().content_key().unwrap().value(),
            self.anchor as u64
        );
    }
}

#[gpui_kit::test]
fn native_cadence_held_moves_keep_scrolling_and_copy_exact_code(cx: &mut TestAppContext) {
    let mut session = Session::new(cx, Surface::Answer);
    let outer = session.outer();
    let sibling = session.driver(1).borrow().scroll.offset();
    session.down();
    let mut stream = session.repeat(16, false);
    session.progress(&stream, true);
    let first = session.copied();
    assert!(!first.is_empty() && CODE[ANCHOR..].starts_with(&first));
    for _ in 0..256 {
        session.advance(16);
        if session.offset() == -session.driver(0).borrow().scroll.max_offset().x {
            break;
        }
    }
    assert_eq!(
        session.offset(),
        -session.driver(0).borrow().scroll.max_offset().x
    );
    let visible = session.driver(0).borrow().visible;
    let end = session.text_point(CODE.len());
    let position = point(visible.right() + px(4.), end.y);
    assert!(position.x > end.x && position.y >= visible.top() && position.y < visible.bottom());
    assert!(stream.task.is_some());
    stream.position.set(position);
    session.moved(position);
    assert_eq!(
        session.snapshot().cursor().content_key().unwrap().value(),
        CODE.len() as u64
    );
    assert_eq!(session.copied(), &CODE[ANCHOR..]);
    stream.position.set(session.edge(false));
    session.moved(stream.position.get());
    session.progress(&stream, false);
    for _ in 0..256 {
        session.advance(16);
        if session.offset() == px(0.) {
            break;
        }
    }
    assert_eq!(session.offset(), px(0.));
    assert_eq!(
        session.snapshot().cursor().content_key().unwrap().value(),
        0
    );
    assert_eq!(session.copied(), &CODE[..ANCHOR]);
    assert_eq!(session.outer(), outer);
    assert_eq!(session.driver(1).borrow().scroll.offset(), sibling);
    stream.stop();
}

#[gpui_kit::test]
fn faster_changed_position_moves_progress_then_pause_and_resume(cx: &mut TestAppContext) {
    let mut session = Session::new(cx, Surface::Answer);
    session.down();
    let mut stream = session.repeat(8, true);
    session.progress(&stream, true);
    stream.position.set(session.edge(false));
    session.moved(stream.position.get());
    session.progress(&stream, false);
    let visible = session.driver(0).borrow().visible;
    stream.varying.set(false);
    stream.position.set(visible.center());
    session.moved(stream.position.get());
    let stopped = session.offset();
    let selected = session.snapshot();
    session.advance(64);
    session.advance(64);
    assert_eq!(session.offset(), stopped);
    assert_eq!(session.snapshot(), selected);
    stream.varying.set(true);
    stream.position.set(session.edge(true));
    session.moved(stream.position.get());
    session.progress(&stream, true);
    stream.stop();
}

#[gpui_kit::test]
fn leaving_code_y_or_releasing_stops_repeated_drag_work(cx: &mut TestAppContext) {
    for release in [false, true] {
        let mut session = Session::new(cx, Surface::Answer);
        session.down();
        let mut stream = session.repeat(8, true);
        session.progress(&stream, true);
        let driver = session.driver(0);
        let visible = driver.borrow().visible;
        let position = point(visible.right() - px(4.), visible.bottom() + px(2.));
        if release {
            stream.stop();
            let position = session.edge(true);
            session
                .visual
                .simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
            session.frame();
        } else {
            stream.varying.set(false);
            stream.position.set(position);
            session.moved(position);
        }
        let stopped = session.offset();
        let selected = session.snapshot();
        session.advance(32);
        session.advance(32);
        assert_eq!(session.offset(), stopped);
        assert_eq!(session.snapshot(), selected);
        stream.stop();
        session
            .visual
            .simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
        session.frame();
        let index = session.visual.read(|cx| {
            let layout = &cx.global::<Probe>().text[&session.window][CODE];
            CODE.char_indices()
                .find(|(index, _)| {
                    let x = layout.position_for_index(*index).unwrap().x;
                    x > visible.left() + px(32.) && x < visible.right() - px(32.)
                })
                .unwrap()
                .0
        });
        session.down_at(index);
        let mut stream = session.repeat(8, true);
        session.progress(&stream, true);
        stream.stop();
    }
}

#[gpui_kit::test]
fn shared_thinking_viewport_progresses_during_repeated_held_moves(cx: &mut TestAppContext) {
    let mut session = Session::new(cx, Surface::Thinking);
    let outer = session.outer();
    session.down();
    let mut stream = session.repeat(16, false);
    session.progress(&stream, true);
    assert_eq!(session.outer(), outer);
    assert_eq!(session.driver(1).borrow().scroll.offset().x, px(0.));
    stream.stop();
}
