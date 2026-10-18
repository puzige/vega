use super::*;
use gpui_kit::{Bounds, VisualTestContext, size};
use vega_conversation::types::{ReasoningProtocol, ReasoningReplaySetting};

const LONG_MODEL: &str = "atlas-reasoning-pro";
const SHORT_MODEL: &str = "o";
const PLAIN_MODEL: &str = "n";
const EFFORT_LABEL: &str = "选择强度";

type FooterGeometry = [(&'static str, Bounds<Pixels>); 5];

fn picker_thread(model: &str) -> Thread {
    Thread {
        id: "composer-picker-thread".into(),
        model: model.into(),
        ..permission_thread()
    }
}

fn reasoning_profile(model: &str) -> ReasoningProfileProjection {
    ReasoningProfileProjection {
        provider: "picker-provider".into(),
        model: model.into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        support: ReasoningSupport::Optional,
        efforts: vec!["low".into(), "high".into()],
        supports_disabled: false,
        disabled_wire: None,
        replay_profile: ReasoningReplaySetting::Auto,
        preference: ReasoningChoice::ProviderDefault,
    }
}

fn open_picker_stream(
    cx: &mut TestAppContext,
    model: &str,
    width: f32,
    dark: bool,
) -> (WindowHandle<StreamHarness>, Entity<ConversationStream>) {
    cx.update(|cx| {
        cx.set_global(if dark {
            vega_theme::Theme::dark()
        } else {
            vega_theme::Theme::light()
        });
    });
    let stream = cx.new(|cx| ConversationStream::new(picker_thread(model), cx));
    stream.update(cx, |stream, cx| {
        stream.set_workspace_width(width, cx);
        let mut options = vec![model.to_owned()];
        for option in [SHORT_MODEL, PLAIN_MODEL] {
            if option != model {
                options.push(option.to_owned());
            }
        }
        stream.apply_model_options(options, cx);
        stream.apply_reasoning_profile(reasoning_profile(model), cx);
    });
    let root_stream = stream.clone();
    let window = cx.open_window(size(px(width), px(640.)), |_, _| StreamHarness {
        stream: root_stream,
    });
    cx.run_until_parked();
    (window, stream)
}

fn restore_picker_history(stream: &Entity<ConversationStream>, cx: &mut TestAppContext) {
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, "Review the selected model"),
                    hydration_assistant(2, "Ready"),
                ],
                None,
            ),
            cx,
        );
    });
    cx.run_until_parked();
}

fn bounds(visual: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing rendered {selector}"));
    assert!(bounds.size.width > px(0.), "zero-width {selector}");
    assert!(bounds.size.height > px(0.), "zero-height {selector}");
    bounds
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let target = bounds(visual, selector);
    visual.simulate_click(target.center(), Default::default());
    visual.run_until_parked();
}

fn focus_input(visual: &mut VisualTestContext, stream: &Entity<ConversationStream>) {
    visual.update(|window, cx| {
        stream.update(cx, |stream, cx| stream.focus_composer(window, cx));
    });
    visual.run_until_parked();
}

fn text_width(visual: &mut VisualTestContext, text: &str) -> Pixels {
    visual.update(|window, _| {
        window
            .text_system()
            .shape_line(
                text.to_owned().into(),
                px(Typography::SIDEBAR),
                &[window.text_style().to_run(text.len())],
                None,
            )
            .width()
    })
}

fn assert_close(actual: Pixels, expected: Pixels, message: &str) {
    assert!(
        (f32::from(actual) - f32::from(expected)).abs() <= 0.5,
        "{message}: {actual:?} != {expected:?}"
    );
}

fn footer_geometry(visual: &mut VisualTestContext) -> FooterGeometry {
    [
        "composer-model",
        "composer-context-usage-hover-target",
        "composer-permission-status",
        "composer-add",
        "composer-send",
    ]
    .map(|selector| (selector, bounds(visual, selector)))
}

