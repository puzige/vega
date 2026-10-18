use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use super::*;
use gpui_kit::{Focusable, TestAppContext, WindowHandle};
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::PermissionHook;
use vega_conversation::types::{
    Approval, Microcents, PermissionDecision, PermissionMode, PermissionRequest, Plan, PlanStatus,
    SummaryCost, TaskBackend, TaskCostSummary, TaskSummaryOutcome, ThreadMode, ThreadStatus,
    ToolCall, ToolCallStatus, ToolResult,
};
use vega_markdown::split_deltas;
use vega_markdown::{ListItem, TableCell};

type DecisionFuture = Pin<Box<dyn Future<Output = PermissionDecision> + Send>>;

mod automation;

mod composer_counter;
mod composer_predictions;
mod core_flow;
mod hydration;

mod model_markdown;
mod permissions_cards;

mod scroll_edges;
mod scroll_follow;
mod subagent_reading;
mod thinking;
mod user_questions;

mod variable_height;

struct StreamHarness {
    stream: Entity<ConversationStream>,
}

impl Render for StreamHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .size_full()
            .child(self.stream.clone())
    }
}

fn permission_thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: "thread-safe-id".into(),
        backend: TaskBackend::Native,
        project_id: "project-safe-id".into(),
        title: "Permission test".into(),
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

fn init_permission_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(false));
        crate::init(cx);
    });
}

fn open_permission_stream(
    cx: &mut TestAppContext,
) -> (WindowHandle<ConversationStream>, PermissionQueue) {
    let queue = PermissionQueue::new();
    let stream_queue = queue.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|cx| {
                ConversationStream::new_with_permission_queue(permission_thread(), stream_queue, cx)
            })
        })
        .expect("test window")
    });
    cx.run_until_parked();
    (window, queue)
}

fn open_controller_stream(
    cx: &mut TestAppContext,
    thread_id: &str,
) -> (
    WindowHandle<StreamHarness>,
    Entity<ConversationStream>,
    Arc<Mutex<Vec<ThreadSettingsRequested>>>,
) {
    init_permission_test(cx);
    let mut thread = permission_thread();
    thread.id = thread_id.to_string();
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let root_stream = stream.clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|cx| {
                cx.subscribe(
                    &root_stream,
                    move |_, _, event: &ThreadSettingsRequested, _| {
                        if let Ok(mut events) = captured.lock() {
                            events.push(event.clone());
                        }
                    },
                )
                .detach();
                StreamHarness {
                    stream: root_stream,
                }
            })
        })
        .expect("controller stream window")
    });
    cx.run_until_parked();
    (window, stream, events)
}

fn focus_composer(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |_, window, cx| {
            let focus = stream.read_with(cx, |stream, cx| stream.input.read(cx).focus_handle(cx));
            window.focus(&focus, cx);
        })
        .expect("composer stream window");
}

fn click_composer_add(window: WindowHandle<StreamHarness>, cx: &mut TestAppContext) {
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let bounds = visual
        .debug_bounds("composer-add")
        .expect("composer add button");
    visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
}

fn bash_call(id: &str, command: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        tool: "bash".into(),
        input_json: serde_json::json!({ "cmd": command }).to_string(),
    }
}

fn propose(window: WindowHandle<ConversationStream>, cx: &mut TestAppContext, call: ToolCall) {
    window
        .update(cx, |stream, _, cx| {
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        })
        .expect("stream window");
}

fn request_permission(queue: &PermissionQueue, call_id: &str, target: &str) -> DecisionFuture {
    let future = queue.request(
        PermissionRequest {
            call_id: call_id.into(),
            tool: "bash".into(),
            display_target: target.into(),
            danger_rule_id: None,
            danger_reason: None,
            external: None,
            acp_options: None,
        },
        CancellationToken::new(),
    );
    Box::pin(async move { future.await.unwrap_or(PermissionDecision::Timeout) })
}

fn has_active_permission(
    window: WindowHandle<ConversationStream>,
    cx: &mut TestAppContext,
) -> bool {
    window
        .update(cx, |stream, _, _| stream.active_permission.is_some())
        .unwrap_or(false)
}

fn hydration_state(older_cursor: Option<i64>, loading: bool, paused: bool) -> HistoryHydration {
    HistoryHydration {
        older_cursor,
        newer_cursor: None,
        loading,
        paused,
        paused_direction: None,
    }
}

fn hydration_user(seq: i64, content: &str) -> HistoryEntry {
    HistoryEntry::UserText {
        seq,
        message_id: format!("user-{seq}"),
        content: content.into(),
    }
}

fn hydration_assistant(seq: i64, content: &str) -> HistoryEntry {
    HistoryEntry::AssistantText {
        seq,
        message_id: format!("assistant-{seq}"),
        content: content.into(),
        status: vega_conversation::history::AssistantStatus::Done,
        execution_duration_ms: None,
    }
}

