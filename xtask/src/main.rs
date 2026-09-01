//! Development tasks for the Vega workspace (bench, run, package).
//!
//! S8-T43 (A2-04): the S3-T17 measurement pipeline was rewritten to the
//! frozen T42 contracts — C1 first-rendered-interactive (next-frame
//! milestone subprocess), C2 release RSS (raw bytes, 20 processes,
//! +5/+10/+15 s medians, gray-zone extension), C6 P2 streaming (production
//! controller entry, 10 s @ 1,000 deltas/s) and real refresh-rate detection
//! for the P1 margin verdict. The legacy `spawn_to_exit` / MiB-as-MB /
//! hardcoded-60Hz / ~500δ/s measurements are gone (SDD §0 verified at
//! `429cb2d`; historical S6 numbers are noncomparable).

mod contract;
mod probe;
mod protocol;
mod provenance;
mod render;
mod report;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use contract::{
    C1_ROUNDS, C1_THRESHOLD_P95_US, C2_EXTENSION_ROUNDS, C2_ROUNDS, C6_THRESHOLD_P99_US,
    P2_WATCHDOG, P8_THRESHOLD_BYTES, STATUS_HARDWARE_PENDING, STATUS_PERFORMANCE_GATE_FAILED,
};
use protocol::{Isolation, Round};
use provenance::{Provenance, ReleaseBuild};

fn main() {
    let _ = probe::STARTED.set(Instant::now());
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Hidden probe subcommand: the re-executed release binary running as the
    // isolated measurement subprocess (see probe::run).
    if let Some(mode) = probe::parse_args(&args) {
        probe::run(mode);
        return;
    }
    if let Err(error) = dispatch(&args) {
        eprintln!("xtask error: {error:#}");
        std::process::exit(1);
    }
}

fn dispatch(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("bench") => bench(),
        Some("bench-p7") => bench_c1c2_only(),
        Some("bench-p2") => bench_p2_only(),
        other => {
            if let Some(other) = other {
                eprintln!("unknown subcommand: {other}");
            }
            eprintln!("usage: cargo xtask bench [or bench-p7 | bench-p2]");
            std::process::exit(2);
        }
    }
}

// ─── bench scenarios ─────────────────────────────────────────────────────────

/// Full run: C1+C2 spawn rounds, C6 P2 stream, C6 P1 render margin.
fn bench() -> Result<()> {
    let started = Instant::now();
    let workspace = workspace_root()?;
    let build = provenance::rebuild_release(&workspace)?;
    let prov = provenance::collect(&workspace, &build)?;

    println!(
        "\nvega bench — S8-T43 frozen contracts (C1/C2/C6; SDD v1.0). \
         Historical S6 numbers are noncomparable.\n"
    );

    let c1c2 = bench_c1_c2(&build, &prov, C2_ROUNDS)?;
    let mut report = report::BenchReport::new(&prov, c1c2.clone(), provenance::cutoff()?);
    match bench_p2_with(&build, &prov) {
        Ok(p2) => {
            let p2 = Some(p2);
            let p1 = render::measure_render_margin(&workspace, &build, &prov)?;
            let p1 = Some(p1);
            report.p2 = p2.clone();
            report.p1_margin = p1.clone();
            let path = report::write(&report)?;
            print_summary(&c1c2, &p2, &p1, &path, started.elapsed());
            Ok(())
        }
        Err(error) => {
            // C1/C2 evidence survives a P2 harness failure; recorded as
            // not-run rather than fabricated.
            eprintln!("P2 probe failed (recorded NOT RUN): {error:#}");
            report.p2 = None;
            let path = report::write(&report)?;
            print_summary(&c1c2, &None, &None, &path, started.elapsed());
            Err(error)
        }
    }
}

/// C1+C2 only (P7/P8 evidence).
fn bench_c1c2_only() -> Result<()> {
    let started = Instant::now();
    let workspace = workspace_root()?;
    let build = provenance::rebuild_release(&workspace)?;
    let prov = provenance::collect(&workspace, &build)?;
    let c1c2 = bench_c1_c2(&build, &prov, C2_ROUNDS)?;
    let report = report::BenchReport::new(&prov, c1c2.clone(), provenance::cutoff()?);
    let path = report::write(&report)?;
    print_summary(&c1c2, &None, &None, &path, started.elapsed());
    Ok(())
}

