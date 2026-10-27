pub mod dashboard;

use rusqlite::{Connection, params};

pub const PRICED_VERSION: &str = "pricing_v1";

/// Insertable token accounting fields.
pub struct NewTokenUsage<'a> {
    /// Owning thread.
    pub thread_id: &'a str,
    /// Assistant message, when known.
    pub message_id: Option<&'a str>,
    /// Provider model id.
    pub model: &'a str,
    /// Prompt tokens.
    pub input_tokens: u64,
    /// Completion tokens.
    pub output_tokens: u64,
    /// Cache-read tokens.
    pub cache_read_tokens: u64,
    /// Cache-write tokens.
    pub cache_write_tokens: u64,
    pub cost_microcents: i64,
    /// Unix milliseconds.
    pub created_at: i64,
    pub pricing_version: Option<&'a str>,
    /// Exact rate profile used by the quote (`base` | `peak_utc_weekly`);
    /// `None` for legacy rows.
    pub pricing_profile: Option<&'a str>,
    /// Unix UTC seconds of the logical provider call start frozen for the
    /// quote; `None` for legacy rows.
    pub call_started_at: Option<i64>,
}

/// Inserts one provider-call usage row.
pub fn insert(conn: &Connection, usage: NewTokenUsage<'_>) -> Result<i64, rusqlite::Error> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Usage)?;
    let input_tokens = sql_i64(usage.input_tokens, "input_tokens")?;
    let output_tokens = sql_i64(usage.output_tokens, "output_tokens")?;
    let cache_read_tokens = sql_i64(usage.cache_read_tokens, "cache_read_tokens")?;
    let cache_write_tokens = sql_i64(usage.cache_write_tokens, "cache_write_tokens")?;
    conn.execute(
        "INSERT INTO token_usage (thread_id, message_id, model, input_tokens, output_tokens, \
         cache_read_tokens, cache_write_tokens, cost_microcents, created_at, \
         pricing_version, pricing_profile, call_started_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            usage.thread_id,
            usage.message_id,
            usage.model,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            usage.cost_microcents,
            usage.created_at,
            usage.pricing_version,
            usage.pricing_profile,
            usage.call_started_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Checked cost total of an aggregate: `Priced` only when every aggregated row
/// carries the exact [`PRICED_VERSION`]; otherwise the partial sum would look
/// trustworthy while mixing legacy zero placeholders (C2/C5 fail closed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregateCost {
    /// Every row is priced; the checked total in microcents (may be zero).
    Priced(i64),
    /// At least one row is legacy/unpriced or carries an unknown version.
    Unavailable,
}

/// Checked usage aggregate over `token_usage` rows (thread or message scope).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageAggregate {
    /// Number of aggregated rows.
    pub row_count: u64,
    /// Sum of prompt tokens.
    pub input_tokens: u64,
    /// Sum of completion tokens.
    pub output_tokens: u64,
    /// Sum of cache-read tokens.
    pub cache_read_tokens: u64,
    /// Sum of cache-write tokens.
    pub cache_write_tokens: u64,
    /// Checked cost outcome over the aggregated rows.
    pub cost: AggregateCost,
}

