use super::*;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{
    DismissEvent, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    Subscription, VisualTestContext, WeakEntity, size,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

use super::issue147_markdown_selection::{
    assistant_entry, drag, layout_for,
    open_selection_harness as open_unactivated_selection_harness, text_point,
};

struct LinkCapture;

impl LinkCapture {
    fn start() -> Self {
        links::LINK_LAYOUTS.with_borrow_mut(|layouts| *layouts = Some(Vec::new()));
        selection::SELECTION_RUN_LAYOUTS.with_borrow_mut(Vec::clear);
        Self
    }

    fn clear(&self) {
        links::LINK_LAYOUTS.with_borrow_mut(|layouts| {
            layouts.as_mut().expect("active link capture").clear();
        });
    }
}

impl Drop for LinkCapture {
    fn drop(&mut self) {
        links::LINK_LAYOUTS.with_borrow_mut(|layouts| *layouts = None);
    }
}

struct MenuCapture {
    menus: Rc<RefCell<Vec<WeakEntity<PopupMenu>>>>,
    _subscription: Subscription,
}

impl MenuCapture {
    fn start(cx: &mut TestAppContext) -> Self {
        let menus = Rc::new(RefCell::new(Vec::new()));
        let observed = menus.clone();
        let subscription = cx.update(|cx| {
            cx.observe_new::<PopupMenu>(move |_, _, cx| {
                observed.borrow_mut().push(cx.entity().downgrade());
            })
        });
        Self {
            menus,
            _subscription: subscription,
        }
    }

    fn close_fixture(&self, visual: &mut VisualTestContext, cx: &mut TestAppContext) {
        visual.update(|window, _| window.remove_window());
        visual.run_until_parked();
        let menus = self.menus.borrow().clone();
        assert!(!menus.is_empty());
        cx.update(|cx| {
            for menu in &menus {
                if let Some(menu) = menu.upgrade() {
                    menu.update(cx, |_, cx| cx.emit(DismissEvent));
                }
            }
        });
        cx.run_until_parked();
        for menu in &menus {
            assert!(menu.upgrade().is_none());
        }
    }
}

fn init_link_ui(cx: &mut TestAppContext) {
    cx.update(|cx| {
        if !cx.has_global::<gpui_kit::component::Theme>() {
            gpui_kit::init(cx);
        }
    });
}

fn activate_harness(visual: &mut VisualTestContext) {
    visual.update(|window, _| window.activate_window());
    visual.run_until_parked();
    assert!(visual.update(|window, _| window.is_window_active()));
}

fn open_selection_harness(
    cx: &mut TestAppContext,
    entry: StreamEntry,
) -> (Entity<ConversationStream>, VisualTestContext) {
    init_link_ui(cx);
    let (stream, mut visual) = open_unactivated_selection_harness(cx, entry);
    activate_harness(&mut visual);
    (stream, visual)
}

fn painted_link(target: &str) -> links::PaintedLinkText {
    links::LINK_LAYOUTS.with_borrow(|layouts| {
        layouts
            .as_ref()
            .expect("active link capture")
            .iter()
            .rev()
            .find(|text| text.links.iter().any(|(_, url)| url == target))
            .cloned()
            .expect("link registered by production renderer")
    })
}

fn link_point(target: &str) -> Point<Pixels> {
    let text = painted_link(target);
    text.regions
        .iter()
        .find(|region| text.links[region.index].1 == target)
        .expect("painted link region")
        .bounds
        .center()
}

fn code_fragment_point(target: &str) -> Point<Pixels> {
    links::LINK_LAYOUTS.with_borrow(|layouts| {
        layouts
            .as_ref()
            .expect("active link capture")
            .iter()
            .rev()
            .find(|text| {
                text.layout.text() == "code🙂" && text.links.iter().any(|(_, url)| url == target)
            })
            .expect("linked inline-code fragment from production renderer")
            .layout
            .bounds()
            .center()
    })
}

fn finish_click(visual: &mut VisualTestContext) {
    visual.executor().advance_clock(Duration::from_millis(501));
    visual.run_until_parked();
}

fn dismiss_context_menu(visual: &mut VisualTestContext) {
    redraw(visual);
    let id = gpui_kit::ElementId::Name("popup-menu".into());
    let menu = visual
        .update(|window, _| gpui_kit::base::test_support::find(window, &[], &id))
        .expect("painted production context menu");
    assert_eq!(menu.role(), Some(gpui_kit::Role::Menu));
    assert!(menu.visible());
    assert_eq!(menu.focused(), Some(true));
    assert!(menu.bounds().size.width > px(0.));
    assert!(menu.bounds().size.height > px(0.));
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    redraw(visual);
    assert!(
        visual
            .update(|window, _| gpui_kit::base::test_support::find(window, &[], &id))
            .is_none()
    );
    assert_eq!(visual.opened_url(), None);
}

fn model(source: &str) -> StreamModel {
    let StreamEntry::Assistant { model, .. } = assistant_entry(source) else {
        panic!("assistant entry")
    };
    model
}

struct ReadHarness {
    model: StreamModel,
    identity: u64,
    shown: bool,
    focus: FocusHandle,
}

impl Render for ReadHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus)
            .child(gpui_kit::base::TextSelectionLayer)
            .children(self.shown.then(|| {
                markdown_item(
                    &self.model,
                    None,
                    &vega_theme::Theme::light().colors,
                    &format!("issue357-read-{}", self.identity),
                )
            }))
    }
}

