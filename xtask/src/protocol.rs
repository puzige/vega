//! C1/C2 parent-side protocol: spawn the isolated release probe, classify
//! the nine frozen FAIL conditions (SDD §2 判失败), take the three C2
//! raw-byte RSS samples at +5/+10/+15 s from the C1 sample point, and keep
//! bounded raw evidence.
//!
//! The protocol never sleeps a fixed period in place of the milestone and
//! never treats a kill as success: `kill` happens only after the timeout and
//! always classifies the round as FAILED.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::contract::{
    self, C1_EXIT_SLACK, C1_PROCESS_TIMEOUT, C1_ROUNDS, C2_HOLD_MS, C2_ROUNDS, C2_SAMPLE_OFFSETS,
    MILESTONE_PREFIX, Milestone,
};
use crate::provenance::rss_resident_bytes;

/// The nine frozen C1 FAIL conditions (SDD §2 判失败). The enum variants are
/// the canonical machine-readable reasons; nothing else may fail a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum C1Fail {
    /// No milestone line arrived before the process exited / timed out.
    MilestoneMissing,
    /// More than one milestone-shaped line appeared.
    MilestoneDuplicated,
    /// The line is not strict `VEGA_C1_MILESTONE {…}` single-line JSON.
    MilestoneMalformed,
    /// Process exited (any code) without delivering a valid milestone.
    EarlyExit,
    /// No milestone within [`C1_PROCESS_TIMEOUT`]; killed (recorded FAIL).
    Timeout,
    /// The probe's attestation shows a data root outside the temp HOME.
    RealProfileAccess,
    /// The probe's attestation is missing/incorrect for provider/network.
    ProviderNetworkAccess,
    /// The probe did not attest a real GPUI next-frame callback source.
    SleepAsFirstFrame,
    /// Reserved invariant: the runner kills only on timeout and only marks
    /// the round FAILED, so this can only surface if the kill path is ever
    /// misused (defense in depth; a passing round may never involve a kill).
    KillAsSuccess,
}

impl std::fmt::Display for C1Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            C1Fail::MilestoneMissing => "milestone_missing",
            C1Fail::MilestoneDuplicated => "milestone_duplicated",
            C1Fail::MilestoneMalformed => "milestone_malformed",
            C1Fail::EarlyExit => "early_exit",
            C1Fail::Timeout => "timeout",
            C1Fail::RealProfileAccess => "real_profile_access",
            C1Fail::ProviderNetworkAccess => "provider_network_access",
            C1Fail::SleepAsFirstFrame => "sleep_as_first_frame",
            C1Fail::KillAsSuccess => "kill_as_success",
        })
    }
}

/// How one round's child process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitKind {
    /// Exited on its own with status success.
    CleanExit,
    /// Exited on its own with a failing status.
    FailedExit,
    /// Killed by the runner after the C1 timeout (always FAIL).
    TimeoutKill,
}

/// Raw evidence for one spawn round (C1 latency + the three C2 RSS samples).
#[derive(Debug, Serialize)]
pub struct Round {
    pub round: usize,
    /// Parent wall-clock µs from the pre-spawn `Instant` to the milestone
    /// line's arrival on the pipe (integer µs per SDD §2). The authoritative
    /// C1 sample.
    pub parent_latency_us: Option<u64>,
    /// Child-reported monotonic µs (diagnostic cross-check only).
    pub child_elapsed_us: Option<u64>,
    /// C2 raw bytes at +5/+10/+15 s from the C1 sample point.
    pub rss_bytes: [Option<u64>; 3],
    pub exit_kind: ExitKind,
    pub exit_code: Option<i32>,
    pub fail: Option<C1Fail>,
    /// Full stdout (bounded, diagnostics only — a passing round's stdout is
    /// exactly the one milestone line).
    pub stdout_lines: Vec<String>,
}

