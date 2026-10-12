use super::*;

fn begin(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "single".into(),
            seq: 1,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::RunStarted {
            message_id: "single".into(),
            started_at: Instant::now() - Duration::from_secs(12),
        },
        cx,
    );
}

#[gpui_kit::test]
async fn issue296_single_activity_default_shows_inline_operations_and_keeps_body(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "single-default");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "single".into(),
                delta: "Reviewing".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "single".into(),
                delta: "First body.".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("shell", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "single".into(),
                delta: "Second body.".into(),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-activity-toggle").is_some());
    assert!(visual.debug_bounds("run-stage-toggle-0").is_some());
    assert!(visual.debug_bounds("tool-activity-single-row").is_some());
    stream.read_with(&visual, |stream, cx| {
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::RunActivity { .. }))
                .count(),
            1
        );
        assert!((0..stream.entries.len()).all(|index| !stream.process_entry_hidden(index, cx)));
    });
}

#[gpui_kit::test]
async fn issue296_single_activity_label_tracks_current_summary(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "single-label");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "single".into(),
                delta: "Checking configuration".into(),
            },
            cx,
        );
        assert!(
            stream.run_activity_groups["single"]
                .read(cx)
                .test_activity_label(cx)
                .contains("Checking configuration")
        );
    });
}

#[gpui_kit::test]
async fn issue296_single_activity_tool_chevron_uses_right_slot(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "single-chevron");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("standalone", "pwd"),
            },
            cx,
        )
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let row = visual
        .debug_bounds("tool-activity-single-row")
        .expect("row");
    let chevron = visual
        .debug_bounds("tool-activity-single-row-chevron")
        .expect("chevron slot");
    assert_eq!(chevron.size.width, px(12.));
    let title = visual
        .debug_bounds("tool-activity-single-row-title")
        .unwrap();
    assert_eq!(chevron.left() - title.right(), px(6.));
    assert!(chevron.right() < row.right());
}

fn finish_tool(id: &str) -> ConversationEvent {
    ConversationEvent::ToolCallFinished {
        call_id: id.into(),
        result: ToolResult {
            status: ToolCallStatus::Success,
            output: "ok".into(),
            reused: false,
            exit_code: Some(0),
            duration_ms: Some(10),
            truncated: Some(false),
            invalid: None,
        },
    }
}

#[gpui_kit::test]
async fn issue296_single_activity_tracks_real_parallel_states_and_discards_old_summary(
    cx: &mut TestAppContext,
) {
    use crate::icons::Icon;
    let (_, stream, _) = open_controller_stream(cx, "single-state");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        let group = stream.run_activity_groups["single"].clone();
        let start = group.read(cx).test_clock().0;
        assert!(matches!(group.read(cx).test_activity_presentation(cx), (label, Icon::Bot, true) if label == "Thinking"));
        stream.apply_event(ConversationEvent::SummaryDelta { message_id: "single".into(), delta: "Prior summary".into() }, cx);
        for call in [bash_call("shell", "pwd"), ToolCall { id: "read".into(), tool: "read".into(), input_json: r#"{"path":"safe.txt"}"#.into() }, bash_call("queued", "pwd")] {
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        }
        assert!(matches!(group.read(cx).test_activity_presentation(cx), (label, Icon::Hand, false) if label == "等待确认"));
        for id in ["shell", "read", "queued"] {
            stream.apply_event(ConversationEvent::ToolCallApproved { call_id: id.into(), approval: Approval::Once }, cx);
        }
        assert_eq!(group.read(cx).test_activity_label(cx), "准备调用工具");
        for id in ["shell", "read"] {
            stream.apply_event(ConversationEvent::ToolCallRunning { call_id: id.into() }, cx);
        }
        assert!(matches!(group.read(cx).test_activity_presentation(cx), (label, Icon::Summary, false) if label == "正在运行命令、读取文件"));
        stream.apply_event(ConversationEvent::ToolCallProposed { call: bash_call("pending", "pwd") }, cx);
        assert_eq!(group.read(cx).test_activity_label(cx), "正在运行命令、读取文件");
        stream.apply_event(finish_tool("shell"), cx);
        assert!(matches!(group.read(cx).test_activity_presentation(cx), (label, Icon::Document, false) if label == "正在读取文件"));
        stream.apply_event(finish_tool("read"), cx);
        assert_eq!(group.read(cx).test_activity_label(cx), "等待确认");
        for id in ["pending", "queued"] { stream.apply_event(finish_tool(id), cx); }
        stream.apply_event(ConversationEvent::ThinkingDelta { message_id: "single".into(), delta: "New thought without summary".into() }, cx);
        assert_eq!(group.read(cx).test_activity_label(cx), "Thinking");
        stream.apply_event(ConversationEvent::SummaryDelta { message_id: "single".into(), delta: "Latest summary".into() }, cx);
        assert_eq!(group.read(cx).test_activity_label(cx), "Latest summary");
        assert_eq!(group.read(cx).test_clock().0, start);
    });
}

