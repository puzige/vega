use super::*;
use crate::conversation_stream::code_viewport::tests::{Fixture, long_code};
use gpui_kit::TestAppContext;

fn thinking(
    source: &str,
    copy: &MessageCopy,
    focus: &FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let style = PreparationStyle {
        width: px(320.),
        font: window.text_style().font(),
        colors: theme(cx).colors,
        scale: window.scale_factor(),
    };
    let prepared = Arc::new(PreparedBody::new(
        source,
        0,
        0,
        &style,
        window.text_system(),
    ));
    let document = SelectionDocumentBuilder::projected(
        copy.clone(),
        focus.clone(),
        gpui_kit::base::Theme::global(cx).tokens.colors.selection,
        prepared.text.clone(),
        0,
    );
    let horizontal_scrolls = window
        .use_keyed_state("thinking-test-scrolls", cx, |_, _| {
            Rc::new(RefCell::new(std::collections::HashMap::new()))
        })
        .read(cx)
        .clone();
    let body = BodyElement {
        horizontal_scrolls,
        prepared,
        document: document.clone(),
    };
    document.wrap(body.into_any_element(), window, cx)
}

#[gpui_kit::test]
fn thinking_code_routes_wheel_and_keeps_viewport_capture(cx: &mut TestAppContext) {
    Fixture::new(cx, thinking, long_code()).verify_wheel_routing();
}

#[gpui_kit::test]
fn thinking_horizontal_gesture_releases_for_vertical_input(cx: &mut TestAppContext) {
    Fixture::new(cx, thinking, long_code()).verify_gesture_release_and_timeout();
}

#[gpui_kit::test]
fn thinking_scrollbar_drags_without_selecting_and_text_still_copies(cx: &mut TestAppContext) {
    Fixture::new(cx, thinking, long_code()).verify_scrollbar_and_selection(px(
        Layout::MARKDOWN_CODE_PADDING + Typography::CODE * Typography::CODE_LINE_HEIGHT / 2.,
    ));
}

#[gpui_kit::test]
fn thinking_code_offsets_remain_independent_after_append(cx: &mut TestAppContext) {
    Fixture::new(cx, thinking, format!("{}\n{}", long_code(), long_code()))
        .verify_independent_state_and_append();
}

#[gpui_kit::test]
fn fitting_thinking_code_contains_horizontal_input_and_passes_vertical(cx: &mut TestAppContext) {
    Fixture::new(cx, thinking, "```text\nx\n```\n".into()).verify_fitting_content();
}