impl Round {
    /// A round passes only when: a valid attested milestone arrived exactly
    /// once, all three C2 samples were taken, and the child exited cleanly
    /// on its own (no kill, no timeout).
    pub fn passed(&self) -> bool {
        self.fail.is_none()
            && self.parent_latency_us.is_some()
            && self.rss_bytes.iter().all(Option::is_some)
            && self.exit_kind == ExitKind::CleanExit
    }

    /// Per-process C2 median (middle of the +5/+10/+15 s triple).
    pub fn rss_median(&self) -> Option<u64> {
        let [a, b, c] = self.rss_bytes;
        Some(contract::median_of_three([a?, b?, c?]))
    }

    /// `+15s − +5s` pair for the stability-guard comparison this round
    /// (`None` when any sample is missing). Shrinkage is not drift; the
    /// frozen `round_drifts` comparison is saturating.
    pub fn rss_drift(&self) -> Option<(u64, u64)> {
        Some((self.rss_bytes[0]?, self.rss_bytes[2]?))
    }
}

/// Isolation environment handed to every probe child (C3: temp HOME, no
/// XDG overrides leaking in, piped stdout, quiet stderr).
pub struct Isolation {
    pub home: PathBuf,
}

impl Isolation {
    pub fn command(&self, probe: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(probe);
        command
            .args(args)
            .env("HOME", &self.home)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null());
        command
    }
}

/// Runs the full C1+C2 protocol (`rounds` spawn rounds, frozen counts at the
/// call sites) and returns the raw rounds.
pub fn run_c1_c2(probe: &Path, isolation: &Isolation, rounds: usize) -> Result<Vec<Round>> {
    let mut out = Vec::with_capacity(rounds);
    for round in 1..=rounds {
        let result = spawn_round(round, probe, isolation)?;
        println!(
            "  round {round:>2}/{rounds}: c1={} c2_median={} drift={} exit={:?}{}",
            result
                .parent_latency_us
                .map(|us| format!("{us}µs"))
                .unwrap_or_else(|| "—".into()),
            result
                .rss_median()
                .map(|b| format!("{b}B"))
                .unwrap_or_else(|| "—".into()),
            result
                .rss_drift()
                .map(|(sample5s, sample15s)| { format!("{}B", sample15s.saturating_sub(sample5s)) })
                .unwrap_or_else(|| "—".into()),
            result.exit_kind,
            result
                .fail
                .map(|fail| format!(" FAIL={fail}"))
                .unwrap_or_default(),
        );
        out.push(result);
    }
    Ok(out)
}

