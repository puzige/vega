use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

pub fn exceptions(conn: &Connection, thread: &str) -> rusqlite::Result<Vec<(String, String)>> {
    conn.prepare(
        "SELECT rule_id, fingerprint FROM credential_privacy_exceptions WHERE thread_id = ?1",
    )?
    .query_map([thread], |row| Ok((row.get(0)?, row.get(1)?)))?
    .collect()
}

pub fn allow(
    conn: &Connection,
    thread: &str,
    rule: &str,
    fingerprint: &str,
) -> rusqlite::Result<()> {
    conn.execute("INSERT OR IGNORE INTO credential_privacy_exceptions(thread_id,rule_id,fingerprint) VALUES (?1,?2,?3)", params![thread,rule,fingerprint])?;
    Ok(())
}

pub fn revoke(
    conn: &Connection,
    thread: &str,
    rule: &str,
    fingerprint: &str,
) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM credential_privacy_exceptions WHERE thread_id=?1 AND rule_id=?2 AND fingerprint=?3", params![thread,rule,fingerprint])?;
    Ok(())
}

pub fn set_exception_checked(
    conn: &Connection,
    thread: &str,
    source_fingerprint: &str,
    rule: &str,
    fingerprint: &str,
    allowed: bool,
) -> rusqlite::Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let current = source_fingerprint_in_transaction(&tx, thread)?;
    if current != source_fingerprint {
        return Err(rusqlite::Error::InvalidParameterName(
            "conversation changed; review again".into(),
        ));
    }
    if allowed {
        allow(&tx, thread, rule, fingerprint)?;
    } else {
        revoke(&tx, thread, rule, fingerprint)?;
    }
    tx.commit()
}

pub fn fields(conn: &Connection, thread: &str) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut query = conn.prepare("SELECT id,'message',content FROM messages WHERE thread_id=?1 UNION ALL SELECT id,'tool.input',input_json FROM tool_calls WHERE thread_id=?1 UNION ALL SELECT id,'tool.output',output_text FROM tool_calls WHERE thread_id=?1 AND output_text IS NOT NULL UNION ALL SELECT CAST(id AS TEXT),'summary',summary FROM context_checkpoints WHERE thread_id=?1 UNION ALL SELECT id,'plan.note',plan_review_note FROM messages WHERE thread_id=?1 AND plan_review_note IS NOT NULL UNION ALL SELECT id,'pending.model',model_content FROM pending_messages WHERE thread_id=?1 AND model_content IS NOT NULL UNION ALL SELECT id,'pending.content',json_extract(payload,'$.content') FROM pending_messages WHERE thread_id=?1 AND payload != '' AND json_valid(payload) AND json_type(payload,'$.content')='text'")?;
    query
        .query_map([thread], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect()
}

pub fn source_fingerprint(conn: &Connection, thread: &str) -> rusqlite::Result<String> {
    let tx = conn.unchecked_transaction()?;
    source_fingerprint_in_transaction(&tx, thread)
}

