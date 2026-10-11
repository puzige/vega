use super::*;

fn surface_quad(
    window: WindowHandle<SettingsHarness>,
    target: Bounds<gpui_kit::Pixels>,
    cx: &mut TestAppContext,
) -> gpui_kit::Quad {
    window
        .update(cx, |_, window, _| {
            let scale = window.scale_factor();
            window.painted_quads().into_iter().find(|quad| {
                (quad.bounds.left().as_f32() / scale - f32::from(target.left())).abs() < 0.2
                    && (quad.bounds.top().as_f32() / scale - f32::from(target.top())).abs() < 0.2
                    && (quad.bounds.size.width.as_f32() / scale - f32::from(target.size.width))
                        .abs()
                        < 0.2
                    && (quad.bounds.size.height.as_f32() / scale - f32::from(target.size.height))
                        .abs()
                        < 0.2
            })
        })
        .expect("painted settings window")
        .expect("painted control surface")
}

#[gpui_kit::test]
async fn issue296_h3a_agents_argv_has_input_surface(cx: &mut TestAppContext) {
    for dark in [false, true] {
        let (view, window) = mount_settings(cx, dark, 960., 600.);
        let original = view.read_with(cx, |view, _| view.codex_args_input.clone());
        view.update(cx, |view, cx| {
            view.section = 7;
            cx.notify();
        });
        cx.run_until_parked();
        let bounds = VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-agent-args-input")
            .unwrap();
        let colors = cx.update(|cx| theme(cx).colors);
        window
            .update(cx, |_, window, _| {
                let quads = window.painted_quads();
                let matching = quads
                    .iter()
                    .filter(|quad| quad.bounds == bounds.scale(window.scale_factor()))
                    .collect::<Vec<_>>();
                assert!(
                    matching
                        .iter()
                        .any(|quad| quad.border_color == colors.border_subtle.into())
                );
                assert!(
                    matching
                        .iter()
                        .any(|quad| quad.background.as_solid() == Some(colors.bg_elevated.into()))
                );
            })
            .unwrap();
        assert_eq!(
            view.read_with(cx, |view, _| view.codex_args_input.clone()),
            original
        );
    }
}

#[gpui_kit::test]
async fn issue296_h3a_pricing_edit_has_complete_model_context(cx: &mut TestAppContext) {
    let (view, window) = mount_settings(cx, false, 960., 600.);
    let model = format!("vendor/{}", "long-model".repeat(25));
    view.update(cx, |view, cx| {
        view.section = 3;
        view.pricing_editor = Some(PricingEditor {
            kind: PricingEditorKind::UpdateCustom,
            model: Some(model.clone()),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let title = visual
        .debug_bounds("pricing-editor-model")
        .expect("editing model title");
    let field = visual.debug_bounds("pricing-rate-0").unwrap();
    assert!(title.size.height > px(Typography::BODY * Typography::BODY_LINE_HEIGHT));
    assert!(field.top() - title.bottom() >= px(12.));
    assert!(
        title.right()
            <= visual
                .debug_bounds("settings-content-column")
                .unwrap()
                .right()
    );
    view.update(cx, |view, cx| {
        view.pricing_editor = Some(PricingEditor {
            kind: PricingEditorKind::AddCustom,
            model: None,
        });
        cx.notify();
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("pricing-model").is_some());
    assert!(visual.debug_bounds("pricing-editor-model").is_none());
}

fn mount_settings(
    cx: &mut TestAppContext,
    dark: bool,
    width: f32,
    height: f32,
) -> (Entity<SettingsView>, WindowHandle<SettingsHarness>) {
    cx.update(|cx| {
        cx.set_global(if dark {
            vega_theme::Theme::dark()
        } else {
            vega_theme::Theme::light()
        });
        cx.set_global(SettingsOpen(true));
        cx.set_global(crate::sidebar::SidebarWidth(Layout::SIDEBAR_MAX_WIDTH));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    let root = view.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(width), px(height)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| SettingsHarness {
                        view: root,
                        closes: Arc::new(AtomicUsize::new(0)),
                    })
                },
            )
        })
        .expect("owned settings fixture");
    cx.run_until_parked();
    (view, window)
}

#[gpui_kit::test]
async fn issue296_h3a_routes_keep_independent_scroll_positions(cx: &mut TestAppContext) {
    fn navigate(
        window: WindowHandle<SettingsHarness>,
        selector: &'static str,
        cx: &mut TestAppContext,
    ) {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        if selector == "settings-nav-reasoning" {
            if visual.debug_bounds("model-settings-reasoning").is_none() {
                let providers = visual.debug_bounds("settings-nav-providers").unwrap();
                visual.simulate_click(providers.center(), Default::default());
                visual.run_until_parked();
                let configure = visual.debug_bounds("model-edit-0").unwrap();
                visual.simulate_click(configure.center(), Default::default());
                visual.run_until_parked();
            }
            let target = visual.debug_bounds("model-settings-reasoning").unwrap();
            visual.simulate_click(target.center(), Default::default());
        } else {
            let target = visual.debug_bounds(selector).unwrap();
            visual.simulate_click(target.center(), Default::default());
        }
        visual.run_until_parked();
    }
    fn scroll(window: WindowHandle<SettingsHarness>, dy: f32, cx: &mut TestAppContext) {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let body = visual.debug_bounds("settings-section-content").unwrap();
        visual.simulate_mouse_move(body.center(), None, Default::default());
        visual.simulate_event(gpui_kit::ScrollWheelEvent {
            position: body.center(),
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(dy))),
            modifiers: Default::default(),
            touch_phase: gpui_kit::TouchPhase::Moved,
        });
        visual.run_until_parked();
    }
    let mut windows = Vec::new();
    for _ in 0..2 {
        let (view, window) = mount_settings(cx, false, 960., 600.);
        view.update(cx, |view, cx| {
            view.config.providers = vec![provider("owned", &["model-0", "model-1", "model-2"])];
            view.apply_reasoning_projection(
                ReasoningSettingsProjection::Ready {
                    generation: 1,
                    profiles: (0..3)
                        .map(|index| explicit_openai_profile("owned", &format!("model-{index}")))
                        .collect(),
                    error: None,
                },
                cx,
            )
        });
        cx.run_until_parked();
        windows.push(window);
    }
    navigate(windows[0], "settings-nav-reasoning", cx);
    let top = VisualTestContext::from_window(windows[0].into(), cx)
        .debug_bounds("reasoning-protocol-0")
        .unwrap()
        .top();
    scroll(windows[1], -200., cx);
    let general = VisualTestContext::from_window(windows[1].into(), cx)
        .debug_bounds("settings-general-appearance")
        .unwrap()
        .top();
    assert!(general < px(0.));
    navigate(windows[1], "settings-nav-reasoning", cx);
    assert_eq!(
        VisualTestContext::from_window(windows[1].into(), cx)
            .debug_bounds("reasoning-protocol-0")
            .unwrap()
            .top(),
        top
    );
    scroll(windows[1], -80., cx);
    let reasoning = VisualTestContext::from_window(windows[1].into(), cx)
        .debug_bounds("reasoning-protocol-0")
        .unwrap()
        .top();
    assert!(reasoning < top);
    navigate(windows[1], "settings-nav-general", cx);
    assert_eq!(
        VisualTestContext::from_window(windows[1].into(), cx)
            .debug_bounds("settings-general-appearance")
            .unwrap()
            .top(),
        general
    );
    navigate(windows[1], "settings-nav-reasoning", cx);
    assert_eq!(
        VisualTestContext::from_window(windows[1].into(), cx)
            .debug_bounds("reasoning-protocol-0")
            .unwrap()
            .top(),
        reasoning
    );
}