fn spawn_round(round: usize, probe: &Path, isolation: &Isolation) -> Result<Round> {
    let hold = C2_HOLD_MS.to_string();
    // C1 protocol step 1 (SDD §2): the parent timestamp is taken BEFORE the
    // spawn of the release binary — never after.
    let spawn_instant = Instant::now();
    let mut child = isolation
        .command(probe, &["__probe", "c1", "--hold-ms", &hold])
        .spawn()
        .with_context(|| format!("failed to spawn the c1 probe (round {round})"))?;
    let stdout = child
        .stdout
        .take()
        .context("probe stdout was not piped (protocol bug)")?;

    // Reader thread: every stdout line arrives with its receipt Instant so
    // the parent latency is the real pipe arrival, never a post-hoc estimate.
    let (line_tx, line_rx) = mpsc::channel::<(Instant, String)>();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line_tx.send((Instant::now(), line)).is_err() {
                break;
            }
        }
    });

    let mut round = Round {
        round,
        parent_latency_us: None,
        child_elapsed_us: None,
        rss_bytes: [None; 3],
        exit_kind: ExitKind::FailedExit,
        exit_code: None,
        fail: None,
        stdout_lines: Vec::new(),
    };

    // ── C1: wait for exactly one valid milestone (or timeout / early exit) ─
    let mut milestone_at: Option<Instant> = None;
    match line_rx.recv_timeout(C1_PROCESS_TIMEOUT) {
        Ok((at, line)) => {
            round.stdout_lines.push(line);
            match parse_milestone(&round.stdout_lines[0]) {
                // The milestone must name THIS round's child pid; a foreign
                // pid is a malformed/replayed milestone.
                Ok(parsed) if parsed.pid != child.id() => {
                    round.fail = Some(C1Fail::MilestoneMalformed);
                }
                Ok(parsed) => match validate_attestation(&parsed, isolation) {
                    Ok(()) => {
                        milestone_at = Some(at);
                        round.parent_latency_us = Some(us(spawn_instant.elapsed()));
                        round.child_elapsed_us = Some(parsed.elapsed_us);
                        // Proceed to C2 sampling; duplicate detection
                        // happens on the post-exit drain.
                    }
                    Err(fail) => {
                        round.fail = Some(fail);
                    }
                },
                Err(fail) => {
                    round.fail = Some(fail);
                }
            }
        }
        Err(RecvTimeoutError::Timeout) => {
            round.fail = Some(C1Fail::Timeout);
        }
        Err(RecvTimeoutError::Disconnected) => {}
    }

    // ── C2: three raw-byte samples at +5/+10/+15 s from the C1 point ──────
    if let (Some(at), None) = (milestone_at, round.fail.as_ref()) {
        for (slot, offset) in C2_SAMPLE_OFFSETS.iter().enumerate() {
            let due = at + *offset;
            let now = Instant::now();
            if due > now {
                thread::sleep(due - now);
            }
            match rss_resident_bytes(child.id()) {
                Ok(bytes) => round.rss_bytes[slot] = Some(bytes),
                Err(_) => {
                    // The child vanished before a sample — early exit.
                    round.fail = Some(C1Fail::EarlyExit);
                    break;
                }
            }
        }
    }

    // ── Reap: wait for the child's own exit; kill only on the hard limit ──
    let hard_deadline = Instant::now() + C1_PROCESS_TIMEOUT + C1_EXIT_SLACK;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                round.exit_code = status.code();
                round.exit_kind = if status.success() {
                    ExitKind::CleanExit
                } else {
                    ExitKind::FailedExit
                };
                break;
            }
            Ok(None) => {
                if Instant::now() >= hard_deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    round.exit_kind = ExitKind::TimeoutKill;
                    if round.fail.is_none() {
                        // SDD §2 kill-当-成功: everything else looked fine,
                        // so the kill itself is the FAIL reason (a kill may
                        // never pass a round; `Round::passed` also rejects
                        // `TimeoutKill` exits independently).
                        round.fail = Some(C1Fail::KillAsSuccess);
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                let _ = reader.join();
                return Err(error).context(format!(
                    "failed to reap the c1 probe (round {})",
                    round.round
                ));
            }
        }
    }
    // Drain any remaining stdout (post-milestone noise fails the round).
    while let Ok((_, line)) = line_rx.try_recv() {
        round.stdout_lines.push(line);
    }
    let _ = reader.join();
    if round.fail.is_none() && round.stdout_lines.len() != 1 {
        round.fail = if round.stdout_lines.is_empty() {
            Some(C1Fail::MilestoneMissing)
        } else {
            Some(C1Fail::MilestoneDuplicated)
        };
    }
    if round.fail.is_none() && milestone_at.is_none() {
        round.fail = Some(C1Fail::MilestoneMissing);
    }
    if round.fail.is_none() && round.exit_kind != ExitKind::CleanExit {
        round.fail = Some(C1Fail::EarlyExit);
    }
    Ok(round)
}

/// Strict milestone parsing: `VEGA_C1_MILESTONE {json}` and nothing else.
pub fn parse_milestone(line: &str) -> Result<Milestone, C1Fail> {
    let Some(payload) = line.strip_prefix(MILESTONE_PREFIX) else {
        return Err(C1Fail::MilestoneMalformed);
    };
    let milestone: Milestone =
        serde_json::from_str(payload).map_err(|_| C1Fail::MilestoneMalformed)?;
    if milestone.schema != "vega-c1"
        || milestone.metric != contract::PROCESS_START_TO_FIRST_RENDERED_INTERACTIVE
        || milestone.elapsed_us == 0
    {
        return Err(C1Fail::MilestoneMalformed);
    }
    Ok(milestone)
}

