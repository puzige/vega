use super::*;
use crate::icons::Icon;
use crate::tool_card::ToolActivityState;
use gpui_kit::{Bounds, Pixels, Quad};
use vega_conversation::types::{
    InvalidToolCode, InvalidToolKind, InvalidToolProjection, McpCallIdentity,
};

fn read_call(id: &str, tool: &str, raw_input: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        tool: tool.into(),
        input_json: raw_input.into(),
    }
}

fn finish(
    stream: &mut ConversationStream,
    id: &str,
    status: ToolCallStatus,
    output: &str,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    cx: &mut Context<ConversationStream>,
) {
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
                output: output.into(),
                reused: false,
                exit_code,
                duration_ms,
                truncated: Some(false),
                invalid: None,
            },
        },
        cx,
    );
}

fn group(stream: &ConversationStream) -> Entity<ToolActivityGroup> {
    stream
        .entries
        .iter()
        .find_map(|entry| match entry {
            StreamEntry::ToolGroup { group } => Some(group.clone()),
            _ => None,
        })
        .expect("tool activity group")
}

fn entry_shapes(stream: &ConversationStream, cx: &App) -> Vec<String> {
    stream
        .entries
        .iter()
        .map(|entry| match entry {
            StreamEntry::Assistant { .. } => "assistant".to_string(),
            StreamEntry::Tool { .. } => "tool:1".to_string(),
            StreamEntry::ToolGroup { group } => format!("group:{}", group.read(cx).len()),
            StreamEntry::Artifact { .. } => "artifact".to_string(),
            StreamEntry::Permission { .. } => "permission".to_string(),
            StreamEntry::Plan { .. } => "plan".to_string(),
            StreamEntry::Summary { .. } => "summary".to_string(),
            StreamEntry::SkillActivation { .. } => "skill".to_string(),
            StreamEntry::Thinking { .. } => "thinking".to_string(),
            StreamEntry::User { .. } => "user".to_string(),
            StreamEntry::UserImages { .. } => "user-images".to_string(),
        })
        .collect()
}

fn activity_texts(stream: &ConversationStream, cx: &App) -> Vec<String> {
    stream
        .entries
        .iter()
        .filter_map(|entry| match entry {
            StreamEntry::Tool { card } => Some(card.read(cx).visible_text()),
            StreamEntry::ToolGroup { group } => Some(group.read(cx).visible_text(cx)),
            _ => None,
        })
        .collect()
}

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    cx.run_until_parked();
    gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing selector {selector}"))
}

fn body_text_width(
    window: WindowHandle<StreamHarness>,
    text: String,
    cx: &mut TestAppContext,
) -> Pixels {
    window
        .update(cx, |_, window, cx| {
            let colors = theme(cx).colors;
            let font = window.text_style().font();
            window
                .text_system()
                .shape_line(
                    text.clone().into(),
                    px(Typography::BODY),
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
        .expect("measure body text")
}

fn logical_quad_matches(quad: &Quad, target: Bounds<Pixels>, scale: f32) -> bool {
    let tolerance = 0.2;
    [
        (quad.bounds.left().as_f32() / scale, target.left().as_f32()),
        (quad.bounds.top().as_f32() / scale, target.top().as_f32()),
        (
            quad.bounds.right().as_f32() / scale,
            target.right().as_f32(),
        ),
        (
            quad.bounds.bottom().as_f32() / scale,
            target.bottom().as_f32(),
        ),
    ]
    .into_iter()
    .all(|(actual, expected)| (actual - expected).abs() <= tolerance)
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let target = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing clickable selector {selector}"));
    visual.simulate_click(target.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
}

fn approve_and_run(
    stream: &mut ConversationStream,
    call_id: &str,
    cx: &mut Context<ConversationStream>,
) {
    stream.apply_event(
        ConversationEvent::ToolCallApproved {
            call_id: call_id.into(),
            approval: Approval::Once,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::ToolCallRunning {
            call_id: call_id.into(),
        },
        cx,
    );
}

#[gpui_kit::test]
async fn issue70_e70_live_bash_elapsed_uses_running_clock_and_terminal_duration(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue70-live-elapsed");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("elapsed", "printf 'tick'"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: "elapsed".into(),
                approval: Approval::Once,
            },
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.tool_cards["elapsed"]
            .read(cx)
            .visible_text()),
        "正在运行 printf 'tick'",
        "approval alone must not estimate a start time"
    );
    cx.run_until_parked();
    let frames_before_running = stream.read_with(cx, |stream, _| {
        stream.counters.frames.load(Ordering::Relaxed)
    });

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallRunning {
                call_id: "elapsed".into(),
            },
            cx,
        );
    });
    cx.run_until_parked();
    let frames_after_running = stream.read_with(cx, |stream, _| {
        stream.counters.frames.load(Ordering::Relaxed)
    });
    assert!(
        frames_after_running > frames_before_running,
        "Running must notify the mounted stream so the 0-second summary paints immediately"
    );
    let _mounted_zero_second_row = bounds(window, "tool-activity-single-row", cx);
    stream.read_with(cx, |stream, cx| {
        let card = stream.tool_cards["elapsed"].read(cx);
        assert_eq!(card.visible_text(), "正在运行 printf 'tick' · 0 秒");
        assert!(card.live_elapsed_active());
        let colors = theme(cx).colors;
        assert!(matches!(card.leading_icon(), Icon::Terminal));
        assert_eq!(card.leading_icon_color(&colors), colors.text_secondary);
    });

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.tool_cards["elapsed"]
            .read(cx)
            .visible_text()),
        "正在运行 printf 'tick' · 1 秒"
    );

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.tool_cards["elapsed"]
            .read(cx)
            .visible_text()),
        "正在运行 printf 'tick' · 2 秒",
        "the compact row refreshes at the next whole-second boundary"
    );

    cx.executor().advance_clock(Duration::from_secs(63));
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.tool_cards["elapsed"]
            .read(cx)
            .visible_text()),
        "正在运行 printf 'tick' · 1 分 5 秒"
    );

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "elapsed".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: "tick".into(),
                    reused: false,
                    exit_code: Some(0),
                    duration_ms: Some(1_234),
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
    });
    let terminal = stream.read_with(cx, |stream, cx| {
        let card = stream.tool_cards["elapsed"].read(cx);
        assert!(
            !card.live_elapsed_active(),
            "terminal state cancels its timer"
        );
        card.visible_text()
    });
    assert_eq!(terminal, "已运行 printf 'tick' · 1.2 秒");
    cx.executor().advance_clock(Duration::from_secs(30));
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.tool_cards["elapsed"]
            .read(cx)
            .visible_text()),
        terminal,
        "advancing the UI clock after terminal cannot replace runtime duration"
    );
}