#[gpui_kit::test]
async fn issue296_single_activity_shimmer_is_exclusive_to_header(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "single-shimmer");
    cx.update(|cx| {
        cx.set_reduce_motion(false);
        cx.set_global(shimmer::PaintProbe::default());
    });
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "single".into(),
                delta: "Current summary".into(),
            },
            cx,
        );
    });
    open_run_details(window, cx);
    for phase in ["thinking", "tool", "waiting", "terminal"] {
        stream.update(cx, |stream, cx| match phase {
            "tool" => {
                stream.apply_event(
                    ConversationEvent::ToolCallProposed {
                        call: bash_call("active", "pwd"),
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::ToolCallApproved {
                        call_id: "active".into(),
                        approval: Approval::Once,
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::ToolCallRunning {
                        call_id: "active".into(),
                    },
                    cx,
                );
            }
            "waiting" => stream.run_activity_groups["single"]
                .clone()
                .update(cx, |group, cx| group.set_waiting(true, cx)),
            "terminal" => stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "single".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                    execution_duration_ms: Some(12000),
                },
                cx,
            ),
            _ => {}
        });
        cx.run_until_parked();
        cx.update(|cx| cx.global::<shimmer::PaintProbe>().0.borrow_mut().clear());
        window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            let samples = cx.global::<shimmer::PaintProbe>().0.borrow();
            assert_eq!(
                samples.iter().filter(|sample| sample.moving).count(),
                usize::from(phase == "thinking"),
                "{phase}"
            );
        });
        stream.read_with(cx, |stream, cx| {
            assert!(
                stream.run_activity_groups["single"]
                    .read(cx)
                    .children()
                    .iter()
                    .all(|child| match child {
                        RunActivityChild::Thinking(card) => card.read(cx).shimmer_epoch.is_none(),
                        _ => true,
                    })
            )
        });
    }
}

pub(super) fn chevron_painted(visual: &mut gpui_kit::VisualTestContext, selector: &str) -> bool {
    let marker = visual
        .debug_bounds(Box::leak(format!("{selector}-paint").into_boxed_str()))
        .expect("chevron paint marker");
    visual.update(|window, _| {
        window.painted_quads().iter().any(|quad| {
            quad.bounds == marker.scale(window.scale_factor())
                && quad.background.as_solid() == Some(gpui_kit::rgba(0x12fe34ff).into())
        })
    })
}

#[gpui_kit::test]
async fn issue296_single_activity_history_is_ordered_and_never_crosses_runs(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "single-order");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        for (thinking, body, tool) in [
            ("first", "first body", "one"),
            ("second", "second body", "two"),
        ] {
            stream.apply_event(
                ConversationEvent::ThinkingDelta {
                    message_id: "single".into(),
                    delta: thinking.into(),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "single".into(),
                    delta: body.into(),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(tool, "pwd"),
                },
                cx,
            );
            stream.apply_event(finish_tool(tool), cx);
        }
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "single".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "next".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "next".into(),
                delta: "next run".into(),
            },
            cx,
        );
        let group = &stream.run_activity_groups["single"];
        let history = group
            .read(cx)
            .children()
            .into_iter()
            .map(|detail| match detail {
                RunActivityChild::Thinking(card) => card.read(cx).text.clone(),
                RunActivityChild::Tool(_) => "tool".into(),
                _ => panic!("unexpected detail"),
            })
            .collect::<Vec<_>>();
        assert_eq!(history, ["first", "tool", "second", "tool"]);
        assert_eq!(
            stream.run_activity_groups["next"].read(cx).children().len(),
            1
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::RunActivity { .. }))
                .count(),
            2
        );
        assert!((0..stream.entries.len()).all(|index| !stream.process_entry_hidden(index, cx)));
    });
}

