use super::*;
use gpui_kit::{Modifiers, VisualTestContext, size};

fn history() -> HistoryPage {
    hydration_page(
        (1..=60)
            .map(|index| {
                if index % 2 == 0 {
                    hydration_assistant(
                        index,
                        "## Result\n\nReadable answer.\n\n```rust\nlet value = 1;\n```\n",
                    )
                } else {
                    hydration_user(index, "A user request with enough context to read.")
                }
            })
            .collect(),
        None,
    )
}

#[gpui_kit::test]
async fn issue296_reading_column_and_navigation_do_not_compete(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-reading");
    stream.update(cx, |stream, cx| stream.apply_history_page(history(), cx));
    for theme in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        let colors = theme.colors;
        cx.update(|cx| cx.set_global(theme));
        for (width, height) in [
            (1037., 814.),
            (834., 714.),
            (594., 554.),
            (863., 714.),
            (864., 714.),
        ] {
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            visual.simulate_resize(size(px(width), px(height)));
            visual.run_until_parked();
            let column = visual.debug_bounds("conversation-column").expect("column");
            let list = visual
                .debug_bounds("conversation-message-list")
                .expect("list");
            let composer = visual.debug_bounds("composer-shell").expect("composer");
            let rail = visual.debug_bounds("message-anchor-rail").expect("rail");
            assert_eq!(list.center().x, composer.center().x, "shared reading axis");
            assert_eq!(column.size.width, px((width - 64.).min(736.)));
            assert_eq!(rail.size.width, px(24.));
            assert_eq!(rail.size.height, px(90.));
            assert_eq!(rail.center().x, px(16.));
            let foreground = visual
                .debug_bounds("conversation-foreground")
                .expect("foreground");
            let readable_center = (list.top() + foreground.top()) / 2.;
            assert!((f32::from(rail.center().y - readable_center)).abs() < 1.);
            assert!(rail.top() >= list.top() && rail.bottom() <= foreground.top());
            assert!(rail.right() <= list.left());
            let (quads, scale) =
                visual.update(|window, _| (window.painted_quads(), window.scale_factor()));
            for (side, color) in [
                (6., colors.conversation_marker),
                (16., colors.text_secondary),
            ] {
                assert!(
                    quads.iter().any(|quad| {
                        (quad.bounds.size.width.as_f32() / scale - side).abs() < 0.1
                            && (quad.bounds.center().x.as_f32() / scale
                                - f32::from(rail.center().x))
                            .abs()
                                < 0.1
                            && quad.background.as_solid() == Some(color.into())
                    }),
                    "navigation must paint {side}px token geometry"
                );
            }
            let navigation = visual
                .debug_bounds("conversation-reading-navigation")
                .expect("reserved navigation row");
            assert_eq!(navigation.size.height, px(40.));
            assert!(navigation.top() >= list.top());
            assert!(list.bottom() >= composer.bottom());
            assert!(navigation.bottom() <= composer.top());
            assert!(visual.debug_bounds("scroll-to-bottom").is_none());
        }
    }
}

#[gpui_kit::test]
async fn issue296_resume_tail_keeps_reading_viewport_stable(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-resume");
    stream.update(cx, |stream, cx| stream.apply_history_page(history(), cx));
    cx.run_until_parked();
    let before = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("conversation-message-list")
        .expect("list");
    stream.update(cx, |stream, cx| {
        stream.list.scroll_by(px(-600.));
        cx.notify();
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let list = visual
        .debug_bounds("conversation-message-list")
        .expect("list");
    let navigation = visual
        .debug_bounds("conversation-reading-navigation")
        .expect("navigation");
    let button = visual
        .debug_bounds("scroll-to-bottom")
        .expect("resume control");
    assert_eq!(list, before);
    assert_eq!(button.size.height, px(32.));
    assert_eq!(button.size.width, button.size.height);
    let composer = visual.debug_bounds("composer-shell").expect("composer");
    assert!(navigation.top() >= list.top());
    assert!(button.bottom() <= composer.top());
    assert!(list.bottom() >= composer.bottom());
    assert_eq!(button.top() - navigation.top(), px(4.));
    assert_eq!(navigation.bottom() - button.bottom(), px(4.));
    let anchor = stream.read_with(&visual, |stream, _| stream.scroll_anchor_snapshot());
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "live-tail".into(),
                seq: 61,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live-tail".into(),
                delta: "New output while reading history. ".repeat(10),
            },
            cx,
        );
    });
    visual.run_until_parked();
    let after = stream.read_with(&visual, |stream, _| stream.scroll_anchor_snapshot());
    assert_eq!(after.identity, anchor.identity);
    assert_eq!(after.message_id, anchor.message_id);
    assert!((after.offset_in_item_px - anchor.offset_in_item_px).abs() < 1.);
    assert!(!stream.read_with(&visual, |stream, _| stream.following_tail()));
    assert_eq!(
        visual.debug_bounds("conversation-message-list"),
        Some(before)
    );
    visual.simulate_click(button.center(), Modifiers::default());
    visual.run_until_parked();
    assert!(stream.read_with(&visual, |stream, _| stream.following_tail()));
    assert!(visual.debug_bounds("scroll-to-bottom").is_none());
    assert_eq!(
        visual.debug_bounds("conversation-message-list"),
        Some(before)
    );
}

