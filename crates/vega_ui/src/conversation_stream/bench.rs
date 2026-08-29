//! Self-measurement mode for `xtask bench render_frame` (S3-T17): the hidden
//! `--vega-bench-render <out.json>` flag boots a real GPUI window running the
//! conversation-stream machinery against a ~10k-row synthetic document, then
//! writes the measured JSON report and quits.
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
//!                 rows → frozen re-materializations must stay 0 (P3).
//!
//! Everything runs through the production path ([`super`]) — the same
//! [`MarkdownStream`] pipeline, [`StreamModel`] diffing, and row rendering the
//! app uses.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div, point, px, uniform_list,
};
use vega_markdown::MarkdownStream;
use vega_theme::Theme;

use super::{INJECT_TICK, StreamCounters, StreamModel, build_rows, sample_document, split_deltas};

/// Probe phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Programmatic scroll over the fully-built document.
    Scroll,
    /// Tail injection with the viewport parked on frozen rows.
    Stream,
    /// Measurement finished; report written, app quitting.
    Done,
}

const SCROLL_SECONDS: u64 = 8;
const STREAM_SECONDS: u64 = 12;
const SCROLL_SPEED_PX_S: f32 = 720.0;
/// Where the viewport parks for the STREAM phase (row ~41; tail ~10k rows away).
const PARK_OFFSET_Y: f32 = 1000.0;
/// ~450 rows per 200-block sample copy → 24 copies ≈ 10.8k rows.
const DOC_SAMPLE_COPIES: usize = 24;
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
    // Bench 模式不经过主应用启动路径：单独注册 light 主题供行渲染取 token。
    cx.set_global(Theme::light());
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
    row_ns: Vec<u128>,
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
            "row_build_p50_us": percentile_us(&self.row_ns, 50),
            "row_build_p99_us": percentile_us(&self.row_ns, 99),
        })
    }
}

/// The probe root view: the same stream/model/row machinery as
/// [`super::ConversationStream`], driven by programmatic scroll + injection.
struct BenchStreamView {
    stream: MarkdownStream,
    model: StreamModel,
    counters: Arc<StreamCounters>,
    scroll: gpui::UniformListScrollHandle,
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
    /// Whether the stream received deltas since the last sync (renders skip
    /// the snapshot diff while clean — the SCROLL phase stays allocation-free).
    dirty: bool,
    scroll_stats: PhaseMeasurements,
    stream_stats: PhaseMeasurements,
    /// Per-second counter deltas for the report's `per_second` array.
    samples: Vec<serde_json::Value>,
    output: PathBuf,
}

impl BenchStreamView {
    fn new(output: PathBuf, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            stream: MarkdownStream::new(),
            model: StreamModel::default(),
            counters: Arc::new(StreamCounters::default()),
            scroll: gpui::UniformListScrollHandle::new(),
            phase: Phase::Scroll,
            started: Instant::now(),
            stream_started: None,
            last_tick: Instant::now(),
            scroll_y: 0.0,
            deltas: Vec::new(),
            cursor: 0,
            deltas_injected: AtomicU64::new(0),
            dirty: true,
            scroll_stats: PhaseMeasurements::default(),
            stream_stats: PhaseMeasurements::default(),
            samples: Vec::new(),
            output,
        };

        // 预构建 ~10k 行文档：同步喂入真实 MarkdownStream 管线（spike 方法）。
        // 同一份文档再切一份 delta 作为 STREAM 阶段的注入载荷（500 δ/s × 12s
        // 需 6000 δ，~277KB 文档足够覆盖）。
        let sample = sample_document(200);
        let mut doc = String::with_capacity(sample.len() * DOC_SAMPLE_COPIES);
        for _ in 0..DOC_SAMPLE_COPIES {
            doc.push_str(&sample);
        }
        for delta in split_deltas(&doc, 0x5EED) {
            view.stream.append(&delta);
        }
        view.deltas = split_deltas(&doc, 0x5EED);
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
                            for delta in &this.deltas[this.cursor..target] {
                                this.stream.append(delta);
                            }
                            let added = (target - this.cursor) as u64;
                            this.cursor = target;
                            this.deltas_injected.fetch_add(added, Ordering::Relaxed);
                            this.dirty = true;
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
                            this.write_report();
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

    fn set_scroll_y(&self, y: f32) {
        self.scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(y), px(0.0)));
    }

    fn write_report(&self) {
        let report = serde_json::json!({
            "timestamp": unix_ms(),
            "mode": "probe_binary",
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "vsync_capped": true,
            "row_count": self.model.row_count(),
            "committed_blocks": self.stream.snapshot().blocks.len(),
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
}

impl Render for BenchStreamView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_t0 = Instant::now();
        let colors = vega_theme::theme(cx).colors;

        // 差量同步：仅在收到新 delta 时执行（SCROLL 阶段保持零分配帧）；
        // STREAM 阶段每次注入只物化 pending 尾块（P3）。
        if self.dirty {
            let snapshot = self.stream.snapshot();
            self.model.sync(&snapshot, &self.counters);
            self.dirty = false;
        }

        let rows = self.model.row_count();
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
                        uniform_list(
                            "bench-stream",
                            rows,
                            cx.processor(
                                move |this: &mut BenchStreamView,
                                      range: std::ops::Range<usize>,
                                      _window,
                                      cx| {
                                    let row_t0 = Instant::now();
                                    let rows = build_rows(&this.model, range, &this.counters, cx);
                                    match this.phase {
                                        Phase::Scroll => this
                                            .scroll_stats
                                            .row_ns
                                            .push(row_t0.elapsed().as_nanos()),
                                        Phase::Stream => this
                                            .stream_stats
                                            .row_ns
                                            .push(row_t0.elapsed().as_nanos()),
                                        Phase::Done => {}
                                    }
                                    rows
                                },
                            ),
                        )
                        .track_scroll(&self.scroll)
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
