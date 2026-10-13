use super::*;
use gpui_kit::{Modifiers, VisualTestContext};

fn start(stream: &mut ConversationStream, id: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: id.into(),
            seq: 1,
        },
        cx,
    );
}

fn thinking(stream: &mut ConversationStream, id: &str, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::SummaryDelta {
            message_id: id.into(),
            delta: "Synthetic phase".into(),
        },
        cx,
    );
}

fn tool(
    stream: &mut ConversationStream,
    id: &str,
    status: ToolCallStatus,
    cx: &mut Context<ConversationStream>,
) {
    stream.apply_event(
        ConversationEvent::ToolCallProposed {
            call: bash_call(id, "pwd"),
        },
        cx,
    );
    if status != ToolCallStatus::Rejected {
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: id.into(),
                approval: Approval::Once,
            },
            cx,
        );
    }
    stream.apply_event(
        ConversationEvent::ToolCallFinished {
            call_id: id.into(),
            result: ToolResult {
                status,
                output: "synthetic output".into(),
                reused: false,
                exit_code: (status == ToolCallStatus::Success).then_some(0),
                duration_ms: (status == ToolCallStatus::Success).then_some(10),
                truncated: (status == ToolCallStatus::Success).then_some(false),
                invalid: None,
            },
        },
        cx,
    );
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let row = visual.debug_bounds(selector).expect(selector);
    visual.simulate_click(row.center(), Modifiers::default());
    visual.run_until_parked();
}

fn leave(visual: &mut VisualTestContext) {
    visual.simulate_mouse_move(gpui_kit::point(px(2.), px(2.)), None, Modifiers::default());
    visual.run_until_parked();
}

