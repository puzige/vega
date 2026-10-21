use super::*;
use crate::icons::Icon;
use crate::tool_card::retained_headline_observation;
use gpui_kit::test::{TestSupportExt, TestWindowExt};
use gpui_kit::{AppContext, EntityId, TestAppContext, WindowHandle, size};
use vega_conversation::agent::PermissionHook;
use vega_conversation::history::AssistantStatus;
use vega_conversation::types::{
    Approval, AssistantResponseKind, AssistantResponseMetadata, ContextCompactionStatus,
    ContextCompactionStatusRecord, ContextCompactionUsageState, ConversationStopReason,
    PermissionRequest, ReadOnlyToolKind, RuntimeUserQuestionRequest, ThreadStatus, ToolCall,
    ToolCallStatus, ToolCardInputProjection, ToolCardResultProjection, ToolOutputChunk, ToolResult,
    UserQuestion, UserQuestionInput, UserQuestionOption,
};
use vega_runtime::RuntimeUserQuestionHook;

const MESSAGE: &str = "retained-message";
const RESPONSE: &str = "initial-response";
const WIDTH: f32 = 768.;

type ToolSnapshot = (
    ToolCallStatus,
    Option<u64>,
    Option<ToolCardResultProjection>,
);

struct HeaderObservation {
    node: gpui_kit::accesskit::Node,
    icon: Option<Icon>,
}

thread_local! {
    static HEADERS: std::cell::RefCell<HashMap<usize, HeaderObservation>> = std::cell::RefCell::new(HashMap::new());
}

pub(super) fn observe_header(segment: usize, header: &impl gpui_kit::Element, icon: Option<Icon>) {
    let mut node = gpui_kit::accesskit::Node::new(gpui_kit::Role::GenericContainer);
    header.write_a11y_info(&mut node);
    HEADERS.with(|headers| {
        headers
            .borrow_mut()
            .insert(segment, HeaderObservation { node, icon });
    });
}

struct StageHost {
    stream: Entity<ConversationStream>,
    segment: Option<usize>,
    visible: bool,
}

impl Render for StageHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.stream.update(cx, |stream, cx| {
            stream.settle_final_presentation(window, cx)
        });
        let group = self
            .stream
            .read(cx)
            .run_activity_groups
            .get(MESSAGE)
            .cloned();
        let content = group.and_then(|group| {
            if group.read(cx).process_in_details() {
                return Some(RunActivityGroup::render(group, window, cx));
            }
            let segment = self
                .segment
                .or_else(|| group.read(cx).segments.len().checked_sub(1))?;
            Some(RunActivityGroup::render_segment_with_top(
                group, segment, 0., window, cx,
            ))
        });
        div().flex().flex_col().child(
            div()
                .id("stage-viewport")
                .test_support()
                .w(px(WIDTH))
                .flex_shrink_0()
                .text_size(px(Typography::CONVERSATION))
                .when(!self.visible, |view| view.hidden())
                .children(content),
        )
    }
}

struct Conversation {
    stream: Entity<ConversationStream>,
    window: WindowHandle<StageHost>,
}

