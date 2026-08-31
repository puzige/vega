//! C3 provenance: every measurement product records git HEAD + dirty state,
//! release profile, absolute binary path with size/mtime/SHA-256, the build
//! command and its exit code, rustc version, OS/CPU/GPU/display refresh
//! rate, scene hash, round count with all raw samples, local + UTC cutoffs,
//! and the result-file SHA-256 (SDD §4; a product missing any of these may
//! be rejected).
//!
//! The binary source rule (SDD §4 MUST, 二选一): the runner unconditionally
//! rebuilds the release probe before measuring — "file exists ≠ provenance"
//! (the `429cb2d` stale-target defect this card removes).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::Serialize;

// ─── SHA-256 (pure-Rust implementation; no new dependency) ───────────────────

/// Minimal FIPS 180-4 SHA-256. Only used for provenance hashes (binary +
/// evidence file integrity); the ~64 MiB probe hashes in well under a second
/// on Apple Silicon.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h) = (
            state[0], state[1], state[2], state[3], state[4], state[5], state[6], state[7],
        );
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read {} for hashing", path.display()))?;
    Ok(sha256_hex(&bytes))
}

// ─── macOS RSS probe (libproc, zero third-party deps) ────────────────────────

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

/// Raw resident bytes via `proc_pidinfo(PROC_PIDTASKINFO).pti_resident_size`
/// (SDD §3/C2: raw bytes, never scaled on ingest).
pub fn rss_resident_bytes(pid: u32) -> Result<u64> {
    let mut info = ProcTaskInfo::default();
    let size = std::mem::size_of::<ProcTaskInfo>() as i32;
    let written = unsafe { proc_pidinfo(pid as i32, PROC_PIDTASKINFO, 0, &mut info, size) };
    if written < size {
        bail!("proc_pidinfo returned {written} bytes, expected {size} (pid may have exited)");
    }
    Ok(info.pti_resident_size)
}

// ─── display refresh rate via CoreGraphics FFI (no new dependency) ───────────

/// Opaque CoreGraphics display-mode handle (pointers only, never built).
#[repr(C)]
struct CgDisplayMode {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayCopyDisplayMode(display: u32) -> *mut CgDisplayMode;
    fn CGDisplayModeGetRefreshRate(mode: *mut CgDisplayMode) -> f64;
    fn CGDisplayModeRelease(mode: *mut CgDisplayMode);
}

/// The main display's current refresh rate in Hz (None when detection fails —
/// the report then records `display_refresh_hz: null` and the run cannot
/// judge literal fps at all).
pub fn main_display_refresh_hz() -> Option<f64> {
    unsafe {
        let display = CGMainDisplayID();
        if display == 0 {
            return None;
        }
        let mode = CGDisplayCopyDisplayMode(display);
        if mode.is_null() {
            return None;
        }
        let hz = CGDisplayModeGetRefreshRate(mode);
        CGDisplayModeRelease(mode);
        (hz > 0.0).then_some(hz)
    }
}

// ─── host + binary provenance ────────────────────────────────────────────────

fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Git HEAD + dirty state of the workspace (C3).
pub fn git_head(workspace: &Path) -> Result<(String, bool)> {
    let head = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("failed to run git rev-parse HEAD")?;
    if !head.status.success() {
        bail!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&head.stderr)
        );
    }
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    let dirty = !Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["status", "--porcelain"])
        .output()
        .map(|output| output.stdout.is_empty())
        .unwrap_or(true);
    Ok((head, dirty))
}

/// Serialisable provenance block attached to every JSON product (C3).
#[derive(Debug, Clone, Serialize)]
pub struct Provenance {
    pub git_head: String,
    pub git_dirty: bool,
    pub profile: &'static str,
    pub binary_path: String,
    pub binary_size_bytes: u64,
    pub binary_mtime_unix_s: u64,
    pub binary_sha256: String,
    pub xtask_binary_path: String,
    pub xtask_binary_sha256: String,
    pub build_command: String,
    pub build_exit_code: i32,
    pub rustc_version: String,
    pub os_version: String,
    pub cpu_model: String,
    pub gpu_model: String,
    pub display_refresh_hz: Option<f64>,
    pub machine: String,
}

