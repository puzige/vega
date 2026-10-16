use super::*;
use gpui_kit::{AppContext, Modifiers, Render, TestAppContext, VisualTestContext, WindowHandle};

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

fn pointer_input(
    cx: &mut TestAppContext,
    text: &str,
    masked: bool,
) -> (Entity<TextInput>, VisualTestContext) {
    let (window, input) = open_input_mode(cx, masked);
    set_text(&input, text, cx);
    let visual = VisualTestContext::from_window(window.into(), cx);
    (input, visual)
}

fn input_point(
    input: &Entity<TextInput>,
    visual: &VisualTestContext,
    offset: usize,
    fraction: f32,
) -> Point<Pixels> {
    input.read_with(visual, |input, _| {
        let display_offset = input.display_offset_for_content_offset(offset);
        let (row, line) = input
            .last_lines
            .iter()
            .enumerate()
            .rfind(|(_, line)| line.start <= display_offset)
            .expect("painted input line");
        assert!(display_offset <= line.start + line.len);
        let bounds = input.last_bounds.expect("painted input bounds");
        let x = line.line.x_for_index(display_offset - line.start);
        let next = input.display_offset_for_content_offset(input.next_boundary(offset));
        let next_x = line.line.x_for_index(next.saturating_sub(line.start));
        point(
            bounds.left() + x + (next_x - x) * fraction,
            bounds.top() + input.last_line_height * (row as f32 + 0.5),
        )
    })
}

fn mouse_down(visual: &mut VisualTestContext, position: Point<Pixels>, count: usize) {
    visual.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        click_count: count,
        modifiers: Modifiers::default(),
        first_mouse: false,
    });
}

fn mouse_up(visual: &mut VisualTestContext, position: Point<Pixels>, count: usize) {
    visual.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        click_count: count,
        modifiers: Modifiers::default(),
    });
}

fn multi_click(visual: &mut VisualTestContext, position: Point<Pixels>, count: usize) {
    mouse_down(visual, position, count);
    mouse_up(visual, position, count);
}

fn assert_selected(
    input: &Entity<TextInput>,
    visual: &VisualTestContext,
    expected: Range<usize>,
    reversed: bool,
) {
    input.read_with(visual, |input, _| {
        assert_eq!(input.selected_range, expected);
        assert_eq!(input.selection_reversed, reversed);
        assert!(input.content.is_char_boundary(input.selected_range.start));
        assert!(input.content.is_char_boundary(input.selected_range.end));
    });
}

fn assert_clipboard(visual: &VisualTestContext, expected: &str) {
    assert_eq!(
        visual
            .read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(expected)
    );
}

fn assert_selection_quad(visual: &mut VisualTestContext, bounds: Bounds<Pixels>) {
    visual.update(|window, cx| {
        let token = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
        assert_ne!(token, theme(cx).colors.bg_active.into());
        let expected = bounds.scale(window.scale_factor());
        assert!(window.painted_quads().iter().any(|quad| {
            quad.background.as_solid() == Some(token)
                && (quad.bounds.left() - expected.left()).as_f32().abs() <= 1.
                && (quad.bounds.right() - expected.right()).as_f32().abs() <= 1.
                && (quad.bounds.top() - expected.top()).as_f32().abs() <= 1.
                && (quad.bounds.bottom() - expected.bottom()).as_f32().abs() <= 1.
        }));
    });
}

fn selection_bounds(
    input: &Entity<TextInput>,
    visual: &VisualTestContext,
    display_range: Range<usize>,
) -> Bounds<Pixels> {
    input.read_with(visual, |input, _| {
        let line = &input.last_lines[0].line;
        let bounds = input.last_bounds.expect("painted input bounds");
        Bounds::new(
            point(
                bounds.left() + line.x_for_index(display_range.start),
                bounds.top(),
            ),
            size(
                line.x_for_index(display_range.end) - line.x_for_index(display_range.start),
                input.last_line_height,
            ),
        )
    })
}