impl Conversation {
    fn idle(cx: &mut TestAppContext) -> Self {
        HEADERS.with(|headers| headers.borrow_mut().clear());
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(vega_theme::Theme::light());
        });
        let stream = cx.new(|cx| ConversationStream::new(thread(), cx));
        let window = cx.open_window(size(px(WIDTH), px(900.)), |_, _| StageHost {
            stream: stream.clone(),
            segment: None,
            visible: true,
        });
        window
            .update(cx, |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        Self { stream, window }
    }

    fn new(cx: &mut TestAppContext) -> Self {
        let this = Self::idle(cx);
        this.stream
            .update(cx, |stream, cx| stream.begin_composer_run(cx));
        this.event(
            ConversationEvent::MessageStarted {
                message_id: MESSAGE.into(),
                seq: 1,
            },
            cx,
        );
        this.response_started(RESPONSE, 0, cx);
        this
    }

    fn aggregate(cx: &mut TestAppContext) -> Self {
        let this = Self::new(cx);
        this.read("seed", "seed.rs", cx);
        this.start("seed", cx);
        this.finish("seed", ToolCallStatus::Success, cx);
        this
    }

    fn event(&self, event: ConversationEvent, cx: &mut TestAppContext) {
        self.stream
            .update(cx, |stream, cx| stream.apply_event(event, cx));
    }

    fn summary(&self, delta: &str, cx: &mut TestAppContext) {
        self.event(
            ConversationEvent::SummaryDelta {
                message_id: MESSAGE.into(),
                delta: delta.into(),
            },
            cx,
        );
    }

    fn thinking(&self, delta: &str, cx: &mut TestAppContext) {
        self.event(
            ConversationEvent::ThinkingDelta {
                message_id: MESSAGE.into(),
                delta: delta.into(),
            },
            cx,
        );
    }

    fn text(&self, delta: &str, cx: &mut TestAppContext) {
        self.event(
            ConversationEvent::TextDelta {
                message_id: MESSAGE.into(),
                delta: delta.into(),
            },
            cx,
        );
    }

    fn propose(&self, id: &str, tool: &str, input: serde_json::Value, cx: &mut TestAppContext) {
        self.event(
            ConversationEvent::ToolCallProposed {
                call: ToolCall {
                    id: id.into(),
                    tool: tool.into(),
                    input_json: input.to_string(),
                },
            },
            cx,
        );
    }

    fn command(&self, id: &str, command: &str, cx: &mut TestAppContext) {
        self.propose(
            id,
            "bash",
            serde_json::json!({"command": command, "timeout": null}),
            cx,
        );
    }

    fn read(&self, id: &str, path: &str, cx: &mut TestAppContext) {
        self.propose(id, "read", serde_json::json!({"path": path}), cx);
    }

    fn approve(&self, id: &str, cx: &mut TestAppContext) {
        self.event(
            ConversationEvent::ToolCallApproved {
                call_id: id.into(),
                approval: Approval::Once,
            },
            cx,
        );
    }

    fn running(&self, id: &str, cx: &mut TestAppContext) {
        self.event(
            ConversationEvent::ToolCallRunning { call_id: id.into() },
            cx,
        );
    }

    fn start(&self, id: &str, cx: &mut TestAppContext) {
        self.approve(id, cx);
        self.running(id, cx);
    }

    fn result(&self, id: &str, result: ToolResult, cx: &mut TestAppContext) {
        self.event(
            ConversationEvent::ToolCallFinished {
                call_id: id.into(),
                result,
            },
            cx,
        );
    }

    fn finish(&self, id: &str, status: ToolCallStatus, cx: &mut TestAppContext) {
        let bash = cx.read(|cx| {
            self.stream.read(cx).tool_cards[id]
                .read(cx)
                .stage_activity_category()
                == ToolActivityCategory::Shell
        });
        self.result(id, result(status, bash, false), cx);
    }

    fn complete_reasoning(&self, response_id: &str, cx: &mut TestAppContext) {
        self.event(
            ConversationEvent::ReasoningCompleted {
                message_id: MESSAGE.into(),
                response_id: response_id.into(),
            },
            cx,
        );
    }

    fn response_started(&self, id: &str, text_start_bytes: usize, cx: &mut TestAppContext) {
        self.event(
            ConversationEvent::AssistantResponseStarted {
                message_id: MESSAGE.into(),
                response_id: id.into(),
                text_start_bytes,
            },
            cx,
        );
    }

    fn response_completed(
        &self,
        id: &str,
        start: usize,
        end: usize,
        kind: AssistantResponseKind,
        cx: &mut TestAppContext,
    ) {
        self.event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: MESSAGE.into(),
                response: AssistantResponseMetadata {
                    response_id: id.into(),
                    text_start_bytes: start,
                    text_end_bytes: end,
                    kind,
                },
            },
            cx,
        );
    }

    fn retry(&self, cx: &mut TestAppContext) {
        self.event(
            ConversationEvent::ResponseRetrying {
                message_id: MESSAGE.into(),
                text_start_bytes: 0,
                attempt: 1,
                max_retries: 3,
            },
            cx,
        );
    }

    fn group(&self, cx: &TestAppContext) -> Entity<RunActivityGroup> {
        cx.read(|cx| self.stream.read(cx).run_activity_groups[MESSAGE].clone())
    }

    fn card(&self, id: &str, cx: &TestAppContext) -> Entity<ToolCard> {
        cx.read(|cx| self.stream.read(cx).tool_cards[id].clone())
    }

    fn tool_snapshot(&self, id: &str, cx: &TestAppContext) -> ToolSnapshot {
        cx.read(|cx| {
            retained_headline_observation::snapshot(self.stream.read(cx).tool_cards[id].read(cx))
        })
    }

    fn draw(&self, cx: &mut TestAppContext) {
        retained_headline_observation::clear_footers();
        self.stream
            .update(cx, |stream, cx| stream.refresh_run_notices(cx));
        gpui_kit::AnyWindowHandle::from(self.window)
            .update(cx, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }

    fn assert_stage(&self, title: &str, icon: Option<Icon>, live: bool, cx: &mut TestAppContext) {
        HEADERS.with(|headers| headers.borrow_mut().clear());
        self.draw(cx);
        let group = self.group(cx);
        cx.read(|cx| {
            let group = group.read(cx);
            let selected = self.window.read(cx).unwrap().segment;
            let index = selected.unwrap_or(group.segments.len() - 1);
            assert_eq!(
                shimmer::observation::rendered_title(group.segments[index].title_view.read(cx)),
                (title, live)
            );
            let expandable = group.segment_can_expand(index, cx);
            let expanded = group.segments[index].expanded;
            let label = if expandable {
                format!(
                    "{title}, {} stage activity",
                    if expanded { "collapse" } else { "expand" }
                )
            } else {
                title.to_owned()
            };
            HEADERS.with(|headers| {
                let headers = headers.borrow();
                let header = headers.get(&index).expect("rendered stage header");
                assert_eq!(header.node.label(), Some(label.as_str()));
                assert_eq!(header.node.is_expanded(), expandable.then_some(expanded));
                assert_eq!(header.icon, icon);
            });
        });
    }

    fn assert_tool_header(&self, id: &str, label: &str, expanded: bool, cx: &mut TestAppContext) {
        self.draw(cx);
        let (node, icon) = retained_headline_observation::header(self.card(id, cx).entity_id());
        assert_eq!(node.label(), Some(label));
        assert_eq!(node.is_expanded(), Some(expanded));
        assert_eq!(icon, Icon::Terminal);
    }

    fn frame_demand(&self, cx: &mut TestAppContext) -> usize {
        gpui_kit::AnyWindowHandle::from(self.window)
            .update(cx, |_, window, cx| {
                window.simulate_next_frame(cx);
                window.render_frame(cx);
                window.simulate_next_frame(cx)
            })
            .unwrap()
    }

    fn advance(&self, seconds: u64, cx: &mut TestAppContext) {
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(seconds));
        cx.run_until_parked();
        self.draw(cx);
    }

    fn details(&self, cx: &TestAppContext) -> Vec<(EntityId, String)> {
        let group = self.group(cx);
        cx.read(|cx| {
            group
                .read(cx)
                .children()
                .into_iter()
                .flat_map(|child| match child {
                    RunActivityChild::Thinking(card) => {
                        vec![(card.entity_id(), card.read(cx).source_copy.original_text())]
                    }
                    RunActivityChild::Tool(card) => vec![(card.entity_id(), String::new())],
                    RunActivityChild::ToolGroup(group) => group
                        .read(cx)
                        .children()
                        .iter()
                        .map(|card| (card.entity_id(), String::new()))
                        .collect(),
                    RunActivityChild::Artifact(card) => vec![(card.entity_id(), String::new())],
                })
                .collect()
        })
    }

    fn visible_details(&self, cx: &TestAppContext) -> Vec<EntityId> {
        let group = self.group(cx);
        cx.read(|cx| {
            RunActivityGroup::compact_children(group.read(cx).visible_segment_children(0, cx), cx)
                .into_iter()
                .map(|child| match child {
                    RunActivityChild::Thinking(card) => card.entity_id(),
                    RunActivityChild::Tool(card) => card.entity_id(),
                    RunActivityChild::ToolGroup(group) => group.entity_id(),
                    RunActivityChild::Artifact(card) => card.entity_id(),
                })
                .collect()
        })
    }

    fn toggle(&self, key: &str, cx: &mut TestAppContext) {
        self.draw(cx);
        let group = self.group(cx);
        gpui_kit::AnyWindowHandle::from(self.window)
            .update(cx, |_, window, cx| {
                let focus = group.read(cx).segments[0].header_focus.clone();
                focus.focus(window, cx);
                window.render_frame(cx);
                window.press(key, cx);
            })
            .unwrap();
    }

    fn assert_geometry_and_focus(&self, expanded: bool, cx: &mut TestAppContext) {
        self.draw(cx);
        let group = self.group(cx);
        gpui_kit::AnyWindowHandle::from(self.window)
            .update(cx, |_, window, cx| {
                assert_eq!(group.read(cx).segments[0].expanded, expanded);
                assert!(group.read(cx).segments[0].header_focus.is_focused(window));
                let actual = window.find("stage-viewport").bounds().size.height;
                let estimated = group
                    .read(cx)
                    .estimated_segment_height_with_top(0, WIDTH, 0., cx);
                assert!(
                    (f32::from(actual) - estimated).abs() < 2.,
                    "painted {actual:?}, estimated {estimated}"
                );
                if !expanded {
                    assert_eq!(actual, px(Layout::CONVERSATION_TOOL_HEIGHT));
                }
            })
            .unwrap();
    }

    fn select_segment(&self, segment: usize, cx: &mut TestAppContext) {
        self.window
            .update(cx, |host, _, _| host.segment = Some(segment))
            .unwrap();
    }

    fn retained_command(&self, cx: &mut TestAppContext) {
        self.command("command", "printf retained", cx);
        self.start("command", cx);
        self.finish("command", ToolCallStatus::Success, cx);
    }
}

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: "retained-thread".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Synthetic conversation".into(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        model: "synthetic-model".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    }
}

