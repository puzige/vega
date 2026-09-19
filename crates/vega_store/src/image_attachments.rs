//! Issue 63 R5: ordered blobs with bounded batch reads inside caller snapshot.
use rusqlite::{Connection, params};

/// Store transport, deliberately redacted in Debug.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageRow {
    pub message_id: String,
    pub ordinal: i64,
    pub encoded: Vec<u8>,
}
impl std::fmt::Debug for ImageRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageRow")
            .field("ordinal", &self.ordinal)
            .field("bytes", &self.encoded.len())
            .finish()
    }
}

/// Inserts only into a user text message; caller owns transaction atomicity.
pub fn insert(
    conn: &Connection,
    message_id: &str,
    ordinal: usize,
    bytes: &[u8],
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "INSERT INTO image_attachments (message_id, ordinal, encoded) SELECT id, ?2, ?3 FROM messages WHERE id = ?1 AND role = 'user' AND kind = 'text'",
        params![message_id, ordinal as i64, bytes],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

/// Reads a page's exact owners in one query, checking byte budgets before copies.
pub fn for_messages(
    conn: &Connection,
    thread_id: &str,
    messages: &[crate::messages::MessageRow],
) -> rusqlite::Result<Vec<ImageRow>> {
    if messages.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; messages.len()].join(",");
    let scope = format!(
        "FROM image_attachments a JOIN messages m ON m.id = a.message_id WHERE m.thread_id = ? AND m.id IN ({placeholders})"
    );
    let values: Vec<&str> = std::iter::once(thread_id)
        .chain(messages.iter().map(|message| message.id.as_str()))
        .collect();
    // R5: SQLite length(BLOB) reads metadata; reject before any blob allocation.
    let size: i64 = conn.query_row(
        &format!("SELECT COALESCE(SUM(length(a.encoded)), 0) {scope}"),
        rusqlite::params_from_iter(values.iter()),
        |row| row.get(0),
    )?;
    if size > 32 * 1024 * 1024 {
        return Err(rusqlite::Error::InvalidParameterName(
            "attachment history exceeds 32 MiB".into(),
        ));
    }
    let mut stmt = conn.prepare(&format!(
        "SELECT a.message_id, a.ordinal, a.encoded {scope} ORDER BY m.seq, a.ordinal"
    ))?;
    let mut rows = stmt.query(rusqlite::params_from_iter(values.iter()))?;
    let mut result = Vec::new();
    let mut total = 0usize;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        if !messages.iter().any(|message| message.id == id) {
            continue;
        }
        let bytes = row.get_ref(2)?.as_blob()?;
        total = total.saturating_add(bytes.len());
        if total > 32 * 1024 * 1024 {
            return Err(rusqlite::Error::InvalidParameterName(
                "attachment history exceeds 32 MiB".into(),
            ));
        }
        result.push(ImageRow {
            message_id: id,
            ordinal: row.get(1)?,
            encoded: bytes.to_vec(),
        });
    }
    Ok(result)
}
