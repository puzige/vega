use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyElement, App, Context, ElementId, FocusHandle, Global, InputEvent, IntoElement,
    ListAlignment, ListOffset, ListState, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, ScrollDelta, ScrollHandle, ScrollWheelEvent,
    TestAppContext, TouchPhase, VisualTestContext, Window, WindowId, div, list, point, prelude::*,
    px,
};

use super::super::{MessageCopy, StreamCounters, StreamModel, render_rows};
use vega_markdown::MarkdownStream;
use vega_theme::Typography;

#[derive(Default)]
struct Observations(HashMap<WindowId, HashMap<ElementId, ScrollHandle>>);

impl Global for Observations {}

pub(super) fn observe(id: ElementId, scroll: ScrollHandle, window: &Window, cx: &mut App) {
    if cx.try_global::<Observations>().is_some() {
        cx.update_global::<Observations, _>(|observations, _| {
            observations
                .0
                .entry(window.window_handle().window_id())
                .or_default()
                .insert(id, scroll);
        });
    }
}

pub(in crate::conversation_stream) type BodyRenderer =
    fn(&str, &MessageCopy, &FocusHandle, &mut Window, &mut App) -> AnyElement;

struct ScrollScene {
    source: Rc<RefCell<String>>,
    copy: MessageCopy,
    focus: FocusHandle,
    list: ListState,
    renderer: BodyRenderer,
}

impl Render for ScrollScene {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let source = self.source.clone();
        let copy = self.copy.clone();
        let focus = self.focus.clone();
        let renderer = self.renderer;
        div()
            .w(px(320.))
            .h(px(400.))
            .child(gpui_kit::base::TextSelectionLayer)
            .child(
                list(self.list.clone(), move |index, window, cx| {
                    if index == 0 {
                        renderer(&source.borrow(), &copy, &focus, window, cx)
                    } else {
                        div().w_full().h(px(80.)).into_any_element()
                    }
                })
                .w_full()
                .h_full(),
            )
    }
}

pub(in crate::conversation_stream) struct Fixture {
    pub cx: VisualTestContext,
    pub copy: MessageCopy,
    source: Rc<RefCell<String>>,
    list: ListState,
}

impl Fixture {
    pub fn new(cx: &mut TestAppContext, renderer: BodyRenderer, source: String) -> Self {
        let copy = MessageCopy::new(&source);
        Self::with_copy(cx, renderer, source, copy)
    }

    fn with_copy(
        cx: &mut TestAppContext,
        renderer: BodyRenderer,
        source: String,
        copy: MessageCopy,
    ) -> Self {
        cx.update(|cx| {
            if cx.try_global::<Observations>().is_none() {
                gpui_kit::init(cx);
                cx.set_global(vega_theme::Theme::light());
                cx.set_global(Observations::default());
            }
        });
        let source = Rc::new(RefCell::new(source));
        let list = ListState::new(20, ListAlignment::Top, px(1000.));
        let (_, visual) = cx.add_window_view({
            let source = source.clone();
            let copy = copy.clone();
            let list = list.clone();
            move |_, cx| ScrollScene {
                source,
                copy,
                focus: cx.focus_handle(),
                list,
                renderer,
            }
        });
        let mut fixture = Self {
            cx: visual.clone(),
            copy,
            source,
            list,
        };
        fixture.draw();
        fixture
    }

    fn draw(&mut self) {
        self.cx.update(|window, cx| window.render_frame(cx));
    }

    pub fn handles(&mut self) -> Vec<ScrollHandle> {
        self.cx.update(|window, cx| {
            let mut handles: Vec<_> = cx.global::<Observations>().0
                [&window.window_handle().window_id()]
                .values()
                .cloned()
                .collect();
            handles.sort_by(|left, right| {
                f32::from(left.bounds().top()).total_cmp(&f32::from(right.bounds().top()))
            });
            handles
        })
    }

    fn outer_y(&self) -> Pixels {
        self.list.scroll_px_offset_for_scrollbar().y
    }

    fn reset_outer(&mut self) {
        self.list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        self.draw();
    }