fn result(status: ToolCallStatus, bash: bool, reused: bool) -> ToolResult {
    let success = status == ToolCallStatus::Success;
    ToolResult {
        status,
        output: "synthetic result".into(),
        reused,
        exit_code: (bash && success).then_some(0),
        duration_ms: (bash && success).then_some(12),
        truncated: (success && !reused).then_some(false),
        invalid: None,
    }
}

#[gpui_kit::test]
fn successful_command_stays_static_until_real_progress_without_a_category_frame(
    cx: &mut TestAppContext,
) {
    let live = Conversation::aggregate(cx);
    live.command("command", "printf retained", cx);
    live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
    live.approve("command", cx);
    live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
    live.running("command", cx);
    live.assert_stage("Running printf retained", Some(Icon::Terminal), true, cx);
    assert!(live.frame_demand(cx) > 0);
    live.finish("command", ToolCallStatus::Success, cx);
    live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
    assert_eq!(live.frame_demand(cx), 0);
    live.advance(120, cx);
    live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
    let group = live.group(cx);
    cx.read(|cx| assert_eq!(group.read(cx).status(), RunActivityStatus::Running));
}

#[gpui_kit::test]
fn pending_events_and_unreadable_summary_do_not_replace_a_retained_command(
    cx: &mut TestAppContext,
) {
    let live = Conversation::aggregate(cx);
    live.retained_command(cx);
    live.response_started("pending-response", 0, cx);
    live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
    live.command("next", "printf next", cx);
    live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
    live.approve("next", cx);
    live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
    for delta in [" \n\t", "**", "unfinished"] {
        live.summary(delta, cx);
        live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
    }
    live.running("next", cx);
    live.assert_stage("Running printf next", Some(Icon::Terminal), true, cx);
    live.summary("**", cx);
    live.assert_stage("unfinished", None, true, cx);
    live.finish("next", ToolCallStatus::Success, cx);
    live.assert_stage("unfinished", None, true, cx);
    live.complete_reasoning("pending-response", cx);
    live.assert_stage("unfinished", None, false, cx);
}

#[gpui_kit::test]
fn detail_only_thinking_replaces_retention_without_becoming_a_retained_summary(
    cx: &mut TestAppContext,
) {
    let live = Conversation::aggregate(cx);
    live.retained_command(cx);
    live.response_started("detail-response", 0, cx);
    live.thinking("Synthetic detailed reasoning stays in its source.", cx);
    live.assert_stage("Thinking", None, true, cx);
    assert_eq!(
        live.details(cx).last().unwrap().1,
        "Synthetic detailed reasoning stays in its source."
    );
    live.complete_reasoning("detail-response", cx);
    live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
    live.advance(30, cx);
    live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
}

#[gpui_kit::test]
fn detail_only_thinking_in_the_same_tool_free_segment_does_not_revive_an_older_summary(
    cx: &mut TestAppContext,
) {
    let live = Conversation::new(cx);
    live.summary("**Retained review**\n", cx);
    live.assert_stage("Retained review", None, true, cx);
    live.complete_reasoning(RESPONSE, cx);
    live.assert_stage("Retained review", None, false, cx);
    let summary = live.details(cx);
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].1, "**Retained review**\n");
    live.response_started("detail-response", 0, cx);
    live.assert_stage("Retained review", None, false, cx);
    let raw = "Synthetic detailed reasoning stays in its source.";
    live.thinking(raw, cx);
    live.assert_stage("Thinking", None, true, cx);
    let details = live.details(cx);
    assert_eq!(details.len(), 2);
    assert_eq!(&details[..1], summary.as_slice());
    assert_eq!(details[1].1, raw);
    live.complete_reasoning("detail-response", cx);
    live.assert_stage("Thought", None, false, cx);
    let group = live.group(cx);
    cx.read(|cx| {
        let group = group.read(cx);
        assert_eq!(group.segments.len(), 1);
        assert!(group.stage_tool_calls(0, cx).is_empty());
        assert_eq!(group.status(), RunActivityStatus::Running);
        assert_eq!(live.stream.read(cx).active_run_activity_segment, Some(0));
    });
    assert_eq!(live.details(cx), details);
    assert_eq!(
        live.visible_details(cx),
        details.iter().map(|(id, _)| *id).collect::<Vec<_>>()
    );
    assert_eq!(live.frame_demand(cx), 0);
}

#[gpui_kit::test]
fn a_new_read_suppresses_older_command_and_summary_retention_after_finishing(
    cx: &mut TestAppContext,
) {
    for summary in [false, true] {
        let live = Conversation::aggregate(cx);
        if summary {
            live.summary("**Retained review**", cx);
            live.complete_reasoning(RESPONSE, cx);
        } else {
            live.retained_command(cx);
        }
        live.read("new-read", "next.rs", cx);
        live.start("new-read", cx);
        live.assert_stage("Reading next.rs", Some(Icon::BookOpen), true, cx);
        live.finish("new-read", ToolCallStatus::Success, cx);
        live.assert_stage(
            if summary {
                "Read files"
            } else {
                "Read files, run commands"
            },
            Some(Icon::BookOpen),
            false,
            cx,
        );
        assert_eq!(live.frame_demand(cx), 0);
    }
}

#[gpui_kit::test]
fn readable_summary_retains_on_reasoning_completion_but_not_blank_body_or_pause(
    cx: &mut TestAppContext,
) {
    let live = Conversation::aggregate(cx);
    live.summary("**Inspect", cx);
    live.assert_stage("Read files", Some(Icon::BookOpen), false, cx);
    live.summary(" boundaries**", cx);
    live.assert_stage("Inspect boundaries", None, true, cx);
    live.text(" \n\t", cx);
    live.advance(60, cx);
    live.assert_stage("Inspect boundaries", None, true, cx);
    live.summary("\n**Inspect boundaries**\n", cx);
    live.complete_reasoning("old-response", cx);
    live.assert_stage("Inspect boundaries", None, true, cx);
    live.complete_reasoning(RESPONSE, cx);
    live.assert_stage("Inspect boundaries", None, false, cx);
    assert_eq!(live.frame_demand(cx), 0);
    live.advance(120, cx);
    live.assert_stage("Inspect boundaries", None, false, cx);
}

#[gpui_kit::test]
fn only_matching_valid_response_completion_freezes_the_readable_title(cx: &mut TestAppContext) {
    for (id, end, finished) in [
        ("old-response", 1, false),
        (RESPONSE, 0, false),
        (RESPONSE, 1, true),
    ] {
        let live = Conversation::aggregate(cx);
        live.text(" ", cx);
        live.summary("**Review response**", cx);
        live.response_completed(id, 0, end, AssistantResponseKind::Commentary, cx);
        live.assert_stage("Review response", None, !finished, cx);
        cx.read(|cx| assert!(live.stream.read(cx).active_agent_message.is_some()));
    }
}