#[gpui_kit::test]
async fn issue296_process_headers_share_the_control_rhythm(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-process");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "run".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "run".into(),
                delta: "Reviewing the request".into(),
            },
            cx,
        );
    });
    open_run_details(window, cx);
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert_eq!(
        visual
            .debug_bounds("run-activity-toggle")
            .unwrap()
            .size
            .height,
        px(23.)
    );
    assert_eq!(
        visual
            .debug_bounds("run-stage-toggle-0")
            .unwrap()
            .size
            .height,
        px(25.)
    );
    assert!(visual.debug_bounds("run-activity-icon").is_none());
    stream.update(&mut visual, |stream, cx| {
        for id in ["one", "two"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "pwd"),
                },
                cx,
            );
        }
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("tool-activity-child-0").is_none());
    let stage = visual
        .debug_bounds("run-stage-toggle-0")
        .expect("collapsed stage");
    visual.simulate_click(stage.center(), Modifiers::default());
    visual.run_until_parked();
    for (selector, icon_selector) in [
        ("run-stage-tool-0-0", "run-stage-tool-0-0-icon"),
        ("run-stage-tool-0-1", "run-stage-tool-0-1-icon"),
    ] {
        let header = visual.debug_bounds(selector).expect("tool process header");
        let icon = visual.debug_bounds(icon_selector).expect("tool icon");
        assert_eq!(header.size.height, px(25.));
        assert_eq!(icon.size, size(px(16.), px(16.)));
        assert_eq!(icon.center().y, header.center().y);
    }
}

#[gpui_kit::test]
async fn issue296_long_model_keeps_composer_actions_inside_the_surface(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-composer");
    stream.update(cx, |stream, cx| {
        stream.composer_defaults.model =
            "model-with-a-very-long-name-and-a-detailed-version-number".into();
        stream.apply_credential_error(cx);
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(594.), px(554.)));
    visual.run_until_parked();
    let shell = visual.debug_bounds("composer-shell").expect("composer");
    for selector in [
        "composer-add",
        "composer-model",
        "composer-permission-status",
        "composer-send",
    ] {
        let control = visual.debug_bounds(selector).expect("composer control");
        assert!(control.left() >= shell.left());
        assert!(control.right() <= shell.right());
        assert_eq!(control.size.height, px(28.), "{selector}");
    }
    let error = visual
        .debug_bounds("conversation-controller-error")
        .expect("error");
    assert_eq!(error.size.width, shell.size.width);
    assert!(error.bottom() <= shell.top());
    assert!(visual.debug_bounds("conversation-status-icon").is_none());
    let text = visual
        .debug_bounds("conversation-error-text")
        .expect("plain error text");
    assert_eq!(text.size.width, error.size.width);
    assert!(text.top() >= error.top() && text.bottom() <= error.bottom());
}

struct MessageFixture {
    user: StreamEntry,
    assistant: StreamEntry,
    focus: Option<FocusHandle>,
}

impl Render for MessageFixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render = |entry: &StreamEntry, window: &mut Window, cx: &mut App| {
            if let Some(focus) = &self.focus {
                render_entry_with_selection(entry, window, cx, focus)
            } else {
                render_entry(entry, window, cx)
            }
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(render(&self.user, window, cx))
            .child(render(&self.assistant, window, cx))
    }
}

