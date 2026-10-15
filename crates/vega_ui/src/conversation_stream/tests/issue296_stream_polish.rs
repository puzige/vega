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
        ConversationEvent::ThinkingDelta {
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
    let (icon, title) = ("thinking-icon", "thinking-title");
    assert_eq!(visual.debug_bounds(icon).unwrap().left(), tool_icon.left());
    assert_eq!(
        visual.debug_bounds(title).unwrap().left(),
        tool_title.left()
    );
    assert_eq!(
        visual.debug_bounds(title).unwrap().left() - tool.left(),
        px(22.)
    );
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
async fn issue296_polish_p03_tool_issues_stay_in_process_without_tail_notice(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "polish-notice");
    stream.update(cx, |stream, cx| start(stream, "polish", cx));
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for (id, status, state) in [
        (
            "failed",
            ToolCallStatus::Failed,
            crate::tool_card::ToolActivityState::Failed,
        ),
        (
            "rejected",
            ToolCallStatus::Rejected,
            crate::tool_card::ToolActivityState::Rejected,
        ),
        (
            "cancelled",
            ToolCallStatus::Cancelled,
            crate::tool_card::ToolActivityState::Cancelled,
        ),
    ] {
        stream.update(&mut visual, |stream, cx| tool(stream, id, status, cx));
        visual.run_until_parked();
        assert!(visual.debug_bounds("run-notice-polish").is_none());
        assert!(visual.debug_bounds("run-activity-exception").is_none());
        stream.read_with(&visual, |stream, cx| {
            assert!(stream.run_notices.is_empty());
            assert_eq!(
                stream.run_activity_groups["polish"].read(cx).status(),
                RunActivityStatus::Running
            );
            assert_eq!(stream.tool_cards[id].read(cx).activity_state(), state);
            assert!(
                !stream.tool_cards[id]
                    .read(cx)
                    .compact_visible_text()
                    .contains("synthetic output")
            );
        });
    }
    stream.update(&mut visual, |stream, cx| {
        thinking(stream, "polish", cx);
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "polish".into(),
                delta: "Continuing after tool results.".into(),
            },
            cx,
        );
        tool(stream, "success", ToolCallStatus::Success, cx);
        confirm_test_final(stream, "polish", cx);
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "polish".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: Some(100),
            },
            cx,
        );
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("run-notice-polish").is_none());
    assert!(visual.debug_bounds("run-activity-content").is_none());
    click(&mut visual, "run-activity-toggle");
    assert!(visual.debug_bounds("run-activity-content").is_some());
    assert!(visual.debug_bounds("run-notice-polish").is_none());
    stream.update(&mut visual, |stream, cx| {
        assert!(stream.run_notices.is_empty());
        assert_eq!(
            stream.run_activity_groups["polish"].read(cx).status(),
            RunActivityStatus::Completed
        );
        for (id, state) in [
            ("failed", crate::tool_card::ToolActivityState::Failed),
            ("rejected", crate::tool_card::ToolActivityState::Rejected),
            ("cancelled", crate::tool_card::ToolActivityState::Cancelled),
            ("success", crate::tool_card::ToolActivityState::Success),
        ] {
            let card = &stream.tool_cards[id];
            assert_eq!(card.read(cx).activity_state(), state);
            card.update(cx, |card, cx| card.set_expanded(true, cx));
            assert!(card.read(cx).visible_text().contains("synthetic output"));
        }
    });
    visual.run_until_parked();
    click(&mut visual, "run-activity-toggle");
    assert!(visual.debug_bounds("run-activity-content").is_none());
    assert!(visual.debug_bounds("run-notice-polish").is_none());
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
                "group" => ("run-stage-toggle-0", "run-stage-chevron-0"),
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
            assert_eq!(
                visual.debug_bounds(arrow).unwrap(),
                bounds,
                "hover preserves geometry"
            );
            click(&mut visual, toggle);
            leave(&mut visual);
            assert!(
                issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} expanded"
            );
            let expanded_bounds = visual.debug_bounds(arrow).unwrap();
            assert_eq!(expanded_bounds.size, bounds.size);
            assert_eq!(expanded_bounds.origin.y, bounds.origin.y);
            if target == "tool" {
                let title = visual
                    .debug_bounds("tool-activity-single-row-title")
                    .unwrap();
                assert_eq!(
                    expanded_bounds.left() - title.right(),
                    px(Layout::CONVERSATION_INLINE_GAP)
                );
                assert!(expanded_bounds.right() <= visual.debug_bounds(toggle).unwrap().right());
                stream.read_with(&visual, |stream, cx| {
                    assert!(
                        stream.tool_cards["one"]
                            .read(cx)
                            .compact_visible_text()
                            .starts_with("Ran command")
                    );
                });
            } else {
                assert_eq!(expanded_bounds, bounds);
            }
            click(&mut visual, toggle);
            leave(&mut visual);
            assert!(
                !issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} collapsed again"
            );
            assert_eq!(
                visual.debug_bounds(arrow).unwrap(),
                bounds,
                "collapse restores original title geometry"
            );
            focus_row(&mut visual, toggle);
            assert_eq!(
                visual.debug_bounds(arrow).unwrap(),
                bounds,
                "focus preserves geometry"
            );
            visual.simulate_keystrokes("enter");
            visual.run_until_parked();
            leave(&mut visual);
            assert!(
                issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} keyboard expands"
            );
            assert_eq!(visual.debug_bounds(arrow).unwrap(), expanded_bounds);
            visual.simulate_keystrokes("space");
            visual.run_until_parked();
            assert!(
                issue296_single_activity::chevron_painted(&mut visual, arrow),
                "{target} keyboard focus"
            );
            assert_eq!(visual.debug_bounds(arrow).unwrap(), bounds);
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
async fn issue296_terminal_notices_history_requires_run_terminal_evidence(cx: &mut TestAppContext) {
    use vega_conversation::history::AssistantStatus;
    for (name, status, expected) in [
        ("tool-only", None, None),
        ("done", Some(AssistantStatus::Done), None),
        (
            "failed",
            Some(AssistantStatus::Failed),
            Some(RunFailureKind::Persisted.message()),
        ),
        ("interrupted", Some(AssistantStatus::Interrupted), None),
    ] {
        let (window, stream, _) = open_controller_stream(cx, name);
        stream.update(cx, |stream, cx| {
            let mut entries = [
                ToolCallStatus::Failed,
                ToolCallStatus::Rejected,
                ToolCallStatus::Cancelled,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, status)| HistoryEntry::Tool {
                seq: index as i64 + 1,
                message_id: "history-notice".into(),
                call_id: format!("tool-{index}"),
                status,
                approval: Some(Approval::Once),
                input: Some(ToolCardInputProjection::ReadOnly {
                    tool: ReadOnlyToolKind::Read,
                    display_target: None,
                    permission_path: None,
                }),
                result: Some(ToolCardResultProjection::ReadOnly {
                    status,
                    output: format!("Synthetic tool output {index}."),
                    reused: false,
                }),
            })
            .collect::<Vec<_>>();
            if let Some(status) = status {
                entries.push(history_body(
                    "history-notice",
                    4,
                    "Original recovered body.",
                    status,
                ));
            }
            stream.apply_history_page(hydration_page(entries, None), cx);
        });
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert_eq!(
            visual.debug_bounds("run-notice-history-notice").is_some(),
            expected.is_some()
        );
        stream.update(&mut visual, |stream, cx| {
            let notices = stream.run_notice_projection(cx);
            assert_eq!(notices.len(), usize::from(expected.is_some()));
            if let Some(expected) = &expected {
                let notice = notices.values().next().unwrap();
                assert_eq!(notice.message_id, "history-notice");
                assert_eq!(&notice.text(), expected);
            }
            for (index, state) in [crate::tool_card::ToolActivityState::Failed, crate::tool_card::ToolActivityState::Rejected, crate::tool_card::ToolActivityState::Cancelled].into_iter().enumerate() {
                let card = &stream.tool_cards[&format!("tool-{index}")];
                assert_eq!(card.read(cx).activity_state(), state);
                card.update(cx, |card, cx| card.set_expanded(true, cx));
                assert!(card.read(cx).visible_text().contains(&format!("Synthetic tool output {index}.")));
            }
            if status.is_some() {
                assert!(stream.entries.iter().any(|entry| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text() == "Original recovered body.")));
            }
        });
        visual.run_until_parked();
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
        assert_eq!(notices.len(), 1);
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
        assert!(!notices.values().any(|notice| notice.message_id == "new"));
        assert_eq!(stream.run_activity_groups["new"].read(cx).status(), RunActivityStatus::Interrupted);
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
            "Provider quota exhausted. Add credit or switch providers in Settings → Providers, then retry.",
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

