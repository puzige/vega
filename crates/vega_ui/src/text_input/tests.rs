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

#[gpui_kit::test]
async fn prediction_uses_placeholder_layout_without_changing_real_text_or_viewport(
    cx: &mut TestAppContext,
) {
    for dark in [false, true] {
        let (window, input) = open_input(cx);
        cx.update(|cx| {
            cx.set_global(if dark {
                vega_theme::Theme::dark()
            } else {
                vega_theme::Theme::light()
            })
        });
        let rows = input.read_with(cx, |input, _| input.visible_rows());
        input.update(cx, |input, cx| {
            input.set_prediction(Some("中文 abc 😀 ".repeat(16)), cx)
        });
        cx.run_until_parked();
        input.read_with(cx, |input, _| {
            assert_eq!(input.text(), "");
            assert_eq!(input.visible_rows(), rows);
            assert_eq!(input.selected_range, 0..0);
            assert_eq!(input.last_lines.len(), rows);
            assert!(input.last_lines[0].line.text.starts_with("中文"));
            assert!(!input.last_lines[0].line.text.contains("draft"));
        });
        window
            .update(cx, |_, window, cx| {
                input.update(cx, |input, cx| {
                    let mut actual = None;
                    assert_eq!(
                        input.text_for_range(0..100, &mut actual, window, cx),
                        Some(String::new())
                    );
                    input.select_all(&SelectAll, window, cx);
                    assert_eq!(
                        input.selected_text_range(false, window, cx).unwrap().range,
                        0..0
                    );
                    assert_eq!(
                        input.character_index_for_point(
                            input.last_bounds.unwrap().center(),
                            window,
                            cx
                        ),
                        Some(0)
                    );
                });
            })
            .unwrap();
    }
}