#[gpui_kit::test]
fn visible_body_finishes_summary_and_new_stage_does_not_inherit_it(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.summary("**Before commentary**", cx);
    live.text("Visible synthetic commentary.", cx);
    live.assert_stage("Before commentary", None, false, cx);
    live.command("new-stage", "printf stage", cx);
    live.draw(cx);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert_eq!(
            stream.run_activity_groups[MESSAGE].read(cx).segments.len(),
            2
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
                    _ => None,
                })
                .collect::<String>(),
            "Visible synthetic commentary."
        );
    });
    live.assert_tool_header(
        "new-stage",
        "Awaiting approval to run printf stage, expand tool details",
        false,
        cx,
    );
    live.select_segment(0, cx);
    live.assert_stage("Before commentary", None, false, cx);
}

#[gpui_kit::test]
fn tool_proposal_finishes_summary_without_pretending_the_tool_started(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.summary("**Await next operation**", cx);
    live.command("next", "printf boundary", cx);
    live.assert_stage("Await next operation", None, false, cx);
    live.approve("next", cx);
    live.assert_stage("Await next operation", None, false, cx);
    live.running("next", cx);
    live.assert_stage("Running printf boundary", Some(Icon::Terminal), true, cx);
    live.finish("next", ToolCallStatus::Success, cx);
    live.assert_stage("Ran printf boundary", Some(Icon::Terminal), false, cx);
}

#[gpui_kit::test]
fn completed_summary_title_is_frozen_while_late_source_is_preserved(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.summary("**Frozen title**\n", cx);
    live.complete_reasoning(RESPONSE, cx);
    live.summary("**Late different title**\n", cx);
    live.complete_reasoning(RESPONSE, cx);
    live.assert_stage("Frozen title", None, false, cx);
    assert_eq!(
        live.details(cx).last().unwrap().1,
        "**Frozen title**\n**Late different title**\n"
    );
    live.response_started("new-response", 0, cx);
    live.summary("**New title**\n", cx);
    live.assert_stage("New title", None, true, cx);
    live.complete_reasoning(RESPONSE, cx);
    live.response_completed(RESPONSE, 0, 1, AssistantResponseKind::Commentary, cx);
    live.assert_stage("New title", None, true, cx);
    live.complete_reasoning("new-response", cx);
    live.assert_stage("New title", None, false, cx);
}

#[gpui_kit::test]
fn parallel_commands_retain_activation_order_after_falling_back_to_a_survivor(
    cx: &mut TestAppContext,
) {
    let live = Conversation::aggregate(cx);
    live.command("a", "printf alpha", cx);
    live.command("b", "printf beta", cx);
    live.start("a", cx);
    live.start("b", cx);
    live.assert_stage("Running printf beta", Some(Icon::Terminal), true, cx);
    live.finish("b", ToolCallStatus::Success, cx);
    live.assert_stage("Running printf alpha", Some(Icon::Terminal), true, cx);
    live.running("a", cx);
    live.event(
        ConversationEvent::ToolCallOutput {
            call_id: "a".into(),
            chunk: ToolOutputChunk("synthetic output".into()),
        },
        cx,
    );
    live.advance(2, cx);
    assert_eq!(
        live.tool_snapshot("a", cx),
        (ToolCallStatus::Running, Some(2), None)
    );
    live.assert_stage("Running printf alpha", Some(Icon::Terminal), true, cx);
    live.finish("a", ToolCallStatus::Success, cx);
    live.assert_stage("Ran printf beta", Some(Icon::Terminal), false, cx);
    live.finish("a", ToolCallStatus::Success, cx);
    live.event(
        ConversationEvent::ToolCallOutput {
            call_id: "b".into(),
            chunk: ToolOutputChunk("late synthetic output".into()),
        },
        cx,
    );
    live.advance(30, cx);
    live.assert_stage("Ran printf beta", Some(Icon::Terminal), false, cx);
}

#[gpui_kit::test]
fn active_tool_beats_completed_summary_and_detail_thinking_prevents_older_retention(
    cx: &mut TestAppContext,
) {
    let live = Conversation::aggregate(cx);
    live.command("a", "printf alpha", cx);
    live.command("b", "printf beta", cx);
    live.start("a", cx);
    live.start("b", cx);
    live.summary("**Latest summary**", cx);
    live.assert_stage("Latest summary", None, true, cx);
    live.running("a", cx);
    live.advance(2, cx);
    live.assert_stage("Latest summary", None, true, cx);
    live.complete_reasoning(RESPONSE, cx);
    live.assert_stage("Running printf beta", Some(Icon::Terminal), true, cx);
    live.finish("b", ToolCallStatus::Success, cx);
    live.assert_stage("Running printf alpha", Some(Icon::Terminal), true, cx);
    live.finish("a", ToolCallStatus::Success, cx);
    live.assert_stage("Latest summary", None, false, cx);
    live.response_started("detail-response", 0, cx);
    live.thinking("Synthetic follow-up reasoning.", cx);
    live.finish("a", ToolCallStatus::Success, cx);
    live.assert_stage("Thinking", None, true, cx);
    live.complete_reasoning("detail-response", cx);
    live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
}

#[gpui_kit::test]
fn completed_command_preserves_truthful_failure_cancel_and_nonzero_titles(cx: &mut TestAppContext) {
    for (status, code, title, footer) in [
        (
            ToolCallStatus::Failed,
            None,
            "Command failed: printf outcome",
            "Failure",
        ),
        (
            ToolCallStatus::Cancelled,
            None,
            "Command cancelled: printf outcome",
            "Cancelled",
        ),
        (
            ToolCallStatus::Success,
            Some(7),
            "Command failed: printf outcome",
            "Failure",
        ),
    ] {
        let live = Conversation::aggregate(cx);
        live.command("outcome", "printf outcome", cx);
        live.start("outcome", cx);
        let mut terminal = result(status, true, false);
        if let Some(code) = code {
            terminal.exit_code = Some(code);
        }
        live.result("outcome", terminal, cx);
        live.assert_stage(title, Some(Icon::Terminal), false, cx);
        assert_eq!(live.frame_demand(cx), 0);
        let card = live.card("outcome", cx);
        card.update(cx, |card, cx| card.set_expanded(true, cx));
        live.group(cx).update(cx, |group, cx| {
            assert!(group.reveal_segment(0, cx));
        });
        live.draw(cx);
        assert_eq!(live.tool_snapshot("outcome", cx).0, status);
        let (node, _) = retained_headline_observation::header(card.entity_id());
        assert!(node.label().unwrap().contains(if footer == "Cancelled" {
            "Command cancelled"
        } else {
            "Command failed"
        }));
        assert!(
            retained_headline_observation::footers()
                .iter()
                .any(|label| label == footer)
        );
        assert_eq!(
            live.tool_snapshot("outcome", cx)
                .2
                .as_ref()
                .map(|projection| matches!(projection, ToolCardResultProjection::Bash { .. })),
            Some(true)
        );
    }
}

