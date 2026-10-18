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

#[cfg(test)]
mod tests {
    use super::*;
    use vega_store::{
        Store,
        token_usage::{NewTokenUsage, PRICED_VERSION, insert},
    };

    fn insert_row(
        store: &Store,
        model: &str,
        stamp: i64,
        input: u64,
        cost: i64,
        version: Option<&str>,
    ) {
        insert(
            store.conn(),
            NewTokenUsage {
                thread_id: "owned-thread",
                message_id: None,
                model,
                input_tokens: input,
                output_tokens: 2,
                cache_read_tokens: 3,
                cache_write_tokens: 1,
                cost_microcents: cost,
                created_at: stamp,
                pricing_version: version,
                pricing_profile: None,
                call_started_at: None,
            },
        )
        .unwrap();
    }

    #[tokio::test]
    async fn real_controller_migrated_db_boundaries_prices_models_and_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("owned.db");
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        let controller = UsageDashboardController::new(path);
        let today = 20_000 * dashboard::DAY_MS;
        let now = today + 1_000;
        let first = today - 364 * dashboard::DAY_MS;
        insert_row(&store, "Old", first - 1, 10, 100, Some(PRICED_VERSION));
        insert_row(&store, "A", first, 10, 100, Some(PRICED_VERSION));
        insert_row(
            &store,
            "A",
            today - 29 * dashboard::DAY_MS - 1,
            10,
            100,
            Some(PRICED_VERSION),
        );
        insert_row(
            &store,
            "A",
            today - 29 * dashboard::DAY_MS,
            10,
            100,
            Some(PRICED_VERSION),
        );
        insert_row(&store, "a", today, 20, 0, None);
        insert_row(&store, "future", now, 10, 100, Some(PRICED_VERSION));
        let result = controller.load(now).await.unwrap();
        assert_eq!(result.timezone_label, "UTC");
        assert_eq!(result.lifetime.calls, 5);
        assert_eq!(result.lifetime.total_tokens, 70); // caches already included in input
        assert_eq!(result.lifetime.priced_cost_microcents, 400);
        assert_eq!(result.lifetime.unpriced_calls, 1);
        assert_eq!(result.days.len(), 365);
        assert_eq!(result.days[0].start_ms, first);
        assert_eq!(result.days[0].totals.calls, 1);
        assert_eq!(result.days[1].totals, UsageTotals::default());
        assert_eq!(result.days[364].totals.total_tokens, 22);
        assert_eq!(result.models.len(), 2);
        assert_eq!(result.models[0].model.as_deref(), Some("A"));
        assert_eq!(result.models[1].model.as_deref(), Some("a"));
        assert_eq!(result.models[0].days[0].totals.calls, 1);
        assert_eq!(result.models[0].days.len(), 30);
        let refreshed = controller.load(now + 1).await.unwrap();
        assert_eq!(refreshed.lifetime.calls, 6);
        assert_eq!(refreshed.models.len(), 3);
        // A read never mutates schema or persisted accounting.
        assert_eq!(
            store
                .conn()
                .query_row("SELECT count(*) FROM token_usage", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            6
        );
    }

    #[tokio::test]
    async fn real_controller_bounded_other_bucket_unknown_price_and_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("owned.db");
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        let controller = UsageDashboardController::new(path);
        let now = 20_000 * dashboard::DAY_MS + 1;
        for i in 0..40 {
            insert_row(
                &store,
                &format!("m{i:02}"),
                now - 1,
                10,
                999,
                Some("future"),
            );
        }
        let data = controller.load(now).await.unwrap();
        assert_eq!(data.models.len(), 33);
        assert_eq!(data.models[32].model, None);
        assert_eq!(data.models[32].days[29].totals.calls, 8);
        assert_eq!(data.lifetime.unpriced_calls, 40);
        assert_eq!(data.lifetime.priced_cost_microcents, 0);
        store
            .conn()
            .execute("UPDATE token_usage SET input_tokens = -1 WHERE id = 1", [])
            .unwrap();
        assert_eq!(
            controller.load(now).await,
            Err(UsageDashboardError::CorruptData)
        );
        store
            .conn()
            .execute(
                "UPDATE token_usage SET input_tokens = 9223372036854775807",
                [],
            )
            .unwrap();
        assert_eq!(
            controller.load(now).await,
            Err(UsageDashboardError::Overflow)
        );
        store.conn().execute("UPDATE token_usage SET input_tokens = 0, cost_microcents = 9223372036854775807, pricing_version = 'pricing_v1'", []).unwrap();
        assert_eq!(
            controller.load(now).await,
            Err(UsageDashboardError::Overflow)
        );
    }

    #[tokio::test]
    async fn missing_database_is_not_created_and_empty_database_is_zero_filled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.db");
        let controller = UsageDashboardController::new(path.clone());
        assert_eq!(
            controller.load(1).await,
            Err(UsageDashboardError::Unavailable)
        );
        assert!(!path.exists());
        let store = Store::open(path).unwrap();
        store.migrate().unwrap();
        let result = controller.load(1).await.unwrap();
        assert_eq!(result.lifetime, UsageTotals::default());
        assert_eq!(result.days.len(), 365);
        assert!(result.models.is_empty());
        assert_eq!(
            controller.load(-1).await,
            Err(UsageDashboardError::CorruptData)
        );
    }
}
