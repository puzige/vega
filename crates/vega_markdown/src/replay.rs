//! Mock delta replayer (S3-T18): turns static markdown content into a paced
//! delta stream feeding [`MarkdownStream`](crate::MarkdownStream) — the shared
//! infra behind the S3 acceptance demo and the S4 mock provider.
//!
//! The replayer is a **pure, UI-free state machine** (tech-spec §5 keeps this
//! crate headless like `vega_runtime`): it holds the pre-split delta list and
//! a rate, and hands out the deltas that are "due" since the last call. The
//! driver loop — GPUI timer in S3 (`vega_ui::conversation_stream`), a tokio
//! interval for the S4 mock provider — stays with the caller; the crate
//! deliberately grows no new dependencies.
//!
//! Pacing uses the spike/T17 self-correcting scheme: the injected count
//! follows `rate × elapsed` instead of trusting tick regularity, so main
//! -thread jitter cannot accumulate drift.
//!
//! # Example
//!
//! ```
//! use std::time::{Duration, Instant};
//! use vega_markdown::{MarkdownStream, MockReplay};
//!
//! let mut replay = MockReplay::new("# Title\n\nBody", 500, 0x5EED);
//! let started = Instant::now();
//! let mut stream = MarkdownStream::new();
//! while !replay.is_finished() {
//!     // One driver tick (the real caller polls on a ~16ms timer):
//!     for delta in replay.take_due_at(started + Duration::from_secs(10)) {
//!         stream.append(&delta);
//!     }
//! }
//! assert!(replay.is_finished());
//! // 消息流结束时（tech-spec §5.4）终结：作废 pending 补全残留。
//! stream.finish();
//! assert!(stream.snapshot().pending.is_none());
//! ```

use std::path::Path;
use std::time::Instant;

/// Splits a document into 3..8-char deltas without splitting UTF-8 codepoints
/// (T14 spike / T15 harness method; shared by the S3 demo injection, the
/// render_frame bench and the future S4 mock provider).
pub fn split_deltas(doc: &str, seed: u64) -> Vec<String> {
    let mut deltas = Vec::new();
    let mut chunk = String::new();
    let mut state = seed;
    for ch in doc.chars() {
        chunk.push(ch);
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        if chunk.chars().count() >= 3 + (state >> 33) as usize % 6 {
            deltas.push(std::mem::take(&mut chunk));
        }
    }
    if !chunk.is_empty() {
        deltas.push(chunk);
    }
    deltas
}

/// Paced mock replay of one markdown document: `content → N δ/s → caller`.
pub struct MockReplay {
    deltas: Vec<String>,
    cursor: usize,
    /// Rate baseline (`rate × elapsed` self-correcting pacing).
    started: Instant,
    rate_dps: f64,
}

impl MockReplay {
    /// Builds a replayer for `content` at `rate_dps` deltas/second. The delta
    /// split is deterministic for a given `seed` (same content + seed → same
    /// stream, keeping demos and bench runs reproducible).
    pub fn new(content: &str, rate_dps: usize, seed: u64) -> Self {
        let mut deltas = split_deltas(content, seed);
        deltas.shrink_to_fit();
        Self {
            deltas,
            cursor: 0,
            started: Instant::now(),
            rate_dps: rate_dps as f64,
        }
    }

    /// Builds a replayer from a local markdown file (任务卡：读本地 md 文件按
    /// N delta/s 注入).
    pub fn from_path(path: &Path, rate_dps: usize, seed: u64) -> std::io::Result<Self> {
        Ok(Self::new(&std::fs::read_to_string(path)?, rate_dps, seed))
    }

    /// Deltas due by `now`: everything up to the self-correcting target
    /// `rate × elapsed` (never more than what remains). The caller appends
    /// the returned batch to its [`MarkdownStream`](crate::MarkdownStream).
    pub fn take_due_at(&mut self, now: Instant) -> Vec<String> {
        let target =
            (now.saturating_duration_since(self.started).as_secs_f64() * self.rate_dps) as usize;
        let end = target.min(self.deltas.len());
        let batch = self.deltas[self.cursor..end].to_vec();
        self.cursor = end;
        batch
    }