#[gpui_kit::test]
fn rejected_reused_and_corrupt_results_without_running_cannot_claim_execution(
    cx: &mut TestAppContext,
) {
    for kind in ["rejected", "reused", "corrupt"] {
        let live = Conversation::aggregate(cx);
        live.retained_command(cx);
        live.command("unstarted", "printf unstarted", cx);
        match kind {
            "rejected" => live.finish("unstarted", ToolCallStatus::Rejected, cx),
            "reused" => live.result("unstarted", result(ToolCallStatus::Success, true, true), cx),
            _ => {
                live.approve("unstarted", cx);
                let mut invalid = result(ToolCallStatus::Success, true, false);
                invalid.exit_code = None;
                live.result("unstarted", invalid, cx);
            }
        }
        live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
        assert_eq!(live.frame_demand(cx), 0);
        match kind {
            "rejected" => assert_eq!(
                live.tool_snapshot("unstarted", cx).0,
                ToolCallStatus::Rejected
            ),
            "reused" => assert!(matches!(
                live.tool_snapshot("unstarted", cx).2,
                Some(ToolCardResultProjection::Bash { reused: true, .. })
            )),
            _ => assert_eq!(
                live.tool_snapshot("unstarted", cx).2,
                Some(ToolCardResultProjection::Corrupt)
            ),
        }
    }
}

#[gpui_kit::test]
fn corrupt_running_result_does_not_resurrect_an_older_completed_command(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.retained_command(cx);
    live.command("broken", "printf broken", cx);
    live.start("broken", cx);
    let mut invalid = result(ToolCallStatus::Success, true, false);
    invalid.duration_ms = None;
    live.result("broken", invalid, cx);
    live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
    assert_eq!(
        live.tool_snapshot("broken", cx).2,
        Some(ToolCardResultProjection::Corrupt)
    );
}

#[gpui_kit::test]
fn a_later_conflicting_result_removes_retained_success_without_reviving_an_older_command(
    cx: &mut TestAppContext,
) {
    let live = Conversation::aggregate(cx);
    live.retained_command(cx);
    live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
    let older = live.tool_snapshot("command", cx);
    live.command("broken", "printf broken", cx);
    live.start("broken", cx);
    live.assert_stage("Running printf broken", Some(Icon::Terminal), true, cx);
    live.finish("broken", ToolCallStatus::Success, cx);
    live.assert_stage("Ran printf broken", Some(Icon::Terminal), false, cx);
    let success = live.tool_snapshot("broken", cx);
    assert_eq!(success.0, ToolCallStatus::Success);
    assert_eq!(success.1, None);
    assert!(matches!(
        success.2,
        Some(ToolCardResultProjection::Bash { .. })
    ));
    live.result("broken", result(ToolCallStatus::Cancelled, true, false), cx);
    live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
    assert_eq!(
        live.tool_snapshot("broken", cx),
        (
            ToolCallStatus::Failed,
            None,
            Some(ToolCardResultProjection::Corrupt)
        )
    );
    assert_eq!(live.tool_snapshot("command", cx), older);
    assert_eq!(live.frame_demand(cx), 0);
}

#[gpui_kit::test]
fn disclosure_dedup_source_selection_and_height_share_the_retained_choice(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.command("command", "printf details", cx);
    live.start("command", cx);
    live.finish("command", ToolCallStatus::Success, cx);
    live.response_started("summary-response", 0, cx);
    live.summary("**Review details**\n", cx);
    live.assert_stage("Review details", None, true, cx);
    let details = live.details(cx);
    let ids = details.iter().map(|item| item.0).collect::<Vec<_>>();
    assert_eq!(details.len(), 3);
    assert_eq!(details[2].1, "**Review details**\n");
    assert_eq!(live.visible_details(cx), ids[..2]);
    live.toggle("enter", cx);
    live.assert_geometry_and_focus(true, cx);
    live.complete_reasoning("summary-response", cx);
    live.assert_stage("Review details", None, false, cx);
    assert_eq!(live.visible_details(cx), ids[..2]);
    live.assert_geometry_and_focus(true, cx);
    live.command("next", "printf replacement", cx);
    live.assert_stage("Review details", None, false, cx);
    live.start("next", cx);
    live.assert_stage("Running printf replacement", Some(Icon::Terminal), true, cx);
    let restored = live.visible_details(cx);
    assert_eq!(&restored[..3], ids.as_slice());
    assert_eq!(restored.iter().filter(|id| **id == ids[2]).count(), 1);
    let group = live.group(cx);
    let thinking = cx.read(|cx| {
        group
            .read(cx)
            .children()
            .into_iter()
            .find_map(|child| match child {
                RunActivityChild::Thinking(card) => Some(card),
                _ => None,
            })
            .unwrap()
    });
    thinking.update(cx, |card, cx| card.set_expanded(true, cx));
    live.assert_geometry_and_focus(true, cx);
    let copy = cx.read(|cx| thinking.read(cx).source_copy.clone());
    copy.set_local_selection_range(2..8);
    copy.set_selected_text(copy.selection_generation(), "Review".into());
    thinking.update(cx, |card, cx| card.copy_selected_source(cx));
    cx.update(|cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("Review")
        )
    });
    live.finish("next", ToolCallStatus::Success, cx);
    live.assert_stage("Ran printf replacement", Some(Icon::Terminal), false, cx);
    assert_eq!(copy.original_text(), "**Review details**\n");
    assert!(copy.has_selected_text());
    live.assert_geometry_and_focus(true, cx);
    live.toggle("space", cx);
    live.assert_geometry_and_focus(false, cx);
    live.response_started("collapsed-response", 0, cx);
    live.summary("**Collapsed summary**\n", cx);
    live.assert_stage("Collapsed summary", None, true, cx);
    live.complete_reasoning("collapsed-response", cx);
    live.assert_stage("Collapsed summary", None, false, cx);
    live.assert_geometry_and_focus(false, cx);
    assert_eq!(&live.details(cx)[..3], details.as_slice());
}

