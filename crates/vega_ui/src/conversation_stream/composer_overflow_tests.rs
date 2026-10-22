use std::cell::RefCell;
use std::io::Cursor;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    Action, App, AppContext, Bounds, Context, Entity, EntityInputHandler, Focusable,
    HeadlessAppContext, InputEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, TouchPhase,
    Window, WindowHandle, div, point, px, size,
};
use vega_conversation::types::{
    ImageAttachment, PermissionMode, TaskBackend, Thread, ThreadMode, ThreadStatus,
};
use vega_theme::{Layout, Typography};

use super::attachments::ImagePreview;
use super::{ComposerSubmitted, ConversationStream, ImagePreviewRequested};
use crate::image_preview::ImagePreviewAction;
use crate::overflow_test_support::{
    PaintedFrame, PaintedGeometry, SceneRecorder, contains, context, logical, visible,
};
use crate::text_input::{self, TextInput};

struct ComposerHost {
    stream: Entity<ConversationStream>,
    escaped_wheels: Rc<RefCell<usize>>,
}

impl Render for ComposerHost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let escaped = self.escaped_wheels.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .on_scroll_wheel(move |_, _, _| *escaped.borrow_mut() += 1)
            .child(self.stream.clone())
    }
}

struct ComposerHarness {
    window: WindowHandle<ComposerHost>,
    stream: Entity<ConversationStream>,
    input: Entity<TextInput>,
    recorder: SceneRecorder,
    escaped: Rc<RefCell<usize>>,
    submissions: Rc<RefCell<Vec<ComposerSubmitted>>>,
    previews: Rc<RefCell<Vec<Entity<crate::image_preview::ImagePreview>>>>,
    cx: HeadlessAppContext,
}

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: "composer-fixture".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Draft".into(),
        mode: ThreadMode::Ask,
        permission_mode: PermissionMode::Confirm,
        model: "fixture-model".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    }
}

impl ComposerHarness {
    fn new(width: f32, dark: bool, draft_route: bool) -> Self {
        let (mut cx, recorder) = context(dark);
        let escaped = Rc::new(RefCell::new(0));
        let submissions = Rc::new(RefCell::new(Vec::new()));
        let previews = Rc::new(RefCell::new(Vec::new()));
        let window = cx
            .open_window(size(px(width), px(900.)), |_, cx| {
                let stream = cx.new(|cx| ConversationStream::new(thread(), cx));
                stream.update(cx, |stream, cx| {
                    stream.set_draft_route(draft_route, cx);
                    if !draft_route {
                        stream.apply_history_page(
                            vega_conversation::history::HistoryPage {
                                subagents: Vec::new(),
                                entries: vec![vega_conversation::history::HistoryEntry::UserText {
                                    seq: 1,
                                    message_id: "previous-message".into(),
                                    content: "previous exchange".into(),
                                }],
                                older_cursor: None,
                                newer_cursor: None,
                                newest_seq: Some(1),
                            },
                            cx,
                        );
                    }
                });
                cx.new(|cx: &mut Context<ComposerHost>| {
                    let submitted = submissions.clone();
                    cx.subscribe(&stream, move |_, _, event: &ComposerSubmitted, _| {
                        submitted.borrow_mut().push(event.clone());
                    })
                    .detach();
                    let opened = previews.clone();
                    cx.subscribe(&stream, move |_, _, event: &ImagePreviewRequested, _| {
                        if matches!(event.action, ImagePreviewAction::Open) {
                            opened.borrow_mut().push(event.image.clone());
                        }
                    })
                    .detach();
                    ComposerHost {
                        stream,
                        escaped_wheels: escaped.clone(),
                    }
                })
            })
            .unwrap();
        let host = window.root(&mut cx).unwrap();
        let stream = cx.update(|cx| host.read(cx).stream.clone());
        let input = cx.update(|cx| stream.read(cx).input.clone());
        let mut harness = Self {
            window,
            stream,
            input,
            recorder,
            escaped,
            submissions,
            previews,
            cx,
        };
        harness.focus_input();
        harness.draw();
        harness
    }

