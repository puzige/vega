use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedNoticeRow {
    pub message_id: String,
    pub text_start_bytes: usize,
    pub text_end_bytes: usize,
    pub kind: String,
    pub turn_limit: Option<u32>,
    pub delivered_images: Option<bool>,
}

impl GeneratedNoticeRow {
    pub fn canonical_text(&self) -> Option<String> {
        match (self.kind.as_str(), self.turn_limit, self.delivered_images) {
            ("turn_limit", Some(limit), None) if limit > 0 => Some(format!(
                "Agent turn limit ({limit}) reached; stopping without issuing another request."
            )),
            ("computer_stopped", None, Some(true)) => Some("已有截图保留在对话中。后续电脑操作连续未取得进展，已停止重复操作。请根据最后一个工具结果处理尚未完成的工作。".to_string()),
            ("computer_stopped", None, Some(false)) => Some("电脑操作连续未取得进展，已停止本轮自动操作。请根据对话中的错误修正目标或参数后重新发起；不会继续重复点击或换用脚本。".to_string()),
            _ => None,
        }
    }

    pub fn is_valid_for(&self, content: &str) -> bool {
        self.text_start_bytes < self.text_end_bytes
            && self.canonical_text().is_some_and(|expected| {
                content.get(self.text_start_bytes..self.text_end_bytes) == Some(expected.as_str())
            })
    }
}

pub fn record_generated_notice(
    conn: &Connection,
    message_id: &str,
    content: &str,
    notice: &GeneratedNoticeRow,
) -> Result<bool, rusqlite::Error> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Messages,
    )?;
    if notice.message_id != message_id || !notice.is_valid_for(content) {
        return Ok(false);
    }
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Messages,
    )?;
    let Some(owner) = find(&tx, message_id)? else {
        return Ok(false);
    };
    if owner.role != "assistant" || owner.content != content {
        return Ok(false);
    }
    let existing = read_for_message(&tx, message_id)?;
    validate_notices(&owner, &existing)?;
    if let Some(previous) = existing
        .iter()
        .find(|row| row.text_start_bytes == notice.text_start_bytes)
    {
        return Ok(previous == notice);
    }
    if owner.status != "streaming"
        || existing.iter().any(|previous| {
            notice.text_start_bytes < previous.text_end_bytes
                && previous.text_start_bytes < notice.text_end_bytes
        })
    {
        return Ok(false);
    }
    let (Ok(start), Ok(end)) = (
        i64::try_from(notice.text_start_bytes),
        i64::try_from(notice.text_end_bytes),
    ) else {
        return Ok(false);
    };
    tx.execute(
        "INSERT INTO generated_notices (message_id, text_start_bytes, text_end_bytes, kind, turn_limit, delivered_images) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            message_id,
            start,
            end,
            notice.kind,
            notice.turn_limit.map(i64::from),
            notice.delivered_images.map(i64::from),
        ],
    )?;
    tx.commit()?;
    Ok(true)
}

fn row_from_query(row: &rusqlite::Row<'_>) -> Result<GeneratedNoticeRow, rusqlite::Error> {
    let start: i64 = row.get(1)?;
    let end: i64 = row.get(2)?;
    let limit: Option<i64> = row.get(4)?;
    let images: Option<i64> = row.get(5)?;
    let turn_limit = limit
        .map(|limit| u32::try_from(limit).map_err(|_| corrupt_row_error("generated notice limit")))
        .transpose()?;
    let delivered_images = match images {
        None => None,
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => return Err(corrupt_row_error("generated notice images")),
    };
    Ok(GeneratedNoticeRow {
        message_id: row.get(0)?,
        text_start_bytes: usize::try_from(start)
            .map_err(|_| corrupt_row_error("generated notice start"))?,
        text_end_bytes: usize::try_from(end)
            .map_err(|_| corrupt_row_error("generated notice end"))?,
        kind: row.get(3)?,
        turn_limit,
        delivered_images,
    })
}

fn read_for_message(
    conn: &Connection,
    message_id: &str,
) -> Result<Vec<GeneratedNoticeRow>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT message_id, text_start_bytes, text_end_bytes, kind, turn_limit, delivered_images FROM generated_notices WHERE message_id = ?1 ORDER BY text_start_bytes")?;
    stmt.query_map([message_id], row_from_query)?.collect()
}

fn validate_notices(
    owner: &MessageRow,
    notices: &[GeneratedNoticeRow],
) -> Result<(), rusqlite::Error> {
    let mut end = 0;
    for notice in notices {
        if owner.role != "assistant"
            || notice.message_id != owner.id
            || !notice.is_valid_for(&owner.content)
            || notice.text_start_bytes < end
        {
            return Err(corrupt_row_error("generated notice provenance"));
        }
        end = notice.text_end_bytes;
    }
    Ok(())
}