#[gpui_kit::test]
fn a_single_completed_command_keeps_its_existing_expanded_detail_title(cx: &mut TestAppContext) {
    let live = Conversation::new(cx);
    live.command("single", "printf single", cx);
    live.start("single", cx);
    live.draw(cx);
    assert!(live.frame_demand(cx) > 0);
    live.finish("single", ToolCallStatus::Success, cx);
    live.assert_tool_header(
        "single",
        "Ran printf single, expand tool details",
        false,
        cx,
    );
    let card = live.card("single", cx);
    gpui_kit::AnyWindowHandle::from(live.window)
        .update(cx, |_, window, cx| {
            let focus = retained_headline_observation::focus(card.entity_id());
            focus.focus(window, cx);
            window.render_frame(cx);
            window.press("enter", cx);
        })
        .unwrap();
    live.assert_tool_header(
        "single",
        "Ran command in 12ms, collapse tool details",
        true,
        cx,
    );
    live.command("second", "printf second", cx);
    live.assert_stage("Ran printf single", Some(Icon::Terminal), false, cx);
    live.group(cx).update(cx, |group, cx| {
        assert!(group.reveal_segment(0, cx));
    });
    live.assert_tool_header(
        "single",
        "Ran command in 12ms, collapse tool details",
        true,
        cx,
    );
    live.start("second", cx);
    live.finish("second", ToolCallStatus::Success, cx);
    live.assert_stage("Ran printf second", Some(Icon::Terminal), false, cx);
}

#[gpui_kit::test]
fn category_fallback_keeps_order_pluralization_first_icon_and_title_limit(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.retained_command(cx);
    live.read("second-read", "second.rs", cx);
    live.propose("find", "glob", serde_json::json!({"pattern": "*.rs"}), cx);
    live.propose(
        "search",
        "grep",
        serde_json::json!({"pattern": "synthetic"}),
        cx,
    );
    let group = live.group(cx);
    let inputs = [
        ToolCardInputProjection::Write {
            path: "written.rs".into(),
            content_bytes: 1,
        },
        ToolCardInputProjection::Edit {
            path: "edited.rs".into(),
            old_string_bytes: 1,
            new_string_bytes: 1,
            edit_count: None,
        },
        ToolCardInputProjection::ComputerUse {
            tool: "browser".into(),
            action: "open".into(),
        },
        ToolCardInputProjection::Corrupt,
    ];
    for input in inputs {
        let card = cx.new(|_| ToolCard::hydrated(Some(input), ToolCallStatus::Failed, None, None));
        group.update(cx, |group, cx| group.append_tool(0, card, false, cx));
    }
    live.event(terminal_event(RunActivityStatus::Completed), cx);
    let expected =
        "Read files, run commands, find files, search content, write files, edit files, …";
    assert!(expected.chars().count() <= 96);
    live.assert_stage(expected, Some(Icon::BookOpen), false, cx);
}

#[gpui_kit::test]
fn trusted_final_collapses_retained_process_once_then_allows_manual_reopening(
    cx: &mut TestAppContext,
) {
    let live = Conversation::aggregate(cx);
    live.retained_command(cx);
    live.summary("**Final review**", cx);
    live.complete_reasoning(RESPONSE, cx);
    live.assert_stage("Final review", None, false, cx);
    live.toggle("enter", cx);
    live.assert_geometry_and_focus(true, cx);
    live.response_started("final-response", 0, cx);
    live.text("Synthetic final answer.", cx);
    live.response_completed(
        "final-response",
        0,
        "Synthetic final answer.".len(),
        AssistantResponseKind::Final,
        cx,
    );
    live.draw(cx);
    let group = live.group(cx);
    gpui_kit::AnyWindowHandle::from(live.window)
        .update(cx, |_, window, cx| {
            assert!(group.read(cx).process_in_details());
            assert!(!group.read(cx).expanded());
            assert!(!group.read(cx).collapse_pending());
            assert!(group.read(cx).header_focus.is_focused(window));
            window.press("enter", cx);
            assert!(group.read(cx).expanded());
        })
        .unwrap();
    live.response_completed(
        "final-response",
        0,
        "Synthetic final answer.".len(),
        AssistantResponseKind::Final,
        cx,
    );
    live.draw(cx);
    cx.read(|cx| {
        assert!(group.read(cx).expanded());
        assert!(group.read(cx).process_in_details());
    });
    assert_eq!(live.frame_demand(cx), 0);
}

#[gpui_kit::test]
fn retry_restores_the_checkpoint_command_instead_of_discarded_completion(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.retained_command(cx);
    live.response_started("retry-response", 0, cx);
    let before = live.details(cx);
    live.command("discarded", "printf discarded", cx);
    live.start("discarded", cx);
    live.finish("discarded", ToolCallStatus::Success, cx);
    live.retry(cx);
    live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
    assert_eq!(live.details(cx), before);
    cx.read(|cx| {
        assert!(
            live.stream
                .read(cx)
                .run_notices
                .values()
                .any(|notice| matches!(
                    notice.kind,
                    run_notice::RunNoticeKind::Retrying {
                        attempt: 1,
                        max_retries: 3
                    }
                ))
        )
    });
    live.summary("**Recovered summary**", cx);
    live.assert_stage("Recovered summary", None, true, cx);
    cx.read(|cx| assert!(live.stream.read(cx).network_retry.is_none()));
    live.complete_reasoning("retry-response", cx);
    live.assert_stage("Recovered summary", None, false, cx);
}

#[gpui_kit::test]
fn retry_restores_a_completed_summary_without_reviving_thinking(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.summary("**Checkpoint summary**", cx);
    live.complete_reasoning(RESPONSE, cx);
    live.response_started("retry-response", 0, cx);
    let before = live.details(cx);
    live.thinking("Discarded synthetic reasoning.", cx);
    live.complete_reasoning("retry-response", cx);
    live.retry(cx);
    live.assert_stage("Checkpoint summary", None, false, cx);
    assert_eq!(live.details(cx), before);
    assert_eq!(live.frame_demand(cx), 0);
}

#[gpui_kit::test]
fn retry_keeps_a_real_surviving_tool_above_checkpoint_summary(cx: &mut TestAppContext) {
    for finished_before_retry in [false, true] {
        let live = Conversation::aggregate(cx);
        live.command("survivor", "printf survivor", cx);
        live.start("survivor", cx);
        live.summary("**Checkpoint review**", cx);
        live.complete_reasoning(RESPONSE, cx);
        live.response_started("retry-response", 0, cx);
        let mut before = live.tool_snapshot("survivor", cx);
        assert_eq!(before.0, ToolCallStatus::Running);
        live.summary("**Discarded review**", cx);
        live.complete_reasoning("retry-response", cx);
        if finished_before_retry {
            live.finish("survivor", ToolCallStatus::Success, cx);
            before = live.tool_snapshot("survivor", cx);
            assert_eq!(before.0, ToolCallStatus::Success);
            assert_eq!(before.1, None);
            assert!(matches!(
                before.2.as_ref(),
                Some(ToolCardResultProjection::Bash { .. })
            ));
        }
        live.retry(cx);
        if finished_before_retry {
            live.assert_stage("Checkpoint review", None, false, cx);
            assert_eq!(live.tool_snapshot("survivor", cx), before);
            assert_eq!(live.frame_demand(cx), 0);
        } else {
            live.assert_stage("Running printf survivor", Some(Icon::Terminal), true, cx);
            assert_eq!(live.tool_snapshot("survivor", cx), before);
            live.finish("survivor", ToolCallStatus::Success, cx);
            live.assert_stage("Checkpoint review", None, false, cx);
        }
    }
}

