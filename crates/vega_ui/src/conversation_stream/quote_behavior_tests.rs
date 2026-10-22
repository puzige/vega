use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Bounds, InputEvent, TestAppContext, TextLayout, WindowHandle, point, size};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
pub(crate) struct PaintedText {
    pub(crate) text: String,
    pub(crate) layout: TextLayout,
    pub(crate) runs: Vec<gpui_kit::TextRun>,
}

thread_local! {
    static TEXT: RefCell<Vec<PaintedText>> = const { RefCell::new(Vec::new()) };
    static NODES: RefCell<Option<Vec<RenderNode>>> = const { RefCell::new(None) };
}

pub(crate) fn model_input(model: &mut StreamModel) {
    NODES.with_borrow(|nodes| {
        if let Some(nodes) = nodes {
            model.committed_lines.clear();
            model.pending_lines = model::flatten_nodes(7, nodes, model::BlockOrigin::Pending);
        }
    });
}

pub(super) fn observe_text(
    text: &str,
    layout: &TextLayout,
    runs: &[gpui_kit::TextRun],
    window: &Window,
) {
    if layout
        .bounds()
        .intersect(&window.content_mask().bounds)
        .is_empty()
    {
        return;
    }
    TEXT.with_borrow_mut(|observations| {
        observations.push(PaintedText {
            text: text.to_owned(),
            layout: layout.clone(),
            runs: runs.to_vec(),
        })
    });
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Path {
    Ordinary,
    Selectable,
    Thinking,
}

const PATHS: [Path; 3] = [Path::Ordinary, Path::Selectable, Path::Thinking];

pub(super) struct QuoteView {
    stream: MarkdownStream,
    model: StreamModel,
    source: String,
    pub(super) copy: MessageCopy,
    focus: FocusHandle,
    path: Path,
    width: f32,
    height: f32,
    offset: f32,
    nodes: Option<Vec<RenderNode>>,
    horizontal: Rc<RefCell<HashMap<usize, ScrollHandle>>>,
}

impl QuoteView {
    fn sync(&mut self) {
        self.model
            .sync(&self.stream.snapshot(), &StreamCounters::default());
        self.copy.replace_text(&self.source);
    }
}

impl Render for QuoteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        NODES.with_borrow_mut(|nodes| *nodes = self.nodes.clone());
        let body = match self.path {
            Path::Ordinary => render_rows::markdown_item(&self.model, None, &colors, "quote"),
            Path::Selectable => render_rows::selectable_markdown_content(
                &self.model,
                &self.copy,
                &self.focus,
                &colors,
                false,
                window,
                cx,
            ),
            Path::Thinking => thinking::quote_test_body(
                &self.source,
                px(self.width),
                px(self.offset),
                self.copy.clone(),
                self.focus.clone(),
                self.horizontal.clone(),
                window,
                cx,
            ),
        };
        NODES.with_borrow_mut(|nodes| *nodes = None);
        div()
            .w(px(self.width))
            .h(px(self.height))
            .overflow_hidden()
            .text_size(px(Typography::CONVERSATION))
            .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
            .child(gpui_kit::base::TextSelectionLayer)
            .child(body)
    }
}

pub(super) fn open(
    cx: &mut TestAppContext,
    path: Path,
    source: &str,
    width: f32,
) -> WindowHandle<QuoteView> {
    cx.update(|cx| {
        if !cx.has_global::<vega_theme::Theme>() {
            gpui_kit::init(cx);
            cx.set_global(vega_theme::Theme::light());
        }
    });
    let source = source.to_owned();
    cx.open_window(size(px(width), px(12000.)), move |_, cx| {
        let mut view = QuoteView {
            stream: MarkdownStream::new(),
            model: StreamModel::default(),
            copy: MessageCopy::new(&source),
            focus: cx.focus_handle(),
            source,
            path,
            width,
            height: 12000.,
            offset: 0.,
            nodes: None,
            horizontal: Default::default(),
        };
        view.stream.append(&view.source);
        view.sync();
        view
    })
}

#[derive(Debug, Clone, PartialEq)]
struct Rect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

pub(crate) struct Frame {
    pub(crate) text: Vec<PaintedText>,
    rails: Vec<Rect>,
}

pub(crate) fn frame<V: Render + 'static>(
    handle: WindowHandle<V>,
    cx: &mut TestAppContext,
) -> Frame {
    TEXT.with_borrow_mut(Vec::clear);
    let rails = cx
        .update_window(handle.into(), |_, window, cx| {
            window.refresh();
            window.render_frame(cx);
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .filter_map(|quad| {
                    if quad.background.as_solid() != Some(vega_theme::LIGHT.border_subtle.into()) {
                        return None;
                    }
                    let clipped = quad.bounds.intersect(&quad.content_mask.bounds);
                    let width = quad.bounds.size.width.as_f32() / scale;
                    if clipped.is_empty() || (width - 2.).abs() > 0.1 {
                        return None;
                    }
                    Some(Rect {
                        left: clipped.left().as_f32() / scale,
                        top: clipped.top().as_f32() / scale,
                        right: clipped.right().as_f32() / scale,
                        bottom: clipped.bottom().as_f32() / scale,
                    })
                })
                .collect()
        })
        .unwrap();
    Frame {
        text: TEXT.with_borrow(Clone::clone),
        rails,
    }
}

