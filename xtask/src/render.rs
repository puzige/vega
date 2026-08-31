//! C6 P1 render margin: drives the existing production render probe
//! (`vega --vega-bench-render`, S3-T17 machinery, unchanged production code)
//! and re-frames its output with the frozen C6 semantics:
//!
//! - the refresh rate is DETECTED via CoreGraphics (`provenance::main_
//!   display_refresh_hz`), never hardcoded to 60 (the SDD §0 defect);
//! - on a 60 Hz host the run only proves the CPU/build margin and reports
//!   `hardware pending` for the literal 120 fps verdict (SDD §7, frozen);
//! - the probe's `stream` phase is NOT the P2 terminal evidence (it injects
//!   ~500 δ/s at the parser level); P2 truth lives in `bench_p2_with` — the
//!   margin run's stream numbers are diagnostics only.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::contract::{
    C6_ANY_SECOND_MIN_FPS, C6_FRAME_BUDGET_120HZ_US, C6_LITERAL_120FPS_MIN_HZ,
    STATUS_HARDWARE_PENDING,
};
use crate::provenance::{ReleaseBuild, main_display_refresh_hz};

/// One JSON margin record (frozen field names).
#[derive(Debug, Clone, Serialize)]
pub struct RenderMargin {
    pub schema: &'static str,
    /// Detected main-display refresh rate (Hz); `None` = detection failed.
    pub refresh_hz: Option<f64>,
    /// Frame-build p50/p99 µs over the whole scroll phase (CPU/build margin).
    pub frame_build_p50_us: f64,
    pub frame_build_p99_us: f64,
    /// Per-second fps histogram (frames are vsync-capped by the host display).
    pub fps_per_second: Vec<u64>,
    pub fps_median: u64,
    /// Any per-second window at or above the 100 fps floor (C6 literal-120fps
    /// criterion); `None` on hosts below 120 Hz where it is not judgeable.
    pub any_second_meets_fps_floor: Option<bool>,
    /// Frozen-region re-materializations during the stream phase (P3: 0).
    pub frozen_remat: u64,
    /// Rows in the synthetic document (~10k per C6).
    pub row_count: u64,
    /// Frozen status vocabulary (SDD §1) — 60 Hz hosts can never PASS the
    /// literal 120 fps here.
    pub verdict: &'static str,
    /// `hardware_pending` when refresh < 120 Hz or unknown; otherwise the
    /// literal verdict may be judged by T50 on real ProMotion hardware.
    pub literal_120fps: &'static str,
    /// Frame budget reference (8.33 ms @ 120 Hz).
    pub budget_us_120hz: u64,
}

pub const RENDER_MARGIN_SCHEMA: &str = "vega-c6-p1-margin.v1";

/// Upper bound for one probe run (~25 s expected).
const RENDER_FRAME_TIMEOUT: Duration = Duration::from_secs(120);
/// CPU sampling interval during the probe.
const CPU_SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

