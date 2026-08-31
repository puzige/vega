use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use super::*;
use gpui::{Focusable, TestAppContext, WindowHandle};
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::PermissionHook;
use vega_conversation::types::{
    Microcents, PermissionDecision, PermissionMode, PermissionRequest, PlanStatus, TaskCostSummary,
    TaskSummaryOutcome, ThreadMode, ThreadStatus, ToolCall, ToolCallStatus, ToolResult,
};
use vega_markdown::split_deltas;
use vega_markdown::{ListItem, TableCell};

// ---------- 锚定状态机（P4） ----------

use anchor::{AnchorAction as Action, AnchorState as State};

type DecisionFuture = Pin<Box<dyn Future<Output = PermissionDecision> + Send>>;

mod composer_counter;
mod core_flow;
mod hydration;
mod model_markdown;
mod permissions_cards;
mod scroll_follow;

struct StreamHarness {
    stream: Entity<ConversationStream>,
}

impl Render for StreamHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.stream.clone()
    }
}

fn permission_thread() -> Thread {
    Thread {
        id: "thread-safe-id".into(),
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

fn focus_setting(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    index: usize,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |_, window, cx| {
            let focus = stream.read(cx).setting_focus[index].clone();
            window.focus(&focus, cx);
        })
        .expect("settings stream window");
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

fn step(state: State, distance: f32, viewport: f32, grew: bool) -> (State, Action) {
    let decision = anchor::step(state, distance, viewport, grew);
    (decision.state, decision.action)
}

// ---------- S8-T45/C7 顶部水合（分页 + 锚定 + 去重） ----------

fn hydration_state(older_cursor: Option<i64>, loading: bool, paused: bool) -> HistoryHydration {
    HistoryHydration {
        older_cursor,
        loading,
        paused,
    }
}

fn hydration_user(seq: i64, content: &str) -> HistoryEntry {
    HistoryEntry::UserText {
        seq,
        content: content.into(),
    }
}

fn hydration_assistant(seq: i64, content: &str) -> HistoryEntry {
    HistoryEntry::AssistantText {
        seq,
        message_id: format!("assistant-{seq}"),
        content: content.into(),
        status: vega_conversation::history::AssistantStatus::Done,
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
        entries,
        older_cursor,
        newest_seq: None,
    }
}

fn hydrated_entry_kinds(stream: &ConversationStream) -> Vec<&'static str> {
    stream
        .entries
        .iter()
        .map(|entry| match entry {
            StreamEntry::User { .. } => "user",
            StreamEntry::Assistant { .. } => "assistant",
            StreamEntry::Tool { .. } => "tool",
            StreamEntry::Artifact { .. } => "artifact",
            StreamEntry::Permission { .. } => "permission",
            StreamEntry::Plan { .. } => "plan",
            StreamEntry::Summary { .. } => "summary",
        })
        .collect()
}

// ---------- RenderNode → 行映射（§5.3 关键分支） ----------

fn spans_text(line: &StreamLine) -> String {
    line.spans.iter().map(|span| span.text.as_str()).collect()
}

// ---------- T18 高亮整合（committed 高亮 / pending 降级） ----------

fn find_span<'a>(lines: &'a [StreamLine], text: &str) -> &'a StreamSpan {
    lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.text == text)
        .unwrap_or_else(|| panic!("span {text:?} not found"))
}

// ---------- T18 消息块（user 回显行模型） ----------

// ---------- StreamModel 差量渲染（P3 冻结契约） ----------

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

// ---------- Composer token counter（S7-T39/A10-05） ----------
