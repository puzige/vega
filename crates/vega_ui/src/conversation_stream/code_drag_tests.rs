use super::super::code_drag::CodeDragController;
use super::super::{CopySelectedText, MessageCopy, StreamCounters, StreamModel, render_rows};
use super::ThinkingBlock;
use gpui_kit as gpui;
use gpui_kit::base::{ScrollbarHandle, TextSelection, TextSelectionLayer};
use gpui_kit::prelude::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    App, Bounds, Context, ElementId, Entity, FocusHandle, Global, Modifiers, MouseButton, Pixels,
    Point, Render, ScrollDelta, ScrollHandle, ScrollWheelEvent, SharedString, TestAppContext,
    TextLayout, VisualTestContext, Window, WindowId, div, point, px, size,
};
use std::time::Duration;
use std::{cell::RefCell, collections::HashMap, rc::Weak};
use vega_markdown::MarkdownStream;
use vega_theme::{Theme, Typography, theme};

const CODE: &str = concat!(
    "起点 α，alpha_01 = 中文值；beta_02 = (x + y)；gamma_03 = \"保持原文\"；",
    "delta_04 = [1, 2, 3]；epsilon_05 = {键: 值}；zeta_06 = a -> b；",
    "eta_07 = λ + 42；theta_08 = \"没有软换行\"；iota_09 = foo(bar)；",
    "kappa_10 = left & right；lambda_11 = 中英文标点，。👩🏽‍💻🚀；tail_终点。"
);
const SIBLING: &str = concat!(
    "sibling_独立代码块 = 保持不动；1111111111222222222233333333334444444444",
    "555555555566666666667777777777888888888899999999990000000000"
);
const ADJACENT: &str = concat!(
    "adjacent_另一条消息 = 保持不动；abcdefghijklmnopqrstuvwxyz_",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz_0123456789"
);
const SHORT: &str = "短行 = 42；";
const QUOTE: &str = concat!(
    "Quote prose must wrap naturally inside the available message width. ",
    "中文引用也应当自然换行，不因为旁边存在不折行的代码而横向溢出。 ",
    "The final quote words remain readable without horizontal scrolling."
);
const PARAGRAPHS: [&str; 3] = [
    "First paragraph keeps ordinary selection.",
    "Second paragraph stays in document order.",
    "Third paragraph completes the selection.",
];
const STEP: Duration = Duration::from_millis(32);

#[derive(Clone)]
struct ScrollObservation {
    id: ElementId,
    offset: Point<Pixels>,
    extent: Point<Pixels>,
    bounds: Bounds<Pixels>,
    visible: Bounds<Pixels>,
}

#[derive(Clone)]
struct TextObservation {
    text: SharedString,
    layout: TextLayout,
    bounds: Bounds<Pixels>,
}

#[derive(Default)]
struct WindowObservations {
    scrolls: HashMap<ElementId, ScrollObservation>,
    handles: HashMap<ElementId, ScrollHandle>,
    documents: HashMap<u64, Vec<TextObservation>>,
    drivers: HashMap<ElementId, Weak<RefCell<CodeDragController>>>,
}

#[derive(Default)]
struct Observations {
    windows: HashMap<WindowId, WindowObservations>,
}

impl Global for Observations {}

pub(in crate::conversation_stream) fn observe_driver(
    window: WindowId,
    id: ElementId,
    driver: Weak<RefCell<CodeDragController>>,
    cx: &mut App,
) {
    if cx.has_global::<Observations>() {
        cx.global_mut::<Observations>()
            .windows
            .entry(window)
            .or_default()
            .drivers
            .insert(id, driver);
    }
}

pub(crate) fn observe_scroll_handle(
    window: WindowId,
    id: Option<ElementId>,
    handle: ScrollHandle,
    cx: &mut App,
) {
    if let Some(id) = id
        && cx.has_global::<Observations>()
    {
        cx.global_mut::<Observations>()
            .windows
            .entry(window)
            .or_default()
            .handles
            .insert(id, handle);
    }
}

pub(crate) fn observe_scroll(
    window: WindowId,
    id: Option<ElementId>,
    handle: &dyn ScrollbarHandle,
    visible: Bounds<Pixels>,
    cx: &mut App,
) {
    if let Some(id) = id
        && cx.has_global::<Observations>()
    {
        let bounds = handle.viewport_bounds();
        let extent = handle.content_size() - bounds.size;
        cx.global_mut::<Observations>()
            .windows
            .entry(window)
            .or_default()
            .scrolls
            .insert(
                id.clone(),
                ScrollObservation {
                    id,
                    offset: handle.offset(),
                    extent: point(extent.width.max(px(0.)), extent.height.max(px(0.))),
                    bounds,
                    visible,
                },
            );
    }
}

pub(crate) fn begin_document(window: WindowId, owner: u64, cx: &mut App) {
    if cx.has_global::<Observations>() {
        cx.global_mut::<Observations>()
            .windows
            .entry(window)
            .or_default()
            .documents
            .insert(owner, Vec::new());
    }
}