#[gpui_kit::test]
async fn issue296_settings_routes_keep_one_readable_column(cx: &mut TestAppContext) {
    for dark in [false, true] {
        for (width, height) in [(1403., 860.), (1200., 760.), (960., 600.)] {
            let (view, window) = mount_settings(cx, dark, width, height);
            for section in 0..8 {
                view.update(cx, |view, cx| {
                    view.section = section;
                    cx.notify();
                });
                cx.run_until_parked();
                let mut visual = VisualTestContext::from_window(window.into(), cx);
                let column = visual
                    .debug_bounds("settings-content-column")
                    .expect("column");
                let body = visual
                    .debug_bounds("settings-section-content")
                    .expect("body");
                let navigation = visual
                    .debug_bounds("settings-navigation")
                    .expect("navigation");
                assert_pixel_close(navigation.size.width, 224., "independent navigation width");
                assert!(column.size.width <= px(720.));
                assert_pixel_close(column.top(), 24., "compact page top");
                assert!(column.size.width <= px(Layout::SETTINGS_CONTENT_MAX_WIDTH));
                assert!(column.right() <= px(width));
                assert!(body.size.height > px(0.));
                assert_pixel_close(body.left(), f32::from(column.left()), "shared leading edge");
                for nav in [
                    "settings-nav-general",
                    "settings-nav-providers",
                    "settings-nav-usage",
                    "settings-nav-mcp",
                    "settings-nav-agents",
                    "settings-nav-skills",
                ] {
                    let bounds = visual.debug_bounds(nav).expect("navigation");
                    assert_pixel_close(bounds.size.height, 32., "navigation height");
                }
            }
        }
    }
}

#[gpui_kit::test]
async fn issue296_skills_actions_are_compact_and_agents_fields_stay_labeled(
    cx: &mut TestAppContext,
) {
    for dark in [false, true] {
        let (view, window) = mount_settings(cx, dark, 960., 600.);
        view.update(cx, |view, cx| {
            view.section = 6;
            cx.notify();
        });
        cx.run_until_parked();
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            let reload = visual.debug_bounds("skills-reload").expect("refresh");
            let content = visual
                .debug_bounds("settings-section-content")
                .expect("content");
            assert!(
                reload.size.height >= px(28.),
                "actions keep desktop hit height"
            );
            assert!(
                reload.size.width < content.size.width / 2.,
                "refresh must hug its label"
            );
        }
        view.update(cx, |view, cx| {
            view.section = 7;
            cx.notify();
        });
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let mut previous_bottom = px(0.);
        for (label_id, input_id) in [
            ("settings-agent-name-label", "settings-agent-name-input"),
            (
                "settings-agent-executable-label",
                "settings-agent-executable-input",
            ),
            ("settings-agent-args-label", "settings-agent-args-input"),
        ] {
            let label = visual
                .debug_bounds(label_id)
                .expect("persistent field label");
            let input = visual.debug_bounds(input_id).expect("original field");
            assert!(label.top() >= previous_bottom);
            assert_pixel_close(input.left() - label.right(), 16., "neighboring input gap");
            assert_pixel_close(label.size.width, 208., "shared field label width");
            assert!(input.top() <= label.bottom());
            assert!(input.size.height > px(0.));
            previous_bottom = input.bottom();
        }
        let save = visual
            .debug_bounds("settings-codex-profile-save")
            .expect("save");
        assert!(save.top() >= previous_bottom);
        assert!(save.size.height >= px(28.));
    }
}