    fn update<R>(&mut self, update: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        self.cx
            .update_window(self.window.into(), |_, window, cx| update(window, cx))
            .unwrap()
    }

    fn draw(&mut self) -> PaintedFrame {
        for _ in 0..3 {
            self.cx.run_until_parked();
            self.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                drop(window.render_to_image().unwrap());
            });
        }
        self.recorder.frame()
    }

    fn scale(&mut self) -> f32 {
        self.update(|window, _| window.scale_factor())
    }

    fn focus_input(&mut self) {
        let input = self.input.clone();
        self.update(|window, cx| input.read(cx).focus_handle(cx).focus(window, cx));
    }

    fn set_text(&mut self, text: &str) {
        let input = self.input.clone();
        self.update(|_, cx| input.update(cx, |input, cx| input.set_text(text, cx)));
        self.draw();
    }

    fn viewport(&mut self) -> Bounds<Pixels> {
        let id = self.input.entity_id();
        self.update(|_, cx| cx.global::<PaintedGeometry>().inputs[&id])
    }

    fn region(&mut self, name: &'static str) -> Bounds<Pixels> {
        self.update(|_, cx| cx.global::<PaintedGeometry>().regions[name])
    }

    fn rows(&mut self) -> Vec<String> {
        let frame = self.draw();
        let viewport = self.viewport();
        let scale = self.scale();
        frame.text_rows(viewport, scale)
    }

    fn dispatch(&mut self, event: impl InputEvent) {
        self.update(|window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
        self.draw();
    }

    fn wheel(&mut self, delta: ScrollDelta, phase: TouchPhase) {
        let position = self.viewport().center();
        self.dispatch(ScrollWheelEvent {
            position,
            delta,
            touch_phase: phase,
            ..Default::default()
        });
    }

    fn key(&mut self, key: &str) {
        self.update(|window, cx| {
            window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
        });
        self.draw();
    }

    fn action(&mut self, action: impl gpui_kit::Action) {
        self.update(|window, cx| window.dispatch_action(action.boxed_clone(), cx));
        self.draw();
    }

    fn selection(&mut self) -> Range<usize> {
        let input = self.input.clone();
        self.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.selected_text_range(false, window, cx).unwrap().range
            })
        })
    }

    fn text(&mut self) -> String {
        let input = self.input.clone();
        self.update(|_, cx| input.read(cx).text().to_string())
    }

    fn assert_focused(&mut self) {
        let input = self.input.clone();
        assert!(self.update(|window, cx| input.read(cx).focus_handle(cx).is_focused(window)));
    }

    fn click(&mut self, position: Point<Pixels>) {
        self.dispatch(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        });
        self.dispatch(MouseDownEvent {
            position,
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
            modifiers: Modifiers::default(),
        });
        self.dispatch(MouseUpEvent {
            position,
            button: MouseButton::Left,
            click_count: 1,
            modifiers: Modifiers::default(),
        });
    }

    fn drag(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        self.dispatch(MouseMoveEvent {
            position: from,
            pressed_button: None,
            modifiers: Modifiers::default(),
        });
        self.dispatch(MouseDownEvent {
            position: from,
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
            modifiers: Modifiers::default(),
        });
        self.dispatch(MouseMoveEvent {
            position: to,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        });
        self.dispatch(MouseUpEvent {
            position: to,
            button: MouseButton::Left,
            click_count: 1,
            modifiers: Modifiers::default(),
        });
    }

    fn resize(&mut self, width: f32) {
        self.update(|window, cx| {
            window.resize(size(px(width), px(900.)));
            window.bounds_changed(cx);
        });
        self.draw();
    }

    fn scrollbar(&mut self) -> Option<Bounds<Pixels>> {
        let viewport = self.viewport();
        self.dispatch(MouseMoveEvent {
            position: point(viewport.right() - px(1.), viewport.center().y),
            pressed_button: None,
            modifiers: Modifiers::default(),
        });
        let frame = self.draw();
        let scale = self.scale();
        frame
            .quads
            .iter()
            .filter(|quad| quad.background.as_solid().is_some_and(|color| color.a > 0.))
            .map(|quad| {
                logical(quad.bounds, scale).intersect(&logical(quad.content_mask.bounds, scale))
            })
            .filter(|bounds| {
                visible(*bounds)
                    && bounds.size.width >= px(2.)
                    && bounds.size.width <= px(16.)
                    && bounds.size.height > bounds.size.width
                    && bounds.size.height < viewport.size.height - px(1.)
                    && bounds.right() >= viewport.right() - px(20.)
                    && bounds.right() <= viewport.right() + px(20.)
                    && bounds.top() >= viewport.top() - px(1.)
                    && bounds.bottom() <= viewport.bottom() + px(1.)
            })
            .max_by(|a, b| f32::from(a.size.height).total_cmp(&f32::from(b.size.height)))
    }

    fn assert_caret_painted(&mut self) {
        let viewport = self.viewport();
        let scale = self.scale();
        let frame = self.draw();
        let accent = self.update(|_, cx| gpui_kit::Hsla::from(vega_theme::theme(cx).colors.accent));
        assert!(
            frame.quads.iter().any(|quad| {
                let bounds = logical(quad.bounds, scale);
                quad.background.as_solid() == Some(accent)
                    && bounds.size.width <= px(3.)
                    && bounds.size.height > px(Typography::BODY)
                    && contains(viewport, bounds)
            }),
            "focused caret must be visibly painted inside the input"
        );
    }

    fn attach(&mut self, dimensions: &[(u32, u32)]) -> Vec<ImageAttachment> {
        let images: Vec<_> = dimensions
            .iter()
            .map(|&(w, h)| image_fixture(w, h))
            .collect();
        let stream = self.stream.clone();
        self.update(|_, cx| {
            stream.update(cx, |stream, cx| {
                for (index, image) in images.iter().enumerate() {
                    let preview =
                        ImagePreview::new(image.clone(), format!("fixture-{index}.png"), cx);
                    stream.observe_image(&preview.view, cx);
                    stream.attachments.push((index as u64 + 1, preview));
                }
                cx.notify();
            });
        });
        self.draw();
        images
    }

    fn thumbnails(&mut self) -> Vec<Bounds<Pixels>> {
        let stream = self.stream.clone();
        self.update(|_, cx| {
            stream
                .read(cx)
                .attachments
                .iter()
                .map(|(_, image)| {
                    cx.global::<PaintedGeometry>().thumbnails[&image.view.entity_id()]
                })
                .collect()
        })
    }
}

