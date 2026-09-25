use super::*;
use gpui_kit::{Modifiers, VisualTestContext, point, px};
use vega_conversation::types::ContextSettings;
use vega_theme::{Appearance, Theme};

fn settings(thread_id: &str, model: &str, context_limit: Option<u64>) -> ContextSettings {
    ContextSettings {
        thread_id: thread_id.to_string(),
        model: model.to_string(),
        context_limit,
        output_reserve: 8_000,
        automatic_compaction: false,
        updated_at: 1,
    }
}

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx).debug_bounds(selector)
}

fn assert_narrow_tooltip_fits_and_clears_controls(
    visual: &mut VisualTestContext,
    viewport: gpui_kit::Size<gpui_kit::Pixels>,
) {
    let tooltip = visual
        .debug_bounds("context-usage-tooltip")
        .expect("context usage tooltip");
    assert!(
        f32::from(tooltip.left()) >= -1.0
            && f32::from(tooltip.right()) <= f32::from(viewport.width) + 1.0,
        "tooltip must stay in the narrow viewport: tooltip={tooltip:?} viewport={viewport:?}"
    );
    for selector in [
        "composer-model",
        "composer-send",
        "composer-footer-branch-chip",
    ] {
        let control = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing Composer control {selector}"));
        assert!(
            !tooltip.intersects(&control),
            "tooltip must not cover {selector}: tooltip={tooltip:?} control={control:?}"
        );
    }
}

#[gpui_kit::test]
async fn issue64_context_usage_known_capacity_shows_hover_tip_without_changing_composer_height(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue64-known");
    let original_height = bounds(window, "composer-shell", cx).unwrap().size.height;
    assert!(bounds(window, "composer-context-usage", cx).is_none());

    stream.update(cx, |stream, cx| {
        assert!(stream.apply_context_projection(
            "issue64-known",
            "mock",
            Some(settings("issue64-known", "mock", Some(258_000))),
            Some(135_000),
            true,
            cx,
        ));
    });

    let indicator = bounds(window, "composer-context-usage", cx).unwrap();
    assert_eq!(indicator.size.width, px(24.0));
    assert_eq!(
        bounds(window, "composer-shell", cx).unwrap().size.height,
        original_height
    );
    assert!(bounds(window, "context-usage-tooltip", cx).is_none());

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_mouse_move(indicator.center(), None, Modifiers::default());
    visual.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(2));
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip-title").is_some());
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-percentage")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-compact")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-estimate")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-capacity")
            .is_some()
    );
    let tooltip = visual.debug_bounds("context-usage-tooltip").unwrap();
    visual.simulate_mouse_move(tooltip.center(), None, Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip-title").is_some());
}

#[gpui_kit::test]
async fn issue64_context_usage_unknown_capacity_shows_no_percentage_and_unknown_estimate_hides(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue64-unknown");
    stream.update(cx, |stream, cx| {
        assert!(stream.apply_context_projection(
            "issue64-unknown",
            "mock",
            Some(settings("issue64-unknown", "mock", None)),
            None,
            true,
            cx,
        ));
    });
    assert!(bounds(window, "composer-context-usage", cx).is_none());

    stream.update(cx, |stream, cx| {
        assert!(stream.apply_context_projection(
            "issue64-unknown",
            "mock",
            Some(settings("issue64-unknown", "mock", None)),
            Some(135_000),
            true,
            cx,
        ));
    });
    let indicator = bounds(window, "composer-context-usage", cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_mouse_move(indicator.center(), None, Modifiers::default());
    visual.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(2));
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip-title").is_some());
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-compact")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-estimate")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-capacity")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-percentage")
            .is_none()
    );
}