pub(crate) fn copy_generated_notices_in_transaction(
    tx: &Transaction<'_>,
    source_message_id: &str,
    destination_message_id: &str,
) -> Result<(), rusqlite::Error> {
    let _permit =
        crate::write_coordination::acquire(tx, crate::write_coordination::WriteCategory::Messages)?;
    let notices = read_for_message(tx, source_message_id)?;
    if notices.is_empty() {
        return Ok(());
    }
    let source = find(tx, source_message_id)?
        .ok_or_else(|| corrupt_row_error("generated notice source owner"))?;
    validate_notices(&source, &notices)?;
    let destination = find(tx, destination_message_id)?
        .ok_or_else(|| corrupt_row_error("generated notice destination owner"))?;
    if destination.role != "assistant" || destination.content != source.content {
        return Err(corrupt_row_error("generated notice destination provenance"));
    }
    let copied = tx.execute(
        "INSERT INTO generated_notices (message_id, text_start_bytes, text_end_bytes, kind, turn_limit, delivered_images) SELECT ?1, text_start_bytes, text_end_bytes, kind, turn_limit, delivered_images FROM generated_notices WHERE message_id = ?2 ORDER BY text_start_bytes",
        params![destination_message_id, source_message_id],
    )?;
    if copied != notices.len() {
        return Err(corrupt_row_error("generated notice copy count"));
    }
    Ok(())
}

pub fn remap_generated_notices_in_transaction<F>(
    tx: &Transaction<'_>,
    thread_id: &str,
    mut redact: F,
) -> Result<(), rusqlite::Error>
where
    F: FnMut(&str, &[usize]) -> (String, Vec<usize>),
{
    let _permit =
        crate::write_coordination::acquire(tx, crate::write_coordination::WriteCategory::Messages)?;
    let mut stmt = tx.prepare("SELECT id FROM messages WHERE thread_id = ?1 AND EXISTS (SELECT 1 FROM generated_notices WHERE message_id = messages.id) ORDER BY seq")?;
    let owners = stmt
        .query_map([thread_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for message_id in owners {
        let owner =
            find(tx, &message_id)?.ok_or_else(|| corrupt_row_error("generated notice owner"))?;
        let notices = read_for_message(tx, &message_id)?;
        validate_notices(&owner, &notices)?;
        let mut remapped = Vec::new();
        let mut previous_end = 0;
        for mut notice in notices {
            let (content, offsets) = redact(
                &owner.content,
                &[notice.text_start_bytes, notice.text_end_bytes],
            );
            let [start, end] = offsets.as_slice() else {
                return Err(corrupt_row_error("generated notice projection offsets"));
            };
            notice.text_start_bytes = *start;
            notice.text_end_bytes = *end;
            if notice.is_valid_for(&content) && notice.text_start_bytes >= previous_end {
                previous_end = notice.text_end_bytes;
                remapped.push(notice);
            }
        }
        tx.execute(
            "DELETE FROM generated_notices WHERE message_id = ?1",
            [&message_id],
        )?;
        for notice in remapped {
            let start = i64::try_from(notice.text_start_bytes)
                .map_err(|_| corrupt_row_error("generated notice projected start"))?;
            let end = i64::try_from(notice.text_end_bytes)
                .map_err(|_| corrupt_row_error("generated notice projected end"))?;
            tx.execute(
                "INSERT INTO generated_notices (message_id, text_start_bytes, text_end_bytes, kind, turn_limit, delivered_images) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    message_id,
                    start,
                    end,
                    notice.kind,
                    notice.turn_limit.map(i64::from),
                    notice.delivered_images.map(i64::from),
                ],
            )?;
        }
    }
    Ok(())
}

pub(super) fn for_page(
    conn: &Connection,
    thread_id: &str,
    rows: &[MessageRow],
) -> Result<Vec<GeneratedNoticeRow>, rusqlite::Error> {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare("SELECT n.message_id, n.text_start_bytes, n.text_end_bytes, n.kind, n.turn_limit, n.delivered_images FROM generated_notices n JOIN messages m ON m.id = n.message_id WHERE m.thread_id = ?1 AND m.seq >= ?2 AND m.seq <= ?3 AND m.status != 'streaming' ORDER BY m.seq, n.text_start_bytes")?;
    let candidates = stmt
        .query_map(params![thread_id, first.seq, last.seq], row_from_query)?
        .collect::<Result<Vec<_>, _>>()?;
    for notice in &candidates {
        if !rows.iter().any(|owner| owner.id == notice.message_id) {
            return Err(corrupt_row_error("generated notice owner"));
        }
    }
    for owner in rows {
        let notices: Vec<_> = candidates
            .iter()
            .filter(|notice| notice.message_id == owner.id)
            .cloned()
            .collect();
        validate_notices(owner, &notices)?;
    }
    Ok(candidates)
}
