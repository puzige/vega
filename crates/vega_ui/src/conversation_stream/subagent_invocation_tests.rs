use super::*;
use gpui_kit::base::test_support as observation;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{ElementId, TestAppContext};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{PermissionHook, UserQuestionQueue};
use vega_conversation::types::{
    Approval, PermissionDecision, PermissionRequest, RuntimeUserQuestionRequest,
    SubagentOpenRequested, SubagentSnapshot, SubagentState, ToolCall, ToolCallStatus, ToolResult,
    USER_QUESTION_TOOL, UserQuestionInput, tool_card_input_projection, tool_card_result_projection,
};
use vega_runtime::{RuntimeUserQuestionHook, RuntimeUserQuestionResponse};

fn parent_thread() -> Thread {
    let mut thread = vega_conversation::threads::draft_thread(None, "fixture", "confirm");
    thread.id = "parent".into();
    thread
}

fn child_thread(id: &str) -> Thread {
    let mut thread = parent_thread();
    thread.id = id.into();
    thread.parent_thread_id = Some("parent".into());
    thread
}

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(false));
        cx.set_global(crate::sidebar::OpenedThread(Some(parent_thread())));
        crate::init(cx);
    });
}

fn agent(id: &str, execution: &str, name: &str) -> SubagentSnapshot {
    SubagentSnapshot {
        agent_id: id.into(),
        parent_thread_id: "parent".into(),
        parent_message_id: "parent-run".into(),
        message_id: execution.into(),
        name: name.into(),
        state: SubagentState::Running,
        result: String::new(),
        truncated: false,
    }
}

fn completed(mut agent: SubagentSnapshot, result: &str) -> SubagentSnapshot {
    agent.state = SubagentState::Completed;
    agent.result = result.into();
    agent
}

fn call(id: &str, tool: &str, snapshot: &SubagentSnapshot) -> ToolCall {
    let input = if tool == "spawn_agent" {
        json!({"name": snapshot.name, "task": "A scoped task"})
    } else {
        json!({"agent_id": snapshot.agent_id, "message_id": "first", "task": "Related follow-up"})
    };
    ToolCall {
        id: id.into(),
        tool: tool.into(),
        input_json: input.to_string(),
    }
}

fn receipt(snapshot: &SubagentSnapshot) -> ToolResult {
    ToolResult {
        status: ToolCallStatus::Success,
        output: json!({"agent": snapshot}).to_string(),
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        invalid: None,
    }
}