#[gpui_kit::test]
fn retained_command_stays_in_its_original_segment_across_visible_body(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.retained_command(cx);
    live.text("Segment boundary.", cx);
    live.response_started("next-response", "Segment boundary.".len(), cx);
    live.thinking("New segment detail.", cx);
    live.assert_stage("Thinking", None, true, cx);
    live.complete_reasoning("next-response", cx);
    live.assert_stage("Thought", None, false, cx);
    live.select_segment(0, cx);
    live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
}

fn terminal_event(status: RunActivityStatus) -> ConversationEvent {
    match status {
        RunActivityStatus::Completed => ConversationEvent::MessageFinished {
            message_id: MESSAGE.into(),
            stop_reason: ConversationStopReason::End,
            execution_duration_ms: Some(25),
        },
        RunActivityStatus::Failed => ConversationEvent::Error {
            message_id: Some(MESSAGE.into()),
            error: Arc::new(vega_runtime::VegaError::Tool {
                tool: "synthetic".into(),
                message: "synthetic failure".into(),
            }),
            execution_duration_ms: Some(25),
        },
        RunActivityStatus::Interrupted => ConversationEvent::Interrupted {
            message_id: MESSAGE.into(),
            execution_duration_ms: Some(25),
        },
        RunActivityStatus::Running => unreachable!(),
    }
}

#[gpui_kit::test]
fn run_terminal_boundaries_clear_command_summary_and_active_presentation(cx: &mut TestAppContext) {
    for terminal in [
        RunActivityStatus::Completed,
        RunActivityStatus::Failed,
        RunActivityStatus::Interrupted,
    ] {
        for progress in ["command", "summary", "running"] {
            let live = Conversation::aggregate(cx);
            live.command("command", "printf terminal", cx);
            live.start("command", cx);
            if progress != "running" {
                live.finish("command", ToolCallStatus::Success, cx);
            }
            if progress == "summary" {
                live.summary("**Terminal review**", cx);
                live.complete_reasoning(RESPONSE, cx);
            }
            let durable = live.tool_snapshot("command", cx);
            live.event(terminal_event(terminal), cx);
            live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
            assert_eq!(live.frame_demand(cx), 0);
            let after = live.tool_snapshot("command", cx);
            assert_eq!(after.0, durable.0);
            assert_eq!(after.2, durable.2);
            live.summary("**Late terminal summary**", cx);
            live.complete_reasoning(RESPONSE, cx);
            live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
            cx.read(|cx| {
                let stream = live.stream.read(cx);
                assert_eq!(
                    stream.run_activity_groups[MESSAGE].read(cx).status(),
                    terminal
                );
                if terminal == RunActivityStatus::Failed {
                    assert!(stream.controller_error.is_some());
                    assert!(stream.run_notices.values().any(|notice| matches!(
                        notice.kind,
                        run_notice::RunNoticeKind::Failure(_)
                    )));
                }
                if terminal == RunActivityStatus::Interrupted {
                    assert!(stream.controller_error.is_none());
                }
            });
        }
    }
}

#[gpui_kit::test]
fn ownership_release_clears_retention_without_fabricating_durable_tool_success(
    cx: &mut TestAppContext,
) {
    for unscoped_error in [false, true] {
        for progress in ["command", "summary", "running"] {
            let live = Conversation::aggregate(cx);
            live.command("command", "printf released", cx);
            live.start("command", cx);
            if progress != "running" {
                live.finish("command", ToolCallStatus::Success, cx);
            }
            if progress == "summary" {
                live.summary("**Owned review**", cx);
                live.complete_reasoning(RESPONSE, cx);
            }
            let before = live.tool_snapshot("command", cx);
            let details = live.details(cx);
            if unscoped_error {
                live.event(
                    ConversationEvent::Error {
                        message_id: None,
                        error: Arc::new(vega_runtime::VegaError::Tool {
                            tool: "synthetic".into(),
                            message: "worker disconnected".into(),
                        }),
                        execution_duration_ms: None,
                    },
                    cx,
                );
            } else {
                live.stream.update(cx, |stream, cx| {
                    assert!(!stream.finish_composer_run(false, cx));
                });
            }
            live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
            assert_eq!(live.frame_demand(cx), 0);
            live.summary("**Late owned summary**", cx);
            live.thinking("Late owned detail.", cx);
            live.complete_reasoning(RESPONSE, cx);
            live.advance(30, cx);
            live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
            assert_eq!(live.details(cx), details);
            let after = live.tool_snapshot("command", cx);
            assert_eq!(after.0, before.0);
            assert_eq!(after.2, before.2);
            cx.read(|cx| {
                let stream = live.stream.read(cx);
                assert!(stream.active_agent_message.is_none());
                assert!(stream.active_run_activity.is_none());
                assert_eq!(stream.controller_error.is_some(), unscoped_error);
            });
        }
    }
}

#[gpui_kit::test]
fn ending_summary_only_presentation_preserves_the_existing_static_no_tool_fallback(
    cx: &mut TestAppContext,
) {
    for boundary in ["completed", "failed", "interrupted", "worker", "error"] {
        let live = Conversation::new(cx);
        live.summary("**Summary only**", cx);
        live.assert_stage("Summary only", None, true, cx);
        live.complete_reasoning(RESPONSE, cx);
        live.assert_stage("Summary only", None, false, cx);
        match boundary {
            "completed" => live.event(terminal_event(RunActivityStatus::Completed), cx),
            "failed" => live.event(terminal_event(RunActivityStatus::Failed), cx),
            "interrupted" => live.event(terminal_event(RunActivityStatus::Interrupted), cx),
            "worker" => live.stream.update(cx, |stream, cx| {
                assert!(!stream.finish_composer_run(false, cx));
            }),
            _ => live.event(
                ConversationEvent::Error {
                    message_id: None,
                    error: Arc::new(vega_runtime::VegaError::Tool {
                        tool: "synthetic".into(),
                        message: "worker disconnected".into(),
                    }),
                    execution_duration_ms: None,
                },
                cx,
            ),
        }
        live.summary("**Late summary only**", cx);
        live.assert_stage("Summary only", None, false, cx);
        assert_eq!(live.frame_demand(cx), 0);
    }
}

