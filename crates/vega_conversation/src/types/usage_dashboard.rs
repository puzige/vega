//! Bounded Settings usage projections. All day boundaries are UTC.

/// Known cost subtotal and explicit coverage; cache counts are input subsets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageTotals {
    pub calls: u64,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub priced_cost_microcents: i64,
    pub unpriced_calls: u64,
}
/// A zero-filled UTC calendar day.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageDay {
    pub start_ms: i64,
    pub totals: UsageTotals,
}
/// A model's 30-day series; None is the explicit Other models bucket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageModelSeries {
    pub model: Option<String>,
    pub days: Vec<UsageDay>,
}
/// Read-only persisted usage snapshot, bounded to 365 days and 33 model series.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageDashboard {
    pub generated_at_ms: i64,
    pub timezone_label: String,
    pub lifetime: UsageTotals,
    pub days: Vec<UsageDay>,
    pub models: Vec<UsageModelSeries>,
}
/// Content-free dashboard failure, suitable for Settings without leaking SQL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UsageDashboardError {
    #[error("{}", vega_i18n::t("domain.usage.unavailable"))]
    Unavailable,
    #[error("{}", vega_i18n::t("domain.usage.corruptdata"))]
    CorruptData,
    #[error("{}", vega_i18n::t("domain.usage.overflow"))]
    Overflow,
}