/// Runs `vega --vega-bench-render <tmp.json>` to completion and builds the
/// frozen margin record from its JSON + the detected refresh rate.
pub fn measure_render_margin(
    workspace: &Path,
    build: &ReleaseBuild,
    prov: &crate::provenance::Provenance,
) -> Result<RenderMargin> {
    let _ = workspace;
    let report_path = std::env::temp_dir().join(format!("vega-render-frame-{}.json", unix_ms()));
    let mut child = spawn_with_idle_assertion(&build.vega_bin, &report_path)
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
                "the --vega-bench-render probe exceeded {}s; killed (timeout is a FAIL, \
                 never counted as success)",
                RENDER_FRAME_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(CPU_SAMPLE_INTERVAL);
    };
    if !status.success() {
        bail!("the --vega-bench-render probe exited with {status}");
    }

    let raw = std::fs::read_to_string(&report_path)
        .with_context(|| format!("failed to read {}", report_path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).context("the probe report is not valid JSON")?;

    let fps_per_second: Vec<u64> = value["per_second"]
        .as_array()
        .map(|samples| {
            samples
                .iter()
                .filter_map(|sample| sample["fps"].as_u64())
                .collect()
        })
        .unwrap_or_default();
    let fps_median = median(&fps_per_second);
    let build_p50 = value["scroll"]["frame_build_p50_us"]
        .as_f64()
        .unwrap_or_default();
    let build_p99 = value["scroll"]["frame_build_p99_us"]
        .as_f64()
        .unwrap_or_default();
    let frozen_remat = value["frozen_rematerializations"]
        .as_u64()
        .unwrap_or(u64::MAX);
    let row_count = value["row_count"].as_u64().unwrap_or_default();

    // Real refresh-rate detection (replaces the hardcoded 60 Hz note).
    let refresh_hz = prov.display_refresh_hz.or_else(main_display_refresh_hz);
    let literal_capable = refresh_hz.is_some_and(|hz| hz >= C6_LITERAL_120FPS_MIN_HZ);
    let margin_ok =
        build_p50 > 0.0 && build_p50 < C6_FRAME_BUDGET_120HZ_US as f64 && frozen_remat == 0;
    // Frozen vocabulary: a 60 Hz host proves margin only and reports
    // `hardware pending` (SDD §7); it can never claim literal 120 fps.
    let verdict = if margin_ok {
        STATUS_HARDWARE_PENDING
    } else {
        crate::contract::STATUS_PERFORMANCE_GATE_FAILED
    };
    // C6 literal-120fps second-window floor (any second ≥ 100 fps): only
    // judgeable on a ≥120 Hz host; below it the vsync-capped histogram can
    // never meet the floor and the field records `null` (not a false fail).
    let any_second_meets_floor = if literal_capable {
        Some(
            fps_per_second
                .iter()
                .any(|fps| *fps >= C6_ANY_SECOND_MIN_FPS),
        )
    } else {
        None
    };

    Ok(RenderMargin {
        schema: RENDER_MARGIN_SCHEMA,
        refresh_hz,
        frame_build_p50_us: build_p50,
        frame_build_p99_us: build_p99,
        fps_per_second,
        fps_median,
        any_second_meets_fps_floor: any_second_meets_floor,
        frozen_remat,
        row_count,
        verdict,
        literal_120fps: if literal_capable {
            "eligible-for-literal-judgment (T50 hardware evidence still required)"
        } else {
            "hardware pending (host below 120 Hz; literal 120fps is T50)"
        },
        budget_us_120hz: C6_FRAME_BUDGET_120HZ_US,
    })
}

/// Spawns the probe binary, wrapped in `caffeinate -i` on macOS so App Nap
/// cannot throttle the measurement window (spike run.sh 前提).
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

/// Median of the per-second fps samples (robust against startup outliers).
fn median(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    match sorted.len() {
        0 => 0,
        even if even % 2 == 0 => sorted[even / 2 - 1].max(sorted[even / 2].saturating_sub(0)),
        len => sorted[len / 2],
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_handles_even_and_odd_lengths() {
        assert_eq!(median(&[]), 0);
        assert_eq!(median(&[5]), 5);
        assert_eq!(median(&[1, 2, 3]), 2);
        // Even length: upper-middle element (deterministic, no float).
        assert_eq!(median(&[1, 2, 3, 4]), 3);
        assert_eq!(median(&[10, 20]), 20);
    }

    #[test]
    fn margin_record_shape_stays_frozen() {
        let margin = RenderMargin {
            schema: RENDER_MARGIN_SCHEMA,
            refresh_hz: Some(60.0),
            frame_build_p50_us: 100.0,
            frame_build_p99_us: 300.0,
            fps_per_second: vec![60; 8],
            fps_median: 60,
            any_second_meets_fps_floor: None,
            frozen_remat: 0,
            row_count: 10_000,
            verdict: STATUS_HARDWARE_PENDING,
            literal_120fps: "hardware pending (host below 120 Hz; literal 120fps is T50)",
            budget_us_120hz: C6_FRAME_BUDGET_120HZ_US,
        };
        let json = serde_json::to_string(&margin).unwrap();
        assert!(json.contains(r#""refresh_hz":60.0"#));
        assert!(json.contains(r#""verdict":"hardware pending""#));
        // A 60 Hz margin record never contains a PASS-shaped verdict.
        assert!(!json.contains("PASS"));
    }
}