    /// Deltas due right now ([`take_due_at`](Self::take_due_at) with the
    /// current clock).
    pub fn take_due(&mut self) -> Vec<String> {
        self.take_due_at(Instant::now())
    }

    /// Deltas handed out so far.
    pub fn injected(&self) -> usize {
        self.cursor
    }

    /// Total delta count of the payload.
    pub fn total(&self) -> usize {
        self.deltas.len()
    }

    /// Whether the whole payload has been handed out (the caller then ends
    /// the message stream — `MarkdownStream::finish()`, tech-spec §5.4).
    pub fn is_finished(&self) -> bool {
        self.cursor >= self.deltas.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MarkdownStream;
    use std::time::Duration;

    #[test]
    fn split_deltas_round_trips_without_splitting_codepoints() {
        let doc = "# 标题\n\n正文 **加粗** 与 English 混排。\n\n```rust\nlet x = 1;\n```\n";
        let deltas = split_deltas(doc, 0x5EED);
        assert!(deltas.len() > 10);
        assert_eq!(deltas.concat(), doc);
        assert!(deltas.iter().all(|delta| delta.chars().count() >= 3));
    }

    #[test]
    fn take_due_follows_rate_times_elapsed_and_drains_in_order() {
        // ~1040 δ（3..8 字符/δ），大于 1s 目标 500 δ。
        let doc = "a".repeat(5000);
        let mut replay = MockReplay::new(&doc, 500, 0x5EED);
        let started = Instant::now();
        // 1s → 目标 500 δ（payload 1000+ δ，未耗尽）。
        let first = replay.take_due_at(started + Duration::from_secs(1));
        assert_eq!(first.len(), 500);
        assert_eq!(replay.injected(), 500);
        assert!(!replay.is_finished());
        // 重复同一时刻：目标数不变 → 空批（自校正不重复注入）。
        assert!(
            replay
                .take_due_at(started + Duration::from_secs(1))
                .is_empty()
        );
        // 3s → 目标 1500 ≥ 总量 → 一次排空，拼接为原文前缀续段。
        let rest = replay.take_due_at(started + Duration::from_secs(3));
        assert_eq!(
            first
                .iter()
                .chain(rest.iter())
                .map(String::as_str)
                .collect::<String>(),
            doc
        );
        assert!(replay.is_finished());
        assert_eq!(replay.injected(), replay.total());
    }

    #[test]
    fn from_path_reads_a_local_markdown_file() {
        let path = std::env::temp_dir().join(format!("vega-replay-test-{}.md", std::process::id()));
        std::fs::write(&path, "# replay source\n").expect("write test fixture");
        let replay = MockReplay::from_path(&path, 100, 1).expect("fixture file must read back");
        assert!(replay.total() > 0);
        let _ = std::fs::remove_file(&path);
        assert!(MockReplay::from_path(&path, 100, 1).is_err());
    }

    #[test]
    fn replay_into_stream_then_finish_discards_the_pending_completion_guess() {
        // tech-spec §5.4：回放结束时 finish() —— terminator 对 `**bo` 的补全
        // 猜测被最终语义重解析覆盖（UI 上不残留补全态）。
        let mut replay = MockReplay::new("stub **bo", 500, 0x5EED);
        let mut stream = MarkdownStream::new();
        while !replay.is_finished() {
            for delta in replay.take_due() {
                stream.append(&delta);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let pending_id = {
            let mid = stream.snapshot();
            let pending = mid.pending.expect("trailing `**bo` must stay pending");
            assert!(matches!(
                pending.nodes,
                [crate::RenderNode::Paragraph { .. }]
            ));
            pending.block_id
        };
        stream.finish();
        let snapshot = stream.snapshot();
        assert!(snapshot.pending.is_none());
        assert_eq!(snapshot.blocks.len(), 1);
        assert_eq!(snapshot.blocks[0].block_id, pending_id);
        assert_eq!(
            snapshot.blocks[0].nodes,
            &[crate::RenderNode::Paragraph {
                spans: vec![crate::Inline::Text("stub **bo".into())]
            }][..]
        );
    }
}