fn assert_same_footer(visual: &mut VisualTestContext, expected: &FooterGeometry) {
    for (selector, before) in expected {
        let after = bounds(visual, selector);
        assert_close(after.left(), before.left(), selector);
        assert_close(after.right(), before.right(), selector);
    }
}

fn assert_inside(outer: Bounds<Pixels>, inner: Bounds<Pixels>, selector: &str) {
    assert!(
        inner.left() >= outer.left() - px(0.5)
            && inner.right() <= outer.right() + px(0.5)
            && inner.top() >= outer.top() - px(0.5)
            && inner.bottom() <= outer.bottom() + px(0.5),
        "{selector} {inner:?} overflows {outer:?}"
    );
}

fn assert_footer_contained(visual: &mut VisualTestContext) {
    let shell = bounds(visual, "composer-shell");
    let model = bounds(visual, "composer-model");
    let label = bounds(visual, "composer-model-label");
    assert_inside(model, label, "model label");
    let footer = footer_geometry(visual);
    for (selector, control) in footer {
        assert_inside(shell, control, selector);
        assert_close(control.center().y, model.center().y, selector);
    }
    let add = bounds(visual, "composer-add");
    let permission = bounds(visual, "composer-permission-status");
    let context = bounds(visual, "composer-context-usage-hover-target");
    let send = bounds(visual, "composer-send");
    assert!(add.right() <= permission.left());
    assert!(permission.right() <= context.left());
    assert!(context.right() <= model.left());
    assert!(model.right() <= send.left());
}

fn assert_label_fits(visual: &mut VisualTestContext, text: &str) {
    let label = bounds(visual, "composer-model-label");
    assert!(
        label.size.width + px(0.5) >= text_width(visual, text),
        "{text} is clipped despite available footer width"
    );
}

fn assert_picker_cycle(
    visual: &mut VisualTestContext,
    stream: &Entity<ConversationStream>,
    model: &str,
    unconstrained: bool,
) {
    focus_input(visual, stream);
    let closed = footer_geometry(visual);
    if unconstrained {
        assert_label_fits(visual, model);
    }
    click(visual, "composer-model");
    assert_eq!(
        stream.read_with(visual, |stream, _| stream.model_picker_level()),
        ModelPickerLevel::Slider
    );
    bounds(visual, "composer-thinking-slider");
    bounds(visual, "thinking-slider-track");
    assert!(visual.debug_bounds("composer-model-menu").is_none());
    assert_same_footer(visual, &closed);
    if unconstrained {
        assert_label_fits(visual, EFFORT_LABEL);
    }
    click(visual, "composer-model");
    assert!(visual.debug_bounds("composer-thinking-slider").is_none());
    assert_same_footer(visual, &closed);
    click(visual, "composer-model");
    assert_same_footer(visual, &closed);
    click(visual, "thinking-slider-title");
    assert_eq!(
        stream.read_with(visual, |stream, _| stream.model_picker_level()),
        ModelPickerLevel::List
    );
    bounds(visual, "composer-model-menu");
    bounds(visual, "composer-model-rows");
    assert!(visual.debug_bounds("composer-thinking-slider").is_none());
    assert_same_footer(visual, &closed);
    visual.dispatch_action(CloseModel);
    visual.run_until_parked();
    assert!(visual.debug_bounds("composer-model-menu").is_none());
    assert_same_footer(visual, &closed);
    click(visual, "composer-model");
    bounds(visual, "composer-thinking-slider");
    assert_same_footer(visual, &closed);
}

fn assert_context_absent(visual: &mut VisualTestContext) {
    for selector in [
        "composer-context-usage",
        "composer-context-usage-hover-target",
        "composer-context-usage-ring",
        "context-usage-tooltip-popover",
        "context-usage-tooltip",
    ] {
        assert!(
            visual.debug_bounds(selector).is_none(),
            "empty conversation renders {selector}"
        );
    }
}