fn open_read_harness(
    cx: &mut TestAppContext,
    source: &str,
) -> (Entity<ReadHarness>, VisualTestContext) {
    init_link_ui(cx);
    init_permission_test(cx);
    let model = model(source);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|cx| ReadHarness {
                model,
                identity: 1,
                shown: true,
                focus: cx.focus_handle(),
            })
        })
        .expect("markdown read window")
    });
    cx.run_until_parked();
    let root = window.root(cx).expect("read harness root");
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(600.), px(1500.)));
    visual.update(|window, cx| {
        let focus = root.read(cx).focus.clone();
        focus.focus(window, cx);
    });
    visual.run_until_parked();
    activate_harness(&mut visual);
    (root, visual)
}

fn redraw(visual: &mut VisualTestContext) {
    visual.update(|window, cx| {
        window.refresh();
        let _ = window.draw(cx);
    });
    visual.run_until_parked();
}

#[test]
fn issue357_inline_autolink_title_and_nested_labels_retain_urls() {
    let source = "[inline](https://inline.example) <https://auto.example> [title](https://title.example \"title\") [**bold**](https://bold.example) **[outer](https://outer.example)** [*italic*](https://italic.example) *[outer-italic](https://outer-italic.example)* [~~old~~](https://old.example) [`代码🙂`](https://code.example) plain";
    let model = model(source);
    let spans: Vec<_> = model
        .committed_lines
        .iter()
        .flat_map(|line| &line.spans)
        .collect();
    for (label, target, style) in [
        ("inline", "https://inline.example", SpanStyle::Link),
        (
            "https://auto.example",
            "https://auto.example",
            SpanStyle::Link,
        ),
        ("title", "https://title.example", SpanStyle::Link),
        ("bold", "https://bold.example", SpanStyle::Strong),
        ("outer", "https://outer.example", SpanStyle::Strong),
        ("italic", "https://italic.example", SpanStyle::Emphasis),
        (
            "outer-italic",
            "https://outer-italic.example",
            SpanStyle::Emphasis,
        ),
        ("old", "https://old.example", SpanStyle::Strikethrough),
        ("代码🙂", "https://code.example", SpanStyle::Code),
    ] {
        let span = spans
            .iter()
            .find(|span| span.text == label)
            .expect("link label span");
        assert_eq!(span.link.as_deref(), Some(target));
        assert_eq!(span.style, style);
    }
    assert!(
        spans
            .iter()
            .find(|span| span.text == " plain")
            .expect("plain suffix")
            .link
            .is_none()
    );
    let mut streamed = MarkdownStream::new();
    for delta in split_deltas(source, 357) {
        streamed.append(&delta);
    }
    streamed.finish();
    let mut streamed_model = StreamModel::default();
    streamed_model.sync(&streamed.snapshot(), &StreamCounters::default());
    assert_eq!(streamed_model.committed_lines, model.committed_lines);
}

