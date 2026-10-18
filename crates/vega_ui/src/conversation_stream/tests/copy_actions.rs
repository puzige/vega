use crate::conversation_stream::*;
use gpui_kit::{
    Bounds, ClipboardItem, ListOffset, Modifiers, MouseButton, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, TestAppContext, VisualTestContext, WindowHandle, point, size,
};
use std::{cell::Cell, rc::Rc};
use vega_conversation::{
    history::AssistantStatus,
    types::{
        AssistantResponseKind, AssistantResponseMetadata, ThreadStatus, ToolCallStatus, ToolResult,
    },
};

fn copy_thread(name: &str) -> Thread {
    Thread {
        parent_thread_id: None,
        id: name.into(),
        backend: TaskBackend::Native,
        project_id: "copy-project".into(),
        title: "Copy actions".into(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        model: "mock".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    }
}

fn start_run(message_id: &str) -> ConversationEvent {
    ConversationEvent::MessageStarted {
        message_id: message_id.into(),
        seq: 2,
    }
}

fn finish_run(message_id: &str) -> ConversationEvent {
    ConversationEvent::MessageFinished {
        message_id: message_id.into(),
        stop_reason: vega_conversation::types::ConversationStopReason::End,
        execution_duration_ms: Some(10),
    }
}

fn thinking_delta(message_id: &str, delta: &str) -> ConversationEvent {
    ConversationEvent::ThinkingDelta {
        message_id: message_id.into(),
        delta: delta.into(),
    }
}

fn bash_call(id: &str, command: &str) -> vega_conversation::types::ToolCall {
    vega_conversation::types::ToolCall {
        id: id.into(),
        tool: "bash".into(),
        input_json: serde_json::json!({ "cmd": command }).to_string(),
    }
}

fn hydration_user(seq: i64, content: &str) -> HistoryEntry {
    HistoryEntry::UserText {
        seq,
        message_id: format!("user-{seq}"),
        content: content.into(),
    }
}

fn hydration_page(entries: Vec<HistoryEntry>, older_cursor: Option<i64>) -> HistoryPage {
    HistoryPage {
        subagents: Vec::new(),
        entries,
        older_cursor,
        newer_cursor: None,
        newest_seq: None,
    }
}

struct CopyHarness {
    stream: Entity<ConversationStream>,
}

impl Render for CopyHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .size_full()
            .on_key_down(|event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            })
            .child(gpui_kit::base::TextSelectionLayer)
            .child(self.stream.clone())
    }
}

fn open_copy_stream(
    cx: &mut TestAppContext,
    name: &str,
) -> (
    WindowHandle<CopyHarness>,
    Entity<ConversationStream>,
    Rc<Cell<usize>>,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(false));
        crate::init(cx);
        cx.set_reduce_motion(true);
    });
    let thread = copy_thread(name);
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let window_stream = stream.clone();
    let submissions = Rc::new(Cell::new(0));
    let observed = submissions.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|cx| {
                    cx.subscribe(&window_stream, move |_, _, _: &ComposerSubmitted, _| {
                        observed.set(observed.get() + 1)
                    })
                    .detach();
                    CopyHarness {
                        stream: window_stream,
                    }
                })
            })
        })
        .expect("copy conversation window");
    cx.run_until_parked();
    cx.simulate_window_resize(window.into(), size(px(1200.), px(900.)));
    cx.run_until_parked();
    (window, stream, submissions)
}

fn selector(prefix: &str, id: u64) -> &'static str {
    Box::leak(format!("{prefix}-{id}").into_boxed_str())
}

fn code_selector(id: u64, ordinal: usize) -> &'static str {
    Box::leak(format!("markdown-code-copy-{id}-{ordinal}").into_boxed_str())
}

fn assistant_ids(stream: &ConversationStream, message_id: &str) -> Vec<u64> {
    stream
        .entries
        .iter()
        .zip(&stream.entry_identities)
        .filter_map(|(entry, identity)| match entry {
            StreamEntry::Assistant { copy, .. }
                if identity.message_id.as_deref() == Some(message_id) =>
            {
                Some(copy.id)
            }
            _ => None,
        })
        .collect()
}

fn last_assistant_id(stream: &ConversationStream, message_id: &str) -> u64 {
    *assistant_ids(stream, message_id)
        .last()
        .expect("rendered assistant source")
}