#[gpui_kit::test]
async fn issue70_e70_group_owns_no_time_and_children_have_independent_elapsed(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "issue70-group-elapsed");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("first", "sleep first"),
            },
            cx,
        );
        approve_and_run(stream, "first", cx);
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    stream.update(cx, |stream, cx| {
        for call in [
            bash_call("second", "sleep second"),
            read_call("read", "read", "{}"),
        ] {
            let id = call.id.clone();
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
            approve_and_run(stream, &id, cx);
        }
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();

    stream.read_with(cx, |stream, cx| {
        let group = group(stream);
        let aggregate = group.read(cx).aggregate_summary(cx);
        assert_eq!(aggregate, "正在处理：运行命令、读取文件");
        assert!(
            !aggregate.contains('秒') && !aggregate.contains("分钟") && !aggregate.contains("毫秒"),
            "aggregate must not expose child or total elapsed time: {aggregate}"
        );
        assert_eq!(group.read(cx).row_count(cx), 1);
    });

    let expanded_group = stream.read_with(cx, |stream, _| group(stream));
    expanded_group.update(cx, ToolActivityGroup::toggle_expanded);
    let expanded = stream.read_with(cx, |stream, cx| group(stream).read(cx).visible_text(cx));
    assert!(
        expanded.contains("正在运行 sleep first · 5 秒"),
        "{expanded}"
    );
    assert!(
        expanded.contains("正在运行 sleep second · 3 秒"),
        "{expanded}"
    );
    assert!(expanded.contains("正在读取文件"), "{expanded}");
    assert_eq!(
        expanded.matches(" · ").count(),
        2,
        "only Bash children show time"
    );
    stream.read_with(cx, |stream, cx| {
        let colors = theme(cx).colors;
        let group = group(stream);
        assert!(matches!(group.read(cx).leading_icon(cx), Icon::Terminal));
        assert_eq!(
            group.read(cx).leading_icon_color(cx, &colors),
            colors.text_secondary
        );
        assert!(!stream.tool_cards["read"].read(cx).live_elapsed_active());
    });

    stream.update(cx, |stream, cx| {
        for (call_id, duration_ms) in [("first", Some(5_100)), ("second", Some(3_000))] {
            stream.apply_event(
                ConversationEvent::ToolCallFinished {
                    call_id: call_id.into(),
                    result: ToolResult {
                        status: ToolCallStatus::Success,
                        output: String::new(),
                        reused: false,
                        exit_code: Some(0),
                        duration_ms,
                        truncated: Some(false),
                        invalid: None,
                    },
                },
                cx,
            );
        }
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "read".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: String::new(),
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
    });
}

#[gpui_kit::test]
async fn issue70_e70_long_bash_keeps_running_and_terminal_duration_visible(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue70-duration-layout");
    window
        .update(cx, |_, window, _| {
            window.resize(gpui_kit::size(px(480.), px(600.)));
        })
        .expect("resize duration layout window");
    let command = format!("printf '{}'", "0123456789abcdef".repeat(32));
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("long-running", &command),
            },
            cx,
        );
        approve_and_run(stream, "long-running", cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: read_call("following-read", "read", "{}"),
            },
            cx,
        );
    });
    cx.run_until_parked();

    stream.read_with(cx, |stream, cx| {
        let aggregate = group(stream).read(cx).aggregate_summary(cx);
        assert_eq!(aggregate, "正在处理：运行命令、读取文件");
        assert!(
            !aggregate.contains('秒') && !aggregate.contains("分钟") && !aggregate.contains("毫秒"),
            "aggregate must remain duration-free: {aggregate}"
        );
    });
    click(window, "tool-activity-group-toggle", cx);

    let row = bounds(window, "tool-activity-child-0", cx);
    let title = bounds(window, "tool-activity-child-0-title", cx);
    let running_duration = bounds(window, "tool-activity-child-0-duration", cx);
    let running_chevron = bounds(window, "tool-activity-child-0-chevron", cx);
    assert!(title.right() <= running_duration.left());
    assert!(running_duration.right() <= running_chevron.left());
    assert!(running_chevron.right() <= row.right());
    assert!(
        title.size.height <= row.size.height,
        "title must stay one line"
    );
    assert!(
        running_duration.size.width >= body_text_width(window, "· 0 秒".into(), cx),
        "running duration must retain its intrinsic width"
    );

    let full_title = format!("正在运行 {command}");
    let intrinsic_title_width = body_text_width(window, full_title, cx);
    assert!(
        intrinsic_title_width > title.size.width,
        "the mounted title lane must be narrower than the full long command"
    );
    stream.read_with(cx, |stream, cx| {
        let card = stream.tool_cards["long-running"].read(cx);
        assert_eq!(card.visible_text(), format!("正在运行 {command} · 0 秒"));
        let colors = theme(cx).colors;
        assert!(matches!(card.leading_icon(), Icon::Terminal));
        assert_eq!(card.leading_icon_color(&colors), colors.text_secondary);
    });

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "long-running".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: String::new(),
                    reused: false,
                    exit_code: Some(0),
                    duration_ms: Some(1_234),
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
    });
    cx.run_until_parked();

    let terminal_title = bounds(window, "tool-activity-child-0-title", cx);
    let terminal_duration = bounds(window, "tool-activity-child-0-duration", cx);
    let terminal_chevron = bounds(window, "tool-activity-child-0-chevron", cx);
    assert!(terminal_title.right() <= terminal_duration.left());
    assert!(terminal_duration.right() <= terminal_chevron.left());
    assert!(
        terminal_duration.size.width >= body_text_width(window, "· 1.2 秒".into(), cx),
        "terminal duration must retain its intrinsic width"
    );
    assert_eq!(
        terminal_duration.right(),
        running_duration.right(),
        "the trailing duration edge stays fixed across running and terminal copy"
    );
    assert_eq!(terminal_chevron, running_chevron);
    stream.read_with(cx, |stream, cx| {
        assert_eq!(
            stream.tool_cards["long-running"].read(cx).visible_text(),
            format!("已运行 {command} · 1.2 秒")
        );
        let aggregate = group(stream).read(cx).aggregate_summary(cx);
        assert!(
            !aggregate.contains('秒') && !aggregate.contains("分钟") && !aggregate.contains("毫秒"),
            "terminal child duration must not enter aggregate copy: {aggregate}"
        );
    });
}