/// Builds the provenance block for a freshly built binary set (C3 fields;
/// the caller already ran the unconditional rebuild).
pub fn collect(workspace: &Path, build: &ReleaseBuild) -> Result<Provenance> {
    let (git_head, git_dirty) = git_head(workspace)?;
    // The vega binary is the measured subject; the xtask binary is the
    // probe subprocess. Both hashes are recorded.
    let subject = &build.vega_bin;
    let metadata = std::fs::metadata(subject)
        .with_context(|| format!("failed to stat {}", subject.display()))?;
    let binary_mtime_unix_s = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    println!("hashing {} for provenance ...", subject.display());
    let binary_sha256 = sha256_file(subject)?;
    println!("hashing {} for provenance ...", build.xtask_bin.display());
    let xtask_sha256 = sha256_file(&build.xtask_bin)?;
    Ok(Provenance {
        git_head,
        git_dirty,
        profile: "release",
        binary_path: subject.display().to_string(),
        binary_size_bytes: metadata.len(),
        binary_mtime_unix_s,
        binary_sha256,
        xtask_binary_path: build.xtask_bin.display().to_string(),
        xtask_binary_sha256: xtask_sha256,
        build_command: build.command.clone(),
        build_exit_code: 0,
        rustc_version: output_of("rustc", &["-Vv"]).unwrap_or_else(|| "unknown".into()),
        os_version: output_of("sw_vers", &["-productVersion"]).unwrap_or_else(|| "unknown".into()),
        cpu_model: output_of("sysctl", &["-n", "machdep.cpu.brand_string"])
            .unwrap_or_else(|| "unknown".into()),
        gpu_model: gpu_model(),
        display_refresh_hz: main_display_refresh_hz(),
        machine: output_of("uname", &["-m"]).unwrap_or_else(|| "unknown".into()),
    })
}

fn gpu_model() -> String {
    // The ioreg one-liner gives the model string without spawning the full
    // system profiler (which is slow).
    output_of(
        "ioreg",
        &["-r", "-d", "1", "-c", "IOPCIDevice", "-k", "model"],
    )
    .and_then(|text| {
        text.lines().find_map(|line| {
            let start = line.find(r#""model"="<"#)? + r#""model"="<"#.len();
            Some(
                line[start..]
                    .split('"')
                    .next()
                    .unwrap_or("unknown")
                    .to_string(),
            )
        })
    })
    .or_else(|| {
        output_of("system_profiler", &["SPDisplaysDataType"]).and_then(|text| {
            text.lines()
                .find(|line| line.trim().starts_with("Chipset Model:"))
                .map(|line| {
                    line.trim()
                        .trim_start_matches("Chipset Model:")
                        .trim()
                        .to_string()
                })
        })
    })
    .unwrap_or_else(|| "unknown".into())
}

/// The release artifacts of one unconditional rebuild (SDD §4 MUST: rebuild,
/// never accept a stale target binary — "文件存在 ≠ provenance").
#[derive(Debug)]
pub struct ReleaseBuild {
    /// The xtask release binary — doubles as the C1/C2/P2 probe subprocess
    /// via the hidden `__probe` re-exec.
    pub xtask_bin: PathBuf,
    /// The vega release binary — subject of the legacy render probe (P1
    /// margin).
    pub vega_bin: PathBuf,
    pub command: String,
}

/// Unconditional release rebuild of both measurement binaries; file
/// existence alone is never accepted as provenance.
pub fn rebuild_release(workspace: &Path) -> Result<ReleaseBuild> {
    let command = "cargo build --release -p xtask -p vega";
    println!("unconditionally rebuilding the release binaries ({command}) ...");
    let started = Instant::now();
    let status = Command::new(cargo_bin())
        .current_dir(workspace)
        .args(["build", "--release", "-p", "xtask", "-p", "vega"])
        .status()
        .with_context(|| format!("failed to run {command}"))?;
    let xtask_bin = workspace.join("target/release/xtask");
    let vega_bin = workspace.join("target/release/vega");
    if !status.success() {
        bail!("{command} failed with {status}");
    }
    for binary in [&xtask_bin, &vega_bin] {
        if !binary.exists() {
            bail!("binary not found at {} after build", binary.display());
        }
    }
    println!(
        "  release build done in {:.1}s",
        started.elapsed().as_secs_f32()
    );
    Ok(ReleaseBuild {
        xtask_bin,
        vega_bin,
        command: command.to_string(),
    })
}

/// Prefers the rustup proxy so rust-toolchain.toml is honored even when
/// Homebrew's cargo shadows it in PATH (same fix as .githooks).
fn cargo_bin() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let proxy = Path::new(&home).join(".cargo/bin/cargo");
        if proxy.exists() {
            return proxy;
        }
    }
    PathBuf::from("cargo")
}