#[gpui_kit::test]
async fn issue296_polish_p01_final_details_share_icon_and_text_axes(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "polish-axes");
    stream.update(cx, |stream, cx| {
        start(stream, "polish", cx);
        thinking(stream, "polish", cx);
        tool(stream, "single", ToolCallStatus::Success, cx);
        thinking(stream, "polish", cx);
        tool(stream, "group-one", ToolCallStatus::Success, cx);
        tool(stream, "group-two", ToolCallStatus::Success, cx);
        confirm_test_final(stream, "polish", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "run-activity-toggle");
    let tool = visual.debug_bounds("tool-activity-single-row").unwrap();
    let tool_icon = visual
        .debug_bounds("tool-activity-single-row-icon")
        .unwrap();
    let tool_title = visual
        .debug_bounds("tool-activity-single-row-title")
        .unwrap();
    for (icon, title) in [("thinking-icon", "thinking-title")] {
        assert_eq!(visual.debug_bounds(icon).unwrap().left(), tool_icon.left());
        assert_eq!(
            visual.debug_bounds(title).unwrap().left(),
            tool_title.left()
        );
        assert_eq!(
            visual.debug_bounds(title).unwrap().left() - tool.left(),
            px(22.)
        );
    }
    assert_eq!(
        visual.debug_bounds("thinking-toggle").unwrap().size.height,
        px(25.)
    );
    for selector in ["run-detail-row-1", "run-detail-row-3", "run-detail-row-4"] {
        let row = visual.debug_bounds(selector).expect("flattened tool row");
        assert_eq!(row.left(), tool.left());
        assert_eq!(row.size.height, px(25.));
    }
    assert!(visual.debug_bounds("tool-activity-group-toggle").is_none());
    let lefts = [
        "thinking-icon",
        "thinking-title",
        "tool-activity-single-row-icon",
        "tool-activity-single-row-title",
    ]
    .map(|selector| (selector, visual.debug_bounds(selector).unwrap().left()));
    let row = visual.debug_bounds("thinking-toggle").unwrap();
    visual.simulate_mouse_move(row.center(), None, Modifiers::default());
    visual.run_until_parked();
    focus_row(&mut visual, "thinking-toggle");
    for (selector, left) in lefts {
        assert_eq!(visual.debug_bounds(selector).unwrap().left(), left);
    }
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    for (selector, left) in lefts {
        assert_eq!(visual.debug_bounds(selector).unwrap().left(), left);
    }
}

#[gpui_kit::test]
async fn issue296_polish_p02_expanded_stage_arrow_survives_mouse_leave(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "polish-arrow");
    stream.update(cx, |stream, cx| {
        start(stream, "polish", cx);
        thinking(stream, "polish", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    leave(&mut visual);
    assert!(!issue296_single_activity::chevron_painted(
        &mut visual,
        "run-stage-chevron-0"
    ));
    let initial = visual.debug_bounds("run-stage-chevron-0").unwrap();
    click(&mut visual, "run-stage-toggle-0");
    leave(&mut visual);
    assert!(issue296_single_activity::chevron_painted(
        &mut visual,
        "run-stage-chevron-0"
    ));
    assert_eq!(visual.debug_bounds("run-stage-chevron-0").unwrap(), initial);
    click(&mut visual, "run-stage-toggle-0");
    leave(&mut visual);
    assert!(!issue296_single_activity::chevron_painted(
        &mut visual,
        "run-stage-chevron-0"
    ));
}

#[gpui_kit::test]
async fn issue296_polish_p03_latest_tool_issue_is_a_single_tail_notice(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "polish-notice");
    stream.update(cx, |stream, cx| {
        start(stream, "polish", cx);
        tool(stream, "failed", ToolCallStatus::Failed, cx);
        tool(stream, "rejected", ToolCallStatus::Rejected, cx);
        tool(stream, "success", ToolCallStatus::Success, cx);
        confirm_test_final(stream, "polish", cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-activity-exception").is_none());
    let notice = visual
        .debug_bounds("run-notice-polish")
        .expect("latest issue at run tail");
    let final_body = visual.debug_bounds("assistant-body-2").unwrap();
    assert!(notice.top() >= final_body.bottom());
    click(&mut visual, "run-activity-toggle");
    assert!(visual.debug_bounds("run-notice-polish").is_some());
    stream.read_with(&visual, |stream, cx| {
        assert_eq!(
            stream.tool_cards["failed"].read(cx).activity_state(),
            crate::tool_card::ToolActivityState::Failed
        );
        assert_eq!(
            stream.tool_cards["rejected"].read(cx).activity_state(),
            crate::tool_card::ToolActivityState::Rejected
        );
        assert_eq!(stream.run_notices.len(), 1);
        assert_eq!(
            stream.run_notices.values().next().unwrap().text(),
            stream.tool_cards["rejected"]
                .read(cx)
                .compact_visible_text()
        );
        assert!(
            !stream
                .run_notices
                .values()
                .next()
                .unwrap()
                .text()
                .contains("synthetic output")
        );
    });
    stream.update(&mut visual, |stream, cx| {
        tool(stream, "cancelled", ToolCallStatus::Cancelled, cx);
        tool(stream, "later-success", ToolCallStatus::Success, cx);
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, cx| {
        assert_eq!(stream.run_notices.len(), 1);
        assert_eq!(
            stream.run_notices.values().next().unwrap().text(),
            stream.tool_cards["cancelled"]
                .read(cx)
                .compact_visible_text()
        );
    });
}

#[gpui_kit::test]
async fn issue296_polish_p02_all_disclosures_theme_keyboard_and_disabled(cx: &mut TestAppContext) {
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        cx.update(|cx| cx.set_global(palette));
        for target in ["run", "stage", "tool", "group", "thinking"] {
            let (window, stream, _) = open_controller_stream(cx, target);
            cx.update(|cx| {
                cx.set_global(palette);
                cx.refresh_windows();
            });
            stream.update(cx, |stream, cx| {
                start(stream, "polish", cx);
                if matches!(target, "stage" | "thinking") {
                    thinking(stream, "polish", cx);
                } else {
                    tool(stream, "one", ToolCallStatus::Success, cx);
                }
                if target == "group" {
                    tool(stream, "two", ToolCallStatus::Success, cx);
                }
                if matches!(target, "run" | "thinking") {
                    confirm_test_final(stream, "polish", cx);
                }
            });
            cx.run_until_parked();
            let mut visual = VisualTestContext::from_window(window.into(), cx);
            if target == "thinking" {
                click(&mut visual, "run-activity-toggle");
            }
            let (toggle, arrow) = match target {
                "run" => ("run-activity-toggle", "run-activity-chevron"),
                "stage" => ("run-stage-toggle-0", "run-stage-chevron-0"),
                "tool" => (
                    "tool-activity-single-row",
                    "tool-activity-single-row-chevron",
                ),
                "group" => ("tool-activity-group-toggle", "tool-activity-group-chevron"),
                _ => ("thinking-toggle", "thinking-chevron"),
            };
            leave(&mut visual);
            assert!(
                !issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} collapsed"
            );
            let bounds = visual.debug_bounds(arrow).unwrap();
            let row = visual.debug_bounds(toggle).unwrap();
            visual.simulate_mouse_move(row.center(), None, Modifiers::default());
            visual.run_until_parked();
            assert!(
                issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} hover"
            );
            click(&mut visual, toggle);
            leave(&mut visual);
            assert!(
                issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} expanded"
            );
            assert_eq!(visual.debug_bounds(arrow).unwrap(), bounds);
            click(&mut visual, toggle);
            leave(&mut visual);
            assert!(
                !issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} collapsed again"
            );
            focus_row(&mut visual, toggle);
            visual.simulate_keystrokes("enter");
            visual.run_until_parked();
            leave(&mut visual);
            assert!(
                issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} keyboard expands"
            );
            visual.simulate_keystrokes("space");
            visual.run_until_parked();
            assert!(
                issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} keyboard focus"
            );
            visual.update(|window, cx| {
                stream.update(cx, |stream, cx| stream.focus_composer(window, cx))
            });
            leave(&mut visual);
            assert!(
                !issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} blur"
            );
        }
        let (window, stream, _) = open_controller_stream(cx, "disabled-run");
        cx.update(|cx| {
            cx.set_global(palette);
            cx.refresh_windows();
        });
        stream.update(cx, |stream, cx| {
            start(stream, "polish", cx);
            thinking(stream, "polish", cx);
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for expanded in [false, true] {
            if expanded {
                stream.update(&mut visual, |stream, cx| {
                    stream.run_activity_groups["polish"]
                        .clone()
                        .update(cx, |group, cx| group.reveal(cx))
                });
            }
            let row = visual.debug_bounds("run-activity-toggle").unwrap();
            visual.simulate_mouse_move(row.center(), None, Modifiers::default());
            visual.run_until_parked();
            assert!(!issue296_single_activity::chevron_painted(
                &mut visual,
                "run-activity-chevron"
            ));
        }
    }
}