#[test]
fn issue357_link_overlay_keeps_nested_fonts_and_one_pixel_accent_underline() {
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        let colors = palette.colors;
        let source = "[plain](https://example.com) [**bold**](https://example.com) **[outer](https://example.com)** [*italic*](https://example.com) [`code`](https://example.com)";
        let model = model(source);
        let line = &model.committed_lines[0];
        let base = message_run_style(colors.text_primary);
        let runs = render_rows::block_text_runs(&line.spans, base.clone(), &colors);
        for (span, run) in line.spans.iter().zip(runs) {
            if span.link.is_none() {
                assert!(run.underline.is_none());
                assert_eq!(run.color, colors.text_primary.into());
                continue;
            }
            assert_eq!(run.color, colors.accent.into());
            let underline = run.underline.expect("link underline");
            assert_eq!(underline.thickness, px(1.));
            assert_eq!(underline.color, Some(colors.accent.into()));
            assert!(!underline.wavy);
            match span.style {
                SpanStyle::Strong => assert_eq!(run.font.weight, FontWeight::BOLD),
                SpanStyle::Emphasis => assert_eq!(run.font.style, FontStyle::Italic),
                SpanStyle::Code => {
                    assert_eq!(run.font.family.as_ref(), MONOFONT);
                    assert_eq!(run.background_color, Some(colors.conversation_code.into()));
                }
                _ => assert_eq!(run.font, base.font()),
            }
        }
        let mut link_style = base.clone();
        apply_span_style(&mut link_style, SpanStyle::Link, &colors);
        assert_eq!(link_style.font_size, base.font_size);
        assert_eq!(link_style.font_weight, base.font_weight);
        assert_eq!(link_style.font_family, base.font_family);
        assert_eq!(link_style.line_height, base.line_height);
    }
}

#[test]
fn issue357_utf8_fragment_link_ranges_are_relative_to_each_fragment() {
    let spans = vec![
        StreamSpan {
            text: "开头 ".into(),
            style: SpanStyle::Plain,
            link: None,
        },
        StreamSpan {
            text: "你好".into(),
            style: SpanStyle::Link,
            link: Some("https://example.com".into()),
        },
        StreamSpan {
            text: "代码🙂".into(),
            style: SpanStyle::Code,
            link: Some("https://example.com".into()),
        },
        StreamSpan {
            text: " end".into(),
            style: SpanStyle::Plain,
            link: None,
        },
    ];
    let ranges = links::link_ranges(&spans);
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let start = text.find("好").unwrap();
    let end = text.find(" end").unwrap();
    let fragments = links::fragment_link_ranges(&ranges, &(start..end));
    assert_eq!(fragments.len(), 2);
    assert_eq!(&text[start..end][fragments[0].0.clone()], "好");
    assert_eq!(&text[start..end][fragments[1].0.clone()], "代码🙂");
    assert!(
        fragments
            .iter()
            .all(|(_, target)| target == "https://example.com")
    );
    assert!(links::fragment_link_ranges(&ranges, &(0.."开头 ".len())).is_empty());
}

fn container_document() -> &'static str {
    "## [Heading](https://heading.example)\n\nParagraph [链接🙂](https://paragraph.example) suffix\n\n- [List](https://list.example)\n\n> [Quote](https://quote.example)\n\n| Left | Center | Right |\n|:---|:---:|---:|\n| [左](https://left.example) | [中](https://center.example) | [右](https://right.example) |\n\nBefore [前`code🙂`后](https://code.example) after"
}