// ─── evidence cutoffs ────────────────────────────────────────────────────────

/// Local + UTC wall-clock at evidence write time (C3; RFC 3339-ish, no new
/// dependency — UTC seconds since the epoch + a local `date` fallback).
#[derive(Debug, Serialize)]
pub struct Cutoff {
    pub utc_unix_s: u64,
    pub utc_rfc3339: String,
    pub local_rfc3339: String,
}

pub fn cutoff() -> Result<Cutoff> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let utc_unix_s = now.as_secs();
    let utc_rfc3339 = format_unix_utc(utc_unix_s);
    let local_rfc3339 = output_of("date", &["+%Y-%m-%dT%H:%M:%S%z"])
        .map(|text| {
            // `date +%z` emits `+0800`; insert the RFC 3339 colon.
            match text.char_indices().nth_back(2) {
                Some((index, _)) => format!("{}:{}", &text[..index], &text[index..]),
                None => text,
            }
        })
        .unwrap_or_else(|| format!("unix:{utc_unix_s}"));
    Ok(Cutoff {
        utc_unix_s,
        utc_rfc3339,
        local_rfc3339,
    })
}

/// Days-from-civil inverse (Howard Hinnant's algorithm) — UTC only, for the
/// RFC 3339 string (no chrono dependency).
fn format_unix_utc(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"The quick brown fox jumps over the lazy dog"),
            "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
        );
        // Multi-block (> 64 bytes) input.
        let long = vec![b'a'; 1_000];
        let hex = sha256_hex(&long);
        assert_eq!(hex.len(), 64);
        // Same input twice → same hash; one byte flip → different hash.
        let mut flipped = long.clone();
        flipped[0] = b'b';
        assert_eq!(sha256_hex(&long), sha256_hex(&long));
        assert_ne!(sha256_hex(&long), sha256_hex(&flipped));
    }

    #[test]
    fn unix_utc_formatting() {
        assert_eq!(format_unix_utc(0), "1970-01-01T00:00:00Z");
        // 2026-08-31T00:00:00Z = 1788134400.
        assert_eq!(format_unix_utc(1_788_134_400), "2026-08-31T00:00:00Z");
        assert_eq!(format_unix_utc(86_399), "1970-01-01T23:59:59Z");
    }

    #[test]
    fn refresh_rate_detection_returns_a_value_or_none() {
        // On this macOS host the detection must either succeed with a sane
        // Hz or return None (never a garbage value).
        if let Some(hz) = main_display_refresh_hz() {
            assert!((23.9..=240.0).contains(&hz), "implausible refresh {hz}Hz");
        }
    }

    #[test]
    fn rss_probe_rejects_bogus_pid() {
        // pid 0 is the kernel; proc_pidinfo fails the size check.
        assert!(rss_resident_bytes(0).is_err());
    }
}