#[gpui_kit::test]
fn issue432_double_click_selects_word_at_first_and_last_glyph(cx: &mut TestAppContext) {
    let (input, mut visual) = pointer_input(cx, "alpha beta gamma", false);
    for (offset, fraction) in [(6, 0.2), (9, 0.85)] {
        let position = input_point(&input, &visual, offset, fraction);
        multi_click(&mut visual, position, 2);
        assert_selected(&input, &visual, 6..10, false);
        let moved = input_point(&input, &visual, 0, 0.2);
        visual.simulate_mouse_move(moved, None, Modifiers::default());
        assert_selected(&input, &visual, 6..10, false);
        visual.simulate_keystrokes("cmd-c");
        assert_clipboard(&visual, "beta");
    }
    let position = input_point(&input, &visual, 2, 0.);
    visual.simulate_click(position, Modifiers::default());
    assert_selected(&input, &visual, 2..2, false);
}

#[gpui_kit::test]
fn issue432_triple_click_selects_logical_line_across_soft_wraps(cx: &mut TestAppContext) {
    let middle = "alpha beta gamma delta epsilon zeta";
    let text = format!("head\n{middle}\nlast");
    let (input, mut visual) = pointer_input(cx, &text, false);
    let start = "head\n".len();
    let end = start + middle.len();
    let wrapped_offset = input.read_with(&visual, |input, _| {
        input
            .last_lines
            .iter()
            .find(|line| line.start > start && line.start < end)
            .expect("middle logical line is soft wrapped")
            .start
    });
    for (offset, count) in [(start + 2, 3), (wrapped_offset, 4)] {
        let position = input_point(&input, &visual, offset, 0.7);
        multi_click(&mut visual, position, count);
        assert_selected(&input, &visual, start..end, false);
        visual.simulate_keystrokes("cmd-c");
        assert_clipboard(&visual, middle);
    }
    assert_eq!(
        input.read_with(&visual, |input, _| input.text().to_owned()),
        text
    );
}

#[gpui_kit::test]
fn issue432_word_drag_keeps_anchor_unit_and_reverses_direction(cx: &mut TestAppContext) {
    let text = "alpha beta gamma delta";
    let (input, mut visual) = pointer_input(cx, text, false);
    let origin = input_point(&input, &visual, 7, 0.2);
    mouse_down(&mut visual, origin, 2);
    assert_selected(&input, &visual, 6..10, false);
    for (offset, expected, reversed) in [
        (15, 6..16, false),
        (2, 0..10, true),
        (21, 6..22, false),
        (8, 6..10, false),
    ] {
        let target = input_point(&input, &visual, offset, 0.85);
        visual.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
        assert_selected(&input, &visual, expected, reversed);
    }
    mouse_up(&mut visual, origin, 2);
    let target = input_point(&input, &visual, 0, 0.2);
    visual.simulate_mouse_move(target, None, Modifiers::default());
    assert_selected(&input, &visual, 6..10, false);
}

#[gpui_kit::test]
fn issue432_line_drag_extends_and_reverses_by_logical_lines(cx: &mut TestAppContext) {
    let text = "zero\none two\nthree four\nfive";
    let (input, mut visual) = pointer_input(cx, text, false);
    let origin = input_point(&input, &visual, 6, 0.2);
    mouse_down(&mut visual, origin, 3);
    assert_selected(&input, &visual, 5..12, false);
    for (offset, expected, reversed) in [(16, 5..23, false), (1, 0..12, true)] {
        let target = input_point(&input, &visual, offset, 0.75);
        visual.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
        assert_selected(&input, &visual, expected, reversed);
    }
    let target = input_point(&input, &visual, 1, 0.75);
    mouse_up(&mut visual, target, 3);
    visual.simulate_keystrokes("cmd-c");
    assert_clipboard(&visual, "zero\none two");
}

