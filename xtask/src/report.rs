//! The frozen JSON report schema (repo-external, SDD §4/C3 provenance +
//! raw samples). Field names/units/percentile math are frozen at the T43
//! baseline; T48/T49 only consume this shape.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::contract::{STATUS_HARDWARE_PENDING, STATUS_PERFORMANCE_GATE_FAILED};
use crate::provenance::{Cutoff, Provenance, sha256_hex};

/// C1+C2 aggregate result (both gates share the same spawn rounds per SDD:
/// each round is one fresh process serving the C1 milestone and the three
/// C2 RSS samples).
#[derive(Debug, Clone, Serialize)]
pub struct C1C2Result {
    /// Every raw round (C1 latency + 3 raw-byte RSS samples + exit/fail).
    pub rounds: Vec<serde_json::Value>,
    /// C1 parent wall-clock samples (integer µs, ascending).
    pub c1_samples_us: Vec<u64>,
    /// C1 summary percentiles in integer µs (SDD §2 step 4: p50/p95/p99/max).
    pub c1_p50_us: u64,
    pub c1_p95_us: u64,
    pub c1_p99_us: u64,
    pub c1_max_us: u64,
    pub c1_gate_passed: bool,
    /// C2 per-process medians (raw bytes, ascending).
    pub c2_medians_bytes: Vec<u64>,
    pub c2_p95_bytes: u64,
    pub c2_gate_passed: bool,
    pub c2_drifting_rounds: usize,
    pub c2_extended: bool,
    /// FAIL-condition records (empty on a fully passing run).
    pub fail_conditions: Vec<String>,
    /// Where the temp sandbox lived (raw logs; repo-external).
    pub sandbox_root: String,
}

impl C1C2Result {
    /// Empty placeholder for the P2-only report variant.
    pub fn empty() -> Self {
        Self {
            rounds: Vec::new(),
            c1_samples_us: Vec::new(),
            c1_p50_us: 0,
            c1_p95_us: 0,
            c1_p99_us: 0,
            c1_max_us: 0,
            c1_gate_passed: false,
            c2_medians_bytes: Vec::new(),
            c2_p95_bytes: 0,
            c2_gate_passed: false,
            c2_drifting_rounds: 0,
            c2_extended: false,
            fail_conditions: Vec::new(),
            sandbox_root: String::new(),
        }
    }

    pub fn has_data(&self) -> bool {
        !self.rounds.is_empty()
    }
}

/// C6 P2 stream result (from the probe's own report).
#[derive(Debug, Clone, Serialize)]
pub struct P2Result {
    pub seconds: u64,
    pub rate_per_s: u64,
    pub events_total: u64,
    pub deltas_total: u64,
    pub frames: u64,
    pub queue_max_depth: u64,
    /// All batch latencies (integer µs; frozen unit).
    pub batch_latencies_us: Vec<u64>,
    pub p50_us: u64,
    pub p99_us: u64,
    pub gate_passed: bool,
    pub schema: String,
    pub sandbox_root: String,
}

/// The full report written outside the repo (C3: 产物不入库).
#[derive(Serialize)]
pub struct BenchReport {
    pub schema: &'static str,
    pub timestamp_unix_ms: u64,
    pub cutoff: Cutoff,
    pub provenance: Provenance,
    pub threshold_p8_bytes: u64,
    pub threshold_p8_note: &'static str,
    pub c1c2: C1C2Result,
    pub p2: Option<P2Result>,
    pub p1_margin: Option<crate::render::RenderMargin>,
}

impl BenchReport {
    pub fn new(provenance: &Provenance, c1c2: C1C2Result, cutoff: Cutoff) -> Self {
        Self {
            schema: "vega-s8-t43.baseline.v1",
            timestamp_unix_ms: unix_ms(),
            cutoff,
            provenance: provenance.clone(),
            threshold_p8_bytes: crate::contract::P8_THRESHOLD_BYTES,
            threshold_p8_note: "OPEN(OWNER: human) unit ruling — pre-ruling literal authority \
                                decimal MB 100,000,000 bytes; ruling frozen before baseline \
                                freeze if changed (SDD §3.1/§10); MB/MiB display fields are \
                                conversions of the stored raw bytes",
            c1c2,
            p2: None,
            p1_margin: None,
        }
    }
}

/// Writes the report JSON to a repo-external temp path and returns its path
/// + SHA-256 (C3: 结果文件 SHA-256; raw products never enter the repo).
pub fn write(report: &BenchReport) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("vega-t43-reports");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let json = serde_json::to_string_pretty(report)?;
    let path = dir.join(format!("{}-baseline.json", report.timestamp_unix_ms));
    std::fs::write(&path, &json).with_context(|| format!("failed to write {}", path.display()))?;
    println!("report sha256: {}", sha256_hex(json.as_bytes()));
    println!("raw report (repo-external): {}", path.display());
    Ok(path)
}

/// Status-vocabulary reminder used by callers when printing verdicts.
pub fn status_word(passed: bool) -> &'static str {
    if passed {
        // Only the P-gates get PASS-shaped words; every non-pass state uses
        // the frozen vocabulary verbatim (never "almost passed").
        "performance gate passed (P7/P8/P2 frozen math)"
    } else {
        STATUS_PERFORMANCE_GATE_FAILED
    }
}

#[allow(unused)]
fn _vocabulary_reference() -> &'static str {
    STATUS_HARDWARE_PENDING
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
    fn status_words_are_frozen_vocabulary() {
        assert_eq!(status_word(false), "performance gate failed");
        assert!(status_word(true).starts_with("performance gate passed"));
        assert_eq!(STATUS_HARDWARE_PENDING, "hardware pending");
    }

    #[test]
    fn empty_c1c2_is_the_p2_only_placeholder() {
        let empty = C1C2Result::empty();
        assert!(!empty.has_data());
        assert_eq!(empty.c1_p95_us, 0);
    }

    #[test]
    fn report_serializes_with_frozen_schema_tag() {
        let provenance = Provenance {
            git_head: "0".repeat(40),
            git_dirty: false,
            profile: "release",
            binary_path: "/tmp/vega".into(),
            binary_size_bytes: 1,
            binary_mtime_unix_s: 0,
            binary_sha256: "0".repeat(64),
            xtask_binary_path: "/tmp/xtask".into(),
            xtask_binary_sha256: "1".repeat(64),
            build_command: "cargo build --release -p xtask -p vega".into(),
            build_exit_code: 0,
            rustc_version: "rustc 1.0".into(),
            os_version: "26.0".into(),
            cpu_model: "Apple Silicon".into(),
            gpu_model: "GPU".into(),
            display_refresh_hz: Some(60.0),
            machine: "arm64".into(),
        };
        let report = BenchReport::new(
            &provenance,
            C1C2Result::empty(),
            Cutoff {
                utc_unix_s: 0,
                utc_rfc3339: "1970-01-01T00:00:00Z".into(),
                local_rfc3339: "1970-01-01T00:00:00Z".into(),
            },
        );
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains(r#""schema":"vega-s8-t43.baseline.v1""#));
        assert!(json.contains(r#""threshold_p8_bytes":100000000"#));
        // The stale-target defect fields are gone forever.
        assert!(!json.contains("rss_mb"));
        assert!(!json.contains("spawn_to_exit"));
    }
}