#[gpui_kit::test]
async fn issue296_provider_detail_and_editor_keep_narrow_scroll_ownership(cx: &mut TestAppContext) {
    for dark in [false, true] {
        let (view, window) = mount_settings(cx, dark, 960., 600.);
        view.update(cx, |view, cx| {
            let models = (0..8)
                .map(|index| format!("vendor/{index}-{}", "long-model-".repeat(15)))
                .collect::<Vec<_>>();
            view.config.providers = vec![provider(
                "Owned",
                &models.iter().map(String::as_str).collect::<Vec<_>>(),
            )];
            view.section = 0;
            cx.notify();
        });
        cx.run_until_parked();
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            let viewport = visual
                .debug_bounds("provider-detail-viewport")
                .expect("detail viewport");
            let flow = visual
                .debug_bounds("provider-detail-flow")
                .expect("detail flow");
            let url = visual
                .debug_bounds("provider-base-url")
                .expect("provider URL");
            assert!(viewport.size.width > px(0.));
            assert!(flow.size.height >= viewport.size.height);
            assert!(url.right() <= viewport.right());
            assert!(url.size.height >= px(Typography::BODY));
            let model = visual
                .debug_bounds("provider-model-name-0")
                .expect("complete model ID");
            assert!(model.size.height > px(Typography::BODY * Typography::BODY_LINE_HEIGHT));
            assert!(model.right() <= viewport.right());
            for id in ["provider-edit", "provider-discover", "provider-add-model"] {
                let action = visual.debug_bounds(id).expect("provider action");
                assert!(action.left() >= viewport.left());
                assert!(action.right() <= viewport.right());
            }
            let edit = visual.debug_bounds("provider-edit").expect("edit");
            visual.simulate_click(edit.center(), Default::default());
        }
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let viewport = visual
            .debug_bounds("provider-detail-viewport")
            .expect("editor viewport");
        let flow = visual
            .debug_bounds("provider-detail-flow")
            .expect("editor flow");
        for id in ["provider-models-input-frame", "provider-models-input-help"] {
            let field = visual.debug_bounds(id).expect("provider editor field");
            assert!(field.size.height > px(0.));
            assert!(field.left() >= viewport.left());
            assert!(field.right() <= viewport.right());
            assert!(field.bottom() <= flow.bottom());
        }
    }
}

#[gpui_kit::test]
async fn issue296_settings_long_status_aligns_icon_to_first_line(cx: &mut TestAppContext) {
    for dark in [false, true] {
        let (view, window) = mount_settings(cx, dark, 960., 600.);
        view.update(cx, |view, cx| {
            view.section = 7;
            view.error = Some(
                "合成错误：配置保存失败，原始输入已保留。请检查设置后重试，不会自动修改当前任务。"
                    .repeat(4),
            );
            cx.notify();
        });
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let status = visual.debug_bounds("settings-status").expect("status");
        let icon_line = visual
            .debug_bounds("settings-status-icon-line")
            .expect("icon line");
        let icon = visual.debug_bounds("settings-status-icon").expect("icon");
        let text = visual
            .debug_bounds("settings-status-text")
            .expect("status text");
        assert!(text.size.height > px(Typography::BODY * Typography::BODY_LINE_HEIGHT));
        assert_pixel_close(icon_line.top(), f32::from(text.top()), "first line origin");
        assert_pixel_close(
            icon.center().y,
            f32::from(text.top()) + Typography::BODY * Typography::BODY_LINE_HEIGHT / 2.,
            "first line centered icon",
        );
        assert_pixel_close(icon.size.width, 16., "icon slot");
        assert!(text.bottom() < status.bottom());
        assert!(text.right() < status.right());
        let colors = cx.update(|cx| theme(cx).colors);
        assert_eq!(
            surface_quad(window, status, cx).border_color,
            gpui_kit::Hsla::from(colors.border_subtle)
        );
    }
}

#[gpui_kit::test]
async fn issue296_settings_hover_and_focus_preserve_control_geometry(cx: &mut TestAppContext) {
    for dark in [false, true] {
        let (view, window) = mount_settings(cx, dark, 960., 600.);
        cx.update(|cx| cx.set_reduce_motion(true));
        view.update(cx, |view, cx| {
            view.section = 6;
            cx.notify();
        });
        cx.run_until_parked();
        let before = VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("skills-reload")
            .expect("refresh");
        let colors = cx.update(|cx| theme(cx).colors);
        assert_eq!(
            surface_quad(window, before, cx).background.as_solid(),
            Some(gpui_kit::Hsla::from(colors.bg_elevated))
        );
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            visual.simulate_mouse_move(before.center(), None, Default::default());
        }
        cx.run_until_parked();
        let hovered = VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("skills-reload")
            .expect("hovered refresh");
        assert_eq!(before, hovered);
        assert_eq!(
            surface_quad(window, hovered, cx).background.as_solid(),
            Some(gpui_kit::Hsla::from(colors.bg_hover))
        );
        {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            visual.simulate_mouse_move(gpui_kit::point(px(0.), px(0.)), None, Default::default());
        }
        cx.run_until_parked();
        assert_eq!(
            before,
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("skills-reload")
                .expect("restored refresh")
        );
        assert_eq!(
            surface_quad(window, before, cx).background.as_solid(),
            Some(gpui_kit::Hsla::from(colors.bg_elevated))
        );
        window
            .update(cx, |_, window, cx| {
                let focus = view.read(cx).section_focuses[6].clone();
                window.focus(&focus, cx);
            })
            .expect("focus navigation");
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert_eq!(
            before,
            visual
                .debug_bounds("skills-reload")
                .expect("refresh after focus")
        );
        assert_pixel_close(
            visual
                .debug_bounds("settings-nav-skills")
                .expect("focused navigation")
                .size
                .height,
            32.,
            "focused navigation height",
        );
    }
}