#[gpui_kit::test]
async fn issue70_e70_non_bash_running_and_hydrated_bash_do_not_invent_elapsed(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "issue70-non-bash-elapsed");
    let fingerprint = "a".repeat(64);
    let mcp_identity = McpCallIdentity {
        server_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        config_revision: 3,
        exact_tool_name: "echo".into(),
        arguments_bytes: 19,
        arguments_sha256: "b".repeat(64),
        argument_preview: "echo: string".into(),
    };
    let non_bash_calls = vec![
        read_call("read", "read", "{}"),
        read_call("find", "glob", "{}"),
        read_call("search", "grep", "{}"),
        ToolCall {
            id: "write".into(),
            tool: "write".into(),
            input_json: format!(
                r#"{{"audit_version":"write_edit_v1","tool":"write","path":"src/lib.rs","content_bytes":3,"fingerprint_v1":"{fingerprint}"}}"#
            ),
        },
        ToolCall {
            id: "edit".into(),
            tool: "edit".into(),
            input_json: format!(
                r#"{{"audit_version":"write_edit_v1","tool":"edit","path":"src/lib.rs","old_string_bytes":3,"new_string_bytes":4,"fingerprint_v1":"{fingerprint}"}}"#
            ),
        },
        ToolCall {
            id: "skill".into(),
            tool: "load_skill".into(),
            input_json: r#"{"name":"example-skill"}"#.into(),
        },
        ToolCall {
            id: "mcp".into(),
            tool: mcp_identity.alias(),
            input_json: serde_json::json!({
                "server_id": mcp_identity.server_id,
                "config_revision": mcp_identity.config_revision,
                "tool": mcp_identity.exact_tool_name,
                "arguments_bytes": mcp_identity.arguments_bytes,
                "arguments_sha256": mcp_identity.arguments_sha256,
                "argument_preview": mcp_identity.argument_preview,
            })
            .to_string(),
        },
    ];
    stream.update(cx, |stream, cx| {
        for call in non_bash_calls {
            let id = call.id.clone();
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
            approve_and_run(stream, &id, cx);
        }
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(65));
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        for id in ["read", "find", "search", "write", "edit", "skill", "mcp"] {
            let card = stream.tool_cards[id].read(cx);
            let text = card.visible_text();
            assert!(
                !card.live_elapsed_active(),
                "{id} must not own an elapsed timer"
            );
            assert!(
                !text.contains('秒') && !text.contains("分钟") && !text.contains("毫秒"),
                "{id} leaked elapsed copy: {text}"
            );
        }
    });

    let (_window, hydrated, _) = open_controller_stream(cx, "issue70-hydrated-running");
    hydrated.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![HistoryEntry::Tool {
                    seq: 1,
                    message_id: "hydrated-message".into(),
                    call_id: "hydrated-running".into(),
                    status: ToolCallStatus::Running,
                    approval: Some(Approval::Once),
                    input: Some(ToolCardInputProjection::Bash {
                        command: "sleep hydrated".into(),
                    }),
                    result: None,
                }],
                None,
            ),
            cx,
        );
    });
    hydrated.read_with(cx, |stream, cx| {
        let card = stream.tool_cards["hydrated-running"].read(cx);
        assert_eq!(card.visible_text(), "正在运行 sleep hydrated");
        assert!(!card.live_elapsed_active());
    });
}

