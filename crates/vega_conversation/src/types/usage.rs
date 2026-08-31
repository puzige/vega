use super::*;

/// Exact pricing provenance attached to a priced usage event (S7-T38/C3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsagePricing {
    /// Engine version that priced the row (e.g. `pricing_v1`).
    pub version: String,
    /// Rate profile chosen by the frozen UTC timestamp (`base` or
    /// `peak_utc_weekly`).
    pub profile: String,
    /// Unix UTC seconds of the logical provider call start used for the
    /// quote.
    pub call_started_at: i64,
}

/// Provider token counts attached to one API call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenUsage {
    /// Prompt tokens.
    pub input: u64,
    /// Completion tokens.
    pub output: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
    /// Cache-write tokens.
    pub cache_write: u64,
}

/// Durable terminal outcome of one summarized assistant task (S7-T40).
///
/// Derived from the persisted `messages.status` DDL vocabulary, never from a
/// UI claim, so the card can never present a running task as finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskSummaryOutcome {
    /// The message converged (`messages.status = 'done'`).
    Completed,
    /// Cancellation interrupted the message (`'interrupted'`).
    Interrupted,
    /// A provider/runtime error failed the message (`'failed'`).
    Failed,
}

/// Checked cost total of a per-task summary (S7-T40/C4).
///
/// Mirrors `vega_store::token_usage::AggregateCost`: `Priced` only when every
/// aggregated row carries the exact priced version, otherwise the total is
/// unavailable rather than a partial sum that would look trustworthy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryCost {
    /// Every provider-call row of the task is priced (may be zero microcents).
    Priced(Microcents),
    /// Rows are missing, legacy/unpriced, or carry an unknown version.
    Unavailable,
}

/// Bounded, typed per-task cost summary (S7-T40/A10-06, C4 contract).
///
/// All fields are projected by `vega_conversation` from the durable
/// `token_usage`/`tool_calls` audits plus the live wall-clock duration; the
/// UI never queries SQLite and never computes a cost formula. Unavailable
/// facts stay typed (`None`/`Unavailable`) and must render as `—`, never as
/// a fabricated zero.
#[derive(Clone, PartialEq, Eq)]
pub struct TaskCostSummary {
    /// Summarized assistant message id (durable recovery key).
    pub message_id: MessageId,
    /// Durable terminal outcome of the message.
    pub outcome: TaskSummaryOutcome,
    /// Four-token aggregate of the task's provider-call rows; `None` when the
    /// task produced no usage rows (typed unavailable).
    pub usage: Option<TokenUsage>,
    /// Checked cost outcome over the task's provider-call rows.
    pub cost: SummaryCost,
    /// Wall-clock task duration in milliseconds. Only available in the live
    /// run's memory (`messages` has no finished timestamp); restart recovery
    /// keeps `None`, which renders as `—` (C4: never faked from tool
    /// durations).
    pub duration_ms: Option<u64>,
    /// Number of persisted tool-call audit rows of the task.
    pub tool_count: u64,
    /// Cache-hit rate in whole percent (half-up), `Some(0)` when input is 0,
    /// `None` when no usage exists (C4: unavailable, not a fabricated ratio).
    pub cache_hit_percent: Option<u8>,
}

impl TaskCostSummary {
    /// Computes the cache-hit percentage for one aggregate: half-up rounded
    /// whole percent of `cache_read / input`, defined as `0%` when input is 0
    /// (C4). The arithmetic runs on `u128` and is checked, so a pathological
    /// aggregate degrades to unavailable instead of wrapping.
    pub fn cache_hit_percent(usage: TokenUsage) -> Option<u8> {
        if usage.input == 0 {
            return Some(0);
        }
        // percent = cache_read * 100 / input, half-up:
        // half_up(a/b) = (2a + b) / (2b) with a = cache_read*100, b = input.
        let numerator = (u128::from(usage.cache_read)) * 200;
        let denominator = u128::from(usage.input) * 2;
        let percent = numerator
            .checked_add(u128::from(usage.input))?
            .checked_div(denominator)?;
        u8::try_from(percent).ok()
    }
}

impl std::fmt::Debug for TaskCostSummary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskCostSummary")
            .field("message_id_bytes", &self.message_id.len())
            .field("outcome", &self.outcome)
            .field("usage", &self.usage)
            .field("cost", &self.cost)
            .field("duration_ms", &self.duration_ms)
            .field("tool_count", &self.tool_count)
            .field("cache_hit_percent", &self.cache_hit_percent)
            .finish()
    }
}