fn image_fixture(width: u32, height: u32) -> ImageAttachment {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([32, 144, 208, 255]));
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    ImageAttachment::from_bytes(encoded.into_inner()).unwrap()
}

fn numbered_draft(rows: usize) -> String {
    (1..=rows)
        .map(|row| format!("draft {row:02}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_rows(harness: &mut ComposerHarness, first: usize, last: usize) {
    assert_eq!(
        harness.rows(),
        (first..=last)
            .map(|row| format!("draft {row:02}"))
            .collect::<Vec<_>>()
    );
}

fn scroll_top(harness: &mut ComposerHarness) {
    harness.wheel(ScrollDelta::Lines(point(0., 100.)), TouchPhase::Moved);
}

fn scroll_bottom(harness: &mut ComposerHarness) {
    harness.wheel(ScrollDelta::Lines(point(0., -100.)), TouchPhase::Moved);
}

#[test]
fn decoded_large_image_thumbnails_stay_above_input_and_footer() {
    for dark in [false, true] {
        for dimensions in [(2400, 1200), (1200, 2400), (1800, 1800)] {
            let mut harness = ComposerHarness::new(720., dark, true);
            harness.set_text("unobscured draft");
            harness.attach(&[dimensions]);
            let frame = harness.draw();
            assert_eq!(
                frame.images.len(),
                1,
                "decoded image must paint, not just a placeholder"
            );
            let thumbnail = harness.thumbnails()[0];
            assert_eq!(
                thumbnail.size,
                size(
                    px(Layout::ATTACHMENT_THUMBNAIL),
                    px(Layout::ATTACHMENT_THUMBNAIL)
                )
            );
            let image = &frame.images[0];
            assert_eq!(
                (image.decoded_size.width.0, image.decoded_size.height.0),
                (dimensions.0 as i32, dimensions.1 as i32)
            );
            let scale = harness.scale();
            let painted = logical(image.bounds, scale).intersect(&logical(image.mask, scale));
            assert!(visible(painted));
            assert!(
                contains(thumbnail, painted),
                "decoded image paint escaped thumbnail: {painted:?} vs {thumbnail:?}"
            );
            let input = harness.viewport();
            let footer = harness.region("composer-footer");
            assert!(thumbnail.bottom() <= input.top());
            assert!(painted.bottom() <= input.top());
            assert!(input.bottom() <= footer.top());
            assert_eq!(harness.rows(), ["unobscured draft"]);
        }
    }
}

#[test]
fn attachment_wrap_preserves_actual_composer_height_preview_remove_and_send() {
    for draft_route in [false, true] {
        let mut harness = ComposerHarness::new(720., false, draft_route);
        harness.set_text("complete attachment draft");
        let originals = harness.attach(&[(2400, 1200), (1200, 2400), (1800, 1800), (1600, 900)]);
        let wide = harness.thumbnails();
        assert!(
            wide.iter().all(|bounds| bounds.top() == wide[0].top()),
            "wide thumbnails must share a row: {wide:?}"
        );
        harness.resize(330.);
        let narrow = harness.thumbnails();
        assert!(
            narrow.iter().any(|bounds| bounds.top() > narrow[0].top()),
            "actual attachment row must wrap"
        );
        let input = harness.viewport();
        let footer = harness.region("composer-footer");
        let send = harness.region("composer-send");
        assert!(narrow.iter().all(|bounds| bounds.bottom() <= input.top()));
        assert!(input.bottom() <= footer.top());
        assert!(contains(footer, send));
        let model = harness.region("composer-model");
        assert!(contains(footer, model));
        assert!(model.top() >= input.bottom());
        let scale = harness.scale();
        assert!(!harness.draw().text_rows(model, scale).is_empty());
        let frame = harness.draw();
        assert_eq!(frame.images.len(), 4);
        for image in &frame.images {
            let painted = logical(image.bounds, scale).intersect(&logical(image.mask, scale));
            assert!(narrow.iter().any(|thumbnail| contains(*thumbnail, painted)));
            assert!(painted.bottom() <= input.top());
        }
        harness.click(point(narrow[0].left() + px(8.), narrow[0].top() + px(36.)));
        assert_eq!(harness.previews.borrow().len(), 1);
        let view = harness.previews.borrow()[0].clone();
        harness.update(|window, cx| view.read(cx).thumbnail_focus().focus(window, cx));
        harness.draw();
        harness.key("space");
        assert_eq!(harness.previews.borrow().len(), 2);
        harness.key("tab");
        harness.key("enter");
        assert_eq!(harness.thumbnails().len(), 3);
        assert_eq!(harness.draw().images.len(), 3);
        assert!(harness.text().contains("complete attachment draft"));
        let last = harness.thumbnails()[2];
        harness.click(point(last.right() - px(12.), last.top() + px(12.)));
        assert_eq!(harness.thumbnails().len(), 2);
        assert_eq!(harness.draw().images.len(), 2);
        let send = harness.region("composer-send");
        harness.click(send.center());
        let submitted = harness.submissions.borrow();
        assert_eq!(submitted.len(), 1);
        assert_eq!(submitted[0].content, "complete attachment draft");
        assert_eq!(submitted[0].images, originals[1..3]);
    }
}

#[test]
fn composer_wheel_and_trackpad_reach_both_ends_without_editing_or_snapback() {
    for draft_route in [false, true] {
        let mut harness = ComposerHarness::new(720., false, draft_route);
        let content = numbered_draft(20);
        harness.set_text(&content);
        assert_rows(&mut harness, 13, 20);
        let selection = harness.selection();
        harness.wheel(ScrollDelta::Lines(point(0., 3.)), TouchPhase::Moved);
        assert_rows(&mut harness, 10, 17);
        scroll_top(&mut harness);
        assert_rows(&mut harness, 1, 8);
        assert_rows(&mut harness, 1, 8);
        for phase in [TouchPhase::Started, TouchPhase::Moved, TouchPhase::Ended] {
            harness.wheel(ScrollDelta::Pixels(point(px(0.), px(-65.))), phase);
        }
        let moved = harness.rows();
        assert_ne!(
            moved,
            numbered_draft(8)
                .split('\n')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
        scroll_bottom(&mut harness);
        assert_rows(&mut harness, 13, 20);
        assert_eq!(harness.selection(), selection);
        assert_eq!(harness.text(), content);
        harness.assert_focused();
        assert!(harness.submissions.borrow().is_empty());
    }
}

#[test]
fn scroll_preserves_nonempty_selection_focus_and_draft() {
    let mut harness = ComposerHarness::new(720., false, true);
    let content = numbered_draft(20);
    harness.set_text(&content);
    harness.action(text_input::SelectAll);
    let selected = harness.selection();
    assert_eq!(selected, 0..content.encode_utf16().count());
    scroll_top(&mut harness);
    assert_rows(&mut harness, 1, 8);
    assert_eq!(harness.selection(), selected);
    assert_eq!(harness.text(), content);
    harness.assert_focused();
    scroll_bottom(&mut harness);
    assert_rows(&mut harness, 13, 20);
    assert_eq!(harness.selection(), selected);
    assert!(harness.submissions.borrow().is_empty());
}

#[test]
fn composer_scrollbar_drag_and_track_update_visible_text_without_selection_changes() {
    for dark in [false, true] {
        let mut harness = ComposerHarness::new(720., dark, true);
        harness.set_text(&numbered_draft(20));
        let selected = harness.selection();
        let viewport = harness.viewport();
        let bottom_thumb = harness
            .scrollbar()
            .expect("overflowing composer must paint a vertical scrollbar");
        let top_target = point(bottom_thumb.center().x, viewport.top() + px(1.));
        harness.drag(bottom_thumb.center(), top_target);
        assert_rows(&mut harness, 1, 8);
        let top_thumb = harness
            .scrollbar()
            .expect("scrollbar remains visible at top");
        assert!(top_thumb.top() < bottom_thumb.top());
        let frame = harness.draw();
        let scale = harness.scale();
        for glyph in &frame.glyphs {
            let bounds = logical(glyph.bounds, scale).intersect(&logical(glyph.mask, scale));
            if visible(bounds) && contains(viewport, bounds) {
                assert!(
                    !visible(bounds.intersect(&top_thumb)),
                    "scrollbar must not obscure draft glyphs"
                );
            }
        }
        harness.click(point(top_thumb.center().x, viewport.bottom() - px(1.)));
        assert_rows(&mut harness, 13, 20);
        let thumb = harness.scrollbar().unwrap();
        harness.click(point(thumb.center().x, viewport.top() + px(1.)));
        assert_rows(&mut harness, 1, 8);
        let thumb = harness.scrollbar().unwrap();
        harness.drag(
            thumb.center(),
            point(thumb.center().x, viewport.bottom() - px(1.)),
        );
        assert_rows(&mut harness, 13, 20);
        assert_eq!(harness.selection(), selected);
        harness.assert_focused();
        assert!(harness.submissions.borrow().is_empty());
    }
}

#[test]
fn composer_row_bounds_and_scrollbar_follow_actual_overflow_only() {
    let mut harness = ComposerHarness::new(720., false, true);
    let line_height = px(Typography::BODY * Typography::BODY_LINE_HEIGHT).round();
    for (text, rows, overflow) in [
        (String::new(), 1, false),
        ("short draft".into(), 1, false),
        (numbered_draft(8), 8, false),
        (numbered_draft(9), 8, true),
    ] {
        harness.set_text(&text);
        let viewport = harness.viewport();
        let rendered_rows = harness
            .recorder
            .frame()
            .text_rows(viewport, harness.scale());
        assert!(
            (f32::from(harness.viewport().size.height - line_height * rows as f32)).abs() <= 1.,
            "viewport={viewport:?}, line_height={line_height:?}, expected_rows={rows}, text={text:?}, rendered_rows={rendered_rows:?}"
        );
        if !text.is_empty() {
            assert_eq!(harness.rows().len(), rows);
        }
        assert_eq!(harness.scrollbar().is_some(), overflow);
        if !overflow {
            let escaped = *harness.escaped.borrow();
            harness.wheel(ScrollDelta::Lines(point(0., 2.)), TouchPhase::Moved);
            assert_eq!(*harness.escaped.borrow(), escaped + 1);
        }
    }
}

#[test]
fn latin_and_cjk_soft_wrap_scroll_and_reflow_without_losing_text() {
    for character in ['w', '中'] {
        let mut harness = ComposerHarness::new(720., false, true);
        let short = character.to_string().repeat(70);
        harness.set_text(&short);
        let wide_viewport = harness.viewport();
        let wide_height = wide_viewport.size.height;
        let wide_rows = harness
            .recorder
            .frame()
            .text_rows(wide_viewport, harness.scale());
        harness.resize(330.);
        let narrow_viewport = harness.viewport();
        let narrow_rows = harness
            .recorder
            .frame()
            .text_rows(narrow_viewport, harness.scale());
        assert!(
            harness.viewport().size.height > wide_height,
            "text={short:?}, wide_viewport={wide_viewport:?}, wide_rows={wide_rows:?}, narrow_viewport={narrow_viewport:?}, narrow_rows={narrow_rows:?}"
        );
        let content = format!("FIRST{}LAST", character.to_string().repeat(800));
        harness.set_text(&content);
        assert_eq!(harness.rows().len(), 8);
        assert!(harness.rows().last().unwrap().ends_with("LAST"));
        assert!(harness.scrollbar().is_some());
        scroll_top(&mut harness);
        assert!(harness.rows()[0].starts_with("FIRST"));
        harness.resize(720.);
        assert!(harness.rows()[0].starts_with("FIRST"));
        scroll_bottom(&mut harness);
        assert!(harness.rows().last().unwrap().ends_with("LAST"));
        assert_eq!(harness.text(), content);
    }
}

#[test]
fn cursor_and_edit_reveal_caret_after_manual_composer_scroll() {
    let mut harness = ComposerHarness::new(720., false, true);
    let content = numbered_draft(20);
    harness.set_text(&content);
    scroll_top(&mut harness);
    assert_rows(&mut harness, 1, 8);
    harness.action(text_input::Left);
    assert_rows(&mut harness, 13, 20);
    harness.assert_caret_painted();
    harness.action(text_input::Home);
    scroll_bottom(&mut harness);
    assert_rows(&mut harness, 13, 20);
    harness.action(text_input::Right);
    assert_rows(&mut harness, 1, 8);
    harness.assert_caret_painted();
    harness.action(text_input::End);
    scroll_top(&mut harness);
    let input = harness.input.clone();
    harness.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, " appended", window, cx)
        });
    });
    assert!(harness.rows().last().unwrap().ends_with("appended"));
    assert_eq!(harness.text(), format!("{content} appended"));
    harness.assert_caret_painted();
}

