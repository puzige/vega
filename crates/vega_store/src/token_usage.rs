//! Provider usage audit over the existing `token_usage` table: one row per
//! provider call (S4-T19) plus the S7-T38 (C5) price-audit columns and the
//! checked thread/message aggregates.

use rusqlite::{Connection, params};

/// Exact pricing version stamped on rows priced by the S7 integer engine.
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
    /// Integer microcents (S4 placeholder is zero; S7 rows are priced).
    pub cost_microcents: i64,
    /// Unix milliseconds.
    pub created_at: i64,
    /// Exact pricing version that priced this row (`pricing_v1`); `None` keeps
    /// the legacy/unpriced semantics of the S4 zero placeholder (C5).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use tempfile::tempdir;

    fn migrated_store() -> (Store, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        (store, dir)
    }

    fn row<'a>(
        thread_id: &'a str,
        message_id: Option<&'a str>,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
        cost: i64,
    ) -> NewTokenUsage<'a> {
        NewTokenUsage {
            thread_id,
            message_id,
            model: "mock-model",
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            cost_microcents: cost,
            created_at: 1_000,
            pricing_version: Some(PRICED_VERSION),
            pricing_profile: Some("base"),
            call_started_at: Some(1),
        }
    }

    #[test]
    fn insert_round_trips_price_audit_columns_and_legacy_shape() {
        let (store, _dir) = migrated_store();
        insert(store.conn(), row("t1", Some("m1"), 10, 20, 5, 1, 77)).unwrap();
        let legacy = NewTokenUsage {
            pricing_version: None,
            pricing_profile: None,
            call_started_at: None,
            ..row("t1", Some("m1"), 1, 2, 0, 0, 0)
        };
        insert(store.conn(), legacy).unwrap();
        let rows: Vec<(Option<String>, Option<String>, Option<i64>)> = store
            .conn()
            .prepare(
                "SELECT pricing_version, pricing_profile, call_started_at \
                 FROM token_usage ORDER BY id",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (Some("pricing_v1".into()), Some("base".into()), Some(1)),
                (None, None, None),
            ]
        );
    }

    #[test]
    fn thread_aggregate_sums_priced_rows_checked() {
        let (store, _dir) = migrated_store();
        insert(store.conn(), row("t1", Some("m1"), 10, 20, 5, 1, 30)).unwrap();
        insert(store.conn(), row("t1", Some("m2"), 100, 0, 0, 0, 300)).unwrap();
        let aggregate = aggregate_by_thread(store.conn(), "t1").unwrap();
        assert_eq!(
            aggregate,
            UsageAggregate {
                row_count: 2,
                input_tokens: 110,
                output_tokens: 20,
                cache_read_tokens: 5,
                cache_write_tokens: 1,
                cost: AggregateCost::Priced(330),
            }
        );
    }

    #[test]
    fn message_aggregate_scopes_to_thread_and_message() {
        let (store, _dir) = migrated_store();
        insert(store.conn(), row("t1", Some("m1"), 10, 20, 0, 0, 30)).unwrap();
        insert(store.conn(), row("t1", Some("m2"), 100, 0, 0, 0, 300)).unwrap();
        insert(store.conn(), row("t2", Some("m1"), 7, 7, 0, 0, 7)).unwrap();
        let aggregate = aggregate_by_message(store.conn(), "t1", "m1").unwrap();
        assert_eq!(
            aggregate,
            UsageAggregate {
                row_count: 1,
                input_tokens: 10,
                output_tokens: 20,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost: AggregateCost::Priced(30),
            }
        );
        assert_eq!(
            aggregate_by_message(store.conn(), "t1", "missing")
                .unwrap()
                .row_count,
            0
        );
    }

    #[test]
    fn legacy_or_unknown_version_rows_make_cost_unavailable_without_partial_sum() {
        let (store, _dir) = migrated_store();
        insert(store.conn(), row("t1", Some("m1"), 10, 20, 0, 0, 30)).unwrap();
        let legacy = NewTokenUsage {
            pricing_version: None,
            pricing_profile: None,
            call_started_at: None,
            ..row("t1", Some("m1"), 1, 2, 0, 0, 0)
        };
        insert(store.conn(), legacy).unwrap();
        let aggregate = aggregate_by_thread(store.conn(), "t1").unwrap();
        assert_eq!(aggregate.row_count, 2);
        assert_eq!(aggregate.cost, AggregateCost::Unavailable);
        // token 计数本身仍是权威值，可以继续累加
        assert_eq!(aggregate.input_tokens, 11);

        // 未知 version 同样 fail closed 为 unavailable
        let unknown = NewTokenUsage {
            pricing_version: Some("future_pricing_v9"),
            ..row("t1", Some("m2"), 3, 0, 0, 0, 999)
        };
        insert(store.conn(), unknown).unwrap();
        let aggregate = aggregate_by_thread(store.conn(), "t1").unwrap();
        assert_eq!(aggregate.cost, AggregateCost::Unavailable);

        // 纯 priced 集合保持 Priced（priced zero 也是可信值）
        insert(store.conn(), row("t2", Some("m1"), 1, 1, 0, 0, 0)).unwrap();
        let priced_zero = aggregate_by_thread(store.conn(), "t2").unwrap();
        assert_eq!(priced_zero.cost, AggregateCost::Priced(0));
    }

    #[test]
    fn negative_or_overflowing_rows_fail_closed() {
        let (store, _dir) = migrated_store();
        store
            .conn()
            .execute(
                "INSERT INTO token_usage (thread_id, model, input_tokens, output_tokens, \
                        cost_microcents, created_at) VALUES ('t1', 'm', -1, 0, 0, 0)",
                [],
            )
            .unwrap();
        let error = aggregate_by_thread(store.conn(), "t1").unwrap_err();
        assert!(matches!(
            error,
            UsageAggregateError::CorruptRow {
                field: "input_tokens",
            }
        ));

        let (store, _dir) = migrated_store();
        store
            .conn()
            .execute(
                "INSERT INTO token_usage (thread_id, model, input_tokens, output_tokens, \
                        cost_microcents, created_at) \
                 VALUES ('t1', 'm', 9223372036854775807, 0, 0, 0), \
                        ('t1', 'm', 9223372036854775807, 0, 0, 0), \
                        ('t1', 'm', 9223372036854775807, 0, 0, 0)",
                [],
            )
            .unwrap();
        let error = aggregate_by_thread(store.conn(), "t1").unwrap_err();
        assert!(matches!(
            error,
            UsageAggregateError::Overflow {
                field: "input_tokens",
            }
        ));

        // priced 行的成本溢出同样 fail closed
        let (store, _dir) = migrated_store();
        store
            .conn()
            .execute(
                "INSERT INTO token_usage (thread_id, model, input_tokens, output_tokens, \
                        cost_microcents, created_at, pricing_version) \
                 VALUES ('t1', 'm', 0, 0, ?1, 0, 'pricing_v1'), \
                        ('t1', 'm', 0, 0, ?2, 0, 'pricing_v1')",
                [i64::MAX, i64::MAX],
            )
            .unwrap();
        let error = aggregate_by_thread(store.conn(), "t1").unwrap_err();
        assert!(matches!(
            error,
            UsageAggregateError::Overflow {
                field: "cost_microcents",
            }
        ));
    }

    #[test]
    fn aggregate_survives_thread_deletion() {
        let (store, _dir) = migrated_store();
        store
            .conn()
            .execute(
                "INSERT INTO projects (id, path, name, created_at, last_opened_at) \
                 VALUES ('p', '/tmp/p', 'p', 0, 0)",
                [],
            )
            .unwrap();
        crate::threads::create(
            store.conn(),
            crate::threads::NewThread {
                id: "t1",
                project_id: "p",
                title: "t",
                mode: "ask",
                permission_mode: "readonly",
                model: "m",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();
        insert(store.conn(), row("t1", Some("m1"), 10, 20, 0, 0, 30)).unwrap();
        crate::threads::delete_thread(store.conn(), "t1").unwrap();
        let aggregate = aggregate_by_thread(store.conn(), "t1").unwrap();
        assert_eq!(aggregate.cost, AggregateCost::Priced(30));
        assert_eq!(aggregate.row_count, 1);
    }
}