#[gpui_kit::test]
async fn issue296_general_pickers_toggle_once_from_keyboard(cx: &mut TestAppContext) {
    let (view, window) = mount_settings(cx, false, 960., 600.);
    window
        .update(cx, |_, window, cx| {
            let last_navigation = view.read(cx).section_focuses[4].clone();
            window.focus(&last_navigation, cx);
            for _ in 0..5 {
                window.focus_next(cx);
            }
        })
        .expect("focus permission picker through tab order");
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.mode_open));
    assert!(!view.read_with(cx, |view, _| view.model_open));
    cx.simulate_keystrokes(window.into(), "space");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.mode_open));
    window
        .update(cx, |_, window, cx| window.focus_next(cx))
        .expect("focus model picker through tab order");
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.model_open));
    cx.simulate_keystrokes(window.into(), "space");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.model_open));
    assert_eq!(
        view.read_with(cx, |view, _| view.config.defaults.model.clone()),
        AppConfig::default().defaults.model
    );
}

fn reveal_settings_control(
    window: WindowHandle<SettingsHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<gpui_kit::Pixels> {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let control = visual.debug_bounds(selector).expect("Settings control");
    let viewport = visual
        .debug_bounds("settings-section-content")
        .expect("Settings viewport");
    let delta = if control.bottom() > viewport.bottom() {
        viewport.bottom() - control.bottom() - px(8.)
    } else if control.top() < viewport.top() {
        viewport.top() - control.top() + px(8.)
    } else {
        px(0.)
    };
    if delta != px(0.) {
        visual.simulate_event(gpui_kit::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), delta)),
            modifiers: Default::default(),
            touch_phase: gpui_kit::TouchPhase::Moved,
        });
        visual.run_until_parked();
    }
    let control = visual
        .debug_bounds(selector)
        .expect("visible Settings control");
    assert!(control.top() >= viewport.top() && control.bottom() <= viewport.bottom());
    assert!(control.left() >= viewport.left() && control.right() <= viewport.right());
    control
}

#[gpui_kit::test]
async fn issue296_v2_notes_are_cached_markdown_collapsed_and_keyboard_reachable(
    cx: &mut TestAppContext,
) {
    use vega_conversation::types::{UpdatePhase, UpdateProjection};
    for dark in [false, true] {
        let (view, window) = mount_settings(cx, dark, 960., 600.);
        let notes = format!("# 更新说明\n\n{}", "- **改进** 设置表单。\n".repeat(40));
        view.update(cx, |view, cx| {
            view.apply_update_projection(
                UpdateProjection {
                    current_version: "0.1.0".into(),
                    latest_version: Some("0.1.1".into()),
                    phase: UpdatePhase::Ready,
                    notes: notes.clone(),
                    ..Default::default()
                },
                cx,
            )
        });
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.updater_notes_open));
        assert!(view.read_with(cx, |view, _| {
            view.updater_notes
                .as_ref()
                .is_some_and(|model| model.row_count() > 40)
        }));
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("updater-notes-body")
                .is_none()
        );
        assert!(view.read_with(cx, |view, _| {
            view.updater_notes
                .as_ref()
                .unwrap()
                .committed_lines
                .iter()
                .any(|line| matches!(line.kind, crate::conversation_stream::LineKind::Heading(1)))
        }));
        let toggle = reveal_settings_control(window, "updater-notes-toggle", cx);
        VisualTestContext::from_window(window.into(), cx)
            .simulate_click(toggle.center(), Default::default());
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let body = visual
            .debug_bounds("updater-notes-body")
            .expect("expanded parsed notes");
        assert!(body.size.height > px(0.) && body.size.height <= px(240.));
        let markdown = visual
            .debug_bounds("assistant-message")
            .expect("native Markdown renderer");
        assert!(markdown.size.height > body.size.height);
        assert!(
            visual
                .debug_bounds("updater-install")
                .expect("installation action retained")
                .bottom()
                < body.top()
        );
        assert!(
            visual
                .debug_bounds("updater-check")
                .expect("checking action retained")
                .bottom()
                < body.top()
        );
        let notes_viewport = reveal_settings_control(window, "updater-notes-body", cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let before_markdown = visual.debug_bounds("assistant-message").unwrap();
        let before_install = visual.debug_bounds("updater-install").unwrap();
        visual.simulate_mouse_move(notes_viewport.center(), None, Default::default());
        visual.simulate_event(gpui_kit::ScrollWheelEvent {
            position: notes_viewport.center(),
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-120.))),
            modifiers: Default::default(),
            touch_phase: gpui_kit::TouchPhase::Moved,
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds("assistant-message").unwrap().top() < before_markdown.top());
        assert_eq!(
            visual.debug_bounds("updater-install").unwrap(),
            before_install
        );
        let cached_lines = view.read_with(cx, |view, _| {
            view.updater_notes
                .as_ref()
                .unwrap()
                .committed_lines
                .as_ptr() as usize
        });
        view.update(cx, |view, cx| {
            let mut projection = view.updater.clone();
            projection.message = "已准备好安装".into();
            view.apply_update_projection(projection, cx);
        });
        assert!(view.read_with(cx, |view, _| view.updater_notes_open));
        assert_eq!(
            cached_lines,
            view.read_with(cx, |view, _| view
                .updater_notes
                .as_ref()
                .unwrap()
                .committed_lines
                .as_ptr() as usize)
        );
        window
            .update(cx, |_, window, cx| {
                let navigation = view.read(cx).section_focuses[4].clone();
                window.focus(&navigation, cx);
                for _ in 0..12 {
                    window.focus_next(cx);
                }
            })
            .expect("notes toggle keyboard path");
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.updater_notes_open));
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.updater_notes_open));
        view.update(cx, |view, cx| {
            let mut projection = view.updater.clone();
            projection.notes.clear();
            view.apply_update_projection(projection, cx);
        });
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.updater_notes.is_none()));
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("updater-notes-body")
                .is_none()
        );
    }
}