#[test]
fn ime_replacement_after_scroll_keeps_composition_and_restores_visible_caret() {
    let mut harness = ComposerHarness::new(720., false, true);
    let content = numbered_draft(20);
    harness.set_text(&content);
    scroll_top(&mut harness);
    let input = harness.input.clone();
    harness.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "拼音", Some(2..2), window, cx);
            assert!(input.is_composing());
            assert_eq!(
                input.marked_text_range(window, cx),
                Some(content.len()..content.len() + 2)
            );
        });
    });
    assert!(harness.rows().last().unwrap().ends_with("拼音"));
    harness.assert_caret_painted();
    scroll_top(&mut harness);
    assert!(harness.update(|_, cx| input.read(cx).is_composing()));
    harness.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, "中文🙂", window, cx)
        });
    });
    assert_eq!(harness.text(), format!("{content}中文🙂"));
    assert!(harness.rows().last().unwrap().ends_with("中文🙂"));
    assert!(!harness.update(|_, cx| input.read(cx).is_composing()));
    harness.assert_caret_painted();
}

#[test]
fn shortening_clearing_and_sending_use_the_full_draft_and_reset_viewport() {
    let mut harness = ComposerHarness::new(720., false, true);
    let content = numbered_draft(20);
    harness.set_text(&content);
    scroll_top(&mut harness);
    assert_rows(&mut harness, 1, 8);
    harness.key("cmd-enter");
    assert_eq!(harness.submissions.borrow().len(), 1);
    assert_eq!(harness.submissions.borrow()[0].content, content);
    let stream = harness.stream.clone();
    harness.update(|_, cx| stream.update(cx, |stream, cx| stream.reject_composer_submission(cx)));
    harness.set_text("replacement");
    assert_eq!(harness.rows(), ["replacement"]);
    assert!(harness.scrollbar().is_none());
    harness.set_text(&content);
    scroll_bottom(&mut harness);
    let input = harness.input.clone();
    harness.update(|_, cx| input.update(cx, TextInput::clear));
    harness.draw();
    assert_eq!(harness.text(), "");
    assert!(harness.scrollbar().is_none());
    harness.set_text("fresh draft");
    assert_eq!(harness.rows(), ["fresh draft"]);
    assert_eq!(
        harness.viewport().size.height,
        px(Typography::BODY * Typography::BODY_LINE_HEIGHT).round()
    );
    harness.assert_caret_painted();
}