fn history_body(
    id: &str,
    seq: i64,
    content: &str,
    status: vega_conversation::history::AssistantStatus,
) -> HistoryEntry {
    HistoryEntry::AssistantText {
        seq,
        message_id: id.into(),
        content: content.into(),
        status,
        execution_duration_ms: Some(100),
    }
}

fn summary_for(id: &str, outcome: TaskSummaryOutcome) -> TaskCostSummary {
    TaskCostSummary {
        message_id: id.into(),
        outcome,
        usage: None,
        cost: SummaryCost::Unavailable,
        duration_ms: None,
        tool_count: 0,
        cache_hit_percent: None,
    }
}

#[gpui_kit::test]
async fn issue296_polish_p04_history_multiple_segments_late_summary_and_explicit_error_owner(
    cx: &mut TestAppContext,
) {
    use vega_conversation::history::AssistantStatus;
    let (window, stream, _) = open_controller_stream(cx, "polish-history");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    history_body("old", 1, "First original.", AssistantStatus::Failed),
                    history_body("old", 2, "Second original.", AssistantStatus::Failed),
                    hydration_user(3, "Next request"),
                    history_body(
                        "new",
                        4,
                        "Interrupted original.",
                        AssistantStatus::Interrupted,
                    ),
                ],
                Some(1),
            ),
            cx,
        );
        stream.apply_agent_runtime_error_for(Some("old"), RunFailureKind::ProviderQuota, cx);
        stream.apply_task_summary(summary_for("old", TaskSummaryOutcome::Failed), cx);
        stream.apply_task_summary(summary_for("old", TaskSummaryOutcome::Failed), cx);
        let notices = stream.run_notice_projection(cx);
        assert_eq!(notices.len(), 2);
        let old = notices
            .iter()
            .find(|(_, notice)| notice.message_id == "old")
            .unwrap();
        let user = stream
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::User { .. }))
            .unwrap();
        assert!(*old.0 < user);
        assert_eq!(old.1.text(), RunFailureKind::ProviderQuota.message());
        assert!(
            notices
                .values()
                .any(|notice| notice.message_id == "new" && notice.text() == "已中断")
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    StreamEntry::Assistant {
                        copy,
                        failure: Some(_),
                        ..
                    } => Some(copy.original_text()),
                    _ => None,
                })
                .collect::<String>(),
            "First original.Second original."
        );
        stream.apply_codex_route_error(
            "供应商额度不足；请充值或在设置 → Providers 切换可用供应商后重试",
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds("conversation-controller-error")
            .is_some()
    );
    assert!(visual.debug_bounds("run-notice-old").is_some());
    assert!(visual.debug_bounds("run-notice-new").is_some());
    stream.update(&mut visual, |stream, cx| {
        assert!(stream.controller_runtime_error.is_none());
        assert!(
            stream
                .run_notice_projection(cx)
                .values()
                .any(|notice| notice.message_id == "old"
                    && notice.text() == RunFailureKind::ProviderQuota.message())
        );
        stream.apply_history_page(
            hydration_page(vec![hydration_user(0, "Earlier request")], None),
            cx,
        );
        assert_eq!(stream.run_notice_projection(cx).len(), 2);
        stream.apply_controller_error(cx);
        assert!(
            stream
                .run_notice_projection(cx)
                .values()
                .any(|notice| notice.message_id == "old"
                    && notice.text() == RunFailureKind::ProviderQuota.message())
        );
    });
    visual.run_until_parked();
    assert!(
        visual
            .debug_bounds("conversation-controller-error")
            .is_some()
    );
}