#[gpui_kit::test]
async fn issue296_v2_updater_switch_keeps_geometry_focus_once_and_busy_guard(
    cx: &mut TestAppContext,
) {
    use vega_conversation::types::{UpdatePhase, UpdateProjection, UpdateRequest};
    for dark in [false, true] {
        let (view, window) = mount_settings(cx, dark, 960., 600.);
        cx.update(|cx| cx.set_reduce_motion(true));
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        cx.update(|cx| {
            cx.subscribe(&view, move |_, request: &UpdateRequest, _| {
                if let UpdateRequest::Automatic(value) = request {
                    captured.lock().unwrap().push(*value);
                }
            })
            .detach()
        });
        for value in [false, true] {
            view.update(cx, |view, cx| {
                view.apply_update_projection(
                    UpdateProjection {
                        automatic: value,
                        phase: UpdatePhase::Idle,
                        ..Default::default()
                    },
                    cx,
                )
            });
            cx.run_until_parked();
            let switch = reveal_settings_control(window, "updater-automatic", cx);
            assert_pixel_close(switch.size.width, 32., "switch width");
            assert_pixel_close(switch.size.height, 20., "switch height");
            let label = VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("updater-auto-label")
                .expect("switch label");
            assert_pixel_close(
                switch.center().y,
                f32::from(label.top()) + 4. + Typography::BODY * Typography::BODY_LINE_HEIGHT / 2.,
                "switch and first text line center",
            );
            window
                .update(cx, |_, window, cx| {
                    let navigation = view.read(cx).section_focuses[4].clone();
                    window.focus(&navigation, cx);
                })
                .expect("switch navigation origin");
            let mut focused_switch = None;
            for _ in 0..16 {
                cx.simulate_keystrokes(window.into(), "tab");
                cx.run_until_parked();
                let bounds = VisualTestContext::from_window(window.into(), cx)
                    .debug_bounds("updater-automatic")
                    .unwrap();
                let painted_focus = window
                    .update(cx, |_, window, cx| {
                        window.painted_quads().iter().any(|quad| {
                            quad.bounds == bounds.scale(window.scale_factor())
                                && quad.border_color == theme(cx).colors.text_primary.into()
                        })
                    })
                    .unwrap();
                if painted_focus {
                    focused_switch = Some(bounds);
                    break;
                }
            }
            let focused_switch =
                focused_switch.expect("switch reachable with visible keyboard focus");
            assert_eq!(focused_switch.size, switch.size);
            let switch = focused_switch;
            let count = events.lock().unwrap().len();
            cx.simulate_keystrokes(window.into(), "enter");
            cx.run_until_parked();
            assert_eq!(events.lock().unwrap().len(), count + 1);
            assert_eq!(events.lock().unwrap().last(), Some(&!value));
            cx.simulate_keystrokes(window.into(), "space");
            cx.run_until_parked();
            assert_eq!(events.lock().unwrap().len(), count + 2);
            VisualTestContext::from_window(window.into(), cx)
                .simulate_click(switch.center(), Default::default());
            cx.run_until_parked();
            assert_eq!(events.lock().unwrap().len(), count + 3);
            view.update(cx, |view, cx| {
                let mut projection = view.updater.clone();
                projection.phase = UpdatePhase::Checking;
                view.apply_update_projection(projection, cx);
            });
            cx.run_until_parked();
            VisualTestContext::from_window(window.into(), cx)
                .simulate_click(switch.center(), Default::default());
            cx.simulate_keystrokes(window.into(), "enter space");
            cx.run_until_parked();
            assert_eq!(events.lock().unwrap().len(), count + 3);
            assert_eq!(
                VisualTestContext::from_window(window.into(), cx)
                    .debug_bounds("updater-automatic")
                    .unwrap(),
                switch
            );
        }
    }
}

