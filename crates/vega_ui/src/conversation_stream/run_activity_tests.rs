use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
use std::{cell::Cell, rc::Rc};
use vega_conversation::types::{
    Approval, ContextCompactionStatus, ContextCompactionStatusRecord, ContextCompactionUsageState,
    ThreadStatus, ToolCall, ToolCallStatus, ToolResult,
};

const MESSAGE: &str = "assistant";

fn open_stream(cx: &mut TestAppContext) -> (Entity<ConversationStream>, &mut VisualTestContext) {
    let _ = shimmer::take_painted_text();
    cx.update(|cx| {
        if cx.try_global::<vega_theme::Theme>().is_none() {
            gpui_kit::init(cx);
            cx.set_global(vega_theme::Theme::light());
            cx.set_reduce_motion(true);
        }
    });
    let (stream, visual) = cx.add_window_view(|_, cx| {
        ConversationStream::new(
            Thread {
                reasoning_preferences: None,
                parent_thread_id: None,
                forked_from_thread_id: None,
                forked_from_message_id: None,
                id: "activity-thread".into(),
                backend: TaskBackend::Native,
                project_id: String::new(),
                title: "Activity".into(),
                mode: ThreadMode::Ask,
                permission_mode: PermissionMode::ReadOnly,
                permission_revision: 0,
                draft_authority_epoch: None,
                full_access_grant: None,
                default_permission: None,
                model: "fixture-model".into(),
                status: ThreadStatus::Active,
                pinned: false,
                unread: false,
                created_at: 0,
                updated_at: 0,
            },
            cx,
        )
    });
    apply(
        &stream,
        visual,
        ConversationEvent::MessageStarted {
            message_id: MESSAGE.into(),
            seq: 1,
        },
    );
    (stream, visual)
}

fn apply(
    stream: &Entity<ConversationStream>,
    cx: &mut VisualTestContext,
    event: ConversationEvent,
) {
    stream.update(cx, |stream, cx| stream.apply_event(event, cx));
    cx.run_until_parked();
}

fn bash(id: &str, description: Option<&str>) -> ToolCall {
    ToolCall {
        id: id.into(),
        tool: "bash".into(),
        input_json: r#"{"command":"printf hello"}"#.into(),
        description: description.map(str::to_owned),
        codex_display: None,
    }
}

fn read(id: &str, description: Option<&str>) -> ToolCall {
    ToolCall {
        id: id.into(),
        tool: "read".into(),
        input_json: r#"{"path":"notes.txt"}"#.into(),
        description: description.map(str::to_owned),
        codex_display: None,
    }
}

fn propose(stream: &Entity<ConversationStream>, cx: &mut VisualTestContext, call: ToolCall) {
    apply(stream, cx, ConversationEvent::ToolCallProposed { call });
}

fn start(stream: &Entity<ConversationStream>, cx: &mut VisualTestContext, id: &str) {
    apply(
        stream,
        cx,
        ConversationEvent::ToolCallApproved {
            call_id: id.into(),
            approval: Approval::Once,
        },
    );
    apply(
        stream,
        cx,
        ConversationEvent::ToolCallRunning { call_id: id.into() },
    );
}

fn finish(
    stream: &Entity<ConversationStream>,
    cx: &mut VisualTestContext,
    id: &str,
    result: ToolResult,
) {
    apply(
        stream,
        cx,
        ConversationEvent::ToolCallFinished {
            call_id: id.into(),
            result,
        },
    );
}

fn shell_result(status: ToolCallStatus) -> ToolResult {
    let success = status == ToolCallStatus::Success;
    ToolResult {
        status,
        output: "completed".into(),
        reused: false,
        exit_code: success.then_some(0),
        duration_ms: success.then_some(7_000),
        truncated: success.then_some(false),
        invalid: None,
        codex_display: None,
    }
}

fn tool_id(
    stream: &Entity<ConversationStream>,
    cx: &mut VisualTestContext,
    call_id: &str,
) -> gpui_kit::ElementId {
    cx.update(|_, cx| {
        (
            "tool-activity-toggle",
            stream.read(cx).tool_cards[call_id].entity_id(),
        )
            .into()
    })
}

fn stage_id() -> gpui_kit::ElementId {
    ("run-stage-toggle", 0usize).into()
}

fn assert_row(cx: &mut VisualTestContext, id: gpui_kit::ElementId, label: &str, expanded: bool) {
    cx.update(|window, _| {
        let row = window.find(id);
        assert!(row.visible(), "the activity row must be visible");
        assert_eq!(row.label(), Some(label));
        assert_eq!(row.expanded(), Some(expanded));
    });
}

