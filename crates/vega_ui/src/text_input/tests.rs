use super::*;
use gpui_kit::{AppContext, Render, TestAppContext, WindowHandle};

struct Harness {
    input: Entity<TextInput>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(120.)).child(self.input.clone())
    }
}

fn open_input(cx: &mut TestAppContext) -> (WindowHandle<Harness>, Entity<TextInput>) {
    open_input_mode(cx, false)
}

fn open_input_mode(
    cx: &mut TestAppContext,
    masked: bool,
) -> (WindowHandle<Harness>, Entity<TextInput>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let input = cx.new(|cx| {
        if masked {
            TextInput::new(cx, "secret", true)
        } else {
            TextInput::new_multiline(cx, "draft", 1)
        }
    });
    let root_input = input.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|_| Harness { input: root_input })
        })
        .expect("text input test window")
    });
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| window.activate_window())
        .expect("activate text input window");
    cx.run_until_parked();
    window
        .update(cx, |_, window, _| assert!(window.is_window_active()))
        .expect("active text input window");
    (window, input)
}

fn set_text(input: &Entity<TextInput>, text: &str, cx: &mut TestAppContext) {
    input.update(cx, |input, cx| input.set_text(text, cx));
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn scroll_edge_input_uses_visual_rows_and_protects_cursor_selection_and_marked_text(
    cx: &mut TestAppContext,
) {
    let (window, input) = open_input(cx);
    let draft = "soft wrap ".repeat(300);
    set_text(&input, &draft, cx);
    let (first, total, visible) = input.read_with(cx, |input, _| {
        (input.first_visible_row, input.total_visual_rows, input.rows)
    });
    assert!(first > 0 && total > visible && !draft.contains('\n'));
    window
        .update(cx, |_, window, cx| {
            let focus = input.read(cx).focus_handle.clone();
            focus.focus(window, cx);
        })
        .expect("focus input");
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        let metrics = input.painted_fade_metrics.expect("input fade metrics");
        assert!(metrics.start.y > px(0.));
        assert_eq!(metrics.end.y, px(0.));
        assert_eq!(input.text(), draft);
    });
    input.update(cx, |input, cx| {
        let start = input.last_lines.first().unwrap().start;
        input.selected_range = start..start + 4;
        input.marked_range = Some(start..start + 4);
        input.editor_scroll = Some((input.selected_range.clone(), input.content_revision, first));
        cx.notify();
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert_eq!(input.painted_fade_metrics.unwrap().start.y, px(0.));
        assert!(input.marked_range.is_some());
        assert_eq!(input.text(), draft);
    });
    input.update(cx, |input, cx| {
        input.marked_range = None;
        input.selected_range = 0..0;
        input.editor_scroll = None;
        cx.notify();
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        let metrics = input.painted_fade_metrics.unwrap();
        assert_eq!(metrics.start.y, px(0.));
        assert!(metrics.end.y > px(0.));
        assert_eq!(input.first_visible_row, 0);
    });
    set_text(&input, "short", cx);
    input.read_with(cx, |input, _| {
        let metrics = input.painted_fade_metrics.unwrap();
        assert_eq!((metrics.start.y, metrics.end.y), (px(0.), px(0.)));
    });
}

#[gpui_kit::test]
async fn scroll_edge_single_line_hidden_extent_uses_shaped_advance_and_protects_active_edge(
    cx: &mut TestAppContext,
) {
    let (window, input) = open_input(cx);
    input.update(cx, |input, cx| {
        input.multiline = false;
        input.rows = 1;
        input.max_rows = 1;
        input.set_text(&"wide glyph 汉字 ".repeat(40), cx);
        input.selected_range = 0..0;
    });
    window
        .update(cx, |_, window, cx| {
            let focus = input.read(cx).focus_handle.clone();
            focus.focus(window, cx)
        })
        .expect("focus single line");
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        let bounds = input.last_bounds.unwrap();
        let hidden = input.last_lines[0].line.width() - bounds.size.width;
        assert!(hidden > px(0.));
        let metrics = input.painted_fade_metrics.unwrap();
        assert_eq!(metrics.start.x, px(0.));
        assert_eq!(metrics.end.x, hidden);
        assert_eq!((metrics.start.y, metrics.end.y), (px(0.), px(0.)));
    });
    input.update(cx, |input, cx| {
        input.selected_range = input.content.len()..input.content.len();
        cx.notify();
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert_eq!(input.painted_fade_metrics.unwrap().end.x, px(0.));
        assert_eq!(
            input.selected_range,
            input.content.len()..input.content.len()
        );
    });
}

#[gpui_kit::test]
async fn visual_wrap_grows_shrinks_and_caps_with_cursor_follow(cx: &mut TestAppContext) {
    let (_window, input) = open_input(cx);

    set_text(&input, &"latin".repeat(40), cx);
    let latin_rows = input.read_with(cx, |input, _| input.visible_rows());
    assert!((2..=8).contains(&latin_rows));

    set_text(&input, "short", cx);
    assert_eq!(input.read_with(cx, |input, _| input.visible_rows()), 1);

    set_text(&input, &"中文".repeat(40), cx);
    let cjk_rows = input.read_with(cx, |input, _| input.visible_rows());
    assert!((2..=8).contains(&cjk_rows));

    set_text(&input, &"wrapped".repeat(500), cx);
    let (rows, first) = input.read_with(cx, |input, _| {
        (input.visible_rows(), input.first_visible_row)
    });
    assert_eq!(rows, 8);
    assert!(first > 0, "cursor-follow viewport must expose the tail");

    set_text(&input, "head", cx);
    let (rows, first) = input.read_with(cx, |input, _| {
        (input.visible_rows(), input.first_visible_row)
    });
    assert_eq!((rows, first), (1, 0));
}

#[gpui_kit::test]
async fn at_completion_replaces_only_the_token_body(cx: &mut TestAppContext) {
    let (_window, input) = open_input(cx);
    set_text(&input, "请读 @notes", cx);
    input.update(cx, |input, cx| input.complete_at_query("notes.txt", cx));
    cx.run_until_parked();
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "请读 @notes.txt "
    );
}