fn text_delta(message_id: &str, text: &str) -> ConversationEvent {
    ConversationEvent::TextDelta {
        message_id: message_id.into(),
        delta: text.into(),
    }
}

fn response(
    response_id: &str,
    kind: AssistantResponseKind,
    start: usize,
    end: usize,
) -> AssistantResponseMetadata {
    AssistantResponseMetadata {
        response_id: response_id.into(),
        text_start_bytes: start,
        text_end_bytes: end,
        kind,
    }
}

fn completed(message_id: &str, response: AssistantResponseMetadata) -> ConversationEvent {
    ConversationEvent::AssistantResponseCompleted {
        message_id: message_id.into(),
        response,
    }
}

fn history_text(seq: i64, message_id: &str, text: &str) -> HistoryEntry {
    HistoryEntry::AssistantText {
        seq,
        message_id: message_id.into(),
        content: text.into(),
        status: AssistantStatus::Done,
        execution_duration_ms: Some(10),
    }
}

fn history_response(
    seq: i64,
    message_id: &str,
    response: AssistantResponseMetadata,
) -> HistoryEntry {
    HistoryEntry::AssistantResponseMetadata {
        seq,
        message_id: message_id.into(),
        response,
    }
}

fn final_run(
    stream: &mut ConversationStream,
    message_id: &str,
    text: &str,
    cx: &mut Context<ConversationStream>,
) {
    stream.apply_event(start_run(message_id), cx);
    stream.apply_event(text_delta(message_id, text), cx);
    stream.apply_event(
        completed(
            message_id,
            response("answer", AssistantResponseKind::Final, 0, text.len()),
        ),
        cx,
    );
    stream.apply_event(finish_run(message_id), cx);
}

fn clipboard(visual: &VisualTestContext) -> String {
    visual
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default()
}

fn copy_click(visual: &mut VisualTestContext, target: &'static str, expected: &str) {
    visual.write_to_clipboard(ClipboardItem::new_string("unchanged clipboard".into()));
    let bounds = visual.debug_bounds(target).expect("copy action is mounted");
    visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
    assert_eq!(clipboard(visual), expected);
}

fn visible_surface(visual: &mut VisualTestContext, bounds: Bounds<Pixels>) -> bool {
    visual.update(|window, _| {
        let bounds = bounds.scale(window.scale_factor());
        window.painted_quads().iter().any(|quad| {
            quad.bounds == bounds && quad.background.as_solid().is_some_and(|color| color.a > 0.)
        })
    })
}

fn focus_by_tab(visual: &mut VisualTestContext, target: &'static str) {
    let bounds = visual.debug_bounds(target).expect("keyboard copy action");
    visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
    for _ in 0..40 {
        visual.simulate_keystrokes("tab");
        visual.run_until_parked();
        if visible_surface(visual, bounds) {
            assert!(visual.update(|window, cx| window.focused(cx).is_some()));
            return;
        }
    }
    panic!("copy action cannot be reached by Tab: {target}");
}

fn text_layout(visual: &mut VisualTestContext, text: &str) -> gpui_kit::TextLayout {
    selection::SELECTION_TARGET_LAYOUT
        .with_borrow_mut(|target| *target = Some((text.into(), None)));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    selection::SELECTION_TARGET_LAYOUT.with_borrow(|target| {
        target
            .as_ref()
            .and_then(|(_, layout)| layout.clone())
            .expect("production selection text geometry")
    })
}

fn text_point(layout: &gpui_kit::TextLayout, index: usize) -> Point<Pixels> {
    let mut point = layout.position_for_index(index).expect("text position");
    point.x += px(0.5);
    point.y += layout.line_height() / 2.;
    point
}

fn drag_text(visual: &mut VisualTestContext, text: &str, end: usize) {
    let layout = text_layout(visual, text);
    visual.simulate_mouse_down(
        text_point(&layout, 0),
        MouseButton::Left,
        Modifiers::default(),
    );
    let mut end = text_point(&layout, end);
    end.x += px(1.5);
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    visual.run_until_parked();
}