/// Mechanical validation of the child's isolation attestation (SDD §2 判失败
/// 真实 profile / provider / network / sleep 冒充 first frame).
fn validate_attestation(milestone: &Milestone, isolation: &Isolation) -> Result<(), C1Fail> {
    let attestation = &milestone.isolation;
    let home_ok = Path::new(&attestation.home) == isolation.home.as_path();
    let data_root_ok = Path::new(&attestation.data_root).starts_with(&isolation.home);
    let sources_ok = attestation.provider == "none" && attestation.network == "none";
    let keychain_ok = attestation.keychain == "not-exercised";
    let frame_source_ok = attestation.first_frame_source == "gpui_next_frame_callback";
    if !home_ok || !data_root_ok {
        return Err(C1Fail::RealProfileAccess);
    }
    if !sources_ok {
        return Err(C1Fail::ProviderNetworkAccess);
    }
    if !keychain_ok {
        return Err(C1Fail::RealProfileAccess);
    }
    if !frame_source_ok {
        return Err(C1Fail::SleepAsFirstFrame);
    }
    Ok(())
}

/// Freezes the C1 p95 gate over the round set: all 20 rounds must pass and
/// the nearest-rank p95 of the parent latencies must be < 50.000 ms
/// (SDD §2 step 4 + 判失败).
pub fn c1_gate(rounds: &[Round]) -> (Vec<u64>, u64, bool) {
    let mut samples: Vec<u64> = rounds
        .iter()
        .filter(|round| round.passed())
        .filter_map(|round| round.parent_latency_us)
        .collect();
    samples.sort_unstable();
    let p95 = contract::percentile(&samples, 95);
    let all_passed = rounds.len() == C1_ROUNDS && rounds.iter().all(Round::passed);
    let gate = all_passed && p95 < contract::C1_THRESHOLD_P95_US;
    (samples, p95, gate)
}

/// C2 per-process medians over passed rounds (raw bytes, ascending).
pub fn c2_medians(rounds: &[Round]) -> Vec<u64> {
    let mut medians: Vec<u64> = rounds
        .iter()
        .filter(|r| r.passed())
        .filter_map(Round::rss_median)
        .collect();
    medians.sort_unstable();
    medians
}

/// C2 gate: nearest-rank p95 of the per-process medians against the frozen
/// threshold, plus the stability verdict (SDD §3 steps 3-4). Returns
/// `(p95_bytes, gate_passed, drifting_rounds)`.
pub fn c2_gate(rounds: &[Round]) -> (u64, bool, usize) {
    let medians = c2_medians(rounds);
    let p95 = contract::percentile(&medians, 95);
    let drift_rounds = rounds
        .iter()
        .filter(|round| {
            round
                .rss_drift()
                .is_some_and(|(sample5s, sample15s)| contract::round_drifts(sample5s, sample15s))
        })
        .count();
    let stable = drift_rounds <= contract::C2_STABILITY_MAX_DRIFTING_ROUNDS;
    let complete = rounds.len() == C2_ROUNDS && rounds.iter().all(Round::passed);
    (
        p95,
        stable && complete && p95 < contract::P8_THRESHOLD_BYTES,
        drift_rounds,
    )
}

fn us(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
}