/// Typed failure of a checked aggregate: corrupt rows (negative values) and
/// checked-arithmetic overflow fail closed instead of returning a partial sum.
#[derive(Debug, thiserror::Error)]
pub enum UsageAggregateError {
    /// A stored value violates the row invariant (e.g. negative tokens).
    #[error("token usage row is corrupt: {field}")]
    CorruptRow {
        /// Corrupt field name (safe, content-free).
        field: &'static str,
    },
    /// The checked sum exceeded the integer budget.
    #[error("token usage aggregate overflow: {field}")]
    Overflow {
        /// Overflowing field name (safe, content-free).
        field: &'static str,
    },
    /// Underlying SQLite failure.
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

/// Aggregates every `token_usage` row of a thread with checked arithmetic.
///
/// Rows survive thread deletion (the table has no thread foreign key), so the
/// usage audit keeps aggregating after the thread itself is gone.
pub fn aggregate_by_thread(
    conn: &Connection,
    thread_id: &str,
) -> Result<UsageAggregate, UsageAggregateError> {
    let mut statement = conn.prepare(
        "SELECT input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
                cost_microcents, pricing_version \
         FROM token_usage WHERE thread_id = ?1 ORDER BY id",
    )?;
    aggregate_rows(statement.query_map([thread_id], map_aggregate_row)?)
}

pub fn latest_primary_call_total(
    conn: &Connection,
    thread_id: &str,
) -> Result<Option<u64>, UsageAggregateError> {
    let mut statement = conn.prepare(
        "SELECT input_tokens, output_tokens FROM token_usage \
         WHERE thread_id = ?1 AND message_id IS NOT NULL ORDER BY id DESC LIMIT 1",
    )?;
    let mut rows = statement.query([thread_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let input_tokens: i64 = row.get(0)?;
    let output_tokens: i64 = row.get(1)?;
    if input_tokens < 0 {
        return Err(UsageAggregateError::CorruptRow {
            field: "input_tokens",
        });
    }
    if output_tokens < 0 {
        return Err(UsageAggregateError::CorruptRow {
            field: "output_tokens",
        });
    }
    if input_tokens == 0 {
        return Ok(None);
    }
    let input_tokens = u64::try_from(input_tokens).unwrap_or(0);
    let output_tokens = u64::try_from(output_tokens).unwrap_or(0);
    input_tokens
        .checked_add(output_tokens)
        .map(Some)
        .ok_or(UsageAggregateError::Overflow {
            field: "input_tokens",
        })
}

pub fn aggregate_task_by_thread(
    conn: &Connection,
    thread_id: &str,
) -> Result<UsageAggregate, UsageAggregateError> {
    let mut statement = conn.prepare("SELECT input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,cost_microcents,pricing_version FROM token_usage WHERE thread_id=?1 OR delegated_parent_thread_id=?1 ORDER BY id")?;
    aggregate_rows(statement.query_map([thread_id], map_aggregate_row)?)
}

pub fn aggregate_task_by_message(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
) -> Result<UsageAggregate, UsageAggregateError> {
    let mut statement = conn.prepare("SELECT input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,cost_microcents,pricing_version FROM token_usage WHERE (thread_id=?1 AND message_id=?2) OR (delegated_parent_thread_id=?1 AND delegated_parent_message_id=?2) ORDER BY id")?;
    aggregate_rows(statement.query_map(params![thread_id, message_id], map_aggregate_row)?)
}

/// Aggregates every `token_usage` row of one assistant message with checked
/// arithmetic (per-task cost scope).
pub fn aggregate_by_message(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
) -> Result<UsageAggregate, UsageAggregateError> {
    let mut statement = conn.prepare(
        "SELECT input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
                cost_microcents, pricing_version \
         FROM token_usage \
         WHERE thread_id = ?1 AND message_id IS ?2 ORDER BY id",
    )?;
    aggregate_rows(statement.query_map(params![thread_id, message_id], map_aggregate_row)?)
}

fn map_aggregate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AggregateRow> {
    Ok(AggregateRow {
        input_tokens: row.get(0)?,
        output_tokens: row.get(1)?,
        cache_read_tokens: row.get(2)?,
        cache_write_tokens: row.get(3)?,
        cost_microcents: row.get(4)?,
        pricing_version: row.get(5)?,
    })
}

fn aggregate_rows(
    rows: impl Iterator<Item = rusqlite::Result<AggregateRow>>,
) -> Result<UsageAggregate, UsageAggregateError> {
    let mut aggregate = UsageAggregate {
        row_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cost: AggregateCost::Priced(0),
    };
    let mut unpriced = false;
    for row in rows {
        let AggregateRow {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            cost_microcents,
            pricing_version,
        } = row?;
        if input_tokens < 0 {
            return Err(UsageAggregateError::CorruptRow {
                field: "input_tokens",
            });
        }
        if output_tokens < 0 {
            return Err(UsageAggregateError::CorruptRow {
                field: "output_tokens",
            });
        }
        if cache_read_tokens < 0 {
            return Err(UsageAggregateError::CorruptRow {
                field: "cache_read_tokens",
            });
        }
        if cache_write_tokens < 0 {
            return Err(UsageAggregateError::CorruptRow {
                field: "cache_write_tokens",
            });
        }
        if cost_microcents < 0 {
            return Err(UsageAggregateError::CorruptRow {
                field: "cost_microcents",
            });
        }
        // 前面已对负值 fail closed，这里 try_from 的兜底保持无 panic。
        let input_tokens = u64::try_from(input_tokens).unwrap_or(0);
        let output_tokens = u64::try_from(output_tokens).unwrap_or(0);
        let cache_read_tokens = u64::try_from(cache_read_tokens).unwrap_or(0);
        let cache_write_tokens = u64::try_from(cache_write_tokens).unwrap_or(0);
        aggregate.row_count = aggregate
            .row_count
            .checked_add(1)
            .ok_or(UsageAggregateError::Overflow { field: "row_count" })?;
        aggregate.input_tokens = aggregate.input_tokens.checked_add(input_tokens).ok_or(
            UsageAggregateError::Overflow {
                field: "input_tokens",
            },
        )?;
        aggregate.output_tokens = aggregate.output_tokens.checked_add(output_tokens).ok_or(
            UsageAggregateError::Overflow {
                field: "output_tokens",
            },
        )?;
        aggregate.cache_read_tokens = aggregate
            .cache_read_tokens
            .checked_add(cache_read_tokens)
            .ok_or(UsageAggregateError::Overflow {
                field: "cache_read_tokens",
            })?;
        aggregate.cache_write_tokens = aggregate
            .cache_write_tokens
            .checked_add(cache_write_tokens)
            .ok_or(UsageAggregateError::Overflow {
                field: "cache_write_tokens",
            })?;
        let priced = pricing_version.as_deref() == Some(PRICED_VERSION);
        unpriced |= !priced;
        if !unpriced && let AggregateCost::Priced(total) = &mut aggregate.cost {
            *total = total
                .checked_add(cost_microcents)
                .ok_or(UsageAggregateError::Overflow {
                    field: "cost_microcents",
                })?;
        }
    }
    if unpriced {
        aggregate.cost = AggregateCost::Unavailable;
    }
    Ok(aggregate)
}

struct AggregateRow {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    cost_microcents: i64,
    pricing_version: Option<String>,
}

fn sql_i64(value: u64, field: &str) -> Result<i64, rusqlite::Error> {
    i64::try_from(value).map_err(|_| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{field} exceeds SQLite INTEGER range"),
        )))
    })
}