#[gpui_kit::test]
fn user_copy_keeps_safe_source_and_stable_hover_geometry(cx: &mut TestAppContext) {
    let (window, stream, _) = open_copy_stream(cx, "user-copy");
    let source = "中文 **literal**\n\napi_key=sk-proj-abcdefghijklmnopqrstuvwxyz012345\n末尾\n\n";
    let expected = "中文 **literal**\n\napi_key=[REDACTED:provider_credential]\n末尾\n\n";
    let id = stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text(source, cx));
        stream.submit_message(cx);
        stream.accept_composer_submission(source, cx);
        let id = match &stream.entries[0] {
            StreamEntry::User { copy, .. } => copy.id,
            _ => panic!("accepted user source"),
        };
        stream.apply_event(start_run("neighbor"), cx);
        stream.apply_event(text_delta("neighbor", "Adjacent assistant text."), cx);
        id
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let target = selector("message-copy-user", id);
    let bubble = visual.debug_bounds("user-message-bubble").unwrap();
    let action = visual.debug_bounds(target).expect("user copy action");
    let neighbor = visual.debug_bounds("assistant-message").unwrap();
    assert_eq!(action.size, size(px(24.), px(24.)));
    assert_eq!(action.right(), bubble.right());
    assert_eq!(action.top(), bubble.bottom() + px(4.));
    visual.update(|window, cx| window.blur(cx));
    visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
    visual.run_until_parked();
    assert!(!visible_surface(&mut visual, action));
    for _ in 0..3 {
        visual.simulate_mouse_move(bubble.center(), None, Modifiers::default());
        visual.simulate_mouse_move(
            point(action.center().x, bubble.bottom() + px(2.)),
            None,
            Modifiers::default(),
        );
        visual.simulate_mouse_move(action.center(), None, Modifiers::default());
        visual.run_until_parked();
        assert!(visible_surface(&mut visual, action));
        copy_click(&mut visual, target, expected);
        visual.update(|window, cx| window.blur(cx));
        visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        visual.run_until_parked();
        assert!(!visible_surface(&mut visual, action));
        assert_eq!(visual.debug_bounds("user-message-bubble"), Some(bubble));
        assert_eq!(visual.debug_bounds("assistant-message"), Some(neighbor));
        assert_eq!(visual.debug_bounds(target), Some(action));
    }
}

#[gpui_kit::test]
fn live_final_copy_excludes_commentary_tools_and_thinking(cx: &mut TestAppContext) {
    let (window, stream, _) = open_copy_stream(cx, "live-final-copy");
    let commentary = "Reviewing the request.\n\n";
    let answer =
        "最终 **答案**\n\n[文档](https://example.com/answer)\n\n```rust\n    done();\n```\n";
    stream.update(cx, |stream, cx| {
        stream.apply_event(start_run("run"), cx);
        stream.apply_event(thinking_delta("run", "Private process text."), cx);
        stream.apply_event(text_delta("run", commentary), cx);
        stream.apply_event(
            completed(
                "run",
                response(
                    "progress",
                    AssistantResponseKind::Commentary,
                    0,
                    commentary.len(),
                ),
            ),
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("tool", "printf tool"),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "tool".into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: "tool output is not the answer".into(),
                    reused: false,
                    exit_code: Some(0),
                    duration_ms: Some(1),
                    truncated: Some(false),
                    invalid: None,
                },
            },
            cx,
        );
        stream.apply_event(text_delta("run", answer), cx);
        stream.apply_event(
            completed(
                "run",
                response(
                    "final",
                    AssistantResponseKind::Final,
                    commentary.len(),
                    commentary.len() + answer.len(),
                ),
            ),
            cx,
        );
        stream.apply_event(finish_run("run"), cx);
    });
    cx.run_until_parked();
    let ids = stream.read_with(cx, |stream, _| assistant_ids(stream, "run"));
    assert!(ids.len() >= 2);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let target = selector("message-copy-assistant", *ids.last().unwrap());
    copy_click(&mut visual, target, answer);
    let action = visual.debug_bounds(target).unwrap();
    assert_eq!(
        action.left(),
        visual.debug_bounds("assistant-message").unwrap().left()
    );
    let toggle = visual.debug_bounds("run-activity-toggle").unwrap();
    for _ in 0..2 {
        visual.simulate_click(toggle.center(), Modifiers::default());
        visual.run_until_parked();
        for id in &ids[..ids.len() - 1] {
            assert!(
                visual
                    .debug_bounds(selector("message-copy-assistant", *id))
                    .is_none()
            );
        }
        copy_click(&mut visual, target, answer);
    }
}