fn click_every_container(visual: &mut VisualTestContext) {
    for target in [
        "https://heading.example",
        "https://paragraph.example",
        "https://list.example",
        "https://quote.example",
        "https://left.example",
        "https://center.example",
        "https://right.example",
        "https://code.example",
    ] {
        let painted = painted_link(target);
        for (range, _) in &painted.links {
            assert!(painted.layout.text().get(range.clone()).is_some());
        }
        let point = if target == "https://code.example" {
            code_fragment_point(target)
        } else {
            link_point(target)
        };
        visual.simulate_click(point, Modifiers::default());
        finish_click(visual);
        assert_eq!(visual.opened_url().as_deref(), Some(target));
    }
}

#[gpui_kit::test]
fn issue357_selectable_production_renderer_opens_links_in_every_container(cx: &mut TestAppContext) {
    let _capture = LinkCapture::start();
    let (_, mut visual) = open_selection_harness(cx, assistant_entry(container_document()));
    assert_eq!(visual.opened_url(), None);
    click_every_container(&mut visual);
}

#[gpui_kit::test]
fn issue357_read_production_renderer_opens_links_in_every_container(cx: &mut TestAppContext) {
    let _capture = LinkCapture::start();
    let (_, mut visual) = open_read_harness(cx, container_document());
    assert_eq!(visual.opened_url(), None);
    click_every_container(&mut visual);
}

#[gpui_kit::test]
fn issue357_drag_copy_and_preexisting_selection_keep_window_selection_semantics(
    cx: &mut TestAppContext,
) {
    let _capture = LinkCapture::start();
    let source = "prefix [Alpha你好🙂](https://selection.example) suffix";
    let entry = assistant_entry(source);
    let StreamEntry::Assistant { copy, .. } = &entry else {
        panic!("assistant")
    };
    let copy = copy.clone();
    let (_, mut visual) = open_selection_harness(cx, entry);
    let text = "prefix Alpha你好🙂 suffix";
    let layout = layout_for(text);
    let start = text.find("Alpha").unwrap();
    let end = text.find(" suffix").unwrap();
    drag(
        &mut visual,
        text_point(&layout, start, false),
        text_point(&layout, end, true),
    );
    finish_click(&mut visual);
    assert_eq!(visual.opened_url(), None);
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        "Alpha你好🙂"
    );
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("Alpha你好🙂")
    );
    assert_eq!(copy.original_text(), source);

    visual.simulate_click(
        link_point("https://selection.example"),
        Modifiers::default(),
    );
    assert_eq!(
        visual.update(gpui_kit::base::TextSelection::selected_text),
        ""
    );
    finish_click(&mut visual);
    assert_eq!(
        visual.opened_url().as_deref(),
        Some("https://selection.example")
    );
}

#[gpui_kit::test]
fn issue357_click_slop_allows_motion_without_selecting_a_character(cx: &mut TestAppContext) {
    let _capture = LinkCapture::start();
    let (_, mut visual) =
        open_selection_harness(cx, assistant_entry("[MMMMMMMM](https://slop.example)"));
    let layout = layout_for("MMMMMMMM");
    let start = text_point(&layout, 0, false);
    let end = start + gpui_kit::point(px(1.), px(0.));
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    assert!(
        visual
            .update(gpui_kit::base::TextSelection::selected_text)
            .is_empty()
    );
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    finish_click(&mut visual);
    assert_eq!(visual.opened_url().as_deref(), Some("https://slop.example"));
}

