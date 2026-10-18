use std::path::Path;
use std::time::Instant;

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