/// Guard that a protocol run never silently produced fewer rounds than the
/// frozen count.
pub fn assert_round_count(rounds: &[Round], expected: usize) -> Result<()> {
    if rounds.len() != expected {
        bail!(
            "protocol produced {} rounds, expected {expected} (frozen contract)",
            rounds.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::IsolationAttestation;

    fn attestation(home: &str) -> IsolationAttestation {
        IsolationAttestation {
            home: home.into(),
            data_root: format!("{home}/Library/Application Support/ai.vega"),
            provider: "none".into(),
            network: "none".into(),
            keychain: "not-exercised".into(),
            first_frame_source: "gpui_next_frame_callback".into(),
        }
    }

    fn milestone(home: &str) -> Milestone {
        Milestone {
            schema: "vega-c1".into(),
            metric: contract::PROCESS_START_TO_FIRST_RENDERED_INTERACTIVE.into(),
            pid: 1,
            elapsed_us: 9_000,
            isolation: attestation(home),
        }
    }

    #[test]
    fn parse_accepts_exactly_the_milestone_line() {
        let line = format!(
            "{}{}",
            MILESTONE_PREFIX,
            serde_json::to_string(&milestone("/tmp/h")).unwrap()
        );
        assert!(parse_milestone(&line).is_ok());
    }

    #[test]
    fn parse_rejects_malformed_shapes() {
        // Not the prefix.
        assert_eq!(
            parse_milestone(&serde_json::to_string(&milestone("/tmp/h")).unwrap()).unwrap_err(),
            C1Fail::MilestoneMalformed
        );
        // Prefix but not JSON.
        assert_eq!(
            parse_milestone("VEGA_C1_MILESTONE not json").unwrap_err(),
            C1Fail::MilestoneMalformed
        );
        // Valid JSON but wrong schema tag.
        let mut wrong = milestone("/tmp/h");
        wrong.schema = "other".into();
        let line = format!(
            "{}{}",
            MILESTONE_PREFIX,
            serde_json::to_string(&wrong).unwrap()
        );
        assert_eq!(
            parse_milestone(&line).unwrap_err(),
            C1Fail::MilestoneMalformed
        );
        // Wrong metric name (schema drift guard).
        let mut wrong = milestone("/tmp/h");
        wrong.metric = "spawn_to_exit".into();
        let line = format!(
            "{}{}",
            MILESTONE_PREFIX,
            serde_json::to_string(&wrong).unwrap()
        );
        assert_eq!(
            parse_milestone(&line).unwrap_err(),
            C1Fail::MilestoneMalformed
        );
        // Zero elapsed (a milestone cannot fire at t=0).
        let mut wrong = milestone("/tmp/h");
        wrong.elapsed_us = 0;
        let line = format!(
            "{}{}",
            MILESTONE_PREFIX,
            serde_json::to_string(&wrong).unwrap()
        );
        assert_eq!(
            parse_milestone(&line).unwrap_err(),
            C1Fail::MilestoneMalformed
        );
    }

    #[test]
    fn attestation_rejects_real_profile_and_provider_and_sleep() {
        let isolation = Isolation {
            home: PathBuf::from("/tmp/h"),
        };
        // Happy path.
        assert!(validate_attestation(&milestone("/tmp/h"), &isolation).is_ok());
        // Data root outside the temp HOME = real profile access.
        let mut escaped = milestone("/tmp/h");
        escaped.isolation.data_root = "/Users/someone/Library/Application Support/ai.vega".into();
        assert_eq!(
            validate_attestation(&escaped, &isolation).unwrap_err(),
            C1Fail::RealProfileAccess
        );
        // HOME mismatch = real profile access.
        assert_eq!(
            validate_attestation(&milestone("/home/other"), &isolation).unwrap_err(),
            C1Fail::RealProfileAccess
        );
        // A provider was constructed = provider/network access.
        let mut wired = milestone("/tmp/h");
        wired.isolation.provider = "openai".into();
        assert_eq!(
            validate_attestation(&wired, &isolation).unwrap_err(),
            C1Fail::ProviderNetworkAccess
        );
        // A fixed-sleep "first frame" = sleep-as-first-frame.
        let mut sleeper = milestone("/tmp/h");
        sleeper.isolation.first_frame_source = "thread_sleep".into();
        assert_eq!(
            validate_attestation(&sleeper, &isolation).unwrap_err(),
            C1Fail::SleepAsFirstFrame
        );
    }

    #[test]
    fn round_pass_and_fail_classification() {
        let mut round = Round {
            round: 1,
            parent_latency_us: Some(30_000),
            child_elapsed_us: Some(29_000),
            rss_bytes: [Some(1), Some(2), Some(3)],
            exit_kind: ExitKind::CleanExit,
            exit_code: Some(0),
            fail: None,
            stdout_lines: vec!["VEGA_C1_MILESTONE {}".into()],
        };
        assert!(round.passed());
        assert_eq!(round.rss_median(), Some(2));
        assert_eq!(round.rss_drift(), Some((1, 3)));
        // A kill never passes (exit kind + explicit FAIL).
        round.exit_kind = ExitKind::TimeoutKill;
        round.fail = Some(C1Fail::Timeout);
        assert!(!round.passed());
        // A missing C2 sample never passes.
        round.exit_kind = ExitKind::CleanExit;
        round.fail = None;
        round.rss_bytes[2] = None;
        assert!(!round.passed());
    }

    #[test]
    fn c1_gate_needs_all_rounds_and_p95_under_threshold() {
        let passing = |us: u64| Round {
            round: 0,
            parent_latency_us: Some(us),
            child_elapsed_us: Some(us),
            rss_bytes: [Some(1); 3],
            exit_kind: ExitKind::CleanExit,
            exit_code: Some(0),
            fail: None,
            stdout_lines: vec![],
        };
        let rounds: Vec<Round> = (0..C1_ROUNDS).map(|i| passing(30_000 + i as u64)).collect();
        let (samples, p95, gate) = c1_gate(&rounds);
        assert_eq!(samples.len(), C1_ROUNDS);
        assert!(gate, "p95 {p95} must pass under 50_000µs");
        // One failed round voids the gate even with fast samples.
        let mut one_failed = rounds;
        one_failed[0].fail = Some(C1Fail::EarlyExit);
        assert!(!c1_gate(&one_failed).2);
        // p95 over the threshold fails.
        let slow: Vec<Round> = (0..C1_ROUNDS).map(|_| passing(51_000)).collect();
        assert!(!c1_gate(&slow).2);
    }

    #[test]
    fn c2_gate_threshold_stability_and_drift() {
        let round = |rss: [u64; 3]| Round {
            round: 0,
            parent_latency_us: Some(30_000),
            child_elapsed_us: Some(29_000),
            rss_bytes: rss.map(Some),
            exit_kind: ExitKind::CleanExit,
            exit_code: Some(0),
            fail: None,
            stdout_lines: vec![],
        };
        // 20 stable rounds well under the threshold pass.
        let rounds: Vec<Round> = (0..C2_ROUNDS)
            .map(|_| round([90_000_000, 90_100_000, 90_200_000]))
            .collect();
        let (p95, gate, drift) = c2_gate(&rounds);
        assert!(gate, "p95 {p95} must pass under 100_000_000B");
        assert_eq!(drift, 0);
        // p95 over the threshold fails.
        let hot: Vec<Round> = (0..C2_ROUNDS).map(|_| round([101_000_000; 3])).collect();
        let (p95, gate, _) = c2_gate(&hot);
        assert!(!gate);
        assert!(p95 >= contract::P8_THRESHOLD_BYTES);
        // Two drifting rounds → unstable gate even under threshold.
        let mut unstable: Vec<Round> = (0..C2_ROUNDS)
            .map(|_| round([90_000_000, 90_100_000, 90_200_000]))
            .collect();
        unstable[0] = round([90_000_000, 91_000_000, 92_200_000]);
        unstable[1] = round([90_000_000, 91_000_000, 92_300_000]);
        let (_, gate, drift) = c2_gate(&unstable);
        assert!(!gate);
        assert!(drift > contract::C2_STABILITY_MAX_DRIFTING_ROUNDS);
    }

    #[test]
    fn round_count_guard() {
        let rounds: Vec<Round> = vec![];
        assert!(assert_round_count(&rounds, 0).is_ok());
        assert!(assert_round_count(&rounds, 20).is_err());
    }
}