#[gpui_kit::test]
fn hydrated_final_spans_fragments_and_survives_the_next_turn(cx: &mut TestAppContext) {
    let (window, stream, _) = open_copy_stream(cx, "fragment-final-copy");
    let commentary = "前置说明\n\n";
    let first = "第一段 **中文**\n\n";
    let second = "第二段 🦀\n";
    let answer = format!("{first}{second}");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, "历史问题"),
                    history_text(2, "old", commentary),
                    history_text(2, "old", first),
                    history_text(2, "old", second),
                    history_response(
                        2,
                        "old",
                        response(
                            "progress",
                            AssistantResponseKind::Commentary,
                            0,
                            commentary.len(),
                        ),
                    ),
                    history_response(
                        2,
                        "old",
                        response(
                            "final",
                            AssistantResponseKind::Final,
                            commentary.len(),
                            commentary.len() + answer.len(),
                        ),
                    ),
                ],
                None,
            ),
            cx,
        );
    });
    cx.run_until_parked();
    let ids = stream.read_with(cx, |stream, _| assistant_ids(stream, "old"));
    assert_eq!(ids.len(), 3);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let old = selector("message-copy-assistant", ids[2]);
    for id in &ids[..2] {
        assert!(
            visual
                .debug_bounds(selector("message-copy-assistant", *id))
                .is_none()
        );
    }
    copy_click(&mut visual, old, &answer);
    stream.update(&mut visual, |stream, cx| {
        final_run(stream, "next", "下一轮的独立答案\n", cx)
    });
    visual.run_until_parked();
    let new_id = stream.read_with(&visual, |stream, _| last_assistant_id(stream, "next"));
    copy_click(
        &mut visual,
        selector("message-copy-assistant", new_id),
        "下一轮的独立答案\n",
    );
    copy_click(&mut visual, old, &answer);
}

#[gpui_kit::test]
fn unfinished_commentary_intermediate_empty_and_failed_text_have_no_final_action(
    cx: &mut TestAppContext,
) {
    for scenario in 0..5 {
        let (window, stream, _) = open_copy_stream(cx, "unconfirmed-copy");
        stream.update(cx, |stream, cx| {
            stream.apply_event(start_run("run"), cx);
            if scenario != 3 {
                stream.apply_event(text_delta("run", "Visible partial answer."), cx);
            }
            match scenario {
                1 => {
                    stream.apply_event(
                        completed(
                            "run",
                            response(
                                "progress",
                                AssistantResponseKind::Commentary,
                                0,
                                "Visible partial answer.".len(),
                            ),
                        ),
                        cx,
                    );
                    stream.apply_event(finish_run("run"), cx);
                }
                2 => stream.close_active_segment_before_tool(),
                3 => stream.apply_event(finish_run("run"), cx),
                4 => stream.apply_event(
                    ConversationEvent::Error {
                        message_id: Some("run".into()),
                        error: Arc::new(vega_runtime::VegaError::Provider {
                            status: Some(500),
                            message: "synthetic provider failure".into(),
                            retryable: false,
                        }),
                        execution_duration_ms: Some(1),
                    },
                    cx,
                ),
                _ => {}
            }
        });
        cx.run_until_parked();
        let ids = stream.read_with(cx, |stream, _| assistant_ids(stream, "run"));
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for id in ids {
            assert!(
                visual
                    .debug_bounds(selector("message-copy-assistant", id))
                    .is_none(),
                "scenario {scenario}"
            );
        }
        if scenario == 4 {
            assert!(visual.debug_bounds("assistant-run-failure").is_some());
        }
        if scenario == 0 {
            drag_text(
                &mut visual,
                "Visible partial answer.",
                "Visible partial".len(),
            );
            visual.simulate_keystrokes("cmd-c");
            assert_eq!(clipboard(&visual), "Visible partial");
        }
    }
}

