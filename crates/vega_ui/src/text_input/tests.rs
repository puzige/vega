use super::*;
use gpui::{Render, TestAppContext, WindowHandle};

struct Harness {
    input: Entity<TextInput>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(120.)).child(self.input.clone())
    }
}

fn open_input(cx: &mut TestAppContext) -> (WindowHandle<Harness>, Entity<TextInput>) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let input = cx.new(|cx| TextInput::new_multiline(cx, "draft", 1));
    let root_input = input.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|_| Harness { input: root_input })
        })
        .expect("text input test window")
    });
    cx.run_until_parked();
    (window, input)
}

fn set_text(input: &Entity<TextInput>, text: &str, cx: &mut TestAppContext) {
    input.update(cx, |input, cx| input.set_text(text, cx));
    cx.run_until_parked();
}

#[gpui::test]
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
