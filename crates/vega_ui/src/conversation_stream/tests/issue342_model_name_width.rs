use super::*;
use gpui_kit::{Bounds, Modifiers, Pixels, VisualTestContext, size};

const LONG_MODEL: &str = "deepseek-v4.1-flash";

const OLD_BUDGET_MODEL: &str = "deepseek-v4.1-flash-preview-2026-10-13";

const OVERLONG_MODEL: &str = "gpt-6-astra-ultra-extended-preview-2026-09-14-build-0001";

const COMPACT_COLUMN_WIDTH: f32 = 496.;

const NARROW_WINDOW_WIDTH: f32 = 420.;

const NARROW_WINDOW_HEIGHT: f32 = 700.;

fn open_model_stream(
    cx: &mut TestAppContext,
    thread_id: &str,
    model: &str,
) -> (WindowHandle<StreamHarness>, Entity<ConversationStream>) {
    let (window, stream, _events) = open_controller_stream(cx, thread_id);
    let model = model.to_string();
    stream.update(cx, |stream, cx| {
        stream.composer_defaults.model = model.clone();
        stream.thread.model = model.clone();
        stream.apply_model_options(vec![model.clone(), "other-model".into()], cx);
    });
    cx.run_until_parked();
    (window, stream)
}

fn open_tiered_model_stream(
    cx: &mut TestAppContext,
    thread_id: &str,
    model: &str,
) -> (WindowHandle<StreamHarness>, Entity<ConversationStream>) {
    let (window, stream) = open_model_stream(cx, thread_id, model);
    stream.update(cx, |stream, cx| {
        let mut profile = ReasoningProfileProjection::unknown("owned", model);
        profile.support = ReasoningSupport::Optional;
        profile.efforts = ["low", "medium", "high"]
            .iter()
            .map(|tier| (*tier).to_string())
            .collect();
        profile.preference = ReasoningChoice::ProviderDefault;
        stream.apply_reasoning_profile(profile, cx);
    });
    cx.run_until_parked();
    (window, stream)
}

fn set_column_width(stream: &Entity<ConversationStream>, width: f32, cx: &mut TestAppContext) {
    stream.update(cx, |stream, cx| stream.set_workspace_width(width, cx));
    cx.run_until_parked();
}

fn shaped_width(
    window: WindowHandle<StreamHarness>,
    text: &str,
    cx: &mut TestAppContext,
) -> Pixels {
    let text = text.to_string();
    window
        .update(cx, |_, window, cx| {
            let colors = theme(cx).colors;
            let font = window.text_style().font();
            window
                .text_system()
                .shape_line(
                    text.clone().into(),
                    px(Typography::SIDEBAR),
                    &[TextRun {
                        len: text.len(),
                        font,
                        color: colors.text_secondary.into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                )
                .width()
        })
        .expect("measure model label")
}

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing selector {selector}"))
}

fn assert_aligned(actual: Pixels, expected: Pixels, label: &str) {
    assert!(
        (f32::from(actual) - f32::from(expected)).abs() <= 1.0,
        "{label}: {actual:?} vs {expected:?}"
    );
}

fn assert_holds_whole_name(
    window: WindowHandle<StreamHarness>,
    model: &str,
    layout: &str,
    cx: &mut TestAppContext,
) {
    let label = bounds(window, "composer-model-label", cx);
    let needed = shaped_width(window, model, cx);
    assert!(
        label.size.width + px(0.5) >= needed,
        "{layout}: the label box ({:?}) must hold the whole shaped name ({needed:?})",
        label.size.width
    );
}

#[gpui_kit::test]
async fn issue342_trigger_shows_the_full_name_in_both_layouts(cx: &mut TestAppContext) {
    let (window, stream) = open_model_stream(cx, "issue342-full-name", LONG_MODEL);
    assert!(!stream.read_with(cx, |stream, _| stream.compact_workspace));
    assert_holds_whole_name(window, LONG_MODEL, "ordinary", cx);

    set_column_width(&stream, COMPACT_COLUMN_WIDTH, cx);
    assert!(stream.read_with(cx, |stream, _| stream.compact_workspace));
    assert_holds_whole_name(window, LONG_MODEL, "compact", cx);
}

#[gpui_kit::test]
async fn issue342_trigger_shows_the_full_name_beyond_the_removed_budget(cx: &mut TestAppContext) {
    let (window, _stream) = open_model_stream(cx, "issue342-old-budget", OLD_BUDGET_MODEL);
    let label = bounds(window, "composer-model-label", cx);
    assert!(
        label.size.width > px(134.),
        "this name must exceed the removed 150px trigger budget's 134px content \
         box, otherwise the case proves nothing: label {:?}",
        label.size.width
    );
    assert_holds_whole_name(window, OLD_BUDGET_MODEL, "ordinary", cx);
}

#[gpui_kit::test]
async fn issue342_trigger_box_holds_the_label_and_the_chevron(cx: &mut TestAppContext) {
    let (window, _stream) = open_model_stream(cx, "issue342-trigger-box", LONG_MODEL);
    let trigger = bounds(window, "composer-model", cx);
    let label = bounds(window, "composer-model-label", cx);
    assert!(
        trigger.size.width > label.size.width,
        "the trigger ({:?}) must be wider than its label box ({:?}) so the \
         chevron and the horizontal padding still fit",
        trigger.size.width,
        label.size.width
    );
    assert!(
        label.left() >= trigger.left() && label.right() <= trigger.right(),
        "the label box ({label:?}) must stay inside the trigger ({trigger:?})"
    );
}

#[gpui_kit::test]
async fn issue342_overlong_name_ellipsises_inside_the_card(cx: &mut TestAppContext) {
    let (window, stream) = open_model_stream(cx, "issue342-overlong", OVERLONG_MODEL);
    set_column_width(
        &stream,
        NARROW_WINDOW_WIDTH - Layout::CONVERSATION_PADDING * 2.0,
        cx,
    );

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(NARROW_WINDOW_WIDTH), px(NARROW_WINDOW_HEIGHT)));
    visual.run_until_parked();

    let card = visual
        .debug_bounds("composer-shell")
        .expect("composer card");
    let trigger = visual
        .debug_bounds("composer-model")
        .expect("model trigger");
    let label = visual
        .debug_bounds("composer-model-label")
        .expect("model label");
    let needed = shaped_width(window, OVERLONG_MODEL, cx);

    assert!(
        label.size.width < needed,
        "the overlong name must really be under pressure: label {:?} vs \
         shaped {needed:?}",
        label.size.width
    );
    assert!(
        trigger.left() >= card.left() && trigger.right() <= card.right(),
        "the trigger ({trigger:?}) must stay inside the card ({card:?})"
    );
    assert!(
        label.left() >= trigger.left() && label.right() <= trigger.right(),
        "the truncated label ({label:?}) must stay inside the trigger ({trigger:?})"
    );
    for selector in ["composer-add", "composer-send"] {
        let control = visual.debug_bounds(selector).expect("footer control");
        assert!(
            control.left() >= card.left() && control.right() <= card.right(),
            "{selector} ({control:?}) must stay inside the card ({card:?})"
        );
    }
}