    fn wheel(
        &mut self,
        position: Point<Pixels>,
        delta: ScrollDelta,
        phase: TouchPhase,
        shift: bool,
    ) {
        self.cx.update(|window, cx| {
            window.dispatch_event(
                ScrollWheelEvent {
                    position,
                    delta,
                    touch_phase: phase,
                    modifiers: Modifiers {
                        shift,
                        ..Default::default()
                    },
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        });
    }

    fn pixels(&mut self, position: Point<Pixels>, x: f32, y: f32) {
        self.wheel(
            position,
            ScrollDelta::Pixels(point(px(x), px(y))),
            TouchPhase::Started,
            false,
        );
    }

    fn verify_contained_pixels(
        &mut self,
        scroll: &ScrollHandle,
        events: &[(f32, f32, TouchPhase, Pixels)],
    ) {
        let list = self.list.clone();
        let position = scroll.bounds().center();
        self.cx.update(|window, cx| {
            for &(x, y, phase, expected_x) in events {
                window.dispatch_event(
                    ScrollWheelEvent {
                        position,
                        delta: ScrollDelta::Pixels(point(px(x), px(y))),
                        touch_phase: phase,
                        modifiers: Default::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
                assert_eq!(scroll.offset().x, expected_x);
                assert_eq!(list.scroll_px_offset_for_scrollbar().y, px(0.));
            }
            window.render_frame(cx);
        });
    }

    fn drag(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        self.cx.update(|window, cx| {
            window.dispatch_event(
                MouseMoveEvent {
                    position: from,
                    pressed_button: None,
                    modifiers: Default::default(),
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
            window.dispatch_event(
                MouseDownEvent {
                    button: MouseButton::Left,
                    position: from,
                    click_count: 1,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
            std::thread::sleep(std::time::Duration::from_millis(12));
            window.dispatch_event(
                MouseMoveEvent {
                    position: to,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Default::default(),
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseUpEvent {
                    button: MouseButton::Left,
                    position: to,
                    click_count: 1,
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        });
    }

    pub fn verify_wheel_routing(&mut self) {
        let scroll = self.handles().remove(0);
        assert!(scroll.max_offset().x > px(200.));
        for delta in [
            ScrollDelta::Pixels(point(px(0.), px(-30.))),
            ScrollDelta::Lines(point(0., -2.)),
        ] {
            self.wheel(scroll.bounds().center(), delta, TouchPhase::Started, false);
            assert_eq!(scroll.offset().x, px(0.));
            assert!(self.outer_y() < px(0.));
            self.reset_outer();
        }
        self.verify_contained_pixels(
            &scroll,
            &[
                (-40., -10., TouchPhase::Started, px(-40.)),
                (0., -1., TouchPhase::Moved, px(-40.)),
                (-10., -15., TouchPhase::Moved, px(-50.)),
            ],
        );
        assert_eq!(scroll.offset().x, px(-50.));
        assert_eq!(self.outer_y(), px(0.));
        self.wheel(
            scroll.bounds().center(),
            ScrollDelta::Pixels(point(px(-40.), px(0.))),
            TouchPhase::Started,
            true,
        );
        assert_eq!(scroll.offset().x, px(-90.));
        assert_eq!(self.outer_y(), px(0.));
        self.wheel(
            scroll.bounds().center(),
            ScrollDelta::Lines(point(-2., 0.)),
            TouchPhase::Started,
            true,
        );
        assert!(scroll.offset().x < px(-90.));
        assert_eq!(self.outer_y(), px(0.));
        scroll.set_offset(point(px(-150.), px(0.)));
        self.draw();
        let edge = point(scroll.bounds().right() - px(2.), scroll.bounds().center().y);
        self.pixels(edge, -40., -10.);
        assert_eq!(scroll.offset().x, px(-190.));
        assert_eq!(self.outer_y(), px(0.));
        for (offset, delta) in [(px(0.), 40.), (-scroll.max_offset().x, -40.)] {
            scroll.set_offset(point(offset, px(0.)));
            self.draw();
            self.pixels(edge, delta, -10.);
            assert_eq!(scroll.offset().x, offset);
            assert_eq!(self.outer_y(), px(0.));
            self.verify_contained_pixels(
                &scroll,
                &[
                    (delta, -10., TouchPhase::Started, offset),
                    (0., -1., TouchPhase::Moved, offset),
                ],
            );
        }
    }

    pub fn verify_gesture_release_and_timeout(&mut self) {
        let scroll = self.handles().remove(0);
        for phase in [TouchPhase::Ended, TouchPhase::Cancelled] {
            scroll.set_offset(point(px(0.), px(0.)));
            self.draw();
            self.verify_contained_pixels(
                &scroll,
                &[
                    (-40., -10., TouchPhase::Started, px(-40.)),
                    (0., -1., TouchPhase::Moved, px(-40.)),
                    (0., -1., phase, px(-40.)),
                ],
            );
            self.wheel(
                scroll.bounds().center(),
                ScrollDelta::Pixels(point(px(0.), px(-1.))),
                TouchPhase::Moved,
                false,
            );
            assert_eq!(scroll.offset().x, px(-40.));
            assert!(self.outer_y() < px(0.));
            self.reset_outer();
            self.pixels(scroll.bounds().center(), 0., -30.);
            assert_eq!(scroll.offset().x, px(-40.));
            assert!(self.outer_y() < px(0.));
            self.reset_outer();
        }
        let expected_x = scroll.offset().x - px(40.);
        let list = self.list.clone();
        let position = scroll.bounds().center();
        self.cx.update(|window, cx| {
            let mut previous_y = px(0.);
            for (delta, phase, changes_y) in [
                (
                    ScrollDelta::Pixels(point(px(-40.), px(-10.))),
                    TouchPhase::Started,
                    false,
                ),
                (ScrollDelta::Lines(point(0., -2.)), TouchPhase::Moved, true),
                (
                    ScrollDelta::Pixels(point(px(0.), px(-1.))),
                    TouchPhase::Moved,
                    true,
                ),
            ] {
                window.dispatch_event(
                    ScrollWheelEvent {
                        position,
                        delta,
                        touch_phase: phase,
                        modifiers: Default::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
                assert_eq!(scroll.offset().x, expected_x);
                let y = list.scroll_px_offset_for_scrollbar().y;
                if changes_y {
                    assert!(y < px(0.));
                    assert_ne!(y, previous_y);
                } else {
                    assert_eq!(y, px(0.));
                }
                previous_y = y;
            }
            window.render_frame(cx);
        });
        self.reset_outer();
        self.pixels(scroll.bounds().center(), -40., -10.);
        let before = scroll.offset().x;
        std::thread::sleep(std::time::Duration::from_millis(40));
        self.wheel(
            scroll.bounds().center(),
            ScrollDelta::Pixels(point(px(0.), px(-1.))),
            TouchPhase::Moved,
            false,
        );
        assert_eq!(scroll.offset().x, before);
        assert!(self.outer_y() < px(0.));
    }

    pub fn verify_scrollbar_and_selection(&mut self, text_y_inset: Pixels) {
        let scroll = self.handles().remove(0);
        let bounds = scroll.bounds();
        let from = point(bounds.left() + px(8.), bounds.bottom() - px(8.));
        let to = point(bounds.center().x, from.y);
        self.drag(from, to);
        assert!(scroll.offset().x < px(-100.));
        assert_eq!(self.outer_y(), px(0.));
        assert!(!self.copy.has_selected_text());
        let before = scroll.offset().x;
        let track = point(bounds.right() - px(8.), from.y);
        self.drag(track, track);
        assert!(scroll.offset().x < before);
        assert_eq!(self.outer_y(), px(0.));
        assert!(!self.copy.has_selected_text());
        scroll.set_offset(point(px(0.), px(0.)));
        self.draw();
        let start = point(bounds.left() + px(18.), bounds.top() + text_y_inset);
        assert!(bounds.contains(&start));
        assert!(start.y < bounds.bottom() - px(16.));
        self.drag(start, start + point(px(90.), px(0.)));
        assert!(self.copy.has_selected_text());
        self.cx.update(|_, cx| {
            self.copy.copy_selected_text(cx);
            let clipboard = cx.read_from_clipboard().and_then(|item| item.text());
            assert!(clipboard.is_some_and(|text| !text.is_empty() && text.contains('x')));
        });
    }

    pub fn verify_independent_state_and_append(&mut self) {
        let handles = self.handles();
        assert_eq!(handles.len(), 2);
        let initial_bounds = handles[0].bounds();
        self.pixels(initial_bounds.center(), -60., 0.);
        assert_eq!(handles[0].offset().x, px(-60.));
        assert_eq!(handles[1].offset().x, px(0.));
        self.pixels(handles[1].bounds().center(), -100., 0.);
        assert_eq!(handles[0].offset().x, px(-60.));
        assert_eq!(handles[1].offset().x, px(-100.));
        self.source.borrow_mut().push_str("\nappended paragraph\n");
        self.copy.append("\nappended paragraph\n");
        self.list.splice(0..1, 1);
        self.draw();
        let after = self.handles();
        assert_eq!(after[0].offset().x, px(-60.));
        assert_eq!(after[1].offset().x, px(-100.));
        assert_eq!(after[0].bounds(), initial_bounds);
        assert_eq!(self.outer_y(), px(0.));
    }

    pub fn verify_fitting_content(&mut self) {
        let scroll = self.handles().remove(0);
        assert_eq!(scroll.max_offset().x, px(0.));
        self.pixels(scroll.bounds().center(), -40., -10.);
        assert_eq!(scroll.offset().x, px(0.));
        assert_eq!(self.outer_y(), px(0.));
        self.verify_contained_pixels(
            &scroll,
            &[
                (-40., -10., TouchPhase::Started, px(0.)),
                (0., -1., TouchPhase::Moved, px(0.)),
            ],
        );
        let bounds = scroll.bounds();
        self.drag(
            point(bounds.left() + px(8.), bounds.bottom() - px(4.)),
            point(bounds.right() - px(8.), bounds.bottom() - px(4.)),
        );
        assert_eq!(scroll.offset().x, px(0.));
        self.pixels(bounds.center(), 0., -30.);
        assert!(self.outer_y() < px(0.));
    }
}

pub(in crate::conversation_stream) fn long_code() -> String {
    format!("```text\n{}\n```\n", "x".repeat(180))
}

fn markdown(
    source: &str,
    copy: &MessageCopy,
    focus: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let mut stream = MarkdownStream::new();
    stream.append(source);
    let mut model = StreamModel::default();
    model.sync(&stream.snapshot(), &StreamCounters::default());
    let colors = vega_theme::theme(cx).colors;
    render_rows::selectable_markdown_content(&model, copy, focus, &colors, false, window, cx)
}

#[gpui_kit::test]
fn markdown_code_routes_wheel_and_keeps_viewport_capture(cx: &mut TestAppContext) {
    Fixture::new(cx, markdown, long_code()).verify_wheel_routing();
}

#[gpui_kit::test]
fn markdown_horizontal_gesture_releases_for_vertical_input(cx: &mut TestAppContext) {
    Fixture::new(cx, markdown, long_code()).verify_gesture_release_and_timeout();
}

#[gpui_kit::test]
fn markdown_scrollbar_drags_without_selecting_and_text_still_copies(cx: &mut TestAppContext) {
    Fixture::new(cx, markdown, long_code())
        .verify_scrollbar_and_selection(px(Typography::CODE * Typography::CODE_LINE_HEIGHT / 2.));
}

#[gpui_kit::test]
fn markdown_code_offsets_remain_independent_after_append(cx: &mut TestAppContext) {
    Fixture::new(cx, markdown, format!("{}\n{}", long_code(), long_code()))
        .verify_independent_state_and_append();
}

#[gpui_kit::test]
fn fitting_markdown_code_contains_horizontal_input_and_passes_vertical(cx: &mut TestAppContext) {
    Fixture::new(cx, markdown, "```text\nx\n```\n".into()).verify_fitting_content();
}

#[gpui_kit::test]
fn same_markdown_identity_in_separate_windows_has_independent_offsets(cx: &mut TestAppContext) {
    let source = long_code();
    let copy = MessageCopy::new(&source);
    let mut first = Fixture::with_copy(cx, markdown, source.clone(), copy.clone());
    let first_scroll = first.handles().remove(0);
    first.pixels(first_scroll.bounds().center(), -80., 0.);
    let mut second = Fixture::with_copy(cx, markdown, source, copy);
    let second_scroll = second.handles().remove(0);
    assert_eq!(second_scroll.offset().x, px(0.));
    second.pixels(second_scroll.bounds().center(), -30., 0.);
    assert_eq!(first_scroll.offset().x, px(-80.));
    assert_eq!(second_scroll.offset().x, px(-30.));
}
