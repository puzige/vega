//! Development tasks for the Vega workspace (bench, run, package).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::Serialize;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = dispatch(&args) {
        eprintln!("xtask error: {error:#}");
        std::process::exit(1);
    }
}

fn dispatch(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("bench") => bench(),
        other => {
            if let Some(other) = other {
                eprintln!("unknown subcommand: {other}");
            }
            eprintln!("usage: cargo xtask bench");
            std::process::exit(2);
        }
    }
}

fn bench() -> Result<()> {
    let workspace = workspace_root()?;
    let binary = ensure_vega_binary(&workspace)?;

    println!("vega bench — S3-T17 measurement pipeline");
    println!(
        "cold_start is still spawn-to-exit (first-frame instrumentation is a separate card); \
         render_frame runs the --vega-bench-render probe\n"
    );

    let cold_start = measure_cold_start(&binary)?;
    let memory_idle = measure_memory_idle(&binary)?;
    println!("building release vega for the render_frame probe (first run takes a while) ...");
    let release_binary = ensure_vega_binary_with_profile(&workspace, true)?;
    println!(
        "measuring render_frame via the --vega-bench-render probe (~25s, a window will open) ..."
    );
    let render_frame = measure_render_frame(&release_binary)?;

    print_table(&cold_start, &memory_idle, &render_frame);

    let report = BenchReport {
        timestamp: unix_ms()?,
        meta: BenchMeta {
            stage: "s3-t17",
            note: "render_frame measured by `vega --vega-bench-render` (probe-binary mode): \
                   #[gpui::test] was evaluated first but this gpui rev runs tests on \
                   NoopTextSystem with no real frame cadence, so the probe reuses the T14 \
                   spike method (render counter + 1s sampling + frame-build percentiles on a \
                   real window). fps is vsync-capped by the 60Hz display; judge P1 by the \
                   frame-build margin per tech-spec §5.2",
        },
        cold_start,
        memory_idle,
        render_frame,
    };
    let path = write_report(&workspace, &report)?;
    println!("\njson report: {}", path.display());
    Ok(())
}

// ─── benchmarks ──────────────────────────────────────────────────────────────

/// Spawning the GUI flashes one Vega window per round; the measurement itself
/// closes each instance right after the startup grace period.
const COLD_START_ROUNDS: u64 = 5;
const STARTUP_GRACE: Duration = Duration::from_secs(2);
const IDLE_SAMPLE_AFTER: Duration = Duration::from_secs(5);

fn measure_cold_start(binary: &Path) -> Result<ColdStart> {
    let mut rounds_ms = Vec::new();
    for round in 1..=COLD_START_ROUNDS {
        let start = Instant::now();
        let mut child = Command::new(binary)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn {} (round {round})", binary.display()))?;
        // TODO(S3): replace the fixed grace period with first-frame instrumentation.
        thread::sleep(STARTUP_GRACE);
        // An already-exited child fails kill(); that is fine, wait() reaps either way.
        child.kill().ok();
        child.wait()?;
        rounds_ms.push(start.elapsed().as_millis() as u64);
    }
    rounds_ms.sort_unstable();
    let (p50_ms, p99_ms) = (percentile(&rounds_ms, 50), percentile(&rounds_ms, 99));
    Ok(ColdStart {
        method: "spawn_to_exit",
        rounds_ms,
        p50_ms,
        p99_ms,
        todo: "measure to first frame once startup instrumentation exists (S3)",
    })
}