#[gpui_kit::test]
fn issue357_first_click_is_cancelled_before_double_and_triple_click_selection(
    cx: &mut TestAppContext,
) {
    let capture = LinkCapture::start();
    for count in [2, 3] {
        capture.clear();
        let (_, mut visual) =
            open_selection_harness(cx, assistant_entry("[Alpha](https://multi.example)"));
        let position = link_point("https://multi.example");
        visual.simulate_click(position, Modifiers::default());
        visual.executor().advance_clock(Duration::from_millis(200));
        visual.run_until_parked();
        assert_eq!(visual.opened_url(), None);
        visual.simulate_event(MouseDownEvent {
            position,
            button: MouseButton::Left,
            click_count: count,
            modifiers: Modifiers::default(),
            first_mouse: false,
        });
        visual.simulate_event(MouseUpEvent {
            position,
            button: MouseButton::Left,
            click_count: count,
            modifiers: Modifiers::default(),
        });
        finish_click(&mut visual);
        assert_eq!(visual.opened_url(), None);
        assert_eq!(
            visual.update(gpui_kit::base::TextSelection::selected_text),
            "Alpha"
        );
    }
}

#[gpui_kit::test]
fn issue357_shift_right_different_link_and_drag_out_return_do_not_open(cx: &mut TestAppContext) {
    let capture = LinkCapture::start();
    let menus = MenuCapture::start(cx);
    for action in ["shift", "right", "other-link", "return"] {
        capture.clear();
        let (_, mut visual) = open_selection_harness(
            cx,
            assistant_entry("[Alpha](https://first.example) plain [Beta](https://second.example)"),
        );
        let start = link_point("https://first.example");
        let other = link_point("https://second.example");
        match action {
            "shift" => visual.simulate_click(
                start,
                Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
            ),
            "right" => {
                visual.simulate_mouse_down(start, MouseButton::Right, Modifiers::default());
                visual.simulate_mouse_up(start, MouseButton::Right, Modifiers::default());
            }
            "other-link" => {
                visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
                visual.simulate_mouse_up(other, MouseButton::Left, Modifiers::default());
            }
            _ => {
                visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
                visual.simulate_mouse_move(other, Some(MouseButton::Left), Modifiers::default());
                visual.simulate_mouse_move(start, Some(MouseButton::Left), Modifiers::default());
                visual.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
            }
        }
        finish_click(&mut visual);
        assert_eq!(visual.opened_url(), None, "{action}");
        if action == "right" {
            dismiss_context_menu(&mut visual);
            menus.close_fixture(&mut visual, cx);
        }
    }
}

#[gpui_kit::test]
fn issue357_any_later_press_cancels_the_pending_single_click(cx: &mut TestAppContext) {
    let capture = LinkCapture::start();
    let menus = MenuCapture::start(cx);
    for button in [MouseButton::Left, MouseButton::Right] {
        capture.clear();
        let (_, mut visual) =
            open_selection_harness(cx, assistant_entry("[Alpha](https://cancel.example) plain"));
        visual.simulate_click(link_point("https://cancel.example"), Modifiers::default());
        let plain = layout_for("Alpha plain");
        let position = text_point(&plain, "Alpha ".len(), false);
        visual.simulate_mouse_down(position, button, Modifiers::default());
        visual.simulate_mouse_up(position, button, Modifiers::default());
        finish_click(&mut visual);
        assert_eq!(visual.opened_url(), None);
        if button == MouseButton::Right {
            dismiss_context_menu(&mut visual);
            menus.close_fixture(&mut visual, cx);
        }
    }
}

#[gpui_kit::test]
fn issue357_unsupported_schemes_and_non_link_text_never_open(cx: &mut TestAppContext) {
    let _capture = LinkCapture::start();
    let (_, mut visual) = open_selection_harness(
        cx,
        assistant_entry(
            "plain [script](javascript:alert) [file](file:///tmp/test) [relative](/relative)",
        ),
    );
    let text = "plain script file relative";
    let layout = layout_for(text);
    for label in ["plain", "script", "file", "relative"] {
        visual.simulate_click(
            text_point(&layout, text.find(label).unwrap(), false),
            Modifiers::default(),
        );
        finish_click(&mut visual);
        assert_eq!(visual.opened_url(), None);
    }
}