/// C6 P2 only (stream latency evidence).
fn bench_p2_only() -> Result<()> {
    let started = Instant::now();
    let workspace = workspace_root()?;
    let build = provenance::rebuild_release(&workspace)?;
    let prov = provenance::collect(&workspace, &build)?;
    let p2 = bench_p2_with(&build, &prov)?;
    let mut report =
        report::BenchReport::new(&prov, report::C1C2Result::empty(), provenance::cutoff()?);
    report.p2 = Some(p2.clone());
    let path = report::write(&report)?;
    print_summary(
        &report::C1C2Result::empty(),
        &Some(p2),
        &None,
        &path,
        started.elapsed(),
    );
    Ok(())
}

/// The C1+C2 protocol over the isolated temp HOME (SDD §2 + §3), including
/// the gray-zone extension rounds when the p95 lands in the band.
fn bench_c1_c2(
    build: &ReleaseBuild,
    prov: &Provenance,
    rounds: usize,
) -> Result<report::C1C2Result> {
    let _ = prov;
    let temp = make_sandbox()?;
    let isolation = Isolation {
        home: temp.home.clone(),
    };
    preseed_profile(&isolation.home)?;

    println!(
        "C1 {name} × {C1_ROUNDS} fresh release processes (p95 < {C1_THRESHOLD_P95_US} µs, \
         next-frame milestone; NOT spawn-to-exit)",
        name = contract::PROCESS_START_TO_FIRST_RENDERED_INTERACTIVE
    );
    let mut all = protocol::run_c1_c2(&build.xtask_bin, &isolation, rounds)?;
    // Frozen-contract guard: never silently proceed with fewer rounds than
    // the schema froze (SDD §3 step 3).
    protocol::assert_round_count(&all, rounds)?;
    let (samples, p95, gate) = protocol::c1_gate(&all);
    println!(
        "C1 p95 = {p95} µs over {} samples → gate {gate}",
        samples.len()
    );

    println!(
        "C2 P8 RSS (raw bytes, +5/+10/+15 s medians, threshold < {P8_THRESHOLD_BYTES} B \
         [OPEN unit ruling: decimal MB])"
    );
    let (mut c2_p95, mut c2_gate, mut drift_rounds) = protocol::c2_gate(&all);
    let mut extended = false;
    if rounds == C2_ROUNDS && contract::in_gray_zone(c2_p95) {
        println!(
            "C2 p95 {c2_p95} B is in the gray zone [98,000,000, 102,000,000) → \
             extending with {C2_EXTENSION_ROUNDS} rounds and recomputing (same math)"
        );
        let more = protocol::run_c1_c2(&build.xtask_bin, &isolation, C2_EXTENSION_ROUNDS)?;
        all.extend(more);
        let (new_p95, new_gate, new_drift) = protocol::c2_gate(&all);
        c2_p95 = new_p95;
        c2_gate = new_gate;
        drift_rounds = new_drift;
        extended = true;
    }
    println!(
        "C2 p95 = {c2_p95} B ({} MB / {} MiB) → gate {c2_gate}, drifting rounds {drift_rounds}{}",
        contract::bytes_as_decimal_mb(c2_p95),
        contract::bytes_as_mib(c2_p95),
        if extended {
            ", extended (gray zone)"
        } else {
            ""
        }
    );

    let failed: Vec<String> = all
        .iter()
        .filter_map(|round| {
            round
                .fail
                .map(|fail| format!("round {}: {fail}", round.round))
        })
        .collect();
    Ok(report::C1C2Result {
        rounds: all.iter().map(round_json).collect(),
        c1_samples_us: samples.clone(),
        c1_p50_us: contract::percentile(&samples, 50),
        c1_p95_us: p95,
        c1_p99_us: contract::percentile(&samples, 99),
        c1_max_us: samples.last().copied().unwrap_or(0),
        c1_gate_passed: gate,
        c2_medians_bytes: protocol::c2_medians(&all),
        c2_p95_bytes: c2_p95,
        c2_gate_passed: c2_gate,
        c2_drifting_rounds: drift_rounds,
        c2_extended: extended,
        fail_conditions: failed,
        sandbox_root: temp.root.display().to_string(),
    })
}