fn measure_memory_idle(binary: &Path) -> Result<MemoryIdle> {
    let mut child = Command::new(binary)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn vega for the idle-memory sample")?;
    thread::sleep(IDLE_SAMPLE_AFTER);
    let pid = child.id();
    let rss_bytes =
        rss_resident_bytes(pid).with_context(|| format!("failed to read RSS of vega pid {pid}"))?;
    child.kill().ok();
    child.wait()?;
    Ok(MemoryIdle {
        method: "proc_pidinfo(PROC_PIDTASKINFO) resident size",
        sample_after_secs: IDLE_SAMPLE_AFTER.as_secs(),
        rss_bytes,
        rss_mb: (rss_bytes as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0,
    })
}

/// Nearest-rank percentile over a sorted slice.
fn percentile(sorted: &[u64], pct: u64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (pct * sorted.len() as u64).div_ceil(100);
    let rank = (rank.max(1) as usize).min(sorted.len());
    sorted[rank - 1]
}

// ─── render_frame probe (S3-T17) ─────────────────────────────────────────────

/// Upper bound for one probe run (~25s expected: 8s scroll + 12s stream +
/// startup + report write).
const RENDER_FRAME_TIMEOUT: Duration = Duration::from_secs(120);
/// CPU sampling interval during the probe (spike run.sh 方法).
const CPU_SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

/// Runs `vega --vega-bench-render <tmp.json>` to completion and returns the
/// probe's measured JSON (the `render_frame` report value), enriched with the
/// externally sampled CPU usage (spike run.sh 方法：ps -o %cpu).
fn measure_render_frame(binary: &Path) -> Result<serde_json::Value> {
    let report_path = std::env::temp_dir().join(format!("vega-render-frame-{}.json", unix_ms()?));
    let mut child = spawn_with_idle_assertion(binary, &report_path)
        .context("failed to spawn the --vega-bench-render probe")?;

    let start = Instant::now();
    let mut cpu_samples: Vec<f64> = Vec::new();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if let Some(pct) = cpu_percent(child.id()) {
            cpu_samples.push(pct);
        }
        if start.elapsed() > RENDER_FRAME_TIMEOUT {
            child.kill().ok();
            bail!(
                "the --vega-bench-render probe exceeded {}s; killed",
                RENDER_FRAME_TIMEOUT.as_secs()
            );
        }
        thread::sleep(CPU_SAMPLE_INTERVAL);
    };
    if !status.success() {
        bail!("the --vega-bench-render probe exited with {status}");
    }

    let raw = std::fs::read_to_string(&report_path)
        .with_context(|| format!("failed to read {}", report_path.display()))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&raw).context("the probe report is not valid JSON")?;
    if !cpu_samples.is_empty() {
        let average = cpu_samples.iter().sum::<f64>() / cpu_samples.len() as f64;
        let max = cpu_samples.iter().cloned().fold(0.0_f64, f64::max);
        value["cpu_avg_pct"] = serde_json::json!((average * 10.0).round() / 10.0);
        value["cpu_max_pct"] = serde_json::json!((max * 10.0).round() / 10.0);
        value["cpu_samples"] = serde_json::json!(cpu_samples.len());
    }
    Ok(value)
}

/// Spawns the probe binary, wrapped in `caffeinate -i` on macOS so App Nap
/// cannot throttle the measurement window (spike run.sh 同款前提).
fn spawn_with_idle_assertion(binary: &Path, report_path: &Path) -> Result<std::process::Child> {
    let caffeinate = Path::new("/usr/bin/caffeinate");
    let mut command = if caffeinate.exists() {
        let mut command = Command::new(caffeinate);
        command.arg("-i").arg(binary);
        command
    } else {
        Command::new(binary)
    };
    command
        .arg("--vega-bench-render")
        .arg(report_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command.spawn()?)
}

/// One process-CPU sample in percent via `ps -o %cpu=` (None = unavailable).
fn cpu_percent(pid: u32) -> Option<f64> {
    let output = Command::new("ps")
        .arg("-o")
        .arg("%cpu=")
        .arg("-p")
        .arg(pid.to_string())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse::<f64>().ok()
}

// ─── vega process helpers ────────────────────────────────────────────────────

fn workspace_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("failed to locate the workspace root from xtask's manifest dir")
}

fn ensure_vega_binary(workspace: &Path) -> Result<PathBuf> {
    ensure_vega_binary_with_profile(workspace, false)
}

/// Locates (building when missing) the vega binary. `release = true` builds
/// `--release` — the render_frame probe measures on an optimized binary so the
/// numbers are comparable with the T14 spike (which ran a release probe).
fn ensure_vega_binary_with_profile(workspace: &Path, release: bool) -> Result<PathBuf> {
    let profile_dir = if release { "release" } else { "debug" };
    let binary = workspace.join(format!("target/{profile_dir}/vega"));
    if !binary.exists() {
        println!(
            "building vega (target/{profile_dir}/vega not found) ... (release builds take a while)"
        );
        let mut command = cargo_command();
        command.arg("build").arg("-p").arg("vega");
        if release {
            command.arg("--release");
        }
        if !command.status()?.success() {
            bail!("cargo build -p vega failed");
        }
    }
    if !binary.exists() {
        bail!("vega binary not found at {} after build", binary.display());
    }
    Ok(binary)
}