#[gpui_kit::test]
async fn issue70_t70_1_single_shell_is_one_surface_free_compact_row(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue70-single-shell");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("shell", "printf ok"),
            },
            cx,
        );
        finish(
            stream,
            "shell",
            ToolCallStatus::Success,
            "ok",
            Some(0),
            Some(12),
            cx,
        );
    });
    cx.run_until_parked();

    let colors = cx.update(|cx| theme(cx).colors);
    let (rows, visible, leading_icon, leading_icon_color) = stream.read_with(cx, |stream, cx| {
        let card = stream.tool_cards["shell"].read(cx);
        (
            stream.entries[0].row_count(cx),
            card.visible_text(),
            card.leading_icon(),
            card.leading_icon_color(&colors),
        )
    });
    assert_eq!(rows, 1, "collapsed one-call activity is one compact row");
    assert_eq!(visible, "已运行 printf ok · 12 毫秒");
    assert!(
        matches!(leading_icon, Icon::Terminal),
        "successful Shell keeps its category icon instead of Check"
    );
    assert_eq!(
        leading_icon_color, colors.text_secondary,
        "successful Shell leading icon stays neutral"
    );
    let row = bounds(window, "tool-activity-single-row", cx);
    assert!(
        gpui_kit::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("tool-activity-single-row-chevron")
            .is_some(),
        "a shell row can disclose its full command and terminal detail"
    );
    let (quads, scale) = window
        .update(cx, |_, window, _| {
            (window.painted_quads(), window.scale_factor())
        })
        .expect("mounted tool row");
    assert!(
        quads.iter().all(|quad| {
            quad.background.as_solid().is_none() || !logical_quad_matches(quad, row, scale)
        }),
        "the resting row must not paint the old full-row card surface"
    );
}