/// The C6 P2 short run (10 s @ 1,000 deltas/s; daily-PR feedback, NOT the
/// 5-minute terminal soak — SDD §7).
fn bench_p2_with(build: &ReleaseBuild, prov: &Provenance) -> Result<report::P2Result> {
    let _ = prov;
    let temp = make_sandbox()?;
    let isolation = Isolation {
        home: temp.home.clone(),
    };
    preseed_profile(&isolation.home)?;
    println!(
        "C6 P2 stream: 10 s @ 1,000 deltas/s through ConversationStream::apply_event \
         (daily-PR feedback; 5-minute soak is T48/T49)"
    );
    let out = temp.root.join("p2-report.json");
    let seconds = contract::C6_STREAM_SECONDS.to_string();
    let rate = contract::C6_INJECT_RATE_PER_S.to_string();
    let mut child = isolation.command(
        &build.xtask_bin,
        &[
            "__probe",
            "p2",
            "--out",
            out.to_str().context("non-UTF8 temp path")?,
            "--seconds",
            &seconds,
            "--rate",
            &rate,
        ],
    );
    child.stdout(std::process::Stdio::inherit());
    let mut child = child.spawn().context("failed to spawn the p2 probe")?;
    // Watchdog: the p2 child must exit on its own well inside this window
    // (10 s stream + boot). A timeout kill is a FAIL, never success
    // (SDD §2 判失败 semantics extended to every probe phase).
    let watchdog_deadline = Instant::now() + P2_WATCHDOG;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= watchdog_deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!(
                        "the p2 probe exceeded the {}s watchdog; killed (a timeout is a \
                         FAIL, never counted as success)",
                        P2_WATCHDOG.as_secs()
                    );
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(error) => return Err(error).context("failed to wait for the p2 probe"),
        }
    };
    if !status.success() {
        bail!("the p2 probe exited with {status}");
    }
    let raw = std::fs::read_to_string(&out)
        .with_context(|| format!("failed to read {}", out.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).context("the p2 probe report is not valid JSON")?;
    let latencies: Vec<u64> = value["batches"]
        .as_array()
        .map(|records| {
            records
                .iter()
                .filter_map(|record| record["latency_us"].as_u64())
                .collect()
        })
        .unwrap_or_default();
    let mut sorted = latencies.clone();
    sorted.sort_unstable();
    let p99 = contract::percentile(&sorted, 99);
    let p50 = contract::percentile(&sorted, 50);
    let run_completed = value["run_completed"].as_bool().unwrap_or(false);
    let gate = p99 < C6_THRESHOLD_P99_US && !sorted.is_empty() && run_completed;
    println!(
        "P2 p50 = {p50} µs, p99 = {p99} µs over {} batches (run_completed={run_completed}) → \
         gate {gate} (< {C6_THRESHOLD_P99_US} µs)",
        sorted.len()
    );
    Ok(report::P2Result {
        seconds: value["seconds"].as_u64().unwrap_or_default(),
        rate_per_s: value["rate_per_s"].as_u64().unwrap_or_default(),
        run_completed,
        events_total: value["events_total"].as_u64().unwrap_or_default(),
        deltas_total: value["deltas_total"].as_u64().unwrap_or_default(),
        frames: value["frames"].as_u64().unwrap_or_default(),
        queue_max_depth: value["queue_max_depth"].as_u64().unwrap_or_default(),
        batch_latencies_us: latencies,
        per_second: value["per_second"].as_array().cloned().unwrap_or_default(),
        p50_us: p50,
        p99_us: p99,
        gate_passed: gate,
        schema: value["schema"].as_str().unwrap_or_default().to_string(),
        sandbox_root: temp.root.display().to_string(),
    })
}

// ─── sandbox (C3 isolation) ──────────────────────────────────────────────────

struct Sandbox {
    root: PathBuf,
    home: PathBuf,
}

/// Creates the isolated temp sandbox: fresh `HOME`, `/tmp` logs and reports,
/// nothing inside the repo (C3 isolation MUSTs).
fn make_sandbox() -> Result<Sandbox> {
    let root = std::env::temp_dir().join(format!("vega-t43-{}", unix_ms()));
    let home = root.join("home");
    std::fs::create_dir_all(&home)
        .with_context(|| format!("failed to create the sandbox HOME at {}", home.display()))?;
    std::fs::create_dir_all(root.join("logs"))?;
    Ok(Sandbox { root, home })
}