fn cargo_command() -> Command {
    // Prefer the rustup proxy so rust-toolchain.toml is honored even when
    // Homebrew's cargo shadows it in PATH (same fix as .githooks).
    let mut cargo = PathBuf::from("cargo");
    if let Ok(home) = std::env::var("HOME") {
        let proxy = Path::new(&home).join(".cargo/bin/cargo");
        if proxy.exists() {
            cargo = proxy;
        }
    }
    Command::new(cargo)
}

// ─── macOS RSS probe (libproc, zero third-party deps) ───────────────────────

const PROC_PIDTASKINFO: i32 = 4;

/// Mirrors Darwin's `struct proc_taskinfo` layout (libproc.h); only
/// `pti_resident_size` is read, the rest exists to keep the ABI faithful.
#[allow(dead_code)]
#[derive(Default)]
#[repr(C)]
struct ProcTaskInfo {
    pti_virtual_size: u64,
    pti_resident_size: u64,
    pti_total_user: u64,
    pti_total_system: u64,
    pti_threads_user: u64,
    pti_threads_system: u64,
    pti_policy: i32,
    pti_faults: i32,
    pti_pageins: i32,
    pti_cow_faults: i32,
    pti_messages_sent: i32,
    pti_messages_received: i32,
    pti_syscalls_mach: i32,
    pti_syscalls_bsd: i32,
    pti_csw: i32,
    pti_threadnum: i32,
    pti_numrunning: i32,
    pti_priority: i32,
}

// proc_pidinfo is re-exported by libSystem, no explicit link attribute needed.
unsafe extern "C" {
    fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut ProcTaskInfo, size: i32) -> i32;
}

fn rss_resident_bytes(pid: u32) -> Result<u64> {
    let mut info = ProcTaskInfo::default();
    let size = std::mem::size_of::<ProcTaskInfo>() as i32;
    let written = unsafe { proc_pidinfo(pid as i32, PROC_PIDTASKINFO, 0, &mut info, size) };
    if written < size {
        bail!("proc_pidinfo returned {written} bytes, expected {size} (pid may have exited)");
    }
    Ok(info.pti_resident_size)
}

// ─── output ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct BenchReport {
    timestamp: u64,
    meta: BenchMeta,
    cold_start: ColdStart,
    memory_idle: MemoryIdle,
    render_frame: serde_json::Value,
}

#[derive(Serialize)]
struct BenchMeta {
    stage: &'static str,
    note: &'static str,
}

#[derive(Serialize)]
struct ColdStart {
    method: &'static str,
    rounds_ms: Vec<u64>,
    p50_ms: u64,
    p99_ms: u64,
    todo: &'static str,
}

#[derive(Serialize)]
struct MemoryIdle {
    method: &'static str,
    sample_after_secs: u64,
    rss_bytes: u64,
    rss_mb: f64,
}

fn print_table(cold_start: &ColdStart, memory_idle: &MemoryIdle, render_frame: &serde_json::Value) {
    println!("{:<14} {:<26} note", "metric", "value");
    println!(
        "{:<14} {:<26} spawn-to-exit placeholder",
        "cold_start",
        format!("p50={}ms p99={}ms", cold_start.p50_ms, cold_start.p99_ms)
    );
    println!(
        "{:<14} {:<26} RSS after {} s",
        "memory_idle",
        format!("{:.1} MB", memory_idle.rss_mb),
        memory_idle.sample_after_secs
    );
    let fps = render_frame
        .get("scroll")
        .and_then(|scroll| scroll.get("fps_median"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let stream_fps = render_frame
        .get("stream")
        .and_then(|stream| stream.get("fps_median"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let build_p50 = render_frame
        .get("stream")
        .and_then(|stream| stream.get("frame_build_p50_us"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let build_p99 = render_frame
        .get("stream")
        .and_then(|stream| stream.get("frame_build_p99_us"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let frozen_remat = render_frame
        .get("frozen_rematerializations")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    let mode = render_frame
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    println!(
        "{:<14} {:<26} mode={mode} (scroll fps; 60Hz vsync cap)",
        "render_frame",
        format!("fps={fps}")
    );
    println!(
        "{:<14} {:<26} stream ~500δ/s: frozen_remat={frozen_remat} (P3, 0 required)",
        "stream_phase",
        format!("fps={stream_fps} build p50={build_p50:.0}µs p99={build_p99:.0}µs")
    );
}

fn write_report(workspace: &Path, report: &BenchReport) -> Result<PathBuf> {
    let dir = workspace.join("bench/results");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = dir.join(format!("{}.json", report.timestamp));
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn unix_ms() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64)
}