#[gpui_kit::test]
fn invalid_final_metadata_never_authorizes_a_whole_reply_copy(cx: &mut TestAppContext) {
    let text = "说明\n最终答案";
    let cases = [
        vec![response(
            "outside",
            AssistantResponseKind::Final,
            0,
            text.len() + 1,
        )],
        vec![response(
            "utf8",
            AssistantResponseKind::Final,
            1,
            text.len(),
        )],
        vec![
            response(
                "progress",
                AssistantResponseKind::Commentary,
                0,
                "说明\n最".len(),
            ),
            response(
                "overlap",
                AssistantResponseKind::Final,
                "说明\n".len(),
                text.len(),
            ),
        ],
        vec![
            response("same", AssistantResponseKind::Final, 0, text.len()),
            response(
                "same",
                AssistantResponseKind::Final,
                "说明\n".len(),
                text.len(),
            ),
        ],
        vec![response(
            "early",
            AssistantResponseKind::Final,
            0,
            "说明\n".len(),
        )],
        vec![
            response("final", AssistantResponseKind::Final, 0, text.len()),
            response("after", AssistantResponseKind::Commentary, 0, 3),
        ],
    ];
    for responses in cases {
        let (window, stream, _) = open_copy_stream(cx, "invalid-final-copy");
        stream.update(cx, |stream, cx| {
            stream.apply_event(start_run("run"), cx);
            stream.apply_event(text_delta("run", text), cx);
            for response in responses {
                stream.apply_event(completed("run", response), cx);
            }
        });
        cx.run_until_parked();
        let ids = stream.read_with(cx, |stream, _| assistant_ids(stream, "run"));
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        for id in ids {
            assert!(
                visual
                    .debug_bounds(selector("message-copy-assistant", id))
                    .is_none()
            );
        }
    }
}

#[gpui_kit::test]
fn stale_final_actions_disappear_after_append_rollback_and_history_replacement(
    cx: &mut TestAppContext,
) {
    for rollback in [false, true] {
        let (window, stream, _) = open_copy_stream(cx, "stale-final-copy");
        stream.update(cx, |stream, cx| {
            stream.apply_event(start_run("run"), cx);
            stream.apply_event(
                ConversationEvent::AssistantResponseStarted {
                    message_id: "run".into(),
                    text_start_bytes: 0,
                },
                cx,
            );
            stream.apply_event(text_delta("run", "Trusted final."), cx);
            stream.apply_event(
                completed(
                    "run",
                    response(
                        "final",
                        AssistantResponseKind::Final,
                        0,
                        "Trusted final.".len(),
                    ),
                ),
                cx,
            );
        });
        cx.run_until_parked();
        let id = stream.read_with(cx, |stream, _| last_assistant_id(stream, "run"));
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let old = selector("message-copy-assistant", id);
        copy_click(&mut visual, old, "Trusted final.");
        stream.update(&mut visual, |stream, cx| {
            let event = if rollback {
                ConversationEvent::ResponseRetrying {
                    message_id: "run".into(),
                    text_start_bytes: 0,
                    attempt: 1,
                    max_retries: 2,
                }
            } else {
                text_delta("run", " Additional unconfirmed text.")
            };
            stream.apply_event(event, cx);
        });
        visual.run_until_parked();
        assert!(visual.debug_bounds(old).is_none());
    }
    let (window, stream, _) = open_copy_stream(cx, "replace-final-copy");
    stream.update(cx, |stream, cx| final_run(stream, "run", "Old final.", cx));
    cx.run_until_parked();
    let id = stream.read_with(cx, |stream, _| last_assistant_id(stream, "run"));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let old = selector("message-copy-assistant", id);
    copy_click(&mut visual, old, "Old final.");
    stream.update(&mut visual, |stream, cx| {
        assert!(stream.replace_history_window(
            hydration_page(
                vec![history_text(5, "replacement", "Unclassified replacement.")],
                None
            ),
            "replacement",
            cx
        ));
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds(old).is_none());
    let replacement = stream.read_with(&visual, |stream, _| {
        last_assistant_id(stream, "replacement")
    });
    assert!(
        visual
            .debug_bounds(selector("message-copy-assistant", replacement))
            .is_none()
    );
}

#[gpui_kit::test]
fn unclassified_history_keeps_user_code_and_selection_copy(cx: &mut TestAppContext) {
    let (window, stream, _) = open_copy_stream(cx, "history-copy");
    let user = "历史 **问题**\n\n";
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, user),
                    history_text(
                        2,
                        "unclassified",
                        "Historical selection.\n\n```\n    历史代码\n\n```\n",
                    ),
                    history_response(
                        2,
                        "unclassified",
                        response("bad", AssistantResponseKind::Final, 1, 9000),
                    ),
                ],
                None,
            ),
            cx,
        )
    });
    cx.run_until_parked();
    let (user_id, assistant_id) = stream.read_with(cx, |stream, _| {
        let user_id = stream
            .entries
            .iter()
            .find_map(|entry| match entry {
                StreamEntry::User { copy, .. } => Some(copy.id),
                _ => None,
            })
            .unwrap();
        (user_id, last_assistant_id(stream, "unclassified"))
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds(selector("message-copy-assistant", assistant_id))
            .is_none()
    );
    copy_click(&mut visual, selector("message-copy-user", user_id), user);
    copy_click(
        &mut visual,
        code_selector(assistant_id, 0),
        "    历史代码\n\n",
    );
    drag_text(&mut visual, "Historical selection.", "Historical".len());
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(clipboard(&visual), "Historical");
}