#[gpui_kit::test]
fn issue432_plain_drag_shift_right_click_and_missing_release_keep_selection(
    cx: &mut TestAppContext,
) {
    let (input, mut visual) = pointer_input(cx, "alpha beta", false);
    let start = input_point(&input, &visual, 8, 0.);
    let end = input_point(&input, &visual, 2, 0.);
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert_selected(&input, &visual, 2..8, true);
    let shift = input_point(&input, &visual, 0, 0.);
    visual.simulate_click(
        shift,
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    );
    assert_selected(&input, &visual, 0..8, true);
    visual.simulate_mouse_down(start, MouseButton::Right, Modifiers::default());
    visual.simulate_mouse_up(start, MouseButton::Right, Modifiers::default());
    assert_selected(&input, &visual, 0..8, true);
    let forward = input_point(&input, &visual, 10, 0.);
    visual.simulate_click(
        forward,
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    );
    assert_selected(&input, &visual, 8..10, false);
    let word = input_point(&input, &visual, 7, 0.2);
    mouse_down(&mut visual, word, 2);
    visual.simulate_mouse_move(end, None, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    assert_selected(&input, &visual, 6..10, false);
}

#[gpui_kit::test]
fn issue432_mixed_unicode_words_whitespace_and_symbols_use_utf8_ranges(cx: &mut TestAppContext) {
    let text = "alpha_2 déjà e\u{301}clair Привет  \t中🙂!";
    let (input, mut visual) = pointer_input(cx, text, false);
    for (needle, expected) in [
        ("_2", "alpha_2"),
        ("jà", "déjà"),
        ("clair", "e\u{301}clair"),
        ("вет", "Привет"),
        ("  \t", "  \t"),
        ("中", "中"),
        ("🙂", "🙂"),
        ("!", "!"),
    ] {
        let offset = text.find(needle).expect("fixture target");
        let expected_start = text.find(expected).expect("fixture selection");
        let position = input_point(&input, &visual, offset, 0.2);
        multi_click(&mut visual, position, 2);
        assert_selected(
            &input,
            &visual,
            expected_start..expected_start + expected.len(),
            false,
        );
        visual.simulate_keystrokes("cmd-c");
        assert_clipboard(&visual, expected);
    }
}

#[gpui_kit::test]
fn issue432_empty_lines_end_clicks_and_overflow_are_safe(cx: &mut TestAppContext) {
    let (input, mut visual) = pointer_input(cx, "", false);
    let position = input.read_with(&visual, |input, _| input.last_bounds.unwrap().center());
    multi_click(&mut visual, position, 2);
    multi_click(&mut visual, position, 3);
    assert_selected(&input, &visual, 0..0, false);
    input.update(&mut visual, |input, cx| input.set_text("alpha\n\nbeta", cx));
    visual.run_until_parked();
    let blank = input_point(&input, &visual, 6, 0.);
    for count in [2, 3] {
        multi_click(&mut visual, blank, count);
        assert_selected(&input, &visual, 6..6, false);
    }
    let tail = input_point(&input, &visual, 11, 0.) + point(px(20.), px(0.));
    multi_click(&mut visual, tail, 2);
    assert_selected(&input, &visual, 7..11, false);
    let long = format!("{}\nfinal", "wrapped text ".repeat(100));
    input.update(&mut visual, |input, cx| input.set_text(&long, cx));
    visual.run_until_parked();
    assert!(input.read_with(&visual, |input, _| input.first_visible_row > 0));
    let tail = input_point(&input, &visual, long.len() - 1, 0.8);
    multi_click(&mut visual, tail, 3);
    assert_selected(&input, &visual, long.len() - 5..long.len(), false);
    let word = "a".repeat(300);
    input.update(&mut visual, |input, cx| input.set_text(&word, cx));
    visual.run_until_parked();
    let tail = input_point(&input, &visual, word.len() - 1, 0.85);
    multi_click(&mut visual, tail, 2);
    assert_selected(&input, &visual, 0..word.len(), false);
    visual.simulate_keystrokes("cmd-c");
    assert_clipboard(&visual, &word);
}

#[gpui_kit::test]
fn issue432_mouse_and_keyboard_selection_paint_theme_token_in_light_and_dark(
    cx: &mut TestAppContext,
) {
    let (input, mut visual) = pointer_input(cx, "alpha beta", false);
    for dark in [false, true] {
        visual.update(|window, cx| {
            let (mode, palette) = if dark {
                (
                    gpui_kit::component::ThemeMode::Dark,
                    vega_theme::Theme::dark(),
                )
            } else {
                (
                    gpui_kit::component::ThemeMode::Light,
                    vega_theme::Theme::light(),
                )
            };
            gpui_kit::component::Theme::change(mode, Some(window), cx);
            cx.set_global(palette);
        });
        visual.run_until_parked();
        let position = input_point(&input, &visual, 9, 0.85);
        multi_click(&mut visual, position, 2);
        let bounds = selection_bounds(&input, &visual, 6..10);
        assert_selection_quad(&mut visual, bounds);
        visual.simulate_keystrokes("cmd-a");
        assert_selected(&input, &visual, 0..10, false);
        let bounds = selection_bounds(&input, &visual, 0..10);
        assert_selection_quad(&mut visual, bounds);
        visual.simulate_keystrokes("right shift-left");
        assert_selected(&input, &visual, 9..10, true);
        let bounds = selection_bounds(&input, &visual, 9..10);
        assert_selection_quad(&mut visual, bounds);
    }
}

#[gpui_kit::test]
fn issue432_masked_selection_maps_bullets_and_refuses_copy_and_cut(cx: &mut TestAppContext) {
    let secret = "a中🙂token";
    let (input, mut visual) = pointer_input(cx, secret, true);
    let start = input_point(&input, &visual, 1, 0.);
    let end = input_point(&input, &visual, "a中🙂".len(), 0.);
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert_selected(&input, &visual, 1.."a中🙂".len(), false);
    let bounds = selection_bounds(
        &input,
        &visual,
        MASK_CHAR.len_utf8()..MASK_CHAR.len_utf8() * 3,
    );
    assert_selection_quad(&mut visual, bounds);
    input.read_with(&visual, |input, _| {
        assert_eq!(
            input.last_lines[0].line.text.as_ref(),
            "•".repeat(secret.chars().count())
        );
        assert!(!input.last_lines[0].line.text.contains("token"));
    });
    visual.write_to_clipboard(ClipboardItem::new_string("sentinel".into()));
    visual.simulate_keystrokes("cmd-c cmd-x");
    assert_clipboard(&visual, "sentinel");
    assert_eq!(
        input.read_with(&visual, |input, _| input.text().to_owned()),
        secret
    );
    let position = input_point(&input, &visual, secret.len() - 1, 0.85);
    multi_click(&mut visual, position, 2);
    assert_selected(&input, &visual, "a中🙂".len()..secret.len(), false);
}

#[gpui_kit::test]
fn issue432_selected_replacement_paste_and_ime_keep_editing_contract(cx: &mut TestAppContext) {
    let (input, mut visual) = pointer_input(cx, "alpha beta", false);
    let position = input_point(&input, &visual, 8, 0.2);
    mouse_down(&mut visual, position, 2);
    visual.write_to_clipboard(ClipboardItem::new_string("new\r\nline".into()));
    visual.dispatch_action(Paste);
    assert_eq!(
        input.read_with(&visual, |input, _| input.text().to_owned()),
        "alpha new\nline"
    );
    visual.simulate_mouse_move(position, Some(MouseButton::Left), Modifiers::default());
    assert_selected(&input, &visual, 14..14, false);
    mouse_up(&mut visual, position, 2);
    visual.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
        });
    });
    visual.run_until_parked();
    input.read_with(&visual, |input, _| {
        assert!(input.is_composing());
        assert_eq!(input.marked_range, Some(14..16));
    });
    visual.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "你", None, window, cx);
            input.unmark_text(window, cx);
        });
    });
    visual.run_until_parked();
    assert_eq!(
        input.read_with(&visual, |input, _| input.text().to_owned()),
        "alpha new\nline你"
    );
    assert!(!input.read_with(&visual, |input, _| input.is_composing()));
    visual.dispatch_action(InsertNewline);
    assert_eq!(
        input.read_with(&visual, |input, _| input.text().to_owned()),
        "alpha new\nline你\n"
    );
}