#[gpui_kit::test]
async fn issue296_v2_populated_pricing_reasoning_and_usage_keep_complete_fields(
    cx: &mut TestAppContext,
) {
    use vega_conversation::types::{UsageDashboard, UsageDay, UsageModelSeries, UsageTotals};
    for dark in [false, true] {
        let (view, window) = mount_settings(cx, dark, 960., 600.);
        let long_model = format!("vendor/{}", "model0123456789".repeat(20));
        let rates = PricingRateInputs {
            input_usd_per_million: "1".into(),
            output_usd_per_million: "2".into(),
            cache_read_usd_per_million: "0.1".into(),
            cache_write_usd_per_million: "0.2".into(),
        };
        view.update(cx, |view, cx| {
            view.section = 3;
            view.apply_pricing_projection(
                PricingSettingsProjection::Ready {
                    generation: 1,
                    entries: vec![PricingEntryProjection {
                        model: long_model.clone(),
                        kind: PricingEntryKind::CustomStatic,
                        base: rates.clone(),
                        peak: None,
                    }],
                    notice: None,
                    draft_reason: None,
                    error: None,
                },
                cx,
            );
        });
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let column = visual.debug_bounds("settings-content-column").unwrap();
        let model = visual
            .debug_bounds("pricing-model-name-0")
            .expect("complete pricing model");
        assert!(model.size.height > px(Typography::BODY * Typography::BODY_LINE_HEIGHT));
        assert!(model.right() <= column.right());
        view.update(cx, |view, cx| view.begin_add_custom(cx));
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for index in 0..4 {
            let label = visual
                .debug_bounds(Box::leak(
                    format!("pricing-rate-{index}-label").into_boxed_str(),
                ))
                .expect("persistent rate label");
            let input = visual
                .debug_bounds(Box::leak(
                    format!("pricing-rate-{index}-control").into_boxed_str(),
                ))
                .expect("original pricing input");
            assert_pixel_close(
                input.left() - label.right(),
                16.,
                "pricing neighboring input",
            );
            assert!(input.right() <= column.right());
        }
        view.update(cx, |view, cx| {
            view.section = 2;
            view.pricing_editor = None;
            view.model_settings = Some(("owned".into(), long_model.clone()));
            view.apply_reasoning_projection(
                ReasoningSettingsProjection::Ready {
                    generation: 1,
                    profiles: vec![explicit_openai_profile("owned", &long_model)],
                    error: None,
                },
                cx,
            );
        });
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for id in [
            "reasoning-protocol-0",
            "reasoning-support-0",
            "reasoning-efforts-0",
            "reasoning-preference-0",
            "reasoning-disabled-0",
            "reasoning-replay-0",
        ] {
            let label = visual
                .debug_bounds(Box::leak(format!("{id}-label").into_boxed_str()))
                .expect("reasoning label");
            let control = visual
                .debug_bounds(Box::leak(format!("{id}-control").into_boxed_str()))
                .expect("reasoning value and action");
            assert_pixel_close(
                control.left() - label.right(),
                16.,
                "reasoning aligned field",
            );
            assert!(control.right() <= column.right());
        }
        let effort = visual
            .debug_bounds("reasoning-effort-0-low")
            .expect("effort option");
        assert!(effort.size.height >= px(28.));
        let days: Vec<_> = (0..365)
            .map(|index| UsageDay {
                start_ms: index * 86_400_000,
                totals: UsageTotals {
                    calls: 1,
                    total_tokens: 100,
                    ..Default::default()
                },
            })
            .collect();
        view.update(cx, |view, cx| {
            view.section = 4;
            view.apply_usage_dashboard(
                Ok(UsageDashboard {
                    generated_at_ms: 0,
                    timezone_label: "UTC".into(),
                    lifetime: UsageTotals {
                        calls: 999_999_999,
                        total_tokens: u64::MAX,
                        priced_cost_microcents: i64::MAX,
                        unpriced_calls: 999_999_999,
                        ..Default::default()
                    },
                    models: vec![UsageModelSeries {
                        model: Some(long_model),
                        days: days[335..].to_vec(),
                    }],
                    days,
                }),
                cx,
            );
        });
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let mut metrics = Vec::new();
        for index in 0..5 {
            let metric = visual
                .debug_bounds(Box::leak(format!("usage-metric-{index}").into_boxed_str()))
                .expect("wrapped metric");
            let value = visual
                .debug_bounds(Box::leak(
                    format!("usage-metric-value-{index}").into_boxed_str(),
                ))
                .expect("complete metric value");
            let label = visual
                .debug_bounds(Box::leak(
                    format!("usage-metric-label-{index}").into_boxed_str(),
                ))
                .expect("complete metric label");
            assert!(value.bottom() <= label.top());
            assert!(value.right() <= metric.right() && label.right() <= metric.right());
            assert!(metric.right() <= column.right());
            for previous in &metrics {
                let previous: &Bounds<gpui_kit::Pixels> = previous;
                assert!(
                    metric.left() >= previous.right()
                        || previous.left() >= metric.right()
                        || metric.top() >= previous.bottom()
                        || previous.top() >= metric.bottom()
                );
            }
            metrics.push(metric);
        }
        let name = visual
            .debug_bounds("usage-model-name-0")
            .expect("complete usage model");
        assert!(name.size.height > px(Typography::BODY * Typography::BODY_LINE_HEIGHT));
        assert!(name.right() <= column.right());
    }
}