fn dispatch_pointer<V: Render + 'static>(
    handle: WindowHandle<V>,
    cx: &mut TestAppContext,
    event: impl InputEvent,
) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.dispatch_event(event.to_platform_input(), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

pub(crate) fn drag_selection<V: Render + 'static>(
    handle: WindowHandle<V>,
    cx: &mut TestAppContext,
    from: gpui_kit::Point<Pixels>,
    to: gpui_kit::Point<Pixels>,
) {
    cx.update_window(handle.into(), |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
    dispatch_pointer(
        handle,
        cx,
        gpui_kit::MouseMoveEvent {
            position: from,
            pressed_button: None,
            modifiers: Default::default(),
        },
    );
    dispatch_pointer(
        handle,
        cx,
        gpui_kit::MouseDownEvent {
            position: from,
            button: MouseButton::Left,
            click_count: 1,
            modifiers: Default::default(),
            first_mouse: false,
        },
    );
    for step in 1..=8 {
        let fraction = step as f32 / 8.;
        dispatch_pointer(
            handle,
            cx,
            gpui_kit::MouseMoveEvent {
                position: point(
                    from.x + (to.x - from.x) * fraction,
                    from.y + (to.y - from.y) * fraction,
                ),
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            },
        );
    }
    dispatch_pointer(
        handle,
        cx,
        gpui_kit::MouseUpEvent {
            position: to,
            button: MouseButton::Left,
            click_count: 1,
            modifiers: Default::default(),
        },
    );
}

pub(crate) fn text_bounds(frame: &Frame, needle: &str) -> Bounds<Pixels> {
    let matches: Vec<_> = frame
        .text
        .iter()
        .filter(|text| text.text.contains(needle))
        .collect();
    assert!(!matches.is_empty(), "missing painted {needle:?}");
    matches
        .into_iter()
        .map(|text| text.layout.bounds())
        .reduce(|a, b| a.union(&b))
        .unwrap()
}

fn covered(frame: &Frame, x: f32, top: f32, bottom: f32) {
    let mut spans: Vec<_> = frame
        .rails
        .iter()
        .filter(|rail| (rail.left - x).abs() < 0.2)
        .map(|rail| (rail.top, rail.bottom))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = top;
    for (start, next) in spans {
        if next < end {
            continue;
        }
        assert!(
            start <= end + 0.2,
            "unpainted rail gap at {x}: {end}..{start}"
        );
        end = end.max(next);
        if end >= bottom - 0.2 {
            return;
        }
    }
    panic!(
        "rail at {x} ends at {end}, expected {bottom}; {:?}",
        frame.rails
    );
}

fn absent(frame: &Frame, x: f32, top: f32, bottom: f32) {
    assert!(
        !frame.rails.iter().any(|rail| (rail.left - x).abs() < 0.2
            && rail.top < bottom - 0.2
            && rail.bottom > top + 0.2),
        "unexpected rail at {x} in {top}..{bottom}: {:?}",
        frame.rails
    );
}

fn outer_x(frame: &Frame) -> f32 {
    frame
        .rails
        .iter()
        .map(|rail| rail.left)
        .min_by(f32::total_cmp)
        .expect("painted quote rail")
}

pub(super) fn continuous(frame: &Frame, first: &str, last: &str) {
    let first = text_bounds(frame, first);
    let last = text_bounds(frame, last);
    covered(
        frame,
        outer_x(frame),
        f32::from(first.top()),
        f32::from(last.bottom()),
    );
}

#[gpui_kit::test]
fn paragraph_rails_cover_preserved_gaps(cx: &mut TestAppContext) {
    for path in PATHS {
        let handle = open(
            cx,
            path,
            "> 第一段 alpha\n>\n> 第二段 beta\n>\n> 第三段 gamma",
            600.,
        );
        let painted = frame(handle, cx);
        for (before, after) in [("第一段", "第二段"), ("第二段", "第三段")] {
            let before = text_bounds(&painted, before);
            let after = text_bounds(&painted, after);
            assert!(
                (f32::from(after.top() - before.bottom()) - 16.).abs() < 0.2,
                "paragraph gap changed in {path:?}"
            );
        }
        continuous(&painted, "第一段", "第三段");
    }
}

#[gpui_kit::test]
fn independent_quotes_do_not_decorate_intervening_body(cx: &mut TestAppContext) {
    for path in PATHS {
        let handle = open(cx, path, "> first\n\nordinary\n\n> second", 600.);
        let painted = frame(handle, cx);
        let body = text_bounds(&painted, "ordinary");
        absent(
            &painted,
            outer_x(&painted),
            f32::from(body.top()),
            f32::from(body.bottom()),
        );
        continuous(&painted, "first", "first");
        continuous(&painted, "second", "second");
    }
}

#[gpui_kit::test]
fn nested_siblings_share_only_the_outer_rail(cx: &mut TestAppContext) {
    for path in PATHS {
        let source = "> outer-start\n>\n> > inner-one\n> >\n> > inner-two\n>\n> outer-middle\n>\n> > sibling\n>\n> outer-end";
        let handle = open(cx, path, source, 600.);
        let painted = frame(handle, cx);
        continuous(&painted, "outer-start", "outer-end");
        let outer = outer_x(&painted);
        let inner = outer + 2. + Layout::MARKDOWN_CODE_PADDING;
        let one = text_bounds(&painted, "inner-one");
        let two = text_bounds(&painted, "inner-two");
        covered(
            &painted,
            inner,
            f32::from(one.top()),
            f32::from(two.bottom()),
        );
        let middle = text_bounds(&painted, "outer-middle");
        absent(
            &painted,
            inner,
            f32::from(two.bottom()) + 0.3,
            f32::from(middle.bottom()),
        );
        let sibling = text_bounds(&painted, "sibling");
        covered(
            &painted,
            inner,
            f32::from(sibling.top()),
            f32::from(sibling.bottom()),
        );
        let end = text_bounds(&painted, "outer-end");
        absent(
            &painted,
            inner,
            f32::from(sibling.bottom()) + 0.3,
            f32::from(end.bottom()),
        );
        assert_eq!(one.left(), sibling.left());
        assert_eq!(middle.left(), end.left());
        assert!(one.left() > middle.left());
    }
}

#[gpui_kit::test]
fn wrapping_and_resize_use_remaining_quote_width(cx: &mut TestAppContext) {
    let source = "> 中文与 English 混排内容需要自然换行，不得超出右侧边界。 repeated words 中文继续自然换行。";
    for path in PATHS {
        let handle = open(cx, path, source, 240.);
        let narrow = frame(handle, cx);
        let bounds = all_text_bounds(&narrow);
        assert!(
            bounds.right() <= px(240.1),
            "{path:?} overflows: {bounds:?}"
        );
        assert!(
            bounds.size.height
                > px(Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT)
        );
        covered(
            &narrow,
            outer_x(&narrow),
            f32::from(bounds.top()),
            f32::from(bounds.bottom()),
        );
        cx.simulate_window_resize(handle.into(), size(px(760.), px(12000.)));
        handle
            .update(cx, |view, _, cx| {
                view.width = 760.;
                cx.notify();
            })
            .unwrap();
        let wide = frame(handle, cx);
        assert!(all_text_bounds(&wide).size.height < bounds.size.height);
        covered(
            &wide,
            outer_x(&wide),
            f32::from(all_text_bounds(&wide).top()),
            f32::from(all_text_bounds(&wide).bottom()),
        );
        let bottom = f32::from(all_text_bounds(&wide).bottom());
        assert!(wide.rails.iter().all(|rail| rail.bottom <= bottom + 0.2));
        let repeated = frame(handle, cx);
        assert_eq!(wide.rails, repeated.rails);
    }
}

#[gpui_kit::test]
fn streamed_intermediate_and_finished_frames_match_fresh_projection(cx: &mut TestAppContext) {
    for path in PATHS {
        let handle = open(cx, path, "> start", 480.);
        let mut source = String::from("> start");
        for delta in [
            "\n>\n> > nested",
            "\n> >\n> > next",
            "\n>\n> tail",
            "\n\nplain\n\n> separate",
        ] {
            source.push_str(delta);
            handle
                .update(cx, |view, _, cx| {
                    view.source.push_str(delta);
                    view.stream.append(delta);
                    view.sync();
                    cx.notify();
                })
                .unwrap();
            let current = frame(handle, cx);
            let fresh_handle = open(cx, path, &source, 480.);
            let fresh = frame(fresh_handle, cx);
            assert_eq!(
                current.rails, fresh.rails,
                "stale streamed rails in {path:?}"
            );
            assert_eq!(
                current
                    .text
                    .iter()
                    .map(|text| &text.text)
                    .collect::<Vec<_>>(),
                fresh.text.iter().map(|text| &text.text).collect::<Vec<_>>()
            );
            let end = if source.contains("tail") {
                "tail"
            } else if source.contains("next") {
                "next"
            } else {
                "nested"
            };
            continuous(&current, "start", end);
        }
        handle
            .update(cx, |view, _, cx| {
                view.stream.finish();
                view.sync();
                cx.notify();
            })
            .unwrap();
        let finished = frame(handle, cx);
        let fresh_handle = open(cx, path, &source, 480.);
        let fresh = frame(fresh_handle, cx);
        assert_eq!(finished.rails, fresh.rails);
        assert_eq!(finished.text.len(), fresh.text.len());
    }
}

#[gpui_kit::test]
fn mixed_quote_children_keep_content_layout_and_copy_separators(cx: &mut TestAppContext) {
    for path in [Path::Selectable, Path::Thinking] {
        for (source, expected) in [
            (
                "> 起点🙂\n>\n> - 项目甲\n> - 项目乙\n>\n> 终点é",
                "起点🙂\n\n• 项目甲\n• 项目乙\n\n终点é",
            ),
            (
                "> 起点🙂\n>\n> ```text\n> alpha\n> beta\n> ```\n>\n> ```text\n> gamma\n> ```\n>\n> 终点é",
                "起点🙂\n\nalpha\nbeta\n\ngamma\n\n终点é",
            ),
        ] {
            let handle = open(cx, path, source, 600.);
            let painted = frame(handle, cx);
            continuous(&painted, "起点", "终点");
            assert_eq!(
                text_bounds(&painted, "起点").left(),
                text_bounds(&painted, "终点").left()
            );
            let first = painted.text.first().expect("first selection run");
            let last = painted.text.last().expect("last selection run");
            let from = first.layout.position_for_index(0).expect("selection start")
                + point(px(0.1), px(3.));
            let to = last
                .layout
                .position_for_index(last.text.len())
                .expect("selection end")
                + point(px(0.1), px(3.));
            let copy = handle.update(cx, |view, _, _| view.copy.clone()).unwrap();
            drag_selection(handle, cx, from, to);
            cx.update(|cx| copy.copy_selected_text(cx));
            assert_eq!(
                cx.read_from_clipboard()
                    .and_then(|item| item.text())
                    .as_deref(),
                Some(expected),
                "{path:?}"
            );
        }
    }
}

#[gpui_kit::test]
fn quote_links_request_original_destination(cx: &mut TestAppContext) {
    for path in PATHS {
        let url = format!("https://example.invalid/{path:?}?q=%E4%B8%AD");
        let source = format!("> before\n>\n> [目标🙂]({url})\n>\n> after");
        let handle = open(cx, path, &source, 600.);
        let painted = frame(handle, cx);
        let target = text_bounds(&painted, "目标");
        cx.update_window(handle.into(), |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        dispatch_pointer(
            handle,
            cx,
            gpui_kit::MouseMoveEvent {
                position: target.center(),
                ..Default::default()
            },
        );
        dispatch_pointer(
            handle,
            cx,
            gpui_kit::MouseDownEvent {
                position: target.center(),
                button: MouseButton::Left,
                click_count: 1,
                modifiers: Default::default(),
                first_mouse: false,
            },
        );
        dispatch_pointer(
            handle,
            cx,
            gpui_kit::MouseUpEvent {
                position: target.center(),
                button: MouseButton::Left,
                click_count: 1,
                modifiers: Default::default(),
            },
        );
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(501));
        cx.run_until_parked();
        assert_eq!(cx.opened_url().as_deref(), Some(url.as_str()));
    }
}

#[gpui_kit::test]
fn thinking_rails_survive_offscreen_first_paragraph_and_gap_only_mask(cx: &mut TestAppContext) {
    let source = format!(
        "> {}\n>\n> later\n>\n> final",
        "long paragraph words ".repeat(90)
    );
    let handle = open(cx, Path::Thinking, &source, 300.);
    let full = frame(handle, cx);
    let later = text_bounds(&full, "later");
    let x = outer_x(&full);
    let offset = f32::from(later.top()) - 4.;
    assert!(offset > 2. * Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT);
    handle
        .update(cx, |view, _, cx| {
            view.offset = offset;
            view.height = 2.;
            cx.notify();
        })
        .unwrap();
    let gap = frame(handle, cx);
    assert!(gap.text.is_empty(), "gap-only mask should not paint text");
    covered(&gap, x, 0., 2.);
    handle
        .update(cx, |view, _, cx| {
            view.offset = f32::from(later.top());
            view.height = 90.;
            cx.notify();
        })
        .unwrap();
    let scrolled = frame(handle, cx);
    let later = text_bounds(&scrolled, "later");
    assert!(f32::from(later.top()).abs() < 0.2);
    covered(
        &scrolled,
        x,
        0.,
        f32::from(text_bounds(&scrolled, "final").bottom()).min(90.),
    );
    cx.simulate_window_resize(handle.into(), size(px(640.), px(12000.)));
    handle
        .update(cx, |view, _, cx| {
            view.width = 640.;
            view.offset = 0.;
            view.height = 12000.;
            cx.notify();
        })
        .unwrap();
    let resized = frame(handle, cx);
    continuous(&resized, "long paragraph", "final");
    assert!(text_bounds(&resized, "final").bottom() < text_bounds(&full, "final").bottom());
}

#[gpui_kit::test]
fn thinking_first_paragraph_is_beyond_culling_overscan(cx: &mut TestAppContext) {
    let source = format!(
        "> first-short\n>\n> {}\n>\n> end-marker",
        "later 中文 words ".repeat(100)
    );
    let handle = open(cx, Path::Thinking, &source, 280.);
    let full = frame(handle, cx);
    let first = text_bounds(&full, "first-short");
    let later_top = full
        .text
        .iter()
        .filter(|run| run.text.contains("later"))
        .map(|run| run.layout.bounds().top())
        .min()
        .unwrap();
    let overscan = Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT;
    let offset = f32::from(later_top) + 4. * overscan;
    assert!(f32::from(first.bottom()) - offset < -overscan - 1.);
    handle
        .update(cx, |view, _, cx| {
            view.offset = offset;
            view.height = 80.;
            cx.notify();
        })
        .unwrap();
    let clipped = frame(handle, cx);
    assert!(
        !clipped
            .text
            .iter()
            .any(|run| run.text.contains("first-short"))
    );
    assert!(!clipped.text.is_empty());
    covered(&clipped, outer_x(&full), 0., 80.);
}

#[gpui_kit::test]
fn soft_wrapped_unicode_copy_has_only_source_paragraph_breaks(cx: &mut TestAppContext) {
    let paragraph = "中文🙂 English é words 混排继续 "
        .repeat(12)
        .trim_end()
        .to_owned();
    let source = format!("> {paragraph}\n>\n> next终点");
    let expected = format!("{paragraph}\n\nnext终点");
    for path in [Path::Selectable, Path::Thinking] {
        let handle = open(cx, path, &source, 200.);
        for width in [200., 700., 200.] {
            cx.simulate_window_resize(handle.into(), size(px(width), px(12000.)));
            handle
                .update(cx, |view, _, cx| {
                    view.width = width;
                    cx.notify();
                })
                .unwrap();
            let painted = frame(handle, cx);
            let first = painted.text.first().unwrap();
            let last = painted.text.last().unwrap();
            let first_y = first.layout.position_for_index(0).unwrap().y;
            let wrapped_bottom = painted
                .text
                .iter()
                .filter(|run| !run.text.contains("next终点"))
                .map(|run| run.layout.bounds().bottom())
                .max()
                .unwrap();
            assert!(
                wrapped_bottom - first_y
                    > px(2. * Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT)
            );
            let from = first.layout.position_for_index(0).unwrap() + point(px(0.1), px(3.));
            let to =
                last.layout.position_for_index(last.text.len()).unwrap() + point(px(0.1), px(3.));
            let copy = handle.update(cx, |view, _, _| view.copy.clone()).unwrap();
            drag_selection(handle, cx, from, to);
            cx.update(|cx| copy.copy_selected_text(cx));
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some(expected.clone()),
                "{path:?} width={width}"
            );
        }
    }
}

