use vega_conversation::types::{Microcents, TaskCostSummary};

/// Read-only cost summary of one finished assistant task.
pub struct SummaryCard {
    summary: TaskCostSummary,
}

impl SummaryCard {
    pub fn new(summary: TaskCostSummary) -> Self {
        Self { summary }
    }

    /// The projected summary (test/observation seam).
    pub fn summary(&self) -> &TaskCostSummary {
        &self.summary
    }

    /// Number of fixed-height virtual rows: header, token line, cost line,
    /// footer line, closing border row.
    pub fn row_count(&self) -> usize {
        5
    }
}

/// Formats priced microcents as `US$<cost>` with enough precision to keep
/// nonzero microcents distinguishable (C4). Exact integer math, no floats.
pub(crate) fn format_usd(microcents: Microcents) -> String {
    let Microcents(microcents) = microcents;
    let negative = microcents < 0;
    let value = microcents.unsigned_abs();
    // 6 fractional digits: 1 microcent = $0.000001.
    let whole = value / 1_000_000;
    let fraction = value % 1_000_000;
    let sign = if negative { "-" } else { "" };
    format!("{sign}US${whole}.{fraction:06}")
}