#[gpui_kit::test]
async fn issue70_t70_2_adjacent_mixed_tools_share_one_collapsed_item(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "issue70-grouping");
    let unrelated_call = bash_call("unrelated", "false");
    let unrelated = cx.new(|_| ToolCard::proposed(&unrelated_call));
    stream.update(cx, |stream, cx| {
        for call in [
            read_call("read", "read", r#"{"path":"/SECRET_READ_PATH"}"#),
            bash_call("shell", "pwd"),
            read_call("grep", "grep", r#"{"pattern":"SECRET_PATTERN"}"#),
        ] {
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        }
        finish(stream, "read", ToolCallStatus::Success, "", None, None, cx);
        finish(
            stream,
            "shell",
            ToolCallStatus::Success,
            "",
            Some(0),
            Some(1_200),
            cx,
        );
        finish(stream, "grep", ToolCallStatus::Success, "", None, None, cx);
    });

    stream.read_with(cx, |stream, cx| {
        assert_eq!(stream.entries.len(), 1);
        let group = group(stream);
        let group = group.read(cx);
        assert_eq!(group.len(), 3);
        assert_eq!(group.row_count(cx), 1);
        let colors = theme(cx).colors;
        assert!(
            matches!(group.leading_icon(cx), Icon::Document),
            "mixed group follows its first Read child category instead of success state"
        );
        assert_eq!(
            group.leading_icon_color(cx, &colors),
            colors.text_secondary,
            "aggregate leading icon stays neutral"
        );
        assert_eq!(
            group.aggregate_summary(cx),
            "已读取文件、运行命令、搜索内容"
        );
        assert_eq!(
            group.children(),
            vec![
                stream.tool_cards["read"].clone(),
                stream.tool_cards["shell"].clone(),
                stream.tool_cards["grep"].clone(),
            ],
            "child order is proposal/audit order"
        );
        assert!(ConversationStream::tool_entry_contains(
            &stream.entries[0],
            &stream.tool_cards["shell"],
            cx
        ));
        assert!(
            !ConversationStream::tool_entry_contains(&stream.entries[0], &unrelated, cx),
            "artifact placement must not treat an unrelated group as its owning tool entry"
        );
    });
}

#[gpui_kit::test]
async fn issue70_t70_3_text_and_permission_boundaries_are_exact(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "issue70-text-boundary");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "answer".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "answer".into(),
                delta: "before".into(),
            },
            cx,
        );
        for id in ["a", "b"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "pwd"),
                },
                cx,
            );
        }
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "answer".into(),
                delta: "after".into(),
            },
            cx,
        );
        for id in ["c", "d"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "true"),
                },
                cx,
            );
        }
    });
    assert_eq!(
        stream.read_with(cx, entry_shapes),
        ["assistant", "group:2", "assistant", "group:2"],
        "nonempty assistant text is a real group boundary"
    );

    let (_window, permission_stream, _) = open_controller_stream(cx, "issue70-permission-boundary");
    let queue = permission_stream.read_with(cx, |stream, _| stream.permission_queue());
    let future = request_permission(&queue, "permission-a", "pwd");
    cx.run_until_parked();
    permission_stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("permission-a", "pwd"),
            },
            cx,
        );
    });
    cx.run_until_parked();
    permission_stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("permission-b", "true"),
            },
            cx,
        );
        assert_eq!(entry_shapes(stream, cx), ["tool:1", "permission", "tool:1"]);
        stream.remove_active_permission(cx);
    });
    drop(future);
    assert_eq!(
        permission_stream.read_with(cx, entry_shapes),
        ["group:2"],
        "removing the transient permission card restores tool adjacency"
    );
}

#[gpui_kit::test]
async fn issue70_t70_3_permission_merge_preserves_right_group_disclosure(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "issue70-permission-expanded-merge");
    stream.update(cx, |stream, cx| {
        for id in ["left-a", "left-b"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "true"),
                },
                cx,
            );
        }
    });
    let queue = stream.read_with(cx, |stream, _| stream.permission_queue());
    let future = request_permission(&queue, "permission-call", "pwd");
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("permission-call", "pwd"),
            },
            cx,
        );
    });
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        for id in ["right-a", "right-b"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, "true"),
                },
                cx,
            );
        }
        assert_eq!(
            entry_shapes(stream, cx),
            ["group:3", "permission", "group:2"]
        );
    });
    let right_group = stream.read_with(cx, |stream, _| match &stream.entries[2] {
        StreamEntry::ToolGroup { group } => group.clone(),
        _ => panic!("right tool group"),
    });
    right_group.update(cx, ToolActivityGroup::toggle_expanded);
    stream.update(cx, |stream, cx| stream.remove_active_permission(cx));
    drop(future);
    stream.read_with(cx, |stream, cx| {
        assert_eq!(entry_shapes(stream, cx), ["group:5"]);
        assert!(
            group(stream).read(cx).expanded(),
            "merging into an existing left group preserves the right disclosure state"
        );
    });
}