#[gpui_kit::test]
fn separate_code_blocks_keep_languages_whitespace_and_list_identity(cx: &mut TestAppContext) {
    let (window, stream, _) = open_copy_stream(cx, "separate-code-copy");
    let document = "```rust\nlet 中文 = 1;\n\n    done();\n```\n\n- item\n\n  ```unknown-language\n    second();\n\n  ```\n\n  ```\n    second();\n  ```\n";
    stream.update(cx, |stream, cx| final_run(stream, "run", document, cx));
    cx.run_until_parked();
    let id = stream.read_with(cx, |stream, _| last_assistant_id(stream, "run"));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for (ordinal, expected) in [
        "let 中文 = 1;\n\n    done();\n",
        "  second();\n\n",
        "  second();\n",
    ]
    .into_iter()
    .enumerate()
    {
        copy_click(&mut visual, code_selector(id, ordinal), expected);
    }
    copy_click(
        &mut visual,
        selector("message-copy-assistant", id),
        document,
    );
}

fn code_nodes(nodes: &[RenderNode]) -> Vec<(Option<String>, String)> {
    nodes
        .iter()
        .flat_map(|node| match node {
            RenderNode::CodeBlock { language, code } => vec![(language.clone(), code.clone())],
            RenderNode::List(list) => list
                .items
                .iter()
                .flat_map(|item| code_nodes(&item.children))
                .collect(),
            RenderNode::BlockQuote { children } => code_nodes(children),
            _ => Vec::new(),
        })
        .collect()
}

fn parsed_codes(stream: &MarkdownStream) -> Vec<(Option<String>, String)> {
    let snapshot = stream.snapshot();
    snapshot
        .blocks
        .iter()
        .flat_map(|block| code_nodes(block.nodes))
        .chain(
            snapshot
                .pending
                .into_iter()
                .flat_map(|block| code_nodes(block.nodes)),
        )
        .collect()
}

#[test]
fn pending_code_projection_excludes_only_synthetic_completion_newlines() {
    let mut stream = MarkdownStream::new();
    stream.append("```rust\n    中文();");
    assert_eq!(
        parsed_codes(&stream),
        vec![(Some("rust".into()), "    中文();".into())]
    );
    stream.append("\n");
    assert_eq!(
        parsed_codes(&stream),
        vec![(Some("rust".into()), "    中文();\n".into())]
    );
    stream.append("\nnext();");
    assert_eq!(
        parsed_codes(&stream),
        vec![(Some("rust".into()), "    中文();\n\nnext();".into())]
    );
    stream.append("\n```\n\n```mystery\n  final\n\n```\n");
    stream.finish();
    assert_eq!(
        parsed_codes(&stream),
        vec![
            (Some("rust".into()), "    中文();\n\nnext();\n".into()),
            (Some("mystery".into()), "  final\n\n".into())
        ]
    );
}