#[gpui_kit::test]
async fn issue64_context_usage_focus_keeps_the_same_tip_after_pointer_leaves(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue64-focus");
    stream.update(cx, |stream, cx| {
        assert!(stream.apply_context_projection(
            "issue64-focus",
            "mock",
            Some(settings("issue64-focus", "mock", Some(100_000))),
            Some(50_000),
            true,
            cx,
        ));
    });
    window
        .update(cx, |_, window, cx| {
            let model_focus = stream.read_with(cx, |stream, _| stream.model_focus.clone());
            window.focus(&model_focus, cx);
            window.focus_prev(cx);
            let usage_focus = stream.read_with(cx, |stream, _| stream.context_usage_focus.clone());
            assert!(usage_focus.contains_focused(window, cx));
        })
        .expect("context usage window");
    cx.run_until_parked();

    let indicator = bounds(window, "composer-context-usage", cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_mouse_move(point(px(1.0), px(1.0)), None, Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip-title").is_some());
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-percentage")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("context-usage-tooltip-capacity")
            .is_some()
    );
    assert!(bounds(window, "composer-context-usage", cx).is_some());
    assert!(indicator.size.height >= px(16.0));
}

#[gpui_kit::test]
async fn issue64_context_usage_hover_and_focus_tips_fit_narrow_window_without_covering_composer_controls(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue64-narrow-window");
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(gpui_kit::size(px(360.0), px(600.0)));
    visual.run_until_parked();

    stream.update(cx, |stream, cx| {
        stream.composer_submit_pending = true;
        stream.accept_composer_submission("existing project conversation", cx);
        assert!(stream.apply_context_projection(
            "issue64-narrow-window",
            "mock",
            Some(settings("issue64-narrow-window", "mock", Some(100_000))),
            Some(50_000),
            true,
            cx,
        ));
    });
    let viewport = window
        .update(cx, |_, window, _| window.viewport_size())
        .expect("narrow viewport size");
    assert_eq!(viewport.width, px(360.0));

    let indicator = visual
        .debug_bounds("composer-context-usage")
        .expect("context usage indicator");
    visual.simulate_mouse_move(indicator.center(), None, Modifiers::default());
    visual.run_until_parked();
    assert_narrow_tooltip_fits_and_clears_controls(&mut visual, viewport);

    let tooltip = visual
        .debug_bounds("context-usage-tooltip")
        .expect("hover context usage tooltip");
    let hover_target = visual
        .debug_bounds("composer-context-usage-hover-target")
        .expect("context usage hover target");
    assert!(
        (f32::from(tooltip.bottom()) - f32::from(hover_target.top())).abs() <= 1.0,
        "popover and trigger must meet without a hover gap: tooltip={tooltip:?} trigger={hover_target:?}"
    );
    let indicator_x = indicator.center().x;
    let mut y = f32::from(indicator.center().y);
    let tooltip_center_y = f32::from(tooltip.center().y);
    while y > tooltip_center_y {
        visual.simulate_mouse_move(point(indicator_x, px(y)), None, Modifiers::default());
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("context-usage-tooltip").is_some(),
            "popover must remain mounted while the pointer crosses from trigger into tooltip at y={y}"
        );
        y = (y - 2.0).max(tooltip_center_y);
    }
    assert_narrow_tooltip_fits_and_clears_controls(&mut visual, viewport);

    visual.simulate_mouse_move(point(px(1.0), px(1.0)), None, Modifiers::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("context-usage-tooltip").is_none());

    window
        .update(cx, |_, window, cx| {
            let focus = stream.read_with(cx, |stream, _| stream.context_usage_focus.clone());
            window.focus(&focus, cx);
        })
        .expect("context usage window focus");
    cx.run_until_parked();
    assert_narrow_tooltip_fits_and_clears_controls(&mut visual, viewport);
}

#[gpui_kit::test]
async fn issue64_context_usage_model_change_clears_previous_projection(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue64-model-switch");
    stream.update(cx, |stream, cx| {
        assert!(stream.apply_context_projection(
            "issue64-model-switch",
            "mock",
            Some(settings("issue64-model-switch", "mock", Some(100_000))),
            Some(80_000),
            true,
            cx,
        ));
    });
    assert!(bounds(window, "composer-context-usage", cx).is_some());

    let mut changed = stream.read_with(cx, |stream, _| stream.thread.clone());
    changed.model = "other-model".to_string();
    stream.update(cx, |stream, cx| stream.apply_thread(changed, cx));
    assert!(bounds(window, "composer-context-usage", cx).is_none());

    stream.update(cx, |stream, cx| {
        assert!(!stream.apply_context_projection(
            "issue64-model-switch",
            "mock",
            Some(settings("issue64-model-switch", "mock", Some(100_000))),
            Some(99_000),
            true,
            cx,
        ));
        assert!(stream.apply_context_projection(
            "issue64-model-switch",
            "other-model",
            Some(settings(
                "issue64-model-switch",
                "other-model",
                Some(100_000)
            )),
            Some(20_000),
            true,
            cx,
        ));
    });
    assert!(bounds(window, "composer-context-usage", cx).is_some());
}

#[gpui_kit::test]
async fn issue64_context_usage_uses_light_and_dark_theme_tokens_and_stays_read_only(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue64-theme");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    cx.update(|cx| {
        cx.subscribe(
            &stream,
            move |_, request: &ThreadModelSelectionRequested, _| {
                captured
                    .lock()
                    .expect("model selection requests")
                    .push(request.model.clone());
            },
        )
        .detach();
    });

    for appearance in [Appearance::Light, Appearance::Dark] {
        cx.update(|cx| {
            cx.set_global(match appearance {
                Appearance::Light => Theme::light(),
                Appearance::Dark => Theme::dark(),
            });
        });
        stream.update(cx, |stream, cx| {
            assert!(stream.apply_context_projection(
                "issue64-theme",
                "mock",
                Some(settings("issue64-theme", "mock", Some(100_000))),
                Some(20_000),
                true,
                cx,
            ));
        });
        assert!(bounds(window, "composer-context-usage-ring", cx).is_some());
        assert!(bounds(window, "composer-context-usage", cx).is_some());
        assert_eq!(cx.update(|cx| theme(cx).appearance), appearance);

        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let indicator = visual.debug_bounds("composer-context-usage").unwrap();
        visual.simulate_click(indicator.center(), Modifiers::default());
        visual.run_until_parked();
    }

    assert_eq!(
        stream.read_with(cx, |stream, _| stream.thread.model.clone()),
        "mock"
    );
    assert!(
        requests
            .lock()
            .expect("model selection requests")
            .is_empty()
    );
}