#[gpui_kit::test]
fn issue357_mailto_and_uppercase_http_open_through_the_production_renderer(
    cx: &mut TestAppContext,
) {
    let _capture = LinkCapture::start();
    let (_, mut visual) = open_read_harness(
        cx,
        "[Mail](mailto:user@example.com) [Web](HTTPS://example.com/path)",
    );
    for target in ["mailto:user@example.com", "HTTPS://example.com/path"] {
        visual.simulate_click(link_point(target), Modifiers::default());
        finish_click(&mut visual);
        assert_eq!(visual.opened_url().as_deref(), Some(target));
    }
}

#[gpui_kit::test]
fn issue357_wrapped_utf8_link_hits_last_glyph_and_excludes_line_tail_blank_space(
    cx: &mut TestAppContext,
) {
    let _capture = LinkCapture::start();
    let target = "https://wrapped.example";
    let label = format!("{}终🙂", "中文 mixed link ".repeat(15));
    let source = format!("prefix [{label}]({target}) suffix");
    let (_, mut visual) = open_read_harness(cx, &source);
    visual.simulate_resize(size(px(330.), px(1500.)));
    redraw(&mut visual);
    let painted = painted_link(target);
    assert!(
        painted
            .layout
            .line_layouts()
            .iter()
            .any(|line| !line.wrap_boundaries.is_empty())
    );
    let text = painted.layout.text();
    let end = text.find(" suffix").unwrap();
    let (glyph_start, _) = text[..end].char_indices().last().unwrap();
    let line = painted.layout.line_layout_for_index(end).unwrap();
    let advance =
        line.unwrapped_layout.x_for_index(end) - line.unwrapped_layout.x_for_index(glyph_start);
    assert!(advance > px(0.));
    let endpoint = painted.layout.position_for_index(end).unwrap();
    let y = endpoint.y + painted.layout.line_height() / 2.;
    let blank = gpui_kit::point(endpoint.x + px(4.), y);
    visual.simulate_click(blank, Modifiers::default());
    finish_click(&mut visual);
    assert_eq!(visual.opened_url(), None);
    let glyph = gpui_kit::point(endpoint.x - advance / 2., y);
    visual.simulate_click(glyph, Modifiers::default());
    finish_click(&mut visual);
    assert_eq!(visual.opened_url().as_deref(), Some(target));
}

#[gpui_kit::test]
fn issue357_aligned_table_link_hits_follow_cell_and_glyph_geometry(cx: &mut TestAppContext) {
    let capture = LinkCapture::start();
    for selectable in [false, true] {
        capture.clear();
        let suffix = if selectable { "selection" } else { "read" };
        let center_target = format!("https://center-{suffix}.example");
        let right_target = format!("https://right-{suffix}.example");
        let source = format!(
            "| Left | Center | Right |\n|:---|:---:|---:|\n| text | [Center]({center_target}) | [Right]({right_target}) |"
        );
        let mut visual = if selectable {
            open_selection_harness(cx, assistant_entry(&source)).1
        } else {
            open_read_harness(cx, &source).1
        };
        let mut targets = Vec::new();
        let before = visual.opened_url();
        for (selector, target, center) in [
            ("markdown-table-1-0-1-1", center_target.as_str(), true),
            ("markdown-table-1-0-1-2", right_target.as_str(), false),
        ] {
            let cell = visual
                .debug_bounds(selector)
                .expect("production table cell");
            let painted = painted_link(target);
            let line = painted
                .layout
                .line_layouts()
                .first()
                .cloned()
                .expect("cell glyph layout");
            let width = line.unwrapped_layout.width;
            let padding = px(Layout::MARKDOWN_TABLE_PADDING_X);
            let inner_width = cell.size.width - padding * 2.;
            assert!(inner_width > width + px(8.));
            let x = if center {
                cell.left() + padding + (inner_width - width) / 2.
            } else {
                cell.right() - padding - width
            };
            let y = cell.top()
                + px(Layout::MARKDOWN_TABLE_PADDING_Y)
                + painted.layout.line_height() / 2.;
            let blank = gpui_kit::point(cell.left() + padding + px(1.), y);
            assert!(blank.x < x);
            visual.simulate_click(blank, Modifiers::default());
            finish_click(&mut visual);
            assert_eq!(visual.opened_url(), before);
            targets.push((target.to_owned(), gpui_kit::point(x + width / 2., y)));
        }
        for (target, point) in targets {
            visual.simulate_click(point, Modifiers::default());
            finish_click(&mut visual);
            assert_eq!(visual.opened_url().as_deref(), Some(target.as_str()));
        }
    }
}