fn assert_empty_keyboard_path(visual: &mut VisualTestContext, stream: &Entity<ConversationStream>) {
    focus_input(visual, stream);
    visual.simulate_keystrokes("tab tab");
    assert_context_absent(visual);
    visual.simulate_keystrokes("enter");
    assert_eq!(
        stream.read_with(visual, |stream, _| stream.model_picker_level()),
        ModelPickerLevel::Slider,
        "empty Composer keyboard navigation must skip context usage"
    );
    bounds(visual, "composer-thinking-slider");
    assert_context_absent(visual);
}

#[gpui_kit::test]
fn long_model_picker_preserves_footer_geometry_in_both_layouts_and_themes(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let mut light_geometry = Vec::new();
    for dark in [false, true] {
        for (index, width) in [800., 480.].into_iter().enumerate() {
            let (window, stream) = open_picker_stream(cx, LONG_MODEL, width, dark);
            restore_picker_history(&stream, cx);
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            if dark {
                assert_same_footer(&mut visual, &light_geometry[index]);
            } else {
                light_geometry.push(footer_geometry(&mut visual));
            }
            assert!(text_width(&mut visual, LONG_MODEL) > text_width(&mut visual, EFFORT_LABEL));
            assert_picker_cycle(&mut visual, &stream, LONG_MODEL, true);
        }
    }
}

#[gpui_kit::test]
fn short_model_picker_reserves_room_for_effort_in_both_layouts_and_themes(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let mut light_geometry = Vec::new();
    for dark in [false, true] {
        for (index, width) in [800., 480.].into_iter().enumerate() {
            let (window, stream) = open_picker_stream(cx, SHORT_MODEL, width, dark);
            restore_picker_history(&stream, cx);
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            if dark {
                assert_same_footer(&mut visual, &light_geometry[index]);
            } else {
                light_geometry.push(footer_geometry(&mut visual));
            }
            assert!(text_width(&mut visual, SHORT_MODEL) < text_width(&mut visual, EFFORT_LABEL));
            assert_picker_cycle(&mut visual, &stream, SHORT_MODEL, true);
        }
    }
}

#[gpui_kit::test]
fn narrow_footer_constrains_exceptionally_long_model_without_overflow(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let model = "exceptionally-long-reasoning-model-name-".repeat(12);
    let (window, stream) = open_picker_stream(cx, &model, 800., false);
    restore_picker_history(&stream, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let wide = bounds(&mut visual, "composer-model");
    visual.simulate_resize(size(px(360.), px(640.)));
    stream.update(&mut visual, |stream, cx| {
        stream.set_workspace_width(360., cx)
    });
    visual.run_until_parked();
    assert!(bounds(&mut visual, "composer-model").size.width < wide.size.width);
    assert!(
        bounds(&mut visual, "composer-model-label").size.width < text_width(&mut visual, &model)
    );
    assert_footer_contained(&mut visual);
    assert_picker_cycle(&mut visual, &stream, &model, false);
    assert_footer_contained(&mut visual);
}

#[gpui_kit::test]
fn selecting_models_recalculates_width_and_tierless_model_opens_list(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, stream) = open_picker_stream(cx, LONG_MODEL, 800., false);
    restore_picker_history(&stream, cx);
    let selections = Arc::new(Mutex::new(Vec::new()));
    let captured = selections.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(
            &stream,
            move |_, event: &ThreadModelSelectionRequested, _| {
                captured.lock().unwrap().push(event.clone());
            },
        )
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    focus_input(&mut visual, &stream);
    let long = bounds(&mut visual, "composer-model");
    let mut previous = long;
    for (model, has_tiers) in [(SHORT_MODEL, true), (PLAIN_MODEL, false)] {
        click(&mut visual, "composer-model");
        click(&mut visual, "thinking-slider-title");
        visual.dispatch_action(NextModel);
        visual.dispatch_action(ActivateModel);
        visual.run_until_parked();
        let request = selections.lock().unwrap().last().cloned().unwrap();
        assert_eq!(request.model, model);
        stream.update(&mut visual, |stream, cx| {
            stream.apply_thread_model_acknowledged(
                &request.thread_id,
                request.request_id,
                &request.model,
                picker_thread(model),
                cx,
            );
            if has_tiers {
                stream.apply_reasoning_profile(reasoning_profile(model), cx);
            } else {
                stream.clear_reasoning_profile(cx);
            }
            assert_eq!(stream.displayed_model(), model);
        });
        visual.run_until_parked();
        let selected = bounds(&mut visual, "composer-model");
        assert!(selected.size.width < previous.size.width);
        assert_close(
            selected.right(),
            long.right(),
            "selected model trailing edge",
        );
        previous = selected;
        if has_tiers {
            assert_picker_cycle(&mut visual, &stream, model, true);
            visual.dispatch_action(CloseModel);
            visual.run_until_parked();
        } else {
            click(&mut visual, "composer-model");
            assert_eq!(
                stream.read_with(&visual, |stream, _| stream.model_picker_level()),
                ModelPickerLevel::List
            );
            bounds(&mut visual, "composer-model-menu");
            bounds(&mut visual, "composer-model-rows");
            assert!(visual.debug_bounds("composer-thinking-slider").is_none());
        }
    }
}