fn start(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_event(
        ConversationEvent::MessageStarted {
            message_id: "parent-run".into(),
            seq: 2,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::RunStarted {
            message_id: "parent-run".into(),
            started_at: Instant::now(),
        },
        cx,
    );
}

fn invoke(
    stream: &mut ConversationStream,
    id: &str,
    tool: &str,
    snapshot: &SubagentSnapshot,
    cx: &mut Context<ConversationStream>,
) {
    stream.apply_event(
        ConversationEvent::ToolCallProposed {
            call: call(id, tool, snapshot),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::ToolCallApproved {
            call_id: id.into(),
            approval: Approval::Once,
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::ToolCallRunning { call_id: id.into() },
        cx,
    );
    stream.apply_subagent(snapshot.clone(), cx);
    stream.apply_event(
        ConversationEvent::ToolCallFinished {
            call_id: id.into(),
            result: receipt(snapshot),
        },
        cx,
    );
}

fn invocation_rows(window: &Window) -> Vec<gpui_kit::test::ElementSnapshot> {
    observation::snapshots(window)
        .into_iter()
        .filter(|element| {
            element.visible()
                && element.label().is_some_and(|label| {
                    label.starts_with("Delegate ") || label.starts_with("Continue ")
                })
        })
        .collect()
}

fn assert_row(window: &Window, action: &str, name: &str, state: &str) -> ElementId {
    let rows = invocation_rows(window)
        .into_iter()
        .filter(|row| {
            row.label().is_some_and(|label| {
                label.starts_with(action) && label.contains(name) && label.contains(state)
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows.len(),
        1,
        "default transcript must contain one visible {action} {name} {state} entry: {:?}",
        invocation_rows(window)
    );
    rows[0].path().last().unwrap().clone()
}

#[gpui_kit::test]
fn each_call_shows_only_its_invoked_agent_and_current_execution_status(cx: &mut TestAppContext) {
    setup(cx);
    let (parent, cx) = cx.add_window_view(|_, cx| ConversationStream::new(parent_thread(), cx));
    let first = agent("parser", "parser-first", "Parser reviewer");
    let second = agent("layout", "layout-first", "Layout reviewer");
    parent.update(cx, |stream, cx| {
        start(stream, cx);
        invoke(stream, "spawn-parser", "spawn_agent", &first, cx);
        invoke(stream, "spawn-layout", "spawn_agent", &second, cx);
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert_row(window, "Delegate", "Parser reviewer", "Running");
        assert_row(window, "Delegate", "Layout reviewer", "Running");
        assert_eq!(invocation_rows(window).len(), 2);
        assert!(
            observation::snapshots(window)
                .iter()
                .all(|row| !row.label().is_some_and(|label| matches!(
                    label,
                    "Open Parser reviewer"
                        | "Open Layout reviewer"
                        | "Stop Parser reviewer"
                        | "Stop Layout reviewer"
                )))
        );
    });
    parent.update(cx, |stream, cx| {
        stream.apply_subagent(completed(first.clone(), "Parser result"), cx)
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert_row(window, "Delegate", "Parser reviewer", "Completed");
        assert_row(window, "Delegate", "Layout reviewer", "Running");
        assert_eq!(invocation_rows(window).len(), 2);
    });
}

#[gpui_kit::test]
fn reused_invocations_keep_old_status_and_late_finish_does_not_detach_current_stream(
    cx: &mut TestAppContext,
) {
    setup(cx);
    let (parent, cx) = cx.add_window_view(|_, cx| ConversationStream::new(parent_thread(), cx));
    let first = agent("worker", "first", "Worker");
    let first_done = completed(first.clone(), "Original result");
    let second = agent("worker", "second", "Worker");
    let child = cx.new(|cx| ConversationStream::new(child_thread("worker"), cx));
    parent.update(cx, |stream, cx| {
        start(stream, cx);
        invoke(stream, "spawn", "spawn_agent", &first, cx);
        stream.apply_subagent(first_done.clone(), cx);
        invoke(stream, "followup", "followup_agent", &second, cx);
        stream.bind_subagent_stream("parent-run", "worker", child.clone(), cx);
        stream.apply_subagent(first_done.clone(), cx);
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert_row(window, "Delegate", "Worker", "Completed");
        assert_row(window, "Continue", "Worker", "Running");
        assert_eq!(invocation_rows(window).len(), 2);
        assert!(child.read(cx).delegated_run_active);
    });
    let entries = [
        ("spawn", "spawn_agent", &first),
        ("followup", "followup_agent", &second),
    ]
    .into_iter()
    .map(|(id, tool, snapshot)| {
        let input = tool_card_input_projection(&call(id, tool, snapshot));
        let result = tool_card_result_projection(Some(&input), &receipt(snapshot));
        HistoryEntry::Tool {
            seq: 2,
            message_id: "parent-run".into(),
            call_id: id.into(),
            status: ToolCallStatus::Success,
            approval: Some(Approval::Once),
            input: Some(input),
            result: Some(result),
        }
    })
    .collect::<Vec<_>>();
    parent.update(cx, |stream, cx| {
        stream.replace_history_window(
            HistoryPage {
                subagents: vec![first_done, second],
                entries,
                older_cursor: None,
                newer_cursor: None,
                newest_seq: Some(2),
            },
            "parent-run",
            cx,
        );
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert_row(window, "Delegate", "Worker", "Completed");
        assert_row(window, "Continue", "Worker", "Running");
        assert_eq!(invocation_rows(window).len(), 2);
    });
}

#[gpui_kit::test]
fn invocation_mouse_and_keyboard_open_the_child_without_changing_parent_draft(
    cx: &mut TestAppContext,
) {
    setup(cx);
    let (parent, cx) = cx.add_window_view(|_, cx| ConversationStream::new(parent_thread(), cx));
    let opened = Arc::new(Mutex::new(Vec::new()));
    let observed = opened.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&parent, move |_, event: &SubagentOpenRequested, _| {
            observed
                .lock()
                .unwrap()
                .push((event.source_thread_id.clone(), event.thread_id.clone()))
        })
    });
    parent.update(cx, |stream, cx| {
        start(stream, cx);
        invoke(
            stream,
            "spawn",
            "spawn_agent",
            &agent("worker", "first", "Worker"),
            cx,
        );
        stream
            .input
            .update(cx, |input, cx| input.set_text("Keep this draft", cx));
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let id = assert_row(window, "Delegate", "Worker", "Running");
        window.click(id.clone(), cx);
        for _ in 0..40 {
            if window.find(id.clone()).focused() == Some(true) {
                break;
            }
            window.press("tab", cx);
        }
        assert_eq!(window.find(id).focused(), Some(true));
        window.press("enter", cx);
        assert_eq!(parent.read(cx).input.read(cx).text(), "Keep this draft");
        assert_eq!(parent.read(cx).thread.id, "parent");
    });
    assert_eq!(
        *opened.lock().unwrap(),
        vec![
            ("parent".into(), "worker".into()),
            ("parent".into(), "worker".into())
        ]
    );
}

#[gpui_kit::test]
fn rejected_admission_never_renders_a_running_or_completed_child(cx: &mut TestAppContext) {
    setup(cx);
    let (parent, cx) = cx.add_window_view(|_, cx| ConversationStream::new(parent_thread(), cx));
    parent.update(cx, |stream, cx| {
        start(stream, cx);
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: call("spawn", "spawn_agent", &agent("worker", "first", "Worker")),
            },
            cx,
        );
        let mut failed = receipt(&agent("worker", "first", "Worker"));
        failed.status = ToolCallStatus::Failed;
        failed.output = "Tool error: capacity_exceeded".into();
        stream.apply_event(
            ConversationEvent::ToolCallFinished {
                call_id: "spawn".into(),
                result: failed,
            },
            cx,
        );
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let rows = invocation_rows(window);
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0]
                .label()
                .is_some_and(|label| label.contains("Failed") || label.contains("Failure"))
        );
        assert!(!rows[0].label().unwrap().contains("Running"));
        assert!(!rows[0].label().unwrap().contains("Completed"));
        assert!(
            observation::snapshots(window)
                .iter()
                .all(|row| row.label() != Some("Open Worker"))
        );
    });
}

