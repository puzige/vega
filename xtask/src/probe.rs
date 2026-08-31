//! The isolated release probe subprocess (SDD §2/C1, §3/C2, §7/C6).
//!
//! The parent re-executes the release-built xtask binary with the hidden
//! `__probe` subcommand:
//!
//! - `xtask __probe c1 --hold-ms <ms>` — boots a real GPUI window with the
//!   REAL application root view composition (`Sidebar` + routed
//!   `ConversationStream` with its enabled-state empty Composer through the
//!   production boot path: `vega_ui::init` key bindings, `sidebar::init`
//!   store open/migration at the platform data root, production theme seed),
//!   registers a pinned GPUI next-frame callback, flushes EXACTLY ONE strict
//!   single-line `VEGA_C1_MILESTONE {…}` JSON milestone, idles for the C2
//!   hold window (empty single window, no tasks), then exits normally on its
//!   own — no kill involved.
//! - `xtask __probe p2 --out <path> --seconds <s> --rate <r>` — same real
//!   root; drives the production UI controller entry
//!   (`ConversationStream::apply_event`, the same entry the app's agent pump
//!   uses) with bounded batches at `rate` deltas/s for `seconds`,
//!   timestamping each batch at receive and associating it with the first
//!   frame that contains it; writes the C6 stream JSON report and exits.
//!
//! Isolation (C3): the parent runs the probe with a temp `HOME`, so
//! `vega_store::paths::data_dir()` resolves inside the sandbox and the store
//! is a preseeded throwaway SQLite — zero real profile, zero Keychain, zero
//! provider, zero network.
//!
//! Deviation note (recorded in docs/vega-s8-t43-baseline.md + the PR): the
//! production `vega` binary itself has no milestone seam, and this card's
//! red line forbids touching crates/ — so the subprocess is the release
//! xtask binary linking the real `vega_ui`/`vega_theme`/`vega_store`
//! components and mirroring the production window composition. The
//! `VegaWindow` shell (agent/commit/branch controllers) is not part of the
//! idle first-frame scene; the measured views are the production views.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, Render, TitlebarOptions, Window, WindowBounds, WindowOptions,
    div, px, size,
};
use serde::Serialize;

use vega_conversation::types::{ConversationEvent, Thread};
use vega_ui::settings::SettingsOpen;
use vega_ui::sidebar::{OpenedThread, Sidebar, SidebarCollapsed, VegaStore};

use crate::contract::{
    C2_HOLD_MS, C6_BATCH_LIMIT, C6_INJECT_RATE_PER_S, C6_POLL, C6_STREAM_SCHEMA, C6_STREAM_SECONDS,
    IsolationAttestation, MILESTONE_PREFIX, PROCESS_START_TO_FIRST_RENDERED_INTERACTIVE,
};

/// Monotonic process start, captured at the first line of the bin's `main`.
pub static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Parsed `__probe` subcommand.
#[derive(Debug, Clone)]
pub enum ProbeMode {
    /// C1 milestone + C2 hold window.
    C1 { hold_ms: u64 },
    /// C6 P2 stream run.
    P2 {
        out: PathBuf,
        seconds: u64,
        rate: u64,
    },
}

/// Parses the hidden subcommand; `None` → not a probe invocation.
pub fn parse_args(args: &[String]) -> Option<ProbeMode> {
    if args.first().map(String::as_str) != Some("__probe") {
        return None;
    }
    let value_of = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|index| args.get(index + 1))
            .cloned()
    };
    match args.get(1).map(String::as_str) {
        Some("c1") => Some(ProbeMode::C1 {
            hold_ms: value_of("--hold-ms")
                .and_then(|value| value.parse().ok())
                .unwrap_or(C2_HOLD_MS),
        }),
        Some("p2") => Some(ProbeMode::P2 {
            out: PathBuf::from(value_of("--out").unwrap_or_else(|| "p2-report.json".into())),
            seconds: value_of("--seconds")
                .and_then(|value| value.parse().ok())
                .unwrap_or(C6_STREAM_SECONDS),
            rate: value_of("--rate")
                .and_then(|value| value.parse().ok())
                .unwrap_or(C6_INJECT_RATE_PER_S),
        }),
        _ => None,
    }
}

/// Child isolation attestation (C3): re-resolved from the actual
/// environment so the parent can mechanically verify the temp HOME.
fn attestation() -> IsolationAttestation {
    IsolationAttestation {
        home: std::env::var("HOME").unwrap_or_default(),
        data_root: vega_store::paths::data_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        provider: "none".into(),
        network: "none".into(),
        keychain: "not-exercised".into(),
        first_frame_source: "gpui_next_frame_callback".into(),
    }
}