#[test]
fn unconsumable_wheel_bubbles_at_both_composer_boundaries() {
    let mut harness = ComposerHarness::new(720., false, true);
    harness.set_text(&numbered_draft(20));
    let horizontal = *harness.escaped.borrow();
    harness.wheel(
        ScrollDelta::Pixels(point(px(80.), px(0.))),
        TouchPhase::Moved,
    );
    assert_eq!(*harness.escaped.borrow(), horizontal + 1);
    assert_rows(&mut harness, 13, 20);
    let at_bottom = *harness.escaped.borrow();
    for _ in 0..3 {
        scroll_bottom(&mut harness);
    }
    assert_eq!(*harness.escaped.borrow(), at_bottom + 3);
    assert_rows(&mut harness, 13, 20);
    let before_up = *harness.escaped.borrow();
    scroll_top(&mut harness);
    assert_eq!(
        *harness.escaped.borrow(),
        before_up,
        "consumed wheel stays local"
    );
    assert_rows(&mut harness, 1, 8);
    for _ in 0..3 {
        scroll_top(&mut harness);
    }
    assert_eq!(*harness.escaped.borrow(), before_up + 3);
    assert_rows(&mut harness, 1, 8);
    assert!(harness.submissions.borrow().is_empty());
}

struct InputHost {
    input: Entity<TextInput>,
    height: Pixels,
    escaped: Rc<RefCell<usize>>,
}

