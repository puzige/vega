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

    println!("vega bench — placeholder measurement pipeline (E4)");
    println!("real instrumentation lands with S3 (first frame / gpui::test frame timing)\n");

    let cold_start = measure_cold_start(&binary)?;
    let memory_idle = measure_memory_idle(&binary)?;
    let render_frame = serde_json::json!({ "status": "not_implemented" });

    print_table(&cold_start, &memory_idle);

    let report = BenchReport {
        timestamp: unix_ms()?,
        meta: BenchMeta {
            stage: "placeholder",
            note: "cold_start measures spawn-to-exit until first-frame instrumentation (S3); \
                   render_frame waits for #[gpui::test] frame timing (S3)",
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

// ─── vega process helpers ────────────────────────────────────────────────────

fn workspace_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("failed to locate the workspace root from xtask's manifest dir")
}

fn ensure_vega_binary(workspace: &Path) -> Result<PathBuf> {
    let binary = workspace.join("target/debug/vega");
    if !binary.exists() {
        println!("building vega (target/debug/vega not found) ...");
        let status = cargo_command()
            .arg("build")
            .arg("-p")
            .arg("vega")
            .status()?;
        if !status.success() {
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

fn print_table(cold_start: &ColdStart, memory_idle: &MemoryIdle) {
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
    println!(
        "{:<14} {:<26} S3: #[gpui::test] frame timing",
        "render_frame", "not implemented"
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