fn click(cx: &mut VisualTestContext, id: gpui_kit::ElementId) {
    let position = cx.update(|window, _| {
        let row = window.find(id);
        assert!(row.visible(), "the disclosure target must be visible");
        row.bounds().center()
    });
    cx.simulate_mouse_move(position, None, Modifiers::default());
    cx.simulate_click(position, Modifiers::default());
    cx.run_until_parked();
}

fn advance(cx: &mut VisualTestContext, duration: Duration) {
    cx.executor().advance_clock(duration);
    cx.run_until_parked();
}

fn assert_painted(expected: &str) {
    let painted = shimmer::take_painted_text();
    assert!(
        painted.iter().any(|text| text == expected),
        "expected visible shimmer text {expected:?}, painted {painted:?}"
    );
}

fn watch_notifications(
    stream: &Entity<ConversationStream>,
    cx: &mut VisualTestContext,
    call_id: &str,
) -> [Rc<Cell<usize>>; 3] {
    let counts = std::array::from_fn(|_| Rc::new(Cell::new(0)));
    cx.update(|_, cx| {
        let card = stream.read(cx).tool_cards[call_id].clone();
        let group = stream.read(cx).run_activity_groups[MESSAGE].clone();
        let card_count = counts[0].clone();
        let group_count = counts[1].clone();
        let stream_count = counts[2].clone();
        cx.observe(&card, move |_, _| card_count.set(card_count.get() + 1))
            .detach();
        cx.observe(&group, move |_, _| group_count.set(group_count.get() + 1))
            .detach();
        cx.observe(stream, move |_, _| stream_count.set(stream_count.get() + 1))
            .detach();
    });
    counts
}

fn assert_notifications_advance(counts: &[Rc<Cell<usize>>; 3], before: [usize; 3]) {
    for (index, count) in counts.iter().enumerate() {
        assert!(
            count.get() > before[index],
            "missing card, group, or conversation notification at index {index}"
        );
    }
}

#[gpui_kit::test]
fn running_shell_disclosure_updates_the_painted_title_without_forced_refresh(
    cx: &mut TestAppContext,
) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(
        &stream,
        cx,
        bash("shell", Some(" Inspect the working tree. ")),
    );
    start(&stream, cx, "shell");
    let id = tool_id(&stream, cx, "shell");
    assert_row(
        cx,
        id.clone(),
        "Inspect the working tree, expand tool details",
        false,
    );
    assert_painted("Inspect the working tree");

    click(cx, id.clone());
    assert_row(
        cx,
        id.clone(),
        "Running command for 0s, Collapse tool details",
        true,
    );
    assert_painted("Running command for 0s");
    let notifications = watch_notifications(&stream, cx, "shell");
    let before = std::array::from_fn(|index| notifications[index].get());

    advance(cx, Duration::from_secs(1));
    assert_notifications_advance(&notifications, before);
    assert_painted("Running command for 1s");
    assert_row(
        cx,
        id.clone(),
        "Running command for 1s, Collapse tool details",
        true,
    );
    let before = std::array::from_fn(|index| notifications[index].get());

    advance(cx, Duration::from_secs(1));
    assert_notifications_advance(&notifications, before);
    assert_painted("Running command for 2s");
    assert_row(
        cx,
        id.clone(),
        "Running command for 2s, Collapse tool details",
        true,
    );

    cx.update(|window, _| {
        assert_eq!(window.find(id.clone()).focused(), Some(true));
    });
    cx.simulate_keystrokes("enter");
    assert_row(
        cx,
        id.clone(),
        "Inspect the working tree, expand tool details",
        false,
    );
    assert_painted("Inspect the working tree");
    advance(cx, Duration::from_secs(1));
    assert_row(
        cx,
        id.clone(),
        "Inspect the working tree, expand tool details",
        false,
    );
    cx.simulate_keystrokes("space");
    assert_row(
        cx,
        id.clone(),
        "Running command for 3s, Collapse tool details",
        true,
    );
    assert_painted("Running command for 3s");

    finish(&stream, cx, "shell", shell_result(ToolCallStatus::Success));
    assert_row(cx, id, "Ran command in 7s, Collapse tool details", true);
}

#[gpui_kit::test]
fn missing_shell_description_keeps_its_fallback_and_live_duration(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, bash("shell", Some(" \t ")));
    start(&stream, cx, "shell");
    let id = tool_id(&stream, cx, "shell");
    assert_row(
        cx,
        id.clone(),
        "Running printf hello, expand tool details",
        false,
    );
    assert_painted("Running printf hello");
    click(cx, id.clone());
    assert_row(
        cx,
        id,
        "Running command for 0s, Collapse tool details",
        true,
    );
    assert_painted("Running command for 0s");
}