#[gpui_kit::test]
async fn issue296_v2_notes_short_content_and_scroll_edges_release_parent(cx: &mut TestAppContext) {
    use vega_conversation::types::{UpdatePhase, UpdateProjection};
    for short in [false, true] {
        let (view, window) = mount_settings(cx, false, 960., 600.);
        view.update(cx, |view, cx| {
            view.apply_update_projection(
                UpdateProjection {
                    phase: UpdatePhase::Ready,
                    notes: if short {
                        "# 更新说明\n\n简短说明。".into()
                    } else {
                        "- 合成更新内容\n".repeat(60)
                    },
                    ..Default::default()
                },
                cx,
            );
            view.updater_notes_open = true;
            cx.notify();
        });
        cx.run_until_parked();
        let body = reveal_settings_control(window, "updater-notes-body", cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let before = visual.debug_bounds("updater-install").unwrap();
        visual.simulate_mouse_move(body.center(), None, Default::default());
        visual.simulate_event(gpui_kit::ScrollWheelEvent {
            position: body.center(),
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(50.))),
            modifiers: Default::default(),
            touch_phase: gpui_kit::TouchPhase::Moved,
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds("updater-install").unwrap().top() > before.top());
        assert_eq!(
            view.read_with(cx, |view, _| view.updater_notes_scroll.offset().y),
            px(0.)
        );
        if short {
            assert_eq!(
                view.read_with(cx, |view, _| view.updater_notes_scroll.max_offset().y),
                px(0.)
            );
        } else {
            let body = reveal_settings_control(window, "updater-notes-body", cx);
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            visual.simulate_mouse_move(body.center(), None, Default::default());
            visual.simulate_event(gpui_kit::ScrollWheelEvent {
                position: body.center(),
                delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-10_000.))),
                modifiers: Default::default(),
                touch_phase: gpui_kit::TouchPhase::Moved,
            });
            visual.run_until_parked();
            let before = visual.debug_bounds("updater-install").unwrap();
            visual.simulate_event(gpui_kit::ScrollWheelEvent {
                position: body.center(),
                delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-50.))),
                modifiers: Default::default(),
                touch_phase: gpui_kit::TouchPhase::Moved,
            });
            visual.run_until_parked();
            assert!(visual.debug_bounds("updater-install").unwrap().top() < before.top());
            view.read_with(cx, |view, _| {
                assert_eq!(
                    view.updater_notes_scroll.offset().y,
                    -view.updater_notes_scroll.max_offset().y
                );
            });
        }
    }
}

#[gpui_kit::test]
async fn issue296_v3_navigation_has_six_object_scoped_routes(cx: &mut TestAppContext) {
    let (_, window) = mount_settings(cx, false, 960., 600.);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let mut previous = px(0.);
    for selector in [
        "settings-nav-general",
        "settings-nav-providers",
        "settings-nav-mcp",
        "settings-nav-agents",
        "settings-nav-skills",
        "settings-nav-usage",
    ] {
        let bounds = visual.debug_bounds(selector).expect("visible navigation");
        assert!(bounds.top() > previous);
        previous = bounds.top();
    }
    assert!(visual.debug_bounds("settings-nav-reasoning").is_none());
    assert!(visual.debug_bounds("settings-nav-pricing").is_none());
}

#[gpui_kit::test]
async fn issue296_v3_model_scope_keeps_shared_price_library_and_dirty_draft(
    cx: &mut TestAppContext,
) {
    let (view, window) = mount_settings(cx, false, 960., 600.);
    view.update(cx, |view, cx| {
        view.config.providers = vec![provider("owned", &["model-a", "model-b"])];
        view.apply_pricing_projection(
            PricingSettingsProjection::Ready {
                generation: 4,
                entries: ["model-a", "model-b", "orphan-model"]
                    .into_iter()
                    .map(|model| PricingEntryProjection {
                        model: model.into(),
                        kind: PricingEntryKind::CustomStatic,
                        base: PricingRateInputs {
                            input_usd_per_million: "1".into(),
                            output_usd_per_million: "2".into(),
                            cache_read_usd_per_million: "3".into(),
                            cache_write_usd_per_million: "4".into(),
                        },
                        peak: None,
                    })
                    .collect(),
                notice: None,
                draft_reason: None,
                error: None,
            },
            cx,
        );
        view.open_model_settings("owned", "model-b", 3, cx);
    });
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("pricing-model-name-0").is_none());
        assert!(visual.debug_bounds("pricing-model-name-1").is_some());
        assert!(visual.debug_bounds("pricing-model-name-2").is_none());
        assert!(visual.debug_bounds("model-settings-basic").is_some());
    }
    view.update(cx, |view, cx| {
        let PricingSettingsProjection::Ready { entries, .. } = &view.pricing else {
            panic!("ready prices")
        };
        view.begin_edit_pricing(entries[1].clone(), cx);
        view.pricing_rate_inputs[0].update(cx, |input, cx| input.set_text("9", cx));
        view.request_settings_section(1, cx);
    });
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.section), 3);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("settings-unsaved-guard").is_some());
    assert!(visual.debug_bounds("pricing-editor-model").is_some());
    let stay = visual.debug_bounds("settings-stay").unwrap();
    visual.simulate_click(stay.center(), Default::default());
    visual.run_until_parked();
    assert!(
        view.read_with(cx, |view, cx| view.pricing_rate_inputs[0].read(cx).text()
            == "9")
    );
    view.update(cx, |view, cx| view.request_settings_section(1, cx));
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let discard = visual
        .debug_bounds("settings-discard-before-leave")
        .unwrap();
    visual.simulate_click(discard.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.section), 1);
    assert!(view.read_with(cx, |view, _| view.pricing_editor.is_none()
        && view.model_settings.is_none()));
    view.update(cx, |view, cx| view.request_settings_section(3, cx));
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("pricing-model-name-2")
            .is_some()
    );
}