#[gpui_kit::test]
fn issue357_one_pixel_drag_into_code_padding_then_back_does_not_open(cx: &mut TestAppContext) {
    let _capture = LinkCapture::start();
    let target = "https://padding.example";
    let (_, mut visual) = open_read_harness(cx, "[前`code🙂`后](https://padding.example)");
    let center = code_fragment_point(target);
    let bounds = links::LINK_LAYOUTS.with_borrow(|layouts| {
        layouts
            .as_ref()
            .unwrap()
            .iter()
            .rev()
            .find(|text| text.layout.text() == "code🙂")
            .unwrap()
            .layout
            .bounds()
    });
    let start = gpui_kit::point(bounds.left() + px(0.5), center.y);
    let padding = gpui_kit::point(bounds.left() - px(0.5), center.y);
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(padding, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_move(start, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
    finish_click(&mut visual);
    assert_eq!(visual.opened_url(), None);
}

#[gpui_kit::test]
fn issue357_pending_click_is_cancelled_on_blur_unmount_and_identity_reuse(cx: &mut TestAppContext) {
    let capture = LinkCapture::start();
    for action in ["window-blur", "focus-out", "unmount", "identity", "target"] {
        capture.clear();
        let (root, mut visual) = open_read_harness(cx, "[Alpha](https://stale.example)");
        visual.simulate_click(link_point("https://stale.example"), Modifiers::default());
        match action {
            "window-blur" => {
                visual.deactivate_window();
                assert!(!visual.update(|window, _| window.is_window_active()));
                activate_harness(&mut visual);
            }
            "focus-out" => {
                let focus = visual.update(|_, cx| cx.focus_handle());
                visual.update(|window, cx| focus.focus(window, cx));
            }
            "unmount" => root.update(&mut visual, |root, cx| {
                root.shown = false;
                cx.notify();
            }),
            "identity" => root.update(&mut visual, |root, cx| {
                root.identity += 1;
                cx.notify();
            }),
            _ => root.update(&mut visual, |root, cx| {
                root.model = model("[Alpha](https://replacement.example)");
                cx.notify();
            }),
        }
        redraw(&mut visual);
        finish_click(&mut visual);
        assert_eq!(visual.opened_url(), None, "{action}");
    }
}

#[gpui_kit::test]
async fn issue357_virtualized_thinking_fragments_keep_link_targets(cx: &mut TestAppContext) {
    let _capture = LinkCapture::start();
    init_link_ui(cx);
    let (window, stream, _) = open_controller_stream(cx, "issue357-thinking");
    let source = "Before [前`code🙂`后](https://thinking.example) after";
    let card = stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "issue357-thinking".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "issue357-thinking".into(),
                delta: source.into(),
            },
            cx,
        );
        let card = stream.active_thinking.clone().expect("thinking card");
        stream.run_activity_groups["issue357-thinking"]
            .update(cx, |group, cx| group.reveal_segment(0, cx));
        card
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    activate_harness(&mut visual);
    assert!(card.read_with(&visual, |card, _| card.test_body_fragment_count()) > 0);
    let painted = painted_link("https://thinking.example");
    assert!(
        painted
            .links
            .iter()
            .all(|(range, _)| painted.layout.text().get(range.clone()).is_some())
    );
    visual.simulate_click(
        code_fragment_point("https://thinking.example"),
        Modifiers::default(),
    );
    finish_click(&mut visual);
    assert_eq!(
        visual.opened_url().as_deref(),
        Some("https://thinking.example")
    );
    assert_eq!(
        card.read_with(&visual, |card, _| card.test_source_copy().original_text()),
        source
    );
}