pub(crate) fn observe_text(
    window: WindowId,
    owner: u64,
    text: SharedString,
    layout: TextLayout,
    bounds: Bounds<Pixels>,
    cx: &mut App,
) {
    if cx.has_global::<Observations>() {
        cx.global_mut::<Observations>()
            .windows
            .entry(window)
            .or_default()
            .documents
            .entry(owner)
            .or_default()
            .push(TextObservation {
                text,
                layout,
                bounds,
            });
    }
}

struct MessageFixture {
    copy: MessageCopy,
    model: StreamModel,
    focus: FocusHandle,
}

impl MessageFixture {
    fn new(source: &str, cx: &mut App) -> Self {
        let mut stream = MarkdownStream::new();
        stream.append(source);
        stream.finish();
        let mut model = StreamModel::default();
        model.sync(&stream.snapshot(), &StreamCounters::default());
        Self {
            copy: MessageCopy::new(source),
            model,
            focus: cx.focus_handle(),
        }
    }

    fn render(&self, window: &mut Window, cx: &mut App) -> gpui_kit::AnyElement {
        let copy = self.copy.clone();
        let colors = theme(cx).colors;
        div()
            .id(("fixture-message", self.copy.id))
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .track_focus(&self.focus)
            .on_action(move |_: &CopySelectedText, _, cx| copy.copy_selected_text(cx))
            .child(render_rows::selectable_markdown_content(
                &self.model,
                &self.copy,
                &self.focus,
                &colors,
                false,
                window,
                cx,
            ))
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
enum Surface {
    Answer,
    Thinking,
}

struct InteractionView {
    messages: Vec<MessageFixture>,
    thinking: Option<Entity<ThinkingBlock>>,
    outer: ScrollHandle,
    outside_focus: FocusHandle,
    mounted: bool,
    clipped: bool,
}

impl InteractionView {
    fn new(surface: Surface, source: Option<&str>, cx: &mut Context<Self>) -> Self {
        let messages = match surface {
            Surface::Answer => vec![
                MessageFixture::new(
                    &source.map(str::to_owned).unwrap_or_else(|| format!(
                        "```text\n{CODE}\n```\n\n```text\n{SIBLING}\n```\n\n```text\n{SHORT}\n```\n"
                    )),
                    cx,
                ),
                MessageFixture::new(&format!("```text\n{ADJACENT}\n```\n"), cx),
            ],
            Surface::Thinking => Vec::new(),
        };
        let thinking = matches!(surface, Surface::Thinking).then(|| {
            cx.new(|cx| {
                let mut card = ThinkingBlock::new(cx);
                card.append(
                    &source.map(str::to_owned).unwrap_or_else(|| {
                        format!(
                            "```text\n{CODE}\n```\n\n```text\n{SIBLING}\n```\n\n{}\n",
                            "ordinary thinking paragraph\n\n".repeat(20)
                        )
                    }),
                    false,
                    usize::MAX,
                );
                card.set_expanded(true, cx);
                card
            })
        });
        Self {
            messages,
            thinking,
            outer: ScrollHandle::new(),
            outside_focus: cx.focus_handle().tab_stop(true),
            mounted: true,
            clipped: false,
        }
    }
}

impl Render for InteractionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = div().w_full().min_w_0().flex_shrink_0().flex().flex_col();
        if self.mounted {
            for message in &self.messages {
                content = content.child(message.render(window, cx));
            }
            if let Some(thinking) = &self.thinking {
                content = content.child(thinking.clone());
            }
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(40.))
            .child(TextSelectionLayer)
            .child(
                div()
                    .id("fixture-outer-scroll")
                    .w(px(420.))
                    .h(px(560.))
                    .overflow_y_scroll()
                    .overflow_x_hidden()
                    .track_scroll(&self.outer)
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .when(self.clipped, |element| element.h(px(0.)).overflow_hidden())
                            .child(content),
                    )
                    .child(div().h(px(900.)).child("Outside the selected message")),
            )
            .child(
                div()
                    .id("fixture-other-focus")
                    .track_focus(&self.outside_focus)
                    .child("Other focus target"),
            )
    }
}

struct Session {
    view: Entity<InteractionView>,
    visual: VisualTestContext,
    owner: u64,
    surface: Surface,
    window: WindowId,
}

impl Session {
    fn new(cx: &mut TestAppContext, surface: Surface) -> Self {
        Self::with_source(cx, surface, None)
    }

    fn with_source(cx: &mut TestAppContext, surface: Surface, source: Option<&str>) -> Self {
        cx.update(|cx| {
            if !cx.has_global::<Observations>() {
                gpui_kit::init(cx);
                cx.set_global(Theme::light());
                cx.set_global(Observations::default());
            }
        });
        let window = cx.open_window(size(px(640.), px(720.)), |_, cx| {
            InteractionView::new(surface, source, cx)
        });
        let view = window.root(cx).unwrap();
        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.update(|window, _| window.activate_window());
        let owner = visual.read(|cx| {
            let view = view.read(cx);
            match surface {
                Surface::Answer => view.messages[0].copy.id,
                Surface::Thinking => view.thinking.as_ref().unwrap().read(cx).source_copy.id,
            }
        });
        let mut session = Self {
            view,
            visual,
            owner,
            surface,
            window: window.window_id(),
        };
        for _ in 0..6 {
            session.frame();
            session.visual.run_until_parked();
        }
        assert!(session.visual.update(|window, _| window.is_window_active()));
        assert_eq!(session.text(CODE).text.as_ref(), CODE);
        session
    }

