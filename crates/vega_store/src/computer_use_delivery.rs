use rusqlite::{Connection, OptionalExtension, params};

pub fn activate_builtin(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
    name: &str,
    version: u32,
    digest: &str,
) -> rusqlite::Result<()> {
    if name != "vega-cua"
        || version == 0
        || digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let changed = conn.execute("INSERT OR IGNORE INTO builtin_skill_activations (message_id,name,version,content_sha256) SELECT id,?3,?4,?5 FROM messages WHERE id=?1 AND thread_id=?2 AND role='assistant' AND status='streaming'",params![message_id,thread_id,name,version,digest])?;
    if changed == 0 {
        let valid: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM builtin_skill_activations a JOIN messages m ON m.id=a.message_id WHERE a.message_id=?1 AND m.thread_id=?2 AND a.name=?3 AND a.version=?4 AND a.content_sha256=?5)",params![message_id,thread_id,name,version,digest],|row|row.get(0))?;
        if !valid {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    Ok(())
}

pub fn insert_screenshot(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
    call_id: &str,
    bytes: &[u8],
) -> rusqlite::Result<usize> {
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let tx = conn.unchecked_transaction()?;
    let prior: Option<(String,i64,Vec<u8>)> = tx.query_row("SELECT s.message_id,s.ordinal,a.encoded FROM computer_screenshot_artifacts s JOIN image_attachments a ON a.message_id=s.message_id AND a.ordinal=s.ordinal JOIN messages m ON m.id=s.message_id WHERE s.call_id=?1 AND m.thread_id=?2",params![call_id,thread_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
    if let Some((owner, ordinal, encoded)) = prior {
        if owner != message_id || encoded != bytes {
            return Err(rusqlite::Error::InvalidQuery);
        }
        return usize::try_from(ordinal).map_err(|_| rusqlite::Error::InvalidQuery);
    }
    let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM tool_calls t JOIN messages m ON m.id=t.message_id WHERE t.id=?1 AND t.message_id=?2 AND t.thread_id=?3 AND m.thread_id=?3 AND m.role='assistant' AND m.kind='text' AND m.status='streaming' AND t.tool='ComputerScreenshot' AND t.status='running')",params![call_id,message_id,thread_id],|row|row.get(0))?;
    if !valid {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let (count,total): (i64,i64) = tx.query_row("SELECT COUNT(*),COALESCE(SUM(length(encoded)),0) FROM image_attachments WHERE message_id=?1",[message_id],|row|Ok((row.get(0)?,row.get(1)?)))?;
    if count >= 4 || total + bytes.len() as i64 > 16 * 1024 * 1024 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    tx.execute(
        "INSERT INTO image_attachments (message_id,ordinal,encoded) VALUES (?1,?2,?3)",
        params![message_id, count, bytes],
    )?;
    tx.execute(
        "INSERT INTO computer_screenshot_artifacts (call_id,message_id,ordinal) VALUES (?1,?2,?3)",
        params![call_id, message_id, count],
    )?;
    tx.commit()?;
    usize::try_from(count).map_err(|_| rusqlite::Error::InvalidQuery)
}
