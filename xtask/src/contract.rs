//! S8-T43 (A2-04) frozen measurement contracts — SDD C1/C2/C6 + §1 status
//! vocabulary. T48/T49/T50 only consume these; the names, units, round
//! counts, percentile math, and thresholds below are frozen and MUST NOT
//! change after the baseline freeze (SDD contract preamble).

use serde::{Deserialize, Serialize};
use std::time::Duration;

// ─── C1 — P7 first rendered interactive (SDD §2) ─────────────────────────────

/// Frozen metric name (SDD §2: "写入 JSON schema，永不更名").
pub const PROCESS_START_TO_FIRST_RENDERED_INTERACTIVE: &str =
    "process_start_to_first_rendered_interactive";

/// C1: 20 fresh processes, nearest-rank p95 < 50.000 ms.
pub const C1_ROUNDS: usize = 20;
/// C1 gate: p95 threshold, `50.000 ms` exactly (integer-µs samples).
pub const C1_THRESHOLD_P95_US: u64 = 50_000;
/// C1: hard per-process timeout; a process that has neither flushed the
/// milestone nor exited by then FAILs (a milestone after this window is not
/// a first frame). Kill happens only here and always classifies FAIL.
pub const C1_PROCESS_TIMEOUT: Duration = Duration::from_secs(15);
/// Slack granted to the child's self-timed hold window before the parent
/// kills it (the child exits at milestone + C2_HOLD + this slack).
pub const C1_EXIT_SLACK: Duration = Duration::from_secs(10);

/// The single-line strict-JSON milestone the isolated subprocess flushes
/// from a pinned GPUI next-frame callback (SDD §2 protocol step 3). Exactly
/// this prefix + JSON and nothing else may appear on the child's stdout.
pub const MILESTONE_PREFIX: &str = "VEGA_C1_MILESTONE ";

/// Child attests its isolation so the parent can mechanically reject a probe
/// that escaped the temp HOME (real profile access) or that would claim a
/// milestone it did not earn from a real next-frame callback.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IsolationAttestation {
    /// The HOME the child actually ran with (must equal the parent's temp).
    pub home: String,
    /// The vega data root the child resolved (must be inside `home`).
    pub data_root: String,
    /// Must be `"none"` — the probe constructs no provider.
    pub provider: String,
    /// Must be `"none"` — the probe makes no network requests.
    pub network: String,
    /// Must be `"not-exercised"` — no Keychain call in the probe path.
    pub keychain: String,
    /// Must be `"gpui_next_frame_callback"` — the milestone fired from a
    /// real pinned GPUI next-frame callback, never from a fixed sleep.
    pub first_frame_source: String,
}

/// Serde shape of the milestone line (after [`MILESTONE_PREFIX`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Milestone {
    /// Must be `"vega-c1"`; rejects foreign JSON on the pipe.
    pub schema: String,
    /// Must be [`PROCESS_START_TO_FIRST_RENDERED_INTERACTIVE`].
    pub metric: String,
    /// The child's pid; the parent cross-checks against the spawned pid.
    pub pid: u32,
    /// Child monotonic µs from its own process start to the next-frame
    /// callback (diagnostic; the parent measures wall-clock independently).
    pub elapsed_us: u64,
    /// Isolation attestation (validated against the parent's temp HOME).
    pub isolation: IsolationAttestation,
}

// ─── C2 — P8 release RSS (SDD §3) ────────────────────────────────────────────

/// C2: 20 fresh release processes.
pub const C2_ROUNDS: usize = 20;
/// C2: per-process samples at +5/+10/+15 s after the C1 sample point.
pub const C2_SAMPLE_OFFSETS: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(15),
];
/// The child holds idle (empty single window, no tasks) for this long after
/// the milestone so the +15 s sample lands, then exits on its own.
pub const C2_HOLD_MS: u64 = 16_000;
/// C2 stability: more than one round with `+15s − +5s > 2 MiB` → `unstable`,
/// gate fails, attribution required (SDD §3 protocol step 4).
pub const C2_STABILITY_GUARD_BYTES: u64 = 2 * 1024 * 1024;
pub const C2_STABILITY_MAX_DRIFTING_ROUNDS: usize = 1;
/// C2 gray zone: p95 in `[98,000,000, 102,000,000)` → extend +20 rounds,
/// merge 40 samples, recompute p95 with the same math (SDD §3 step 5).
pub const C2_GRAY_ZONE: std::ops::Range<u64> = 98_000_000..102_000_000;
pub const C2_EXTENSION_ROUNDS: usize = 20;

