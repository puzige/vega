//! Provider usage audit inserts over the existing `token_usage` table.

use rusqlite::{Connection, params};

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
    /// Integer microcents (S4 placeholder is zero).
    pub cost_microcents: i64,
    /// Unix milliseconds.
    pub created_at: i64,
}

/// Inserts one provider-call usage row.
pub fn insert(conn: &Connection, usage: NewTokenUsage<'_>) -> Result<i64, rusqlite::Error> {
    let input_tokens = sql_i64(usage.input_tokens, "input_tokens")?;
    let output_tokens = sql_i64(usage.output_tokens, "output_tokens")?;
    let cache_read_tokens = sql_i64(usage.cache_read_tokens, "cache_read_tokens")?;
    let cache_write_tokens = sql_i64(usage.cache_write_tokens, "cache_write_tokens")?;
    conn.execute(
        "INSERT INTO token_usage (thread_id, message_id, model, input_tokens, output_tokens, \
         cache_read_tokens, cache_write_tokens, cost_microcents, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn sql_i64(value: u64, field: &str) -> Result<i64, rusqlite::Error> {
    i64::try_from(value).map_err(|_| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{field} exceeds SQLite INTEGER range"),
        )))
    })
}