#[gpui_kit::test]
fn empty_composer_removes_context_targets_and_reclaims_their_footer_space(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let model = "long-model-with-no-spare-footer-space-".repeat(12);
    let (window, stream) = open_picker_stream(cx, &model, 360., false);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert_context_absent(&mut visual);
    let empty_model = bounds(&mut visual, "composer-model");
    assert_empty_keyboard_path(&mut visual, &stream);
    visual.dispatch_action(CloseModel);
    visual.run_until_parked();
    restore_picker_history(&stream, cx);
    let context = bounds(&mut visual, "composer-context-usage-hover-target");
    let populated_model = bounds(&mut visual, "composer-model");
    assert_close(empty_model.left(), context.left(), "reclaimed context slot");
    assert_close(
        empty_model.right(),
        populated_model.right(),
        "model trailing edge",
    );
    assert!(empty_model.size.width > populated_model.size.width);
}

#[gpui_kit::test]
fn draft_and_context_estimate_do_not_create_context_usage_controls(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let model = "unsent-draft-model-with-no-spare-footer-space-".repeat(12);
    let (window, stream) = open_picker_stream(cx, &model, 360., true);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let before = bounds(&mut visual, "composer-model");
    stream.update(cx, |stream, cx| {
        stream
            .composer_input()
            .update(cx, |input, cx| input.set_text("An unsent draft", cx));
        assert!(stream.apply_context_projection(
            "composer-picker-thread",
            &model,
            None,
            ContextUsageProjection::new(Some(32_768), Some(4_096)),
            false,
            cx,
        ));
        assert_eq!(stream.hydrated_entry_count(), 0);
    });
    cx.run_until_parked();
    assert_context_absent(&mut visual);
    let after = bounds(&mut visual, "composer-model");
    assert_close(after.left(), before.left(), "draft model leading edge");
    assert_close(after.right(), before.right(), "draft model trailing edge");
    visual.simulate_mouse_move(after.center(), None, Default::default());
    visual.run_until_parked();
    assert_context_absent(&mut visual);
    assert_empty_keyboard_path(&mut visual, &stream);
    assert_eq!(
        stream.read_with(&visual, |stream, cx| stream
            .composer_input()
            .read(cx)
            .text()
            .to_owned()),
        "An unsent draft"
    );
}