/// P8 threshold literal authority while the unit ruling is
/// OPEN(OWNER: human) (SDD §3.1/§10): candidate A `<100,000,000` bytes
/// (decimal MB). If the human ruling lands as candidate B (`104,857,600`
/// bytes = 100 MiB) it must be frozen into this constant via a docs erratum
/// BEFORE the baseline freeze; after the freeze the unit never changes.
/// The stored measurement is always raw bytes; MB/MiB are display-only.
pub const P8_THRESHOLD_BYTES: u64 = 100_000_000;

// ─── C6 — P1/P2 (SDD §7) ─────────────────────────────────────────────────────

/// C6 daily-PR feedback run: 10 s @ 1,000 deltas/s (the 5-minute release
/// soak stays with T48/T49; same schema, not terminal evidence).
pub const C6_STREAM_SECONDS: u64 = 10;
pub const C6_INJECT_RATE_PER_S: u64 = 1_000;
/// C6 P2 gate: p99 receive-to-render < 16.000 ms (one 60 Hz frame budget).
pub const C6_THRESHOLD_P99_US: u64 = 16_000;
/// Production agent-event pump cadence mirrored by the P2 probe
/// (`AGENT_EVENT_POLL` in the app entry, `429cb2d`).
pub const C6_POLL: Duration = Duration::from_millis(4);
/// Production bounded-batch drain limit mirrored by the P2 probe
/// (`AGENT_EVENT_BATCH` in the app entry, `429cb2d`).
pub const C6_BATCH_LIMIT: usize = 128;
/// C6 P1 frame budget for the margin report (8.33 ms @ 120 Hz; 60 Hz hosts
/// only prove CPU/build margin — `hardware pending` per SDD §7).
pub const C6_FRAME_BUDGET_120HZ_US: u64 = 8_333;
/// C6 P1 literal-120fps second window floor (any-second ≥ 100 fps).
pub const C6_ANY_SECOND_MIN_FPS: u64 = 100;
/// C6 P2 report schema tag.
pub const C6_STREAM_SCHEMA: &str = "vega-c6-stream.v1";
/// Parent watchdog for the P2 probe child (10 s stream + boot headroom):
/// a child that has not exited by then is killed and the run is a FAIL.
pub const P2_WATCHDOG: Duration = Duration::from_secs(180);
/// Refresh rate at or above which a host may judge literal 120 fps
/// (below it: margin only + `hardware pending`; SDD §7, frozen).
pub const C6_LITERAL_120FPS_MIN_HZ: f64 = 120.0;

// ─── status vocabulary (SDD §1, normative; never paraphrase, never漂白) ─────

pub const STATUS_PERFORMANCE_GATE_FAILED: &str = "performance gate failed";
pub const STATUS_HARDWARE_PENDING: &str = "hardware pending";
/// The only §1 pass-shaped word allowed in the bench context: the xtask
/// short runs measure a deterministic mock/temp-home fixture, so a passing
/// gate is a fixture pass — never terminal evidence (T48/T49 own the soak).
pub const STATUS_ENGINEERING_FIXTURE_PASSED: &str = "engineering fixture passed";

// ─── frozen math ─────────────────────────────────────────────────────────────

/// Nearest-rank percentile over an ascending-sorted sample list, integer µs
/// domain: `rank = ceil(pct/100 × n)` clamped to `[1, n]`, empty → 0. This
/// exact function is the frozen percentile math (SDD §7 机械判定); the unit
/// tests pin its behavior so T48 cannot quietly redefine it.
pub fn percentile(sorted: &[u64], pct: u64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (pct * sorted.len() as u64).div_ceil(100);
    let rank = rank.clamp(1, sorted.len() as u64) as usize;
    sorted[rank - 1]
}

/// Per-process median of the three C2 samples (+5/+10/+15 s): the middle
/// value of the sorted triple (SDD §3 protocol step 3).
pub fn median_of_three(mut samples: [u64; 3]) -> u64 {
    samples.sort_unstable();
    samples[1]
}

/// The C2 stability guard on one round: true when `+15s − +5s > 2 MiB`
/// (shrinkage is not drift; saturating subtraction).
pub fn round_drifts(sample5s: u64, sample15s: u64) -> bool {
    sample15s.saturating_sub(sample5s) > C2_STABILITY_GUARD_BYTES
}

/// C2 gray-zone check (SDD §3 step 5).
pub fn in_gray_zone(p95_bytes: u64) -> bool {
    C2_GRAY_ZONE.contains(&p95_bytes)
}

/// Display-only MB/MiB conversions (rounded to 3 decimals) — the stored
/// number is always raw bytes; these exist so every report can show both
/// rulings side by side while the human unit decision is OPEN
/// (SDD §3: 显示换算与入库数值分离). Decimal MB = bytes / 10^6 (the frozen
/// threshold 100,000,000 bytes reads as exactly 100.000 MB); MiB = bytes / 2^20.
pub fn bytes_as_decimal_mb(bytes: u64) -> f64 {
    (bytes as f64 / 1_000_000.0 * 1000.0).round() / 1000.0
}