impl Render for InputHost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let escaped = self.escaped.clone();
        div()
            .w_full()
            .h(self.height)
            .flex()
            .flex_col()
            .on_scroll_wheel(move |_, _, _| *escaped.borrow_mut() += 1)
            .child(self.input.clone())
    }
}

#[test]
fn single_line_input_is_not_a_multiline_scroll_viewport() {
    let (mut cx, recorder) = context(false);
    let escaped = Rc::new(RefCell::new(0));
    let window = cx
        .open_window(size(px(480.), px(100.)), |_, cx| {
            let input = cx.new(|cx| TextInput::new(cx, "single", false));
            input.update(cx, |input, cx| input.set_text("single line", cx));
            cx.new(|_| InputHost {
                input,
                height: px(40.),
                escaped: escaped.clone(),
            })
        })
        .unwrap();
    let root = window.root(&mut cx).unwrap();
    let input = cx.update(|cx| root.read(cx).input.clone());
    let (before, viewport, scale) = draw_input(&mut cx, window, &input, &recorder);
    assert_eq!(before.text_rows(viewport, scale), ["single line"]);
    let result = cx.update_window(window.into(), |_, window, cx| {
        input.read(cx).focus_handle(cx).focus(window, cx);
        window.dispatch_event(
            ScrollWheelEvent {
                position: viewport.center(),
                delta: ScrollDelta::Lines(point(0., 10.)),
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
    });
    result.unwrap();
    let (after, bounds, scale) = draw_input(&mut cx, window, &input, &recorder);
    assert_eq!(after.text_rows(bounds, scale), ["single line"]);
    assert_eq!(*escaped.borrow(), 1);
    cx.update(|cx| assert_eq!(input.read(cx).text(), "single line"));
}

#[test]
fn full_editor_keeps_tall_viewport_scrolling_caret_follow_and_editing() {
    let (mut cx, recorder) = context(false);
    let window = cx
        .open_window(size(px(720.), px(480.)), |_, cx| {
            let input =
                cx.new(|cx| TextInput::new_multiline(cx, "editor", 1).with_editor_viewport());
            input.update(cx, |input, cx| input.set_text(&numbered_draft(40), cx));
            cx.new(|_| InputHost {
                input,
                height: px(420.),
                escaped: Rc::new(RefCell::new(0)),
            })
        })
        .unwrap();
    let root = window.root(&mut cx).unwrap();
    let input = cx.update(|cx| root.read(cx).input.clone());
    let (bottom, viewport, scale) = draw_input(&mut cx, window, &input, &recorder);
    let rows = bottom.text_rows(viewport, scale);
    assert!(rows.len() > 8 && rows.len() <= 24);
    assert_eq!(rows.last().unwrap(), "draft 40");
    cx.update_window(window.into(), |_, window, cx| {
        input.read(cx).focus_handle(cx).focus(window, cx);
        window.dispatch_event(
            ScrollWheelEvent {
                position: viewport.center(),
                delta: ScrollDelta::Lines(point(0., 100.)),
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
    })
    .unwrap();
    let (top, viewport, scale) = draw_input(&mut cx, window, &input, &recorder);
    assert_eq!(top.text_rows(viewport, scale).first().unwrap(), "draft 01");
    cx.update_window(window.into(), |_, window, cx| {
        window.dispatch_action(text_input::Left.boxed_clone(), cx)
    })
    .unwrap();
    let (followed, viewport, scale) = draw_input(&mut cx, window, &input, &recorder);
    assert_eq!(
        followed.text_rows(viewport, scale).last().unwrap(),
        "draft 40"
    );
    cx.update_window(window.into(), |_, window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, "edited", window, cx)
        });
    })
    .unwrap();
    let (edited, viewport, scale) = draw_input(&mut cx, window, &input, &recorder);
    assert!(
        edited
            .text_rows(viewport, scale)
            .last()
            .unwrap()
            .contains("edited")
    );
    cx.update(|cx| assert!(input.read(cx).text().contains("edited")));
}

fn draw_input(
    cx: &mut HeadlessAppContext,
    window: WindowHandle<InputHost>,
    input: &Entity<TextInput>,
    recorder: &SceneRecorder,
) -> (PaintedFrame, Bounds<Pixels>, f32) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            drop(window.render_to_image().unwrap());
        })
        .unwrap();
    }
    let (viewport, scale) = cx
        .update_window(window.into(), |_, window, cx| {
            (
                cx.global::<PaintedGeometry>().inputs[&input.entity_id()],
                window.scale_factor(),
            )
        })
        .unwrap();
    (recorder.frame(), viewport, scale)
}
