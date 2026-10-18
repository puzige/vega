//! Blocking-safe read-only Settings usage controller.
use std::path::PathBuf;

use crate::types::{UsageDashboard, UsageDashboardError, UsageDay, UsageModelSeries, UsageTotals};
use vega_store::token_usage::{UsageAggregateError, dashboard};

/// Read capability scoped to the application's existing usage database.
#[derive(Clone)]
pub struct UsageDashboardController {
    database_path: PathBuf,
}
impl UsageDashboardController {
    /// Construct from the application's configured database path.
    pub fn new(database_path: PathBuf) -> Self {
        Self { database_path }
    }

    /// Reload persisted accounting off the caller's UI/executor thread.
    /// `now_ms` is an exclusive upper bound; every displayed day is UTC.
    pub async fn load(&self, now_ms: i64) -> Result<UsageDashboard, UsageDashboardError> {
        let path = self.database_path.clone();
        let data = tokio::task::spawn_blocking(move || {
            vega_store::token_usage::dashboard::load_path(&path, now_ms)
        })
        .await
        .map_err(|_| UsageDashboardError::Unavailable)?
        .map_err(|error| match error {
            UsageAggregateError::CorruptRow { .. } => UsageDashboardError::CorruptData,
            UsageAggregateError::Overflow { .. } => UsageDashboardError::Overflow,
            UsageAggregateError::Sql(_) => UsageDashboardError::Unavailable,
        })?;
        let trend_start = data.start_ms
            + (dashboard::ACTIVITY_DAYS - dashboard::TREND_DAYS) as i64 * dashboard::DAY_MS;
        Ok(UsageDashboard {
            generated_at_ms: now_ms,
            timezone_label: "UTC".into(),
            lifetime: data.lifetime.into(),
            days: days(data.start_ms, data.days),
            models: data
                .models
                .into_iter()
                .map(|model| UsageModelSeries {
                    model: model.model,
                    days: days(trend_start, model.days),
                })
                .collect(),
        })
    }
}
fn days(start_ms: i64, totals: Vec<dashboard::Totals>) -> Vec<UsageDay> {
    totals
        .into_iter()
        .enumerate()
        .map(|(index, totals)| UsageDay {
            start_ms: start_ms + index as i64 * dashboard::DAY_MS,
            totals: totals.into(),
        })
        .collect()
}
impl From<dashboard::Totals> for UsageTotals {
    fn from(value: dashboard::Totals) -> Self {
        Self {
            calls: value.calls,
            total_tokens: value.total_tokens,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            cache_read_tokens: value.cache_read_tokens,
            cache_write_tokens: value.cache_write_tokens,
            priced_cost_microcents: value.priced_cost_microcents,
            unpriced_calls: value.unpriced_calls,
        }
    }
}
