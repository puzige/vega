//! Bounded checked Settings aggregates over a single read snapshot.
use super::{PRICED_VERSION, UsageAggregateError};
use rusqlite::Connection;

pub const DAY_MS: i64 = 86_400_000;
pub const ACTIVITY_DAYS: usize = 365;
pub const TREND_DAYS: usize = 30;
pub const MODEL_LIMIT: usize = 32;

/// Persisted totals; priced subtotal must be presented with unpriced count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub calls: u64,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub priced_cost_microcents: i64,
    pub unpriced_calls: u64,
}
/// One exact model or the explicit remainder bucket.
pub struct ModelSeries {
    pub model: Option<String>,
    pub days: Vec<Totals>,
}
/// Bounded store result, mapped at the conversation boundary.
pub struct Dashboard {
    pub start_ms: i64,
    pub lifetime: Totals,
    pub days: Vec<Totals>,
    pub models: Vec<ModelSeries>,
}

fn add(a: &mut u64, b: u64, field: &'static str) -> Result<(), UsageAggregateError> {
    *a = a
        .checked_add(b)
        .ok_or(UsageAggregateError::Overflow { field })?;
    Ok(())
}
impl Totals {
    fn include(
        &mut self,
        counts: [u64; 4],
        cost: i64,
        priced: bool,
    ) -> Result<(), UsageAggregateError> {
        add(&mut self.calls, 1, "calls")?;
        add(&mut self.input_tokens, counts[0], "input_tokens")?;
        add(&mut self.output_tokens, counts[1], "output_tokens")?;
        add(&mut self.cache_read_tokens, counts[2], "cache_read_tokens")?;
        add(
            &mut self.cache_write_tokens,
            counts[3],
            "cache_write_tokens",
        )?;
        add(&mut self.total_tokens, counts[0], "total_tokens")?;
        add(&mut self.total_tokens, counts[1], "total_tokens")?;
        if priced {
            self.priced_cost_microcents = self.priced_cost_microcents.checked_add(cost).ok_or(
                UsageAggregateError::Overflow {
                    field: "cost_microcents",
                },
            )?;
        } else {
            add(&mut self.unpriced_calls, 1, "unpriced_calls")?;
        }
        Ok(())
    }
}

/// Stream rows with constant-size grouped output. Caller supplies a read-only
/// connection; this one SELECT observes one SQLite statement snapshot.
pub fn load(conn: &Connection, now_ms: i64) -> Result<Dashboard, UsageAggregateError> {
    if now_ms < 0 {
        return Err(UsageAggregateError::CorruptRow {
            field: "refresh_timestamp",
        });
    }
    let today = now_ms / DAY_MS * DAY_MS;
    let start_ms = today - (ACTIVITY_DAYS as i64 - 1) * DAY_MS;
    let trend_start = today - (TREND_DAYS as i64 - 1) * DAY_MS;
    let mut result = Dashboard {
        start_ms,
        lifetime: Totals::default(),
        days: vec![Totals::default(); ACTIVITY_DAYS],
        models: Vec::new(),
    };
    let mut statement = conn.prepare("SELECT model, created_at, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost_microcents, pricing_version FROM token_usage WHERE created_at < ?1 ORDER BY model COLLATE BINARY, id")?;
    let mut rows = statement.query([now_ms])?;
    while let Some(row) = rows.next()? {
        let stamp: i64 = row.get(1)?;
        if stamp < 0 {
            return Err(UsageAggregateError::CorruptRow {
                field: "created_at",
            });
        }
        let mut counts = [0_u64; 4];
        for (i, field) in [
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
        ]
        .iter()
        .enumerate()
        {
            counts[i] = u64::try_from(row.get::<_, i64>(i + 2)?)
                .map_err(|_| UsageAggregateError::CorruptRow { field })?;
        }
        let cost: i64 = row.get(6)?;
        if cost < 0 {
            return Err(UsageAggregateError::CorruptRow {
                field: "cost_microcents",
            });
        }
        let priced = row.get::<_, Option<String>>(7)?.as_deref() == Some(PRICED_VERSION);
        result.lifetime.include(counts, cost, priced)?;
        if stamp >= start_ms {
            result.days[((stamp - start_ms) / DAY_MS) as usize].include(counts, cost, priced)?;
        }
        if stamp >= trend_start {
            let model: String = row.get(0)?;
            let index = if let Some(index) = result
                .models
                .iter()
                .position(|item| item.model.as_ref() == Some(&model))
            {
                index
            } else if result.models.len() < MODEL_LIMIT {
                result.models.push(ModelSeries {
                    model: Some(model),
                    days: vec![Totals::default(); TREND_DAYS],
                });
                result.models.len() - 1
            } else {
                if result.models.len() == MODEL_LIMIT {
                    result.models.push(ModelSeries {
                        model: None,
                        days: vec![Totals::default(); TREND_DAYS],
                    });
                }
                MODEL_LIMIT
            };
            result.models[index].days[((stamp - trend_start) / DAY_MS) as usize]
                .include(counts, cost, priced)?;
        }
    }
    Ok(result)
}

/// Open an existing database read-only; never create or migrate from Settings.
pub fn load_path(path: &std::path::Path, now_ms: i64) -> Result<Dashboard, UsageAggregateError> {
    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    load(&conn, now_ms)
}
