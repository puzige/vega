//! Message CRUD over the existing `messages` table (tech-spec §2).

use rusqlite::{Connection, OptionalExtension, params};

/// One persisted conversation message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRow {
    /// Message id.
    pub id: String,
    /// Owning thread id.
    pub thread_id: String,
    /// Monotonic thread-local sequence.
    pub seq: i64,
    /// DDL role string.
    pub role: String,
    /// DDL kind string.
    pub kind: String,
    /// Complete Markdown content.
    pub content: String,
    /// DDL lifecycle status.
    pub status: String,
    /// Unix milliseconds.
    pub created_at: i64,
}

/// Returns the next message sequence for `thread_id`.
pub fn next_seq(conn: &Connection, thread_id: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE thread_id = ?1",
        [thread_id],
        |row| row.get(0),
    )
}

/// Inserts one message without changing the schema.
pub fn insert(conn: &Connection, row: &MessageRow) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            row.id,
            row.thread_id,
            row.seq,
            row.role,
            row.kind,
            row.content,
            row.status,
            row.created_at
        ],
    )?;
    Ok(())
}

/// Replaces complete message content and status.
pub fn finish(
    conn: &Connection,
    id: &str,
    content: &str,
    status: &str,
) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "UPDATE messages SET content = ?1, status = ?2 WHERE id = ?3",
        params![content, status, id],
    )
}

/// Loads the newest `limit` completed/history messages in chronological order.
pub fn recent(
    conn: &Connection,
    thread_id: &str,
    limit: usize,
) -> Result<Vec<MessageRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, seq, role, kind, content, status, created_at \
         FROM (SELECT id, thread_id, seq, role, kind, content, status, created_at \
               FROM messages WHERE thread_id = ?1 AND status != 'streaming' \
               ORDER BY seq DESC LIMIT ?2) ORDER BY seq ASC",
    )?;
    stmt.query_map(params![thread_id, limit as i64], row_from_query)?
        .collect()
}

/// Finds one message by id.
pub fn find(conn: &Connection, id: &str) -> Result<Option<MessageRow>, rusqlite::Error> {
    conn.query_row(
        "SELECT id, thread_id, seq, role, kind, content, status, created_at \
         FROM messages WHERE id = ?1",
        [id],
        row_from_query,
    )
    .optional()
}

fn row_from_query(row: &rusqlite::Row<'_>) -> Result<MessageRow, rusqlite::Error> {
    Ok(MessageRow {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        seq: row.get(2)?,
        role: row.get(3)?,
        kind: row.get(4)?,
        content: row.get(5)?,
        status: row.get(6)?,
        created_at: row.get(7)?,
    })
}