#[gpui_kit::test]
async fn issue296_polish_p04_empty_runtime_failure_and_summary_have_one_notice(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "polish-empty");
    stream.update(cx, |stream, cx| {
        start(stream, "empty", cx);
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("empty".into()),
                error: Arc::new(std::io::Error::other("PRIVATE_RUNTIME_SENTINEL").into()),
                execution_duration_ms: Some(1),
            },
            cx,
        );
        stream.apply_task_summary(summary_for("empty", TaskSummaryOutcome::Failed), cx);
        let notices = stream.run_notice_projection(cx);
        assert_eq!(notices.len(), 1);
        assert_eq!(
            notices.values().next().unwrap().text(),
            RunFailureKind::Runtime.message()
        );
        assert!(stream.entries.iter().any(|entry| matches!(
            entry,
            StreamEntry::Assistant {
                failure: Some(RunFailureKind::Runtime),
                ..
            }
        )));
        assert_eq!(
            stream.controller_error_message(),
            Some(RunFailureKind::Runtime.message())
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-notice-empty").is_some());
    assert!(
        visual
            .debug_bounds("conversation-controller-error")
            .is_none()
    );
    stream.update(&mut visual, |stream, cx| {
        stream.apply_agent_runtime_error(RunFailureKind::Runtime, cx)
    });
    visual.run_until_parked();
    assert!(
        visual
            .debug_bounds("conversation-controller-error")
            .is_some(),
        "unowned runtime failure remains independent"
    );
}

fn focus_row(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual.debug_bounds(selector).unwrap();
    visual.simulate_keystrokes("tab");
    visual.update(|window, cx| window.blur(cx));
    for _ in 0..40 {
        visual.update(|window, cx| window.focus_next(cx));
        visual.run_until_parked();
        if visual.update(|window, cx| {
            let colors = theme(cx).colors;
            window.painted_quads().iter().any(|quad| {
                quad.bounds == bounds.scale(window.scale_factor())
                    && (quad.background.as_solid() == Some(colors.bg_hover.into())
                        || quad.border_color == colors.accent.into())
            })
        }) {
            return;
        }
    }
    panic!("disclosure did not receive keyboard focus: {selector}");
}

struct DisabledDisclosureHarness {
    expanded: bool,
    focus: FocusHandle,
}
impl Render for DisabledDisclosureHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .id("disabled-disclosure")
            .group("disabled-disclosure")
            .debug_selector(|| "disabled-disclosure".into())
            .h(px(25.))
            .w(px(200.))
            .flex()
            .track_focus(&self.focus)
            .children([
                crate::disclosure::chevron(
                    "disabled-disclosure".into(),
                    "disabled-legacy".into(),
                    &self.focus,
                    self.expanded,
                    false,
                    colors,
                    window,
                ),
                crate::disclosure::inline_chevron(
                    "disabled-disclosure".into(),
                    "disabled-inline".into(),
                    &self.focus,
                    self.expanded,
                    false,
                    colors,
                    window,
                ),
            ])
    }
}

