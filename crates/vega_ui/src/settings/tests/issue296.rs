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
async fn issue296_settings_eight_pages_keep_one_readable_column(cx: &mut TestAppContext) {
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
                assert!(column.size.width <= px(Layout::SETTINGS_CONTENT_MAX_WIDTH));
                assert!(column.right() <= px(width));
                assert!(body.size.height > px(0.));
                assert_pixel_close(body.left(), f32::from(column.left()), "shared leading edge");
                for nav in [
                    "settings-nav-general",
                    "settings-nav-providers",
                    "settings-nav-reasoning",
                    "settings-nav-pricing",
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
            assert!(input.top() >= label.bottom());
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
            let last_navigation = view.read(cx).section_focuses[6].clone();
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
