use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone)]
pub struct PendingMessageRow {
    pub id: String,
    pub thread_id: String,
    pub ordinal: i64,
    pub behavior: String,
    pub generation: String,
    pub payload: String,
    pub created_at: i64,
}

pub fn insert(conn: &Connection, row: &PendingMessageRow) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::PendingMessages,
    )?;
    conn.execute(
        "INSERT INTO pending_messages (id, thread_id, ordinal, behavior, generation, payload, created_at) SELECT ?1, ?2, COALESCE(MAX(ordinal), -1) + 1, ?3, ?4, ?5, ?6 FROM pending_messages WHERE thread_id = ?2",
        params![row.id, row.thread_id, row.behavior, row.generation, row.payload, row.created_at],
    )?;
    Ok(())
}

pub fn list(conn: &Connection, thread_id: &str) -> rusqlite::Result<Vec<PendingMessageRow>> {
    let mut query = conn.prepare("SELECT id, thread_id, ordinal, behavior, generation, payload, created_at FROM pending_messages WHERE thread_id = ?1 AND state != 'delivered' ORDER BY ordinal")?;
    query.query_map([thread_id], read_row)?.collect()
}

pub fn count(conn: &Connection, thread_id: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM pending_messages WHERE thread_id = ?1 AND state != 'delivered'",
        [thread_id],
        |row| row.get(0),
    )
}

pub fn list_metadata(
    conn: &Connection,
    thread_id: &str,
) -> rusqlite::Result<Vec<PendingMessageRow>> {
    let mut query = conn.prepare("SELECT id, thread_id, ordinal, behavior, generation, '', created_at FROM pending_messages WHERE thread_id = ?1 AND state != 'delivered' ORDER BY ordinal")?;
    query.query_map([thread_id], read_row)?.collect()
}

pub fn find(
    conn: &Connection,
    thread_id: &str,
    id: &str,
) -> rusqlite::Result<Option<PendingMessageRow>> {
    conn.query_row("SELECT id, thread_id, ordinal, behavior, generation, payload, created_at FROM pending_messages WHERE thread_id = ?1 AND id = ?2 AND state != 'delivered'", params![thread_id, id], read_row).optional()
}

pub fn next_steer(
    conn: &Connection,
    thread_id: &str,
    generation: &str,
) -> rusqlite::Result<Option<PendingMessageRow>> {
    conn.query_row("SELECT id, thread_id, ordinal, behavior, generation, payload, created_at FROM pending_messages WHERE thread_id = ?1 AND generation = ?2 AND behavior = 'steer' AND state = 'pending' ORDER BY ordinal LIMIT 1", params![thread_id, generation], read_row).optional()
}

pub fn next_steer_metadata(
    conn: &Connection,
    thread_id: &str,
    generation: &str,
) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT id FROM pending_messages WHERE thread_id=?1 AND generation=?2 AND behavior='steer' AND state='pending' ORDER BY ordinal LIMIT 1", params![thread_id,generation], |row| row.get(0)).optional()
}

pub fn promote(
    conn: &Connection,
    thread_id: &str,
    id: &str,
    generation: &str,
) -> rusqlite::Result<bool> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::PendingMessages,
    )?;
    Ok(conn.execute("UPDATE pending_messages SET behavior = 'steer', generation = ?3 WHERE thread_id = ?1 AND id = ?2 AND state = 'pending'", params![thread_id, id, generation])? == 1)
}

pub fn preserve_model_input(
    conn: &Connection,
    thread_id: &str,
    id: &str,
    content: &str,
) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::PendingMessages,
    )?;
    conn.execute("UPDATE pending_messages SET model_content = ?3 WHERE thread_id = ?1 AND id = ?2 AND state = 'delivered'", params![thread_id, id, content])?;
    Ok(())
}

pub fn claim_steer(
    conn: &Connection,
    thread_id: &str,
    id: &str,
    generation: &str,
) -> rusqlite::Result<bool> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::PendingMessages,
    )?;
    Ok(conn.execute("UPDATE pending_messages SET state = 'claimed' WHERE thread_id = ?1 AND id = ?2 AND generation = ?3 AND behavior = 'steer' AND state = 'pending'", params![thread_id, id, generation])? == 1)
}

pub fn mark_delivered(conn: &Connection, thread_id: &str, id: &str) -> rusqlite::Result<bool> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::PendingMessages,
    )?;
    Ok(conn.execute("UPDATE pending_messages SET state = 'delivered', payload = '' WHERE thread_id = ?1 AND id = ?2 AND state != 'delivered'", params![thread_id, id])? == 1)
}

pub fn mark_accepted(
    conn: &Connection,
    thread_id: &str,
    id: &str,
    previous_assistant_id: &str,
) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE pending_messages SET state = 'delivered', payload = '', accepted_after_assistant_id = ?3 \
         WHERE thread_id = ?1 AND id = ?2 AND state != 'delivered' \
         AND EXISTS(SELECT 1 FROM messages WHERE id = ?3 AND thread_id = ?1 AND role = 'assistant' AND status = 'streaming')",
        params![thread_id, id, previous_assistant_id],
    )? == 1)
}

pub fn remove(conn: &Connection, thread_id: &str, id: &str) -> rusqlite::Result<bool> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::PendingMessages,
    )?;
    Ok(conn.execute(
        "DELETE FROM pending_messages WHERE thread_id = ?1 AND id = ?2 AND state = 'pending'",
        params![thread_id, id],
    )? == 1)
}

pub fn release_claims(conn: &Connection, thread_id: &str) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::PendingMessages,
    )?;
    conn.execute(
        "UPDATE pending_messages SET state = 'pending' WHERE thread_id = ?1 AND state = 'claimed'",
        [thread_id],
    )?;
    Ok(())
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PendingMessageRow> {
    Ok(PendingMessageRow {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        ordinal: row.get(2)?,
        behavior: row.get(3)?,
        generation: row.get(4)?,
        payload: row.get(5)?,
        created_at: row.get(6)?,
    })
}
