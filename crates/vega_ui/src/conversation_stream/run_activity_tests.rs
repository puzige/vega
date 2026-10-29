use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
use std::{cell::Cell, rc::Rc, sync::OnceLock};
use vega_conversation::types::{
    Approval, BashStartClock, ContextCompactionStatus, ContextCompactionStatusRecord,
    ContextCompactionUsageState, ThreadStatus, ToolCall, ToolCallStatus, ToolResult,
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

fn start(
    stream: &Entity<ConversationStream>,
    cx: &mut VisualTestContext,
    id: &str,
    bash_started_at: Option<Arc<OnceLock<Instant>>>,
) {
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
        ConversationEvent::ToolCallRunning {
            call_id: id.into(),
            bash_started_at: bash_started_at.map(BashStartClock::from_test_start),
        },
    );
}

fn publish_bash_start(clock: &OnceLock<Instant>, cx: &VisualTestContext) {
    assert!(
        clock.set(cx.executor().now()).is_ok(),
        "the Bash start clock can only be published once"
    );
}

fn start_bash(stream: &Entity<ConversationStream>, cx: &mut VisualTestContext, id: &str) {
    let clock = Arc::new(OnceLock::new());
    publish_bash_start(&clock, cx);
    start(stream, cx, id, Some(clock));
}