#[gpui_kit::test]
fn streaming_code_copy_uses_latest_pending_and_committed_source(cx: &mut TestAppContext) {
    let (window, stream, _) = open_copy_stream(cx, "streaming-code-copy");
    stream.update(cx, |stream, cx| {
        stream.apply_event(start_run("run"), cx);
        stream.apply_event(text_delta("run", "```rust\n    中文();"), cx);
    });
    cx.run_until_parked();
    let id = stream.read_with(cx, |stream, _| last_assistant_id(stream, "run"));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let first = code_selector(id, 0);
    copy_click(&mut visual, first, "    中文();");
    for (delta, expected) in [
        ("\n", "    中文();\n"),
        ("\nnext();", "    中文();\n\nnext();"),
    ] {
        stream.update(&mut visual, |stream, cx| {
            stream.apply_event(text_delta("run", delta), cx)
        });
        visual.run_until_parked();
        copy_click(&mut visual, first, expected);
    }
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(text_delta("run", "\n```\n\n```\nsecond"), cx);
    });
    visual.run_until_parked();
    copy_click(&mut visual, first, "    中文();\n\nnext();\n");
    copy_click(&mut visual, code_selector(id, 1), "second");
    stream.update(&mut visual, |stream, cx| {
        stream.apply_event(text_delta("run", "\n```\n\n```\n```\n"), cx);
        stream.apply_event(finish_run("run"), cx);
    });
    visual.run_until_parked();
    copy_click(&mut visual, first, "    中文();\n\nnext();\n");
    copy_click(&mut visual, code_selector(id, 1), "second\n");
    visual.write_to_clipboard(ClipboardItem::new_string("keep nonempty clipboard".into()));
    if let Some(empty) = visual.debug_bounds(code_selector(id, 2)) {
        visual.simulate_click(empty.center(), Modifiers::default());
        visual.run_until_parked();
    }
    assert_eq!(clipboard(&visual), "keep nonempty clipboard");
    assert!(
        visual
            .debug_bounds(selector("message-copy-assistant", id))
            .is_none()
    );
}

#[gpui_kit::test]
fn code_header_copy_stays_fixed_when_long_content_scrolls_in_both_themes(cx: &mut TestAppContext) {
    let (window, stream, _) = open_copy_stream(cx, "scrolling-code-copy");
    let line = format!("let value = \"{}\";", "中文 long code ".repeat(30));
    let document = format!("```rust\n{line}\n```\n");
    stream.update(cx, |stream, cx| final_run(stream, "run", &document, cx));
    cx.run_until_parked();
    let id = stream.read_with(cx, |stream, _| last_assistant_id(stream, "run"));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(size(px(960.), px(700.)));
    visual.run_until_parked();
    for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
        visual.update(|window, cx| {
            cx.set_global(palette);
            window.refresh();
        });
        visual.run_until_parked();
        let target = code_selector(id, 0);
        let action = visual.debug_bounds(target).unwrap();
        let code = visual.debug_bounds("markdown-code-0").unwrap();
        let before = text_layout(&mut visual, &line)
            .position_for_index(0)
            .unwrap();
        assert!(action.left() >= code.left() && action.right() <= code.right());
        assert!(action.top() >= code.top() && action.bottom() <= code.bottom());
        visual.simulate_event(ScrollWheelEvent {
            position: point(code.center().x, code.bottom() - px(8.)),
            delta: ScrollDelta::Pixels(point(-px(150.), px(0.))),
            ..Default::default()
        });
        visual.run_until_parked();
        let after = text_layout(&mut visual, &line)
            .position_for_index(0)
            .unwrap();
        assert!(after.x < before.x, "code content must actually scroll");
        assert_eq!(visual.debug_bounds(target), Some(action));
        copy_click(&mut visual, target, &format!("{line}\n"));
    }
}