#[gpui_kit::test]
fn quoted_code_horizontal_scroll_moves_code_not_quote_or_paragraph(cx: &mut TestAppContext) {
    let code = format!("HEAD{}TAIL", "abcdefghijklmnopqrstuvwxyz".repeat(14));
    let source =
        format!("> stationary-before\n>\n> ```text\n> {code}\n> ```\n>\n> stationary-after");
    for path in PATHS {
        let handle = open(cx, path, &source, 220.);
        let before = frame(handle, cx);
        let paragraph = text_bounds(&before, "stationary-before");
        let code_run = before
            .text
            .iter()
            .find(|run| run.text.contains("HEAD"))
            .unwrap();
        let head_x = code_run.layout.position_for_index(0).unwrap().x;
        let tail_x = code_run
            .layout
            .position_for_index(code_run.text.find("TAIL").unwrap())
            .unwrap()
            .x;
        assert!(tail_x > px(220.) + px(100.));
        let position = point(px(110.), code_run.layout.bounds().center().y);
        cx.update_window(handle.into(), |_, window, cx| {
            window.dispatch_event(
                gpui_kit::MouseMoveEvent {
                    position,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                gpui_kit::ScrollWheelEvent {
                    position,
                    delta: gpui_kit::ScrollDelta::Pixels(point(-(tail_x - px(160.)), px(0.))),
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
        })
        .unwrap();
        let after = frame(handle, cx);
        assert_eq!(text_bounds(&after, "stationary-before"), paragraph);
        assert_eq!(after.rails, before.rails);
        let tail = after
            .text
            .iter()
            .find(|run| run.text.contains("TAIL"))
            .unwrap();
        let moved_tail = tail
            .layout
            .position_for_index(tail.text.find("TAIL").unwrap())
            .unwrap()
            .x;
        eprintln!(
            "quote scroll {path:?}: wheel_target={position:?}, delta_x={:?}, before_bounds={:?}, after_bounds={:?}, head_before={head_x:?}, tail_before={tail_x:?}, tail_after={moved_tail:?}, code_before={:?}, code_after={:?}",
            -(tail_x - px(160.)),
            code_run.layout.bounds(),
            tail.layout.bounds(),
            code_run.text,
            tail.text,
        );
        assert!(moved_tail >= px(0.) && moved_tail < px(220.));
        assert!(moved_tail < tail_x - px(100.));
        if let Some(head) = after.text.iter().find(|run| run.text.contains("HEAD")) {
            assert!(head.layout.position_for_index(0).unwrap().x < head_x - px(100.));
        }
    }
}

#[gpui_kit::test]
fn unquoted_body_and_code_have_no_quote_rails(cx: &mut TestAppContext) {
    for path in PATHS {
        let handle = open(
            cx,
            path,
            "ordinary\n\n```text\ncode words stay on one physical line\n```",
            240.,
        );
        let painted = frame(handle, cx);
        assert!(painted.rails.is_empty());
        let code = text_bounds(&painted, "code words");
        assert!(code.size.height <= px(Typography::CODE * Typography::CODE_LINE_HEIGHT + 0.2));
    }
}

fn all_text_bounds(frame: &Frame) -> Bounds<Pixels> {
    frame
        .text
        .iter()
        .map(|text| text.layout.bounds())
        .reduce(|a, b| a.union(&b))
        .expect("painted text")
}

struct ObservedText {
    content: String,
    text: StyledText,
    runs: Option<Vec<gpui_kit::TextRun>>,
}

pub(super) fn observed_text(
    content: String,
    text: StyledText,
    runs: Option<Vec<gpui_kit::TextRun>>,
) -> AnyElement {
    ObservedText {
        content,
        text,
        runs,
    }
    .into_any_element()
}

impl IntoElement for ObservedText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui_kit::Element for ObservedText {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<gpui_kit::ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&gpui_kit::GlobalElementId>,
        inspector: Option<&gpui_kit::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui_kit::LayoutId, ()) {
        if self.runs.is_none() {
            self.runs = Some(vec![window.text_style().to_run(self.content.len())]);
        }
        self.text.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&gpui_kit::GlobalElementId>,
        inspector: Option<&gpui_kit::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text
            .prepaint(id, inspector, bounds, &mut (), window, cx);
        observe_text(
            &self.content,
            self.text.layout(),
            self.runs.as_deref().expect("rendered text runs"),
            window,
        );
    }
    fn paint(
        &mut self,
        id: Option<&gpui_kit::GlobalElementId>,
        inspector: Option<&gpui_kit::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text
            .paint(id, inspector, bounds, &mut (), &mut (), window, cx);
    }
}

#[gpui_kit::test]
fn sibling_quotes_in_one_stream_block_remain_distinct(cx: &mut TestAppContext) {
    let paragraph = |text: &str| RenderNode::Paragraph {
        spans: vec![Inline::Text(text.to_owned())],
    };
    let nodes = vec![RenderNode::BlockQuote {
        children: vec![
            paragraph("outer-first"),
            RenderNode::BlockQuote {
                children: vec![paragraph("left-sibling")],
            },
            RenderNode::BlockQuote {
                children: vec![paragraph("right-sibling")],
            },
            paragraph("outer-last"),
        ],
    }];
    for path in PATHS {
        let handle = open(cx, path, "", 600.);
        handle
            .update(cx, |view, _, cx| {
                view.nodes = Some(nodes.clone());
                view.model.pending_lines =
                    model::flatten_nodes(7, &nodes, model::BlockOrigin::Pending);
                cx.notify();
            })
            .unwrap();
        let painted = frame(handle, cx);
        continuous(&painted, "outer-first", "outer-last");
        let left = text_bounds(&painted, "left-sibling");
        let right = text_bounds(&painted, "right-sibling");
        assert!(right.top() > left.bottom());
        let inner = outer_x(&painted) + 2. + Layout::MARKDOWN_CODE_PADDING;
        covered(
            &painted,
            inner,
            f32::from(left.top()),
            f32::from(left.bottom()),
        );
        covered(
            &painted,
            inner,
            f32::from(right.top()),
            f32::from(right.bottom()),
        );
        absent(
            &painted,
            inner,
            f32::from(left.bottom()) + 0.3,
            f32::from(right.top()) - 0.3,
        );
    }
}

#[gpui_kit::test]
fn mixed_children_preserve_ordinary_layout(cx: &mut TestAppContext) {
    for path in PATHS {
        for source in [
            "> before\n>\n> - item-one\n> - item-two\n>\n> after",
            "> before\n>\n> ```text\n> code-one\n> code-two\n> ```\n>\n> after",
        ] {
            let handle = open(cx, path, source, 600.);
            let painted = frame(handle, cx);
            continuous(&painted, "before", "after");
            let before = text_bounds(&painted, "before");
            let after = text_bounds(&painted, "after");
            assert_eq!(before.left(), after.left());
            if source.contains("item-one") {
                assert!(text_bounds(&painted, "item-one").left() > before.left());
                assert!(painted.text.iter().any(|text| text.text.contains('•')));
            } else {
                let code = text_bounds(&painted, "code-one");
                assert!(
                    code.size.height <= px(Typography::CODE * Typography::CODE_LINE_HEIGHT + 0.2)
                );
            }
        }
    }
}

#[gpui_kit::test]
fn unicode_nested_selection_tracks_append_and_resize(cx: &mut TestAppContext) {
    for path in [Path::Selectable, Path::Thinking] {
        let handle = open(cx, path, "> 起点🙂\n>\n> > 内层é\n>\n> 终点中", 260.);
        for width in [260., 640.] {
            cx.simulate_window_resize(handle.into(), size(px(width), px(12000.)));
            handle
                .update(cx, |view, _, cx| {
                    view.width = width;
                    cx.notify();
                })
                .unwrap();
            let painted = frame(handle, cx);
            copy_between(handle, cx, &painted, "起点🙂", "终点中");
            assert_eq!(
                cx.read_from_clipboard()
                    .and_then(|item| item.text())
                    .as_deref(),
                Some("起点🙂\n\n内层é\n\n终点中")
            );
        }
        handle
            .update(cx, |view, _, cx| {
                let delta = "\n>\n> 增量Ω";
                view.source.push_str(delta);
                view.stream.append(delta);
                view.sync();
                cx.notify();
            })
            .unwrap();
        let painted = frame(handle, cx);
        copy_between(handle, cx, &painted, "内层é", "增量Ω");
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("内层é\n\n终点中\n\n增量Ω")
        );
    }
}

fn copy_between(
    handle: WindowHandle<QuoteView>,
    cx: &mut TestAppContext,
    painted: &Frame,
    first: &str,
    last: &str,
) {
    let first = painted
        .text
        .iter()
        .find(|text| text.text == first)
        .expect("first text");
    let last = painted
        .text
        .iter()
        .find(|text| text.text == last)
        .expect("last text");
    let from = first.layout.position_for_index(0).unwrap() + point(px(0.1), px(3.));
    let to = last.layout.position_for_index(last.text.len()).unwrap() + point(px(0.1), px(3.));
    let copy = handle.update(cx, |view, _, _| view.copy.clone()).unwrap();
    drag_selection(handle, cx, from, to);
    cx.update(|cx| copy.copy_selected_text(cx));
}

mod container_order_tests {
    use super::*;

    const FIRST: &str =
        "quote-one 中文 mixed words wrap inside the real list and quote ancestor columns";
    const SECOND: &str =
        "quote-two 🙂 more words keep the paragraph visible while the window changes width";

    #[derive(Clone, Copy, Debug)]
    enum Ancestry {
        ListQuote,
        NestedListQuote,
        QuoteList,
        QuoteListQuote,
    }

    fn paragraph(text: &str) -> RenderNode {
        RenderNode::Paragraph {
            spans: vec![Inline::Text(text.to_owned())],
        }
    }

    fn quote(children: Vec<RenderNode>) -> RenderNode {
        RenderNode::BlockQuote { children }
    }

    fn list(children: Vec<RenderNode>) -> RenderNode {
        RenderNode::List(ListBlock {
            ordered: false,
            start: 1,
            items: vec![vega_markdown::ListItem {
                checked: None,
                children,
            }],
        })
    }

    fn fixture(ancestry: Ancestry) -> (String, Vec<RenderNode>) {
        let pair = vec![paragraph(FIRST), paragraph(SECOND)];
        let (source, node) = match ancestry {
            Ancestry::ListQuote => (
                format!("- item\n\n  > {FIRST}\n  >\n  > {SECOND}\n\n"),
                list(vec![paragraph("item"), quote(pair)]),
            ),
            Ancestry::NestedListQuote => (
                format!(
                    "- parent-item\n  - nested-item\n\n    > {FIRST}\n    >\n    > {SECOND}\n\n"
                ),
                list(vec![
                    paragraph("parent-item"),
                    list(vec![paragraph("nested-item"), quote(pair)]),
                ]),
            ),
            Ancestry::QuoteList => (
                format!(
                    "> outer-before\n>\n> - item\n>\n> {FIRST}\n>\n> {SECOND}\n>\n> outer-after\n\n"
                ),
                quote(vec![
                    paragraph("outer-before"),
                    list(vec![paragraph("item")]),
                    paragraph(FIRST),
                    paragraph(SECOND),
                    paragraph("outer-after"),
                ]),
            ),
            Ancestry::QuoteListQuote => (
                format!(
                    "> outer-before\n>\n> - item\n>\n>   > {FIRST}\n>   >\n>   > {SECOND}\n>\n> outer-after\n\n"
                ),
                quote(vec![
                    paragraph("outer-before"),
                    list(vec![paragraph("item"), quote(pair)]),
                    paragraph("outer-after"),
                ]),
            ),
        };
        (
            format!("outside-before\n\n{source}outside-after\n"),
            vec![
                paragraph("outside-before"),
                node,
                paragraph("outside-after"),
            ],
        )
    }

    fn parsed_nodes(stream: &MarkdownStream) -> Vec<RenderNode> {
        let snapshot = stream.snapshot();
        snapshot
            .blocks
            .iter()
            .flat_map(|block| block.nodes.iter())
            .chain(
                snapshot
                    .pending
                    .into_iter()
                    .flat_map(|block| block.nodes.iter()),
            )
            .cloned()
            .collect()
    }

    fn paragraph_bounds(painted: &Frame, expected: &str) -> Bounds<Pixels> {
        let start = painted
            .text
            .iter()
            .position(|run| !run.text.is_empty() && expected.starts_with(run.text.as_str()))
            .expect("painted paragraph start");
        let mut content = String::new();
        let mut bounds: Option<Bounds<Pixels>> = None;
        for run in &painted.text[start..] {
            content.push_str(&run.text);
            assert!(
                expected.starts_with(&content),
                "paragraph changed: {content:?}"
            );
            let next = run.layout.bounds();
            bounds = Some(bounds.map_or(next, |bounds| bounds.union(&next)));
            if content == expected {
                return bounds.expect("painted paragraph bounds");
            }
        }
        panic!("incomplete painted paragraph: {content:?}");
    }

    fn verify_geometry(painted: &Frame, ancestry: Ancestry, width: f32, path: Path) -> f32 {
        let before = text_bounds(painted, "outside-before");
        let after = text_bounds(painted, "outside-after");
        let first = paragraph_bounds(painted, FIRST);
        let second = paragraph_bounds(painted, SECOND);
        let origin = f32::from(before.left());
        let indent = Layout::MARKDOWN_LIST_INDENT;
        let gutter = 2. + Layout::MARKDOWN_CODE_PADDING;
        let (quote_x, text_x) = match ancestry {
            Ancestry::ListQuote => (indent, indent + gutter),
            Ancestry::NestedListQuote => (2. * indent, 2. * indent + gutter),
            Ancestry::QuoteList => (0., gutter),
            Ancestry::QuoteListQuote => (gutter + indent, 2. * gutter + indent),
        };
        let quote_x = origin + quote_x;
        let (item_text, item_x) = match ancestry {
            Ancestry::ListQuote => ("item", indent),
            Ancestry::NestedListQuote => ("nested-item", 2. * indent),
            Ancestry::QuoteList | Ancestry::QuoteListQuote => ("item", gutter + indent),
        };
        let item = text_bounds(painted, item_text);
        assert!(
            (f32::from(item.left()) - origin - item_x).abs() < 0.2,
            "{path:?} {ancestry:?}: list item column changed: {item:?}"
        );
        if matches!(ancestry, Ancestry::NestedListQuote) {
            let parent = text_bounds(painted, "parent-item");
            assert!((f32::from(parent.left()) - origin - indent).abs() < 0.2);
        }
        assert!(
            painted
                .rails
                .iter()
                .any(|rail| (rail.left - quote_x).abs() < 0.2),
            "{path:?} {ancestry:?}: expected rail x={quote_x}, actual {:?}",
            painted.rails
        );
        for bounds in [first, second] {
            assert!(
                (f32::from(bounds.left()) - origin - text_x).abs() < 0.2,
                "{path:?} {ancestry:?}: text inset changed: {bounds:?}"
            );
        }
        assert!(
            (f32::from(second.top() - first.bottom()) - Layout::MARKDOWN_BLOCK_GAP).abs() < 0.2,
            "{path:?} {ancestry:?}: quote paragraph gap changed"
        );
        covered(
            painted,
            quote_x,
            f32::from(first.top()),
            f32::from(second.bottom()),
        );
        let has_outer = matches!(ancestry, Ancestry::QuoteList | Ancestry::QuoteListQuote);
        let (top, bottom) = if has_outer {
            let outer_before = text_bounds(painted, "outer-before");
            let outer_after = text_bounds(painted, "outer-after");
            assert!((f32::from(outer_before.left()) - origin - gutter).abs() < 0.2);
            assert_eq!(outer_before.left(), outer_after.left());
            covered(
                painted,
                origin,
                f32::from(outer_before.top()),
                f32::from(outer_after.bottom()),
            );
            if matches!(ancestry, Ancestry::QuoteListQuote) {
                absent(
                    painted,
                    quote_x,
                    f32::from(outer_before.top()),
                    f32::from(first.top()) - 0.3,
                );
                absent(
                    painted,
                    quote_x,
                    f32::from(second.bottom()) + 0.3,
                    f32::from(outer_after.bottom()),
                );
            }
            (outer_before.top(), outer_after.bottom())
        } else {
            absent(
                painted,
                quote_x,
                f32::from(item.top()),
                f32::from(item.bottom()),
            );
            (first.top(), second.bottom())
        };
        for rail in &painted.rails {
            assert!((rail.right - rail.left - 2.).abs() < 0.2);
            assert!(
                (rail.left - quote_x).abs() < 0.2
                    || (has_outer && (rail.left - origin).abs() < 0.2),
                "{path:?} {ancestry:?}: unrelated rail {rail:?}"
            );
            assert!(rail.top >= f32::from(top) - 0.2);
            assert!(rail.bottom <= f32::from(bottom) + 0.2);
        }
        for x in [origin, quote_x] {
            absent(
                painted,
                x,
                f32::from(before.top()),
                f32::from(before.bottom()),
            );
            absent(
                painted,
                x,
                f32::from(after.top()),
                f32::from(after.bottom()),
            );
        }
        assert_eq!(after.left(), before.left());
        for run in &painted.text {
            let bounds = run.layout.bounds();
            assert!(bounds.left() >= before.left() - px(0.2));
            assert!(
                bounds.right() <= px(width + 0.2),
                "{path:?} {ancestry:?}: text exceeds width {width}: {bounds:?}"
            );
        }
        f32::from(first.size.height + second.size.height)
    }

    fn verify_ancestry(cx: &mut TestAppContext, ancestry: Ancestry) {
        let (source, expected) = fixture(ancestry);
        let mut stream = MarkdownStream::new();
        stream.append(&source);
        assert_eq!(parsed_nodes(&stream), expected, "pending {ancestry:?}");
        stream.finish();
        assert_eq!(parsed_nodes(&stream), expected, "finished {ancestry:?}");
        for path in PATHS {
            let handle = open(cx, path, &source, 240.);
            let mut narrow = None;
            let mut narrow_rails = None;
            for width in [240., 760., 240.] {
                cx.simulate_window_resize(handle.into(), size(px(width), px(12000.)));
                handle
                    .update(cx, |view, _, cx| {
                        view.width = width;
                        cx.notify();
                    })
                    .unwrap();
                let painted = frame(handle, cx);
                let height = verify_geometry(&painted, ancestry, width, path);
                if let Some(narrow_height) = narrow {
                    if width == 760. {
                        assert!(height < narrow_height, "{path:?} {ancestry:?}: no reflow");
                    } else {
                        assert!((height - narrow_height).abs() < 0.2);
                        assert_eq!(Some(&painted.rails), narrow_rails.as_ref());
                    }
                } else {
                    narrow = Some(height);
                    narrow_rails = Some(painted.rails.clone());
                }
            }
        }
    }

    #[gpui_kit::test]
    fn list_owned_quote_rails_start_at_the_list_content_column(cx: &mut TestAppContext) {
        verify_ancestry(cx, Ancestry::ListQuote);
    }

    #[gpui_kit::test]
    fn nested_list_owned_quote_rails_keep_both_list_insets(cx: &mut TestAppContext) {
        verify_ancestry(cx, Ancestry::NestedListQuote);
    }

    #[gpui_kit::test]
    fn outer_quote_rails_do_not_follow_descendant_list_insets(cx: &mut TestAppContext) {
        verify_ancestry(cx, Ancestry::QuoteList);
    }

    #[gpui_kit::test]
    fn inner_list_owned_quote_rails_preserve_the_outer_quote_origin(cx: &mut TestAppContext) {
        verify_ancestry(cx, Ancestry::QuoteListQuote);
    }
}

mod quote_first_item_tests {
    use super::*;

    const FIRST: &str =
        "first 中文 mixed words stay in the quote content column across narrow and wide layouts";
    const SECOND: &str =
        "second 🙂 continuation words use the same content column when the window changes width";

    fn paragraph(text: &str) -> RenderNode {
        RenderNode::Paragraph {
            spans: vec![Inline::Text(text.to_owned())],
        }
    }

    fn list(children: Vec<RenderNode>) -> RenderNode {
        RenderNode::List(ListBlock {
            ordered: false,
            start: 1,
            items: vec![vega_markdown::ListItem {
                checked: None,
                children,
            }],
        })
    }

    fn fixture(depth: usize) -> (String, Vec<RenderNode>, String) {
        let quote = RenderNode::BlockQuote {
            children: vec![paragraph(FIRST), paragraph(SECOND)],
        };
        let (body, node, copied) = match depth {
            0 => (
                format!("- > {FIRST}\n  >\n  > {SECOND}\n\n"),
                list(vec![quote]),
                format!("• {FIRST}\n\n{SECOND}"),
            ),
            1 => (
                format!("- parent\n  - > {FIRST}\n    >\n    > {SECOND}\n\n"),
                list(vec![paragraph("parent"), list(vec![quote])]),
                format!("• parent\n  • {FIRST}\n\n{SECOND}"),
            ),
            _ => panic!("unsupported fixture depth"),
        };
        (
            format!("outside-before\n\n{body}outside-after\n"),
            vec![
                paragraph("outside-before"),
                node,
                paragraph("outside-after"),
            ],
            format!("outside-before\n\n{copied}\n\noutside-after"),
        )
    }

    fn parsed_nodes(stream: &MarkdownStream) -> Vec<RenderNode> {
        let snapshot = stream.snapshot();
        snapshot
            .blocks
            .iter()
            .flat_map(|block| block.nodes.iter())
            .chain(
                snapshot
                    .pending
                    .into_iter()
                    .flat_map(|block| block.nodes.iter()),
            )
            .cloned()
            .collect()
    }

    fn paragraph_bounds(painted: &Frame, expected: &str) -> Bounds<Pixels> {
        let start = painted
            .text
            .iter()
            .position(|run| !run.text.is_empty() && expected.starts_with(run.text.as_str()))
            .expect("painted paragraph start");
        let mut content = String::new();
        let mut bounds: Option<Bounds<Pixels>> = None;
        for run in &painted.text[start..] {
            content.push_str(&run.text);
            assert!(
                expected.starts_with(&content),
                "changed paragraph: {content:?}"
            );
            let next = run.layout.bounds();
            bounds = Some(bounds.map_or(next, |bounds| bounds.union(&next)));
            if content == expected {
                return bounds.expect("painted paragraph bounds");
            }
        }
        panic!("incomplete painted paragraph: {content:?}");
    }

    fn verify_geometry(painted: &Frame, depth: usize, width: f32, path: Path) -> f32 {
        let before = text_bounds(painted, "outside-before");
        let after = text_bounds(painted, "outside-after");
        let first = paragraph_bounds(painted, FIRST);
        let second = paragraph_bounds(painted, SECOND);
        let origin = f32::from(before.left());
        let marker_x = origin + depth as f32 * Layout::MARKDOWN_LIST_INDENT;
        let rail_x = marker_x + Layout::MARKDOWN_LIST_INDENT;
        let text_x = rail_x + 2. + Layout::MARKDOWN_CODE_PADDING;
        let markers: Vec<_> = painted
            .text
            .iter()
            .filter(|run| {
                run.text == "• " && (f32::from(run.layout.bounds().top() - first.top())).abs() < 0.2
            })
            .collect();
        assert_eq!(
            markers.len(),
            1,
            "{path:?} depth={depth}: missing first-item marker"
        );
        let marker = markers[0].layout.bounds();
        assert!(
            (f32::from(marker.left()) - marker_x).abs() < 0.2,
            "{path:?} depth={depth}: marker must start at {marker_x}, actual {marker:?}"
        );
        assert!(f32::from(marker.right()) <= rail_x + 0.2);
        for bounds in [first, second] {
            assert!(
                (f32::from(bounds.left()) - text_x).abs() < 0.2,
                "{path:?} depth={depth}: paragraph must start at {text_x}, actual {bounds:?}"
            );
        }
        assert!(
            (f32::from(second.top() - first.bottom()) - Layout::MARKDOWN_BLOCK_GAP).abs() < 0.2,
            "{path:?} depth={depth}: quote paragraph gap changed"
        );
        covered(
            painted,
            rail_x,
            f32::from(first.top()),
            f32::from(second.bottom()),
        );
        for rail in &painted.rails {
            assert!(
                (rail.left - rail_x).abs() < 0.2,
                "{path:?}: unrelated rail {rail:?}"
            );
            assert!((rail.right - rail.left - 2.).abs() < 0.2);
            assert!(rail.top >= f32::from(first.top()) - 0.2);
            assert!(rail.bottom <= f32::from(second.bottom()) + 0.2);
        }
        assert_eq!(before.left(), after.left());
        for run in &painted.text {
            let bounds = run.layout.bounds();
            assert!(bounds.left() >= before.left() - px(0.2));
            assert!(
                bounds.right() <= px(width + 0.2),
                "{path:?}: text overflows {bounds:?}"
            );
        }
        f32::from(first.size.height + second.size.height)
    }

    fn verify_path(cx: &mut TestAppContext, path: Path) {
        for depth in [0, 1] {
            let (source, expected, copied) = fixture(depth);
            let mut stream = MarkdownStream::new();
            stream.append(&source);
            assert_eq!(parsed_nodes(&stream), expected, "pending depth={depth}");
            stream.finish();
            assert_eq!(parsed_nodes(&stream), expected, "finished depth={depth}");
            let handle = open(cx, path, &source, 240.);
            let mut narrow = None;
            let mut narrow_rails = None;
            for width in [240., 760., 240.] {
                cx.simulate_window_resize(handle.into(), size(px(width), px(12000.)));
                handle
                    .update(cx, |view, _, cx| {
                        view.width = width;
                        assert_eq!(view.source, source);
                        assert!(view.nodes.is_none());
                        cx.notify();
                    })
                    .unwrap();
                let painted = frame(handle, cx);
                if matches!(path, Path::Selectable | Path::Thinking) {
                    copy_between(handle, cx, &painted, "outside-before", "outside-after");
                    assert_eq!(
                        cx.read_from_clipboard()
                            .and_then(|item| item.text())
                            .as_deref(),
                        Some(copied.as_str()),
                        "{path:?} depth={depth} width={width}: visible copy order changed"
                    );
                }
                let height = verify_geometry(&painted, depth, width, path);
                if let Some(narrow_height) = narrow {
                    if width == 760. {
                        assert!(height < narrow_height, "{path:?} depth={depth}: no reflow");
                    } else {
                        assert!((height - narrow_height).abs() < 0.2);
                        assert_eq!(Some(&painted.rails), narrow_rails.as_ref());
                    }
                } else {
                    narrow = Some(height);
                    narrow_rails = Some(painted.rails.clone());
                }
            }
        }
    }

    #[gpui_kit::test]
    fn ordinary_quote_first_items_keep_markers_outside_the_quote(cx: &mut TestAppContext) {
        verify_path(cx, Path::Ordinary);
    }

    #[gpui_kit::test]
    fn selectable_quote_first_items_keep_marker_order_and_content_columns(cx: &mut TestAppContext) {
        verify_path(cx, Path::Selectable);
    }

    #[gpui_kit::test]
    fn thinking_quote_first_items_keep_marker_order_and_content_columns(cx: &mut TestAppContext) {
        verify_path(cx, Path::Thinking);
    }
}