#[gpui_kit::test]
async fn issue70_t70_4_group_and_child_disclosure_are_scoped_and_remeasure(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue70-disclosure");
    stream.update(cx, |stream, cx| {
        for call in [
            read_call("read", "read", "{}"),
            read_call("glob", "glob", "{}"),
            bash_call("shell", "printf 'full command'"),
        ] {
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        }
        finish(stream, "read", ToolCallStatus::Success, "", None, None, cx);
        finish(stream, "glob", ToolCallStatus::Success, "", None, None, cx);
        finish(
            stream,
            "shell",
            ToolCallStatus::Success,
            "line one\nline two",
            Some(0),
            Some(25),
            cx,
        );
    });
    cx.run_until_parked();
    let collapsed_height = f32::from(bounds(window, "tool-activity-group", cx).size.height);
    assert!(
        gpui_kit::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("tool-activity-child-0")
            .is_none()
    );

    click(window, "tool-activity-group-toggle", cx);
    let expanded_rows = stream.read_with(cx, |stream, cx| group(stream).read(cx).row_count(cx));
    assert_eq!(expanded_rows, 4, "aggregate plus three compact children");
    for selector in [
        "tool-activity-child-0",
        "tool-activity-child-1",
        "tool-activity-child-2",
    ] {
        assert!(
            gpui_kit::VisualTestContext::from_window(window.into(), cx)
                .debug_bounds(selector)
                .is_some(),
            "expanded group mounts {selector}"
        );
    }
    let children_height = f32::from(bounds(window, "tool-activity-group", cx).size.height);
    assert!(children_height > collapsed_height);

    click(window, "tool-activity-child-2", cx);
    assert!(
        gpui_kit::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("tool-activity-child-2-detail")
            .is_some()
    );
    assert!(
        gpui_kit::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("tool-activity-child-0-detail")
            .is_none(),
        "opening one child does not open another"
    );
    let detail_height = f32::from(bounds(window, "tool-activity-group", cx).size.height);
    assert!(detail_height > children_height);
    assert!(
        stream.read_with(cx, |stream, cx| group(stream).read(cx).row_count(cx)) > expanded_rows,
        "opening one child adds only its bounded detail rows"
    );
    let (shell_text, footer_state) = stream.read_with(cx, |stream, cx| {
        let card = stream.tool_cards["shell"].read(cx);
        (card.visible_text(), card.detail_footer_state())
    });
    assert_eq!(
        footer_state,
        Some(ToolActivityState::Success),
        "the rendered terminal footer uses the success semantic color"
    );
    let colors = cx.update(|cx| theme(cx).colors);
    assert_eq!(
        footer_state.map(|state| state.terminal_color(&colors)),
        Some(colors.success),
        "the footer state resolves to the theme success token"
    );
    for expected in [
        "Shell",
        "$ printf 'full command'",
        "line one",
        "line two",
        "已完成 · exit 0 · 25 毫秒",
    ] {
        assert!(shell_text.contains(expected), "missing detail {expected:?}");
    }

    click(window, "tool-activity-child-2", cx);
    assert_eq!(
        stream.read_with(cx, |stream, cx| group(stream).read(cx).row_count(cx)),
        expanded_rows,
        "closing the child restores the child-list row count"
    );
    assert_eq!(
        f32::from(bounds(window, "tool-activity-group", cx).size.height),
        children_height
    );
    click(window, "tool-activity-group-toggle", cx);
    assert_eq!(
        stream.read_with(cx, |stream, cx| group(stream).read(cx).row_count(cx)),
        1,
        "closing the group restores the aggregate-only row count"
    );
    assert_eq!(
        f32::from(bounds(window, "tool-activity-group", cx).size.height),
        collapsed_height
    );
}

#[gpui_kit::test]
async fn issue70_t70_5_lifecycle_updates_keep_entities_and_truthful_failure(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "issue70-lifecycle");
    stream.update(cx, |stream, cx| {
        for id in ["first", "second"] {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: bash_call(id, if id == "first" { "true" } else { "false" }),
                },
                cx,
            );
        }
    });
    let before = stream.read_with(cx, |stream, cx| {
        let group = group(stream);
        assert!(
            group
                .read(cx)
                .aggregate_summary(cx)
                .starts_with("正在处理：")
        );
        group.read(cx).children()
    });

    stream.update(cx, |stream, cx| {
        finish(
            stream,
            "first",
            ToolCallStatus::Success,
            "",
            Some(0),
            Some(1),
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallApproved {
                call_id: "second".into(),
                approval: Approval::Once,
            },
            cx,
        );
    });
    assert!(stream.read_with(cx, |stream, cx| {
        group(stream)
            .read(cx)
            .aggregate_summary(cx)
            .starts_with("正在处理：")
    }));
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "second".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: String::new(),
                    reused: false,
                    exit_code: Some(1),
                    duration_ms: Some(4),
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
    });
    stream.read_with(cx, |stream, cx| {
        let group = group(stream);
        assert_eq!(group.read(cx).children(), before);
        assert!(
            group
                .read(cx)
                .aggregate_summary(cx)
                .starts_with("存在失败的调用：")
        );
        assert!(
            stream.tool_cards["second"]
                .read(cx)
                .visible_text()
                .contains("exit 1")
        );
        assert_eq!(
            stream.tool_cards["second"].read(cx).detail_footer_state(),
            Some(ToolActivityState::Failed),
            "the rendered terminal footer uses the danger semantic color"
        );
        let colors = theme(cx).colors;
        assert!(
            matches!(group.read(cx).leading_icon(cx), Icon::Terminal),
            "failed Shell group keeps its category icon instead of Warning"
        );
        assert_eq!(
            group.read(cx).leading_icon_color(cx, &colors),
            colors.text_secondary,
            "failed group leading icon stays neutral"
        );
        assert!(
            matches!(
                stream.tool_cards["second"].read(cx).leading_icon(),
                Icon::Terminal
            ),
            "failed Shell child keeps its category icon instead of Warning"
        );
        assert_eq!(
            stream.tool_cards["second"]
                .read(cx)
                .leading_icon_color(&colors),
            colors.text_secondary,
            "failed child leading icon stays neutral"
        );
        assert_eq!(
            stream.tool_cards["second"]
                .read(cx)
                .detail_footer_state()
                .map(|state| state.terminal_color(&colors)),
            Some(colors.danger),
            "the failed footer state resolves to the theme danger token"
        );
        assert_eq!(group.read(cx).row_count(cx), 1);
    });
}