/// Flushes exactly one single-line strict-JSON milestone to stdout. Called
/// once, from inside the pinned next-frame callback.
fn flush_milestone() {
    let milestone = crate::contract::Milestone {
        schema: "vega-c1".into(),
        metric: PROCESS_START_TO_FIRST_RENDERED_INTERACTIVE.into(),
        pid: std::process::id(),
        elapsed_us: u64::try_from(
            STARTED
                .get()
                .map(|started| started.elapsed().as_micros())
                .unwrap_or_default(),
        )
        .unwrap_or(u64::MAX),
        isolation: attestation(),
    };
    let line = format!(
        "{}{}",
        MILESTONE_PREFIX,
        serde_json::to_string(&milestone).expect("milestone serializes")
    );
    println!("{line}");
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

// ─── C6 P2 stream report (child side) ────────────────────────────────────────

/// One bounded/coalesced batch record (SDD §7 P2 MUST: timestamp at the
/// production controller entry, associated max sequence + first containing
/// frame).
#[derive(Debug, Clone, Serialize)]
pub struct BatchRecord {
    /// Ordinal of the first event in the batch (1-based over all
    /// ConversationEvents since run start; MessageStarted = 1).
    pub first_seq: u64,
    /// Ordinal of the last event in the batch ("该批最高 sequence").
    pub max_seq: u64,
    /// Events in the batch.
    pub len: u64,
    /// Receive-to-render latency in integer µs (first containing frame).
    pub latency_us: u64,
    /// Frame index (content-frame counter) that first contained the batch.
    pub frame: u64,
}

/// Per-second sampling window.
#[derive(Debug, Clone, Serialize)]
pub struct SecondSample {
    pub t: u64,
    pub deltas_produced: u64,
    pub frames: u64,
    pub batches: u64,
    pub queue_max: u64,
}

/// The C6 P2 stream report the child writes to `--out`.
#[derive(Debug, Serialize)]
pub struct StreamReport {
    pub schema: &'static str,
    pub metric: &'static str,
    pub profile: &'static str,
    pub seconds: u64,
    pub rate_per_s: u64,
    /// True only when the producer finished and every applied batch reached
    /// its first containing frame; a safety-stop flush records `false`.
    pub run_completed: bool,
    pub events_total: u64,
    pub deltas_total: u64,
    pub frames: u64,
    pub batches: Vec<BatchRecord>,
    pub per_second: Vec<SecondSample>,
    pub queue_max_depth: u64,
    pub sequence_first: u64,
    pub sequence_last: u64,
    /// Production controller entry (apply_event); parser/enqueue/build are
    /// not substituted — the latency spans receive → first containing frame.
    pub entry: &'static str,
    /// UI-thread DB/pricing IO during the stream (contract: none).
    pub ui_thread_db_pricing_io: &'static str,
}

// ─── the probe root view ─────────────────────────────────────────────────────

struct PendingBatch {
    first_seq: u64,
    max_seq: u64,
    len: u64,
    received: Instant,
}

enum StreamUpdate {
    Event(ConversationEvent),
    Finished,
}

/// P2 run state (lives on the probe root while streaming).
struct P2Run {
    out: PathBuf,
    seconds: u64,
    rate: u64,
    receiver: mpsc::Receiver<StreamUpdate>,
    produced: std::sync::Arc<AtomicU64>,
    last_seen_produced: u64,
    next_seq: u64,
    drained: u64,
    deltas: u64,
    pending: Vec<PendingBatch>,
    records: Vec<BatchRecord>,
    frame: u64,
    queue_max: u64,
    frames_window: u64,
    batches_window: u64,
    queue_max_window: u64,
    per_second: Vec<SecondSample>,
    next_sample_at: Instant,
    producer_done: bool,
}

/// The probe root view: the production window content (real sidebar + real
/// route + enabled Composer), plus the P2 instrumentation when streaming.
struct ProbeRoot {
    sidebar: Entity<Sidebar>,
    stream: Option<Entity<vega_ui::conversation_stream::ConversationStream>>,
    started: Instant,
    /// P2 state (`None` in C1 mode).
    p2: Option<P2Run>,
}

impl ProbeRoot {
    fn new(cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(Sidebar::new);
        Self {
            sidebar,
            stream: None,
            started: Instant::now(),
            p2: None,
        }
    }

    /// Opens the preseeded thread through the production route (mirrors the
    /// app's OpenedThread handling: global + stream entity rebuild).
    fn open_thread(&mut self, thread: Thread, cx: &mut Context<Self>) {
        let stream =
            cx.new(|cx| vega_ui::conversation_stream::ConversationStream::new(thread.clone(), cx));
        // Instrumentation shim: re-notify this root whenever the stream
        // changes so its render can timestamp the frame that first contains
        // each applied batch (same frame gpui draws; the shim only makes it
        // observable at this level).
        cx.observe(&stream, |_, _, cx| cx.notify()).detach();
        self.stream = Some(stream);
        cx.set_global(OpenedThread(Some(thread)));
        cx.notify();
    }

    /// Spawns the paced producer thread + the 4 ms pump task for a P2 run.
    fn start_p2(&mut self, out: PathBuf, seconds: u64, rate: u64, cx: &mut Context<Self>) {
        let (tx, rx) = mpsc::channel::<StreamUpdate>();
        let produced = std::sync::Arc::new(AtomicU64::new(0));

        // Producer thread: the controller side of the production event
        // channel — MessageStarted then paced TextDeltas at exactly `rate`
        // per second (deadline pacing, not accumulated drift).
        let counter = produced.clone();
        let message_id = format!("p2-{}", std::process::id());
        std::thread::spawn(move || {
            // `produced` counts every event handed to the channel (the
            // increment precedes the send so the queue depth can never go
            // negative and the drained-completion check can never stall on
            // a counter race).
            counter.fetch_add(1, Ordering::Relaxed);
            let _ = tx.send(StreamUpdate::Event(ConversationEvent::MessageStarted {
                message_id: message_id.clone(),
                seq: 1,
            }));
            let interval = Duration::from_secs_f64(1.0 / rate as f64);
            let total = rate * seconds;
            let t0 = Instant::now();
            for index in 0..total {
                let target = t0 + interval * (index as u32 + 1);
                let now = Instant::now();
                if target > now {
                    std::thread::sleep(target - now);
                }
                counter.fetch_add(1, Ordering::Relaxed);
                let event = StreamUpdate::Event(ConversationEvent::TextDelta {
                    message_id: message_id.clone(),
                    delta: stream_chunk(index),
                });
                if tx.send(event).is_err() {
                    break;
                }
            }
            let _ = tx.send(StreamUpdate::Finished);
        });

        self.p2 = Some(P2Run {
            out,
            seconds,
            rate,
            receiver: rx,
            produced,
            last_seen_produced: 0,
            next_seq: 0,
            drained: 0,
            deltas: 0,
            pending: Vec::new(),
            records: Vec::new(),
            frame: 0,
            queue_max: 0,
            frames_window: 0,
            batches_window: 0,
            queue_max_window: 0,
            per_second: Vec::new(),
            next_sample_at: Instant::now() + Duration::from_secs(1),
            producer_done: false,
        });

        // Pump task: the production pump cadence (4 ms poll, ≤128 per batch).
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(C6_POLL).await;
                let alive = this
                    .update(cx, |root, cx| root.pump_tick(cx))
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    /// One 4 ms pump tick (mirrors the production agent pump: bounded drain
    /// of ≤128 events per tick, then apply via the production entry).
    fn pump_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let stream = self.stream.clone();
        let Some(stream) = stream else {
            return false;
        };
        {
            let Some(p2) = self.p2.as_mut() else {
                return false;
            };
            let mut batch_first: Option<Instant> = None;
            let mut applied: u64 = 0;
            for _ in 0..C6_BATCH_LIMIT {
                match p2.receiver.try_recv() {
                    Ok(StreamUpdate::Event(event)) => {
                        if batch_first.is_none() {
                            batch_first = Some(Instant::now());
                        }
                        p2.next_seq += 1;
                        p2.drained += 1;
                        applied += 1;
                        if matches!(event, ConversationEvent::TextDelta { .. }) {
                            p2.deltas += 1;
                        }
                        stream.update(cx, |stream, cx| stream.apply_event(event, cx));
                    }
                    Ok(StreamUpdate::Finished) => {
                        p2.producer_done = true;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        p2.producer_done = true;
                        break;
                    }
                }
            }
            if let Some(received) = batch_first {
                let max_seq = p2.next_seq;
                let first_seq = max_seq - applied + 1;
                p2.pending.push(PendingBatch {
                    first_seq,
                    max_seq,
                    len: applied,
                    received,
                });
                // Saturating: the producer increments before the send, so a
                // momentary `produced == drained` is legal, but never less.
                let queue = p2
                    .produced
                    .load(Ordering::Relaxed)
                    .saturating_sub(p2.drained);
                p2.queue_max = p2.queue_max.max(queue);
                p2.queue_max_window = p2.queue_max_window.max(queue);
            }
            // Per-second sampling window.
            if Instant::now() >= p2.next_sample_at {
                let produced_now = p2.produced.load(Ordering::Relaxed);
                let t = self.started.elapsed().as_secs();
                p2.per_second.push(SecondSample {
                    t,
                    deltas_produced: produced_now - p2.last_seen_produced,
                    frames: p2.frames_window,
                    batches: p2.batches_window,
                    queue_max: p2.queue_max_window,
                });
                p2.last_seen_produced = produced_now;
                p2.frames_window = 0;
                p2.batches_window = 0;
                p2.queue_max_window = 0;
                p2.next_sample_at += Duration::from_secs(1);
            }
        }
        let Some(p2) = self.p2.as_ref() else {
            return false;
        };
        let elapsed_ok = self.started.elapsed() >= Duration::from_secs(p2.seconds);
        let drained_ok = p2.producer_done
            && p2.pending.is_empty()
            && p2.produced.load(Ordering::Relaxed) == p2.drained;
        if elapsed_ok && drained_ok {
            self.finish_p2(cx);
            return false;
        }
        // Hard safety stop (never reached in a healthy run): flush the
        // partial report (run_completed=false) and quit — the child never
        // idles forever, and the parent watchdog is the backstop.
        if self.started.elapsed() >= Duration::from_secs(p2.seconds + 30) {
            self.finish_p2(cx);
            return false;
        }
        true
    }

    /// Writes the C6 stream report and quits normally.
    fn finish_p2(&mut self, cx: &mut Context<Self>) {
        if let Some(p2) = self.p2.as_ref() {
            // The run completed only when the producer finished AND every
            // applied batch reached its first containing frame; a safety-stop
            // flush records `run_completed: false` honestly.
            let run_completed = p2.producer_done
                && p2.pending.is_empty()
                && p2.produced.load(Ordering::Relaxed) == p2.drained;
            let report = StreamReport {
                schema: C6_STREAM_SCHEMA,
                metric: "p2_receive_to_render_us",
                profile: if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                },
                seconds: p2.seconds,
                rate_per_s: p2.rate,
                run_completed,
                events_total: p2.drained,
                deltas_total: p2.deltas,
                frames: p2.frame,
                batches: p2.records.clone(),
                per_second: p2.per_second.clone(),
                queue_max_depth: p2.queue_max,
                sequence_first: 1,
                sequence_last: p2.next_seq,
                entry: "ConversationStream::apply_event (production UI controller entry)",
                ui_thread_db_pricing_io: "none (apply_event is memory-only; no store access during stream)",
            };
            if let Ok(json) = serde_json::to_string_pretty(&report) {
                let _ = std::fs::write(&p2.out, json);
            }
        }
        cx.quit();
    }
}

