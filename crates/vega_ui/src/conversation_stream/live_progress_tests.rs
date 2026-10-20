use super::*;
use gpui_kit::test::{TestSupportExt, TestWindowExt};
use gpui_kit::{AppContext, TestAppContext, WindowHandle, size};
use vega_conversation::agent::PermissionHook;
use vega_conversation::history::AssistantStatus;
use vega_conversation::types::{
    Approval, AssistantResponseKind, AssistantResponseMetadata, ContextCompactionStatus,
    ContextCompactionStatusRecord, ContextCompactionUsageState, ConversationStopReason,
    PermissionRequest, ReadOnlyToolKind, TaskBackend, ThreadStatus, ToolCall, ToolCallStatus,
    ToolCardInputProjection, ToolCardResultProjection, ToolOutputChunk, ToolResult,
};
use vega_runtime::RuntimeUserQuestionHook;

const MESSAGE: &str = "live-message";
const INITIAL_RESPONSE: &str = "initial-response";
const WIDTH: f32 = 768.;

struct HeaderObservation {
    node: gpui_kit::accesskit::Node,
    icon: Option<crate::icons::Icon>,
}

thread_local! {
    static HEADERS: std::cell::RefCell<HashMap<usize, HeaderObservation>> = std::cell::RefCell::new(HashMap::new());
}

pub(super) fn observe_header(
    segment: usize,
    header: &impl gpui_kit::Element,
    icon: Option<crate::icons::Icon>,
) {
    let mut node = gpui_kit::accesskit::Node::new(gpui_kit::Role::GenericContainer);
    header.write_a11y_info(&mut node);
    HEADERS.with(|headers| {
        headers
            .borrow_mut()
            .insert(segment, HeaderObservation { node, icon });
    });
}

fn assert_header(
    segment: usize,
    label: &str,
    expanded: Option<bool>,
    icon: Option<crate::icons::Icon>,
) {
    HEADERS.with(|headers| {
        let headers = headers.borrow();
        let header = headers.get(&segment).expect("rendered stage header");
        assert_eq!(header.node.label(), Some(label));
        assert_eq!(header.node.is_expanded(), expanded);
        assert_eq!(header.icon, icon);
    });
}

struct StageHost {
    stream: Entity<ConversationStream>,
    segment: Option<usize>,
    visible: bool,
}

