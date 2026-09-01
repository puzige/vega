//! Self-measurement mode for `xtask bench render_frame` (S3-T17 → S8-T44):
//! the hidden `--vega-bench-render <out.json>` flag boots a real GPUI window
//! running the conversation-stream machinery against a ~10k-item variable-
//! height semantic document (markdown / wrapped CJK / emoji / code / all
//! card kinds), then writes the measured JSON report and quits.
//!
//! Mechanism note (task-card decision): `#[gpui::test]` frame timing was
//! evaluated first, but at this gpui rev tests run on `TestPlatform` with
//! `NoopTextSystem` and no real frame cadence, so neither fps nor frame-build
//! times would be representative (text shaping is the dominant cost of a
//! markdown stream). This probe-binary mode reuses the T14 spike measurement
//! method instead: render-callback frame counting + 1s sampling + per-phase
//! frame-build percentiles on a real window (tech-spec §5.2 判定口径).
//!
//! Phases:
//!   SCROLL (8s): programmatic scroll at 720 px/s → P1 fps (vsync-capped).
//!   STREAM (12s): ~500 δ/s injection at the tail, viewport parked on frozen
//!                 items → frozen re-materializations must stay 0 (P3).
//!
//! Everything runs through the production path ([`super`]) — the same
//! variable-height `list`, [`StreamModel`] diffing, and semantic item
//! rendering the app uses. Old 24px uniform-list numbers are noncomparable
//! (SDD §5: the P1 baseline follows the variable-height semantics).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::prelude::*;
use gpui::{App, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div, px};
use vega_conversation::types::{
    Plan, PlanStatus, ReadOnlyToolKind, SummaryCost, TaskCostSummary, TaskSummaryOutcome,
    ToolCallStatus, ToolCardInputProjection, ToolCardResultProjection,
};
use vega_markdown::{MarkdownStream, split_deltas};

use super::{INJECT_TICK, StreamCounters, StreamEntry, StreamModel, render_entry};

use crate::plan_card::PlanCard;
use crate::summary_card::SummaryCard;
use crate::tool_card::ToolCard;

/// Probe phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Programmatic scroll over the fully-built document.
    Scroll,
    /// Tail injection with the viewport parked on frozen items.
    Stream,
    /// Measurement finished; report written, app quitting.
    Done,
}

const SCROLL_SECONDS: u64 = 8;
const STREAM_SECONDS: u64 = 12;
const SCROLL_SPEED_PX_S: f32 = 720.0;
/// Where the viewport parks for the STREAM phase (~1000px into the document;
/// the streaming tail stays far away among the frozen items).
const PARK_OFFSET_Y: f32 = 1000.0;
/// C6 scenario size: 10k mixed semantic items (markdown/wrapped CJK/emoji/
/// code/all card kinds), one GPUI `list` item per [`StreamEntry`].
const ITEM_COUNT: usize = 10_000;
/// Injection target rate during STREAM (δ/s，与演示注入同口径).
const INJECT_RATE: usize = 500;

/// Parses the hidden `--vega-bench-render <path>` flag. `Some` → the binary
/// runs the bench mode instead of the normal app (xtask invokes this).
pub fn output_path_from_args() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let position = args.iter().position(|arg| arg == "--vega-bench-render")?;
    args.get(position + 1).map(PathBuf::from)
}