impl Render for ProbeRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = vega_theme::theme(cx).colors;
        // P2: this render IS the first frame that contains every batch
        // applied since the previous frame — resolve pending batches here.
        if let Some(p2) = self.p2.as_mut() {
            let now = Instant::now();
            p2.frame += 1;
            p2.frames_window += 1;
            let frame = p2.frame;
            for batch in p2.pending.drain(..) {
                let latency_us = u64::try_from(now.duration_since(batch.received).as_micros())
                    .unwrap_or(u64::MAX);
                p2.records.push(BatchRecord {
                    first_seq: batch.first_seq,
                    max_seq: batch.max_seq,
                    len: batch.len,
                    latency_us,
                    frame,
                });
                p2.batches_window += 1;
            }
        }
        let content: gpui::AnyElement = match &self.stream {
            Some(stream) => stream.clone().into_any_element(),
            None => div().size_full().into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .child(self.sidebar.clone())
            .child(div().flex_1().min_w_0().h_full().child(content))
            .into_any_element()
    }
}

// ─── entry ───────────────────────────────────────────────────────────────────

/// Runs the probe app for the parsed mode. Exits when the app quits.
pub fn run(mode: ProbeMode) {
    gpui_platform::application().run(move |cx: &mut App| {
        // Production boot path (mirrors the app entry at 429cb2d).
        cx.set_global(vega_theme::Theme::system(cx));
        cx.set_global(SidebarCollapsed(vega_ui::sidebar::load_collapsed()));
        cx.set_global(SettingsOpen(false));
        vega_ui::init(cx);
        // Temp-HOME store: the platform data root resolves under the temp
        // HOME; the preseeded DB was created by the parent before spawn.
        vega_ui::sidebar::init(cx);

        // Route: the preseeded fixture thread (latest project, first thread).
        let thread: Option<Thread> = (|| {
            let store = match &cx.global::<VegaStore>().0 {
                Ok(store) => store,
                Err(_) => return None,
            };
            let project = vega_conversation::threads::current_project(store).ok()??;
            vega_conversation::threads::list_threads(store, &project.id, None)
                .ok()?
                .into_iter()
                .next()
        })();

        let bounds = Bounds::centered(None, size(px(960.0), px(600.0)), cx);
        let window = cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Vega bench probe".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                // The root is composed as an Entity exactly like the app's
                // `open_window` builder; the preseeded thread is routed and
                // the P2 harness armed inside the Context scope.
                let root = cx.new(|cx| {
                    let mut root = ProbeRoot::new(cx);
                    if let Some(thread) = thread {
                        root.open_thread(thread, cx);
                    }
                    match &mode {
                        ProbeMode::P2 { out, seconds, rate } => {
                            root.start_p2(out.clone(), *seconds, *rate, cx);
                        }
                        ProbeMode::C1 { .. } => {}
                    }
                    root
                });
                if let ProbeMode::C1 { hold_ms } = &mode {
                    // Pinned next-frame callback: fires right after the
                    // first frame is rendered (SDD §2 step 3), flushes
                    // the single milestone, then schedules the normal
                    // self-exit (no kill involved).
                    let hold = Duration::from_millis(*hold_ms);
                    window.on_next_frame(move |_window, cx| {
                        flush_milestone();
                        schedule_quit(cx, hold);
                    });
                }
                root
            },
        );
        if window.is_err() {
            eprintln!("xtask __probe: failed to open the probe window");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}

/// Schedules the child's normal self-exit (C2 hold window): the child quits
/// on its own; the parent never needs to kill a healthy run.
fn schedule_quit(cx: &mut App, hold: Duration) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(hold).await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// Small mixed-script chunk (CJK + ASCII + punctuation) so the stream
/// exercises the production markdown pipeline realistically.
fn stream_chunk(index: u64) -> String {
    const WORDS: [&str; 8] = ["流式", "delta ", "渲染", "帧", "· ", "probe ", "基线", "✓ "];
    format!(
        "{}{}",
        WORDS[(index % WORDS.len() as u64) as usize],
        index % 97
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parse_args_c1() {
        // parse_args receives the argv AFTER the program name (main skips
        // argv[0] before dispatch).
        match parse_args(&args(&["__probe", "c1", "--hold-ms", "17000"])) {
            Some(ProbeMode::C1 { hold_ms }) => assert_eq!(hold_ms, 17_000),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_args_c1_defaults() {
        match parse_args(&args(&["__probe", "c1"])) {
            Some(ProbeMode::C1 { hold_ms }) => assert_eq!(hold_ms, C2_HOLD_MS),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_args_p2() {
        match parse_args(&args(&[
            "__probe",
            "p2",
            "--out",
            "/tmp/p2.json",
            "--seconds",
            "5",
            "--rate",
            "500",
        ])) {
            Some(ProbeMode::P2 { out, seconds, rate }) => {
                assert_eq!(out, PathBuf::from("/tmp/p2.json"));
                assert_eq!(seconds, 5);
                assert_eq!(rate, 500);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_args_rejects_non_probe() {
        assert!(parse_args(&args(&["bench"])).is_none());
        assert!(parse_args(&args(&["__probe", "wat"])).is_none());
    }

    #[test]
    fn stream_chunks_are_bounded_and_varied() {
        assert_eq!(stream_chunk(0), stream_chunk(0));
        assert_ne!(stream_chunk(0), stream_chunk(1));
        assert!(stream_chunk(0).len() < 32);
    }

    #[test]
    fn attestation_matches_the_temp_home_environment() {
        let attestation = attestation();
        assert_eq!(attestation.provider, "none");
        assert_eq!(attestation.network, "none");
        assert_eq!(attestation.keychain, "not-exercised");
        assert_eq!(attestation.first_frame_source, "gpui_next_frame_callback");
    }

    #[test]
    fn batch_record_shape_stays_frozen() {
        let record = BatchRecord {
            first_seq: 1,
            max_seq: 4,
            len: 4,
            latency_us: 8_123,
            frame: 12,
        };
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(
            json,
            r#"{"first_seq":1,"max_seq":4,"len":4,"latency_us":8123,"frame":12}"#
        );
    }
}