#[gpui_kit::test]
fn keyboard_copy_preserves_draft_selection_context_menu_and_links(cx: &mut TestAppContext) {
    let (window, stream, submissions) = open_copy_stream(cx, "keyboard-copy");
    let user = "Selectable words https://example.com/copy";
    let answer = "Final **answer**\n\n```rust\nlet v = 1;\n```\n";
    let (user_id, assistant_id) = stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text(user, cx));
        stream.submit_message(cx);
        stream.accept_composer_submission(user, cx);
        let user_id = match &stream.entries[0] {
            StreamEntry::User { copy, .. } => copy.id,
            _ => unreachable!(),
        };
        final_run(stream, "run", answer, cx);
        stream
            .input
            .update(cx, |input, cx| input.set_text("Keep this draft", cx));
        (user_id, last_assistant_id(stream, "run"))
    });
    cx.run_until_parked();
    let initial_submissions = submissions.get();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| window.activate_window());
    visual.run_until_parked();
    drag_text(&mut visual, user, "Selectable".len());
    visual.simulate_keystrokes("cmd-c");
    assert_eq!(clipboard(&visual), "Selectable");
    for (target, expected) in [
        (selector("message-copy-user", user_id), user),
        (selector("message-copy-assistant", assistant_id), answer),
        (code_selector(assistant_id, 0), "let v = 1;\n"),
    ] {
        focus_by_tab(&mut visual, target);
        for key in ["enter", "space"] {
            visual.write_to_clipboard(ClipboardItem::new_string("keyboard sentinel".into()));
            visual.simulate_keystrokes(key);
            visual.run_until_parked();
            assert_eq!(clipboard(&visual), expected);
            assert_eq!(
                stream.read_with(&visual, |stream, cx| stream
                    .input
                    .read(cx)
                    .text()
                    .to_string()),
                "Keep this draft"
            );
            assert_eq!(submissions.get(), initial_submissions);
        }
    }
    drag_text(&mut visual, user, "Selectable words".len());
    let layout = text_layout(&mut visual, user);
    let selected_point = text_point(&layout, "Selectable".len());
    visual.simulate_mouse_down(selected_point, MouseButton::Right, Modifiers::default());
    visual.simulate_mouse_up(selected_point, MouseButton::Right, Modifiers::default());
    visual.run_until_parked();
    visual.write_to_clipboard(ClipboardItem::new_string("menu sentinel".into()));
    visual.simulate_keystrokes("down enter");
    visual.run_until_parked();
    assert_eq!(clipboard(&visual), "Selectable words");
    visual.update(gpui_kit::base::TextSelection::clear);
    let layout = text_layout(&mut visual, user);
    visual.simulate_click(
        text_point(&layout, "Selectable words https://".len()),
        Modifiers::default(),
    );
    visual.executor().advance_clock(Duration::from_millis(501));
    visual.run_until_parked();
    assert_eq!(
        visual.opened_url().as_deref(),
        Some("https://example.com/copy")
    );
    assert_eq!(submissions.get(), initial_submissions);
}

#[gpui_kit::test]
fn remounted_copy_actions_keep_their_source_and_do_not_resume_tail_following(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_copy_stream(cx, "virtual-copy");
    let source = format!("Long message {}\n\n", "中文 source ".repeat(60));
    stream.update(cx, |stream, cx| {
        let mut entries = vec![hydration_user(1, &source)];
        entries
            .extend((2..=90).map(|seq| hydration_user(seq, &format!("Different message {seq}"))));
        stream.apply_history_page(hydration_page(entries, None), cx);
        stream.list.pause_following_tail();
        stream.list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let ids = stream.read_with(cx, |stream, _| {
        stream
            .entries
            .iter()
            .filter_map(|entry| match entry {
                StreamEntry::User { copy, .. } => Some(copy.id),
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let first = selector("message-copy-user", ids[0]);
    let action = visual.debug_bounds(first).unwrap();
    let before = stream.read_with(&visual, |stream, _| {
        (
            stream.list.logical_scroll_top(),
            stream.list.bounds_for_item(0).unwrap(),
        )
    });
    for _ in 0..5 {
        visual.simulate_mouse_move(action.center(), None, Modifiers::default());
        visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        visual.run_until_parked();
    }
    stream.read_with(&visual, |stream, _| {
        let after = stream.list.logical_scroll_top();
        assert_eq!(
            (after.item_ix, after.offset_in_item),
            (before.0.item_ix, before.0.offset_in_item)
        );
        assert_eq!(stream.list.bounds_for_item(0), Some(before.1));
        assert!(!stream.following_tail());
    });
    copy_click(&mut visual, first, &source);
    stream.update(&mut visual, |stream, cx| {
        stream.list.scroll_to(ListOffset {
            item_ix: 80,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds(first).is_none());
    copy_click(
        &mut visual,
        selector("message-copy-user", ids[80]),
        "Different message 81",
    );
    stream.update(&mut visual, |stream, cx| {
        stream.list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    visual.run_until_parked();
    copy_click(&mut visual, first, &source);
    assert!(!stream.read_with(&visual, |stream, _| stream.following_tail()));
}