#[gpui_kit::test]
fn accepted_first_user_message_enables_unknown_usage_then_projected_usage(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, stream) = open_picker_stream(cx, LONG_MODEL, 800., false);
    let submissions = Arc::new(Mutex::new(Vec::new()));
    let captured = submissions.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &ComposerSubmitted, _| {
            captured.lock().unwrap().push(event.clone());
        })
    });
    stream.update(cx, |stream, cx| {
        stream
            .composer_input()
            .update(cx, |input, cx| input.set_text("First accepted message", cx));
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "composer-send");
    assert_context_absent(&mut visual);
    let submitted = submissions.lock().unwrap().last().cloned().unwrap();
    assert_eq!(submitted.content, "First accepted message");
    stream.update(&mut visual, |stream, cx| {
        assert!(stream.composer_submission_pending());
        stream.accept_composer_submission(&submitted.content, cx);
        assert!(!stream.composer_submission_pending());
        assert_eq!(stream.hydrated_entry_count(), 1);
    });
    visual.run_until_parked();
    bounds(&mut visual, "composer-context-usage-ring");
    click(&mut visual, "composer-context-usage");
    bounds(&mut visual, "context-usage-tooltip");
    bounds(&mut visual, "context-usage-tooltip-compact");
    bounds(&mut visual, "context-usage-tooltip-capacity");
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-percentage")
            .is_none()
    );
    assert!(visual.debug_bounds("context-usage-bar").is_none());
    visual.simulate_mouse_move(gpui_kit::point(px(1.), px(1.)), None, Default::default());
    visual.simulate_keystrokes("escape");
    assert!(visual.debug_bounds("context-usage-tooltip").is_none());
    visual.simulate_keystrokes("enter");
    bounds(&mut visual, "context-usage-tooltip");
    stream.update(&mut visual, |stream, cx| {
        assert!(stream.apply_context_projection(
            "composer-picker-thread",
            LONG_MODEL,
            None,
            ContextUsageProjection::new(Some(32_768), Some(4_096)),
            false,
            cx,
        ));
    });
    visual.run_until_parked();
    bounds(&mut visual, "context-usage-tooltip-percentage");
    bounds(&mut visual, "context-usage-bar");
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-capacity")
            .is_none()
    );
}

#[gpui_kit::test]
fn restored_history_shows_usage_and_switching_to_empty_drops_open_tooltip(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let (window, populated) = open_picker_stream(cx, LONG_MODEL, 480., true);
    restore_picker_history(&populated, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    bounds(&mut visual, "composer-context-usage-ring");
    click(&mut visual, "composer-context-usage");
    bounds(&mut visual, "context-usage-tooltip-popover");
    bounds(&mut visual, "context-usage-tooltip");
    let stale_target = bounds(&mut visual, "composer-context-usage-hover-target");
    visual.simulate_mouse_move(stale_target.center(), None, Default::default());
    visual.run_until_parked();
    let empty = cx.new(|cx| {
        ConversationStream::new(
            Thread {
                id: "fresh-composer-thread".into(),
                ..picker_thread(LONG_MODEL)
            },
            cx,
        )
    });
    empty.update(cx, |stream, cx| {
        stream.set_workspace_width(480., cx);
        stream.apply_model_options(vec![LONG_MODEL.into()], cx);
        stream.apply_reasoning_profile(reasoning_profile(LONG_MODEL), cx);
        assert!(stream.apply_context_projection(
            "fresh-composer-thread",
            LONG_MODEL,
            None,
            ContextUsageProjection::new(Some(32_768), Some(4_096)),
            false,
            cx,
        ));
    });
    for target in [empty.clone(), populated, empty.clone()] {
        let is_empty = target == empty;
        window
            .update(cx, |harness, _, cx| {
                harness.stream = target;
                cx.notify();
            })
            .expect("switch rendered conversation");
        visual.run_until_parked();
        if is_empty {
            assert_context_absent(&mut visual);
            visual.simulate_mouse_move(stale_target.center(), None, Default::default());
            visual.simulate_click(stale_target.center(), Default::default());
            visual.run_until_parked();
            assert_context_absent(&mut visual);
        } else {
            bounds(&mut visual, "composer-context-usage-ring");
            if visual.debug_bounds("context-usage-tooltip").is_none() {
                click(&mut visual, "composer-context-usage");
            }
            bounds(&mut visual, "context-usage-tooltip");
        }
    }
    assert_empty_keyboard_path(&mut visual, &empty);
}