pub(super) fn focus_row(visual: &mut VisualTestContext, selector: &'static str) {
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
async fn issue296_polish_p05_no_tool_notice_preserves_source_anchor(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "polish-anchor");
    stream.update(cx, |stream, cx| {
        start(stream, "polish", cx);
        tool(stream, "problem", ToolCallStatus::Failed, cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let old = stream.read_with(&visual, |stream, _| {
        assert!(stream.run_notices.is_empty());
        stream.entries.len() - 1
    });
    assert!(visual.debug_bounds("run-notice-polish").is_none());
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
    assert!(visual.debug_bounds("run-notice-polish").is_none());
    let (body, anchor) = stream.read_with(&visual, |stream, cx| {
        assert!(stream.run_notices.is_empty());
        let index = stream.entries.len() - 1;
        assert!(index > old);
        assert_eq!(
            stream.tool_cards["problem"].read(cx).activity_state(),
            crate::tool_card::ToolActivityState::Failed
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
    assert!(
        visual
            .debug_bounds(Box::leak(format!("assistant-body-{body}").into_boxed_str()))
            .is_some()
    );
    stream.update(&mut visual, |stream, cx| {
        let before = stream.entry_identities[body].key.clone();
        stream.apply_task_summary(summary_for("polish", TaskSummaryOutcome::Completed), cx);
        assert_eq!(stream.entry_identities[body].key, before);
        assert!(stream.restore_scroll_anchor(&anchor));
    });
    visual.run_until_parked();
    stream.read_with(&visual, |stream, _| {
        assert!(stream.run_notices.is_empty());
        assert!(stream.entry_identities.iter().any(|identity| Some(&identity.key) == anchor.identity.as_ref()));
        assert!(matches!(&stream.entries[body], StreamEntry::Assistant { copy, .. } if copy.original_text() == "New source body.\n\n".repeat(40)));
    });
    stream.read_with(&visual, |stream, _| {
        assert_eq!(stream.list.logical_scroll_top().item_ix, body);
        assert_eq!(stream.list.logical_scroll_top().offset_in_item, px(0.));
        assert!(!stream.list.is_following_tail());
    });
}

#[gpui_kit::test]
async fn issue75_resume_refresh_neutral_cancellation_does_not_interrupt_run_or_hide_real_failure(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "resume-refresh");
    stream.update(cx,|stream,cx| {
        start(stream,"polish",cx);
        let call_id="resume-superseded".to_string();
        stream.apply_event(ConversationEvent::ToolCallProposed{call:vega_conversation::types::ToolCall {
            id:call_id.clone(),tool:"ComputerScreenshot".into(),
            input_json:serde_json::json!({"kind":"computer_use","version":1,"action":"screenshot","argument_bytes":2,"argument_digest":"a".repeat(64)}).to_string(),
        }},cx);
        stream.apply_event(ConversationEvent::ToolCallApproved{call_id:call_id.clone(),approval:vega_conversation::types::Approval::Once},cx);
        stream.apply_event(ConversationEvent::ToolCallRunning{call_id:call_id.clone()},cx);
        stream.apply_event(ConversationEvent::ToolCallFinished{call_id:call_id.clone(),result:vega_conversation::types::ToolResult {
            status:ToolCallStatus::Cancelled,output:vega_conversation::types::SUPERSEDED_CONTROL_OUTPUT.into(),reused:false,exit_code:None,duration_ms:None,truncated:None,invalid:None,
        }},cx);
        assert_eq!(stream.tool_cards[&call_id].read(cx).activity_state(),crate::tool_card::ToolActivityState::Superseded);
        assert_eq!(stream.run_activity_groups["polish"].read(cx).status(),RunActivityStatus::Running);
        tool(stream,"real-failure",ToolCallStatus::Failed,cx);
        assert_eq!(stream.tool_cards["real-failure"].read(cx).activity_state(),crate::tool_card::ToolActivityState::Failed);
        stream.apply_event(ConversationEvent::TextDelta{message_id:"polish".into(),delta:"Replanned from fresh targets.".into()},cx);
        confirm_test_final(stream,"polish",cx);
        stream.apply_event(ConversationEvent::MessageFinished{message_id:"polish".into(),stop_reason:vega_conversation::types::ConversationStopReason::End,execution_duration_ms:Some(100)},cx);
        assert_eq!(stream.run_activity_groups["polish"].read(cx).status(),RunActivityStatus::Completed);
        assert!(stream.run_notices.is_empty());
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-notice-polish").is_none());
    assert!(visual.debug_bounds("run-activity-exception").is_none());
    click(&mut visual, "run-activity-toggle");
    stream.read_with(&visual, |stream, cx| {
        assert!(
            stream.tool_cards["resume-superseded"]
                .read(cx)
                .visible_text()
                .contains("Not run after takeover")
        );
        assert_eq!(
            stream.tool_cards["real-failure"].read(cx).activity_state(),
            crate::tool_card::ToolActivityState::Failed
        );
    });
}