    fn frame(&mut self) {
        self.visual.update(|window, cx| window.render_frame(cx));
        self.visual.run_until_parked();
        self.visual.update(|window, cx| window.render_frame(cx));
    }

    fn tick(&mut self) {
        self.visual.executor().advance_clock(STEP);
        self.visual.run_until_parked();
        self.frame();
        self.visual.read(|cx| {
            for scroll in cx.global::<Observations>().windows[&self.window]
                .scrolls
                .values()
            {
                let id = scroll.id.to_string();
                if id.starts_with("markdown-code-scroll-") || id.starts_with("thinking-horizontal")
                {
                    assert!(scroll.offset.x <= px(0.) && scroll.offset.x >= -scroll.extent.x);
                    assert_eq!(scroll.offset.y, px(0.));
                }
            }
        });
    }

    fn hold(&mut self, steps: usize) {
        for _ in 0..steps {
            self.tick();
        }
    }

    fn code(&self, ordinal: usize) -> ScrollObservation {
        self.visual.read(|cx| {
            let mut scrolls: Vec<_> = cx.global::<Observations>().windows[&self.window]
                .scrolls
                .values()
                .filter(|scroll| match self.surface {
                    Surface::Answer => scroll
                        .id
                        .to_string()
                        .starts_with(&format!("markdown-code-scroll-{}-", self.owner)),
                    Surface::Thinking => scroll.id.to_string().starts_with("thinking-horizontal"),
                })
                .cloned()
                .collect();
            scrolls
                .sort_by(|left, right| left.bounds.top().partial_cmp(&right.bounds.top()).unwrap());
            scrolls
                .get(ordinal)
                .cloned()
                .expect("rendered production code viewport")
        })
    }

    fn text(&self, text: &str) -> TextObservation {
        self.visual.read(|cx| {
            cx.global::<Observations>().windows[&self.window]
                .documents
                .get(&self.owner)
                .and_then(|runs| runs.iter().find(|run| run.text.as_ref() == text))
                .cloned()
                .expect("production text layout")
        })
    }

    fn text_point(&self, text: &str, byte: usize) -> Point<Pixels> {
        let run = self.text(text);
        run.layout.position_for_index(byte).unwrap()
            + point(px(0.1), run.layout.line_height().half())
    }

    fn edge(&self, right: bool) -> Point<Pixels> {
        let viewport = self.code(0).visible;
        point(
            if right {
                viewport.right() + px(1.)
            } else {
                viewport.left() - px(1.)
            },
            self.text_point(CODE, 0).y,
        )
    }

    fn visible_character(&self) -> usize {
        let viewport = self.code(0).visible;
        let run = self.text(CODE);
        CODE.char_indices()
            .filter_map(|(index, _)| {
                let point = run.layout.position_for_index(index)?;
                (point.x > viewport.left() + px(24.) && point.x < viewport.right() - px(24.))
                    .then_some((index, (f32::from(point.x - viewport.center().x)).abs()))
            })
            .min_by(|left, right| left.1.partial_cmp(&right.1).unwrap())
            .expect("visible character away from edge trigger")
            .0
    }

    fn down(&mut self, point: Point<Pixels>) {
        self.visual
            .simulate_mouse_move(point, None, Modifiers::none());
        self.visual
            .simulate_mouse_down(point, MouseButton::Left, Modifiers::none());
        self.frame();
    }

    fn drag(&mut self, point: Point<Pixels>) {
        self.visual
            .simulate_mouse_move(point, MouseButton::Left, Modifiers::none());
        self.frame();
    }

    fn up(&mut self, point: Point<Pixels>) {
        self.visual
            .simulate_mouse_up(point, MouseButton::Left, Modifiers::none());
        self.frame();
    }

    fn wheel(&mut self, point: Point<Pixels>, x: f32, y: f32) {
        self.visual
            .simulate_mouse_move(point, None, Modifiers::none());
        self.visual.simulate_event(ScrollWheelEvent {
            position: point,
            delta: ScrollDelta::Pixels(gpui_kit::point(px(x), px(y))),
            ..Default::default()
        });
        self.frame();
    }

