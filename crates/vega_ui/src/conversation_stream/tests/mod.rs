use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use super::*;
use gpui::{Focusable, TestAppContext, WindowHandle};
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::PermissionHook;
use vega_conversation::types::{
    Microcents, PermissionDecision, PermissionMode, PermissionRequest, Plan, PlanStatus,
    ReadOnlyToolKind, SummaryCost, TaskCostSummary, TaskSummaryOutcome, ThreadMode, ThreadStatus,
    ToolCall, ToolCallStatus, ToolCardInputProjection, ToolCardResultProjection, ToolResult,
};
use vega_markdown::split_deltas;
use vega_markdown::{ListItem, TableCell};

// ---------- 锚定委托（S8-T44：P4 语义由变高列表原生 Tail follow 承担） ----------

type DecisionFuture = Pin<Box<dyn Future<Output = PermissionDecision> + Send>>;

mod composer_counter;
mod core_flow;
mod e2e_variable_height;
mod hydration;
mod model_markdown;
mod permissions_cards;
mod scroll_follow;
mod variable_height;

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

// ---------- S8-T44 变高虚拟化（E2E 与窄测共用 fixture） ----------

/// One mixed markdown turn (CJK/emoji/inline styles/list).
fn mixed_markdown_turn(index: usize) -> String {
    let emoji = ["✅", "🚀", "📌", "🌊"][index % 4];
    format!(
        "## 转次 {index}：混排流式 {emoji}\n\n\
         段落 {index} 带 **加粗**、*斜体*、`行内代码` 与 CJK 混排；\
         中文与 English 交错换行时应自然折行，{emoji} 计入显示宽度。\n\n\
         - 任务甲 {index}\n- [x] 已完成项 {emoji}\n  - 嵌套项 `code`\n\n"
    )
}

/// One code turn (committed fence → T16 highlight tokens).
fn mixed_code_turn(index: usize) -> String {
    format!(
        "```rust\nfn bench_{index}() -> u64 {{\n    let value = {index} * 42;\n    value\n}}\n```\n\n"
    )
}

/// One GFM table turn with CJK cells.
fn mixed_table_turn(index: usize) -> String {
    format!("| 列 A {index} | 列 B |\n|:--|--:|\n| 1 | 中文数据 {index} |\n\n")
}

/// One long wrapped-CJK paragraph turn (C4 禁截断：全文保留，自然折行).
fn mixed_wrapped_cjk_turn(index: usize) -> String {
    format!(
        "长段落 {index}：这一段用于验证变高 item 在窗口宽度变化时的自然折行。\
         中文与 English 混排需要保持稳定，CJK 计两列宽；内容完整呈现，\
         不允许以截断凑高度（C4）。变高几何下每一行都应完整出现在 item 内，\
         且滚动锚点漂移小于 1px。段落编号 {index} 结束。"
    )
}

fn mixed_user_echo(index: usize) -> String {
    format!("帮我看看第 {index} 段的输出 ✅")
}

/// Builds one mixed semantic entry per index (markdown turns, code turns,
/// table turns, wrapped-CJK turns, user echoes, and every card kind — the
/// C6 10k 混合语义项 scenario). Fixture materialization runs against a
/// throwaway counter set; the stream's own counters stay dedicated to the
/// render path's tail syncs (the frozen-remat assertions).
fn mixed_entry(
    index: usize,
    user_seq: &mut u64,
    cx: &mut Context<ConversationStream>,
) -> StreamEntry {
    let counters = StreamCounters::default();
    let assistant = |doc: &str| {
        let mut stream = MarkdownStream::new();
        stream.append(doc);
        stream.finish();
        let mut model = StreamModel::default();
        model.sync(&stream.snapshot(), &counters);
        StreamEntry::Assistant {
            stream: Box::new(stream),
            model,
        }
    };
    match index % 25 {
        0..=8 => assistant(&mixed_markdown_turn(index)),
        9..=11 => assistant(&mixed_code_turn(index)),
        12..=14 => assistant(&mixed_table_turn(index)),
        15..=17 => assistant(&mixed_wrapped_cjk_turn(index)),
        18..=19 | 21 | 22 => {
            let block_id = USER_BLOCK_BASE + *user_seq;
            *user_seq += 1;
            StreamEntry::User {
                lines: user_message_lines(block_id, &mixed_user_echo(index)),
            }
        }
        20 => StreamEntry::Tool {
            card: cx.new(|_| {
                ToolCard::hydrated(
                    Some(ToolCardInputProjection::ReadOnly {
                        tool: ReadOnlyToolKind::Read,
                    }),
                    ToolCallStatus::Success,
                    None,
                    Some(ToolCardResultProjection::ReadOnly {
                        status: ToolCallStatus::Success,
                        output: format!("tool output {index}\nline two ✅"),
                        reused: false,
                    }),
                )
            }),
        },
        23 => StreamEntry::Plan {
            card: cx.new(|cx| {
                PlanCard::new(
                    Plan {
                        id: format!("test-plan-{index}"),
                        thread_id: "thread-safe-id".into(),
                        content: mixed_markdown_turn(index),
                        status: PlanStatus::Approved,
                        review_note: None,
                        reviewed_at: Some(1),
                    },
                    cx,
                )
            }),
        },
        _ => StreamEntry::Summary {
            card: cx.new(|_| {
                SummaryCard::new(TaskCostSummary {
                    message_id: format!("test-summary-{index}"),
                    outcome: TaskSummaryOutcome::Completed,
                    usage: None,
                    cost: SummaryCost::Unavailable,
                    duration_ms: Some(1_200),
                    tool_count: 1,
                    cache_hit_percent: None,
                })
            }),
        },
    }
}

// ---------- Composer token counter（S7-T39/A10-05） ----------