/// Preseeds the isolated profile (C3): a project row AND one active thread
/// exist before the probe boots so the real route (Sidebar → routed
/// ConversationStream with its empty Composer) opens without any
/// provider/key interaction.
fn preseed_profile(home: &Path) -> Result<()> {
    let data_dir = vega_store::paths::data_dir_from(None, home);
    std::fs::create_dir_all(&data_dir)?;
    let store = vega_store::Store::open(data_dir.join("vega.db"))?;
    store.migrate()?;
    let project =
        vega_store::projects::create(store.conn(), "/tmp/vega-bench-repo", "vega-bench", None)?;
    let now_ms = unix_ms() as i64;
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            id: "vega-bench-fixture-thread",
            project_id: &project.id,
            title: "vega bench fixture",
            mode: "execute",
            permission_mode: "confirm",
            model: "vega-bench-mock",
            status: "active",
            pinned: false,
            unread: false,
            created_at: now_ms,
            updated_at: now_ms,
        },
    )?;
    Ok(())
}

fn round_json(round: &Round) -> serde_json::Value {
    serde_json::json!({
        "round": round.round,
        "c1_parent_latency_us": round.parent_latency_us,
        "c1_child_elapsed_us": round.child_elapsed_us,
        "c2_rss_bytes_5s_10s_15s": round.rss_bytes,
        "c2_median_bytes": round.rss_median(),
        "exit_kind": round.exit_kind,
        "exit_code": round.exit_code,
        "fail": round.fail,
        "stdout_lines": round.stdout_lines,
    })
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

fn print_summary(
    c1c2: &report::C1C2Result,
    p2: &Option<report::P2Result>,
    p1: &Option<render::RenderMargin>,
    path: &Path,
    elapsed: Duration,
) {
    println!("\n{:<16} frozen-contract result", "metric");
    if c1c2.has_data() {
        println!(
            "{:<16} p50={}µs p95={}µs p99={}µs max={}µs / threshold {}µs → {}",
            "P7 (C1)",
            c1c2.c1_p50_us,
            c1c2.c1_p95_us,
            c1c2.c1_p99_us,
            c1c2.c1_max_us,
            C1_THRESHOLD_P95_US,
            verdict(c1c2.c1_gate_passed)
        );
        println!(
            "{:<16} p95={}B ({} MB / {} MiB) / threshold {P8_THRESHOLD_BYTES}B [OPEN unit: \
             decimal MB] → {} (drifting rounds {}{})",
            "P8 (C2)",
            c1c2.c2_p95_bytes,
            contract::bytes_as_decimal_mb(c1c2.c2_p95_bytes),
            contract::bytes_as_mib(c1c2.c2_p95_bytes),
            verdict(c1c2.c2_gate_passed),
            c1c2.c2_drifting_rounds,
            if c1c2.c2_extended { ", extended" } else { "" }
        );
    }
    if let Some(p2) = p2 {
        println!(
            "{:<16} p99={}µs / threshold {}µs → {}",
            "P2 (C6)",
            p2.p99_us,
            C6_THRESHOLD_P99_US,
            verdict(p2.gate_passed)
        );
    }
    if let Some(p1) = p1 {
        println!(
            "{:<16} frame_build p50={}µs p99={}µs / budget {}µs @ {} → {}",
            "P1 margin (C6)",
            p1.frame_build_p50_us,
            p1.frame_build_p99_us,
            contract::C6_FRAME_BUDGET_120HZ_US,
            p1.refresh_hz
                .map(|hz| format!("{hz:.0}Hz"))
                .unwrap_or_else(|| "unknown".into()),
            p1.verdict
        );
    }
    println!(
        "\njson report: {} (bench ran {:.1}s)",
        path.display(),
        elapsed.as_secs_f32()
    );
    println!(
        "frozen status vocabulary: `{}` / `{}` (SDD §1)",
        STATUS_PERFORMANCE_GATE_FAILED, STATUS_HARDWARE_PENDING
    );
}

fn verdict(passed: bool) -> &'static str {
    report::status_word(passed)
}

fn workspace_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("failed to locate the workspace root from xtask's manifest dir")
}