pub fn bytes_as_mib(bytes: u64) -> f64 {
    (bytes as f64 / (1024.0 * 1024.0) * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut values: Vec<u64>) -> Vec<u64> {
        values.sort_unstable();
        values
    }

    #[test]
    fn percentile_nearest_rank_pinned() {
        // ceil(95/100 × 20) = 19 → the 19th value (1-based).
        let samples = sorted((1..=20).collect());
        assert_eq!(percentile(&samples, 95), 19);
        // ceil(50/100 × 20) = 10 → the 10th value.
        assert_eq!(percentile(&samples, 50), 10);
        // ceil(99/100 × 20) = 20 → max.
        assert_eq!(percentile(&samples, 99), 20);
        // ceil(95/100 × 1) = 1 → the only value.
        assert_eq!(percentile(&[42], 95), 42);
        assert_eq!(percentile(&[], 95), 0);
        // 40 merged rounds: ceil(95/100 × 40) = 38.
        let samples = sorted((1..=40).collect());
        assert_eq!(percentile(&samples, 95), 38);
    }

    #[test]
    fn c1_threshold_is_fifty_thousand_us() {
        // Two slow rounds push the 19th of 20 sorted samples past the gate:
        // ceil(95/100 × 20) = 19 → the 19th value = 51_000 → gate fails.
        let mut samples: Vec<u64> = vec![40_000; 18];
        samples.push(51_000);
        samples.push(51_000);
        assert!(percentile(&samples, 95) > C1_THRESHOLD_P95_US);
        // A single slow round (rank 20) never fails the p95 gate: the 19th
        // value stays 49_999 < 50_000.
        let mut samples: Vec<u64> = vec![40_000; 18];
        samples.push(49_999);
        samples.push(90_000);
        let samples = sorted(samples);
        assert!(percentile(&samples, 95) < C1_THRESHOLD_P95_US);
    }

    #[test]
    fn median_of_three_is_middle() {
        assert_eq!(median_of_three([1, 2, 3]), 2);
        assert_eq!(median_of_three([3, 1, 2]), 2);
        assert_eq!(median_of_three([9, 9, 9]), 9);
        assert_eq!(median_of_three([100, 1, 2]), 2);
    }

    #[test]
    fn stability_guard_two_mib_boundary() {
        assert!(!round_drifts(100_000_000, 100_000_000 + 2 * 1024 * 1024));
        assert!(round_drifts(100_000_000, 100_000_000 + 2 * 1024 * 1024 + 1));
        // Shrinking RSS never drifts (saturating subtraction).
        assert!(!round_drifts(100_000_000, 90_000_000));
    }

    #[test]
    fn gray_zone_bounds() {
        assert!(!in_gray_zone(97_999_999));
        assert!(in_gray_zone(98_000_000));
        assert!(in_gray_zone(101_999_999));
        assert!(!in_gray_zone(102_000_000));
    }

    #[test]
    fn display_conversions_are_display_only() {
        // 100,000,000 bytes = exactly 100.000 decimal MB = 95.367 MiB.
        assert_eq!(bytes_as_decimal_mb(100_000_000), 100.0);
        assert_eq!(bytes_as_mib(100_000_000), 95.367);
        // 104,857,600 bytes = 104.858 decimal MB = exactly 100.000 MiB.
        assert_eq!(bytes_as_mib(104_857_600), 100.0);
        assert_eq!(bytes_as_decimal_mb(104_857_600), 104.858);
    }

    #[test]
    fn milestone_shape_round_trips() {
        let milestone = Milestone {
            schema: "vega-c1".into(),
            metric: PROCESS_START_TO_FIRST_RENDERED_INTERACTIVE.into(),
            pid: 4242,
            elapsed_us: 12_345,
            isolation: IsolationAttestation {
                home: "/tmp/x".into(),
                data_root: "/tmp/x/dr".into(),
                provider: "none".into(),
                network: "none".into(),
                keychain: "not-exercised".into(),
                first_frame_source: "gpui_next_frame_callback".into(),
            },
        };
        let line = format!(
            "{}{}",
            MILESTONE_PREFIX,
            serde_json::to_string(&milestone).unwrap()
        );
        assert!(line.starts_with(r#"VEGA_C1_MILESTONE {"#));
        let parsed: Milestone =
            serde_json::from_str(line.strip_prefix(MILESTONE_PREFIX).unwrap()).unwrap();
        assert_eq!(parsed, milestone);
    }
}