#[gpui_kit::test]
async fn bound_child_permission_and_question_remain_single_actionable_parent_responders(
    cx: &mut TestAppContext,
) {
    setup(cx);
    let (parent, cx) = cx.add_window_view(|_, cx| ConversationStream::new(parent_thread(), cx));
    let permissions = PermissionQueue::new();
    let questions = UserQuestionQueue::new();
    let child = cx.new(|cx| {
        ConversationStream::new_with_request_queues(
            child_thread("worker"),
            permissions.clone(),
            questions.clone(),
            cx,
        )
    });
    let question_input = UserQuestionInput::from_json(&json!({"questions": [{"id": "scope", "header": "Scope", "question": "Which scope?", "options": [{"label": "Parser", "description": "Inspect parser"}, {"label": "Layout", "description": "Inspect layout"}]}]}).to_string()).unwrap();
    let permission_response = permissions.request(
        PermissionRequest {
            call_id: "permission".into(),
            tool: "bash".into(),
            display_target: "echo scoped".into(),
            danger_rule_id: None,
            danger_reason: None,
            external: None,
            acp_options: None,
        },
        CancellationToken::new(),
    );
    let question_response = questions.request(
        RuntimeUserQuestionRequest {
            call_id: "question".into(),
            input: question_input.clone(),
        },
        CancellationToken::new(),
    );
    child.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "first".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: ToolCall {
                    id: "permission".into(),
                    tool: "bash".into(),
                    input_json: json!({"cmd": "echo scoped"}).to_string(),
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: ToolCall {
                    id: "question".into(),
                    tool: USER_QUESTION_TOOL.into(),
                    input_json: question_input.to_json().unwrap(),
                },
            },
            cx,
        );
        stream.install_pending_permission(cx);
        stream.install_pending_user_question(cx);
    });
    parent.update(cx, |stream, cx| {
        start(stream, cx);
        invoke(
            stream,
            "spawn",
            "spawn_agent",
            &agent("worker", "first", "Worker"),
            cx,
        );
        stream.bind_subagent_stream("parent-run", "worker", child.clone(), cx);
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let permission = child.read(cx).active_permission.as_ref().unwrap().clone();
        let button = format!("permission-once-{}", permission.entity_id());
        assert!(window.find(button.clone()).visible());
        window.click(button, cx);
        window.click(("question-option", 0usize), cx);
        window.click(("question-action", 2usize), cx);
        assert!(!permissions.has_pending());
        assert!(!questions.has_pending());
        assert!(parent.read(cx).active_permission.is_none());
        assert!(parent.read(cx).active_user_question.is_none());
    });
    assert_eq!(permission_response.await.unwrap(), PermissionDecision::Once);
    let RuntimeUserQuestionResponse::Answered(answers) = question_response.await else {
        panic!("the real question responder must receive the selected answer")
    };
    assert_eq!(answers.answers["scope"].answers, vec!["Parser"]);
}