    fn copied(&mut self) -> String {
        self.visual.update(|_, cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                "clipboard sentinel".to_owned(),
            ));
        });
        self.visual.dispatch_action(CopySelectedText);
        self.visual.run_until_parked();
        self.visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap()
    }

    fn outer(&self) -> Point<Pixels> {
        self.visual.read(|cx| self.view.read(cx).outer.offset())
    }

    fn vertical(&self) -> ScrollObservation {
        self.visual.read(|cx| {
            let view = self.view.read(cx);
            let (id, handle) = match self.surface {
                Surface::Answer => ("fixture-outer-scroll", view.outer.clone()),
                Surface::Thinking => (
                    "thinking-content",
                    view.thinking.as_ref().unwrap().read(cx).scroll.clone(),
                ),
            };
            ScrollObservation {
                id: id.into(),
                offset: handle.offset(),
                extent: handle.max_offset(),
                bounds: handle.bounds(),
                visible: handle.bounds(),
            }
        })
    }

    fn window_offsets(&self) -> Vec<(String, Point<Pixels>, Bounds<Pixels>)> {
        self.visual.read(|cx| {
            let mut offsets: Vec<_> = cx.global::<Observations>().windows[&self.window]
                .scrolls
                .values()
                .map(|scroll| (scroll.id.to_string(), scroll.offset, scroll.bounds))
                .collect();
            offsets.sort_by(|left, right| left.0.cmp(&right.0));
            offsets
        })
    }

    fn other_offsets(&self) -> Vec<(String, Point<Pixels>, Bounds<Pixels>)> {
        let target = self.code(0).id;
        self.visual.read(|cx| {
            let mut offsets: Vec<_> = cx.global::<Observations>().windows[&self.window]
                .scrolls
                .values()
                .filter(|scroll| scroll.id != target)
                .map(|scroll| (scroll.id.to_string(), scroll.offset, scroll.bounds))
                .collect();
            offsets.sort_by(|left, right| left.0.cmp(&right.0));
            offsets
        })
    }

    fn begin_right(&mut self) -> usize {
        let start = self.visible_character();
        self.down(self.text_point(CODE, start));
        self.drag(self.edge(true));
        start
    }

    fn assert_progress(&mut self, right: bool) {
        let before = self.code(0).offset.x;
        self.tick();
        let first = self.code(0).offset.x;
        self.tick();
        let second = self.code(0).offset.x;
        if right {
            assert!(
                first < before && second < first,
                "held right edge must scroll on successive ticks: {before:?}, {first:?}, {second:?}"
            );
        } else {
            assert!(
                first > before && second > first,
                "held left edge must scroll on successive ticks: {before:?}, {first:?}, {second:?}"
            );
        }
    }
}

fn assert_right_interaction(cx: &mut TestAppContext, surface: Surface) {
    let mut session = Session::new(cx, surface);
    let anchor = CODE.find("alpha_01").unwrap();
    let expected = &CODE[anchor..];
    let outer = session.outer();
    let others = session.other_offsets();
    assert!(session.code(0).extent.x > session.code(0).bounds.size.width);
    session.down(session.text_point(CODE, anchor));
    session.drag(session.edge(true));
    let before = session.code(0).offset.x;
    session.tick();
    let first = session.code(0).offset.x;
    let first_text = session.copied();
    session.tick();
    let second = session.code(0).offset.x;
    let second_text = session.copied();
    assert!(
        first < before && second < first,
        "held right edge must advance twice without pointer movement: {before:?}, {first:?}, {second:?}"
    );
    assert!(!first_text.is_empty() && expected.starts_with(&first_text));
    assert!(second_text.starts_with(&first_text) && second_text.len() > first_text.len());
    assert_eq!(session.outer(), outer);
    assert_eq!(session.other_offsets(), others);
    session.hold(256);
    let end = session.code(0);
    assert_eq!(end.offset.x, -end.extent.x);
    assert_eq!(session.copied(), expected);
    session.hold(3);
    assert_eq!(session.code(0).offset, end.offset);
    session.drag(session.edge(false));
    session.assert_progress(false);
    session.hold(256);
    assert_eq!(session.code(0).offset.x, px(0.));
    assert_eq!(session.copied(), &CODE[..anchor]);
    session.hold(3);
    assert_eq!(session.code(0).offset.x, px(0.));
    assert_eq!(session.outer(), outer);
    assert_eq!(session.other_offsets(), others);
    session.up(session.edge(false));
}

fn assert_left_interaction(cx: &mut TestAppContext, surface: Surface) {
    let mut session = Session::new(cx, surface);
    let wheel_point = session.code(0).visible.center();
    session.wheel(wheel_point, -100_000., 0.);
    let before = session.code(0);
    assert_eq!(before.offset.x, -before.extent.x);
    let anchor = CODE.find("tail_终点").unwrap();
    assert!(before.visible.contains(&session.text_point(CODE, anchor)));
    let outer = session.outer();
    session.down(session.text_point(CODE, anchor));
    session.drag(session.edge(false));
    session.assert_progress(false);
    session.hold(256);
    assert_eq!(session.code(0).offset.x, px(0.));
    assert_eq!(session.copied(), &CODE[..anchor]);
    session.hold(3);
    assert_eq!(session.code(0).offset.x, px(0.));
    session.up(session.edge(false));
    let selected = session.copied();
    session.hold(3);
    assert_eq!(session.copied(), selected);
    assert_eq!(session.code(0).offset.x, px(0.));
    assert_eq!(session.outer(), outer);
    if matches!(surface, Surface::Thinking) {
        let inner = session.visual.read(|cx| {
            cx.global::<Observations>().windows[&session.window].scrolls
                [&ElementId::from("thinking-content")]
                .clone()
        });
        assert!(inner.extent.y > px(0.));
        session.wheel(inner.visible.center(), 0., -64.);
        let moved = session.visual.read(|cx| {
            cx.global::<Observations>().windows[&session.window].scrolls
                [&ElementId::from("thinking-content")]
                .offset
        });
        assert!(moved.y < inner.offset.y);
        assert_eq!(session.outer(), outer);
    }
}