#[gpui_kit::test]
async fn issue296_v3_reasoning_legacy_entry_selects_exact_model(cx: &mut TestAppContext) {
    let (view, window) = mount_settings(cx, false, 960., 600.);
    view.update(cx, |view, cx| {
        view.section = 2;
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 8,
                profiles: vec![
                    explicit_openai_profile("a", "one"),
                    explicit_openai_profile("b", "two"),
                ],
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("reasoning-protocol-0").is_none());
    let select = visual.debug_bounds("reasoning-model-select-1").unwrap();
    visual.simulate_click(select.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.model_settings.clone()),
        Some(("b".into(), "two".into()))
    );
    assert!(visual.debug_bounds("reasoning-protocol-0").is_none());
    assert!(visual.debug_bounds("reasoning-protocol-1").is_some());
    let choose = reveal_settings_control(window, "reasoning-support-1-option-0", cx);
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(choose.center(), Default::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| matches!(&view.reasoning, ReasoningSettingsProjection::Saving { profile, .. } if profile.provider == "b" && profile.model == "two" && profile.support == ReasoningSupport::Required)));
}

#[gpui_kit::test]
async fn issue296_v3_update_help_pins_dismisses_and_preserves_errors(cx: &mut TestAppContext) {
    let (view, window) = mount_settings(cx, false, 960., 600.);
    view.update(cx, |view, cx| {
        let mut projection = view.updater.clone();
        projection.phase = vega_conversation::types::UpdatePhase::Error;
        projection.message = "签名验证失败，未安装更新".into();
        view.apply_update_projection(projection, cx);
    });
    cx.run_until_parked();
    let trigger = reveal_settings_control(window, "updater-help", cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(trigger.center(), Default::default());
    visual.run_until_parked();
    visual.simulate_mouse_move(gpui_kit::point(px(0.), px(0.)), None, Default::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("settings-help-popup").is_some());
    assert!(visual.debug_bounds("settings-status").is_some());
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-help-popup")
            .is_none()
    );
    assert!(
        window
            .update(cx, |_, window, cx| view
                .read(cx)
                .settings_help_focus
                .is_focused(window))
            .unwrap()
    );
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(trigger.center(), Default::default());
    visual.run_until_parked();
    visual.simulate_click(gpui_kit::point(px(500.), px(10.)), Default::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("settings-help-popup").is_none());
    assert!(visual.debug_bounds("settings-status").is_some());
}

#[gpui_kit::test]
async fn issue296_v3_selected_reasoning_options_never_reset_custom_fields(cx: &mut TestAppContext) {
    let (view, window) = mount_settings(cx, false, 1100., 900.);
    let mut profile = explicit_openai_profile("owned", "custom");
    profile.efforts = vec!["high".into()];
    profile.preference = ReasoningChoice::Effort("high".into());
    profile.preserve_reasoning_content = true;
    let expected = profile.clone();
    let saves = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    cx.update(|cx| {
        let saves = saves.clone();
        cx.subscribe(
            &view,
            move |_, request: &ReasoningProfileSaveRequested, _| {
                saves.lock().unwrap().push(request.clone());
            },
        )
        .detach();
    });
    view.update(cx, |view, cx| {
        view.section = 2;
        view.model_settings = Some(("owned".into(), "custom".into()));
        view.apply_reasoning_projection(
            ReasoningSettingsProjection::Ready {
                generation: 1,
                profiles: vec![profile],
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    let bounds = reveal_settings_control(window, "reasoning-protocol-0-option-1", cx);
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
    assert!(saves.lock().unwrap().is_empty());
    view.read_with(cx, |view, _| {
        let ReasoningSettingsProjection::Ready { profiles, .. } = &view.reasoning else {
            panic!("same selection must not save")
        };
        assert_eq!(profiles, &[expected]);
    });
}

fn wait_issue75_preferences(cx: &mut TestAppContext, view: &Entity<SettingsView>) {
    for _ in 0..150 {
        cx.run_until_parked();
        if view.read_with(cx, |view, _| !view.provider_management.saving) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("preferences did not settle");
}

#[gpui_kit::test]
async fn issue75_native_toggles_preserve_other_disk_settings_and_reject_conflict(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let config = AppConfig::default();
    config.save_to(&path).unwrap();
    let (view, window) = mount_settings(cx, false, 960., 600.);
    view.update(cx, |view, cx| {
        view.config = config.clone();
        view.config_path = Some(path.clone());
        view.section = 7;
        cx.notify();
    });
    cx.run_until_parked();
    let mut external = config.clone();
    external.agent.turn_limit = 19;
    external.save_to(&path).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let desktop = visual.debug_bounds("settings-desktop-use-toggle").unwrap();
    let browser = visual.debug_bounds("settings-browser-use-toggle").unwrap();
    assert!(desktop.bottom() < px(600.));
    assert!(browser.bottom() < px(600.));
    visual.simulate_click(desktop.center(), Default::default());
    cx.run_until_parked();
    wait_issue75_preferences(cx, &view);
    let saved = config::read_from(&path).unwrap();
    assert!(saved.agent.desktop_use_enabled);
    assert!(!saved.agent.browser_use_enabled);
    assert_eq!(saved.agent.turn_limit, 19);
    cx.simulate_keystrokes(window.into(), "space");
    cx.run_until_parked();
    wait_issue75_preferences(cx, &view);
    assert!(!config::read_from(&path).unwrap().agent.desktop_use_enabled);
    let mut external = config::read_from(&path).unwrap();
    external.agent.browser_use_enabled = true;
    external.save_to(&path).unwrap();
    view.update(cx, |view, cx| {
        let mut candidate = view.config.clone();
        candidate.agent.browser_use_enabled = true;
        view.save_preferences(candidate, cx);
    });
    wait_issue75_preferences(cx, &view);
    assert!(view.read_with(cx, |view, _| view.error.is_some()));
    assert!(config::read_from(&path).unwrap().agent.browser_use_enabled);
}