#[gpui_kit::test]
fn issue296_message_rhythm_matches_for_selected_and_plain_renderers(cx: &mut TestAppContext) {
    init_permission_test(cx);
    for selectable in [false, true] {
        let (_, visual) = cx.add_window_view(|_, cx| {
            let mut line = StreamLine::new(1, LineKind::Paragraph);
            line.spans.push(StreamSpan {
                text: "One answer".into(),
                style: SpanStyle::Plain,
                link: None,
            });
            let mut model = StreamModel::default();
            model.committed_lines.push(line);
            MessageFixture {
                user: StreamEntry::User {
                    lines: user_message_lines(1, "One request\n\nThird line"),
                    copy: MessageCopy::new("One request\n\nThird line"),
                },
                assistant: StreamEntry::Assistant {
                    stream: Box::default(),
                    model,
                    failure: None,
                    copy: MessageCopy::new("One answer"),
                },
                focus: selectable.then(|| cx.focus_handle()),
            }
        });
        visual.simulate_resize(size(px(600.), px(600.)));
        visual.run_until_parked();
        let user = visual.debug_bounds("user-message").expect("user row");
        let bubble = visual.debug_bounds("user-message-bubble").expect("bubble");
        let assistant = visual.debug_bounds("assistant-message").expect("assistant");
        assert_eq!(bubble.top() - user.top(), px(24.));
        assert_eq!(user.bottom() - bubble.bottom(), px(48.));
        assert_eq!(assistant.top(), user.bottom());
        assert!(
            (f32::from(assistant.size.height)
                - (16. + Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT))
                .abs()
                < 1.
        );
        assert!(visual.debug_bounds("message-copy-user").is_none());
        assert!(visual.debug_bounds("message-copy-assistant").is_none());
    }
}

fn advance_interaction(visual: &mut VisualTestContext) {
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(120));
    visual.update(|window, cx| window.simulate_next_frame(cx));
    visual.run_until_parked();
}

fn has_hover_surface(
    visual: &mut VisualTestContext,
    target: gpui_kit::Bounds<Pixels>,
    color: gpui_kit::Rgba,
) -> bool {
    visual.update(|window, _| {
        let scale = window.scale_factor();
        window.painted_quads().iter().any(|quad| {
            quad.bounds == target.scale(scale) && quad.background.as_solid() == Some(color.into())
        })
    })
}

fn append_standalone_tool_group(stream: &Entity<ConversationStream>, cx: &mut TestAppContext) {
    stream.update(cx, |stream, cx| {
        let first = cx.new(|_| ToolCard::proposed(&bash_call("standalone-one", "pwd")));
        let second = cx.new(|_| ToolCard::proposed(&bash_call("standalone-two", "pwd")));
        let group = cx.new(|_| ToolActivityGroup::new(first, second));
        let index = stream.entries.len();
        stream.entries.push(StreamEntry::ToolGroup { group });
        stream.list_append(index);
        cx.notify();
    });
}