fn history_tool(
    seq: i64,
    id: &str,
    input: ToolCardInputProjection,
    result: ToolCardResultProjection,
) -> HistoryEntry {
    HistoryEntry::Tool {
        seq,
        message_id: "hydrated-message".into(),
        call_id: id.into(),
        status: ToolCallStatus::Success,
        approval: Some(Approval::Once),
        input: Some(input),
        result: Some(result),
    }
}

fn parity_history() -> Vec<HistoryEntry> {
    vec![
        hydration_assistant(1, "before"),
        history_tool(
            2,
            "read",
            ToolCardInputProjection::ReadOnly {
                tool: ReadOnlyToolKind::Read,
                permission_path: None,
            },
            ToolCardResultProjection::ReadOnly {
                status: ToolCallStatus::Success,
                output: String::new(),
                reused: false,
            },
        ),
        history_tool(
            3,
            "glob",
            ToolCardInputProjection::ReadOnly {
                tool: ReadOnlyToolKind::Glob,
                permission_path: None,
            },
            ToolCardResultProjection::ReadOnly {
                status: ToolCallStatus::Success,
                output: String::new(),
                reused: false,
            },
        ),
        history_tool(
            4,
            "shell",
            ToolCardInputProjection::Bash {
                command: "true".into(),
            },
            ToolCardResultProjection::Bash {
                status: ToolCallStatus::Success,
                output: String::new(),
                exit_code: Some(0),
                duration_ms: Some(1),
                truncated: Some(false),
                reused: false,
            },
        ),
        hydration_assistant(5, "after"),
        history_tool(
            6,
            "grep",
            ToolCardInputProjection::ReadOnly {
                tool: ReadOnlyToolKind::Grep,
                permission_path: None,
            },
            ToolCardResultProjection::ReadOnly {
                status: ToolCallStatus::Success,
                output: String::new(),
                reused: false,
            },
        ),
    ]
}

#[gpui_kit::test]
async fn issue70_t70_6_hydration_matches_live_and_reopen_resets_expansion(cx: &mut TestAppContext) {
    let (_live_window, live, _) = open_controller_stream(cx, "issue70-live-parity");
    live.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "live".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live".into(),
                delta: "before".into(),
            },
            cx,
        );
        for call in [
            read_call("read", "read", "{}"),
            read_call("glob", "glob", "{}"),
            bash_call("shell", "true"),
        ] {
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        }
        finish(stream, "read", ToolCallStatus::Success, "", None, None, cx);
        finish(stream, "glob", ToolCallStatus::Success, "", None, None, cx);
        finish(
            stream,
            "shell",
            ToolCallStatus::Success,
            "",
            Some(0),
            Some(1),
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live".into(),
                delta: "after".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: read_call("grep", "grep", "{}"),
            },
            cx,
        );
        finish(stream, "grep", ToolCallStatus::Success, "", None, None, cx);
    });

    let (hydrated_window, hydrated, _) = open_controller_stream(cx, "issue70-hydrated-parity");
    hydrated.update(cx, |stream, cx| {
        stream.apply_history_page(hydration_page(parity_history(), None), cx)
    });
    assert_eq!(
        live.read_with(cx, entry_shapes),
        hydrated.read_with(cx, entry_shapes)
    );
    let live_text = live.read_with(cx, activity_texts);
    let hydrated_text = hydrated.read_with(cx, activity_texts);
    assert_eq!(live_text, hydrated_text);
    assert_eq!(
        hydrated_text,
        ["已读取文件、查找文件、运行命令", "已搜索内容"],
        "live and hydrated activity copy preserves boundaries, categories, and terminal state"
    );
    assert_eq!(
        hydrated.read_with(cx, |stream, cx| group(stream).read(cx).row_count(cx)),
        1
    );
    assert!(hydrated.read_with(cx, |stream, _| {
        !stream.actions.running && stream.active_permission.is_none()
    }));

    click(hydrated_window, "tool-activity-group-toggle", cx);
    assert!(hydrated.read_with(cx, |stream, cx| group(stream).read(cx).row_count(cx)) > 1);
    let (_reopen_window, reopened, _) = open_controller_stream(cx, "issue70-hydrated-reopen");
    reopened.update(cx, |stream, cx| {
        stream.apply_history_page(hydration_page(parity_history(), None), cx)
    });
    assert_eq!(
        reopened.read_with(cx, |stream, cx| group(stream).read(cx).row_count(cx)),
        1,
        "route reopen starts collapsed and does not persist UI expansion"
    );
}

