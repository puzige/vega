use super::*;
use rusqlite::types::Value;
use std::collections::{BTreeMap, BTreeSet};

type RedactionCache = BTreeMap<String, (String, BTreeMap<usize, usize>)>;

#[derive(Clone)]
struct PreparedUpdate {
    sql: &'static str,
    values: Vec<Value>,
    expected: usize,
}

#[derive(Clone)]
pub struct PreparedLegacyRedaction {
    thread_id: String,
    source: ContextSourceVersion,
    metadata: String,
    updates: Vec<PreparedUpdate>,
    changed: bool,
}

impl PreparedLegacyRedaction {
    pub(crate) fn belongs_to(&self, thread: &str) -> bool {
        self.thread_id == thread
    }

    pub fn validate(&self, transaction: &Transaction<'_>) -> rusqlite::Result<bool> {
        Ok(
            load_source_version_in_transaction(transaction, &self.thread_id)? == self.source
                && metadata_fingerprint(transaction, &self.thread_id)? == self.metadata,
        )
    }

    pub fn install(self, transaction: &mut Transaction<'_>) -> rusqlite::Result<bool> {
        if !self.validate(transaction)? {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let savepoint = transaction.savepoint()?;
        let changed = self.install_preparation_updates(&savepoint)?;
        savepoint.commit()?;
        Ok(changed)
    }

    pub(crate) fn install_preparation_updates(
        &self,
        transaction: &Connection,
    ) -> rusqlite::Result<bool> {
        for update in &self.updates {
            if transaction.execute(update.sql, rusqlite::params_from_iter(&update.values))?
                != update.expected
            {
                return Err(rusqlite::Error::InvalidQuery);
            }
        }
        Ok(self.changed)
    }
}

pub fn prepare_legacy_thread_redaction<F>(
    conn: &Connection,
    thread_id: &str,
    mut redact: F,
) -> rusqlite::Result<PreparedLegacyRedaction>
where
    F: FnMut(&str, &[usize]) -> (String, Vec<usize>),
{
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Deferred)?;
    let source = load_source_version_in_transaction(&transaction, thread_id)?;
    let metadata = metadata_fingerprint(&transaction, thread_id)?;
    let mut inputs = BTreeMap::<String, BTreeSet<usize>>::new();
    let mut prepare = |text: &str, offsets: Vec<usize>| {
        inputs.entry(text.to_owned()).or_default().extend(offsets);
    };
    let texts = {
        let mut query = transaction.prepare(
            "SELECT output_text FROM tool_calls WHERE thread_id=?1 AND output_text IS NOT NULL UNION ALL SELECT summary FROM context_checkpoints WHERE thread_id=?1",
        )?;
        query
            .query_map([thread_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for text in texts {
        prepare(&text, Vec::new());
    }
    prepare(REDACTED_HISTORY_RESULT, Vec::new());
    let owners = {
        let mut query = transaction.prepare(
            "SELECT id,content,role,status FROM messages WHERE thread_id=?1 AND ((role='assistant' AND status<>'streaming') OR EXISTS(SELECT 1 FROM generated_notices WHERE message_id=messages.id)) ORDER BY seq,id",
        )?;
        query
            .query_map([thread_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (id, text, role, status) in &owners {
        if role == "assistant" && status != "streaming" {
            prepare(text, Vec::new());
            let mut query = transaction.prepare(
                "SELECT text_offset_bytes FROM tool_calls WHERE thread_id=?1 AND message_id=?2 AND text_offset_bytes IS NOT NULL",
            )?;
            let offsets = query
                .query_map(params![thread_id, id], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            prepare(text, checked_offsets(text, offsets)?);
            for table in ["message_timestamp_boundaries", "assistant_response_starts"] {
                let mut query = transaction.prepare(&format!(
                    "SELECT text_start_bytes FROM {table} WHERE message_id=?1"
                ))?;
                let offsets = query
                    .query_map([id], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                prepare(text, checked_offsets(text, offsets)?);
            }
        }
        let mut query = transaction.prepare(
            "SELECT text_start_bytes,text_end_bytes FROM generated_notices WHERE message_id=?1 ORDER BY text_start_bytes",
        )?;
        let notices = query
            .query_map([id], |row| {
                Ok(vec![row.get::<_, i64>(0)?, row.get::<_, i64>(1)?])
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for offsets in notices {
            prepare(text, checked_offsets(text, offsets)?);
        }
    }
    let mut cache = RedactionCache::new();
    for (text, offsets) in inputs {
        let offsets = offsets.into_iter().collect::<Vec<_>>();
        let (projected, mapped) = redact(&text, &offsets);
        if mapped.len() != offsets.len()
            || mapped
                .iter()
                .any(|offset| !projected.is_char_boundary(*offset))
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        cache.insert(text, (projected, offsets.into_iter().zip(mapped).collect()));
    }
    let mut updates = Vec::new();
    let mut changed = false;
    let mut query = transaction.prepare(
        "SELECT id,tool,status,output_text FROM tool_calls WHERE thread_id=?1 AND output_text IS NOT NULL",
    )?;
    let mut rows = query.query([thread_id])?;
    while let Some(row) = rows.next()? {
        let text: String = row.get(3)?;
        let mut projected = projection(&cache, &text)?.0;
        if projected != &text {
            let tool: String = row.get(1)?;
            let status: String = row.get(2)?;
            if tool == "read_conversation" && status == "success" {
                projected = projection(&cache, REDACTED_HISTORY_RESULT)?.0;
            }
            updates.push(PreparedUpdate {
                sql: "UPDATE tool_calls SET output_text=?1 WHERE id=?2 AND thread_id=?3",
                values: vec![
                    projected.clone().into(),
                    row.get::<_, String>(0)?.into(),
                    thread_id.to_owned().into(),
                ],
                expected: 1,
            });
            changed = true;
        }
    }
    drop(rows);
    drop(query);
    for (id, text, role, status) in owners {
        let (projected, mapping) = projection(&cache, &text)?;
        prepare_notices(
            &transaction,
            &mut updates,
            &id,
            &text,
            &role,
            projected,
            mapping,
        )?;
        if role != "assistant" || status == "streaming" || projected == &text {
            continue;
        }
        for (select, update) in [
            (
                "SELECT id,text_offset_bytes FROM tool_calls WHERE message_id=?1 AND text_offset_bytes IS NOT NULL",
                "UPDATE tool_calls SET text_offset_bytes=?1 WHERE id=?2",
            ),
            (
                "SELECT operation_key,text_start_bytes FROM message_timestamp_boundaries WHERE message_id=?1",
                "UPDATE message_timestamp_boundaries SET text_start_bytes=?1 WHERE operation_key=?2 AND message_id=?3",
            ),
            (
                "SELECT response_id,text_start_bytes FROM assistant_response_starts WHERE message_id=?1",
                "UPDATE assistant_response_starts SET text_start_bytes=?1 WHERE response_id=?2 AND message_id=?3",
            ),
        ] {
            let mut query = transaction.prepare(select)?;
            let mut rows = query.query([&id])?;
            while let Some(row) = rows.next()? {
                let mut values = vec![
                    mapped_offset(mapping, row.get(1)?)?.into(),
                    row.get::<_, String>(0)?.into(),
                ];
                if select
                    != "SELECT id,text_offset_bytes FROM tool_calls WHERE message_id=?1 AND text_offset_bytes IS NOT NULL"
                {
                    values.push(id.clone().into());
                }
                updates.push(PreparedUpdate {
                    sql: update,
                    values,
                    expected: 1,
                });
            }
        }
        updates.push(PreparedUpdate {
            sql: "UPDATE messages SET content=?1 WHERE id=?2 AND thread_id=?3 AND role='assistant' AND status<>'streaming'",
            values: vec![projected.clone().into(), id.into(), thread_id.to_owned().into()],
            expected: 1,
        });
        changed = true;
    }
    let mut query =
        transaction.prepare("SELECT id,summary FROM context_checkpoints WHERE thread_id=?1")?;
    let mut rows = query.query([thread_id])?;
    while let Some(row) = rows.next()? {
        let text: String = row.get(1)?;
        let projected = projection(&cache, &text)?.0;
        if projected != &text {
            updates.push(PreparedUpdate {
                sql: "UPDATE context_checkpoints SET summary=?1 WHERE id=?2 AND thread_id=?3",
                values: vec![
                    projected.clone().into(),
                    row.get::<_, i64>(0)?.into(),
                    thread_id.to_owned().into(),
                ],
                expected: 1,
            });
            changed = true;
        }
    }
    drop(rows);
    drop(query);
    transaction.commit()?;
    Ok(PreparedLegacyRedaction {
        thread_id: thread_id.to_owned(),
        source,
        metadata,
        updates,
        changed,
    })
}

fn projection<'a>(
    cache: &'a RedactionCache,
    text: &str,
) -> rusqlite::Result<(&'a String, &'a BTreeMap<usize, usize>)> {
    cache
        .get(text)
        .map(|(text, offsets)| (text, offsets))
        .ok_or(rusqlite::Error::InvalidQuery)
}

fn mapped_offset(mapping: &BTreeMap<usize, usize>, offset: i64) -> rusqlite::Result<i64> {
    usize::try_from(offset)
        .ok()
        .and_then(|offset| mapping.get(&offset))
        .and_then(|offset| i64::try_from(*offset).ok())
        .ok_or(rusqlite::Error::InvalidQuery)
}

fn prepare_notices(
    transaction: &Transaction<'_>,
    updates: &mut Vec<PreparedUpdate>,
    id: &str,
    text: &str,
    role: &str,
    projected: &str,
    mapping: &BTreeMap<usize, usize>,
) -> rusqlite::Result<()> {
    let mut query = transaction.prepare("SELECT text_start_bytes,text_end_bytes,kind,turn_limit,delivered_images FROM generated_notices WHERE message_id=?1 ORDER BY text_start_bytes")?;
    let mut rows = query.query([id])?;
    let mut previous_end = 0;
    let mut projected_end = 0;
    let mut remapped = Vec::new();
    while let Some(row) = rows.next()? {
        let start: i64 = row.get(0)?;
        let end: i64 = row.get(1)?;
        let mut notice = crate::messages::GeneratedNoticeRow {
            message_id: id.to_owned(),
            text_start_bytes: usize::try_from(start).map_err(|_| rusqlite::Error::InvalidQuery)?,
            text_end_bytes: usize::try_from(end).map_err(|_| rusqlite::Error::InvalidQuery)?,
            kind: row.get(2)?,
            turn_limit: row
                .get::<_, Option<i64>>(3)?
                .map(u32::try_from)
                .transpose()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            delivered_images: match row.get::<_, Option<i64>>(4)? {
                None => None,
                Some(0) => Some(false),
                Some(1) => Some(true),
                _ => return Err(rusqlite::Error::InvalidQuery),
            },
        };
        if role != "assistant"
            || !notice.is_valid_for(text)
            || notice.text_start_bytes < previous_end
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        previous_end = notice.text_end_bytes;
        notice.text_start_bytes = usize::try_from(mapped_offset(mapping, start)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        notice.text_end_bytes = usize::try_from(mapped_offset(mapping, end)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        if notice.is_valid_for(projected) && notice.text_start_bytes >= projected_end {
            projected_end = notice.text_end_bytes;
            remapped.push(notice);
        }
    }
    if previous_end != 0 {
        updates.push(PreparedUpdate {
            sql: "DELETE FROM generated_notices WHERE message_id=?1",
            values: vec![id.to_owned().into()],
            expected: usize::try_from(transaction.query_row(
                "SELECT COUNT(*) FROM generated_notices WHERE message_id=?1",
                [id],
                |row| row.get::<_, i64>(0),
            )?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        });
        for notice in remapped {
            updates.push(PreparedUpdate {
                sql: "INSERT INTO generated_notices(message_id,text_start_bytes,text_end_bytes,kind,turn_limit,delivered_images) VALUES(?1,?2,?3,?4,?5,?6)",
                values: vec![id.to_owned().into(), i64::try_from(notice.text_start_bytes).map_err(|_| rusqlite::Error::InvalidQuery)?.into(), i64::try_from(notice.text_end_bytes).map_err(|_| rusqlite::Error::InvalidQuery)?.into(), notice.kind.into(), notice.turn_limit.map(i64::from).into(), notice.delivered_images.map(i64::from).into()],
                expected: 1,
            });
        }
    }
    Ok(())
}

fn checked_offsets(text: &str, offsets: Vec<i64>) -> rusqlite::Result<Vec<usize>> {
    offsets
        .into_iter()
        .map(|offset| {
            usize::try_from(offset)
                .ok()
                .filter(|offset| text.is_char_boundary(*offset))
                .ok_or(rusqlite::Error::InvalidQuery)
        })
        .collect()
}

fn metadata_fingerprint(conn: &Connection, thread_id: &str) -> rusqlite::Result<String> {
    let mut digest = Sha256::new();
    for sql in [
        "SELECT json_array(id,summary) FROM context_checkpoints WHERE thread_id=?1 ORDER BY id",
        "SELECT json_array(b.message_id,b.operation_key,b.text_start_bytes) FROM message_timestamp_boundaries b JOIN messages m ON m.id=b.message_id WHERE m.thread_id=?1 ORDER BY b.message_id,b.operation_key",
        "SELECT json_array(b.message_id,b.response_id,b.text_start_bytes) FROM assistant_response_starts b JOIN messages m ON m.id=b.message_id WHERE m.thread_id=?1 ORDER BY b.message_id,b.response_id",
        "SELECT json_array(n.message_id,n.text_start_bytes,n.text_end_bytes,n.kind,n.turn_limit,n.delivered_images) FROM generated_notices n JOIN messages m ON m.id=n.message_id WHERE m.thread_id=?1 ORDER BY n.message_id,n.text_start_bytes",
    ] {
        digest.update((sql.len() as u64).to_le_bytes());
        digest.update(sql.as_bytes());
        let mut query = conn.prepare(sql)?;
        let mut rows = query.query([thread_id])?;
        while let Some(row) = rows.next()? {
            let field = row.get_ref(0)?;
            let value = field.as_str()?;
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}