fn hydration_summary(message_id: &str) -> HistoryEntry {
    HistoryEntry::Summary {
        seq: 2,
        summary: TaskCostSummary {
            message_id: message_id.into(),
            outcome: TaskSummaryOutcome::Completed,
            usage: None,
            cost: vega_conversation::types::SummaryCost::Unavailable,
            duration_ms: None,
            tool_count: 0,
            cache_hit_percent: None,
        },
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

fn hydrated_entry_kinds(stream: &ConversationStream) -> Vec<&'static str> {
    stream
        .entries
        .iter()
        .map(|entry| match entry {
            StreamEntry::UserImages { .. } => "user-images",
            StreamEntry::RunActivity { .. } => "run-activity",
            StreamEntry::RunActivitySegment { .. } => "run-activity-segment",
            StreamEntry::User { .. } => "user",
            StreamEntry::Assistant { .. } => "assistant",
            StreamEntry::Tool { .. } => "tool",
            StreamEntry::ToolGroup { .. } => "tool-group",
            StreamEntry::Artifact { .. } => "artifact",
            StreamEntry::Permission { .. } => "permission",
            StreamEntry::Plan { .. } => "plan",
            StreamEntry::Summary { .. } => "summary",
            StreamEntry::ContextCompaction { .. } => "compaction",
            StreamEntry::SkillActivation { .. } => "skill-activation",
            StreamEntry::Screenshot { .. } => "screenshot",
            StreamEntry::Subagents { .. } => "subagents",
        })
        .collect()
}

// ---------- RenderNode → 行映射（§5.3 关键分支） ----------

fn spans_text(line: &StreamLine) -> String {
    line.spans.iter().map(|span| span.text.as_str()).collect()
}

fn find_span<'a>(lines: &'a [StreamLine], text: &str) -> &'a StreamSpan {
    lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.text == text)
        .unwrap_or_else(|| panic!("span {text:?} not found"))
}

fn stream_long_doc(blocks: usize) -> (MarkdownStream, usize) {
    let doc = sample_document(blocks);
    let deltas = split_deltas(&doc, 0x5EED);
    let mut stream = MarkdownStream::new();
    let total = deltas.len();
    for delta in &deltas {
        stream.append(delta);
    }
    (stream, total)
}

fn open_run_details(window: WindowHandle<StreamHarness>, cx: &mut TestAppContext) {
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let header = visual
        .debug_bounds("run-activity-toggle")
        .expect("single process header");
    visual.simulate_click(header.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
}

fn confirm_test_final(
    stream: &mut ConversationStream,
    message_id: &str,
    cx: &mut Context<ConversationStream>,
) {
    let start = stream
        .entries
        .iter()
        .zip(&stream.entry_identities)
        .filter_map(|(entry, identity)| {
            if identity.message_id.as_deref() == Some(message_id) {
                match entry {
                    StreamEntry::Assistant { copy, .. } => Some(copy.original_text().len()),
                    _ => None,
                }
            } else {
                None
            }
        })
        .sum();
    stream.close_active_segment_before_tool();
    stream.apply_event(
        ConversationEvent::TextDelta {
            message_id: message_id.into(),
            delta: "Final answer.".into(),
        },
        cx,
    );
    stream.apply_event(
        ConversationEvent::AssistantResponseCompleted {
            message_id: message_id.into(),
            response: vega_conversation::types::AssistantResponseMetadata {
                response_id: "test-final".into(),
                text_start_bytes: start,
                text_end_bytes: start + "Final answer.".len(),
                kind: vega_conversation::types::AssistantResponseKind::Final,
            },
        },
        cx,
    );
}

#[gpui_kit::test]
fn privacy_controls_render_and_disable_without_component_theme(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "privacy-theme");
    cx.update(|cx| assert!(!cx.has_global::<gpui_kit::component::Theme>()));
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    assert!(visual.debug_bounds("credential-review-close").is_none());
    let plus = visual.debug_bounds("composer-add").expect("composer menu");
    visual.simulate_click(plus.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    let button = visual
        .debug_bounds("composer-action-credentials")
        .expect("review menu action");
    visual.simulate_click(button.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    stream.read_with(&visual, |stream, _| {
        assert!(stream.privacy.opened);
        assert!(stream.privacy.pending);
    });
    let button = visual
        .debug_bounds("credential-review-close")
        .expect("disabled close");
    visual.simulate_click(button.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    stream.read_with(&visual, |stream, _| assert!(stream.privacy.opened));
    stream.update(&mut visual, |stream, cx| {
        stream.apply_privacy_review(
            Ok(vega_conversation::types::PrivacyReview {
                source_fingerprint: "synthetic".into(),
                findings: vec![],
            }),
            cx,
        );
    });
    visual.run_until_parked();
    let button = visual
        .debug_bounds("credential-review-close")
        .expect("enabled close");
    visual.simulate_click(button.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    stream.read_with(&visual, |stream, _| assert!(!stream.privacy.opened));
    assert!(visual.debug_bounds("credential-review-close").is_none());
}