fn start_waiting_bash(
    stream: &Entity<ConversationStream>,
    cx: &mut VisualTestContext,
    id: &str,
) -> Arc<OnceLock<Instant>> {
    let clock = Arc::new(OnceLock::new());
    start(stream, cx, id, Some(clock.clone()));
    clock
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

fn assert_waiting_shell(cx: &mut VisualTestContext, id: gpui_kit::ElementId, expanded: bool) {
    cx.update(|window, _| {
        let row = window.find(id);
        assert!(row.visible(), "the waiting activity row must be visible");
        assert_eq!(row.expanded(), Some(expanded));
        let label = row.label().expect("the activity row must have a label");
        let suffix = if expanded {
            ", Collapse tool details"
        } else {
            ", expand tool details"
        };
        let title = label
            .strip_suffix(suffix)
            .expect("the activity row must describe its disclosure action");
        assert!(!title.is_empty(), "the waiting title must not be empty");
        assert!(
            !title.starts_with("Running") && !title.contains("Running command for"),
            "the waiting title must not claim a Shell has started: {title:?}"
        );
        assert!(
            !title
                .split(|character: char| !character.is_ascii_alphanumeric())
                .any(|word| {
                    word.strip_suffix('s').is_some_and(|seconds| {
                        !seconds.is_empty() && seconds.chars().all(|digit| digit.is_ascii_digit())
                    })
                }),
            "the waiting title must not invent elapsed seconds: {title:?}"
        );
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

fn assert_no_running_shell_title_painted() {
    let painted = shimmer::take_painted_text();
    assert!(
        painted
            .iter()
            .all(|text| { !text.starts_with("Running") && !text.contains("Running command for") }),
        "a Shell without an active start clock must not paint a running title: {painted:?}"
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
    start_bash(&stream, cx, "shell");
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
    assert_row(
        cx,
        id.clone(),
        "Ran command in 7s, Collapse tool details",
        true,
    );
    let before = notifications[0].get();
    let _ = shimmer::take_painted_text();
    advance(cx, Duration::from_secs(3));
    assert_eq!(notifications[0].get(), before);
    assert_no_running_shell_title_painted();
    assert_row(cx, id, "Ran command in 7s, Collapse tool details", true);
}

#[gpui_kit::test]
fn shell_waits_for_process_start_before_automatically_painting_elapsed_time(
    cx: &mut TestAppContext,
) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, bash("shell", Some("Inspect the working tree")));
    let clock = start_waiting_bash(&stream, cx, "shell");
    let id = tool_id(&stream, cx, "shell");
    assert!(clock.get().is_none());
    assert_row(
        cx,
        id.clone(),
        "Inspect the working tree, expand tool details",
        false,
    );
    assert_painted("Inspect the working tree");
    click(cx, id.clone());
    assert_no_running_shell_title_painted();
    assert_waiting_shell(cx, id.clone(), true);

    advance(cx, Duration::from_secs(30));
    assert!(clock.get().is_none());
    assert_no_running_shell_title_painted();
    assert_waiting_shell(cx, id.clone(), true);
    let notifications = watch_notifications(&stream, cx, "shell");
    let before = std::array::from_fn(|index| notifications[index].get());
    let _ = shimmer::take_painted_text();

    publish_bash_start(&clock, cx);
    advance(cx, Duration::from_millis(250));
    assert_notifications_advance(&notifications, before);
    assert_painted("Running command for 0s");
    assert_row(
        cx,
        id.clone(),
        "Running command for 0s, Collapse tool details",
        true,
    );
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

    click(cx, id.clone());
    assert_row(
        cx,
        id.clone(),
        "Inspect the working tree, expand tool details",
        false,
    );
    assert_painted("Inspect the working tree");
    advance(cx, Duration::from_secs(2));
    assert_row(
        cx,
        id.clone(),
        "Inspect the working tree, expand tool details",
        false,
    );
    click(cx, id.clone());
    assert_painted("Running command for 3s");
    assert_row(
        cx,
        id.clone(),
        "Running command for 3s, Collapse tool details",
        true,
    );
    finish(&stream, cx, "shell", shell_result(ToolCallStatus::Success));
    assert_row(cx, id, "Ran command in 7s, Collapse tool details", true);
}

#[gpui_kit::test]
fn shell_first_expanded_after_running_shows_cumulative_process_time(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, bash("shell", Some("Inspect the working tree")));
    let clock = start_waiting_bash(&stream, cx, "shell");
    let id = tool_id(&stream, cx, "shell");
    advance(cx, Duration::from_secs(30));
    assert!(clock.get().is_none());
    assert_no_running_shell_title_painted();
    assert_row(
        cx,
        id.clone(),
        "Inspect the working tree, expand tool details",
        false,
    );

    let notifications = watch_notifications(&stream, cx, "shell");
    let before = std::array::from_fn(|index| notifications[index].get());
    publish_bash_start(&clock, cx);
    advance(cx, Duration::from_millis(5_250));
    assert_notifications_advance(&notifications, before);
    assert_row(
        cx,
        id.clone(),
        "Inspect the working tree, expand tool details",
        false,
    );
    let _ = shimmer::take_painted_text();
    click(cx, id.clone());
    assert_painted("Running command for 5s");
    assert_row(
        cx,
        id,
        "Running command for 5s, Collapse tool details",
        true,
    );
}

#[gpui_kit::test]
fn late_process_start_after_terminal_cannot_restart_the_clock(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    for (status, expected) in [
        (
            ToolCallStatus::Success,
            "Ran command in 7s, Collapse tool details",
        ),
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
        let clock = start_waiting_bash(&stream, visual, "shell");
        let id = tool_id(&stream, visual, "shell");
        click(visual, id.clone());
        assert_no_running_shell_title_painted();
        assert_waiting_shell(visual, id.clone(), true);
        finish(&stream, visual, "shell", shell_result(status));
        assert!(clock.get().is_none());
        assert_row(visual, id.clone(), expected, true);

        let notifications = watch_notifications(&stream, visual, "shell");
        let before = notifications[0].get();
        let _ = shimmer::take_painted_text();
        publish_bash_start(&clock, visual);
        advance(visual, Duration::from_secs(3));
        assert_eq!(notifications[0].get(), before);
        assert_no_running_shell_title_painted();
        assert_row(visual, id.clone(), expected, true);
        click(visual, id.clone());
        click(visual, id.clone());
        assert_no_running_shell_title_painted();
        assert_row(visual, id, expected, true);
        visual.update(|window, _| window.remove_window());
        visual.run_until_parked();
    }
}

#[gpui_kit::test]
fn missing_shell_description_keeps_its_fallback_and_live_duration(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, bash("shell", Some(" \t ")));
    start_bash(&stream, cx, "shell");
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
fn shell_without_usable_description_stays_neutral_until_its_clock_starts(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    for description in [None, Some(" \t ")] {
        let (stream, visual) = open_stream(cx);
        propose(&stream, visual, bash("shell", description));
        let clock = start_waiting_bash(&stream, visual, "shell");
        let id = tool_id(&stream, visual, "shell");
        assert_no_running_shell_title_painted();
        assert_waiting_shell(visual, id.clone(), false);
        click(visual, id.clone());
        assert_no_running_shell_title_painted();
        assert_waiting_shell(visual, id.clone(), true);
        advance(visual, Duration::from_secs(30));
        assert!(clock.get().is_none());
        assert_no_running_shell_title_painted();
        assert_waiting_shell(visual, id.clone(), true);

        let notifications = watch_notifications(&stream, visual, "shell");
        let before = std::array::from_fn(|index| notifications[index].get());
        let _ = shimmer::take_painted_text();
        publish_bash_start(&clock, visual);
        advance(visual, Duration::from_millis(250));
        assert_notifications_advance(&notifications, before);
        assert_painted("Running command for 0s");
        assert_row(
            visual,
            id.clone(),
            "Running command for 0s, Collapse tool details",
            true,
        );
        click(visual, id.clone());
        assert_row(
            visual,
            id,
            "Running printf hello, expand tool details",
            false,
        );
        assert_painted("Running printf hello");
        visual.update(|window, _| window.remove_window());
        visual.run_until_parked();
    }
}

#[gpui_kit::test]
fn running_other_tool_keeps_its_description_when_expanded(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, read("read", Some("Review the notes")));
    start(&stream, cx, "read", None);
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
    start_bash(&stream, cx, "old");
    propose(&stream, cx, read("new", None));
    assert_row(
        cx,
        stage_id(),
        "Awaiting approval to read notes.txt, expand stage activity",
        false,
    );
    start(&stream, cx, "new", None);
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
fn older_shell_start_and_ticks_do_not_replace_a_newer_tool(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, bash("old", Some("Inspect the working tree")));
    let old_clock = start_waiting_bash(&stream, cx, "old");
    propose(&stream, cx, read("new", None));
    start(&stream, cx, "new", None);
    assert_row(
        cx,
        stage_id(),
        "Reading notes.txt, expand stage activity",
        false,
    );
    assert_painted("Reading notes.txt");

    let notifications = watch_notifications(&stream, cx, "old");
    let before = notifications[0].get();
    publish_bash_start(&old_clock, cx);
    advance(cx, Duration::from_millis(250));
    assert!(notifications[0].get() > before);
    assert_no_running_shell_title_painted();
    assert_row(
        cx,
        stage_id(),
        "Reading notes.txt, expand stage activity",
        false,
    );
    let before = notifications[0].get();
    advance(cx, Duration::from_secs(1));
    assert!(notifications[0].get() > before);
    assert_no_running_shell_title_painted();
    assert_row(
        cx,
        stage_id(),
        "Reading notes.txt, expand stage activity",
        false,
    );
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
    let before = notifications[0].get();
    advance(cx, Duration::from_secs(1));
    assert!(notifications[0].get() > before);
    assert_no_running_shell_title_painted();
    assert_row(
        cx,
        stage_id(),
        "Read notes.txt, expand stage activity",
        false,
    );
}

#[gpui_kit::test]
fn concurrent_shells_use_independent_start_clocks(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, bash("first", Some("Inspect the working tree")));
    let first_clock = start_waiting_bash(&stream, cx, "first");
    propose(&stream, cx, bash("second", Some("Review build status")));
    let second_clock = start_waiting_bash(&stream, cx, "second");
    assert_row(
        cx,
        stage_id(),
        "Review build status, expand stage activity",
        false,
    );
    assert_painted("Review build status");
    click(cx, stage_id());
    let first = tool_id(&stream, cx, "first");
    let second = tool_id(&stream, cx, "second");
    click(cx, first.clone());
    click(cx, second.clone());
    assert_waiting_shell(cx, first.clone(), true);
    assert_waiting_shell(cx, second.clone(), true);

    publish_bash_start(&first_clock, cx);
    advance(cx, Duration::from_millis(250));
    assert_row(
        cx,
        first.clone(),
        "Running command for 0s, Collapse tool details",
        true,
    );
    assert!(second_clock.get().is_none());
    assert_waiting_shell(cx, second.clone(), true);
    advance(cx, Duration::from_secs(2));
    assert_row(
        cx,
        first.clone(),
        "Running command for 2s, Collapse tool details",
        true,
    );
    assert_waiting_shell(cx, second.clone(), true);

    publish_bash_start(&second_clock, cx);
    advance(cx, Duration::from_millis(250));
    assert_row(
        cx,
        first.clone(),
        "Running command for 2s, Collapse tool details",
        true,
    );
    assert_row(
        cx,
        second.clone(),
        "Running command for 0s, Collapse tool details",
        true,
    );
    advance(cx, Duration::from_secs(1));
    assert_row(
        cx,
        first,
        "Running command for 3s, Collapse tool details",
        true,
    );
    assert_row(
        cx,
        second,
        "Running command for 1s, Collapse tool details",
        true,
    );
}

#[gpui_kit::test]
fn compaction_takes_precedence_over_a_running_shell(cx: &mut TestAppContext) {
    assert_eq!(vega_i18n::language(), "en");
    let (stream, cx) = open_stream(cx);
    propose(&stream, cx, bash("shell", Some("Inspect the working tree")));
    start_bash(&stream, cx, "shell");
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
        start_bash(&stream, visual, "shell");
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