/// Installs the probe in a running app: registers the theme, opens the probe
/// window, and starts the phase drivers. Called by the `vega` binary (which
/// owns the `application().run` boot) instead of the normal app startup.
pub fn start(output: PathBuf, cx: &mut App) {
    // Bench 模式不经过主应用启动路径：单独注册 light 主题供 item 渲染取 token。
    cx.set_global(vega_theme::Theme::light());
    let bounds = Bounds::centered(None, gpui::size(px(1200.0), px(800.0)), cx);
    let window = cx.open_window(
        WindowOptions {
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("Vega Bench — render_frame".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_, cx| cx.new(|cx| BenchStreamView::new(output, cx)),
    );
    if window.is_err() {
        tracing::error!("vega --vega-bench-render: failed to open the probe window");
        cx.quit();
        return;
    }
    cx.activate(true);
}

/// Aggregated per-phase measurements (all percentiles over raw ns samples).
#[derive(Default)]
struct PhaseMeasurements {
    render_ns: Vec<u128>,
    item_ns: Vec<u128>,
    fps: Vec<u64>,
    frames: u64,
}

/// Nearest-rank percentile over a raw ns sample list, reported in µs.
fn percentile_us(samples: &[u128], pct: u64) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (pct * sorted.len() as u64).div_ceil(100);
    let rank = (rank.max(1) as usize).min(sorted.len());
    sorted[rank - 1] as f64 / 1000.0
}

/// Median of the per-second fps samples (robust against startup outliers).
fn fps_median(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    match sorted.len() {
        0 => 0,
        len => sorted[len / 2],
    }
}

impl PhaseMeasurements {
    fn to_json(&self, seconds: u64) -> serde_json::Value {
        serde_json::json!({
            "seconds": seconds,
            "fps_median": fps_median(&self.fps),
            "frames": self.frames,
            "frame_build_p50_us": percentile_us(&self.render_ns, 50),
            "frame_build_p99_us": percentile_us(&self.render_ns, 99),
            "item_build_p50_us": percentile_us(&self.item_ns, 50),
            "item_build_p99_us": percentile_us(&self.item_ns, 99),
        })
    }
}

/// The probe root view: the same entries/list/item machinery as
/// [`super::ConversationStream`], driven by programmatic scroll + injection.
struct BenchStreamView {
    entries: Vec<StreamEntry>,
    counters: Arc<StreamCounters>,
    list: gpui::ListState,
    phase: Phase,
    started: Instant,
    /// When the STREAM phase began (injection-rate baseline).
    stream_started: Option<Instant>,
    last_tick: Instant,
    scroll_y: f32,
    /// Injection payload (delta-split document; target rate ~500 δ/s).
    deltas: Vec<String>,
    cursor: usize,
    deltas_injected: AtomicU64,
    /// Whether the tail received deltas since the last sync (renders skip the
    /// snapshot diff while clean — the SCROLL phase stays allocation-free).
    tail_dirty: bool,
    scroll_stats: PhaseMeasurements,
    stream_stats: PhaseMeasurements,
    /// Per-second counter deltas for the report's `per_second` array.
    samples: Vec<serde_json::Value>,
    output: PathBuf,
}

/// One mixed markdown turn (paragraphs with CJK/emoji/inline styles, a list).
fn markdown_turn(index: usize) -> String {
    let emoji = ["✅", "🚀", "📌", "🌊"][index % 4];
    format!(
        "## 转次 {index}：混排流式 {emoji}\n\n\
         段落 {index} 带 **加粗**、*斜体*、`行内代码` 与 CJK 混排；\
         中文与 English 交错换行时应自然折行，{emoji} 计入显示宽度。\n\n\
         - 任务甲 {index}\n- [x] 已完成项 {emoji}\n  - 嵌套项 `code`\n\n"
    )
}

/// One code turn (a fenced rust block; committed blocks highlight via T16).
fn code_turn(index: usize) -> String {
    format!(
        "```rust\nfn bench_{index}() -> u64 {{\n    let value = {index} * 42;\n    // 中文注释 {index}\n    value\n}}\n```\n\n"
    )
}

/// One table turn (CJK-width-padded GFM table).
fn table_turn(index: usize) -> String {
    format!(
        "| 列 A {index} | 列 B | 列 C |\n|:--|:-:|--:|\n| 1 | 中文数据 {index} | 3 |\n| 4 | 🚀 | 6 |\n\n"
    )
}

/// One long wrapped-CJK paragraph turn (exercises natural item reflow; the
/// content must appear in full — C4 禁截断).
fn wrapped_cjk_turn(index: usize) -> String {
    format!(
        "长段落 {index}：这一段用于验证变高 item 在窗口宽度变化时的自然折行。\
         中文与 English 混排需要保持稳定，CJK 计两列宽；内容完整呈现，\
         不允许以截断凑高度（C4）。变高几何下每一行都应完整出现在 item 内，\
         且滚动锚点漂移小于 1px。段落编号 {index} 结束。"
    )
}

fn user_echo(index: usize) -> String {
    format!("帮我看看第 {index} 段的输出 ✅")
}

fn user_echo_entry(index: usize, seq: u64) -> StreamEntry {
    let block_id = u64::MAX - (1 << 32) + seq;
    StreamEntry::User {
        lines: super::user_message_lines(block_id, &user_echo(index)),
    }
}

fn finished_assistant(doc: &str, counters: &StreamCounters) -> StreamEntry {
    let mut stream = MarkdownStream::new();
    stream.append(doc);
    stream.finish();
    let mut model = StreamModel::default();
    model.sync(&stream.snapshot(), counters);
    StreamEntry::Assistant {
        stream: Box::new(stream),
        model,
    }
}

/// Builds the 10k mixed semantic fixture: markdown turns (CJK/emoji/inline
/// styles), code turns, table turns, wrapped-CJK turns, user echoes, tool
/// cards, plan cards, and summary cards. Every assistant model syncs eagerly
/// so the first frame renders full natural heights (S8-T45 hydration 同语义).
fn build_mixed_entries(count: usize, cx: &mut Context<BenchStreamView>) -> Vec<StreamEntry> {
    let counters = StreamCounters::default();
    let mut entries: Vec<StreamEntry> = Vec::with_capacity(count + 1);
    let mut user_seq = 0u64;
    for index in 0..count {
        match index % 25 {
            0..=8 => entries.push(finished_assistant(&markdown_turn(index), &counters)),
            9..=11 => entries.push(finished_assistant(&code_turn(index), &counters)),
            12..=14 => entries.push(finished_assistant(&table_turn(index), &counters)),
            15..=17 => entries.push(finished_assistant(&wrapped_cjk_turn(index), &counters)),
            18..=19 => {
                entries.push(user_echo_entry(index, user_seq));
                user_seq += 1;
            }
            // Tool card: terminal read-only projection with bounded output.
            20 => {
                let card = cx.new(|_| {
                    ToolCard::hydrated(
                        Some(ToolCardInputProjection::ReadOnly {
                            tool: ReadOnlyToolKind::Read,
                        }),
                        ToolCallStatus::Success,
                        None,
                        Some(ToolCardResultProjection::ReadOnly {
                            status: ToolCallStatus::Success,
                            output: format!("tool output {index}\nline two ✅\nline three"),
                            reused: false,
                        }),
                    )
                });
                entries.push(StreamEntry::Tool { card });
            }
            // Plan card every 100 items; user echo otherwise.
            21 if index % 100 == 21 => {
                let card = cx.new(|cx| {
                    PlanCard::new(
                        Plan {
                            id: format!("bench-plan-{index}"),
                            thread_id: "bench".into(),
                            content: markdown_turn(index),
                            status: PlanStatus::Approved,
                            review_note: None,
                            reviewed_at: Some(1),
                        },
                        cx,
                    )
                });
                entries.push(StreamEntry::Plan { card });
            }
            21 => {
                entries.push(user_echo_entry(index, user_seq));
                user_seq += 1;
            }
            // Summary card every 100 items; user echo otherwise.
            22 if index % 100 == 22 => {
                let card = cx.new(|_| {
                    SummaryCard::new(TaskCostSummary {
                        message_id: format!("bench-summary-{index}"),
                        outcome: TaskSummaryOutcome::Completed,
                        usage: None,
                        cost: SummaryCost::Unavailable,
                        duration_ms: Some(1_200),
                        tool_count: 1,
                        cache_hit_percent: None,
                    })
                });
                entries.push(StreamEntry::Summary { card });
            }
            22 => {
                entries.push(user_echo_entry(index, user_seq));
                user_seq += 1;
            }
            _ => entries.push(finished_assistant(&wrapped_cjk_turn(index), &counters)),
        }
    }
    entries
}

impl BenchStreamView {
    fn new(output: PathBuf, cx: &mut Context<Self>) -> Self {
        // 预构建 10k 混合语义项：每项经真实 MarkdownStream + StreamModel
        // 管线物化（C6 场景：markdown/wrapped CJK/emoji/代码/全卡型）。
        // 末尾再追加一条专门的 streaming tail assistant turn。
        let mut entries = build_mixed_entries(ITEM_COUNT, cx);
        entries.push(finished_assistant(
            &markdown_turn(ITEM_COUNT),
            &StreamCounters::default(),
        ));
        let deltas = split_deltas(&markdown_turn(ITEM_COUNT + 1), 0x5EED);

        let list = gpui::ListState::new(entries.len(), gpui::ListAlignment::Top, px(600.0))
            .with_uniform_item_height(px(48.0));
        // SCROLL 阶段为纯程序化滚动：关闭原生 tail follow，防止首帧吸附到
        // 底部；STREAM 阶段视口停在冻结区，注入由显式失效驱动。
        list.set_follow_mode(gpui::FollowMode::Normal);

        let mut view = Self {
            entries,
            counters: Arc::new(StreamCounters::default()),
            list,
            phase: Phase::Scroll,
            started: Instant::now(),
            stream_started: None,
            last_tick: Instant::now(),
            scroll_y: 0.0,
            deltas,
            cursor: 0,
            deltas_injected: AtomicU64::new(0),
            tail_dirty: false,
            scroll_stats: PhaseMeasurements::default(),
            stream_stats: PhaseMeasurements::default(),
            samples: Vec::new(),
            output,
        };
        view.deltas.shrink_to_fit();

        // 帧驱动：滚动步进 + 阶段切换（1ms tick，spike 同款）。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(1))
                    .await;
                let alive = this
                    .update(cx, |this, cx| {
                        match this.phase {
                            Phase::Scroll => {
                                let dt = this.last_tick.elapsed().as_secs_f32();
                                this.last_tick = Instant::now();
                                this.scroll_y += SCROLL_SPEED_PX_S * dt;
                                this.set_scroll_y(this.scroll_y);
                                if this.started.elapsed() >= Duration::from_secs(SCROLL_SECONDS) {
                                    // 停在冻结区，尾部远离视口（spike 布置）。
                                    this.phase = Phase::Stream;
                                    this.stream_started = Some(Instant::now());
                                    this.scroll_y = PARK_OFFSET_Y;
                                    this.set_scroll_y(this.scroll_y);
                                }
                                cx.notify();
                            }
                            Phase::Stream => {
                                if this.started.elapsed()
                                    >= Duration::from_secs(SCROLL_SECONDS + STREAM_SECONDS)
                                {
                                    this.phase = Phase::Done;
                                }
                                cx.notify();
                            }
                            Phase::Done => {}
                        }
                        this.phase != Phase::Done
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();

        // 尾部注入：仅 STREAM 阶段，目标 ~500 δ/s。定时器节拍受主线程帧循环
        // 影响会抖动，这里按「应注入目标数 = 速率 × 已流时间」自校正补齐。
        // 注入直接写入最后一个 assistant item 自己的 MarkdownStream（生产
        // 语义：TextDelta → stream.append）。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(INJECT_TICK).await;
                let alive = this
                    .update(cx, |this, cx| {
                        if this.phase != Phase::Stream {
                            return this.phase != Phase::Done;
                        }
                        let elapsed = this
                            .stream_started
                            .map(|started| started.elapsed().as_secs_f64())
                            .unwrap_or(0.0);
                        let target = (elapsed * INJECT_RATE as f64) as usize;
                        let target = target.min(this.deltas.len());
                        if this.cursor < target {
                            {
                                let Some(StreamEntry::Assistant { stream, .. }) =
                                    this.entries.last_mut()
                                else {
                                    return false;
                                };
                                for delta in &this.deltas[this.cursor..target] {
                                    stream.append(delta);
                                }
                            }
                            let added = (target - this.cursor) as u64;
                            this.cursor = target;
                            this.deltas_injected.fetch_add(added, Ordering::Relaxed);
                            this.tail_dirty = true;
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();

        // 1s 采样：fps + 计数器差分（spike 口径）；DONE 时写报告退出。
        cx.spawn(async move |this, cx| {
            let mut second = 0u64;
            let mut previous = (0u64, 0u64, 0u64, 0u64); // frames, frozen, pending, deltas
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                second += 1;
                let done = this
                    .update(cx, |this, cx| {
                        let counters = &this.counters;
                        let frames = counters.frames.load(Ordering::Relaxed);
                        let frozen = counters.frozen_rematerializations.load(Ordering::Relaxed);
                        let pending = counters.pending_materializations.load(Ordering::Relaxed);
                        let deltas = this.deltas_injected.load(Ordering::Relaxed);
                        let (previous_frames, previous_frozen, previous_pending, previous_deltas) =
                            previous;
                        let fps = frames.saturating_sub(previous_frames);
                        this.samples.push(serde_json::json!({
                            "t": second,
                            "phase": phase_name(this.phase),
                            "fps": fps,
                            "frozen_remat": frozen.saturating_sub(previous_frozen),
                            "pending_mat": pending.saturating_sub(previous_pending),
                            "deltas": deltas.saturating_sub(previous_deltas),
                        }));
                        match this.phase {
                            Phase::Scroll => this.scroll_stats.fps.push(fps),
                            Phase::Stream => this.stream_stats.fps.push(fps),
                            Phase::Done => {}
                        }
                        previous = (frames, frozen, pending, deltas);
                        let done = this.phase == Phase::Done;
                        if done {
                            this.write_report(cx);
                            cx.quit();
                        }
                        done
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();

        view
    }

    /// Programmatic scroll to an absolute pixel offset via the list's own
    /// scroll API (unmeasured items keep their height hint until first paint;
    /// measured heights take over as items render).
    fn set_scroll_y(&mut self, y: f32) {
        let current = -f32::from(self.list.scroll_px_offset_for_scrollbar().y);
        let distance = y - current;
        if distance != 0.0 {
            self.list.scroll_by(px(distance));
        }
    }

    /// 差量同步：仅 mutable tail（最后一个 assistant item）参与快照 diff
    /// （P3/C4 白名单）；冻结段永不重物化。
    fn sync_tail(&mut self) {
        if !self.tail_dirty {
            return;
        }
        let index = self.entries.len().saturating_sub(1);
        let Some(StreamEntry::Assistant { stream, model }) = self.entries.last_mut() else {
            return;
        };
        let snapshot = stream.snapshot();
        model.sync(&snapshot, &self.counters);
        self.list.remeasure_items(index..index + 1);
        self.tail_dirty = false;
    }

    fn write_report(&self, cx: &App) {
        let subrow_count: usize = self.entries.iter().map(|entry| entry.row_count(cx)).sum();
        let report = serde_json::json!({
            "timestamp": unix_ms(),
            "mode": "probe_binary",
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "vsync_capped": true,
            "row_count": self.entries.len(),
            "item_count": self.entries.len(),
            "subrow_count": subrow_count,
            "committed_blocks": self.committed_blocks(),
            "deltas_injected": self.deltas_injected.load(Ordering::Relaxed),
            "committed_materializations": self
                .counters
                .committed_materializations
                .load(Ordering::Relaxed),
            "frozen_rematerializations": self
                .counters
                .frozen_rematerializations
                .load(Ordering::Relaxed),
            "pending_materializations": self
                .counters
                .pending_materializations
                .load(Ordering::Relaxed),
            "scroll": self.scroll_stats.to_json(SCROLL_SECONDS),
            "stream": self.stream_stats.to_json(STREAM_SECONDS),
            "per_second": self.samples,
        });
        let json = match serde_json::to_string_pretty(&report) {
            Ok(json) => json,
            Err(error) => {
                tracing::error!(%error, "vega --vega-bench-render: failed to serialize report");
                return;
            }
        };
        if let Err(error) = std::fs::write(&self.output, json) {
            tracing::error!(%error, path = %self.output.display(),
                "vega --vega-bench-render: failed to write report");
        }
    }

    fn committed_blocks(&self) -> usize {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                StreamEntry::Assistant { stream, .. } => Some(stream.snapshot().blocks.len()),
                _ => None,
            })
            .sum()
    }
}

impl Render for BenchStreamView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_t0 = Instant::now();
        let colors = vega_theme::theme(cx).colors;

        // 差量同步：仅在收到新 delta 时执行（SCROLL 阶段保持零分配帧）；
        // STREAM 阶段每次注入只物化 mutable tail 的新块（P3/C4 白名单）。
        self.sync_tail();

        let list = self.list.clone();
        let element = div()
            .size_full()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .child(
                div()
                    .id("bench-scroll")
                    .size_full()
                    .overflow_hidden()
                    .child(
                        gpui::list(
                            list,
                            cx.processor(
                                move |this: &mut BenchStreamView, index: usize, window, cx| {
                                    let item_t0 = Instant::now();
                                    let item = match this.entries.get(index) {
                                        Some(entry) => {
                                            render_entry(entry, &this.counters, window, cx)
                                        }
                                        None => div().into_any_element(),
                                    };
                                    this.record_item_build(item_t0);
                                    item
                                },
                            ),
                        )
                        .h_full()
                        .w_full(),
                    ),
            )
            .into_any_element();

        // render 回调耗时计入当前阶段（spike 口径的 frame build 时间）；
        // frames 计数器供 1s 采样器取每秒 fps。
        self.counters.record_render(render_t0);
        match self.phase {
            Phase::Scroll => {
                self.scroll_stats.frames += 1;
                self.scroll_stats
                    .render_ns
                    .push(render_t0.elapsed().as_nanos());
            }
            Phase::Stream => {
                self.stream_stats.frames += 1;
                self.stream_stats
                    .render_ns
                    .push(render_t0.elapsed().as_nanos());
            }
            Phase::Done => {}
        }
        element
    }
}

impl BenchStreamView {
    fn record_item_build(&mut self, started: Instant) {
        let elapsed = started.elapsed().as_nanos();
        match self.phase {
            Phase::Scroll => self.scroll_stats.item_ns.push(elapsed),
            Phase::Stream => self.stream_stats.item_ns.push(elapsed),
            Phase::Done => {}
        }
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Scroll => "scroll",
        Phase::Stream => "stream",
        Phase::Done => "done",
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}
