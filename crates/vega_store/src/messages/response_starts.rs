use super::*;

pub fn record_assistant_response_start(
    conn: &Connection,
    message_id: &str,
    response_id: &str,
    text_start_bytes: usize,
    started_at_ms: i64,
) -> rusqlite::Result<i64> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Messages,
    )?;
    let tx = conn
        .is_autocommit()
        .then(|| {
            crate::write_coordination::WriteTransaction::new(
                conn,
                crate::write_coordination::WriteCategory::Messages,
            )
        })
        .transpose()?;
    let invalid = || rusqlite::Error::InvalidParameterName("invalid response start".into());
    if response_id.is_empty()
        || response_id.len() > 128
        || response_id.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    let start = i64::try_from(text_start_bytes).map_err(|_| invalid())?;
    let existing: Option<(i64, i64)> = conn
        .query_row(
            "SELECT text_start_bytes, started_at_ms FROM assistant_response_starts WHERE message_id=?1 AND response_id=?2",
            params![message_id, response_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((previous_start, previous_time)) = existing {
        return if previous_start == start {
            Ok(previous_time)
        } else {
            Err(invalid())
        };
    }
    let owner = find(conn, message_id)?.ok_or_else(invalid)?;
    if owner.role != "assistant"
        || owner.status != "streaming"
        || text_start_bytes != owner.content.len()
    {
        return Err(invalid());
    }
    conn.execute(
        "INSERT INTO assistant_response_starts(message_id,response_id,ordinal,text_start_bytes,started_at_ms) SELECT ?1,?2,COALESCE(MAX(ordinal),-1)+1,?3,?4 FROM assistant_response_starts WHERE message_id=?1",
        params![message_id, response_id, start, started_at_ms],
    )?;
    if let Some(tx) = tx {
        tx.commit()?;
    }
    Ok(started_at_ms)
}

pub fn last_assistant_response_start(
    conn: &Connection,
    message_id: &str,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT started_at_ms FROM assistant_response_starts WHERE message_id=?1 ORDER BY ordinal DESC LIMIT 1",
        [message_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn rollback_assistant_response_starts(
    conn: &Connection,
    message_id: &str,
    text_start_bytes: usize,
    response_id: Option<&str>,
) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Messages,
    )?;
    let start = i64::try_from(text_start_bytes)
        .map_err(|_| rusqlite::Error::InvalidParameterName("invalid response rollback".into()))?;
    conn.execute(
        "DELETE FROM assistant_responses WHERE message_id=?1 AND (text_end_bytes>?2 OR response_id=?3 OR response_id IN (SELECT response_id FROM assistant_response_starts WHERE message_id=?1 AND (text_start_bytes>?2 OR ordinal>(SELECT ordinal FROM assistant_response_starts WHERE message_id=?1 AND response_id=?3))))",
        params![message_id, start, response_id],
    )?;
    conn.execute(
        "DELETE FROM assistant_response_starts WHERE message_id=?1 AND (text_start_bytes>?2 OR ordinal>(SELECT ordinal FROM assistant_response_starts WHERE message_id=?1 AND response_id=?3))",
        params![message_id, start, response_id],
    )?;
    conn.execute(
        "UPDATE message_timestamp_boundaries SET text_start_bytes=?2 WHERE message_id=?1 AND text_start_bytes>?2",
        params![message_id, start],
    )?;
    Ok(())
}

pub(super) fn for_page(
    conn: &Connection,
    thread_id: &str,
    rows: &[MessageRow],
) -> rusqlite::Result<std::collections::HashMap<String, Option<i64>>> {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(Default::default());
    };
    let mut query = conn.prepare(
        "SELECT r.message_id,r.started_at_ms,r.text_start_bytes FROM assistant_response_starts r JOIN messages m ON m.id=r.message_id WHERE m.thread_id=?1 AND m.seq>=?2 AND m.seq<=?3 AND r.ordinal=(SELECT MAX(last.ordinal) FROM assistant_response_starts last WHERE last.message_id=r.message_id)",
    )?;
    let clocks = query.query_map(params![thread_id, first.seq, last.seq], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut result = std::collections::HashMap::new();
    for clock in clocks {
        let (id, time, offset) = clock?;
        if let Some(owner) = rows.iter().find(|row| row.id == id) {
            let valid = owner.role == "assistant"
                && usize::try_from(offset)
                    .is_ok_and(|offset| owner.content.is_char_boundary(offset));
            result.insert(id, valid.then_some(time));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