#[gpui_kit::test]
fn history_hydration_does_not_reconstruct_ephemeral_completion_retention(cx: &mut TestAppContext) {
    let live = Conversation::idle(cx);
    live.stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            HistoryPage {
                subagents: Vec::new(),
                entries: vec![
                    HistoryEntry::Tool {
                        seq: 1,
                        message_id: MESSAGE.into(),
                        call_id: "history-read".into(),
                        status: ToolCallStatus::Success,
                        approval: Some(Approval::Once),
                        input: Some(ToolCardInputProjection::ReadOnly {
                            tool: ReadOnlyToolKind::Read,
                            display_target: Some("saved.rs".into()),
                            permission_path: None,
                        }),
                        result: Some(ToolCardResultProjection::ReadOnly {
                            status: ToolCallStatus::Success,
                            output: "saved".into(),
                            reused: false,
                        }),
                    },
                    HistoryEntry::Tool {
                        seq: 2,
                        message_id: MESSAGE.into(),
                        call_id: "history-command".into(),
                        status: ToolCallStatus::Success,
                        approval: Some(Approval::Once),
                        input: Some(ToolCardInputProjection::Bash {
                            command: "printf saved".into(),
                        }),
                        result: Some(ToolCardResultProjection::Bash {
                            status: ToolCallStatus::Success,
                            output: "saved".into(),
                            exit_code: Some(0),
                            duration_ms: Some(12),
                            truncated: Some(false),
                            reused: false,
                        }),
                    },
                    HistoryEntry::Tool {
                        seq: 3,
                        message_id: MESSAGE.into(),
                        call_id: "history-running".into(),
                        status: ToolCallStatus::Running,
                        approval: Some(Approval::Once),
                        input: Some(ToolCardInputProjection::Bash {
                            command: "printf interrupted".into(),
                        }),
                        result: None,
                    },
                    HistoryEntry::AssistantText {
                        seq: 4,
                        message_id: MESSAGE.into(),
                        content: "Saved synthetic reply".into(),
                        status: AssistantStatus::Interrupted,
                        execution_duration_ms: Some(50),
                    },
                ],
                older_cursor: None,
                newer_cursor: None,
                newest_seq: Some(4),
            },
            cx,
        )
    });
    live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
    assert_eq!(live.frame_demand(cx), 0);
    live.advance(120, cx);
    live.assert_stage("Read files, run commands", Some(Icon::BookOpen), false, cx);
}

#[gpui_kit::test]
fn pending_permission_stops_motion_without_losing_retained_progress_or_permission_entry(
    cx: &mut TestAppContext,
) {
    let live = Conversation::aggregate(cx);
    live.retained_command(cx);
    live.command("permission", "printf approval", cx);
    let queue = cx.read(|cx| live.stream.read(cx).permission_queue.clone());
    let pending = queue.request(
        PermissionRequest {
            call_id: "permission".into(),
            tool: "bash".into(),
            display_target: "printf approval".into(),
            danger_rule_id: None,
            danger_reason: None,
            external: None,
            acp_options: None,
        },
        tokio_util::sync::CancellationToken::new(),
    );
    live.stream
        .update(cx, |stream, cx| stream.install_pending_permission(cx));
    live.assert_stage("Ran printf retained", Some(Icon::Terminal), false, cx);
    assert_eq!(live.frame_demand(cx), 0);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.active_permission.is_some());
        assert!(
            stream
                .entries
                .iter()
                .any(|entry| matches!(entry, StreamEntry::Permission { .. }))
        );
    });
    drop(pending);
    live.stream
        .update(cx, |stream, cx| stream.timeout_permission(cx));
}

#[gpui_kit::test]
fn a_pending_answer_keeps_the_question_available_alongside_static_summary(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.summary("**Await a choice**", cx);
    live.complete_reasoning(RESPONSE, cx);
    let input = UserQuestionInput {
        questions: vec![UserQuestion {
            id: "direction".into(),
            header: "Direction".into(),
            question: "Which synthetic file?".into(),
            options: vec![
                UserQuestionOption {
                    label: "Alpha".into(),
                    description: "Review alpha.rs".into(),
                },
                UserQuestionOption {
                    label: "Beta".into(),
                    description: "Review beta.rs".into(),
                },
            ],
        }],
    };
    live.propose(
        "question",
        vega_conversation::types::USER_QUESTION_TOOL,
        serde_json::to_value(&input).unwrap(),
        cx,
    );
    let queue = cx.read(|cx| live.stream.read(cx).user_question_queue());
    let pending = queue.request(
        RuntimeUserQuestionRequest {
            call_id: "question".into(),
            input,
        },
        tokio_util::sync::CancellationToken::new(),
    );
    live.stream
        .update(cx, |stream, cx| stream.install_pending_user_question(cx));
    live.assert_stage("Await a choice", None, false, cx);
    assert_eq!(live.frame_demand(cx), 0);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.active_user_question.is_some());
        assert!(stream.has_pending_user_question());
    });
    drop(pending);
}

#[gpui_kit::test]
fn compaction_keeps_its_real_notice_and_suspends_active_headline_motion(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.retained_command(cx);
    live.response_started("compaction-response", 0, cx);
    live.summary("**Compaction review**", cx);
    live.assert_stage("Compaction review", None, true, cx);
    live.stream.update(cx, |stream, cx| {
        assert!(stream.apply_context_status(
            "retained-thread",
            "synthetic-model",
            ContextCompactionStatusRecord {
                generation: 1,
                status: ContextCompactionStatus::Compacting,
                updated_at: 0,
                estimated_tokens: None,
                input_budget: None,
                target_tokens: None,
                source_version: None,
                failure: None,
                usage: ContextCompactionUsageState::Pending
            },
            cx
        ));
    });
    live.assert_stage("Compaction review", None, false, cx);
    assert_eq!(live.frame_demand(cx), 0);
    cx.read(|cx| assert!(live.stream.read(cx).entries.iter().any(|entry| matches!(entry, StreamEntry::ContextCompaction { record, .. } if record.status == ContextCompactionStatus::Compacting))));
    live.complete_reasoning("compaction-response", cx);
    live.assert_stage("Compaction review", None, false, cx);
}

#[gpui_kit::test]
fn motion_gates_still_apply_before_and_after_summary_retention(cx: &mut TestAppContext) {
    let live = Conversation::aggregate(cx);
    live.summary("**Observe motion**", cx);
    live.assert_stage("Observe motion", None, true, cx);
    assert!(live.frame_demand(cx) > 0);
    cx.update(|cx| cx.set_reduce_motion(true));
    assert_eq!(live.frame_demand(cx), 0);
    cx.update(|cx| cx.set_reduce_motion(false));
    live.window
        .update(cx, |host, _, _| host.visible = false)
        .unwrap();
    assert_eq!(live.frame_demand(cx), 0);
    live.window
        .update(cx, |host, _, _| host.visible = true)
        .unwrap();
    assert!(live.frame_demand(cx) > 0);
    let other = cx.open_window(size(px(40.), px(40.)), |_, _| StageHost {
        stream: live.stream.clone(),
        segment: None,
        visible: false,
    });
    other
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(live.frame_demand(cx), 0);
    live.window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert!(live.frame_demand(cx) > 0);
    live.complete_reasoning(RESPONSE, cx);
    live.assert_stage("Observe motion", None, false, cx);
    assert_eq!(live.frame_demand(cx), 0);
    live.window
        .update(cx, |host, _, _| host.visible = false)
        .unwrap();
    assert_eq!(live.frame_demand(cx), 0);
    live.window
        .update(cx, |host, _, _| host.visible = true)
        .unwrap();
    live.assert_stage("Observe motion", None, false, cx);
    assert_eq!(live.frame_demand(cx), 0);
}