#[gpui_kit::test]
fn running_other_tool_keeps_its_description_when_expanded(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, read("read", Some("Review the notes")));
    start(&stream, cx, "read");
    let id = tool_id(&stream, cx, "read");
    assert_row(
        cx,
        id.clone(),
        "Review the notes, expand tool details",
        false,
    );
    assert_painted("Review the notes");
    click(cx, id.clone());
    assert_row(cx, id, "Review the notes, Collapse tool details", true);
    assert_painted("Review the notes");
}

#[gpui_kit::test]
fn latest_tool_and_thinking_events_do_not_restore_an_older_running_shell(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, bash("old", Some("Inspect the working tree")));
    start(&stream, cx, "old");
    propose(&stream, cx, read("new", None));
    assert_row(
        cx,
        stage_id(),
        "Awaiting approval to read notes.txt, expand stage activity",
        false,
    );
    start(&stream, cx, "new");
    assert_row(
        cx,
        stage_id(),
        "Reading notes.txt, expand stage activity",
        false,
    );
    assert_painted("Reading notes.txt");
    finish(
        &stream,
        cx,
        "new",
        ToolResult {
            status: ToolCallStatus::Success,
            output: "notes".into(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: Some(false),
            invalid: None,
            codex_display: None,
        },
    );
    assert_row(
        cx,
        stage_id(),
        "Read notes.txt, expand stage activity",
        false,
    );

    apply(
        &stream,
        cx,
        ConversationEvent::AssistantResponseStarted {
            message_id: MESSAGE.into(),
            response_id: "response".into(),
            text_start_bytes: 0,
        },
    );
    apply(
        &stream,
        cx,
        ConversationEvent::ThinkingDelta {
            message_id: MESSAGE.into(),
            delta: "Review the output".into(),
        },
    );
    assert_row(cx, stage_id(), "Thinking, expand stage activity", false);
    assert_painted("Thinking");
    apply(
        &stream,
        cx,
        ConversationEvent::ReasoningCompleted {
            message_id: MESSAGE.into(),
            response_id: "response".into(),
        },
    );
    assert_row(cx, stage_id(), "Thought, expand stage activity", false);
}

#[gpui_kit::test]
fn compaction_takes_precedence_over_a_running_shell(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, bash("shell", Some("Inspect the working tree")));
    start(&stream, cx, "shell");
    let card = tool_id(&stream, cx, "shell");
    click(cx, card);
    assert_painted("Running command for 0s");
    let accepted = stream.update(cx, |stream, cx| {
        stream.apply_context_status(
            "activity-thread",
            "fixture-model",
            ContextCompactionStatusRecord {
                generation: 1,
                status: ContextCompactionStatus::Compacting,
                updated_at: 1,
                estimated_tokens: None,
                input_budget: None,
                target_tokens: None,
                source_version: None,
                failure: None,
                usage: ContextCompactionUsageState::Pending,
            },
            cx,
        )
    });
    assert!(accepted, "the current compaction status must be accepted");
    cx.run_until_parked();
    assert_row(
        cx,
        stage_id(),
        "Compacting context, Collapse stage activity",
        true,
    );
    assert_painted("Compacting context");
    advance(cx, Duration::from_secs(1));
    assert_row(
        cx,
        stage_id(),
        "Compacting context, Collapse stage activity",
        true,
    );
}

#[gpui_kit::test]
fn failed_or_cancelled_shell_does_not_invent_elapsed_time(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    for (status, expected) in [
        (
            ToolCallStatus::Failed,
            "Command failed, Collapse tool details",
        ),
        (
            ToolCallStatus::Cancelled,
            "Command cancelled, Collapse tool details",
        ),
    ] {
        let (stream, visual) = open_stream(cx);
        propose(
            &stream,
            visual,
            bash("shell", Some("Inspect the working tree")),
        );
        start(&stream, visual, "shell");
        let id = tool_id(&stream, visual, "shell");
        click(visual, id.clone());
        let _ = shimmer::take_painted_text();
        advance(visual, Duration::from_secs(1));
        assert_painted("Running command for 1s");
        finish(&stream, visual, "shell", shell_result(status));
        assert_row(visual, id, expected, true);
        visual.update(|window, _| window.remove_window());
        visual.run_until_parked();
    }
}