fn final_history() -> HistoryPage {
    use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata};
    let mut entries = Vec::new();
    let final_text = "Final answer.\n\n".repeat(100);
    let mut bytes = 0;
    for index in 0..12 {
        let content = format!("Step {index}: review the synthetic result.\n\n");
        bytes += content.len();
        entries.push(HistoryEntry::AssistantText {
            seq: index * 2 + 1,
            message_id: "restored".into(),
            content,
            status: vega_conversation::history::AssistantStatus::Done,
            execution_duration_ms: None,
        });
        entries.push(HistoryEntry::Tool {
            seq: index * 2 + 2,
            message_id: "restored".into(),
            call_id: format!("tool-{index}"),
            status: ToolCallStatus::Success,
            approval: Some(Approval::Once),
            input: Some(ToolCardInputProjection::ReadOnly {
                tool: ReadOnlyToolKind::Read,
                permission_path: None,
            }),
            result: Some(ToolCardResultProjection::ReadOnly {
                status: ToolCallStatus::Success,
                output: "safe result".into(),
                reused: false,
            }),
        });
    }
    entries.push(HistoryEntry::AssistantText {
        seq: 25,
        message_id: "restored".into(),
        content: final_text.clone(),
        status: vega_conversation::history::AssistantStatus::Done,
        execution_duration_ms: None,
    });
    for (seq, id, start, end, kind) in [
        (
            26,
            "commentary",
            0,
            bytes,
            AssistantResponseKind::Commentary,
        ),
        (
            27,
            "final",
            bytes,
            bytes + final_text.len(),
            AssistantResponseKind::Final,
        ),
    ] {
        entries.push(HistoryEntry::AssistantResponseMetadata {
            seq,
            message_id: "restored".into(),
            response: AssistantResponseMetadata {
                response_id: id.into(),
                text_start_bytes: start,
                text_end_bytes: end,
                kind,
            },
        });
    }
    hydration_page(entries, None)
}