pub fn source_fingerprint_in_transaction(
    conn: &rusqlite::Transaction<'_>,
    thread: &str,
) -> rusqlite::Result<String> {
    let source = crate::context_compaction::load_source_version_in_transaction(conn, thread)?;
    let mut values = fields(conn, thread)?;
    values.sort();
    let mut digest = Sha256::new();
    digest.update(source.fingerprint.as_bytes());
    for (id, field, text) in values {
        for value in [id, field, text] {
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn clean<F>(
    conn: &Connection,
    thread: &str,
    expected: &str,
    mut redact: F,
) -> rusqlite::Result<()>
where
    F: FnMut(&str, &[usize]) -> (String, Vec<usize>),
{
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let source = source_fingerprint_in_transaction(&tx, thread)?;
    let running: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id=?1 AND status='streaming')",
        [thread],
        |row| row.get(0),
    )?;
    if running || source != expected {
        return Err(rusqlite::Error::InvalidParameterName(
            "conversation changed; review again".into(),
        ));
    }
    let responses = {
        let mut query = tx.prepare("SELECT r.message_id,r.response_id,r.text_start_bytes,r.text_end_bytes,m.content FROM assistant_responses r JOIN messages m ON m.id=r.message_id WHERE m.thread_id=?1")?;
        query
            .query_map([thread], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (message, id, start, end, text) in responses {
        let start = usize::try_from(start).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let end = usize::try_from(end).map_err(|_| rusqlite::Error::InvalidQuery)?;
        if start > end || text.get(start..end).is_none() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (projected, offsets) = redact(&text, &[start, end]);
        let [start, end] = offsets.as_slice() else {
            return Err(rusqlite::Error::InvalidParameterName(
                "invalid response offsets".into(),
            ));
        };
        if start > end || projected.get(*start..*end).is_none() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if start < end {
            let start = i64::try_from(*start).map_err(|_| rusqlite::Error::InvalidQuery)?;
            let end = i64::try_from(*end).map_err(|_| rusqlite::Error::InvalidQuery)?;
            tx.execute("UPDATE assistant_responses SET text_start_bytes=?1,text_end_bytes=?2 WHERE message_id=?3 AND response_id=?4", params![start,end,message,id])?;
        } else {
            tx.execute(
                "DELETE FROM assistant_responses WHERE message_id=?1 AND response_id=?2",
                params![message, id],
            )?;
        }
    }
    crate::context_compaction::redact_legacy_thread_content(&tx, thread, &mut redact)?;
    for (id, field, value) in fields(&tx, thread)? {
        let changed = match field.as_str() {
            "message" | "plan.note" | "pending.model" | "pending.content" => redact(&value, &[]).0,
            "tool.input" => {
                fn visit<F: FnMut(&str, &[usize]) -> (String, Vec<usize>)>(
                    value: &mut serde_json::Value,
                    redact: &mut F,
                ) {
                    match value {
                        serde_json::Value::String(text) => *text = redact(text, &[]).0,
                        serde_json::Value::Array(items) => {
                            for item in items {
                                visit(item, redact);
                            }
                        }
                        serde_json::Value::Object(fields) => {
                            let original = std::mem::take(fields);
                            for (name, mut value) in original {
                                visit(&mut value, redact);
                                let projected = redact(&name, &[]).0;
                                let mut name = projected.clone();
                                let mut suffix = 0usize;
                                while fields.contains_key(&name) {
                                    suffix += 1;
                                    name = format!("{projected}_{suffix}");
                                }
                                fields.insert(name, value);
                            }
                        }
                        _ => {}
                    }
                }
                let mut json: serde_json::Value = serde_json::from_str(&value).map_err(|_| {
                    rusqlite::Error::InvalidParameterName("invalid historical tool input".into())
                })?;
                visit(&mut json, &mut redact);
                serde_json::to_string(&json).map_err(|_| {
                    rusqlite::Error::InvalidParameterName("invalid historical tool input".into())
                })?
            }
            _ => continue,
        };
        if value != changed {
            match field.as_str() {
                "message" => {
                    tx.execute("UPDATE messages SET content=?1 WHERE id=?2 AND thread_id=?3 AND role='user'", params![changed,id,thread])?;
                }
                "plan.note" => {
                    tx.execute(
                        "UPDATE messages SET plan_review_note=?1 WHERE id=?2 AND thread_id=?3",
                        params![changed, id, thread],
                    )?;
                }
                "pending.model" => {
                    tx.execute(
                        "UPDATE pending_messages SET model_content=?1 WHERE id=?2 AND thread_id=?3",
                        params![changed, id, thread],
                    )?;
                }
                "pending.content" => {
                    tx.execute("UPDATE pending_messages SET payload=json_set(payload,'$.content',?1) WHERE id=?2 AND thread_id=?3", params![changed,id,thread])?;
                }
                "tool.input" => {
                    tx.execute(
                        "UPDATE tool_calls SET input_json=?1 WHERE id=?2 AND thread_id=?3",
                        params![changed, id, thread],
                    )?;
                }
                _ => {}
            }
        }
    }
    tx.execute(
        "DELETE FROM context_checkpoints WHERE thread_id=?1",
        [thread],
    )?;
    tx.execute("UPDATE credential_continuations SET state='unknown',reason='source_changed' WHERE thread_id=?1 AND state IN ('ready','claimed')", [thread])?;
    tx.commit()
}

pub fn source_message_id(
    conn: &Connection,
    field: &str,
    id: &str,
) -> rusqlite::Result<Option<String>> {
    match field {
        "message" | "plan.note" => Ok(Some(id.into())),
        "tool.input" | "tool.output" => conn
            .query_row(
                "SELECT message_id FROM tool_calls WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional(),
        _ => Ok(None),
    }
}

pub fn refresh_epoch(conn: &Connection, digest: &str) -> rusqlite::Result<()> {
    fn current(conn: &Connection) -> rusqlite::Result<Option<String>> {
        conn.query_row(
            "SELECT digest FROM credential_privacy_epoch WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
    }
    if current(conn)?.as_deref() == Some(digest) {
        return Ok(());
    }
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let prior = current(&tx)?;
    if prior.as_deref() != Some(digest) {
        tx.execute("DELETE FROM credential_privacy_exceptions", [])?;
        tx.execute("INSERT INTO credential_privacy_epoch(singleton,digest) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET digest=excluded.digest", [digest])?;
    }
    tx.commit()
}

pub fn source_label(conn: &Connection, field: &str, id: &str) -> rusqlite::Result<String> {
    let label = match field {
        "message" => {
            let role: String =
                conn.query_row("SELECT role FROM messages WHERE id=?1", [id], |row| {
                    row.get(0)
                })?;
            if role == "user" {
                "用户消息"
            } else {
                "助手回复"
            }
        }
        "tool.input" => "工具调用",
        "tool.output" => "工具结果",
        "summary" => "对话摘要",
        "plan.note" => "计划反馈",
        "pending.model" => "已发送请求",
        "pending.content" => "待发送消息",
        _ => "对话记录",
    };
    Ok(label.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use std::sync::{Arc, Barrier};

    #[test]
    fn unchanged_epoch_is_readable_while_another_connection_prepares_a_turn() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("epoch.db");
        let first = Store::open(&path).unwrap();
        first.migrate().unwrap();
        let second = Store::open(&path).unwrap();
        refresh_epoch(first.conn(), "synthetic-epoch").unwrap();
        let transaction = first.immediate_transaction().unwrap();
        refresh_epoch(second.conn(), "synthetic-epoch").unwrap();
        transaction.commit().unwrap();
    }

    #[test]
    fn concurrent_epoch_initialization_serializes_without_read_upgrade_failures() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("epoch.db");
        let setup = Store::open(&path).unwrap();
        setup.migrate().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let stores = [Store::open(&path).unwrap(), Store::open(&path).unwrap()];
        let handles = stores
            .into_iter()
            .map(|store| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    refresh_epoch(store.conn(), "synthetic-epoch").unwrap();
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
        let epoch: String = setup
            .conn()
            .query_row(
                "SELECT digest FROM credential_privacy_epoch WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(epoch, "synthetic-epoch");
    }
}