#[gpui_kit::test]
fn answer_code_held_edges_extend_utf8_selection_and_reverse_at_clamps(cx: &mut TestAppContext) {
    assert_right_interaction(cx, Surface::Answer);
}

#[gpui_kit::test]
fn answer_code_left_drag_scrolls_after_a_real_horizontal_wheel(cx: &mut TestAppContext) {
    assert_left_interaction(cx, Surface::Answer);
}

#[gpui_kit::test]
fn thinking_code_held_edges_extend_utf8_selection_and_reverse_at_clamps(cx: &mut TestAppContext) {
    assert_right_interaction(cx, Surface::Thinking);
}

#[gpui_kit::test]
fn thinking_code_left_drag_scrolls_after_a_real_horizontal_wheel(cx: &mut TestAppContext) {
    assert_left_interaction(cx, Surface::Thinking);
}

#[gpui_kit::test]
fn releasing_or_returning_to_the_middle_stops_code_selection_scroll(cx: &mut TestAppContext) {
    let mut session = Session::new(cx, Surface::Answer);
    let mut failures = Vec::new();
    for release in [true, false] {
        session.wheel(session.code(0).visible.center(), 100_000., 0.);
        let anchor = session.begin_right();
        let before = session.code(0).offset.x;
        session.hold(2);
        if session.code(0).offset.x >= before {
            failures.push(format!(
                "release={release}: edge drag never started scrolling"
            ));
        }
        let selected = if release {
            session.up(session.edge(true));
            let selected = session.copied();
            if selected.is_empty() || !CODE[anchor..].starts_with(&selected) {
                failures.push("release changed the selected logical substring".to_owned());
            }
            selected
        } else {
            let end = session.visible_character();
            session.drag(session.text_point(CODE, end));
            let selected = session.copied();
            let range = anchor.min(end)..anchor.max(end);
            if selected != CODE[range] {
                failures.push("middle stop changed the logical anchor or cursor".to_owned());
            }
            selected
        };
        let stopped = session.code(0).offset;
        session.hold(3);
        if session.code(0).offset != stopped || session.copied() != selected {
            failures.push(format!(
                "release={release}: scroll or selection continued after stop"
            ));
        }
        session.up(session.edge(true));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[gpui_kit::test]
fn clearing_focus_loss_and_deactivation_stop_held_code_selection_scroll(cx: &mut TestAppContext) {
    let mut session = Session::new(cx, Surface::Answer);
    let mut failures = Vec::new();
    for stop in ["clear", "focus", "deactivate"] {
        session.wheel(session.code(0).visible.center(), 100_000., 0.);
        session.begin_right();
        let before = session.code(0).offset.x;
        session.hold(2);
        if session.code(0).offset.x >= before {
            failures.push(format!("{stop}: edge drag never started scrolling"));
        }
        match stop {
            "clear" => session.visual.update(TextSelection::clear),
            "focus" => {
                let focus = session
                    .visual
                    .read(|cx| session.view.read(cx).outside_focus.clone());
                session.visual.update(|window, cx| focus.focus(window, cx));
            }
            "deactivate" => session.visual.deactivate_window(),
            _ => unreachable!(),
        }
        session.frame();
        let stopped = session.code(0).offset;
        session.hold(3);
        if session.code(0).offset != stopped {
            failures.push(format!("{stop}: inactive drag kept scrolling"));
        }
        if stop == "clear" && session.visual.update(TextSelection::has_selection) {
            failures.push("clear: selection remained active".to_owned());
        }
        session.up(session.edge(true));
    }
    assert!(!session.visual.update(|window, _| window.is_window_active()));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[gpui_kit::test]
fn removing_replacing_and_clipping_code_retire_the_original_scroll_driver(cx: &mut TestAppContext) {
    let mut session = Session::new(cx, Surface::Answer);
    let mut failures = Vec::new();
    for stop in ["remove", "replace", "clip"] {
        session.wheel(session.code(0).visible.center(), 100_000., 0.);
        session.begin_right();
        let original = session.code(0);
        let handle = session.visual.read(|cx| {
            cx.global::<Observations>().windows[&session.window]
                .handles
                .get(&original.id)
                .cloned()
                .unwrap()
        });
        session.hold(2);
        if handle.offset().x >= original.offset.x {
            failures.push(format!("{stop}: edge drag never started scrolling"));
        }
        session.visual.update(|_, cx| {
            session.view.update(cx, |view, cx| {
                match stop {
                    "remove" => view.mounted = false,
                    "replace" => {
                        let source = format!("```text\n{}\n```\n", CODE.replace("起点", "替换"));
                        view.messages[0].copy.replace_text(&source);
                        view.messages[0].model = MessageFixture::new(&source, cx).model;
                    }
                    "clip" => view.clipped = true,
                    _ => unreachable!(),
                }
                cx.notify();
            });
        });
        session.frame();
        let stopped = handle.offset();
        let outer = session.outer();
        session.hold(3);
        if handle.offset() != stopped || session.outer() != outer {
            failures.push(format!(
                "{stop}: retired code viewport still changed offsets"
            ));
        }
        if stop == "replace" && session.visual.update(TextSelection::has_selection) {
            failures.push("replace: old logical selection survived source replacement".to_owned());
        }
        session.visual.simulate_mouse_up(
            point(px(450.), px(120.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        session.visual.update(|_, cx| {
            session.view.update(cx, |view, cx| {
                view.mounted = true;
                view.clipped = false;
                view.messages[0] = MessageFixture::new(
                    &format!("```text\n{CODE}\n```\n\n```text\n{SIBLING}\n```\n"),
                    cx,
                );
                cx.notify();
            });
        });
        session.owner = session
            .visual
            .read(|cx| session.view.read(cx).messages[0].copy.id);
        session.frame();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[gpui_kit::test]
fn code_hover_short_lines_and_crossing_into_a_sibling_never_scroll_the_wrong_viewport(
    cx: &mut TestAppContext,
) {
    let mut session = Session::new(cx, Surface::Answer);
    let initial = session.code(0).offset;
    let edge = session.edge(true);
    session
        .visual
        .simulate_mouse_move(edge, None, Modifiers::none());
    session.hold(3);
    assert_eq!(session.code(0).offset, initial);
    let short = session.code(2);
    assert_eq!(short.extent.x, px(0.));
    session.down(session.text_point(SHORT, 0));
    session.drag(point(
        short.visible.right() - px(1.),
        session.text_point(SHORT, 0).y,
    ));
    let outer = session.outer();
    session.hold(3);
    assert_eq!(session.code(2).offset, Point::default());
    assert_eq!(session.outer(), outer);
    session.up(session.text_point(SHORT, SHORT.len()));
    session.begin_right();
    session.hold(2);
    let sibling = session.code(1);
    session.drag(point(
        sibling.visible.right() - px(1.),
        session.text_point(SIBLING, 0).y,
    ));
    let stopped = session.code(0).offset;
    let others = session.other_offsets();
    session.hold(3);
    assert_eq!(session.code(0).offset, stopped);
    assert_eq!(session.other_offsets(), others);
    assert_eq!(session.outer(), outer);
    session.up(session.text_point(SIBLING, 0));
}

#[gpui_kit::test]
fn quote_wrapping_code_nowrap_manual_wheel_and_vertical_reading_are_preserved(
    cx: &mut TestAppContext,
) {
    let mut session = Session::new(cx, Surface::Answer);
    session.visual.update(|_, cx| {
        session.view.update(cx, |view, cx| {
            view.messages = vec![MessageFixture::new(
                &format!("> {QUOTE}\n\n```text\n{CODE}\n```\n"),
                cx,
            )];
            cx.notify();
        });
    });
    session.owner = session
        .visual
        .read(|cx| session.view.read(cx).messages[0].copy.id);
    session.frame();
    let quote = session.text(QUOTE);
    assert!(quote.bounds.size.height > quote.layout.line_height() * 2.);
    assert!(quote.bounds.size.width <= session.code(0).bounds.size.width + px(24.));
    let code = session.text(CODE);
    assert_eq!(
        code.layout.position_for_index(0).unwrap().y,
        code.layout.position_for_index(CODE.len()).unwrap().y
    );
    assert!(code.bounds.size.width > session.code(0).bounds.size.width);
    assert!(
        code.bounds.size.height <= px(Typography::CODE * Typography::CODE_LINE_HEIGHT) + px(1.)
    );
    let original = session.code(0).offset;
    session.wheel(session.code(0).visible.center(), -120., 0.);
    let moved = session.code(0).offset;
    assert!(moved.x < original.x);
    session.hold(3);
    assert_eq!(session.code(0).offset, moved);
    let before = session.outer();
    session.wheel(point(px(240.), px(500.)), 0., -80.);
    assert!(session.outer().y < before.y);
    assert_eq!(session.code(0).offset.x, moved.x);
}

fn assert_code_vertical_handoff(cx: &mut TestAppContext, surface: Surface) {
    let rows = (0..48)
        .map(|index| {
            format!(
                "row_{index:02} = 中文 value_{index:02}；{}",
                "0123456789_".repeat(16)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!("```text\n{CODE}\n{rows}\n```\n");
    let mut session = Session::with_source(cx, surface, Some(&source));
    let anchor = CODE.find("alpha_01").unwrap();
    let expected = format!("{}\n{rows}", &CODE[anchor..]);
    let mut failures = Vec::new();
    for corner in [false, true] {
        let viewport = session.vertical();
        session.wheel(viewport.bounds.center(), 0., 100_000.);
        let viewport = session.vertical();
        assert_eq!(viewport.offset.y, px(0.));
        assert!(viewport.extent.y > viewport.bounds.size.height);
        let code = session.code(0);
        assert!(code.extent.x > px(0.));
        let outer = session.outer();
        let position = point(
            if corner {
                code.visible.right() - px(1.)
            } else {
                code.visible.center().x
            },
            viewport.bounds.bottom() + px(1.),
        );
        let wheel_target = point(position.x, viewport.bounds.bottom() - px(1.));
        assert!(code.visible.contains(&wheel_target));
        session.down(session.text_point(CODE, anchor));
        session.drag(position);
        session.tick();
        let first = session.vertical().offset;
        let first_text = session.copied();
        session.tick();
        let second = session.vertical().offset;
        let second_text = session.copied();
        if !(first.y < viewport.offset.y && second.y < first.y) {
            failures.push(format!(
                "corner={corner}: vertical code-edge hold must move the related y viewport on successive ticks: {:?}, {first:?}, {second:?}",
                viewport.offset
            ));
        }
        if first.x != viewport.offset.x
            || second.x != viewport.offset.x
            || session.code(0).offset != code.offset
        {
            failures.push(format!(
                "corner={corner}: vertical selection wheel was remapped to code x"
            ));
        }
        if first_text.is_empty()
            || !expected.starts_with(&first_text)
            || !expected.starts_with(&second_text)
            || !second_text.starts_with(&first_text)
            || second_text.len() <= first_text.len()
        {
            failures.push(format!(
                "corner={corner}: vertical hold did not extend the original logical code selection"
            ));
        }
        if matches!(surface, Surface::Thinking) && session.outer() != outer {
            failures.push(format!("corner={corner}: Thinking selection scrolled the outer conversation instead of its own viewport"));
        }
        session.up(position);
        let stopped = session.vertical().offset;
        let selected = session.copied();
        session.hold(3);
        if session.vertical().offset != stopped || session.copied() != selected {
            failures.push(format!(
                "corner={corner}: vertical selection kept changing after release"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[gpui_kit::test]
fn answer_code_vertical_edges_and_corners_keep_y_scroll_without_x_remapping(
    cx: &mut TestAppContext,
) {
    assert_code_vertical_handoff(cx, Surface::Answer);
}

#[gpui_kit::test]
fn thinking_code_vertical_edges_and_corners_use_the_inner_y_viewport(cx: &mut TestAppContext) {
    assert_code_vertical_handoff(cx, Surface::Thinking);
}

#[gpui_kit::test]
fn non_left_code_drag_never_starts_selection_or_horizontal_scrolling(cx: &mut TestAppContext) {
    for surface in [Surface::Answer, Surface::Thinking] {
        let mut session = Session::new(cx, surface);
        let initial = session.window_offsets();
        let outer = session.outer();
        let anchor = session.text_point(CODE, CODE.find("alpha_01").unwrap());
        let edge = session.edge(true);
        session
            .visual
            .simulate_mouse_move(anchor, None, Modifiers::none());
        session
            .visual
            .simulate_mouse_down(anchor, MouseButton::Middle, Modifiers::none());
        session
            .visual
            .simulate_mouse_move(edge, MouseButton::Middle, Modifiers::none());
        session.hold(3);
        assert_eq!(session.window_offsets(), initial);
        assert_eq!(session.outer(), outer);
        assert!(!session.visual.update(TextSelection::has_selection));
        session
            .visual
            .simulate_mouse_up(edge, MouseButton::Middle, Modifiers::none());
        session.hold(3);
        assert_eq!(session.window_offsets(), initial);
        assert!(!session.visual.update(TextSelection::has_selection));
    }
}

#[gpui_kit::test]
fn identical_thinking_code_ids_in_two_windows_keep_drags_and_observations_isolated(
    cx: &mut TestAppContext,
) {
    let mut first = Session::new(cx, Surface::Thinking);
    let first_initial = first.window_offsets();
    let mut second = Session::new(cx, Surface::Thinking);
    assert_ne!(first.window, second.window);
    assert_ne!(first.owner, second.owner);
    assert_eq!(first.code(0).id, second.code(0).id);
    assert_eq!(first.window_offsets(), first_initial);
    first.visual.read(|cx| {
        let windows = &cx.global::<Observations>().windows;
        assert!(windows[&first.window].documents.contains_key(&first.owner));
        assert!(
            windows[&second.window]
                .documents
                .contains_key(&second.owner)
        );
        assert!(!windows[&first.window].documents.contains_key(&second.owner));
        assert!(!windows[&second.window].documents.contains_key(&first.owner));
    });
    first.visual.update(|window, _| window.activate_window());
    first.frame();
    let second_before = second.window_offsets();
    let second_text_bounds = second.text(CODE).bounds;
    let second_outer = second.outer();
    let anchor = first.begin_right();
    first.assert_progress(true);
    let selected = first.copied();
    assert!(!selected.is_empty() && CODE[anchor..].starts_with(&selected));
    second.frame();
    assert_eq!(second.window_offsets(), second_before);
    assert_eq!(second.text(CODE).bounds, second_text_bounds);
    assert_eq!(second.outer(), second_outer);
    first.up(first.edge(true));
    second.visual.update(|window, _| window.activate_window());
    second.frame();
    let first_before = first.window_offsets();
    let first_text_bounds = first.text(CODE).bounds;
    let first_outer = first.outer();
    let anchor = second.begin_right();
    second.assert_progress(true);
    let selected = second.copied();
    assert!(!selected.is_empty() && CODE[anchor..].starts_with(&selected));
    first.frame();
    assert_eq!(first.window_offsets(), first_before);
    assert_eq!(first.text(CODE).bounds, first_text_bounds);
    assert_eq!(first.outer(), first_outer);
    second.up(second.edge(true));
}

#[gpui_kit::test]
fn ordinary_vertical_text_selection_uses_real_events_and_exact_clipboard_order(
    cx: &mut TestAppContext,
) {
    let mut session = Session::new(cx, Surface::Answer);
    session.visual.update(|_, cx| {
        session.view.update(cx, |view, cx| {
            view.messages = vec![MessageFixture::new(&PARAGRAPHS.join("\n\n"), cx)];
            cx.notify();
        });
    });
    session.owner = session
        .visual
        .read(|cx| session.view.read(cx).messages[0].copy.id);
    session.frame();
    let outer = session.outer();
    let start = PARAGRAPHS[0].find("paragraph").unwrap();
    let end = PARAGRAPHS[2].find("completes").unwrap();
    session.down(session.text_point(PARAGRAPHS[0], start));
    session.drag(session.text_point(PARAGRAPHS[2], end));
    session.up(session.text_point(PARAGRAPHS[2], end));
    assert_eq!(
        session.copied(),
        format!(
            "{}\n\n{}\n\n{}",
            &PARAGRAPHS[0][start..],
            PARAGRAPHS[1],
            &PARAGRAPHS[2][..end]
        )
    );
    session.hold(3);
    assert_eq!(session.outer(), outer);
}

#[gpui_kit::test]
fn code_copy_preserves_empty_lines_and_emoji_on_both_surfaces(cx: &mut TestAppContext) {
    for surface in [Surface::Answer, Surface::Thinking] {
        let source = format!("```text\n{CODE}\n\n中间👩🏽‍💻 = 原文；\n\n终行🚀。\n```\n");
        let mut session = Session::with_source(cx, surface, Some(&source));
        let anchor = CODE.find("alpha_01").unwrap();
        let end = "终行🚀。";
        session.down(session.text_point(CODE, anchor));
        session.drag(session.text_point(end, end.len()));
        session.up(session.text_point(end, end.len()));
        assert_eq!(
            session.copied(),
            format!("{}\n\n中间👩🏽‍💻 = 原文；\n\n终行🚀。", &CODE[anchor..])
        );
    }
}

#[gpui_kit::test]
fn adjacent_message_keys_copy_exact_utf8_and_replacement_invalidates_old_selection(
    cx: &mut TestAppContext,
) {
    let mut session = Session::new(cx, Surface::Answer);
    let first = "首条🙂正文，保留后缀。";
    let second = "第二条👩🏽‍💻正文，止于这里🚀。";
    session.visual.update(|_, cx| {
        session.view.update(cx, |view, cx| {
            view.messages = vec![
                MessageFixture::new(first, cx),
                MessageFixture::new(second, cx),
            ];
            cx.notify();
        });
    });
    session.owner = session
        .visual
        .read(|cx| session.view.read(cx).messages[0].copy.id);
    session.frame();
    let first_start = first.find('🙂').unwrap();
    session.down(session.text_point(first, first_start));
    session.drag(session.text_point(first, first.len()));
    session.up(session.text_point(first, first.len()));
    assert_eq!(session.copied(), &first[first_start..]);
    let second_owner = session
        .visual
        .read(|cx| session.view.read(cx).messages[1].copy.id);
    session.owner = second_owner;
    let second_end = second.find('🚀').unwrap();
    let cursor = session.text_point(second, second_end);
    session.down(session.text_point(second, 0));
    session.drag(cursor);
    session.up(cursor);
    assert_eq!(session.copied(), &second[..second_end]);
    assert_eq!(
        session.visual.update(TextSelection::selected_text),
        &second[..second_end]
    );
    assert!(
        !session
            .visual
            .read(|cx| session.view.read(cx).messages[0].copy.has_selected_text())
    );
    session.visual.update(|_, cx| {
        session.view.update(cx, |view, cx| {
            view.messages[1].copy.replace_text("替换后的新正文🙂。");
            view.messages[1].model = MessageFixture::new("替换后的新正文🙂。", cx).model;
            cx.notify();
        });
    });
    session.frame();
    assert!(!session.visual.update(TextSelection::has_selection));
    assert!(
        !session
            .visual
            .read(|cx| session.view.read(cx).messages[1].copy.has_selected_text())
    );
}

#[gpui_kit::test]
fn pending_cursor_refresh_is_rejected_after_controller_end_or_content_replacement(
    cx: &mut TestAppContext,
) {
    for replace in [false, true] {
        let mut session = Session::new(cx, Surface::Answer);
        session.begin_right();
        let id = ElementId::from((session.code(0).id, "code-drag"));
        let driver = session.visual.read(|cx| {
            cx.global::<Observations>().windows[&session.window].drivers[&id]
                .upgrade()
                .unwrap()
        });
        session.visual.update(|window, cx| {
            super::super::code_drag::verify_pending_refresh_cancellation(
                &driver, replace, window, cx,
            );
        });
        session.up(session.edge(true));
        let stopped = session.code(0).offset;
        session.hold(3);
        assert_eq!(session.code(0).offset, stopped);
    }
}
