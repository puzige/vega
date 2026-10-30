use super::*;

pub fn record_timestamp_boundary(
    conn: &Connection,
    message_id: &str,
    operation_key: &str,
    text_start_bytes: usize,
) -> rusqlite::Result<usize> {
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
    let invalid = || rusqlite::Error::InvalidParameterName("invalid timestamp boundary".into());
    if operation_key.is_empty() {
        return Err(invalid());
    }
    let existing: Option<i64> = conn.query_row(
        "SELECT text_start_bytes FROM message_timestamp_boundaries WHERE message_id=?1 AND operation_key=?2",
        params![message_id, operation_key],
        |row| row.get(0),
    ).optional()?;
    if let Some(offset) = existing {
        return usize::try_from(offset).map_err(|_| invalid());
    }
    let owner = find(conn, message_id)?.ok_or_else(invalid)?;
    if !matches!(owner.role.as_str(), "assistant" | "user")
        || !owner.content.is_char_boundary(text_start_bytes)
    {
        return Err(invalid());
    }
    let offset = i64::try_from(text_start_bytes).map_err(|_| invalid())?;
    conn.execute(
        "INSERT INTO message_timestamp_boundaries(message_id,operation_key,text_start_bytes) VALUES(?1,?2,?3)",
        params![message_id, operation_key, offset],
    )?;
    if let Some(tx) = tx {
        tx.commit()?;
    }
    Ok(text_start_bytes)
}

pub fn timestamp_break_before(conn: &Connection, message_id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages m JOIN thread_timestamp_fork_prefixes f ON f.thread_id=m.thread_id WHERE m.id=?1 AND m.seq>f.prefix_seq AND NOT EXISTS(SELECT 1 FROM messages earlier WHERE earlier.thread_id=m.thread_id AND earlier.seq>f.prefix_seq AND earlier.seq<m.seq))",
        [message_id],
        |row| row.get(0),
    )
}

pub(super) fn for_page(
    conn: &Connection,
    thread_id: &str,
    rows: &[MessageRow],
) -> rusqlite::Result<std::collections::HashMap<String, Vec<usize>>> {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(Default::default());
    };
    let mut query = conn.prepare(
        "SELECT b.message_id,b.text_start_bytes FROM message_timestamp_boundaries b JOIN messages m ON m.id=b.message_id WHERE m.thread_id=?1 AND m.seq>=?2 AND m.seq<=?3 ORDER BY m.seq,b.text_start_bytes,b.operation_key",
    )?;
    let boundaries = query.query_map(params![thread_id, first.seq, last.seq], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut result = std::collections::HashMap::<String, Vec<usize>>::new();
    for boundary in boundaries {
        let (id, offset) = boundary?;
        let offset = usize::try_from(offset).map_err(|_| rusqlite::Error::InvalidQuery)?;
        result.entry(id).or_default().push(offset);
    }
    Ok(result)
}

pub(super) fn breaks_for_page(
    conn: &Connection,
    thread_id: &str,
    rows: &[MessageRow],
) -> rusqlite::Result<Vec<String>> {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(Vec::new());
    };
    conn.prepare(
        "SELECT DISTINCT m.id FROM thread_timestamp_fork_prefixes f JOIN messages m ON m.thread_id=f.thread_id AND m.seq=(SELECT MIN(next.seq) FROM messages next WHERE next.thread_id=f.thread_id AND next.seq>f.prefix_seq) WHERE f.thread_id=?1 AND m.seq>=?2 AND m.seq<=?3 ORDER BY m.seq",
    )?.query_map(params![thread_id, first.seq, last.seq], |row| row.get(0))?.collect()
}

pub(crate) fn remap_timestamp_offsets(
    conn: &Connection,
    message_id: &str,
    content: &str,
    redact: &mut impl FnMut(&str, &[usize]) -> (String, Vec<usize>),
) -> rusqlite::Result<()> {
    for (table, key) in [
        ("message_timestamp_boundaries", "operation_key"),
        ("assistant_response_starts", "response_id"),
    ] {
        let mut query = conn.prepare(&format!(
            "SELECT {key},text_start_bytes FROM {table} WHERE message_id=?1"
        ))?;
        let stored = query
            .query_map([message_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let offsets = stored
            .iter()
            .map(|(_, offset)| {
                usize::try_from(*offset)
                    .ok()
                    .filter(|offset| content.is_char_boundary(*offset))
                    .ok_or(rusqlite::Error::InvalidQuery)
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let (projected, offsets) = redact(content, &offsets);
        if offsets.len() != stored.len() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        for ((id, _), offset) in stored.into_iter().zip(offsets) {
            if !projected.is_char_boundary(offset) {
                return Err(rusqlite::Error::InvalidQuery);
            }
            let offset = i64::try_from(offset).map_err(|_| rusqlite::Error::InvalidQuery)?;
            conn.execute(
                &format!("UPDATE {table} SET text_start_bytes=?3 WHERE message_id=?1 AND {key}=?2"),
                params![message_id, id, offset],
            )?;
        }
    }
    Ok(())
}

pub fn record_thread_tail_timestamp_boundary(
    conn: &Connection,
    thread_id: &str,
    operation_key: &str,
) -> rusqlite::Result<()> {
    let owner: Option<(String, i64)> = conn.query_row(
        "SELECT id,length(CAST(content AS BLOB)) FROM messages WHERE thread_id=?1 ORDER BY seq DESC LIMIT 1",
        [thread_id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    if let Some((message_id, offset)) = owner {
        let offset = usize::try_from(offset).map_err(|_| rusqlite::Error::InvalidQuery)?;
        record_timestamp_boundary(conn, &message_id, operation_key, offset)?;
    }
    Ok(())
}
