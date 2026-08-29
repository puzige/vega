//! Tool-call audit CRUD over the existing `tool_calls` table.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};

/// Insertable tool-call fields.
pub struct NewToolCall<'a> {
    /// Provider call id.
    pub id: &'a str,
    /// Owning thread.
    pub thread_id: &'a str,
    /// Assistant message that proposed the call.
    pub message_id: &'a str,
    /// Monotonic call sequence.
    pub seq: i64,
    /// Tool name.
    pub tool: &'a str,
    /// Raw JSON arguments.
    pub input_json: &'a str,
    /// Initial DDL status.
    pub status: &'a str,
    /// Unix milliseconds.
    pub created_at: i64,
}

/// Returns the next tool-call sequence for a thread.
pub fn next_seq(conn: &Connection, thread_id: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM tool_calls WHERE thread_id = ?1",
        [thread_id],
        |row| row.get(0),
    )
}

/// Inserts a proposed tool call.
pub fn insert(conn: &Connection, call: NewToolCall<'_>) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, status, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![call.id, call.thread_id, call.message_id, call.seq, call.tool, call.input_json, call.status, call.created_at],
    )?;
    Ok(())
}

/// Whether a call id is already persisted.
pub fn exists(conn: &Connection, id: &str) -> Result<bool, rusqlite::Error> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM tool_calls WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(found.is_some())
}

/// Updates the lifecycle status, optional approval, and optional output.
pub fn update(
    conn: &Connection,
    id: &str,
    status: &str,
    approval: Option<&str>,
    output_text: Option<&str>,
    finished_at: Option<i64>,
) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "UPDATE tool_calls SET status = ?1, approval = COALESCE(?2, approval), \
         output_text = COALESCE(?3, output_text), finished_at = COALESCE(?4, finished_at) \
         WHERE id = ?5",
        params![status, approval, output_text, finished_at, id],
    )
}

/// Loads successful results for restart/retry call-id deduplication.
pub fn successful_results(
    conn: &Connection,
    thread_id: &str,
) -> Result<HashMap<String, String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(output_text, '') FROM tool_calls \
         WHERE thread_id = ?1 AND status = 'success'",
    )?;
    let rows = stmt.query_map([thread_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}