#[gpui_kit::test]
async fn issue296_polish_p02_both_helpers_disabled_even_expanded_hovered_focused(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        cx.update(|cx| cx.set_global(palette));
        let entity = cx.new(|cx| DisabledDisclosureHarness {
            expanded: false,
            focus: cx.focus_handle().tab_stop(true),
        });
        let root = entity.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, _| root)
                .unwrap()
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for expanded in [false, true] {
            entity.update(&mut visual, |this, cx| {
                this.expanded = expanded;
                cx.notify();
            });
            let row = visual.debug_bounds("disabled-disclosure").unwrap();
            visual.simulate_mouse_move(row.center(), None, Modifiers::default());
            visual.run_until_parked();
            for selector in ["disabled-legacy", "disabled-inline"] {
                assert!(!issue296_single_activity::chevron_painted(
                    &mut visual,
                    selector
                ));
            }
            visual.simulate_keystrokes("tab");
            visual.update(|window, cx| {
                let focus = entity.read(cx).focus.clone();
                focus.focus(window, cx);
            });
            visual.run_until_parked();
            for selector in ["disabled-legacy", "disabled-inline"] {
                assert!(!issue296_single_activity::chevron_painted(
                    &mut visual,
                    selector
                ));
            }
        }
    }
}

#[gpui_kit::test]
async fn issue296_polish_p05_notice_moves_tail_without_changing_source_anchor(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "polish-anchor");
    stream.update(cx, |stream, cx| {
        start(stream, "polish", cx);
        tool(stream, "problem", ToolCallStatus::Failed, cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let old = stream.read_with(&visual, |stream, _| {
        *stream.run_notices.keys().next().unwrap()
    });
    let old_notice = visual.debug_bounds("run-notice-polish").unwrap();
    stream.update(&mut visual, |stream, cx| {
        stream.capture_visible_entry_heights();
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "polish".into(),
                delta: "New source body.\n\n".repeat(40),
            },
            cx,
        );
    });
    visual.run_until_parked();
    let notice = visual.debug_bounds("run-notice-polish").unwrap();
    assert!(notice.top() > old_notice.top());
    let (body, anchor) = stream.read_with(&visual, |stream, cx| {
        assert_eq!(stream.run_notices.len(), 1);
        assert!(!stream.run_notices.contains_key(&old));
        let index = *stream.run_notices.keys().next().unwrap();
        assert!(index > old);
        assert_eq!(
            stream.run_notices[&index].text(),
            stream.tool_cards["problem"].read(cx).compact_visible_text()
        );
        (
            index,
            ThreadScrollAnchor {
                identity: Some(stream.entry_identities[index].key.clone()),
                message_id: Some("polish".into()),
                offset_in_item_px: 0.,
                following_tail: false,
            },
        )
    });
    let body_bounds = visual
        .debug_bounds(Box::leak(format!("assistant-body-{body}").into_boxed_str()))
        .unwrap();
    assert!(notice.top() >= body_bounds.bottom());
    stream.update(&mut visual, |stream, cx| {
        let before = stream.entry_identities[body].key.clone();
        stream.apply_task_summary(summary_for("polish", TaskSummaryOutcome::Completed), cx);
        assert_eq!(stream.entry_identities[body].key, before);
        assert!(stream.restore_scroll_anchor(&anchor));
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, _| {
        assert_eq!(stream.run_notices.len(), 1);
        assert!(!stream.run_notices.contains_key(&body));
        assert!(stream.entry_identities.iter().any(|identity| Some(&identity.key) == anchor.identity.as_ref()));
        assert!(matches!(&stream.entries[body], StreamEntry::Assistant { copy, .. } if copy.original_text() == "New source body.\n\n".repeat(40)));
    });
    stream.read_with(&visual, |stream, _| {
        assert_eq!(stream.list.logical_scroll_top().item_ix, body);
        assert_eq!(stream.list.logical_scroll_top().offset_in_item, px(0.));
        assert!(!stream.list.is_following_tail());
    });
}