#[gpui_kit::test]
async fn issue296_single_activity_hidden_body_anchor_restores_inside_its_only_details(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "single-anchor");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(final_history(), cx)
    });
    cx.run_until_parked();
    let (anchor, expected_detail) = stream.read_with(cx, |stream, cx| {
        let group = &stream.run_activity_groups["restored"];
        let details = stream.run_details(group, cx);
        let (detail_index, (source, _)) = details.iter().enumerate().find(|(_, (_, detail))| match detail {
            phase::RunDetail::Assistant(index) => matches!(&stream.entries[*index], StreamEntry::Assistant { copy, .. } if copy.original_text().starts_with("Step 5:")),
            _ => false,
        }).expect("target commentary");
        assert!(stream.process_entry_hidden(*source, cx));
        (ThreadScrollAnchor { identity: Some(stream.entry_identities[*source].key.clone()), message_id: Some("restored".into()), offset_in_item_px: 12., following_tail: false }, detail_index)
    });
    stream.update(cx, |stream, cx| {
        assert!(stream.replace_history_window_with_anchor(
            final_history(),
            "restored",
            Some(&anchor),
            cx
        ))
    });
    cx.run_until_parked();
    let scroll = stream.read_with(cx, |stream, cx| {
        let group = stream.run_activity_groups["restored"].read(cx);
        assert!(group.expanded());
        assert_eq!(stream.list.logical_scroll_top().item_ix, 0);
        assert!(!stream.list.is_following_tail());
        group.test_scroll_handle()
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-activity-content").is_some());
    assert_eq!(scroll.top_item(), expected_detail);
    assert_eq!(scroll.logical_scroll_top().1, px(-12.));
    assert!(scroll.offset().y < px(0.));
    assert!(scroll.bounds().size.height <= px(Layout::TOOL_GROUP_MAX_HEIGHT));
    stream.update(&mut visual, |stream, cx| {
        assert!(stream.reveal_loaded_message("restored", cx))
    });
    visual.run_until_parked();
    assert_eq!(scroll.top_item(), 0);
    assert_eq!(scroll.offset().y, px(0.));
}

#[gpui_kit::test]
async fn issue296_single_activity_visible_artifact_anchor_stays_outside_details(
    cx: &mut TestAppContext,
) {
    let repo = tempfile::tempdir().expect("artifact workspace");
    let workspace = std::sync::Arc::new(
        vega_conversation::GitWorkspaceService::new(repo.path()).expect("artifact service root"),
    );
    let service = vega_conversation::ArtifactService::new(workspace, "p".into(), "t".into(), 1)
        .expect("artifact route");
    let call = ToolCall {
        id: "c".into(),
        tool: "write".into(),
        input_json: serde_json::json!({"audit_version": "write_edit_v1", "tool": "write", "path": "artifact.txt", "content_bytes": 1, "fingerprint_v1": "a".repeat(64)}).to_string(),
    };
    let projection = service.capture(&call, &ToolResult {
        status: ToolCallStatus::Success,
        output: serde_json::json!({"path": "artifact.txt", "bytes_written": 1, "checkpoint_ref": "preimage-v1/id-70/id-74/id-63"}).to_string(),
        reused: false, exit_code: None, duration_ms: None, truncated: Some(false), invalid: None,
    }, tokio_util::sync::CancellationToken::new()).await.expect("safe artifact capture").expect("artifact metadata");
    let card = cx
        .new(|cx| crate::artifact_card::ArtifactCard::new("t".into(), "p".into(), projection, cx));
    let (window, stream, _) = open_controller_stream(cx, "single-artifact-anchor");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        assert!(stream.apply_artifact_card("c", card.clone(), cx));
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "single".into(),
                delta: "Body before the next operation.\n\n".repeat(30),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("later", "pwd"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "single".into(),
                delta: "Body after the operation.\n\n".repeat(30),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let (artifact_source, artifact_anchor, tool_anchor) = stream.read_with(cx, |stream, cx| {
        let group = &stream.run_activity_groups["single"];
        let mut segments =
            stream
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| match entry {
                    StreamEntry::RunActivitySegment { segment, .. } => Some((index, *segment)),
                    _ => None,
                });
        let (artifact_source, artifact_segment) = segments.next().expect("artifact segment");
        let (tool_source, _) = segments.next().expect("tool-only segment");
        assert!(
            group
                .read(cx)
                .segment_children(artifact_segment)
                .iter()
                .any(|child| matches!(child, RunActivityChild::Artifact(_)))
        );
        let anchor = |source: usize| ThreadScrollAnchor {
            identity: Some(stream.entry_identities[source].key.clone()),
            message_id: Some("single".into()),
            offset_in_item_px: 12.,
            following_tail: false,
        };
        (
            artifact_source,
            anchor(artifact_source),
            anchor(tool_source),
        )
    });
    stream.update(cx, |stream, cx| {
        assert!(!stream.restore_process_anchor(&artifact_anchor, cx));
        assert!(stream.restore_scroll_anchor(&artifact_anchor));
        assert!(!stream.run_activity_groups["single"].read(cx).expanded());
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, _| {
        let offset = stream.list.logical_scroll_top();
        assert_eq!(offset.item_ix, artifact_source);
        assert_eq!(offset.offset_in_item, px(12.));
        assert!(!stream.list.is_following_tail());
    });
    stream.update(cx, |stream, cx| {
        assert!(!stream.restore_process_anchor(&tool_anchor, cx));
        assert!(stream.restore_scroll_anchor(&tool_anchor));
        assert!(!stream.run_activity_groups["single"].read(cx).expanded());
        stream.resume_tail(cx);
        confirm_test_final(stream, "single", cx);
    });
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        assert!(stream.restore_process_anchor(&tool_anchor, cx));
        assert!(stream.run_activity_groups["single"].read(cx).expanded());
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("run-activity-content").is_some());
}

#[gpui_kit::test]
async fn issue296_single_activity_narrow_summary_preserves_clock_and_right_slot(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "single-narrow");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        stream.apply_event(
            ConversationEvent::SummaryDelta {
                message_id: "single".into(),
                delta: "正在核对非常长的摘要内容与状态变化的可读性。".repeat(30),
            },
            cx,
        );
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        visual.update(|_, cx| {
            cx.set_global(palette);
            cx.set_global(shimmer::PaintProbe::default());
            cx.refresh_windows();
        });
        visual.simulate_resize(gpui_kit::size(px(480.), px(800.)));
        visual.run_until_parked();
        let row = visual.debug_bounds("run-activity-toggle").expect("header");
        let slot = visual.debug_bounds("run-activity-chevron").expect("slot");
        let title = visual
            .debug_bounds("run-activity-status")
            .expect("clock title");
        let stage = visual.debug_bounds("run-stage-toggle-0").expect("stage");
        let stage_title = visual.debug_bounds("run-stage-title-0").expect("summary");
        let stage_slot = visual
            .debug_bounds("run-stage-chevron-0")
            .expect("stage slot");
        assert_eq!(row.size.height, px(23.));
        assert_eq!(slot.size.width, px(12.));
        assert!(title.right() <= slot.left());
        assert_eq!(stage_slot.left() - stage_title.right(), px(6.));
        assert!(stage_slot.right() <= stage.right());
        assert!(visual.debug_bounds("run-activity-icon").is_none());
        visual.update(|_, cx| {
            assert!(
                cx.global::<shimmer::PaintProbe>()
                    .0
                    .borrow()
                    .last()
                    .expect("painted summary")
                    .text
                    .ends_with('…')
            )
        });
    }
}