#[gpui_kit::test]
async fn issue296_disclosures_enter_and_leave_hover_without_moving(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-hover");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "run".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "run".into(),
                delta: "Reviewing".into(),
            },
            cx,
        );
        for id in ["one", "two"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "pwd"),
                },
                cx,
            );
        }
    });
    stream.update(cx, |stream, cx| confirm_test_final(stream, "run", cx));
    open_run_details(window, cx);
    append_standalone_tool_group(&stream, cx);
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("tool-activity-child-0").is_none());
    let group = visual
        .debug_bounds("tool-activity-group-toggle")
        .expect("collapsed tool group");
    visual.simulate_click(group.center(), Modifiers::default());
    visual.run_until_parked();
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        let colors = palette.colors;
        cx.update(|cx| {
            cx.set_global(palette);
            cx.refresh_windows();
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        for selector in [
            "run-activity-toggle",
            "thinking-toggle",
            "tool-activity-group-toggle",
            "tool-activity-child-0",
            "composer-model",
            "composer-permission-status",
        ] {
            let chevron = match selector {
                "run-activity-toggle" => Some("run-activity-chevron"),
                "thinking-toggle" => Some("thinking-chevron"),
                "tool-activity-group-toggle" => Some("tool-activity-group-chevron"),
                "tool-activity-child-0" => Some("tool-activity-child-0-chevron"),
                _ => None,
            };
            let row = visual.debug_bounds(selector).expect("interactive row");
            visual.simulate_mouse_move(gpui_kit::point(px(1.), px(1.)), None, Modifiers::default());
            visual.run_until_parked();
            advance_interaction(&mut visual);
            assert!(
                !has_hover_surface(&mut visual, row, colors.bg_hover),
                "{selector} before hover"
            );
            if let Some(chevron) = chevron {
                assert!(
                    issue296_single_activity::chevron_painted(&mut visual, chevron)
                        == matches!(
                            selector,
                            "run-activity-toggle" | "tool-activity-group-toggle"
                        ),
                    "{selector} resting"
                );
            }
            visual.simulate_mouse_move(row.center(), None, Modifiers::default());
            visual.run_until_parked();
            advance_interaction(&mut visual);
            assert!(
                has_hover_surface(&mut visual, row, colors.bg_hover)
                    == matches!(selector, "composer-model" | "composer-permission-status"),
                "{selector} hover feedback"
            );
            if let Some(chevron) = chevron {
                assert!(
                    issue296_single_activity::chevron_painted(&mut visual, chevron),
                    "{selector} hover"
                );
            }
            assert_eq!(visual.debug_bounds(selector), Some(row));
            visual.simulate_mouse_move(gpui_kit::point(px(1.), px(1.)), None, Modifiers::default());
            visual.run_until_parked();
            advance_interaction(&mut visual);
            assert!(
                !has_hover_surface(&mut visual, row, colors.bg_hover),
                "{selector} leaves hover"
            );
            if let Some(chevron) = chevron {
                assert!(
                    issue296_single_activity::chevron_painted(&mut visual, chevron)
                        == matches!(
                            selector,
                            "run-activity-toggle" | "tool-activity-group-toggle"
                        ),
                    "{selector} left"
                );
            }
            assert_eq!(visual.debug_bounds(selector), Some(row));
        }
    }
}

#[gpui_kit::test]
async fn issue296_multiline_error_is_plain_and_permission_keeps_first_line_icon_alignment(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-status");
    stream.update(cx, |stream, cx| {
        stream.controller_error = Some("The request needs attention. ".repeat(15));
        cx.notify();
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(594.), px(554.)));
    visual.run_until_parked();
    assert!(visual.debug_bounds("conversation-status-icon").is_none());
    let text = visual
        .debug_bounds("conversation-error-text")
        .expect("plain error text");
    let error = visual
        .debug_bounds("conversation-controller-error")
        .expect("error region");
    assert!(text.size.height > px(Typography::BODY * Typography::BODY_LINE_HEIGHT * 2.));
    assert_eq!(text.left(), error.left());
    assert_eq!(text.right(), error.right());
    assert!(text.top() >= error.top() && text.bottom() <= error.bottom());
    assert!(!visual.update(|window, _| {
        let scale = window.scale_factor();
        window.painted_quads().iter().any(|quad| {
            (quad.bounds == text.scale(scale) || quad.bounds == error.scale(scale))
                && quad.border_color == vega_theme::LIGHT.border_subtle.into()
        })
    }));

    init_permission_test(cx);
    let (window, queue) = open_permission_stream(cx);
    propose(window, cx, bash_call("issue296-permission", "printf ok"));
    let decision = request_permission(&queue, "issue296-permission", "printf ok");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let card = visual
        .debug_bounds("conversation-permission-card")
        .expect("permission surface");
    let icon = visual
        .debug_bounds("permission-card-icon")
        .expect("permission icon");
    let label = visual
        .debug_bounds("permission-card-label")
        .expect("permission label");
    let action = visual
        .debug_bounds("permission-action")
        .expect("permission action");
    assert_eq!(icon.size, size(px(16.), px(16.)));
    assert!(
        (f32::from(icon.center().y - label.top())
            - Typography::BODY * Typography::BODY_LINE_HEIGHT / 2.)
            .abs()
            < 1.
    );
    assert_eq!(action.size.height, px(28.));
    assert!(action.left() >= card.left() && action.right() <= card.right());
    assert!(visual.update(|window, _| {
        let scale = window.scale_factor();
        window.painted_quads().iter().any(|quad| {
            quad.bounds == card.scale(scale)
                && quad.border_color == vega_theme::LIGHT.border_subtle.into()
                && (quad.corner_radii.top_left.as_f32() / scale - 8.).abs() < 1.
        })
    }));
    visual.simulate_keystrokes("enter");
    assert_eq!(decision.await, PermissionDecision::Once);
}