impl Render for StageHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let group = self
            .stream
            .read(cx)
            .run_activity_groups
            .get(MESSAGE)
            .cloned();
        let content = group.and_then(|group| {
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

struct LiveConversation {
    stream: Entity<ConversationStream>,
    window: WindowHandle<StageHost>,
}

impl LiveConversation {
    fn idle(cx: &mut TestAppContext) -> Self {
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
        let this = Self { stream, window };
        this.window
            .update(cx, |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        this
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
        this.response_started(INITIAL_RESPONSE, 0, cx);
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

    fn finish(&self, id: &str, status: ToolCallStatus, cx: &mut TestAppContext) {
        let bash = cx.read(|cx| {
            self.stream.read(cx).tool_cards[id]
                .read(cx)
                .stage_activity_category()
                == ToolActivityCategory::Shell
        });
        self.event(
            ConversationEvent::ToolCallFinished {
                call_id: id.into(),
                result: ToolResult {
                    status,
                    output: "synthetic result".into(),
                    reused: false,
                    exit_code: (bash && status == ToolCallStatus::Success).then_some(0),
                    duration_ms: (bash && status == ToolCallStatus::Success).then_some(12),
                    truncated: (status == ToolCallStatus::Success).then_some(false),
                    invalid: None,
                },
            },
            cx,
        );
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

    fn response_started(
        &self,
        response_id: &str,
        text_start_bytes: usize,
        cx: &mut TestAppContext,
    ) {
        self.event(
            ConversationEvent::AssistantResponseStarted {
                message_id: MESSAGE.into(),
                response_id: response_id.into(),
                text_start_bytes,
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

    fn visible_details(&self, cx: &TestAppContext) -> Vec<gpui_kit::EntityId> {
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

    fn draw(&self, cx: &mut TestAppContext) {
        self.stream
            .update(cx, |stream, cx| stream.refresh_run_notices(cx));
        gpui_kit::AnyWindowHandle::from(self.window)
            .update(cx, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }

    fn assert_stage(
        &self,
        title: &str,
        icon: Option<crate::icons::Icon>,
        shimmering: bool,
        cx: &mut TestAppContext,
    ) {
        self.draw(cx);
        let group = self.group(cx);
        let selected = cx.read(|cx| self.window.read(cx).unwrap().segment);
        cx.read(|cx| {
            let group = group.read(cx);
            let segment = selected.unwrap_or(group.segments.len() - 1);
            let view = group.segments[segment].title_view.read(cx);
            assert_eq!(
                shimmer::observation::rendered_title(view),
                (title, shimmering)
            );
            let expandable = group.segment_can_expand(segment, cx);
            let expanded = group.segments[segment].expanded;
            let label = if expandable {
                format!(
                    "{title}, {} stage activity",
                    if expanded { "collapse" } else { "expand" }
                )
            } else {
                title.to_owned()
            };
            assert_header(segment, &label, expandable.then_some(expanded), icon);
        });
    }

    fn assert_thinking(&self, title: &str, cx: &mut TestAppContext) {
        self.assert_stage(title, None, true, cx);
    }

    fn assert_geometry_and_focus(&self, expanded: bool, cx: &mut TestAppContext) {
        self.draw(cx);
        let group = self.group(cx);
        gpui_kit::AnyWindowHandle::from(self.window)
            .update(cx, |_, window, cx| {
                assert_eq!(group.read(cx).segments[0].expanded, expanded);
                assert!(group.read(cx).segments[0].header_focus.is_focused(window));
                let height = window.find("stage-viewport").bounds().size.height;
                let estimated = group
                    .read(cx)
                    .estimated_segment_height_with_top(0, WIDTH, 0., cx);
                assert!(
                    (f32::from(height) - estimated).abs() < 2.,
                    "painted {height:?}, estimated {estimated}"
                );
                if !expanded {
                    assert_eq!(height, px(Layout::CONVERSATION_TOOL_HEIGHT));
                }
            })
            .unwrap();
    }

    fn tool_snapshot(
        &self,
        id: &str,
        cx: &TestAppContext,
    ) -> (
        ToolCallStatus,
        Option<u64>,
        Option<ToolCardResultProjection>,
    ) {
        cx.read(|cx| {
            crate::tool_card::live_progress_observation::snapshot(
                self.stream.read(cx).tool_cards[id].read(cx),
            )
        })
    }

    fn finished_read(&self, id: &str, path: &str, cx: &mut TestAppContext) {
        self.read(id, path, cx);
        self.start(id, cx);
        self.finish(id, ToolCallStatus::Success, cx);
    }

    fn details(&self, cx: &TestAppContext) -> Vec<(gpui_kit::EntityId, String)> {
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

    fn frame_demand(&self, cx: &mut TestAppContext) -> usize {
        gpui_kit::AnyWindowHandle::from(self.window)
            .update(cx, |_, window, cx| {
                window.simulate_next_frame(cx);
                window.render_frame(cx);
                window.simulate_next_frame(cx)
            })
            .unwrap()
    }
}

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: "live-thread".into(),
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

#[gpui_kit::test]
fn command_headline_requires_execution_and_retires_before_the_run_ends(cx: &mut TestAppContext) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.command("command", "printf progress", cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    live.approve("command", cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    live.running("command", cx);
    live.assert_stage(
        "Running printf progress",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.finish("command", ToolCallStatus::Success, cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    cx.read(|cx| {
        assert_eq!(
            live.stream.read(cx).run_activity_groups[MESSAGE]
                .read(cx)
                .status(),
            RunActivityStatus::Running
        )
    });
}

#[gpui_kit::test]
fn latest_active_progress_yields_to_accumulated_categories_after_each_boundary(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.thinking("private detail", cx);
    live.read("first-read", "alpha.rs", cx);
    live.start("first-read", cx);
    live.assert_stage(
        "Reading alpha.rs",
        Some(crate::icons::Icon::BookOpen),
        true,
        cx,
    );
    live.finish("first-read", ToolCallStatus::Success, cx);
    live.command("command", "printf one", cx);
    live.start("command", cx);
    live.assert_stage(
        "Running printf one",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.summary("**Compare results**", cx);
    live.assert_thinking("Compare results", cx);
    live.finish("command", ToolCallStatus::Success, cx);
    live.assert_thinking("Compare results", cx);
    live.read("second-read", "beta.rs", cx);
    live.start("second-read", cx);
    live.assert_stage(
        "Reading beta.rs",
        Some(crate::icons::Icon::BookOpen),
        true,
        cx,
    );
    live.summary("**Check invariants**", cx);
    live.assert_thinking("Check invariants", cx);
    live.complete_reasoning(INITIAL_RESPONSE, cx);
    live.assert_stage(
        "Reading beta.rs",
        Some(crate::icons::Icon::BookOpen),
        true,
        cx,
    );
    live.finish("second-read", ToolCallStatus::Success, cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    live.draw(cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
}

#[gpui_kit::test]
fn detailed_thinking_stays_in_source_while_readable_summary_owns_the_headline(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.thinking("Detailed reasoning must remain in the disclosure.", cx);
    live.assert_thinking("Thinking", cx);
    let first = live.details(cx);
    assert_eq!(
        first[0].1,
        "Detailed reasoning must remain in the disclosure."
    );
    live.summary("**Inspect boundaries**", cx);
    live.assert_thinking("Inspect boundaries", cx);
    live.thinking(" More private reasoning.", cx);
    live.assert_thinking("Inspect boundaries", cx);
    cx.read(|cx| {
        let card = live
            .stream
            .read(cx)
            .active_thinking
            .as_ref()
            .unwrap()
            .read(cx);
        assert_eq!(
            card.text,
            "Detailed reasoning must remain in the disclosure. More private reasoning."
        );
        assert_eq!(card.source_copy.original_text(), "**Inspect boundaries**");
    });
}

#[gpui_kit::test]
fn summary_preview_updates_only_on_readable_changes_and_never_times_out(cx: &mut TestAppContext) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.command("command", "printf stable", cx);
    live.start("command", cx);
    for delta in [" \n", "**", "not closed"] {
        live.summary(delta, cx);
        live.assert_stage(
            "Running printf stable",
            Some(crate::icons::Icon::Terminal),
            true,
            cx,
        );
    }
    live.summary("**", cx);
    live.assert_thinking("not closed", cx);
    live.summary("\n```rust\nsecret();\n```\n", cx);
    live.assert_thinking("not closed", cx);
    live.summary("**Inspect", cx);
    live.assert_thinking("not closed", cx);
    live.summary(" boundaries**", cx);
    live.assert_thinking("Inspect boundaries", cx);
    live.summary("\n**Inspect boundaries**\n", cx);
    live.assert_thinking("Inspect boundaries", cx);
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(30));
    cx.run_until_parked();
    live.assert_thinking("Inspect boundaries", cx);
    live.complete_reasoning(INITIAL_RESPONSE, cx);
    live.assert_stage(
        "Running printf stable",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.summary(" \n```\nnot a summary\n```\n", cx);
    live.assert_stage(
        "Running printf stable",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
}

#[gpui_kit::test]
fn parallel_headlines_follow_start_order_not_proposal_order_or_duplicate_running(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.command("second", "printf second", cx);
    live.command("first", "printf first", cx);
    live.start("first", cx);
    live.assert_stage(
        "Running printf first",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.start("second", cx);
    live.assert_stage(
        "Running printf second",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.running("first", cx);
    live.assert_stage(
        "Running printf second",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.finish("second", ToolCallStatus::Success, cx);
    live.assert_stage(
        "Running printf first",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.finish("first", ToolCallStatus::Success, cx);
    live.assert_stage(
        "Run commands",
        Some(crate::icons::Icon::Terminal),
        false,
        cx,
    );
}

#[gpui_kit::test]
fn old_tool_updates_do_not_finish_or_reorder_new_thinking(cx: &mut TestAppContext) {
    let live = LiveConversation::new(cx);
    live.command("old", "printf old", cx);
    live.command("next", "printf next", cx);
    live.start("old", cx);
    live.summary("**Review current progress**", cx);
    live.assert_thinking("Review current progress", cx);
    live.event(
        ConversationEvent::ToolCallOutput {
            call_id: "old".into(),
            chunk: ToolOutputChunk("old output".into()),
        },
        cx,
    );
    live.running("old", cx);
    cx.run_until_parked();
    assert_eq!(
        live.tool_snapshot("old", cx),
        (ToolCallStatus::Running, Some(0), None)
    );
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert_eq!(
        live.tool_snapshot("old", cx),
        (ToolCallStatus::Running, Some(2), None)
    );
    live.event(
        ConversationEvent::UsageUpdated {
            message_id: MESSAGE.into(),
            usage: vega_conversation::types::TokenUsage {
                input: 1,
                output: 0,
                cache_read: None,
                cache_write: None,
            },
            cost: vega_conversation::types::Microcents(0),
            pricing: None,
        },
        cx,
    );
    live.assert_thinking("Review current progress", cx);
    live.finish("old", ToolCallStatus::Success, cx);
    live.assert_thinking("Review current progress", cx);
    live.start("next", cx);
    live.assert_stage(
        "Running printf next",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.command("boundary", "printf boundary", cx);
    live.thinking("A new detailed thought", cx);
    live.assert_thinking("Thinking", cx);
    live.event(
        ConversationEvent::ToolCallOutput {
            call_id: "next".into(),
            chunk: ToolOutputChunk("next output".into()),
        },
        cx,
    );
    live.running("next", cx);
    live.finish("next", ToolCallStatus::Success, cx);
    live.assert_thinking("Thinking", cx);
}

#[gpui_kit::test]
fn response_completion_and_visible_body_end_thinking_but_blank_body_does_not(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.summary("**First progress**", cx);
    live.assert_thinking("First progress", cx);
    live.text(" \n\t", cx);
    live.assert_thinking("First progress", cx);
    cx.read(|cx| {
        assert_eq!(
            live.stream.read(cx).run_activity_groups[MESSAGE]
                .read(cx)
                .segments
                .len(),
            1
        )
    });
    live.complete_reasoning(INITIAL_RESPONSE, cx);
    live.assert_stage("Read files", Some(crate::icons::Icon::BookOpen), false, cx);
    live.response_started("visible-response", " \n\t".len(), cx);
    live.summary("**Second progress**", cx);
    live.assert_thinking("Second progress", cx);
    live.text("Visible commentary.", cx);
    live.assert_stage("Read files", Some(crate::icons::Icon::BookOpen), false, cx);
    live.complete_reasoning("visible-response", cx);
    live.assert_stage("Read files", Some(crate::icons::Icon::BookOpen), false, cx);
    live.command("next", "printf next-stage", cx);
    live.start("next", cx);
    live.thinking("New-stage detail", cx);
    live.assert_thinking("Thinking", cx);
    let group = live.group(cx);
    cx.read(|cx| {
        assert_eq!(group.read(cx).segments.len(), 2);
        let entries = &live.stream.read(cx).entries;
        let text = entries
            .iter()
            .filter_map(|entry| match entry {
                StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, " \n\tVisible commentary.");
    });
    live.window
        .update(cx, |host, _, _| host.segment = Some(0))
        .unwrap();
    live.assert_stage("Read files", Some(crate::icons::Icon::BookOpen), false, cx);
}

#[gpui_kit::test]
fn nonempty_response_metadata_finishes_thinking_without_finishing_the_run(cx: &mut TestAppContext) {
    let live = LiveConversation::new(cx);
    live.text("Visible commentary.", cx);
    live.summary("**Review commentary**", cx);
    live.assert_thinking("Review commentary", cx);
    let details = live.details(cx);
    let response = AssistantResponseMetadata {
        response_id: INITIAL_RESPONSE.into(),
        text_start_bytes: 0,
        text_end_bytes: "Visible commentary.".len(),
        kind: AssistantResponseKind::Commentary,
    };
    assert!(response.is_valid_for("Visible commentary."));
    live.event(
        ConversationEvent::AssistantResponseCompleted {
            message_id: MESSAGE.into(),
            response,
        },
        cx,
    );
    live.assert_stage("Review commentary", None, false, cx);
    assert_eq!(live.frame_demand(cx), 0);
    assert_eq!(live.details(cx), details);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.active_agent_message.is_some());
        assert!(stream.active_run_activity.is_some());
        assert_eq!(
            stream.run_activity_groups[MESSAGE].read(cx).status(),
            RunActivityStatus::Running
        );
        let text = stream
            .entries
            .iter()
            .filter_map(|entry| match entry {
                StreamEntry::Assistant { copy, .. } => Some(copy.original_text()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "Visible commentary.");
    });
}

#[gpui_kit::test]
fn reasoning_completion_matches_request_identity_without_finishing_parallel_tools(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.command("command", "printf parallel", cx);
    live.start("command", cx);
    live.response_started("request-a", 0, cx);
    live.summary("**First request**", cx);
    live.assert_thinking("First request", cx);
    let tool = live.tool_snapshot("command", cx);
    live.complete_reasoning("request-a", cx);
    live.assert_stage(
        "Running printf parallel",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    let completed = live.details(cx);
    live.complete_reasoning("request-a", cx);
    live.assert_stage(
        "Running printf parallel",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    assert_eq!(live.details(cx), completed);
    live.response_started("request-b", 0, cx);
    live.summary("**Second request**", cx);
    live.assert_thinking("Second request", cx);
    live.complete_reasoning("request-a", cx);
    live.assert_thinking("Second request", cx);
    live.event(
        ConversationEvent::ReasoningCompleted {
            message_id: "unrelated-message".into(),
            response_id: "request-b".into(),
        },
        cx,
    );
    live.assert_thinking("Second request", cx);
    live.complete_reasoning("request-b", cx);
    live.assert_stage(
        "Running printf parallel",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.complete_reasoning("request-b", cx);
    live.assert_stage(
        "Running printf parallel",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    assert_eq!(live.tool_snapshot("command", cx), tool);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.active_agent_message.is_some());
        assert!(stream.active_run_activity.is_some());
        assert_eq!(
            stream.run_activity_groups[MESSAGE].read(cx).status(),
            RunActivityStatus::Running
        );
        assert!(stream.run_responses.is_empty());
    });
    live.finish("command", ToolCallStatus::Success, cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    assert_eq!(live.frame_demand(cx), 0);
}

#[gpui_kit::test]
fn empty_retry_completion_clears_reconnect_without_body_metadata_or_run_completion(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.command("command", "printf still-running", cx);
    live.start("command", cx);
    live.response_started("retry-response", 0, cx);
    live.summary("**Discard this attempt**", cx);
    live.retry(cx);
    live.draw(cx);
    let before = cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.network_retry.is_some());
        assert!(stream.run_notices.values().any(|notice| matches!(
            notice.kind,
            run_notice::RunNoticeKind::Retrying {
                attempt: 1,
                max_retries: 3
            }
        )));
        (stream.entries.len(), stream.meter.snapshot())
    });
    let tool = live.tool_snapshot("command", cx);
    live.complete_reasoning("unrelated-response", cx);
    live.draw(cx);
    cx.read(|cx| {
        assert!(live.stream.read(cx).network_retry.is_some());
    });
    live.complete_reasoning("retry-response", cx);
    live.assert_stage(
        "Running printf still-running",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    assert_eq!(live.tool_snapshot("command", cx), tool);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.network_retry.is_none());
        assert!(stream.run_notices.is_empty());
        assert_eq!((stream.entries.len(), stream.meter.snapshot()), before);
        assert!(stream.run_responses.is_empty());
        assert!(
            !stream
                .entries
                .iter()
                .any(|entry| matches!(entry, StreamEntry::Assistant { .. }))
        );
        assert!(stream.active_agent_message.is_some());
        assert_eq!(
            stream.run_activity_groups[MESSAGE].read(cx).status(),
            RunActivityStatus::Running
        );
    });
    live.text("abcd", cx);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.meter.snapshot().provisional);
        assert_eq!(stream.meter.snapshot().tokens, before.1.tokens + 1);
    });
}

#[gpui_kit::test]
fn disclosure_focus_expansion_detail_order_and_height_survive_headline_switches(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.command("command", "printf details", cx);
    live.summary("**Review details**\n", cx);
    live.assert_thinking("Review details", cx);
    let group = live.group(cx);
    let before = live.details(cx);
    let before_ids = before.iter().map(|item| item.0).collect::<Vec<_>>();
    assert_eq!(before.len(), 3);
    assert_eq!(before[2].1, "**Review details**\n");
    assert_eq!(live.visible_details(cx), before_ids[..2]);
    gpui_kit::AnyWindowHandle::from(live.window)
        .update(cx, |_, window, cx| {
            let header_focus = group.read(cx).segments[0].header_focus.clone();
            header_focus.focus(window, cx);
            window.render_frame(cx);
            window.press("enter", cx);
            assert!(group.read(cx).segments[0].expanded);
            assert!(group.read(cx).segments[0].header_focus.is_focused(window));
            assert_header(
                0,
                "Review details, collapse stage activity",
                Some(true),
                None,
            );
        })
        .unwrap();
    live.assert_geometry_and_focus(true, cx);
    live.start("command", cx);
    live.assert_stage(
        "Running printf details",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    assert_eq!(live.details(cx), before);
    assert_eq!(live.visible_details(cx), before_ids);
    live.assert_geometry_and_focus(true, cx);
    live.command("next", "printf next-details", cx);
    live.start("next", cx);
    live.response_started("detail-response", 0, cx);
    live.summary("**Review details again**\n", cx);
    live.assert_thinking("Review details again", cx);
    let second = live.details(cx);
    let second_ids = second.iter().map(|item| item.0).collect::<Vec<_>>();
    assert_eq!(second.len(), 5);
    assert_eq!(second[..3], before);
    assert_ne!(second[4].0, before[2].0);
    cx.read(|cx| {
        assert_eq!(
            second[3].0,
            live.stream.read(cx).tool_cards["next"].entity_id()
        );
    });
    assert_eq!(second[4].1, "**Review details again**\n");
    assert_eq!(live.visible_details(cx), second_ids[..4]);
    live.summary("**Review details again**\n", cx);
    live.assert_thinking("Review details again", cx);
    let mut expected = second;
    expected[4].1.push_str("**Review details again**\n");
    assert_eq!(live.details(cx), expected);
    assert_eq!(live.visible_details(cx), second_ids[..4]);
    live.assert_geometry_and_focus(true, cx);
    live.complete_reasoning("detail-response", cx);
    live.assert_stage(
        "Running printf next-details",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    assert_eq!(live.details(cx), expected);
    assert_eq!(live.visible_details(cx), second_ids);
    live.assert_geometry_and_focus(true, cx);
    live.finish("next", ToolCallStatus::Success, cx);
    live.finish("command", ToolCallStatus::Success, cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    assert_eq!(live.details(cx), expected);
    assert_eq!(live.visible_details(cx), second_ids);
    live.assert_geometry_and_focus(true, cx);
    gpui_kit::AnyWindowHandle::from(live.window)
        .update(cx, |_, window, cx| {
            window.press("space", cx);
            assert!(!group.read(cx).segments[0].expanded);
            assert_header(
                0,
                "Read files, run commands, expand stage activity",
                Some(false),
                Some(crate::icons::Icon::BookOpen),
            );
        })
        .unwrap();
    live.assert_geometry_and_focus(false, cx);
    live.response_started("collapsed-response", 0, cx);
    live.summary("**Another pass**", cx);
    live.assert_thinking("Another pass", cx);
    let collapsed = live.details(cx);
    assert_eq!(collapsed[..5], expected);
    assert_eq!(live.visible_details(cx), second_ids);
    live.assert_geometry_and_focus(false, cx);
    live.complete_reasoning("collapsed-response", cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    assert_eq!(live.details(cx), collapsed);
    assert_eq!(
        live.visible_details(cx),
        collapsed.iter().map(|item| item.0).collect::<Vec<_>>()
    );
    live.assert_geometry_and_focus(false, cx);
}

#[gpui_kit::test]
fn single_items_stay_direct_until_a_second_logical_entry_and_running_tools_animate(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.command("only", "printf single", cx);
    live.start("only", cx);
    live.draw(cx);
    let group = live.group(cx);
    cx.read(|cx| {
        assert!(!group.read(cx).stage_is_aggregate(0, cx));
        assert_eq!(group.read(cx).stage_process_entry_count(0, cx), 1);
    });
    assert!(
        live.frame_demand(cx) > 0,
        "the actual running direct tool title must schedule Shimmer"
    );
    live.finish("only", ToolCallStatus::Success, cx);
    assert_eq!(live.frame_demand(cx), 0);
    cx.read(|cx| assert!(!group.read(cx).stage_is_aggregate(0, cx)));
    live.summary("**Second entry**", cx);
    live.assert_thinking("Second entry", cx);
    cx.read(|cx| {
        assert!(group.read(cx).stage_is_aggregate(0, cx));
        assert_eq!(group.read(cx).stage_process_entry_count(0, cx), 2);
    });
    let thought = LiveConversation::new(cx);
    thought.thinking("Only one thought", cx);
    thought.assert_thinking("Thinking", cx);
    cx.read(|cx| {
        assert!(
            !thought.stream.read(cx).run_activity_groups[MESSAGE]
                .read(cx)
                .stage_is_aggregate(0, cx)
        )
    });
    thought.command("second", "printf second", cx);
    thought.assert_stage(
        "Run commands",
        Some(crate::icons::Icon::Terminal),
        false,
        cx,
    );
    cx.read(|cx| {
        assert!(
            thought.stream.read(cx).run_activity_groups[MESSAGE]
                .read(cx)
                .stage_is_aggregate(0, cx)
        )
    });
}

#[gpui_kit::test]
fn retry_rollback_restores_checkpoint_activity_without_leaking_discarded_progress(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.command("survivor", "printf survivor", cx);
    live.start("survivor", cx);
    let group = live.group(cx);
    group.update(cx, |group, cx| {
        group.reveal_segment(0, cx);
    });
    live.response_started("retry-response", 0, cx);
    let checkpoint = live.details(cx);
    live.summary("**Discarded summary**", cx);
    live.command("discarded", "printf discarded", cx);
    live.start("discarded", cx);
    live.assert_stage(
        "Running printf discarded",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.retry(cx);
    assert_eq!(live.details(cx), checkpoint);
    live.assert_stage(
        "Running printf survivor",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    cx.read(|cx| assert!(group.read(cx).segments[0].expanded));
    live.summary("**Replacement summary**", cx);
    live.assert_thinking("Replacement summary", cx);
    live.running("survivor", cx);
    live.assert_thinking("Replacement summary", cx);
    live.complete_reasoning("retry-response", cx);
    live.assert_stage(
        "Running printf survivor",
        Some(crate::icons::Icon::Terminal),
        true,
        cx,
    );
    live.finish("survivor", ToolCallStatus::Success, cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
}

#[gpui_kit::test]
fn tool_and_run_terminal_states_stop_animation_without_hiding_failures(cx: &mut TestAppContext) {
    for status in [
        ToolCallStatus::Failed,
        ToolCallStatus::Cancelled,
        ToolCallStatus::Rejected,
    ] {
        let live = LiveConversation::new(cx);
        live.command("survivor", "printf survivor", cx);
        live.start("survivor", cx);
        live.command("terminal", "printf terminal", cx);
        if status != ToolCallStatus::Rejected {
            live.start("terminal", cx);
        }
        live.finish("terminal", status, cx);
        live.assert_stage(
            "Running printf survivor",
            Some(crate::icons::Icon::Terminal),
            true,
            cx,
        );
        live.finish("survivor", ToolCallStatus::Success, cx);
        live.assert_stage(
            "Run commands",
            Some(crate::icons::Icon::Terminal),
            false,
            cx,
        );
        cx.read(|cx| {
            let state = live.stream.read(cx).tool_cards["terminal"]
                .read(cx)
                .activity_state();
            assert_eq!(
                state,
                match status {
                    ToolCallStatus::Failed => crate::tool_card::ToolActivityState::Failed,
                    ToolCallStatus::Cancelled => crate::tool_card::ToolActivityState::Cancelled,
                    ToolCallStatus::Rejected => crate::tool_card::ToolActivityState::Rejected,
                    _ => unreachable!(),
                }
            );
        });
    }
    for terminal in [
        RunActivityStatus::Completed,
        RunActivityStatus::Failed,
        RunActivityStatus::Interrupted,
    ] {
        let live = LiveConversation::new(cx);
        live.command("command", "printf terminal-run", cx);
        live.start("command", cx);
        live.summary("**Last live summary**", cx);
        let event = match terminal {
            RunActivityStatus::Completed => ConversationEvent::MessageFinished {
                message_id: MESSAGE.into(),
                stop_reason: ConversationStopReason::End,
                execution_duration_ms: Some(25),
            },
            RunActivityStatus::Failed => ConversationEvent::Error {
                message_id: Some(MESSAGE.into()),
                error: Arc::new(vega_runtime::VegaError::Tool {
                    tool: "synthetic".into(),
                    message: "failure".into(),
                }),
                execution_duration_ms: Some(25),
            },
            RunActivityStatus::Interrupted => ConversationEvent::Interrupted {
                message_id: MESSAGE.into(),
                execution_duration_ms: Some(25),
            },
            RunActivityStatus::Running => unreachable!(),
        };
        live.event(event, cx);
        live.assert_stage(
            "Run commands",
            Some(crate::icons::Icon::Terminal),
            false,
            cx,
        );
        live.summary("**Stale summary**", cx);
        live.running("command", cx);
        live.assert_stage(
            "Run commands",
            Some(crate::icons::Icon::Terminal),
            false,
            cx,
        );
        cx.read(|cx| {
            assert_eq!(
                live.stream.read(cx).run_activity_groups[MESSAGE]
                    .read(cx)
                    .status(),
                terminal
            );
            let stream = live.stream.read(cx);
            if terminal == RunActivityStatus::Failed {
                assert!(stream.controller_error.is_some());
                assert!(
                    stream
                        .run_notices
                        .values()
                        .any(|notice| matches!(notice.kind, run_notice::RunNoticeKind::Failure(_)))
                );
            } else if terminal == RunActivityStatus::Interrupted {
                assert!(stream.controller_error.is_none());
            }
        });
    }
}

#[gpui_kit::test]
fn unscoped_error_releases_live_presentation_without_fabricating_tool_results(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.command("command", "printf disconnected", cx);
    live.start("command", cx);
    live.summary("**Before disconnect**", cx);
    live.assert_thinking("Before disconnect", cx);
    assert!(live.frame_demand(cx) > 0);
    let details = live.details(cx);
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
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    assert_eq!(live.frame_demand(cx), 0);
    assert_eq!(live.details(cx), details);
    let error = cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.active_agent_message.is_none());
        assert!(stream.active_run_activity.is_none());
        assert_eq!(
            stream.controller_error,
            Some(RunFailureKind::Runtime.message())
        );
        stream.controller_error.clone()
    });
    live.summary("**Late summary**", cx);
    live.thinking("Late detailed reasoning", cx);
    live.running("command", cx);
    live.event(
        ConversationEvent::ToolCallOutput {
            call_id: "command".into(),
            chunk: ToolOutputChunk("late output".into()),
        },
        cx,
    );
    live.complete_reasoning(INITIAL_RESPONSE, cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    assert_eq!(live.frame_demand(cx), 0);
    assert_eq!(live.details(cx), details);
    let tool = live.tool_snapshot("command", cx);
    assert_eq!(tool.0, ToolCallStatus::Running);
    assert_eq!(tool.2, None);
    live.stream.update(cx, |stream, cx| {
        assert!(!stream.finish_composer_run(false, cx));
    });
    live.draw(cx);
    cx.read(|cx| assert_eq!(live.stream.read(cx).controller_error, error));
}

#[gpui_kit::test]
fn worker_release_stops_live_presentation_without_a_final_conversation_event(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.command("command", "printf orphaned", cx);
    live.start("command", cx);
    live.summary("**Worker owned progress**", cx);
    live.assert_thinking("Worker owned progress", cx);
    assert!(live.frame_demand(cx) > 0);
    let details = live.details(cx);
    let group = live.group(cx);
    live.stream.update(cx, |stream, cx| {
        assert!(!stream.finish_composer_run(false, cx));
    });
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    assert_eq!(live.frame_demand(cx), 0);
    assert_eq!(live.details(cx), details);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.active_agent_message.is_none());
        assert!(stream.active_run_activity.is_none());
        assert_eq!(stream.run_activity_groups[MESSAGE], group);
        assert!(stream.controller_error.is_none());
        assert!(stream.run_responses.is_empty());
    });
    live.summary("**Late worker summary**", cx);
    live.running("command", cx);
    live.complete_reasoning(INITIAL_RESPONSE, cx);
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    assert_eq!(live.frame_demand(cx), 0);
    assert_eq!(live.details(cx), details);
    let tool = live.tool_snapshot("command", cx);
    assert_eq!(tool.0, ToolCallStatus::Running);
    assert_eq!(tool.2, None);
}

#[gpui_kit::test]
fn historical_tool_projection_is_static_even_when_storage_status_was_running(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::idle(cx);
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
                            display_target: Some("alpha.rs".into()),
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
                        status: ToolCallStatus::Running,
                        approval: Some(Approval::Once),
                        input: Some(ToolCardInputProjection::Bash {
                            command: "printf history".into(),
                        }),
                        result: None,
                    },
                    HistoryEntry::AssistantText {
                        seq: 3,
                        message_id: MESSAGE.into(),
                        content: "Saved reply".into(),
                        status: AssistantStatus::Interrupted,
                        execution_duration_ms: Some(50),
                    },
                ],
                older_cursor: None,
                newer_cursor: None,
                newest_seq: Some(3),
            },
            cx,
        )
    });
    live.assert_stage(
        "Read files, run commands",
        Some(crate::icons::Icon::BookOpen),
        false,
        cx,
    );
    assert_eq!(live.frame_demand(cx), 0);
}

#[gpui_kit::test]
fn permission_retry_and_compaction_keep_their_independent_feed_and_stop_waiting_motion(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.command("permission", "printf approval", cx);
    live.summary("**Before permission**", cx);
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
    live.draw(cx);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.active_permission.is_some());
        assert!(
            stream
                .entries
                .iter()
                .any(|entry| matches!(entry, StreamEntry::Permission { .. }))
        );
        assert!(
            stream.run_activity_groups[MESSAGE]
                .read(cx)
                .waiting_for_user
        );
    });
    let group = live.group(cx);
    cx.read(|cx| {
        assert!(
            !shimmer::observation::rendered_title(group.read(cx).segments[0].title_view.read(cx)).1
        )
    });
    assert_eq!(live.frame_demand(cx), 0);
    drop(pending);
    live.stream
        .update(cx, |stream, cx| stream.timeout_permission(cx));
    live.response_started("retry-response", 0, cx);
    live.summary("**Transient progress**", cx);
    live.retry(cx);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.run_notices.values().any(|notice| matches!(
            notice.kind,
            run_notice::RunNoticeKind::Retrying {
                attempt: 1,
                max_retries: 3
            }
        )));
    });
    assert_eq!(live.frame_demand(cx), 0);
    live.summary("\n**Recovered progress**", cx);
    live.assert_thinking("Recovered progress", cx);
    cx.read(|cx| assert!(live.stream.read(cx).network_retry.is_none()));
    live.stream.update(cx, |stream, cx| {
        assert!(stream.apply_context_status(
            "live-thread",
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
                usage: ContextCompactionUsageState::Pending,
            },
            cx
        ))
    });
    assert_eq!(live.frame_demand(cx), 0);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.run_activity_groups[MESSAGE].read(cx).phase == RunPhase::Compacting);
        assert!(stream.entries.iter().any(|entry| matches!(entry, StreamEntry::ContextCompaction { record, .. } if record.status == ContextCompactionStatus::Compacting)));
    });
}

#[gpui_kit::test]
fn waiting_for_an_answer_keeps_the_question_available_without_live_headline_motion(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    let input = vega_conversation::types::UserQuestionInput {
        questions: vec![vega_conversation::types::UserQuestion {
            id: "direction".into(),
            header: "Direction".into(),
            question: "Which file should be reviewed?".into(),
            options: vec![
                vega_conversation::types::UserQuestionOption {
                    label: "Alpha".into(),
                    description: "Review alpha.rs".into(),
                },
                vega_conversation::types::UserQuestionOption {
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
    live.summary("**Waiting for a choice**", cx);
    let queue = cx.read(|cx| live.stream.read(cx).user_question_queue());
    let pending = queue.request(
        vega_conversation::types::RuntimeUserQuestionRequest {
            call_id: "question".into(),
            input,
        },
        tokio_util::sync::CancellationToken::new(),
    );
    live.stream
        .update(cx, |stream, cx| stream.install_pending_user_question(cx));
    live.draw(cx);
    cx.read(|cx| {
        let stream = live.stream.read(cx);
        assert!(stream.active_user_question.is_some());
        assert!(stream.has_pending_user_question());
        assert!(
            stream.run_activity_groups[MESSAGE]
                .read(cx)
                .waiting_for_user
        );
    });
    assert_eq!(live.frame_demand(cx), 0);
    drop(pending);
}

#[gpui_kit::test]
fn live_shimmer_respects_reduce_motion_hidden_content_and_inactive_windows(
    cx: &mut TestAppContext,
) {
    let live = LiveConversation::new(cx);
    live.finished_read("read", "alpha.rs", cx);
    live.summary("**Observe motion**", cx);
    live.assert_thinking("Observe motion", cx);
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
    live.complete_reasoning(INITIAL_RESPONSE, cx);
    live.assert_stage("Read files", Some(crate::icons::Icon::BookOpen), false, cx);
    assert_eq!(live.frame_demand(cx), 0);
}