#[gpui_kit::test]
async fn issue296_single_activity_mcp_names_are_safe_bounded_and_deduplicated(
    cx: &mut TestAppContext,
) {
    use vega_conversation::types::McpCallIdentity;
    let (_, stream, _) = open_controller_stream(cx, "single-safe-tools");
    stream.update(cx, |stream, cx| {
        begin(stream, cx);
        for (id, name) in [("one", "lookup"), ("two", "query"), ("duplicate", "lookup"), ("three", "inspect")] {
            let identity = McpCallIdentity {
                server_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(), config_revision: 3,
                exact_tool_name: name.into(), arguments_bytes: 100,
                arguments_sha256: "b".repeat(64), argument_preview: "query: string".into(),
            };
            let call = ToolCall {
                id: id.into(), tool: identity.alias(),
                input_json: serde_json::json!({"server_id": identity.server_id, "config_revision": identity.config_revision, "tool": identity.exact_tool_name, "arguments_bytes": identity.arguments_bytes, "arguments_sha256": identity.arguments_sha256, "argument_preview": identity.argument_preview}).to_string(),
            };
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
            stream.apply_event(ConversationEvent::ToolCallApproved { call_id: id.into(), approval: Approval::Once }, cx);
            stream.apply_event(ConversationEvent::ToolCallRunning { call_id: id.into() }, cx);
        }
        let label = stream.run_activity_groups["single"].read(cx).test_activity_label(cx);
        assert_eq!(label, "正在调用 MCP · lookup、调用 MCP · query等 3 项");
        assert!(!label.contains("01ARZ") && !label.contains("query: string") && !label.contains(&"b".repeat(64)));
    });
}

#[gpui_kit::test]
async fn issue296_single_activity_terminal_icons_never_suggest_refresh(cx: &mut TestAppContext) {
    use crate::icons::Icon;
    for status in [
        RunActivityStatus::Completed,
        RunActivityStatus::Failed,
        RunActivityStatus::Interrupted,
    ] {
        let (_, stream, _) = open_controller_stream(cx, "single-terminal-icon");
        stream.update(cx, |stream, cx| {
            begin(stream, cx);
            let group = stream.run_activity_groups["single"].clone();
            group.update(cx, |group, cx| group.complete(status, Some(12_000), cx));
            let (_, icon, shimmer) = group.read(cx).test_activity_presentation(cx);
            assert!(!shimmer);
            assert!(matches!(
                (status, icon),
                (RunActivityStatus::Completed, Icon::Check)
                    | (RunActivityStatus::Failed, Icon::Warning)
                    | (RunActivityStatus::Interrupted, Icon::Close)
            ));
        });
    }
}