#[gpui_kit::test]
async fn issue70_t70_7_grouping_preserves_redaction_and_fail_closed_visibility(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue70-redaction");
    let fingerprint = "a".repeat(64);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: read_call(
                    "SECRET_READ_CALL_ID",
                    "read",
                    r#"{"path":"/SECRET_ABSOLUTE_ROOT/private.txt"}"#,
                ),
            },
            cx,
        );
        finish(
            stream,
            "SECRET_READ_CALL_ID",
            ToolCallStatus::Success,
            "SAFE_BOUNDED_OUTPUT",
            None,
            None,
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: read_call(
                    "SECRET_GLOB_CALL_ID",
                    "glob",
                    r#"{"pattern":"SECRET_GLOB_PATTERN","path":"/SECRET_GLOB_ROOT"}"#,
                ),
            },
            cx,
        );
        finish(
            stream,
            "SECRET_GLOB_CALL_ID",
            ToolCallStatus::Success,
            "",
            None,
            None,
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: read_call(
                    "SECRET_GREP_CALL_ID",
                    "grep",
                    r#"{"pattern":"SECRET_GREP_PATTERN","path":"/SECRET_GREP_ROOT"}"#,
                ),
            },
            cx,
        );
        finish(
            stream,
            "SECRET_GREP_CALL_ID",
            ToolCallStatus::Success,
            "",
            None,
            None,
            cx,
        );

        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: ToolCall {
                    id: "SECRET_WRITE_CALL_ID".into(),
                    tool: "write".into(),
                    input_json: format!(
                        r#"{{"audit_version":"write_edit_v1","tool":"write","path":"src/lib.rs","content_bytes":3,"fingerprint_v1":"{fingerprint}"}}"#
                    ),
                },
            },
            cx,
        );
        finish(
            stream,
            "SECRET_WRITE_CALL_ID",
            ToolCallStatus::Success,
            r#"{"path":"src/lib.rs","bytes_written":3,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            None,
            None,
            cx,
        );

        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "SECRET_INVALID_CALL_ID".into(),
                result: ToolResult {
                    status: ToolCallStatus::Rejected,
                    output: "Tool error: invalid write input (malformed_json)".into(),
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: None,
                    invalid: Some(InvalidToolProjection::new(
                        InvalidToolKind::Write,
                        InvalidToolCode::MalformedJson,
                    )),
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "SECRET_FORGED_INVALID_CALL_ID".into(),
                result: ToolResult {
                    status: ToolCallStatus::Rejected,
                    output: "SECRET_INVALID_BODY".into(),
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: None,
                    invalid: Some(InvalidToolProjection::new(
                        InvalidToolKind::Write,
                        InvalidToolCode::MalformedJson,
                    )),
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: ToolCall {
                    id: "SECRET_CORRUPT_CALL_ID".into(),
                    tool: "write".into(),
                    input_json: r#"{"path":"/SECRET_DATA_ROOT/file","body":"SECRET_RAW_BODY"}"#
                        .into(),
                },
            },
            cx,
        );
    });
    click(window, "tool-activity-group-toggle", cx);
    click(window, "tool-activity-child-0", cx);
    let visible = stream.read_with(cx, |stream, cx| group(stream).read(cx).visible_text(cx));
    for safe in [
        "SAFE_BOUNDED_OUTPUT",
        "src/lib.rs · 3 bytes",
        "malformed_json",
        "工具结果损坏",
    ] {
        assert!(visible.contains(safe), "missing safe state {safe:?}");
    }
    for secret in [
        "SECRET_READ_CALL_ID",
        "SECRET_WRITE_CALL_ID",
        "SECRET_GLOB_CALL_ID",
        "SECRET_GREP_CALL_ID",
        "SECRET_INVALID_CALL_ID",
        "SECRET_FORGED_INVALID_CALL_ID",
        "SECRET_CORRUPT_CALL_ID",
        "SECRET_ABSOLUTE_ROOT",
        "SECRET_GLOB_PATTERN",
        "SECRET_GLOB_ROOT",
        "SECRET_GREP_PATTERN",
        "SECRET_GREP_ROOT",
        "SECRET_INVALID_BODY",
        "SECRET_DATA_ROOT",
        "SECRET_RAW_BODY",
        "preimage-v1",
        "aaaaaaaaaaaaaaaa",
    ] {
        assert!(!visible.contains(secret), "leaked {secret:?}: {visible}");
    }
    assert!(
        visible.starts_with("存在失败的调用："),
        "a mixed failed group must not claim success: {visible}"
    );
}