#[gpui_kit::test]
async fn issue342_label_wording_is_unchanged(_cx: &mut TestAppContext) {
    use crate::conversation_stream::render::{PICKER_EFFORT_LABEL, model_trigger_label};

    assert_eq!(PICKER_EFFORT_LABEL, "选择强度");
    assert_eq!(
        model_trigger_label(LONG_MODEL, ModelPickerLevel::Closed, false),
        LONG_MODEL
    );
    assert_eq!(
        model_trigger_label(LONG_MODEL, ModelPickerLevel::Slider, false),
        PICKER_EFFORT_LABEL
    );
    assert_eq!(
        model_trigger_label(LONG_MODEL, ModelPickerLevel::List, false),
        LONG_MODEL
    );
    assert_eq!(
        model_trigger_label(LONG_MODEL, ModelPickerLevel::Closed, true),
        "保存中…"
    );
    assert_eq!(
        model_trigger_label("", ModelPickerLevel::Closed, false),
        "模型"
    );
}

#[gpui_kit::test]
async fn issue342_width_rule_holds_in_both_themes(cx: &mut TestAppContext) {
    let (window, _stream) = open_model_stream(cx, "issue342-themes", LONG_MODEL);
    for (name, palette) in [
        ("light", vega_theme::Theme::light()),
        ("dark", vega_theme::Theme::dark()),
    ] {
        cx.update(|cx| {
            cx.set_global(palette);
            cx.refresh_windows();
        });
        assert_holds_whole_name(window, LONG_MODEL, name, cx);
    }
}

#[gpui_kit::test]
async fn issue342_picker_layers_keep_their_geometry(cx: &mut TestAppContext) {
    let (window, _stream) = open_tiered_model_stream(cx, "issue342-picker", LONG_MODEL);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let trigger = visual
        .debug_bounds("composer-model")
        .expect("model trigger");
    visual.simulate_click(trigger.center(), Modifiers::default());
    visual.run_until_parked();

    let slider = visual
        .debug_bounds("composer-thinking-slider")
        .expect("slider layer");
    assert_aligned(
        slider.right(),
        trigger.right(),
        "the slider layer still right-aligns with the trigger",
    );

    let title = visual
        .debug_bounds("thinking-slider-title")
        .expect("slider title");
    visual.simulate_click(title.center(), Modifiers::default());
    visual.run_until_parked();

    let menu = visual
        .debug_bounds("composer-model-menu")
        .expect("model list layer");
    assert!(
        (f32::from(menu.size.width) - Layout::MENU_MAX_WIDTH).abs() <= 1.0,
        "the list keeps the frozen menu width: {:?}",
        menu.size.width
    );
    assert_aligned(
        menu.right(),
        trigger.right(),
        "the list still right-aligns with the trigger",
    );
}