#[gpui_kit::test]
async fn issue296_native_and_codex_footers_keep_actions_visible_in_compact_panes(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-compact");
    for backend in [TaskBackend::Native, TaskBackend::Codex] {
        for width in [594., 400., 320.] {
            for running in [false, true] {
                stream.update(cx, |stream, cx| {
                    stream.thread.backend = backend;
                    stream.codex_mode_confirmed = true;
                    stream.composer_defaults.model =
                        "model-with-a-very-long-name-and-a-detailed-version-number".into();
                    stream.actions.running = running;
                    stream.set_workspace_width(width, cx);
                    cx.notify();
                });
                let mut visual = VisualTestContext::from_window(window.into(), cx);
                visual.simulate_resize(size(px(width), px(554.)));
                visual.run_until_parked();
                let shell = visual.debug_bounds("composer-shell").expect("composer");
                let status = if backend == TaskBackend::Native {
                    "composer-permission-status"
                } else {
                    "composer-backend-status"
                };
                let send = if running {
                    "composer-stop"
                } else {
                    "composer-send"
                };
                let status_bounds = visual.debug_bounds(status).expect("permission status");
                let action_bounds = visual.debug_bounds(send).expect("send or stop");
                for control in [status_bounds, action_bounds] {
                    assert!(
                        control.left() >= shell.left() && control.right() <= shell.right(),
                        "{backend:?} width {width}, {control:?}, {shell:?}"
                    );
                    assert_eq!(control.size.height, px(28.));
                }
                assert_eq!(status_bounds.center().y, action_bounds.center().y);
                assert!(visual.debug_bounds("composer-footer-branch").is_none());
            }
        }
    }
}

#[gpui_kit::test]
async fn issue296_disclosure_focus_rings_do_not_move_icons_or_labels(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue296-focus");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "run".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "run".into(),
                delta: "Reviewing".into(),
            },
            cx,
        );
        for id in ["one", "two"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "pwd"),
                },
                cx,
            );
        }
    });
    stream.update(cx, |stream, cx| confirm_test_final(stream, "run", cx));
    open_run_details(window, cx);
    append_standalone_tool_group(&stream, cx);
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("tool-activity-child-0").is_none());
    let group = visual
        .debug_bounds("tool-activity-group-toggle")
        .expect("collapsed tool group");
    visual.simulate_click(group.center(), Modifiers::default());
    visual.run_until_parked();
    let selectors = [
        (
            "run-activity-toggle",
            "run-activity-status",
            "run-activity-status",
        ),
        ("thinking-toggle", "thinking-icon", "thinking-title"),
        (
            "tool-activity-group-toggle",
            "tool-activity-group-icon",
            "tool-activity-group-label",
        ),
        (
            "tool-activity-child-0",
            "tool-activity-child-0-icon",
            "tool-activity-child-0-title",
        ),
    ];
    let before = selectors.map(|(row, icon, label)| {
        (
            visual.debug_bounds(row).expect("row"),
            visual.debug_bounds(icon).expect("icon"),
            visual.debug_bounds(label).expect("label"),
        )
    });
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| window.blur(cx));
    let mut focused = [false; 4];
    for _ in 0..16 {
        visual.update(|window, cx| window.focus_next(cx));
        visual.run_until_parked();
        let (quads, scale) =
            visual.update(|window, _| (window.painted_quads(), window.scale_factor()));
        for (index, (row, icon, label)) in selectors.iter().enumerate() {
            assert_eq!(visual.debug_bounds(row), Some(before[index].0));
            assert_eq!(visual.debug_bounds(icon), Some(before[index].1));
            assert_eq!(visual.debug_bounds(label), Some(before[index].2));
            let receives_focus = quads.iter().any(|quad| {
                quad.bounds == before[index].0.scale(scale)
                    && (quad.border_color == vega_theme::LIGHT.accent.into()
                        || quad.background.as_solid() == Some(vega_theme::LIGHT.bg_hover.into()))
            });
            if receives_focus {
                let chevron = [
                    "run-activity-chevron",
                    "thinking-chevron",
                    "tool-activity-group-chevron",
                    "tool-activity-child-0-chevron",
                ][index];
                assert!(
                    issue296_single_activity::chevron_painted(&mut visual, chevron),
                    "{row} keyboard disclosure"
                );
            }
            focused[index] |= receives_focus;
        }
    }
    assert!(
        focused.iter().all(|focused| *focused),
        "each disclosure must receive a visible keyboard focus ring: {focused:?}"
    );
}
